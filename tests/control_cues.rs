//! Tests für Control-Cues: Start, Stop, Pause, Load, Reset, Devamp, GoTo,
//! Arm/Disarm – sowohl die Domain-Logik (CueList/GO) als auch die
//! Engine-Kommandos (MockMediaEngine mit virtueller Clock).

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{ControlAction, Cue, CueList, CueStatus, CueType, LoopMode};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

fn audio(number: &str) -> Cue {
    Cue::new(number, number, CueType::Audio).with_target("/media/x.wav")
}

fn control(number: &str, action: ControlAction, target: Option<&str>) -> Cue {
    let cue = Cue::new(number, number, CueType::Control).with_control(action);
    match target {
        Some(t) => cue.with_control_target(t),
        None => cue,
    }
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

// ---------------------------------------------------------------------------
// Domain: Control-Cues sind Sofort-Cues
// ---------------------------------------------------------------------------

#[test]
fn control_cues_are_instant_and_complete_immediately() {
    let mut list = CueList::new();
    list.add(control("1", ControlAction::Stop, None));
    list.add(control("2", ControlAction::Load, None));
    list.add(control("3", ControlAction::Arm, None));

    for expected_commands in [1, 1, 0] {
        let outcome = list.go().expect("control cue must trigger");
        assert_eq!(outcome.commands.len(), expected_commands);
        assert_eq!(list.cues()[outcome.index].status, CueStatus::Completed);
    }
    assert!(list.go().is_none());
}

#[test]
fn start_with_target_triggers_target_directly_bypassing_playhead() {
    let mut list = CueList::new();
    list.add(audio("1"));
    list.add(control("2", ControlAction::Start, Some("1")));
    list.add(audio("3"));

    // Playhead steht auf "1" (Index 0).
    let outcome = list.go().unwrap();
    assert_eq!(outcome.index, 0);
    // GO triggert den Control-Cue "2" – Start feuert Ziel "1" direkt,
    // obwohl der Playhead bereits auf "3" (Index 2) zeigen würde.
    let outcome = list.go().unwrap();
    assert_eq!(outcome.index, 1);
    assert_eq!(
        outcome.commands.len(),
        0,
        "Start of an already-playing cue is a no-op command-wise"
    );
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Playing,
        "target was already running"
    );
    assert_eq!(list.cues()[1].status, CueStatus::Completed);
}

#[test]
fn start_targets_a_standby_cue_regardless_of_playhead() {
    let mut list = CueList::new();
    list.add(audio("1"));
    list.add(control("2", ControlAction::Start, Some("3")));
    list.add(audio("3").with_pre_wait(Duration::from_millis(250)));

    let _ = list.go().unwrap(); // "1" läuft, Playhead → 1
    let outcome = list.go().unwrap(); // Control "2": Start auf "3" (Index 2)
    assert_eq!(
        outcome.commands.len(),
        1,
        "Start must produce the target's Play command"
    );
    assert!(matches!(outcome.commands[0], EngineCommand::Play { .. }));
    assert_eq!(
        list.cues()[2].status,
        CueStatus::PreWait,
        "target has pre_wait > 0"
    );
    assert_eq!(list.cues()[1].status, CueStatus::Completed);
}

#[test]
fn stop_without_target_sends_stop_all_with_target_sends_stop() {
    let mut list = CueList::new();
    let target = audio("3");
    let target_id = target.id;
    list.add(control("1", ControlAction::Stop, None));
    list.add(control("2", ControlAction::Stop, Some("3")));
    list.add(target);

    let outcome = list.go().unwrap();
    assert_eq!(outcome.commands, vec![EngineCommand::StopAll]);

    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::Stop { cue_id: target_id }]
    );
}

#[test]
fn pause_without_target_pauses_every_running_cue() {
    let mut list = CueList::new();
    list.add(audio("1"));
    list.add(audio("2"));
    list.add(control("3", ControlAction::Pause, None));

    let _ = list.go().unwrap();
    let _ = list.go().unwrap();
    let id1 = list.cues()[0].id;
    let id2 = list.cues()[1].id;

    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![
            EngineCommand::Pause { cue_id: id1 },
            EngineCommand::Pause { cue_id: id2 }
        ],
        "Pause ohne Ziel pausiert alle laufenden Cues"
    );
}

#[test]
fn pause_with_target_sends_single_pause() {
    let mut list = CueList::new();
    let target = audio("2");
    let target_id = target.id;
    list.add(control("1", ControlAction::Pause, Some("2")));
    list.add(target);

    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::Pause { cue_id: target_id }]
    );
}

#[test]
fn load_sends_load_command_with_target_path() {
    let mut list = CueList::new();
    list.add(control("1", ControlAction::Load, Some("2")));
    list.add(audio("2").with_target("/media/preload.wav"));

    let target_id = list.cues()[1].id;
    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::Load {
            cue_id: target_id,
            path: Some("/media/preload.wav".to_string())
        }]
    );
}

#[test]
fn devamp_without_target_sends_devamp_all_with_target_sends_devamp() {
    let mut list = CueList::new();
    let target = audio("3");
    let target_id = target.id;
    list.add(control("1", ControlAction::Devamp, None));
    list.add(control("2", ControlAction::Devamp, Some("3")));
    list.add(target);

    let outcome = list.go().unwrap();
    assert_eq!(outcome.commands, vec![EngineCommand::DevampAll]);

    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::Devamp { cue_id: target_id }]
    );
}

#[test]
fn arm_and_disarm_toggle_the_target_cue() {
    let mut list = CueList::new();
    list.add(audio("1"));
    list.add(control("2", ControlAction::Disarm, Some("1")));
    list.add(control("3", ControlAction::Arm, Some("1")));

    assert!(list.cues()[0].is_armed);
    let _ = list.go().unwrap(); // "1" läuft (Playhead → 1)
                                // Disarm "1" (Control "2").
    let _ = list.go().unwrap();
    assert!(!list.cues()[0].is_armed, "Disarm deaktiviert den Ziel-Cue");
    // Arm "1" (Control "3").
    let _ = list.go().unwrap();
    assert!(list.cues()[0].is_armed, "Arm aktiviert den Ziel-Cue erneut");
}

#[test]
fn disarmed_target_is_skipped_by_later_go() {
    let mut list = CueList::new();
    list.add(control("1", ControlAction::Disarm, Some("2")));
    list.add(audio("2"));
    list.add(audio("3"));

    let _ = list.go().unwrap(); // Disarm "2"
    let outcome = list.go().unwrap();
    assert_eq!(outcome.index, 2, "disarmed cue \"2\" wird übersprungen");
}

// ---------------------------------------------------------------------------
// Engine: Control-Cue-Commands in der MockMediaEngine
// ---------------------------------------------------------------------------

#[test]
fn engine_load_emits_loaded_event() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();

    engine
        .handle(EngineCommand::Load {
            cue_id: id,
            path: Some("/media/x.wav".to_string()),
        })
        .unwrap();
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Loaded { cue_id } if *cue_id == id)));
}

#[test]
fn engine_devamp_finishes_an_infinite_loop_immediately() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();

    // Endloser Loop (Devamp beendet ihn sofort und springt zum Outro).
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_millis(100)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Infinite,
            slices: Vec::new(),
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // Loop-Iteration 1

    engine.handle(EngineCommand::Devamp { cue_id: id }).unwrap();
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Devamp must finish an infinite loop: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::PostWaitElapsed { cue_id } if *cue_id == id)),
        "post_wait = 0 → PostWaitElapsed folgt sofort"
    );
    assert_eq!(engine.active_playback_count(), 0);

    // Danach läuft nichts mehr nach.
    engine.advance_time(Duration::from_secs(5));
    assert!(drain(&rx).is_empty());
}

#[test]
fn engine_devamp_all_finishes_every_playback() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = Uuid::new_v4();
        ids.push(id);
        engine
            .handle(EngineCommand::Play {
                cue_id: id,
                path: None,
                duration: Some(Duration::from_secs(60)),
                pre_wait: Duration::ZERO,
                post_wait: Duration::ZERO,
                loop_mode: LoopMode::Infinite,
                slices: Vec::new(),
                route: None,
                timecode_in: None,
                stage: None,
                layer: 0,
                surface: None,
            })
            .unwrap();
    }
    engine.advance_time(Duration::from_millis(100)); // alle gestartet
    engine.handle(EngineCommand::DevampAll).unwrap();

    let events = drain(&rx);
    for id in &ids {
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::Finished { cue_id } if cue_id == id)),
            "DevampAll must finish cue {id}"
        );
    }
    assert_eq!(engine.active_playback_count(), 0);
}

#[test]
fn full_control_cue_flow_go_to_engine_roundtrip() {
    // Kompletter Ablauf: Control-Cue feuert Devamp-Command, Engine beendet Loop,
    // Event landet als Completed in der CueList (wie die App es abbildet).
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);

    let mut list = CueList::new();
    list.add(
        Cue::new("1", "Endless", CueType::Audio)
            .with_duration(Duration::from_millis(100))
            .with_loop(LoopMode::Infinite),
    );
    let target_id = list.cues()[0].id;
    list.add(control("2", ControlAction::Devamp, Some("1")));

    // GO 1: Endless-Loop startet.
    let outcome = list.go().unwrap();
    for cmd in outcome.commands {
        engine.handle(cmd).unwrap();
    }
    engine.advance_time(Duration::from_millis(100));
    let _ = list.set_status(target_id, CueStatus::Playing);
    drain(&rx);

    // GO 2: Devamp-Control-Cue.
    let outcome = list.go().unwrap();
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::Devamp { cue_id: target_id }]
    );
    for cmd in outcome.commands {
        engine.handle(cmd).unwrap();
    }
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Completed,
        "control cue is instant"
    );

    // Engine-Event → Domain-Status.
    let events = drain(&rx);
    for event in events {
        if let EngineEvent::Finished { cue_id } = event {
            let _ = list.set_status(cue_id, CueStatus::Completed);
        }
    }
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "devamped cue completed"
    );
}
