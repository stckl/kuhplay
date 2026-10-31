//! Tab „Triggers": Timecode-In (Chasing) für den selektierten Cue.
//!
//! MIDI-, OSC- und Network-Trigger sind geplant (siehe `docs/BACKLOG.md`).

use std::time::Duration;

use egui::RichText;

use crate::core::{format_timecode, Fps, Timecode};
use crate::engine::EngineCommand;

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Triggers".
pub fn draw_triggers(ui: &mut egui::Ui, app: &mut ShowApp) {
    let engine = &app.engine;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    theme::form_grid("cue_triggers_grid").show(ui, |ui| {
        theme::field_label(ui, "Timecode In");
        let mut chasing = cue.timecode_in.is_some();
        if ui
            .checkbox(&mut chasing, "cue follows external timecode (LTC/MTC)")
            .changed()
        {
            if chasing {
                let tc = cue
                    .timecode_in
                    .unwrap_or_else(|| Timecode::from_duration(Duration::ZERO, Fps::Fps25));
                cue.timecode_in = Some(tc);
            } else {
                cue.timecode_in = None;
            }
        }
        ui.end_row();

        if let Some(tc) = &mut cue.timecode_in {
            theme::field_label(ui, "FPS");
            let mut fps = tc.fps;
            egui::ComboBox::from_id_salt("timecode_fps_combo")
                .selected_text(fps.label())
                .show_ui(ui, |ui| {
                    for f in Fps::ALL {
                        ui.selectable_value(&mut fps, f, f.label());
                    }
                });
            if fps != tc.fps {
                tc.fps = fps;
            }
            ui.end_row();

            theme::field_label(ui, "Offset");
            ui.label(format_timecode(tc.to_duration()));
            ui.end_row();

            theme::field_label(ui, "Chase");
            if ui
                .button("Start Timecode Chasing")
                .on_hover_text("Send EngineCommand::ChaseTimecode to the engine")
                .clicked()
            {
                let command = EngineCommand::ChaseTimecode {
                    cue_id: cue.id,
                    timecode_in: Some(*tc),
                };
                if let Err(err) = engine.send(command) {
                    tracing::warn!(%err, "inspector: chase_timecode send failed");
                }
            }
            ui.end_row();
        }
    });
    ui.separator();
    ui.label(
        RichText::new("MIDI, OSC and network triggers are not available yet.")
            .weak()
            .small(),
    );
}
