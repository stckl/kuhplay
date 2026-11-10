//! Tab „Curve" (Fade-Cues, siehe docs/AUFTRAG-fade.md).
//!
//! Fade-Cue: Ziel-Cue, Zielwerte für Volume und Opacity (unabhängig aktivierbar),
//! Stop-when-done, Kurve (gilt für Volume; Opacity läuft immer linear).

use egui::RichText;

use crate::core::{Cue, CueType, FadeCurve};

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Curve".
pub fn draw_curve(ui: &mut egui::Ui, app: &mut ShowApp) {
    let choices = cue_choices(app);
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    match cue.cue_type {
        CueType::Fade => draw_fade(ui, cue, &choices),
        _ => {
            ui.label(RichText::new("Curve only for fade cues").weak());
        }
    }
}

/// Nummern aller Cues im aktiven Cart (Ziel-Auswahl) als (label, id).
fn cue_choices(app: &ShowApp) -> Vec<(String, uuid::Uuid)> {
    let list = app.carts.active();
    list.cues()
        .iter()
        .map(|c| (format!("{} · {}", c.number, c.name), c.id))
        .collect()
}

/// Ziel-Combo („keins" + alle Cues).
fn target_combo(
    ui: &mut egui::Ui,
    id_salt: &str,
    choices: &[(String, uuid::Uuid)],
    target: &mut Option<uuid::Uuid>,
) {
    let selected = target
        .and_then(|id| {
            choices
                .iter()
                .find(|(_, cid)| *cid == id)
                .map(|(l, _)| l.clone())
        })
        .unwrap_or_else(|| "— none —".to_string());
    let response = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected.to_string())
        .show_ui(ui, |ui| {
            ui.selectable_value(target, None, "— none —");
            for (label, id) in choices {
                ui.selectable_value(target, Some(*id), label);
            }
        })
        .response
        .on_hover_text("Pick a cue or drag one here from the cue list");
    // Cue aus der Liste hierher gezogen.
    if let Some(id) = super::dropped_cue(ui, &response) {
        if choices.iter().any(|(_, cid)| *cid == id) {
            *target = Some(id);
        }
    }
}

/// Optionaler 0..1-Zielwert: Checkbox + DragValue.
fn optional_level(ui: &mut egui::Ui, label: &str, value: &mut Option<f32>) {
    theme::field_label(ui, label);
    let mut active = value.is_some();
    if ui.checkbox(&mut active, "").changed() {
        *value = if active {
            Some(value.unwrap_or(1.0))
        } else {
            None
        };
    }
    if active {
        let mut v = value.unwrap_or(1.0);
        if ui
            .add(egui::DragValue::new(&mut v).speed(0.01).range(0.0..=1.0))
            .changed()
        {
            *value = Some(v);
        }
    }
    ui.end_row();
}

fn draw_fade(ui: &mut egui::Ui, cue: &mut Cue, choices: &[(String, uuid::Uuid)]) {
    theme::form_grid("inspector_curve_grid").show(ui, |ui| {
        theme::field_label(ui, "Target");
        target_combo(ui, "fade_target_combo", choices, &mut cue.fade_target);
        ui.end_row();

        optional_level(ui, "Volume to", &mut cue.fade_volume);
        optional_level(ui, "Opacity to", &mut cue.fade_opacity);

        theme::field_label(ui, "Stop when done");
        ui.checkbox(&mut cue.stop_when_done, "stop target at end of fade");
        ui.end_row();

        theme::field_label(ui, "Curve (volume)");
        let mut curve = cue.fade_curve;
        egui::ComboBox::from_id_salt("fade_curve_combo")
            .selected_text(curve.label().to_string())
            .show_ui(ui, |ui| {
                // Equal-Power-Kurven sind nur intern für Crossfades.
                for c in [FadeCurve::Linear, FadeCurve::Decibel] {
                    ui.selectable_value(&mut curve, c, c.label());
                }
            });
        if curve != cue.fade_curve {
            cue.fade_curve = curve;
        }
        ui.end_row();
    });
}
