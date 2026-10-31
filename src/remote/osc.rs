//! OSC-Steuerung (Open Sound Control über UDP).
//!
//! Ergänzt Web-Remote/Companion um das in der Show-Control-Welt übliche
//! OSC-Protokoll (QLab „OSC"-Netzwerk-Steuerung):
//!
//! ## Eingehend (OSC → [`RemoteCommand`])
//!
//! | Adresse | Argumente | Command |
//! |---|---|---|
//! | `/kuhplay/go` | – | [`RemoteCommand::Go`] |
//! | `/kuhplay/panic` | – | [`RemoteCommand::Panic`] |
//! | `/kuhplay/pause` | – | [`RemoteCommand::PauseAll`] |
//! | `/kuhplay/resume` | – | [`RemoteCommand::ResumeAll`] |
//! | `/kuhplay/reset` | – | [`RemoteCommand::Reset`] |
//! | `/kuhplay/goto` | `string` Cue-Nummer | [`RemoteCommand::GoTo`] |
//! | `/kuhplay/arm` | `string` Cue, `int/float/bool` armed | [`RemoteCommand::Arm`] |
//! | `/kuhplay/showmode` | `int/float/bool` enabled | [`RemoteCommand::SetShowMode`] |
//! | `/kuhplay/status` | – | [`RemoteCommand::GetStatus`] |
//!
//! ## Ausgehend ([`RemoteEvent`] → OSC)
//!
//! Events werden als OSC-Messages an das konfigurierte Ziel geschickt
//! (Default `127.0.0.1:9001`), z. B. `/kuhplay/cue/started <id> <number>
//! <name>` oder `/kuhplay/status <show> <mode> <cart> <cart-name> <playhead>
//! <standby#> <running-count>`.
//!
//! ## Config (Env)
//!
//! `KUHPLAY_OSC=off`, `KUHPLAY_OSC_PORT` (Default 9000),
//! `KUHPLAY_OSC_OUT` = `host:port` (Default `127.0.0.1:9001`) oder `off`.
//!
//! ## Threads
//!
//! Zwei std-Threads (blocking UDP) – In: Listener → Commands, Out:
//! Event-Broadcast → OSC. `blocking_send`/`blocking_recv` sind von diesen
//! Nicht-Runtime-Threads aus erlaubt.

use std::net::UdpSocket;

use rosc::{decoder, encoder, OscMessage, OscPacket, OscType};
use tokio::sync::{broadcast, mpsc};

use super::{RemoteCommand, RemoteEvent};

/// Normalisiert ein OSC-Adress-Präfix: führendes `/` sicherstellen, trailing
/// `/` entfernen (Default-Präfix: `/kuhplay`).
pub fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    let with_slash = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_slash.trim_end_matches('/').to_string()
}

/// Startet den OSC-Server (In: UDP-Listener, Out: Event→OSC-Bridge).
///
/// `prefix`: Adress-Präfix für In und Out (z. B. `/kuhplay` oder `/qlab`).
/// Rückgabe wie bei Web-Remote/Companion: Command-Receiver +
/// Event-Broadcast. `out_addr = None` deaktiviert den OSC-Out.
pub fn start_osc_server(
    listen_port: u16,
    out_addr: Option<(String, u16)>,
    prefix: &str,
) -> (
    mpsc::Receiver<RemoteCommand>,
    broadcast::Sender<RemoteEvent>,
) {
    let prefix = normalize_prefix(prefix);
    let (command_tx, command_rx) = mpsc::channel::<RemoteCommand>(64);
    let (event_tx, _) = broadcast::channel::<RemoteEvent>(256);

    // In: UDP-Listener → Commands.
    {
        let command_tx = command_tx.clone();
        let prefix_in = prefix.clone();
        std::thread::Builder::new()
            .name("kuhplay-osc-in".to_string())
            .spawn(move || {
                let socket = match UdpSocket::bind(("0.0.0.0", listen_port)) {
                    Ok(s) => s,
                    Err(e) => {
                        // Port belegt o.ä. – Live-Betrieb nicht wegen OSC killen.
                        tracing::error!(port = listen_port, %e, "osc bind fehlgeschlagen");
                        return;
                    }
                };
                tracing::info!(port = listen_port, "osc server gestartet");
                let mut buf = [0u8; 65535];
                loop {
                    let (len, peer) = match socket.recv_from(&mut buf) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::error!(%e, "osc recv fehlgeschlagen");
                            continue;
                        }
                    };
                    match decoder::decode_udp(&buf[..len]) {
                        Ok((_, packet)) => {
                            for cmd in osc_packet_to_commands(&packet, &prefix_in) {
                                // blocking_send: von diesem std-Thread aus
                                // erlaubt (nicht innerhalb des tokio-Runtime).
                                if command_tx.blocking_send(cmd).is_err() {
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(%peer, %e, "osc: ungültiges Paket verworfen");
                        }
                    }
                }
            })
            .expect("osc-in thread must spawn");
    }

    // Out: Events → OSC-Messages an out_addr. Die Subscription wird VOR dem
    // Thread-Spawn angelegt, damit keine Events zwischen Server-Start und
    // Thread-Start verloren gehen.
    if let Some((host, port)) = out_addr {
        let mut event_rx = event_tx.subscribe();
        let prefix_out = prefix.clone();
        std::thread::Builder::new()
            .name("kuhplay-osc-out".to_string())
            .spawn(move || {
                let socket = match UdpSocket::bind("0.0.0.0:0") {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(%e, "osc-out socket fehlgeschlagen");
                        return;
                    }
                };
                let target = format!("{host}:{port}");
                loop {
                    let event = match event_rx.blocking_recv() {
                        Ok(e) => e,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(n, "osc-out: events verworfen (lagged)");
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    };
                    for msg in event_to_osc_messages(&event, &prefix_out) {
                        match encoder::encode(&OscPacket::Message(msg)) {
                            Ok(bytes) => {
                                if let Err(e) = socket.send_to(&bytes, &target) {
                                    tracing::warn!(%e, %target, "osc-out send fehlgeschlagen");
                                }
                            }
                            Err(e) => tracing::warn!(%e, "osc-out encode fehlgeschlagen"),
                        }
                    }
                }
            })
            .expect("osc-out thread must spawn");
    }

    (command_rx, event_tx)
}

/// Wandelt ein OSC-Paket in Remote-Commands um (0..n pro Paket, mit `prefix`).
pub fn osc_packet_to_commands(packet: &OscPacket, prefix: &str) -> Vec<RemoteCommand> {
    let OscPacket::Message(msg) = packet else {
        return Vec::new(); // Bundles: V1 nicht unterstützt.
    };
    osc_message_to_command(msg, prefix).into_iter().collect()
}

/// Wandelt eine OSC-Message in genau 0 oder 1 Remote-Command um.
/// Das `prefix` wird von der Adresse gestrippt, dann matcht der Suffix
/// (`/go`, `/panic`, …).
pub fn osc_message_to_command(msg: &OscMessage, prefix: &str) -> Option<RemoteCommand> {
    let args = &msg.args;
    let suffix = msg.addr.strip_prefix(prefix).unwrap_or("");
    match suffix {
        "/go" => Some(RemoteCommand::Go),
        "/panic" => Some(RemoteCommand::Panic),
        "/pause" => Some(RemoteCommand::PauseAll),
        "/resume" => Some(RemoteCommand::ResumeAll),
        "/reset" => Some(RemoteCommand::Reset),
        "/goto" => {
            let cue = osc_string_arg(args.first())?;
            Some(RemoteCommand::GoTo { cue })
        }
        "/arm" => {
            let cue = osc_string_arg(args.first())?;
            let armed = osc_bool_arg(args.get(1))?;
            Some(RemoteCommand::Arm { cue, armed })
        }
        "/showmode" => {
            let enabled = osc_bool_arg(args.first())?;
            Some(RemoteCommand::SetShowMode { enabled })
        }
        "/status" => Some(RemoteCommand::GetStatus),
        "/timecode" => {
            let text = osc_string_arg(args.first())?;
            let fps = match args.get(1) {
                Some(OscType::Int(v)) => crate::core::Fps::from_fps(*v as u64),
                Some(OscType::Float(v)) => crate::core::Fps::from_fps(*v as u64),
                _ => crate::core::Fps::default(),
            };
            let tc = crate::core::Timecode::parse_hmsf(&text, fps)?;
            Some(RemoteCommand::ReceiveTimecode { tc })
        }
        other => {
            tracing::warn!(%other, "osc: unbekannte Adresse");
            None
        }
    }
}

/// Wandelt ein Remote-Event in OSC-Messages um (0..n, mit `prefix`).
pub fn event_to_osc_messages(event: &RemoteEvent, prefix: &str) -> Vec<OscMessage> {
    let msg = |suffix: &str, args: Vec<OscType>| OscMessage {
        addr: format!("{prefix}{suffix}"),
        args,
    };
    let s = OscType::String;
    let i = OscType::Int;
    match event {
        RemoteEvent::Status(status) => vec![msg(
            "/status",
            vec![
                s(status.show_name.clone()),
                i(status.show_mode as i32),
                i(status.active_cart as i32),
                s(status.active_cart_name.clone()),
                i(status.playhead as i32),
                s(status
                    .standby
                    .as_ref()
                    .map(|c| c.number.clone())
                    .unwrap_or_default()),
                i(status.running.len() as i32),
            ],
        )],
        RemoteEvent::CueStarted { id, number, name } => vec![msg(
            "/cue/started",
            vec![s(id.to_string()), s(number.clone()), s(name.clone())],
        )],
        RemoteEvent::CuePaused { id } => vec![msg("/cue/paused", vec![s(id.to_string())])],
        RemoteEvent::CueResumed { id } => {
            vec![msg("/cue/resumed", vec![s(id.to_string())])]
        }
        RemoteEvent::CueFinished { id } => {
            vec![msg("/cue/finished", vec![s(id.to_string())])]
        }
        RemoteEvent::CueStopped { id } => {
            vec![msg("/cue/stopped", vec![s(id.to_string())])]
        }
        RemoteEvent::CueError { id, message } => vec![msg(
            "/cue/error",
            vec![s(id.to_string()), s(message.clone())],
        )],
        RemoteEvent::PlayheadMoved { cart, index } => {
            vec![msg("/playhead", vec![i(*cart as i32), i(*index as i32)])]
        }
        RemoteEvent::StandbyChanged { number, name } => {
            vec![msg("/standby", vec![s(number.clone()), s(name.clone())])]
        }
        RemoteEvent::ShowModeChanged { enabled } => {
            vec![msg("/showmode", vec![i(*enabled as i32)])]
        }
        RemoteEvent::Panicked => vec![msg("/panicked", vec![])],
        RemoteEvent::Pong => vec![msg("/pong", vec![])],
        RemoteEvent::Workspace(_) | RemoteEvent::Error { .. } => vec![],
    }
}

/// OSC-String-Argument (nur `OscType::String`).
fn osc_string_arg(arg: Option<&OscType>) -> Option<String> {
    match arg {
        Some(OscType::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// OSC-Bool-Argument (`Int`/`Float`/`Bool`/`String`).
fn osc_bool_arg(arg: Option<&OscType>) -> Option<bool> {
    match arg {
        Some(OscType::Int(v)) => Some(*v != 0),
        Some(OscType::Float(v)) => Some(*v != 0.0),
        Some(OscType::Bool(v)) => Some(*v),
        Some(OscType::String(v)) => Some(matches!(v.as_str(), "1" | "true" | "on")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "/kuhplay";

    fn msg(addr: &str, args: Vec<OscType>) -> OscMessage {
        OscMessage {
            addr: addr.to_string(),
            args,
        }
    }

    #[test]
    fn osc_in_address_mapping() {
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/go", vec![]), PREFIX),
            Some(RemoteCommand::Go)
        ));
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/panic", vec![]), PREFIX),
            Some(RemoteCommand::Panic)
        ));
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/pause", vec![]), PREFIX),
            Some(RemoteCommand::PauseAll)
        ));
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/resume", vec![]), PREFIX),
            Some(RemoteCommand::ResumeAll)
        ));
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/reset", vec![]), PREFIX),
            Some(RemoteCommand::Reset)
        ));
        assert!(matches!(
            osc_message_to_command(&msg("/kuhplay/status", vec![]), PREFIX),
            Some(RemoteCommand::GetStatus)
        ));
        // Anderes Präfix matcht nicht.
        assert!(osc_message_to_command(&msg("/kuhplay/go", vec![]), "/qlab").is_none());
        // Unbekannt → None.
        assert!(osc_message_to_command(&msg("/other/go", vec![]), PREFIX).is_none());
        // Fehlende Argumente → None.
        assert!(osc_message_to_command(&msg("/kuhplay/goto", vec![]), PREFIX).is_none());
        assert!(osc_message_to_command(
            &msg("/kuhplay/arm", vec![OscType::String("1".into())]),
            PREFIX
        )
        .is_none());
    }

    #[test]
    fn osc_in_custom_prefix() {
        // QLab-kompatibles Präfix: /qlab/go → Go.
        assert!(matches!(
            osc_message_to_command(&msg("/qlab/go", vec![]), "/qlab"),
            Some(RemoteCommand::Go)
        ));
        let cmd = osc_message_to_command(
            &msg("/qlab/goto", vec![OscType::String("5.1".into())]),
            "/qlab",
        );
        assert!(matches!(cmd, Some(RemoteCommand::GoTo { ref cue }) if cue == "5.1"));
        // Anderes Präfix matcht nicht mehr.
        assert!(osc_message_to_command(&msg("/kuhplay/go", vec![]), "/qlab").is_none());
    }

    #[test]
    fn osc_prefix_normalization() {
        assert_eq!(normalize_prefix("/kuhplay"), "/kuhplay");
        assert_eq!(normalize_prefix("kuhplay"), "/kuhplay");
        assert_eq!(normalize_prefix("/kuhplay/"), "/kuhplay");
        assert_eq!(normalize_prefix(" /qlab/ "), "/qlab");
    }

    #[test]
    fn osc_in_argument_parsing() {
        let cmd = osc_message_to_command(
            &msg("/kuhplay/goto", vec![OscType::String("5.1".into())]),
            PREFIX,
        );
        assert!(matches!(cmd, Some(RemoteCommand::GoTo { ref cue }) if cue == "5.1"));

        // Bool-Argumente: Int, Float, Bool, String.
        for (arg, expected) in [
            (OscType::Int(1), true),
            (OscType::Int(0), false),
            (OscType::Float(1.0), true),
            (OscType::Bool(true), true),
            (OscType::String("on".into()), true),
            (OscType::String("off".into()), false),
        ] {
            let cmd = osc_message_to_command(&msg("/kuhplay/showmode", vec![arg]), PREFIX);
            assert!(
                matches!(cmd, Some(RemoteCommand::SetShowMode { enabled }) if enabled == expected)
            );
        }

        let cmd = osc_message_to_command(
            &msg(
                "/kuhplay/arm",
                vec![OscType::String("3".into()), OscType::Int(1)],
            ),
            PREFIX,
        );
        assert!(matches!(cmd, Some(RemoteCommand::Arm { ref cue, armed: true }) if cue == "3"));
    }

    #[test]
    fn osc_out_event_encoding_roundtrip() {
        let id = uuid::Uuid::new_v4();
        let events = vec![
            RemoteEvent::CueStarted {
                id,
                number: "2".into(),
                name: "Intro".into(),
            },
            RemoteEvent::CuePaused { id },
            RemoteEvent::ShowModeChanged { enabled: true },
            RemoteEvent::Panicked,
            RemoteEvent::Pong,
        ];
        for event in &events {
            for msg in event_to_osc_messages(event, PREFIX) {
                // Encode → Decode: Adresse und Argumente müssen erhalten bleiben.
                let bytes = encoder::encode(&OscPacket::Message(msg.clone())).unwrap();
                let (_, decoded) = decoder::decode_udp(&bytes).unwrap();
                let OscPacket::Message(back) = decoded else {
                    panic!("nur Messages erwartet");
                };
                assert_eq!(back.addr, msg.addr);
                assert_eq!(back.args, msg.args);
            }
        }
        // Konkrete Adressen prüfen (mit Präfix).
        let msgs = event_to_osc_messages(
            &RemoteEvent::CueStarted {
                id,
                number: "2".into(),
                name: "Intro".into(),
            },
            PREFIX,
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].addr, "/kuhplay/cue/started");
        assert_eq!(
            msgs[0].args,
            vec![
                OscType::String(id.to_string()),
                OscType::String("2".into()),
                OscType::String("Intro".into())
            ]
        );
        // Custom-Präfix landet in den Out-Adressen.
        let msgs = event_to_osc_messages(&RemoteEvent::Panicked, "/qlab");
        assert_eq!(msgs[0].addr, "/qlab/panicked");
    }
}
