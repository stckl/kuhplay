//! Tests für Diashow-Cues (`SlideShowCue`).
//!
//! Deckt ab: Validierung der Slide-Definition, `PlaySlideShow`-Command,
//! **Auto-Advance mit exakten Wechsel-Zeitpunkten** (VirtualClock), Loop-Wrap,
//! manuelles Schalten via `SlideNav` und Control-Cues (`NextSlide`/
//! `PrevSlide`), explizite Cue-Duration als Obergrenze sowie Fehlerfälle
//! (fehlende Bilder, Nav auf Nicht-Diashow).

use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{ControlAction, Cue, CueList, CueStatus, CueType, Slide, SlideNav, SlideShow};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock};

type TestEngine = MockMediaEngine<VirtualClock>;

fn slide(name: &str, hold_ms: u64) -> Slide {
    Slide::new(
        format!("/slides/{name}.jpg"),
        Duration::from_millis(hold_ms),
    )
    .expect("test slide is valid")
}

fn show(slides: Vec<Slide>, auto_advance: bool) -> SlideShow {
    SlideShow::new(slides, Duration::from_millis(300), auto_advance).expect("test show is valid")
}

fn play_show_cmd(cue_id: Uuid, show: SlideShow) -> EngineCommand {
    EngineCommand::PlaySlideShow {
        cue_id,
        show,
        stage: Some("Stage 1".to_string()),
        layer: 1,
        surface: None,
        duration: None,
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
    }
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

/// SlideChanged-Events (Index, Zeitpunkt) in Empfangsreihenfolge.
fn slide_changes(events: &[EngineEvent]) -> Vec<(usize, Duration)> {
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::SlideChanged { index, at, .. } => Some((*index, *at)),
            _ => None,
        })
        .collect()
}

/// Startet eine Diashow in der Engine (Tick bei t=0 → Started + Slide 0).
/// Rückgabe: Events des Start-Ticks (inkl. `SlideChanged { index: 0, at: 0 }`).
fn start_show(
    engine: &mut TestEngine,
    rx: &Receiver<EngineEvent>,
    cue_id: Uuid,
    show: SlideShow,
) -> Vec<EngineEvent> {
    engine
        .handle(play_show_cmd(cue_id, show))
        .expect("engine must accept PlaySlideShow");
    engine.advance_time(Duration::ZERO);
    let events = drain(rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { cue_id: id, .. } if *id == cue_id)),
        "engine must report Started: {events:?}"
    );
    events
}

// ---------------------------------------------------------------------------
// Domain: Command-Erzeugung und Validierung
// ---------------------------------------------------------------------------

#[test]
fn slideshow_cue_go_builds_play_slideshow_command() {
    let mut list = CueList::new();
    list.add(
        Cue::new("1", "photos", CueType::SlideShow)
            .with_slideshow(show(vec![slide("a", 1000), slide("b", 2000)], true))
            .with_stage("Stage 1")
            .with_layer(1),
    );

    let outcome = list.go().expect("slideshow cue must trigger");
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    assert_eq!(outcome.commands.len(), 1);
    match &outcome.commands[0] {
        EngineCommand::PlaySlideShow {
            cue_id,
            show,
            stage,
            layer,
            ..
        } => {
            assert_eq!(*cue_id, list.cues()[0].id);
            assert_eq!(show.slide_count(), 2);
            assert_eq!(show.crossfade, Duration::from_millis(300));
            assert!(show.auto_advance);
            assert_eq!(stage.as_deref(), Some("Stage 1"));
            assert_eq!(*layer, 1);
        }
        other => panic!("expected PlaySlideShow, got {other:?}"),
    }
}

#[test]
fn slideshow_cue_without_definition_errors_on_go() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty show", CueType::SlideShow));
    let outcome = list.go().expect("cue triggers (into error)");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn slideshow_cue_serde_roundtrip_preserves_definition() {
    let mut definition = show(vec![slide("a", 1000), slide("b", 2500)], true);
    definition.loop_slides = true;
    let cue = Cue::new("1", "photos", CueType::SlideShow)
        .with_slideshow(definition)
        .with_stage("Stage 2");
    let json = serde_json::to_string(&cue).expect("serialize");
    let back: Cue = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.cue_type, CueType::SlideShow);
    assert_eq!(back.slideshow, cue.slideshow);
    assert_eq!(back.stage.as_deref(), Some("Stage 2"));
}

// ---------------------------------------------------------------------------
// Engine: Auto-Advance mit exakten Zeitpunkten
// ---------------------------------------------------------------------------

#[test]
fn auto_advance_emits_slide_changes_at_exact_times_and_finishes() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    // Holds: 1000 + 2000 + 1000 = 4000 ms Gesamtdauer.
    let events = start_show(
        &mut engine,
        &rx,
        id,
        show(
            vec![slide("a", 1000), slide("b", 2000), slide("c", 1000)],
            true,
        ),
    );

    // Slide 0 ist ab Start aktiv.
    assert_eq!(slide_changes(&events), vec![(0, Duration::ZERO)]);

    // Exakte Wechsel-Zeitpunkte: t=1000 → Slide 1, t=3000 → Slide 2.
    engine.advance_time(Duration::from_millis(1000));
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(1, Duration::from_millis(1000))]
    );

    engine.advance_time(Duration::from_millis(2000));
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(2, Duration::from_millis(3000))]
    );

    // t=4000 = Gesamtdauer → natürliches Ende (kein weiterer Wechsel).
    engine.advance_time(Duration::from_millis(1000));
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "show must finish after the last hold: {events:?}"
    );
    assert!(
        slide_changes(&events).is_empty(),
        "no slide change after the last slide"
    );
}

#[test]
fn auto_advance_emits_nothing_before_first_hold_elapsed() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    start_show(
        &mut engine,
        &rx,
        id,
        show(vec![slide("a", 1000), slide("b", 1000)], true),
    );

    engine.advance_time(Duration::from_millis(999));
    assert!(
        slide_changes(&drain(&rx)).is_empty(),
        "999 ms < hold 1000 ms → noch kein Wechsel"
    );
    engine.advance_time(Duration::from_millis(1));
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(1, Duration::from_millis(1000))],
        "exakt bei 1000 ms wechselt die Show"
    );
}

#[test]
fn loop_slides_wraps_to_first_slide() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    let mut definition = show(vec![slide("a", 1000), slide("b", 1000)], true);
    definition.loop_slides = true;
    start_show(&mut engine, &rx, id, definition);

    engine.advance_time(Duration::from_millis(1000));
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(1, Duration::from_millis(1000))]
    );

    // t=2000: Ende der 2. Slide → Wrap auf Slide 0 (Loop).
    engine.advance_time(Duration::from_millis(1000));
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(0, Duration::from_millis(2000))]
    );

    // Show läuft endlos weiter (kein Finished).
    engine.advance_time(Duration::from_millis(5000));
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "looping slideshow must not finish: {events:?}"
    );
    assert_eq!(slide_changes(&events).len(), 5, "weitere Wechsel im Loop");
}

#[test]
fn manual_slideshow_does_not_auto_advance_and_runs_until_stop() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    start_show(
        &mut engine,
        &rx,
        id,
        show(vec![slide("a", 1000), slide("b", 1000)], false),
    );

    engine.advance_time(Duration::from_secs(10));
    let events = drain(&rx);
    assert!(
        slide_changes(&events).is_empty(),
        "manual show must not auto-advance: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "manual show runs until Stop"
    );

    engine.handle(EngineCommand::Stop { cue_id: id }).unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Stopped { cue_id } if *cue_id == id)));
}

#[test]
fn explicit_duration_caps_the_slideshow() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::PlaySlideShow {
            cue_id: id,
            show: show(
                vec![slide("a", 1000), slide("b", 2000), slide("c", 1000)],
                true,
            ),
            stage: None,
            layer: 0,
            surface: None,
            duration: Some(Duration::from_millis(1500)), // kürzer als Gesamt (4000)
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();
    engine.advance_time(Duration::ZERO);
    engine.advance_time(Duration::from_millis(1000)); // Slide 1
    engine.advance_time(Duration::from_millis(500)); // t=1500 → Duration erreicht
    let events = drain(&rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
        "explicit duration must cap the show: {events:?}"
    );
    assert!(
        !slide_changes(&events).contains(&(2, Duration::from_millis(3000))),
        "no advance past the explicit duration"
    );
}

// ---------------------------------------------------------------------------
// Engine: Manuelles Schalten (SlideNav)
// ---------------------------------------------------------------------------

#[test]
fn slide_nav_switches_slides_manually_with_clamping() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    let events = start_show(
        &mut engine,
        &rx,
        id,
        show(
            vec![slide("a", 60_000), slide("b", 60_000), slide("c", 60_000)],
            false,
        ),
    );
    assert_eq!(slide_changes(&events), vec![(0, Duration::ZERO)]);

    // Next → 1, Next → 2, Next → bleibt auf 2 (Clamp am Ende).
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Next,
        })
        .unwrap();
    assert_eq!(slide_changes(&drain(&rx)), vec![(1, Duration::ZERO)]);
    engine.advance_time(Duration::from_millis(500));
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Next,
        })
        .unwrap();
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(2, Duration::from_millis(500))],
        "exakter Zeitpunkt des manuellen Schalters"
    );
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Next,
        })
        .unwrap();
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(2, Duration::from_millis(500))]
    );

    // Prev → 1, Prev → 0, Prev → bleibt auf 0 (Clamp am Anfang).
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Prev,
        })
        .unwrap();
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![(1, Duration::from_millis(500))]
    );
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Prev,
        })
        .unwrap();
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Prev,
        })
        .unwrap();
    assert_eq!(
        slide_changes(&drain(&rx)),
        vec![
            (0, Duration::from_millis(500)),
            (0, Duration::from_millis(500))
        ]
    );
}

#[test]
fn slide_nav_on_non_slideshow_cue_errors() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::Play {
            cue_id: id,
            path: None,
            duration: Some(Duration::from_secs(5)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: kuhplay::core::LoopMode::Once,
            slices: Vec::new(),
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        })
        .unwrap();
    engine.advance_time(Duration::ZERO); // Started
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Next,
        })
        .unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(eid), message }
                if *eid == id && message.contains("not a running slideshow")
        )),
        "nav on a media cue must produce an Error event: {events:?}"
    );
}

#[test]
fn slide_nav_on_unknown_cue_errors() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::SlideNav {
            cue_id: id,
            nav: SlideNav::Next,
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Error { cue_id: Some(eid), .. } if *eid == id)));
}

// ---------------------------------------------------------------------------
// Domain: Control-Cues NextSlide / PrevSlide
// ---------------------------------------------------------------------------

#[test]
fn next_and_prev_slide_control_cues_send_slide_nav_commands() {
    let mut list = CueList::new();
    let slideshow = Cue::new("2", "photos", CueType::SlideShow)
        .with_slideshow(show(vec![slide("a", 1000)], false));
    let show_id = slideshow.id;
    list.add(
        Cue::new("1", "next", CueType::Control)
            .with_control(ControlAction::NextSlide)
            .with_control_target("2"),
    );
    list.add(slideshow);
    list.add(
        Cue::new("3", "prev", CueType::Control)
            .with_control(ControlAction::PrevSlide)
            .with_control_target("2"),
    );

    let outcome = list.go().unwrap(); // Control "1": NextSlide
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "control cue is instant"
    );
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::SlideNav {
            cue_id: show_id,
            nav: SlideNav::Next
        }]
    );

    let _ = list.go().unwrap(); // Diashow startet
    let outcome = list.go().unwrap(); // Control "3": PrevSlide
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::SlideNav {
            cue_id: show_id,
            nav: SlideNav::Prev
        }]
    );
}

#[test]
fn slide_control_cue_with_unknown_target_errors() {
    let mut list = CueList::new();
    list.add(
        Cue::new("1", "next nowhere", CueType::Control)
            .with_control(ControlAction::NextSlide)
            .with_control_target("99"),
    );
    let outcome = list.go().unwrap();
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

// ---------------------------------------------------------------------------
// Resilienz: fehlende Bilddateien
// ---------------------------------------------------------------------------

#[test]
fn slideshow_with_missing_image_errors() {
    let (tx, rx) = unbounded();
    let (engine, _clock) = TestEngine::with_virtual_clock(tx);
    let mut engine = engine.with_path_validation();
    let id = Uuid::new_v4();
    let mut definition = show(vec![slide("a", 1000)], true);
    definition
        .slides
        .push(Slide::new("/nonexistent/photo.jpg", Duration::from_secs(2)).unwrap());
    engine.handle(play_show_cmd(id, definition)).unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(eid), message }
                if *eid == id && message.contains("media file not found")
        )),
        "missing image must produce an Error event (no panic): {events:?}"
    );
    assert_eq!(engine.active_playback_count(), 0);
}
