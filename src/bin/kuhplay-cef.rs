//! CEF-Subprocess für HTML5-Cues: **Chromium Embedded Framework mit
//! Offscreen-Rendering (windowless)** – derselbe Ansatz wie OBS Studio und
//! CasparCG (docs/KONZEPT-html-templates.md §9).
//!
//! JSON-lines-IPC über stdio (src/engine/webview_backend.rs):
//! App → Subprocess: `{"shm": {"path": …}}`, `{"load": <HtmlSource>}`,
//! `{"eval": "<js>"}`, `{"push_json": <payload>}`,
//! `{"snapshot": {"width": W, "height": H}}` (Zielgröße), `{"close": true}`;
//! Subprocess → App: `{"loaded":true}`, `{"js_executed":true}`,
//! `{"json_received":true}`, `{"done":true}`, `{"error":"…"}` und pro Bild
//! `{"frame_shm":{…}}` – die Pixel selbst liegen im Shared Memory
//! (src/engine/frame_shm.rs). Ohne `shm` (Test-Doubles) antwortet er auf
//! `snapshot` mit `{"frame":{…,"rgba_b64":"…"}}`.
//!
//! Bauen und bündeln: `cargo build --features cef`, dann
//! `cargo run --features cef --bin kuhplay-cef-bundle` – legt
//! `target/<profil>/cef/` an (macOS: `kuhplay-cef.app` mit Framework und
//! Helper-Apps), dort findet die Engine den Renderer; Override:
//! `KUHPLAY_CEF_BIN=<pfad>`.
//!
//! Offscreen-Rendering: `OnPaint` liefert BGRA (premultiplied) mit
//! `windowless_frame_rate` (60 fps) auf transparentem Hintergrund – hier
//! nach RGBA (straight alpha für Key/Fill) konvertiert.

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use cef::*;
use kuhplay::core::HtmlSource;
use kuhplay::engine::frame_shm::FrameShmWriter;

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
    /// Shared-Memory-Ausgang (`{"shm": …}`): jedes passende OnPaint-Bild
    /// landet direkt dort; `None` = alter Weg (Base64 auf Anfrage).
    shm: Arc<Mutex<Option<FrameShmWriter>>>,
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
            shm: Arc::new(Mutex::new(None)),
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
            let src = unsafe { std::slice::from_raw_parts(buffer, len) };
            let (w, h) = (width as u32, height as u32);
            // Shared Memory: jedes Bild in Zielgröße sofort weiterreichen
            // (Tempo = CEF-Bildrate, kein Anfrage-Antwort-Takt).
            if let Some(shm) = self.shared.shm.lock().unwrap().as_mut() {
                let view = *self.shared.view.lock().unwrap();
                if (width, height) == view {
                    match shm.write(w, h, |dst| bgra_to_rgba_into(src, dst)) {
                        Ok((slot, seq)) => {
                            let _ = self.events.send(format!(
                                "{{\"frame_shm\":{{\"width\":{w},\"height\":{h},\"slot\":{slot},\"seq\":{seq}}}}}"
                            ));
                        }
                        Err(err) => {
                            let _ = self.events.send(
                                serde_json::json!({ "error": format!("snapshot: shm {err}") })
                                    .to_string(),
                            );
                        }
                    }
                }
                return;
            }
            let data = src.to_vec();
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
    impl App {
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Offscreen ohne GPU-Prozess (Software-Compositing, läuft auch
            // headless und auf Rechnern ohne passenden Treiber).
            // use-mock-keychain: Chromium will sonst auf macOS den Schlüsselbund
            // („Chromium Safe Storage") – unnötig, der Cache ist temporär.
            if let Some(cmd) = command_line {
                for switch in [
                    "disable-gpu",
                    "disable-gpu-compositing",
                    "no-sandbox",
                    "use-mock-keychain",
                ] {
                    cmd.append_switch(Some(&CefString::from(switch)));
                }
                cmd.append_switch_with_value(
                    Some(&CefString::from("password-store")),
                    Some(&CefString::from("basic")),
                );
                // Linux: offscreen ohne X11/Wayland-Anzeige (auch auf CI).
                #[cfg(target_os = "linux")]
                cmd.append_switch_with_value(
                    Some(&CefString::from("ozone-platform")),
                    Some(&CefString::from("headless")),
                );
            }
        }
    }
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
    bgra_to_rgba_into(src, &mut out);
    out
}

/// Wie [`bgra_to_rgba`], direkt in `dst` (gleiche Länge).
fn bgra_to_rgba_into(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        let (b, g, r, a) = (s[0], s[1], s[2], s[3]);
        if a == 255 || a == 0 {
            d.copy_from_slice(&[r, g, b, a]);
        } else {
            let un = |c: u8| ((c as u16 * 255) / a as u16).min(255) as u8;
            d.copy_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
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

/// Fataler Fehler als `{"error": …}` auf stdout – die Engine zeigt ihn als
/// Cue-Fehler an, statt dass der Cue schwarz bleibt.
fn emit_error(msg: &str) {
    let mut so = std::io::stdout().lock();
    let _ = writeln!(so, "{}", serde_json::json!({ "error": msg }));
    let _ = so.flush();
}

/// macOS: Pfad zum CEF-Framework im Bundle. Hauptprozess:
/// `kuhplay-cef.app/Contents/MacOS/kuhplay-cef` → `../Frameworks`; Helper:
/// `…/Frameworks/kuhplay-cef Helper (…).app/Contents/MacOS/…` → `../../..`.
#[cfg(target_os = "macos")]
fn framework_path(helper: bool) -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let up = if helper { "../../.." } else { "../Frameworks" };
    let path = exe
        .parent()?
        .join(up)
        .join("Chromium Embedded Framework.framework/Chromium Embedded Framework");
    path.exists().then_some(path)
}

fn main() {
    // Unterprozesse (Renderer/GPU/Utility) starten mit --type=… und zweigen
    // unten in execute_process ab.
    let is_subprocess = std::env::args().any(|a| a.starts_with("--type="));

    // macOS: CEF liegt als Framework im Bundle (siehe kuhplay-cef-bundle)
    // und muss vor jedem CEF-Aufruf geladen werden.
    #[cfg(target_os = "macos")]
    let _library = {
        if framework_path(is_subprocess).is_none() {
            emit_error(
                "CEF-Framework nicht gefunden: kuhplay-cef muss aus kuhplay-cef.app laufen \
                 (gebaut mit `cargo run --features cef --bin kuhplay-cef-bundle`)",
            );
            std::process::exit(1);
        }
        let exe = std::env::current_exe().expect("eigener Pfad");
        let loader = cef::library_loader::LibraryLoader::new(&exe, is_subprocess);
        if !loader.load() {
            emit_error("cef: Framework konnte nicht geladen werden");
            std::process::exit(1);
        }
        loader
    };
    let _ = api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let args = cef::args::Args::new();
    let mut app = OsrApp::new();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if is_subprocess {
        std::process::exit(code);
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

    let mut settings = Settings {
        no_sandbox: 1,
        windowless_rendering_enabled: 1,
        multi_threaded_message_loop: 0,
        external_message_pump: 1, // eigener Loop mit do_message_loop_work
        log_severity: LogSeverity::DISABLE,
        ..Default::default()
    };
    // Ressourcen findet CEF selbst: im Framework (macOS) bzw. neben dem
    // Binary (Windows/Linux). Eigener Cache pro Prozess, beim Ende gelöscht.
    let cache = std::env::temp_dir().join(format!("kuhplay-cef-cache-{}", std::process::id()));
    std::fs::create_dir_all(&cache).ok();
    settings.root_cache_path = CefString::from(cache.to_string_lossy().as_ref());
    settings.cache_path = CefString::from(cache.to_string_lossy().as_ref());
    // macOS: Bundle-Pfade explizit. Liegt kuhplay-cef.app in einem anderen
    // Bundle (KuhPlay.app/Contents/Helpers), sucht Chromium sonst im äußeren
    // Bundle und stürzt beim Start ab (SIGTRAP).
    #[cfg(target_os = "macos")]
    if let Some(app) = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.parent()?.to_path_buf()))
    {
        let frameworks = app.join("Contents/Frameworks");
        let as_cef = |p: &std::path::Path| CefString::from(p.to_string_lossy().as_ref());
        settings.main_bundle_path = as_cef(&app);
        settings.framework_dir_path =
            as_cef(&frameworks.join("Chromium Embedded Framework.framework"));
        settings.browser_subprocess_path =
            as_cef(&frameworks.join("kuhplay-cef Helper.app/Contents/MacOS/kuhplay-cef Helper"));
    }

    let rc = initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if rc != 1 {
        emit_error(&format!("cef initialize fehlgeschlagen (rc={rc})"));
        let _ = std::fs::remove_dir_all(&cache);
        return;
    }

    // Kein Eltern-Fenster. Der Handle-Typ ist je Plattform anders (macOS
    // Pointer, Linux Ganzzahl, Windows HWND) – „null" ist überall gültig.
    // SAFETY: alle drei Varianten sind Werte ohne Invarianten, null = keins.
    let no_parent: cef::sys::cef_window_handle_t = unsafe { std::mem::zeroed() };
    let window_info = WindowInfo::default().set_as_windowless(no_parent);
    let browser_settings = BrowserSettings {
        windowless_frame_rate: 60, // flüssige Animationen wie OBS
        // Voll transparent (ARGB 0): Seiten ohne eigenen Hintergrund liegen
        // mit Alpha über Video/Bildern bzw. auf dem Key-Ausgang.
        background_color: 0,
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
        emit_error("cef create_browser_sync failed");
        shutdown();
        return;
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
        match cmd_rx.recv_timeout(Duration::from_millis(4)) {
            Ok(cmd) => {
                if cmd.get("close").and_then(|v| v.as_bool()) == Some(true) {
                    running = false;
                } else if let Some(path) = cmd
                    .get("shm")
                    .and_then(|v| v.get("path"))
                    .and_then(|v| v.as_str())
                {
                    match FrameShmWriter::open(std::path::Path::new(path)) {
                        Ok(writer) => *shared.shm.lock().unwrap() = Some(writer),
                        Err(err) => emit_error(&format!("shm {path}: {err}")),
                    }
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
                    let size = (w.max(1) as i32, h.max(1) as i32);
                    // Shared Memory: OnPaint liefert ohnehin jedes Bild – nur
                    // bei neuer Größe neu zeichnen lassen.
                    if shared.shm.lock().unwrap().is_some() {
                        let changed = *shared.view.lock().unwrap() != size;
                        *shared.view.lock().unwrap() = size;
                        if changed {
                            if let Some(host) = browser.host() {
                                host.was_resized();
                                host.invalidate(PaintElementType::VIEW);
                            }
                        }
                        continue;
                    }
                    // View auf Zielgröße → nächster OnPaint rendert direkt passend.
                    *shared.view.lock().unwrap() = size;
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
    let _ = std::fs::remove_dir_all(&cache);
}
