//! Renderer-Backend für HTML5-Cues: Subprocess `kuhplay-cef` (CEF mit
//! Offscreen-Rendering, ein Prozess pro Cue) mit **JSON-lines IPC über
//! stdio** – eine JSON-Message pro Zeile.
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
//! ([`cef_binary`]), sonst den virtuellen Fallback (siehe
//! `real_engine.rs`). Das Binary liegt hinter dem Cargo-Feature `cef`
//! (`src/bin/kuhplay-cef.rs`) – die Engine selbst linkt kein CEF.

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
    /// `{"trigger_done": "<name>"}` – Trigger-Handler der Seite ist fertig
    /// (auch nach einem zurückgegebenen Promise).
    TriggerDone(String),
    /// `{"error": "…"}` – Fehler im Subprocess.
    Error(String),
}

/// Eine laufende WebView-Instanz (ein Subprocess pro Cue).
pub struct WebviewInstance {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<WebviewEvent>,
    _reader: std::thread::JoinHandle<()>,
    /// Einmalig gesetzt, wenn der Subprocess gestorben ist (damit genau ein
    /// Fehler-Event gemeldet wird, danach nur noch None).
    death_reported: bool,
    /// Shared-Memory-Datei der Frames (beim Drop gelöscht).
    shm_path: std::path::PathBuf,
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
        // Frames kommen über Shared Memory (siehe frame_shm); über die Pipe
        // nur die Meldung, welcher Slot fertig ist.
        let shm_path = std::env::temp_dir().join(format!(
            "kuhplay-html-{}-{cue_id}.frames",
            std::process::id()
        ));
        // Reader-Thread: stdout-Zeilen → Events (App pollt non-blocking); er
        // kopiert gemeldete Bilder gleich aus dem Shared Memory.
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut shm = super::frame_shm::FrameShmReader::new(shm_path.clone());
        let reader = std::thread::Builder::new()
            .name(format!("kuhplay-webview-rx-{cue_id}"))
            .spawn(move || {
                let mut lines = BufReader::new(stdout).lines();
                while let Some(Ok(line)) = lines.next() {
                    let event = match parse_shm_frame(&line) {
                        Some((width, height, slot)) => {
                            shm.read(width, height, slot)
                                .map(|rgba| WebviewEvent::Frame {
                                    width,
                                    height,
                                    rgba,
                                })
                        }
                        None => parse_event_line(&line),
                    };
                    if let Some(event) = event {
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
            death_reported: false,
            shm_path,
        };
        let path = instance.shm_path.to_string_lossy().into_owned();
        // Stirbt der Prozess sofort (z. B. CEF-Runtime fehlt), schlägt das
        // Schreiben fehl – das meldet poll_event dann genau einmal als Fehler.
        let sent = instance
            .send_json(&serde_json::json!({ "shm": { "path": path } }))
            .and_then(|()| instance.send_json(&serde_json::json!({ "load": source })));
        if let Err(err) = sent {
            if err.kind() != std::io::ErrorKind::BrokenPipe {
                return Err(err);
            }
        }
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

    /// Löst einen Template-Trigger aus (`{"trigger": {"name", "payload"}}`,
    /// z. B. `in`/`out`); die Seite meldet [`WebviewEvent::TriggerDone`].
    pub fn trigger(&mut self, name: &str, payload: serde_json::Value) -> std::io::Result<()> {
        self.send_json(&serde_json::json!({ "trigger": { "name": name, "payload": payload } }))
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

    /// Non-blocking Poll der Events vom Subprocess. Ist der Subprocess
    /// gestorben (z. B. CEF-Runtime fehlt), kommt genau einmal ein
    /// [`WebviewEvent::Error`] – sonst würde der Cue ewig schwarz bleiben.
    pub fn poll_event(&mut self) -> Option<WebviewEvent> {
        if let Ok(event) = self.rx.try_recv() {
            return Some(event);
        }
        if !self.death_reported {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.death_reported = true;
                return Some(WebviewEvent::Error(format!(
                    "cef subprocess beendet (status: {status}) – CEF-Runtime fehlt oder startet nicht (CEF_PATH / .app-Layout prüfen)"
                )));
            }
        }
        None
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
        let _ = std::fs::remove_file(&self.shm_path);
    }
}

/// `{"frame_shm":{"width","height","slot",…}}` → (Breite, Höhe, Slot).
fn parse_shm_frame(line: &str) -> Option<(u32, u32, u32)> {
    if !line.contains("\"frame_shm\"") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let frame = value.get("frame_shm")?;
    let get = |key: &str| frame.get(key)?.as_u64().map(|v| v as u32);
    Some((get("width")?, get("height")?, get("slot")?))
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
    if let Some(name) = value.get("trigger_done").and_then(|v| v.as_str()) {
        return Some(WebviewEvent::TriggerDone(name.to_string()));
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

/// Pfad des CEF-Renderer-Binarys (einziger HTML5-Renderer): `KUHPLAY_CEF_BIN`
/// (Override), sonst der erste vorhandene Kandidat aus [`cef_sibling_paths`].
/// `None` = nicht gebaut (z. B. Build ohne Feature „cef") → virtueller
/// Fallback in der Engine.
pub fn cef_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KUHPLAY_CEF_BIN") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let exe = std::env::current_exe().ok()?;
    cef_sibling_paths(exe.parent()?)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Kandidaten für den Renderer relativ zum Ordner des Hauptprogramms
/// (`kuhplay-cef-bundle` legt ihn samt CEF-Runtime in einen Ordner `cef/`).
///
/// * macOS: `cef/kuhplay-cef.app` daneben (Entwicklung: `target/<profil>/`),
///   im fertigen KuhPlay.app unter `Contents/Helpers/` bzw.
///   `Contents/Resources/cef/` (Installer); zuletzt das nackte Binary – es
///   meldet dann selbst, dass das Framework fehlt.
/// * Windows/Linux: `cef/kuhplay-cef` daneben; Linux-Paket zusätzlich unter
///   `../lib/kuhplay/cef/`; zuletzt direkt daneben.
fn cef_sibling_paths(dir: &std::path::Path) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    return vec![
        dir.join("cef/kuhplay-cef.app/Contents/MacOS/kuhplay-cef"),
        dir.join("../Helpers/kuhplay-cef.app/Contents/MacOS/kuhplay-cef"),
        dir.join("../Resources/cef/kuhplay-cef.app/Contents/MacOS/kuhplay-cef"),
        dir.join("kuhplay-cef"),
    ];
    #[cfg(target_os = "windows")]
    return vec![
        dir.join("cef/kuhplay-cef.exe"),
        dir.join("kuhplay-cef.exe"),
        dir.join("kuhplay-cef"),
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    vec![
        dir.join("cef/kuhplay-cef"),
        dir.join("../lib/kuhplay/cef/kuhplay-cef"),
        dir.join("../lib/KuhPlay/cef/kuhplay-cef"),
        dir.join("kuhplay-cef"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cef_sibling_paths_include_windows_exe_variant() {
        let candidates = cef_sibling_paths(std::path::Path::new("/app"));
        assert!(candidates.contains(&std::path::PathBuf::from("/app/kuhplay-cef")));
        #[cfg(target_os = "windows")]
        assert!(candidates.contains(&std::path::PathBuf::from("/app/kuhplay-cef.exe")));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_prefers_the_app_bundle() {
        let candidates = cef_sibling_paths(std::path::Path::new("/x/MacOS"));
        assert_eq!(
            candidates[0],
            PathBuf::from("/x/MacOS/cef/kuhplay-cef.app/Contents/MacOS/kuhplay-cef")
        );
        assert!(candidates.contains(&PathBuf::from(
            "/x/MacOS/../Helpers/kuhplay-cef.app/Contents/MacOS/kuhplay-cef"
        )));
    }

    #[test]
    fn shm_frame_line_is_recognised() {
        assert_eq!(
            parse_shm_frame(r#"{"frame_shm":{"width":4,"height":2,"slot":1,"seq":7}}"#),
            Some((4, 2, 1))
        );
        assert_eq!(parse_shm_frame(r#"{"loaded":true}"#), None);
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
