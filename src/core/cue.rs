//! Domain-Model eines einzelnen [`Cue`]s.
//!
//! Ein Cue beschreibt *was* passieren soll (Typ, Ziel, Zeiten, Loops) und *in
//! welchem Zustand* er sich befindet. Er ist die zentrale Einheit der Show.
//!
//! ## Erweiterung
//!
//! Zusätzliche Cue-Typen werden über [`CueType`] und eine neue Branche in
//! [`Cue::build_trigger_command`] ergänzt. Persistierte Shows profitieren
//! automatisch, weil [`Cue`] vollständig über `serde` serialisierbar ist.
//! `duration_ms` (Serde für [`Duration`] als ms) lebt in [`crate::core`].

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::audio::{AudioRoute, FadeCurve, FadeParam, MediaSlice};
use crate::core::error::{ShowError, ShowResult};
use crate::core::html::HtmlSource;
use crate::core::slideshow::SlideShow;
use crate::core::stage::SurfaceGeometry;
use crate::core::timecode::Timecode;
use crate::engine::EngineCommand;

/// Kategorisiert einen Cue. Bestimmt Icon, Label und das erzeugte Engine-Kommando.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CueType {
    /// Audio-Wiedergabe (Datei oder Stream).
    Audio,
    /// Video-Wiedergabe (Datei; Hardware-Out via GStreamer-Plugins).
    Video,
    /// Parameter-Fade über [`Cue::duration`] (Volume und/oder Opacity des
    /// Ziel-Cues, Kurve linear/dB; siehe docs/AUFTRAG-fade.md).
    Fade,
    /// Crossfade: „Nach" startet mit Volume/Opacity 0 und blendet ein, während
    /// „Von" ausblendet (Equal-Power am Ton) – ein GO, ein Übergang.
    Crossfade,
    /// Container für Kinder-Cues (Group/QFolder).
    Group,
    /// Stoppt alle laufenden Playbacks (Blackout / Panic).
    Stop,
    /// Platzhalter für Script-/MIDI-/OSC-Cues.
    Script,
    /// Steuer-Cue: Start/Stop/Pause/Load/Reset/Devamp/GoTo/Arm/Disarm/
    /// NextSlide/PrevSlide.
    Control,
    /// HTML5-Cue: WebView rendert HTML/JS/CSS auf einer Video-Stage;
    /// JavaScript-Trigger und JSON-Pushes siehe [`EngineCommand::EvalJs`]
    /// bzw. [`EngineCommand::PushJson`].
    Html,
    /// Diashow-Cue: Fotoliste mit Hold-Zeiten, Crossfades und Auto-Advance
    /// (manuelles Schalten via Control-Cues `NextSlide`/`PrevSlide`).
    SlideShow,
    /// Daten-Cue: pusht einen JSON-Payload an die WebView eines aktiven
    /// HTML5-Cues (dynamische Lower-Thirds, Spielstände, Countdowns).
    HtmlData,
    /// Wait-Cue: tut nichts, wartet nur seine `duration` ab –
    /// reine Timer-Unit in der Engine ([`EngineCommand::Wait`]).
    Wait,
    /// Memo-Cue: zeigt nur seine Notiz an, tut sonst nichts
    /// (kein Engine-Kommando, gilt sofort als abgeschlossen).
    Memo,
    /// Text-Cue: rendert Text ([`TextContent`]) als Inline-HTML
    /// über die HTML5-Pipeline auf einer Video-Stage.
    Text,
    /// Network-Cue: sendet eine Nachricht per UDP/TCP an
    /// `host:port` ([`EngineCommand::SendNetwork`]) – Sofort-Cue.
    Network,
    /// MIDI-Cue: sendet eine MIDI-Nachricht an das Ausgabegerät
    /// ([`EngineCommand::SendMidi`]) – Sofort-Cue.
    Midi,
    /// Light-Cue (Minimal-Variante): sendet ein Art-Net/sACN-DMX-Paket
    /// ([`LightMessage`]) per UDP ins Licht-Netz – Sofort-Cue.
    Light,
    /// Companion-Cue: triggert Bitfocus Companion über dessen HTTP-API
    /// (Button press/down/up, [`CompanionTrigger`]) – Sofort-Cue.
    Companion,
    /// Timecode-Cue: feuert, wenn der externe Timecode (LTC/MTC/OSC
    /// `/kuhplay/timecode`) seinen Trigger [`Cue::tc_trigger`] erreicht.
    /// Manuelles GO: Wait mit `duration` (0 = Sofort-Cue).
    Timecode,
}

impl CueType {
    /// Alle Cue-Typen (z. B. für ComboBoxen im Inspector).
    pub const ALL: [CueType; 19] = [
        CueType::Audio,
        CueType::Video,
        CueType::Fade,
        CueType::Crossfade,
        CueType::Group,
        CueType::Stop,
        CueType::Script,
        CueType::Control,
        CueType::Html,
        CueType::SlideShow,
        CueType::HtmlData,
        CueType::Wait,
        CueType::Memo,
        CueType::Text,
        CueType::Network,
        CueType::Midi,
        CueType::Light,
        CueType::Companion,
        CueType::Timecode,
    ];

    /// Symbol für Tabellenzellen.
    pub fn icon(self) -> &'static str {
        match self {
            CueType::Audio => "♪",
            CueType::Video => "▶",
            CueType::Fade => "◐",
            CueType::Crossfade => "⇄",
            CueType::Group => "▣",
            CueType::Stop => "■",
            CueType::Script => "✎",
            CueType::Control => "◆",
            CueType::Html => "🌐",
            CueType::SlideShow => "🖼",
            CueType::HtmlData => "📊",
            CueType::Wait => "⏳",
            CueType::Memo => "📝",
            CueType::Text => "🅣",
            CueType::Network => "📡",
            CueType::Midi => "🎹",
            CueType::Light => "💡",
            CueType::Companion => "🎛",
            CueType::Timecode => "⏱",
        }
    }

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            CueType::Audio => "Audio",
            CueType::Video => "Video",
            CueType::Fade => "Fade",
            CueType::Crossfade => "Crossfade",
            CueType::Group => "Group",
            CueType::Script => "Script",
            CueType::Stop => "Stop",
            CueType::Control => "Control",
            CueType::Html => "HTML5",
            CueType::SlideShow => "Slideshow",
            CueType::HtmlData => "HTML Data",
            CueType::Wait => "Wait",
            CueType::Memo => "Memo",
            CueType::Text => "Text",
            CueType::Network => "Network",
            CueType::Midi => "MIDI",
            CueType::Light => "Light",
            CueType::Companion => "Companion",
            CueType::Timecode => "Timecode",
        }
    }
}

/// Ordnet einer Datei anhand der Extension einen passenden Cue-Typ zu
/// (docs/TODO-offen.md Phase 6: Drag & Drop aus dem Finder legt passende Cues an).
///
/// Audio-Endungen → Audio-Cue, Video-Endungen → Video-Cue, Bild-Endungen
/// → Diashow-Cue (ein Bild = eine Slide). Unbekannt → `None`.
pub fn cue_type_for_file(path: &std::path::Path) -> Option<CueType> {
    const AUDIO: &[&str] = &[
        "mp3", "wav", "aif", "aiff", "m4a", "flac", "ogg", "opus", "wma", "mpga", "aac", "ac3",
        "eac3", "dts", "mka", "mp2", "wv", "ape",
    ];
    const VIDEO: &[&str] = &[
        "mp4", "mov", "mkv", "avi", "webm", "m4v", "mpg", "mpeg", "wmv", "flv", "3gp", "ts", "mts",
        "m2ts", "mxf", "ogv", "vob", "m2v", "asf",
    ];
    const IMAGE: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "heic",
    ];
    let ext = path.extension()?.to_str()?.to_lowercase();
    if AUDIO.contains(&ext.as_str()) {
        Some(CueType::Audio)
    } else if VIDEO.contains(&ext.as_str()) {
        Some(CueType::Video)
    } else if IMAGE.contains(&ext.as_str()) {
        Some(CueType::SlideShow)
    } else {
        None
    }
}

/// Kaskadierungs-Modus eines Group-Cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GroupMode {
    /// Alle armed Kinder starten gleichzeitig (ein GO triggert alle Commands).
    #[default]
    Simultaneous,
    /// Kinder werden der Reihe nach in die Cue-Liste eingespielt
    /// (jedes GO triggert das nächste Kind).
    Sequential,
    /// Timeline: wie [`GroupMode::Simultaneous`], aber die Kinder staffeln sich
    /// über ihre individuellen Pre-Waits (zeitversetzter Ablauf).
    Timeline,
    /// Wie [`GroupMode::Sequential`], aber die Kinder-Kette wiederholt sich
    /// entsprechend [`Cue::loop_mode`] (Loop-Gruppe).
    Loop,
    /// Wie [`GroupMode::Sequential`], aber die Kinder-Reihenfolge wird beim
    /// Triggern deterministisch (seed-basiert) gemischt.
    Shuffle,
}

impl GroupMode {
    /// Alle Modi (für UI-ComboBoxen).
    pub const ALL: [GroupMode; 5] = [
        GroupMode::Simultaneous,
        GroupMode::Sequential,
        GroupMode::Timeline,
        GroupMode::Loop,
        GroupMode::Shuffle,
    ];

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            GroupMode::Simultaneous => "Simultaneous",
            GroupMode::Sequential => "Sequential",
            GroupMode::Timeline => "Timeline",
            GroupMode::Loop => "Loop",
            GroupMode::Shuffle => "Shuffle",
        }
    }
}

/// Aktion eines Control-Cues ([`CueType::Control`]).
///
/// Ziel-Cues werden über [`Cue::control_target`] (Cue-Nummer) adressiert;
/// ohne Ziel gilt die Aktion für *alle* laufenden Cues bzw. den Cue selbst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControlAction {
    /// Startet den Ziel-Cue direkt (unabhängig vom Playhead).
    #[default]
    Start,
    /// Stoppt den Ziel-Cue (ohne Ziel: alle laufenden Cues).
    Stop,
    /// Pausiert den Ziel-Cue (ohne Ziel: alle laufenden Cues).
    Pause,
    /// Lädt den Ziel-Cue vor (Preload, ohne Wiedergabe).
    Load,
    /// Reset der gesamten Cue-Liste (Playhead + alle Status → Standby).
    Reset,
    /// Devamp: beendet Loops sofort und springt zum Ende (Outro).
    Devamp,
    /// GoTo (Target): setzt den Playhead auf den Ziel-Cue.
    GoTo,
    /// Armed den Ziel-Cue (ohne Ziel: sich selbst).
    Arm,
    /// Disarmed den Ziel-Cue (ohne Ziel: sich selbst); GO überspringt ihn.
    Disarm,
    /// Schaltet einen Diashow-Cue ([`CueType::SlideShow`]) ein Dia weiter.
    NextSlide,
    /// Schaltet einen Diashow-Cue ein Dia zurück.
    PrevSlide,
}

impl ControlAction {
    /// Alle Aktionen (für UI-ComboBoxen).
    pub const ALL: [ControlAction; 11] = [
        ControlAction::Start,
        ControlAction::Stop,
        ControlAction::Pause,
        ControlAction::Load,
        ControlAction::Reset,
        ControlAction::Devamp,
        ControlAction::GoTo,
        ControlAction::Arm,
        ControlAction::Disarm,
        ControlAction::NextSlide,
        ControlAction::PrevSlide,
    ];

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            ControlAction::Start => "Start",
            ControlAction::Stop => "Stop",
            ControlAction::Pause => "Pause",
            ControlAction::Load => "Load",
            ControlAction::Reset => "Reset",
            ControlAction::Devamp => "Devamp",
            ControlAction::GoTo => "GoTo (Target)",
            ControlAction::Arm => "Arm",
            ControlAction::Disarm => "Disarm",
            ControlAction::NextSlide => "Next Slide",
            ControlAction::PrevSlide => "Prev Slide",
        }
    }
}

/// Continue-Mode eines Cues („Continue Mode", letzte Spalte der
/// Cue-Liste).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ContinueMode {
    /// Cue endet, nächster Cue startet nicht automatisch (Standard).
    #[default]
    Off,
    /// Auto-Continue: Beim Start dieses Cues startet der folgende Cue mit
    /// (ggf. nach diesem Cue's Post-Wait – V1: sofort).
    AutoContinue,
    /// Auto-Follow: Startet den folgenden Cue, sobald dieser Cue endet.
    AutoFollow,
}

impl ContinueMode {
    /// Alle Modi (für UI-ComboBoxen).
    pub const ALL: [ContinueMode; 3] = [
        ContinueMode::Off,
        ContinueMode::AutoContinue,
        ContinueMode::AutoFollow,
    ];

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            ContinueMode::Off => "Off",
            ContinueMode::AutoContinue => "Auto-Continue",
            ContinueMode::AutoFollow => "Auto-Follow",
        }
    }

    /// Spalten-Icon (Symbole statt Text).
    pub fn icon(self) -> &'static str {
        match self {
            ContinueMode::Off => "·",
            ContinueMode::AutoContinue => "⏩",
            ContinueMode::AutoFollow => "⏭",
        }
    }
}

/// Loop-Modus der Wiedergabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LoopMode {
    /// Einmalig abspielen (Standard).
    #[default]
    Once,
    /// Genau `n` Mal abspielen.
    Count(u32),
    /// Endlos wiederholen (bis Stop).
    Infinite,
}

impl LoopMode {
    /// Anzeige-Label.
    pub fn label(self) -> String {
        match self {
            LoopMode::Once => "Once".to_string(),
            LoopMode::Count(n) => format!("{n}×"),
            LoopMode::Infinite => "∞".to_string(),
        }
    }

    /// Gesamtzahl der Wiedergaben (`Infinite` → `u32::MAX` als Sentinel).
    pub fn total_iterations(self) -> u32 {
        match self {
            LoopMode::Once => 1,
            LoopMode::Count(n) => n.max(1),
            LoopMode::Infinite => u32::MAX,
        }
    }

    /// `true` bei Endlosschleife.
    pub fn is_infinite(self) -> bool {
        matches!(self, LoopMode::Infinite)
    }
}

/// Lebenszyklus eines Cues (State-Pattern).
///
/// Deterministischer Ablauf:
///
/// ```text
/// Standby ──GO──▶ Pre-Wait ──▶ Playing ◀──▶ Paused
///                 (pre_wait>0)   │  │
///                                │  │ Finished (post_wait>0)
///                                │  ▼
///                                │  Post-Wait ──▶ Completed
///                                │                ▲
///                                └── (post_wait=0)─┘
///
///  Jederzeit (Operator-Override: Reset/Panic):  * ──▶ Standby
///  Stop: Pre-Wait/Playing/Paused/Post-Wait ──▶ Stopped
///  Fehler: Standby/Pre-Wait/Playing/Paused/Post-Wait ──▶ Error
/// ```
///
/// Gültige Übergänge werden von [`CueStatus::can_transition_to`] definiert und
/// von [`Cue::transition_status`] erzwungen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CueStatus {
    /// Bereit zum Triggern.
    #[default]
    Standby,
    /// Pre-Wait läuft (GO empfangen, Engine startet noch nicht).
    PreWait,
    /// Wiedergabe läuft.
    Playing,
    /// Wiedergabe pausiert.
    Paused,
    /// Post-Wait läuft (Playback beendet, Cue noch nicht abgeschlossen).
    PostWait,
    /// Cue ist abgelaufen (natürliches Ende oder Sofort-Cue).
    Completed,
    /// Cue wurde gestoppt (Panic/Stop-Cue); wartet auf Reset.
    Stopped,
    /// Fehlerzustand (Engine-Error, fehlende Datei, …); Reset erholt.
    Error,
}

impl CueStatus {
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            CueStatus::Standby => "Standby",
            CueStatus::PreWait => "Pre-Wait",
            CueStatus::Playing => "Playing",
            CueStatus::Paused => "Paused",
            CueStatus::PostWait => "Post-Wait",
            CueStatus::Completed => "Completed",
            CueStatus::Stopped => "Stopped",
            CueStatus::Error => "Error",
        }
    }

    /// Prüft, ob ein Zustandsübergang `self -> next` erlaubt ist.
    /// Identische Zustände (idempotent) sind immer erlaubt.
    pub fn can_transition_to(self, next: CueStatus) -> bool {
        use CueStatus::*;
        if self == next {
            return true;
        }
        // Operator-Override: Reset/Panic darf jederzeit nach Standby zwingen.
        if next == Standby {
            return true;
        }
        matches!(
            (self, next),
            (Standby, PreWait)
                | (Standby, Playing)
                | (Standby, Completed)
                | (Standby, Error)
                | (PreWait, Playing)
                | (PreWait, Stopped)
                | (PreWait, Error)
                | (Playing, Paused)
                | (Playing, PostWait)
                | (Playing, Completed)
                | (Playing, Stopped)
                | (Playing, Error)
                | (Paused, Playing)
                | (Paused, Stopped)
                | (Paused, Error)
                | (PostWait, Completed)
                | (PostWait, Stopped)
                | (PostWait, Error)
        )
    }

    /// `true`, wenn der Cue gerade aktiv ist (Pre-Wait/Playing/Paused/Post-Wait).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            CueStatus::PreWait | CueStatus::Playing | CueStatus::Paused | CueStatus::PostWait
        )
    }
}

fn default_armed() -> bool {
    true
}

fn default_volume() -> f32 {
    1.0
}

/// Vordefinierte Cue-Farben (Palette, RGB) – Auswahl im Inspector.
pub const CUE_COLORS: [[u8; 3]; 8] = [
    [229, 76, 60],   // rot
    [230, 126, 34],  // orange
    [241, 196, 15],  // gelb
    [39, 174, 96],   // grün
    [41, 128, 185],  // blau
    [142, 68, 173],  // lila
    [219, 68, 142],  // pink
    [127, 140, 141], // grau
];

/// Ein einzelner Cue in der Cue-Liste.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    /// Eindeutige, stabile ID (überlebt Einfügungen/Löschungen in der Liste).
    pub id: Uuid,
    /// Anzeigenummer in der Liste (z. B. `"1"`, `"1.2"`, `"Q7"`).
    pub number: String,
    /// Anzeigename.
    pub name: String,
    /// Cue-Typ.
    pub cue_type: CueType,
    /// Ziel-Datei/-Stream (Audio/Video).
    #[serde(default)]
    pub target_path: Option<String>,
    /// Wartezeit *vor* dem Trigger.
    #[serde(default, with = "crate::core::duration_ms")]
    pub pre_wait: Duration,
    /// Laufzeit des Cues bzw. Fade-Dauer (`Duration::ZERO` = "auto").
    #[serde(default, with = "crate::core::duration_ms")]
    pub duration: Duration,
    /// Wartezeit *nach* dem Trigger (vor Abschluss des Cues).
    #[serde(default, with = "crate::core::duration_ms")]
    pub post_wait: Duration,
    /// Ziel-Lautstärke für Fade-Cues (0.0 – 1.0).
    #[serde(default)]
    pub target_volume: Option<f32>,
    /// Start-Lautstärke für Fade-Cues (None = "aktuelle" Lautstärke; Mock: 1.0).
    #[serde(default)]
    pub from_volume: Option<f32>,
    /// Wiedergabe-Lautstärke (0.0 – 1.0, 1.0 = volle Lautstärke).
    /// Wird beim Triggern als `SetVolume` nach dem `Play`-Command
    /// nachgeführt, falls ≠ 1.0 (siehe [`CueList`](crate::core::CueList)).
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Ziel-Cue des Fades (Fade fadet einen Parameter eines ANDEREN Cues);
    /// `None` = der Fade-Cue selbst (altes Verhalten).
    #[serde(default)]
    pub fade_target: Option<Uuid>,
    /// Welcher Parameter gefadet wird (Volume/Rate/Opacity).
    #[serde(default)]
    pub fade_param: FadeParam,
    /// Fade-Kurve (linear oder dB; gilt nur für Volume).
    #[serde(default)]
    pub fade_curve: FadeCurve,
    /// Ziel-Lautstärke des Fades (0.0 – 1.0, None = inaktiv).
    #[serde(default)]
    pub fade_volume: Option<f32>,
    /// Ziel-Deckkraft des Fades (0.0 – 1.0, None = inaktiv).
    #[serde(default)]
    pub fade_opacity: Option<f32>,
    /// Stoppt das Ziel am Ende des Fades (vor dem Finished-Event).
    #[serde(default)]
    pub stop_when_done: bool,
    /// Crossfade: laufender Cue, der ausblendet (None = keiner).
    #[serde(default)]
    pub crossfade_from: Option<Uuid>,
    /// Crossfade: Cue, der startet (Pflicht).
    #[serde(default)]
    pub crossfade_to: Option<Uuid>,
    /// Loop-Modus der Wiedergabe.
    #[serde(default)]
    pub loop_mode: LoopMode,
    /// Continue-Mode (Auto-Continue/Auto-Follow, Cue Sequences).
    #[serde(default)]
    pub continue_mode: ContinueMode,
    /// Kill-Switch: Beim Start dieses Cues alle anderen laufenden Media-Cues
    /// (Video/Diashow/HTML) stoppen.
    #[serde(default)]
    pub kill_others: bool,
    /// Video/Diashow: Am Ende bleibt das letzte Bild stehen, bis es gestoppt oder
    /// auf demselben Layer ersetzt wird. Sonst wird das Bild schwarz.
    #[serde(default)]
    pub hold_last_frame: bool,
    /// „Vorherige Cues beenden": Beim Start dieses Cues alle
    /// laufenden Cues stoppen, die in der Liste VOR diesem Cue stehen.
    #[serde(default)]
    pub stop_previous: bool,
    /// Media-Slices: Segment-Plan mit individuellen Wiederholungszählern
    /// (überschreibt `loop_mode`/`duration` bei der Wiedergabe).
    #[serde(default)]
    pub slices: Vec<MediaSlice>,
    /// Audio-Routing (Kanäle/Pan) – Vorbereitung für Mehrkanal-Ausgabe.
    #[serde(default)]
    pub route: Option<AudioRoute>,
    /// Kaskadierungs-Modus (nur für [`CueType::Group`] relevant).
    #[serde(default)]
    pub group_mode: GroupMode,
    /// Kinder-Cues (nur für [`CueType::Group`]).
    #[serde(default)]
    pub children: Vec<Cue>,
    /// Eltern-Gruppe (wird beim Triggern gesetzt; reiner Laufzeit-Zustand).
    #[serde(default)]
    pub parent_group: Option<Uuid>,
    /// Bereits absolvierte Iterationen (nur für [`GroupMode::Loop`]).
    #[serde(default)]
    pub group_iterations_done: u32,
    /// Aktion (nur für [`CueType::Control`]).
    #[serde(default)]
    pub control_action: ControlAction,
    /// Ziel-Cue-Nummer für Control-Cues (None = "alle"/Selbst, je nach Aktion).
    #[serde(default)]
    pub control_target: Option<String>,
    /// Ziel-Stage für Video-Cues (Name).
    #[serde(default)]
    pub stage: Option<String>,
    /// Video-Layer (Reihenfolge der Überlagerung).
    #[serde(default)]
    pub layer: u32,
    /// Surface-Geometrie auf der Stage (Position/Größe, Area of Interest,
    /// Hard-Edge, Deckkraft, Sichtbarkeit) – nur Video-/Diashow-Cues.
    #[serde(default)]
    pub surface: Option<SurfaceGeometry>,
    /// Timecode-In: koppelt den Cue an eine externe Clock (LTC/MTC-Chasing).
    #[serde(default)]
    pub timecode_in: Option<Timecode>,
    /// Quelle des HTML5-Cues (Datei/URL/Inline) – nur [`CueType::Html`].
    #[serde(default)]
    pub html_source: Option<HtmlSource>,
    /// Diashow-Definition (Slides, Crossfade, Auto-Advance) – nur
    /// [`CueType::SlideShow`].
    #[serde(default)]
    pub slideshow: Option<SlideShow>,
    /// Ziel-Cue-ID eines HTML5-Cues – nur [`CueType::HtmlData`] (Targeting).
    #[serde(default)]
    pub data_target: Option<Uuid>,
    /// JSON-Payload als Text (Editor-Puffer) – nur [`CueType::HtmlData`].
    /// Wird beim Triggern geparst; ungültiges JSON → Cue-`Error` (kein Panic).
    #[serde(default)]
    pub data_json_text: String,
    /// Test-Script für die JS-Bridge (Inspector: manueller `eval_js`-Trigger).
    #[serde(default)]
    pub js_test_script: String,
    /// JavaScript-Quelltext – nur [`CueType::Script`] (läuft in eigener
    /// WebView-Instanz, siehe [`EngineCommand::RunScript`]).
    #[serde(default)]
    pub script: String,
    /// Text-Inhalt – nur [`CueType::Text`] (wird als Inline-HTML gerendert,
    /// siehe [`TextContent::to_html`]).
    #[serde(default)]
    pub text_content: Option<crate::core::text::TextContent>,
    /// Netzwerk-Nachricht – nur [`CueType::Network`].
    #[serde(default)]
    pub network: Option<crate::core::network::NetworkMessage>,
    /// MIDI-Nachricht – nur [`CueType::Midi`].
    #[serde(default)]
    pub midi: Option<crate::core::midi::MidiMessage>,
    /// Licht-Nachricht (Art-Net/sACN DMX) – nur [`CueType::Light`].
    #[serde(default)]
    pub light: Option<crate::core::light::LightMessage>,
    /// Companion-Trigger (HTTP-API) – nur [`CueType::Companion`].
    #[serde(default)]
    pub companion: Option<crate::core::companion::CompanionTrigger>,
    /// Timecode-Trigger – nur [`CueType::Timecode`]: feuert, wenn der externe
    /// Timecode diesen Wert erreicht (Eingang z. B. OSC `/kuhplay/timecode`).
    #[serde(default)]
    pub tc_trigger: Option<Timecode>,
    /// Freitext-Notizen für Operatoren.
    #[serde(default)]
    pub notes: String,
    /// Markierung für später (Taste F), wird mit der Show gespeichert.
    #[serde(default)]
    pub flagged: bool,
    /// Anzeigefarbe des Cues („Cue Color", RGB) – `None` = keine Farbe.
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// Aktueller Lebenszyklus-Zustand (wird vom Engine-Feedback gepflegt).
    #[serde(default)]
    pub status: CueStatus,
    /// "Armed" (roter Balken) – Cue wird von GO berücksichtigt.
    #[serde(default = "default_armed")]
    pub is_armed: bool,
    /// Group-Cues: Kinder ein-/ausgeklappt (Aufklapp-Dreieck,
    /// docs/TODO-offen.md Phase 3). Default: ausgeklappt.
    #[serde(default)]
    pub collapsed: bool,
}

impl Cue {
    /// Erzeugt einen neuen Cue mit frischer UUID. Default: armed, Standby.
    pub fn new(number: impl Into<String>, name: impl Into<String>, cue_type: CueType) -> Self {
        Self {
            id: Uuid::new_v4(),
            number: number.into(),
            name: name.into(),
            cue_type,
            target_path: None,
            pre_wait: Duration::ZERO,
            duration: Duration::ZERO,
            post_wait: Duration::ZERO,
            target_volume: None,
            from_volume: None,
            volume: 1.0,
            fade_target: None,
            fade_param: FadeParam::Volume,
            fade_curve: FadeCurve::Linear,
            fade_volume: None,
            fade_opacity: None,
            stop_when_done: false,
            crossfade_from: None,
            crossfade_to: None,
            loop_mode: LoopMode::Once,
            kill_others: false,
            hold_last_frame: false,
            stop_previous: false,
            continue_mode: ContinueMode::Off,
            slices: Vec::new(),
            route: None,
            group_mode: GroupMode::Simultaneous,
            children: Vec::new(),
            parent_group: None,
            group_iterations_done: 0,
            control_action: ControlAction::Start,
            control_target: None,
            stage: None,
            layer: 0,
            surface: None,
            timecode_in: None,
            html_source: None,
            slideshow: None,
            data_target: None,
            data_json_text: String::new(),
            js_test_script: String::new(),
            script: String::new(),
            text_content: None,
            network: None,
            midi: None,
            light: None,
            companion: None,
            tc_trigger: None,
            notes: String::new(),
            flagged: false,
            color: None,
            status: CueStatus::Standby,
            is_armed: true,
            collapsed: false,
        }
    }

    /// Erzeugt einen Group-Cue mit Kinder-Cues und Kaskadierungs-Modus.
    pub fn new_group(
        number: impl Into<String>,
        name: impl Into<String>,
        mode: GroupMode,
        children: Vec<Cue>,
    ) -> Self {
        let mut cue = Self::new(number, name, CueType::Group);
        cue.group_mode = mode;
        cue.children = children;
        cue
    }

    /// Builder: Ziel-Pfad setzen.
    pub fn with_target(mut self, path: impl Into<String>) -> Self {
        self.target_path = Some(path.into());
        self
    }

    /// Builder: Pre-Wait setzen.
    pub fn with_pre_wait(mut self, pre_wait: Duration) -> Self {
        self.pre_wait = pre_wait;
        self
    }

    /// Migriert alte Fade-Felder (target_volume/from_volume/fade_param) auf
    /// die neuen (fade_volume/fade_opacity). Beim Laden aufrufen.
    pub fn migrate_fade_fields(&mut self) {
        if self.fade_volume.is_none() && self.fade_opacity.is_none() {
            match self.fade_param {
                FadeParam::Opacity => self.fade_opacity = self.target_volume,
                _ => self.fade_volume = self.target_volume,
            }
        }
        self.from_volume = None; // entfällt: Fade startet beim aktuellen Wert
    }

    /// Builder: Laufzeit/Fade-Dauer setzen.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Builder: Post-Wait setzen.
    pub fn with_post_wait(mut self, post_wait: Duration) -> Self {
        self.post_wait = post_wait;
        self
    }

    /// Builder: Fade-Ziellautstärke setzen (0.0 – 1.0, wird geklemmt).
    pub fn with_fade_target(mut self, volume: f32) -> Self {
        self.target_volume = Some(volume.clamp(0.0, 1.0));
        self
    }

    /// Builder: Fade-Startlautstärke setzen (0.0 – 1.0, wird geklemmt).
    pub fn with_from_volume(mut self, volume: f32) -> Self {
        self.from_volume = Some(volume.clamp(0.0, 1.0));
        self
    }

    /// Builder: Fade-Parameter setzen (Volume/Rate/Opacity).
    pub fn with_fade_param(mut self, param: FadeParam) -> Self {
        self.fade_param = param;
        self
    }

    /// Builder: Fade-Kurve setzen (linear/dB).
    pub fn with_fade_curve(mut self, curve: FadeCurve) -> Self {
        self.fade_curve = curve;
        self
    }

    /// Builder: Loop-Modus setzen.
    pub fn with_loop(mut self, loop_mode: LoopMode) -> Self {
        self.loop_mode = loop_mode;
        self
    }

    /// Builder: Continue-Mode setzen (Auto-Continue/Auto-Follow).
    pub fn with_continue_mode(mut self, mode: ContinueMode) -> Self {
        self.continue_mode = mode;
        self
    }

    /// Builder: Media-Slices setzen (Segment-Plan mit Play Counts).
    pub fn with_slices(mut self, slices: Vec<MediaSlice>) -> Self {
        self.slices = slices;
        self
    }

    /// Builder: Audio-Routing setzen (Kanäle/Pan).
    pub fn with_route(mut self, route: AudioRoute) -> Self {
        self.route = Some(route);
        self
    }

    /// Builder: Kaskadierungs-Modus setzen (nur für Group-Cues relevant).
    pub fn with_group_mode(mut self, mode: GroupMode) -> Self {
        self.group_mode = mode;
        self
    }

    /// Builder: Kinder-Cues setzen (nur für Group-Cues relevant).
    pub fn with_children(mut self, children: Vec<Cue>) -> Self {
        self.children = children;
        self
    }

    /// Builder: Control-Aktion setzen (nur für [`CueType::Control`]).
    pub fn with_control(mut self, action: ControlAction) -> Self {
        self.control_action = action;
        self
    }

    /// Builder: Control-Ziel (Cue-Nummer) setzen.
    pub fn with_control_target(mut self, number: impl Into<String>) -> Self {
        self.control_target = Some(number.into());
        self
    }

    /// Builder: Ziel-Stage setzen (nur für Video-Cues relevant).
    pub fn with_stage(mut self, stage: impl Into<String>) -> Self {
        self.stage = Some(stage.into());
        self
    }

    /// Builder: Video-Layer setzen.
    pub fn with_layer(mut self, layer: u32) -> Self {
        self.layer = layer;
        self
    }

    /// Builder: Surface-Geometrie setzen (Position/Größe, Area of Interest,
    /// Hard-Edge, Deckkraft, Sichtbarkeit auf der Stage).
    pub fn with_surface(mut self, surface: SurfaceGeometry) -> Self {
        self.surface = Some(surface);
        self
    }

    /// Builder: Timecode-In setzen (Chasing-Kopplung an externe Clock).
    pub fn with_timecode_in(mut self, tc: Timecode) -> Self {
        self.timecode_in = Some(tc);
        self
    }

    /// Builder: HTML5-Quelle setzen (nur [`CueType::Html`] relevant).
    pub fn with_html_source(mut self, source: HtmlSource) -> Self {
        self.html_source = Some(source);
        self
    }

    /// Builder: Diashow-Definition setzen (nur [`CueType::SlideShow`] relevant).
    pub fn with_slideshow(mut self, show: SlideShow) -> Self {
        self.slideshow = Some(show);
        self
    }

    /// Builder: Ziel-Cue-ID für JSON-Pushes setzen (nur [`CueType::HtmlData`]).
    pub fn with_data_target(mut self, target: Uuid) -> Self {
        self.data_target = Some(target);
        self
    }

    /// Builder: JSON-Payload als Text setzen (nur [`CueType::HtmlData`]).
    /// Ungültiges JSON wird erst beim Triggern gemeldet (Cue → `Error`).
    pub fn with_data_json(mut self, json: impl Into<String>) -> Self {
        self.data_json_text = json.into();
        self
    }

    /// Builder: JS-Test-Script setzen (Inspector: manueller Trigger).
    pub fn with_js_test_script(mut self, script: impl Into<String>) -> Self {
        self.js_test_script = script.into();
        self
    }

    /// Builder: Notizen setzen.
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = notes.into();
        self
    }

    /// Parst den JSON-Payload eines [`CueType::HtmlData`]-Cues.
    /// Leerer oder ungültiger Text → [`ShowError::InvalidParameter`].
    pub fn data_payload(&self) -> ShowResult<serde_json::Value> {
        let text = self.data_json_text.trim();
        if text.is_empty() {
            return Err(ShowError::InvalidParameter(format!(
                "html data cue \"{}\" ({}): empty JSON payload",
                self.number, self.name
            )));
        }
        serde_json::from_str(text).map_err(|err| {
            ShowError::InvalidParameter(format!(
                "html data cue \"{}\" ({}): invalid JSON: {err}",
                self.number, self.name
            ))
        })
    }

    /// Validiert die Konfiguration vor dem Triggern. `Err` → der Cue geht
    /// kontrolliert in den Zustand [`CueStatus::Error`] (kein Panic im
    /// Live-Betrieb). Betrifft aktuell [`CueType::Html`],
    /// [`CueType::SlideShow`] und [`CueType::HtmlData`]; alle anderen Typen
    /// sind permissiv (abwärtskompatibel).
    pub fn validate_trigger(&self) -> ShowResult<()> {
        match self.cue_type {
            CueType::Html => match &self.html_source {
                Some(source) => source.validate(),
                None => Err(ShowError::InvalidParameter(format!(
                    "html cue \"{}\" ({}): no source configured",
                    self.number, self.name
                ))),
            },
            CueType::SlideShow => match &self.slideshow {
                Some(show) => show.validate(),
                None => Err(ShowError::InvalidParameter(format!(
                    "slideshow cue \"{}\" ({}): no slides configured",
                    self.number, self.name
                ))),
            },
            CueType::HtmlData => {
                if self.data_target.is_none() {
                    return Err(ShowError::InvalidParameter(format!(
                        "html data cue \"{}\" ({}): no target html cue",
                        self.number, self.name
                    )));
                }
                self.data_payload().map(|_| ())
            }
            CueType::Text => match &self.text_content {
                Some(_) => Ok(()),
                None => Err(ShowError::InvalidParameter(format!(
                    "text cue \"{}\" ({}): no text configured",
                    self.number, self.name
                ))),
            },
            CueType::Network => match &self.network {
                Some(n) => n.validate().map_err(|_| {
                    ShowError::InvalidParameter(format!(
                        "network cue \"{}\" ({}): invalid host/port",
                        self.number, self.name
                    ))
                }),
                None => Err(ShowError::InvalidParameter(format!(
                    "network cue \"{}\" ({}): no message configured",
                    self.number, self.name
                ))),
            },
            CueType::Midi => match &self.midi {
                Some(m) => m.validate().map_err(|_| {
                    ShowError::InvalidParameter(format!(
                        "midi cue \"{}\" ({}): invalid channel/data",
                        self.number, self.name
                    ))
                }),
                None => Err(ShowError::InvalidParameter(format!(
                    "midi cue \"{}\" ({}): no message configured",
                    self.number, self.name
                ))),
            },
            CueType::Companion => match &self.companion {
                Some(c) => c.validate().map_err(|_| {
                    ShowError::InvalidParameter(format!(
                        "companion cue \"{}\" ({}): invalid host/port",
                        self.number, self.name
                    ))
                }),
                None => Err(ShowError::InvalidParameter(format!(
                    "companion cue \"{}\" ({}): no trigger configured",
                    self.number, self.name
                ))),
            },
            CueType::Light => match &self.light {
                Some(l) => l.validate().map_err(|_| {
                    ShowError::InvalidParameter(format!(
                        "light cue \"{}\" ({}): invalid universe/channels",
                        self.number, self.name
                    ))
                }),
                None => Err(ShowError::InvalidParameter(format!(
                    "light cue \"{}\" ({}): no DMX values configured",
                    self.number, self.name
                ))),
            },
            CueType::Script => {
                if self.script.trim().is_empty() {
                    Err(ShowError::InvalidParameter(format!(
                        "script cue \"{}\" ({}): empty script",
                        self.number, self.name
                    )))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Sucht einen Cue rekursiv (inkl. Group-Kinder) anhand seiner ID.
    pub fn find(&self, id: Uuid) -> Option<&Cue> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find(id))
        }
    }

    /// Mutable Variante von [`Cue::find`].
    pub fn find_mut(&mut self, id: Uuid) -> Option<&mut Cue> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter_mut().find_map(|c| c.find_mut(id))
        }
    }

    /// Erzwingt die Zustands-State-Machine: Ungültige Übergänge liefern
    /// [`ShowError::InvalidStateTransition`], der Zustand bleibt unverändert.
    pub fn transition_status(&mut self, next: CueStatus) -> ShowResult<()> {
        if self.status == next {
            return Ok(()); // idempotent
        }
        if self.status.can_transition_to(next) {
            self.status = next;
            Ok(())
        } else {
            Err(ShowError::InvalidStateTransition {
                from: self.status.label().to_string(),
                to: next.label().to_string(),
            })
        }
    }

    /// Setzt alle Zustände rekursiv auf Standby (Reset inkl. Kinder).
    pub fn reset_status_recursive(&mut self) {
        let _ = self.transition_status(CueStatus::Standby);
        for child in &mut self.children {
            child.reset_status_recursive();
        }
    }

    /// Stoppt laufende/pausierte Wiedergaben rekursiv (Panic inkl. Kinder).
    pub fn stop_running_recursive(&mut self) {
        if self.status.is_active() {
            let _ = self.transition_status(CueStatus::Stopped);
        }
        for child in &mut self.children {
            child.stop_running_recursive();
        }
    }

    /// Stoppt diesen Cue und alle aktiven Kinder rekursiv und sammelt die
    /// `Stop`-Commands (pre-order: zuerst der Cue selbst, dann die Kinder).
    /// Rückgabe: Parent-Group für Close-if-done-Prüfung.
    pub(crate) fn stop_running_collect(
        &mut self,
        commands: &mut Vec<EngineCommand>,
    ) -> Option<Uuid> {
        if self.status.is_active() && self.transition_status(CueStatus::Stopped).is_ok() {
            commands.push(EngineCommand::Stop { cue_id: self.id });
        }
        let parent = self.parent_group;
        for child in &mut self.children {
            child.stop_running_collect(commands);
        }
        parent
    }

    /// Pausiert diesen Cue (`Playing` → `Paused`) und alle laufenden Kinder
    /// rekursiv; sammelt die `Pause`-Commands. Andere Zustände bleiben
    /// unberührt (State-Machine würde `PreWait`/`PostWait` → `Paused`
    /// ohnehin ablehnen).
    pub(crate) fn pause_running_collect(
        &mut self,
        commands: &mut Vec<EngineCommand>,
    ) -> Option<Uuid> {
        if self.status == CueStatus::Playing && self.transition_status(CueStatus::Paused).is_ok() {
            commands.push(EngineCommand::Pause { cue_id: self.id });
        }
        let parent = self.parent_group;
        for child in &mut self.children {
            child.pause_running_collect(commands);
        }
        parent
    }

    /// Setzt diesen Cue (`Paused` → `Playing`) und alle pausierten Kinder
    /// rekursiv fort; sammelt die `Resume`-Commands.
    pub(crate) fn resume_running_collect(
        &mut self,
        commands: &mut Vec<EngineCommand>,
    ) -> Option<Uuid> {
        if self.status == CueStatus::Paused && self.transition_status(CueStatus::Playing).is_ok() {
            commands.push(EngineCommand::Resume { cue_id: self.id });
        }
        let parent = self.parent_group;
        for child in &mut self.children {
            child.resume_running_collect(commands);
        }
        parent
    }

    /// Pausiert alle laufenden Cues rekursiv („Pause All");
    /// sammelt die `Pause`-Commands.
    pub(crate) fn pause_all_collect(&mut self, commands: &mut Vec<EngineCommand>) {
        if self.status == CueStatus::Playing && self.transition_status(CueStatus::Paused).is_ok() {
            commands.push(EngineCommand::Pause { cue_id: self.id });
        }
        for child in &mut self.children {
            child.pause_all_collect(commands);
        }
    }

    /// Setzt alle pausierten Cues rekursiv fort („Resume All");
    /// sammelt die `Resume`-Commands.
    pub(crate) fn resume_all_collect(&mut self, commands: &mut Vec<EngineCommand>) {
        if self.status == CueStatus::Paused && self.transition_status(CueStatus::Playing).is_ok() {
            commands.push(EngineCommand::Resume { cue_id: self.id });
        }
        for child in &mut self.children {
            child.resume_all_collect(commands);
        }
    }

    /// Erzeugt das Engine-Kommando, das beim Triggern dieses Cues gesendet wird.
    ///
    /// # Command-Pattern
    ///
    /// Jeder Cue-Typ kapselt, *wie* er in der Engine ausgeführt wird. Neue
    /// Cue-Typen erweitern nur diesen Match-Zweig. `None` bedeutet: Der Cue
    /// erzeugt keine Engine-Aktion (z. B. Group-Container, Control-Cues –
    /// diese steuern die Cue-Liste selbst) und gilt sofort als abgeschlossen.
    pub fn build_trigger_command(&self) -> Option<EngineCommand> {
        match self.cue_type {
            CueType::Audio | CueType::Video => Some(EngineCommand::Play {
                cue_id: self.id,
                path: self.target_path.clone(),
                duration: if self.duration.is_zero() {
                    None
                } else {
                    Some(self.duration)
                },
                pre_wait: self.pre_wait,
                post_wait: self.post_wait,
                loop_mode: self.loop_mode,
                slices: self.slices.clone(),
                route: self.route,
                timecode_in: self.timecode_in,
                stage: self.stage.clone(),
                layer: self.layer,
                surface: self.surface,
            }),
            CueType::Fade => {
                // Alte Felder (target_volume/fade_param) migrieren: Opacity -> fade_opacity.
                let volume = self.fade_volume.or(self.target_volume);
                let opacity = self.fade_opacity.or(match self.fade_param {
                    FadeParam::Opacity => self.target_volume,
                    _ => None,
                });
                if volume.is_none() && opacity.is_none() {
                    None // stummer Fehlschlag vermeiden: kein Kommando, cue_list meldet Error
                } else {
                    Some(EngineCommand::Fade {
                        cue_id: self.id,
                        target_cue: self.fade_target,
                        from: None, // None = aktueller Wert des Ziels (kein Sprung)
                        volume,
                        opacity,
                        duration: self.duration,
                        curve: self.fade_curve,
                        stop_when_done: self.stop_when_done,
                    })
                }
            }
            CueType::Crossfade => Some(EngineCommand::Crossfade {
                cue_id: self.id,
                from: self.crossfade_from,
                to: self.crossfade_to,
                // cue_list ersetzt to_volume durch die Lautstärke des „Nach"-Cues.
                to_volume: 1.0,
                duration: self.duration,
            }),
            CueType::Stop => Some(EngineCommand::StopAll),
            CueType::Html => self
                .html_source
                .clone()
                .map(|source| EngineCommand::PlayHtml {
                    cue_id: self.id,
                    source,
                    stage: self.stage.clone(),
                    layer: self.layer,
                    duration: if self.duration.is_zero() {
                        None
                    } else {
                        Some(self.duration)
                    },
                    pre_wait: self.pre_wait,
                    post_wait: self.post_wait,
                }),
            CueType::SlideShow => self
                .slideshow
                .clone()
                .map(|show| EngineCommand::PlaySlideShow {
                    cue_id: self.id,
                    show,
                    stage: self.stage.clone(),
                    layer: self.layer,
                    duration: if self.duration.is_zero() {
                        None
                    } else {
                        Some(self.duration)
                    },
                    pre_wait: self.pre_wait,
                    post_wait: self.post_wait,
                    surface: self.surface,
                }),
            CueType::HtmlData => {
                // Sofort-Cue: JSON-Push an die Ziel-WebView. Ohne gültiges
                // Ziel/Payload liefert `validate_trigger` den Fehler (GO setzt
                // den Cue auf Error); hier nur die glückliche Variante.
                let target = self.data_target?;
                let payload = self.data_payload().ok()?;
                Some(EngineCommand::PushJson { target, payload })
            }
            CueType::Wait => Some(EngineCommand::Wait {
                cue_id: self.id,
                duration: self.duration,
                pre_wait: self.pre_wait,
                post_wait: self.post_wait,
            }),
            CueType::Text => self.text_content.as_ref().map(|t| EngineCommand::PlayHtml {
                cue_id: self.id,
                source: HtmlSource::Inline(t.to_html()),
                stage: self.stage.clone(),
                layer: self.layer,
                duration: if self.duration.is_zero() {
                    None
                } else {
                    Some(self.duration)
                },
                pre_wait: self.pre_wait,
                post_wait: self.post_wait,
            }),
            CueType::Network => {
                // Sofort-Cue: Nachricht an die Engine, Cue gilt direkt als
                // abgeschlossen (wie StopAll/PushJson in `CueList::go_single`).
                self.network.as_ref().map(|n| EngineCommand::SendNetwork {
                    cue_id: self.id,
                    protocol: n.protocol,
                    host: n.host.clone(),
                    port: n.port,
                    message: n.message.clone(),
                })
            }
            CueType::Midi => self.midi.as_ref().map(|m| EngineCommand::SendMidi {
                cue_id: self.id,
                bytes: m.to_bytes(),
            }),
            CueType::Light => self.light.as_ref().map(|l| EngineCommand::SendDmx {
                cue_id: self.id,
                message: l.clone(),
            }),
            CueType::Companion => self
                .companion
                .as_ref()
                .map(|c| EngineCommand::SendHttpPost {
                    cue_id: self.id,
                    host: c.host.clone(),
                    port: c.port,
                    path: c.path(),
                }),
            CueType::Timecode => {
                // Manuelles GO: mit duration = Wait-Timer, sonst Sofort-Cue.
                if self.duration > Duration::ZERO {
                    Some(EngineCommand::Wait {
                        cue_id: self.id,
                        duration: self.duration,
                        pre_wait: self.pre_wait,
                        post_wait: self.post_wait,
                    })
                } else {
                    None
                }
            }
            CueType::Script => Some(EngineCommand::RunScript {
                cue_id: self.id,
                script: self.script.clone(),
            }),
            CueType::Group | CueType::Control | CueType::Memo => None,
        }
    }
}

/// Formatiert eine [`Duration`] als Timecode `MM:SS.mmm` bzw. `HH:MM:SS.mmm`.
pub fn format_timecode(d: Duration) -> String {
    let total_ms = d.as_millis();
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}

/// Parst einen Timecode zu einer [`Duration`] (Eingabe in den
/// Zeit-Spalten, docs/TODO-offen.md Phase 3: Inline-Edit).
///
/// Akzeptiert:
/// * `MM:SS` / `MM:SS.mmm` (z. B. `01:04`, `00:01.500`),
/// * `HH:MM:SS` / `HH:MM:SS.mmm` (z. B. `01:00:02.000`),
/// * nackte Sekunden als Dezimalzahl (z. B. `3`, `3.5` = 3,5 s).
///
/// Ungültige Eingaben liefern `None` (der Aufrufer behält den alten Wert).
pub fn parse_timecode(text: &str) -> Option<Duration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // Sekunden-Bruchteil abtrennen („5" = 500 ms, auf 3 Stellen aufgefüllt).
    let (whole, frac_ms) = match text.split_once('.') {
        Some((head, frac)) => {
            if frac.is_empty() || frac.len() > 3 || !frac.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ms = frac.parse::<u64>().ok()? * 10u64.pow(3 - frac.len() as u32);
            (head, ms)
        }
        None => (text, 0),
    };
    let base = if whole.contains(':') {
        parse_whole_timecode(whole)?
    } else {
        // Nackte Sekunden (Dezimalzahl).
        let secs: f64 = whole.parse().ok()?;
        if !secs.is_finite() || secs < 0.0 {
            return None;
        }
        Duration::from_secs_f64(secs)
    };
    Some(base + Duration::from_millis(frac_ms))
}

/// Parst `MM:SS` oder `HH:MM:SS` (ohne Bruchteil) zu einer [`Duration`].
fn parse_whole_timecode(text: &str) -> Option<Duration> {
    let parts: Vec<&str> = text.split(':').collect();
    let (hours, minutes, seconds): (u64, u64, u64) = match parts.as_slice() {
        [m, s] => (0, m.parse().ok()?, s.parse().ok()?),
        [h, m, s] => (h.parse().ok()?, m.parse().ok()?, s.parse().ok()?),
        _ => return None,
    };
    if minutes > 59 || seconds > 59 {
        return None;
    }
    Some(Duration::from_secs(hours * 3600 + minutes * 60 + seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 6: Datei-Endung → passender Cue-Typ (Drag & Drop aus Finder).
    #[test]
    fn cue_type_for_file_maps_extensions() {
        use std::path::Path;
        assert_eq!(
            cue_type_for_file(Path::new("/media/song.mp3")),
            Some(CueType::Audio)
        );
        assert_eq!(
            cue_type_for_file(Path::new("/media/Sound.WAV")),
            Some(CueType::Audio)
        );
        assert_eq!(
            cue_type_for_file(Path::new("/media/clip.mp4")),
            Some(CueType::Video)
        );
        assert_eq!(
            cue_type_for_file(Path::new("/media/clip.MOV")),
            Some(CueType::Video)
        );
        assert_eq!(
            cue_type_for_file(Path::new("/slides/bild.PNG")),
            Some(CueType::SlideShow)
        );
        assert_eq!(
            cue_type_for_file(Path::new("/slides/bild.jpg")),
            Some(CueType::SlideShow)
        );
        assert_eq!(cue_type_for_file(Path::new("/etc/notes.txt")), None);
        assert_eq!(cue_type_for_file(Path::new("/etc/noext")), None);
    }

    #[test]
    fn timecode_formats_millis_and_hours() {
        assert_eq!(format_timecode(Duration::from_millis(1500)), "00:01.500");
        assert_eq!(
            format_timecode(Duration::from_millis(3_600_000 + 2_000)),
            "01:00:02.000"
        );
    }

    #[test]
    fn parse_timecode_accepts_common_formats() {
        // MM:SS(.mmm)
        assert_eq!(parse_timecode("01:04"), Some(Duration::from_secs(64)));
        assert_eq!(
            parse_timecode("00:01.500"),
            Some(Duration::from_millis(1500))
        );
        // „5" hinter dem Punkt = 500 ms (auf 3 Stellen aufgefüllt).
        assert_eq!(parse_timecode("00:01.5"), Some(Duration::from_millis(1500)));
        // HH:MM:SS(.mmm)
        assert_eq!(
            parse_timecode("01:00:02.000"),
            Some(Duration::from_secs(3602))
        );
        // Nackte Sekunden.
        assert_eq!(parse_timecode("3"), Some(Duration::from_secs(3)));
        assert_eq!(parse_timecode("3.5"), Some(Duration::from_millis(3500)));
        assert_eq!(parse_timecode(" 2 "), Some(Duration::from_secs(2)));
        // Ungültig → None (Aufrufer behält den alten Wert).
        assert_eq!(parse_timecode(""), None);
        assert_eq!(parse_timecode("abc"), None);
        assert_eq!(parse_timecode("1:99"), None);
        assert_eq!(parse_timecode("1:2:3:4"), None);
        assert_eq!(parse_timecode("-5"), None);
        assert_eq!(parse_timecode("00:01.1234"), None);
    }

    #[test]
    fn parse_timecode_roundtrips_format_timecode() {
        for ms in [0u64, 500, 1500, 64_000, 3_600_000 + 2_000] {
            let d = Duration::from_millis(ms);
            assert_eq!(parse_timecode(&format_timecode(d)), Some(d));
        }
    }

    #[test]
    fn audio_cue_builds_play_command_with_timing_loop_slices_and_route() {
        let cue = Cue::new("1", "Jingle", CueType::Audio)
            .with_target("/media/jingle.wav")
            .with_duration(Duration::from_secs(4))
            .with_pre_wait(Duration::from_millis(500))
            .with_post_wait(Duration::from_millis(250))
            .with_loop(LoopMode::Count(2))
            .with_slices(vec![MediaSlice::new(
                Duration::ZERO,
                Duration::from_millis(500),
                2,
            )])
            .with_route(AudioRoute::new(2));
        let cmd = cue
            .build_trigger_command()
            .expect("audio cue must produce a command");
        match cmd {
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
            } => {
                assert_eq!(cue_id, cue.id);
                assert_eq!(path.as_deref(), Some("/media/jingle.wav"));
                assert_eq!(duration, Some(Duration::from_secs(4)));
                assert_eq!(pre_wait, Duration::from_millis(500));
                assert_eq!(post_wait, Duration::from_millis(250));
                assert_eq!(loop_mode, LoopMode::Count(2));
                assert_eq!(slices.len(), 1);
                assert_eq!(route, Some(AudioRoute::new(2)));
                assert_eq!(timecode_in, None);
                assert_eq!(stage, None);
                assert_eq!(surface, None);
                assert_eq!(layer, 0);
            }
            other => panic!("expected Play, got {other:?}"),
        }
    }

    #[test]
    fn fade_cue_builds_fade_command_with_volume_and_curve() {
        let mut cue = Cue::new("1", "Fade", CueType::Fade)
            .with_duration(Duration::from_secs(2))
            .with_fade_curve(FadeCurve::Decibel);
        cue.fade_volume = Some(0.25); // neues Feld (alte target_volume migriert)
        match cue.build_trigger_command() {
            Some(EngineCommand::Fade {
                cue_id,
                from,
                volume,
                opacity,
                duration,
                curve,
                stop_when_done,
                ..
            }) => {
                assert_eq!(cue_id, cue.id);
                assert_eq!(from, None, "from=None = aktueller Wert (kein Sprung)");
                assert_eq!(volume, Some(0.25));
                assert_eq!(opacity, None);
                assert_eq!(duration, Duration::from_secs(2));
                assert_eq!(curve, FadeCurve::Decibel);
                assert!(!stop_when_done);
            }
            other => panic!("expected Fade, got {other:?}"),
        }
    }

    #[test]
    fn fade_cue_without_volume_or_opacity_builds_no_command() {
        // Stummer Fehlschlag vermeiden: kein Kommando ohne Zielwert.
        let cue = Cue::new("1", "Fade", CueType::Fade);
        assert_eq!(cue.build_trigger_command(), None);
    }

    #[test]
    fn fade_cue_migrates_legacy_fields() {
        let mut cue = Cue::new("1", "Fade", CueType::Fade);
        cue.target_volume = Some(0.3);
        cue.fade_param = FadeParam::Opacity;
        cue.from_volume = Some(0.9);
        cue.migrate_fade_fields();
        assert_eq!(cue.fade_opacity, Some(0.3));
        assert_eq!(cue.fade_volume, None);
        assert_eq!(cue.from_volume, None, "from_volume entfaellt");
        // Alt: Volume-Fade landet in fade_volume.
        let mut cue2 = Cue::new("2", "Fade", CueType::Fade);
        cue2.target_volume = Some(0.5);
        cue2.fade_param = FadeParam::Volume;
        cue2.migrate_fade_fields();
        assert_eq!(cue2.fade_volume, Some(0.5));
    }

    #[test]
    fn stop_cue_builds_stop_all() {
        let cue = Cue::new("9", "Blackout", CueType::Stop);
        assert_eq!(cue.build_trigger_command(), Some(EngineCommand::StopAll));
    }

    #[test]
    fn group_and_control_cues_build_no_command() {
        let group = Cue::new("5", "Scene", CueType::Group);
        assert_eq!(group.build_trigger_command(), None);
        let control = Cue::new("6", "Reset", CueType::Control).with_control(ControlAction::Reset);
        assert_eq!(control.build_trigger_command(), None);
    }

    #[test]
    fn state_machine_allows_documented_transitions() {
        use CueStatus::*;
        let valid = [
            (Standby, PreWait),
            (Standby, Playing),
            (Standby, Completed),
            (Standby, Error),
            (PreWait, Playing),
            (PreWait, Stopped),
            (PreWait, Error),
            (Playing, Paused),
            (Playing, PostWait),
            (Playing, Completed),
            (Playing, Stopped),
            (Playing, Error),
            (Paused, Playing),
            (Paused, Stopped),
            (Paused, Error),
            (PostWait, Completed),
            (PostWait, Stopped),
            (PostWait, Error),
            // Operator-Override: Reset/Panic darf jederzeit nach Standby.
            (PreWait, Standby),
            (Playing, Standby),
            (Paused, Standby),
            (PostWait, Standby),
            (Completed, Standby),
            (Stopped, Standby),
            (Error, Standby),
        ];
        for (from, to) in valid {
            assert!(
                from.can_transition_to(to),
                "{from:?} -> {to:?} must be valid"
            );
            assert!(
                from.can_transition_to(from),
                "identity {from:?} must be valid"
            );
        }
        let invalid = [
            (Standby, Paused),
            (Standby, PostWait),
            (PreWait, Paused),
            (PreWait, Completed),
            (Completed, Playing),
            (Completed, Paused),
            (Completed, PreWait),
            (Paused, Completed),
            (Paused, PostWait),
            (Stopped, Playing),
            (Stopped, Completed),
            (Error, Playing),
            (Error, Completed),
        ];
        for (from, to) in invalid {
            assert!(
                !from.can_transition_to(to),
                "{from:?} -> {to:?} must be invalid"
            );
        }
    }

    #[test]
    fn transition_status_rejects_invalid_transition_and_keeps_state() {
        let mut cue = Cue::new("1", "a", CueType::Audio);
        let err = cue.transition_status(CueStatus::Paused).unwrap_err();
        assert!(matches!(err, ShowError::InvalidStateTransition { .. }));
        assert_eq!(cue.status, CueStatus::Standby, "state must be unchanged");
    }

    #[test]
    fn transition_status_full_lifecycle_with_waits() {
        let mut cue = Cue::new("1", "a", CueType::Audio);
        cue.transition_status(CueStatus::PreWait).unwrap();
        cue.transition_status(CueStatus::Playing).unwrap();
        cue.transition_status(CueStatus::Paused).unwrap();
        cue.transition_status(CueStatus::Playing).unwrap();
        cue.transition_status(CueStatus::PostWait).unwrap();
        cue.transition_status(CueStatus::Completed).unwrap();
        cue.transition_status(CueStatus::Standby).unwrap(); // Reset/Re-Arm
        assert_eq!(cue.status, CueStatus::Standby);
    }

    #[test]
    fn find_locates_nested_children() {
        let child = Cue::new("1.1", "child", CueType::Audio);
        let child_id = child.id;
        let group = Cue::new_group("1", "group", GroupMode::Simultaneous, vec![child]);
        assert!(group.find(child_id).is_some());
        assert!(group.find(Uuid::new_v4()).is_none());
    }

    #[test]
    fn cue_serde_roundtrip_preserves_timing_loops_and_new_fields() {
        let cue = Cue::new("1", "Test", CueType::Video)
            .with_target("/media/a.mp4")
            .with_pre_wait(Duration::from_millis(1500))
            .with_duration(Duration::from_secs(2))
            .with_post_wait(Duration::from_millis(250))
            .with_loop(LoopMode::Count(3))
            .with_slices(vec![MediaSlice::new(
                Duration::ZERO,
                Duration::from_secs(1),
                2,
            )])
            .with_route(AudioRoute::new(6))
            .with_stage("Stage 1")
            .with_layer(2)
            .with_timecode_in(Timecode::from_hmsf(0, 1, 0, 0, crate::core::Fps::Fps25));
        let json = serde_json::to_string(&cue).expect("serialize");
        let back: Cue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, cue.id);
        assert_eq!(back.pre_wait, Duration::from_millis(1500));
        assert_eq!(back.duration, Duration::from_secs(2));
        assert_eq!(back.post_wait, Duration::from_millis(250));
        assert_eq!(back.loop_mode, LoopMode::Count(3));
        assert_eq!(back.target_path.as_deref(), Some("/media/a.mp4"));
        assert_eq!(back.slices.len(), 1);
        assert_eq!(back.route, Some(AudioRoute::new(6)));
        assert_eq!(back.stage.as_deref(), Some("Stage 1"));
        assert_eq!(back.layer, 2);
        assert_eq!(
            back.timecode_in,
            Some(Timecode::from_hmsf(0, 1, 0, 0, crate::core::Fps::Fps25))
        );
        assert!(back.is_armed);
    }

    #[test]
    fn control_cue_serde_roundtrip() {
        let cue = Cue::new("7", "Blackout", CueType::Control)
            .with_control(ControlAction::Devamp)
            .with_control_target("3");
        let json = serde_json::to_string(&cue).expect("serialize");
        let back: Cue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.cue_type, CueType::Control);
        assert_eq!(back.control_action, ControlAction::Devamp);
        assert_eq!(back.control_target.as_deref(), Some("3"));
    }

    #[test]
    fn group_cue_serde_roundtrip_with_children() {
        let group = Cue::new_group(
            "1",
            "Scene",
            GroupMode::Sequential,
            vec![
                Cue::new("1.1", "a", CueType::Audio),
                Cue::new("1.2", "b", CueType::Video),
            ],
        );
        let json = serde_json::to_string(&group).expect("serialize");
        let back: Cue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.cue_type, CueType::Group);
        assert_eq!(back.group_mode, GroupMode::Sequential);
        assert_eq!(back.children.len(), 2);
        assert_eq!(back.children[1].cue_type, CueType::Video);
    }

    #[test]
    fn cue_color_serde_roundtrip_and_backcompat() {
        // Roundtrip mit Farbe.
        let mut cue = Cue::new("1", "Cue", CueType::Audio);
        assert_eq!(cue.color, None);
        cue.color = Some(CUE_COLORS[3]);
        let json = serde_json::to_string(&cue).expect("serialize");
        let back: Cue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.color, Some(CUE_COLORS[3]));

        // Backcompat: JSON ohne color-Feld (alte Shows) → None.
        let old_json = json.replace(
            &format!(
                ",\"color\":[{},{},{}]",
                CUE_COLORS[3][0], CUE_COLORS[3][1], CUE_COLORS[3][2]
            ),
            "",
        );
        assert_ne!(old_json, json, "color-Feld sollte im JSON sein");
        let back: Cue = serde_json::from_str(&old_json).expect("deserialize ohne color");
        assert_eq!(back.color, None);
    }
}
