//! Light-Cue (Minimal): Art-Net-DMX-Paket per UDP (Port 6454).
//! Core-Verhalten (Sofort-Cue, Paket-Bytes, Validierung) + UDP-Loopback
//! über die direkt konstruierte Real-Engine (deterministisch, keine Soundkarte).

use std::net::UdpSocket;
use std::time::Duration;

use uuid::Uuid;

use kuhplay::core::{Cue, CueList, CueStatus, CueType, LightMessage, ARTNET_PORT};
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
fn light_cue_triggers_send_dmx_with_artnet_packet_and_completes_instantly() {
    let mut list = CueList::new();
    let mut cue = Cue::new("1", "dimmer", CueType::Light);
    cue.light = Some(LightMessage {
        protocol: Default::default(),
        universe: 0,
        start_channel: 1,
        values: vec![255, 128],
        host: "10.0.0.255".to_string(),
    });
    list.add(cue);

    let outcome = list.go().expect("GO must trigger");
    assert_eq!(outcome.commands.len(), 1);
    match &outcome.commands[0] {
        EngineCommand::SendDmx { message, .. } => {
            assert_eq!(message.host, "10.0.0.255");
            assert_eq!(message.protocol.port(), ARTNET_PORT);
            let packet = message.to_artnet();
            assert_eq!(&packet[0..8], b"Art-Net\0");
            assert_eq!(u16::from_le_bytes([packet[8], packet[9]]), 0x5000);
            assert_eq!(&packet[18..20], &[255, 128]);
        }
        other => panic!("expected SendDmx, got {other:?}"),
    }
    assert_eq!(list.cues()[0].status, CueStatus::Completed);
}

#[test]
fn light_cue_without_values_goes_to_error() {
    let mut list = CueList::new();
    list.add(Cue::new("1", "empty", CueType::Light));
    let outcome = list.go().expect("GO must trigger");
    assert!(outcome.commands.is_empty());
    assert_eq!(list.cues()[0].status, CueStatus::Error);
}

#[test]
fn real_engine_sends_artnet_packet_via_udp() {
    // Art-Net-Port ist fix 6454: Loopback-Socket auf 127.0.0.1:6454 binden.
    // Ist der Port belegt (paralleler Test), überspringt der Test sauber.
    let Ok(rx) = UdpSocket::bind(("127.0.0.1", ARTNET_PORT)) else {
        return;
    };
    rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

    let (mut engine, _rx_events) = direct_engine();
    let light = LightMessage {
        protocol: Default::default(),
        universe: 2,
        start_channel: 10,
        values: vec![200, 100, 0],
        host: "127.0.0.1".to_string(),
    };
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: light.clone(),
        })
        .unwrap();

    let mut buf = [0u8; 600];
    let (n, _from) = rx
        .recv_from(&mut buf)
        .expect("Art-Net-Paket muss an Port 6454 ankommen");
    let p = &buf[..n];
    assert_eq!(&p[0..8], b"Art-Net\0");
    assert_eq!(u16::from_le_bytes([p[8], p[9]]), 0x5000, "OpDmx");
    assert_eq!(p[14], 2, "Universe 2 (SubUni)");
    // Vollständiges Universe (512 B); start_channel=10 → Daten-Index 9.
    assert_eq!(u16::from_be_bytes([p[16], p[17]]), 512, "Data length");
    assert_eq!(&p[18 + 9..18 + 12], &[200, 100, 0]);
}

#[test]
fn real_engine_dmx_state_mixed_pro_universe() {
    // Regression Review A2: start_channel darf Kanäle davor nicht auf 0 setzen.
    let Ok(rx) = UdpSocket::bind(("127.0.0.1", ARTNET_PORT)) else {
        return;
    };
    rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (mut engine, _rx) = direct_engine();
    let base = LightMessage {
        universe: 3,
        host: "127.0.0.1".to_string(),
        ..Default::default()
    };
    // Cue 1: Kanal 1 = 9.
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: LightMessage {
                start_channel: 1,
                values: vec![9],
                ..base.clone()
            },
        })
        .unwrap();
    // Cue 2: nur Kanal 10 = 7.
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: LightMessage {
                start_channel: 10,
                values: vec![7],
                ..base.clone()
            },
        })
        .unwrap();
    // Zwei Pakete einsammeln (UDP-Reihenfolge der Send-Threads nicht garantiert).
    let mut packets = Vec::new();
    let mut buf = [0u8; 600];
    for _ in 0..2 {
        let (n, _) = rx.recv_from(&mut buf).expect("Paket");
        packets.push(buf[..n].to_vec());
    }
    // Das Paket mit Kanal 10 = 7 muss Kanal 1 = 9 behalten (State-Mixing).
    let mixed = packets
        .iter()
        .find(|p| p[18 + 9] == 7)
        .expect("Paket mit Kanal 10 = 7 fehlt");
    assert_eq!(mixed[18], 9, "Kanal 1 behält letzten Stand (nicht 0)");
}

#[test]
fn real_engine_sacn_sequence_cid_source_name() {
    // Regression Review A5: Sequenz läuft pro Universe, CID fest, Source-Name gesetzt.
    use kuhplay::core::{LightProtocol, SACN_PORT};
    let Ok(rx) = UdpSocket::bind(("127.0.0.1", SACN_PORT)) else {
        return;
    };
    rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (mut engine, _rx) = direct_engine();
    let msg = LightMessage {
        protocol: LightProtocol::Sacn,
        universe: 7,
        start_channel: 1,
        values: vec![255, 0, 128],
        host: "127.0.0.1".to_string(),
    };
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: msg.clone(),
        })
        .unwrap();
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: msg.clone(),
        })
        .unwrap();
    let mut buf = [0u8; 700];
    let mut packets = Vec::new();
    for _ in 0..2 {
        let (n, _) = rx.recv_from(&mut buf).expect("sACN-Paket");
        packets.push(buf[..n].to_vec());
    }
    // Reihenfolge der Send-Threads nicht garantiert → nach Sequenz sortieren.
    packets.sort_by_key(|p| p[111]);
    let p1 = &packets[0];
    let p2 = &packets[1];
    assert_eq!(p1[111], 1, "erste Sequenz = 1");
    assert_eq!(p2[111], 2, "Sequenz läuft hoch");
    assert_eq!(&p1[22..38], &p2[22..38], "CID identisch");
    assert!(p1[22..38].iter().any(|b| *b != 0), "CID nicht Null");
    assert_eq!(&p1[44..51], b"KuhPlay", "Source-Name");
    assert_eq!(&p1[126..129], &[255, 0, 128]);
}

#[test]
fn real_engine_broadcast_host_erlaubt() {
    // CI/Runner ohne Broadcast-Route (ENETUNREACH/EACCES) sauber überspringen.
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.set_broadcast(true)?;
        s.send_to(&[0u8; 1], "255.255.255.255:9930")
    });
    if let Err(e) = probe {
        println!("SKIP real_engine_broadcast_host_erlaubt: Broadcast in dieser Umgebung nicht routbar ({e})");
        return;
    }
    // Regression Review A3: 255.255.255.255 ohne SO_BROADCAST → Permission denied.
    let (mut engine, rx_events) = direct_engine();
    let msg = LightMessage {
        host: "255.255.255.255".to_string(),
        values: vec![255],
        ..Default::default()
    };
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: msg,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let err = rx_events
        .try_iter()
        .find(|e| matches!(e, EngineEvent::Error { .. }));
    assert!(
        err.is_none(),
        "Broadcast-Senden darf nicht fehlschlagen: {err:?}"
    );
}

#[test]
fn real_engine_sends_sacn_packet_via_udp_port_5568() {
    use kuhplay::core::{LightProtocol, SACN_PORT};
    // sACN-Port fix 5568; bei Belegung (paralleler Test) sauber überspringen.
    let Ok(rx) = UdpSocket::bind(("127.0.0.1", SACN_PORT)) else {
        return;
    };
    rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();

    let (mut engine, _rx_events) = direct_engine();
    let light = LightMessage {
        protocol: LightProtocol::Sacn,
        universe: 7,
        start_channel: 1,
        values: vec![255, 0, 128],
        host: "127.0.0.1".to_string(),
    };
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            message: light.clone(),
        })
        .unwrap();

    let mut buf = [0u8; 600];
    let (n, _from) = rx
        .recv_from(&mut buf)
        .expect("sACN-Paket muss an Port 5568 ankommen");
    let p = &buf[..n];
    assert_eq!(&p[4..16], b"ASC-E1.31\0\0\0");
    assert_eq!(u16::from_be_bytes([p[113], p[114]]), 7, "Universe 7");
    assert_eq!(&p[126..129], &[255, 0, 128]);
}
