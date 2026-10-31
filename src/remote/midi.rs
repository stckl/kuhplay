//! MIDI-Steuerung: Controller-Note-On → RemoteCommands.
//!
//! Note-Mapping (Channel egal, Note-On = Velocity > 0), Default:
//! * Note 0 (C-1) → [`RemoteCommand::Go`]
//! * Note 1       → [`RemoteCommand::Panic`]
//! * Note 2       → [`RemoteCommand::PauseAll`]
//! * Note 3       → [`RemoteCommand::ResumeAll`]
//! * Note 4       → [`RemoteCommand::Reset`]
//!
//! Aktivierung/Device/Map siehe [`super::RemoteConfig::from_env`]
//! (`KUHPLAY_MIDI=off`, `KUHPLAY_MIDI_DEVICE=<namens-substring>`,
//! `KUHPLAY_MIDI_MAP=go=36,panic=37,…`).
//! MIDI läuft callback-basiert auf dem CoreMIDI/ALSA-Thread (midir) –
//! im Gegensatz zu OSC/UDP gibt es hier keinen eigenen Server-Thread.

use midir::{MidiInput, MidiInputConnection};
use tokio::sync::mpsc;

use super::RemoteCommand;

/// Note-Mapping für die MIDI-Steuerung (Note-On → RemoteCommand).
///
/// Default: Note 0 (C-1) = Go, 1 = Panic, 2 = Pause, 3 = Resume, 4 = Reset.
/// Konfigurierbar via `KUHPLAY_MIDI_MAP` (z. B. `go=36,panic=37`), siehe
/// [`MidiNoteMap::from_map_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNoteMap {
    pub go: u8,
    pub panic: u8,
    pub pause: u8,
    pub resume: u8,
    pub reset: u8,
}

impl Default for MidiNoteMap {
    fn default() -> Self {
        Self {
            go: 0,
            panic: 1,
            pause: 2,
            resume: 3,
            reset: 4,
        }
    }
}

impl MidiNoteMap {
    /// Parst das `KUHPLAY_MIDI_MAP`-Format: `go=36,panic=37,pause=38,resume=39,reset=40`
    /// (Komma-getrennte `action=note`-Paare). Unbekannte Actions, fehlende
    /// Werte und Noten außerhalb 0–127 werden ignoriert (Default bleibt).
    pub fn from_map_str(s: &str) -> Self {
        let mut map = Self::default();
        for pair in s.split(',') {
            let Some((action, note)) = pair.split_once('=') else {
                continue;
            };
            let Ok(note) = note.trim().parse::<u8>() else {
                continue;
            };
            if note > 127 {
                continue;
            }
            match action.trim() {
                "go" => map.go = note,
                "panic" => map.panic = note,
                "pause" => map.pause = note,
                "resume" => map.resume = note,
                "reset" => map.reset = note,
                _ => {
                    tracing::warn!(%action, "midi map: unbekannte action – ignoriert");
                }
            }
        }
        map
    }

    /// Command für eine Note (aus dem Note-On), `None` wenn nicht gemappt.
    pub fn command_for_note(&self, note: u8) -> Option<RemoteCommand> {
        if note == self.go {
            Some(RemoteCommand::Go)
        } else if note == self.panic {
            Some(RemoteCommand::Panic)
        } else if note == self.pause {
            Some(RemoteCommand::PauseAll)
        } else if note == self.resume {
            Some(RemoteCommand::ResumeAll)
        } else if note == self.reset {
            Some(RemoteCommand::Reset)
        } else {
            None
        }
    }
}

/// Wandelt rohe MIDI-Bytes in einen RemoteCommand um (mit `map`).
/// `None` bei allem, was kein Note-On (Velocity > 0) auf einer
/// gemappten Note ist (Note-Off, Control-Change, andere Noten …).
pub fn midi_bytes_to_command(bytes: &[u8], map: &MidiNoteMap) -> Option<RemoteCommand> {
    let (&status, rest) = bytes.split_first()?;
    if status & 0xF0 != 0x90 {
        return None; // nur Note-On, Channel egal
    }
    let (&note, rest) = rest.split_first()?;
    let (&velocity, _) = rest.split_first()?;
    if velocity == 0 {
        return None; // Note-On mit Velocity 0 = Note-Off
    }
    map.command_for_note(note)
}

/// Fehler beim MIDI-Start (in der App per Graceful Degradation nur geloggt).
#[derive(Debug, thiserror::Error)]
pub enum MidiError {
    #[error("midi init fehlgeschlagen: {0}")]
    Init(String),
    #[error("kein midi input device gefunden: {0}")]
    NoDevice(String),
    #[error("midi connect fehlgeschlagen: {0}")]
    Connect(String),
}

/// Guard für ein gestartetes MIDI-Input: hält die Verbindung offen
/// (Drop der Connection = Disconnect).
pub struct MidiInputGuard {
    _conn: MidiInputConnection<mpsc::Sender<RemoteCommand>>,
}

/// Verbindet ein MIDI-Input-Device (`device` = optionaler Namens-Substring,
/// sonst das erste Device) mit dem Note-`map`. Rückgabe: Command-Receiver +
/// Guard (muss leben, solange MIDI empfangen werden soll).
///
/// Der Callback läuft auf dem CoreMIDI/ALSA-Thread (kein tokio-Kontext) –
/// `blocking_send` ist dort erlaubt und blockiert höchstens bei vollem
/// 64er-Puffer kurz den MIDI-Thread.
pub fn start_midi_input(
    device: Option<&str>,
    map: &MidiNoteMap,
) -> Result<(mpsc::Receiver<RemoteCommand>, MidiInputGuard), MidiError> {
    // CoreMIDI verweigert das Anlegen gelegentlich für den ganzen Prozess
    // (gemessen: > 30 s, 1 von 30 Prozessstarts). Wiederholen hilft dann nicht,
    // nur ein Neustart; der Aufrufer meldet das daher sichtbar.
    let midi_in = MidiInput::new("kuhplay").map_err(|e| MidiError::Init(e.to_string()))?;
    let ports = midi_in.ports();
    let port = match device {
        Some(name) => {
            let needle = name.to_lowercase();
            ports
                .into_iter()
                .find(|p| {
                    midi_in
                        .port_name(p)
                        .map(|n| n.to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .ok_or_else(|| MidiError::NoDevice(format!("kein device mit name '{name}'")))?
        }
        None => ports
            .into_iter()
            .next()
            .ok_or_else(|| MidiError::NoDevice("keine devices vorhanden".to_string()))?,
    };
    let port_name = midi_in.port_name(&port).unwrap_or_else(|_| "?".to_string());
    let (tx, rx) = mpsc::channel::<RemoteCommand>(64);
    let map = *map;
    let conn = midi_in
        .connect(
            &port,
            "kuhplay-midi",
            move |_ts, bytes, tx: &mut mpsc::Sender<RemoteCommand>| {
                if let Some(cmd) = midi_bytes_to_command(bytes, &map) {
                    // Receiver weg (App zu) → send schlägt fehl, ignorieren.
                    let _ = tx.blocking_send(cmd);
                }
            },
            tx,
        )
        .map_err(|e| MidiError::Connect(e.to_string()))?;
    tracing::info!(device = %port_name, "midi input gestartet");
    Ok((rx, MidiInputGuard { _conn: conn }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_map_default() {
        let map = MidiNoteMap::default();
        // Note-On, Channel 1, Velocity 100.
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 0, 100], &map),
            Some(RemoteCommand::Go)
        ));
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 1, 100], &map),
            Some(RemoteCommand::Panic)
        ));
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 2, 100], &map),
            Some(RemoteCommand::PauseAll)
        ));
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 3, 100], &map),
            Some(RemoteCommand::ResumeAll)
        ));
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 4, 100], &map),
            Some(RemoteCommand::Reset)
        ));
        // Anderer Channel (0x95) mappt genauso.
        assert!(matches!(
            midi_bytes_to_command(&[0x95, 0, 100], &map),
            Some(RemoteCommand::Go)
        ));
    }

    #[test]
    fn note_on_map_custom() {
        let map = MidiNoteMap::from_map_str("go=36,panic=37,pause=38,resume=39,reset=40");
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 36, 100], &map),
            Some(RemoteCommand::Go)
        ));
        assert!(matches!(
            midi_bytes_to_command(&[0x90, 40, 100], &map),
            Some(RemoteCommand::Reset)
        ));
        // Alte Default-Noten sind jetzt nicht mehr gemappt.
        assert!(midi_bytes_to_command(&[0x90, 0, 100], &map).is_none());
    }

    #[test]
    fn note_map_parsing() {
        // Leer → Default.
        assert_eq!(MidiNoteMap::from_map_str(""), MidiNoteMap::default());
        // Teilweise → Rest Default.
        let map = MidiNoteMap::from_map_str("go=36");
        assert_eq!(map.go, 36);
        assert_eq!(map.panic, 1);
        // Unbekannte Action / kaputte Paare / Note > 127 → ignoriert.
        let map = MidiNoteMap::from_map_str("go=36,bogus=1,panic,panic=200,resume=abc");
        assert_eq!(map.go, 36);
        assert_eq!(map.panic, 1);
        assert_eq!(map.resume, 3);
        // Whitespace tolerant.
        let map = MidiNoteMap::from_map_str(" go = 36 , reset = 127 ");
        assert_eq!(map.go, 36);
        assert_eq!(map.reset, 127);
    }

    #[test]
    fn note_on_edge_cases() {
        let map = MidiNoteMap::default();
        // Velocity 0 = Note-Off → kein Command.
        assert!(midi_bytes_to_command(&[0x90, 0, 0], &map).is_none());
        // Note-Off-Status (0x80) → kein Command.
        assert!(midi_bytes_to_command(&[0x80, 0, 100], &map).is_none());
        // Control-Change (0xB0) → kein Command.
        assert!(midi_bytes_to_command(&[0xB0, 0, 100], &map).is_none());
        // Nicht gemappte Note.
        assert!(midi_bytes_to_command(&[0x90, 60, 100], &map).is_none());
        // Zu kurz / kaputt.
        assert!(midi_bytes_to_command(&[], &map).is_none());
        assert!(midi_bytes_to_command(&[0x90], &map).is_none());
        assert!(midi_bytes_to_command(&[0x90, 0], &map).is_none());
        // System-Realtime-Bytes (1 Byte, z. B. 0xF8 Clock) → kein Command.
        assert!(midi_bytes_to_command(&[0xF8], &map).is_none());
    }
}
