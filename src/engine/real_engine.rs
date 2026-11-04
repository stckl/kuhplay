//! **Produktive Media-Engine** mit echtem Audio- und Video-Playout.
//!
//! ## Backends
//!
//! * **Audio:** [`super::audio_backend::AudioBackend`] – symphonia-Dekodierung
//!   (mp3/aac/flac/ogg/wav/m4a/…) + cpal-Ausgabe (CoreAudio/WASAPI/ALSA).
//! * **Video:** [`super::video_backend::VideoBackend`] – ffmpeg-Subprocess
//!   (`rawvideo`/`rgba`), frame-genaues Pacing über eine virtuelle Uhr.
//! * **Diashow:** `image`-Preloading + Crossfade im Engine-Tick.
//! * **HTML5:** virtuell (wie Mock) – Events `HtmlLoaded`/`JsExecuted`/
//!   `JsonReceived` mit exakten Timestamps; eine produktive `WryEngine`
//!   mappt sie später auf `wry::WebView::evaluate_script`.
//!
//! ## Scheduling
//!
//! Spiegelt die Semantik der [`super::MockMediaEngine`] (Pre-Wait → Started →
//! Loops/Slices → Finished → Post-Wait), nur dass das Einheiten-Ende aus dem
//! Backend gelesen wird: Audio endet bei `is_finished` (EOF + FIFO leer),
//! Video bei `is_eof` (ffmpeg-Pipe zu) – jeweils begrenzt durch die
//! Cue-Duration (früheres Ende gewinnt).
//!
//! ## Compositor (Screens / Area of Interest / Hard Edge)
//!
//! Pro Stage wird ein Canvas (Größe aus [`Stage::geometry`]) komponiert –
//! siehe [`super::compositor`]. Das Ergebnis landet im `stage_frames`-Store
//! des [`super::EngineHandle`]; die GUI zeigt es als Stage-Fenster und/oder
//! auf einem Hardware-Display (Screen-Zuordnung, [`Stage::screen`]).
//! Zusätzlich kann eine Stage auf eine Blackmagic-Decklink-Karte ausgegeben
//! werden (ffmpeg-Subprocess, siehe [`super::decklink`]; konfiguriert via
//! `KUHPLAY_DECKLINK_*`).
//!
//! ## Erweiterung
//!
//! * GPU-Compositor (wgpu) statt CPU-Nearest; NDI-Out als Stage-Senke.

use std::collections::{HashMap, HashSet};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::core::{
    db_to_linear, linear_to_db_clamped, FadeCurve, FadeParam, HtmlSource, LoopMode, ShowResult,
    SlideNav, SlideShow, Stage, SurfaceGeometry, Timecode,
};

use super::audio_backend::AudioBackend;
use super::compositor::{canvas_size, Compositor, SurfaceEntry};
use super::decklink::{scale_nearest, DecklinkConfig, DecklinkSink};
use super::frame::{
    new_frame_store, new_opacity_store, new_route_frame_store, new_stage_frame_store, FrameStore,
    OpacityStore, RouteFrameStore, StageFrameStore, VideoFrame,
};
use super::route_renderer;
use super::video_backend::VideoBackend;
use super::webview_backend::{cef_binary, WebviewInstance};
use super::{EngineCommand, EngineEvent, MediaEngine};
use crate::core::{OutputRoute, RouteManager};

/// Tick-Intervall der Engine (Real-Uhr).
// 16 ms: Stage-/Routen-Re-Render mindestens mit ~60 Hz, damit Videoausgabe
// (25/30 fps) nicht ruckelt (50 ms wären nur 20 Hz und sichtbares Judder).
const TICK: Duration = Duration::from_millis(16);
/// Simulierte Laufzeit für HTML5-Cues ohne `duration` (wie Mock).
const DEFAULT_PLAYBACK: Duration = Duration::from_secs(5);

/// Art eines laufenden Playbacks.
#[derive(Debug, Clone)]
enum PlaybackKind {
    /// Audio über cpal/symphonia.
    Audio,
    /// Video über ffmpeg.
    Video,
    /// HTML5 (virtuell – WebView-Bridge-Events).
    Html(HtmlSource),
    /// Diashow (Preload + Crossfade im Tick).
    SlideShow(SlideShowState),
    /// Wait-Cue: reine Timer-Unit (kein Backend, Position = Wanduhr).
    Wait,
}

/// Diashow-Laufzeitzustand.
#[derive(Debug, Clone)]
struct SlideShowState {
    show: SlideShow,
    /// Vorab dekodierte RGBA-Frames (einmaliges Preloading).
    frames: Vec<Arc<VideoFrame>>,
    index: usize,
    started_at: Duration,
    next_advance_at: Option<Duration>,
    /// Crossfade: (vorheriger Index, Start-Zeitpunkt).
    crossfade: Option<(usize, Duration)>,
}

/// Ein laufendes Playback (spiegelt die Mock-Semantik).
struct ActivePlayback {
    kind: PlaybackKind,
    /// Medienpfad (Audio/Video – für Loop-Neustart und Slice-Seeks).
    path: Option<String>,
    /// Audio-Routing (Mehrkanal-Patch/Pan – an das Audio-Backend gereicht).
    route: Option<crate::core::AudioRoute>,
    stage: Option<String>,
    layer: u32,
    /// Surface-Geometrie auf der Stage (AoI/Hard-Edge/Opacity).
    surface: Option<SurfaceGeometry>,
    received_at: Duration,
    pre_wait: Duration,
    started_at: Option<Duration>,
    /// Akkumulierte Laufzeit vor Pausen (nur Html/Diashow; Media: Backend).
    accumulated: Duration,
    paused_since: Option<Duration>,
    /// In-Point im Medium (Seek nach Play-Start).
    in_at: Option<Duration>,
    /// Out-Point im Medium (danach Unit-Ende).
    out_at: Option<Duration>,
    /// Cue-Duration (0 = bis EOF/Stop).
    duration: Duration,
    loop_mode: LoopMode,
    /// Abgeflachter Slice-Plan (Segment-Dauern × Play Count).
    plan: Vec<Duration>,
    /// Slice-Startpositionen (parallel zu `plan`, für Backend-Seeks).
    plan_offset: Vec<Duration>,
    plan_index: usize,
    iterations_done: u32,
    finished_at: Option<Duration>,
    post_wait: Duration,
    post_wait_done: bool,
    /// Medien-Dauer aus ffprobe (Video).
    media_duration: Option<Duration>,
}

impl ActivePlayback {
    /// Vergangene Wanduhr-Zeit (Pausen eingefroren) – für Html/Diashow.
    fn wall_elapsed(&self, now: Duration) -> Duration {
        match (self.started_at, self.paused_since) {
            (Some(start), None) => self.accumulated + now.saturating_sub(start),
            _ => self.accumulated,
        }
    }

    /// Darf das Bild dieses Cues gezeigt werden? Ein vorgeladenes Video
    /// liegt schon im Frame-Store – sichtbar erst nach dem Start (Pre-Wait).
    fn shows_picture(&self) -> bool {
        !matches!(self.kind, PlaybackKind::Video) || self.started_at.is_some()
    }

    /// Dauer der aktuellen Einheit.
    fn current_unit(&self) -> Duration {
        if matches!(self.kind, PlaybackKind::Wait) {
            // Wait-Cue: explizite Dauer (0 = sofortiges Ende), nie Default.
            return self.duration;
        }
        let unit0 = if !self.plan.is_empty() {
            self.plan
                .get(self.plan_index)
                .copied()
                .unwrap_or(DEFAULT_PLAYBACK)
        } else {
            Duration::MAX
        };
        // Out-Point begrenzt die Einheit (In/Out).
        if let Some(out) = self.out_at {
            let start = self.in_at.unwrap_or(Duration::ZERO);
            return out.saturating_sub(start).min(unit0);
        }
        if !self.plan.is_empty() {
            return unit0;
        }
        if let PlaybackKind::SlideShow(state) = &self.kind {
            if !self.duration.is_zero() {
                return self.duration;
            }
            if !state.show.auto_advance || state.show.loop_slides {
                return Duration::MAX;
            }
            return state.show.total_duration();
        }
        if !self.duration.is_zero() {
            return self.duration;
        }
        match &self.kind {
            // Ohne Cue-Duration: bis EOF (Media); HTML5 läuft bis zum Stoppen
            // (wie Video/Bild als active cue), hat aber kein EOF.
            PlaybackKind::Audio | PlaybackKind::Video => {
                self.media_duration.unwrap_or(Duration::MAX)
            }
            PlaybackKind::Html(_) => Duration::MAX,
            _ => DEFAULT_PLAYBACK,
        }
    }

    /// Startet eine neue Einheit (Loop/Slice): Position zurücksetzen.
    fn restart_unit(&mut self, now: Duration) {
        self.accumulated = Duration::ZERO;
        self.started_at = Some(now);
        if let PlaybackKind::SlideShow(state) = &mut self.kind {
            state.index = 0;
            state.started_at = now;
            state.crossfade = None;
            state.next_advance_at = if state.show.auto_advance {
                Some(now + state.show.hold_at(0))
            } else {
                None
            };
        }
    }
}

/// Ein laufender Fade. Schlüssel in `fades` ist (Ziel-Cue, Parameter) –
/// ein neuer Fade auf denselben Parameter ersetzt den laufenden (docs/AUFTRAG-fade.md).
struct ActiveFade {
    /// Fade-Cue, zu dem dieser Fade gehört (meldet Finished, wenn alle fertig).
    fade_cue: Uuid,
    from: f32,
    target: f32,
    duration: Duration,
    curve: FadeCurve,
    started_at: Duration,
    last_value: f32,
}

/// Aktive WebView-Instanz eines HTML5-Cues: echtes Subprocess-Backend
/// (`webview`), wenn verfügbar, sonst virtueller Fallback (`None`).
/// Laufender Script-Cue: WebView-Instanz + Settle-Deadline. `Loaded` allein
/// beendet den Cue nicht sofort – sonst würde asynchrones JS (setTimeout, fetch)
/// abgeschnitten, wenn die Instanz per Drop gekillt wird (Review E12).
struct ScriptState {
    webview: super::webview_backend::WebviewInstance,
    /// Ab wann der Cue als fertig gilt (nach Loaded + Settle-Frist).
    finish_at: Option<Duration>,
}

/// Settle-Frist nach `loaded`, bevor ein Script-Cue endet
/// (Default 1000 ms, `KUHPLAY_SCRIPT_SETTLE_MS`).
const SCRIPT_SETTLE: Duration = Duration::from_millis(1000);

fn script_settle() -> Duration {
    std::env::var("KUHPLAY_SCRIPT_SETTLE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(SCRIPT_SETTLE)
}

/// Stehendes letztes Bild eines beendeten Cues.
struct HeldFrame {
    stage: String,
    layer: u32,
    surface: SurfaceGeometry,
    frame: Arc<VideoFrame>,
}

/// Alte Cues, die gehen, sobald der neue ein Bild zeigt.
struct DeferredStop {
    cue_id: Uuid,
    stops: Vec<Uuid>,
}

struct ActiveHtml {
    #[allow(dead_code)]
    source: HtmlSource,
    #[allow(dead_code)]
    stage: Option<String>,
    #[allow(dead_code)]
    layer: u32,
    #[allow(dead_code)]
    started_at: Duration,
    /// Echtes CEF-Subprocess (Drop killt den Subprocess).
    webview: Option<WebviewInstance>,
    /// Nächster fälliger Snapshot (Compositing). `None` bis `Loaded` – vor
    /// dem Laden würde jeder Snapshot fehlschlagen.
    next_snapshot_at: Option<Duration>,
    /// Fehlgeschlagene Snapshots in Folge (Backoff, siehe
    /// [`WEBVIEW_MAX_SNAPSHOT_FAILURES`]).
    snapshot_failures: u32,
    /// Seit wann ein Snapshot unterwegs ist. Es läuft immer nur einer: Dauert
    /// ein Bild länger als das Intervall, stauten sich sonst die Anfragen und
    /// das Bild hinkte immer weiter hinterher.
    snapshot_pending_since: Option<Duration>,
    /// Seite geladen (`Loaded`): Daten und Trigger gehen ab jetzt direkt raus.
    loaded: bool,
    /// Data-Cues vor dem Laden – werden nach `Loaded` zugestellt, dann `in`.
    pending_data: Vec<serde_json::Value>,
    /// Stop mit Out-Animation läuft seit … (Trigger `out` geschickt). Endet bei
    /// `TriggerDone("out")` oder nach [`HTML_OUT_TIMEOUT`].
    closing_since: Option<Duration>,
}

/// So lange ohne neuen Timecode gilt er als angehalten (Chasing pausiert).
/// Bei 25 fps kommen sonst alle 40 ms neue Werte.
const CHASE_TIMECODE_TIMEOUT: Duration = Duration::from_millis(300);

/// Höchstens so lange wartet ein Stop auf die Out-Animation des Templates
/// (docs/KONZEPT-html-templates.md §5); ein zweites Stop beendet sofort.
const HTML_OUT_TIMEOUT: Duration = Duration::from_secs(3);

/// Snapshot-Intervall für WebView-Compositing (HTML5-Cues auf Stages).
// ~60 fps für HTML5-Cues: CSS/JS-Animationen sollen flüssig laufen
// (Offscreen-Rendering wie in OBS Studio), nicht ruckeln.
const WEBVIEW_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(16);

/// Nach dieser Anzahl fehlgeschlagener Snapshots in Folge stellt die Engine
/// die Anfragen ein (z. B. Seite ohne Canvas auf Plattformen ohne nativen
/// Snapshot) – Fehler werden nicht endlos wiederholt.
/// Backoff nach Snapshot-Fehlern (Retry statt Dauer-Stopp).
const WEBVIEW_SNAPSHOT_RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// Bleibt die Antwort so lange aus, gilt der Snapshot als verloren.
const WEBVIEW_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);

/// So lange wartet ein Kill höchstens auf das erste Bild des neuen Cues.
const STOP_WHEN_VISIBLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Default-Snapshot-Größe, wenn der Cue keiner Stage zugeordnet ist.
const WEBVIEW_SNAPSHOT_DEFAULT: (u32, u32) = (1280, 720);

/// Die produktive Media-Engine (cpal/symphonia + ffmpeg + Software-Compositor).
pub struct RealMediaEngine {
    audio: Option<AudioBackend>,
    video: VideoBackend,
    frames: FrameStore,
    opacities: OpacityStore,
    stage_frames: StageFrameStore,
    events: Sender<EngineEvent>,
    started_at: Instant,
    active: HashMap<Uuid, ActivePlayback>,
    /// Cues, deren letztes Bild am Ende stehen bleiben soll.
    hold_on_end: HashSet<Uuid>,
    /// Stehende letzte Bilder.
    held: HashMap<Uuid, HeldFrame>,
    /// Warten auf das erste Bild eines neuen Cues.
    deferred_stops: Vec<DeferredStop>,
    fades: HashMap<(Uuid, FadeParam), ActiveFade>,
    /// Noch laufende Fades pro Fade-Cue (Finished erst bei 0).
    fade_pending: HashMap<Uuid, u32>,
    /// Data-Cues für HTML-Cues, die gestartet, aber noch nicht angelegt sind
    /// (HTML-Instanz entsteht erst im nächsten Tick) – Auto-Continue direkt
    /// nach dem HTML-Cue darf seine Daten nicht verlieren.
    pending_html_data: HashMap<Uuid, Vec<serde_json::Value>>,
    /// Stop-when-done: Ziel-Cues pro Fade-Cue (Gruppen-Ziel: mehrere Kinder).
    fade_stop_target: HashMap<Uuid, Vec<Uuid>>,
    /// Aktueller Lautstärke-Wert pro Cue (für from=None und Reset nach Neustart).
    current_volumes: HashMap<Uuid, f32>,
    chasing: HashMap<Uuid, Timecode>,
    /// Engine-Zeit des letzten empfangenen Timecodes (Chasing).
    last_timecode_at: Option<Duration>,
    /// Vom Chasing pausierte Cues (Timecode steht) – laufen mit ihm weiter.
    chase_paused: HashSet<Uuid>,
    stages: HashMap<String, Stage>,
    /// Testbild pro Stage-Name („Test Pattern"; überdeckt die Inhalte).
    /// Output Routes und ihre gerenderten Bilder.
    routes: RouteManager,
    route_frames: RouteFrameStore,
    /// Routen, die neu gerendert werden muessen (Config oder Stage-Aenderung).
    route_dirty: HashSet<Uuid>,
    html: HashMap<Uuid, ActiveHtml>,
    compositor: Compositor,
    /// Fade-Zwischenwerte (im Tick nach der Iteration anwenden).
    pending_fade_values: Vec<(Uuid, Uuid, FadeParam, f32)>,
    /// Audio-Device-Fehler nur einmal melden (kein Log-Spam pro Cue).
    audio_broken_reported: bool,
    /// Zusätzliche Audio-Streams (weitere Ausgabegeräte parallel); teilen den
    /// Mixer des primären Backends. Pro Gerät höchstens einer (device_name).
    extra_audio: Vec<AudioBackend>,
    /// DMX-Stand pro Universe (Art-Net und sACN getrennt über Protokoll im
    /// Schlüssel): Light-Cues mit start_channel > 1 mischen sich hinein, statt
    /// Kanäle davor auf 0 zu setzen.
    dmx_state: HashMap<(crate::core::LightProtocol, u16), [u8; crate::core::light::DMX_CHANNELS]>,
    /// Laufende sACN-Sequenznummer pro Universe.
    sacn_seq: HashMap<u16, u8>,
    /// Feste sACN-CID dieser KuhPlay-Instanz (pro Start generiert).
    sacn_cid: [u8; 16],
    /// Zuletzt angewendeter Audio-Patch (überlebt Gerätewechsel des Primären).
    audio_patch: crate::core::AudioPatch,
    /// Decklink-Hardware-Ausgabe (optional, siehe [`Self::start_decklink_from_env`]).
    decklink: Option<DecklinkSink>,
    /// Geteilte Statistik für das EngineHandle (GUI liest mit, G4).
    decklink_stats_handle: std::sync::Arc<std::sync::Mutex<super::decklink::DecklinkStats>>,
    /// Zweiter Decklink-Ausgang für den Key-SDI (Key/Fill, 2-Kabel):
    /// `KUHPLAY_DECKLINK_KEY_DEVICE`. Push: Alpha der Key-Route als Luma.
    decklink_key: Option<DecklinkSink>,
    /// Stage, deren Compositor-Frames auf die Decklink-Karte gehen.
    decklink_stage: String,
    /// Decklink-Framerate (Pacing der Frame-Pushes).
    decklink_fps: u32,
    /// Nächster Push-Zeitpunkt (Engine-Zeit).
    decklink_next_push: Duration,
    /// CEF-Subprocess-Binary für HTML5-Cues (`None` = virtueller Fallback).
    renderer_bin: Option<PathBuf>,
    /// Cue-IDs mit aktiver Audio-Voice (Spiegel für [`super::EngineHandle::audio_voices`]).
    audio_voices: Arc<Mutex<HashSet<Uuid>>>,
    /// MIDI-Ausgang für MIDI-Cues (lazy geöffnet beim ersten `SendMidi`,
    /// Device via `KUHPLAY_MIDI_OUT_DEVICE`; `None` = noch nicht versucht).
    midi_out: Option<super::midi_out::MidiOutputGuard>,
    /// Laufende Script-Cues: WebView-Instanz (führt das JS aus) + Post-Wait.
    /// Nach `Loaded`/Fehler → `Finished` und Entfernen (Tick pollt Events).
    scripts: HashMap<Uuid, ScriptState>,
}

impl RealMediaEngine {
    /// Erzeugt die Engine. Die Stores sind die geteilten des
    /// [`super::EngineHandle`]. Ohne Audio-Device läuft die Engine trotzdem
    /// (Video-only) – Audio-Cues melden `Error`.
    pub fn new(
        frames: FrameStore,
        opacities: OpacityStore,
        stage_frames: StageFrameStore,
        route_frames: RouteFrameStore,
        events: Sender<EngineEvent>,
    ) -> Self {
        let audio_off = std::env::var("KUHPLAY_AUDIO")
            .map(|v| v == "off")
            .unwrap_or(false);
        let audio = if audio_off {
            None
        } else {
            match AudioBackend::new() {
                Ok(backend) => {
                    debug!("audio backend ready (cpal)");
                    Some(backend)
                }
                Err(err) => {
                    warn!(%err, "audio backend unavailable – video-only mode");
                    None
                }
            }
        };
        let video = VideoBackend::new(frames.clone());
        Self {
            audio,
            video,
            frames,
            opacities,
            stage_frames,
            events,
            started_at: Instant::now(),
            dmx_state: HashMap::new(),
            sacn_seq: HashMap::new(),
            sacn_cid: *Uuid::new_v4().as_bytes(),
            active: HashMap::new(),
            hold_on_end: HashSet::new(),
            held: HashMap::new(),
            deferred_stops: Vec::new(),
            fades: HashMap::new(),
            fade_pending: HashMap::new(),
            pending_html_data: HashMap::new(),
            fade_stop_target: HashMap::new(),
            current_volumes: HashMap::new(),
            chasing: HashMap::new(),
            last_timecode_at: None,
            chase_paused: HashSet::new(),
            stages: HashMap::new(),
            routes: RouteManager::new(),
            route_frames,
            route_dirty: HashSet::new(),
            html: HashMap::new(),
            compositor: Compositor::new(),
            pending_fade_values: Vec::new(),
            audio_broken_reported: false,
            extra_audio: Vec::new(),
            audio_patch: crate::core::AudioPatch::default(),
            decklink: None,
            decklink_stats_handle: std::sync::Arc::new(std::sync::Mutex::new(
                super::decklink::DecklinkStats::default(),
            )),
            decklink_key: None,
            decklink_stage: String::new(),
            decklink_fps: 25,
            decklink_next_push: Duration::ZERO,
            renderer_bin: cef_binary(),
            audio_voices: Arc::new(Mutex::new(HashSet::new())),
            midi_out: None,
            scripts: HashMap::new(),
        }
    }

    /// Setzt den Voice-Store (produktiver Pfad: `spawn_real_engine_with_join`
    /// reicht ihn an die [`super::EngineHandle`] durch).
    pub fn with_audio_voice_store(mut self, store: Arc<Mutex<HashSet<Uuid>>>) -> Self {
        self.audio_voices = store;
        self
    }

    /// Setzt das CEF-Subprocess-Binary für HTML5-Cues (Builder; für
    /// Tests – produktiv wird es in `new()` aufgelöst).
    pub fn with_renderer_bin(mut self, bin: PathBuf) -> Self {
        self.renderer_bin = Some(bin);
        self
    }

    /// Startet die Decklink-Ausgabe aus den Umgebungsvariablen
    /// (`KUHPLAY_DECKLINK_*`, siehe [`DecklinkConfig::from_env`]) und hängt
    /// sie an die Engine. Produktiver Einstiegspunkt (aus
    /// [`spawn_real_engine_with_join`]); `new()` lässt Decklink bewusst aus,
    /// damit Tests die Engine ohne Env-Side-Effects bauen können.
    pub fn start_decklink_from_env(&mut self) {
        let Some((config, stage)) = DecklinkConfig::from_env() else {
            return;
        };
        // Key-Ausgang (Key/Fill): eigener Decklink-Port, Alpha → Luma.
        if let Ok(key_device) = std::env::var("KUHPLAY_DECKLINK_KEY_DEVICE") {
            if !key_device.trim().is_empty() {
                let key_config = DecklinkConfig {
                    device: key_device.clone(),
                    ..config.clone()
                };
                match DecklinkSink::start(&key_config) {
                    Ok(mut out) => {
                        info!(device = %key_device, "decklink key output gestartet");
                        if let Err(err) = out.start_scheduled() {
                            warn!(%err, "decklink key: scheduled nicht gestartet – Sync-Modus");
                        }
                        self.decklink_key = Some(out);
                    }
                    Err(err) => warn!(%err, "decklink key output fehlgeschlagen"),
                }
            }
        }
        match DecklinkSink::start(&config) {
            Ok(mut output) => {
                info!(
                    device = %config.device,
                    stage = %stage,
                    "{}x{}@{} → decklink",
                    config.width,
                    config.height,
                    config.fps
                );
                // G4: Scheduled Playback – die Karte gibt den Takt vor.
                if let Err(err) = output.start_scheduled() {
                    warn!(%err, "decklink scheduled playback nicht gestartet – Sync-Modus");
                }
                self.decklink = Some(output);
                self.decklink_stage = stage;
                self.decklink_fps = config.fps.max(1);
            }
            Err(err) => {
                warn!(%err, "decklink output konnte nicht gestartet werden – deaktiviert");
            }
        }
    }

    /// Setzt den gemeinsamen Statistik-Store (G4) – aus spawn_real_engine_with_join.
    pub fn with_decklink_stats(
        mut self,
        stats: std::sync::Arc<std::sync::Mutex<super::decklink::DecklinkStats>>,
    ) -> Self {
        self.decklink_stats_handle = stats;
        self
    }

    /// Hängt eine Decklink-Senke programmatisch an (für Tests: Capture-Senke).
    pub fn with_decklink(mut self, output: DecklinkSink, stage: String, fps: u32) -> Self {
        self.decklink = Some(output);
        self.decklink_stage = stage;
        self.decklink_fps = fps.max(1);
        self
    }

    /// DeckLink-Statistik (late, dropped, queued) – G4; (0,0,0) ohne native Senke.
    pub fn decklink_stats(&self) -> (u64, u64, u64) {
        self.decklink
            .as_ref()
            .map(|d| d.stats())
            .unwrap_or((0, 0, 0))
    }

    /// Pflegt die geteilte Statistik im EngineHandle (GUI/Remote, G4).
    fn publish_decklink_stats(&self) {
        let (late, dropped, queued) = self.decklink_stats();
        let frames = self.decklink_frames_pushed();
        *self.decklink_stats_handle.lock().unwrap() = super::decklink::DecklinkStats {
            frames,
            late,
            dropped,
            queued,
        };
    }

    /// Anzahl der auf die Decklink-Ausgabe gepushten Frames (0 ohne Decklink).
    pub fn decklink_frames_pushed(&self) -> u64 {
        self.decklink
            .as_ref()
            .map(|d| d.frames_pushed())
            .unwrap_or(0)
    }

    /// Pusht das aktuelle Compositor-Frame der Decklink-Stage im
    /// konfigurierten Takt auf die Hardware. SDI braucht einen
    /// kontinuierlichen Stream – auch bei statischem Inhalt; bis zum ersten
    /// Composite der Stage wird ein schwarzes Frame geschickt. Ein
    /// fehlgeschlagener Push (z. B. Größen-Mismatch oder ffmpeg gestorben)
    /// deaktiviert die Ausgabe (kein Spam, kein Panic).
    fn tick_decklink(&mut self, now: Duration) {
        let Some(sink) = &self.decklink else {
            return;
        };
        // Die Karte ist der Taktgeber: Liefern, sobald ihr Vorlauf sinkt. So kann
        // die Engine-Uhr nicht von der Karte wegdriften.
        match sink.needs_frame() {
            Some(false) => return,
            Some(true) => {}
            None => {
                if now < self.decklink_next_push {
                    return;
                }
                // Review G2: Bruch-Bildraten (29,97/59,94) korrekt takten. Fester
                // Abstand, damit der grobe Engine-Takt die Bildrate nicht senkt.
                let interval = super::decklink::fps_frame_interval(self.decklink_fps);
                self.decklink_next_push = if now > self.decklink_next_push + interval * 2 {
                    now + interval
                } else {
                    self.decklink_next_push + interval
                };
            }
        }

        let (width, height) = match &self.decklink {
            Some(out) => (out.width(), out.height()),
            None => return,
        };
        // Route-Bild statt Stage-Bild: Die konfigurierte Stage (KUHPLAY_DECKLINK_STAGE)
        // liefert ueber ihre erste Region die Route, deren gerendertes Bild auf
        // die Karte geht. Ohne Route: schwarz (wie vorher ohne Canvas).
        // Key/Fill: Die Bild-Region geht auf den Haupt-Ausgang, die Alpha der
        // Key-Region als Luma auf den Key-Ausgang.
        let (fill_route, key_route) = self
            .stages
            .get(&self.decklink_stage)
            .map(|stage| stage.fill_and_key_routes())
            .unwrap_or((None, None));
        let route_frames = self.route_frames.clone();
        let frame_of = |route_id: Option<uuid::Uuid>| -> Option<Arc<VideoFrame>> {
            route_id.and_then(|id| route_frames.lock().unwrap().get(&id).cloned())
        };
        let fill_frame = frame_of(fill_route);
        let key_frame = frame_of(key_route);
        let black = Arc::new(VideoFrame::black(width, height));
        let frame = fill_frame.as_ref().unwrap_or(&black);
        // Route-Bild ≠ Karten-Auflösung → Nearest-Neighbor-Scaling.
        let scaled;
        let frame = if frame.width == width && frame.height == height {
            frame
        } else {
            scaled = Arc::new(scale_nearest(frame, width, height));
            &scaled
        };
        self.publish_decklink_stats();
        use super::output_sink::OutputSink as _;
        let result = self.decklink.as_mut().unwrap().push(frame);
        if let Err(err) = result {
            warn!(%err, "decklink push fehlgeschlagen – Ausgabe deaktiviert");
            self.decklink = None;
        }
        // Key-SDI (Key/Fill): Alpha der Key-Route als Luma auf den Key-Port.
        if let Some(key_out) = self.decklink_key.as_mut() {
            if let Some(kf) = key_frame {
                let key_luma = if kf.width == width && kf.height == height {
                    super::decklink::alpha_to_luma(&kf)
                } else {
                    super::decklink::alpha_to_luma(&scale_nearest(&kf, width, height))
                };
                if let Err(err) = key_out.push(&Arc::new(key_luma)) {
                    warn!(%err, "decklink key push fehlgeschlagen – Key-Ausgabe deaktiviert");
                    self.decklink_key = None;
                }
            }
        }
    }

    fn now(&self) -> Duration {
        self.started_at.elapsed()
    }

    fn emit(&self, event: EngineEvent) {
        // UI könnte geschlossen sein → Fehler ignorieren (kein Panic).
        let _ = self.events.send(event);
    }

    /// Medien-Position eines Playbacks (Backend für Media, Wanduhr sonst).
    ///
    /// Assoziierte Funktion mit Feld-Referenzen, damit sie innerhalb des
    /// Tick-Loops aufgerufen werden kann, während `active` mutably geborgt ist
    /// (disjunkte Felder → keine Borrow-Konflikte).
    fn position_of(
        audio: &Option<AudioBackend>,
        video: &VideoBackend,
        playback: &ActivePlayback,
        cue_id: Uuid,
        now: Duration,
    ) -> Duration {
        match &playback.kind {
            PlaybackKind::Audio => audio
                .as_ref()
                .and_then(|a| a.position(cue_id))
                .unwrap_or_default(),
            PlaybackKind::Video => video.position(cue_id).unwrap_or_default(),
            _ => playback.wall_elapsed(now),
        }
    }

    /// Backend-EOF erreicht? (Einheiten-Ende ohne Cue-Duration).
    fn backend_eof(
        audio: &Option<AudioBackend>,
        video: &VideoBackend,
        playback: &ActivePlayback,
        cue_id: Uuid,
    ) -> bool {
        match &playback.kind {
            PlaybackKind::Audio => audio
                .as_ref()
                .map(|a| a.is_finished(cue_id))
                .unwrap_or(false),
            PlaybackKind::Video => video.is_eof(cue_id),
            _ => false,
        }
    }

    /// Startet die Backend-Wiedergabe eines Media-Cues (Audio/Video).
    #[allow(clippy::too_many_arguments)] // Backend-Setup: alle Parameter fachlich nötig
    fn start_media_backend(
        audio: &mut Option<AudioBackend>,
        video: &mut VideoBackend,
        audio_broken_reported: &mut bool,
        cue_id: Uuid,
        kind: &PlaybackKind,
        path: &Option<String>,
        seek: Duration,
        route: Option<crate::core::AudioRoute>,
    ) -> Result<Option<Duration>, String> {
        let path = match path {
            Some(path) if !path.trim().is_empty() => path,
            _ => return Err("no media path".to_string()),
        };
        if !Path::new(path).exists() {
            return Err(format!("media file not found: {path}"));
        }
        match kind {
            PlaybackKind::Audio => {
                let audio = audio.as_mut().ok_or_else(|| {
                    if !*audio_broken_reported {
                        *audio_broken_reported = true;
                        warn!("audio device unavailable");
                    }
                    "no audio output device available".to_string()
                })?;
                audio
                    .add_voice(cue_id, path, seek, route)
                    .map_err(|e| format!("audio: {e}"))?;
                Ok(None)
            }
            PlaybackKind::Video => {
                let info = video
                    .play(cue_id, path, seek)
                    .map_err(|e| format!("video: {e}"))?;
                // Tonspur des Videos mitlaufen lassen: symphonia dekodiert den
                // Audio-Track aus demselben Container, gepaces gegen den cpal-
                // Callback (Echtzeit) — Start im selben Tick wie das Video, also
                // synchron. Videos ohne Tonspur: Fehler wird ignoriert (nur Log).
                if let Some(audio) = audio.as_mut() {
                    if let Err(err) = audio.add_voice(cue_id, path, seek, route) {
                        debug!(%err, "video cue has no decodable audio track");
                    }
                }
                Ok(info.duration)
            }
            _ => Ok(None),
        }
    }

    /// Startet eine Einheit neu (Loop/Slice) im Backend.
    fn restart_backend(
        audio: &mut Option<AudioBackend>,
        video: &mut VideoBackend,
        cue_id: Uuid,
        playback: &ActivePlayback,
    ) {
        let offset = playback
            .plan_offset
            .get(playback.plan_index)
            .copied()
            .unwrap_or_default();
        match &playback.kind {
            PlaybackKind::Audio => {
                if let Some(audio) = audio.as_mut() {
                    audio.seek(cue_id, offset);
                }
            }
            PlaybackKind::Video => {
                if let Some(path) = &playback.path {
                    let _ = video.play(cue_id, path, offset);
                }
                if let Some(audio) = audio.as_mut() {
                    audio.seek(cue_id, offset);
                }
            }
            _ => {}
        }
    }

    /// Wendet den Audio-Patch an: primäres Gerät (System-Standard bzw. vom
    /// Nutzer gewählt) plus alle weiteren im Patch referenzierten Geräte
    /// parallel – jeder Stream mit dem gemeinsamen Mixer, damit dieselben
    /// Voices überall laufen. Nicht mehr referenzierte Extra-Streams werden
    /// geschlossen, fehlende Geräte melden `Error` (ohne die Show zu stoppen).
    fn apply_audio_patch(&mut self) {
        let patch = self.audio_patch.clone();
        // Primäres Backend sicherstellen.
        if self.audio.is_none() {
            match AudioBackend::new() {
                Ok(backend) => self.audio = Some(backend),
                Err(err) => {
                    if !self.audio_broken_reported {
                        self.audio_broken_reported = true;
                        warn!(%err, "audio device unavailable");
                    }
                    self.emit(EngineEvent::Error {
                        cue_id: None,
                        message: format!("no audio output device available: {err}"),
                    });
                    return;
                }
            }
        }
        if let Some(primary) = &self.audio {
            primary.set_channel_outputs(patch.device_output_mask(None));
        }
        // Weitere Geräte parallel verwalten.
        let wanted: Vec<String> = patch.devices().into_iter().flatten().collect();
        self.extra_audio
            .retain(|b| b.device_name().is_some_and(|n| wanted.contains(&n)));
        let Some(primary) = &self.audio else {
            return;
        };
        let mixer = primary.mixer();
        for name in &wanted {
            let existing = self
                .extra_audio
                .iter()
                .find(|b| b.device_name().as_deref() == Some(name.as_str()));
            if let Some(backend) = existing {
                backend.set_channel_outputs(patch.device_output_mask(Some(name)));
                continue;
            }
            match AudioBackend::with_shared_mixer(Some(name), mixer.clone()) {
                Ok(backend) => {
                    backend.set_channel_outputs(patch.device_output_mask(Some(name)));
                    info!(device = %name, "zusätzliches Audio-Gerät parallel gestartet");
                    self.extra_audio.push(backend);
                }
                Err(err) => {
                    warn!(device = %name, %err, "zusätzliches Audio-Gerät nicht verfügbar");
                    self.emit(EngineEvent::Error {
                        cue_id: None,
                        message: format!("audio output \"{name}\" not available: {err}"),
                    });
                }
            }
        }
    }

    /// Video vorstarten (ffmpeg pausiert) – beim GO ist das erste Bild sofort
    /// da (kein Schwarz zwischen Video-Cues). Andere Dateien: nichts zu tun.
    /// Dabei verdrängte Standby-Vorladungen melden `Unloaded`.
    fn preload_video(&mut self, cue_id: Uuid, path: &str, pin: bool) {
        if super::video_backend::probe(path).is_err() {
            return;
        }
        let before = self.video.preloaded_ids();
        if let Err(err) = self.video.preload(cue_id, path, Duration::ZERO, pin) {
            debug!(%cue_id, %err, "video preload fehlgeschlagen");
        }
        let after = self.video.preloaded_ids();
        for id in before.into_iter().filter(|id| !after.contains(id)) {
            self.emit(EngineEvent::Unloaded { cue_id: id });
        }
        if !pin && self.video.is_preloaded(cue_id) {
            self.emit(EngineEvent::Loaded { cue_id });
        }
    }

    /// Stoppt das Backend eines Cues (Stop/Devamp/Ende).
    fn stop_backend(
        audio: &mut Option<AudioBackend>,
        video: &mut VideoBackend,
        cue_id: Uuid,
        playback: &ActivePlayback,
    ) {
        match &playback.kind {
            PlaybackKind::Audio => {
                if let Some(audio) = audio.as_mut() {
                    audio.stop_voice(cue_id);
                }
            }
            PlaybackKind::Video => {
                video.stop(cue_id);
                if let Some(audio) = audio.as_mut() {
                    audio.stop_voice(cue_id);
                }
            }
            _ => {}
        }
    }

    /// Aktueller Wert eines Parameters am Ziel (für from=None: kein Sprung).
    fn current_value(&self, target: Uuid, param: FadeParam) -> f32 {
        match param {
            FadeParam::Volume => self.current_volumes.get(&target).copied().unwrap_or(1.0),
            FadeParam::Opacity => self
                .opacities
                .lock()
                .unwrap()
                .get(&target)
                .copied()
                .unwrap_or(1.0),
            FadeParam::Rate => 1.0,
        }
    }

    /// Trägt einen Fade ein. Ersetzt er einen laufenden Fade eines anderen
    /// Fade-Cues auf dasselbe Ziel/denselben Parameter, gilt dieser als fertig
    /// (AUFTRAG §3) – sonst bliebe der alte Fade-Cue ewig „läuft".
    fn insert_fade(&mut self, key: (Uuid, FadeParam), fade: ActiveFade) {
        let new_cue = fade.fade_cue;
        if let Some(old) = self.fades.insert(key, fade) {
            if old.fade_cue != new_cue {
                self.settle_fade(old.fade_cue, false);
            }
        }
    }

    /// Ein Fade von `fade_cue` ist beendet. Sind alle seine Fades durch,
    /// meldet der Fade-Cue genau einmal: `completed` (regulär zu Ende) →
    /// Stop when done stoppt die Ziele, dann `Finished`; sonst (ersetzt oder
    /// Ziel gestoppt) `Stopped` – ohne Auto-Continue.
    fn settle_fade(&mut self, fade_cue: Uuid, completed: bool) {
        let Some(pending) = self.fade_pending.get_mut(&fade_cue) else {
            return;
        };
        *pending = pending.saturating_sub(1);
        if *pending > 0 {
            return;
        }
        self.fade_pending.remove(&fade_cue);
        let targets = self.fade_stop_target.remove(&fade_cue).unwrap_or_default();
        if completed {
            for target in targets {
                self.stop_cue_simple(target);
            }
            self.emit(EngineEvent::Finished { cue_id: fade_cue });
        } else {
            self.emit(EngineEvent::Stopped { cue_id: fade_cue });
        }
    }

    /// Entfernt alle Fades, die auf `target` wirken, und rechnet sie bei
    /// ihren Fade-Cues ab (Ziel gestoppt → Fade endet ohne Stop-when-done).
    fn drop_fades_on(&mut self, target: Uuid) {
        let keys: Vec<(Uuid, FadeParam)> = self
            .fades
            .keys()
            .filter(|(t, _)| *t == target)
            .copied()
            .collect();
        for key in keys {
            if let Some(fade) = self.fades.remove(&key) {
                self.settle_fade(fade.fade_cue, false);
            }
        }
    }

    /// Bricht einen Fade-Cue ab (Stop auf den Fade-Cue selbst): seine Fades
    /// bleiben beim aktuellen Wert stehen, kein Finished.
    fn cancel_fade_cue(&mut self, fade_cue: Uuid) -> bool {
        let had = self.fade_pending.remove(&fade_cue).is_some();
        self.fade_stop_target.remove(&fade_cue);
        self.fades.retain(|_, f| f.fade_cue != fade_cue);
        had
    }

    /// Stoppt einen Cue intern (Stop-when-done): Backends zu,
    /// Status-Maps raus, Stopped-Event.
    fn stop_cue_simple(&mut self, cue_id: Uuid) {
        self.drop_fades_on(cue_id);
        if let Some(playback) = self.active.remove(&cue_id) {
            Self::stop_backend(&mut self.audio, &mut self.video, cue_id, &playback);
            self.audio_voices.lock().unwrap().remove(&cue_id);
            self.chasing.remove(&cue_id);
            self.html.remove(&cue_id);
            self.frames.lock().unwrap().remove(&cue_id);
            self.opacities.lock().unwrap().remove(&cue_id);
            self.current_volumes.remove(&cue_id);
            if let Some(stage) = &playback.stage {
                let stage = stage.clone();
                self.compositor.mark_dirty(&stage);
            }
            self.emit(EngineEvent::Stopped { cue_id });
        }
    }

    /// Wendet einen Fade-Zwischenwert auf die Backends/Stores an.
    fn apply_fade_value(&mut self, cue_id: Uuid, param: FadeParam, value: f32) {
        match param {
            FadeParam::Volume => {
                // Aktuellen Wert merken: der nächste Fade (from=None) setzt
                // hier an, ohne Sprung.
                self.current_volumes.insert(cue_id, value);
                if let Some(audio) = self.audio.as_mut() {
                    audio.set_volume(cue_id, value);
                }
            }
            FadeParam::Opacity => {
                self.opacities.lock().unwrap().insert(cue_id, value);
                if let Some(playback) = self.active.get(&cue_id) {
                    if let Some(stage) = &playback.stage {
                        let stage = stage.clone();
                        self.compositor.mark_dirty(&stage);
                    }
                }
            }
            // Rate wird nicht mehr unterstützt (aus UI/Command entfernt).
            FadeParam::Rate => {}
        }
    }

    /// Sammelt die Surfaces einer Stage (laufende Video-/Diashow-Cues).
    fn surfaces_of_stage(&self, stage_name: &str) -> Vec<SurfaceEntry> {
        let mut surfaces = Vec::new();
        for (cue_id, playback) in &self.active {
            if playback.finished_at.is_some() {
                continue;
            }
            if playback.stage.as_deref() != Some(stage_name) {
                continue;
            }
            // HTML5-Cues compositen über WebView-Snapshots (Frame-Store).
            if !matches!(
                playback.kind,
                PlaybackKind::Video | PlaybackKind::SlideShow(_) | PlaybackKind::Html(_)
            ) {
                continue;
            }
            let surface = playback.surface.unwrap_or_else(SurfaceGeometry::fit);
            let frame = playback
                .shows_picture()
                .then(|| self.frames.lock().unwrap().get(cue_id).cloned())
                .flatten();
            let fade_opacity = self
                .opacities
                .lock()
                .unwrap()
                .get(cue_id)
                .copied()
                .unwrap_or(1.0);
            surfaces.push((*cue_id, playback.layer, surface, frame, fade_opacity));
        }
        // Gehaltene Bilder: Startzeit 0, ein laufender Cue liegt also darüber.
        for (cue_id, held) in &self.held {
            if held.stage == stage_name {
                let fade_opacity = self
                    .opacities
                    .lock()
                    .unwrap()
                    .get(cue_id)
                    .copied()
                    .unwrap_or(1.0);
                surfaces.push((
                    *cue_id,
                    held.layer,
                    held.surface,
                    Some(held.frame.clone()),
                    fade_opacity,
                ));
            }
        }
        // Layer-Regel: höheres Layer liegt oben; gleiches Layer: später
        // gestarteter Cue liegt oben (neuester Content sichtbar).
        surfaces.sort_by_key(|(cue_id, layer, _, _, _)| {
            let started = self
                .active
                .get(cue_id)
                .map(|p| p.started_at.unwrap_or(p.received_at))
                .unwrap_or_default();
            (*layer, started)
        });
        surfaces
    }

    /// Hält das letzte Bild eines gerade beendeten Cues, wenn der das will.
    fn hold_frame_if_wanted(&mut self, cue_id: Uuid) {
        if !self.hold_on_end.remove(&cue_id) {
            return;
        }
        let Some(playback) = self.active.get(&cue_id) else {
            return;
        };
        let Some(stage) = playback.stage.clone() else {
            return;
        };
        if !matches!(
            playback.kind,
            PlaybackKind::Video | PlaybackKind::SlideShow(_)
        ) {
            return;
        }
        let Some(frame) = self.frames.lock().unwrap().get(&cue_id).cloned() else {
            return;
        };
        let layer = playback.layer;
        let surface = playback.surface.unwrap_or_else(SurfaceGeometry::fit);
        // Ein neues Standbild ersetzt das alte auf demselben Layer.
        let replaced: Vec<Uuid> = self
            .held
            .iter()
            .filter(|(_, h)| h.stage == stage && h.layer == layer)
            .map(|(id, _)| *id)
            .collect();
        for id in replaced {
            self.release_held(id);
        }
        self.compositor.mark_dirty(&stage);
        self.held.insert(
            cue_id,
            HeldFrame {
                stage,
                layer,
                surface,
                frame,
            },
        );
        self.emit(EngineEvent::FrameHeld { cue_id });
    }

    /// Nimmt ein gehaltenes Bild weg. `true`, wenn es eines gab.
    fn release_held(&mut self, cue_id: Uuid) -> bool {
        let Some(held) = self.held.remove(&cue_id) else {
            return false;
        };
        self.compositor.mark_dirty(&held.stage);
        self.opacities.lock().unwrap().remove(&cue_id);
        self.emit(EngineEvent::HoldEnded { cue_id });
        true
    }

    /// Ein laufendes Bild auf demselben Layer löst das Standbild ab.
    fn release_covered_holds(&mut self) {
        let frames = self.frames.lock().unwrap();
        let covered: Vec<Uuid> = self
            .held
            .iter()
            .filter(|(id, held)| {
                self.active.iter().any(|(other, playback)| {
                    other != *id
                        && playback.finished_at.is_none()
                        && playback.layer == held.layer
                        && playback.stage.as_deref() == Some(held.stage.as_str())
                        && playback.shows_picture()
                        && frames.contains_key(other)
                })
            })
            .map(|(id, _)| *id)
            .collect();
        drop(frames);
        for id in covered {
            self.release_held(id);
        }
    }

    /// Stoppt die alten Cues, sobald der neue ein Bild zeigt.
    fn tick_deferred_stops(&mut self, now: Duration) {
        for deferred in std::mem::take(&mut self.deferred_stops) {
            let has_picture = self.frames.lock().unwrap().contains_key(&deferred.cue_id)
                && self
                    .active
                    .get(&deferred.cue_id)
                    .is_some_and(ActivePlayback::shows_picture);
            let playback = self.active.get(&deferred.cue_id);
            let timed_out = playback
                .and_then(|p| p.started_at)
                .is_some_and(|started| now.saturating_sub(started) >= STOP_WHEN_VISIBLE_TIMEOUT);
            if has_picture || timed_out {
                for cue_id in deferred.stops {
                    let _ = self.handle(EngineCommand::Stop { cue_id });
                }
            } else if playback.is_some() {
                self.deferred_stops.push(deferred);
            }
            // Sonst ist der neue Cue ausgefallen: Die alten laufen weiter.
        }
    }

    /// Kompiliert alle Stages neu, deren Surfaces sich geändert haben.
    fn composite_dirty_stages(&mut self) {
        // Dirty-Prüfung: Frame-Pointer oder Opacity geändert → Stage neu.
        let frames = self.frames.lock().unwrap();
        let opacities = self.opacities.lock().unwrap();
        for (cue_id, playback) in &self.active {
            let Some(stage) = &playback.stage else {
                continue;
            };
            // HTML5-Cues compositen über WebView-Snapshots (Frame-Store).
            if !matches!(
                playback.kind,
                PlaybackKind::Video | PlaybackKind::SlideShow(_) | PlaybackKind::Html(_)
            ) {
                continue;
            }
            let frame = frames.get(cue_id).filter(|_| playback.shows_picture());
            let opacity = opacities.get(cue_id).copied().unwrap_or(1.0);
            if self.compositor.is_dirty(stage, *cue_id, frame, opacity) {
                let stage = stage.clone();
                self.compositor.mark_dirty(&stage);
            }
        }
        drop(frames);
        drop(opacities);

        // Konfigurierte Stages ohne Canvas: einmalig leeren Canvas anlegen.
        let stage_names: Vec<String> = self.stages.keys().cloned().collect();
        let have_canvas: HashSet<String> =
            self.stage_frames.lock().unwrap().keys().cloned().collect();
        for name in stage_names {
            if !have_canvas.contains(&name) {
                self.compositor.mark_dirty(&name);
            }
        }

        let dirty = self.compositor.take_dirty();
        if dirty.is_empty() {
            return;
        }
        // Phase 1: Jobs sammeln (Canvas + Surfaces pro Stage, immutable Borrows).
        let mut jobs: Vec<(String, (u32, u32), Vec<SurfaceEntry>)> = Vec::new();
        for stage_name in dirty {
            let canvas = canvas_size(&self.stages, &stage_name);
            let surfaces = self.surfaces_of_stage(&stage_name);
            jobs.push((stage_name, canvas, surfaces));
        }
        // Phase 2: parallel komponieren (reine Funktion, std::thread::scope).
        let rendered: Vec<(String, (u32, u32), Vec<u8>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|(name, canvas, surfaces)| {
                    scope.spawn(move || {
                        (
                            name.clone(),
                            *canvas,
                            super::compositor::composite_canvas(*canvas, surfaces),
                        )
                    })
                })
                .collect();
            // Scheitert eine Stage, läuft der Rest weiter: Die Engine bleibt am Leben.
            handles
                .into_iter()
                .filter_map(|h| match h.join() {
                    Ok(rendered) => Some(rendered),
                    Err(_) => {
                        tracing::error!("stage composite thread panicked, frame skipped");
                        None
                    }
                })
                .collect()
        });
        // Phase 3: in den Store legen + Dirty-Tracking + Routen markieren.
        for (stage_name, canvas, rgba) in rendered {
            let (cw, ch) = (canvas.0.max(1), canvas.1.max(1));
            self.stage_frames
                .lock()
                .unwrap()
                .insert(stage_name.clone(), Arc::new(VideoFrame::new(cw, ch, rgba)));
            // Surfaces für note_composited nachziehen.
            let surfaces = self.surfaces_of_stage(&stage_name);
            let frames = self.frames.lock().unwrap();
            let opacities = self.opacities.lock().unwrap();
            self.compositor
                .note_composited(&stage_name, &surfaces, &frames, &opacities);
            drop(frames);
            drop(opacities);
            // Stage-Canvas hat sich geaendert -> alle Routen dieser Stage
            // neu rendern (Regionen verweisen auf Routen).
            if let Some(stage) = self.stages.get(&stage_name) {
                for region in &stage.regions {
                    if let Some(route) = region.route {
                        self.route_dirty.insert(route);
                    }
                }
            }
        }
    }

    /// Rendert alle als dirty markierten Routen in den Route-Frame-Store
    /// (nur bei Aenderung: keine Stage geaendert -> keine Arbeit). Routen
    /// ohne Bild einmalig anlegen (schwarzer Raster).
    fn render_dirty_routes(&mut self) {
        let have: HashSet<Uuid> = self.route_frames.lock().unwrap().keys().copied().collect();
        for route in self.routes.routes() {
            if !have.contains(&route.id) {
                self.route_dirty.insert(route.id);
            }
        }
        let dirty = std::mem::take(&mut self.route_dirty);
        if dirty.is_empty() {
            return;
        }
        // Jobs: Route klonen; fehlende Route → aus dem Store nehmen.
        let mut jobs: Vec<OutputRoute> = Vec::new();
        {
            let mut route_frames = self.route_frames.lock().unwrap();
            for route_id in dirty {
                match self.routes.find(route_id).cloned() {
                    Some(route) => jobs.push(route),
                    None => {
                        route_frames.remove(&route_id);
                    }
                }
            }
        }
        if jobs.is_empty() {
            return;
        }
        // render_route sortiert die Stages selbst (Layer-Regel); reine Funktion
        // → parallel über alle dirty Routen rendern.
        let stages: Vec<Stage> = self.stages.values().cloned().collect();
        let stage_frames = self.stage_frames.lock().unwrap().clone();
        let stages_ref = &stages;
        let frames_ref = &stage_frames;
        let rendered: Vec<(Uuid, VideoFrame)> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|route| {
                    scope.spawn(move || {
                        (
                            route.id,
                            route_renderer::render_route(route, stages_ref, frames_ref),
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("route render thread"))
                .collect::<Vec<(Uuid, VideoFrame)>>()
        });
        let mut route_frames = self.route_frames.lock().unwrap();
        for (route_id, frame) in rendered {
            route_frames.insert(route_id, Arc::new(frame));
        }
    }

    /// Aktualisiert die Diashow (Auto-Advance/Crossfade) und schreibt das
    /// aktuelle Frame in den Store. Assoziierte Funktion (Feld-Referenzen).
    fn tick_slideshow(
        events: &Sender<EngineEvent>,
        frames: &FrameStore,
        cue_id: Uuid,
        state: &mut SlideShowState,
        now: Duration,
    ) {
        if let Some(at) = state.next_advance_at {
            if now >= at {
                match state.show.next_index(state.index) {
                    Some(next) => {
                        let prev = state.index;
                        state.index = next;
                        state.next_advance_at = if state.show.auto_advance {
                            Some(at + state.show.hold_at(next))
                        } else {
                            None
                        };
                        if state.show.transition != crate::core::SlideTransition::Cut
                            && !state.show.crossfade.is_zero()
                        {
                            state.crossfade = Some((prev, now));
                        }
                        let _ = events.send(EngineEvent::SlideChanged {
                            cue_id,
                            index: next,
                            at: now,
                        });
                    }
                    None => {
                        state.next_advance_at = None;
                    }
                }
            }
        }
        let current = match state.frames.get(state.index) {
            Some(frame) => frame.clone(),
            None => return,
        };
        let frame = match state.crossfade {
            Some((prev, started)) if !state.show.crossfade.is_zero() => {
                let t = (now.saturating_sub(started).as_secs_f32()
                    / state.show.crossfade.as_secs_f32())
                .clamp(0.0, 1.0);
                if t >= 1.0 {
                    state.crossfade = None;
                    current
                } else if let Some(prev_frame) = state.frames.get(prev) {
                    match state.show.transition {
                        crate::core::SlideTransition::DipToBlack => {
                            // Erst auf Schwarz ab-, dann einblenden.
                            let black = Arc::new(VideoFrame::black(current.width, current.height));
                            if t < 0.5 {
                                Arc::new(VideoFrame::blend(prev_frame, &black, t * 2.0))
                            } else {
                                Arc::new(VideoFrame::blend(&black, &current, (t - 0.5) * 2.0))
                            }
                        }
                        _ => Arc::new(VideoFrame::blend(prev_frame, &current, t)),
                    }
                } else {
                    current
                }
            }
            _ => current,
        };
        frames.lock().unwrap().insert(cue_id, frame);
    }

    /// Lädt die Slides einer Diashow vor (einmalig, `image`-Decode).
    fn preload_slideshow(&self, show: &SlideShow) -> Result<Vec<Arc<VideoFrame>>, String> {
        let mut frames = Vec::with_capacity(show.slides.len());
        for slide in &show.slides {
            let path = slide.path.display().to_string();
            if !Path::new(&path).exists() {
                return Err(format!("slide image not found: {path}"));
            }
            let img = image::open(&path)
                .map_err(|e| format!("cannot decode slide {path}: {e}"))?
                .to_rgba8();
            let (w, h) = img.dimensions();
            frames.push(Arc::new(VideoFrame::new(w, h, img.into_raw())));
        }
        Ok(frames)
    }

    /// Markiert die Stage eines Cues als dirty (Compositor).
    /// Assoziierte Funktion (Feld-Referenz auf den Compositor).
    fn mark_stage_dirty(compositor: &mut Compositor, playback: &ActivePlayback) {
        if let Some(stage) = &playback.stage {
            let stage = stage.clone();
            compositor.mark_dirty(&stage);
        }
    }
}

impl RealMediaEngine {
    /// Steht der Timecode (länger als [`CHASE_TIMECODE_TIMEOUT`] nichts
    /// empfangen), pausieren die Cues, die ihm folgen; der nächste Timecode
    /// setzt sie fort. Ohne je empfangenen Timecode läuft ein Cue normal.
    fn pause_chasing_when_timecode_stops(&mut self, now: Duration) {
        let Some(last) = self.last_timecode_at else {
            return;
        };
        if now.saturating_sub(last) < CHASE_TIMECODE_TIMEOUT {
            return;
        }
        let to_pause: Vec<Uuid> = self
            .chasing
            .keys()
            .filter(|id| !self.chase_paused.contains(id))
            .filter(|id| {
                self.active.get(id).is_some_and(|p| {
                    p.started_at.is_some() && p.finished_at.is_none() && p.paused_since.is_none()
                })
            })
            .copied()
            .collect();
        for cue_id in to_pause {
            let _ = self.handle(EngineCommand::Pause { cue_id });
            self.chase_paused.insert(cue_id);
        }
    }

    /// Externer Timecode (LTC/MTC/OSC) einspeisen: alle chasing Cues springen
    /// auf `tc - timecode_in`, sobald der Sprung mehr als einen Frame beträgt.
    /// Media-Backends (Audio/Video) seeken passend mit (Video: ffmpeg-Neustart
    /// ab Position, Pause-Zustand bleibt), Wallclock-Kinds (Html/Diashow/Wait)
    /// nur ihre akkumulierte Position – gespiegelt an die Mock-Semantik.
    pub fn receive_timecode(&mut self, tc: Timecode) {
        let frame = tc.fps.frame_duration();
        let now = self.now();
        // Timecode läuft (wieder): vom Chasing pausierte Cues fortsetzen.
        self.last_timecode_at = Some(now);
        for cue_id in std::mem::take(&mut self.chase_paused) {
            if self.chasing.contains_key(&cue_id) {
                let _ = self.handle(EngineCommand::Resume { cue_id });
            }
        }
        let mut jumps: Vec<(Uuid, Duration, Duration)> = Vec::new();
        for (cue_id, offset) in &self.chasing {
            let Some(playback) = self.active.get(cue_id) else {
                continue;
            };
            if playback.started_at.is_none() || playback.finished_at.is_some() {
                continue;
            }
            let target = tc.to_duration().saturating_sub(offset.to_duration());
            let current = Self::position_of(&self.audio, &self.video, playback, *cue_id, now);
            // Exakter Vergleich in Nanosekunden (Duration) – f64 wäre bei
            // genau einem Frame Toleranz durch Rundung unzuverlässig.
            let delta = target.abs_diff(current);
            if delta > frame {
                let clamped = target.min(playback.current_unit());
                jumps.push((*cue_id, current, clamped));
            }
        }
        for (cue_id, from, to) in jumps {
            if let Some(playback) = self.active.get_mut(&cue_id) {
                match &playback.kind {
                    PlaybackKind::Audio => {
                        if let Some(audio) = self.audio.as_mut() {
                            audio.seek(cue_id, to);
                        }
                    }
                    PlaybackKind::Video => {
                        if let Some(path) = playback.path.clone() {
                            let was_paused = self.video.is_paused(cue_id);
                            let _ = self.video.play(cue_id, &path, to);
                            if was_paused {
                                self.video.pause(cue_id);
                            }
                        }
                    }
                    // Wallclock-Kinds: Position direkt setzen.
                    _ => {
                        playback.accumulated = to;
                        playback.started_at = Some(now);
                    }
                }
            }
            debug!(%cue_id, ?from, ?to, "timecode chase jump (real)");
            self.emit(EngineEvent::TimecodeJump { cue_id, from, to });
            self.emit(EngineEvent::Position {
                cue_id,
                position: to,
            });
        }
    }
}

/// Meldet das Ergebnis eines asynchronen Sends (ggf. als `EngineEvent::Error`)
/// – läuft im gespawnten Send-Thread.
fn finish_send(events: Sender<EngineEvent>, cue_id: Uuid, label: &str, result: Result<(), String>) {
    if let Err(message) = result {
        let _ = events.send(EngineEvent::Error {
            cue_id: Some(cue_id),
            message: format!("{label} send failed: {message}"),
        });
    }
}

/// Sendet `bytes` per UDP an `host:port` in einem eigenen Thread
/// (fire-and-forget; ein blockierendes Ziel hält die Engine nicht auf).
fn spawn_udp_send(
    events: Sender<EngineEvent>,
    cue_id: Uuid,
    label: &'static str,
    host: String,
    port: u16,
    bytes: Vec<u8>,
) {
    std::thread::spawn(move || {
        let result = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
            .and_then(|socket| {
                // Broadcast-Ziele (255.255.255.255) brauchen SO_BROADCAST,
                // sonst EACCES/Permission denied (Default-Host der Light-Cues).
                socket.set_broadcast(true)?;
                socket.send_to(&bytes, (host.as_str(), port)).map(|_| ())
            })
            .map_err(|err| err.to_string());
        finish_send(events, cue_id, label, result);
    });
}

impl MediaEngine for RealMediaEngine {
    fn name(&self) -> &'static str {
        "real-media-engine"
    }

    fn handle(&mut self, command: EngineCommand) -> ShowResult<()> {
        let now = self.now();
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
                surface,
                in_point,
                out_point,
            } => {
                debug!(%cue_id, ?path, ?duration, ?stage, layer, "play (real)");
                self.release_held(cue_id);
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
                if let Some(surface) = &surface {
                    if let Err(err) = surface.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
                // Audio vs. Video aus dem Medium ableiten: ffprobe findet
                // einen Video-Stream → Video, sonst Audio.
                let kind = match &path {
                    Some(path) if !path.trim().is_empty() => {
                        if super::video_backend::probe(path).is_ok() {
                            PlaybackKind::Video
                        } else {
                            PlaybackKind::Audio
                        }
                    }
                    _ => PlaybackKind::Audio,
                };
                // Slice-Plan abflachen (Dauer + Startposition pro Segment).
                let mut plan = Vec::new();
                let mut plan_offset = Vec::new();
                for slice in &slices {
                    for _ in 0..slice.play_count.max(1) {
                        plan.push(slice.duration());
                        plan_offset.push(slice.start);
                    }
                }
                if let Some(tc) = timecode_in {
                    self.chasing.insert(cue_id, tc);
                }
                let media_duration = if matches!(kind, PlaybackKind::Video) {
                    path.as_ref()
                        .and_then(|p| super::video_backend::probe(p).ok())
                        .and_then(|info| info.duration)
                } else {
                    None
                };
                let playback = ActivePlayback {
                    kind,
                    path,
                    route,
                    stage: stage.clone(),
                    layer,
                    surface,
                    received_at: now,
                    pre_wait,
                    started_at: None,
                    accumulated: Duration::ZERO,
                    paused_since: None,
                    in_at: in_point,
                    out_at: out_point,
                    duration: duration.unwrap_or(Duration::ZERO),
                    loop_mode,
                    plan,
                    plan_offset,
                    plan_index: 0,
                    iterations_done: 0,
                    finished_at: None,
                    post_wait,
                    post_wait_done: false,
                    media_duration,
                };
                Self::mark_stage_dirty(&mut self.compositor, &playback);
                self.active.insert(cue_id, playback);
            }
            EngineCommand::Wait {
                cue_id,
                duration,
                pre_wait,
                post_wait,
            } => {
                debug!(%cue_id, ?duration, ?pre_wait, ?post_wait, "wait (real)");
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        kind: PlaybackKind::Wait,
                        path: None,
                        route: None,
                        stage: None,
                        layer: 0,
                        surface: None,
                        in_at: None,
                        out_at: None,
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration,
                        loop_mode: LoopMode::Once,
                        plan: Vec::new(),
                        plan_offset: Vec::new(),
                        plan_index: 0,
                        iterations_done: 0,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                        media_duration: None,
                    },
                );
            }
            EngineCommand::Pause { cue_id } => {
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    if let Some(start) = playback.started_at {
                        if playback.paused_since.is_none() && playback.finished_at.is_none() {
                            playback.accumulated += now.saturating_sub(start);
                            playback.started_at = None;
                            playback.paused_since = Some(now);
                            match &playback.kind {
                                PlaybackKind::Audio => {
                                    if let Some(audio) = self.audio.as_mut() {
                                        audio.pause(cue_id);
                                    }
                                }
                                PlaybackKind::Video => {
                                    self.video.pause(cue_id);
                                    if let Some(audio) = self.audio.as_mut() {
                                        audio.pause(cue_id);
                                    }
                                }
                                _ => {}
                            }
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
                        match &playback.kind {
                            PlaybackKind::Audio => {
                                if let Some(audio) = self.audio.as_mut() {
                                    audio.resume(cue_id);
                                }
                            }
                            PlaybackKind::Video => {
                                self.video.resume(cue_id);
                                if let Some(audio) = self.audio.as_mut() {
                                    audio.resume(cue_id);
                                }
                            }
                            _ => {}
                        }
                        self.emit(EngineEvent::Resumed { cue_id });
                    }
                }
            }
            EngineCommand::HoldLastFrame { cue_id } => {
                self.hold_on_end.insert(cue_id);
            }
            EngineCommand::StopWhenVisible {
                cue_id,
                stops,
                wait_for_picture,
            } => {
                if wait_for_picture {
                    // Bild eines früheren Laufs zählt nicht.
                    self.frames.lock().unwrap().remove(&cue_id);
                    self.deferred_stops.push(DeferredStop { cue_id, stops });
                } else {
                    for cue_id in stops {
                        let _ = self.handle(EngineCommand::Stop { cue_id });
                    }
                }
            }
            EngineCommand::Stop { cue_id } => {
                // HTML5 mit geladener Seite: erst Trigger „out" (Animation),
                // gestoppt wird nach TriggerDone/Timeout – ein zweites Stop
                // (closing läuft schon) beendet sofort.
                if let Some(html) = self.html.get_mut(&cue_id) {
                    if html.closing_since.is_none() && html.loaded {
                        if let Some(webview) = &mut html.webview {
                            if webview.trigger("out", serde_json::Value::Null).is_ok() {
                                html.closing_since = Some(now);
                                return Ok(());
                            }
                        }
                    }
                }
                self.hold_on_end.remove(&cue_id);
                self.release_held(cue_id);
                self.deferred_stops.retain(|d| d.cue_id != cue_id);
                // Script-Cue: WebView-Instanz schließen (Drop killt Subprocess).
                self.scripts.remove(&cue_id);
                self.pending_html_data.remove(&cue_id);
                if let Some(playback) = self.active.remove(&cue_id) {
                    Self::stop_backend(&mut self.audio, &mut self.video, cue_id, &playback);
                    self.audio_voices.lock().unwrap().remove(&cue_id);
                    self.chasing.remove(&cue_id);
                    self.html.remove(&cue_id);
                    self.drop_fades_on(cue_id);
                    self.current_volumes.remove(&cue_id);
                    self.frames.lock().unwrap().remove(&cue_id);
                    self.opacities.lock().unwrap().remove(&cue_id);
                    Self::mark_stage_dirty(&mut self.compositor, &playback);
                    self.emit(EngineEvent::Stopped { cue_id });
                } else if self.cancel_fade_cue(cue_id) {
                    // Laufender Fade-Cue gestoppt: Fades anhalten.
                    self.emit(EngineEvent::Stopped { cue_id });
                }
            }
            EngineCommand::StopAll => {
                self.hold_on_end.clear();
                self.deferred_stops.clear();
                for cue_id in self.held.keys().copied().collect::<Vec<_>>() {
                    self.release_held(cue_id);
                }
                let stopped: Vec<Uuid> = self.active.keys().copied().collect();
                let stages: Vec<String> = self
                    .active
                    .values()
                    .filter_map(|p| p.stage.clone())
                    .collect();
                self.active.clear();
                self.fades.clear();
                self.fade_pending.clear();
                self.fade_stop_target.clear();
                self.current_volumes.clear();
                self.chasing.clear();
                self.chase_paused.clear();
                self.html.clear();
                self.pending_html_data.clear();
                self.scripts.clear();
                if let Some(audio) = self.audio.as_mut() {
                    for cue_id in &stopped {
                        audio.stop_voice(*cue_id);
                    }
                }
                for cue_id in &stopped {
                    self.video.stop(*cue_id);
                }
                self.frames.lock().unwrap().clear();
                self.opacities.lock().unwrap().clear();
                for stage in stages {
                    self.compositor.mark_dirty(&stage);
                }
                for cue_id in stopped {
                    self.emit(EngineEvent::Stopped { cue_id });
                }
            }
            EngineCommand::Fade {
                cue_id,
                target_cue,
                from,
                volume,
                opacity,
                duration,
                curve,
                stop_when_done,
            } => {
                let target = target_cue.unwrap_or(cue_id);
                let running = self.active.contains_key(&target) || self.html.contains_key(&target);
                if !running {
                    // Ziel läuft nicht: Fade endet sofort ohne Wirkung und ohne
                    // Engine-Fehler (AUFTRAG §2.5) – außer derselbe Fade-Cue hat
                    // noch Fades auf anderen Gruppen-Kindern laufen.
                    if !self.fade_pending.contains_key(&cue_id) {
                        self.emit(EngineEvent::Finished { cue_id });
                    }
                    return Ok(());
                }
                let mut n = 0u32;
                for (param, to) in [(FadeParam::Volume, volume), (FadeParam::Opacity, opacity)] {
                    let Some(to) = to else { continue };
                    if duration.is_zero() {
                        self.apply_fade_value(target, param, to);
                        continue;
                    }
                    // from=None: aktueller Wert des Ziels (kein Sprung).
                    let from_v = from.unwrap_or_else(|| self.current_value(target, param));
                    // Opacity läuft immer linear, die Kurve gilt nur für Volume.
                    let curve = match param {
                        FadeParam::Volume => curve,
                        _ => FadeCurve::Linear,
                    };
                    self.insert_fade(
                        (target, param),
                        ActiveFade {
                            fade_cue: cue_id,
                            from: from_v,
                            target: to,
                            duration,
                            curve,
                            started_at: now,
                            last_value: from_v,
                        },
                    );
                    self.apply_fade_value(target, param, from_v);
                    n += 1;
                }
                if n == 0 {
                    // Duration 0 (Werte schon gesetzt) oder weder Volume noch
                    // Opacity aktiv: sofort fertig.
                    if stop_when_done {
                        self.stop_cue_simple(target);
                    }
                    if !self.fade_pending.contains_key(&cue_id) {
                        self.emit(EngineEvent::Finished { cue_id });
                    }
                } else {
                    // Gruppen-Ziel: mehrere Fade-Kommandos desselben Fade-Cues
                    // (eins je Kind) – Finished erst, wenn alle durch sind.
                    *self.fade_pending.entry(cue_id).or_insert(0) += n;
                    if stop_when_done {
                        self.fade_stop_target
                            .entry(cue_id)
                            .or_default()
                            .push(target);
                    }
                }
            }
            EngineCommand::Seek { cue_id, position } => {
                if let Some(playback) = self.active.get(&cue_id) {
                    let kind = playback.kind.clone();
                    let path = playback.path.clone();
                    match &kind {
                        PlaybackKind::Audio => {
                            if let Some(audio) = self.audio.as_mut() {
                                audio.seek(cue_id, position);
                            }
                        }
                        PlaybackKind::Video => {
                            if let Some(path) = &path {
                                let _ = self.video.seek(cue_id, path, position);
                            }
                            if let Some(audio) = self.audio.as_mut() {
                                audio.seek(cue_id, position);
                            }
                        }
                        _ => {}
                    }
                    let from = Self::position_of(&self.audio, &self.video, playback, cue_id, now);
                    self.emit(EngineEvent::TimecodeJump {
                        cue_id,
                        from,
                        to: position,
                    });
                }
            }
            EngineCommand::SetVolume { cue_id, volume } => {
                self.current_volumes.insert(cue_id, volume);
                if let Some(audio) = self.audio.as_mut() {
                    audio.set_volume(cue_id, volume);
                }
            }
            EngineCommand::PreloadNext { cue_id, path } => {
                self.preload_video(cue_id, &path, false);
            }
            EngineCommand::Unload { cue_id } => {
                let unloaded = match cue_id {
                    Some(id) => self.video.unload(id).then_some(id).into_iter().collect(),
                    None => self.video.unload_all(),
                };
                for cue_id in unloaded {
                    self.emit(EngineEvent::Unloaded { cue_id });
                }
            }
            EngineCommand::Load { cue_id, path } => match &path {
                Some(path) if Path::new(path).exists() => {
                    self.preload_video(cue_id, path, true);
                    self.emit(EngineEvent::Loaded { cue_id });
                }
                _ => {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: format!(
                            "cannot load media: {}",
                            path.as_deref().unwrap_or("<no path>")
                        ),
                    });
                }
            },
            EngineCommand::Devamp { cue_id } => {
                let was_running = self
                    .active
                    .get(&cue_id)
                    .map(|p| p.finished_at.is_none())
                    .unwrap_or(false);
                if was_running {
                    if let Some(playback) = self.active.get_mut(&cue_id) {
                        playback.finished_at = Some(now);
                    }
                    // Backend stoppen (Loop sofort beenden) – vor dem Entfernen.
                    if let Some(playback) = self.active.get(&cue_id) {
                        Self::stop_backend(&mut self.audio, &mut self.video, cue_id, playback);
                    }
                    self.emit(EngineEvent::Finished { cue_id });
                    // Post-Wait 0 → sofort entfernen + PostWaitElapsed.
                    let post_wait_zero = self
                        .active
                        .get(&cue_id)
                        .map(|p| p.post_wait.is_zero())
                        .unwrap_or(false);
                    if post_wait_zero {
                        let stage = self.active.get(&cue_id).and_then(|p| p.stage.clone());
                        self.active.remove(&cue_id);
                        self.chasing.remove(&cue_id);
                        self.html.remove(&cue_id);
                        if let Some(stage) = &stage {
                            let stage = stage.clone();
                            self.compositor.mark_dirty(&stage);
                        }
                        self.emit(EngineEvent::PostWaitElapsed { cue_id });
                    }
                }
            }
            EngineCommand::DevampAll => {
                let ids: Vec<Uuid> = self
                    .active
                    .iter()
                    .filter(|(_, p)| p.finished_at.is_none())
                    .map(|(id, _)| *id)
                    .collect();
                for cue_id in ids {
                    self.handle(EngineCommand::Devamp { cue_id })?;
                }
            }
            EngineCommand::ChaseTimecode {
                cue_id,
                timecode_in,
            } => match timecode_in {
                Some(tc) => {
                    self.chasing.insert(cue_id, tc);
                }
                None => {
                    self.chasing.remove(&cue_id);
                }
            },
            EngineCommand::SetAudioPatch { patch } => {
                self.audio_patch = (*patch).clone();
                self.apply_audio_patch();
            }
            EngineCommand::SetAudioOutput { device } => {
                // Laufende Voices lassen sich nicht umziehen; ein Wechsel würde
                // sie mitten in der Show abreißen.
                let audio_running = self
                    .active
                    .values()
                    .any(|playback| matches!(playback.kind, PlaybackKind::Audio));
                if audio_running {
                    self.emit(EngineEvent::Error {
                        cue_id: None,
                        message: "Stop running audio cues before changing the output.".to_string(),
                    });
                    return Ok(());
                }
                // Primäres Gerät neu aufbauen; Mixer (Voices) und Patch mitnehmen.
                let opened = match &self.audio {
                    Some(old) => {
                        let mixer = old.mixer();
                        AudioBackend::with_shared_mixer(device.as_deref(), mixer)
                    }
                    None => AudioBackend::with_device(device.as_deref()),
                };
                match opened {
                    Ok(backend) => {
                        backend.set_channel_outputs(self.audio_patch.device_output_mask(None));
                        self.audio = Some(backend);
                        self.audio_broken_reported = false;
                        self.emit(EngineEvent::AudioOutputChanged { device });
                    }
                    Err(message) => self.emit(EngineEvent::Error {
                        cue_id: None,
                        message,
                    }),
                }
            }
            EngineCommand::ConfigureStage { stage } => {
                if let Err(err) = stage.validate() {
                    self.emit(EngineEvent::Error {
                        cue_id: None,
                        message: err.to_string(),
                    });
                    return Ok(());
                }
                let name = stage.name.clone();
                let regions = stage.regions.clone();
                self.stages.insert(name.clone(), stage);
                self.compositor.mark_dirty(&name);
                for region in regions {
                    if let Some(route) = region.route {
                        self.route_dirty.insert(route);
                    }
                }
                // Decklink: Stage-Auflösung != Karte → Frames werden per
                // Nearest-Neighbor skaliert (siehe tick_decklink).
                if self.decklink.is_some() && name == self.decklink_stage {
                    let (cw, ch) = canvas_size(&self.stages, &name);
                    if let Some(out) = &self.decklink {
                        if (cw, ch) != (out.width(), out.height()) {
                            info!(
                                stage = %name,
                                canvas = %format!("{cw}x{ch}"),
                                decklink = %format!("{}x{}", out.width(), out.height()),
                                "decklink: Stage-Auflösung != Decklink-Auflösung – wird skaliert"
                            );
                        }
                    }
                }
                self.emit(EngineEvent::StageConfigured { stage_name: name });
            }
            EngineCommand::ConfigureRoutes { routes } => {
                debug!(count = routes.len(), "configure routes");
                for route in routes.routes() {
                    if let Err(err) = route.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: None,
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
                self.routes = routes;
                // Konfiguration geaendert -> alle Routen neu rendern.
                for route in self.routes.routes() {
                    self.route_dirty.insert(route.id);
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
                debug!(%cue_id, source = %source.describe(), ?stage, layer, "play html (real)");
                if let Err(err) = source.validate() {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: err.to_string(),
                    });
                    return Ok(());
                }
                if let HtmlSource::File(path) = &source {
                    let path = path.display().to_string();
                    if !Path::new(&path).exists() {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: format!("html file not found: {path}"),
                        });
                        return Ok(());
                    }
                }
                self.active.insert(
                    cue_id,
                    ActivePlayback {
                        kind: PlaybackKind::Html(source),
                        path: None,
                        route: None,
                        stage,
                        layer,
                        surface: None,
                        in_at: None,
                        out_at: None,
                        received_at: now,
                        pre_wait,
                        started_at: None,
                        accumulated: Duration::ZERO,
                        paused_since: None,
                        duration: duration.unwrap_or(Duration::ZERO),
                        loop_mode: LoopMode::Once,
                        plan: Vec::new(),
                        plan_offset: Vec::new(),
                        plan_index: 0,
                        iterations_done: 0,
                        finished_at: None,
                        post_wait,
                        post_wait_done: false,
                        media_duration: None,
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
                surface,
            } => {
                debug!(%cue_id, slides = show.slide_count(), "play slideshow (real)");
                if let Err(err) = show.validate() {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: err.to_string(),
                    });
                    return Ok(());
                }
                if let Some(surface) = &surface {
                    if let Err(err) = surface.validate() {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: err.to_string(),
                        });
                        return Ok(());
                    }
                }
                match self.preload_slideshow(&show) {
                    Ok(frames) => {
                        let playback = ActivePlayback {
                            in_at: None,
                            out_at: None,
                            kind: PlaybackKind::SlideShow(SlideShowState {
                                show,
                                frames,
                                index: 0,
                                started_at: now,
                                next_advance_at: None,
                                crossfade: None,
                            }),
                            path: None,
                            route: None,
                            stage: stage.clone(),
                            layer,
                            surface,
                            received_at: now,
                            pre_wait,
                            started_at: None,
                            accumulated: Duration::ZERO,
                            paused_since: None,
                            duration: duration.unwrap_or(Duration::ZERO),
                            loop_mode: LoopMode::Once,
                            plan: Vec::new(),
                            plan_offset: Vec::new(),
                            plan_index: 0,
                            iterations_done: 0,
                            finished_at: None,
                            post_wait,
                            post_wait_done: false,
                            media_duration: None,
                        };
                        Self::mark_stage_dirty(&mut self.compositor, &playback);
                        self.active.insert(cue_id, playback);
                    }
                    Err(message) => {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message,
                        });
                    }
                }
            }
            EngineCommand::EvalJs { cue_id, script } => {
                if let Some(html) = self.html.get_mut(&cue_id) {
                    // Echtes WebView-Backend: Script an den Subprocess senden.
                    if let Some(webview) = &mut html.webview {
                        if let Err(err) = webview.eval_js(&script) {
                            warn!(%cue_id, %err, "webview eval_js fehlgeschlagen");
                        }
                    }
                    self.emit(EngineEvent::JsExecuted {
                        cue_id,
                        script,
                        at: now,
                    });
                } else {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: "no active html instance for this cue".to_string(),
                    });
                }
            }
            EngineCommand::PushJson { target, payload } => {
                if let Some(html) = self.html.get_mut(&target) {
                    // Echtes Backend: Payload pushen – vor dem Laden puffern,
                    // sonst ginge sie an die noch leere Seite verloren.
                    if let Some(webview) = &mut html.webview {
                        if !html.loaded {
                            html.pending_data.push(payload.clone());
                        } else if let Err(err) = webview.push_json(&payload) {
                            warn!(%target, %err, "webview push_json fehlgeschlagen");
                        }
                    }
                    self.emit(EngineEvent::JsonReceived {
                        target,
                        payload,
                        at: now,
                    });
                } else if self
                    .active
                    .get(&target)
                    .is_some_and(|p| matches!(p.kind, PlaybackKind::Html(_)))
                {
                    // HTML-Cue gestartet, Instanz kommt im nächsten Tick.
                    self.pending_html_data
                        .entry(target)
                        .or_default()
                        .push(payload.clone());
                    self.emit(EngineEvent::JsonReceived {
                        target,
                        payload,
                        at: now,
                    });
                } else {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(target),
                        message: "no active html instance for target cue".to_string(),
                    });
                }
            }
            EngineCommand::SlideNav { cue_id, nav } => {
                let now = self.now();
                let mut new_index: Option<usize> = None;
                let mut err: Option<String> = None;
                if let Some(playback) = self.active.get_mut(&cue_id) {
                    if let PlaybackKind::SlideShow(state) = &mut playback.kind {
                        let target = match nav {
                            SlideNav::Next => state.show.next_index(state.index),
                            SlideNav::Prev => Some(state.show.prev_index(state.index)),
                        };
                        if let Some(next) = target {
                            if next != state.index {
                                let prev = state.index;
                                state.index = next;
                                if !state.show.crossfade.is_zero() {
                                    state.crossfade = Some((prev, now));
                                }
                                // Frame sofort aktualisieren.
                                if let Some(frame) = state.frames.get(next).cloned() {
                                    self.frames.lock().unwrap().insert(cue_id, frame);
                                }
                                let stage = playback.stage.clone();
                                if let Some(stage) = &stage {
                                    let stage = stage.clone();
                                    self.compositor.mark_dirty(&stage);
                                }
                            }
                        }
                        // Nächsten Auto-Advance ab dem manuell gewählten Dia.
                        state.next_advance_at = if state.show.auto_advance {
                            Some(now + state.show.hold_at(state.index))
                        } else {
                            None
                        };
                        new_index = Some(state.index);
                    } else {
                        err = Some(format!("cue {cue_id} is not a running slideshow"));
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
                debug!(%cue_id, ?protocol, %host, port, "send network (real)");
                // Fire-and-forget in einem eigenen Thread: ein blockierendes
                // Ziel (TCP-Connect-Timeout, DNS) darf den Engine-Thread nicht
                // aufhalten. Fehler landen als Error-Event.
                let events = self.events.clone();
                if protocol == crate::core::NetworkProtocol::Udp {
                    spawn_udp_send(
                        events,
                        cue_id,
                        "network cue",
                        host,
                        port,
                        message.into_bytes(),
                    );
                    return Ok(());
                }
                std::thread::spawn(move || {
                    let result: Result<(), String> = {
                        use std::io::Write;
                        let addr = match (host.as_str(), port).to_socket_addrs() {
                            Ok(mut addrs) => match addrs.next() {
                                Some(addr) => addr,
                                None => {
                                    return finish_send(
                                        events,
                                        cue_id,
                                        "network cue",
                                        Err("no address resolved".to_string()),
                                    );
                                }
                            },
                            Err(err) => {
                                return finish_send(
                                    events,
                                    cue_id,
                                    "network cue",
                                    Err(err.to_string()),
                                );
                            }
                        };
                        std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500))
                            .and_then(|mut stream| {
                                stream.write_all(message.as_bytes())?;
                                stream.flush()?;
                                Ok(())
                            })
                            .map_err(|err| err.to_string())
                    };
                    finish_send(events, cue_id, "network cue", result);
                });
            }
            EngineCommand::SendDmx { cue_id, message } => {
                // Paket aus dem Universe-Stand bauen (start_channel wird
                // eingemischt, Kanäle davor bleiben auf ihrem letzten Wert).
                let state = self
                    .dmx_state
                    .entry((message.protocol, message.universe))
                    .or_insert([0u8; crate::core::light::DMX_CHANNELS]);
                message.apply_to(state);
                let (packet, port) = match message.protocol {
                    crate::core::LightProtocol::ArtNet => (
                        crate::core::light::artnet_packet(message.universe, state, 0),
                        message.protocol.port(),
                    ),
                    crate::core::LightProtocol::Sacn => {
                        let seq = self.sacn_seq.entry(message.universe).or_insert(0);
                        *seq = seq.wrapping_add(1);
                        (
                            crate::core::light::sacn_packet(
                                message.universe,
                                state,
                                *seq,
                                &self.sacn_cid,
                                "KuhPlay",
                            ),
                            message.protocol.port(),
                        )
                    }
                };
                debug!(%cue_id, host = %message.host, port, packet_len = packet.len(), "send dmx (real)");
                // Art-Net/sACN: UDP an Protokoll-Port, fire-and-forget wie Network-UDP.
                spawn_udp_send(
                    self.events.clone(),
                    cue_id,
                    "light cue",
                    message.host.clone(),
                    port,
                    packet,
                );
            }
            EngineCommand::SendMidi { cue_id, bytes } => {
                debug!(%cue_id, ?bytes, "send midi (real)");
                // Ausgang lazy öffnen (erstes SendMidi); Fehler → Error-Event.
                if self.midi_out.is_none() {
                    match super::midi_out::open_midi_output(
                        super::midi_out::midi_out_device_from_env().as_deref(),
                    ) {
                        Ok(guard) => {
                            info!(port = %guard.port_name, "midi output connected");
                            self.midi_out = Some(guard);
                        }
                        Err(err) => {
                            self.emit(EngineEvent::Error {
                                cue_id: Some(cue_id),
                                message: format!("midi cue: {err}"),
                            });
                            return Ok(());
                        }
                    }
                }
                if let Some(out) = self.midi_out.as_mut() {
                    if let Err(err) = out.send(&bytes) {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: format!("midi cue send failed: {err}"),
                        });
                    }
                }
            }
            EngineCommand::InjectTimecode { tc } => {
                self.receive_timecode(tc);
            }
            EngineCommand::RunScript { cue_id, script } => {
                debug!(%cue_id, script_len = script.len(), "run script (real)");
                // User-JS in eine Inline-Seite kapseln (</script> escapen).
                let escaped = script.replace("</script>", "<\\/script>");
                let html =
                    format!("<!DOCTYPE html><html><body><script>{escaped}</script></body></html>");
                let Some(bin) = self.renderer_bin.clone() else {
                    self.emit(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: "script cue: no cef binary (build with --features cef)"
                            .to_string(),
                    });
                    return Ok(());
                };
                match super::webview_backend::WebviewInstance::spawn(
                    &bin,
                    cue_id,
                    &crate::core::HtmlSource::Inline(html),
                ) {
                    Ok(instance) => {
                        self.scripts.insert(
                            cue_id,
                            ScriptState {
                                webview: instance,
                                finish_at: None,
                            },
                        );
                    }
                    Err(err) => {
                        self.emit(EngineEvent::Error {
                            cue_id: Some(cue_id),
                            message: format!("script cue: webview spawn failed: {err}"),
                        });
                    }
                }
            }
            EngineCommand::SendHttpPost {
                cue_id,
                host,
                port,
                path,
            } => {
                debug!(%cue_id, %host, port, %path, "http post (real)");
                // Fire-and-forget im eigenen Thread (Connect-Timeout 500 ms).
                let events = self.events.clone();
                std::thread::spawn(move || {
                    use std::io::{Read, Write};
                    let result: Result<(), String> = (|| {
                        let addr = (host.as_str(), port)
                            .to_socket_addrs()
                            .map_err(|e| e.to_string())?
                            .next()
                            .ok_or_else(|| "no address resolved".to_string())?;
                        let mut stream =
                            std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500))
                                .map_err(|e| e.to_string())?;
                        let request = format!(
                            "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        stream
                            .write_all(request.as_bytes())
                            .map_err(|e| e.to_string())?;
                        stream.flush().map_err(|e| e.to_string())?;
                        // Statuszeile auswerten: Companion meldet z. B. 404 für
                        // einen unbekannten Button – sonst schiene alles ok.
                        stream
                            .set_read_timeout(Some(Duration::from_millis(1000)))
                            .ok();
                        let mut buf = [0u8; 256];
                        let n = stream
                            .read(&mut buf)
                            .map_err(|e| format!("no answer: {e}"))?;
                        let answer = String::from_utf8_lossy(&buf[..n]);
                        let status = answer.lines().next().unwrap_or_default();
                        match status.split_whitespace().nth(1) {
                            Some(code) if code.starts_with('2') => Ok(()),
                            Some(_) => Err(format!("{host}:{port}{path} → {status}")),
                            None => Err(format!("{host}:{port}{path}: no HTTP answer")),
                        }
                    })();
                    finish_send(events, cue_id, "companion cue", result);
                });
            }
            EngineCommand::Ping(seq) => {
                self.emit(EngineEvent::Pong(seq));
            }
        }
        Ok(())
    }

    fn tick(&mut self) {
        let now = self.now();
        let mut out: Vec<EngineEvent> = Vec::new();

        // Fades: Zwischenwerte + Abschluss (linear, dB oder Equal-Power).
        let mut fades_done: Vec<(Uuid, FadeParam)> = Vec::new();
        for ((target, param), fade) in &mut self.fades {
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
                // Equal-Power für Crossfade-Ton: cos(t·π/2) abwärts, sin(t·π/2) aufwärts.
                FadeCurve::EqualPowerDown => {
                    fade.target
                        + (fade.from - fade.target) * (progress * std::f32::consts::FRAC_PI_2).cos()
                }
                FadeCurve::EqualPowerUp => {
                    fade.from
                        + (fade.target - fade.from) * (progress * std::f32::consts::FRAC_PI_2).sin()
                }
            };
            if value != fade.last_value {
                fade.last_value = value;
                self.pending_fade_values
                    .push((*target, fade.fade_cue, *param, value));
                out.push(EngineEvent::FadeProgress {
                    cue_id: fade.fade_cue,
                    param: *param,
                    value,
                });
            }
            if progress >= 1.0 {
                fades_done.push((*target, *param));
            }
        }
        // Erst die Werte anwenden (Endwert sitzt), dann abrechnen:
        // Stop-when-done stoppt das Ziel, bevor Finished gemeldet wird.
        let pending_fades = std::mem::take(&mut self.pending_fade_values);
        for (target, _fade_cue, param, value) in pending_fades {
            self.apply_fade_value(target, param, value);
        }
        // settle_fade meldet direkt – Progress-Events vorher raus (Reihenfolge).
        for event in out.drain(..) {
            self.emit(event);
        }
        for (target, param) in fades_done {
            if let Some(fade) = self.fades.remove(&(target, param)) {
                self.settle_fade(fade.fade_cue, true);
            }
        }

        // Playbacks: Pre-Wait → Laufend → Loops/Slices → Finished → Post-Wait.
        let mut finished_now = Vec::new();
        let mut post_wait_elapsed = Vec::new();
        let mut html_starts: Vec<(Uuid, HtmlSource, Option<String>, u32)> = Vec::new();
        let mut restarts: Vec<Uuid> = Vec::new();
        // Backend-Startfehler (werden nach dem Loop gemeldet/gemeldet+entfernt).
        let mut start_errors: Vec<(Uuid, String)> = Vec::new();

        let active_ids: Vec<Uuid> = self.active.keys().copied().collect();
        for cue_id in active_ids {
            let Some(playback) = self.active.get_mut(&cue_id) else {
                continue;
            };
            // Post-Wait-Phase nach Finished.
            if let Some(finished_at) = playback.finished_at {
                if !playback.post_wait_done && now.saturating_sub(finished_at) >= playback.post_wait
                {
                    playback.post_wait_done = true;
                    post_wait_elapsed.push(cue_id);
                }
                continue;
            }
            // Pre-Wait-Phase.
            if playback.started_at.is_none() && playback.paused_since.is_none() {
                if now.saturating_sub(playback.received_at) >= playback.pre_wait {
                    playback.started_at = Some(now);
                    let kind = playback.kind.clone();
                    let path = playback.path.clone();
                    let route = playback.route;
                    let offset = playback.plan_offset.first().copied().unwrap_or_default()
                        + playback.in_at.unwrap_or(Duration::ZERO);
                    match &mut playback.kind {
                        PlaybackKind::Audio | PlaybackKind::Video => {
                            match Self::start_media_backend(
                                &mut self.audio,
                                &mut self.video,
                                &mut self.audio_broken_reported,
                                cue_id,
                                &kind,
                                &path,
                                offset,
                                route,
                            ) {
                                Ok(media_duration) => {
                                    if playback.media_duration.is_none() {
                                        playback.media_duration = media_duration;
                                    }
                                    self.audio_voices.lock().unwrap().insert(cue_id);
                                }
                                // Fehler: nach dem Loop melden + Playback entfernen.
                                Err(message) => start_errors.push((cue_id, message)),
                            }
                        }
                        PlaybackKind::Html(source) => {
                            html_starts.push((
                                cue_id,
                                source.clone(),
                                playback.stage.clone(),
                                playback.layer,
                            ));
                        }
                        PlaybackKind::SlideShow(state) => {
                            state.index = 0;
                            state.started_at = now;
                            state.crossfade = None;
                            state.next_advance_at = if state.show.auto_advance {
                                Some(now + state.show.hold_at(0))
                            } else {
                                None
                            };
                            out.push(EngineEvent::SlideChanged {
                                cue_id,
                                index: 0,
                                at: now,
                            });
                            if let Some(frame) = state.frames.first().cloned() {
                                self.frames.lock().unwrap().insert(cue_id, frame);
                            }
                        }
                        // Wait-Cue: kein Backend-Start, Position läuft über die Wanduhr.
                        PlaybackKind::Wait => {}
                    }
                    // Started erst nach Backend-Start: Medien-Dauer ist dann bekannt.
                    out.push(EngineEvent::Started {
                        cue_id,
                        duration: playback.media_duration,
                    });
                } else {
                    continue;
                }
            }
            // Pausiert → nichts zu tun.
            if playback.paused_since.is_some() {
                continue;
            }

            let unit = playback.current_unit();
            let position = Self::position_of(&self.audio, &self.video, playback, cue_id, now);
            out.push(EngineEvent::Position {
                cue_id,
                position: position.min(unit),
            });

            // Diashow: Auto-Advance + Frame-Update.
            if matches!(playback.kind, PlaybackKind::SlideShow(_)) {
                let mut state = match &mut playback.kind {
                    PlaybackKind::SlideShow(state) => state.clone(),
                    _ => unreachable!(),
                };
                Self::tick_slideshow(&self.events, &self.frames, cue_id, &mut state, now);
                if let PlaybackKind::SlideShow(s) = &mut playback.kind {
                    *s = state;
                }
                Self::mark_stage_dirty(&mut self.compositor, playback);
            }

            // Einheiten-Ende: Position ≥ Unit ODER Backend-EOF.
            let eof = Self::backend_eof(&self.audio, &self.video, playback, cue_id);
            let media_kind = matches!(playback.kind, PlaybackKind::Audio | PlaybackKind::Video);
            if position >= unit || (eof && media_kind) {
                if !playback.plan.is_empty() {
                    playback.plan_index += 1;
                    playback.iterations_done += 1;
                    if playback.plan_index >= playback.plan.len() {
                        playback.finished_at = Some(now);
                        finished_now.push(cue_id);
                        if playback.post_wait.is_zero() {
                            post_wait_elapsed.push(cue_id);
                        }
                    } else {
                        playback.restart_unit(now);
                        restarts.push(cue_id);
                        out.push(EngineEvent::LoopIteration {
                            cue_id,
                            iteration: playback.iterations_done,
                        });
                    }
                } else {
                    let total = playback.loop_mode.total_iterations();
                    if playback.loop_mode.is_infinite() || playback.iterations_done + 1 < total {
                        playback.iterations_done += 1;
                        playback.restart_unit(now);
                        restarts.push(cue_id);
                        out.push(EngineEvent::LoopIteration {
                            cue_id,
                            iteration: playback.iterations_done,
                        });
                    } else {
                        playback.finished_at = Some(now);
                        finished_now.push(cue_id);
                        if playback.post_wait.is_zero() {
                            post_wait_elapsed.push(cue_id);
                        }
                    }
                }
            }
        }

        // Backend-Startfehler: Playback entfernen + Error-Event (nach dem Loop,
        // damit keine Borrows von `active` mehr leben).
        for (cue_id, message) in start_errors {
            self.active.remove(&cue_id);
            self.chasing.remove(&cue_id);
            out.push(EngineEvent::Error {
                cue_id: Some(cue_id),
                message,
            });
        }

        // Loop-Neustarts im Backend (nach dem Iterations-Loop, Borrows frei).
        for cue_id in restarts {
            if let Some(playback) = self.active.get(&cue_id) {
                Self::restart_backend(&mut self.audio, &mut self.video, cue_id, playback);
                Self::mark_stage_dirty(&mut self.compositor, playback);
            }
        }

        // WebView-Instanzen aktivieren: echtes Subprocess-Backend, wenn das
        // Binary verfügbar ist, sonst virtueller Fallback.
        for (cue_id, source, stage, layer) in html_starts {
            let webview = self.renderer_bin.as_deref().and_then(
                |bin| match WebviewInstance::spawn(bin, cue_id, &source) {
                    Ok(instance) => Some(instance),
                    Err(err) => {
                        warn!(%cue_id, %err, "webview spawn fehlgeschlagen – virtueller Fallback");
                        None
                    }
                },
            );
            self.html.insert(
                cue_id,
                ActiveHtml {
                    source,
                    stage,
                    layer,
                    started_at: now,
                    webview,
                    // Snapshots erst nach bestätigtem Load anfordern.
                    next_snapshot_at: None,
                    snapshot_failures: 0,
                    snapshot_pending_since: None,
                    loaded: false,
                    pending_data: self.pending_html_data.remove(&cue_id).unwrap_or_default(),
                    closing_since: None,
                },
            );
            out.push(EngineEvent::HtmlLoaded { cue_id });
        }
        self.video.tick_seeks();
        // WebView-Snapshots anfordern (Compositing) und Events pollen.
        // Fehler werden als Engine-Events gemeldet; Erfolgs-Bestätigungen
        // werden bereits beim Command emittiert.
        let mut closing_done: Vec<Uuid> = Vec::new();
        for (cue_id, html) in &mut self.html {
            let Some(webview) = &mut html.webview else {
                continue;
            };
            if html
                .closing_since
                .is_some_and(|since| now.saturating_sub(since) >= HTML_OUT_TIMEOUT)
            {
                closing_done.push(*cue_id);
            }
            // Fälligen Snapshot anfordern (nach Load). Bei Fehlern kein
            // dauerhaftes Stoppen, sondern Backoff – sonst friert das HTML
            // nach ein paar Snapshot-Fehlern ein (z. B. Offscreen-Fenster).
            let snapshot_free = html
                .snapshot_pending_since
                .is_none_or(|since| now.saturating_sub(since) >= WEBVIEW_SNAPSHOT_TIMEOUT);
            if let Some(due) = html.next_snapshot_at.filter(|_| snapshot_free) {
                if now >= due {
                    html.snapshot_pending_since = Some(now);
                    let (w, h) = match &html.stage {
                        Some(stage) => canvas_size(&self.stages, stage),
                        None => WEBVIEW_SNAPSHOT_DEFAULT,
                    };
                    if let Err(err) = webview.request_snapshot(w, h) {
                        warn!(%cue_id, %err, "webview snapshot request fehlgeschlagen");
                        html.snapshot_failures += 1;
                    }
                    let interval = if html.snapshot_failures > 0 {
                        WEBVIEW_SNAPSHOT_RETRY_INTERVAL
                    } else {
                        WEBVIEW_SNAPSHOT_INTERVAL
                    };
                    html.next_snapshot_at = Some(now + interval);
                }
            }
            while let Some(event) = webview.poll_event() {
                match event {
                    super::webview_backend::WebviewEvent::Frame {
                        width,
                        height,
                        rgba,
                    } => {
                        html.snapshot_failures = 0;
                        html.snapshot_pending_since = None;
                        let frame = Arc::new(VideoFrame::new(width, height, rgba));
                        self.frames.lock().unwrap().insert(*cue_id, frame);
                    }
                    super::webview_backend::WebviewEvent::Loaded => {
                        // Erst nach bestätigtem Load Snapshots anfordern.
                        if html.next_snapshot_at.is_none() {
                            html.next_snapshot_at = Some(now);
                        }
                        // Gepufferte Daten zustellen, dann Trigger „in".
                        if !html.loaded {
                            html.loaded = true;
                            for payload in std::mem::take(&mut html.pending_data) {
                                let _ = webview.push_json(&payload);
                            }
                            let _ = webview.trigger("in", serde_json::Value::Null);
                        }
                    }
                    super::webview_backend::WebviewEvent::TriggerDone(name) => {
                        if name == "out" && html.closing_since.is_some() {
                            closing_done.push(*cue_id);
                        }
                    }
                    super::webview_backend::WebviewEvent::Error(message) => {
                        if message.starts_with("snapshot:") {
                            html.snapshot_pending_since = None;
                            // Snapshot-Fehler: einmal melden, dann Backoff (Retry).
                            html.snapshot_failures += 1;
                            if html.snapshot_failures == 1 {
                                out.push(EngineEvent::Error {
                                    cue_id: Some(*cue_id),
                                    message,
                                });
                            }
                        } else {
                            out.push(EngineEvent::Error {
                                cue_id: Some(*cue_id),
                                message,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        // Out-Animation fertig (oder Timeout): jetzt wirklich stoppen.
        for cue_id in closing_done {
            let _ = self.handle(EngineCommand::Stop { cue_id });
        }
        // Script-Cues: WebView-Events pollen – nach Loaded/Fehler ist das
        // Script fertig → Finished + PostWaitElapsed (Cue endet).
        let script_ids: Vec<Uuid> = self.scripts.keys().copied().collect();
        for cue_id in script_ids {
            let mut finish_now = false;
            let mut error: Option<String> = None;
            if let Some(state) = self.scripts.get_mut(&cue_id) {
                while let Some(event) = state.webview.poll_event() {
                    match event {
                        // Review E12: Loaded startet nur die Settle-Frist –
                        // asynchrones JS läuft in der Frist weiter, die Instanz
                        // wird erst danach per Drop beendet.
                        super::webview_backend::WebviewEvent::Loaded => {
                            if state.finish_at.is_none() {
                                state.finish_at = Some(now + script_settle());
                            }
                        }
                        super::webview_backend::WebviewEvent::Done => finish_now = true,
                        // Späte JS-Events (z. B. asynchrones Script-JS) werden
                        // an die UI durchgereicht, solange die Instanz lebt (E12).
                        super::webview_backend::WebviewEvent::JsExecuted => {
                            out.push(EngineEvent::JsExecuted {
                                cue_id,
                                script: String::new(),
                                at: now,
                            });
                        }
                        super::webview_backend::WebviewEvent::Error(msg) => {
                            finish_now = true;
                            error = Some(msg);
                        }
                        _ => {}
                    }
                }
                if !finish_now {
                    if let Some(deadline) = state.finish_at {
                        if now >= deadline {
                            finish_now = true;
                        }
                    }
                }
            }
            if finish_now {
                self.scripts.remove(&cue_id);
                if let Some(message) = error {
                    out.push(EngineEvent::Error {
                        cue_id: Some(cue_id),
                        message: format!("script cue: {message}"),
                    });
                }
                out.push(EngineEvent::Finished { cue_id });
                out.push(EngineEvent::PostWaitElapsed { cue_id });
            }
        }
        // Natürliches Ende: Backends stoppen (Video-Bild + Audio-Voice).
        // Erst halten: Das Stoppen löscht das Bild. Ohne Halten wird die Stage neu
        // aufgebaut, das Bild verschwindet also wirklich.
        for cue_id in &finished_now {
            self.hold_frame_if_wanted(*cue_id);
            if let Some(stage) = self.active.get(cue_id).and_then(|p| p.stage.clone()) {
                self.compositor.mark_dirty(&stage);
            }
        }
        for cue_id in &finished_now {
            self.video.stop(*cue_id);
            if let Some(audio) = self.audio.as_mut() {
                audio.stop_voice(*cue_id);
            }
            self.audio_voices.lock().unwrap().remove(cue_id);
        }
        for cue_id in finished_now {
            out.push(EngineEvent::Finished { cue_id });
        }
        for cue_id in post_wait_elapsed {
            out.push(EngineEvent::PostWaitElapsed { cue_id });
            self.active.remove(&cue_id);
            self.chasing.remove(&cue_id);
            self.html.remove(&cue_id);
            self.audio_voices.lock().unwrap().remove(&cue_id);
        }

        self.tick_deferred_stops(now);
        self.release_covered_holds();
        self.pause_chasing_when_timecode_stops(now);

        // Compositor: geänderte Stages neu aufbauen.
        self.composite_dirty_stages();
        // Routen neu rendern, deren Stage-Canvas sich geaendert hat.
        self.render_dirty_routes();

        // Decklink: aktuelles Stage-Frame im konfigurierten Takt pushen.
        self.tick_decklink(now);

        for event in out {
            self.emit(event);
        }
    }

    fn shutdown(&mut self) {
        // Alle Backends sauber herunterfahren (keine hängenden Prozesse).
        self.scripts.clear();
        self.decklink_key = None;
        let ids: Vec<Uuid> = self.active.keys().copied().collect();
        if let Some(audio) = self.audio.as_mut() {
            for cue_id in &ids {
                audio.stop_voice(*cue_id);
            }
        }
        for cue_id in &ids {
            self.video.stop(*cue_id);
        }
        debug!("real engine shut down");
    }
}

/// Startet die RealMediaEngine in einem eigenen Thread (wie Mock) und gibt
/// das Handle zurück.
pub fn spawn_real_engine() -> super::EngineHandle {
    spawn_real_engine_with_join().0
}

/// Wie [`spawn_real_engine`], liefert zusätzlich das [`std::thread::JoinHandle`].
pub fn spawn_real_engine_with_join() -> (super::EngineHandle, std::thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<EngineCommand>();
    let (evt_tx, evt_rx) = crossbeam_channel::unbounded::<EngineEvent>();
    let frames = new_frame_store();
    let opacities = new_opacity_store();
    let stage_frames = new_stage_frame_store();
    let route_frames = new_route_frame_store();

    let frames_e = frames.clone();
    let opacities_e = opacities.clone();
    let stage_frames_e = stage_frames.clone();
    let route_frames_e = route_frames.clone();
    let audio_voices = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let audio_voices_e = audio_voices.clone();
    let decklink_stats = std::sync::Arc::new(std::sync::Mutex::new(
        super::decklink::DecklinkStats::default(),
    ));
    let decklink_stats_e = decklink_stats.clone();

    let join = std::thread::Builder::new()
        .name("real-playback-engine".to_string())
        .spawn(move || {
            let mut engine = RealMediaEngine::new(
                frames_e,
                opacities_e,
                stage_frames_e,
                route_frames_e,
                evt_tx,
            )
            .with_audio_voice_store(audio_voices_e)
            .with_decklink_stats(decklink_stats_e);
            // Decklink-Hardware-Ausgabe starten, falls konfiguriert
            // (KUHPLAY_DECKLINK_* – siehe DecklinkConfig::from_env).
            engine.start_decklink_from_env();
            debug!(engine = engine.name(), "engine loop started");
            let ticker = crossbeam_channel::tick(TICK);
            loop {
                crossbeam_channel::select! {
                    recv(cmd_rx) -> msg => match msg {
                        Ok(command) => {
                            if let Err(err) = engine.handle(command) {
                                warn!(%err, "engine command failed");
                            }
                        }
                        Err(_) => break,
                    },
                    recv(ticker) -> _ => engine.tick(),
                }
            }
            engine.shutdown();
            debug!("engine loop terminated");
        })
        .expect("real playback engine thread must spawn");

    (
        super::EngineHandle {
            decklink_stats,
            commands: cmd_tx,
            events: evt_rx,
            frames,
            opacities,
            stage_frames,
            route_frames,
            audio_voices,
        },
        join,
    )
}
