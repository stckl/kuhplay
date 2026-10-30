//! Companion (Bitfocus) TCP-Server: bidirektionales JSON-lines Protokoll.
//!
//! Companion verbindet sich per TCP (Default-Port 9999) und tauscht
//! JSON-Nachrichten aus (eine pro Zeile, `\n`-getrennt):
//!
//! * **Companion → App:** [`RemoteCommand`] (go, panic, go_to, …)
//! * **App → Companion:** [`RemoteEvent`] (Status-Updates, Cue-Events)
//!
//! ## Bidirektionales Feedback
//! Die App pusht bei jeder Änderung ein Event an alle verbundenen Clients.
//! Companion kann damit Variablen aktualisieren und Button-Feedbacks zeigen
//! (z. B. laufender Cue grün, Standby-Cue gelb, Panic rot blinken).
//!
//! ## Protokoll-Beispiel
//! ```text
//! → {"action":"go"}
//! ← {"type":"cue_started","id":"…","number":"1","name":"Intro"}
//! ← {"type":"status","show_name":"…","playhead":1,…}
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex};
use uuid::Uuid;

use super::{RemoteCommand, RemoteEvent, RemoteStatus};

/// Gemeinsamer Zustand für Companion-Server.
#[derive(Clone)]
pub struct CompanionState {
    /// Kanal für eingehende Commands.
    pub command_tx: mpsc::Sender<RemoteCommand>,
    /// Broadcast für ausgehende Events.
    pub event_tx: broadcast::Sender<RemoteEvent>,
    /// Aktueller Status (für neue Clients).
    pub status: Arc<Mutex<Option<RemoteStatus>>>,
    /// Aktive Verbindungen.
    pub connections: Arc<Mutex<HashSet<Uuid>>>,
}

/// Startet den Companion TCP-Server.
pub fn start_companion_server(
    port: u16,
) -> (
    mpsc::Receiver<RemoteCommand>,
    broadcast::Sender<RemoteEvent>,
) {
    let (command_tx, command_rx) = mpsc::channel::<RemoteCommand>(64);
    let (event_tx, _) = broadcast::channel::<RemoteEvent>(256);

    let state = CompanionState {
        command_tx,
        event_tx: event_tx.clone(),
        status: Arc::new(Mutex::new(None)),
        connections: Arc::new(Mutex::new(HashSet::new())),
    };

    // Status-Cacher: hält den zuletzt gebroadcasteten `RemoteEvent::Status`
    // für neu verbindende Clients (die App pusht den Status als Event).
    {
        let mut status_rx = event_tx.subscribe();
        let state = state.clone();
        tokio::spawn(async move {
            while let Ok(event) = status_rx.recv().await {
                if let RemoteEvent::Status(status) = event {
                    *state.status.lock().await = Some(status);
                }
            }
        });
    }

    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{port}");
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                // Port belegt o.ä. – Live-Betrieb nicht wegen Remote killen.
                tracing::error!(%addr, %e, "companion bind fehlgeschlagen");
                return;
            }
        };
        tracing::info!(%addr, "companion tcp server gestartet");
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    tracing::info!(%peer, "companion client verbunden");
                    let state = state.clone();
                    tokio::spawn(handle_client(stream, state));
                }
                Err(e) => {
                    tracing::error!(%e, "companion accept error");
                }
            }
        }
    });

    (command_rx, event_tx)
}

/// Aktualisiert den Status (von der App).
pub async fn update_status(state: &CompanionState, status: RemoteStatus) {
    *state.status.lock().await = Some(status);
}

/// Broadcastet ein Event an alle Companion-Clients.
pub fn broadcast_event(state: &CompanionState, event: RemoteEvent) {
    let _ = state.event_tx.send(event);
}

/// Behandelt eine einzelne Companion-Client-Verbindung.
async fn handle_client(stream: TcpStream, state: CompanionState) {
    let conn_id = Uuid::new_v4();
    state.connections.lock().await.insert(conn_id);

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut event_rx = state.event_tx.subscribe();

    // Initial: aktuellen Status senden.
    if let Some(status) = state.status.lock().await.as_ref() {
        let msg = serde_json::to_string(&RemoteEvent::Status(status.clone())).unwrap();
        let _ = writer.write_all(format!("{msg}\n").as_bytes()).await;
    }

    // Task: Events an Client senden.
    let send_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let json = match serde_json::to_string(&event) {
                Ok(j) => j,
                Err(_) => continue,
            };
            if writer
                .write_all(format!("{json}\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Commands vom Client empfangen.
    let cmd_tx = state.command_tx.clone();
    let recv_task = tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<RemoteCommand>(&line) {
                Ok(cmd) if !cmd.is_playback() => {
                    tracing::warn!(?cmd, "companion: nur Playback-Befehle erlaubt");
                }
                Ok(cmd) => {
                    if cmd_tx.send(cmd).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(%e, %line, "companion: ungültiges command");
                }
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    state.connections.lock().await.remove(&conn_id);
    tracing::info!(%conn_id, "companion client getrennt");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn companion_tcp_bidirectional() {
        // Server auf ephemerem Port starten.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let (mut cmd_rx, event_tx) = start_companion_server(port);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Client verbinden.
        let stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        // 1. Command senden: GO
        writer.write_all(b"{\"action\":\"go\"}\n").await.unwrap();
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, RemoteCommand::Go));

        // 2. Event empfangen (von Server gepusht).
        event_tx.send(RemoteEvent::Pong).unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        let event: RemoteEvent = serde_json::from_str(&line).unwrap();
        assert!(matches!(event, RemoteEvent::Pong));

        // 3. Weiteres Command: Panic
        writer.write_all(b"{\"action\":\"panic\"}\n").await.unwrap();
        let cmd = cmd_rx.recv().await.unwrap();
        assert!(matches!(cmd, RemoteCommand::Panic));
    }

    #[tokio::test]
    async fn companion_multiple_commands() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let (mut cmd_rx, _event_tx) = start_companion_server(port);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();

        // Mehrere Commands senden.
        let cmds = [
            r#"{"action":"go"}"#,
            r#"{"action":"pause_all"}"#,
            r#"{"action":"resume_all"}"#,
            r#"{"action":"reset"}"#,
            r#"{"action":"go_to","cue":"5.1"}"#,
            r#"{"action":"arm","cue":"5","armed":true}"#,
            r#"{"action":"set_show_mode","enabled":true}"#,
            r#"{"action":"get_status"}"#,
        ];
        for cmd in &cmds {
            stream
                .write_all(format!("{cmd}\n").as_bytes())
                .await
                .unwrap();
        }

        for _ in 0..cmds.len() {
            let cmd = cmd_rx.recv().await.unwrap();
            // Einfach prüfen, dass Commands ankommen.
            let _ = serde_json::to_string(&cmd).unwrap();
        }
    }

    #[tokio::test]
    async fn companion_invalid_json_ignored() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let (mut cmd_rx, _event_tx) = start_companion_server(port);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();

        // Ungültiges JSON + gültiges Command.
        stream.write_all(b"{ invalid json\n").await.unwrap();
        stream.write_all(b"{\"action\":\"go\"}\n").await.unwrap();

        // Nur das gültige Command sollte ankommen.
        let cmd = tokio::time::timeout(std::time::Duration::from_secs(1), cmd_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(cmd, RemoteCommand::Go));
    }
}
