//! Toolbox: linke Leiste mit den Cue-Typ-Buttons – die EINZIGE
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

/// Cue-Typ-Gruppen der Toolbox.
const GROUPS: &[(&str, &[CueType])] = &[
    (
        "Media",
        &[
            CueType::Audio,
            CueType::Video,
            CueType::Html,
            CueType::SlideShow,
            CueType::Text,
        ],
    ),
    ("Fade", &[CueType::Fade, CueType::Crossfade]),
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
    // Wait/Memo: Timer- und Notiz-Cues ohne eigenes Medium.
    ("Misc", &[CueType::Wait, CueType::Memo, CueType::Timecode]),
    // Network/MIDI: UDP/TCP-Nachricht bzw. MIDI an Ziele im Show-Netz.
    (
        "Network",
        &[CueType::Network, CueType::Midi, CueType::Companion],
    ),
    // Lighting: Art-Net DMX ins Licht-Netz.
    ("Lighting", &[CueType::Light]),
];

/// Cue-Typen je Gruppe in der vom Benutzer festgelegten Reihenfolge.
pub fn ordered_groups(order: &[CueType]) -> Vec<(&'static str, Vec<CueType>)> {
    GROUPS
        .iter()
        .map(|(group, types)| {
            let mut types = types.to_vec();
            types.sort_by_key(|t| order.iter().position(|o| o == t).unwrap_or(usize::MAX));
            (*group, types)
        })
        .collect()
}

/// Verschiebt einen Cue-Typ innerhalb seiner Gruppe und liefert die neue Reihenfolge.
fn moved_order(order: &[CueType], cue_type: CueType, delta: isize) -> Vec<CueType> {
    let mut groups = ordered_groups(order);
    for (_, types) in &mut groups {
        if let Some(index) = types.iter().position(|t| *t == cue_type) {
            let target = index as isize + delta;
            if target >= 0 && (target as usize) < types.len() {
                types.swap(index, target as usize);
            }
        }
    }
    groups.into_iter().flat_map(|(_, types)| types).collect()
}

/// Zeichnet die Toolbox (linkes SidePanel, ~136 px breit).
///
/// Jeder Cue-Typ ist eine Listenzeile: Icon mit eigenem Platz links,
/// Label daneben, Hover hellt die Zeile auf (docs/UI.md §5.3).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    let editable = !app.show_mode;
    // Sichtbarer Balken: Auf kleinen Displays muss klar sein, dass mehr da ist.
    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (gi, (group, types)) in ordered_groups(&app.ui.toolbox_order)
            .into_iter()
            .enumerate()
        {
            if gi > 0 {
                ui.add_space(size::SPACE_MEDIUM);
            }
            ui.horizontal(|ui| {
                ui.add_space(size::SPACE_SMALL);
                theme::section_label(ui, group);
            });
            for cue_type in types {
                let response = type_row(ui, cue_type, editable)
                    .on_hover_text(format!("New {} Cue", cue_type.label()));
                if response.clicked() {
                    app.add_cue(cue_type);
                }
                response.context_menu(|ui| {
                    for (label, delta) in [("Move Up", -1), ("Move Down", 1)] {
                        if ui.button(label).clicked() {
                            app.ui.toolbox_order =
                                moved_order(&app.ui.toolbox_order, cue_type, delta);
                            ui.close_menu();
                        }
                    }
                });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_moves_within_its_group_only() {
        let order = moved_order(&[], CueType::Video, 1);
        let media = ordered_groups(&order).remove(0).1;
        assert!(
            media.iter().position(|t| *t == CueType::Video)
                > media.iter().position(|t| *t == CueType::Html)
        );
        // Fade-Gruppe hat zwei Typen (Fade, Crossfade): Fade rutscht innerhalb
        // der Gruppe nach hinten, Crossfade bleibt letzter der Gruppe.
        let moved = moved_order(&order, CueType::Fade, 1);
        assert_ne!(moved, order, "Gruppe mit zwei Typen bewegt sich");
        let fade_pos = |o: &Vec<CueType>| o.iter().position(|t| *t == CueType::Fade).unwrap();
        let xf_pos = |o: &Vec<CueType>| o.iter().position(|t| *t == CueType::Crossfade).unwrap();
        assert!(fade_pos(&moved) > fade_pos(&order));
        assert!(
            xf_pos(&moved) < fade_pos(&moved),
            "Crossfade rutscht innerhalb der Gruppe vor Fade"
        );
    }
}
