//! Integrationstest MIDI-Steuerung: virtueller CoreMIDI-Port (keine Hardware
//! nötig) → Note-On → RemoteCommand über `start_midi_input`.
//!
//! Topologie: `MidiOutput::create_virtual` legt eine virtuelle MIDI-*Quelle*
//! an (MIDISourceCreate, systemweit sichtbar); die Produktion verbindet ihren
//! Input per Namens-Substring darauf und empfängt die gesendeten Note-Ons.
//!
//! Virtual Ports gibt es nur auf Unix (nicht Windows) – Test daher `cfg(unix)`.

#![cfg(unix)]

use std::time::Duration;

use kuhplay::remote::midi::{start_midi_input, MidiError, MidiInputGuard, MidiNoteMap};
use kuhplay::remote::RemoteCommand;
use midir::os::unix::VirtualOutput;
use midir::MidiOutput;

/// MIDI-Client für die virtuelle Quelle. `None`: CoreMIDI verweigert in
/// diesem Prozess jeden Client (siehe `start_midi_input`), der Test kann dann
/// nichts prüfen und wird mit Hinweis übersprungen.
fn midi_output() -> Option<MidiOutput> {
    match MidiOutput::new("kuhplay-test-virtual") {
        Ok(out) => Some(out),
        Err(e) => {
            eprintln!("SKIP: CoreMIDI nicht verfügbar: {e}");
            None
        }
    }
}

/// Startet den MIDI-Input; `None` wie bei [`midi_output`].
fn midi_input(
    device: &str,
    map: &MidiNoteMap,
) -> Option<(tokio::sync::mpsc::Receiver<RemoteCommand>, MidiInputGuard)> {
    match start_midi_input(Some(device), map) {
        Ok(input) => Some(input),
        Err(MidiError::Init(e)) => {
            eprintln!("SKIP: CoreMIDI nicht verfügbar: {e}");
            None
        }
        Err(e) => panic!("midi input: {e}"),
    }
}

#[tokio::test]
async fn midi_note_on_triggers_remote_command() {
    // Virtuelle MIDI-Quelle anlegen (erscheint systemweit als Input-Port).
    let name = format!("kuhplay-test-{}", uuid::Uuid::new_v4());
    let Some(out) = midi_output() else {
        return;
    };
    let mut sender = out.create_virtual(&name).unwrap();

    // Produktion: verbindet per Namens-Substring (Default-Map).
    let Some((mut rx, _guard)) = midi_input(&name, &MidiNoteMap::default()) else {
        return;
    };

    // Note 0 (C-1) = Go. Wiederholen, bis die Verbindung steht (CoreMIDI
    // braucht kurz, bis Events nach dem Connect fließen).
    let mut got_go = false;
    for _ in 0..40 {
        sender.send(&[0x90, 0, 100]).unwrap();
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(RemoteCommand::Go)) => {
                got_go = true;
                break;
            }
            Ok(Some(other)) => panic!("unerwarteter command: {other:?}"),
            Ok(None) => panic!("midi channel geschlossen"),
            Err(_) => continue, // Timeout → erneut senden
        }
    }
    assert!(got_go, "kein Go-Command vom virtuellen MIDI-Port empfangen");

    // Note 1 = Panic.
    sender.send(&[0x90, 1, 100]).unwrap();
    let cmd = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout panic")
        .expect("channel geschlossen");
    assert!(matches!(cmd, RemoteCommand::Panic));

    // Nicht gemappte Note (60) und Note-Off (Velocity 0) erzeugen nichts.
    sender.send(&[0x90, 60, 100]).unwrap();
    sender.send(&[0x90, 0, 0]).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
            .is_err(),
        "nicht gemappte Nachrichten dürfen keine Commands erzeugen"
    );
}

#[test]
fn midi_no_device_is_graceful_error() {
    // Namens-Substring, der garantiert nicht existiert.
    match start_midi_input(Some("kuhplay-gibt-es-nicht-zzz"), &MidiNoteMap::default()) {
        Err(MidiError::Init(e)) => eprintln!("SKIP: CoreMIDI nicht verfügbar: {e}"),
        Err(e) => assert!(e.to_string().contains("kein midi input device")),
        Ok(_) => panic!("erwartet NoDevice-Fehler"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn midi_custom_note_map_end_to_end() {
    // Virtuelle MIDI-Quelle anlegen.
    let name = format!("kuhplay-test-{}", uuid::Uuid::new_v4());
    let Some(out) = midi_output() else {
        return;
    };
    let mut sender = out.create_virtual(&name).unwrap();

    // Custom-Map: Go auf Note 36 (C2), Panic auf 127.
    let map = MidiNoteMap::from_map_str("go=36,panic=127");
    let Some((mut rx, _guard)) = midi_input(&name, &map) else {
        return;
    };

    let mut got_go = false;
    for _ in 0..40 {
        sender.send(&[0x90, 36, 100]).unwrap();
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Some(RemoteCommand::Go)) => {
                got_go = true;
                break;
            }
            Ok(Some(other)) => panic!("unerwarteter command: {other:?}"),
            Ok(None) => panic!("midi channel geschlossen"),
            Err(_) => continue,
        }
    }
    assert!(got_go, "kein Go-Command für Note 36 (custom map)");

    // Default-Note 0 ist mit der Custom-Map nicht mehr gemappt.
    sender.send(&[0x90, 0, 100]).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(400), rx.recv())
            .await
            .is_err(),
        "Note 0 darf mit Custom-Map keinen Command erzeugen"
    );

    sender.send(&[0x90, 127, 100]).unwrap();
    let cmd = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout panic")
        .expect("channel geschlossen");
    assert!(matches!(cmd, RemoteCommand::Panic));
}
