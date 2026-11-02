//! Integrationstests für das WebView-Backend (`src/engine/webview_backend.rs`)
//! und dessen Engine-Integration (RealMediaEngine):
//!
//! * JSON-lines IPC gegen einen **Fake-WebView** (Shell-Script statt
//!   wry-Binary – headless testbar)
//! * `KUHPLAY_WEBVIEW_BIN` überschreibt die Binary-Auflösung
//! * Spawn-Fehler bei nicht existierendem Binary
//! * Engine: PlayHtml nutzt das echte Backend (WebView-Error wird als
//!   EngineEvent::Error durchgereicht), EvalJs/PushJson bestätigt
//! * Engine: Fallback auf virtuellen Modus, wenn das Binary fehlt

// Die Shell-Script-Tests sind `#[cfg(unix)]`; auf Windows bleiben Imports/Helper
// ungenutzt – daher das crate-level allow für nicht-unix.
#![cfg_attr(not(unix), allow(unused_imports, dead_code))]

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use kuhplay::core::{HtmlSource, Stage, StageGeometry};
use kuhplay::engine::webview_backend::{webview_binary, WebviewEvent, WebviewInstance};
use kuhplay::engine::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
    EngineCommand, EngineEvent, FrameStore, MediaEngine, RealMediaEngine, StageFrameStore,
    VideoFrame,
};

/// 2×2-RGBA-Testframe: rot, grün, blau, weiß (Row-Major).
#[cfg(unix)]
const FRAME_RGBA: [u8; 16] = [
    255, 0, 0, 255, // rot
    0, 255, 0, 255, // grün
    0, 0, 255, 255, // blau
    255, 255, 255, 255, // weiß
];

/// Schreibt einen Fake-WebView (Shell-Script): beantwortet `load` mit
/// `{"loaded":true}` + `{"error":"probe-loaded"}` (unterscheidet echtes
/// Backend vom virtuellen Fallback), `eval`/`push_json` mit Bestätigungen.
/// Mit `frame_b64` beantwortet es `snapshot`-Requests mit einem 2×2-Frame,
/// mit `snapshot_error` stattdessen mit einem Snapshot-Fehler (für den
/// Backoff-Test; loggt alle Requests nach `requests.log`).
#[cfg(unix)]
fn write_fake_webview(
    dir: &std::path::Path,
    frame_b64: Option<&str>,
    snapshot_error: bool,
) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-webview.sh");
    let snapshot_case = match (frame_b64, snapshot_error) {
        (Some(b64), _) => format!(
            "    *'\"snapshot\"'*) printf '%s\\n' '{{\"frame\":{{\"width\":2,\"height\":2,\"rgba_b64\":\"{b64}\"}}}}' ;;\n"
        ),
        (None, true) => String::from(
            "    *'\"snapshot\"'*) printf '%s\\n' '{\"error\":\"snapshot: no canvas element\"}' ;;\n",
        ),
        (None, false) => String::new(),
    };
    let script = format!(
        r#"#!/bin/sh
while IFS= read -r line; do
  echo "$line" >> "{log}"
  case "$line" in
    *'"load"'*) printf '%s\n' '{{"loaded":true}}' '{{"error":"probe-loaded"}}' ;;
    *'"eval"'*) printf '%s\n' '{{"js_executed":true}}' ;;
    *'"push_json"'*) printf '%s\n' '{{"json_received":true}}' ;;
{snapshot_case}    *'"close"'*) exit 0 ;;
  esac
done
"#,
        log = dir.join("requests.log").display()
    );
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Wartet, bis `rx` ein Event liefert, das `pred` erfüllt (max. 2 s).
#[cfg(unix)]
fn wait_for_event(inst: &WebviewInstance, pred: impl Fn(&WebviewEvent) -> bool) -> WebviewEvent {
    for _ in 0..100 {
        while let Some(event) = inst.poll_event() {
            if pred(&event) {
                return event;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("kein passendes WebviewEvent innerhalb von 2 s");
}

#[cfg(unix)]
#[test]
fn webview_binary_env_override_and_ipc_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_fake_webview(dir.path(), None, false);

    // Env-Override: KUHPLAY_WEBVIEW_BIN gewinnt vor der Sibling-Auflösung.
    std::env::set_var("KUHPLAY_WEBVIEW_BIN", &script);
    assert_eq!(webview_binary().as_deref(), Some(script.as_path()));

    // IPC-Roundtrip über das aufgelöste Binary.
    let cue_id = uuid::Uuid::new_v4();
    let mut inst =
        WebviewInstance::spawn(&script, cue_id, &HtmlSource::Inline("<h1>hi</h1>".into())).unwrap();
    assert_eq!(
        wait_for_event(&inst, |e| *e == WebviewEvent::Loaded),
        WebviewEvent::Loaded
    );
    // Das Fake meldet zusätzlich einen Fehler für den Load (Probe).
    assert_eq!(
        wait_for_event(&inst, |e| matches!(e, WebviewEvent::Error(_))),
        WebviewEvent::Error("probe-loaded".to_string())
    );

    inst.eval_js("1+1").unwrap();
    assert_eq!(
        wait_for_event(&inst, |e| *e == WebviewEvent::JsExecuted),
        WebviewEvent::JsExecuted
    );

    inst.push_json(&serde_json::json!({ "cue": "1.1" }))
        .unwrap();
    assert_eq!(
        wait_for_event(&inst, |e| *e == WebviewEvent::JsonReceived),
        WebviewEvent::JsonReceived
    );

    inst.close();
    std::env::remove_var("KUHPLAY_WEBVIEW_BIN");
}

#[test]
fn webview_spawn_failure_bei_fehlendem_binary() {
    let result = WebviewInstance::spawn(
        PathBuf::from("/nonexistent/kuhplay-webview").as_path(),
        uuid::Uuid::new_v4(),
        &HtmlSource::Inline("<h1>hi</h1>".into()),
    );
    assert!(result.is_err());
}

/// Engine-Events sammeln, bis `pred` erfüllt ist (max. 2 s, mit Ticks).
#[cfg(unix)]
fn collect_until(
    engine: &mut RealMediaEngine,
    rx: &crossbeam_channel::Receiver<EngineEvent>,
    pred: impl Fn(&EngineEvent) -> bool,
) -> Vec<EngineEvent> {
    let mut collected = Vec::new();
    for _ in 0..100 {
        engine.tick();
        while let Ok(event) = rx.try_recv() {
            let hit = pred(&event);
            collected.push(event);
            if hit {
                return collected;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    collected
}

#[cfg(unix)]
#[test]
fn engine_nutzt_echtes_webview_backend() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_fake_webview(dir.path(), None, false);

    let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(script);

    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::Inline("<h1>hi</h1>".into()),
            stage: None,
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // Tick aktiviert die Instanz → HtmlLoaded.
    let events = collect_until(&mut engine, &evt_rx, |e| {
        matches!(e, EngineEvent::HtmlLoaded { .. })
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::HtmlLoaded { .. })),
        "HtmlLoaded erwartet, got: {events:?}"
    );

    // Das Fake-WebView meldet seinen Probe-Fehler → Engine reicht ihn durch.
    let events = collect_until(
        &mut engine,
        &evt_rx,
        |e| matches!(e, EngineEvent::Error { message, .. } if message == "probe-loaded"),
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::Error { message, .. } if message == "probe-loaded")),
        "WebView-Error erwartet, got: {events:?}"
    );

    // EvalJs / PushJson werden bestätigt (und an den Subprocess geleitet).
    engine
        .handle(EngineCommand::EvalJs {
            cue_id,
            script: "1+1".to_string(),
        })
        .unwrap();
    engine
        .handle(EngineCommand::PushJson {
            target: cue_id,
            payload: serde_json::json!({ "cue": "1.1" }),
        })
        .unwrap();
    let events = collect_until(&mut engine, &evt_rx, |e| {
        matches!(e, EngineEvent::JsonReceived { .. })
    });
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::JsExecuted { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::JsonReceived { .. })));
}

#[cfg(unix)]
#[test]
fn webview_ipc_snapshot_request_and_frame_event() {
    use base64::engine::general_purpose::STANDARD;

    let dir = tempfile::tempdir().unwrap();
    let b64 = STANDARD.encode(FRAME_RGBA);
    let script = write_fake_webview(dir.path(), Some(&b64), false);

    let cue_id = uuid::Uuid::new_v4();
    let mut inst =
        WebviewInstance::spawn(&script, cue_id, &HtmlSource::Inline("<h1>hi</h1>".into())).unwrap();
    // Snapshot anfordern (Wunschgröße 64×32 – das Fake antwortet 2×2).
    inst.request_snapshot(64, 32).unwrap();
    let event = wait_for_event(&inst, |e| matches!(e, WebviewEvent::Frame { .. }));
    assert_eq!(
        event,
        WebviewEvent::Frame {
            width: 2,
            height: 2,
            rgba: FRAME_RGBA.to_vec(),
        }
    );
    inst.close();
}

/// Wartet, bis Frame-Store und Stage-Canvas des Cues vorliegen (max. 2 s).
#[cfg(unix)]
fn wait_for_frame_and_stage(
    engine: &mut RealMediaEngine,
    frames: &FrameStore,
    stage_frames: &StageFrameStore,
    cue_id: uuid::Uuid,
    stage: &str,
) -> (VideoFrame, VideoFrame) {
    for _ in 0..100 {
        engine.tick();
        let frame = frames.lock().unwrap().get(&cue_id).cloned();
        let stage_frame = stage_frames.lock().unwrap().get(stage).cloned();
        if let (Some(f), Some(s)) = (frame, stage_frame) {
            return (f.as_ref().clone(), s.as_ref().clone());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("kein WebView-Frame/Stage-Canvas innerhalb von 2 s");
}

#[cfg(unix)]
#[test]
fn engine_webview_frame_landet_im_frame_store_and_composites() {
    use base64::engine::general_purpose::STANDARD;

    let dir = tempfile::tempdir().unwrap();
    let b64 = STANDARD.encode(FRAME_RGBA);
    let script = write_fake_webview(dir.path(), Some(&b64), false);

    let frames = new_frame_store();
    let stage_frames = new_stage_frame_store();
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        frames.clone(),
        new_opacity_store(),
        stage_frames.clone(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(script);

    // Stage konfigurieren, dann HTML5-Cue auf die Stage spielen.
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(100.0, 100.0)),
        })
        .unwrap();
    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::Inline("<h1>hi</h1>".into()),
            stage: Some("Main".to_string()),
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    let (frame, stage_frame) =
        wait_for_frame_and_stage(&mut engine, &frames, &stage_frames, cue_id, "Main");

    // Frame-Store: exakter Snapshot (2×2, RGBA wie vom Fake geliefert).
    assert_eq!((frame.width, frame.height), (2, 2));
    assert_eq!(frame.rgba, FRAME_RGBA.to_vec());

    // Stage-Canvas: 100×100, der 2×2-Snapshot füllt die Fläche in Quadranten
    // (Compositor skaliert Nearest-Neighbor, deckend bei voller Opacity).
    assert_eq!((stage_frame.width, stage_frame.height), (100, 100));
    let px = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * 100 + x) * 4) as usize;
        stage_frame.rgba[i..i + 4].try_into().unwrap()
    };
    assert_eq!(px(0, 0), [255, 0, 0, 255], "oben links: rot");
    assert_eq!(px(99, 0), [0, 255, 0, 255], "oben rechts: grün");
    assert_eq!(px(0, 99), [0, 0, 255, 255], "unten links: blau");
    assert_eq!(px(99, 99), [255, 255, 255, 255], "unten rechts: weiß");
}

#[cfg(unix)]
#[test]
fn engine_requests_only_one_snapshot_until_the_answer_arrives() {
    let dir = tempfile::tempdir().unwrap();
    // Dieses Fake beantwortet Snapshots nie.
    let script = write_fake_webview(dir.path(), None, false);
    let (evt_tx, _evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(script);
    engine
        .handle(EngineCommand::ConfigureStage {
            stage: Stage::new("Main", StageGeometry::new(100.0, 100.0)),
        })
        .unwrap();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id: uuid::Uuid::new_v4(),
            source: HtmlSource::Inline("<h1>hi</h1>".into()),
            stage: Some("Main".to_string()),
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();
    for _ in 0..40 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(20));
    }
    let log = std::fs::read_to_string(dir.path().join("requests.log")).unwrap();
    assert_eq!(log.matches("\"snapshot\"").count(), 1, "{log}");
}

#[cfg(unix)]
#[test]
fn engine_webview_snapshot_backoff_bei_fehlern() {
    let dir = tempfile::tempdir().unwrap();
    // Fake antwortet auf snapshot mit einem Fehler (z. B. Seite ohne Canvas).
    let script = write_fake_webview(dir.path(), None, true);

    let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(script);

    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::Inline("<h1>hi</h1>".into()),
            stage: None,
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // ~1 s ticken (Zeitfenster statt fester Tickzahl, damit ausgelastete
    // CI-Runner nicht mehr Requests produzieren): ohne Backoff wären es
    // ~10 Snapshot-Requests (100 ms Raster).
    let mut snapshot_errors = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        engine.tick();
        while let Ok(event) = evt_rx.try_recv() {
            if matches!(event, EngineEvent::Error { message, .. } if message.starts_with("snapshot:"))
            {
                snapshot_errors += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    // Backoff: nach Fehlschlägen wird nur noch alle 500 ms retried (statt
    // dauerhaft zu stoppen, damit HTML nie einfriert) – in 1 s deutlich
    // weniger Requests als ohne Backoff (~30 bei 33 ms).
    let log = std::fs::read_to_string(dir.path().join("requests.log")).unwrap();
    let snapshot_requests = log.matches("\"snapshot\"").count();
    assert!(
        snapshot_requests <= 6,
        "Backoff greift nicht: {snapshot_requests} Snapshot-Requests in 1 s"
    );
    // … und der Fehler wird nur einmal als Engine-Event gemeldet.
    assert_eq!(
        snapshot_errors, 1,
        "Snapshot-Fehler darf nur einmal gemeldet werden"
    );
}

#[cfg(unix)]
#[test]
fn engine_fallback_virtuell_wenn_binary_fehlt() {
    let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(PathBuf::from("/nonexistent/kuhplay-webview"));

    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::PlayHtml {
            cue_id,
            source: HtmlSource::Inline("<h1>hi</h1>".into()),
            stage: None,
            layer: 1,
            duration: Some(Duration::from_secs(60)),
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
        })
        .unwrap();

    // Virtueller Fallback: HtmlLoaded kommt trotzdem, aber kein
    // WebView-Fehler (kein Subprocess vorhanden).
    let events = collect_until(&mut engine, &evt_rx, |e| {
        matches!(e, EngineEvent::HtmlLoaded { .. })
    });
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::HtmlLoaded { .. })));
    for _ in 0..10 {
        engine.tick();
        while let Ok(event) = evt_rx.try_recv() {
            assert!(
                !matches!(event, EngineEvent::Error { .. }),
                "virtueller Fallback darf keine WebView-Fehler melden"
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Fake-WebView, das nach `loaded` verzögert einen JS-Fehler meldet
/// (simuliert asynchrones Script-JS: setTimeout/fetch nach Page-Load).
#[cfg(unix)]
fn write_fake_webview_late_error(dir: &std::path::Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-webview-late.sh");
    let script = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"load"'*)
      printf '%s\n' '{"loaded":true}'
      ( sleep 0.3; printf '%s\n' '{"js_executed":true}' ) &
      ;;
    *'"close"'*) exit 0 ;;
  esac
done
"#;
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Review E12: Script-Cue darf nicht beim `loaded`-Event enden – sonst wird
/// asynchrones JS abgeschnitten (Instanz per Drop gekillt). Stattdessen
/// Settle-Frist; späte Fehler innerhalb der Frist werden noch gemeldet.
#[cfg(unix)]
#[test]
fn e12_script_cue_settle_faengt_spaete_js_fehler() {
    std::env::set_var("KUHPLAY_SCRIPT_SETTLE_MS", "3000");
    let dir = tempfile::tempdir().unwrap();
    let bin = write_fake_webview_late_error(dir.path());
    let (evt_tx, evt_rx) = crossbeam_channel::unbounded();
    let mut engine = RealMediaEngine::new(
        new_frame_store(),
        new_opacity_store(),
        new_stage_frame_store(),
        new_route_frame_store(),
        evt_tx,
    )
    .with_webview_bin(bin);
    let cue_id = uuid::Uuid::new_v4();
    engine
        .handle(EngineCommand::RunScript {
            cue_id,
            script: "setTimeout(() => { throw new Error('late'); }, 100)".into(),
        })
        .unwrap();

    // Phase 1: 500 ms ticken – loaded und das späte js_executed sind da,
    // Finished darf noch nicht kommen (pre-fix: Finished sofort nach loaded,
    // Instanz per Drop gekillt, js_executed käme nie an).
    let mut saw_finished_early = false;
    let mut saw_late_js = false;
    for _ in 0..25 {
        engine.tick();
        while let Ok(e) = evt_rx.try_recv() {
            match e {
                EngineEvent::Finished { cue_id: id } if id == cue_id => saw_finished_early = true,
                EngineEvent::JsExecuted { cue_id: id, .. } if id == cue_id => saw_late_js = true,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        saw_late_js,
        "spätes JS-Event innerhalb der Settle-Frist fehlt"
    );
    assert!(
        !saw_finished_early,
        "Finished direkt nach loaded – asynchrones JS würde abgeschnitten (E12)"
    );

    // Phase 2: nach Ablauf der Settle-Frist (3 s) kommt Finished.
    let mut saw_finished = false;
    for _ in 0..220 {
        engine.tick();
        while let Ok(e) = evt_rx.try_recv() {
            if matches!(e, EngineEvent::Finished { cue_id: id } if id == cue_id) {
                saw_finished = true;
            }
        }
        if saw_finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::env::remove_var("KUHPLAY_SCRIPT_SETTLE_MS");
    assert!(saw_finished, "Finished nach Ablauf der Settle-Frist fehlt");
}
