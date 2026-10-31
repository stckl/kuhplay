//! Kommandos von der UI/Domain an die Playback-Engine.
//!
//! Rein datenorientiert (nur `uuid` + `std::time` + Typen aus
//! [`crate::core`]), damit `core` Commands bauen kann, ohne die Engine zu
//! kennen – und die Engine Commands empfängt, ohne das Domain-Model im
//! Detail zu kennen.

use std::time::Duration;

use uuid::Uuid;

use crate::core::{
    AudioRoute, FadeCurve, FadeParam, HtmlSource, LoopMode, MediaSlice, RouteManager, SlideNav,
    SlideShow, Stage, SurfaceGeometry, Timecode,
};

/// Eine Anweisung an die Playback-Engine.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineCommand {
    /// Startet Wiedergabe.
    ///
    /// * `duration = None` → Wiedergabe bis Stop/EOF (Mock: 5 s Default).
    /// * `pre_wait` verzögert den tatsächlichen Start (Engine-seitig).
    /// * `post_wait` verzögert das `PostWaitElapsed`-Event nach `Finished`.
    /// * `loop_mode` wiederholt die Wiedergabe (wird von `slices` überschrieben).
    /// * `slices`: Segment-Plan mit individuellen Wiederholungszählern
    ///   (überschreibt `duration`/`loop_mode` bei der Wiedergabe).
    /// * `route`: Audio-Routing (Kanäle/Pan) – Vorbereitung Mehrkanal.
    /// * `timecode_in`: koppelt den Cue an eine externe Clock (Chasing).
    /// * `stage`/`layer`: Ziel-Stage und Layer für Video-Cues.
    /// * `surface`: Surface-Geometrie auf der Stage (Position/Größe,
    ///   Area of Interest, Hard-Edge, Deckkraft, Sichtbarkeit).
    Play {
        cue_id: Uuid,
        path: Option<String>,
        duration: Option<Duration>,
        pre_wait: Duration,
        post_wait: Duration,
        loop_mode: LoopMode,
        slices: Vec<MediaSlice>,
        route: Option<AudioRoute>,
        timecode_in: Option<Timecode>,
        stage: Option<String>,
        layer: u32,
        surface: Option<SurfaceGeometry>,
    },
    /// Pausiert einen laufenden Cue.
    Pause { cue_id: Uuid },
    /// Setzt einen pausierten Cue fort.
    Resume { cue_id: Uuid },
    /// Stoppt einen einzelnen Cue.
    Stop { cue_id: Uuid },
    /// Stoppt alle laufenden Playbacks (Panic / Blackout).
    StopAll,
    /// Fadet einen Parameter (`param`: Volume/Rate/Opacity) von `from` auf
    /// `target` über `duration` – linear oder dB-interpoliert (`curve`).
    Fade {
        cue_id: Uuid,
        /// Ziel-Cue, dessen Parameter gefadet wird (`None` = der Fade-Cue selbst).
        target_cue: Option<Uuid>,
        param: FadeParam,
        from: f32,
        target: f32,
        duration: Duration,
        curve: FadeCurve,
    },
    /// Springt an eine Position im Medium (Timecode-Sprung).
    Seek { cue_id: Uuid, position: Duration },
    /// Setzt die Lautstärke (0.0 – 1.0).
    SetVolume { cue_id: Uuid, volume: f32 },
    /// Lädt ein Medium vor (Preload, ohne Wiedergabe) → `EngineEvent::Loaded`.
    Load { cue_id: Uuid, path: Option<String> },
    /// Devamp: beendet Loops/Slices sofort und springt zum Ende (Outro).
    Devamp { cue_id: Uuid },
    /// Devamp für alle laufenden Playbacks.
    DevampAll,
    /// Aktiviert/deaktiviert Timecode-Chasing für einen Cue
    /// (`timecode_in`: Offset der externen Clock; `None` = deaktivieren).
    ChaseTimecode {
        cue_id: Uuid,
        timecode_in: Option<Timecode>,
    },
    /// Wechselt das Audio-Ausgabegerät (`None` = System-Standard) →
    /// `AudioOutputChanged`, oder `Error` (altes Gerät bleibt aktiv), wenn das
    /// Gerät fehlt oder gerade Audio-Cues laufen.
    SetAudioOutput { device: Option<String> },
    /// Legt die logischen Audio-Kanäle auf Geräte-Ausgänge — als Matrix:
    /// jeder Kanal kann auf mehrere Ausgänge und mehrere Geräte gleichzeitig
    /// (Parallelbetrieb). Wirkt sofort, auch auf laufende Cues.
    SetAudioPatch { patch: Box<crate::core::AudioPatch> },
    /// Konfiguriert eine Video-Stage (Geometrie/Layer) → `StageConfigured`
    /// oder `Error` bei ungültiger Geometrie.
    ConfigureStage { stage: Stage },
    /// Konfiguriert die Output Routes (QLab 5 "Output Routing"): logische
    /// Ausgänge mit Auflösung, Device, Testbild und Skalierung. Die Engine
    /// rendert pro Route ein Bild in den Route-Frame-Store (Stage ->
    /// Region -> Route; Route-Testbild überdeckt den Inhalt).
    ConfigureRoutes { routes: RouteManager },
    /// Startet einen HTML5-Cue: Die Engine lädt die `source` in eine
    /// WebView-Instanz (auf `stage`/`layer`) und aktiviert die JS-Bridge.
    /// `duration = None` → Mock-Default (5 s); danach `Finished`.
    PlayHtml {
        cue_id: Uuid,
        source: HtmlSource,
        stage: Option<String>,
        layer: u32,
        duration: Option<Duration>,
        pre_wait: Duration,
        post_wait: Duration,
    },
    /// Startet einen Diashow-Cue: Die Engine spielt die `show` (Slides mit
    /// Hold-Zeiten) ab – Auto-Advance oder manuell via `SlideNav`.
    /// `surface`: Surface-Geometrie auf der Stage (wie bei [`EngineCommand::Play`]).
    PlaySlideShow {
        cue_id: Uuid,
        show: SlideShow,
        stage: Option<String>,
        layer: u32,
        duration: Option<Duration>,
        pre_wait: Duration,
        post_wait: Duration,
        surface: Option<SurfaceGeometry>,
    },
    /// Führt JavaScript in der laufenden WebView-Instanz eines HTML5-Cues
    /// aus (`eval_js`). Ohne aktive Instanz → `EngineEvent::Error`.
    EvalJs { cue_id: Uuid, script: String },
    /// Pusht einen JSON-Payload an die WebView-Instanz des Ziel-Cues
    /// (HTML-Data-Cue, z. B. `window.onShowControlData(payload)`).
    /// Ohne aktive Ziel-Instanz → `EngineEvent::Error`.
    PushJson {
        target: Uuid,
        payload: serde_json::Value,
    },
    /// Schaltet eine laufende Diashow manuell (Control-Cues
    /// `NextSlide`/`PrevSlide`).
    SlideNav { cue_id: Uuid, nav: SlideNav },
    /// Wait-Cue (QLab 5): reine Timer-Unit ohne Medium – die Engine startet
    /// nach `pre_wait` und meldet nach `duration` `Finished` (danach
    /// `post_wait`). `duration = 0` → sofortiges Ende nach dem Pre-Wait.
    /// Pausieren/Stoppen/Devamp wirken wie bei normalen Playbacks.
    Wait {
        cue_id: Uuid,
        duration: Duration,
        pre_wait: Duration,
        post_wait: Duration,
    },
    /// Network-Cue (QLab 5): sendet `message` per UDP/TCP an `host:port`.
    /// Sofort-Cue – die Engine feuert asynchron (spawned Thread) und meldet
    /// nur Fehler als [`super::EngineEvent::Error`]; der Cue selbst gilt in
    /// der Cue-Liste direkt als abgeschlossen.
    SendNetwork {
        cue_id: Uuid,
        protocol: crate::core::NetworkProtocol,
        host: String,
        port: u16,
        message: String,
    },
    /// Health-Check: Engine antwortet mit [`super::EngineEvent::Pong`].
    /// Dient Stress-Tests zur verlässlichen "keine Message-Drops"-Prüfung.
    Ping(u64),
}
