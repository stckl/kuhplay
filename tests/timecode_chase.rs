//! Tests für Timecode-Chasing (LTC/MTC): Cues koppeln sich an eine externe
//! Clock. Bei Timecode-Sprüngen (vorwärts *und* rückwärts) springt das Medium
//! exakt auf die entsprechende Millisekunde – keine Wasserfall-Wiedergabe.

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Fps, LoopMode, Timecode};
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

fn tc(seconds: u64) -> Timecode {
    Timecode::from_duration(Duration::from_secs(seconds), Fps::Fps25)
}

fn play_chasing(cue_id: Uuid, duration: Duration, timecode_in: Timecode) -> EngineCommand {
    EngineCommand::Play {
        cue_id,
        path: None,
        duration: Some(duration),
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
        loop_mode: LoopMode::Once,
        slices: Vec::new(),
        route: None,
        timecode_in: Some(timecode_in),
        stage: None,
        layer: 0,
        surface: None,
        in_point: None,
        out_point: None,
    }
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

fn jumps(events: &[EngineEvent]) -> Vec<(Duration, Duration)> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::TimecodeJump { from, to, .. } => Some((*from, *to)),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Timecode-Werttyp (Integrations-Sanity)
// ---------------------------------------------------------------------------

#[test]
fn timecode_converts_frame_exact() {
    let t = Timecode::from_hmsf(0, 0, 10, 0, Fps::Fps25);
    assert_eq!(t.to_duration(), Duration::from_secs(10));
    assert_eq!(t.to_string(), "00:00:10:00");
    // 1 Frame @ 25 fps = 40 ms.
    assert_eq!(t.add_frames(1).to_duration(), Duration::from_millis(10040));
    assert_eq!(t.add_frames(1).sub_frames(1), t);
}

// ---------------------------------------------------------------------------
// Chasing
// ---------------------------------------------------------------------------

#[test]
fn chase_in_sync_produces_no_jump() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    // Cue startet bei LTC 10 s; Medienposition = LTC − 10 s.
    engine
        .handle(play_chasing(id, Duration::from_secs(60), tc(10)))
        .unwrap();
    engine.advance_time(Duration::from_millis(1000)); // Started (t=1000)
    engine.advance_time(Duration::from_millis(1000)); // t=2000: Medienposition 1 s
    drain(&rx);

    // LTC = 11 s → Ziel-Medienposition 1 s = aktuelle Position → kein Sprung.
    engine.receive_timecode(tc(11));
    assert!(
        jumps(&drain(&rx)).is_empty(),
        "in-sync timecode must not cause a jump"
    );
}

#[test]
fn chase_forward_jump_seeks_to_the_exact_millisecond() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_chasing(id, Duration::from_secs(60), tc(10)))
        .unwrap();
    engine.advance_time(Duration::from_millis(1000)); // Started
    engine.advance_time(Duration::from_millis(1000)); // Medienposition 1 s
    drain(&rx);

    // LTC springt von 11 s auf 13 s → Medium muss exakt auf 3 s springen.
    engine.receive_timecode(tc(13));
    let events = drain(&rx);
    assert_eq!(
        jumps(&events),
        vec![(Duration::from_secs(1), Duration::from_secs(3))],
        "TimecodeJump reports from/to exactly"
    );
    assert_eq!(last_position(&events), Some(Duration::from_secs(3)));

    // Wiedergabe läuft ab der Sprungposition weiter.
    engine.advance_time(Duration::from_millis(500));
    assert_eq!(
        last_position(&drain(&rx)),
        Some(Duration::from_millis(3500))
    );
}

#[test]
fn chase_backward_jump_seeks_back_exactly() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_chasing(id, Duration::from_secs(60), tc(10)))
        .unwrap();
    engine.advance_time(Duration::from_millis(1000));
    engine.advance_time(Duration::from_millis(4000)); // Medienposition 4 s
    drain(&rx);

    // LTC springt rückwärts: 14 s → 11 s + 13 Frames (11.52 s, frame-exact)
    // → Medium exakt auf 1.52 s.
    engine.receive_timecode(Timecode::from_hmsf(0, 0, 11, 13, Fps::Fps25));
    let events = drain(&rx);
    assert_eq!(
        jumps(&events),
        vec![(Duration::from_secs(4), Duration::from_millis(1520))]
    );
    assert_eq!(last_position(&events), Some(Duration::from_millis(1520)));
}

#[test]
fn chase_ignores_drift_within_one_frame() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_chasing(id, Duration::from_secs(60), tc(10)))
        .unwrap();
    engine.advance_time(Duration::from_millis(1000));
    engine.advance_time(Duration::from_millis(1000)); // Medienposition 1 s
    drain(&rx);

    // +1 Frame (40 ms @ 25 fps) Drift → innerhalb der Toleranz, kein Sprung.
    engine.receive_timecode(Timecode::from_duration(
        Duration::from_millis(11_040),
        Fps::Fps25,
    ));
    assert!(
        jumps(&drain(&rx)).is_empty(),
        "drift ≤ 1 frame must be tolerated (no waterfall playback)"
    );

    // +2 Frames (80 ms) → echter Sprung.
    engine.receive_timecode(Timecode::from_duration(
        Duration::from_millis(11_080),
        Fps::Fps25,
    ));
    let events = drain(&rx);
    assert_eq!(jumps(&events).len(), 1, "larger drift causes an exact seek");
}

#[test]
fn chase_jump_is_clamped_to_the_media_duration() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(play_chasing(id, Duration::from_secs(5), tc(0)))
        .unwrap();
    engine.advance_time(Duration::from_millis(1000));
    engine.advance_time(Duration::from_millis(1000)); // Medienposition 1 s
    drain(&rx);

    // LTC springt weit über das Medienende hinaus → clamp auf 5 s.
    engine.receive_timecode(tc(100));
    let events = drain(&rx);
    assert_eq!(
        jumps(&events),
        vec![(Duration::from_secs(1), Duration::from_secs(5))],
        "jump target is clamped to the media duration"
    );

    // Nächster Tick: Position erreicht das Ende → Finished.
    engine.advance_time(Duration::from_millis(1));
    assert!(
        drain(&rx)
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "playback finishes right after the clamped jump"
    );
}

#[test]
fn non_chasing_cue_is_unaffected_by_timecode_input() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: Vec::new(),
            route: None,
            timecode_in: None, // kein Chasing
            stage: None,
            layer: 0,
            surface: None,
            in_point: None,
            out_point: None,
        })
        .unwrap();
    engine.advance_time(Duration::from_millis(2000));
    drain(&rx);

    engine.receive_timecode(tc(30));
    assert!(
        drain(&rx).is_empty(),
        "cues without timecode_in must ignore external timecode"
    );
}

#[test]
fn chasing_can_be_enabled_and_disabled_via_command() {
    let (mut engine, rx) = setup();
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: Vec::new(),
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
            in_point: None,
            out_point: None,
        })
        .unwrap();
    engine.advance_time(Duration::from_millis(1000)); // Started
    engine.advance_time(Duration::from_millis(1000)); // Medienposition 1 s
    drain(&rx);

    // Chasing aktivieren (Offset 0) → LTC 5 s → Sprung auf 5 s.
    engine
        .handle(EngineCommand::ChaseTimecode {
            cue_id: id,
            timecode_in: Some(tc(0)),
        })
        .unwrap();
    engine.receive_timecode(tc(5));
    assert_eq!(
        jumps(&drain(&rx)),
        vec![(Duration::from_secs(1), Duration::from_secs(5))]
    );

    // Chasing deaktivieren → LTC-Eingaben ohne Wirkung.
    engine
        .handle(EngineCommand::ChaseTimecode {
            cue_id: id,
            timecode_in: None,
        })
        .unwrap();
    engine.receive_timecode(tc(50));
    assert!(drain(&rx).is_empty(), "disabled chasing must not react");
}
