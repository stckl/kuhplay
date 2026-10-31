//! Light-Cue (QLab 5, Minimal): Art-Net-DMX-Paket per UDP (Port 6454).
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
        EngineCommand::SendDmx {
            host, port, packet, ..
        } => {
            assert_eq!(*port, ARTNET_PORT);
            assert_eq!(host, "10.0.0.255");
            assert_eq!(&packet[0..8], b"Art-Net\0");
            assert_eq!(u16::from_le_bytes([packet[8], packet[9]]), 0x5000);
            assert_eq!(&packet[18..], &[255, 128]);
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
    let packet = light.to_artnet();
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            host: "127.0.0.1".to_string(),
            port: ARTNET_PORT,
            packet,
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
    assert_eq!(u16::from_be_bytes([p[16], p[17]]), 3, "Data length");
    assert_eq!(&p[18..21], &[200, 100, 0]);
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
    let packet = light.to_sacn();
    engine
        .handle(EngineCommand::SendDmx {
            cue_id: Uuid::new_v4(),
            host: "127.0.0.1".to_string(),
            port: SACN_PORT,
            packet,
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
