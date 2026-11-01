//! Gemeinsame native DeckLink-Schnittstelle (Review G3): eine API für alle
//! Plattformen, implementiert pro Plattform innerhalb dieses Moduls –
//! Windows über COM (`decklink_com.rs`, SDK-16.0-GUIDs), macOS/Linux über die
//! C++-Bridge (`decklink_bridge.cpp` via FFI, Feature `decklink`). Außerhalb
//! des DeckLink-Moduls gibt es keine Plattformweiche.
//!
//! Funktionen: Treiber vorhanden?, Geräteliste, Modi der Karte abfragen,
//! Ausgabe öffnen (UYVY-Frames pushen), schließen. Status-Meldungen für
//! „Treiber fehlt“ und „Treiber da, aber 0 Geräte“ (G5/G8).

use crate::core::{ShowError, ShowResult};
use crate::engine::decklink::{bmd_display_mode, rgba_to_uyvy422_cs, ColorSpace, DecklinkConfig};
use crate::engine::frame::VideoFrame;

/// Treiber/Bridge vorhanden? (bei „ja, aber 0 Geräte“ siehe `list_devices` + Meldung).
pub fn available() -> bool {
    platform::available()
}

/// Liefert (available, devices, hint): `hint` ist die Meldung für die UI, wenn
/// der Treiber da ist, aber 0 Geräte gefunden wurden (z. B. nach einem
/// macOS-Update: Desktop Video neu installieren/registrieren, siehe G8).
pub fn devices_with_hint() -> (bool, Vec<String>, Option<String>) {
    if !available() {
        return (
            false,
            Vec::new(),
            Some("DeckLink driver not found. Install Blackmagic Desktop Video (16.0 or newer) and restart the app.".to_string()),
        );
    }
    let devices = platform::list_devices();
    let hint = if devices.is_empty() {
        Some("DeckLink driver is present but no device was found. After a macOS update, reinstall/register Desktop Video (the setup may look active).".to_string())
    } else {
        None
    };
    (true, devices, hint)
}

/// Ein unterstützter Display-Mode der Karte (G5: von der Karte abgefragt).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeMode {
    pub width: u32,
    pub height: u32,
    /// fps als Bruch (num/den), z. B. 30000/1001 für 29,97.
    pub fps_num: u32,
    pub fps_den: u32,
    pub interlaced: bool,
}

/// Offene native Ausgabe auf einer Karte.
pub struct NativeOutput {
    inner: platform::Output,
    width: u32,
    height: u32,
    uyvy: Vec<u8>,
    device_name: String,
    frames_pushed: u64,
}

impl NativeOutput {
    /// Öffnet das Gerät (Name-Substring; leer/„auto“ = erstes mit Output) im
    /// gewünschten Modus. Liefert klare Fehlermeldungen (G5).
    pub fn open(config: &DecklinkConfig) -> ShowResult<Self> {
        if !available() {
            return Err(ShowError::Engine(
                "DeckLink driver not found. Install Blackmagic Desktop Video (16.0 or newer)."
                    .into(),
            ));
        }
        let mode = bmd_display_mode(config.width, config.height, config.fps).ok_or_else(|| {
            ShowError::Engine(format!(
                "unsupported video mode {}x{}@{} for DeckLink",
                config.width, config.height, config.fps
            ))
        })?;
        let (inner, device_name) =
            platform::open(&config.device, mode, config.width, config.height)?;
        Ok(Self {
            inner,
            width: config.width,
            height: config.height,
            uyvy: vec![0u8; (config.width * config.height * 2) as usize],
            device_name,
            frames_pushed: 0,
        })
    }

    /// Name des geöffneten Geräts.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Modi, die die Karte laut eigener Abfrage unterstützt (G5).
    pub fn modes(&self) -> Vec<NativeMode> {
        platform::modes(&self.inner)
    }

    /// Pusht ein RGBA-Frame (UYVY-Konvertierung mit Farbraum aus der Höhe, G1).
    pub fn push_frame(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        if frame.width != self.width || frame.height != self.height {
            return Err(ShowError::Engine(format!(
                "frame {}x{} passt nicht zur DeckLink-Ausgabe {}x{}",
                frame.width, frame.height, self.width, self.height
            )));
        }
        let cs = ColorSpace::from_height(self.height);
        rgba_to_uyvy422_cs(frame, &mut self.uyvy, cs);
        platform::push(&mut self.inner, &self.uyvy)?;
        self.frames_pushed += 1;
        Ok(())
    }

    /// Startet Scheduled Playback (G4): die Karte ist Taktgeber, Frames werden
    /// eingereiht. Key/Fill auf zwei Ports laufen damit kartengetaktet synchron.
    pub fn start_scheduled(&mut self) -> ShowResult<()> {
        platform::start_scheduled(&mut self.inner)
    }

    /// (late, dropped, queued) – verspätete/verlorene Frames seit Start (G4).
    pub fn stats(&self) -> (u64, u64, u64) {
        platform::stats(&self.inner)
    }

    /// Anzahl gepushter Frames.
    pub fn frames_pushed(&self) -> u64 {
        self.frames_pushed
    }

    /// Konfigurierte Breite.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Konfigurierte Höhe.
    pub fn height(&self) -> u32 {
        self.height
    }
}

// ---------------------------------------------------------------------------
// Plattform-Implementierungen (nur innerhalb dieses Moduls)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use crate::engine::decklink_com::{list_devices as com_list, ComDeckLinkOutput};

    pub struct Output {
        com: ComDeckLinkOutput,
    }

    pub fn available() -> bool {
        com_list().map(|d| !d.is_empty()).unwrap_or(false) || com_list().is_ok()
    }

    pub fn list_devices() -> Vec<String> {
        com_list().unwrap_or_default()
    }

    pub fn open(device: &str, _mode: u32, width: u32, height: u32) -> ShowResult<(Output, String)> {
        let config = DecklinkConfig {
            device: device.to_string(),
            width,
            height,
            fps: 25,
            pix_fmt: "uyvy422".into(),
        };
        let com = ComDeckLinkOutput::start(&config)?;
        let name = com.device_name().to_string();
        Ok((Output { com }, name))
    }

    pub fn modes(_out: &Output) -> Vec<NativeMode> {
        Vec::new() // Windows: Modi-Abfrage folgt mit G5 (COM-Iterator)
    }

    pub fn push(out: &mut Output, uyvy: &[u8]) -> ShowResult<()> {
        out.com.push_uyvy(uyvy)
    }

    pub fn start_scheduled(out: &mut Output) -> ShowResult<()> {
        out.com.start_scheduled()
    }

    pub fn stats(out: &Output) -> (u64, u64, u64) {
        out.com.stats()
    }
}

#[cfg(all(not(target_os = "windows"), feature = "decklink"))]
mod platform {
    use super::*;
    use std::ffi::{c_char, c_void, CString};

    extern "C" {
        fn kp_dl_available() -> i32;
        fn kp_dl_device_count() -> i32;
        fn kp_dl_device_name(index: i32, out: *mut c_char, outlen: i32) -> i32;
        fn kp_dl_last_error(out: *mut c_char, outlen: i32) -> i32;
        fn kp_dl_open(name: *const c_char, mode: u32, width: i32, height: i32) -> *mut c_void;

        fn kp_dl_mode_count(h: *mut c_void) -> i32;
        fn kp_dl_mode(
            h: *mut c_void,
            index: i32,
            w: *mut i32,
            h: *mut i32,
            fps_num: *mut i32,
            fps_den: *mut i32,
            interlaced: *mut i32,
        );
        fn kp_dl_push(h: *mut c_void, uyvy: *const u8, len: i32) -> i32;
        fn kp_dl_start_scheduled(h: *mut c_void) -> i32;
        fn kp_dl_stats(
            h: *mut c_void,
            late: *mut i64,
            dropped: *mut i64,
            queued: *mut i64,
            scheduled: *mut i64,
        );
        fn kp_dl_close(h: *mut c_void);
    }

    pub struct Output {
        handle: *mut c_void,
    }

    // COM-Handles sind raw pointers → Send/Sync manuell (nur Engine-Thread nutzt sie).
    unsafe impl Send for Output {}

    fn c_str(buf: &[u8]) -> String {
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).to_string()
    }

    fn last_error() -> String {
        let mut buf = [0u8; 512];
        unsafe { kp_dl_last_error(buf.as_mut_ptr() as *mut c_char, buf.len() as i32) };
        c_str(&buf)
    }

    pub fn available() -> bool {
        unsafe { kp_dl_available() == 1 }
    }

    pub fn list_devices() -> Vec<String> {
        let n = unsafe { kp_dl_device_count() };
        if n <= 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0..n {
            let mut buf = [0u8; 256];
            unsafe { kp_dl_device_name(i, buf.as_mut_ptr() as *mut c_char, buf.len() as i32) };
            let s = c_str(&buf);
            if !s.is_empty() {
                out.push(s);
            }
        }
        out
    }

    pub fn open(device: &str, mode: u32, width: u32, height: u32) -> ShowResult<(Output, String)> {
        let name = CString::new(device).unwrap_or_default();
        let handle = unsafe { kp_dl_open(name.as_ptr(), mode, width as i32, height as i32) };
        if handle.is_null() {
            return Err(ShowError::Engine(format!(
                "DeckLink open failed: {}",
                last_error()
            )));
        }
        let devices = list_devices();
        let name = devices.first().cloned().unwrap_or_default();
        Ok((Output { handle }, name))
    }

    pub fn modes(out: &Output) -> Vec<NativeMode> {
        let n = unsafe { kp_dl_mode_count(out.handle) };
        let mut v = Vec::new();
        for i in 0..n.max(0) {
            let (mut w, mut h, mut num, mut den, mut inter) = (0, 0, 0, 0, 0);
            unsafe {
                kp_dl_mode(
                    out.handle, i, &mut w, &mut h, &mut num, &mut den, &mut inter,
                )
            };
            if w > 0 && h > 0 && num > 0 {
                v.push(NativeMode {
                    width: w as u32,
                    height: h as u32,
                    fps_num: num as u32,
                    fps_den: den.max(1) as u32,
                    interlaced: inter != 0,
                });
            }
        }
        v
    }

    pub fn push(out: &mut Output, uyvy: &[u8]) -> ShowResult<()> {
        let rc = unsafe { kp_dl_push(out.handle, uyvy.as_ptr(), uyvy.len() as i32) };
        if rc != 0 {
            return Err(ShowError::Engine(format!(
                "DeckLink push failed ({rc}): {}",
                last_error()
            )));
        }
        Ok(())
    }

    pub fn start_scheduled(out: &mut Output) -> ShowResult<()> {
        let rc = unsafe { kp_dl_start_scheduled(out.handle) };
        if rc != 0 {
            return Err(ShowError::Engine(format!(
                "DeckLink StartScheduledPlayback failed ({rc}): {}",
                last_error()
            )));
        }
        Ok(())
    }

    pub fn stats(out: &Output) -> (u64, u64, u64) {
        let (mut late, mut dropped, mut queued, mut scheduled) = (0i64, 0i64, 0i64, 0i64);
        unsafe {
            kp_dl_stats(
                out.handle,
                &mut late,
                &mut dropped,
                &mut queued,
                &mut scheduled,
            )
        };
        let _ = scheduled;
        (
            late.max(0) as u64,
            dropped.max(0) as u64,
            queued.max(0) as u64,
        )
    }

    impl Drop for Output {
        fn drop(&mut self) {
            unsafe { kp_dl_close(self.handle) };
        }
    }
}

#[cfg(all(not(target_os = "windows"), not(feature = "decklink")))]
mod platform {
    use super::*;

    pub struct Output;

    pub fn available() -> bool {
        false
    }

    pub fn list_devices() -> Vec<String> {
        Vec::new()
    }

    pub fn open(
        _device: &str,
        _mode: u32,
        _width: u32,
        _height: u32,
    ) -> ShowResult<(Output, String)> {
        Err(ShowError::Engine(
            "native DeckLink build missing (compile with --features decklink)".into(),
        ))
    }

    pub fn modes(_out: &Output) -> Vec<NativeMode> {
        Vec::new()
    }

    pub fn push(_out: &mut Output, _uyvy: &[u8]) -> ShowResult<()> {
        Err(ShowError::Engine("native DeckLink build missing".into()))
    }

    pub fn start_scheduled(_out: &mut Output) -> ShowResult<()> {
        Err(ShowError::Engine("native DeckLink build missing".into()))
    }

    pub fn stats(_out: &Output) -> (u64, u64, u64) {
        (0, 0, 0)
    }
}
