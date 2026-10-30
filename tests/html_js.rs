//! Tests für HTML5-Cues und die JavaScript-Bridge.
//!
//! Deckt ab: Registrierung/Validierung von HTML5-Cues im Domain-Modell,
//! `PlayHtml`/`EvalJs` in der MockMediaEngine, **exakte Trigger-Zeitpunkte**
//! der JS-Befehle (VirtualClock), Fehlerfälle (fehlende Instanz, fehlende
//! Datei, leere Scripts, ungültige Quellen) und Concurrency (JS-Trigger-Sturm
//! ohne Message-Drops).

use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, HtmlSource};
use kuhplay::engine::{
    spawn_mock_engine, EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock,
};

type TestEngine = MockMediaEngine<VirtualClock>;

fn html_cue(number: &str, source: HtmlSource) -> Cue {
    Cue::new(number, number, CueType::Html).with_html_source(source)
}

fn play_html_cmd(cue_id: Uuid, source: HtmlSource) -> EngineCommand {
    EngineCommand::PlayHtml {
        cue_id,
        source,
        stage: Some("Stage 1".to_string()),
        layer: 1,
        duration: Some(Duration::from_secs(30)),
        pre_wait: Duration::ZERO,
        post_wait: Duration::ZERO,
    }
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

/// Startet einen HTML5-Cue und verifiziert Started + HtmlLoaded (t = 0).
fn start_html(
    engine: &mut TestEngine,
    rx: &Receiver<EngineEvent>,
    cue_id: Uuid,
    source: HtmlSource,
) {
    engine
        .handle(play_html_cmd(cue_id, source))
        .expect("engine must accept PlayHtml");
    engine.advance_time(Duration::ZERO); // Tick bei t=0 → Started + HtmlLoaded
    let events = drain(rx);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Started { cue_id: id, .. } if *id == cue_id)),
        "engine must report Started: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::HtmlLoaded { cue_id: id } if *id == cue_id)),
        "engine must report HtmlLoaded: {events:?}"
    );
    assert_eq!(engine.html_instance_count(), 1);
}

// ---------------------------------------------------------------------------
// Domain: HTML5-Cue-Registrierung und Command-Erzeugung
// ---------------------------------------------------------------------------

#[test]
fn html_cue_go_builds_play_html_command() {
    let mut list = CueList::new();
    list.add(
        html_cue("1", HtmlSource::url("https://example.com/lower-third"))
            .with_stage("Stage 1")
            .with_layer(2)
            .with_duration(Duration::from_secs(8)),
    );

    let outcome = list.go().expect("html cue must trigger");
    assert_eq!(list.cues()[0].status, CueStatus::Playing);
    assert_eq!(outcome.commands.len(), 1);
    match &outcome.commands[0] {
        EngineCommand::PlayHtml {
            cue_id,
            source,
            stage,
            layer,
            duration,
            pre_wait,
            post_wait,
        } => {
            assert_eq!(*cue_id, list.cues()[0].id);
            assert_eq!(*source, HtmlSource::url("https://example.com/lower-third"));
            assert_eq!(stage.as_deref(), Some("Stage 1"));
            assert_eq!(*layer, 2);
            assert_eq!(*duration, Some(Duration::from_secs(8)));
            assert_eq!(*pre_wait, Duration::ZERO);
            assert_eq!(*post_wait, Duration::ZERO);
        }
        other => panic!("expected PlayHtml, got {other:?}"),
    }
}

#[test]
fn html_cue_with_pre_wait_enters_pre_wait_state() {
    let mut list = CueList::new();
    list.add(
        html_cue("1", HtmlSource::inline("<h1>hi</h1>")).with_pre_wait(Duration::from_millis(500)),
    );
    let outcome = list.go().unwrap();
    assert!(matches!(
        outcome.commands[0],
        EngineCommand::PlayHtml { .. }
    ));
    assert_eq!(list.cues()[0].status, CueStatus::PreWait);
}

#[test]
fn html_cue_without_source_errors_on_go() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "broken html", CueType::Html));
    let outcome = list.go().expect("cue triggers (into error)");
    assert!(outcome.commands.is_empty(), "no command for invalid cue");
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Error,
        "missing source must put the cue into Error (no panic)"
    );
}

#[test]
fn html_cue_with_invalid_source_errors_on_go() {
    let mut list = CueList::new();
    list.add(html_cue("1", HtmlSource::url("ftp://not-supported")));
    let outcome = list.go().unwrap();
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn html_cue_serde_roundtrip_preserves_source_and_test_script() {
    let cue = html_cue("1", HtmlSource::inline("<p>lower third</p>"))
        .with_js_test_script("window.ping()")
        .with_stage("Stage 1")
        .with_layer(3);
    let json = serde_json::to_string(&cue).expect("serialize");
    let back: Cue = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.cue_type, CueType::Html);
    assert_eq!(
        back.html_source,
        Some(HtmlSource::inline("<p>lower third</p>"))
    );
    assert_eq!(back.js_test_script, "window.ping()");
    assert_eq!(back.stage.as_deref(), Some("Stage 1"));
    assert_eq!(back.layer, 3);
}

// ---------------------------------------------------------------------------
// Engine: WebView-Instanzen und JS-Bridge
// ---------------------------------------------------------------------------

#[test]
fn html_instance_activates_on_start_and_deactivates_on_stop() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();

    assert_eq!(engine.html_instance_count(), 0);
    start_html(&mut engine, &rx, id, HtmlSource::inline("<h1>x</h1>"));
    assert_eq!(engine.html_instance_count(), 1);
    assert_eq!(
        engine.html_source(id),
        Some(&HtmlSource::inline("<h1>x</h1>"))
    );

    engine.handle(EngineCommand::Stop { cue_id: id }).unwrap();
    assert_eq!(
        engine.html_instance_count(),
        0,
        "stop must drop the webview instance"
    );
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Stopped { cue_id } if *cue_id == id)));
}

#[test]
fn eval_js_executes_in_active_instance_with_exact_timestamp() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    start_html(&mut engine, &rx, id, HtmlSource::inline("<h1>x</h1>"));

    // Exakt 1500 ms nach Start feuert der JS-Trigger.
    engine.advance_time(Duration::from_millis(1500));
    engine
        .handle(EngineCommand::EvalJs {
            cue_id: id,
            script: "triggerAnimation({speed: 2})".to_string(),
        })
        .unwrap();

    let events = drain(&rx);
    let js = events
        .iter()
        .find_map(|e| match e {
            EngineEvent::JsExecuted { cue_id, script, at } if *cue_id == id => {
                Some((script.clone(), *at))
            }
            _ => None,
        })
        .expect("JsExecuted event expected");
    assert_eq!(js.0, "triggerAnimation({speed: 2})");
    assert_eq!(
        js.1,
        Duration::from_millis(1500),
        "JS trigger must arrive at the exact engine timestamp"
    );
    // Instanz-Protokoll bestätigt Zeitpunkt und Script.
    assert_eq!(
        engine.html_js_log(id),
        Some(
            &[(
                Duration::from_millis(1500),
                "triggerAnimation({speed: 2})".to_string()
            )][..]
        )
    );
}

#[test]
fn eval_js_during_pre_wait_errors_until_instance_started() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id: id,
            source: HtmlSource::inline("<h1>x</h1>"),
            stage: None,
            layer: 0,
            duration: None,
            pre_wait: Duration::from_secs(1),
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // Noch in Pre-Wait → keine aktive Instanz → Error-Event (kein Panic).
    engine
        .handle(EngineCommand::EvalJs {
            cue_id: id,
            script: "tooEarly()".to_string(),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(eid), .. } if *eid == id
        )),
        "eval_js before start must produce an Error event: {events:?}"
    );

    // Nach Ablauf des Pre-Wait ist die Instanz aktiv.
    engine.advance_time(Duration::from_secs(1));
    engine
        .handle(EngineCommand::EvalJs {
            cue_id: id,
            script: "onTime()".to_string(),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::JsExecuted { cue_id, .. } if *cue_id == id)));
}

#[test]
fn eval_js_without_instance_errors_gracefully() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::EvalJs {
            cue_id: id,
            script: "nobody()".to_string(),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events.iter().any(|e| matches!(
        e,
        EngineEvent::Error { cue_id: Some(eid), message }
            if *eid == id && message.contains("no active HTML5 instance")
    )));
}

#[test]
fn eval_js_with_empty_script_errors() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    start_html(&mut engine, &rx, id, HtmlSource::inline("<h1>x</h1>"));
    engine
        .handle(EngineCommand::EvalJs {
            cue_id: id,
            script: "   ".to_string(),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Error { cue_id: Some(eid), .. } if *eid == id)));
    assert!(engine.html_js_log(id).unwrap().is_empty());
}

#[test]
fn play_html_with_missing_file_errors() {
    let (tx, rx) = unbounded();
    let (engine, _clock) = TestEngine::with_virtual_clock(tx);
    let mut engine = engine.with_path_validation();
    let id = Uuid::new_v4();
    engine
        .handle(play_html_cmd(
            id,
            HtmlSource::file("/nonexistent/lower-third.html"),
        ))
        .unwrap();
    let events = drain(&rx);
    assert!(
        events.iter().any(|e| matches!(
            e,
            EngineEvent::Error { cue_id: Some(eid), message }
                if *eid == id && message.contains("media file not found")
        )),
        "missing html file must produce an Error event: {events:?}"
    );
    assert_eq!(engine.active_playback_count(), 0);
    assert_eq!(engine.html_instance_count(), 0);
}

#[test]
fn play_html_with_invalid_source_errors() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(play_html_cmd(id, HtmlSource::url("not-a-url")))
        .unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Error { cue_id: Some(eid), .. } if *eid == id)));
    assert_eq!(engine.active_playback_count(), 0);
}

#[test]
fn html_cue_finishes_after_duration_and_drops_instance() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let id = Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id: id,
            source: HtmlSource::inline("<h1>x</h1>"),
            stage: None,
            layer: 0,
            duration: Some(Duration::from_millis(500)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();
    engine.advance_time(Duration::ZERO); // Started + HtmlLoaded
    assert_eq!(engine.html_instance_count(), 1);
    engine.advance_time(Duration::from_millis(500)); // Finished + PostWaitElapsed
    assert_eq!(
        engine.html_instance_count(),
        0,
        "finished cue must drop the webview"
    );
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)));
}

// ---------------------------------------------------------------------------
// Concurrency: JS-Trigger-Sturm ohne Message-Drops
// ---------------------------------------------------------------------------

#[test]
fn concurrent_eval_js_triggers_arrive_without_drops() {
    let engine = spawn_mock_engine();
    let id = Uuid::new_v4();
    engine
        .send(play_html_cmd(id, HtmlSource::inline("<h1>x</h1>")))
        .unwrap();

    // Warten, bis die WebView-Instanz aktiv ist (HtmlLoaded).
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut loaded = false;
    while Instant::now() < deadline && !loaded {
        match engine.try_recv_event() {
            Some(EngineEvent::HtmlLoaded { cue_id }) if cue_id == id => loaded = true,
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert!(loaded, "webview instance must become active");

    // 4 Threads × 250 JS-Trigger gleichzeitig.
    std::thread::scope(|s| {
        for t in 0..4 {
            let engine = &engine;
            s.spawn(move || {
                for i in 0..250 {
                    engine
                        .send(EngineCommand::EvalJs {
                            cue_id: id,
                            script: format!("trigger({t},{i})"),
                        })
                        .expect("send must succeed");
                }
            });
        }
    });

    // Alle 1000 Trigger müssen ankommen (keine Drops, kein Deadlock).
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut executed = 0;
    while Instant::now() < deadline && executed < 1000 {
        match engine.try_recv_event() {
            Some(EngineEvent::JsExecuted { cue_id, .. }) => {
                assert_eq!(cue_id, id);
                executed += 1;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert_eq!(
        executed, 1000,
        "all 1000 JS triggers must be executed exactly once"
    );
}
