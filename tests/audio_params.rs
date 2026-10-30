//! Tests für Audio-Features: dB-Fades (gehörrichtige Interpolation),
//! Rate-Fades, Media-Slices (Segment-Plan mit Play Counts) und
//! Audio-Routing-Validierung – deterministisch auf die Millisekunde.

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{AudioRoute, FadeCurve, FadeParam, LoopMode, MediaSlice};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

fn setup() -> (TestEngine, Receiver<EngineEvent>) {
    let (tx, rx) = unbounded();
    let (engine, _clock) = MockMediaEngine::with_virtual_clock(tx);
    (engine, rx)
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

fn play_cmd(cue_id: Uuid, duration_ms: u64) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: None,
        duration: Some(Duration::from_millis(duration_ms)),
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

fn last_fade(events: &[EngineEvent]) -> Option<(FadeParam, f32)> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::FadeProgress { param, value, .. } => Some((*param, *value)),
            _ => None,
        })
        .next_back()
}

// ---------------------------------------------------------------------------
// dB-Fades
// ---------------------------------------------------------------------------

#[test]
fn db_fade_hits_exact_decibel_midpoint() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    // 0 dB → -6.02 dB (Halbierung). Hälfte der Zeit = -3.01 dB = √0.5 ≈ 0.7071.
    engine
        .handle(EngineCommand::Fade {
            target_cue: None,
            cue_id: id,
            param: FadeParam::Volume,
            from: 1.0,
            target: 0.5,
            duration: Duration::from_millis(1000),
            curve: FadeCurve::Decibel,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(500));
    let (param, value) = last_fade(&drain(&rx)).expect("fade progress at t=500ms");
    assert_eq!(param, FadeParam::Volume, "FadeProgress carries the param");
    assert!(
        (value - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3,
        "dB midpoint must be sqrt(0.5) ≈ 0.7071, got {value}"
    );

    engine.advance_time(Duration::from_millis(500)); // t = 1000 ms
    let events = drain(&rx);
    let (param, value) = last_fade(&events).expect("final fade value");
    assert_eq!(param, FadeParam::Volume);
    assert!(
        (value - 0.5).abs() < 1e-5,
        "final value must be 0.5, got {value}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "fade finishes at its duration"
    );
}

#[test]
fn linear_fade_midpoint_is_the_arithmetic_mean() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Fade {
            target_cue: None,
            cue_id: id,
            param: FadeParam::Volume,
            from: 1.0,
            target: 0.5,
            duration: Duration::from_millis(1000),
            curve: FadeCurve::Linear,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(500));
    assert_eq!(
        last_fade(&drain(&rx)),
        Some((FadeParam::Volume, 0.75)),
        "linear midpoint = arithmetic mean (contrast to dB)"
    );
}

#[test]
fn rate_fade_emits_rate_param_events() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Fade {
            target_cue: None,
            cue_id: id,
            param: FadeParam::Rate,
            from: 1.0,
            target: 2.0,
            duration: Duration::from_millis(1000),
            curve: FadeCurve::Linear,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(250));
    assert_eq!(last_fade(&drain(&rx)), Some((FadeParam::Rate, 1.25)));

    engine.advance_time(Duration::from_millis(750));
    let events = drain(&rx);
    assert_eq!(last_fade(&events), Some((FadeParam::Rate, 2.0)));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

// ---------------------------------------------------------------------------
// Media-Slices
// ---------------------------------------------------------------------------

#[test]
fn slices_play_each_segment_with_its_own_play_count() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    // Plan: A (100 ms) 2×, dann B (50 ms) 1× → LoopIteration bei 200/300 ms,
    // Finished bei 350 ms (exakt: 100 + 100 + 50).
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_secs(99)), // wird von slices überschrieben
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Infinite, // wird von slices überschrieben
            slices: vec![
                MediaSlice::new(Duration::ZERO, Duration::from_millis(100), 2),
                MediaSlice::new(Duration::from_millis(100), Duration::from_millis(150), 1),
            ],
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(100)); // t=100: Started, Slice A (Rep 1)
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { .. })));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EngineEvent::LoopIteration { .. })),
        "no loop boundary inside the first slice repetition"
    );

    engine.advance_time(Duration::from_millis(100)); // t=200: A Rep 1 fertig → Loop 1
    engine.advance_time(Duration::from_millis(100)); // t=300: A Rep 2 fertig → Loop 2
    let events = drain(&rx);
    let iterations: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::LoopIteration { iteration, .. } => Some(*iteration),
            _ => None,
        })
        .collect();
    assert_eq!(
        iterations,
        vec![1, 2],
        "one LoopIteration per slice repetition"
    );

    engine.advance_time(Duration::from_millis(49)); // t=349: Slice B läuft (49 ms)
    assert!(
        !drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { .. })),
        "not finished 1 ms before the plan ends"
    );
    engine.advance_time(Duration::from_millis(1)); // t=350: Plan erschöpft
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "Finished exactly at the end of the slice plan (350 ms)"
    );
    assert_eq!(engine.active_playback_count(), 0);
}

#[test]
fn slices_report_position_within_the_current_segment() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: None,
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: vec![
                MediaSlice::new(Duration::ZERO, Duration::from_millis(100), 2),
                MediaSlice::new(Duration::ZERO, Duration::from_millis(50), 1),
            ],
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();

    engine.advance_time(Duration::from_millis(100)); // t=100: Started, Slice A Rep 1
    engine.advance_time(Duration::from_millis(100)); // t=200: Rep 1 fertig → Rep 2
    engine.advance_time(Duration::from_millis(50)); // t=250: Rep 2, 50 ms
    let positions: Vec<Duration> = drain(&rx)
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Position { position, .. } => Some(*position),
            _ => None,
        })
        .collect();
    assert_eq!(
        positions.last(),
        Some(&Duration::from_millis(50)),
        "position is relative to the current slice repetition"
    );
}

// ---------------------------------------------------------------------------
// Validierung: Slices & Routing
// ---------------------------------------------------------------------------

#[test]
fn invalid_slice_yields_error_event_and_no_playback() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_millis(100)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: vec![MediaSlice::new(
                Duration::from_millis(200),
                Duration::from_millis(100), // end < start → ungültig
                1,
            )],
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();

    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(cid), message }
                if *cid == id && message.contains("slice")
        )),
        "invalid slice must produce an Error event: {events:?}"
    );
    assert_eq!(engine.active_playback_count(), 0, "no playback may start");
}

#[test]
fn invalid_route_yields_error_event_and_valid_route_plays() {
    let (mut engine, rx) = setup();

    // Ungültig: 0 Kanäle.
    let bad = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: bad,
            path: None,
            duration: Some(Duration::from_millis(100)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: Vec::new(),
            route: Some(AudioRoute::new(0)),
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(cid), message }
                if *cid == bad && message.contains("channel")
        )),
        "invalid route must produce an Error event: {events:?}"
    );

    // Gültig: 8 Kanäle.
    let good = Uuid::new_v4();
    let mut cmd = play_cmd(good, 100);
    if let EngineCommand::Play { route, .. } = &mut cmd {
        *route = Some(AudioRoute::new(8));
    }
    engine.handle(cmd).unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // Finished
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == good)));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == good)),
        "valid route plays normally"
    );
}

#[test]
fn route_patch_validation_end_to_end() {
    let (mut engine, rx) = setup();

    // Patch überhängt die 16 Device-Kanäle (Start 16, 2 Kanäle → 16–17).
    let bad = Uuid::new_v4();
    let mut cmd = play_cmd(bad, 100);
    if let EngineCommand::Play { route, .. } = &mut cmd {
        *route = Some(AudioRoute::with_patch(2, 16));
    }
    engine.handle(cmd).unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(cid), message }
                if *cid == bad && message.contains("patch")
        )),
        "overhanging patch must produce an Error event: {events:?}"
    );
    assert_eq!(engine.active_playback_count(), 0, "no playback may start");

    // Mono-Patch auf den letzten Device-Kanal (16) ist gültig.
    let good = Uuid::new_v4();
    let mut cmd = play_cmd(good, 100);
    if let EngineCommand::Play { route, .. } = &mut cmd {
        *route = Some(AudioRoute::with_patch(1, 16));
    }
    engine.handle(cmd).unwrap();
    engine.advance_time(Duration::from_millis(100)); // Started
    engine.advance_time(Duration::from_millis(100)); // Finished
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { cue_id, .. } if *cue_id == good)),
        "valid patch must play: {events:?}"
    );
}
