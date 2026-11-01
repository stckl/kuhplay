//! Zentrale Anwendungsstruktur ([`eframe::App`]) – Workspace-Layout.
//!
//! Verbindet Domain ([`CartManager`]/[`CueList`](crate::core::CueList)) und
//! Engine ([`EngineHandle`]) mit der `egui`-Oberfläche. Alle
//! Zustandsänderungen laufen über Methoden dieser Struktur; die Panels
//! selbst sind in eigene Module ausgelagert.
//!
//! ## Layout („The Workspace")
//!
//! Verbindlich beschrieben in **`docs/UI.md`** (Layout, Farben,
//! Typografie, Komponenten). Gezeichnet wird es in
//! [`ShowApp::draw_workspace`]:
//!
//! ```text
//! Menüleiste
//! Masthead:  [GO]  STANDBY / Q# • Name / Notes        [⏸][▶][↺][Panic]
//! Toolbox | Cue-Liste                                  | Sidebar
//! Inspector (volle Breite)
//! Footer (unterste Zeile)
//! ```
//!
//! * Toolbox = einzige Cue-Typ-Button-Quelle (Masthead-Toolbar entfällt).
//! * Screens/Stages: eigenes Fenster „Workspace Settings" (Seite „Video").
//! * UI-Layout (Panel-Sichtbarkeit, Tabs) persistiert via `eframe::Storage`
//!   ([`super::state::UiState`]).
//!
//! **Wichtig:** Panel-Reihenfolge in egui ist `top → bottom → side →
//! central` – das CentralPanel muss zuletzt hinzugefügt werden und claimt
//! den Restplatz. Bei Bottom-Panels ist das ZUERST hinzugefügte das
//! UNTERSTE (Footer vor Inspector).
//!
//! ## Erweiterung
//!
//! * **Persistenz:** Show als JSON speichern/laden (Datei-Dialog via `rfd`,
//!   siehe `ShowDocument`).
//! * **Mehrere Engine-Instanzen:** Audio- und Video-Engine parallel betreiben.
//! * **Netzwerk-Remote:** Engine-Commands zusätzlich per OSC/WebSocket an
//!   andere Rechner (Show-Caller) weiterleiten.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, RichText};
use uuid::Uuid;

use crate::core::{
    cue_type_for_file, default_routes_for_displays, CartManager, Cue, CueStatus, CueType,
    DeviceRef, DisplayRefInfo, OutputRoute, Region, RouteManager, ScreenManager, ShowDocument,
    ShowResult, Stage, StageGeometry, StageManager,
};
use crate::engine::waveform::{Waveform, WaveformCache, WAVEFORM_BUCKETS};
use crate::engine::{EngineCommand, EngineEvent, EngineHandle};
use crate::remote::{
    CartInfo, CueInfo, RemoteCommand, RemoteConfig, RemoteEvent, RemoteStatus, RemoteWorkspace,
    RunningCue, StandbyCue,
};

use super::shortcuts::ShortcutAction;
use super::{
    audition_window, cue_table, footer, inspector, masthead, menu_bar, shortcuts, sidebar,
    stage_view, status_window, target_search, theme, toolbox,
};

/// Tab-Auswahl der Sidebar („Lists & Carts" / „Active Cues";
/// Screens liegen in den Workspace Settings → Video).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SidebarTab {
    /// Cue-Listen & Carts.
    ListsCarts,
    /// Laufende Cues mit Einzel-Steuerung.
    ActiveCues,
}

/// Tab-Auswahl des Inspectors (typabhängige Tabs, TODO.md Phase 4).
///
/// Serde per Varianten-Name – neue Varianten sind abwärtskompatibel
/// (alte gespeicherte Zustände deserialisieren weiter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InspectorTab {
    /// Grundparameter (Basics-Reihenfolge, zweispaltig).
    Basics,
    /// Trigger (Timecode-In / Chasing).
    Triggers,
    /// Time & Loops (Loop-Modus, Slices, Waveform) – Audio/Video.
    TimeLoops,
    /// Geometry (Stage/Layer/Surface) – Video/HTML5/Diashow.
    Geometry,
    /// Audio Levels (Volume/Patch) – Audio/Video.
    AudioLevels,
    /// Curve (Fade-Parameter/-Kurve/Start/Ziel) – Fade-Cues.
    Curve,
    /// Source (HTML-Quelle + JS-Test) – HTML5-Cues.
    Source,
    /// Slides (Slide-Liste + Transitions) – Diashow-Cues.
    Slides,
    /// Data (Ziel-HTML + JSON-Payload) – HTML-Data-Cues.
    Data,
    /// Text (Text-Inhalt/Schrift/Farbe/Ausrichtung) – Text-Cues.
    Text,
    /// Network (Protokoll/Host/Port/Nachricht) – Network-Cues.
    Network,
    /// MIDI (Typ/Kanal/Note/Velocity) – MIDI-Cues.
    Midi,
    /// Light (Universe/Kanal/Werte/Host) – Light-Cues (Art-Net/sACN DMX).
    Light,
    /// Companion (Host/Port/Page/Row/Column/Aktion) – Companion-Cues.
    Companion,
    /// Timecode (Trigger-Zeitcode + FPS) – Timecode-Cues.
    Timecode,
    /// Script (JavaScript-Quelltext) – Script-Cues.
    ScriptTab,
}

impl InspectorTab {
    /// Anzeigename des Tabs (Begriffe, TODO.md Phase 4).
    pub fn label(self) -> &'static str {
        match self {
            InspectorTab::Basics => "Basics",
            InspectorTab::Triggers => "Triggers",
            InspectorTab::TimeLoops => "Time & Loops",
            InspectorTab::Geometry => "Geometry",
            InspectorTab::AudioLevels => "Audio Levels",
            InspectorTab::Curve => "Curve",
            InspectorTab::Source => "Source",
            InspectorTab::Slides => "Slides",
            InspectorTab::Data => "Data",
            InspectorTab::Text => "Text",
            InspectorTab::Network => "Network",
            InspectorTab::Midi => "MIDI",
            InspectorTab::Light => "Light",
            InspectorTab::Companion => "Companion",
            InspectorTab::Timecode => "Timecode",
            InspectorTab::ScriptTab => "Script",
        }
    }
}

/// Hauptanwendung: Show-Zustand (Carts), Engine-Verbindung und
/// Panel-Sichtbarkeit (Workspace).
pub struct ShowApp {
    /// Cue-Listen & Carts (Domain-Modell).
    pub carts: CartManager,
    /// Playback-Engine (non-blocking).
    pub engine: EngineHandle,
    /// Anzeige-Name der Engine („real"/„mock") – z. B. Workspace-Status-Fenster.
    pub engine_label: String,
    /// Name der Show (Fenster-Titel + Persistenz).
    pub show_name: String,
    /// Show-Mode: Editing deaktiviert (Sicherheitsfunktion).
    pub show_mode: bool,
    /// Audition-Mode: Video-Cues spielen ohne Stage-Ziel (Audio normal).
    pub audition_mode: bool,
    /// Reiner UI-Zustand (Panel-Sichtbarkeit, Tabs, Fenster) – serde-fähig
    /// für die Layout-Persistenz via `eframe::Storage` (TODO.md Phase 0b).
    pub ui: super::state::UiState,
    /// Workspace-Templates („Workspace Templates"): benannte
    /// Layout-Vorlagen, Persistenz über eigenen Storage-Key.
    pub workspace_templates: Vec<super::state::WorkspaceTemplate>,
    /// Eingabe-Puffer für den Namen eines neuen Workspace-Templates
    /// (Settings → General). Reiner Laufzeit-Zustand.
    pub template_name_buffer: String,
    /// Aktuell geöffnete Datei (für „Save Show" ohne Dialog).
    current_file: Option<PathBuf>,
    /// Space-GO wurde gedrückt (Ausführung nach den Panels, damit Hotkeys
    /// nicht mit fokussierten Buttons kollidieren).
    pub(crate) pending_go: bool,
    /// Zuletzt aufgetretener Fehler (UI-Anzeige).
    last_error: Option<String>,
    /// Widget-IDs aller Textfelder des aktuellen Frames. Für das
    /// Hotkey-Gate „Textfeld fokussiert?“: egui 0.30 unterscheidet
    /// Textfelder nicht von Slidern (beide fordern Fokus beim Klick),
    /// daher registrieren die Zeichenfunktionen ihre Textfelder hier.
    pub text_field_ids: HashSet<egui::Id>,
    /// Textfeld-IDs des vorherigen Frames (die Hotkeys lesen diese – ein
    /// Frame Verzögerung ist unkritisch, da Felder vor dem Anklicken
    /// bereits existieren müssen).
    text_field_ids_prev: HashSet<egui::Id>,
    /// Zwischengespeicherter Gate-Wert „Textfeld fokussiert" (aktualisiert
    /// am Ende jedes Frames via [`ShowApp::refresh_text_field_gate`]).
    /// Cache statt Live-Abfrage, weil egui den Fokus bei Esc bereits in
    /// `begin_pass` löscht – bevor die Hotkeys laufen.
    text_field_active: bool,
    /// Startzeitpunkte laufender Cues (für elapsed-Anzeige in der Sidebar).
    started_at: HashMap<Uuid, Instant>,
    /// Medien-Dauer pro Cue (aus `EngineEvent::Started`), Basis fürs Spulen.
    media_durations: HashMap<Uuid, Duration>,
    /// Zwischenablage für „Copy/Paste Cue Properties" (JSON der Cue-Felder).
    properties_clipboard: Option<serde_json::Value>,
    /// Akkumulierte aktive Zeit pausierter Cues.
    banked: HashMap<Uuid, Duration>,
    /// Startzeitpunkte von Cues in Pre-Wait (Countdown in der Pre-Wait-
    /// Spalte, TODO.md Phase 2).
    prewait_started: HashMap<Uuid, Instant>,
    /// Letzter gesendeter Scrub-Seek pro Cue (Zeitstempel, Fraktion) –
    /// Throttle gegen ffmpeg-Neustart-Spam beim Ziehen im Active-Cues-Balken.
    scrub_last: HashMap<Uuid, (Instant, f32)>,
    /// Stage-Manager: alle Video-Stages mit Screen-Mapping & Geometrie.
    pub stages: StageManager,
    /// Screen-Manager: physische Ausgabebereiche („Screens") –
    /// frei platzierbar (Teile eines Displays, mehrere Displays, mehrere
    /// Screens pro Display), mit Drehung und Testbild.
    pub screens: ScreenManager,
    /// Output Routes ("Video -> Output Routing"): logische Ausgaenge
    /// mit eigener Aufloesung auf Displays oder Blackmagic-Geraeten.
    pub routes: RouteManager,
    /// Offen gehaltene Stage-Vorschau-Fenster („Stage Window").
    pub stage_windows: HashSet<String>,
    /// Per Esc von Hand geschlossene Device-Ausgaben (bis zum naechsten
    /// Moduswechsel; Display-ID).
    pub(super) closed_device_outputs:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<u32>>>,
    /// Erkannte Displays (für Screen-Mapping in der UI).
    pub displays: Vec<display_info::DisplayInfo>,
    /// Stage-Editor (eigenes Fenster): Stage-ID, oder `None` = zu.
    pub stage_editor_open: Option<Uuid>,
    /// Route-Editor (eigenes Fenster): Route-ID, oder `None` = zu.
    pub route_editor_open: Option<Uuid>,
    /// Multi-Output-Assistent („New Stage → Multi-Output Stage…“).
    pub multi_output_wizard: Option<MultiOutputWizard>,
    /// Optionaler Auto-Demo-Modus (automatische GO-Abfolge für Screen-Tests).
    autodemo: Option<AutodemoState>,
    /// Remote-Server (Web-Remote + Companion), falls aktiviert.
    remote: Option<RemoteServers>,
    /// Laufender Ctrl-Klick, der als Rechtsklick gilt (macOS).
    ctrl_click_active: bool,
    /// Zuletzt an die Web-Remote gesendete Show (nur bei Änderung neu senden).
    last_workspace_json: String,
    /// Aktuelle Video-Ausgabe.
    pub output_mode: OutputMode,
    /// Von Hand geschlossene Vorschau-Fenster (bis zum nächsten Moduswechsel).
    pub(super) closed_previews: std::sync::Arc<std::sync::Mutex<HashSet<Uuid>>>,
    /// macOS-Menüleiste; `None` = egui-Menüleiste (auch in Headless-Tests).
    #[cfg(target_os = "macos")]
    pub(super) native_menu: Option<super::native_menu::NativeMenu>,
    /// Aktives Audio-Ausgabegerät (`None` = System-Standard).
    audio_output: Option<String>,
    /// Zuordnung der Audio-Kanäle zu Geräte-Ausgängen.
    pub audio_patch: crate::core::AudioPatch,
    /// Geräteliste der Audio-Seite; `None` = beim nächsten Öffnen neu abfragen.
    pub(super) audio_devices: Option<AudioDeviceList>,
    /// Remote-Konfiguration aus der Umgebung (Anzeige in den Settings).
    pub(super) remote_config: RemoteConfig,
    /// Seit dem letzten Push relevante Änderung → Remote-Status broadcasten.
    remote_dirty: bool,
    /// Zeitpunkt des letzten Remote-Status-Push (sonst periodisch 1×/s).
    last_status_push: Instant,
    /// Broken-Cue-Cache (inkl. Gruppen-Kinder für das ✕ in der Cue-Liste;
    /// der Footer zählt nur Top-Level-Cues) – siehe
    /// [`problems::cue_problems`](super::problems::cue_problems).
    problems: Vec<BrokenCue>,
    /// Zeitpunkt der letzten Broken-Neuberechnung (`None` = noch nie;
    /// Neuberechnung max. alle 2 s, nicht jeden Frame das Dateisystem).
    problems_at: Option<Instant>,
    /// Waveform-Peaks pro Datei (Cache für die Inspector-Vorschau).
    pub waveforms: WaveformCache,
}

/// Assistent für eine Multi-Output-Stage: Raster (Zeilen × Spalten) aus
/// Projektoren, Auflösung je Projektor und Überlappung (getrennt horizontal /
/// vertikal, Pixel).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiOutputWizard {
    /// Spalten (Projektoren nebeneinander).
    pub cols: u32,
    /// Zeilen (Projektoren untereinander).
    pub rows: u32,
    /// Auflösung je Projektor (Pixel).
    pub width: u32,
    /// Höhe je Projektor (Pixel).
    pub height: u32,
    /// Horizontale Überlappung benachbarter Projektoren (Pixel).
    pub overlap_x: u32,
    /// Vertikale Überlappung benachbarter Projektoren (Pixel).
    pub overlap_y: u32,
}

impl Default for MultiOutputWizard {
    fn default() -> Self {
        Self {
            cols: 2,
            rows: 1,
            width: 1920,
            height: 1080,
            overlap_x: 200,
            overlap_y: 200,
        }
    }
}

/// Zustand des optionalen Auto-Demo-Modus (für automatische Screen-Tests).
struct AutodemoState {
    /// Zeitpunkt des nächsten GO.
    next_go: Instant,
    /// Anzahl bereits ausgelöster GOs.
    count: u32,
}

/// Ein kaputter Cue (Eintrag des Broken-Cache, TODO.md Phase 2).
#[derive(Debug, Clone)]
struct BrokenCue {
    /// Cart-Index.
    cart: usize,
    /// Index in der Liste (bei Gruppen-Kindern: Index der Gruppe).
    index: usize,
    /// Cue-ID.
    id: Uuid,
    /// `true` = Gruppen-Kind (nur Top-Level-Cues zählen für den Footer).
    child: bool,
    /// Broken-Gründe (Tooltip des ✕ in der Cue-Liste).
    problems: Vec<String>,
}

/// Status-Werte, die als „laufend" gelten (für den Remote-Status).
fn is_running_status(status: CueStatus) -> bool {
    matches!(
        status,
        CueStatus::PreWait | CueStatus::Playing | CueStatus::Paused | CueStatus::PostWait
    )
}

/// Gestartete Remote-Server (Web-Remote HTTP/WS + Companion TCP).
///
/// Hält den tokio-Runtime, auf dem die Server laufen (muss leben, solange
/// die App läuft), plus die Kanäle: eingehende Commands (pro Server) und
/// ausgehende Event-Broadcasts (pro Server, da beide eigene
/// `broadcast::Sender` besitzen).
pub struct RemoteServers {
    /// tokio-Runtime der Server (Worker-Threads laufen hier).
    _runtime: tokio::runtime::Runtime,
    /// Commands vom Web-Remote (HTTP/WS).
    web_auth: crate::remote::web::WebAuth,
    web_commands: Option<tokio::sync::mpsc::Receiver<RemoteCommand>>,
    /// Commands von Companion (TCP).
    companion_commands: Option<tokio::sync::mpsc::Receiver<RemoteCommand>>,
    /// Commands von OSC (UDP).
    osc_commands: Option<tokio::sync::mpsc::Receiver<RemoteCommand>>,
    /// Commands von MIDI (Controller-Input).
    /// Laufender MIDI-Start im Hintergrund (`None`: fertig oder deaktiviert).
    midi_start: Option<std::sync::mpsc::Receiver<MidiStartResult>>,
    midi_commands: Option<tokio::sync::mpsc::Receiver<RemoteCommand>>,
    /// Hält die MIDI-Verbindung offen (Drop = disconnect).
    _midi_guard: Option<crate::remote::midi::MidiInputGuard>,
    /// CoreMIDI hat den Client abgelehnt (nur ein Neustart hilft).
    midi_init_failed: bool,
    /// Event-Broadcast an Web-Remote-Clients.
    web_events: Option<tokio::sync::broadcast::Sender<RemoteEvent>>,
    /// Event-Broadcast an Companion-Clients.
    companion_events: Option<tokio::sync::broadcast::Sender<RemoteEvent>>,
    /// Event-Broadcast an OSC-Out.
    osc_events: Option<tokio::sync::broadcast::Sender<RemoteEvent>>,
}

/// Prüft einen Show-Pfad aus der Remote: nur `.kuhplay`-Dateien, damit sich über
/// den Browser keine beliebigen Dateien lesen oder überschreiben lassen.
fn remote_show_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path.trim());
    if path.extension().and_then(|e| e.to_str()) == Some("kuhplay") && path.is_absolute() {
        Ok(path)
    } else {
        Err("Use a full path ending in .kuhplay.".to_string())
    }
}

/// Überträgt die Felder aus `changes` (JSON-Objekt) auf eine Kopie von `value`.
/// Felder in `protected` bleiben unverändert.
fn merge_json<T: serde::Serialize + serde::de::DeserializeOwned>(
    value: &T,
    changes: serde_json::Value,
    protected: &[&str],
) -> Result<T, String> {
    let serde_json::Value::Object(changes) = changes else {
        return Err("Changes must be a JSON object.".to_string());
    };
    let mut merged = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let object = merged.as_object_mut().ok_or("Not a JSON object.")?;
    for (key, change) in changes {
        if !protected.contains(&key.as_str()) {
            object.insert(key, change);
        }
    }
    serde_json::from_value(merged).map_err(|e| format!("Invalid value: {e}"))
}

/// Wohin Stages ausgegeben werden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Keine automatische Ausgabe; einzelne Stages lassen sich von Hand öffnen.
    #[default]
    Off,
    /// Ein Vorschau-Fenster pro Screen (Arbeit an einem einzelnen Display).
    Preview,
    /// Vollbild auf den Displays.
    Displays,
}

/// Ausgabegeräte für die Audio-Seite der Workspace Settings.
pub(super) struct AudioDeviceList {
    pub(super) devices: Vec<crate::engine::audio_backend::OutputDevice>,
    pub(super) default_name: Option<String>,
}

/// Ergebnis des MIDI-Starts im Hintergrund-Thread.
type MidiStartResult = Result<
    (
        tokio::sync::mpsc::Receiver<RemoteCommand>,
        crate::remote::midi::MidiInputGuard,
    ),
    crate::remote::midi::MidiError,
>;

impl RemoteServers {
    /// Startet die aktivierten Remote-Server. `None`, wenn deaktiviert oder
    /// der Runtime nicht gebaut werden konnte (App läuft dann ohne Remote).
    fn start(config: RemoteConfig) -> Option<Self> {
        if !config.web_enabled
            && !config.companion_enabled
            && !config.osc_enabled
            && !config.midi_enabled
        {
            return None;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .thread_name("kuhplay-remote")
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!(%e, "remote runtime konnte nicht erstellt werden");
                return None;
            }
        };
        // tokio::spawn in den Server-Startern braucht einen Runtime-Kontext.
        let _guard = runtime.enter();
        let web_auth = crate::remote::web::WebAuth::new(config.web_pin.clone());
        let (web_commands, web_events) = if config.web_enabled {
            let (rx, tx) =
                crate::remote::web::start_web_remote_with_auth(config.web_port, web_auth.clone());
            (Some(rx), Some(tx))
        } else {
            (None, None)
        };
        let (companion_commands, companion_events) = if config.companion_enabled {
            let (rx, tx) = crate::remote::companion::start_companion_server(config.companion_port);
            (Some(rx), Some(tx))
        } else {
            (None, None)
        };
        let (osc_commands, osc_events) = if config.osc_enabled {
            let (rx, tx) = crate::remote::osc::start_osc_server(
                config.osc_port,
                config.osc_out.clone(),
                &config.osc_prefix,
            );
            (Some(rx), Some(tx))
        } else {
            (None, None)
        };
        // MIDI läuft callback-basiert (kein Server, kein Runtime nötig);
        // Fehler (z. B. kein Device) sind nicht fatal – App läuft ohne MIDI.
        // MIDI im Hintergrund starten: CoreMIDI braucht fürs Anlegen des
        // Clients teils mehrere Sekunden, das darf den App-Start nicht aufhalten.
        // Das Ergebnis holt `poll_midi_start` ab.
        let midi_start = config.midi_enabled.then(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            let (device, map) = (config.midi_device.clone(), config.midi_map);
            std::thread::spawn(move || {
                let result = crate::remote::midi::start_midi_input(device.as_deref(), &map);
                // Empfänger weg (App zu): Ergebnis wird nicht mehr gebraucht.
                let _ = tx.send(result);
            });
            rx
        });
        tracing::info!(
            web = config.web_enabled,
            web_port = config.web_port,
            companion = config.companion_enabled,
            companion_port = config.companion_port,
            osc = config.osc_enabled,
            osc_port = config.osc_port,
            midi = config.midi_enabled,
            "remote server gestartet"
        );
        Some(Self {
            _runtime: runtime,
            web_auth,
            web_commands,
            companion_commands,
            osc_commands,
            midi_start,
            midi_commands: None,
            _midi_guard: None,
            midi_init_failed: false,
            web_events,
            companion_events,
            osc_events,
        })
    }
}

impl ShowApp {
    /// Erzeugt die App mit einer Demo-Show (Default-UI-Layout).
    pub fn new(engine: EngineHandle) -> Self {
        Self::new_inner(engine, None)
    }

    /// Erzeugt die App mit einer Demo-Show und lädt das persistierte
    /// UI-Layout aus dem `eframe::Storage` (fehlend/ungültig → Defaults).
    pub fn new_with_storage(engine: EngineHandle, storage: &dyn eframe::Storage) -> Self {
        Self::new_inner(engine, Some(storage))
    }

    /// Gemeinsamer Konstruktor (UI-Layout optional aus Storage laden).
    fn new_inner(engine: EngineHandle, storage: Option<&dyn eframe::Storage>) -> Self {
        let mut carts = CartManager::new();
        for cue in super::demo::demo_cues() {
            carts.active_mut().add(cue);
        }
        carts.select(None);
        let stages = StageManager::new();
        let autodemo = std::env::var_os("KUHPLAY_AUTODEMO")
            .is_some_and(|v| v == "1" || v == "true")
            .then(|| AutodemoState {
                next_go: Instant::now() + Duration::from_millis(1500),
                count: 0,
            });
        // Default-Screens: einer pro Display .
        let displays = stage_view::detect_displays();
        let screens = ScreenManager::from_displays(&displays);
        // UI-Layout aus Storage laden (fehlend/ungültig → Defaults).
        let ui = storage
            .and_then(|s| s.get_string(super::state::UI_STATE_STORAGE_KEY))
            .map(|json| super::state::UiState::from_json(&json))
            .unwrap_or_default();
        // Workspace-Templates aus Storage laden (fehlend/ungültig → leer).
        let workspace_templates = storage
            .and_then(|s| s.get_string(super::state::UI_TEMPLATES_STORAGE_KEY))
            .map(|json| super::state::templates_from_json(&json))
            .unwrap_or_default();
        let mut app = Self {
            carts,
            engine,
            engine_label: "real".to_string(),
            show_name: "Demo Show".to_string(),
            show_mode: false,
            audition_mode: false,
            ui,
            workspace_templates,
            template_name_buffer: String::new(),
            current_file: None,
            pending_go: false,
            last_error: None,
            text_field_ids: HashSet::new(),
            text_field_ids_prev: HashSet::new(),
            text_field_active: false,
            started_at: HashMap::new(),
            media_durations: HashMap::new(),
            properties_clipboard: None,
            banked: HashMap::new(),
            prewait_started: HashMap::new(),
            scrub_last: HashMap::new(),
            stages,
            screens,
            routes: RouteManager::new(),
            stage_windows: HashSet::new(),
            closed_device_outputs: Default::default(),
            displays,
            stage_editor_open: None,
            route_editor_open: None,
            multi_output_wizard: None,
            autodemo,
            remote: None,
            ctrl_click_active: false,
            last_workspace_json: String::new(),
            output_mode: OutputMode::Off,
            closed_previews: Default::default(),
            #[cfg(target_os = "macos")]
            native_menu: None,
            audio_output: None,
            audio_patch: crate::core::AudioPatch::default(),
            audio_devices: None,
            remote_config: RemoteConfig::default(),
            remote_dirty: false,
            last_status_push: Instant::now(),
            problems: Vec::new(),
            problems_at: None,
            waveforms: WaveformCache::new(),
        };
        // Alle Stages beim Start an die Engine melden (Stage-Konfiguration).
        for stage in app.stages.stages().to_vec() {
            app.configure_stage(stage);
        }
        // Auto-Demo: Stage 1 dem ersten Screen zuordnen (Vollbild-Ausgabe auf
        // dem ersten Display), Stage-Vorschau-Fenster öffnen und optional das
        // Testbild aktivieren (KUHPLAY_AUTOTESTPATTERN=1 – zum Screen-Setup).
        if app.autodemo.is_some() {
            app.stage_windows = app.stages.names().into_iter().collect();
            let autotest = std::env::var_os("KUHPLAY_AUTOTESTPATTERN")
                .is_some_and(|v| v == "1" || v == "true");
            if autotest {
                if let Some(route) = app.routes.routes().first().cloned() {
                    app.routes.find_mut(route.id).unwrap().test_pattern = true;
                    app.configure_routes();
                }
            }
        }
        // Default-Video-Setup: pro Display eine Route ("Display n")
        // und Stages mit einer Region ueber die ganze Flaeche; Stage 1 bleibt
        // fuer die Demo-Cues bestehen und liegt auf dem ersten Display.
        app.setup_default_routes();
        // Remote-Server starten (Web-Remote + Companion, konfigurierbar via
        // Umgebungsvariablen – siehe RemoteConfig::from_env).
        app.remote_config = RemoteConfig::from_env();
        app.remote = RemoteServers::start(app.remote_config.clone());
        app
    }

    /// Zugriff auf den Cart-Manager (nur lesend).
    pub fn carts(&self) -> &CartManager {
        &self.carts
    }

    /// Zugriff auf den Cart-Manager (mutabel).
    pub fn carts_mut(&mut self) -> &mut CartManager {
        &mut self.carts
    }

    // ------------------------------------------------------------------
    // Stages & Screens (Stage → Screen, Hard Edge, Area of Interest)
    // ------------------------------------------------------------------

    /// Übernimmt eine Stage-Änderung: im Manager speichern und an die
    /// Engine melden (`ConfigureStage`).
    pub fn configure_stage(&mut self, stage: Stage) {
        let name = stage.name.clone();
        if let Some(existing) = self.stages.find_mut(&name) {
            *existing = stage;
        } else {
            self.stages.add(stage);
        }
        if let Some(found) = self.stages.find(&name) {
            let _ = self.engine.send(EngineCommand::ConfigureStage {
                stage: found.clone(),
            });
        }
    }

    /// Baut das Default-Video-Setup einer neuen Show: pro Display
    /// eine Route ("Display n", Aufloesung in echten Pixeln) und pro
    /// weiterem Display eine Stage in dessen Pixelgroesse mit einer Region
    /// ueber die ganze Flaeche. Stage 1 bleibt fuer die Demo-Cues bestehen
    /// und liegt mit einer Full-Canvas-Region auf dem ersten Display.
    pub fn setup_default_routes(&mut self) {
        let displays: Vec<DisplayRefInfo> =
            self.displays.iter().map(DisplayRefInfo::from).collect();
        self.routes = default_routes_for_displays(&displays);
        let route_ids = self.routes.ids();
        let Some(first) = route_ids.first().copied() else {
            return;
        };
        if let Some(stage) = self.stages.find_mut("Stage 1") {
            let (w, h) = (
                stage.geometry.width.max(1.0),
                stage.geometry.height.max(1.0),
            );
            let mut region = Region::full_stage(w, h);
            region.route = Some(first);
            stage.regions = vec![region];
        }
        for (i, id) in route_ids.iter().enumerate().skip(1) {
            let Some(route) = self.routes.find(*id) else {
                continue;
            };
            let (w, h) = (route.width as f32, route.height as f32);
            let mut stage = Stage::new(format!("Stage {}", i + 1), StageGeometry::new(w, h));
            let mut region = Region::full_stage(w, h);
            region.route = Some(*id);
            stage.regions = vec![region];
            self.stages.add(stage);
        }
        self.resolve_device_refs();
        // Stages mit den neuen Regionen erneut an die Engine melden: Die
        // Startupschleife in new() laeuft VOR dieser Methode, die Engine
        // kennt die Regionen sonst nicht (Route-Bild bliebe schwarz).
        for stage in self.stages.stages().to_vec() {
            self.configure_stage(stage);
        }
        self.configure_routes();
    }

    /// Stellt sicher, dass eine Stage existiert (Default 1920×1080) und
    /// meldet sie an die Engine. Rückgabe: `true` bei Neuanlage.
    pub fn ensure_stage(&mut self, name: &str) -> bool {
        let created = self.stages.ensure(name);
        if created {
            if let Some(found) = self.stages.find(name) {
                let _ = self.engine.send(EngineCommand::ConfigureStage {
                    stage: found.clone(),
                });
            }
        }
        created
    }

    /// Legt eine neue leere Stage an (1920×1080, ohne Regionen).
    pub fn add_empty_stage(&mut self) {
        let mut number = self.stages.len() + 1;
        while self.stages.find(&format!("Stage {number}")).is_some() {
            number += 1;
        }
        self.configure_stage(Stage::new(
            format!("Stage {number}"),
            StageGeometry::new(1920.0, 1080.0),
        ));
    }

    /// Legt eine Stage mit Ausgabe an: neue Route (ohne Device) und eine
    /// Region über die ganze Fläche („New Stage with Output“).
    pub fn add_stage_with_output(&mut self) {
        let mut number = self.stages.len() + 1;
        while self.stages.find(&format!("Stage {number}")).is_some() {
            number += 1;
        }
        let mut route_number = self.routes.len() + 1;
        while self
            .routes
            .find_by_name(&format!("Route {route_number}"))
            .is_some()
        {
            route_number += 1;
        }
        let route = OutputRoute::new(format!("Route {route_number}"), 1920, 1080);
        let route_id = route.id;
        if self.routes.add(route).is_err() {
            return;
        }
        let mut stage = Stage::new(
            format!("Stage {number}"),
            StageGeometry::new(1920.0, 1080.0),
        );
        let mut region = Region::full_stage(1920.0, 1080.0);
        region.route = Some(route_id);
        stage.regions = vec![region];
        self.configure_stage(stage);
        self.configure_routes();
    }

    /// Multi-Output-Stage: `count` Routen (Projektoren) mit `width`×`height`,
    /// horizontal mit `overlap` Pixeln Überlappung; eine Stage, deren
    /// Regionen die Stage in Projektionsbreite aufteilen (Auto-Blend an).
    pub fn add_multi_output_stage(&mut self, wizard: &MultiOutputWizard) {
        let cols = wizard.cols.clamp(1, 6);
        let rows = wizard.rows.clamp(1, 6);
        let (w, h) = (wizard.width.clamp(1, 30000), wizard.height.clamp(1, 30000));
        let overlap_x = wizard.overlap_x.min(w.saturating_sub(1));
        let overlap_y = wizard.overlap_y.min(h.saturating_sub(1));
        let mut number = self.stages.len() + 1;
        while self.stages.find(&format!("Stage {number}")).is_some() {
            number += 1;
        }
        // Routen: „Route n“ (je Projektor, zeilenweise von links oben).
        let mut route_ids = Vec::new();
        for _ in 0..(cols * rows) {
            let mut route_number = self.routes.len() + 1;
            while self
                .routes
                .find_by_name(&format!("Route {route_number}"))
                .is_some()
            {
                route_number += 1;
            }
            let route = OutputRoute::new(format!("Route {route_number}"), w, h);
            let id = route.id;
            if self.routes.add(route).is_ok() {
                route_ids.push(id);
            }
        }
        // Stage: Raster-Gesamtgröße = cols·w − (cols−1)·overlap_x bzw. Zeilen.
        let stage_w = (cols as f32 * w as f32 - (cols as f32 - 1.0) * overlap_x as f32).max(1.0);
        let stage_h = (rows as f32 * h as f32 - (rows as f32 - 1.0) * overlap_y as f32).max(1.0);
        let mut stage = Stage::new(
            format!("Stage {number}"),
            StageGeometry::new(stage_w, stage_h),
        );
        // Regionen zeilenweise (row-major): oben links → rechts, dann nächste Zeile.
        for (index, id) in route_ids.iter().enumerate() {
            let col = (index as u32) % cols;
            let row = (index as u32) / cols;
            let x = col as f32 * (w as f32 - overlap_x as f32);
            let y = row as f32 * (h as f32 - overlap_y as f32);
            let mut region = Region::new(x, y, w as f32, h as f32);
            region.route = Some(*id);
            region.auto_blend = true;
            stage.regions.push(region);
        }
        self.configure_stage(stage);
        self.configure_routes();
    }

    /// Legt eine neue Route an (ohne Device). Rückgabe: ID.
    pub fn add_route(&mut self) -> Uuid {
        let mut route_number = self.routes.len() + 1;
        while self
            .routes
            .find_by_name(&format!("Route {route_number}"))
            .is_some()
        {
            route_number += 1;
        }
        let route = OutputRoute::new(format!("Route {route_number}"), 1920, 1080);
        let id = route.id;
        let _ = self.routes.add(route);
        self.configure_routes();
        id
    }

    /// Legt eine neue Route für ein Display an (Auflösung in echten
    /// Pixeln, Name „Display n“). Rückgabe: ID.
    pub fn add_route_for_display(&mut self, display: &DisplayRefInfo) -> Uuid {
        let (w, h) = display.pixel_size();
        let mut route = OutputRoute::new(display.name.clone(), w, h);
        route.device = Some(DeviceRef::Display {
            id: display.id,
            name: display.name.clone(),
            width: display.width,
            height: display.height,
        });
        let id = route.id;
        let _ = self.routes.add(route);
        self.configure_routes();
        id
    }

    /// Entfernt eine Route; Regionen, die sie nutzen, verlieren ihre
    /// Zuweisung ( betroffene Stages werden neu gemeldet).
    pub fn remove_route(&mut self, id: Uuid) {
        self.routes.remove(id);
        let mut affected: Vec<Stage> = Vec::new();
        for stage in self.stages.stages_mut() {
            let mut changed = false;
            for region in &mut stage.regions {
                if region.route == Some(id) {
                    region.route = None;
                    changed = true;
                }
            }
            if changed {
                affected.push(stage.clone());
            }
        }
        for stage in affected {
            self.configure_stage(stage);
        }
        self.configure_routes();
    }

    /// Meldet die Output Routes an die Engine (nach jeder Änderung an
    /// `self.routes` oder den Regionen einer Stage).
    pub fn configure_routes(&mut self) {
        let _ = self.engine.send(EngineCommand::ConfigureRoutes {
            routes: self.routes.clone(),
        });
    }

    /// Ordnet eine Stage einem Screen zu (`None` = nur In-App-Vorschau) und
    /// meldet die Änderung an die Engine (Legacy-Screen-Modell).
    pub fn assign_stage_to_screen(&mut self, stage_name: &str, screen: Option<Uuid>) {
        if let Some(stage) = self.stages.find_mut(stage_name) {
            stage.screen = screen;
            let stage = stage.clone();
            let _ = self.engine.send(EngineCommand::ConfigureStage { stage });
        }
    }

    /// Aktiviert/deaktiviert das Testbild eines Screens. Das Screen-Modell
    /// ist Legacy (Migration in Routen); das Route-Testbild haengt an der
    /// Route selbst (siehe `Self::configure_routes`).
    pub fn set_screen_test_pattern(&mut self, screen: Uuid, enabled: bool) {
        if let Some(s) = self.screens.find_mut(screen) {
            s.test_pattern = enabled;
        }
    }

    /// Schaltet das Testbild für **alle** Screens an/aus (LED-Wand-Setup).
    pub fn set_all_test_patterns(&mut self, enabled: bool) {
        let ids = self.screens.ids();
        for id in ids {
            self.set_screen_test_pattern(id, enabled);
        }
    }

    /// Benennt eine Stage um, auch in allen Cues. Leere oder vergebene Namen
    /// werden ignoriert.
    pub fn rename_stage(&mut self, old: &str, new: &str) {
        let new = new.trim();
        if new.is_empty() || self.stages.find(new).is_some() {
            return;
        }
        let Some(stage) = self.stages.find_mut(old) else {
            return;
        };
        stage.name = new.to_string();
        let stage = stage.clone();
        self.carts.rename_stage(old, new);
        if self.stage_windows.remove(old) {
            self.stage_windows.insert(new.to_string());
        }
        self.configure_stage(stage);
    }

    /// Löscht eine Stage, sofern kein Cue sie nutzt.
    pub fn remove_stage(&mut self, name: &str) -> bool {
        if self.carts.stage_in_use(name) {
            return false;
        }
        self.stage_windows.remove(name);
        self.stages.remove(name)
    }

    /// Beim App-Start: Stages ohne Screen auf den ersten Screen legen, damit
    /// GO ein Bild zeigt; mit nur einem Display Vorschau statt Vollbild, das
    /// Beim App-Start: Mit nur einem Display Vorschau statt Vollbild, das
    /// sonst die App selbst verdecken wuerde. Das Default-Routing (pro
    /// Display eine Route, Stage 1 mit Region auf dem ersten Display)
    /// liefert bereits ein Bild.
    pub fn apply_startup_output(&mut self) {
        if self.displays.len() <= 1 {
            self.set_output_mode(OutputMode::Preview);
        }
    }

    /// Wechselt die Video-Ausgabe. „Displays" öffnet alle zugeordneten Stages
    /// Wechselt die Video-Ausgabe: `Displays` oeffnet randlose Fenster auf
    /// allen belegten Displays, `Preview` ein Vorschau-Fenster pro Route.
    /// Ein Wechsel schliesst von Hand geschlossene Fenster wieder auf.
    pub fn set_output_mode(&mut self, mode: OutputMode) {
        self.output_mode = mode;
        self.closed_previews.lock().unwrap().clear();
        self.closed_device_outputs.lock().unwrap().clear();
    }

    /// Liest die verfügbaren Displays neu ein und löst die Display-Referenzen
    /// der Routen auf (ID weg → per Name+Auflösung neu finden). Bleibt etwas
    /// „(missing)“, bleibt die UUID-Verknüpfung Region→Route stabil — das
    /// restliche Video-Patching verschiebt sich nicht.
    pub fn refresh_displays(&mut self) {
        self.displays = stage_view::detect_displays();
        self.resolve_device_refs();
    }

    /// Löst alle Display-Referenzen der Routen gegen die erkannten Displays
    /// auf (siehe []) und
    /// meldet geänderte Routen an die Engine.
    pub fn resolve_device_refs(&mut self) {
        let displays: Vec<DisplayRefInfo> =
            self.displays.iter().map(DisplayRefInfo::from).collect();
        if self.routes.resolve_device_refs(&displays) > 0 {
            self.configure_routes();
        }
    }

    // ------------------------------------------------------------------
    // GO / Panic / Reset / Pause / Resume (Masthead + Sidebar)
    // ------------------------------------------------------------------

    /// GO: nächsten armed Cue des aktiven Carts triggern, inkl.
    /// Auto-Continue-Ketten („Cue Sequences").
    pub fn go_now(&mut self) {
        match self.carts.go_chained() {
            Some(outcome) => {
                let mut commands = outcome.commands;
                // Kill-Switch: alle anderen laufenden Media-Cues (Video/Diashow/
                // HTML) sofort stoppen, wenn der getriggerte Cue das verlangt.
                let kill = self
                    .carts
                    .active()
                    .cues()
                    .get(outcome.index)
                    .is_some_and(|c| c.kill_others);
                if kill {
                    let others: Vec<Uuid> = self
                        .carts
                        .active()
                        .cues()
                        .iter()
                        .filter(|c| {
                            c.id != self.carts.active().cues()[outcome.index].id
                                && c.status.is_active()
                                && matches!(
                                    c.cue_type,
                                    CueType::Video
                                        | CueType::SlideShow
                                        | CueType::Html
                                        | CueType::Text
                                )
                        })
                        .map(|c| c.id)
                        .collect();
                    for id in others {
                        commands.push(EngineCommand::Stop { cue_id: id });
                    }
                }
                // „Vorherige Cues beenden": alle laufenden Cues stoppen,
                // die in der Liste vor dem getriggerten Cue stehen.
                let stop_prev = self
                    .carts
                    .active()
                    .cues()
                    .get(outcome.index)
                    .is_some_and(|c| c.stop_previous);
                if stop_prev {
                    let prev: Vec<Uuid> = self
                        .carts
                        .active()
                        .cues()
                        .iter()
                        .enumerate()
                        .filter(|(i, c)| *i < outcome.index && c.status.is_active())
                        .map(|(_, c)| c.id)
                        .collect();
                    for id in prev {
                        if !commands.iter().any(
                            |cmd| matches!(cmd, EngineCommand::Stop { cue_id } if *cue_id == id),
                        ) {
                            commands.push(EngineCommand::Stop { cue_id: id });
                        }
                    }
                }
                self.dispatch(commands);
            }
            None => tracing::warn!("GO: no armed cue in standby"),
        }
        self.remote_dirty = true;
    }

    /// Panic: alles sofort stoppen (workspace-weit, Sidebar).
    pub fn panic(&mut self) {
        self.dispatch(vec![EngineCommand::StopAll]);
        self.carts.stop_all_running_all();
        self.clear_timing();
        self.remote_dirty = true;
    }

    /// Reset: alle Cues stoppen + Playhead aller Carts an den Anfang
    /// (Sidebar: Reset wirkt workspace-weit).
    pub fn reset_all(&mut self) {
        self.dispatch(vec![EngineCommand::StopAll]);
        self.carts.reset_all();
        self.clear_timing();
        self.remote_dirty = true;
    }

    /// Pause All: alle laufenden Cues pausieren (Sidebar).
    pub fn pause_all(&mut self) {
        let commands = self.carts.pause_all_running();
        self.dispatch(commands);
        self.remote_dirty = true;
    }

    /// Resume All: alle pausierten Cues fortsetzen (Sidebar).
    pub fn resume_all(&mut self) {
        let commands = self.carts.resume_all_running();
        self.dispatch(commands);
        self.remote_dirty = true;
    }

    // ------------------------------------------------------------------
    // Einzel-Cue-Steuerung (Sidebar „Active Cues")
    // ------------------------------------------------------------------

    /// Stoppt einen einzelnen laufenden Cue → Commands zurückgeben.
    pub fn stop_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        self.carts.stop_running(id)
    }

    /// Pausiert einen einzelnen laufenden Cue → Commands zurückgeben.
    pub fn pause_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        self.carts.pause_running(id)
    }

    /// Setzt einen einzelnen pausierten Cue fort → Commands zurückgeben.
    pub fn resume_running(&mut self, id: Uuid) -> Vec<EngineCommand> {
        self.carts.resume_running(id)
    }

    // ------------------------------------------------------------------
    // Edit-Aktionen (Masthead-Toolbar, Menü, Kontextmenü)
    // ------------------------------------------------------------------

    /// Fügt einen Cue des Typs nach der Selektion ein,
    /// nummeriert ihn automatisch (höchste vorhandene Nummer + 1, nie
    /// doppelt) und selektiert ihn.
    pub fn add_cue(&mut self, cue_type: CueType) {
        let list = self.carts.active_mut();
        let at = list.selected_insert_index();
        let n = list.next_free_number();
        let mut cue = Cue::new(n.to_string(), format!("New Cue {n}"), cue_type);
        // Wait-Cue: sinnvolle Vorgabe-Dauer statt 0.
        if cue_type == CueType::Wait {
            cue.duration = std::time::Duration::from_secs(1);
        }
        // Text-Cue: leerer Text-Inhalt als Vorgabe (sonst „kaputt").
        if cue_type == CueType::Text {
            cue.text_content = Some(crate::core::TextContent::default());
        }
        // Network-Cue: leere Nachricht mit Platzhalter-Ziel als Vorgabe.
        if cue_type == CueType::Network {
            cue.network = Some(crate::core::NetworkMessage::default());
        }
        // MIDI-Cue: Note On C4 als Vorgabe.
        if cue_type == CueType::Midi {
            cue.midi = Some(crate::core::MidiMessage::default());
        }
        // Light-Cue: Universe 0, Kanal 1 auf 255 (Broadcast) als Vorgabe.
        if cue_type == CueType::Light {
            cue.light = Some(crate::core::LightMessage::default());
        }
        // Companion-Cue: localhost:8000, Seite 1, Button 0/0 press.
        if cue_type == CueType::Companion {
            cue.companion = Some(crate::core::CompanionTrigger::default());
        }
        // Timecode-Cue: Trigger 01:00:00:00 @ 25 fps als Vorgabe.
        if cue_type == CueType::Timecode {
            cue.tc_trigger = Some(crate::core::Timecode::from_hmsf(
                1,
                0,
                0,
                0,
                crate::core::Fps::Fps25,
            ));
        }
        // Script-Cue: Hallo-Welt-Vorgabe (sonst „kaputt" wegen leerem Script).
        if cue_type == CueType::Script {
            cue.script = "console.log(\"hello from kuhplay\");".to_string();
        }
        list.insert(at, cue);
        list.select(Some(at));
    }

    /// Legt aus einer Datei (Drag & Drop aus dem Finder, TODO.md Phase 6)
    /// einen passenden Cue an: Endung → Cue-Typ (siehe [`cue_type_for_file`]),
    /// Datei als Target, Dateiname als Cue-Name; neuer Cue wird selektiert.
    /// Im Show-Mode oder bei unbekannter Endung → `None`.
    /// Rückgabe: ID des neuen Cues. Public für headless GUI-Tests.
    pub fn add_cue_from_file(&mut self, path: &Path) -> Option<Uuid> {
        if self.show_mode {
            return None;
        }
        let cue_type = cue_type_for_file(path)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let list = self.carts.active_mut();
        let n = list.next_free_number();
        let at = list.cues().len();
        let cue = Cue::new(n.to_string(), name, cue_type).with_target(path.to_string_lossy());
        let id = cue.id;
        list.insert(at, cue);
        list.select(Some(at));
        Some(id)
    }

    /// Löscht den Cue an `index` (aktiver Cart) – ein laufender Cue wird
    /// vorher gestoppt (Engine `Stop`), sonst würde er nach dem Löschen
    /// weiterlaufen (gelöschte Cues werden mitgestoppt).
    pub fn delete_cue_at(&mut self, index: usize) {
        if let Some(cue) = self.carts.active().cues().get(index) {
            let id = cue.id;
            let commands = self.carts.stop_running(id);
            self.dispatch(commands);
        }
        let _ = self.carts.active_mut().remove(index);
    }

    /// Löscht einen Cue per ID (Top-Level oder Group-Kind, TODO.md Phase 3
    /// Item 7) – ein laufender Cue wird vorher gestoppt.
    pub fn delete_cue_by_id(&mut self, id: Uuid) {
        let commands = self.carts.stop_running(id);
        self.dispatch(commands);
        let _ = self.carts.active_mut().remove_by_id(id);
    }

    /// Löscht die primäre Selektion (stoppt sie vorher, siehe
    /// `delete_cue_by_id`; auch Group-Kind möglich).
    pub fn delete_selected(&mut self) {
        if let Some(id) = self.carts.active().primary_selection() {
            self.delete_cue_by_id(id);
        }
    }

    /// Dupliziert Cues per ID (Kontextmenü „Duplicate", Mehrfachauswahl).
    /// Rückgabe: IDs der Kopien.
    pub fn duplicate_cues(&mut self, ids: &[Uuid]) -> Vec<Uuid> {
        self.carts.active_mut().duplicate_ids(ids)
    }

    /// Zwischenablage für Cue-Properties gefüllt?
    pub fn has_properties_clipboard(&self) -> bool {
        self.properties_clipboard.is_some()
    }

    /// „Copy Cue Properties" (Edit-Menü): serialisiert die Properties des
    /// selektierten Cues (ohne ID/Nummer/Status/Kinder) in die Zwischenablage.
    pub fn copy_cue_properties(&mut self) {
        const PROTECTED: &[&str] = &[
            "id",
            "number",
            "status",
            "children",
            "parent_group",
            "group_iterations_done",
        ];
        if let Some(cue) = self.carts.selected() {
            if let Ok(mut json) = serde_json::to_value(cue) {
                if let Some(obj) = json.as_object_mut() {
                    for key in PROTECTED {
                        obj.remove(*key);
                    }
                }
                self.properties_clipboard = Some(json);
            }
        }
    }

    /// „Paste Cue Properties": Properties aus der Zwischenablage auf alle
    /// selektierten Cues mergen (ID/Nummer/Status bleiben).
    pub fn paste_cue_properties(&mut self) -> usize {
        let Some(props) = self.properties_clipboard.clone() else {
            return 0;
        };
        let ids = self.carts.active().selection().to_vec();
        let mut applied = 0;
        for id in ids {
            if self.remote_update_cue(id, props.clone()).is_ok() {
                applied += 1;
            }
        }
        applied
    }

    /// Gruppiert Cues per ID zu einem neuen Group-Cue (Kontextmenü
    /// „Group Cues"). Rückgabe: ID der neuen Gruppe.
    pub fn group_cues(&mut self, ids: &[Uuid]) -> Option<Uuid> {
        self.carts.active_mut().group_ids(ids)
    }

    /// Setzt den Armed-Zustand mehrerer Cues (Kontextmenü „Arm/Disarm",
    /// Mehrfachauswahl; auch Group-Kinder).
    pub fn set_cues_armed(&mut self, ids: &[Uuid], armed: bool) {
        let list = self.carts.active_mut();
        for id in ids {
            if let Some(cue) = list.find_cue_mut(*id) {
                cue.is_armed = armed;
            }
        }
    }

    /// Setzt die Anzeigefarbe mehrerer Cues (Kontextmenü „Set Color",
    /// Mehrfachauswahl; auch Group-Kinder).
    pub fn set_cues_color(&mut self, ids: &[Uuid], color: Option<[u8; 3]>) {
        let list = self.carts.active_mut();
        for id in ids {
            if let Some(cue) = list.find_cue_mut(*id) {
                cue.color = color;
            }
        }
    }

    /// Verschiebt den selektierten Cue um `delta` (−1 = hoch, +1 = runter).
    pub fn move_selected(&mut self, delta: i32) {
        let Some(i) = self.carts.selected_index() else {
            return;
        };
        let list = self.carts.active_mut();
        if delta < 0 {
            list.move_up(i);
        } else {
            list.move_down(i);
        }
    }

    /// Setzt den Armed-Zustand des selektierten Cues.
    pub fn set_selected_armed(&mut self, armed: bool) {
        if let Some(cue) = self.carts.selected_mut() {
            cue.is_armed = armed;
        }
    }

    /// Setzt die Anzeigefarbe des selektierten Cues („Cue Color",
    /// `None` = keine Farbe).
    pub fn set_selected_color(&mut self, color: Option<[u8; 3]>) {
        if let Some(cue) = self.carts.selected_mut() {
            cue.color = color;
        }
    }

    /// Waveform-Peaks für eine Datei (gecacht; `None` bei Dekodierfehler).
    pub fn waveform_for(&mut self, path: &Path) -> Option<std::sync::Arc<Waveform>> {
        self.waveforms.get_or_compute(path, WAVEFORM_BUCKETS)
    }

    /// Zuletzt aufgetretener Fehler (für die Footer-Fehleranzeige + Tests).
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Verwirft die angezeigte Fehlermeldung (Footer „×").
    pub fn dismiss_error(&mut self) {
        self.last_error = None;
    }

    // ------------------------------------------------------------------
    // UI-Layout-Persistenz (Workspace bleibt über Neustarts erhalten)
    // ------------------------------------------------------------------

    /// Speichert den UI-Zustand (Panel-Sichtbarkeit, Tabs, Fenster) als
    /// JSON im `eframe::Storage` (wird von `eframe::App::save` aufgerufen).
    pub fn save_ui_state(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(super::state::UI_STATE_STORAGE_KEY, self.ui.to_json());
    }

    /// Lädt den UI-Zustand aus dem `eframe::Storage` (fehlend/ungültig →
    /// die bisherigen Werte bleiben; `from_json` liefert Defaults).
    pub fn load_ui_state(&mut self, storage: &dyn eframe::Storage) {
        if let Some(json) = storage.get_string(super::state::UI_STATE_STORAGE_KEY) {
            self.ui = super::state::UiState::from_json(&json);
        }
    }

    /// Speichert die Workspace-Templates als JSON im `eframe::Storage`
    /// (wird von `eframe::App::save` aufgerufen).
    pub fn save_ui_templates(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            super::state::UI_TEMPLATES_STORAGE_KEY,
            super::state::templates_to_json(&self.workspace_templates),
        );
    }

    /// Speichert das aktuelle UI-Layout als Workspace-Template
    /// („Workspace Templates"). Gleicher Name → ersetzt (Rename-Semantik);
    /// leerer Name → ignoriert.
    pub fn save_workspace_template(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let layout = self.ui.clone();
        self.workspace_templates.retain(|t| t.name != name);
        self.workspace_templates
            .push(super::state::WorkspaceTemplate { name, layout });
    }

    /// Wendet ein Workspace-Template an (Layout wiederherstellen).
    /// Laufzeit-Zustand (Find-Leiste) bleibt erhalten.
    pub fn apply_workspace_template(&mut self, index: usize) {
        if let Some(template) = self.workspace_templates.get(index) {
            let find = self.ui.find.clone();
            let mut layout = template.layout.clone();
            layout.find = find;
            self.ui = layout;
        }
    }

    /// Löscht ein Workspace-Template.
    pub fn delete_workspace_template(&mut self, index: usize) {
        if index < self.workspace_templates.len() {
            self.workspace_templates.remove(index);
        }
    }

    // ------------------------------------------------------------------
    // Broken Cues (Warnungs-Zähler im Footer, ✕ in der Cue-Liste)
    // ------------------------------------------------------------------

    /// Aktualisiert den Broken-Cache (max. alle 2 s – Dateisystem-Checks
    /// nicht jeden Frame; siehe [`super::problems::cue_problems`]).
    /// Gruppen-Kinder werden einzeln gecacht (✕ in der eingerückten Zeile);
    /// der Footer-Zähler zählt nur Top-Level-Cues (kein Doppelzählen).
    pub fn refresh_problems(&mut self) {
        let stale = self
            .problems_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(2));
        if !stale {
            return;
        }
        self.problems_at = Some(Instant::now());
        let mut found = Vec::new();
        for cart_idx in 0..self.carts.cart_count() {
            let list = self.carts.cart(cart_idx);
            for (index, cue) in list.cues().iter().enumerate() {
                let cue_problems = super::problems::cue_problems(cue, self);
                if !cue_problems.is_empty() {
                    found.push(BrokenCue {
                        cart: cart_idx,
                        index,
                        id: cue.id,
                        child: false,
                        problems: cue_problems,
                    });
                }
                for child in &cue.children {
                    let child_problems = super::problems::cue_problems(child, self);
                    if !child_problems.is_empty() {
                        found.push(BrokenCue {
                            cart: cart_idx,
                            index,
                            id: child.id,
                            child: true,
                            problems: child_problems,
                        });
                    }
                }
            }
        }
        self.problems = found;
    }

    /// Anzahl kaputter Cues (Footer: „⚠ N"; Top-Level-Cues, Gruppen-
    /// Kinder zählen nicht doppelt).
    /// Anzeige-Name der Engine (Workspace-Status-Fenster, Footer).
    pub fn engine_name(&self) -> &str {
        &self.engine_label
    }

    /// DeckLink-Statistik aus dem EngineHandle (frames, late, dropped, queued) – G4.
    pub fn engine_decklink_stats(&self) -> (u64, u64, u64, u64) {
        let st = self.engine.decklink_stats.lock().unwrap();
        (st.frames, st.late, st.dropped, st.queued)
    }

    pub fn broken_count(&mut self) -> usize {
        self.refresh_problems();
        self.problems.iter().filter(|b| !b.child).count()
    }

    /// Probleme des ersten kaputten Cues (Tooltip des Footer-Indikators).
    pub fn first_broken_problems(&mut self) -> Option<Vec<String>> {
        self.refresh_problems();
        self.problems
            .iter()
            .find(|b| !b.child)
            .map(|b| b.problems.clone())
    }

    /// Springt zum ersten kaputten Cue (Footer-Indikator: Cart aktivieren,
    /// Cue selektieren + Playhead setzen).
    pub fn jump_to_first_broken(&mut self) {
        self.refresh_problems();
        let Some((cart, index)) = self
            .problems
            .iter()
            .find(|b| !b.child)
            .map(|b| (b.cart, b.index))
        else {
            return;
        };
        if self.carts.active_index() != cart {
            let _ = self.carts.set_active(cart);
        }
        let list = self.carts.active_mut();
        list.select(Some(index));
        list.set_cursor(index);
    }

    /// Probleme eines einzelnen Cues (gecacht; für das ✕ in der Cue-Liste,
    /// Phase 2). Funktioniert auch für Gruppen-Kinder (eigener Eintrag);
    /// die Gruppe rollt Kinder-Probleme zusätzlich hoch.
    pub fn problems_of(&mut self, cue: &Cue) -> Vec<String> {
        self.refresh_problems();
        self.problems
            .iter()
            .find(|b| b.id == cue.id)
            .map(|b| b.problems.clone())
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Anzeige-Metadaten für die Cue-Tabelle (TODO.md Phase 2)
    // ------------------------------------------------------------------

    /// Vergangene Zeit seit Phasenbeginn (Playing/Paused/Post-Wait) – für
    /// die Fortschrittsbalken in der Cue-Liste (Phase 2). Während Post-Wait
    /// läuft `started_at` bis `Completed` weiter → Gesamtzeit seit Start.
    pub fn phase_elapsed_of(&self, cue: &Cue) -> Option<Duration> {
        match cue.status {
            CueStatus::Playing | CueStatus::PostWait => {
                let banked = self.banked.get(&cue.id).copied().unwrap_or_default();
                self.started_at
                    .get(&cue.id)
                    .map(|start| banked + start.elapsed())
            }
            CueStatus::Paused => self.banked.get(&cue.id).copied(),
            _ => None,
        }
    }

    /// Verbleibende Pre-Wait-Zeit (Countdown in der Pre-Wait-Spalte,
    /// TODO.md Phase 2). `None`, wenn der Cue nicht in Pre-Wait ist.
    pub fn pre_wait_remaining(&self, cue: &Cue) -> Option<Duration> {
        if cue.status != CueStatus::PreWait {
            return None;
        }
        let start = self.prewait_started.get(&cue.id)?;
        Some(cue.pre_wait.saturating_sub(start.elapsed()))
    }

    /// Pflegt die Pre-Wait-Startzeitpunkte: Cues, die neu in Pre-Wait sind,
    /// bekommen `Instant::now()`, Einträge nicht mehr laufender Cues
    /// werden entfernt.
    fn reconcile_prewait_timings(&mut self) {
        let now = Instant::now();
        let mut seen = HashSet::new();
        for cue in self.carts.active().cues() {
            if cue.status == CueStatus::PreWait {
                seen.insert(cue.id);
                self.prewait_started.entry(cue.id).or_insert(now);
            }
            for child in &cue.children {
                if child.status == CueStatus::PreWait {
                    seen.insert(child.id);
                    self.prewait_started.entry(child.id).or_insert(now);
                }
            }
        }
        self.prewait_started.retain(|id, _| seen.contains(id));
    }

    /// Baut die Anzeige-Metadaten für die Cue-Tabelle (Broken-Cache,
    /// Phasen-Zeiten, Pre-Wait-Countdown; TODO.md Phase 2). Einmalig pro
    /// Frame aufrufen (verwaltet dabei auch die Pre-Wait-Zeitpunkte).
    pub fn cue_row_meta(&mut self) -> HashMap<Uuid, cue_table::CueRowMeta> {
        self.reconcile_prewait_timings();
        // Cues klonen: problems_of braucht &mut self (Cache), die Liste wird
        // danach für die Tabelle mutable geborgt.
        let snapshot: Vec<Cue> = self.carts.active().cues().to_vec();
        let mut map = HashMap::new();
        for cue in &snapshot {
            map.insert(
                cue.id,
                cue_table::CueRowMeta {
                    problems: self.problems_of(cue),
                    elapsed: self.phase_elapsed_of(cue),
                    pre_wait_remaining: self.pre_wait_remaining(cue),
                    media_duration: self.media_durations.get(&cue.id).copied(),
                },
            );
            for child in &cue.children {
                map.insert(
                    child.id,
                    cue_table::CueRowMeta {
                        problems: self.problems_of(child),
                        elapsed: self.phase_elapsed_of(child),
                        pre_wait_remaining: self.pre_wait_remaining(child),
                        media_duration: self.media_durations.get(&child.id).copied(),
                    },
                );
            }
        }
        map
    }

    // ------------------------------------------------------------------
    // Show-Mode (Sicherheitsfunktion)
    // ------------------------------------------------------------------

    /// Schaltet den Show-Mode um (Editing deaktiviert/aktiviert).
    pub fn set_show_mode(&mut self, show_mode: bool) {
        self.show_mode = show_mode;
        if show_mode {
            // Show-Mode: Test-Grid immer aus (kein Overlay im Playout).
            self.set_test_grid(false);
        }
        self.remote_dirty = true;
    }

    /// Test-Grid über allen Regionen an/aus (Footer-Icon). Wirkt sofort auf
    /// alle Stages/Routen.
    pub fn set_test_grid(&mut self, on: bool) {
        self.ui.test_grid = on;
        for stage in self.stages.stages_mut() {
            for region in &mut stage.regions {
                region.grid = on;
            }
        }
        self.configure_routes();
        self.remote_dirty = true;
    }

    /// Toggelt den Show-Mode.
    pub fn toggle_show_mode(&mut self) {
        self.set_show_mode(!self.show_mode);
    }

    // ------------------------------------------------------------------
    // Remote-Steuerung (Web-Remote + Companion)
    // ------------------------------------------------------------------

    /// Non-blocking Poll eingehender Remote-Commands (beide Server).
    /// Public für headless Tests der Web-Remote.
    pub fn poll_remote_commands(&mut self) {
        self.poll_midi_start();
        let Some(remote) = &mut self.remote else {
            return;
        };
        let mut commands = Vec::new();
        if let Some(rx) = &mut remote.web_commands {
            while let Ok(cmd) = rx.try_recv() {
                commands.push(cmd);
            }
        }
        if let Some(rx) = &mut remote.companion_commands {
            while let Ok(cmd) = rx.try_recv() {
                commands.push(cmd);
            }
        }
        if let Some(rx) = &mut remote.osc_commands {
            while let Ok(cmd) = rx.try_recv() {
                commands.push(cmd);
            }
        }
        if let Some(rx) = &mut remote.midi_commands {
            while let Ok(cmd) = rx.try_recv() {
                commands.push(cmd);
            }
        }
        for cmd in commands {
            self.handle_remote_command(cmd);
        }
    }

    /// Übernimmt das Ergebnis des MIDI-Starts, sobald der Hintergrund-Thread
    /// fertig ist. Public für headless GUI-Tests.
    pub fn poll_midi_start(&mut self) {
        let Some(remote) = &mut self.remote else {
            return;
        };
        let Some(pending) = &remote.midi_start else {
            return;
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                remote.midi_start = None;
                tracing::warn!("midi start thread beendet ohne Ergebnis");
                return;
            }
        };
        remote.midi_start = None;
        match result {
            Ok((rx, guard)) => {
                remote.midi_commands = Some(rx);
                remote._midi_guard = Some(guard);
            }
            Err(e) => {
                tracing::warn!(%e, "midi input nicht gestartet");
                if matches!(e, crate::remote::midi::MidiError::Init(_)) {
                    remote.midi_init_failed = true;
                    // Vor der Show sichtbar machen: MIDI bliebe sonst unbemerkt aus.
                    self.last_error =
                        Some("MIDI unavailable. Restart KuhPlay to retry.".to_string());
                }
            }
        }
    }

    /// `true`, solange der MIDI-Start im Hintergrund läuft.
    pub fn midi_starting(&self) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|remote| remote.midi_start.is_some())
    }

    /// Verarbeitet ein Remote-Command (Web-Remote / Companion) wie die
    /// entsprechende UI-Aktion.
    pub fn handle_remote_command(&mut self, cmd: RemoteCommand) {
        if self.show_mode && cmd.is_edit() {
            self.report_remote_error("Show Mode is active: editing is disabled.".to_string());
            return;
        }
        if let Err(message) = self.apply_remote_command(cmd) {
            self.report_remote_error(message);
        }
        self.remote_dirty = true;
    }

    fn report_remote_error(&self, message: String) {
        tracing::warn!(%message, "remote command abgelehnt");
        self.broadcast_remote_event(RemoteEvent::Error { message });
    }

    /// Führt ein Remote-Command aus, wie die entsprechende UI-Aktion.
    fn apply_remote_command(&mut self, cmd: RemoteCommand) -> Result<(), String> {
        match cmd {
            RemoteCommand::Go => self.go_now(),
            RemoteCommand::Panic => {
                self.panic();
                self.broadcast_remote_event(RemoteEvent::Panicked);
            }
            RemoteCommand::PauseAll => self.pause_all(),
            RemoteCommand::ResumeAll => self.resume_all(),
            RemoteCommand::Reset => self.reset_all(),
            RemoteCommand::GoTo { cue } => match self.carts.go_to_number(&cue) {
                Some(outcome) => self.dispatch(outcome.commands),
                None => {
                    tracing::warn!(%cue, "remote go_to: cue nicht gefunden oder nicht triggerbar")
                }
            },
            RemoteCommand::Arm { cue, armed } => {
                if !self.carts.set_armed_by_number(&cue, armed) {
                    tracing::warn!(%cue, "remote arm: cue nicht gefunden");
                }
            }
            RemoteCommand::PauseCue { id } => {
                let commands = self.pause_running(id);
                self.dispatch(commands);
            }
            RemoteCommand::ResumeCue { id } => {
                let commands = self.resume_running(id);
                self.dispatch(commands);
            }
            RemoteCommand::StopCue { id } => {
                let commands = self.stop_running(id);
                self.dispatch(commands);
            }
            RemoteCommand::SetShowMode { enabled } => {
                self.set_show_mode(enabled);
                self.broadcast_remote_event(RemoteEvent::ShowModeChanged { enabled });
            }
            RemoteCommand::GetStatus => {}
            RemoteCommand::ReceiveTimecode { tc } => self.receive_timecode(tc),
            RemoteCommand::NewCue { cue_type } => self.add_cue(cue_type),
            RemoteCommand::UpdateCue { id, changes } => self.remote_update_cue(id, changes)?,
            RemoteCommand::DeleteCue { id } => self.delete_cue_by_id(id),
            RemoteCommand::DuplicateCues { ids } => {
                self.duplicate_cues(&ids);
            }
            RemoteCommand::GroupCues { ids } => {
                self.group_cues(&ids);
            }
            RemoteCommand::MoveCue { id, to } => {
                let list = self.carts.active_mut();
                let from = list
                    .cues()
                    .iter()
                    .position(|cue| cue.id == id)
                    .ok_or("Only top-level cues can be moved.")?;
                list.move_to(from, to);
            }
            RemoteCommand::SetPlayhead { id } => {
                let index = self
                    .carts
                    .active()
                    .cues()
                    .iter()
                    .position(|cue| cue.id == id)
                    .ok_or("The playhead can only be set on top-level cues.")?;
                self.carts.set_cursor(index);
            }
            RemoteCommand::SetActiveCart { index } => {
                self.carts.set_active(index).map_err(|e| e.to_string())?
            }
            RemoteCommand::AddCart { name } => {
                self.carts.add_cart(name);
            }
            RemoteCommand::SetShowName { name } => self.show_name = name,
            RemoteCommand::NewShow => self.new_show(),
            RemoteCommand::SaveShow => {
                let path = self
                    .current_file
                    .clone()
                    .ok_or("This show has no file yet. Use Save As in the app.")?;
                self.save_show(&path);
                if let Some(err) = &self.last_error {
                    return Err(err.clone());
                }
            }
            RemoteCommand::OpenShow { path } => {
                self.load_show(&remote_show_path(&path)?)
                    .map_err(|e| format!("Could not open the show: {e}"))?;
            }
            RemoteCommand::SaveShowAs { path } => {
                self.save_show(&remote_show_path(&path)?);
                if let Some(err) = &self.last_error {
                    return Err(err.clone());
                }
            }
            RemoteCommand::SetAudition { enabled } => self.set_audition_mode(enabled),
            RemoteCommand::SetAudioOutput { device } => self.set_audio_output(device),
            RemoteCommand::SetAudioPatch { patch } => self.set_audio_patch(*patch), // migriert in set_audio_patch
            RemoteCommand::RefreshAudioDevices => self.audio_devices = None,
            RemoteCommand::SetOutputMode { mode } => self.set_output_mode(match mode.as_str() {
                "off" => OutputMode::Off,
                "preview" => OutputMode::Preview,
                "displays" => OutputMode::Displays,
                other => return Err(format!("Unknown output mode: {other}")),
            }),
            RemoteCommand::AddRoute {
                name,
                width,
                height,
            } => {
                let route = crate::core::OutputRoute::new(name, width, height);
                self.routes.add(route).map_err(|e| e.to_string())?;
                self.configure_routes();
            }
            RemoteCommand::UpdateRoute { id, changes } => {
                let route = self.routes.find(id).ok_or("Route not found.")?;
                let updated: crate::core::OutputRoute = merge_json(route, changes, &["id"])?;
                if let Some(found) = self.routes.find_mut(id) {
                    *found = updated;
                }
                self.configure_routes();
            }
            RemoteCommand::RemoveRoute { id } => {
                self.remove_route(id);
            }
            RemoteCommand::AddStage { name } => {
                self.ensure_stage(&name);
            }
            RemoteCommand::UpdateStage { name, changes } => {
                let stage = self
                    .stages
                    .find(&name)
                    .ok_or_else(|| format!("Stage not found: {name}"))?;
                let updated = merge_json(stage, changes, &["id", "name"])?;
                self.configure_stage(updated);
            }
            RemoteCommand::RemoveStage { name } => {
                if !self.remove_stage(&name) {
                    return Err("Cues still use this stage.".to_string());
                }
            }
            RemoteCommand::RenameStage { old, new } => {
                self.rename_stage(&old, &new);
            }
            RemoteCommand::AddRegion { stage, changes } => {
                let found = self.stages.find(&stage).ok_or("Stage not found.")?;
                let (sw, sh) = (found.geometry.width, found.geometry.height);
                let base = crate::core::Region::new(sw / 4.0, sh / 4.0, sw / 2.0, sh / 2.0);
                let region: crate::core::Region = merge_json(&base, changes, &["id"])?;
                let mut updated = found.clone();
                updated.regions.push(region);
                self.configure_stage(updated);
            }
            RemoteCommand::UpdateRegion {
                stage,
                region,
                changes,
            } => {
                let found = self.stages.find(&stage).ok_or("Stage not found.")?;
                let existing = found
                    .regions
                    .iter()
                    .find(|r| r.id == region)
                    .ok_or("Region not found.")?;
                let updated: crate::core::Region = merge_json(existing, changes, &["id"])?;
                let mut stage_updated = found.clone();
                if let Some(existing) = stage_updated.regions.iter_mut().find(|r| r.id == region) {
                    *existing = updated;
                }
                self.configure_stage(stage_updated);
            }
            RemoteCommand::RemoveRegion { stage, region } => {
                let found = self.stages.find(&stage).ok_or("Stage not found.")?;
                let mut updated = found.clone();
                updated.regions.retain(|r| r.id != region);
                self.configure_stage(updated);
            }
            RemoteCommand::EvalJs { id, script } => {
                self.dispatch(vec![EngineCommand::EvalJs { cue_id: id, script }]);
            }
            RemoteCommand::PushJson { id } => {
                let cue = self.carts.find_cue_any(id).ok_or("Cue not found.")?;
                let target = cue.data_target.ok_or("Choose a target HTML cue first.")?;
                let payload = cue.data_payload().map_err(|e| e.to_string())?;
                self.dispatch(vec![EngineCommand::PushJson { target, payload }]);
            }
        }
        Ok(())
    }

    /// Ändert Felder eines Cues. Laufzeit-Zustand und Identität bleiben.
    fn remote_update_cue(&mut self, id: Uuid, changes: serde_json::Value) -> Result<(), String> {
        const PROTECTED: &[&str] = &[
            "id",
            "status",
            "children",
            "parent_group",
            "group_iterations_done",
        ];
        let cue = self.carts.find_cue_any(id).ok_or("Cue not found.")?;
        let updated: Cue = merge_json(cue, changes, PROTECTED)?;
        if let Some(route) = &updated.route {
            route.validate().map_err(|e| e.to_string())?;
        }
        let (volume_changed, playing) = (
            updated.volume != cue.volume,
            cue.status == CueStatus::Playing,
        );
        if let Some(stage) = updated.stage.clone() {
            self.ensure_stage(&stage);
        }
        let volume = updated.volume;
        *self.carts.find_cue_any_mut(id).ok_or("Cue not found.")? = updated;
        if volume_changed && playing {
            self.dispatch(vec![EngineCommand::SetVolume { cue_id: id, volume }]);
        }
        Ok(())
    }

    /// Baut die komplette Show für die Web-Remote.
    pub fn build_remote_workspace(&mut self) -> RemoteWorkspace {
        use crate::core::{
            BlendCurve, ContinueMode, ControlAction, EdgeSide, FadeCurve, FadeParam, Fps,
            GroupMode, ScreenRotation, TestPatternKind,
        };
        use crate::engine::audio_backend::{default_output_name, output_devices};
        use crate::remote::{RemoteAudioDevice, RemoteDevice, RemoteOption, RemoteShortcut};
        fn options<T: serde::Serialize + Copy>(
            items: &[T],
            label: impl Fn(T) -> String,
        ) -> Vec<RemoteOption> {
            items
                .iter()
                .map(|item| RemoteOption {
                    value: serde_json::to_value(item).unwrap_or_default(),
                    label: label(*item),
                })
                .collect()
        }
        let list = self.audio_devices.get_or_insert_with(|| AudioDeviceList {
            devices: output_devices(),
            default_name: default_output_name(),
        });
        let audio_devices = list
            .devices
            .iter()
            .map(|d| RemoteAudioDevice {
                name: d.name.clone(),
                channels: d.channels,
                sample_rate: d.sample_rate,
            })
            .collect();
        let default_audio_device = list.default_name.clone();
        let cues: Vec<Cue> = self.carts.active().cues().to_vec();
        let mut problems = HashMap::new();
        for cue in cues.iter().chain(cues.iter().flat_map(|c| &c.children)) {
            let found = self.problems_of(cue);
            if !found.is_empty() {
                problems.insert(cue.id, found);
            }
        }
        let on_off = |on: bool, value: String| if on { value } else { "Off".to_string() };
        let config = &self.remote_config;
        let map = config.midi_map;
        let midi_status = if !config.midi_enabled {
            "Off"
        } else if self.midi_connected() {
            "Connected"
        } else if self.midi_init_failed() {
            "Unavailable, restart KuhPlay to retry"
        } else {
            "No device connected"
        };
        // G5/G8: Blackmagic-Geräte + Treiber-Hinweis für die Geräteauswahl.
        let (dl_avail, dl_devices, dl_hint) = crate::engine::decklink_native::devices_with_hint();
        let _ = dl_avail;
        RemoteWorkspace {
            decklink_devices: dl_devices,
            decklink_hint: dl_hint,
            show_name: self.show_name.clone(),
            file: self.current_file.as_ref().map(|p| p.display().to_string()),
            audition_mode: self.audition_mode,
            output_mode: match self.output_mode {
                OutputMode::Off => "off",
                OutputMode::Preview => "preview",
                OutputMode::Displays => "displays",
            }
            .to_string(),
            audio_output: self.audio_output.clone(),
            audio_patch: self.audio_patch.clone(),
            audio_devices,
            default_audio_device,
            cues,
            problems,
            stages: self.stages.stages().to_vec(),
            routes: self.routes.routes().to_vec(),
            devices: self
                .displays
                .iter()
                .map(|d| RemoteDevice {
                    id: d.id,
                    name: d.name.clone(),
                    x: d.x,
                    y: d.y,
                    width: d.width,
                    height: d.height,
                    scale_factor: d.scale_factor,
                    frequency: d.frequency,
                })
                .collect(),
            options: [
                (
                    "cue_type",
                    options(&CueType::ALL, |v| v.label().to_string()),
                ),
                (
                    "continue_mode",
                    options(&ContinueMode::ALL, |v| v.label().to_string()),
                ),
                (
                    "group_mode",
                    options(&GroupMode::ALL, |v| v.label().to_string()),
                ),
                (
                    "control_action",
                    options(&ControlAction::ALL, |v| v.label().to_string()),
                ),
                (
                    "fade_param",
                    options(&FadeParam::ALL, |v| v.label().to_string()),
                ),
                (
                    "fade_curve",
                    options(&FadeCurve::ALL, |v| v.label().to_string()),
                ),
                ("fps", options(&Fps::ALL, |v| v.label().to_string())),
                (
                    "edge_side",
                    options(&EdgeSide::ALL, |v| v.label().to_string()),
                ),
                (
                    "blend_curve",
                    options(&BlendCurve::ALL, |v| v.label().to_string()),
                ),
                (
                    "geometry_mode",
                    options(&crate::core::GeometryMode::ALL, |v| v.label().to_string()),
                ),
                (
                    "rotation",
                    options(&ScreenRotation::ALL, |v| v.label().to_string()),
                ),
                (
                    "route_scaling",
                    options(&crate::core::RouteScaling::ALL, |v| v.label().to_string()),
                ),
                (
                    "test_pattern_kind",
                    options(&TestPatternKind::ALL, |v| v.label().to_string()),
                ),
            ]
            .into_iter()
            .map(|(key, values)| (key.to_string(), values))
            .collect(),
            shortcuts: shortcuts::ALL
                .iter()
                .map(|s| RemoteShortcut {
                    group: s.group.to_string(),
                    label: s.label.to_string(),
                    keys: s.display(),
                })
                .collect(),
            network: vec![
                (
                    "Web Remote".to_string(),
                    on_off(config.web_enabled, format!("Port {}", config.web_port)),
                ),
                (
                    "Companion".to_string(),
                    on_off(
                        config.companion_enabled,
                        format!("Port {}", config.companion_port),
                    ),
                ),
                (
                    "OSC In".to_string(),
                    on_off(config.osc_enabled, format!("Port {}", config.osc_port)),
                ),
                (
                    "OSC Prefix".to_string(),
                    on_off(config.osc_enabled, config.osc_prefix.clone()),
                ),
            ],
            midi: vec![
                ("Input".to_string(), midi_status.to_string()),
                ("Go".to_string(), format!("Note {}", map.go)),
                ("Panic".to_string(), format!("Note {}", map.panic)),
                ("Pause".to_string(), format!("Note {}", map.pause)),
                ("Resume".to_string(), format!("Note {}", map.resume)),
                ("Reset".to_string(), format!("Note {}", map.reset)),
            ],
        }
    }

    /// Schickt Status und, falls geändert, die Show an die Remote-Clients:
    /// sofort bei Änderungen, sonst einmal pro Sekunde.
    pub fn pump_remote(&mut self) {
        if self.remote.is_none() {
            return;
        }
        if !self.remote_dirty && self.last_status_push.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.remote_dirty = false;
        self.last_status_push = Instant::now();
        self.broadcast_remote_status();
        let web = self
            .remote
            .as_ref()
            .and_then(|remote| remote.web_events.clone());
        if let Some(web) = web {
            let workspace = self.build_remote_workspace();
            let json = serde_json::to_string(&workspace).unwrap_or_default();
            if json != self.last_workspace_json {
                self.last_workspace_json = json;
                let _ = web.send(RemoteEvent::Workspace(Box::new(workspace)));
            }
        }
    }

    /// Sendet ein Event an alle verbundenen Remote-Clients (Web + Companion
    /// + OSC-Out).
    fn broadcast_remote_event(&self, event: RemoteEvent) {
        if let Some(remote) = &self.remote {
            if let Some(tx) = &remote.web_events {
                let _ = tx.send(event.clone());
            }
            if let Some(tx) = &remote.companion_events {
                let _ = tx.send(event.clone());
            }
            if let Some(tx) = &remote.osc_events {
                let _ = tx.send(event.clone());
            }
        }
    }

    /// Baut einen Status-Snapshot für Remote-Clients aus dem App-Zustand.
    pub fn build_remote_status(&self) -> RemoteStatus {
        let active = self.carts.active();
        let standby = active.standby().map(|c| StandbyCue {
            id: c.id,
            number: c.number.clone(),
            name: c.name.clone(),
            armed: c.is_armed,
        });
        let running = active
            .cues()
            .iter()
            .filter(|c| is_running_status(c.status))
            .map(|c| RunningCue {
                id: c.id,
                number: c.number.clone(),
                name: c.name.clone(),
                status: format!("{:?}", c.status),
                elapsed_ms: self
                    .elapsed_of(c)
                    .map(crate::remote::duration_ms)
                    .unwrap_or(0),
                duration_ms: c.duration.as_millis() as u64,
            })
            .collect();
        let carts = (0..self.carts.cart_count())
            .map(|i| CartInfo {
                index: i,
                name: self.carts.cart_name(i).to_string(),
                cue_count: self.carts.cart_cue_count(i),
            })
            .collect();
        let cues = active
            .cues()
            .iter()
            .map(|c| CueInfo {
                id: c.id,
                number: c.number.clone(),
                name: c.name.clone(),
                cue_type: format!("{:?}", c.cue_type),
                armed: c.is_armed,
                status: format!("{:?}", c.status),
                is_running: is_running_status(c.status),
                duration_ms: c.duration.as_millis() as u64,
            })
            .collect();
        RemoteStatus::from_app(
            &self.show_name,
            self.show_mode,
            self.carts.active_index(),
            self.carts.cart_name(self.carts.active_index()),
            active.cursor(),
            standby,
            running,
            carts,
            cues,
        )
    }

    /// Pusht den aktuellen Status-Snapshot an alle Remote-Clients. Die
    /// Server cachen ihn für `GET /api/status` und neue Verbindungen.
    fn broadcast_remote_status(&self) {
        let status = self.build_remote_status();
        self.broadcast_remote_event(RemoteEvent::Status(status));
    }

    /// Q-Nummer + Name eines Cues (für Remote-Events) – über alle Carts
    /// gesucht (der Cue kann in einem nicht-aktiven Cart liegen).
    fn cue_label(&self, id: Uuid) -> (String, String) {
        self.carts
            .find_cue_any(id)
            .map(|c| (c.number.clone(), c.name.clone()))
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // Timing-Anzeige (Sidebar „Active Cues")
    // ------------------------------------------------------------------

    /// Spult einen laufenden Media-Cue (Audio/Video) an `position`
    /// („Scrub" in den Active Cues; Bild+Ton springen synchron, weil
    /// beide Backends im selben Engine-Command den Seek bekommen).
    /// Externer Timecode (OSC `/kuhplay/timecode`, später LTC/MTC): in die
    /// Engine injizieren (chasing Cues springen mit) und alle Timecode-Trigger-
    /// Cues feuern, deren Trigger ≤ tc ist („timecode trigger").
    pub fn receive_timecode(&mut self, tc: crate::core::Timecode) {
        let mut commands = vec![EngineCommand::InjectTimecode { tc }];
        commands.extend(self.carts.receive_timecode(tc));
        self.dispatch(commands);
    }

    /// Spult einen laufenden Cue an `position` (Active-Cues-Scrubber).
    ///
    /// **Timecode-Gruppe:** Hat der Cue ein `timecode_in`
    /// (Chasing), wird zusätzlich der externe Timecode `timecode_in + position`
    /// in die Engine injiziert – alle chasing Cues springen damit auf ihre
    /// neue Zeit mit (`EngineCommand::InjectTimecode`).
    pub fn seek_running(&mut self, id: Uuid, position: Duration) {
        let mut commands = vec![EngineCommand::Seek {
            cue_id: id,
            position,
        }];
        if let Some(cue) = self.carts.find_cue_any(id) {
            if let Some(t0) = cue.timecode_in {
                let frames = crate::core::Timecode::from_duration(position, t0.fps).frames;
                commands.push(EngineCommand::InjectTimecode {
                    tc: crate::core::Timecode {
                        frames: t0.frames + frames,
                        fps: t0.fps,
                    },
                });
            }
        }
        self.dispatch(commands);
    }

    /// Darf für diesen Cue gerade ein Scrub-Seek dispatched werden?
    /// Throttled während des Ziehens (sonst ffmpeg-Neustart pro Frame → Ruckeln):
    /// mind. 150 ms Abstand oder > 2 % Positionssprung; `force` (Klick/Loslassen)
    /// geht immer durch. Rückgabe: true = senden.
    pub fn scrub_throttle(&mut self, id: Uuid, frac: f32, force: bool) -> bool {
        const MIN_INTERVAL: Duration = Duration::from_millis(150);
        const MIN_DELTA: f32 = 0.02;
        let now = Instant::now();
        let allow = match self.scrub_last.get(&id) {
            Some((at, last_frac)) => {
                force
                    || now.duration_since(*at) >= MIN_INTERVAL
                    || (frac - last_frac).abs() >= MIN_DELTA
            }
            None => true,
        };
        if allow {
            self.scrub_last.insert(id, (now, frac));
        }
        allow
    }

    /// Medien-Dauer eines Cues (aus dem Started-Event), falls bekannt.
    pub fn media_duration_of(&self, cue: &Cue) -> Option<Duration> {
        self.media_durations.get(&cue.id).copied()
    }

    /// Elapsed-Zeit eines aktiven Cues (Playing: laufend akkumuliert,
    /// Paused: akkumulierter Wert bis zur Pause).
    pub fn elapsed_of(&self, cue: &Cue) -> Option<Duration> {
        match cue.status {
            CueStatus::Playing => {
                let banked = self.banked.get(&cue.id).copied().unwrap_or_default();
                self.started_at
                    .get(&cue.id)
                    .map(|start| banked + start.elapsed())
            }
            CueStatus::Paused => self.banked.get(&cue.id).copied(),
            _ => None,
        }
    }

    fn note_started(&mut self, id: Uuid) {
        self.started_at.insert(id, Instant::now());
    }

    fn note_paused(&mut self, id: Uuid) {
        if let Some(start) = self.started_at.remove(&id) {
            *self.banked.entry(id).or_default() += start.elapsed();
        }
    }

    fn note_resumed(&mut self, id: Uuid) {
        self.started_at.insert(id, Instant::now());
    }

    fn note_finished(&mut self, id: Uuid) {
        self.note_paused(id);
        self.banked.remove(&id);
    }

    fn clear_timing(&mut self) {
        self.started_at.clear();
        self.banked.clear();
        self.prewait_started.clear();
    }

    // ------------------------------------------------------------------
    // Persistenz (File-Menü)
    // ------------------------------------------------------------------

    /// Wechselt das Audio-Ausgabegerät (`None` = System-Standard). Übernommen
    /// wird es erst, wenn die Engine `AudioOutputChanged` meldet; schlägt der
    /// Wechsel fehl, bleibt das bisherige Gerät aktiv und der Footer zeigt den
    /// Fehler.
    pub fn set_audio_output(&mut self, device: Option<String>) {
        if device != self.audio_output {
            self.dispatch(vec![EngineCommand::SetAudioOutput { device }]);
        }
    }

    /// Übernimmt die Kanal-Zuordnung (Matrix: Kanal → mehrere Geräte-Ausgänge)
    /// und meldet sie an die Engine.
    pub fn set_audio_patch(&mut self, mut patch: crate::core::AudioPatch) {
        patch.migrate_legacy();
        self.audio_patch = patch.clone();
        self.dispatch(vec![EngineCommand::SetAudioPatch {
            patch: Box::new(patch),
        }]);
    }

    /// Aktives Audio-Ausgabegerät (`None` = System-Standard).
    pub fn audio_output(&self) -> Option<&str> {
        self.audio_output.as_deref()
    }

    /// Speichert die Show als JSON-Datei (`.kuhplay`).
    pub fn save_show(&mut self, path: &Path) {
        let doc = ShowDocument {
            name: self.show_name.clone(),
            carts: self.carts.clone(),
            stages: self.stages.clone(),
            routes: self.routes.clone(),
            screens: self.screens.clone(),
            audio_output: self.audio_output.clone(),
            audio_patch: self.audio_patch.clone(),
        };
        let result = doc
            .to_json()
            .and_then(|json| std::fs::write(path, json).map_err(crate::core::ShowError::from));
        match result {
            Ok(()) => {
                self.current_file = Some(path.to_path_buf());
                self.last_error = None;
            }
            Err(err) => {
                self.last_error = Some(err.to_string());
                tracing::error!(%err, "failed to save show");
            }
        }
    }

    /// Lädt eine Show aus einer JSON-Datei.
    pub fn load_show(&mut self, path: &Path) -> ShowResult<()> {
        let json = std::fs::read_to_string(path)?;
        let mut doc = ShowDocument::from_json(&json)?;
        // Alte Shows (Screens/Stage.screen) migrieren: Routen + Regionen.
        // Vor der Übernahme der Felder, damit `doc` noch ganz ist.
        let displays: Vec<DisplayRefInfo> =
            self.displays.iter().map(DisplayRefInfo::from).collect();
        doc.migrate_legacy_screens(&displays);
        // Device-Referenzen gegen die Displays DIESES Rechners auflösen
        // (alte IDs können sich geändert haben).
        let _ = doc.routes.resolve_device_refs(&displays);
        self.show_name = doc.name;
        self.carts = doc.carts;
        // Stages + Screens übernehmen; bei leeren Defaults neu aufbauen.
        self.stages = if doc.stages.is_empty() {
            StageManager::new()
        } else {
            doc.stages.clone()
        };
        self.routes = doc.routes.clone();
        self.screens = if doc.screens.is_empty() {
            ScreenManager::from_displays(&self.displays)
        } else {
            doc.screens.clone()
        };
        self.set_audio_output(doc.audio_output);
        self.set_audio_patch(doc.audio_patch);
        self.stage_windows.clear();
        // Alle Stages an die Engine melden.
        for stage in self.stages.stages().to_vec() {
            self.configure_stage(stage);
        }
        self.configure_routes();
        self.current_file = Some(path.to_path_buf());
        self.clear_timing();
        self.last_error = None;
        Ok(())
    }

    /// Neue leere Show (File → New Show).
    pub fn new_show(&mut self) {
        self.set_audio_output(None);
        self.set_audio_patch(crate::core::AudioPatch::default());
        self.carts = CartManager::new();
        self.stages = StageManager::new();
        self.screens = ScreenManager::from_displays(&self.displays);
        self.setup_default_routes();
        self.stage_windows.clear();
        // Default-Stages an die Engine melden.
        for stage in self.stages.stages().to_vec() {
            self.configure_stage(stage);
        }
        self.configure_routes();
        self.show_name = "Untitled Show".to_string();
        self.current_file = None;
        self.show_mode = false;
        self.clear_timing();
    }

    /// Öffnet den nativen Datei-Dialog und lädt die gewählte Show.
    ///
    /// **Hinweis:** `rfd` blockiert kurz den UI-Thread (macOS: native Dialoge
    /// laufen auf dem Main-Thread); die Engine arbeitet weiter.
    pub fn open_show_dialog(&mut self, _ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("KuhPlay Show", &["kuhplay", "json"])
            .set_title("Open Show")
            .pick_file()
        {
            if let Err(err) = self.load_show(&path) {
                self.last_error = Some(err.to_string());
                tracing::error!(%err, "failed to load show");
            }
        }
    }

    /// Speichert die Show; ohne geöffnete Datei → „Save As"-Dialog.
    pub fn save_show_dialog(&mut self, ctx: &egui::Context) {
        if let Some(path) = &self.current_file {
            let path = path.clone();
            self.save_show(&path);
        } else {
            self.save_show_as_dialog(ctx);
        }
    }

    /// Öffnet den „Save As"-Dialog und speichert die Show.
    pub fn save_show_as_dialog(&mut self, _ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("KuhPlay Show", &["kuhplay", "json"])
            .set_title("Save Show As")
            .set_file_name(format!("{}.kuhplay", self.show_name))
            .save_file()
        {
            self.save_show(&path);
        }
    }

    /// Beendet die App (⌘Q / File → Quit).
    pub fn quit(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    // ------------------------------------------------------------------
    // Engine-Kommunikation
    // ------------------------------------------------------------------

    /// Sendet Commands an die Engine (non-blocking; Fehler werden geloggt,
    /// nie gepanict – Live-Betrieb). Im Audition-Mode werden Stage-Ziele
    /// aus Video-Commands entfernt (siehe [`Self::apply_audition`]).
    pub fn dispatch(&mut self, commands: Vec<EngineCommand>) {
        for command in commands {
            let command = self.apply_audition(command);
            // Fades melden kein `Started` (keine Pre-Wait-Phase in der
            // Engine) – Startzeit hier setzen, sonst fehlt die Restzeit.
            if let EngineCommand::Fade { cue_id, .. } = &command {
                self.note_started(*cue_id);
            }
            if let Err(err) = self.engine.send(command) {
                self.last_error = Some(err.to_string());
                tracing::error!("failed to dispatch engine command: {err}");
            }
        }
    }

    /// Audition-Mode („Workspace → Audition Mode"): Video-Cues spielen
    /// ohne Stage-Ziel – das Bild landet nicht auf einer Stage, Audio bleibt
    /// unverändert (Audio-Play ignoriert `stage` in der Engine ohnehin).
    pub fn set_audition_mode(&mut self, enabled: bool) {
        self.audition_mode = enabled;
        self.remote_dirty = true;
    }

    /// Entfernt im Audition-Mode das Stage-Ziel aus Video-Commands
    /// (`Play`, `PlayHtml`, `PlaySlideShow`); alle anderen Commands bleiben.
    pub fn apply_audition(&self, command: EngineCommand) -> EngineCommand {
        if !self.audition_mode {
            return command;
        }
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
                stage: _,
                layer,
                surface,
            } => EngineCommand::Play {
                cue_id,
                path,
                duration,
                pre_wait,
                post_wait,
                loop_mode,
                slices,
                route,
                timecode_in,
                stage: None,
                layer,
                surface,
            },
            EngineCommand::PlayHtml {
                cue_id,
                source,
                stage: _,
                layer,
                duration,
                pre_wait,
                post_wait,
            } => EngineCommand::PlayHtml {
                cue_id,
                source,
                stage: None,
                layer,
                duration,
                pre_wait,
                post_wait,
            },
            EngineCommand::PlaySlideShow {
                cue_id,
                show,
                stage: _,
                layer,
                duration,
                pre_wait,
                post_wait,
                surface,
            } => EngineCommand::PlaySlideShow {
                cue_id,
                show,
                stage: None,
                layer,
                duration,
                pre_wait,
                post_wait,
                surface,
            },
            other => other,
        }
    }

    /// HTML5-Cue-Ziele des aktiven Carts (für HTML-Data-Cues im Inspector).
    pub fn html_targets(&self) -> Vec<(Uuid, String)> {
        self.carts
            .active()
            .cues()
            .iter()
            .filter(|c| c.cue_type == CueType::Html)
            .map(|c| (c.id, format!("{}  {}", c.number, c.name)))
            .collect()
    }

    // ------------------------------------------------------------------
    // Engine-Events → Domain-Status (+ Timing-Hooks + Auto-Follow)
    // ------------------------------------------------------------------

    /// `true`, wenn CoreMIDI den MIDI-Client abgelehnt hat.
    pub(super) fn midi_init_failed(&self) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|remote| remote.midi_init_failed)
    }

    /// Aktueller PIN der Web-Remote (`None` = keine Anmeldung).
    pub(super) fn web_pin(&self) -> Option<String> {
        self.remote
            .as_ref()
            .and_then(|remote| remote.web_auth.pin())
    }

    /// Setzt einen neuen PIN, speichert ihn und meldet alle Browser ab.
    pub(super) fn set_web_pin(&mut self, pin: String) {
        if let Err(e) = crate::remote::web::save_pin(&pin) {
            self.last_error = Some(format!("Could not save the PIN: {e}"));
        }
        if let Some(remote) = &self.remote {
            remote.web_auth.set_pin(pin);
        }
    }

    /// `true`, wenn ein MIDI-Input-Device verbunden ist.
    pub(super) fn midi_connected(&self) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|remote| remote._midi_guard.is_some())
    }

    /// Non-blocking Poll der Engine-Events → Status-Updates (über alle Carts,
    /// da Engine-Events auch Cues in nicht-aktiven Carts betreffen).
    /// Public für headless GUI-Tests (tests/gui_interaction.rs).
    pub fn poll_engine_events(&mut self) {
        while let Some(event) = self.engine.try_recv_event() {
            match event {
                EngineEvent::Started { cue_id, duration } => {
                    // Auto-Follow feuert beim Start.
                    self.apply_status_with_follow(cue_id, CueStatus::Playing);
                    self.note_started(cue_id);
                    if let Some(d) = duration {
                        self.media_durations.insert(cue_id, d);
                    }
                    let (number, name) = self.cue_label(cue_id);
                    self.broadcast_remote_event(RemoteEvent::CueStarted {
                        id: cue_id,
                        number,
                        name,
                    });
                    self.remote_dirty = true;
                }
                EngineEvent::Paused { cue_id } => {
                    self.apply_status(cue_id, CueStatus::Paused);
                    self.note_paused(cue_id);
                    self.broadcast_remote_event(RemoteEvent::CuePaused { id: cue_id });
                    self.remote_dirty = true;
                }
                EngineEvent::Resumed { cue_id } => {
                    self.apply_status(cue_id, CueStatus::Playing);
                    self.note_resumed(cue_id);
                    self.broadcast_remote_event(RemoteEvent::CueResumed { id: cue_id });
                    self.remote_dirty = true;
                }
                EngineEvent::Finished { cue_id } => {
                    // Post-Wait-Phase oder direkt Completed (je nach Cue).
                    // Über alle Carts gesucht (Engine-Events betreffen auch
                    // Cues in nicht-aktiven Carts).
                    let post_wait = self
                        .carts
                        .find_cue_any(cue_id)
                        .map(|c| c.post_wait)
                        .unwrap_or_default();
                    let target = if post_wait > Duration::ZERO {
                        CueStatus::PostWait
                    } else {
                        CueStatus::Completed
                    };
                    self.apply_status_with_follow(cue_id, target);
                    if target == CueStatus::Completed {
                        self.note_finished(cue_id);
                    }
                    self.broadcast_remote_event(RemoteEvent::CueFinished { id: cue_id });
                    self.remote_dirty = true;
                }
                EngineEvent::PostWaitElapsed { cue_id } => {
                    self.apply_status_with_follow(cue_id, CueStatus::Completed);
                    self.note_finished(cue_id);
                    self.broadcast_remote_event(RemoteEvent::CueFinished { id: cue_id });
                    self.remote_dirty = true;
                }
                EngineEvent::Stopped { cue_id } => {
                    self.apply_status(cue_id, CueStatus::Stopped);
                    self.note_finished(cue_id);
                    self.broadcast_remote_event(RemoteEvent::CueStopped { id: cue_id });
                    self.remote_dirty = true;
                }
                EngineEvent::AudioOutputChanged { device } => {
                    self.audio_output = device;
                }
                EngineEvent::Error { cue_id, message } => {
                    if let Some(id) = cue_id {
                        self.apply_status(id, CueStatus::Error);
                        self.note_finished(id);
                        self.broadcast_remote_event(RemoteEvent::CueError {
                            id,
                            message: message.clone(),
                        });
                    }
                    self.last_error = Some(message);
                    self.remote_dirty = true;
                }
                EngineEvent::HtmlLoaded { cue_id } => {
                    tracing::debug!(%cue_id, "html5 webview instance loaded");
                }
                EngineEvent::JsExecuted { cue_id, at, .. } => {
                    tracing::debug!(%cue_id, ?at, "js executed in webview");
                }
                EngineEvent::JsonReceived { target, at, .. } => {
                    tracing::debug!(%target, ?at, "json payload pushed to webview");
                }
                EngineEvent::SlideChanged { cue_id, index, at } => {
                    tracing::debug!(%cue_id, index, ?at, "slideshow changed slide");
                }
                // Spulen (Seek): elapsed-Uhr an die Sprungposition hängen.
                EngineEvent::TimecodeJump { cue_id, to, .. } => {
                    if self
                        .carts
                        .find_cue_any(cue_id)
                        .map(|c| c.status.is_active())
                        .unwrap_or(false)
                    {
                        self.banked.insert(cue_id, to);
                        if self.started_at.contains_key(&cue_id) {
                            self.started_at.insert(cue_id, Instant::now());
                        }
                    }
                }
                // Informative Events (V1 ohne UI-Anzeige, über Events abgedeckt).
                EngineEvent::Position { .. }
                | EngineEvent::LoopIteration { .. }
                | EngineEvent::FadeProgress { .. }
                | EngineEvent::FrameDrop { .. }
                | EngineEvent::Loaded { .. }
                | EngineEvent::StageConfigured { .. }
                | EngineEvent::Pong(_) => {}
            }
        }
    }

    /// Wendet einen Status an (State-Machine validiert; Fehler werden geloggt,
    /// nie gepanict – Live-Betrieb).
    fn apply_status(&mut self, cue_id: Uuid, status: CueStatus) {
        if let Err(err) = self.carts.set_status(cue_id, status) {
            tracing::warn!(%cue_id, %err, "status update rejected by state machine");
        }
    }

    /// Status anwenden + Auto-Follow („Cue Sequences": bei `Completed`
    /// feuert der nächste Cue, wenn dieser auf Auto-Follow steht).
    fn apply_status_with_follow(&mut self, cue_id: Uuid, status: CueStatus) {
        let (result, follow_commands) = self.carts.set_status_with_follow(cue_id, status);
        if let Err(err) = result {
            tracing::warn!(%cue_id, %err, "status update rejected by state machine");
        }
        self.dispatch(follow_commands);
    }

    /// Globale Hotkeys (TODO.md Phase 5): liest die zentrale
    /// Tabelle [`shortcuts::ALL`] – Space = GO, Esc = Panic, ↑/↓ = Playhead,
    /// ⌘/⌥⌘+↑/↓ = Cue verschieben, ⌘0–9 = neue Cues, ⌘F/⌘G = Find,
    /// ⌘D/⌘G = Duplicate/Find Next, ⌘I/L/K = Panels, ⌘N/O/S/⇧⌘S/Q = Datei.
    /// Public für headless GUI-Tests (tests/gui_interaction.rs).
    ///
    /// Gate: Nur ein fokussiertes **Textfeld** blockiert Space/Esc (dort
    /// sollen die Tasten im Feld wirken). egui 0.30 kann das nicht selbst
    /// erkennen (Slider fordern genau wie Textfelder Fokus beim Klick),
    /// daher registrieren die Zeichenfunktionen ihre Textfeld-IDs in
    /// [`ShowApp::text_field_ids`] und der Gate-Wert wird pro Frame in
    /// [`ShowApp::text_field_active`] gecacht (Esc löscht den Fokus bereits
    /// in egui's `begin_pass`, bevor die Hotkeys laufen – eine Live-Abfrage
    /// wäre auf der Esc-Frame daher immer leer). Pfeiltasten blockiert
    /// zusätzlich JEDES fokussierte Widget, weil Slider/Textfelder Pfeile
    /// selbst konsumieren (Wert/Cursor ändern).
    pub fn handle_hotkeys(&mut self, ctx: &egui::Context) {
        let text_input = self.text_field_active;
        let any_focus = ctx.memory(|m| m.focused().is_some());
        // WICHTIG: Innerhalb von `ctx.input` darf KEINE andere
        // `ctx`-Methode aufgerufen werden (`Context::input` hält den
        // Write-Lock – `ctx.memory_mut`/`send_viewport_cmd` im Closure
        // deadlocken). Daher: erst matchende Aktionen sammeln, dann
        // außerhalb des Closures dispatchen.
        let mut matched: Vec<ShortcutAction> = Vec::new();
        ctx.input(|i| {
            // Eine zentrale Tabelle (TODO.md Phase 5): Menüanzeige,
            // Shortcuts-Fenster und dieses Verhalten lesen alle aus
            // `shortcuts::ALL` – so kann nichts auseinanderlaufen.
            for sc in shortcuts::ALL {
                let Some(key) = sc.key else { continue };
                if !i.key_pressed(key) || !sc.matches(key, &i.modifiers) {
                    continue;
                }
                matched.push(sc.action);
            }
        });
        // Bei aktiver Device-Ausgabe beendet Esc die Vollbild-Ausgabe statt
        // Panic: Die Ausgabe liegt über allem, sonst käme man nicht mehr
        // zurück (Esc im Ausgabefenster selbst schließt es ebenfalls).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && self.output_mode == OutputMode::Displays
            && !text_input
        {
            self.set_output_mode(OutputMode::Off);
            // PanicAll (Esc) in diesem Fall nicht zusätzlich auslösen.
            matched.retain(|a| !matches!(a, ShortcutAction::PanicAll));
        }
        let mut consume_space = false;
        for action in matched {
            let editable = !self.show_mode;
            match action {
                // Space = GO – auch wenn z. B. ein Slider fokussiert ist;
                // nur Textfelder (Tippen) blockieren. Das Event
                // wird konsumiert, damit ein fokussierter Button
                // (Tab-Fokus) nicht zusätzlich feuert (kein Doppel-GO).
                ShortcutAction::Go => {
                    if text_input {
                        continue;
                    }
                    self.pending_go = true;
                    consume_space = true;
                }
                // Esc = Panic – nicht während Texteingabe (Esc beendet
                // dort die Eingabe und räumt den Fokus auf).
                ShortcutAction::PanicAll => {
                    if text_input {
                        continue;
                    }
                    self.panic();
                }
                // Nackte Klammern: Pause All / Resume All –
                // nicht während Texteingabe.
                ShortcutAction::PauseAll => {
                    if text_input {
                        continue;
                    }
                    self.pause_all();
                }
                ShortcutAction::ResumeAll => {
                    if text_input {
                        continue;
                    }
                    self.resume_all();
                }
                ShortcutAction::ResetAll => {
                    if text_input {
                        continue;
                    }
                    self.reset_all();
                }
                // Pfeile: Playhead bewegen (nackt) bzw. selektierten Cue
                // verschieben (⌘/⌥⌘). Pfeiltasten blockiert zusätzlich
                // JEDES fokussierte Widget (Slider/Textfelder konsumieren
                // Pfeile selbst).
                ShortcutAction::PlayheadUp => {
                    if any_focus {
                        continue;
                    }
                    let c = self.carts.cursor().saturating_sub(1);
                    self.carts.set_cursor(c);
                }
                ShortcutAction::PlayheadDown => {
                    if any_focus {
                        continue;
                    }
                    self.carts.set_cursor(self.carts.cursor() + 1);
                }
                ShortcutAction::MoveCueUp => {
                    if any_focus || !editable {
                        continue;
                    }
                    self.move_selected(-1);
                }
                ShortcutAction::MoveCueDown => {
                    if any_focus || !editable {
                        continue;
                    }
                    self.move_selected(1);
                }
                // ⌘0–⌘9: neuen Cue des Typs anlegen (Cues-Menü).
                ShortcutAction::NewCue(cue_type) => {
                    if !editable {
                        continue;
                    }
                    self.add_cue(cue_type);
                }
                ShortcutAction::ToggleInspector => {
                    self.ui.inspector_visible = !self.ui.inspector_visible;
                }
                ShortcutAction::ToggleSidebar => {
                    self.ui.sidebar_visible = !self.ui.sidebar_visible;
                }
                ShortcutAction::ToggleToolbox => {
                    self.ui.toolbox_visible = !self.ui.toolbox_visible;
                }
                ShortcutAction::NewShow => self.new_show(),
                ShortcutAction::OpenShow => self.open_show_dialog(ctx),
                ShortcutAction::SaveShow => self.save_show_dialog(ctx),
                ShortcutAction::SaveShowAs => self.save_show_as_dialog(ctx),
                ShortcutAction::WorkspaceSettings => {
                    self.ui.workspace_settings_open = true;
                }
                ShortcutAction::Quit => self.quit(ctx),
                // ⌘F: Find-Leiste toggeln + Fokus; ⌘G: nächster Treffer.
                ShortcutAction::Find => {
                    // F1.6: Find ist im Show-Mode gesperrt.
                    if self.show_mode {
                        continue;
                    }
                    self.ui.find.open = !self.ui.find.open;
                    if self.ui.find.open {
                        ctx.memory_mut(|m| m.request_focus(cue_table::find_bar_id()));
                    } else {
                        self.ui.find.query.clear();
                        self.ui.find.current = 0;
                    }
                }
                ShortcutAction::FindNext => {
                    if !self.ui.find.open {
                        self.ui.find.open = true;
                        ctx.memory_mut(|m| m.request_focus(cue_table::find_bar_id()));
                    }
                    self.find_next();
                }
                // ⌫/Delete/⌘⌫: selektierten Cue löschen – nur Edit-Mode
                // und nie während Texteingabe/anderem Fokus.
                ShortcutAction::DeleteCue => {
                    if text_input || any_focus || !editable {
                        continue;
                    }
                    self.delete_selected();
                }
                ShortcutAction::DuplicateCues => {
                    if !editable {
                        continue;
                    }
                    let ids = self.carts.active().selection().to_vec();
                    if !ids.is_empty() {
                        self.duplicate_cues(&ids);
                    }
                }
                ShortcutAction::GroupCues => {
                    if !editable {
                        continue;
                    }
                    let ids = self.carts.active().selection().to_vec();
                    if !ids.is_empty() {
                        self.group_cues(&ids);
                    }
                }
            }
        }
        if consume_space {
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key {
                            key: Key::Space,
                            pressed: true,
                            ..
                        }
                    )
                });
            });
        }
    }

    /// Find (⌘F/⌘G, TODO.md Phase 5): nächster Treffer – Playhead +
    /// Selektion springen dorthin (Auto-Scroll scrollt zur Cursor-Zeile).
    /// Public für headless GUI-Tests.
    pub fn find_next(&mut self) {
        let list = self.carts.active_mut();
        cue_table::find_next_hit(list, &mut self.ui.find);
    }

    /// Führt ein ausstehendes Space-GO aus (in `update` nach den Panels,
    /// damit Hotkeys nicht mit fokussierten Buttons kollidieren).
    /// Public für headless GUI-Tests (tests/gui_interaction.rs).
    pub fn flush_pending_go(&mut self) {
        if self.pending_go {
            self.pending_go = false;
            self.go_now();
        }
    }

    /// `true`, wenn aktuell ein Textfeld fokussiert ist (Hotkey-Gate:
    /// Textfelder sollen die Tasten selbst bekommen). Nutzt die
    /// Textfeld-Registrierung des vorherigen Frames (siehe
    /// [`ShowApp::text_field_ids`]) – plus den Zellen-Editor der Cue-Tabelle
    /// (TODO.md Phase 3: Esc bricht den Inline-Edit ab statt Panic).
    pub fn text_field_focused(&self, ctx: &egui::Context) -> bool {
        let Some(focused) = ctx.memory(|m| m.focused()) else {
            return false;
        };
        if self.text_field_ids_prev.contains(&focused) {
            return true;
        }
        ctx.data_mut(|d| d.get_temp::<egui::Id>(super::cue_table::cell_edit_active_id()))
            .is_some_and(|edit_id| edit_id == focused)
    }

    /// Übernimmt die Textfeld-Registrierung: das aktuelle Frame wird zum
    /// „vorherigen" (die Hotkeys lesen die Registrierung des Vor-Frames –
    /// ein Frame Verzögerung ist unkritisch, da Felder vor dem Anklicken
    /// bereits existieren müssen). Public für headless GUI-Tests.
    pub fn swap_text_field_registry(&mut self) {
        std::mem::swap(&mut self.text_field_ids_prev, &mut self.text_field_ids);
        self.text_field_ids.clear();
    }

    /// Zeichnet den kompletten Workspace (Panels + Fenster) in der
    /// verbindlichen Reihenfolge aus docs/UI.md §2:
    /// `menu_bar → masthead → footer → inspector → toolbox → sidebar →
    /// cue list`. Bei egui ist das zuerst hinzugefügte Bottom-Panel das
    /// UNTERSTE – deshalb Footer vor Inspector (Footer = unterste Zeile).
    ///
    /// Public, damit die headless GUI-Tests exakt dasselbe Layout rendern.
    pub fn draw_workspace(&mut self, ctx: &egui::Context) {
        use theme::{palette, size};

        #[cfg(target_os = "macos")]
        let native_menu = self.native_menu.is_some();
        #[cfg(not(target_os = "macos"))]
        let native_menu = false;
        if !native_menu {
            egui::TopBottomPanel::top("menu_bar")
                .frame(
                    egui::Frame::none()
                        .fill(palette::BACKGROUND)
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0)),
                )
                .show(ctx, |ui| {
                    menu_bar::draw(ui, self);
                });
        }
        if self.ui.masthead_visible {
            // Show-Mode sofort erkennbar: roter Rahmen um den Masthead.
            let mut masthead_frame =
                theme::panel_frame(size::SPACE_MEDIUM_LARGE, size::SPACE_MEDIUM_LARGE);
            if self.show_mode {
                masthead_frame = masthead_frame
                    .stroke(egui::Stroke::new(2.0_f32, palette::PANIC))
                    .fill(palette::SHOW_MODE_BG);
            }
            // F1.5: Masthead-Höhe per Drag am unteren Rand änderbar (persistiert).
            let inner = egui::TopBottomPanel::top("masthead")
                .frame(masthead_frame)
                .exact_height(self.ui.masthead_height.clamp(48.0, 320.0))
                .resizable(true)
                .min_height(48.0)
                .max_height(320.0)
                .show(ctx, |ui| {
                    masthead::draw(ui, self);
                });
            // Tatsächliche Höhe persistieren (Drag des Panel-Rands).
            let h = inner.response.rect.height();
            if (h - self.ui.masthead_height).abs() > 0.5 {
                self.ui.masthead_height = h.clamp(48.0, 320.0);
            }
        }
        // Footer ZUERST unter den Bottom-Panels → unterste Zeile.
        let mut footer_frame = theme::panel_frame(size::SPACE_MEDIUM, 2.0);
        if self.show_mode {
            footer_frame = footer_frame.fill(palette::SHOW_MODE_BG);
        }
        egui::TopBottomPanel::bottom("footer")
            .frame(footer_frame)
            .exact_height(size::FOOTER_HEIGHT)
            .resizable(false)
            .show(ctx, |ui| {
                footer::draw(ui, self);
            });
        if self.ui.inspector_visible {
            egui::TopBottomPanel::bottom("inspector")
                .frame(theme::panel_frame(
                    size::SPACE_LARGE,
                    size::SPACE_MEDIUM_LARGE,
                ))
                .resizable(true)
                .min_height(120.0)
                .default_height(size::INSPECTOR_HEIGHT)
                .show(ctx, |ui| {
                    inspector::draw(ui, self);
                });
        }
        if self.ui.toolbox_visible {
            egui::SidePanel::left("toolbox")
                .frame(theme::panel_frame(size::SPACE_SMALL, size::SPACE_MEDIUM))
                .resizable(true)
                .default_width(size::TOOLBOX_WIDTH)
                .width_range(110.0..=220.0)
                .show(ctx, |ui| {
                    toolbox::draw(ui, self);
                });
        }
        if self.ui.sidebar_visible {
            egui::SidePanel::right("sidebar")
                .frame(theme::panel_frame(
                    size::SPACE_MEDIUM_LARGE,
                    size::SPACE_MEDIUM,
                ))
                .resizable(true)
                .default_width(size::SIDEBAR_WIDTH)
                .width_range(180.0..=420.0)
                .show(ctx, |ui| {
                    sidebar::draw(ui, self);
                });
        }

        // CentralPanel IMMER zuletzt (egui-Regel) – claimt den Restplatz.
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(palette::LIST_BG))
            .show(ctx, |ui| {
                // Find-Leiste über der Cue-Liste (TODO.md Phase 5, ⌘F).
                if self.ui.find.open {
                    cue_table::draw_find_bar(
                        ui,
                        self.carts.active_mut(),
                        &mut self.ui.find,
                        &mut self.text_field_ids,
                    );
                }
                // Anzeige-Metadaten (Broken-✕, Fortschrittsbalken,
                // Pre-Wait-Countdown) vor dem Zeichnen bauen (Phase 2).
                let row_meta = self.cue_row_meta();
                // Tabelle zeichnen; strukturelle Aktionen (Löschen,
                // Duplizieren, Gruppieren, Drag & Drop) werden NACH dem
                // Rendern ausgeführt (TODO.md Phase 3).
                let actions = cue_table::draw(
                    ui,
                    self.carts.active_mut(),
                    !self.show_mode,
                    &row_meta,
                    &mut self.ui.cue_columns,
                    &self.ui.find,
                );
                for id in &actions.delete {
                    // Löschen stoppt laufende Cues vorher (Engine `Stop`).
                    self.delete_cue_by_id(*id);
                }
                if let Some((from, to)) = actions.move_to {
                    self.carts.active_mut().move_to(from, to);
                }
                if let Some((cue, group)) = actions.move_into_group {
                    self.carts.active_mut().move_into_group(cue, group);
                }
                if !actions.duplicate.is_empty() {
                    self.duplicate_cues(&actions.duplicate);
                }
                if !actions.group.is_empty() {
                    self.group_cues(&actions.group);
                }
                if let Some((ids, armed)) = &actions.arm {
                    self.set_cues_armed(ids, *armed);
                }
                if let Some((ids, color)) = &actions.color {
                    self.set_cues_color(ids, *color);
                }
            });

        // About-Fenster (echtes OS-Fenster, in Headless-Tests Overlay-Fallback).
        if self.ui.about_open {
            let mut open = true;
            super::viewport_window::draw(
                ctx,
                &super::viewport_window::WindowSpec::new(
                    "kuhplay-about",
                    "About KuhPlay",
                    [360.0, 200.0],
                    [280.0, 150.0],
                )
                .resizable(false)
                .show_mode_badge(self.show_mode),
                &mut open,
                self,
                |_ctx, ui, app| {
                    ui.label(RichText::new("KuhPlay").size(24.0).strong());
                    ui.label("Show control & cue playback – Rust/egui.");
                    ui.separator();
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    if ui.button("Close").clicked() {
                        app.ui.about_open = false;
                    }
                },
            );
            if !open {
                self.ui.about_open = false;
            }
        }

        // „Keyboard Shortcuts"-Fenster (Help-Menü, TODO.md Phase 5):
        // rendert dieselbe zentrale Tabelle wie Menüs und Hotkeys.
        shortcuts::draw_window(ctx, &mut self.ui.shortcuts_open);

        // „File Target Search" (Tools-Menü, Workflow-Tool).
        target_search::draw(ctx, self);

        // „Workspace Status" (Tools-Menü, Status-Übersicht).
        status_window::draw(ctx, self);

        // Audition-Fenster (Workspace-Menü).
        audition_window::draw(ctx, self);

        stage_view::draw_route_previews(self, ctx);

        // Workspace-Settings-Fenster (Workspace → Workspace Settings;
        // Screens/Stages auf der Seite „Video").
        super::workspace_settings::draw(ctx, self);
        // Stage-Editor, Route-Editor und Multi-Output-Assistent (Video).
        super::stage_editor::draw(ctx, self);
        super::workspace_settings::draw_route_editor(ctx, self);
        super::workspace_settings::draw_multi_output_wizard(ctx, self);
    }

    /// Aktualisiert den gecachten Hotkey-Gate-Wert „Textfeld fokussiert"
    /// (am Ende von `update` aufgerufen, nach dem Rendern der Panels).
    /// Public für headless GUI-Tests.
    pub fn refresh_text_field_gate(&mut self, ctx: &egui::Context) {
        self.text_field_active = self.text_field_focused(ctx);
    }
}

/// macOS: Ctrl-Klick ist dort ein Rechtsklick, egui kennt das aber nicht. Wandelt
/// den Klick um. `active` merkt sich das Drücken, denn beim Loslassen kann Ctrl
/// schon wieder gelöst sein.
pub fn ctrl_click_to_secondary(events: &mut [egui::Event], active: &mut bool) {
    for event in events {
        if let egui::Event::PointerButton {
            button,
            pressed,
            modifiers,
            ..
        } = event
        {
            if *button != egui::PointerButton::Primary {
                continue;
            }
            if *pressed {
                *active = modifiers.ctrl;
            }
            if *active {
                *button = egui::PointerButton::Secondary;
            }
            if !*pressed {
                *active = false;
            }
        }
    }
}

impl eframe::App for ShowApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if cfg!(target_os = "macos") {
            ctrl_click_to_secondary(&mut raw_input.events, &mut self.ctrl_click_active);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Theme wird einmalig in `main` (CreationContext) gesetzt – nicht
        // jeden Frame (TODO.md Phase 0b).
        self.swap_text_field_registry();
        self.poll_engine_events();
        self.poll_remote_commands();
        self.handle_hotkeys(ctx);

        // Datei-Drop aus dem Finder (TODO.md Phase 6): Audio-/Video-/
        // Bild-Dateien legen passende Cues im aktiven Cart an.
        let dropped: Vec<egui::DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());
        for file in &dropped {
            if let Some(path) = &file.path {
                self.add_cue_from_file(path);
            }
        }

        #[cfg(target_os = "macos")]
        self.poll_native_menu(ctx);
        self.draw_workspace(ctx);

        // Stage-Vorschau-Fenster („Stage Window") und Vollbild-Screen-
        // Ausgaben (borderless auf dem zugeordneten Display).
        stage_view::draw_stage_windows(self, ctx);
        stage_view::draw_device_outputs(self, ctx);

        // Optionaler Auto-Demo-Modus: automatische GO-Abfolge für Screen-Tests.
        if let Some(demo) = &mut self.autodemo {
            if demo.count < 4 && Instant::now() >= demo.next_go {
                demo.count += 1;
                demo.next_go = Instant::now() + Duration::from_secs(3);
                self.pending_go = true;
            }
        }

        self.flush_pending_go();

        // Hotkey-Gate für das nächste Frame aktualisieren (nach den Panels,
        // damit Klicks des aktuellen Frames berücksichtigt sind).
        self.refresh_text_field_gate(ctx);

        self.pump_remote();

        // Repaint für Live-Updates (Engine-Events, Timecode, elapsed-Anzeige).
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// Persistiert das UI-Layout (Panel-Sichtbarkeit, Tabs, Fenster) beim
    /// Beenden (Workspace bleibt über Neustarts erhalten).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_ui_state(storage);
        self.save_ui_templates(storage);
    }
}
