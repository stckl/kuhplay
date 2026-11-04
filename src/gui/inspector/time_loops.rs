//! Tab „Time & Loops" (Audio/Video): Loop-Modus, Media-Slices
//! (Segment-Plan mit Play-Counts) und große Waveform-Vorschau.
//!
//! Zeiten werden als Timecode `MM:SS.mmm` angezeigt/editiert
//! (siehe [`super::time_field`]).

use std::path::Path;
use std::time::Duration;

use egui::RichText;

use crate::core::{CueType, LoopMode, MediaSlice};
use crate::engine::waveform::{Waveform, WaveformState, WAVEFORM_BUCKETS};

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
            // --- Von/Bis (In/Out): spielt nur [From, To) des Mediums ---
            // (ein Slice; für Segmentpläne siehe „Slices" darunter).
            theme::field_label(ui, "Play from/to");
            let is_local = cue
                .target_path
                .as_ref()
                .is_some_and(|p| !p.starts_with("http://") && !p.starts_with("https://"));
            let waveform_state = cue
                .target_path
                .as_ref()
                .filter(|_| is_local)
                .map(|p| waveforms.request(Path::new(p), WAVEFORM_BUCKETS));
            let media_end = match &waveform_state {
                Some(WaveformState::Ready(waveform)) => Some(waveform.duration),
                _ => None,
            };
            ui.horizontal(|ui| {
                let mut from = cue.slices.first().map(|s| s.start).unwrap_or_default();
                // „Bis zum Ende" (SLICE_TO_END, z. B. aus alten Shows) als
                // Medienende anzeigen, sobald es bekannt ist.
                let mut to = cue
                    .slices
                    .first()
                    .map(|s| match media_end {
                        Some(end) if s.end > end => end,
                        _ => s.end,
                    })
                    .or(media_end)
                    .unwrap_or_default();
                let single = cue.slices.len() <= 1;
                ui.add_enabled_ui(single, |ui| {
                    ui.label(RichText::new("from").color(theme::palette::TEXT_DIM));
                    let r1 = time_field(ui, &mut from);
                    ui.label(RichText::new("to").color(theme::palette::TEXT_DIM));
                    let r2 = time_field(ui, &mut to);
                    if r1.changed() || r2.changed() {
                        if to <= from {
                            to = media_end
                                .filter(|end| *end > from)
                                .unwrap_or(from + Duration::from_millis(100));
                        }
                        if cue.slices.len() == 1 {
                            cue.slices[0].start = from;
                            cue.slices[0].end = to;
                        } else {
                            cue.slices = vec![MediaSlice {
                                start: from,
                                end: to,
                                play_count: 1,
                            }];
                        }
                    }
                });
                if single && !cue.slices.is_empty() && ui.small_button("Whole Clip").clicked() {
                    cue.slices.clear();
                }
                if !single {
                    ui.label(
                        RichText::new("multi-slice plan active – see Slices below")
                            .small()
                            .color(theme::palette::TEXT_DIM),
                    );
                }
            })
            .response
            .on_hover_text(
                "Play only from/to (In/Out). Click the waveform to set In, Shift-click to set Out.",
            );
            ui.end_row();

            // --- Media-Slices (Segment-Plan mit Play-Counts) ---
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

            // Waveform (nur lokale Dateien, nicht Streams): Hier setzt man In und Out.
            if let Some(state) = waveform_state {
                theme::field_label(ui, "Waveform");
                let marks =
                    (cue.slices.len() == 1).then(|| (cue.slices[0].start, cue.slices[0].end));
                match state {
                    WaveformState::Ready(waveform) => {
                        let total = waveform.duration;
                        if let Some(edit) = draw_waveform(ui, &waveform, marks, 96.0) {
                            let (start, end) = marks.unwrap_or((Duration::ZERO, total));
                            let (start, end) = moved_marks((start, end), edit, total);
                            cue.slices = vec![MediaSlice {
                                start,
                                end,
                                play_count: cue.slices.first().map_or(1, |s| s.play_count),
                            }];
                        }
                    }
                    WaveformState::Pending => {
                        ui.label(RichText::new("reading audio…").weak());
                        ui.ctx().request_repaint_after(Duration::from_millis(100));
                    }
                    WaveformState::Failed => {
                        ui.label(RichText::new("no waveform").weak());
                    }
                }
                ui.end_row();
            }
        }
    });
}

/// Änderung an einer Marke der Waveform.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MarkEdit {
    position: Duration,
    is_out: bool,
}

/// Setzt In oder Out; die jeweils andere Marke bleibt und wird nie überholt.
fn moved_marks(
    (start, end): (Duration, Duration),
    edit: MarkEdit,
    total: Duration,
) -> (Duration, Duration) {
    const MIN_LENGTH: Duration = Duration::from_millis(100);
    let position = edit.position.min(total);
    if edit.is_out {
        let start = start.min(total.saturating_sub(MIN_LENGTH));
        (start, position.max(start + MIN_LENGTH))
    } else {
        let start = position.min(end.saturating_sub(MIN_LENGTH));
        (start, end)
    }
}

/// Waveform mit In- und Out-Marke. Klick setzt In, Shift-Klick Out, Ziehen bewegt
/// die nächstliegende Marke. Rückgabe: die Änderung.
fn draw_waveform(
    ui: &mut egui::Ui,
    waveform: &Waveform,
    marks: Option<(Duration, Duration)>,
    height: f32,
) -> Option<MarkEdit> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click_and_drag(),
    );
    let total = waveform.duration.as_secs_f64().max(0.001);
    let x_of = |d: Duration| rect.left() + (d.as_secs_f64() / total) as f32 * rect.width();
    let position_at = |x: f32| {
        Duration::from_secs_f64(
            f64::from(((x - rect.left()) / rect.width()).clamp(0.0, 1.0)) * total,
        )
    };
    let (in_mark, out_mark) = marks.unwrap_or((Duration::ZERO, waveform.duration));
    let drag_id = response.id.with("mark");
    let mut edit = None;
    if response.drag_started() {
        if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
            let is_out = (pos.x - x_of(out_mark)).abs() < (pos.x - x_of(in_mark)).abs();
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, is_out));
        }
    }
    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let is_out = ui.ctx().data_mut(|d| d.get_temp(drag_id)).unwrap_or(false);
            edit = Some(MarkEdit {
                position: position_at(pos.x),
                is_out,
            });
        }
    }
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            edit = Some(MarkEdit {
                position: position_at(pos.x),
                is_out: ui.input(|i| i.modifiers.shift),
            });
        }
    }
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
        egui::Stroke::new(1.0_f32, theme::palette::STOPPED),
    );
    let n = waveform.bucket_count().max(1);
    let bw = rect.width() / n as f32;
    let stroke = egui::Stroke::new(1.0_f32, theme::palette::PLAYING);
    for (i, &(min, max)) in waveform.buckets.iter().enumerate() {
        let x = rect.left() + (i as f32 + 0.5) * bw;
        let y_top = mid - max.clamp(0.0, 1.0) * half;
        let y_bottom = mid - min.clamp(-1.0, 0.0) * half;
        painter.vline(x, y_top..=y_bottom.max(y_top + 1.0), stroke);
    }
    // Was nicht gespielt wird, ist abgedunkelt; In und Out sind Linien.
    let shade = egui::Color32::from_black_alpha(160);
    let (in_x, out_x) = (x_of(in_mark), x_of(out_mark));
    painter.rect_filled(
        egui::Rect::from_x_y_ranges(rect.left()..=in_x, rect.y_range()),
        0.0,
        shade,
    );
    painter.rect_filled(
        egui::Rect::from_x_y_ranges(out_x..=rect.right(), rect.y_range()),
        0.0,
        shade,
    );
    let mark = egui::Stroke::new(2.0_f32, theme::palette::ACCENT);
    painter.vline(in_x, rect.y_range(), mark);
    painter.vline(out_x, rect.y_range(), mark);
    edit
}

#[cfg(test)]
mod mark_tests {
    use super::*;

    const TOTAL: Duration = Duration::from_secs(100);
    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }
    fn edit(s: u64, is_out: bool) -> MarkEdit {
        MarkEdit {
            position: secs(s),
            is_out,
        }
    }

    #[test]
    fn in_keeps_the_end_of_the_clip() {
        assert_eq!(
            moved_marks((secs(0), TOTAL), edit(30, false), TOTAL),
            (secs(30), TOTAL)
        );
    }

    #[test]
    fn out_keeps_the_start() {
        assert_eq!(
            moved_marks((secs(10), TOTAL), edit(40, true), TOTAL),
            (secs(10), secs(40))
        );
    }

    #[test]
    fn marks_never_cross() {
        let (start, end) = moved_marks((secs(10), secs(20)), edit(50, false), TOTAL);
        assert_eq!(end, secs(20));
        assert!(start < end);
        let (start, end) = moved_marks((secs(10), secs(20)), edit(5, true), TOTAL);
        assert_eq!(start, secs(10));
        assert!(end > start);
    }
}
