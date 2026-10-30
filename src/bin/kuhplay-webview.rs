//! KuhPlay WebView-Subprocess (tao + wry) für HTML5-Cues.
//!
//! Ein OS-Fenster pro Prozess; **JSON-lines IPC über stdin/stdout** – das
//! Protokoll ist in `src/engine/webview_backend.rs` dokumentiert.
//!
//! Bewusst ein eigener Prozess: tao braucht auf macOS den Main-Thread, den
//! die eframe-App belegt. Die Engine startet pro HTML5-Cue eine Instanz
//! (`WebviewInstance::spawn`) und fällt auf einen virtuellen Fallback zurück,
//! wenn dieses Binary nicht auffindbar ist.
//!
//! **Compositing:** Auf `{"snapshot": …}` antwortet der Subprocess mit
//! `{"frame": {…}}` (RGBA, base64) – macOS nativ über WKWebView
//! `takeSnapshot`, auf anderen Plattformen kooperativ über das erste
//! Page-Canvas (`canvas[data-kuhplay-capture]` bevorzugt). Die Engine legt
//! die Frames in ihren Frame-Store → Stage-Compositor.
//!
//! Bauen: `cargo build --features webview` (Binary: `kuhplay-webview`).

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::thread;

use base64::Engine;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::{WebView, WebViewBuilder};

use kuhplay::core::HtmlSource;

/// JS-Bridge, die in jede geladene Seite injiziert wird
/// (Initialisierungsscript – läuft vor dem Seiten-JS).
const BRIDGE_JS: &str = r#"
window.kuhplay = window.kuhplay || {
    version: "1",
    // Wird vom Show-Operator gesetzt; PushJson ruft ihn mit dem Payload auf.
    onShowControlData: null,
};
"#;

fn main() {
    // stdin wird auf einem eigenen Thread gelesen (der Main-Thread gehört
    // der tao-Event-Loop); die Zeilen landen als Strings im Channel.
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let event_loop = EventLoop::new();
    // Offscreen-Fenster: sichtbar (sonst throttled macOS rAF/JS-Timer der
    // WebView → Standbild), aber außerhalb jedes Screens — es erscheint nicht
    // auf dem Desktop. Der Snapshot geht per WKWebView takeSnapshot in die Stage.
    let window = WindowBuilder::new()
        .with_title("KuhPlay WebView")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 720.0))
        .with_position(tao::dpi::Position::new(tao::dpi::PhysicalPosition::new(
            -10000, -10000,
        )))
        .with_decorations(false)
        .with_resizable(false)
        .build(&event_loop)
        .expect("webview window must build");
    let webview = WebViewBuilder::new()
        .with_initialization_script(BRIDGE_JS)
        .build(&window)
        .expect("webview must build");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        // Alle pending stdin-Nachrichten verarbeiten.
        let mut close_requested = false;
        while let Ok(line) = rx.try_recv() {
            if handle_message(&line, &webview, &window) {
                close_requested = true;
            }
        }
        if close_requested {
            *control_flow = ControlFlow::Exit;
            return;
        }
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// Verarbeitet eine IPC-Zeile. Rückgabe: `true` = Close gewünscht.
fn handle_message(line: &str, webview: &WebView, window: &tao::window::Window) -> bool {
    let message: serde_json::Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(_) => {
            emit(&serde_json::json!({ "error": format!("invalid json line: {line}") }));
            return false;
        }
    };
    if message.get("close").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if let Some(source) = message.get("load") {
        match serde_json::from_value::<HtmlSource>(source.clone()) {
            Ok(HtmlSource::File(path)) => {
                let url = format!("file://{}", path.display());
                match webview.load_url(&url) {
                    Ok(()) => emit(&serde_json::json!({ "loaded": true })),
                    Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
                }
            }
            Ok(HtmlSource::Url(url)) => match webview.load_url(&url) {
                Ok(()) => emit(&serde_json::json!({ "loaded": true })),
                Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
            },
            Ok(HtmlSource::Inline(html)) => match webview.load_html(&html) {
                Ok(()) => emit(&serde_json::json!({ "loaded": true })),
                Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
            },
            Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
        }
        return false;
    }
    if let Some(script) = message.get("eval").and_then(|v| v.as_str()) {
        match webview.evaluate_script(script) {
            Ok(()) => emit(&serde_json::json!({ "js_executed": true })),
            Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
        }
        return false;
    }
    if let Some(payload) = message.get("push_json") {
        // Bridge-Aufruf: Payload an window.kuhplay.onShowControlData übergeben.
        let script = format!(
            "window.kuhplay && window.kuhplay.onShowControlData \
             && window.kuhplay.onShowControlData({payload})"
        );
        match webview.evaluate_script(&script) {
            Ok(()) => emit(&serde_json::json!({ "json_received": true })),
            Err(err) => emit(&serde_json::json!({ "error": err.to_string() })),
        }
        return false;
    }
    if let Some(snap) = message.get("snapshot") {
        let width = snap.get("width").and_then(|v| v.as_u64()).unwrap_or(1280) as u32;
        let height = snap.get("height").and_then(|v| v.as_u64()).unwrap_or(720) as u32;
        handle_snapshot(webview, window, width, height);
        return false;
    }
    emit(&serde_json::json!({ "error": format!("unknown message: {line}") }));
    false
}

/// Verarbeitet einen Snapshot-Request. macOS: nativ über WKWebView
/// `takeSnapshot` (sichtbarer Bereich in WebView-Größe); andere Plattformen:
/// kooperativ über ein Page-Canvas in der Wunschgröße.
fn handle_snapshot(webview: &WebView, window: &tao::window::Window, width: u32, height: u32) {
    #[cfg(target_os = "macos")]
    {
        // takeSnapshot bildet die tatsächliche WebView-Größe ab. Deshalb das
        // Fenster vor dem Snapshot auf die Zielgröße (Stage-Auflösung) setzen:
        // HTML rendert dann exakt in der Auflösung der Stage, nicht in der
        // Fenstergröße.
        let target = tao::dpi::LogicalSize::new(f64::from(width), f64::from(height));
        if window.inner_size().to_logical::<f64>(window.scale_factor()) != target {
            window.set_inner_size(target);
        }
        native_snapshot(webview);
    }
    #[cfg(not(target_os = "macos"))]
    {
        js_snapshot(webview, width, height);
    }
}

/// Nativer Snapshot (macOS): WKWebView `takeSnapshotWithConfiguration`.
/// `WebViewExtMacOS::webview()` liefert die `WryWebView` – eine
/// WKWebView-Subklasse, der Snapshot-Call läuft via Deref auf WKWebView.
/// Alles auf dem Main-Thread (IPC-Handler); der Completion-Handler
/// antwortet asynchron mit PNG → [`emit_frame_from_png`].
#[cfg(target_os = "macos")]
fn native_snapshot(webview: &WebView) {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;

    let wk = webview.webview();
    let block = block2::RcBlock::new(|image: *mut NSImage, error: *mut NSError| {
        if !error.is_null() {
            let msg = unsafe { &*error }.localizedDescription().to_string();
            emit(&serde_json::json!({ "error": format!("snapshot: {msg}") }));
            return;
        }
        if image.is_null() {
            emit(&serde_json::json!({ "error": "snapshot: no image" }));
            return;
        }
        // NSImage → TIFF → NSBitmapImageRep → PNG-Daten.
        let png = unsafe {
            (*image)
                .TIFFRepresentation()
                .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                .and_then(|rep| {
                    rep.representationUsingType_properties(
                        NSBitmapImageFileType::PNG,
                        &NSDictionary::new(),
                    )
                })
        };
        match png {
            Some(data) => emit_frame_from_png(data.bytes()),
            None => emit(&serde_json::json!({ "error": "snapshot: png encode failed" })),
        }
    });
    // nil-Configuration = Default-Snapshot des sichtbaren Bereichs.
    unsafe {
        wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
    }
}

/// Kooperativer Snapshot (andere Plattformen): die Seite zeichnet ihr
/// erstes Canvas (`canvas[data-kuhplay-capture]` bevorzugt) skaliert auf die
/// Wunschgröße – `toDataURL` liefert PNG. Antwort asynchron via Callback.
#[cfg(not(target_os = "macos"))]
fn js_snapshot(webview: &WebView, width: u32, height: u32) {
    let js = format!(
        r#"
(function() {{
  var src = document.querySelector('canvas[data-kuhplay-capture]') || document.querySelector('canvas');
  if (!src) return JSON.stringify({{ error: 'no canvas element' }});
  var c = document.createElement('canvas');
  c.width = {width};
  c.height = {height};
  var ctx = c.getContext('2d');
  if (!ctx) return JSON.stringify({{ error: 'no 2d context' }});
  ctx.drawImage(src, 0, 0, {width}, {height});
  return c.toDataURL('image/png');
}})()
"#
    );
    let callback = |result: String| {
        // Der Callback erhält den JS-Rückgabewert JSON-serialisiert.
        let Ok(serde_json::Value::String(inner)) =
            serde_json::from_str::<serde_json::Value>(&result)
        else {
            emit(&serde_json::json!({ "error": "snapshot: unexpected js result" }));
            return;
        };
        if let Some(b64) = inner.strip_prefix("data:image/png;base64,") {
            match base64::engine::general_purpose::STANDARD.decode(b64) {
                Ok(png) => emit_frame_from_png(&png),
                Err(err) => {
                    emit(&serde_json::json!({ "error": format!("snapshot: base64: {err}") }))
                }
            }
            return;
        }
        // Fehlerseite: die Seite meldet { error: '…' } als JSON-String.
        let reason = serde_json::from_str::<serde_json::Value>(&inner)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or(inner);
        emit(&serde_json::json!({ "error": format!("snapshot: {reason}") }));
    };
    if let Err(err) = webview.evaluate_script_with_callback(&js, callback) {
        emit(&serde_json::json!({ "error": format!("snapshot: eval: {err}") }));
    }
}

/// Dekodiert PNG → RGBA und emitted `{"frame": {…}}` (base64-kodiert).
fn emit_frame_from_png(png: &[u8]) {
    let Ok(img) = image::load_from_memory(png) else {
        emit(&serde_json::json!({ "error": "snapshot: png decode failed" }));
        return;
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let b64 = base64::engine::general_purpose::STANDARD.encode(rgba.into_raw());
    emit(&serde_json::json!({
        "frame": { "width": w, "height": h, "rgba_b64": b64 }
    }));
}

/// Schreibt ein Event als JSON-Zeile nach stdout (an die Engine).
fn emit(value: &serde_json::Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(
        stdout,
        "{}",
        serde_json::to_string(value).unwrap_or_default()
    );
    let _ = stdout.flush();
}
