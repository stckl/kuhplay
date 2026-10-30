//! Tab „Time & Loops" (QLab 5, Audio/Video): Loop-Modus, Media-Slices
//! (Segment-Plan mit Play-Counts) und große Waveform-Vorschau.
//!
//! Zeiten werden als Timecode `MM:SS.mmm` angezeigt/editiert
//! (siehe [`super::time_field`]).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use egui::RichText;

use crate::core::{CueType, LoopMode, MediaSlice};
use crate::engine::waveform::{Waveform, WAVEFORM_BUCKETS};

use super::{theme, time_field};
use crate::gui::app::ShowApp;

/// Tab „Time & Loops".
pub fn draw_time_loops(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Waveform-Cache (disjunkt borgen – Peaks werden on-demand berechnet).
    let waveforms = &mut app.waveforms;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    theme::form_grid("inspector_time_loops_grid").show(ui, |ui| {
        theme::field_label(ui, "Loop");
        ui.horizontal(|ui| {
            let mut mode = cue.loop_mode;
            egui::ComboBox::from_id_salt("loop_mode_combo")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, LoopMode::Once, LoopMode::Once.label());
                    ui.selectable_value(&mut mode, LoopMode::Count(2), LoopMode::Count(2).label());
                    ui.selectable_value(&mut mode, LoopMode::Count(5), LoopMode::Count(5).label());
                    ui.selectable_value(&mut mode, LoopMode::Infinite, LoopMode::Infinite.label());
                });
            if mode != cue.loop_mode {
                cue.loop_mode = mode;
            }
            if let LoopMode::Count(n) = &mut cue.loop_mode {
                ui.add(egui::DragValue::new(n).range(1..=999));
            }
        });
        ui.end_row();

        if matches!(cue.cue_type, CueType::Audio | CueType::Video) {
            // --- Media-Slices (QLab: Segment-Plan mit Play-Counts) ---
            theme::field_label(ui, "Slices");
            ui.vertical(|ui| {
                if cue.slices.is_empty() {
                    ui.label(RichText::new("no slices – loop mode applies").weak());
                }
                let mut remove: Option<usize> = None;
                for (i, slice) in cue.slices.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("#{}", i + 1));
                        ui.label(RichText::new("start").color(theme::palette::TEXT_DIM));
                        time_field(ui, &mut slice.start);
                        ui.label(RichText::new("end").color(theme::palette::TEXT_DIM));
                        time_field(ui, &mut slice.end);
                        ui.label(RichText::new("plays").color(theme::palette::TEXT_DIM));
                        ui.add(egui::DragValue::new(&mut slice.play_count).range(1..=999));
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    cue.slices.remove(i);
                }
                if ui.button("+ Add Slice").clicked() {
                    cue.slices
                        .push(MediaSlice::new(Duration::ZERO, Duration::from_secs(1), 1));
                }
            });
            ui.end_row();

            // Waveform-Vorschau (nur lokale Dateien, nicht Streams) –
            // hier groß (QLab: „Time & Loops" zeigt die Waveform).
            if let Some(path) = &cue.target_path {
                if !path.starts_with("http://") && !path.starts_with("https://") {
                    theme::field_label(ui, "Waveform");
                    let waveform = waveforms.get_or_compute(Path::new(path), WAVEFORM_BUCKETS);
                    draw_waveform(ui, waveform, 96.0);
                    ui.end_row();
                }
            }
        }
    });
}

/// Waveform-Vorschau: min/max-Balken pro Bucket (egui-Painter, ohne
/// Plot-Dependency). `None` = Datei nicht dekodierbar.
fn draw_waveform(ui: &mut egui::Ui, waveform: Option<Arc<Waveform>>, height: f32) {
    let Some(waveform) = waveform else {
        ui.label(RichText::new("no waveform").weak());
        return;
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    response.on_hover_text(format!(
        "{} buckets · {:.1} s · {} Hz",
        waveform.bucket_count(),
        waveform.duration.as_secs_f64(),
        waveform.sample_rate
    ));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::palette::BACKGROUND);
    let mid = rect.center().y;
    let half = rect.height() / 2.0 - 2.0;
    painter.hline(
        rect.x_range(),
        mid,
        egui::Stroke::new(1.0, theme::palette::STOPPED),
    );
    let n = waveform.bucket_count().max(1);
    let bw = rect.width() / n as f32;
    let stroke = egui::Stroke::new(1.0, theme::palette::PLAYING);
    for (i, &(min, max)) in waveform.buckets.iter().enumerate() {
        let x = rect.left() + (i as f32 + 0.5) * bw;
        let y_top = mid - max.clamp(0.0, 1.0) * half;
        let y_bottom = mid - min.clamp(-1.0, 0.0) * half;
        painter.vline(x, y_top..=y_bottom.max(y_top + 1.0), stroke);
    }
}
