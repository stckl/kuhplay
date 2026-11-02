//! CEF-Subprocess für HTML5-Cues: **Chromium Embedded Framework mit
//! Offscreen-Rendering (windowless)** – derselbe Ansatz wie OBS Studio und
//! CasparCG (docs/KONZEPT-html-templates.md §9).
//!
//! Gleiches JSON-lines-IPC über stdio wie `kuhplay-webview`
//! (src/engine/webview_backend.rs), damit die Engine nur das Binary wechselt:
//! App → Subprocess: `{"load": <HtmlSource>}`, `{"eval": "<js>"}`,
//! `{"push_json": <payload>}`, `{"snapshot": {"width": W, "height": H}}`,
//! `{"close": true}`; Subprocess → App: `{"loaded":true}`,
//! `{"js_executed":true}`, `{"json_received":true}`, `{"done":true}`,
//! `{"frame":{"width":W,"height":H,"rgba_b64":"…"}}`, `{"error":"…"}`.
//!
//! Bauen: `cargo build --features cef` (Binary: `kuhplay-cef`). Zur Laufzeit
//! wird die CEF-Distribution über `CEF_PATH` gefunden (cef-dll-sys lädt sie
//! automatisch herunter, wenn sie fehlt). Auswahl in der App:
//! `KUHPLAY_HTML_RENDERER=cef` oder `KUHPLAY_CEF_BIN=<pfad>`.
//!
//! Offscreen-Rendering: `OnPaint` liefert BGRA (premultiplied) mit
//! `windowless_frame_rate` (60 fps) – hier nach RGBA (straight alpha für
//! Key/Fill) konvertiert und auf die Snapshot-Größe skaliert.

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use cef::args::Args;
use cef::*;
use kuhplay::core::HtmlSource;

/// JS-Bridge, die in jede geladene Seite injiziert wird (wie im wry-Subprocess).
const BRIDGE_JS: &str = r#"
window.kuhplay = window.kuhplay || {
    version: "1",
    onShowControlData: null,
    done: function () { document.title = "__kuhplay_done__"; },
};
"#;

/// Titel-Marker, über den `window.kuhplay.done()` meldet.
const DONE_TITLE: &str = "__kuhplay_done__";

/// Default-View-Größe, bis die Engine eine Snapshot-Größe anfordert.
const DEFAULT_VIEW: (i32, i32) = (1280, 720);

/// Letzter OnPaint-Frame (BGRA, premultiplied).
type FrameBuf = Arc<Mutex<Option<(u32, u32, Vec<u8>)>>>;

#[derive(Clone)]
struct Shared {
    frame: FrameBuf,
    /// Angeforderte View-Größe (Snapshot-Ziel), initial DEFAULT_VIEW.
    view: Arc<Mutex<(i32, i32)>>,
    /// Offener Snapshot-Request (Größe) – wird bei passendem OnPaint sofort
    /// als Frame beantwortet, sonst skaliert der Request den letzten Frame.
    pending: Arc<Mutex<Option<(u32, u32)>>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            frame: Arc::new(Mutex::new(None)),
            view: Arc::new(Mutex::new(DEFAULT_VIEW)),
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

wrap_render_handler! {
    struct OsrHandler {
        shared: Shared,
        events: mpsc::Sender<String>,
    }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(r) = rect {
                let (w, h) = *self.shared.view.lock().unwrap();
                *r = Rect { x: 0, y: 0, width: w, height: h };
            }
        }
        fn on_paint(&self, _browser: Option<&mut Browser>, _t: PaintElementType, _dirty: Option<&[Rect]>, buffer: *const u8, width: i32, height: i32) {
            if buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            let len = (width as usize) * (height as usize) * 4;
            let data = unsafe { std::slice::from_raw_parts(buffer, len) }.to_vec();
            let (w, h) = (width as u32, height as u32);
            *self.shared.frame.lock().unwrap() = Some((w, h, data));
            // Offener Snapshot mit genau dieser Größe → sofort als Frame senden.
            let pending = self.shared.pending.lock().unwrap().take();
            if let Some((pw, ph)) = pending {
                if pw == w && ph == h {
                    if let Some((_, _, bgra)) = self.shared.frame.lock().unwrap().clone() {
                        emit_frame(&self.events, w, h, &bgra);
                    }
                } else {
                    *self.shared.pending.lock().unwrap() = Some((pw, ph));
                }
            }
        }
    }
}

wrap_load_handler! {
    struct LoadH {
        events: mpsc::Sender<String>,
    }
    impl LoadHandler {
        fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition: TransitionType) {
            // Bridge vor dem Seiten-JS injizieren.
            if let Some(f) = frame {
                if f.is_main() != 0 {
                    f.execute_java_script(Some(&CefString::from(BRIDGE_JS)), None, 0);
                }
            }
        }
        fn on_load_end(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _code: i32) {
            let _ = self.events.send("{\"loaded\":true}".to_string());
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, code: Errorcode, url: Option<&CefString>, error: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() != 0) {
                let msg = format!(
                    "load error {} at {}: {}", code.get_raw(),
                    url.map(|u| u.to_string()).unwrap_or_default(),
                    error.map(|e| e.to_string()).unwrap_or_default()
                );
                let _ = self.events.send(serde_json::json!({ "error": msg }).to_string());
            }
        }
    }
}

wrap_display_handler! {
    struct DispH {
        events: mpsc::Sender<String>,
    }
    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if title.map(|s| s.to_string()).as_deref() == Some(DONE_TITLE) {
                let _ = self.events.send("{\"done\":true}".to_string());
            }
        }
    }
}

wrap_client! {
    struct OsrClient {
        render: RenderHandler,
        load: LoadHandler,
        disp: DisplayHandler,
    }
    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.disp.clone())
        }
    }
}

wrap_app! {
    struct OsrApp {}
    impl App {}
}

/// Sendet einen Frame als `{"frame":{…}}` (BGRA → RGBA straight alpha).
fn emit_frame(events: &mpsc::Sender<String>, w: u32, h: u32, bgra: &[u8]) {
    let rgba = bgra_to_rgba(bgra);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
    let _ = events.send(
        serde_json::json!({
            "frame": { "width": w, "height": h, "rgba_b64": b64 }
        })
        .to_string(),
    );
}

/// BGRA (premultiplied, wie CEF OnPaint liefert) → RGBA straight alpha.
fn bgra_to_rgba(src: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    for (si, chunk) in src.chunks_exact(4).enumerate() {
        let (b, g, r, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        let di = si * 4;
        out[di] = r;
        out[di + 1] = g;
        out[di + 2] = b;
        out[di + 3] = a;
        if a > 0 && a < 255 {
            for c in 0..3 {
                out[di + c] = ((out[di + c] as u16 * 255) / a as u16).min(255) as u8;
            }
        }
    }
    out
}

/// Nearest-neighbor-Skalierung BGRA → RGBA in Zielgröße.
fn scale_bgra_to_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        let sy = (y as u64 * sh as u64 / dh as u64) as usize;
        for x in 0..dw {
            let sx = (x as u64 * sw as u64 / dw as u64) as usize;
            let si = (sy * sw as usize + sx) * 4;
            let di = (y * dw + x) as usize * 4;
            out[di] = src[si + 2];
            out[di + 1] = src[si + 1];
            out[di + 2] = src[si];
            out[di + 3] = src[si + 3];
            let a = src[si + 3];
            if a > 0 && a < 255 {
                for c in 0..3 {
                    out[di + c] = ((out[di + c] as u16 * 255) / a as u16).min(255) as u8;
                }
            }
        }
    }
    out
}

/// `HtmlSource` → URL für CEF (Inline als data-URL, Datei als file://).
fn source_to_url(source: &HtmlSource) -> String {
    match source {
        HtmlSource::Url(url) => url.clone(),
        HtmlSource::File(path) => {
            let p = path.to_string_lossy().replace(' ', "%20");
            format!("file://{p}")
        }
        HtmlSource::Inline(html) => format!(
            "data:text/html;charset=utf-8;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(html.as_bytes())
        ),
    }
}

fn main() {
    // macOS: libcef manuell laden (CEF_PATH env, siehe cef-dll-sys).
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        if let Some(dir) = cef::sys::get_cef_dir() {
            let fw = dir.join(cef::sys::FRAMEWORK_PATH);
            let fw = fw.canonicalize().unwrap_or(fw);
            let c = CString::new(fw.as_os_str().as_bytes()).expect("Pfad ohne NUL");
            if unsafe { cef::sys::cef_load_library(c.as_ptr().cast()) } != 1 {
                eprintln!("cef_load_library fehlgeschlagen: {}", fw.display());
                std::process::exit(1);
            }
        }
    }

    let (event_tx, event_rx) = mpsc::channel::<String>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<serde_json::Value>();

    // stdin auf eigenem Thread (der Main-Thread gehört dem CEF-Message-Loop).
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        if cmd_tx.send(v).is_err() {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        // stdin zu = App weg → mitbeenden.
        let _ = cmd_tx.send(serde_json::json!({ "close": true }));
    });

    let shared = Shared::default();
    let render = OsrHandler::new(shared.clone(), event_tx.clone());
    let load = LoadH::new(event_tx.clone());
    let disp = DispH::new(event_tx.clone());
    let client = OsrClient::new(render, load, disp);

    let args = Args::new();
    let mut settings = Settings {
        no_sandbox: 1,
        windowless_rendering_enabled: 1,
        multi_threaded_message_loop: 0,
        external_message_pump: 1, // eigener Loop mit do_message_loop_work
        log_severity: LogSeverity::DISABLE,
        ..Default::default()
    };
    // Ressourcen explizit setzen (Subprocess läuft nicht in einem .app-Bundle).
    if let Some(dir) = cef::sys::get_cef_dir() {
        let res = dir.join("Chromium Embedded Framework.framework/Resources");
        settings.resources_dir_path = CefString::from(res.to_string_lossy().as_ref());
        settings.locales_dir_path = CefString::from(res.join("locales").to_string_lossy().as_ref());
    }
    let cache = std::env::temp_dir().join(format!("kuhplay-cef-cache-{}", std::process::id()));
    std::fs::create_dir_all(&cache).ok();
    settings.cache_path = CefString::from(cache.to_string_lossy().as_ref());

    let _app = OsrApp::new();
    let rc = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        None,
        std::ptr::null_mut(),
    );
    if rc != 0 {
        let _ = writeln!(std::io::stderr(), "cef initialize failed: {rc}");
        std::process::exit(1);
    }

    let window_info = WindowInfo::default().set_as_windowless(std::ptr::null_mut());
    let browser_settings = BrowserSettings {
        windowless_frame_rate: 60, // flüssige Animationen wie OBS
        ..Default::default()
    };
    let url = CefString::from("about:blank");
    let mut client = client;
    let browser = browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&browser_settings),
        None,
        None,
    );
    let Some(browser) = browser else {
        let _ = writeln!(std::io::stderr(), "cef create_browser_sync failed");
        shutdown();
        std::process::exit(1);
    };

    let stdout = std::io::stdout();
    let mut running = true;
    while running {
        do_message_loop_work();
        while let Ok(line) = event_rx.try_recv() {
            let mut so = stdout.lock();
            let _ = writeln!(so, "{line}");
            let _ = so.flush();
        }
        match cmd_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(cmd) => {
                if cmd.get("close").and_then(|v| v.as_bool()) == Some(true) {
                    running = false;
                } else if let Some(load) = cmd.get("load") {
                    match serde_json::from_value::<HtmlSource>(load.clone()) {
                        Ok(source) => {
                            if let Some(frame) = browser.main_frame() {
                                frame.load_url(Some(&CefString::from(
                                    source_to_url(&source).as_str(),
                                )));
                            }
                        }
                        Err(err) => {
                            let mut so = stdout.lock();
                            let _ = writeln!(
                                so,
                                "{}",
                                serde_json::json!({ "error": format!("load: {err}") })
                            );
                            let _ = so.flush();
                        }
                    }
                } else if let Some(js) = cmd.get("eval").and_then(|v| v.as_str()) {
                    if let Some(frame) = browser.main_frame() {
                        frame.execute_java_script(Some(&CefString::from(js)), None, 0);
                    }
                    let mut so = stdout.lock();
                    let _ = writeln!(so, "{{\"js_executed\":true}}");
                    let _ = so.flush();
                } else if let Some(payload) = cmd.get("push_json") {
                    let js = format!(
                        "window.kuhplay && window.kuhplay.onShowControlData \
                         && window.kuhplay.onShowControlData({payload})"
                    );
                    if let Some(frame) = browser.main_frame() {
                        frame.execute_java_script(Some(&CefString::from(js.as_str())), None, 0);
                    }
                    let mut so = stdout.lock();
                    let _ = writeln!(so, "{{\"json_received\":true}}");
                    let _ = so.flush();
                } else if let Some(snap) = cmd.get("snapshot") {
                    let w = snap.get("width").and_then(|v| v.as_u64()).unwrap_or(1280) as u32;
                    let h = snap.get("height").and_then(|v| v.as_u64()).unwrap_or(720) as u32;
                    // View auf Zielgröße → nächster OnPaint rendert direkt passend.
                    *shared.view.lock().unwrap() = (w.max(1) as i32, h.max(1) as i32);
                    *shared.pending.lock().unwrap() = Some((w.max(1), h.max(1)));
                    if let Some(host) = browser.host() {
                        host.invalidate(PaintElementType::VIEW);
                    }
                    // Fallback: letzter Frame skaliert, falls kein OnPaint folgt.
                    if let Some((fw, fh, bgra)) = shared.frame.lock().unwrap().clone() {
                        if (fw, fh) != (w, h) {
                            let rgba = scale_bgra_to_rgba(&bgra, fw, fh, w, h);
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&rgba);
                            let mut so = stdout.lock();
                            let _ = writeln!(
                                so,
                                "{{\"frame\":{{\"width\":{w},\"height\":{h},\"rgba_b64\":\"{b64}\"}}}}"
                            );
                            let _ = so.flush();
                            *shared.pending.lock().unwrap() = None;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => running = false,
        }
    }
    quit_message_loop();
    shutdown();
}
