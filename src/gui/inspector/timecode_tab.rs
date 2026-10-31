//! Inspector-Tab „Timecode" (Timecode-Cue, QLab 5): Trigger-Zeitcode
//! (`HH:MM:SS:FF`) + Bildrate. Der Cue feuert, wenn der externe Timecode
//! (OSC `/kuhplay/timecode "01:00:00:00" 25`, später LTC/MTC) den Trigger
//! erreicht. Manuelles GO verhält sich wie ein Wait-Cue mit `duration`.

use egui::RichText;

use crate::core::{CueType, Fps, Timecode};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Timecode" (Timecode-Cue): Trigger-Zeitcode + FPS.
pub fn draw_timecode(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Timecode {
        ui.label(RichText::new("Timecode only for timecode cues").weak());
        return;
    }
    if cue.tc_trigger.is_none() {
        cue.tc_trigger = Some(Timecode::default());
    }
    let Some(tc) = &mut cue.tc_trigger else {
        return;
    };

    theme::form_grid("inspector_timecode_grid").show(ui, |ui| {
        theme::field_label(ui, "Trigger");
        // Anzeige HH:MM:SS:FF als vier DragValues.
        let fps = tc.fps.frames_per_second();
        let total = tc.frames;
        let mut h = total / (fps * 3600);
        let mut m = (total / (fps * 60)) % 60;
        let mut s = (total / fps) % 60;
        let mut f = total % fps;
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut h)
                    .speed(1.0)
                    .range(0..=99)
                    .prefix("h"),
            );
            ui.label(":");
            ui.add(
                egui::DragValue::new(&mut m)
                    .speed(1.0)
                    .range(0..=59)
                    .prefix("m"),
            );
            ui.label(":");
            ui.add(
                egui::DragValue::new(&mut s)
                    .speed(1.0)
                    .range(0..=59)
                    .prefix("s"),
            );
            ui.label(":");
            ui.add(
                egui::DragValue::new(&mut f)
                    .speed(1.0)
                    .range(0..=fps - 1)
                    .prefix("f"),
            );
        });
        tc.frames = (h * 3600 + m * 60 + s) * fps + f;
        ui.end_row();

        theme::field_label(ui, "FPS");
        let mut fps_sel = match tc.fps {
            Fps::Fps24 => 24u64,
            Fps::Fps25 => 25,
            Fps::Fps30 => 30,
        };
        egui::ComboBox::from_id_salt("timecode_fps_combo")
            .selected_text(format!("{fps_sel} fps"))
            .show_ui(ui, |ui| {
                for v in [24u64, 25, 30] {
                    ui.selectable_value(&mut fps_sel, v, format!("{v} fps"));
                }
            });
        tc.fps = Fps::from_fps(fps_sel);
        ui.end_row();

        theme::field_label(ui, "Trigger at");
        ui.label(
            RichText::new(format!("{tc}"))
                .monospace()
                .color(theme::palette::TEXT),
        );
        ui.end_row();
    });

    ui.add_space(theme::size::SPACE_SMALL);
    ui.label(
        RichText::new(
            "Fires when incoming timecode reaches the trigger. Input e.g. OSC: \
             /kuhplay/timecode \"01:00:00:00\" 25  (LTC/MTC receiver planned). \
             Manual GO acts like a Wait cue with the cue's duration.",
        )
        .small()
        .color(theme::palette::TEXT_DIM),
    );
    if cue.tc_trigger.is_none() {
        ui.label(
            RichText::new("No trigger set (cue would be broken)").color(theme::palette::ERROR),
        );
    }
}
