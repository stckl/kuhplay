//! Kommandos von der UI/Domain an die Playback-Engine.
//!
//! Rein datenorientiert (nur `uuid` + `std::time` + Typen aus
//! [`crate::core`]), damit `core` Commands bauen kann, ohne die Engine zu
//! kennen – und die Engine Commands empfängt, ohne das Domain-Model im
//! Detail zu kennen.

use std::time::Duration;

use uuid::Uuid;

use crate::core::{
    AudioRoute, FadeCurve, HtmlSource, LightMessage, LoopMode, MediaSlice, RouteManager, SlideNav,
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
        /// In-Point im Medium (None = 0).
        in_point: Option<Duration>,
        /// Out-Point im Medium (None = Medienende).
        out_point: Option<Duration>,
    },
    /// Das letzte Bild dieses Cues bleibt nach dem Ende stehen.
    HoldLastFrame { cue_id: Uuid },
    /// Stoppt `stops`, sobald `cue_id` sein erstes Bild zeigt, damit dazwischen
    /// nichts schwarz wird. Ohne `wait_for_picture` sofort. Fällt `cue_id` aus,
    /// laufen die alten Cues weiter.
    StopWhenVisible {
        cue_id: Uuid,
        stops: Vec<Uuid>,
        wait_for_picture: bool,
    },
    /// Pausiert einen laufenden Cue.
    Pause { cue_id: Uuid },
    /// Setzt einen pausierten Cue fort.
    Resume { cue_id: Uuid },
    /// Stoppt einen einzelnen Cue.
    Stop { cue_id: Uuid },
    /// Stoppt alle laufenden Playbacks (Panic / Blackout).
    StopAll,
    /// Fadet Volume und/oder Opacity eines Ziel-Cues über `duration`
    /// (docs/AUFTRAG-fade.md). `from: None` = aktueller Wert des Ziels
    /// (kein Sprung). Der Fade-Cue meldet erst `Finished`, wenn alle seine
    /// Fades fertig sind; `stop_when_done` stoppt das Ziel vorher.
    Fade {
        cue_id: Uuid,
        /// Ziel-Cue (`None` = der Fade-Cue selbst).
        target_cue: Option<Uuid>,
        /// Startwert (None = aktueller Wert des Ziels).
        from: Option<f32>,
        /// Ziel-Lautstärke (None = Volume wird nicht gefadet).
        volume: Option<f32>,
        /// Ziel-Deckkraft (None = Opacity wird nicht gefadet).
        opacity: Option<f32>,
        duration: Duration,
        /// Kurve (gilt für Volume; Opacity läuft immer linear).
        curve: FadeCurve,
        /// Stoppt das Ziel, bevor der Fade-Cue Finished meldet.
        stop_when_done: bool,
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
    /// Konfiguriert die Output Routes ("Output Routing"): logische
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
    /// Externer Timecode (LTC/MTC/OSC-Eingang) in die Engine einspeisen:
    /// alle chasing Cues (`timecode_in`) springen auf `tc - timecode_in`
    /// (Sprung > 1 Frame; Media-Backends seeken mit, Wallclock-Kinds nur
    /// ihre Position). Siehe `MediaEngine`-Implementierungen.
    InjectTimecode { tc: crate::core::Timecode },
    /// Wait-Cue: reine Timer-Unit ohne Medium – die Engine startet
    /// nach `pre_wait` und meldet nach `duration` `Finished` (danach
    /// `post_wait`). `duration = 0` → sofortiges Ende nach dem Pre-Wait.
    /// Pausieren/Stoppen/Devamp wirken wie bei normalen Playbacks.
    Wait {
        cue_id: Uuid,
        duration: Duration,
        pre_wait: Duration,
        post_wait: Duration,
    },
    /// Network-Cue: sendet `message` per UDP/TCP an `host:port`.
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
    /// MIDI-Cue: sendet `bytes` (Wire-Format, siehe
    /// [`crate::core::MidiMessage::to_bytes`]) an das MIDI-Ausgabegerät.
    /// Sofort-Cue; Fehler (kein Gerät, Send-Fehler) als
    /// [`super::EngineEvent::Error`].
    SendMidi { cue_id: Uuid, bytes: Vec<u8> },
    /// Light-Cue (Minimal): sendet ein Art-Net-DMX-Paket
    /// (OpDmx, [`crate::core::LightMessage::to_artnet`]) per UDP an
    /// `host:6454`. Sofort-Cue; Fehler als [`super::EngineEvent::Error`].
    SendDmx {
        cue_id: Uuid,
        /// Licht-Nachricht; die Engine baut das Paket (hält pro Universe den
        /// DMX-Stand und die sACN-Sequenz, siehe Block A des Review-Docs).
        message: LightMessage,
    },
    /// Companion-Cue: HTTP POST an `http://host:port/path` (leerer Body) –
    /// Bitfocus Companion Remote-Control-API. Sofort-Cue; die Real-Engine
    /// feuert asynchron (spawned Thread, Connect-Timeout 500 ms), Fehler als
    /// [`super::EngineEvent::Error`].
    SendHttpPost {
        cue_id: Uuid,
        host: String,
        port: u16,
        path: String,
    },
    /// Script-Cue („Script Cue"): führt `script` (JavaScript) in einer
    /// eigenen WebView-Instanz aus (ohne Stage, unsichtbar). Nach dem Laden
    /// (Script-Ende) kommt `Finished` – der Cue ist damit eine Play-artige
    /// Unit. Ohne WebView-Binary → `EngineEvent::Error`.
    RunScript { cue_id: Uuid, script: String },
    /// Health-Check: Engine antwortet mit [`super::EngineEvent::Pong`].
    /// Dient Stress-Tests zur verlässlichen "keine Message-Drops"-Prüfung.
    Ping(u64),
}
