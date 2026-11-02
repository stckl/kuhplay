//! Dark-Theme und Status-Farben.
//!
//! ## Erweiterung
//!
//! * Theme als `serde`-fähige Konfiguration (Dark / Light / High-Contrast)
//!   in einer `theme.toml` persistieren.

use egui::{Color32, Stroke, Visuals};

use crate::core::CueStatus;

/// Farbpalette (Dark-Mode).
///
/// Aufräumen Phase 2 (TODO.md): alle Farben als benannte Konstanten, Text-
/// farben mit WCAG-AA-Kontrast (≥ 4,5:1) auf [`palette::PANEL`] – siehe
/// Unit-Test `text_colors_meet_wcag_aa`. Rot ist ausschließlich „kaputt /
/// Fehler" ([`palette::ERROR`]) bzw. Panic ([`palette::PANIC`]) – ein
/// armierter Cue in Standby ist neutral (siehe
/// [`status_color`]).
pub mod palette {
    use egui::Color32;

    // Flächen (dunkel → hell), siehe docs/UI.md §3.
    /// Fenster-Hintergrund und Eingabefelder.
    pub const BACKGROUND: Color32 = Color32::from_rgb(18, 18, 21);
    /// Cue-Liste (dunkelste große Fläche = Fokus).
    pub const LIST_BG: Color32 = Color32::from_rgb(22, 22, 25);
    /// Jede zweite Cue-Zeile (Zebra-Streifen).
    pub const ROW_STRIPE: Color32 = Color32::from_rgb(27, 27, 31);
    /// Masthead, Toolbox, Sidebar, Inspector, Footer.
    pub const PANEL: Color32 = Color32::from_rgb(33, 33, 38);
    /// Buttons, inaktive Tabs, Karten.
    pub const RAISED: Color32 = Color32::from_rgb(45, 45, 52);
    /// 1-px-Trennlinien.
    pub const BORDER: Color32 = Color32::from_rgb(52, 52, 60);

    /// Primärtext (Namen, Werte ≠ 0).
    pub const TEXT: Color32 = Color32::from_rgb(222, 222, 228);
    /// Sekundärtext: Spaltenköpfe, Labels, Targets, Icons.
    pub const TEXT_DIM: Color32 = Color32::from_rgb(150, 150, 160);
    /// Leere Werte (`00:00.000`, „–") und Deaktiviertes – bewusst unter
    /// WCAG AA, weil „nichts da" zurücktreten soll.
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(92, 92, 102);

    /// Allgemeine Akzentfarbe (Audition-Pill, aktive Panel-Toggles,
    /// Continue-Icons). NICHT für den Cue-Status „Standby" – der ist
    /// neutral (Phase 2).
    pub const ACCENT: Color32 = Color32::from_rgb(255, 176, 0);
    /// Pre-Wait: Amber (Warten vor dem Start).
    pub const PRE_WAIT: Color32 = Color32::from_rgb(255, 145, 0);
    /// Playing: Grün.
    pub const PLAYING: Color32 = Color32::from_rgb(46, 204, 113);
    /// Paused: Blau.
    pub const PAUSED: Color32 = Color32::from_rgb(255, 195, 0); // gelb (F1.1)
    /// Post-Wait: Türkis (Playback vorbei, Cue noch offen).
    pub const POST_WAIT: Color32 = Color32::from_rgb(39, 174, 150);
    /// Completed / ausgegraut (Disarmed).
    pub const COMPLETED: Color32 = Color32::from_rgb(140, 140, 148);
    /// Stopped: Schiefergrau (dezente Grafiken, z. B. GO-Rahmen ohne Standby).
    pub const STOPPED: Color32 = Color32::from_rgb(112, 118, 132);
    /// Broken / Fehler: Rot (Rot = kaputt).
    pub const ERROR: Color32 = Color32::from_rgb(235, 90, 72);

    /// GO-Button.
    pub const GO: Color32 = Color32::from_rgb(39, 174, 96);
    /// Panic/Stop-All (und SHOW-MODE-Label im Footer).
    pub const PANIC: Color32 = Color32::from_rgb(232, 95, 70);

    /// Selektierte Zeile (Blau, unabhängig vom Playhead).
    pub const SELECTION: Color32 = Color32::from_rgb(58, 84, 112);
    /// Playhead-Zeile (dezente Hervorhebung; unter der Selektion).
    pub const CURSOR_ROW: Color32 = Color32::from_rgb(50, 54, 64);
    /// Deckkraft der Cue-Color-Zeilen-Tönung (ganze Zeile leicht
    /// getönt, niedriger Alpha-Wert).
    pub const ROW_TINT: f32 = 0.10;
    /// Footer-Hintergrund im Show-Mode (TODO.md Phase 6: Show-Mode muss
    /// sofort erkennbar sein – eingefärbter Footer + „SHOW"-Label).
    pub const SHOW_MODE_BG: Color32 = Color32::from_rgb(58, 22, 18);
}

/// Größen & Abstände (Layout-Konstanten – keine Magic Numbers in den
/// Panel-Modulen; Phase 1 des UI-Refactorings).
pub mod size {
    use egui::Vec2;

    /// GO-Button (Masthead, groß, mindestens 120×64).
    pub const GO_BUTTON: Vec2 = Vec2::new(132.0, 64.0);
    /// Standby-Anzeige (Schriftgröße).
    pub const STANDBY_TEXT: f32 = 22.0;
    /// Kleine Labels: Spaltenköpfe, Gruppen-Labels, „STANDBY".
    pub const LABEL_TEXT: f32 = 11.0;
    /// Masthead-Höhe (fest).
    pub const MASTHEAD_HEIGHT: f32 = 160.0;
    /// Footer-Höhe (fest).
    pub const FOOTER_HEIGHT: f32 = 38.0;
    /// Inspector-Default-Höhe (ziehbar).
    pub const INSPECTOR_HEIGHT: f32 = 260.0;
    /// Toolbox-Breite (linke Leiste, schmal, einklappbar).
    pub const TOOLBOX_WIDTH: f32 = 136.0;
    /// Toolbox-Zeile (Icon + Label).
    pub const TOOLBOX_ROW_HEIGHT: f32 = 24.0;
    /// Label-Spalte in Formularen (rechtsbündig, docs/UI.md §6.4).
    pub const FORM_LABEL_WIDTH: f32 = 96.0;
    /// Einheitliche Breite von Zeit- und Zahlenfeldern in Formularen.
    pub const FORM_NUMBER_WIDTH: f32 = 88.0;
    /// Sidebar-Breite (rechte Leiste).
    pub const SIDEBAR_WIDTH: f32 = 240.0;
    /// Transport-Buttons im Masthead (Pause/Resume/Reset).
    pub const MASTHEAD_ICON_BUTTON: Vec2 = Vec2::new(32.0, 32.0);
    /// Icon-Button im Footer (Panel-Toggles, Workspace Settings).
    pub const FOOTER_ICON_BUTTON: Vec2 = Vec2::new(32.0, 32.0);
    /// Transport-Icon-Buttons (Sidebar: Reset/Pause/Resume/Panic).
    pub const TRANSPORT_ICON_BUTTON: Vec2 = Vec2::new(26.0, 26.0);
    /// Farbiger Streifen am linken Rand einer Cue-Zeile („Cue Color").
    pub const CUE_COLOR_STRIP: f32 = 3.0;
    /// Pause/Resume/Stop-Buttons im Active-Cues-Tab (TODO.md Phase 6:
    /// gut klickbar, min. 28 px hoch).
    pub const ACTIVE_CUE_BUTTON: Vec2 = Vec2::new(28.0, 28.0);

    /// Einheitliche Abstände (TODO.md Phase 6: nur diese verwenden –
    /// 4/8/12/16 px).
    pub const SPACE_SMALL: f32 = 4.0;
    pub const SPACE_MEDIUM: f32 = 8.0;
    pub const SPACE_MEDIUM_LARGE: f32 = 12.0;
    pub const SPACE_LARGE: f32 = 16.0;
}

/// EINE Status-Farben-Quelle (TODO.md Phase 2 + Phase 0b).
///
/// Semantik: Standby (auch armiert) ist **neutral** – Rot bedeutet
/// „kaputt / Problem" (siehe [`palette::ERROR`]). Die Status-Pille in
/// der Cue-Liste und die Fortschrittsbalken nutzen ausschließlich diese
/// Funktion.
/// Farbe mit Alpha (0–255, gamma-korrigiert wie bisher, siehe
/// docs/TODO-video-routing.md §7) – für dezente Füllungen.
pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    c.gamma_multiply(f32::from(a) / 255.0)
}

pub fn status_color(status: CueStatus) -> Color32 {
    match status {
        CueStatus::Standby => palette::TEXT,
        CueStatus::PreWait => palette::PRE_WAIT,
        CueStatus::Playing => palette::PLAYING,
        CueStatus::Paused => palette::PAUSED,
        CueStatus::PostWait => palette::POST_WAIT,
        CueStatus::Completed => palette::COMPLETED,
        CueStatus::Stopped => palette::STOPPED,
        CueStatus::Error => palette::ERROR,
    }
}

/// Hintergrund von Fortschrittsbalken: dezent genug, dass die Restzeit in
/// Statusfarbe darauf lesbar bleibt. Transparenz immer über `gamma_multiply`:
/// `linear_multiply` und `from_rgba_unmultiplied` rechnen im linearen Raum
/// und wirken auf dunklem Grund viel heller als ihr Wert vermuten lässt.
pub fn progress_fill(color: Color32) -> Color32 {
    color.gamma_multiply(0.18)
}

/// Wendet das Dark-Theme auf den egui-Kontext an (docs/UI.md §3–§6).
pub fn apply_theme(ctx: &egui::Context) {
    // Phosphor-Icons-Font registrieren (idempotent, pro Context einmalig).
    super::icons::install(ctx);

    let mut visuals = Visuals::dark();
    visuals.panel_fill = palette::PANEL;
    visuals.window_fill = palette::PANEL;
    visuals.window_stroke = Stroke::new(1.0_f32, palette::BORDER);
    visuals.extreme_bg_color = palette::BACKGROUND;
    visuals.faint_bg_color = palette::ROW_STRIPE;
    visuals.code_bg_color = palette::BACKGROUND;
    visuals.override_text_color = Some(palette::TEXT);
    visuals.hyperlink_color = palette::PAUSED;
    visuals.selection.bg_fill = palette::SELECTION;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(120, 160, 200));

    // Linien: 1 px, kaum sichtbar (Flächen statt Linien).
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, palette::BORDER);
    visuals.widgets.noninteractive.bg_fill = palette::PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = palette::PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, palette::TEXT_DIM);
    visuals.widgets.inactive.bg_fill = palette::RAISED;
    visuals.widgets.inactive.weak_bg_fill = palette::RAISED;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, palette::TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(58, 58, 68);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(58, 58, 68);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(80, 80, 92));
    visuals.widgets.active.bg_fill = Color32::from_rgb(70, 70, 82);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(70, 70, 82);

    // Abgerundete Widgets (weiche Kanten statt scharfer Kästchen).
    let rounding = egui::Rounding::same(5.0);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.open.rounding = rounding;
    visuals.window_rounding = egui::Rounding::same(8.0);

    // Immer dunkel, auch wenn das System hell ist: egui folgt sonst dem
    // System und nutzt dessen Standard-Stil statt dieser Palette.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);

    // Feste Abstände (4/8/12/16) und Schriftgrößen (docs/UI.md §4).
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = egui::vec2(size::SPACE_MEDIUM, size::SPACE_SMALL + 2.0);
        style.spacing.button_padding = egui::vec2(size::SPACE_MEDIUM, size::SPACE_SMALL);
        style.spacing.interact_size.y = 22.0;
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(18.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(13.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
        ]
        .into();
    });
}

/// Panel-Frame (Fläche `PANEL`) mit Innenabstand – ein Stil für alle
/// Seitenbereiche (docs/UI.md §3).
pub fn panel_frame(margin_x: f32, margin_y: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(palette::PANEL)
        .inner_margin(egui::Margin::symmetric(margin_x, margin_y))
}

/// Kleines Bereichs-Label (11 px, gedimmt, Großbuchstaben) – Toolbox-
/// Gruppen, „STANDBY" (docs/UI.md §4).
pub fn section_label(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(size::LABEL_TEXT)
            .color(palette::TEXT_DIM),
    )
}

/// Formular-Raster: ein Abstand für alle Formulare (docs/UI.md §6.4).
pub fn form_grid(id: &str) -> egui::Grid {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([size::SPACE_MEDIUM_LARGE, size::SPACE_SMALL + 2.0])
}

/// Formular-Label: feste Spaltenbreite, rechtsbündig, `TEXT_DIM`
/// (docs/UI.md §6.4).
pub fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(size::FORM_LABEL_WIDTH, ui.spacing().interact_size.y),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(size::FORM_LABEL_WIDTH);
            ui.label(egui::RichText::new(text).color(palette::TEXT_DIM));
        },
    );
}

/// Listenzeile einer Navigation (Workspace Settings): Selektion = ganze
/// Zeile `SELECTION`, Hover = `RAISED`.
pub fn nav_row(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), size::TOOLBOX_ROW_HEIGHT),
        egui::Sense::click(),
    );
    let fill = if selected {
        palette::SELECTION
    } else if response.hovered() {
        palette::RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    ui.painter().text(
        egui::pos2(rect.left() + size::SPACE_MEDIUM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        palette::TEXT,
    );
    response
}

/// Einheitliche Tab-Leiste (docs/UI.md §6.2): Text-Tabs, aktiv mit
/// 2-px-Unterstrich, darunter eine 1-px-Linie über die volle Breite.
/// Gibt `true` zurück, wenn sich der Tab geändert hat.
pub fn tab_bar<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    current: &mut T,
    tabs: &[(T, String)],
) -> bool {
    let mut changed = false;
    let line_y;
    {
        let r = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = size::SPACE_MEDIUM_LARGE;
            for (value, label) in tabs {
                let active = *current == *value;
                let color = if active {
                    palette::TEXT
                } else {
                    palette::TEXT_DIM
                };
                let text = egui::RichText::new(label).color(color);
                let text = if active { text.strong() } else { text };
                let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if active {
                    let r = resp.rect;
                    ui.painter().line_segment(
                        [
                            egui::pos2(r.left(), r.bottom() + 3.0),
                            egui::pos2(r.right(), r.bottom() + 3.0),
                        ],
                        Stroke::new(2.0_f32, palette::TEXT),
                    );
                } else if resp.hovered() {
                    ui.painter().line_segment(
                        [
                            egui::pos2(resp.rect.left(), resp.rect.bottom() + 3.0),
                            egui::pos2(resp.rect.right(), resp.rect.bottom() + 3.0),
                        ],
                        Stroke::new(2.0_f32, palette::BORDER),
                    );
                }
                if resp.clicked() && !active {
                    *current = *value;
                    changed = true;
                }
            }
        });
        line_y = r.response.rect.bottom() + 4.0;
    }
    let full = ui.max_rect();
    ui.painter().line_segment(
        [
            egui::pos2(full.left(), line_y),
            egui::pos2(full.right(), line_y),
        ],
        Stroke::new(1.0_f32, palette::BORDER),
    );
    ui.add_space(size::SPACE_MEDIUM);
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_color_semantics() {
        // Standby (auch armiert) ist neutral – Rot ist „kaputt"
        // (TODO.md Phase 2).
        assert_eq!(status_color(CueStatus::Standby), palette::TEXT);
        assert_eq!(status_color(CueStatus::PreWait), palette::PRE_WAIT);
        assert_eq!(status_color(CueStatus::Playing), palette::PLAYING);
        assert_eq!(status_color(CueStatus::Paused), palette::PAUSED);
        assert_eq!(status_color(CueStatus::PostWait), palette::POST_WAIT);
        assert_eq!(status_color(CueStatus::Completed), palette::COMPLETED);
        assert_eq!(status_color(CueStatus::Stopped), palette::STOPPED);
        assert_eq!(status_color(CueStatus::Error), palette::ERROR);
    }

    /// WCAG AA: Textfarben auf Panel-Hintergrund brauchen ≥ 4,5:1
    /// (TODO.md Phase 2: „ausreichender Kontrast").
    #[test]
    fn text_colors_meet_wcag_aa() {
        fn luminance(c: Color32) -> f64 {
            fn channel(v: u8) -> f64 {
                let c = f64::from(v) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        fn contrast(a: Color32, b: Color32) -> f64 {
            let (la, lb) = (luminance(a), luminance(b));
            let (hi, lo) = (la.max(lb), la.min(lb));
            (hi + 0.05) / (lo + 0.05)
        }

        for color in [
            palette::TEXT,
            palette::TEXT_DIM,
            palette::ACCENT,
            palette::PRE_WAIT,
            palette::PLAYING,
            palette::PAUSED,
            palette::POST_WAIT,
            palette::COMPLETED,
            palette::ERROR,
            palette::PANIC,
            palette::GO,
        ] {
            let ratio = contrast(color, palette::PANEL);
            assert!(
                ratio >= 4.5,
                "WCAG AA verfehlt: {color:?} auf PANEL = {ratio:.2}"
            );
        }
        // Selektions- und Playhead-Zeile: Text darauf ebenfalls lesbar.
        for bg in [palette::SELECTION, palette::CURSOR_ROW] {
            let ratio = contrast(palette::TEXT, bg);
            assert!(
                ratio >= 4.5,
                "WCAG AA verfehlt: TEXT auf {bg:?} = {ratio:.2}"
            );
        }
    }
}
