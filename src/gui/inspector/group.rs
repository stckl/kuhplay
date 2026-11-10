//! Tab „Group" (Group-Cues): Modus, Einstellungen je Modus und bei
//! [`GroupMode::Timeline`] ein Timeline-Editor (Start = Pre-Wait des Kindes,
//! Länge = Duration, beides per Ziehen).

use std::time::Duration;

use egui::{pos2, vec2, Color32, CursorIcon, RichText, Sense, Stroke};

use crate::core::{format_timecode, CueStatus, CueType, GroupMode, LoopMode};

use crate::gui::app::ShowApp;
use crate::gui::icons;
use crate::gui::theme;

/// Rasterweite beim Ziehen in der Timeline.
const SNAP: Duration = Duration::from_millis(100);
/// Länge für Kinder ohne bekannte Dauer (Platzhalter, nur Anzeige).
const UNKNOWN_LENGTH: Duration = Duration::from_secs(5);

/// Länge eines Kindes in der Timeline und woher sie kommt.
#[derive(Clone, Copy)]
enum Length {
    /// Fest eingestellt (Duration > 0).
    Set(Duration),
    /// Automatisch aus der Mediendauer.
    Media(Duration),
    /// Unbekannt (Medium noch nicht geprüft) – Platzhalter.
    Unknown,
    /// Sofort-Cue ohne Laufzeit (Control, Netzwerk, …).
    Instant,
}

impl Length {
    fn shown(self) -> Duration {
        match self {
            Length::Set(d) | Length::Media(d) => d,
            Length::Unknown => UNKNOWN_LENGTH,
            Length::Instant => Duration::ZERO,
        }
    }
}

/// Tab „Group".
pub fn draw_group(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Längen vorab (braucht &app), danach den Cue mutabel.
    let lengths: Vec<Length> = match app.carts.selected() {
        Some(group) => group
            .children
            .iter()
            .map(|child| child_length(app, child))
            .collect(),
        None => Vec::new(),
    };
    // Gesamtlaufzeit (None = unbekannt bzw. endlos) und Restzeit im Lauf.
    let (runtime, remaining) = match app.carts.selected() {
        Some(group) if group.cue_type == CueType::Group => (
            app.group_runtime(group),
            group
                .status
                .is_active()
                .then(|| app.remaining_of(group))
                .flatten(),
        ),
        _ => (None, None),
    };
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Group {
        ui.label(RichText::new("Only for group cues").weak());
        return;
    }

    theme::form_grid("inspector_group_grid").show(ui, |ui| {
        theme::field_label(ui, "Mode");
        let mut mode = cue.group_mode;
        egui::ComboBox::from_id_salt("group_tab_mode_combo")
            .selected_text(mode.label())
            .show_ui(ui, |ui| {
                for m in GroupMode::ALL {
                    ui.selectable_value(&mut mode, m, m.label());
                }
            });
        if mode != cue.group_mode {
            cue.group_mode = mode;
        }
        ui.end_row();

        theme::field_label(ui, "");
        ui.label(RichText::new(mode_help(cue.group_mode)).color(theme::palette::TEXT_DIM));
        ui.end_row();

        theme::field_label(ui, "Total");
        let endless = matches!(cue.group_mode, GroupMode::Sequential | GroupMode::Shuffle)
            && cue.loop_mode == LoopMode::Infinite;
        let total = match runtime {
            Some(total) => format_timecode(total),
            None if endless => "∞ (endless)".to_string(),
            None => "unknown – a child has no known length".to_string(),
        };
        let total = match remaining {
            Some(left) => format!("{total}   (−{} left)", format_timecode(left)),
            None => total,
        };
        ui.label(RichText::new(total).monospace());
        ui.end_row();

        theme::field_label(ui, "Fade");
        ui.horizontal(|ui| {
            ui.label(RichText::new("In").color(theme::palette::TEXT_DIM));
            super::time_field(ui, &mut cue.group_fade_in)
                .on_hover_text("Fade sound and picture in when the group starts (0 = off)");
            ui.add_space(theme::size::SPACE_MEDIUM);
            ui.label(RichText::new("Out").color(theme::palette::TEXT_DIM));
            super::time_field(ui, &mut cue.group_fade_out).on_hover_text(
                "Fade sound and picture out before the group ends and when it is stopped (0 = off)",
            );
        });
        ui.end_row();

        let sequential = matches!(cue.group_mode, GroupMode::Sequential | GroupMode::Shuffle);
        if sequential {
            theme::field_label(ui, "Playback");
            ui.vertical(|ui| {
                ui.radio_value(
                    &mut cue.group_go_per_child,
                    false,
                    "play through (one GO plays all children in turn)",
                );
                ui.radio_value(
                    &mut cue.group_go_per_child,
                    true,
                    "one GO per child (the group's GO starts the first)",
                );
            });
            ui.end_row();
        }

        if sequential {
            theme::field_label(ui, "Repeat");
            ui.horizontal(|ui| {
                let mut infinite = cue.loop_mode == LoopMode::Infinite;
                let mut count = match cue.loop_mode {
                    LoopMode::Count(n) => n.max(1),
                    LoopMode::Once => 1,
                    LoopMode::Infinite => 2,
                };
                let before = (infinite, count);
                ui.radio_value(&mut infinite, false, "");
                ui.add_enabled(
                    !infinite,
                    egui::DragValue::new(&mut count)
                        .range(1..=9999)
                        .suffix(" ×"),
                );
                ui.radio_value(&mut infinite, true, "until Devamp / Stop");
                if (infinite, count) != before {
                    cue.loop_mode = if infinite {
                        LoopMode::Infinite
                    } else if count == 1 {
                        LoopMode::Once
                    } else {
                        LoopMode::Count(count)
                    };
                }
            });
            ui.end_row();
            if cue.status == CueStatus::Playing {
                theme::field_label(ui, "Pass");
                let total = match cue.loop_mode {
                    LoopMode::Infinite => "∞".to_string(),
                    other => other.total_iterations().to_string(),
                };
                ui.label(format!("{} of {total}", cue.group_iterations_done + 1));
                ui.end_row();
            }
        }

        if cue.group_mode == GroupMode::Shuffle {
            theme::field_label(ui, "Order");
            ui.vertical(|ui| {
                ui.radio_value(
                    &mut cue.group_shuffle_each_run,
                    false,
                    "same shuffled order every show",
                );
                ui.radio_value(&mut cue.group_shuffle_each_run, true, "new order each run");
            });
            ui.end_row();

            if !cue.group_shuffle_each_run {
                theme::field_label(ui, "Shuffle ID");
                ui.horizontal(|ui| {
                    ui.monospace(format!("{:08X}", cue.shuffle_seed() >> 32));
                    if ui
                        .button("New Shuffle")
                        .on_hover_text("Roll a new fixed order for this group")
                        .clicked()
                    {
                        cue.new_shuffle();
                    }
                });
                ui.end_row();

                if !cue.children.is_empty() {
                    theme::field_label(ui, "Shuffled");
                    let order: Vec<&str> = cue
                        .shuffled_children()
                        .iter()
                        .map(|c| c.number.as_str())
                        .collect();
                    ui.label(RichText::new(order.join(" → ")).color(theme::palette::TEXT_DIM));
                    ui.end_row();
                }
            }
        }
    });

    ui.separator();
    if cue.children.is_empty() {
        ui.label(RichText::new("No children – drag cues onto the group in the cue list").weak());
        return;
    }
    if cue.group_mode == GroupMode::Timeline {
        timeline_editor(ui, cue, &lengths);
    } else {
        for child in &cue.children {
            ui.horizontal(|ui| {
                icons::draw(
                    ui,
                    icons::Icon::for_cue_type(child.cue_type),
                    14.0,
                    theme::palette::TEXT,
                );
                ui.label(format!("{}  {}", child.number, child.name));
                if !child.is_armed {
                    ui.label(RichText::new("disarmed").color(theme::palette::TEXT_DIM));
                }
            });
        }
    }
}

/// Kurzbeschreibung je Modus.
fn mode_help(mode: GroupMode) -> &'static str {
    match mode {
        GroupMode::Timeline => {
            "All children start with one GO, each at its own time on the timeline below."
        }
        GroupMode::Sequential => "Children play one after another in list order.",
        GroupMode::Shuffle => {
            "Children play one after another in shuffled order (fixed by the Shuffle ID)."
        }
    }
}

/// Länge eines Kindes für die Timeline.
fn child_length(app: &ShowApp, child: &crate::core::Cue) -> Length {
    if !child.duration.is_zero() {
        return Length::Set(child.duration);
    }
    match child.cue_type {
        CueType::Audio | CueType::Video => match app.media_duration_of(child) {
            Some(d) => Length::Media(d),
            None => Length::Unknown,
        },
        CueType::Html
        | CueType::SlideShow
        | CueType::Text
        | CueType::Fade
        | CueType::Wait
        | CueType::Group => Length::Unknown,
        _ => Length::Instant,
    }
}

/// Auf das Raster runden.
fn snap(d: Duration) -> Duration {
    let step = SNAP.as_millis() as u64;
    Duration::from_millis((d.as_millis() as u64 + step / 2) / step * step)
}

/// Timeline-Editor: eine Zeile je Kind, Balken = [Pre-Wait, Pre-Wait + Länge].
/// Balken ziehen verschiebt den Start, rechte Kante ziehen ändert die Länge
/// (Doppelklick auf die Kante: wieder automatisch = Mediendauer).
fn timeline_editor(ui: &mut egui::Ui, group: &mut crate::core::Cue, lengths: &[Length]) {
    const LABEL_W: f32 = 150.0;
    const ROW_H: f32 = 24.0;
    const RULER_H: f32 = 18.0;
    const EDGE: f32 = 6.0;

    ui.label(
        RichText::new(
            "Drag a bar to move its start, drag its right edge to change the length \
             (double-click the edge: automatic length).",
        )
        .small()
        .color(theme::palette::TEXT_DIM),
    );

    // Sichtbarer Bereich: bis zum Ende des letzten Kindes plus Luft. Während
    // des Ziehens fest, sonst „läuft" die Skala unter der Maus weg.
    let span_id = ui.id().with(("timeline_span", group.id));
    let end = group
        .children
        .iter()
        .zip(lengths)
        .map(|(c, l)| c.pre_wait + l.shown())
        .max()
        .unwrap_or_default();
    let wanted = (end.as_secs_f32() * 1.25).max(10.0);
    let dragging = ui.ctx().dragged_id().is_some();
    let span = match ui.data(|d| d.get_temp::<f32>(span_id)) {
        Some(s) if dragging => s,
        _ => wanted,
    };
    ui.data_mut(|d| d.insert_temp(span_id, span));

    let rows = group.children.len();
    let width = ui.available_width().max(LABEL_W + 200.0);
    let (rect, _) = ui.allocate_exact_size(
        vec2(width, RULER_H + rows as f32 * ROW_H + 4.0),
        Sense::hover(),
    );
    let track = egui::Rect::from_min_max(pos2(rect.left() + LABEL_W, rect.top()), rect.max);
    let px_per_s = track.width() / span;
    let x_of = |d: Duration| track.left() + d.as_secs_f32() * px_per_s;
    let painter = ui.painter_at(rect);

    // Skala: Striche im passenden Abstand mit Zeit.
    let step = [1.0f32, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
        .into_iter()
        .find(|s| s * px_per_s >= 60.0)
        .unwrap_or(600.0);
    let mut t = 0.0f32;
    while t <= span {
        let x = track.left() + t * px_per_s;
        painter.line_segment(
            [pos2(x, track.top() + RULER_H - 5.0), pos2(x, rect.bottom())],
            Stroke::new(1.0, theme::palette::TEXT_FAINT.gamma_multiply(0.35)),
        );
        painter.text(
            pos2(x + 2.0, track.top()),
            egui::Align2::LEFT_TOP,
            format_timecode(Duration::from_secs_f32(t)),
            egui::FontId::proportional(10.0),
            theme::palette::TEXT_DIM,
        );
        t += step;
    }

    for (i, (child, length)) in group.children.iter_mut().zip(lengths).enumerate() {
        let top = rect.top() + RULER_H + i as f32 * ROW_H;
        let row = egui::Rect::from_min_size(pos2(rect.left(), top), vec2(width, ROW_H));
        if i % 2 == 1 {
            painter.rect_filled(row, 0.0, theme::palette::TEXT_FAINT.gamma_multiply(0.06));
        }
        painter.text(
            pos2(rect.left() + 4.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{}  {}", child.number, child.name),
            egui::FontId::proportional(12.0),
            if child.is_armed {
                theme::palette::TEXT
            } else {
                theme::palette::TEXT_DIM
            },
        );

        let color = child
            .color
            .map(|[r, g, b]| Color32::from_rgb(r, g, b))
            .unwrap_or(theme::palette::ACCENT);
        let start_x = x_of(child.pre_wait);
        let bar_y = row.shrink2(vec2(0.0, 4.0));
        let id = ui.id().with(("timeline_child", child.id));

        if matches!(length, Length::Instant) {
            // Sofort-Cue: Raute, nur der Start ist verschiebbar.
            let hit = egui::Rect::from_center_size(pos2(start_x, row.center().y), vec2(14.0, 14.0));
            let response = ui
                .interact(hit, id.with("start"), Sense::drag())
                .on_hover_cursor(CursorIcon::Grab);
            drag_start(ui, &response, child, px_per_s, id);
            let c = pos2(start_x, row.center().y);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + vec2(0.0, -7.0),
                    c + vec2(7.0, 0.0),
                    c + vec2(0.0, 7.0),
                    c + vec2(-7.0, 0.0),
                ],
                color,
                Stroke::NONE,
            ));
            response.on_hover_text(format!("starts at {}", format_timecode(child.pre_wait)));
            continue;
        }

        let end_x = x_of(child.pre_wait + length.shown()).max(start_x + EDGE * 2.0);
        let bar = egui::Rect::from_min_max(pos2(start_x, bar_y.top()), pos2(end_x, bar_y.bottom()));
        let body = ui
            .interact(bar.with_max_x(end_x - EDGE), id.with("body"), Sense::drag())
            .on_hover_cursor(CursorIcon::Grab);
        drag_start(ui, &body, child, px_per_s, id);
        let edge = ui
            .interact(
                egui::Rect::from_min_max(
                    pos2(end_x - EDGE, bar.top()),
                    pos2(end_x + 2.0, bar.bottom()),
                ),
                id.with("edge"),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(CursorIcon::ResizeHorizontal);
        let length_key = id.with("drag_length");
        if edge.drag_started() {
            ui.data_mut(|d| d.insert_temp(length_key, length.shown()));
        }
        if edge.dragged() {
            let from: Duration = ui
                .data(|d| d.get_temp(length_key))
                .unwrap_or(length.shown());
            let total = drag_dx(ui, &edge) / px_per_s;
            let new = (from.as_secs_f32() + total).max(SNAP.as_secs_f32());
            child.duration = snap(Duration::from_secs_f32(new)).max(SNAP);
        }
        if edge.double_clicked() {
            child.duration = Duration::ZERO;
        }

        let fill = if child.is_armed {
            color.gamma_multiply(0.75)
        } else {
            color.gamma_multiply(0.3)
        };
        painter.rect_filled(bar, 3.0, fill);
        if matches!(length, Length::Unknown) {
            // Länge unbekannt: gestrichelter Rand statt fester Kante.
            painter.rect_stroke(bar, 3.0, Stroke::new(1.0, theme::palette::TEXT_DIM));
        }
        painter.rect_filled(
            egui::Rect::from_min_max(
                pos2(end_x - 3.0, bar.top() + 3.0),
                pos2(end_x - 1.0, bar.bottom() - 3.0),
            ),
            1.0,
            Color32::WHITE.gamma_multiply(0.6),
        );
        painter.text(
            pos2(start_x + 4.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            format_timecode(child.pre_wait),
            egui::FontId::monospace(10.0),
            Color32::WHITE,
        );
        let length_text = match length {
            Length::Set(d) => format!("length {} (set)", format_timecode(*d)),
            Length::Media(d) => format!("length {} (media)", format_timecode(*d)),
            Length::Unknown => "length automatic (unknown yet)".to_string(),
            Length::Instant => String::new(),
        };
        body.on_hover_text(format!(
            "starts at {}\n{length_text}",
            format_timecode(child.pre_wait)
        ));
        edge.on_hover_text(length_text);
    }
}

/// Start (Pre-Wait) per Ziehen verschieben, bezogen auf den Wert beim
/// Drag-Beginn (kein Aufsummieren von Rundungsfehlern).
fn drag_start(
    ui: &egui::Ui,
    response: &egui::Response,
    child: &mut crate::core::Cue,
    px_per_s: f32,
    id: egui::Id,
) {
    let key = id.with("drag_start");
    if response.drag_started() {
        ui.data_mut(|d| d.insert_temp(key, child.pre_wait));
    }
    if response.dragged() {
        let from: Duration = ui.data(|d| d.get_temp(key)).unwrap_or(child.pre_wait);
        let total = drag_dx(ui, response) / px_per_s;
        let new = (from.as_secs_f32() + total).max(0.0);
        child.pre_wait = snap(Duration::from_secs_f32(new));
    }
}

/// Horizontaler Weg seit Drag-Beginn (Pixel).
fn drag_dx(ui: &egui::Ui, response: &egui::Response) -> f32 {
    let origin = ui.input(|i| i.pointer.press_origin());
    match (origin, response.interact_pointer_pos()) {
        (Some(origin), Some(now)) => now.x - origin.x,
        _ => 0.0,
    }
}
