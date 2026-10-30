//! Web-Remote: HTTP + WebSocket Server (axum) für Browser-Fernsteuerung.
//!
//! Startet einen HTTP-Server (Default-Port 8080) mit:
//! * `GET /` – HTML-UI (eingebettet, keine externen Dateien)
//! * `POST /api/command` – JSON-[`RemoteCommand`] ausführen
//! * `GET /api/status` – aktueller [`RemoteStatus`] als JSON
//! * `GET /api/workspace` – komplette Show ([`RemoteWorkspace`]) als JSON
//! * `WS /ws` – WebSocket für bidirektionale Echtzeit-Updates
//!
//! ## Sicherheit
//! Der Server lauscht auf `0.0.0.0` (alle Interfaces) und hat **keine
//! Anmeldung**: Wer ihn im Netz erreicht, kann die Show bearbeiten. Er gehört
//! also in ein vertrauenswürdiges Show-Netz; `KUHPLAY_REMOTE_WEB=off` schaltet
//! ihn ab. Anfragen aus fremden Webseiten werden über den `Origin`-Header
//! abgelehnt ([`origin_allowed`]), sonst könnte jede im Browser geöffnete Seite
//! die Show steuern.

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use tokio::sync::{broadcast, mpsc, Mutex};
use uuid::Uuid;

use super::{RemoteCommand, RemoteEvent, RemoteStatus, RemoteWorkspace};

/// Gemeinsamer Zustand für alle Handler.
#[derive(Clone)]
pub struct WebRemoteState {
    /// Kanal für eingehende Commands (an die App).
    pub command_tx: mpsc::Sender<RemoteCommand>,
    /// Broadcast-Kanal für ausgehende Events (an alle WebSocket-Clients).
    pub event_tx: broadcast::Sender<RemoteEvent>,
    /// Zuletzt bekannter Status (für neue Clients + HTTP-API).
    pub status: Arc<Mutex<Option<RemoteStatus>>>,
    /// Zuletzt bekannte Show (für neue Clients + HTTP-API).
    pub workspace: Arc<Mutex<Option<Box<RemoteWorkspace>>>>,
    /// Aktive WebSocket-Verbindungen (für Cleanup).
    pub connections: Arc<Mutex<HashSet<Uuid>>>,
}

/// Startet den Web-Remote Server (blockiert nicht – tokio::spawn).
pub fn start_web_remote(
    port: u16,
) -> (
    mpsc::Receiver<RemoteCommand>,
    broadcast::Sender<RemoteEvent>,
) {
    let (command_tx, command_rx) = mpsc::channel::<RemoteCommand>(64);
    let (event_tx, _) = broadcast::channel::<RemoteEvent>(256);

    let state = WebRemoteState {
        command_tx,
        event_tx: event_tx.clone(),
        status: Arc::new(Mutex::new(None)),
        workspace: Arc::new(Mutex::new(None)),
        connections: Arc::new(Mutex::new(HashSet::new())),
    };

    // Status-Cacher: hält den zuletzt gebroadcasteten `RemoteEvent::Status`
    // für `GET /api/status` und neue WebSocket-Clients. Die App pusht den
    // Status regelmäßig als Event – hier wird er nur zwischengespeichert.
    {
        let mut status_rx = event_tx.subscribe();
        let state = state.clone();
        tokio::spawn(async move {
            while let Ok(event) = status_rx.recv().await {
                match event {
                    RemoteEvent::Status(status) => *state.status.lock().await = Some(status),
                    RemoteEvent::Workspace(workspace) => {
                        *state.workspace.lock().await = Some(workspace)
                    }
                    _ => {}
                }
            }
        });
    }

    let app = Router::new()
        .route("/", get(index_html))
        .route("/api/command", post(handle_command))
        .route("/api/status", get(handle_status))
        .route("/api/workspace", get(handle_workspace))
        .route("/ws", get(ws_handler))
        .with_state(state);

    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{port}");
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                // Port belegt o.ä. – Live-Betrieb nicht wegen Remote killen.
                tracing::error!(%addr, %e, "web-remote bind fehlgeschlagen");
                return;
            }
        };
        tracing::info!(%addr, "web-remote server gestartet");
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(%e, "web-remote serve beendet mit fehler");
        }
    });

    (command_rx, event_tx)
}

/// Aktualisiert den gespeicherten Status (von der App aufgerufen).
pub async fn update_status(state: &WebRemoteState, status: RemoteStatus) {
    *state.status.lock().await = Some(status);
}

/// Sendet ein Event an alle WebSocket-Clients.
pub fn broadcast_event(state: &WebRemoteState, event: RemoteEvent) {
    let _ = state.event_tx.send(event);
}

// ---------------------------------------------------------------------
// HTTP-Handler
// ---------------------------------------------------------------------

async fn index_html() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// `false`, wenn die Anfrage aus einer fremden Webseite stammt: Der Browser
/// setzt dann einen `Origin`, der nicht zum angesprochenen Host passt. Ohne
/// `Origin` (curl, Companion-Skripte) ist die Anfrage keine fremde Webseite.
pub fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    let origin_host = origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://"))
        .map(|(_, rest)| rest);
    host.is_some() && host == origin_host
}

async fn handle_workspace(State(state): State<WebRemoteState>) -> Response {
    match state.workspace.lock().await.as_ref() {
        Some(workspace) => (StatusCode::OK, Json(workspace.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no workspace yet"})),
        )
            .into_response(),
    }
}

async fn handle_command(
    State(state): State<WebRemoteState>,
    headers: HeaderMap,
    Json(cmd): Json<RemoteCommand>,
) -> impl IntoResponse {
    if !origin_allowed(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"ok": false, "error": "foreign origin"})),
        );
    }
    match state.command_tx.send(cmd).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"ok": false, "error": "command channel closed"})),
        ),
    }
}

async fn handle_status(State(state): State<WebRemoteState>) -> Response {
    let status = state.status.lock().await;
    match status.as_ref() {
        Some(s) => (StatusCode::OK, Json(s.clone())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no status yet"})),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------
// WebSocket-Handler
// ---------------------------------------------------------------------

async fn ws_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<WebRemoteState>,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: WebRemoteState) {
    let conn_id = Uuid::new_v4();
    state.connections.lock().await.insert(conn_id);

    // Initial: aktuellen Status senden (falls vorhanden).
    if let Some(status) = state.status.lock().await.clone() {
        let msg = serde_json::to_string(&RemoteEvent::Status(status)).unwrap();
        let _ = socket.send(Message::Text(msg.into())).await;
    }

    if let Some(workspace) = state.workspace.lock().await.clone() {
        let msg = serde_json::to_string(&RemoteEvent::Workspace(workspace)).unwrap();
        let _ = socket.send(Message::Text(msg.into())).await;
    }

    // Eine Schleife per select!: Events an den Client senden + Commands
    // vom Client empfangen (axum-WebSocket hat eigene recv/send-Methoden,
    // daher kein futures_util::StreamExt/split nötig).
    let mut event_rx = state.event_tx.subscribe();
    loop {
        tokio::select! {
            event = event_rx.recv() => match event {
                Ok(event) => {
                    let json = match serde_json::to_string(&event) {
                        Ok(j) => j,
                        Err(_) => continue,
                    };
                    if socket.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<RemoteCommand>(&text) {
                        Ok(cmd) => {
                            if state.command_tx.send(cmd).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => tracing::warn!(%e, "web-remote: ungültiges command"),
                    }
                }
                // Close, Verbindungsfehler oder Stream-Ende → trennen.
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
        }
    }

    state.connections.lock().await.remove(&conn_id);
}

// ---------------------------------------------------------------------
// Eingebettete HTML-UI (kein externes File nötig)
// ---------------------------------------------------------------------

const INDEX_HTML: &str = include_str!("web_ui.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_html_contains_ui_elements() {
        assert!(INDEX_HTML.contains("GO"));
        assert!(INDEX_HTML.contains("Panic"));
        assert!(INDEX_HTML.contains("/ws"));
        assert!(INDEX_HTML.contains("/api/command"));
    }

    #[tokio::test]
    async fn web_remote_state_creation() {
        let (cmd_tx, _) = mpsc::channel(8);
        let (event_tx, _) = broadcast::channel(8);
        let state = WebRemoteState {
            command_tx: cmd_tx,
            event_tx,
            status: Arc::new(Mutex::new(None)),
            workspace: Arc::new(Mutex::new(None)),
            connections: Arc::new(Mutex::new(HashSet::new())),
        };
        // Status update + broadcast testen.
        let status = RemoteStatus {
            show_name: "Test".to_string(),
            show_mode: false,
            active_cart: 0,
            active_cart_name: "Cart 1".to_string(),
            playhead: 0,
            standby: None,
            running: vec![],
            carts: vec![],
            cues: vec![],
        };
        update_status(&state, status).await;
        broadcast_event(&state, RemoteEvent::Pong);
        assert!(state.status.lock().await.is_some());
    }

    #[test]
    fn origin_check_blocks_foreign_sites() {
        fn headers(origin: Option<&str>) -> HeaderMap {
            let mut map = HeaderMap::new();
            map.insert(header::HOST, "show.local:8080".parse().unwrap());
            if let Some(origin) = origin {
                map.insert(header::ORIGIN, origin.parse().unwrap());
            }
            map
        }
        assert!(origin_allowed(&headers(None)), "curl hat keinen Origin");
        assert!(origin_allowed(&headers(Some("http://show.local:8080"))));
        assert!(!origin_allowed(&headers(Some("https://evil.example"))));
        assert!(!origin_allowed(&headers(Some("http://show.local:9999"))));
        assert!(!origin_allowed(&headers(Some("null"))));
    }
}
