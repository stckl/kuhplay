//! Integrationstests für die OSC-Steuerung (UDP In/Out, `src/remote/osc.rs`):
//! * OSC-In: Adressen → RemoteCommands (über echtes UDP)
//! * OSC-Out: RemoteEvents → OSC-Messages (über echtes UDP)
//! * Unbekannte Adressen werden verworfen (Reihenfolge bleibt erhalten)

use std::net::UdpSocket;
use std::time::Duration;

use kuhplay::remote::osc::start_osc_server;
use kuhplay::remote::{RemoteCommand, RemoteEvent, RemoteStatus};
use rosc::{decoder, encoder, OscMessage, OscPacket, OscType};

fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Empfangs-Socket für OSC-Out. Direkt gebunden statt erst einen freien Port
/// zu suchen und später zu binden: Dazwischen kann ein parallel laufender
/// Test denselben Port belegen.
fn out_socket() -> (UdpSocket, u16) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}

/// Sendet eine OSC-Message über UDP (eine Socket-Instanz → Reihenfolge sicher).
fn send_osc(socket: &UdpSocket, port: u16, addr: &str, args: Vec<OscType>) {
    let msg = OscMessage {
        addr: addr.to_string(),
        args,
    };
    let bytes = encoder::encode(&OscPacket::Message(msg)).unwrap();
    socket.send_to(&bytes, ("127.0.0.1", port)).unwrap();
}

/// Empfängt eine OSC-Message (mit Timeout).
fn recv_osc(socket: &UdpSocket) -> OscMessage {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut buf = [0u8; 65535];
    let (len, _) = socket.recv_from(&mut buf).unwrap();
    match decoder::decode_udp(&buf[..len]).unwrap() {
        (_, OscPacket::Message(msg)) => msg,
        _ => panic!("nur Messages erwartet"),
    }
}

#[test]
fn osc_in_maps_addresses_to_commands() {
    let port = free_port();
    let (mut cmd_rx, _event_tx) = start_osc_server(port, None, "/kuhplay");
    std::thread::sleep(Duration::from_millis(100));

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();

    send_osc(&socket, port, "/kuhplay/go", vec![]);
    assert!(matches!(cmd_rx.blocking_recv().unwrap(), RemoteCommand::Go));

    send_osc(
        &socket,
        port,
        "/kuhplay/goto",
        vec![OscType::String("5.1".into())],
    );
    match cmd_rx.blocking_recv().unwrap() {
        RemoteCommand::GoTo { cue } => assert_eq!(cue, "5.1"),
        other => panic!("erwartet GoTo, got {other:?}"),
    }

    send_osc(
        &socket,
        port,
        "/kuhplay/arm",
        vec![OscType::String("3".into()), OscType::Int(1)],
    );
    match cmd_rx.blocking_recv().unwrap() {
        RemoteCommand::Arm { cue, armed } => {
            assert_eq!(cue, "3");
            assert!(armed);
        }
        other => panic!("erwartet Arm, got {other:?}"),
    }

    send_osc(&socket, port, "/kuhplay/showmode", vec![OscType::Int(0)]);
    match cmd_rx.blocking_recv().unwrap() {
        RemoteCommand::SetShowMode { enabled } => assert!(!enabled),
        other => panic!("erwartet SetShowMode, got {other:?}"),
    }

    send_osc(&socket, port, "/kuhplay/panic", vec![]);
    assert!(matches!(
        cmd_rx.blocking_recv().unwrap(),
        RemoteCommand::Panic
    ));

    // Unbekannte Adresse geht als Cue-OSC-Trigger an die App, danach Reset.
    send_osc(&socket, port, "/other/go", vec![]);
    send_osc(&socket, port, "/kuhplay/reset", vec![]);
    assert!(matches!(
        cmd_rx.blocking_recv().unwrap(),
        RemoteCommand::OscAddress { ref address } if address == "/other/go"
    ));
    assert!(matches!(
        cmd_rx.blocking_recv().unwrap(),
        RemoteCommand::Reset
    ));
}

#[test]
fn osc_out_encodes_events() {
    let listen = free_port();
    let (socket, out_port) = out_socket();
    let (_cmd_rx, event_tx) = start_osc_server(
        listen,
        Some(("127.0.0.1".to_string(), out_port)),
        "/kuhplay",
    );
    std::thread::sleep(Duration::from_millis(100));

    // CueStarted → /kuhplay/cue/started <id> <number> <name>
    let id = uuid::Uuid::new_v4();
    event_tx
        .send(RemoteEvent::CueStarted {
            id,
            number: "2".to_string(),
            name: "Intro".to_string(),
        })
        .unwrap();
    let msg = recv_osc(&socket);
    assert_eq!(msg.addr, "/kuhplay/cue/started");
    assert_eq!(
        msg.args,
        vec![
            OscType::String(id.to_string()),
            OscType::String("2".to_string()),
            OscType::String("Intro".to_string())
        ]
    );

    // Status → /kuhplay/status mit 7 Argumenten.
    let status = RemoteStatus::from_app("Show", true, 0, "Cart 1", 3, None, vec![], vec![], vec![]);
    event_tx.send(RemoteEvent::Status(status)).unwrap();
    let msg = recv_osc(&socket);
    assert_eq!(msg.addr, "/kuhplay/status");
    assert_eq!(msg.args.len(), 7);
    assert_eq!(msg.args[0], OscType::String("Show".to_string()));
    assert_eq!(msg.args[1], OscType::Int(1));
    assert_eq!(msg.args[4], OscType::Int(3));

    // Panicked → /kuhplay/panicked ohne Argumente.
    event_tx.send(RemoteEvent::Panicked).unwrap();
    let msg = recv_osc(&socket);
    assert_eq!(msg.addr, "/kuhplay/panicked");
    assert!(msg.args.is_empty());
}

#[test]
fn osc_custom_prefix_in_and_out() {
    let listen = free_port();
    let (socket, out_port) = out_socket();
    let (mut cmd_rx, event_tx) =
        start_osc_server(listen, Some(("127.0.0.1".to_string(), out_port)), "/show");
    std::thread::sleep(Duration::from_millis(100));

    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();

    // In: /show/go → Go (Default-Präfix /kuhplay matcht nicht).
    send_osc(&sender, listen, "/show/go", vec![]);
    assert!(matches!(cmd_rx.blocking_recv().unwrap(), RemoteCommand::Go));

    // Out: Events landen unter /show/…
    let id = uuid::Uuid::new_v4();
    event_tx
        .send(RemoteEvent::CueStarted {
            id,
            number: "1".to_string(),
            name: "Szene".to_string(),
        })
        .unwrap();
    let msg = recv_osc(&socket);
    assert_eq!(msg.addr, "/show/cue/started");
    assert_eq!(msg.args[1], OscType::String("1".to_string()));
}
