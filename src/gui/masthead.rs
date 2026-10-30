//! Masthead (QLab 5): GO-Button + Standby-Anzeige + Notes-Feld +
//! globaler Transport.
//!
//! Layout (docs/UI.md §5.1):
//! * GO-Button LINKS (132×64; grün umrandet, sobald ein Cue bereitsteht;
//!   Space = GO).
//! * Daneben: Label „STANDBY", darunter „Q#  Name" groß, darunter das
//!   Notes-Feld des Standby-Cues bis zum Transport-Block.
//! * RECHTS: Pause All / Resume All / Reset (neutrale Icon-Buttons) und
//!   Panic (beschriftet, roter Rahmen; Esc). GO und Panic sind die einzigen
//!   „lauten" Buttons im Fenster.
//! * Die Toolbox (linke Leiste) ist die einzige Button-Quelle für neue Cues.

use egui::RichText;

use super::app::ShowApp;
use super::icons::{self, Icon};
use super::theme::{self, palette, size};

/// Breite des Transport-Blocks rechts (3 Icon-Buttons + Panic).
const TRANSPORT_WIDTH: f32 = 3.0 * 32.0 + 96.0 + 4.0 * size::SPACE_SMALL;

/// Zeichnet den Masthead (oberer Bereich unter der Menüleiste).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    let editable = !app.show_mode;
    ui.horizontal(|ui| {
        // GO-Button (QLab: groß, dunkel, grün umrandet wenn ein Cue bereitsteht).
        let ready = app.carts.standby().is_some();
        let stroke = if ready {
            egui::Stroke::new(3.0_f32, palette::PLAYING)
        } else {
            egui::Stroke::new(1.0_f32, palette::STOPPED)
        };
        let go = egui::Button::new(
            RichText::new("GO")
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

        let middle_width = (ui.available_width() - TRANSPORT_WIDTH - size::SPACE_LARGE).max(120.0);
        ui.allocate_ui_with_layout(
            egui::vec2(middle_width, size::GO_BUTTON.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(middle_width);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    theme::section_label(ui, "Standby");
                    // Audition-Mode-Badge (QLab: dezentes Pill-Label).
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
                // heißt in QLab „kaputt").
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
                // Notes-Feld (QLab: gebunden an den Standby-Cue).
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

        // Transport rechts (right_to_left: das Erste steht ganz rechts).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = size::SPACE_SMALL;
            if panic_button(ui)
                .on_hover_text("Panic – stop everything immediately (Esc)")
                .clicked()
            {
                app.panic();
            }
            ui.add_space(size::SPACE_SMALL);
            if icons::button(
                ui,
                Icon::Reset,
                palette::TEXT,
                size::MASTHEAD_ICON_BUTTON,
                true,
            )
            .on_hover_text("Reset All – stop all cues, playhead to start")
            .clicked()
            {
                app.reset_all();
            }
            if icons::button(
                ui,
                Icon::Resume,
                palette::TEXT,
                size::MASTHEAD_ICON_BUTTON,
                true,
            )
            .on_hover_text("Resume All – resume all paused cues")
            .clicked()
            {
                app.resume_all();
            }
            if icons::button(
                ui,
                Icon::Pause,
                palette::TEXT,
                size::MASTHEAD_ICON_BUTTON,
                true,
            )
            .on_hover_text("Pause All – pause all running cues")
            .clicked()
            {
                app.pause_all();
            }
        });
    });
}

/// Panic-Button: Icon + „Panic", roter Rahmen, keine rote Fläche
/// (docs/UI.md §6.1 – Primär-Buttons sind umrandet).
fn panic_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 40.0), egui::Sense::click());
    let fill = if response.hovered() {
        palette::PANIC.gamma_multiply(0.18)
    } else {
        palette::RAISED
    };
    ui.painter()
        .rect(rect, 8.0, fill, egui::Stroke::new(2.0_f32, palette::PANIC));
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 14.0, rect.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    icons::paint_at(ui, Icon::Panic, icon_rect, palette::PANIC);
    ui.painter().text(
        egui::pos2(icon_rect.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Panic",
        egui::FontId::proportional(15.0),
        palette::TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
