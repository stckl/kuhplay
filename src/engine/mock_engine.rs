//! [`MockMediaEngine`] und thread-sicherer [`EngineHandle`] (Commander).
//!
//! Die MockMediaEngine simuliert Wiedergabe vollständig virtuell – keine
//! Hardware, keine echten Codecs. Sie ist die Referenzimplementierung für
//! [`MediaEngine`] und deckt alle Timing-Features ab:
//!
//! * **Pre-Wait/Post-Wait** mit Millisekunden-Präzision (über [`Clock`])
//! * **Loops** (`Once` / `Count(n)` / `Infinite`) mit `LoopIteration`-Events
//! * **Slices**: Segment-Plan mit individuellen Wiederholungszählern
//!   (überschreibt `loop_mode`/`duration`)
//! * **Fades** (Volume/Rate/Opacity) mit `FadeProgress`-Events – linear oder
//!   dB-interpoliert (gehörrichtig)
//! * **Timecode-Chasing** (LTC/MTC): `receive_timecode` simuliert externe
//!   Clock-Eingaben; Sprünge > 1 Frame seek *exakt* (vorwärts/rückwärts)
//! * **HTML5-Cues & JS-Bridge:** `PlayHtml` aktiviert eine virtuelle
//!   WebView-Instanz (nach Pre-Wait, `HtmlLoaded`); `EvalJs` und `PushJson`
//!   werden protokolliert und mit exaktem Engine-Zeitstempel als
//!   `JsExecuted`/`JsonReceived` quittiert
//! * **Diashows:** `PlaySlideShow` spielt Slides mit Auto-Advance (exakte
//!   Wechsel-Zeitpunkte via `SlideChanged`), `SlideNav` schaltet manuell
//! * **Devamp** (Loops sofort beenden), **Load** (Preload), **ConfigureStage**
//! * **Timecode/Position** pro Tick, `Seek` (Sprung), `simulate_frame_drop`
//! * **Fehler-Simulation:** fehlende Dateien (auch HTML-/Bild-Pfade), leere
//!   (korrupte) Container, Treiberausfälle, ungültige Slices/Routes/
//!   HTML-Quellen → `EngineEvent::Error`
//!
//! ## Deterministische Tests
//!
//! Mit [`VirtualClock`] lässt sich die Engine manuell takten:
//! [`MockMediaEngine::advance_time`] stellt die Uhr vor und führt einen Tick
//! aus. So verifizieren die Integrationstests exakte Millisekunden-Grenzen.
//!
//! ## Threading
//!
//! [`spawn_mock_engine`] startet einen dedizierten Thread, der per
//! `crossbeam_channel::select!` Kommandos und einen 50-ms-Tick verarbeitet.
//! UI und Engine teilen ausschließlich die Kanäle in [`EngineHandle`] –
//! der UI-Thread blockiert nie.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::core::{
    db_to_linear, linear_to_db_clamped, FadeCurve, FadeParam, HtmlSource, LoopMode, ShowError,
    ShowResult, SlideNav, SlideShow, Stage, Timecode,
};

use super::clock::{Clock, RealClock, VirtualClock};
use super::frame::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store, FrameStore,
    OpacityStore, RouteFrameStore, StageFrameStore, VideoFrame,
};
use super::{EngineCommand, EngineEvent, MediaEngine};

/// Tick-Intervall der Engine (nur RealClock/Thread-Modus).
const TICK: Duration = Duration::from_millis(50);
/// Simulierte Laufzeit, wenn der Cue keine `duration` vorgibt.
const DEFAULT_PLAYBACK: Duration = Duration::from_secs(5);

/// Thread-sichere Schnittstelle zur Engine (Commander).
///
/// Über `commands` sendet die UI Kommandos (UI → Engine), über `events`
/// empfängt sie Rückmeldungen (Engine → UI, non-blocking via `try_recv`).
pub struct EngineHandle {
    /// Kanal für Kommandos an die Engine.
    pub commands: Sender<EngineCommand>,
    /// Kanal für Events von der Engine.
    pub events: Receiver<EngineEvent>,
    /// Neuestes dekodiertes Video-Frame pro Cue (Real-Engine; Mock: leer).
    pub frames: FrameStore,
    /// Aktuelle Deckkraft (Opacity) pro Cue – wird von Fades aktualisiert.
    pub opacities: OpacityStore,
    /// Kompositioniertes Stage-Canvas pro Stage-Name (Compositor).
    pub stage_frames: StageFrameStore,
    /// Gerendertes Route-Bild pro Route-ID (Real-Engine; Mock: leer).
    pub route_frames: RouteFrameStore,
    /// Cue-IDs mit aktiver Audio-Voice (Real-Engine; Mock: leer). Video-Cues
    /// legen ebenfalls eine Voice an (Tonspur des Videos).
    pub audio_voices: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<Uuid>>>,
}

impl EngineHandle {
    /// Sendet ein Kommando an die Engine (non-blocking, unbounded channel).
    pub fn send(&self, command: EngineCommand) -> ShowResult<()> {
        self.commands
            .send(command)
            .map_err(|err| ShowError::Engine(err.to_string()))
    }

    /// Holt ein Event, falls vorhanden (nie blockierend).
    pub fn try_recv_event(&self) -> Option<EngineEvent> {
        self.events.try_recv().ok()
    }

    /// Neuestes Video-Frame eines Cues (Real-Engine), falls vorhanden.
    pub fn latest_frame(&self, cue_id: Uuid) -> Option<Arc<VideoFrame>> {
        self.frames.lock().unwrap().get(&cue_id).cloned()
    }

    /// Läuft für diesen Cue eine Audio-Voice (Audio- oder Video-Cue mit Ton)?
    pub fn has_audio_voice(&self, cue_id: Uuid) -> bool {
        self.audio_voices.lock().unwrap().contains(&cue_id)
    }

    /// Aktuelle Deckkraft (Opacity) eines Cues (Default 1.0 = voll deckend).
    pub fn opacity(&self, cue_id: Uuid) -> f32 {
        self.opacities
            .lock()
            .unwrap()
            .get(&cue_id)
            .copied()
            .unwrap_or(1.0)
    }

    /// Aktuelles kompositioniertes Stage-Canvas (Compositor).
    pub fn latest_stage_frame(&self, stage: &str) -> Option<Arc<VideoFrame>> {
        self.stage_frames.lock().unwrap().get(stage).cloned()
    }

    /// Aktuelles Route-Bild (Video-Routing; Real-Engine rendert, Mock leer).
    pub fn latest_route_frame(&self, route: Uuid) -> Option<Arc<VideoFrame>> {
        self.route_frames.lock().unwrap().get(&route).cloned()
    }
}

/// Startet die MockMediaEngine in einem eigenen Thread (RealClock) und gibt
/// das Handle zurück.
pub fn spawn_mock_engine() -> EngineHandle {
    spawn_mock_engine_with_join().0
}

/// Wie [`spawn_mock_engine`], liefert zusätzlich das [`std::thread::JoinHandle`]
/// – für Tests, die den sauberen Shutdown des Engine-Threads verifizieren.
pub fn spawn_mock_engine_with_join() -> (EngineHandle, std::thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
    let (evt_tx, evt_rx) = crossbeam_channel::unbounded::<EngineEvent>();

    let join = std::thread::Builder::new()
        .name("playback-engine".to_string())
        .spawn(move || run_engine_loop(MockMediaEngine::new(RealClock::new(), evt_tx), cmd_rx))
        .expect("playback engine thread must spawn");

    (
        EngineHandle {
            commands: cmd_tx,
            events: evt_rx,
            frames: new_frame_store(),
            opacities: new_opacity_store(),
            stage_frames: new_stage_frame_store(),
            route_frames: new_route_frame_store(),
            audio_voices: Default::default(),
        },
        join,
    )
}

/// Engine-Loop: verarbeitet Kommandos und Ticks, bis alle Sender geschlossen sind.
fn run_engine_loop<C: Clock>(mut engine: MockMediaEngine<C>, commands: Receiver<EngineCommand>) {
    debug!(engine = engine.name(), "engine loop started");
    let ticker = crossbeam_channel::tick(TICK);
    loop {
        crossbeam_channel::select! {
            recv(commands) -> msg => match msg {
                Ok(command) => {
                    if let Err(err) = engine.handle(command) {
                        warn!(%err, "engine command failed");
                    }
                }
                // UI wurde geschlossen → Engine herunterfahren.
                Err(_) => break,
            },
            recv(ticker) -> _ => engine.tick(),
        }
    }
    engine.shutdown();
    debug!("engine loop terminated");
}

/// Art eines laufenden Playbacks (bestimmt das Tick-Verhalten).
enum PlaybackKind {
    /// Klassisches Audio/Video-Medium.
    Media,
    /// HTML5-Cue: WebView-Instanz (JS-Bridge aktiv ab Start).
    Html(HtmlSource),
    /// Diashow-Cue: Slide-Index + Auto-Advance-Zustand.
    SlideShow(SlideShowState),
    /// Wait-Cue: reine Timer-Unit (kein Medium, kein Backend).
    Wait,
}

/// Laufzeit-Zustand einer Diashow.
struct SlideShowState {
    /// Definition (Slides, Crossfade, Auto-Advance, Loop).
    show: SlideShow,
    /// Aktueller Slide-Index.
    index: usize,
    /// Zeitpunkt des Starts (nach Pre-Wait) – Basis für Auto-Advance.
    #[allow(dead_code)]
    started_at: Duration,
    /// Nächster automatischer Wechsel (nur bei `auto_advance`).
    next_advance_at: Option<Duration>,
}

/// Eine laufende (simulierte) WebView-Instanz eines HTML5-Cues.
struct ActiveHtml {
    /// Geladene Quelle (eine produktive Engine rendert sie via `wry`).
    source: HtmlSource,
    /// Ziel-Stage (eine produktive Engine routet hierhin).
    #[allow(dead_code)]
    stage: Option<String>,
    /// Video-Layer (Reihenfolge der Überlagerung).
    #[allow(dead_code)]
    layer: u32,
    /// Zeitpunkt der Aktivierung (nach Pre-Wait).
    #[allow(dead_code)]
    started_at: Duration,
    /// Protokollierte JS-Trigger: (Zeitpunkt, Script).
    js_log: Vec<(Duration, String)>,
    /// Protokollierte JSON-Pushes: (Zeitpunkt, Payload).
    json_log: Vec<(Duration, serde_json::Value)>,
}

/// Ein laufender (simulierter) Playback.
struct ActivePlayback {
    #[allow(dead_code)]
    path: Option<String>,
    #[allow(dead_code)]
    route: Option<crate::core::AudioRoute>,
    /// Ziel-Stage (Video/HTML5/Diashow – eine produktive Engine routet hierhin).
    stage: Option<String>,
    /// Video-Layer (Reihenfolge der Überlagerung).
    layer: u32,
    /// Art des Playbacks (Media/HTML5/Diashow).
    kind: PlaybackKind,
    /// Zeitpunkt des Command-Eingangs (Basis für Pre-Wait).
    received_at: Duration,
    pre_wait: Duration,
    /// `None` während Pre-Wait oder Pause (akkumulierte Zeit eingefroren).
    started_at: Option<Duration>,
    /// Akkumulierte Laufzeit vor Pausen/Seek (Position innerhalb der Unit).
    accumulated: Duration,
    paused_since: Option<Duration>,
    /// Dauer einer Loop-Iteration (`ZERO` → [`DEFAULT_PLAYBACK`]).
    duration: Duration,
    loop_mode: LoopMode,
    /// Slice-Plan (abgeflachte Segment-Dauern, inkl. Wiederholungen).
    /// Leer → `duration`/`loop_mode` gelten.
    plan: Vec<Duration>,
    /// Index in `plan` (aktuelles Segment).
    plan_index: usize,
    /// Anzahl abgeschlossener Iterationen/Segmente.
    iterations_done: u32,
    /// Durch Frame-Drops verlorene Zeit (Wiedergabe hinkt hinterher).
    dropped: Duration,
    /// Zeitpunkt des `Finished`-Events (Basis für Post-Wait).
    finished_at: Option<Duration>,
    post_wait: Duration,
    post_wait_done: bool,
}

impl ActivePlayback {
    /// Vergangene Wanduhr-Zeit (Pausen eingefroren).
    fn wall_elapsed(&self, now: Duration) -> Duration {
        match (self.started_at, self.paused_since) {
            (Some(start), None) => self.accumulated + now.saturating_sub(start),
            _ => self.accumulated,
        }
    }

    /// Medien-Timecode (Frame-Drops abgezogen, nie negativ).
    fn media_position(&self, now: Duration) -> Duration {
        self.wall_elapsed(now).saturating_sub(self.dropped)
    }

    /// Dauer der aktuellen Einheit: Slice-Plan-Eintrag, Loop-Iteration,
    /// Diashow-Gesamtdauer oder explizite Cue-Duration.
    fn current_unit(&self) -> Duration {
        if matches!(self.kind, PlaybackKind::Wait) {
            // Wait-Cue: explizite Dauer (0 = sofortiges Ende), nie Default.
            return self.duration;
        }
        if !self.plan.is_empty() {
            self.plan
                .get(self.plan_index)
                .copied()
                .unwrap_or(DEFAULT_PLAYBACK)
        } else if let PlaybackKind::SlideShow(state) = &self.kind {
            if !self.duration.is_zero() {
                // Explizite Cue-Duration begrenzt die Show.
                self.duration
            } else if !state.show.auto_advance || state.show.loop_slides {
                // Manuell oder endlos loopend: läuft bis Stop/Devamp.
                Duration::MAX
            } else {
                // Auto-Advance ohne Loop: Summe der Hold-Zeiten.
                state.show.total_duration()
            }
        } else if self.duration.is_zero() {
            DEFAULT_PLAYBACK
        } else {
            self.duration
        }
    }

    /// Startet eine neue Iteration/Einheit (Loop-Neustart, Slice-Wechsel):
    /// Position zurücksetzen und Diashows auf Slide 0 neu beginnen.
    fn restart_unit(&mut self, now: Duration) {
        self.accumulated = Duration::ZERO;
        self.dropped = Duration::ZERO;
        self.started_at = Some(now);
        if let PlaybackKind::SlideShow(state) = &mut self.kind {
            state.index = 0;
            state.started_at = now;
            state.next_advance_at = if state.show.auto_advance {
                Some(now + state.show.hold_at(0))
            } else {
                None
            };
        }
    }
}

/// Ein laufender Fade.
struct ActiveFade {
    param: FadeParam,
    from: f32,
    target: f32,
    duration: Duration,
    curve: FadeCurve,
    started_at: Duration,
    last_value: f32,
}

/// Simulierte Media-Engine ohne Hardware-Abhängigkeit.
///
/// Generisch über [`Clock`]: [`RealClock`] für den Produktiv-Thread,
/// [`VirtualClock`] für deterministische Tests.
pub struct MockMediaEngine<C: Clock = RealClock> {
    clock: C,
    active: HashMap<Uuid, ActivePlayback>,
    fades: HashMap<Uuid, ActiveFade>,
    /// Timecode-Chasing: Cue-ID → Timecode-In (Offset der externen Clock).
    chasing: HashMap<Uuid, Timecode>,
    /// Konfigurierte Video-Stages (Name → Stage).
    stages: HashMap<String, Stage>,
    /// Aktive WebView-Instanzen der HTML5-Cues (Cue-ID → Instanz).
    html: HashMap<Uuid, ActiveHtml>,
    events: Sender<EngineEvent>,
    /// Wenn `true`, werden Medienpfade gegen das Dateisystem geprüft
    /// (fehlende/leere Dateien → `EngineEvent::Error`).
    validate_paths: bool,
    /// Wenn `true`, schlägt jedes Play fehl (Treibersimulation).
    simulate_driver_failure: bool,
}

impl<C: Clock> MockMediaEngine<C> {
    /// Neue Engine mit gegebener Uhr und Event-Sender.
    pub fn new(clock: C, events: Sender<EngineEvent>) -> Self {
        Self {
            clock,
            active: HashMap::new(),
            fades: HashMap::new(),
            chasing: HashMap::new(),
            stages: HashMap::new(),
            html: HashMap::new(),
            events,
            validate_paths: false,
            simulate_driver_failure: false,
        }
    }

    /// Aktiviert die Dateisystem-Validierung (für Resilienz-Tests).
    pub fn with_path_validation(mut self) -> Self {
        self.validate_paths = true;
        self
    }

    /// Aktiviert die Treiberausfall-Simulation (für Resilienz-Tests).
    pub fn with_driver_failure(mut self) -> Self {
        self.simulate_driver_failure = true;
        self
    }

    /// Anzahl aktiver Playbacks (für Tests/Debugging).
    pub fn active_playback_count(&self) -> usize {
        self.active.len()
    }

    /// Anzahl der Cues mit aktivem Timecode-Chasing.
    pub fn chasing_count(&self) -> usize {
        self.chasing.len()
    }

    /// Ob eine Stage mit diesem Namen konfiguriert wurde.
    pub fn has_stage(&self, name: &str) -> bool {
        self.stages.contains_key(name)
    }

    /// Anzahl aktiver WebView-Instanzen (HTML5-Cues).
    pub fn html_instance_count(&self) -> usize {
        self.html.len()
    }

    /// Geladene Quelle einer aktiven WebView-Instanz (Test-Introspektion).
    pub fn html_source(&self, cue_id: Uuid) -> Option<&HtmlSource> {
        self.html.get(&cue_id).map(|h| &h.source)
    }

    /// Protokollierte JS-Trigger einer WebView-Instanz: (Zeitpunkt, Script).
    pub fn html_js_log(&self, cue_id: Uuid) -> Option<&[(Duration, String)]> {
        self.html.get(&cue_id).map(|h| h.js_log.as_slice())
    }

    /// Protokollierte JSON-Pushes einer WebView-Instanz: (Zeitpunkt, Payload).
    pub fn html_json_log(&self, cue_id: Uuid) -> Option<&[(Duration, serde_json::Value)]> {
        self.html.get(&cue_id).map(|h| h.json_log.as_slice())
    }

    fn emit(&self, event: EngineEvent) {
        // UI evtl. schon geschlossen → Fehler bewusst ignorieren.
        let _ = self.events.send(event);
    }

    /// Prüft Medienverfügbarkeit. `false` → Error-Event wurde bereits gesendet.
    fn validate_media(&self, cue_id: Uuid, path: &Option<String>) -> bool {
        if self.simulate_driver_failure {
            self.emit(EngineEvent::Error {
                cue_id: Some(cue_id),
                message: "simulated driver failure: audio/video device unavailable".to_string(),
            });
            return false;
        }
        if !self.validate_paths {
            return true;
        }
        if let Some(p) = path {
            let path = Path::new(p);
            if !path.exists() {
                self.emit(EngineEvent::Error {
                    cue_id: Some(cue_id),
                    message: format!("media file not found: {p}"),
                });
                return false;
            }
            match path.metadata() {
                Ok(meta) if meta.len() == 0 => {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: format!("corrupted media file (empty container): {p}"),
                    });
                    false
                }
                Ok(_) => true,
                Err(err) => {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: format!("media file unreadable: {err}"),
                    });
                    false
                }
            }
        } else {
            true
        }
    }

    /// Simuliert Frame-Drops: Die Wiedergabe-Position fällt um
    /// `dropped_frames * frame_duration` hinter die Wanduhr zurück – die
    /// Wiedergabe dauert entsprechend länger. Emittiert `FrameDrop`.
    pub fn simulate_frame_drop(
        &mut self,
        cue_id: Uuid,
        dropped_frames: u32,
        frame_duration: Duration,
    ) {
        if let Some(playback) = self.active.get_mut(&cue_id) {
            if playback.started_at.is_some() && playback.finished_at.is_none() {
                playback.dropped += frame_duration * dropped_frames;
                let total_dropped = playback.dropped;
                self.emit(EngineEvent::FrameDrop {
                    cue_id,
                    dropped_frames,
                    total_dropped,
                });
            }
        }
    }

    /// Beendet einen Playback sofort (Devamp): `Finished` + ggf. sofortiges
    /// `PostWaitElapsed` (bei post_wait = 0) – exakt wie ein natürliches Ende.
    fn finish_now(&mut self, cue_id: Uuid) {
        let now = self.clock.now();
        let post_wait = match self.active.get_mut(&cue_id) {
            Some(p) if p.finished_at.is_none() => {
                p.finished_at = Some(now);
                p.post_wait
            }
            _ => return,
        };
        self.emit(EngineEvent::Finished { cue_id });
        if post_wait.is_zero() {
            self.emit(EngineEvent::PostWaitElapsed { cue_id });
            self.active.remove(&cue_id);
            self.chasing.remove(&cue_id);
            self.html.remove(&cue_id);
        }
    }

    /// Simuliert einen externen Timecode-Eingang (LTC/MTC).
    ///
    /// **Chasing:** Für jeden chasing Cue wird die Ziel-Medienposition
    /// `tc - timecode_in` berechnet. Weicht sie um mehr als einen Frame von
    /// der aktuellen Position ab (Sprung vorwärts *oder* rückwärts), springt
    /// das Medium *exakt* dorthin (`TimecodeJump` + `Position`). Kleinere
    /// Abweichungen (Drift ≤ 1 Frame) werden ausgesessen – kein Wasserfall.
    pub fn receive_timecode(&mut self, tc: Timecode) {
        let frame = tc.fps.frame_duration();
        let now = self.clock.now();
        let mut jumps: Vec<(Uuid, Duration, Duration)> = Vec::new();
        for (cue_id, offset) in &self.chasing {
            let Some(playback) = self.active.get_mut(cue_id) else {
                continue;
            };
            if playback.started_at.is_none() || playback.finished_at.is_some() {
                continue;
            }
            let target = tc.to_duration().saturating_sub(offset.to_duration());
            let current = playback.media_position(now);
            // Exakter Vergleich in Nanosekunden (Duration) – f64 wäre bei
            // genau einem Frame Toleranz durch Rundung unzuverlässig.
            let delta = target.abs_diff(current);
            if delta > frame {
                let clamped = target.min(playback.current_unit());
                playback.accumulated = clamped;
                playback.dropped = Duration::ZERO;
                playback.started_at = Some(now);
                jumps.push((*cue_id, current, clamped));
            }
        }
        for (cue_id, from, to) in jumps {
            debug!(%cue_id, ?from, ?to, "timecode chase jump");
            self.emit(EngineEvent::TimecodeJump { cue_id, from, to });
            self.emit(EngineEvent::Position {
                cue_id,
                position: to,
            });
        }
    }
}

impl MockMediaEngine<VirtualClock> {
    /// Engine mit virtueller Uhr: `(engine, clock_handle)`.
    pub fn with_virtual_clock(events: Sender<EngineEvent>) -> (Self, VirtualClock) {
        let clock = VirtualClock::new();
        (Self::new(clock.clone(), events), clock)
    }

    /// Stellt die virtuelle Uhr um `by` vor und führt einen Tick aus.
    /// Deterministischer Taktgeber für Timing-Tests.
    pub fn advance_time(&mut self, by: Duration) {
        self.clock.advance(by);
        self.tick();
    }

    /// Setzt die virtuelle Uhr auf einen absoluten Zeitpunkt und ticked.
    pub fn set_time(&mut self, t: Duration) {
        self.clock.set(t);
        self.tick();
    }
}

impl<C: Clock> MediaEngine for MockMediaEngine<C> {
    fn name(&self) -> &'static str {
        "mock-media-engine"
    }

    fn handle(&mut self, command: EngineCommand) -> ShowResult<()> {
        let now = self.clock.now();
        match command {
            EngineCommand::Play {
                cue_id,
                path,
                duration,
                pre_wait,
                post_wait,
                loop_mode,
                slices,
                route,
                timecode_in,
                stage,
                layer,
                // Surface-Geometrie (AoI/Hard-Edge) ist Real-Engine-Compositor;
                // die Mock-Engine bildet keine Pixel ab und ignoriert sie.
                surface: _,
            } => {
                debug!(%cue_id, ?path, ?duration, ?pre_wait, ?post_wait, ?loop_mode, "play");
                // Slices und Route validieren (defekte Parameter → Error).
                for slice in &slices {
                    if let Err(err) = slice.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
                if let Some(route) = &route {
                    if let Err(err) = route.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
                if !self.validate_media(cue_id, &path) {
                    return Ok(());
                }
                // Slice-Plan abflachen (Segment-Dauer × Play Count).
                let plan: Vec<Duration> = slices
                    .iter()
                    .flat_map(|s| std::iter::repeat_n(s.duration(), s.play_count.max(1) as usize))
                    .collect();
                if let Some(tc) = timecode_in {
                    self.chasing.insert(cue_id, tc);
                }
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        path,
                        route,
                        stage,
                        layer,
                        kind: PlaybackKind::Media,
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration: duration.unwrap_or(Duration::ZERO),
                        loop_mode,
                        plan,
                        plan_index: 0,
                        iterations_done: 0,
                        dropped: Duration::ZERO,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                    },
                );
            }
            EngineCommand::Wait {
                cue_id,
                duration,
                pre_wait,
                post_wait,
            } => {
                debug!(%cue_id, ?duration, ?pre_wait, ?post_wait, "wait");
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        path: None,
                        route: None,
                        stage: None,
                        layer: 0,
                        kind: PlaybackKind::Wait,
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration,
                        loop_mode: LoopMode::Once,
                        plan: Vec::new(),
                        plan_index: 0,
                        iterations_done: 0,
                        dropped: Duration::ZERO,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                    },
                );
            }
            EngineCommand::Pause { cue_id } => {
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    if let Some(start) = playback.started_at {
                        if playback.paused_since.is_none() && playback.finished_at.is_none() {
                            // Laufzeit einfrieren.
                            playback.accumulated += now.saturating_sub(start);
                            playback.started_at = None;
                            playback.paused_since = Some(now);
                            self.emit(EngineEvent::Paused { cue_id });
                        }
                    }
                }
            }
            EngineCommand::Resume { cue_id } => {
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    if playback.paused_since.is_some() {
                        playback.paused_since = None;
                        playback.started_at = Some(now);
                        self.emit(EngineEvent::Resumed { cue_id });
                    }
                }
            }
            EngineCommand::Stop { cue_id } => {
                if self.active.remove(&cue_id).is_some() {
                    self.chasing.remove(&cue_id);
                    self.html.remove(&cue_id);
                    self.emit(EngineEvent::Stopped { cue_id });
                }
            }
            EngineCommand::StopAll => {
                let stopped: Vec<Uuid> = self.active.keys().copied().collect();
                self.active.clear();
                self.fades.clear();
                self.chasing.clear();
                self.html.clear();
                for cue_id in stopped {
                    self.emit(EngineEvent::Stopped { cue_id });
                }
            }
            EngineCommand::Fade {
                cue_id,
                target_cue: _,
                param,
                from,
                target,
                duration,
                curve,
            } => {
                debug!(%cue_id, ?param, from, target, ?duration, ?curve, "fade");
                if duration.is_zero() {
                    // Sofort-Fade.
                    self.emit(EngineEvent::FadeProgress {
                        cue_id,
                        param,
                        value: target,
                    });
                    self.emit(EngineEvent::Finished { cue_id });
                } else {
                    self.fades.insert(
                        cue_id,
                        ActiveFade {
                            param,
                            from,
                            target,
                            duration,
                            curve,
                            started_at: now,
                            last_value: from,
                        },
                    );
                }
            }
            EngineCommand::Seek { cue_id, position } => {
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    if playback.finished_at.is_none() {
                        playback.accumulated = position;
                        playback.dropped = Duration::ZERO;
                        if playback.paused_since.is_none() {
                            playback.started_at = Some(now);
                        }
                        debug!(%cue_id, ?position, "seek");
                        self.emit(EngineEvent::Position { cue_id, position });
                    }
                }
            }
            EngineCommand::SetVolume { cue_id, volume } => {
                debug!(%cue_id, volume, "set volume (not simulated)");
            }
            EngineCommand::Load { cue_id, path } => {
                debug!(%cue_id, ?path, "load (preload)");
                if !self.validate_media(cue_id, &path) {
                    return Ok(());
                }
                self.emit(EngineEvent::Loaded { cue_id });
            }
            EngineCommand::Devamp { cue_id } => {
                debug!(%cue_id, "devamp: finish immediately");
                self.finish_now(cue_id);
            }
            EngineCommand::DevampAll => {
                debug!("devamp all: finish all playbacks immediately");
                let ids: Vec<Uuid> = self.active.keys().copied().collect();
                for cue_id in ids {
                    self.finish_now(cue_id);
                }
            }
            EngineCommand::ChaseTimecode {
                cue_id,
                timecode_in,
            } => {
                debug!(%cue_id, ?timecode_in, "chase timecode");
                match timecode_in {
                    Some(tc) => {
                        self.chasing.insert(cue_id, tc);
                    }
                    None => {
                        self.chasing.remove(&cue_id);
                    }
                }
            }
            EngineCommand::SetAudioPatch { .. } => {}
            EngineCommand::SetAudioOutput { device } => {
                self.emit(EngineEvent::AudioOutputChanged { device });
            }
            EngineCommand::ConfigureStage { stage } => {
                debug!(stage = %stage.name, "configure stage");
                match stage.validate() {
                    Ok(()) => {
                        let name = stage.name.clone();
                        self.stages.insert(name.clone(), stage);
                        self.emit(EngineEvent::StageConfigured { stage_name: name });
                    }
                    Err(err) => {
                        self.emit(EngineEvent::Error {
                            cue_id: None,
                            message: err.to_string(),
                        });
                    }
                }
            }
            EngineCommand::ConfigureRoutes { routes } => {
                // Mock rendert keine Route-Bilder; Kommando wird nur
                // protokolliert und validiert (Parität zur Real-Engine).
                debug!(count = routes.len(), "configure routes (mock)");
                for route in routes.routes() {
                    if let Err(err) = route.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: None,
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
            }
            EngineCommand::PlayHtml {
                cue_id,
                source,
                stage,
                layer,
                duration,
                pre_wait,
                post_wait,
            } => {
                debug!(%cue_id, source = %source.describe(), ?stage, layer, "play html");
                if let Err(err) = source.validate() {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: err.to_string(),
                    });
                    return Ok(());
                }
                // Datei-Quellen gegen das Dateisystem prüfen (Resilienz).
                if self.validate_paths {
                    if let HtmlSource::File(path) = &source {
                        let path = path.display().to_string();
                        if !self.validate_media(cue_id, &Some(path)) {
                            return Ok(());
                        }
                    }
                }
                // Die WebView-Instanz wird erst beim Start (nach Pre-Wait)
                // aktiv – siehe tick().
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        path: None,
                        route: None,
                        stage,
                        layer,
                        kind: PlaybackKind::Html(source),
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration: duration.unwrap_or(Duration::ZERO),
                        loop_mode: LoopMode::Once,
                        plan: Vec::new(),
                        plan_index: 0,
                        iterations_done: 0,
                        dropped: Duration::ZERO,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                    },
                );
            }
            EngineCommand::PlaySlideShow {
                cue_id,
                show,
                stage,
                layer,
                duration,
                pre_wait,
                post_wait,
                // Surface-Geometrie: siehe Play (Real-Engine-Compositor).
                surface: _,
            } => {
                debug!(%cue_id, slides = show.slide_count(), "play slideshow");
                if let Err(err) = show.validate() {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: err.to_string(),
                    });
                    return Ok(());
                }
                // Bildpfade gegen das Dateisystem prüfen (Resilienz).
                if self.validate_paths {
                    for slide in &show.slides {
                        let path = slide.path.display().to_string();
                        if !self.validate_media(cue_id, &Some(path)) {
                            return Ok(());
                        }
                    }
                }
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        path: None,
                        route: None,
                        stage,
                        layer,
                        kind: PlaybackKind::SlideShow(SlideShowState {
                            show,
                            index: 0,
                            started_at: Duration::ZERO,
                            next_advance_at: None,
                        }),
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration: duration.unwrap_or(Duration::ZERO),
                        loop_mode: LoopMode::Once,
                        plan: Vec::new(),
                        plan_index: 0,
                        iterations_done: 0,
                        dropped: Duration::ZERO,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                    },
                );
            }
            EngineCommand::EvalJs { cue_id, script } => {
                debug!(%cue_id, "eval js");
                if script.trim().is_empty() {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: "eval_js: empty script".to_string(),
                    });
                    return Ok(());
                }
                // Nur laufende WebView-Instanzen empfangen JS – sonst Error.
                let mut executed = false;
                if let Some(html) = self.html.get_mut(&cue_id) {
                    html.js_log.push((now, script.clone()));
                    executed = true;
                }
                if executed {
                    self.emit(EngineEvent::JsExecuted {
                        cue_id,
                        script,
                        at: now,
                    });
                } else {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: format!(
                            "no active HTML5 instance for cue {cue_id} (not started or already stopped)"
                        ),
                    });
                }
            }
            EngineCommand::PushJson { target, payload } => {
                debug!(%target, "push json");
                let mut received = false;
                if let Some(html) = self.html.get_mut(&target) {
                    html.json_log.push((now, payload.clone()));
                    received = true;
                }
                if received {
                    self.emit(EngineEvent::JsonReceived {
                        target,
                        payload,
                        at: now,
                    });
                } else {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(target),
                        message: format!("no active HTML5 instance for target cue {target}"),
                    });
                }
            }
            EngineCommand::SlideNav { cue_id, nav } => {
                debug!(%cue_id, ?nav, "slide nav");
                let mut new_index: Option<usize> = None;
                let mut err: Option<String> = None;
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    let can_nav = playback.started_at.is_some() && playback.finished_at.is_none();
                    match &mut playback.kind {
                        PlaybackKind::SlideShow(state) if can_nav => {
                            match nav {
                                SlideNav::Next => {
                                    // Am Ende ohne Loop: auf dem letzten Dia bleiben.
                                    if let Some(next) = state.show.next_index(state.index) {
                                        state.index = next;
                                    }
                                }
                                SlideNav::Prev => {
                                    state.index = state.show.prev_index(state.index);
                                }
                            }
                            // Nächsten Auto-Advance ab dem manuell gewählten Dia.
                            state.next_advance_at = if state.show.auto_advance {
                                Some(now + state.show.hold_at(state.index))
                            } else {
                                None
                            };
                            new_index = Some(state.index);
                        }
                        _ => {
                            err = Some(format!("cue {cue_id} is not a running slideshow"));
                        }
                    }
                } else {
                    err = Some(format!("no running playback for cue {cue_id}"));
                }
                match new_index {
                    Some(index) => self.emit(EngineEvent::SlideChanged {
                        cue_id,
                        index,
                        at: now,
                    }),
                    None => self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: err.unwrap_or_default(),
                    }),
                }
            }
            EngineCommand::SendNetwork {
                cue_id,
                protocol,
                host,
                port,
                message,
            } => {
                // Sofort-Cue: die Mock-Engine sendet nichts, protokolliert aber.
                debug!(%cue_id, ?protocol, %host, port, %message, "send network (mock)");
            }
            EngineCommand::Ping(n) => {
                self.emit(EngineEvent::Pong(n));
            }
        }
        Ok(())
    }

    fn tick(&mut self) {
        let now = self.clock.now();
        // Events werden gesammelt und erst nach den Mutationen emittiert
        // (Borrow-Checker-freundlich und atomar pro Tick).
        let mut out: Vec<EngineEvent> = Vec::new();

        // Fades: Zwischenwerte + Abschluss (linear oder dB-interpoliert).
        let mut fades_done = Vec::new();
        for (cue_id, fade) in &mut self.fades {
            let elapsed = now.saturating_sub(fade.started_at);
            let progress = if fade.duration.is_zero() {
                1.0
            } else {
                (elapsed.as_secs_f32() / fade.duration.as_secs_f32()).clamp(0.0, 1.0)
            };
            let value = match fade.curve {
                FadeCurve::Linear => fade.from + (fade.target - fade.from) * progress,
                FadeCurve::Decibel => {
                    let from_db = linear_to_db_clamped(fade.from);
                    let to_db = linear_to_db_clamped(fade.target);
                    db_to_linear(from_db + (to_db - from_db) * progress)
                }
            };
            if value != fade.last_value {
                fade.last_value = value;
                out.push(EngineEvent::FadeProgress {
                    cue_id: *cue_id,
                    param: fade.param,
                    value,
                });
            }
            if progress >= 1.0 {
                fades_done.push(*cue_id);
            }
        }
        for cue_id in fades_done {
            self.fades.remove(&cue_id);
            out.push(EngineEvent::Finished { cue_id });
        }

        // Playbacks: Pre-Wait → Laufend → Loops/Slices → Finished → Post-Wait.
        let mut finished_now = Vec::new();
        let mut post_wait_elapsed = Vec::new();
        // Beim Start aktivierte WebView-Instanzen (Cue-ID, Quelle, Stage, Layer).
        let mut html_starts: Vec<(Uuid, HtmlSource, Option<String>, u32)> = Vec::new();
        for (cue_id, playback) in &mut self.active {
            // Post-Wait-Phase nach Finished.
            if let Some(finished_at) = playback.finished_at {
                if !playback.post_wait_done && now.saturating_sub(finished_at) >= playback.post_wait
                {
                    playback.post_wait_done = true;
                    post_wait_elapsed.push(*cue_id);
                }
                continue;
            }
            // Pre-Wait-Phase.
            if playback.started_at.is_none() && playback.paused_since.is_none() {
                if now.saturating_sub(playback.received_at) >= playback.pre_wait {
                    playback.started_at = Some(now);
                    out.push(EngineEvent::Started {
                        cue_id: *cue_id,
                        duration: None,
                    });
                    match &mut playback.kind {
                        PlaybackKind::Html(source) => {
                            // WebView-Instanz wird aktiv (JS-Bridge ab jetzt offen).
                            html_starts.push((
                                *cue_id,
                                source.clone(),
                                playback.stage.clone(),
                                playback.layer,
                            ));
                        }
                        PlaybackKind::SlideShow(state) => {
                            // Diashow startet auf Slide 0.
                            state.index = 0;
                            state.started_at = now;
                            state.next_advance_at = if state.show.auto_advance {
                                Some(now + state.show.hold_at(0))
                            } else {
                                None
                            };
                            out.push(EngineEvent::SlideChanged {
                                cue_id: *cue_id,
                                index: 0,
                                at: now,
                            });
                        }
                        PlaybackKind::Media => {}
                        // Wait-Cue: kein Start-Zustand, Position läuft über die Uhr.
                        PlaybackKind::Wait => {}
                    }
                } else {
                    continue;
                }
            }
            // Pausiert → nichts zu tun.
            if playback.paused_since.is_some() {
                continue;
            }

            let unit = playback.current_unit();
            let position = playback.media_position(now);
            out.push(EngineEvent::Position {
                cue_id: *cue_id,
                position: position.min(unit),
            });

            // Diashow: Auto-Advance (Zeitpunkte exakt aus dem Slide-Plan).
            if let PlaybackKind::SlideShow(state) = &mut playback.kind {
                while let Some(at) = state.next_advance_at {
                    if now < at {
                        break;
                    }
                    match state.show.next_index(state.index) {
                        Some(next) => {
                            state.index = next;
                            // Raster exakt ab dem geplanten Zeitpunkt verankern.
                            state.next_advance_at = Some(at + state.show.hold_at(next));
                            out.push(EngineEvent::SlideChanged {
                                cue_id: *cue_id,
                                index: next,
                                at: now,
                            });
                        }
                        None => {
                            // Ende ohne Loop: letzter Slide bleibt stehen.
                            state.next_advance_at = None;
                            break;
                        }
                    }
                }
            }

            if position >= unit {
                if !playback.plan.is_empty() {
                    // Slice-Plan: nächstes Segment (inkl. Wiederholungen).
                    playback.plan_index += 1;
                    playback.iterations_done += 1;
                    if playback.plan_index >= playback.plan.len() {
                        playback.finished_at = Some(now);
                        finished_now.push(*cue_id);
                        if playback.post_wait.is_zero() {
                            post_wait_elapsed.push(*cue_id);
                        }
                    } else {
                        playback.restart_unit(now);
                        if matches!(playback.kind, PlaybackKind::SlideShow(_)) {
                            out.push(EngineEvent::SlideChanged {
                                cue_id: *cue_id,
                                index: 0,
                                at: now,
                            });
                        }
                        out.push(EngineEvent::LoopIteration {
                            cue_id: *cue_id,
                            iteration: playback.iterations_done,
                        });
                    }
                } else {
                    let total = playback.loop_mode.total_iterations();
                    if playback.loop_mode.is_infinite() || playback.iterations_done + 1 < total {
                        // Nächste Loop-Iteration.
                        playback.iterations_done += 1;
                        playback.restart_unit(now);
                        if matches!(playback.kind, PlaybackKind::SlideShow(_)) {
                            out.push(EngineEvent::SlideChanged {
                                cue_id: *cue_id,
                                index: 0,
                                at: now,
                            });
                        }
                        out.push(EngineEvent::LoopIteration {
                            cue_id: *cue_id,
                            iteration: playback.iterations_done,
                        });
                    } else {
                        playback.finished_at = Some(now);
                        finished_now.push(*cue_id);
                        // Post-Wait von 0 ms gilt sofort als abgelaufen.
                        if playback.post_wait.is_zero() {
                            post_wait_elapsed.push(*cue_id);
                        }
                    }
                }
            }
        }
        // WebView-Instanzen aktivieren (nach dem Playback-Loop, damit die
        // Borrows von `active` abgelaufen sind).
        for (cue_id, source, stage, layer) in html_starts {
            let at = self.clock.now();
            self.html.insert(
                cue_id,
                ActiveHtml {
                    source,
                    stage,
                    layer,
                    started_at: at,
                    js_log: Vec::new(),
                    json_log: Vec::new(),
                },
            );
            out.push(EngineEvent::HtmlLoaded { cue_id });
        }
        for cue_id in finished_now {
            out.push(EngineEvent::Finished { cue_id });
        }
        for cue_id in post_wait_elapsed {
            out.push(EngineEvent::PostWaitElapsed { cue_id });
            self.active.remove(&cue_id);
            self.chasing.remove(&cue_id);
            self.html.remove(&cue_id);
        }

        for event in out {
            self.emit(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn play_cmd(cue_id: Uuid, duration: Option<Duration>) -> EngineCommand {
        EngineCommand::Play {
            cue_id,
            path: None,
            duration,
            pre_wait: Duration::ZERO,
            post_wait: Duration::ZERO,
            loop_mode: LoopMode::Once,
            slices: Vec::new(),
            route: None,
            timecode_in: None,
            stage: None,
            layer: 0,
            surface: None,
        }
    }

    #[test]
    fn play_finishes_after_duration() {
        let engine = spawn_mock_engine();
        let cue_id = Uuid::new_v4();
        engine
            .send(play_cmd(cue_id, Some(Duration::from_millis(120))))
            .expect("send must succeed");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut saw_started = false;
        let mut saw_finished = false;
        while Instant::now() < deadline && !saw_finished {
            match engine.try_recv_event() {
                Some(EngineEvent::Started { cue_id: id, .. }) => {
                    assert_eq!(id, cue_id);
                    saw_started = true;
                }
                Some(EngineEvent::Finished { cue_id: id }) => {
                    assert_eq!(id, cue_id);
                    saw_finished = true;
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(saw_started, "engine must report Started");
        assert!(saw_finished, "engine must report Finished");
    }

    #[test]
    fn stop_all_reports_stopped_for_each_playback() {
        let engine = spawn_mock_engine();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        for cue_id in [a, b] {
            engine.send(play_cmd(cue_id, None)).unwrap();
        }
        engine.send(EngineCommand::StopAll).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stopped = Vec::new();
        while Instant::now() < deadline && stopped.len() < 2 {
            if let Some(EngineEvent::Stopped { cue_id }) = engine.try_recv_event() {
                stopped.push(cue_id);
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        assert!(stopped.contains(&a) && stopped.contains(&b));
    }

    #[test]
    fn ping_is_answered_with_pong() {
        let engine = spawn_mock_engine();
        engine.send(EngineCommand::Ping(42)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(EngineEvent::Pong(42)) = engine.try_recv_event() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("engine must answer Ping with Pong");
    }

    #[test]
    fn load_emits_loaded_and_devamp_finishes_infinite_loop() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);

        let id = Uuid::new_v4();
        engine
            .handle(EngineCommand::Load {
                cue_id: id,
                path: None,
            })
            .unwrap();
        assert!(rx
            .try_iter()
            .any(|e| matches!(e, EngineEvent::Loaded { cue_id } if cue_id == id)));

        // Endloser Loop + Devamp → sofortiges Finished.
        let mut cmd = play_cmd(id, Some(Duration::from_millis(100)));
        if let EngineCommand::Play { loop_mode, .. } = &mut cmd {
            *loop_mode = LoopMode::Infinite;
        }
        engine.handle(cmd).unwrap();
        engine.advance_time(Duration::from_millis(100)); // Started
        engine.handle(EngineCommand::Devamp { cue_id: id }).unwrap();
        let events: Vec<_> = rx.try_iter().collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::Finished { cue_id } if *cue_id == id)),
            "devamp must finish an infinite loop immediately: {events:?}"
        );
        assert_eq!(engine.active_playback_count(), 0);
    }

    #[test]
    fn configure_stage_validates_geometry() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (mut engine, _clock) = MockMediaEngine::with_virtual_clock(tx);

        let valid = Stage::new("Main", crate::core::StageGeometry::new(1920.0, 1080.0));
        engine
            .handle(EngineCommand::ConfigureStage { stage: valid })
            .unwrap();
        assert!(engine.has_stage("Main"));
        assert!(rx.try_iter().any(
            |e| matches!(e, EngineEvent::StageConfigured { stage_name } if stage_name == "Main")
        ));

        let invalid = Stage::new("Broken", crate::core::StageGeometry::new(0.0, 1080.0));
        engine
            .handle(EngineCommand::ConfigureStage { stage: invalid })
            .unwrap();
        assert!(
            rx.try_iter()
                .any(|e| matches!(e, EngineEvent::Error { .. })),
            "invalid geometry must produce an Error event"
        );
    }
}
