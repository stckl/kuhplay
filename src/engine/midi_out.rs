//! MIDI-Ausgang für MIDI-Cues (QLab 5 „MIDI Cue" sendet): hält eine
//! midir-`MidiOutputConnection` offen und schickt Wire-Bytes
//! ([`crate::core::MidiMessage::to_bytes`]).
//!
//! Gerät: `KUHPLAY_MIDI_OUT_DEVICE` (Namens-Substring, sonst erstes
//! Output-Device). Wird lazy beim ersten `SendMidi` geöffnet – die Engine
//! startet auch ohne MIDI-Hardware normal.

use midir::{MidiOutput, MidiOutputConnection};

/// Fehler beim Öffnen/Senden des MIDI-Ausgangs.
#[derive(Debug, thiserror::Error)]
pub enum MidiOutError {
    #[error("midi output init fehlgeschlagen: {0}")]
    Init(String),
    #[error("kein midi output device gefunden: {0}")]
    NoDevice(String),
    #[error("midi output connect fehlgeschlagen: {0}")]
    Connect(String),
    #[error("midi send fehlgeschlagen: {0}")]
    Send(String),
}

/// Geöffneter MIDI-Ausgang (Drop = Disconnect).
pub struct MidiOutputGuard {
    conn: MidiOutputConnection,
    /// Name des verbundenen Ports (für Fehlermeldungen/Logs).
    pub port_name: String,
}

impl MidiOutputGuard {
    /// Sendet Wire-Bytes an das verbundene Gerät.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), MidiOutError> {
        self.conn
            .send(bytes)
            .map_err(|e| MidiOutError::Send(e.to_string()))
    }
}

/// Öffnet das MIDI-Output-Device (`device` = optionaler Namens-Substring,
/// sonst erstes Device) und verbindet es als „kuhplay-out".
pub fn open_midi_output(device: Option<&str>) -> Result<MidiOutputGuard, MidiOutError> {
    let midi_out = MidiOutput::new("kuhplay-out").map_err(|e| MidiOutError::Init(e.to_string()))?;
    let ports = midi_out.ports();
    let port = match device {
        Some(name) => {
            let needle = name.to_lowercase();
            ports
                .into_iter()
                .find(|p| {
                    midi_out
                        .port_name(p)
                        .map(|n| n.to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    MidiOutError::NoDevice(format!("kein output device mit name '{name}'"))
                })?
        }
        None => ports
            .into_iter()
            .next()
            .ok_or_else(|| MidiOutError::NoDevice("keine output devices vorhanden".to_string()))?,
    };
    let port_name = midi_out
        .port_name(&port)
        .unwrap_or_else(|_| "?".to_string());
    let conn = midi_out
        .connect(&port, "kuhplay-out")
        .map_err(|e| MidiOutError::Connect(e.to_string()))?;
    Ok(MidiOutputGuard { conn, port_name })
}

/// Device-Substring aus der Umgebung (`KUHPLAY_MIDI_OUT_DEVICE`).
pub fn midi_out_device_from_env() -> Option<String> {
    std::env::var("KUHPLAY_MIDI_OUT_DEVICE")
        .ok()
        .filter(|s| !s.trim().is_empty())
}
