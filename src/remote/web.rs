//! Web-Remote: HTTP + WebSocket Server (axum) für Browser-Fernsteuerung.
//!
//! Startet einen HTTP-Server (Default-Port 8080) mit:
//! * `GET /` – HTML-UI (eingebettet, keine externen Dateien)
//! * `POST /api/command` – JSON-[`RemoteCommand`] ausführen
//! * `GET /api/status` – aktueller [`RemoteStatus`] als JSON
//! * `POST /api/login`, `GET /api/session` – Anmeldung mit PIN
//! * `GET /api/workspace` – komplette Show ([`RemoteWorkspace`]) als JSON
//! * `WS /ws` – WebSocket für bidirektionale Echtzeit-Updates
//!
//! ## Sicherheit
//! Der Server lauscht auf `0.0.0.0` (alle Interfaces). Der Zugriff ist mit
//! einem PIN geschützt ([`WebAuth`]): Nach `POST /api/login` gilt ein Cookie,
//! falsche Eingaben sperren die Anmeldung kurz. Die Verbindung ist unverschlüsselt
//! (HTTP), der PIN schützt also nicht gegen Mitlesen im Netz. Anfragen aus fremden
//! Webseiten werden zusätzlich über den `Origin`-Header abgelehnt
//! ([`origin_allowed`]).

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

const SESSION_COOKIE: &str = "kuhplay_session";
/// Fehlversuche bis zur Sperre und deren Dauer (bremst das Durchprobieren).
const MAX_FAILURES: u32 = 5;
const LOCKOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Ergebnis einer Anmeldung.
#[derive(Debug, PartialEq, Eq)]
pub enum LoginResult {
    /// Richtig: Sitzungs-Token für das Cookie.
    Ok(String),
    Wrong,
    Locked,
}

#[derive(Default)]
struct AuthState {
    pin: Option<String>,
    sessions: HashSet<String>,
    failures: u32,
    locked_until: Option<std::time::Instant>,
}

/// Anmeldung der Web-Remote. Geteilt mit der App, die den PIN ändern kann.
#[derive(Clone, Default)]
pub struct WebAuth(Arc<std::sync::Mutex<AuthState>>);

impl WebAuth {
    pub fn new(pin: Option<String>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(AuthState {
            pin,
            ..Default::default()
        })))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, AuthState> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `true`, wenn ein PIN gesetzt ist.
    pub fn required(&self) -> bool {
        self.state().pin.is_some()
    }

    pub fn pin(&self) -> Option<String> {
        self.state().pin.clone()
    }

    /// Ändert den PIN und meldet alle Browser ab.
    pub fn set_pin(&self, pin: String) {
        let mut state = self.state();
        state.pin = Some(pin);
        state.sessions.clear();
        state.failures = 0;
        state.locked_until = None;
    }

    pub fn login(&self, pin: &str) -> LoginResult {
        let mut state = self.state();
        if state
            .locked_until
            .is_some_and(|until| std::time::Instant::now() < until)
        {
            return LoginResult::Locked;
        }
        if state.pin.as_deref() != Some(pin) {
            state.failures += 1;
            if state.failures >= MAX_FAILURES {
                state.failures = 0;
                state.locked_until = Some(std::time::Instant::now() + LOCKOUT);
            }
            return LoginResult::Wrong;
        }
        state.failures = 0;
        state.locked_until = None;
        let token = Uuid::new_v4().to_string();
        state.sessions.insert(token.clone());
        LoginResult::Ok(token)
    }

    /// Prüft das Cookie einer Anfrage; ohne PIN ist alles erlaubt.
    pub fn allowed(&self, headers: &HeaderMap) -> bool {
        let state = self.state();
        state.pin.is_none()
            || session_token(headers).is_some_and(|token| state.sessions.contains(&token))
    }

    fn session_valid(&self, token: &Option<String>) -> bool {
        let state = self.state();
        state.pin.is_none()
            || token
                .as_ref()
                .is_some_and(|token| state.sessions.contains(token))
    }
}

/// Liest das Sitzungs-Token aus dem `Cookie`-Header.
fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|cookie| {
            cookie
                .trim()
                .strip_prefix(SESSION_COOKIE)?
                .strip_prefix('=')
                .map(str::to_string)
        })
}

/// Neuer zufälliger 4-stelliger PIN.
pub fn generate_pin() -> String {
    let random = u128::from_le_bytes(*Uuid::new_v4().as_bytes());
    format!("{:04}", random % 10_000)
}

/// Gültig sind 4 bis 8 Ziffern.
pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

/// Datei mit dem PIN im Konfigurationsordner des Benutzers.
fn pin_path() -> Option<std::path::PathBuf> {
    let dir = if cfg!(target_os = "windows") {
        std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("KuhPlay")
    } else if cfg!(target_os = "macos") {
        std::path::PathBuf::from(std::env::var_os("HOME")?)
            .join("Library/Application Support/KuhPlay")
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(config).join("kuhplay")
    } else {
        std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config/kuhplay")
    };
    Some(dir.join("remote_pin"))
}

/// Speichert den PIN, unter Unix nur für den Benutzer lesbar.
pub fn save_pin(pin: &str) -> std::io::Result<()> {
    let path = pin_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, pin.as_bytes())
}

/// Liest den gespeicherten PIN oder legt beim ersten Start einen neuen an.
pub fn load_or_create_pin() -> String {
    if let Some(pin) = pin_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|pin| pin.trim().to_string())
        .filter(|pin| valid_pin(pin))
    {
        return pin;
    }
    let pin = generate_pin();
    if let Err(e) = save_pin(&pin) {
        tracing::warn!(%e, "remote-PIN konnte nicht gespeichert werden");
    }
    pin
}

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
    /// Anmeldung mit PIN.
    pub auth: WebAuth,
}

/// Startet den Web-Remote Server ohne PIN (blockiert nicht – tokio::spawn).
pub fn start_web_remote(
    port: u16,
) -> (
    mpsc::Receiver<RemoteCommand>,
    broadcast::Sender<RemoteEvent>,
) {
    start_web_remote_with_auth(port, WebAuth::default())
}

/// Startet den Web-Remote Server mit Anmeldung (blockiert nicht).
pub fn start_web_remote_with_auth(
    port: u16,
    auth: WebAuth,
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
        auth,
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
        .route("/api/login", post(handle_login))
        .route("/api/session", get(handle_session))
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

/// Anfrage der Anmeldung.
#[derive(serde::Deserialize)]
struct LoginRequest {
    pin: String,
}

async fn handle_login(
    State(state): State<WebRemoteState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.auth.login(&request.pin) {
        LoginResult::Ok(token) => (
            [(
                header::SET_COOKIE,
                format!("{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/"),
            )],
            Json(serde_json::json!({"ok": true})),
        )
            .into_response(),
        LoginResult::Wrong => StatusCode::UNAUTHORIZED.into_response(),
        LoginResult::Locked => StatusCode::TOO_MANY_REQUESTS.into_response(),
    }
}

async fn handle_session(State(state): State<WebRemoteState>, headers: HeaderMap) -> Response {
    Json(serde_json::json!({ "authenticated": state.auth.allowed(&headers) })).into_response()
}

/// Bearbeitet ein Command aus dem Browser (Datei-Zugriff nur mit PIN).
async fn accept_command(state: &WebRemoteState, cmd: RemoteCommand) -> bool {
    if cmd.accesses_files() && !state.auth.required() {
        let _ = state.event_tx.send(RemoteEvent::Error {
            message: "Opening and saving files needs a remote PIN.".to_string(),
        });
        return true;
    }
    state.command_tx.send(cmd).await.is_ok()
}

async fn handle_workspace(State(state): State<WebRemoteState>, headers: HeaderMap) -> Response {
    if !state.auth.allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
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
    if !state.auth.allowed(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"ok": false, "error": "pin required"})),
        );
    }
    match accept_command(&state, cmd).await {
        true => (StatusCode::OK, Json(serde_json::json!({"ok": true}))),
        false => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"ok": false, "error": "command channel closed"})),
        ),
    }
}

async fn handle_status(State(state): State<WebRemoteState>, headers: HeaderMap) -> Response {
    if !state.auth.allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
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
    if !state.auth.allowed(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let token = session_token(&headers);
    ws.on_upgrade(move |socket| handle_socket(socket, state, token))
}

async fn handle_socket(mut socket: WebSocket, state: WebRemoteState, token: Option<String>) {
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
        // PIN geändert: Sitzung ist ungültig, Verbindung beenden.
        if !state.auth.session_valid(&token) {
            break;
        }
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
                            if !accept_command(&state, cmd).await {
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
            auth: WebAuth::default(),
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

    #[test]
    fn login_locks_after_repeated_wrong_pins_and_pin_change_ends_sessions() {
        let auth = WebAuth::new(Some("1234".to_string()));
        let LoginResult::Ok(token) = auth.login("1234") else {
            panic!("richtiger PIN");
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("other=1; {SESSION_COOKIE}={token}")
                .parse()
                .unwrap(),
        );
        assert!(auth.allowed(&headers));
        assert!(!auth.allowed(&HeaderMap::new()));

        for _ in 0..MAX_FAILURES {
            assert_eq!(auth.login("0000"), LoginResult::Wrong);
        }
        assert_eq!(
            auth.login("1234"),
            LoginResult::Locked,
            "auch richtig gesperrt"
        );
        assert!(auth.allowed(&headers), "bestehende Sitzung bleibt");

        auth.set_pin("5678".to_string());
        assert!(!auth.allowed(&headers), "neuer PIN meldet alle ab");
        assert!(matches!(auth.login("5678"), LoginResult::Ok(_)));
    }

    #[test]
    fn pin_rules() {
        assert!(valid_pin(&generate_pin()));
        assert!(valid_pin("12345678"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("123456789"));
        assert!(!valid_pin("12a4"));
    }
}
