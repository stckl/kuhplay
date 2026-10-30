//! Tests für HTML-Data-Cues (`HtmlDataCue`) und die JSON-Bridge.
//!
//! Deckt ab: Validierung/Parsing von JSON-Payloads im Domain-Modell
//! (inkl. Fehlerfälle ohne Panic), `PushJson`-Command-Erzeugung beim GO,
//! **exakte Zustell-Zeitpunkte** der JSON-Payloads in der Ziel-WebView
//! (VirtualClock), Targeting-Fehler (inaktive/unbekannte Instanz) und
//! Concurrency (JSON-Push-Sturm ohne Message-Drops).

use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver};
use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, HtmlSource};
use kuhplay::engine::{
    spawn_mock_engine, EngineCommand, EngineEvent, MediaEngine, MockMediaEngine, VirtualClock,
};

type TestEngine = MockMediaEngine<VirtualClock>;

fn data_cue(number: &str, target: Uuid, json: &str) -> Cue {
    Cue::new(number, number, CueType::HtmlData)
        .with_data_target(target)
        .with_data_json(json)
}

fn drain(rx: &Receiver<EngineEvent>) -> Vec<EngineEvent> {
    rx.try_iter().collect()
}

/// Startet einen HTML5-Cue in der Engine (t=0: Started + HtmlLoaded).
fn start_html(engine: &mut TestEngine, rx: &Receiver<EngineEvent>, cue_id: Uuid) {
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::inline("<h1>scoreboard</h1>"),
            stage: Some("Stage 1".to_string()),
            layer: 2,
            duration: Some(Duration::from_secs(30)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .expect("engine must accept PlayHtml");
    engine.advance_time(Duration::ZERO);
    let events = drain(rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::HtmlLoaded { cue_id: id } if *id == cue_id)));
}

// ---------------------------------------------------------------------------
// Domain: JSON-Validierung und Command-Erzeugung
// ---------------------------------------------------------------------------

#[test]
fn data_payload_parses_arbitrary_json() {
    let target = Uuid::new_v4();
    let cases: [(&str, serde_json::Value); 4] = [
        (
            r#"{"score":{"home":2,"guest":1}}"#,
            serde_json::json!({"score": {"home": 2, "guest": 1}}),
        ),
        (r#"[1,2,3]"#, serde_json::json!([1, 2, 3])),
        (
            r#""lower third text""#,
            serde_json::json!("lower third text"),
        ),
        ("42", serde_json::json!(42)),
    ];
    for (text, expected) in cases {
        let cue = data_cue("1", target, text);
        assert_eq!(
            cue.data_payload().expect("valid json"),
            expected,
            "payload: {text}"
        );
    }
}

#[test]
fn data_payload_rejects_invalid_or_empty_json_without_panic() {
    let target = Uuid::new_v4();
    for bad in ["{not json", "", "   ", "{\"unclosed\": "] {
        let cue = data_cue("1", target, bad);
        assert!(
            cue.data_payload().is_err(),
            "payload must be rejected: {bad:?}"
        );
    }
}

#[test]
fn html_data_cue_go_builds_push_json_command_and_completes_instantly() {
    let mut list = CueList::new();
    let target = Uuid::new_v4();
    list.add(data_cue("1", target, r#"{"clock":"12:34","running":true}"#));

    let outcome = list.go().expect("data cue must trigger");
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "data cue is an instant cue"
    );
    assert_eq!(
        outcome.commands,
        vec![EngineCommand::PushJson {
            target,
            payload: serde_json::json!({"clock": "12:34", "running": true}),
        }]
    );
}

#[test]
fn html_data_cue_without_target_errors_on_go() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "no target", CueType::HtmlData).with_data_json("{}"));
    let outcome = list.go().expect("cue triggers (into error)");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn html_data_cue_with_invalid_json_errors_on_go_without_panic() {
    let mut list = CueList::new();
    list.add(data_cue("1", Uuid::new_v4(), "{kaputt"));
    let outcome = list.go().expect("cue triggers (into error)");
    assert!(
        outcome.commands.is_empty(),
        "invalid JSON must not produce a command"
    );
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Error,
        "invalid JSON → controlled Error state (Live-Betrieb: kein Panic)"
    );
}

#[test]
fn html_data_cue_with_empty_payload_errors_on_go() {
    let mut list = CueList::new();
    list.add(data_cue("1", Uuid::new_v4(), "  "));
    let outcome = list.go().unwrap();
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn html_data_cue_serde_roundtrip_preserves_target_and_payload_text() {
    let target = Uuid::new_v4();
    let cue = data_cue("1", target, r#"{"a":1}"#);
    let json = serde_json::to_string(&cue).expect("serialize");
    let back: Cue = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.cue_type, CueType::HtmlData);
    assert_eq!(back.data_target, Some(target));
    assert_eq!(back.data_json_text, r#"{"a":1}"#);
    assert_eq!(back.data_payload().unwrap(), serde_json::json!({"a": 1}));
}

#[test]
fn data_cue_can_target_html_cue_started_earlier_in_the_show() {
    // Show-Ablauf: HTML5-Cue "1" starten, dann Data-Cue "2" pusht JSON an "1".
    let mut list = CueList::new();
    let html = Cue::new("1", "lower third", CueType::Html)
        .with_html_source(HtmlSource::inline("<div id='data'></div>"));
    let html_id = html.id;
    list.add(html);
    list.add(data_cue("2", html_id, r#"{"name":"Ada"}"#));

    let first = list.go().unwrap();
    assert!(matches!(first.commands[0], EngineCommand::PlayHtml { .. }));
    let second = list.go().unwrap();
    assert_eq!(
        second.commands,
        vec![EngineCommand::PushJson {
            target: html_id,
            payload: serde_json::json!({"name": "Ada"}),
        }]
    );
}

// ---------------------------------------------------------------------------
// Engine: JSON-Push an die Ziel-WebView mit exaktem Zeitpunkt
// ---------------------------------------------------------------------------

#[test]
fn push_json_reaches_target_instance_with_exact_timestamp() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let target = Uuid::new_v4();
    start_html(&mut engine, &rx, target);

    // Exakt 2500 ms nach Start wird der Payload zugestellt.
    engine.advance_time(Duration::from_millis(2500));
    let payload = serde_json::json!({"score": {"home": 3, "guest": 1}, "clock": "45:12"});
    engine
        .handle(EngineCommand::PushJson {
            target,
            payload: payload.clone(),
        })
        .unwrap();

    let events = drain(&rx);
    let received = events
        .iter()
        .find_map(|e| match e {
            EngineEvent::JsonReceived {
                target: t,
                payload,
                at,
            } if *t == target => Some((payload.clone(), *at)),
            _ => None,
        })
        .expect("JsonReceived event expected");
    assert_eq!(received.0, payload, "Payload muss unverändert ankommen");
    assert_eq!(
        received.1,
        Duration::from_millis(2500),
        "JSON-Push muss exakt zum definierten Zeitpunkt ankommen"
    );
    // Instanz-Protokoll bestätigt Zeitpunkt und Payload.
    assert_eq!(
        engine.html_json_log(target),
        Some(&[(Duration::from_millis(2500), payload)][..])
    );
}

#[test]
fn push_json_to_unknown_or_inactive_target_errors() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let target = Uuid::new_v4();

    // Nie gestartete Instanz → Error.
    engine
        .handle(EngineCommand::PushJson {
            target,
            payload: serde_json::json!({}),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events.iter().any(|e| matches!(
        e,
        EngineEvent::Error { cue_id: Some(id), message }
            if *id == target && message.contains("no active HTML5 instance")
    )));

    // Gestartet, aber wieder gestoppt → ebenfalls Error (kein Panic).
    start_html(&mut engine, &rx, target);
    engine
        .handle(EngineCommand::Stop { cue_id: target })
        .unwrap();
    drain(&rx);
    engine
        .handle(EngineCommand::PushJson {
            target,
            payload: serde_json::json!({"x": 1}),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::Error { cue_id: Some(id), .. } if *id == target)));
}

#[test]
fn json_payload_survives_command_event_roundtrip_nested() {
    let (tx, rx) = unbounded();
    let (mut engine, _clock) = TestEngine::with_virtual_clock(tx);
    let target = Uuid::new_v4();
    start_html(&mut engine, &rx, target);

    let payload = serde_json::json!({
        "lowerThird": {
            "name": "Ada Lovelace",
            "role": "Keynote Speaker",
            "tags": ["VIP", "Speaker"],
            "meta": {"updated": 1719234567, "visible": true, "opacity": 0.95}
        }
    });
    engine
        .handle(EngineCommand::PushJson {
            target,
            payload: payload.clone(),
        })
        .unwrap();
    let events = drain(&rx);
    assert!(events.iter().any(|e| matches!(
        e,
        EngineEvent::JsonReceived { target: t, payload: p, .. }
            if *t == target && *p == payload
    )));
}

// ---------------------------------------------------------------------------
// Concurrency: JSON-Push-Sturm ohne Message-Drops
// ---------------------------------------------------------------------------

#[test]
fn concurrent_push_json_arrives_without_drops() {
    let engine = spawn_mock_engine();
    let target = Uuid::new_v4();
    engine
        .send(EngineCommand::PlayHtml {
            cue_id: target,
            source: HtmlSource::inline("<div id='data'></div>"),
            stage: None,
            layer: 0,
            duration: Some(Duration::from_secs(30)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // Warten, bis die Ziel-Instanz aktiv ist.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut loaded = false;
    while Instant::now() < deadline && !loaded {
        match engine.try_recv_event() {
            Some(EngineEvent::HtmlLoaded { cue_id }) if cue_id == target => loaded = true,
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert!(loaded, "target webview instance must become active");

    // 4 Threads × 250 JSON-Pushes gleichzeitig.
    std::thread::scope(|s| {
        for t in 0..4 {
            let engine = &engine;
            s.spawn(move || {
                for i in 0..250 {
                    engine
                        .send(EngineCommand::PushJson {
                            target,
                            payload: serde_json::json!({"thread": t, "seq": i}),
                        })
                        .expect("send must succeed");
                }
            });
        }
    });

    // Alle 1000 Pushes müssen ankommen (keine Drops, kein Deadlock).
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut received = 0;
    while Instant::now() < deadline && received < 1000 {
        match engine.try_recv_event() {
            Some(EngineEvent::JsonReceived {
                target: t, payload, ..
            }) => {
                assert_eq!(t, target);
                assert!(payload.get("seq").is_some());
                received += 1;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    assert_eq!(
        received, 1000,
        "all 1000 JSON payloads must be delivered exactly once"
    );
}
