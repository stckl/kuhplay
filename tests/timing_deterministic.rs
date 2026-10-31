//! Deterministische Timing-Tests mit virtueller Clock.
//!
//! Die [`MockMediaEngine`] wird mit einer [`VirtualClock`] betrieben, die der
//! Test manuell vorstellt (`advance_time`). So werden Pre-Wait, Post-Wait,
//! Fades, Loop-Grenzen, Timecode/Seek und Frame-Drops auf die **exakte
//! Millisekunde** verifiziert – ohne `sleep()` und ohne Flakiness.

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{FadeCurve, FadeParam, LoopMode};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

fn setup() -> (TestEngine, VirtualClock, Receiver<EngineEvent>) {
    let (tx, rx) = unbounded();
    let (engine, clock) = MockMediaEngine::with_virtual_clock(tx);
    (engine, clock, rx)
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

fn play_cmd(
    cue_id: Uuid,
    duration_ms: u64,
    pre_ms: u64,
    post_ms: u64,
    loop_mode: LoopMode,
) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: None,
        duration: Some(Duration::from_millis(duration_ms)),
        pre_wait: Duration::from_millis(pre_ms),
        post_wait: Duration::from_millis(post_ms),
        loop_mode,
        slices: Vec::new(),
        route: None,
        timecode_in: None,
        stage: None,
        layer: 0,
        surface: None,
    }
}

fn count_matching(events: &[EngineEvent], pred: impl Fn(&EngineEvent) -> bool) -> usize {
    events.iter().filter(|e| pred(e)).count()
}

fn last_fade_value(events: &[EngineEvent]) -> Option<f32> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::FadeProgress { value, .. } => Some(*value),
            _ => None,
        })
        .next_back()
}

fn last_position(events: &[EngineEvent]) -> Option<Duration> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Position { position, .. } => Some(*position),
            _ => None,
        })
        .next_back()
}

// ---------------------------------------------------------------------------
// Pre-Wait / Post-Wait
// ---------------------------------------------------------------------------

#[test]
fn pre_wait_starts_playback_at_the_exact_millisecond() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 100, 200, 0, LoopMode::Once))
        .unwrap();

    engine.advance_time(Duration::from_millis(199));
    assert!(
        drain(&rx).is_empty(),
        "no events before pre-wait has elapsed"
    );

    engine.advance_time(Duration::from_millis(1)); // t = 200 ms
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == id)),
        "Started exactly when pre-wait elapsed"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { .. })));

    engine.advance_time(Duration::from_millis(99)); // t = 299 ms
    assert!(
        !drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { .. })),
        "playback not finished before its duration"
    );

    engine.advance_time(Duration::from_millis(1)); // t = 300 ms
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Finished exactly at pre_wait + duration"
    );
}

#[test]
fn post_wait_elapses_exactly_after_finish() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 100, 0, 150, LoopMode::Once))
        .unwrap();

    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // t = 200: Finished
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::PostWaitElapsed { .. })),
        "post-wait must not elapse together with Finished"
    );

    engine.advance_time(Duration::from_millis(149)); // t = 349
    assert!(
        drain(&rx).is_empty(),
        "post-wait not elapsed one millisecond early"
    );

    engine.advance_time(Duration::from_millis(1)); // t = 350
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::PostWaitElapsed { cue_id } if *cue_id == id)),
        "PostWaitElapsed exactly at Finished + post_wait"
    );
}

// ---------------------------------------------------------------------------
// Loops
// ---------------------------------------------------------------------------

#[test]
fn loop_count_plays_exactly_n_times_with_iteration_events() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 100, 0, 0, LoopMode::Count(3)))
        .unwrap();

    for _ in 0..4 {
        engine.advance_time(Duration::from_millis(100));
    }
    let events = drain(&rx);

    assert_eq!(
        count_matching(&events, |e| matches!(e, EngineEvent::LoopIteration { .. })),
        2,
        "3 plays = 2 loop boundaries"
    );
    let iterations: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::LoopIteration { iteration, .. } => Some(*iteration),
            _ => None,
        })
        .collect();
    assert_eq!(iterations, vec![1, 2], "iterations are 1-based and ordered");
    assert_eq!(
        count_matching(&events, |e| matches!(e, EngineEvent::Finished { .. })),
        1,
        "exactly one Finished after the last iteration"
    );

    // Danach: keine weiteren Events (Playback entfernt).
    engine.advance_time(Duration::from_secs(10));
    assert!(drain(&rx).is_empty(), "no events after completion");
}

#[test]
fn loop_infinite_never_finishes() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 100, 0, 0, LoopMode::Infinite))
        .unwrap();

    for _ in 0..10 {
        engine.advance_time(Duration::from_millis(100));
    }
    let events = drain(&rx);
    assert_eq!(
        count_matching(&events, |e| matches!(e, EngineEvent::LoopIteration { .. })),
        9,
        "t=100 Started, boundaries at 200..1000"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { .. })),
        "infinite loop must not finish"
    );
    assert_eq!(engine.active_playback_count(), 1);
}

// ---------------------------------------------------------------------------
// Fades
// ---------------------------------------------------------------------------

#[test]
fn fade_hits_exact_intermediate_values() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Fade {
            target_cue: None,
            cue_id: id,
            param: FadeParam::Volume,
            from: 0.0,
            target: 1.0,
            duration: Duration::from_millis(1000),
            curve: FadeCurve::Linear,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(250));
    assert_eq!(last_fade_value(&drain(&rx)), Some(0.25), "t=250ms");

    engine.advance_time(Duration::from_millis(250));
    assert_eq!(last_fade_value(&drain(&rx)), Some(0.5), "t=500ms");

    engine.advance_time(Duration::from_millis(499));
    let v = last_fade_value(&drain(&rx)).expect("fade progress at t=999ms");
    assert!(v < 1.0, "fade not complete one millisecond early: {v}");

    engine.advance_time(Duration::from_millis(1)); // t = 1000 ms
    let events = drain(&rx);
    assert_eq!(last_fade_value(&events), Some(1.0), "final fade value");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "fade finishes exactly at its duration"
    );
}

#[test]
fn zero_duration_fade_finishes_immediately() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Fade {
            target_cue: None,
            cue_id: id,
            param: FadeParam::Volume,
            from: 0.2,
            target: 0.8,
            duration: Duration::ZERO,
            curve: FadeCurve::Linear,
        })
        .unwrap();
    let events = drain(&rx);
    assert_eq!(last_fade_value(&events), Some(0.8));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

// ---------------------------------------------------------------------------
// Timecode, Seek, Frame-Drop
// ---------------------------------------------------------------------------

#[test]
fn position_reports_media_timecode() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 10_000, 0, 0, LoopMode::Once))
        .unwrap();

    engine.advance_time(Duration::from_millis(1000)); // Started
    engine.advance_time(Duration::from_millis(4000)); // t = 5000
    assert_eq!(
        last_position(&drain(&rx)),
        Some(Duration::from_millis(4000)),
        "timecode = wall time since start"
    );

    engine.advance_time(Duration::from_millis(5000)); // t = 10000
    assert_eq!(
        last_position(&drain(&rx)),
        Some(Duration::from_millis(9000))
    );

    engine.advance_time(Duration::from_millis(1000)); // t = 11000 → media = 10 s
    let events = drain(&rx);
    assert_eq!(last_position(&events), Some(Duration::from_millis(10_000)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

#[test]
fn seek_jumps_timecode_and_finish_time_shifts_accordingly() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 10_000, 0, 0, LoopMode::Once))
        .unwrap();
    engine.advance_time(Duration::from_millis(2000)); // Started + 1 s abgespielt

    engine
        .handle(EngineCommand::Seek {
            cue_id: id,
            position: Duration::from_secs(8),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(e, EngineEvent::Position { position, .. } if *position == Duration::from_secs(8))),
        "Position event reflects the seek target"
    );

    engine.advance_time(Duration::from_millis(1999)); // 9.999 s → noch nicht fertig
    assert!(
        !drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { .. })),
        "not finished 1 ms before the end"
    );
    engine.advance_time(Duration::from_millis(1)); // 10 s → Finished
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

#[test]
fn frame_drop_delays_finish_by_exactly_the_dropped_time() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 1000, 0, 0, LoopMode::Once))
        .unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // t=200, 100 ms abgespielt

    // 10 Frames @ 25 fps = 400 ms verlorene Zeit.
    engine.simulate_frame_drop(id, 10, Duration::from_millis(40));
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::FrameDrop { dropped_frames: 10, total_dropped, .. }
                if *total_dropped == Duration::from_millis(400)
        )),
        "FrameDrop event reports dropped frames and total"
    );

    engine.advance_time(Duration::from_millis(800)); // t=1000: wall 900 ms, media 500 ms
    let events = drain(&rx);
    assert_eq!(
        last_position(&events),
        Some(Duration::from_millis(500)),
        "timecode reflects dropped frames"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { .. })),
        "not finished: playback is 400 ms behind"
    );

    engine.advance_time(Duration::from_millis(500)); // t=1500: media = 1000 ms
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Finished exactly when media timecode reaches the duration"
    );
}

// ---------------------------------------------------------------------------
// Pause / Resume
// ---------------------------------------------------------------------------

#[test]
fn pause_freezes_the_playback_clock() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_cmd(id, 1000, 0, 0, LoopMode::Once))
        .unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(200)); // 200 ms abgespielt

    engine.handle(EngineCommand::Pause { cue_id: id }).unwrap();
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Paused { .. })));

    engine.advance_time(Duration::from_secs(5)); // Pause: keine Zeit vergeht
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, EngineEvent::Position { .. })),
        "no position updates while paused"
    );

    engine.handle(EngineCommand::Resume { cue_id: id }).unwrap();
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Resumed { .. })));

    engine.advance_time(Duration::from_millis(800)); // 200 + 800 = 1000 ms
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Finished based on media time, not wall time"
    );
}

// ---------------------------------------------------------------------------
// Wait-Cue (QLab 5): reine Timer-Unit
// ---------------------------------------------------------------------------

fn wait_cmd(cue_id: Uuid, duration_ms: u64, pre_ms: u64, post_ms: u64) -> EngineCommand {
    EngineCommand::Wait {
        cue_id,
        duration: Duration::from_millis(duration_ms),
        pre_wait: Duration::from_millis(pre_ms),
        post_wait: Duration::from_millis(post_ms),
    }
}

#[test]
fn wait_cue_finishes_exactly_after_pre_wait_plus_duration() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine.handle(wait_cmd(id, 500, 200, 0)).unwrap();

    engine.advance_time(Duration::from_millis(199));
    assert!(drain(&rx).is_empty(), "no events before pre-wait elapsed");

    engine.advance_time(Duration::from_millis(1)); // t = 200: Started
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == id)));

    engine.advance_time(Duration::from_millis(499)); // t = 699
    assert!(!drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { .. })));

    engine.advance_time(Duration::from_millis(1)); // t = 700 = 200 + 500
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Finished exactly at pre_wait + duration"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::PostWaitElapsed { cue_id } if *cue_id == id)),
        "post_wait = 0 → PostWaitElapsed immediately after Finished"
    );
}

#[test]
fn wait_cue_zero_duration_finishes_immediately_after_pre_wait() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine.handle(wait_cmd(id, 0, 0, 0)).unwrap();

    engine.advance_time(Duration::from_millis(1));
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == id)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

#[test]
fn wait_cue_pause_freezes_the_timer_and_resume_continues() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine.handle(wait_cmd(id, 1000, 0, 0)).unwrap();

    engine.advance_time(Duration::from_millis(200)); // t=200: Started
    engine.advance_time(Duration::from_millis(300)); // t=500: 300 ms reine Laufzeit
    drain(&rx);
    engine.handle(EngineCommand::Pause { cue_id: id }).unwrap();
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Paused { cue_id } if *cue_id == id)));

    engine.advance_time(Duration::from_millis(5000)); // t=5500: Pause zählt nicht
    assert!(!drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { .. })));

    engine.handle(EngineCommand::Resume { cue_id: id }).unwrap();
    drain(&rx);
    engine.advance_time(Duration::from_millis(699)); // 300 + 699 = 999 ms
    assert!(!drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { .. })));
    engine.advance_time(Duration::from_millis(1)); // 1000 ms reine Laufzeit
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

#[test]
fn wait_cue_stop_emits_stopped_and_no_finish() {
    let (mut engine, _clock, rx) = setup();
    let id = Uuid::new_v4();
    engine.handle(wait_cmd(id, 1000, 0, 0)).unwrap();
    engine.advance_time(Duration::from_millis(100));
    drain(&rx);

    engine.handle(EngineCommand::Stop { cue_id: id }).unwrap();
    assert!(drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Stopped { cue_id } if *cue_id == id)));

    engine.advance_time(Duration::from_millis(5000));
    assert!(!drain(&rx)
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { .. })));
}
