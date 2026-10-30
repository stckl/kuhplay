//! Fehler- & Resilienz-Tests.
//!
//! Simuliert fehlerhafte Dateipfade, beschädigte (leere) Container und
//! Treiberausfälle. Erwartung: Die Engine meldet `EngineEvent::Error`, der
//! betroffene Cue wechselt sauber in den `Error`-Zustand (State-Machine),
//! das System stürzt nicht ab und bleibt arbeitsfähig (Graceful Recovery).

use std::io::Write;
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, LoopMode};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

fn setup_validating() -> (TestEngine, Receiver<EngineEvent>) {
    let (tx, rx) = unbounded();
    let (engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    (engine.with_path_validation(), rx)
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

fn play_cmd(cue_id: Uuid, path: Option<String>) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path,
        duration: Some(Duration::from_millis(100)),
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: None,
        stage: None,
        layer: 0,
        surface: None,
    }
}

fn assert_error_for(events: &[EngineEvent], cue_id: Uuid, needle: &str) {
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(id), message }
                if *id == cue_id && message.contains(needle)
        )),
        "expected Error event containing '{needle}' for cue {cue_id}, got: {events:?}"
    );
}

#[test]
fn missing_media_file_yields_error_event_and_engine_survives() {
    let (mut engine, rx) = setup_validating();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(
            id,
            Some("/definitely/not/existing.wav".to_string()),
        ))
        .unwrap();

    let events = drain(&rx);
    assert_error_for(&events, id, "not found");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { .. })),
        "no playback may start for a missing file"
    );

    // Engine bleibt arbeitsfähig (kein Crash, kein Hang).
    engine.handle(EngineCommand::Ping(7)).unwrap();
    assert!(drain(&rx).contains(&EngineEvent::Pong(7)));
}

#[test]
fn empty_container_yields_corruption_error() {
    let (mut engine, rx) = setup_validating();
    let file = tempfile::NamedTempFile::new().expect("temp file");
    // 0 Bytes → simulierter beschädigter Container.
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(
            id,
            Some(file.path().to_string_lossy().to_string()),
        ))
        .unwrap();

    let events = drain(&rx);
    assert_error_for(&events, id, "corrupted");
}

#[test]
fn valid_media_file_plays_and_finishes_without_error() {
    let (mut engine, rx) = setup_validating();
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(b"RIFF....WAVEfmt ").unwrap();
    file.flush().unwrap();

    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(
            id,
            Some(file.path().to_string_lossy().to_string()),
        ))
        .unwrap();

    engine.advance_time(Duration::from_millis(100)); // pre-wait = 0 → Started
    engine.advance_time(Duration::from_millis(100)); // duration → Finished
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == id)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::Error { .. })),
        "valid file must not produce errors"
    );
}

#[test]
fn driver_failure_yields_error_and_engine_survives() {
    let (tx, rx) = unbounded();
    let (engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let mut engine = engine.with_driver_failure();

    let id = Uuid::new_v4();
    engine.handle(play_cmd(id, None)).unwrap();
    let events = drain(&rx);
    assert_error_for(&events, id, "driver");

    // Engine bleibt arbeitsfähig.
    engine.handle(EngineCommand::Ping(1)).unwrap();
    assert!(drain(&rx).contains(&EngineEvent::Pong(1)));
}

#[test]
fn error_cue_recovers_via_reset_and_can_be_retriggered() {
    // Domain-Seite der Resilienz: Error → Reset → Standby → GO.
    let mut list = CueList::new();
    list.add(Cue::new("1", "defekt", CueType::Audio).with_target("/missing.wav"));

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    assert_eq!(list.cues()[0].status, CueStatus::Playing);

    // Engine-Error trifft ein → Error (gültiger Übergang Playing → Error).
    let id = list.cues()[0].id;
    list.set_status(id, CueStatus::Error).unwrap();
    assert_eq!(list.cues()[0].status, CueStatus::Error);

    // Recovery: Reset → Standby, Cue kann erneut getriggert werden.
    list.reset();
    assert_eq!(list.cues()[0].status, CueStatus::Standby);
    assert!(
        list.go().is_some(),
        "cue must be re-triggerable after reset"
    );
}

#[test]
fn stop_all_during_error_storm_leaves_no_active_playbacks() {
    let (mut engine, rx) = setup_validating();
    // 50 fehlschlagende Plays + 50 gültige (ohne Datei = stille Wiedergabe).
    for i in 0..50 {
        let bad = Uuid::new_v4();
        engine
            .handle(play_cmd(bad, Some(format!("/missing/{i}.wav"))))
            .unwrap();
        let good = Uuid::new_v4();
        engine.handle(play_cmd(good, None)).unwrap();
    }
    engine.handle(EngineCommand::StopAll).unwrap();

    let events = drain(&rx);
    let errors = events
        .iter()
        .filter(|e| matches!(e, EngineEvent::Error { .. }))
        .count();
    assert_eq!(
        errors, 50,
        "every missing file must produce exactly one Error"
    );
    engine.advance_time(Duration::from_secs(60));
    // Nach StopAll: keine Playbacks aktiv, keine verspäteten Events.
    assert_eq!(engine.active_playback_count(), 0);
    assert!(drain(&rx).is_empty(), "no stale events after StopAll");
}
