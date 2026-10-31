//! Companion-Cue: triggert Bitfocus Companion per HTTP-POST
//! (`POST /api/location/<page>/<row>/<col>/<action>`) auf dessen Web-Port.
//! Core-Verhalten (Sofort-Cue, Pfad, Validierung) + HTTP-Loopback gegen
//! einen lokalen Test-Server (direkt konstruierte Real-Engine).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use uuid::Uuid;

use kuhplay::core::{CompanionAction, CompanionTrigger, Cue, CueList, CueStatus, CueType};
use kuhplay::engine::frame::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, RealMediaEngine};

fn direct_engine() -> (RealMediaEngine, crossbeam_channel::Receiver<EngineEvent>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (
        RealMediaEngine::new(
            new_frame_store(),
            new_opacity_store(),
            new_stage_frame_store(),
            new_route_frame_store(),
            tx,
        ),
        rx,
    )
}

#[test]
fn companion_cue_triggers_http_post_and_completes_instantly() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "bank", CueType::Companion);
    cue.companion = Some(CompanionTrigger {
        host: "192.168.88.50".to_string(),
        port: 8000,
        page: 2,
        row: 3,
        column: 5,
        action: CompanionAction::Press,
    });
    list.add(cue);

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    assert!(
        matches!(&outcome.commands[0], EngineCommand::SendHttpPost { host, port, path, .. }
            if host == "192.168.88.50" && *port == 8000 && path == "/api/location/2/3/5/press"),
        "companion cue must produce SendHttpPost with the API path"
    );
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

#[test]
fn companion_cue_without_trigger_goes_to_error() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty", CueType::Companion));
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn real_engine_posts_to_companion_api_loopback() {
    // Mini-HTTP-Server: liest Request-Zeile + Header, antwortet 200.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        // Header bis Leerzeile lesen.
        let mut line = String::new();
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
        }
        let mut stream = reader.into_inner();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        request_line
    });

    let (mut engine, _rx) = direct_engine();
    engine
        .handle(EngineCommand::SendHttpPost {
            cue_id: Uuid::new_v4(),
            host: "127.0.0.1".to_string(),
            port,
            path: "/api/location/1/0/0/press".to_string(),
        })
        .unwrap();

    let request_line = server.join().unwrap();
    assert!(
        request_line.starts_with("POST /api/location/1/0/0/press HTTP/1.1"),
        "falsche Request-Zeile: {request_line}"
    );
}

#[test]
fn real_engine_reports_http_connect_failure_as_error_event() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        p
    };
    let (mut engine, rx) = direct_engine();
    let cue_id = Uuid::new_v4();
    engine
        .handle(EngineCommand::SendHttpPost {
            cue_id,
            host: "127.0.0.1".to_string(),
            port,
            path: "/api/location/1/0/0/press".to_string(),
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        while let Ok(e) = rx.try_recv() {
            if matches!(e, EngineEvent::Error { cue_id: id, .. } if id == Some(cue_id)) {
                return; // Error-Event da – gut.
            }
        }
        assert!(
            Instant::now() < deadline,
            "kein Error-Event beim Connect-Fehler"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
