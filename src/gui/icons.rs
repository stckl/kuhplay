//! Icons für die gesamte GUI – **Phosphor Icons** (<https://phosphoricons.com/>).
//!
//! Warum Phosphor statt Font-Glyphen (⏵, ■, …) oder selbst gezeichneter
//! Vektor-Icons? egui rendert Emoji und viele Unicode-Symbole als
//! „Tofu"-Kästchen (fehlende Glyphen im Standard-Font). Phosphor ist ein
//! professionelles, konsistentes Icon-Set (7.000+ Icons, 6 Gewichte). Der
//! Font (`assets/fonts/phosphor/Phosphor-Regular.ttf`, MIT-Lizenz, siehe
//! `assets/fonts/phosphor/ATTRIBUTION.md`) ist eingebettet und wird über
//! [`install`] als eigene Font-Family registriert – scharfe Darstellung auf
//! jedem Display (inkl. Retina), Farbe folgt der Palette (`theme::palette`).
//!
//! Die Glyph-Codepoints (`glyphs`) sind die offiziellen Phosphor-Codepoints
//! (Stabil, siehe phosphoricons.com); nur das Regular-Weight wird genutzt.
//!
//! ## Erweiterung
//!
//! Neue Icon-Variante = Variante im [`Icon`]-Enum + Codepoint in `glyphs` +
//! ggf. Mapping in `for_cue_type` / `for_status` / `for_continue_mode`.

use egui::{Align2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::core::{ContinueMode, CueStatus, CueType};

/// Font-Family-Name der Phosphor-Icons (reguläres Gewicht).
pub const PHOSPHOR_FAMILY: &str = "phosphor";

/// Eingebetteter Phosphor-Regular-Font (MIT, siehe ATTRIBUTION.md).
const PHOSPHOR_FONT: &[u8] = include_bytes!("../../assets/fonts/phosphor/Phosphor-Regular.ttf");

/// [`FontId`] für Phosphor-Icons in gegebener Pixel-Größe.
pub fn font_id(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(PHOSPHOR_FAMILY.into()))
}

/// Registriert den Phosphor-Font im `egui::Context` (einmalig, idempotent).
///
/// Wird aus `main.rs` (Creation-Callback) und `theme::apply_theme` aufgerufen
/// (Guard über Context-Daten, daher mehrfach sicher).
pub fn install(ctx: &egui::Context) {
    let key = egui::Id::new("kuhplay/phosphor-fonts-installed");
    let already = ctx.data_mut(|d| d.get_persisted::<bool>(key).unwrap_or(false));
    if already {
        return;
    }
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        PHOSPHOR_FAMILY.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(PHOSPHOR_FONT)),
    );
    fonts.families.insert(
        egui::FontFamily::Name(PHOSPHOR_FAMILY.into()),
        vec![PHOSPHOR_FAMILY.to_owned()],
    );
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_persisted(key, true));
}

/// Phosphor-Glyph-Codepoints (offizielle Codepoints, Regular-Weight).
///
/// Namen entsprechen phosphoricons.com (Kebab-Case in Klammern). Alle
/// Codepoints sind gegen die cmap der eingebetteten
/// `Phosphor-Regular.ttf` validiert (sonst würde egui „Tofu"-Kästchen
/// rendern).
mod glyphs {
    pub const APERTURE: &str = "\u{E00A}"; // aperture
    pub const ARROW_CLOCKWISE: &str = "\u{E036}"; // arrow-clockwise
    pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}"; // arrow-counter-clockwise
    pub const ARROWS_IN: &str = "\u{E09A}"; // arrows-in
    pub const ARROWS_OUT: &str = "\u{E0A2}"; // arrows-out
    pub const ARROWS_OUT_CARDINAL: &str = "\u{E0A4}"; // arrows-out-cardinal
    pub const BOUNDING_BOX: &str = "\u{E6CE}"; // bounding-box
    pub const BROWSERS: &str = "\u{E0F6}"; // browsers
    pub const CARDS: &str = "\u{E0F8}"; // cards
    pub const CARET_DOUBLE_RIGHT: &str = "\u{E12A}"; // caret-double-right
    pub const CARET_DOWN: &str = "\u{E136}"; // caret-down
    pub const CARET_LEFT: &str = "\u{E138}"; // caret-left
    pub const CARET_LINE_RIGHT: &str = "\u{E130}"; // caret-line-right
    pub const CARET_RIGHT: &str = "\u{E13A}"; // caret-right
    pub const CARET_UP: &str = "\u{E13C}"; // caret-up
    pub const CHECK: &str = "\u{E182}"; // check
    pub const CIRCLE: &str = "\u{E18A}"; // circle
    pub const CODE: &str = "\u{E1BC}"; // code
    pub const CONTROL: &str = "\u{ECA6}"; // control
    pub const CORNERS_IN: &str = "\u{E1CE}"; // corners-in
    pub const CORNERS_OUT: &str = "\u{E1D0}"; // corners-out
    pub const CROSSHAIR: &str = "\u{E1D6}"; // crosshair
    pub const CURSOR: &str = "\u{E1DC}"; // cursor
    pub const CURSOR_CLICK: &str = "\u{E7C8}"; // cursor-click
    pub const CUBE: &str = "\u{E1DA}"; // cube
    pub const DATABASE: &str = "\u{E1DE}"; // database
    pub const DESKTOP: &str = "\u{E560}"; // desktop
    pub const EJECT: &str = "\u{E212}"; // eject
    pub const FADERS: &str = "\u{E228}"; // faders
    pub const FILM_STRIP: &str = "\u{E792}"; // film-strip
    pub const GEAR: &str = "\u{E270}"; // gear
    pub const GLOBE: &str = "\u{E288}"; // globe
    pub const GRID_FOUR: &str = "\u{E296}"; // grid-four
    pub const GRID_NINE: &str = "\u{EC8C}"; // grid-nine
    pub const HAND: &str = "\u{E298}"; // hand
    pub const HAND_POINTING: &str = "\u{E29A}"; // hand-pointing
    pub const HOURGLASS: &str = "\u{E1B8}"; // hourglass
    pub const NOTE: &str = "\u{E5A2}"; // note
    pub const PAPER_PLANE_TILT: &str = "\u{E6D0}"; // paper-plane-tilt
    pub const IMAGE: &str = "\u{E2CA}"; // image
    pub const IMAGES: &str = "\u{E836}"; // images
    pub const LAYOUT: &str = "\u{E6D6}"; // layout
    pub const MAGIC_WAND: &str = "\u{E6B6}"; // magic-wand
    pub const MAGNIFYING_GLASS: &str = "\u{E30C}"; // magnifying-glass
    pub const MAGNIFYING_GLASS_PLUS: &str = "\u{E310}"; // magnifying-glass-plus
    pub const MINUS: &str = "\u{E32A}"; // minus
    pub const MONITOR: &str = "\u{E32E}"; // monitor
    pub const MONITOR_PLAY: &str = "\u{E58C}"; // monitor-play
    pub const PAINT_BRUSH: &str = "\u{E6F0}"; // paint-brush
    pub const PAUSE: &str = "\u{E39E}"; // pause
    pub const PLAY: &str = "\u{E3D0}"; // play
    pub const PLUS: &str = "\u{E3D4}"; // plus
    pub const PROJECTOR_SCREEN: &str = "\u{E658}"; // projector-screen
    pub const RECTANGLE: &str = "\u{E3F0}"; // rectangle
    pub const RESIZE: &str = "\u{ED6E}"; // resize
    pub const SCREENCAST: &str = "\u{E404}"; // screencast
    pub const SIDEBAR: &str = "\u{EAB6}"; // sidebar
    pub const SELECTION: &str = "\u{E69A}"; // selection
    pub const SELECTION_BACKGROUND: &str = "\u{EAF8}"; // selection-background
    pub const SELECTION_PLUS: &str = "\u{E69C}"; // selection-plus
    pub const SELECTION_SLASH: &str = "\u{E69E}"; // selection-slash
    pub const SLIDERS: &str = "\u{E432}"; // sliders
    pub const SPEAKER_HIGH: &str = "\u{E44A}"; // speaker-high
    pub const STACK: &str = "\u{E466}"; // stack
    pub const STOP: &str = "\u{E46C}"; // stop
    pub const TARGET: &str = "\u{E47C}"; // target
    pub const TEXT_T: &str = "\u{E7C0}"; // text-t
    pub const TRASH: &str = "\u{E4A6}"; // trash
    pub const VIDEO_CAMERA: &str = "\u{E4DA}"; // video-camera
    pub const WARNING: &str = "\u{E4E0}"; // warning
    pub const WARNING_CIRCLE: &str = "\u{E4E2}"; // warning-circle
    pub const X: &str = "\u{E4F6}"; // x
}

/// Alle verfügbaren KuhPlay-Icons (Mapping auf Phosphor-Glyphs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    // Cue-Typen (QLab-Type-Icons)
    Audio,
    Video,
    Fade,
    Group,
    Stop,
    Script,
    Control,
    Html,
    SlideShow,
    HtmlData,
    Wait,
    Memo,
    Text,
    Network,
    // Status
    Playing,
    Paused,
    Error,
    /// Kaputter Cue (QLab „broken cue": rotes ✕ in der Status-Spalte).
    Broken,
    // Playhead / Continue-Modi
    Playhead,
    ContinueOff,
    AutoContinue,
    AutoFollow,
    // Globale Controls (Sidebar / Masthead)
    Reset,
    Pause,
    Resume,
    Panic,
    // Stage/Screen
    Globe,
    Monitor,
    ProjectorScreen,
    Desktop,
    // Allgemein (Editor, Fenster, Listen, Footer-Toggles)
    Close,
    Plus,
    Trash,
    Gear,
    Sidebar,
    Sliders,
    Check,
    Minus,
    ArrowClockwise,
    ArrowCounterClockwise,
    Resize,
    ArrowsOut,
    ArrowsIn,
    ArrowsOutCardinal,
    CornersOut,
    CornersIn,
    Cursor,
    CursorClick,
    Hand,
    HandPointing,
    Target,
    // Testbild / Auswahl / Geometrie
    TestPattern,
    GridNine,
    Crosshair,
    Selection,
    SelectionPlus,
    SelectionSlash,
    SelectionBackground,
    Aperture,
    BoundingBox,
    Cards,
    Cube,
    Eject,
    Browsers,
    FilmStrip,
    Image,
    Layout,
    MagicWand,
    MagnifyingGlass,
    MagnifyingGlassPlus,
    MonitorPlay,
    PaintBrush,
    Rectangle,
    Screencast,
    CaretDown,
    CaretUp,
    CaretLeft,
    CaretRight,
}

impl Icon {
    /// Alle Icon-Varianten (für Smoke-Tests / Icon-Übersichten).
    pub const ALL: &'static [Icon] = &[
        // Cue-Typen
        Icon::Audio,
        Icon::Video,
        Icon::Fade,
        Icon::Group,
        Icon::Stop,
        Icon::Script,
        Icon::Control,
        Icon::Html,
        Icon::SlideShow,
        Icon::HtmlData,
        Icon::Wait,
        Icon::Memo,
        Icon::Text,
        Icon::Network,
        // Status
        Icon::Playing,
        Icon::Paused,
        Icon::Error,
        Icon::Broken,
        // Playhead / Continue-Modi
        Icon::Playhead,
        Icon::ContinueOff,
        Icon::AutoContinue,
        Icon::AutoFollow,
        // Globale Controls
        Icon::Reset,
        Icon::Pause,
        Icon::Resume,
        Icon::Panic,
        // Stage/Screen
        Icon::Globe,
        Icon::Monitor,
        Icon::ProjectorScreen,
        Icon::Desktop,
        // Allgemein
        Icon::Close,
        Icon::Plus,
        Icon::Trash,
        Icon::Gear,
        Icon::Sidebar,
        Icon::Sliders,
        Icon::Check,
        Icon::Minus,
        Icon::ArrowClockwise,
        Icon::ArrowCounterClockwise,
        Icon::Resize,
        Icon::ArrowsOut,
        Icon::ArrowsIn,
        Icon::ArrowsOutCardinal,
        Icon::CornersOut,
        Icon::CornersIn,
        Icon::Cursor,
        Icon::CursorClick,
        Icon::Hand,
        Icon::HandPointing,
        Icon::Target,
        // Testbild / Auswahl / Geometrie
        Icon::TestPattern,
        Icon::GridNine,
        Icon::Crosshair,
        Icon::Selection,
        Icon::SelectionPlus,
        Icon::SelectionSlash,
        Icon::SelectionBackground,
        Icon::Aperture,
        Icon::BoundingBox,
        Icon::Cards,
        Icon::Cube,
        Icon::Eject,
        Icon::Browsers,
        Icon::FilmStrip,
        Icon::Image,
        Icon::Layout,
        Icon::MagicWand,
        Icon::MagnifyingGlass,
        Icon::MagnifyingGlassPlus,
        Icon::MonitorPlay,
        Icon::PaintBrush,
        Icon::Rectangle,
        Icon::Screencast,
        Icon::CaretDown,
        Icon::CaretUp,
        Icon::CaretLeft,
        Icon::CaretRight,
    ];

    /// Icon für einen Cue-Typ (QLab-Type-Spalte).
    pub fn for_cue_type(cue_type: CueType) -> Icon {
        match cue_type {
            CueType::Audio => Icon::Audio,
            CueType::Video => Icon::Video,
            CueType::Fade => Icon::Fade,
            CueType::Group => Icon::Group,
            CueType::Stop => Icon::Stop,
            CueType::Script => Icon::Script,
            CueType::Control => Icon::Control,
            CueType::Html => Icon::Html,
            CueType::SlideShow => Icon::SlideShow,
            CueType::HtmlData => Icon::HtmlData,
            CueType::Wait => Icon::Wait,
            CueType::Memo => Icon::Memo,
            CueType::Text => Icon::Text,
            CueType::Network => Icon::Network,
        }
    }

    /// Icon für einen Cue-Status (Status-Pille), `None` = kein Icon
    /// (Standby/Completed zeigen im Playhead-Modus keinen Pill-Inhalt).
    pub fn for_status(status: CueStatus) -> Option<Icon> {
        match status {
            CueStatus::Playing | CueStatus::PreWait | CueStatus::PostWait => Some(Icon::Playing),
            CueStatus::Paused => Some(Icon::Paused),
            CueStatus::Error => Some(Icon::Error),
            _ => None,
        }
    }

    /// Icon für den Continue-Mode (QLab-Spalte „Cont.").
    pub fn for_continue_mode(mode: ContinueMode) -> Icon {
        match mode {
            ContinueMode::Off => Icon::ContinueOff,
            ContinueMode::AutoContinue => Icon::AutoContinue,
            ContinueMode::AutoFollow => Icon::AutoFollow,
        }
    }

    /// Phosphor-Glyph (Unicode-Codepoint) dieses Icons.
    pub fn glyph(&self) -> &'static str {
        match self {
            // Cue-Typen
            Icon::Audio => glyphs::SPEAKER_HIGH,
            Icon::Video => glyphs::VIDEO_CAMERA,
            Icon::Fade => glyphs::FADERS,
            Icon::Group => glyphs::STACK,
            Icon::Stop => glyphs::STOP,
            Icon::Script => glyphs::CODE,
            Icon::Control => glyphs::CONTROL,
            Icon::Html => glyphs::GLOBE,
            Icon::SlideShow => glyphs::IMAGES,
            Icon::HtmlData => glyphs::DATABASE,
            Icon::Wait => glyphs::HOURGLASS,
            Icon::Memo => glyphs::NOTE,
            Icon::Text => glyphs::TEXT_T,
            Icon::Network => glyphs::PAPER_PLANE_TILT,
            // Status
            Icon::Playing => glyphs::PLAY,
            Icon::Paused => glyphs::PAUSE,
            Icon::Error => glyphs::WARNING_CIRCLE,
            Icon::Broken => glyphs::X,
            // Playhead / Continue-Modi
            Icon::Playhead => glyphs::CARET_RIGHT,
            Icon::ContinueOff => glyphs::CIRCLE,
            Icon::AutoContinue => glyphs::CARET_DOUBLE_RIGHT,
            // QLab-Auto-Follow (▶|): „nach diesem Cue folgt der nächste" –
            // Phosphor `caret-line-right` (|▶) drückt dieselbe Richtung aus.
            Icon::AutoFollow => glyphs::CARET_LINE_RIGHT,
            // Globale Controls
            Icon::Reset => glyphs::ARROW_COUNTER_CLOCKWISE,
            Icon::Pause => glyphs::PAUSE,
            Icon::Resume => glyphs::PLAY,
            Icon::Panic => glyphs::WARNING,
            // Stage/Screen
            Icon::Globe => glyphs::GLOBE,
            Icon::Monitor => glyphs::MONITOR,
            Icon::ProjectorScreen => glyphs::PROJECTOR_SCREEN,
            Icon::Desktop => glyphs::DESKTOP,
            // Allgemein (Editor, Fenster, Listen, Footer-Toggles)
            Icon::Close => glyphs::X,
            Icon::Plus => glyphs::PLUS,
            Icon::Trash => glyphs::TRASH,
            Icon::Gear => glyphs::GEAR,
            Icon::Sidebar => glyphs::SIDEBAR,
            Icon::Sliders => glyphs::SLIDERS,
            Icon::Check => glyphs::CHECK,
            Icon::Minus => glyphs::MINUS,
            Icon::ArrowClockwise => glyphs::ARROW_CLOCKWISE,
            Icon::ArrowCounterClockwise => glyphs::ARROW_COUNTER_CLOCKWISE,
            Icon::Resize => glyphs::RESIZE,
            Icon::ArrowsOut => glyphs::ARROWS_OUT,
            Icon::ArrowsIn => glyphs::ARROWS_IN,
            Icon::ArrowsOutCardinal => glyphs::ARROWS_OUT_CARDINAL,
            Icon::CornersOut => glyphs::CORNERS_OUT,
            Icon::CornersIn => glyphs::CORNERS_IN,
            Icon::Cursor => glyphs::CURSOR,
            Icon::CursorClick => glyphs::CURSOR_CLICK,
            Icon::Hand => glyphs::HAND,
            Icon::HandPointing => glyphs::HAND_POINTING,
            Icon::Target => glyphs::TARGET,
            // Testbild / Auswahl / Geometrie
            Icon::TestPattern => glyphs::GRID_FOUR,
            Icon::GridNine => glyphs::GRID_NINE,
            Icon::Crosshair => glyphs::CROSSHAIR,
            Icon::Selection => glyphs::SELECTION,
            Icon::SelectionPlus => glyphs::SELECTION_PLUS,
            Icon::SelectionSlash => glyphs::SELECTION_SLASH,
            Icon::SelectionBackground => glyphs::SELECTION_BACKGROUND,
            Icon::Aperture => glyphs::APERTURE,
            Icon::BoundingBox => glyphs::BOUNDING_BOX,
            Icon::Cards => glyphs::CARDS,
            Icon::Cube => glyphs::CUBE,
            Icon::Eject => glyphs::EJECT,
            Icon::Browsers => glyphs::BROWSERS,
            Icon::FilmStrip => glyphs::FILM_STRIP,
            Icon::Image => glyphs::IMAGE,
            Icon::Layout => glyphs::LAYOUT,
            Icon::MagicWand => glyphs::MAGIC_WAND,
            Icon::MagnifyingGlass => glyphs::MAGNIFYING_GLASS,
            Icon::MagnifyingGlassPlus => glyphs::MAGNIFYING_GLASS_PLUS,
            Icon::MonitorPlay => glyphs::MONITOR_PLAY,
            Icon::PaintBrush => glyphs::PAINT_BRUSH,
            Icon::Rectangle => glyphs::RECTANGLE,
            Icon::Screencast => glyphs::SCREENCAST,
            Icon::CaretDown => glyphs::CARET_DOWN,
            Icon::CaretUp => glyphs::CARET_UP,
            Icon::CaretLeft => glyphs::CARET_LEFT,
            Icon::CaretRight => glyphs::CARET_RIGHT,
        }
    }

    /// Zeichnet das Icon zentriert in `rect` (Phosphor-Glyph, regulär).
    pub fn paint(&self, painter: &Painter, rect: Rect, color: Color32) {
        let size = rect.width().min(rect.height()) * 0.92;
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            self.glyph(),
            font_id(size),
            color,
        );
    }
}

/// Malt ein Icon an absoluter Position (z. B. in einem Menü neben einem Label).
pub fn paint_at(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    icon.paint(ui.painter(), rect, color);
}

/// Zeichnet ein Icon als nicht-interaktives Widget (hover-sensed) in `ui`.
/// Liefert die `Response` (z. B. für Tooltips).
pub fn draw(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    icon.paint(ui.painter(), rect, color);
    response
}

/// Zeichnet ein Icon als klickbaren Button mit Mindestgröße.
pub fn button(ui: &mut Ui, icon: Icon, color: Color32, min_size: Vec2, enabled: bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(min_size, Sense::click());
    if enabled {
        if response.hovered() {
            ui.painter().rect_filled(
                rect,
                4.0,
                ui.visuals().widgets.hovered.bg_fill.linear_multiply(0.5),
            );
        }
        icon.paint(ui.painter(), rect.shrink(6.0), color);
    } else {
        icon.paint(ui.painter(), rect.shrink(6.0), color.linear_multiply(0.4));
        response.sense = Sense::hover();
    }
    response
}

/// Button mit Icon über Text-Label (für die Sidebar-Globals, QLab-Look:
/// quadratische Kacheln mit Symbol + Beschriftung).
/// `fill` = Hintergrundfarbe (z. B. `Some(PANIC)` für den Panic-Button).
pub fn stacked_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    color: Color32,
    fill: Option<Color32>,
    size: Vec2,
    enabled: bool,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let bg = if !enabled {
        visuals.widgets.inactive.bg_fill.linear_multiply(0.4)
    } else if let Some(fill) = fill {
        if response.hovered() {
            fill.linear_multiply(1.15)
        } else {
            fill
        }
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let stroke = if response.hovered() && enabled {
        Stroke::new(1.0_f32, visuals.widgets.hovered.fg_stroke.color)
    } else {
        visuals.widgets.inactive.fg_stroke
    };
    ui.painter().rect(rect, 6.0, bg, stroke);
    let icon_size = size.y * 0.42;
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + rect.height() * 0.34),
        Vec2::splat(icon_size),
    );
    icon.paint(ui.painter(), icon_rect, color);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - rect.height() * 0.16),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(size.y * 0.16),
        color,
    );
    if enabled {
        response
    } else {
        response.sense = Sense::hover();
        response
    }
}
