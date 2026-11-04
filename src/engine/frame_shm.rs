//! Shared Memory für HTML5-Frames zwischen CEF-Renderer (`kuhplay-cef`) und
//! Engine: eine Datei, von beiden Prozessen eingeblendet (`memmap2`), mit
//! **zwei Bild-Slots im Wechsel**. Der Renderer schreibt Slot `seq % 2` und
//! meldet ihn per `{"frame_shm":{"width","height","slot","seq"}}` über die
//! Pipe; die Engine kopiert den Slot heraus. Über die Pipe laufen damit nur
//! noch wenige Bytes pro Bild statt Base64 (1080p: ~11 MB Text pro Bild).
//!
//! Layout: `[Header 64 B][Slot 0: w·h·4 RGBA][Slot 1: w·h·4 RGBA]`. Ändert
//! sich die Bildgröße, wächst bzw. schrumpft die Datei; der Leser prüft die
//! Länge vor jedem Zugriff.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use memmap2::{Mmap, MmapMut};

/// Platz vor den Slots (reserviert für spätere Metadaten).
pub const HEADER_LEN: usize = 64;

/// Byte-Offset und Länge eines Slots bei `width`×`height`.
pub fn slot_range(width: u32, height: u32, slot: u32) -> (usize, usize) {
    let len = width as usize * height as usize * 4;
    (HEADER_LEN + (slot as usize % 2) * len, len)
}

/// Gesamtlänge der Datei für zwei Slots `width`×`height`.
pub fn file_len(width: u32, height: u32) -> usize {
    HEADER_LEN + 2 * width as usize * height as usize * 4
}

/// Schreibseite (Renderer).
pub struct FrameShmWriter {
    file: File,
    map: Option<MmapMut>,
    size: (u32, u32),
    seq: u64,
}

impl FrameShmWriter {
    /// Öffnet (bzw. legt an) die Datei unter `path`.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Ok(Self {
            file,
            map: None,
            size: (0, 0),
            seq: 0,
        })
    }

    /// Schreibt das nächste Bild: `fill` bekommt den RGBA-Slot
    /// (`width`·`height`·4 Bytes). Rückgabe: (Slot, Sequenznummer).
    pub fn write(
        &mut self,
        width: u32,
        height: u32,
        fill: impl FnOnce(&mut [u8]),
    ) -> io::Result<(u32, u64)> {
        if self.size != (width, height) || self.map.is_none() {
            self.map = None;
            self.file.set_len(file_len(width, height) as u64)?;
            // SAFETY: Die Datei gehört dieser Instanz; der Leser greift nur
            // lesend und nach Längenprüfung zu.
            self.map = Some(unsafe { MmapMut::map_mut(&self.file)? });
            self.size = (width, height);
        }
        self.seq += 1;
        let slot = (self.seq % 2) as u32;
        let (offset, len) = slot_range(width, height, slot);
        let map = self.map.as_mut().expect("gerade eingeblendet");
        fill(&mut map[offset..offset + len]);
        Ok((slot, self.seq))
    }
}

/// Leseseite (Engine): blendet die Datei bei Bedarf neu ein.
pub struct FrameShmReader {
    path: PathBuf,
    map: Option<Mmap>,
}

impl FrameShmReader {
    pub fn new(path: PathBuf) -> Self {
        Self { path, map: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Kopiert Slot `slot` (`width`×`height` RGBA) heraus; `None`, wenn die
    /// Datei (noch) nicht passt.
    pub fn read(&mut self, width: u32, height: u32, slot: u32) -> Option<Vec<u8>> {
        let need = file_len(width, height);
        if self.map.as_ref().is_none_or(|m| m.len() != need) {
            let file = File::open(&self.path).ok()?;
            if file.metadata().ok()?.len() as usize != need {
                return None;
            }
            // SAFETY: nur lesend; die Länge ist geprüft, der Schreiber ändert
            // sie nur bei neuer Bildgröße (dann wird hier neu eingeblendet).
            self.map = Some(unsafe { Mmap::map(&file).ok()? });
        }
        let map = self.map.as_ref()?;
        let (offset, len) = slot_range(width, height, slot);
        map.get(offset..offset + len).map(<[u8]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_and_reader_share_alternating_slots() {
        let path = std::env::temp_dir().join(format!("kuhplay-shm-test-{}", std::process::id()));
        let mut writer = FrameShmWriter::open(&path).unwrap();
        let mut reader = FrameShmReader::new(path.clone());
        let (slot_a, seq_a) = writer.write(4, 2, |px| px.fill(7)).unwrap();
        let (slot_b, seq_b) = writer.write(4, 2, |px| px.fill(9)).unwrap();
        assert_ne!(slot_a, slot_b, "Slots wechseln");
        assert_eq!(seq_b, seq_a + 1);
        assert_eq!(reader.read(4, 2, slot_a).unwrap(), vec![7; 32]);
        assert_eq!(reader.read(4, 2, slot_b).unwrap(), vec![9; 32]);
        // Neue Größe: Leser blendet neu ein, alte Größe passt nicht mehr.
        let (slot_c, _) = writer.write(8, 8, |px| px.fill(1)).unwrap();
        assert_eq!(reader.read(8, 8, slot_c).unwrap(), vec![1; 256]);
        assert!(reader.read(4, 2, slot_c).is_none());
        let _ = std::fs::remove_file(path);
    }
}
