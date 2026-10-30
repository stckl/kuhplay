//! Trait-Definition der Playback-Engine und ihrer Rückmelde-Events.

use std::time::Duration;

use uuid::Uuid;

use crate::core::{FadeParam, ShowResult};

use super::EngineCommand;

/// Rückmeldungen der Engine an die UI (asynchron, thread-sicher).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// Wiedergabe gestartet (nach Ablauf von Pre-Wait).
    Started { cue_id: Uuid },
    /// Wiedergabe pausiert.
    Paused { cue_id: Uuid },
    /// Wiedergabe fortgesetzt.
    Resumed { cue_id: Uuid },
    /// Natürliches Ende der Wiedergabe (alle Loops/Slices durchlaufen).
    Finished { cue_id: Uuid },
    /// Explizites Stoppen durch Stop/StopAll.
    Stopped { cue_id: Uuid },
    /// Post-Wait nach `Finished` ist abgelaufen (Auto-Continue-Kandidat).
    PostWaitElapsed { cue_id: Uuid },
    /// Fortschritts-Update: Medien-Timecode (bei Sprüngen/Frame-Drops angepasst).
    Position { cue_id: Uuid, position: Duration },
    /// Eine Loop-Iteration bzw. ein Slice (inkl. Wiederholung) wurde
    /// abgeschlossen (1-basiert).
    LoopIteration { cue_id: Uuid, iteration: u32 },
    /// Aktueller Fade-Wert des Parameters `param` (bei jeder Änderung gesendet).
    FadeProgress {
        cue_id: Uuid,
        param: FadeParam,
        value: f32,
    },
    /// Simulierte Frame-Drops (nur Mock-Engine): Wiedergabe hinkt der Wanduhr
    /// um `total_dropped` hinterher.
    FrameDrop {
        cue_id: Uuid,
        dropped_frames: u32,
        total_dropped: Duration,
    },
    /// Timecode-Chasing: Exakter Sprung nach externem Timecode-Eingang
    /// (LTC/MTC) – `from`/`to` sind Medien-Positionen.
    TimecodeJump {
        cue_id: Uuid,
        from: Duration,
        to: Duration,
    },
    /// Medium wurde vorgeladen (`EngineCommand::Load`).
    Loaded { cue_id: Uuid },
    /// Stage wurde konfiguriert (`EngineCommand::ConfigureStage`).
    StageConfigured { stage_name: String },
    /// HTML5-Instanz wurde geladen und ist aktiv (nach Pre-Wait, zusammen
    /// mit [`EngineEvent::Started`]). Ab jetzt empfängt sie `EvalJs`/`PushJson`.
    HtmlLoaded { cue_id: Uuid },
    /// JavaScript wurde in der WebView-Instanz ausgeführt
    /// (`EngineCommand::EvalJs`). `at` = exakter Engine-Zeitpunkt (Clock).
    JsExecuted {
        cue_id: Uuid,
        script: String,
        at: Duration,
    },
    /// JSON-Payload wurde an die WebView-Zielinstanz zugestellt
    /// (`EngineCommand::PushJson`). `at` = exakter Engine-Zeitpunkt (Clock).
    JsonReceived {
        target: Uuid,
        payload: serde_json::Value,
        at: Duration,
    },
    /// Diashow hat das Dia gewechselt (Auto-Advance oder `SlideNav`).
    /// `at` = exakter Engine-Zeitpunkt (Clock).
    SlideChanged {
        cue_id: Uuid,
        index: usize,
        at: Duration,
    },
    /// Audio-Ausgabegerät wurde gewechselt (`None` = System-Standard).
    AudioOutputChanged { device: Option<String> },
    /// Fehler; `cue_id = None` bei globalem Engine-Fehler.
    Error {
        cue_id: Option<Uuid>,
        message: String,
    },
    /// Antwort auf [`EngineCommand::Ping`] (Health-Check / No-Drop-Prüfung).
    Pong(u64),
}

/// Abstrakte Media-Engine.
///
/// Implementierungen: [`super::MockMediaEngine`] (Prototyp, virtuelle oder
/// reale Clock, simuliert Wiedergabe/Fades/Loops/Slices/Timecode-Chasing/
/// Frame-Drops, HTML5-WebViews mit JS-Bridge/JSON-Push, Diashows und Fehler),
/// künftig `GstEngine` (GStreamer), `CpalEngine` (Low-Latency-Audio) und/oder
/// `WryEngine` (WebView via `wry` für HTML5-Cues).
///
/// Die Engine läuft in einem eigenen Thread (siehe [`super::EngineHandle`]);
/// `handle` und `tick` werden ausschließlich von diesem Thread aufgerufen.
///
/// ## HTML5/JS-Bridge (WebView-Steuerung)
///
/// Über die Kommandos [`EngineCommand::PlayHtml`], [`EngineCommand::EvalJs`]
/// und [`EngineCommand::PushJson`] steuert die Engine WebView-Instanzen:
/// `EvalJs` entspricht `webview.eval_js(script)`, `PushJson` injiziert einen
/// JSON-Payload in den JS-Execution-Context (z. B. Aufruf von
/// `window.onShowControlData(json)`). Eine produktive `WryEngine` mappt dies
/// auf `wry::WebView::evaluate_script` bzw. ein vorab injiziertes
/// Bridge-Script – ohne Änderung an Domain oder UI.
pub trait MediaEngine: Send {
    /// Anzeigename (für Status/Logs).
    fn name(&self) -> &'static str;

    /// Verarbeitet ein Kommando. Darf nicht blockieren – rechenintensive
    /// Arbeit gehört in einen Backend-eigenen Thread/Task.
    fn handle(&mut self, command: EngineCommand) -> ShowResult<()>;

    /// Periodischer Tick (Timer, Bus-Polling, Fortschritts-Checks).
    /// Default-Implementierung ist ein No-Op.
    fn tick(&mut self) {}

    /// Aufräumen beim Herunterfahren (Default: nichts).
    fn shutdown(&mut self) {}
}
