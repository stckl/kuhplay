//! # KuhPlay
//!
//! Plattformübergreifende Show-Control- und Cue-Playback-Software in Rust
//! (Windows, macOS, Linux) – mit Fokus auf latenzfreie
//! Audio-/Video-Wiedergabe.
//!
//! ## Architektur (strikte Schichtentrennung)
//!
//! 1. [`core`] – Domain Data Model, kennt weder UI noch Media-Backends:
//!    * [`core::Cue`] / [`core::CueType`] / [`core::CueStatus`] – Cue,
//!      10 Typen (Audio, Video, Fade, Group, Stop, Script, Control, **Html**,
//!      **SlideShow**, **HtmlData**) und die 8-stufige State-Machine
//!      `Standby → Pre-Wait → Playing/Paused → Post-Wait →
//!      Completed/Stopped/Error`.
//!    * [`core::CueList`] – Playhead-Logik, GO-Trigger, Selektion,
//!      Reordering, Arm/Disarm, Group-Kaskadierung.
//!    * [`core::GroupMode`] – Simultaneous, Sequential, Timeline (zeitversetzt),
//!      Loop (re-arm) und Shuffle (deterministisch via Cue-ID-Seed).
//!    * [`core::ControlAction`] – Start, Stop, Pause, Load, Reset, Devamp,
//!      GoTo, Arm, Disarm, **NextSlide/PrevSlide** (Flow-Control-Cues).
//!    * [`core::audio`] – Fade-Parameter (Volume/Rate/Opacity), dB-Kurven
//!      (`linear_to_db`), Mehrkanal-Routing (`AudioRoute`) und
//!      `MediaSlice` (Segmente mit Play-Count).
//!    * [`core::timecode`] – SMPTE-Timecode (`HH:MM:SS:FF`) mit
//!      Frame-genauer Arithmetik für Timecode-Chasing (LTC/MTC).
//!    * [`core::stage`] – Video-Stages mit Layern und Geometrie
//!      (Position, Skalierung, Crop).
//!    * [`core::cart`] – [`core::CartManager`]: mehrere Cue-Listen (Carts)
//!      mit aktivem Cart und nahtlosem Umschalten.
//!    * [`core::html`] – [`core::HtmlSource`] (Datei/URL/Inline) für
//!      HTML5-Cues; die JS-Bridge läuft über Engine-Commands
//!      (`EvalJs`/`PushJson`).
//!    * [`core::slideshow`] – [`core::SlideShow`]/[`core::Slide`]/
//!      [`core::SlideNav`]: Diashow-Definition (Hold-Zeiten, Crossfade,
//!      Auto-Advance, Loop).
//!    * [`core::error`] – [`core::ShowError`] (thiserror).
//! 2. [`engine`] – Playback-Abstraktion ([`engine::MediaEngine`]-Trait) plus
//!    thread-sicherem Commander ([`engine::EngineHandle`]) und einer
//!    simulierenden Mock-Engine mit virtueller Uhr ([`engine::VirtualClock`]),
//!    virtuellen WebView-Instanzen (JS-Bridge + JSON-Push mit exakten
//!    Timestamps) und Diashow-Simulation. Eine produktive GStreamer-/cpal-/
//!    wry-Engine wird hier eingehängt, ohne den Rest des Systems zu ändern.
//! 3. [`gui`] – `egui`-Oberfläche im Dark-Look: Top-Bar (mit Cart-Switcher),
//!    Cue-Tabelle, Inspector (inkl. HTML5-Quellen-Editor, JS-Test-Button,
//!    Slide-Editor, JSON-Editor), GO-Leiste.
//!
//! ## Threading-Modell (UI blockiert nie)
//!
//! * **UI-Thread** (eframe/winit): rendert, sammelt Eingaben, pollt
//!   Engine-Events per `try_recv` (non-blocking).
//! * **Engine-Thread** (`std::thread` + `crossbeam-channel::select!`):
//!   empfängt Kommandos, treibt die Wiedergabe, sendet Events zurück.
//! * Kommunikation ausschließlich über Kanäle – es gibt keine Locks im
//!   UI-Pfad und keine blocking Calls von der UI in die Engine.
//!
//! ## Test-Strategie (`cargo test` – 186 Tests)
//!
//! * **Unit-Tests** in `src/core` und `src/engine`: State-Machine, Command-
//!   Erzeugung, Serde-Roundtrips, dB-/Timecode-/Stage-/Cart-/HTML-/Slide-Logik,
//!   Engine-Smoke-Tests (RealClock).
//! * **`tests/domain_model.rs`** (11): State-Machine-Übergangstabelle,
//!   Group-Kaskadierung, ID-Stabilität, Reordering, 500-Cue-GO-Stress.
//! * **`tests/timing_deterministic.rs`** (10): [`engine::VirtualClock`] +
//!   [`engine::MockMediaEngine`] – Pre-Wait/Post-Wait/Fades/Loops/Seek/
//!   Frame-Drops auf die exakte Millisekunde.
//! * **`tests/control_cues.rs`** (14): Start/Stop/Pause/Load/Reset/Devamp/
//!   GoTo/Arm/Disarm inkl. Fehlerpfade (unbekanntes Ziel, Disarm-Skip).
//! * **`tests/group_modes.rs`** (7): Simultaneous, Sequential, Timeline
//!   (Stagger), Loop (Re-Arm + Iterationslimit), Shuffle (deterministisch).
//! * **`tests/audio_params.rs`** (7): dB-Interpolation, Fade-Kurven,
//!   Slice-Validierung, Audio-Routing-Validierung.
//! * **`tests/timecode_chase.rs`** (8): Timecode-Chasing mit Frame-Toleranz,
//!   Sprüngen vor/zurück, Clamping, Loop-Grenzen.
//! * **`tests/stages.rs`** (8): Stage-Geometrie-Validierung, Layer, Crop,
//!   StageManager, ConfigureStage-Kommando.
//! * **`tests/carts.rs`** (10): Cart anlegen/löschen/umschalten, letzter Cart
//!   nicht löschbar, Delegation an die aktive CueList.
//! * **`tests/html_js.rs`** (14): HTML5-Cue-Registrierung, `PlayHtml`/
//!   `EvalJs` mit **exakten Trigger-Zeitpunkten**, Fehlerfälle (fehlende
//!   Instanz/Datei, leere Scripts, ungültige Quellen), JS-Trigger-Sturm
//!   (1000 Nachrichten, 4 Threads, keine Drops).
//! * **`tests/slideshow.rs`** (14): Slide-Validierung, Auto-Advance mit
//!   exakten Wechsel-Zeitpunkten, Loop-Wrap, manuelles `SlideNav` (Clamping),
//!   Control-Cues Next/PrevSlide, explizite Duration-Obergrenze, fehlende Bilder.
//! * **`tests/json_data.rs`** (12): JSON-Payload-Validierung (inkl. ungültigem
//!   JSON ohne Panic), `PushJson` mit **exaktem Zustell-Zeitpunkt** in der
//!   Ziel-WebView, Targeting-Fehler, JSON-Push-Sturm (1000 Nachrichten,
//!   4 Threads, keine Drops).
//! * **`tests/concurrency_stress.rs`** (3): Ping/Pong-No-Drop-Prüfung (1000
//!   Nachrichten, 4 Threads), Play/Stop-Sturm, sauberer Shutdown.
//! * **`tests/resilience.rs`** (6): fehlende/leere Medien, Treiberausfall →
//!   `Error`-Event + `Error`-Zustand, Recovery via Reset.
//! * **`tests/integration_tests.rs`** (5): End-to-End-Show-Simulation
//!   (Domain + Engine + Event-Mapping wie in der App).

pub mod core;
pub mod engine;
pub mod gui;
pub mod remote;
