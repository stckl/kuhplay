//! Playback-Engine-Layer.
//!
//! Kapselt jegliche Media-Wiedergabe hinter dem [`MediaEngine`]-Trait. Die UI
//! spricht die Engine **nie direkt** an, sondern über [`EngineHandle`]
//! (thread-sichere Kanäle, siehe [`mock_engine`]).
//!
//! ## Bestandteile
//!
//! * [`command`] – [`EngineCommand`]: UI/Domain → Engine (Play mit Pre-Wait,
//!   Loops, Slices, Fades linear/dB, Seek, Stop, Load/Devamp, Timecode-Chasing,
//!   ConfigureStage, HTML5: PlayHtml/EvalJs/PushJson, Diashow:
//!   PlaySlideShow/SlideNav, Ping/Health-Check).
//! * [`media_engine`] – [`MediaEngine`]-Trait + [`EngineEvent`]: Engine → UI
//!   (Started/Finished/Position/LoopIteration/FadeProgress/PostWaitElapsed/
//!   FrameDrop/TimecodeJump/Loaded/StageConfigured/HtmlLoaded/JsExecuted/
//!   JsonReceived/SlideChanged/Error/Pong).
//! * [`clock`] – [`Clock`]-Trait: [`RealClock`] (Produktion) und
//!   [`VirtualClock`] (deterministische Tests auf die Millisekunde).
//! * [`mock_engine`] – [`MockMediaEngine`]: virtuelle Wiedergabe ohne Hardware,
//!   inkl. WebView-Instanzen (JS-Bridge/JSON-Push mit exakten Timestamps),
//!   Diashows (Auto-Advance/SlideNav), Frame-Drop- und Fehler-Simulation.
//!
//! ## Erweiterung: GStreamer-Anbindung (produktionstauglich)
//!
//! 1. Neues Modul `engine/gst_engine.rs` mit `struct GstEngine`.
//! 2. `impl MediaEngine for GstEngine`:
//!    * `Play` → `playbin` mit `uri=file://…`, State-Change auf `Playing`
//!      (Pre-Wait via `GstController` oder Scheduler-Task).
//!    * `Fade` → `GstController`-Interpolation auf das `volume`-Property.
//!    * `Loop` → `Eos` am Bus → `playbin` seek auf 0 + Re-Start (Count/Infinite).
//!    * `Stop`/`Pause`/`Resume` → State-Changes `Null`/`Paused`/`Playing`.
//!    * Hardware-Video (Decklink/NDI) über `playbin` + entsprechendem
//!      GStreamer-Plugin – ohne Änderung am Rest des Systems.
//! 3. `tick()` pollt den `GstBus` (non-blocking) und mappt Messages auf
//!    [`EngineEvent`] (`Eos` → `Finished`/`LoopIteration`, `Error` → `Error`).
//! 4. In `main.rs` `spawn_mock_engine()` durch `spawn_gst_engine()` ersetzen.
//!
//! ## Erweiterung: cpal-Audio für latenzkritische Wiedergabe
//!
//! Für <10 ms Latenz eignet sich ein `cpal`- oder `rodio`-Backend; beide
//! implementieren ebenfalls [`MediaEngine`]. Mehrere Engines können parallel
//! laufen (Audio-Engine + Video-Engine), koordiniert über denselben
//! `EngineCommand`-Strom.
//!
//! ## Erweiterung: wry-WebView für HTML5-Cues (produktiver WebView-Backend)
//!
//! 1. Neues Modul `engine/wry_engine.rs` mit `struct WryEngine` (wry + winit
//!    oder tao als Event-Loop für eingebettete WebViews).
//! 2. `impl MediaEngine for WryEngine`:
//!    * `PlayHtml` → `wry::WebViewBuilder` mit `HtmlSource::File` →
//!      `load_file`, `Url` → `load_url`, `Inline` → `load_html`.
//!    * `EvalJs` → `webview.evaluate_script(script)` – exakt das
//!      `triggerAnimation(payload)`-Muster aus der Show-Steuerung.
//!    * `PushJson` → `evaluate_script` mit einem vorab injizierten Bridge-Script,
//!      das `window.onShowControlData(json)` mit dem serialisierten Payload
//!      aufruft (JSON-Injection in den JS-Execution-Context).
//!    * `PlaySlideShow` → Bild-Preloading + Crossfade-Rendering auf einer
//!      Stage (GPU-Texturen), `SlideNav` → sofortiger Slide-Wechsel.
//! 3. `wry` benötigt systemabhängig webkit2gtk (Linux), WKWebView (macOS)
//!    bzw. WebView2 (Windows) – deshalb bewusst *nicht* einkompiliert;
//!    die Abstraktion über [`MediaEngine`] erlaubt den späteren Tausch
//!    ohne Änderung an Domain oder UI. `tokio` steht für asynchrone
//!    WebView-Events bereit.

pub mod audio_backend;
pub mod clock;
pub mod command;
pub mod compositor;
pub mod decklink;
pub mod frame;
pub mod media_engine;
pub mod mock_engine;
pub mod pattern;
pub mod real_engine;
pub mod route_renderer;
pub mod video_backend;
pub mod waveform;
pub mod webview_backend;

pub use clock::{Clock, RealClock, VirtualClock};
pub use command::EngineCommand;
pub use frame::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store, FrameStore,
    OpacityStore, StageFrameStore, VideoFrame,
};
pub use media_engine::{EngineEvent, MediaEngine};
pub use mock_engine::{
    spawn_mock_engine, spawn_mock_engine_with_join, EngineHandle, MockMediaEngine,
};
pub use pattern::draw_test_pattern;
pub use real_engine::{spawn_real_engine, spawn_real_engine_with_join, RealMediaEngine};
