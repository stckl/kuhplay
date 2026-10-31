//! Network-Cue (QLab 5): UDP/TCP-Versand über die Real-Engine (Loopback)
//! plus Core-Verhalten (Sofort-Cue, Validierung).

use std::io::Read;
use std::net::{TcpListener, UdpSocket};
use std::time::{Duration, Instant};

use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, NetworkMessage, NetworkProtocol};
use kuhplay::engine::frame::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store,
};
use kuhplay::engine::{EngineCommand, EngineEvent, MediaEngine, RealMediaEngine};

/// Real-Engine direkt (ohne spawn): In CI/Sandbox blockiert `AudioBackend::new()`
/// gelegentlich den Engine-Thread-Start – der direkte `handle`-Aufruf ist
/// deterministisch und braucht keine Soundkarte.
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

// ---------------------------------------------------------------------
// Core: Sofort-Cue + Validierung
// ---------------------------------------------------------------------

#[test]
fn network_cue_triggers_send_network_and_completes_instantly() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "trigger", CueType::Network);
    cue.network = Some(NetworkMessage {
        protocol: NetworkProtocol::Udp,
        host: "192.168.1.20".to_string(),
        port: 7000,
        message: "GO".to_string(),
    });
    list.add(cue);

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    assert!(
        matches!(&outcome.commands[0], EngineCommand::SendNetwork { host, port, message, protocol, .. }
            if host == "192.168.1.20" && *port == 7000 && message == "GO" && *protocol == NetworkProtocol::Udp),
        "network cue must produce SendNetwork with its payload"
    );
    assert_eq!(
        list.cues()[0].status,
        CueStatus::Completed,
        "network cue is an instant cue (completed immediately)"
    );
}

#[test]
fn network_cue_without_target_goes_to_error() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "broken", CueType::Network);
    cue.network = Some(NetworkMessage::default()); // host/port fehlen
    list.add(cue);
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);

    // Ganz ohne network-Feld ebenfalls Error.
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty", CueType::Network));
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn network_cue_in_group_completes_instantly() {
    use kuhplay::core::{CueList, GroupMode};
    let mut child = Cue::new("1.1", "net", CueType::Network);
    child.network = Some(NetworkMessage {
        host: "10.0.0.1".to_string(),
        port: 1234,
        ..Default::default()
    });
    let mut list = CueList::new();
    list.add(kuhplay::core::Cue::new_group(
        "1",
        "g",
        GroupMode::Simultaneous,
        vec![child],
    ));
    let outcome = list.go().expect("GO must trigger group");
    assert!(outcome
        .commands
        .iter()
        .any(|c| matches!(c, EngineCommand::SendNetwork { .. })));
    let group = &list.cues()[0];
    assert_eq!(group.children[0].status, CueStatus::Completed);
    assert_eq!(group.status, CueStatus::Completed);
}

// ---------------------------------------------------------------------
// Real-Engine: UDP/TCP-Loopback
// ---------------------------------------------------------------------

fn recv_until(
    rx: &crossbeam_channel::Receiver<EngineEvent>,
    pred: impl Fn(&EngineEvent) -> bool,
    timeout: Duration,
) -> Option<EngineEvent> {
    let deadline = Instant::now() + timeout;
    loop {
        while let Ok(e) = rx.try_recv() {
            if pred(&e) {
                return Some(e);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn real_engine_sends_udp_datagram() {
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = receiver.local_addr().unwrap().port();
    receiver
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    let (mut engine, _rx) = direct_engine();
    engine
        .handle(EngineCommand::SendNetwork {
            cue_id: Uuid::new_v4(),
            protocol: NetworkProtocol::Udp,
            host: "127.0.0.1".to_string(),
            port,
            message: "hello-kuhplay".to_string(),
        })
        .unwrap();

    let mut buf = [0u8; 256];
    let (n, _from) = receiver
        .recv_from(&mut buf)
        .expect("UDP datagram must arrive");
    assert_eq!(&buf[..n], b"hello-kuhplay");
}

#[test]
fn real_engine_sends_tcp_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("TCP connect must arrive");
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .expect("read to EOF (client closes after send)");
        buf
    });

    let (mut engine, _rx) = direct_engine();
    engine
        .handle(EngineCommand::SendNetwork {
            cue_id: Uuid::new_v4(),
            protocol: NetworkProtocol::Tcp,
            host: "127.0.0.1".to_string(),
            port,
            message: "tcp-payload".to_string(),
        })
        .unwrap();

    let buf = handle.join().unwrap();
    assert_eq!(buf, b"tcp-payload");
}

#[test]
fn real_engine_reports_tcp_connect_failure_as_error_event() {
    // Port garantiert geschlossen: Listener binden, Port merken, schließen.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    };

    let (mut engine, rx) = direct_engine();
    let cue_id = Uuid::new_v4();
    engine
        .handle(EngineCommand::SendNetwork {
            cue_id,
            protocol: NetworkProtocol::Tcp,
            host: "127.0.0.1".to_string(),
            port,
            message: "x".to_string(),
        })
        .unwrap();

    let event = recv_until(
        &rx,
        |e| matches!(e, EngineEvent::Error { cue_id: id, .. } if *id == Some(cue_id)),
        Duration::from_secs(3),
    );
    assert!(
        event.is_some(),
        "connect to closed port must produce an Error event"
    );
}
