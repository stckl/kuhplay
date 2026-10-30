//! Tab „Audio Levels" (QLab 5, Audio/Video): Wiedergabe-Lautstärke (mit
//! Live-Update laufender Cues) und Routing: Matrix Datei-Kanal → Audio-Kanal.
//! Die Kanäle liegen in den Workspace Settings (Audio) auf Geräte-Ausgängen.

use egui::RichText;

use crate::core::{linear_to_db_clamped, AudioRoute, CueStatus, CueType, AUDIO_CHANNEL_COUNT};
use crate::engine::EngineCommand;

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Audio Levels".
pub fn draw_audio_levels(ui: &mut egui::Ui, app: &mut ShowApp) {
    let engine = &app.engine;
    let patch = &app.audio_patch;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if !matches!(cue.cue_type, CueType::Audio | CueType::Video) {
        ui.label(RichText::new("Audio Levels only for audio and video cues").weak());
        return;
    }
    theme::form_grid("inspector_audio_levels_grid").show(ui, |ui| {
        theme::field_label(ui, "Volume");
        ui.horizontal(|ui| {
            let mut volume = cue.volume;
            if ui
                .add(egui::Slider::new(&mut volume, 0.0..=1.0).max_decimals(2))
                .changed()
            {
                cue.volume = volume;
                // Live-Update, wenn der Cue gerade läuft.
                if cue.status == CueStatus::Playing {
                    let _ = engine.send(EngineCommand::SetVolume {
                        cue_id: cue.id,
                        volume,
                    });
                }
            }
            ui.label(format!("{:.1} dB", linear_to_db_clamped(volume)));
        });
        ui.end_row();

        // Ohne eigene Route wird eine Stereo-Route angezeigt und erst bei der
        // ersten Änderung übernommen.
        let mut route = cue.route.unwrap_or_else(|| AudioRoute::new(2));
        let mut changed = false;

        theme::field_label(ui, "File Channels");
        ui.horizontal(|ui| {
            let before = route.channels;
            changed |= ui
                .add(egui::DragValue::new(&mut route.channels).range(1..=16))
                .on_hover_text("Channels the file is decoded to")
                .changed();
            // Neu hinzugekommene Kanäle sind sonst in der Matrix stumm.
            if let Some(matrix) = &mut route.matrix {
                let added = usize::from(before)..usize::from(route.channels);
                for (source, mask) in matrix.iter_mut().enumerate() {
                    if added.contains(&source) {
                        *mask = 1 << source;
                    }
                }
            }
            ui.add_space(theme::size::SPACE_MEDIUM);
            ui.label(RichText::new("Pan").color(theme::palette::TEXT_DIM));
            changed |= ui
                .add(egui::Slider::new(&mut route.pan, -1.0..=1.0).max_decimals(2))
                .changed();
            if ui
                .small_button("Reset")
                .on_hover_text("Back to the default: channel 1 to Ch 1, channel 2 to Ch 2, …")
                .clicked()
            {
                cue.route = None;
            }
        });
        ui.end_row();

        theme::field_label(ui, "Routing");
        egui::Grid::new("inspector_audio_matrix")
            .spacing([2.0, 2.0])
            .show(ui, |ui| {
                ui.label("");
                for (channel, info) in patch.channels.iter().enumerate() {
                    let output = info.output_summary();
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new((channel + 1).to_string())
                                .size(theme::size::LABEL_TEXT)
                                .color(theme::palette::TEXT_DIM),
                        )
                        .on_hover_text(format!("{} ({output})", info.name));
                    });
                }
                ui.end_row();
                for source in 0..usize::from(route.channels).min(AUDIO_CHANNEL_COUNT) {
                    ui.label(
                        RichText::new(format!("File {}", source + 1))
                            .color(theme::palette::TEXT_DIM),
                    );
                    for channel in 0..AUDIO_CHANNEL_COUNT {
                        let mut connected = route.channel_mask(source) & (1 << channel) != 0;
                        let hover = format!(
                            "File channel {} to {}",
                            source + 1,
                            patch.channels[channel].name
                        );
                        if ui
                            .checkbox(&mut connected, "")
                            .on_hover_text(hover)
                            .changed()
                        {
                            route.toggle(source, channel);
                            changed = true;
                        }
                    }
                    ui.end_row();
                }
            });
        ui.end_row();

        if changed {
            cue.route = Some(route);
        }
    });
}
