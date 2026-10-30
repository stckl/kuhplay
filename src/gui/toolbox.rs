//! Toolbox (QLab 5): linke Leiste mit den Cue-Typ-Buttons – die EINZIGE
//! Button-Quelle für neue Cues (die Masthead-Toolbar entfällt,
//! TODO.md Phase 1).
//!
//! Listenzeilen mit Icon + Beschriftung (Typ-Name), gruppiert (Media /
//! Fade / Control / Data). Breite ~136 px; einklappbar über View-Menü / ⌘K.
//! Layout-Regeln: docs/UI.md §5.3.

use egui::Vec2;

use crate::core::CueType;

use super::app::ShowApp;
use super::icons::{self, Icon};
use super::theme::{self, palette, size};

/// Cue-Typ-Gruppen der Toolbox (QLab-ähnliche Gruppierung).
const GROUPS: &[(&str, &[CueType])] = &[
    (
        "Media",
        &[
            CueType::Audio,
            CueType::Video,
            CueType::Html,
            CueType::SlideShow,
        ],
    ),
    ("Fade", &[CueType::Fade]),
    (
        "Control",
        &[
            CueType::Group,
            CueType::Stop,
            CueType::Script,
            CueType::Control,
        ],
    ),
    ("Data", &[CueType::HtmlData]),
];

/// Zeichnet die Toolbox (linkes SidePanel, ~136 px breit).
///
/// Jeder Cue-Typ ist eine Listenzeile: Icon mit eigenem Platz links,
/// Label daneben, Hover hellt die Zeile auf (docs/UI.md §5.3).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    let editable = !app.show_mode;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (gi, (group, types)) in GROUPS.iter().enumerate() {
            if gi > 0 {
                ui.add_space(size::SPACE_MEDIUM_LARGE);
            }
            ui.horizontal(|ui| {
                ui.add_space(size::SPACE_SMALL);
                theme::section_label(ui, group);
            });
            for cue_type in *types {
                if type_row(ui, *cue_type, editable)
                    .on_hover_text(format!("New {} Cue", cue_type.label()))
                    .clicked()
                {
                    app.add_cue(*cue_type);
                }
            }
        }
    });
}

/// Eine Toolbox-Zeile: [Icon 16 px] 8 px [Label].
fn type_row(ui: &mut egui::Ui, cue_type: CueType, enabled: bool) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), size::TOOLBOX_ROW_HEIGHT),
        sense,
    );
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 5.0, palette::RAISED);
    }
    let (icon_color, text_color) = if enabled {
        (palette::TEXT_DIM, palette::TEXT)
    } else {
        (palette::TEXT_FAINT, palette::TEXT_FAINT)
    };
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + size::SPACE_MEDIUM, rect.center().y - 8.0),
        Vec2::splat(16.0),
    );
    icons::paint_at(ui, Icon::for_cue_type(cue_type), icon_rect, icon_color);
    ui.painter().text(
        egui::pos2(icon_rect.right() + size::SPACE_MEDIUM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        cue_type.label(),
        egui::FontId::proportional(13.5),
        text_color,
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
