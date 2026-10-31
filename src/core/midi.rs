//! MIDI-Cue (QLab 5 „MIDI Cue"): sendet eine MIDI-Nachricht (Note On/Off,
//! Control Change, Program Change) an das konfigurierte MIDI-Ausgabegerät,
//! wenn der Cue feuert.
//!
//! Der Cue gilt sofort als abgeschlossen (Sofort-Cue); das Senden erledigt
//! die Engine über [`crate::engine::EngineCommand::SendMidi`] – die
//! Byterepräsentation liegt hier zentral ([`MidiMessage::to_bytes`]).
//!
//! Kanal/Note/ Controller/ Velocity werden 0-basiert gespeichert (MIDI-Wire-
//! Format); die UI zeigt 1-basierte Kanäle (wie QLab).

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Art der MIDI-Nachricht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MidiMessageKind {
    /// Note On (0x90) – `velocity = 0` ist formal Note Off.
    #[default]
    NoteOn,
    /// Note Off (0x80).
    NoteOff,
    /// Control Change (0xB0) – Controller + Wert.
    ControlChange,
    /// Program Change (0xC0) – Programm (2 Bytes auf dem Wire).
    ProgramChange,
}

impl MidiMessageKind {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [MidiMessageKind; 4] = [
        MidiMessageKind::NoteOn,
        MidiMessageKind::NoteOff,
        MidiMessageKind::ControlChange,
        MidiMessageKind::ProgramChange,
    ];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            MidiMessageKind::NoteOn => "Note On",
            MidiMessageKind::NoteOff => "Note Off",
            MidiMessageKind::ControlChange => "Control Change",
            MidiMessageKind::ProgramChange => "Program Change",
        }
    }
    /// Status-Byte (High-Nibble) des Wire-Formats.
    fn status(self, channel: u8) -> u8 {
        let base = match self {
            MidiMessageKind::NoteOn => 0x90,
            MidiMessageKind::NoteOff => 0x80,
            MidiMessageKind::ControlChange => 0xB0,
            MidiMessageKind::ProgramChange => 0xC0,
        };
        base | (channel & 0x0F)
    }
}

/// MIDI-Nachricht eines MIDI-Cues ([`crate::core::CueType::Midi`]).
///
/// Alle Werte 0-basiert (Wire-Format); `channel` 0–15, Note/Controller/
/// Value/Program 0–127.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiMessage {
    /// Nachrichtentyp.
    #[serde(default)]
    pub kind: MidiMessageKind,
    /// Kanal (0-basiert; UI zeigt 1–16).
    #[serde(default)]
    pub channel: u8,
    /// Note (Note On/Off) oder Controller (CC) – je nach `kind`.
    #[serde(default = "default_note")]
    pub data1: u8,
    /// Velocity (Note On/Off) oder Wert (CC); bei Program Change = Programm.
    #[serde(default = "default_velocity")]
    pub data2: u8,
}

fn default_note() -> u8 {
    60 // C4
}

fn default_velocity() -> u8 {
    100
}

impl Default for MidiMessage {
    fn default() -> Self {
        Self {
            kind: MidiMessageKind::NoteOn,
            channel: 0,
            data1: default_note(),
            data2: default_velocity(),
        }
    }
}

impl MidiMessage {
    /// Prüft die Wertebereiche (Wire-Format 0–127, Kanal 0–15).
    pub fn validate(&self) -> ShowResult<()> {
        if self.channel > 15 {
            return Err(ShowError::InvalidParameter(
                "midi cue: channel must be 0-15".to_string(),
            ));
        }
        if self.data1 > 127 || self.data2 > 127 {
            return Err(ShowError::InvalidParameter(
                "midi cue: data bytes must be 0-127".to_string(),
            ));
        }
        Ok(())
    }

    /// Wire-Bytes der Nachricht (2 Bytes bei Program Change, sonst 3).
    pub fn to_bytes(&self) -> Vec<u8> {
        match self.kind {
            MidiMessageKind::ProgramChange => {
                vec![self.kind.status(self.channel), self.data1 & 0x7F]
            }
            _ => vec![
                self.kind.status(self.channel),
                self.data1 & 0x7F,
                self.data2 & 0x7F,
            ],
        }
    }

    /// Anzeige-Text (QLab-ähnlich: „Note On ch 1 note 60 vel 100“).
    pub fn describe(&self) -> String {
        let ch = self.channel + 1;
        match self.kind {
            MidiMessageKind::NoteOn => {
                format!("note on  ch {ch}  note {}  vel {}", self.data1, self.data2)
            }
            MidiMessageKind::NoteOff => {
                format!("note off ch {ch}  note {}  vel {}", self.data1, self.data2)
            }
            MidiMessageKind::ControlChange => {
                format!("cc       ch {ch}  ctrl {}  val {}", self.data1, self.data2)
            }
            MidiMessageKind::ProgramChange => {
                format!("program  ch {ch}  pgm {}", self.data1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_bytes_wire_format() {
        let msg = MidiMessage {
            kind: MidiMessageKind::NoteOn,
            channel: 0,
            data1: 60,
            data2: 100,
        };
        assert_eq!(msg.to_bytes(), vec![0x90, 60, 100]);
        let msg = MidiMessage {
            kind: MidiMessageKind::NoteOff,
            channel: 15,
            data1: 127,
            data2: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0x8F, 127, 0]);
        let msg = MidiMessage {
            kind: MidiMessageKind::ControlChange,
            channel: 2,
            data1: 7,
            data2: 127,
        };
        assert_eq!(msg.to_bytes(), vec![0xB2, 7, 127]);
        // Program Change: 2 Bytes.
        let msg = MidiMessage {
            kind: MidiMessageKind::ProgramChange,
            channel: 0,
            data1: 5,
            data2: 0,
        };
        assert_eq!(msg.to_bytes(), vec![0xC0, 5]);
    }

    #[test]
    fn validate_ranges() {
        assert!(MidiMessage::default().validate().is_ok());
        let msg = MidiMessage {
            channel: 16,
            ..Default::default()
        };
        assert!(msg.validate().is_err());
        let msg = MidiMessage {
            data1: 200,
            ..Default::default()
        };
        assert!(msg.validate().is_err());
    }

    #[test]
    fn serde_roundtrip_and_legacy_default() {
        let msg = MidiMessage {
            kind: MidiMessageKind::ControlChange,
            channel: 3,
            data1: 11,
            data2: 64,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: MidiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        // Legacy: leeres Objekt → Defaults.
        let back: MidiMessage = serde_json::from_str("{}").unwrap();
        assert_eq!(back, MidiMessage::default());
        assert_eq!(MidiMessageKind::NoteOn.label(), "Note On");
        assert!(msg.describe().contains("ctrl 11"));
    }
}
