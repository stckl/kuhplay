//! End-to-End-Integrationstests: Komplette Show-Simulation.
//!
//! Verbindet Domain ([`CueList`]) und Engine ([`MockMediaEngine`] mit
//! [`VirtualClock`]) so, wie es die App tut: GO → Commands an die Engine →
//! virtuelle Zeit voranschreiten → Events zurück in die CueList mappen.
//! Verifiziert eine komplette Show inkl. Fade, Loop, Group-Kaskade, Stop und
//! Fehlerfall – deterministisch auf die Millisekunde.

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, GroupMode, LoopMode};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

/// Bildet Engine-Events auf Cue-Status ab – exakt wie `ShowApp::poll_engine_events`.
fn apply_events(list: &mut CueList, events: Vec<EngineEvent>) {
    for event in events {
        match event {
            EngineEvent::Started { cue_id, .. } => {
                let _ = list.set_status(cue_id, CueStatus::Playing);
            }
            EngineEvent::Paused { cue_id } => {
                let _ = list.set_status(cue_id, CueStatus::Paused);
            }
            EngineEvent::Resumed { cue_id } => {
                let _ = list.set_status(cue_id, CueStatus::Playing);
            }
            EngineEvent::Finished { cue_id } => {
                // Post-Wait-Phase oder direkt Completed (je nach Cue).
                let post_wait = list
                    .find_cue(cue_id)
                    .map(|c| c.post_wait)
                    .unwrap_or_default();
                let target = if post_wait > Duration::ZERO {
                    CueStatus::PostWait
                } else {
                    CueStatus::Completed
                };
                let _ = list.set_status(cue_id, target);
            }
            EngineEvent::PostWaitElapsed { cue_id } => {
                let _ = list.set_status(cue_id, CueStatus::Completed);
            }
            EngineEvent::Stopped { cue_id } => {
                let _ = list.set_status(cue_id, CueStatus::Stopped);
            }
            EngineEvent::Error {
                cue_id: Some(cue_id),
                ..
            } => {
                let _ = list.set_status(cue_id, CueStatus::Error);
            }
            _ => {}
        }
    }
}

/// GO auslösen und alle erzeugten Commands an die Engine senden (wie die App).
fn go(engine: &mut TestEngine, list: &mut CueList) {
    let outcome = list.go().expect("GO must trigger a cue");
    for command in outcome.commands {
        engine.handle(command).expect("engine must accept command");
    }
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

fn build_show() -> CueList {
    let mut list = CueList::new();
    list.add(
        Cue::new("1", "Fade In", CueType::Fade)
            .with_fade_target(1.0)
            .with_duration(Duration::from_millis(500)),
    );
    list.add(
        Cue::new("2", "Jingle", CueType::Audio)
            .with_target("/media/j.wav")
            .with_pre_wait(Duration::from_millis(100))
            .with_duration(Duration::from_millis(300))
            .with_loop(LoopMode::Count(2))
            .with_post_wait(Duration::from_millis(50)),
    );
    list.add(Cue::new_group(
        "3",
        "Videos",
        GroupMode::Simultaneous,
        vec![
            Cue::new("3.1", "Video A", CueType::Video)
                .with_target("/media/a.mp4")
                .with_duration(Duration::from_millis(400)),
            Cue::new("3.2", "Video B", CueType::Video)
                .with_target("/media/b.mp4")
                .with_duration(Duration::from_millis(400)),
        ],
    ));
    list.add(Cue::new("4", "Blackout", CueType::Stop));
    list
}

#[test]
fn full_show_runs_to_completion_with_exact_timing() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let mut list = build_show();

    // --- Cue 1: Fade (500 ms) ---
    go(&mut engine, &mut list);
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    engine.advance_time(Duration::from_millis(500));
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.cues()[0].status, CueStatus::Completed);

    // --- Cue 2: Jingle (pre 100 ms, 2 × 300 ms, post 50 ms) ---
    go(&mut engine, &mut list);
    engine.advance_time(Duration::from_millis(100)); // Pre-Wait
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.cues()[1].status, CueStatus::Playing);

    for _ in 0..6 {
        engine.advance_time(Duration::from_millis(100));
    }
    let events = drain(&rx);
    let loops = events
        .iter()
        .filter(|e| matches!(e, EngineEvent::LoopIteration { .. }))
        .count();
    assert_eq!(loops, 1, "2 plays = 1 loop boundary");
    apply_events(&mut list, events);
    assert_eq!(
        list.cues()[1].status,
        CueStatus::PostWait,
        "Finished with post_wait > 0 enters the Post-Wait phase"
    );

    engine.advance_time(Duration::from_millis(50)); // Post-Wait
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::PostWaitElapsed { .. })),
        "PostWaitElapsed after post_wait"
    );
    apply_events(&mut list, events);
    assert_eq!(
        list.cues()[1].status,
        CueStatus::Completed,
        "PostWaitElapsed completes the cue"
    );

    // --- Cue 3: Simultaneous-Group (2 Videos à 400 ms) ---
    let outcome = list.go().expect("group must trigger");
    assert_eq!(outcome.commands.len(), 2, "both videos start with one GO");
    for command in outcome.commands {
        engine.handle(command).unwrap();
    }
    engine.advance_time(Duration::from_millis(400)); // Started
    engine.advance_time(Duration::from_millis(400)); // Finished
    apply_events(&mut list, drain(&rx));
    let group = &list.cues()[2];
    assert!(
        group
            .children
            .iter()
            .all(|c| c.status == CueStatus::Completed),
        "both children finished"
    );
    assert_eq!(
        group.status,
        CueStatus::Completed,
        "group closes after all children finished"
    );

    // --- Cue 4: Blackout (Stop) ---
    let outcome = list.go().expect("stop cue must trigger");
    assert!(matches!(
        outcome.commands.as_slice(),
        [EngineCommand::StopAll]
    ));
    assert_eq!(list.cues()[3].status, CueStatus::Completed);

    // Show ist komplett durchgelaufen.
    assert!(list.go().is_none(), "no cue left in standby");
    assert!(
        list.cues().iter().all(|c| c.status == CueStatus::Completed),
        "all top-level cues finished"
    );
}

#[test]
fn show_with_missing_media_marks_cue_error_and_recovers() {
    let (tx, rx) = unbounded();
    let (engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let mut engine = engine.with_path_validation();

    let mut list = CueList::new();
    list.add(
        Cue::new("1", "kaputt", CueType::Audio)
            .with_target("/missing.wav")
            .with_duration(Duration::from_millis(100)),
    );

    go(&mut engine, &mut list);
    assert_eq!(list.cues()[0].status, CueStatus::Playing);

    // Engine meldet Fehler (Datei fehlt) → Cue wechselt nach Broken.
    apply_events(&mut list, drain(&rx));
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Error,
        "missing media must break the cue, not crash the show"
    );

    // Recovery: Reset → Standby → erneut triggerbar.
    list.reset();
    assert_eq!(list.cues()[0].status, CueStatus::Standby);
    assert!(list.go().is_some());
}

#[test]
fn panic_stops_everything_and_show_can_restart() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    let mut list = CueList::new();
    list.add(
        Cue::new("1", "Loop A", CueType::Audio)
            .with_duration(Duration::from_millis(500))
            .with_loop(LoopMode::Infinite),
    );
    list.add(
        Cue::new("2", "Loop B", CueType::Audio)
            .with_duration(Duration::from_millis(500))
            .with_loop(LoopMode::Infinite),
    );

    go(&mut engine, &mut list);
    go(&mut engine, &mut list);
    engine.advance_time(Duration::from_millis(200));
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.running_count(), 2);

    // Panic: StopAll an die Engine + lokaler Stop (wie App::stop_all).
    engine.handle(EngineCommand::StopAll).unwrap();
    list.stop_all_running();
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.running_count(), 0);
    assert!(
        list.cues().iter().all(|c| c.status == CueStatus::Stopped),
        "panic leaves cues in Stopped (not Standby)"
    );

    // Show kann von vorne beginnen.
    list.reset();
    go(&mut engine, &mut list);
    engine.advance_time(Duration::from_millis(100));
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
}

#[test]
fn sequential_group_show_flows_child_by_child() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);

    let mut list = CueList::new();
    list.add(Cue::new_group(
        "1",
        "Sequenz",
        GroupMode::Sequential,
        vec![
            Cue::new("1.1", "Step A", CueType::Audio).with_duration(Duration::from_millis(100)),
            Cue::new("1.2", "Step B", CueType::Audio).with_duration(Duration::from_millis(100)),
        ],
    ));

    // GO 1: Gruppe splicet Kinder in die Liste.
    go(&mut engine, &mut list);
    assert_eq!(list.len(), 3);

    // GO 2: erstes Kind.
    go(&mut engine, &mut list);
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // Finished
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.cues()[1].status, CueStatus::Completed);
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Playing,
        "group still open"
    );

    // GO 3: zweites Kind.
    go(&mut engine, &mut list);
    engine.advance_time(Duration::from_millis(100));
    engine.advance_time(Duration::from_millis(100));
    apply_events(&mut list, drain(&rx));
    assert_eq!(list.cues()[2].status, CueStatus::Completed);
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "group closes after last child"
    );
    assert!(list.go().is_none());
}

#[test]
fn cue_id_lookup_works_across_spliced_and_nested_children() {
    let mut list = CueList::new();
    let group = Cue::new_group(
        "1",
        "G",
        GroupMode::Sequential,
        vec![Cue::new("1.1", "child", CueType::Audio)],
    );
    let group_id = group.id;
    list.add(group);
    let _ = list.go().unwrap(); // splicet child

    let child_id = list.cues()[1].id;
    assert!(list.find_cue(child_id).is_some());
    assert!(list.find_cue(group_id).is_some());
    assert!(list.find_cue(Uuid::new_v4()).is_none());
}
