//! Masthead: GO-Button + Standby-Anzeige + Notes-Feld +
//! globaler Transport.
//!
//! Layout (docs/UI.md §5.1):
//! * GO-Button LINKS (132×64; grün umrandet, sobald ein Cue bereitsteht;
//!   Space = GO).
//! * Daneben: Label „STANDBY", darunter „Q#  Name" groß, darunter das
//!   Notes-Feld des Standby-Cues.
//! * Darunter die Toolbar: schwarze Buttons für die Cue-Typen in den Gruppen
//!   der Toolbox, ausblendbar im Footer.
//! * Die Transport-Buttons liegen oben in der Sidebar.

use egui::RichText;

use super::app::ShowApp;
use super::icons::Icon;
use super::theme::{self, palette, size};

/// Zeichnet den Masthead (oberer Bereich unter der Menüleiste).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    let editable = !app.show_mode;
    draw_go_row(ui, app, editable);
    if app.ui.toolbar_visible {
        ui.add_space(size::SPACE_SMALL);
        draw_toolbar(ui, app, editable);
    }
}

/// Toolbar: schwarze Icon-Buttons auf einer Linie, je Toolbox-Gruppe durch
/// eine größere Lücke getrennt.
fn draw_toolbar(ui: &mut egui::Ui, app: &mut ShowApp, editable: bool) {
    const BUTTON_GAP: f32 = 2.0;
    const GROUP_GAP: f32 = 12.0;
    let button = size::MASTHEAD_ICON_BUTTON;
    let (strip, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), button.y),
        egui::Sense::hover(),
    );
    let mut x = strip.left();
    for (group, types) in super::toolbox::ordered_groups(&app.ui.toolbox_order) {
        for cue_type in types {
            let rect = egui::Rect::from_min_size(egui::pos2(x, strip.top()), button);
            x += button.x + BUTTON_GAP;
            let sense = if editable {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let response = ui
                .allocate_rect(rect, sense)
                .on_hover_text(format!("{group}: New {} Cue", cue_type.label()));
            let fill = if editable && response.hovered() {
                palette::RAISED
            } else {
                egui::Color32::BLACK
            };
            ui.painter().rect_filled(rect, 4.0, fill);
            let color = if editable {
                palette::TEXT
            } else {
                palette::TEXT_FAINT
            };
            Icon::for_cue_type(cue_type).paint(ui.painter(), rect.shrink(6.0), color);
            if response.clicked() {
                app.add_cue(cue_type);
            }
        }
        x += GROUP_GAP - BUTTON_GAP;
    }
}

fn draw_go_row(ui: &mut egui::Ui, app: &mut ShowApp, editable: bool) {
    ui.horizontal(|ui| {
        // GO-Button (groß, dunkel, grün umrandet wenn ein Cue bereitsteht).
        // F1.3: roter Rahmen, solange der Doppel-GO-Schutz aktiv ist (pending_go);
        // im Audition-Mode heißt der Button „AUDITION“.
        let ready = app.carts.standby().is_some();
        let stroke = if app.pending_go {
            egui::Stroke::new(3.0_f32, palette::PANIC)
        } else if ready {
            egui::Stroke::new(3.0_f32, palette::PLAYING)
        } else {
            egui::Stroke::new(1.0_f32, palette::STOPPED)
        };
        let go_label = if app.audition_mode { "AUDITION" } else { "GO" };
        let go = egui::Button::new(
            RichText::new(go_label)
                .size(30.0)
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(palette::RAISED)
        .stroke(stroke)
        .rounding(egui::Rounding::same(8.0))
        .min_size(size::GO_BUTTON);
        if ui
            .add(go)
            .on_hover_text("GO – start the next cue (Space)")
            .clicked()
        {
            app.go_now();
        }
        ui.add_space(size::SPACE_LARGE);

        let middle_width = (ui.available_width() - size::SPACE_LARGE).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(middle_width, size::GO_BUTTON.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(middle_width);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    theme::section_label(ui, "Standby");
                    // Audition-Mode-Badge (dezentes Pill-Label).
                    if app.audition_mode {
                        egui::Frame::none()
                            .fill(palette::ACCENT.gamma_multiply(0.15))
                            .rounding(egui::Rounding::same(8.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("AUDITION")
                                        .color(palette::ACCENT)
                                        .size(size::LABEL_TEXT)
                                        .strong(),
                                )
                            })
                            .inner
                            .on_hover_text(
                                "Audition Mode active: video cues play without a stage target",
                            );
                    }
                });
                // Standby-Display: „Q# • Name" (Standby ist neutral – Rot
                // heißt „kaputt").
                match app.carts.standby() {
                    Some(cue) => {
                        ui.label(
                            RichText::new(format!("{} • {}", cue.number, cue.name))
                                .color(palette::TEXT)
                                .size(size::STANDBY_TEXT)
                                .strong(),
                        )
                        .on_hover_text("Playhead – the next GO starts this cue");
                    }
                    None => {
                        ui.label(
                            RichText::new("No cue standing by")
                                .color(palette::TEXT_FAINT)
                                .size(size::STANDBY_TEXT),
                        );
                    }
                }
                ui.add_space(size::SPACE_SMALL);
                // Notes-Feld (gebunden an den Standby-Cue).
                if editable {
                    let width = ui.available_width();
                    let r = match app.carts.standby_mut() {
                        Some(cue) => ui.add(
                            egui::TextEdit::singleline(&mut cue.notes)
                                .hint_text("Notes (standby cue)…")
                                .desired_width(width),
                        ),
                        None => ui.add_enabled(
                            false,
                            egui::TextEdit::singleline(&mut String::new())
                                .hint_text("Notes (standby cue)…")
                                .desired_width(width),
                        ),
                    };
                    // Für das Hotkey-Gate registrieren („Textfeld fokussiert?“).
                    app.text_field_ids.insert(r.id);
                } else if let Some(cue) = app.carts.standby() {
                    ui.label(RichText::new(&cue.notes).color(palette::TEXT_DIM));
                }
            },
        );
    });
}
