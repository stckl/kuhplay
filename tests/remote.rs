//! Integrationstests für die Remote-Steuerung:
//! * Companion TCP-Server (JSON-lines, bidirektional + Status-Cache)
//! * Web-Remote HTTP-Server (POST /api/command, GET /api/status, GET /)
//! * Web-Remote WebSocket (Handshake, Command-Frame, Event-Frame)
//! * RemoteConfig aus Umgebungsvariablen
//! * Command-Routing auf App-Ebene (ShowApp::handle_remote_command)

use std::time::Duration;

use kuhplay::core::CueStatus;
use kuhplay::engine::spawn_mock_engine;
use kuhplay::gui::ShowApp;
use kuhplay::remote::{
    start_companion_server, start_web_remote, MidiNoteMap, RemoteCommand, RemoteConfig,
    RemoteEvent, RemoteStatus,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Ephemerer Port: kurz binden, Port merken, Listener wieder droppen.
async fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Cue per Q-Nummer aus dem aktiven Cart (Test-Helfer).
fn cue_by_number(app: &ShowApp, number: &str) -> kuhplay::core::Cue {
    app.carts()
        .active()
        .cues()
        .iter()
        .find(|c| c.number == number)
        .unwrap()
        .clone()
}

/// Beispiel-Status (so baut ihn die App in `build_remote_status`).
fn sample_status() -> RemoteStatus {
    RemoteStatus::from_app(
        "Test Show",
        false,
        0,
        "Cart 1",
        2,
        None,
        vec![],
        vec![],
        vec![],
    )
}

/// Rohes HTTP/1.1-Request (mit `Connection: close`) → komplette Antwort.
async fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut buf))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

/// Sendet einen WebSocket-Text-Frame (Client→Server: zwingend gemasked).
async fn ws_send_text(stream: &mut TcpStream, text: &str) {
    let payload = text.as_bytes();
    assert!(payload.len() < 126, "Test-Helfer: nur kurze Frames");
    let mask = [0x12u8, 0x34, 0x56, 0x78];
    let mut frame = vec![0x81u8, 0x80 | payload.len() as u8]; // FIN+Text, MASK
    frame.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    stream.write_all(&frame).await.unwrap();
}

/// Liest einen WebSocket-Frame vom Server (unmasked, kurze Payloads).
async fn ws_read_frame(stream: &mut TcpStream) -> String {
    let mut header = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut header))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(header[0] & 0x0F, 0x1, "nur Text-Frames erwartet");
    assert_eq!(header[1] & 0x80, 0, "Server-Frames sind nicht gemasked");
    let len = (header[1] & 0x7F) as usize;
    assert!(len < 126, "Test-Helfer: nur kurze Frames");
    let mut payload = vec![0u8; len];
    tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut payload))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(payload).unwrap()
}

#[tokio::test]
async fn companion_tcp_bidirektional() {
    let port = free_port().await;
    let (mut cmd_rx, event_tx) = start_companion_server(port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client verbinden.
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    // Command senden → landet im Command-Kanal der App.
    writer.write_all(b"{\"action\":\"go\"}\n").await.unwrap();
    let cmd = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cmd, RemoteCommand::Go));

    // Event-Broadcast → erreicht den Client als JSON-Zeile.
    event_tx.send(RemoteEvent::Pong).unwrap();
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let event: RemoteEvent = serde_json::from_str(&line).unwrap();
    assert!(matches!(event, RemoteEvent::Pong));
}

#[tokio::test]
async fn companion_status_cache_fuer_neue_clients() {
    let port = free_port().await;
    let (_cmd_rx, event_tx) = start_companion_server(port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // App pusht Status (wird vom Server gecacht).
    event_tx.send(RemoteEvent::Status(sample_status())).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Neuer Client bekommt den gecachten Status als erste Zeile – ohne
    // dass die App erneut pushen muss.
    let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let mut lines = BufReader::new(stream).lines();
    let line = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let event: RemoteEvent = serde_json::from_str(&line).unwrap();
    match event {
        RemoteEvent::Status(status) => assert_eq!(status.show_name, "Test Show"),
        other => panic!("erwartet Status-Event, bekommen: {other:?}"),
    }
}

#[tokio::test]
async fn web_remote_http_end_to_end() {
    let port = free_port().await;
    let (mut cmd_rx, event_tx) = start_web_remote(port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // GET / → eingebettete HTML-UI.
    let resp = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 200"), "GET /: {resp}");
    assert!(resp.contains("KuhPlay Remote"));
    assert!(resp.contains("/ws"));

    // GET /api/status → 404, solange kein Status gepusht wurde.
    let resp = http_request(
        port,
        "GET /api/status HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 404"), "GET /api/status: {resp}");

    // POST /api/command → Command landet im Kanal der App.
    let body = r#"{"action":"panic"}"#;
    let req = format!(
        "POST /api/command HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let resp = http_request(port, &req).await;
    assert!(
        resp.starts_with("HTTP/1.1 200"),
        "POST /api/command: {resp}"
    );
    assert!(resp.contains("\"ok\":true"));
    let cmd = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cmd, RemoteCommand::Panic));

    // Ungültiges JSON → 4xx, kein Command im Kanal.
    let body = "not json";
    let req = format!(
        "POST /api/command HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let resp = http_request(port, &req).await;
    assert!(resp.starts_with("HTTP/1.1 4"), "ungültiges JSON: {resp}");

    // Status-Broadcast → /api/status liefert ihn (Status-Cacher).
    event_tx.send(RemoteEvent::Status(sample_status())).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let resp = http_request(
        port,
        "GET /api/status HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(resp.starts_with("HTTP/1.1 200"), "GET /api/status: {resp}");
    assert!(resp.contains("Test Show"));
}

#[tokio::test]
async fn web_remote_websocket_end_to_end() {
    let port = free_port().await;
    let (mut cmd_rx, event_tx) = start_web_remote(port);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .unwrap();

    // WebSocket-Handshake (HTTP Upgrade).
    let req = format!(
        "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: close, Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut byte))
            .await
            .unwrap()
            .unwrap();
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&head);
    assert!(head.starts_with("HTTP/1.1 101"), "WS-Handshake: {head}");

    // Command als Text-Frame → landet im Command-Kanal der App.
    ws_send_text(&mut stream, r#"{"action":"panic"}"#).await;
    let cmd = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(cmd, RemoteCommand::Panic));

    // Event-Broadcast → kommt als Text-Frame beim Client an.
    event_tx.send(RemoteEvent::Pong).unwrap();
    let frame = ws_read_frame(&mut stream).await;
    let event: RemoteEvent = serde_json::from_str(&frame).unwrap();
    assert!(matches!(event, RemoteEvent::Pong));
}

#[test]
fn remote_config_from_env() {
    // Defaults (keine KUHPLAY_REMOTE*-Vars in diesem Test-Prozess gesetzt).
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.web_port, 8080);
    assert_eq!(cfg.companion_port, 9999);
    assert!(cfg.web_enabled && cfg.companion_enabled);

    // Ports überschreibbar, einzelner Server abschaltbar.
    std::env::set_var("KUHPLAY_REMOTE_WEB_PORT", "18080");
    std::env::set_var("KUHPLAY_REMOTE_COMPANION_PORT", "19999");
    std::env::set_var("KUHPLAY_REMOTE_WEB", "off");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.web_port, 18080);
    assert_eq!(cfg.companion_port, 19999);
    assert!(!cfg.web_enabled);
    assert!(cfg.companion_enabled);

    // OSC-Defaults.
    assert!(cfg.osc_enabled);
    assert_eq!(cfg.osc_port, 9000);
    assert_eq!(cfg.osc_out, Some(("127.0.0.1".to_string(), 9001)));

    std::env::set_var("KUHPLAY_OSC_PORT", "19000");
    std::env::set_var("KUHPLAY_OSC_OUT", "192.168.1.10:9001");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.osc_port, 19000);
    assert_eq!(cfg.osc_out, Some(("192.168.1.10".to_string(), 9001)));

    std::env::set_var("KUHPLAY_OSC_OUT", "off");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.osc_out, None);

    // Global aus → Web + Companion deaktiviert (OSC unabhängig).
    std::env::set_var("KUHPLAY_REMOTE", "0");
    let cfg = RemoteConfig::from_env();
    assert!(!cfg.web_enabled && !cfg.companion_enabled);
    assert!(cfg.osc_enabled);

    std::env::set_var("KUHPLAY_OSC", "off");
    let cfg = RemoteConfig::from_env();
    assert!(!cfg.osc_enabled);

    // MIDI-Defaults.
    assert!(cfg.midi_enabled);
    assert_eq!(cfg.midi_device, None);

    std::env::set_var("KUHPLAY_MIDI_DEVICE", "Launchpad");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.midi_device.as_deref(), Some("Launchpad"));

    std::env::set_var("KUHPLAY_MIDI", "off");
    let cfg = RemoteConfig::from_env();
    assert!(!cfg.midi_enabled);

    // OSC-Präfix (Default + Custom).
    assert_eq!(cfg.osc_prefix, "/kuhplay");
    std::env::set_var("KUHPLAY_OSC_PREFIX", "/qlab");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.osc_prefix, "/qlab");

    // MIDI-Map (Default + Custom; kaputte Paare werden ignoriert).
    assert_eq!(cfg.midi_map, MidiNoteMap::default());
    std::env::set_var("KUHPLAY_MIDI_MAP", "go=36,panic=37,bogus=1,pause=abc");
    let cfg = RemoteConfig::from_env();
    assert_eq!(cfg.midi_map.go, 36);
    assert_eq!(cfg.midi_map.panic, 37);
    assert_eq!(cfg.midi_map.pause, MidiNoteMap::default().pause);

    std::env::remove_var("KUHPLAY_REMOTE_WEB_PORT");
    std::env::remove_var("KUHPLAY_REMOTE_COMPANION_PORT");
    std::env::remove_var("KUHPLAY_REMOTE_WEB");
    std::env::remove_var("KUHPLAY_REMOTE");
    std::env::remove_var("KUHPLAY_OSC_PORT");
    std::env::remove_var("KUHPLAY_OSC_OUT");
    std::env::remove_var("KUHPLAY_OSC");
    std::env::remove_var("KUHPLAY_OSC_PREFIX");
    std::env::remove_var("KUHPLAY_MIDI_DEVICE");
    std::env::remove_var("KUHPLAY_MIDI");
    std::env::remove_var("KUHPLAY_MIDI_MAP");
}

#[test]
fn app_remote_command_routing() {
    let mut app = ShowApp::new(spawn_mock_engine());

    // SetShowMode → Show-Mode gesetzt.
    app.handle_remote_command(RemoteCommand::SetShowMode { enabled: true });
    assert!(app.show_mode);

    // Arm per Q-Nummer (unbekannte Nummer → nur Warnung, kein Panic).
    app.handle_remote_command(RemoteCommand::Arm {
        cue: "1".to_string(),
        armed: false,
    });
    assert!(!cue_by_number(&app, "1").is_armed);
    app.handle_remote_command(RemoteCommand::Arm {
        cue: "1".to_string(),
        armed: true,
    });
    assert!(cue_by_number(&app, "1").is_armed);
    app.handle_remote_command(RemoteCommand::Arm {
        cue: "999".to_string(),
        armed: true,
    });

    // GoTo auf echten Cue → Status verlässt Standby (Playing/PreWait/Error).
    app.handle_remote_command(RemoteCommand::GoTo {
        cue: "1".to_string(),
    });
    assert_ne!(cue_by_number(&app, "1").status, CueStatus::Standby);
    // GoTo auf unbekannte Nummer → kein Panic.
    app.handle_remote_command(RemoteCommand::GoTo {
        cue: "999".to_string(),
    });

    // Einzel-Cue-Steuerung + globale Commands → keine Panics.
    let id = cue_by_number(&app, "1").id;
    app.handle_remote_command(RemoteCommand::PauseCue { id });
    app.handle_remote_command(RemoteCommand::ResumeCue { id });
    app.handle_remote_command(RemoteCommand::StopCue { id });
    app.handle_remote_command(RemoteCommand::Go);
    app.handle_remote_command(RemoteCommand::PauseAll);
    app.handle_remote_command(RemoteCommand::ResumeAll);
    app.handle_remote_command(RemoteCommand::Panic);
    app.handle_remote_command(RemoteCommand::Reset);
    app.handle_remote_command(RemoteCommand::GetStatus);

    // Status-Snapshot spiegelt den App-Zustand wider.
    let status = app.build_remote_status();
    assert_eq!(status.show_name, "Demo Show");
    assert!(status.show_mode);
    assert_eq!(status.cues.len(), app.carts().active().len());
    assert!(!status.carts.is_empty());
    assert_eq!(
        status.active_cart_name,
        status.carts[status.active_cart].name
    );
}

// ---------------------------------------------------------------------
// Bearbeiten über die Remote
// ---------------------------------------------------------------------

fn remote_app() -> ShowApp {
    ShowApp::new(kuhplay::engine::spawn_mock_engine())
}

#[test]
fn remote_edits_cue_fields_and_keeps_runtime_state() {
    let mut app = remote_app();
    let id = app.carts.active().cues()[1].id;
    let original_id = id;
    app.handle_remote_command(RemoteCommand::UpdateCue {
        id,
        changes: serde_json::json!({
            "name": "Neu",
            "pre_wait": 1500,
            "is_armed": false,
            "id": uuid::Uuid::new_v4(),
            "status": "Playing",
        }),
    });
    let cue = &app.carts.active().cues()[1];
    assert_eq!(cue.name, "Neu");
    assert_eq!(cue.pre_wait, Duration::from_millis(1500));
    assert!(!cue.is_armed);
    assert_eq!(cue.id, original_id, "ID ist geschützt");
    assert_eq!(
        cue.status,
        kuhplay::core::CueStatus::Standby,
        "Status ist geschützt"
    );
}

#[test]
fn remote_rejects_invalid_values_without_changing_the_cue() {
    let mut app = remote_app();
    let id = app.carts.active().cues()[1].id;
    for changes in [
        serde_json::json!({ "pre_wait": "nonsense" }),
        serde_json::json!({ "route": { "channels": 99, "pan": 0.0 } }),
        serde_json::json!("kein objekt"),
    ] {
        app.handle_remote_command(RemoteCommand::UpdateCue { id, changes });
    }
    let cue = &app.carts.active().cues()[1];
    assert!(cue.route.is_none());
    assert_eq!(cue.name, "Intro Video");
}

#[test]
fn show_mode_blocks_remote_edits_but_not_playback() {
    let mut app = remote_app();
    let id = app.carts.active().cues()[1].id;
    let count = app.carts.active().len();
    app.handle_remote_command(RemoteCommand::SetShowMode { enabled: true });
    app.handle_remote_command(RemoteCommand::UpdateCue {
        id,
        changes: serde_json::json!({ "name": "Gesperrt" }),
    });
    app.handle_remote_command(RemoteCommand::NewCue {
        cue_type: kuhplay::core::CueType::Audio,
    });
    app.handle_remote_command(RemoteCommand::DeleteCue { id });
    assert_eq!(app.carts.active().cues()[1].name, "Intro Video");
    assert_eq!(app.carts.active().len(), count);

    app.handle_remote_command(RemoteCommand::Go);
    assert!(
        app.carts.active().cues()[0].status.is_active(),
        "GO bleibt möglich"
    );
    app.handle_remote_command(RemoteCommand::SetShowMode { enabled: false });
    app.handle_remote_command(RemoteCommand::NewCue {
        cue_type: kuhplay::core::CueType::Audio,
    });
    assert_eq!(app.carts.active().len(), count + 1);
}

#[test]
fn remote_structure_and_settings_commands_apply() {
    let mut app = remote_app();
    let cues = app.carts.active().cues().to_vec();
    let count = cues.len();
    app.handle_remote_command(RemoteCommand::MoveCue {
        id: cues[2].id,
        to: 0,
    });
    assert_eq!(app.carts.active().cues()[0].id, cues[2].id);
    app.handle_remote_command(RemoteCommand::DuplicateCues {
        ids: vec![cues[3].id],
    });
    assert_eq!(app.carts.active().len(), count + 1);
    app.handle_remote_command(RemoteCommand::DeleteCue { id: cues[3].id });
    assert_eq!(app.carts.active().len(), count);
    app.handle_remote_command(RemoteCommand::SetPlayhead { id: cues[5].id });
    assert_eq!(
        app.carts.active().cues()[app.carts.active().cursor()].id,
        cues[5].id
    );
    app.handle_remote_command(RemoteCommand::AddCart {
        name: "Zwei".to_string(),
    });
    assert_eq!(app.carts.cart_count(), 2);

    let mut patch = app.audio_patch.clone();
    patch.channels[0].output = 4;
    app.handle_remote_command(RemoteCommand::SetAudioPatch {
        patch: Box::new(patch),
    });
    assert_eq!(app.audio_patch.channels[0].output, 4);

    // Video-Routing (QLab 5: Route/Region statt Screen).
    app.handle_remote_command(RemoteCommand::AddRoute {
        name: "Beamer".to_string(),
        width: 1920,
        height: 1080,
    });
    let route = app.routes.routes().last().unwrap().id;
    app.handle_remote_command(RemoteCommand::UpdateRoute {
        id: route,
        changes: serde_json::json!({ "width": 1280, "name": "Links" }),
    });
    assert_eq!(app.routes.find(route).unwrap().width, 1280);
    let stage = app.stages.stages()[0].name.clone();
    app.handle_remote_command(RemoteCommand::UpdateStage {
        name: stage.clone(),
        changes: serde_json::json!({ "layer": 3 }),
    });
    assert_eq!(app.stages.find(&stage).unwrap().layer, 3);
    app.handle_remote_command(RemoteCommand::RenameStage {
        old: stage.clone(),
        new: "Hauptstage".to_string(),
    });
    assert!(app.stages.find("Hauptstage").is_some());
    app.handle_remote_command(RemoteCommand::RenameStage {
        old: "Hauptstage".to_string(),
        new: stage.clone(),
    });
    // Region anlegen, ändern, entfernen.
    app.handle_remote_command(RemoteCommand::AddRegion {
        stage: stage.clone(),
        changes: serde_json::json!({ "route": route, "x": 10.0, "y": 20.0 }),
    });
    let region = app.stages.find(&stage).unwrap().regions.last().unwrap().id;
    app.handle_remote_command(RemoteCommand::UpdateRegion {
        stage: stage.clone(),
        region,
        changes: serde_json::json!({ "width": 100.0, "auto_blend": false }),
    });
    let updated = app
        .stages
        .find(&stage)
        .unwrap()
        .regions
        .iter()
        .find(|r| r.id == region)
        .unwrap();
    assert_eq!(updated.width, 100.0);
    assert!(!updated.auto_blend);
    app.handle_remote_command(RemoteCommand::RemoveRegion {
        stage: stage.clone(),
        region,
    });
    assert!(app
        .stages
        .find(&stage)
        .unwrap()
        .regions
        .iter()
        .all(|r| r.id != region));
    app.handle_remote_command(RemoteCommand::RemoveRoute { id: route });
    assert!(app.routes.find(route).is_none(), "Route entfernt");
    // Stage loeschen (mit Cues in Nutzung abgelehnt).
    app.handle_remote_command(RemoteCommand::RemoveStage {
        name: "Stage $NOPE$".to_string(),
    });

    app.handle_remote_command(RemoteCommand::SetOutputMode {
        mode: "preview".to_string(),
    });
    assert_eq!(app.output_mode, kuhplay::gui::OutputMode::Preview);
}

#[test]
fn remote_workspace_contains_full_show_and_survives_json() {
    let mut app = remote_app();
    let workspace = app.build_remote_workspace();
    assert_eq!(workspace.cues.len(), app.carts.active().len());
    assert!(!workspace.options["cue_type"].is_empty());
    assert!(!workspace.problems.is_empty(), "Demo-Show hat kaputte Cues");
    let event = RemoteEvent::Workspace(Box::new(workspace));
    let json = serde_json::to_string(&event).unwrap();
    let back: RemoteEvent = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, RemoteEvent::Workspace(_)));
    assert!(json.starts_with("{\"type\":\"workspace\""));
}

// ---------------------------------------------------------------------
// Anmeldung und Zugriffsrechte
// ---------------------------------------------------------------------

/// Minimaler HTTP-Client; Rückgabe: (Status, komplette Antwort).
async fn http(port: u16, request: String) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let status = response.split(' ').nth(1).unwrap().parse().unwrap();
    (status, response)
}

fn post(path: &str, body: &str, cookie: &str) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

fn get(path: &str, cookie: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n{cookie}\r\n")
}

#[tokio::test]
async fn web_remote_requires_pin() {
    let port = free_port().await;
    let auth = kuhplay::remote::web::WebAuth::new(Some("4711".to_string()));
    let (mut cmd_rx, _events) = kuhplay::remote::web::start_web_remote_with_auth(port, auth);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let go = r#"{"action":"go"}"#;

    assert_eq!(
        http(port, get("/", "")).await.0,
        200,
        "Seite selbst ist offen"
    );
    assert_eq!(http(port, get("/api/status", "")).await.0, 401);
    assert_eq!(http(port, get("/api/workspace", "")).await.0, 401);
    let upgrade = "GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: close, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
    assert_eq!(http(port, upgrade.to_string()).await.0, 401);
    assert_eq!(http(port, post("/api/command", go, "")).await.0, 401);
    assert!(cmd_rx.try_recv().is_err(), "ohne PIN kommt nichts an");

    let wrong = post("/api/login", r#"{"pin":"0000"}"#, "");
    assert_eq!(http(port, wrong).await.0, 401);
    let (status, response) = http(port, post("/api/login", r#"{"pin":"4711"}"#, "")).await;
    assert_eq!(status, 200);
    let cookie = response
        .lines()
        .find(|line| line.to_lowercase().starts_with("set-cookie:"))
        .unwrap();
    assert!(cookie.contains("HttpOnly"));
    let value = cookie.split(' ').nth(1).unwrap().trim_end_matches(';');
    let header = format!("Cookie: {value}\r\n");
    assert_eq!(http(port, post("/api/command", go, &header)).await.0, 200);
    let cmd = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv()).await;
    assert!(matches!(cmd, Ok(Some(RemoteCommand::Go))));
}

#[tokio::test]
async fn file_commands_need_a_pin() {
    let port = free_port().await;
    let (mut cmd_rx, _events) = start_web_remote(port);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let body = r#"{"action":"open_show","path":"/tmp/a.kuhplay"}"#;
    http(port, post("/api/command", body, "")).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(cmd_rx.try_recv().is_err(), "ohne PIN kein Dateizugriff");
}

#[tokio::test]
async fn companion_only_accepts_playback() {
    let port = free_port().await;
    let (mut cmd_rx, _events) = start_companion_server(port);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    for line in [
        r#"{"action":"open_show","path":"/tmp/a.kuhplay"}"#,
        r#"{"action":"new_show"}"#,
        r#"{"action":"new_cue","cue_type":"Audio"}"#,
        r#"{"action":"go"}"#,
    ] {
        stream
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
    }
    let cmd = tokio::time::timeout(Duration::from_secs(2), cmd_rx.recv()).await;
    assert!(matches!(cmd, Ok(Some(RemoteCommand::Go))), "{cmd:?}");
}

#[test]
fn remote_show_files_are_limited_to_kuhplay_extension() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = remote_app();
    let victim = dir.path().join("wichtig.txt");
    std::fs::write(&victim, "inhalt").unwrap();
    app.handle_remote_command(RemoteCommand::SaveShowAs {
        path: victim.display().to_string(),
    });
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "inhalt");

    let show = dir.path().join("show.kuhplay");
    app.handle_remote_command(RemoteCommand::SaveShowAs {
        path: show.display().to_string(),
    });
    assert!(show.exists());
    app.handle_remote_command(RemoteCommand::SetShowName {
        name: "Geändert".to_string(),
    });
    app.handle_remote_command(RemoteCommand::OpenShow {
        path: show.display().to_string(),
    });
    assert_eq!(app.show_name, "Demo Show");
}
