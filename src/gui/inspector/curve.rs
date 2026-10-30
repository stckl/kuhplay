//! Tab „Curve" (QLab 5, Fade-Cues): gefadeter Parameter, Kurve sowie
//! Start- und Zielwert.
//!
//! KuhPlay-Fades haben kein Ziel-Cue/-Stage (der Parameter des eigenen
//! Cues wird gefadet) – daher gibt es hier kein „Audio Levels"/„Geometry"
//! je nach Ziel. // TODO(qlab-parity): verifizieren, ob QLab 5 Fades mit
//! Ziel-Cue vorsieht.

use egui::RichText;

use crate::core::{CueType, FadeCurve, FadeParam};

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Curve".
pub fn draw_curve(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Fade {
        ui.label(RichText::new("Curve only for fade cues").weak());
        return;
    }
    theme::form_grid("inspector_curve_grid").show(ui, |ui| {
        theme::field_label(ui, "Param");
        let mut param = cue.fade_param;
        egui::ComboBox::from_id_salt("fade_param_combo")
            .selected_text(param.label())
            .show_ui(ui, |ui| {
                for p in FadeParam::ALL {
                    ui.selectable_value(&mut param, p, p.label());
                }
            });
        if param != cue.fade_param {
            cue.fade_param = param;
        }
        ui.end_row();

        theme::field_label(ui, "Curve");
        let mut curve = cue.fade_curve;
        egui::ComboBox::from_id_salt("fade_curve_combo")
            .selected_text(curve.label())
            .show_ui(ui, |ui| {
                for c in FadeCurve::ALL {
                    ui.selectable_value(&mut curve, c, c.label());
                }
            });
        if curve != cue.fade_curve {
            cue.fade_curve = curve;
        }
        ui.end_row();

        theme::field_label(ui, "Fade from");
        let mut from = cue.from_volume.unwrap_or(1.0);
        if ui
            .add(egui::DragValue::new(&mut from).speed(0.01).range(0.0..=1.0))
            .changed()
        {
            cue.from_volume = Some(from);
        }
        ui.end_row();

        theme::field_label(ui, "Fade to");
        // Rate darf > 1 sein (Speed-up), Volume/Opacity bleiben 0..1.
        let max = if cue.fade_param == FadeParam::Rate {
            4.0
        } else {
            1.0
        };
        let mut volume = cue.target_volume.unwrap_or(1.0);
        if ui
            .add(
                egui::DragValue::new(&mut volume)
                    .speed(0.01)
                    .range(0.0..=max),
            )
            .changed()
        {
            cue.target_volume = Some(volume);
        }
        ui.end_row();
    });
}
