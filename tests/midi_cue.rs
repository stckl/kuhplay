//! MIDI-Cue (QLab 5): sendet Wire-Bytes an das MIDI-Ausgabegerät.
//! Core-Verhalten (Sofort-Cue, Bytes, Validierung) – das midir-Output-Device
//! selbst wird nicht getestet (CoreMIDI verweigert in manchen CI-Prozessen
//! den Client, siehe src/remote/midi.rs).

use kuhplay::core::{Cue, CueList, CueStatus, CueType, MidiMessage, MidiMessageKind};
use kuhplay::engine::EngineCommand;

#[test]
fn midi_cue_triggers_send_midi_with_wire_bytes_and_completes_instantly() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "note", CueType::Midi);
    cue.midi = Some(MidiMessage {
        kind: MidiMessageKind::NoteOn,
        channel: 0,
        data1: 60,
        data2: 100,
    });
    list.add(cue);

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    assert!(
        matches!(&outcome.commands[0], EngineCommand::SendMidi { bytes, .. } if *bytes == vec![0x90, 60, 100]),
        "midi cue must produce SendMidi with wire bytes"
    );
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

#[test]
fn midi_cue_program_change_sends_two_bytes() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "pgm", CueType::Midi);
    cue.midi = Some(MidiMessage {
        kind: MidiMessageKind::ProgramChange,
        channel: 9,
        data1: 42,
        data2: 0,
    });
    list.add(cue);
    let outcome = list.go().unwrap();
    assert!(
        matches!(&outcome.commands[0], EngineCommand::SendMidi { bytes, .. } if *bytes == vec![0xC9, 42])
    );
}

#[test]
fn midi_cue_without_message_goes_to_error() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty", CueType::Midi));
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn midi_cue_serde_legacy_and_default() {
    let mut cue = Cue::new("2", "m", CueType::Midi);
    cue.midi = Some(MidiMessage::default());
    let json = serde_json::to_string(&cue).unwrap();
    let back: Cue = serde_json::from_str(&json).unwrap();
    assert_eq!(back.cue_type, CueType::Midi);
    assert!(back.midi.is_some());
    // Alte Show ohne midi-Feld → None (Cue ist "kaputt" bis konfiguriert).
    let old_json = json.replace(
        ",\"midi\":{\"kind\":\"NoteOn\",\"channel\":0,\"data1\":60,\"data2\":100}",
        "",
    );
    let back_old: Cue = serde_json::from_str(&old_json).unwrap();
    assert!(back_old.midi.is_none());
    assert_eq!(CueType::Midi.label(), "MIDI");
    assert!(CueType::ALL.contains(&CueType::Midi));
    // Default: Note On, Kanal 1 (0-basiert 0), C4, Velocity 100.
    let d = MidiMessage::default();
    assert_eq!(d.to_bytes(), vec![0x90, 60, 100]);
}
