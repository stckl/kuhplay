//! Domain Data Model: Cue-Typen, [`CueList`], Carts, Fehler.
//!
//! Dieser Layer kennt weder UI noch konkrete Media-Backends. Er definiert das
//! Vokabular der Show und die Regeln für Cue-Lebenszyklen (State-Pattern).
//!
//! ## Module
//!
//! * [`cue`]: [`Cue`], [`CueType`], [`CueStatus`]-State-Machine, [`GroupMode`],
//!   [`LoopMode`], Control-Cues ([`ControlAction`]).
//! * [`cue_list`]: [`CueList`] – Playhead, GO-Logik, Group-Kaskadierung.
//! * [`cart`]: [`CartManager`] – mehrere unabhängige Cue-Listen (QLab-Carts).
//! * [`audio`]: Fade-Parameter/-Kurven (linear/dB), [`AudioRoute`], [`MediaSlice`].
//! * [`timecode`]: [`Timecode`]/[`Fps`] – Grundlage für Timecode-Chasing.
//! * [`stage`]: [`Stage`]/[`StageGeometry`]/[`Crop`] – Video-Ausgabeflächen
//!   mit Regionen und Layern.
//! * [`video_routing`]: [`OutputRoute`]/[`Region`]/[`RouteManager`] –
//!   Stage → Region → Output Route → Device (QLab 5).
//! * [`html`]: [`HtmlSource`] – Quelle eines HTML5-Cues (Datei/URL/Inline).
//! * [`slideshow`]: [`SlideShow`]/[`Slide`]/[`SlideNav`] – Diashow-Cues.
//! * [`error`] – [`ShowError`].
//!
//! ## Erweiterung
//!
//! * **Neue Cue-Typen:** [`CueType`] erweitern und den Match-Zweig in
//!   [`Cue::build_trigger_command`] ergänzen – UI und Engine bleiben unangetastet.
//! * **Pre-/Post-Wait-Scheduler:** Ein Timer-Thread, der nach Ablauf der
//!   Wartezeiten automatisch `CueList::go()` auslöst.
//! * **Undo/Redo:** Alle Mutationen als wiederholbare Commands kapseln.
//! * **Persistenz:** `Cue`, `CueList` und `CartManager` sind `serde`-fähig.

pub mod audio;
pub mod cart;
pub mod cue;
pub mod cue_list;
pub mod error;
pub mod html;
pub mod screen;
pub mod show;
pub mod slideshow;
pub mod stage;
pub mod timecode;
pub mod video_routing;

/// Serde-Hilfsmodul: `Duration` als Millisekunden (JSON-lesbar).
pub(crate) mod duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}

pub use audio::{
    db_to_linear, linear_to_db, linear_to_db_clamped, AudioChannel, AudioPatch, AudioRoute,
    FadeCurve, FadeParam, MediaSlice, AUDIO_CHANNEL_COUNT, DB_FLOOR,
};
pub use cart::CartManager;
pub use cue::{
    cue_type_for_file, format_timecode, parse_timecode, ContinueMode, ControlAction, Cue,
    CueStatus, CueType, GroupMode, LoopMode, CUE_COLORS,
};
pub use cue_list::{CueList, GoOutcome};
pub use error::{ShowError, ShowResult};
pub use html::{HtmlSource, HtmlSourceKind};
pub use screen::{
    screen_color_for_index, DisplayGeometry, Screen, ScreenManager, ScreenRotation,
    TestPatternKind, TestPatternSpec, SCREEN_COLOR_PALETTE,
};
pub use show::ShowDocument;
pub use slideshow::{Slide, SlideNav, SlideShow};
pub use stage::{
    BlendCurve, Crop, EdgeSide, GeometryMode, HardEdge, Stage, StageGeometry, StageManager,
    SurfaceGeometry,
};
pub use timecode::{Fps, Timecode};
pub use video_routing::{
    default_routes_for_displays, edge_factor, region_letter, regions_for_routes_proportional,
    regions_in_reading_order, DeviceArea, DeviceRef, DisplayRefInfo, EdgeBlend, OutputRoute,
    Region, RouteManager, RouteScaling,
};
