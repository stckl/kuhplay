//! WebView-Backend für HTML5-Cues: Subprocess `kuhplay-webview` (tao+wry,
//! eigenes OS-Fenster pro Cue) mit **JSON-lines IPC über stdio** – eine
//! JSON-Message pro Zeile.
//!
//! ## Protokoll
//!
//! App → WebView (stdin):
//! * `{"load": <HtmlSource>}` – Quelle laden (File/Url/Inline, siehe
//!   [`crate::core::HtmlSource`])
//! * `{"eval": "<js>"}` – JavaScript ausführen
//! * `{"push_json": <payload>}` – JSON an `window.onShowControlData` pushen
//! * `{"snapshot": {"width": W, "height": H}}` – Frame-Snapshot anfordern
//!   (W×H = Wunschgröße, das WebView antwortet mit der tatsächlichen Größe)
//! * `{"close": true}` – Fenster schließen
//!
//! WebView → App (stdout):
//! * `{"loaded": true}` / `{"js_executed": true}` / `{"json_received": true}`
//! * `{"frame": {"width": W, "height": H, "rgba_b64": "…"}}` – RGBA-Snapshot
//!   (Row-Major, 4 Bytes/Pixel, base64) für das Stage-Compositing
//! * `{"error": "<msg>"}`
//!
//! Die Engine nutzt dieses Backend, wenn das Binary auffindbar ist
//! ([`webview_binary`]), sonst den virtuellen Fallback (siehe
//! `real_engine.rs`). Das Binary liegt hinter dem Cargo-Feature `webview`
//! (`src/bin/kuhplay-webview.rs`) – die Engine selbst braucht kein wry.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crossbeam_channel::Receiver;
use uuid::Uuid;

use crate::core::HtmlSource;

/// Events vom WebView-Subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebviewEvent {
    /// `{"loaded": true}` – Load-Command akzeptiert.
    Loaded,
    /// `{"js_executed": true}` – `eval` ausgeführt.
    JsExecuted,
    /// `{"done": true}` – Script-Cue meldet sich fertig (window.kuhplay.done()).
    Done,
    /// `{"json_received": true}` – `push_json` zugestellt.
    JsonReceived,
    /// `{"frame": {…}}` – RGBA-Snapshot für das Stage-Compositing
    /// (Row-Major, `width * height * 4` Bytes, Alpha = 255).
    Frame {
        /// Breite des Frames in Pixeln.
        width: u32,
        /// Höhe des Frames in Pixeln.
        height: u32,
        /// RGBA-Puffer (base64-dekodiert, Länge geprüft).
        rgba: Vec<u8>,
    },
    /// `{"error": "…"}` – Fehler im Subprocess.
    Error(String),
}

/// Eine laufende WebView-Instanz (ein Subprocess pro Cue).
pub struct WebviewInstance {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<WebviewEvent>,
    _reader: std::thread::JoinHandle<()>,
}

impl WebviewInstance {
    /// Startet den WebView-Subprocess und lädt die Quelle.
    pub fn spawn(binary: &Path, cue_id: Uuid, source: &HtmlSource) -> std::io::Result<Self> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin ist gepiped");
        let stdout = child.stdout.take().expect("stdout ist gepiped");
        // Reader-Thread: stdout-Zeilen → Events (App pollt non-blocking).
        let (tx, rx) = crossbeam_channel::unbounded();
        let reader = std::thread::Builder::new()
            .name(format!("kuhplay-webview-rx-{cue_id}"))
            .spawn(move || {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(Ok(line)) = lines.next() {
                    if let Some(event) = parse_event_line(&line) {
                        if tx.send(event).is_err() {
                            break; // Receiver weg → Thread endet
                        }
                    }
                }
            })?;
        let mut instance = Self {
            child,
            stdin,
            rx,
            _reader: reader,
        };
        instance.send_json(&serde_json::json!({ "load": source }))?;
        Ok(instance)
    }

    /// Sendet eine JSON-Message (eine Zeile) an den Subprocess.
    fn send_json(&mut self, value: &serde_json::Value) -> std::io::Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()
    }

    /// Führt JavaScript in der WebView aus (`{"eval": …}`).
    pub fn eval_js(&mut self, script: &str) -> std::io::Result<()> {
        self.send_json(&serde_json::json!({ "eval": script }))
    }

    /// Pusht einen JSON-Payload an `window.onShowControlData`.
    pub fn push_json(&mut self, payload: &serde_json::Value) -> std::io::Result<()> {
        self.send_json(&serde_json::json!({ "push_json": payload }))
    }

    /// Fordert einen Frame-Snapshot an (`{"snapshot": {"width": W, "height": H}}`).
    /// Die Antwort kommt als [`WebviewEvent::Frame`] (mit der tatsächlichen
    /// Größe des WebViews) asynchron über [`WebviewInstance::poll_event`].
    pub fn request_snapshot(&mut self, width: u32, height: u32) -> std::io::Result<()> {
        self.send_json(&serde_json::json!({ "snapshot": { "width": width, "height": height } }))
    }

    /// Non-blocking Poll der Events vom Subprocess.
    pub fn poll_event(&self) -> Option<WebviewEvent> {
        self.rx.try_recv().ok()
    }

    /// Beendet den Subprocess (Drop killt ebenfalls).
    pub fn close(mut self) {
        let _ = self.send_json(&serde_json::json!({ "close": true }));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for WebviewInstance {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Parst eine Event-Zeile vom Subprocess. `None` bei unbekannten Messages
/// oder eingehenden Commands (z. B. Echo eines `cat`-Testdoubles).
pub fn parse_event_line(line: &str) -> Option<WebviewEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("loaded").and_then(|v| v.as_bool()) == Some(true) {
        return Some(WebviewEvent::Loaded);
    }
    if value.get("js_executed").and_then(|v| v.as_bool()) == Some(true) {
        return Some(WebviewEvent::JsExecuted);
    }
    if value.get("done").and_then(|v| v.as_bool()) == Some(true) {
        return Some(WebviewEvent::Done);
    }
    if value.get("json_received").and_then(|v| v.as_bool()) == Some(true) {
        return Some(WebviewEvent::JsonReceived);
    }
    if let Some(frame) = value.get("frame") {
        return parse_frame_event(frame);
    }
    if let Some(msg) = value.get("error").and_then(|v| v.as_str()) {
        return Some(WebviewEvent::Error(msg.to_string()));
    }
    None
}

/// Parst das `frame`-Objekt einer Event-Zeile. `None` bei fehlenden Feldern,
/// ungültigem Base64 oder Pufferlänge ≠ `width * height * 4`.
fn parse_frame_event(frame: &serde_json::Value) -> Option<WebviewEvent> {
    let width = frame.get("width")?.as_u64()? as u32;
    let height = frame.get("height")?.as_u64()? as u32;
    let b64 = frame.get("rgba_b64")?.as_str()?;
    use base64::Engine;
    let rgba = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if rgba.len() != width as usize * height as usize * 4 {
        return None;
    }
    Some(WebviewEvent::Frame {
        width,
        height,
        rgba,
    })
}

/// Pfad des WebView-Binarys: `KUHPLAY_WEBVIEW_BIN` → Datei neben dem
/// aktuellen Executable (`kuhplay-webview`). `None` = nicht gebaut/installiert
/// → virtueller Fallback.
pub fn webview_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KUHPLAY_WEBVIEW_BIN") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let exe = std::env::current_exe().ok()?;
    webview_sibling_paths(exe.parent()?)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Kandidaten-Pfade für das WebView-Binary neben dem Hauptprogramm
/// (Windows zusätzlich `kuhplay-webview.exe`, da `is_file` keine Endung anhängt).
fn webview_sibling_paths(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    return vec![dir.join("kuhplay-webview"), dir.join("kuhplay-webview.exe")];
    #[cfg(not(target_os = "windows"))]
    vec![dir.join("kuhplay-webview")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_paths_include_windows_exe_variant() {
        let candidates = webview_sibling_paths(std::path::Path::new("/app"));
        assert!(candidates.contains(&std::path::PathBuf::from("/app/kuhplay-webview")));
        #[cfg(target_os = "windows")]
        assert!(candidates.contains(&std::path::PathBuf::from("/app/kuhplay-webview.exe")));
    }

    #[test]
    fn parse_event_line_events() {
        assert_eq!(
            parse_event_line(r#"{"loaded":true}"#),
            Some(WebviewEvent::Loaded)
        );
        assert_eq!(
            parse_event_line(r#"{"js_executed":true}"#),
            Some(WebviewEvent::JsExecuted)
        );
        assert_eq!(
            parse_event_line(r#"{"json_received":true}"#),
            Some(WebviewEvent::JsonReceived)
        );
        assert_eq!(
            parse_event_line(r#"{"error":"boom"}"#),
            Some(WebviewEvent::Error("boom".to_string()))
        );
    }

    #[test]
    fn parse_event_line_ignores_unknown_and_in_messages() {
        // Echo eines `cat`-Testdoubles (eingehende Commands) → kein Event.
        assert_eq!(parse_event_line(r#"{"eval":"1+1"}"#), None);
        assert_eq!(parse_event_line(r#"{"load":{"File":"/tmp/a.html"}}"#), None);
        // Kaputt / keine JSON-Object-Zeile.
        assert_eq!(parse_event_line("not json"), None);
        assert_eq!(parse_event_line(r#"{"loaded":false}"#), None);
        assert_eq!(parse_event_line(r#"{"other":1}"#), None);
    }

    #[test]
    fn parse_event_line_frame() {
        use base64::Engine;
        // 2×2 RGBA = 16 Bytes: rot, grün, blau, weiß.
        let rgba = [
            255, 0, 0, 255, // rot
            0, 255, 0, 255, // grün
            0, 0, 255, 255, // blau
            255, 255, 255, 255, // weiß
        ];
        let b64 = base64::engine::general_purpose::STANDARD.encode(rgba);
        let line = format!(r#"{{"frame":{{"width":2,"height":2,"rgba_b64":"{b64}"}}}}"#);
        assert_eq!(
            parse_event_line(&line),
            Some(WebviewEvent::Frame {
                width: 2,
                height: 2,
                rgba: rgba.to_vec(),
            })
        );
    }

    #[test]
    fn parse_event_line_frame_rejects_malformed() {
        // Falsche Pufferlänge (1×1 statt 2×2).
        assert_eq!(
            parse_event_line(r#"{"frame":{"width":2,"height":2,"rgba_b64":"AAAA"}}"#),
            None
        );
        // Ungültiges Base64.
        assert_eq!(
            parse_event_line(r#"{"frame":{"width":1,"height":1,"rgba_b64":"!!!"}}"#),
            None
        );
        // Fehlende Felder.
        assert_eq!(parse_event_line(r#"{"frame":{"width":1}}"#), None);
        assert_eq!(parse_event_line(r#"{"frame":{}}"#), None);
    }
}
