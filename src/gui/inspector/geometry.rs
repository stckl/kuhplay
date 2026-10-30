//! Tab „Geometry" (QLab 5, Video/HTML5/Diashow): Stage, Layer, Surface
//! (Position/Größe, Crop, Hard Edge, Deckkraft) und
//! Stage-Einstellungen (Canvas/Screen/Visible).

use egui::RichText;

use super::push_stage;
use crate::core::{CueType, SurfaceGeometry};
use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Geometry".
pub fn draw_geometry(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Engine-Referenz VOR dem Cue-Borrow extrahieren (disjunkte Felder).
    let engine = &app.engine;
    // Content-Groesse des selektierten Cues (dekodiertes Frame, sonst 1920×1080)
    // fuer die Geometrie-Vorschau – vor den Borrows ermitteln.
    let content_size: Option<(u32, u32)> = app
        .carts
        .selected()
        .map(|cue| cue.id)
        .and_then(|id| engine.latest_frame(id))
        .map(|frame| (frame.width, frame.height));
    // Nach dem Zeichnen geöffneter Stage-Editor (borrow-frei setzen).
    let mut edit_stage: Option<String> = None;
    // Stage-/Display-/Fenster-Felder ebenfalls disjunkt borgen.
    let stages = &mut app.stages;
    let stage_windows = &mut app.stage_windows;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if !matches!(
        cue.cue_type,
        CueType::Video | CueType::Html | CueType::SlideShow
    ) {
        ui.label(RichText::new("Geometry only for video, HTML5 and slideshow cues").weak());
        return;
    }
    ui.columns(2, |uis| {
        let [left, right] = uis else {
            return;
        };
        theme::form_grid("inspector_geometry_grid").show(left, |ui| {
            // --- Stage (QLab: Dropdown vorhandener Stages) ---
            theme::field_label(ui, "Stage");
            let stage_names = stages.names();
            let mut stage_name = cue.stage.clone().unwrap_or_default();
            let mut stage_picked: Option<String> = None;
            let text_resp = ui
                .horizontal(|ui| {
                    egui::ComboBox::from_id_salt("cue_stage_combo")
                        .selected_text(if stage_name.is_empty() {
                            "— none —".to_string()
                        } else {
                            stage_name.clone()
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(stage_name.is_empty(), "— none —")
                                .clicked()
                            {
                                stage_picked = Some(String::new());
                            }
                            for name in &stage_names {
                                if ui.selectable_label(stage_name == *name, name).clicked() {
                                    stage_picked = Some(name.clone());
                                }
                            }
                        });
                    if let Some(picked) = &stage_picked {
                        stage_name = picked.clone();
                    }
                    // Freitext für neue Stage-Namen (QLab legt Stages
                    // on-the-fly an) + „+"-Button für „Stage N".
                    ui.add(
                        egui::TextEdit::singleline(&mut stage_name)
                            .hint_text("New stage…")
                            .desired_width(160.0),
                    )
                })
                .inner;
            app.text_field_ids.insert(text_resp.id);
            ui.end_row();
            if text_resp.changed() || stage_picked.is_some() {
                let trimmed = stage_name.trim().to_string();
                if trimmed.is_empty() {
                    cue.stage = None;
                } else {
                    // Unbekannte Stage anlegen + an Engine melden.
                    if stages.ensure(&trimmed) {
                        push_stage(engine, stages, &trimmed);
                    }
                    cue.stage = Some(trimmed);
                }
            }
            theme::field_label(ui, "");
            ui.horizontal(|ui| {
                let next = format!("Stage {}", stages.len() + 1);
                if ui.small_button(format!("+ {next}")).clicked() {
                    stages.ensure(&next);
                    push_stage(engine, stages, &next);
                    cue.stage = Some(next);
                }
                if let Some(name) = cue.stage.clone() {
                    if !stage_windows.contains(&name)
                        && ui
                            .small_button("Open Stage Window")
                            .on_hover_text("Open preview window for this stage")
                            .clicked()
                    {
                        stage_windows.insert(name.clone());
                    }
                }
            });
            ui.end_row();

            theme::field_label(ui, "Layer");
            ui.add(egui::DragValue::new(&mut cue.layer).range(0..=99));
            ui.end_row();

            // --- Stage-Editor (QLab: "Edit Stage…") ---
            if let Some(stage_name) = cue.stage.clone() {
                theme::field_label(ui, "Stage");
                let known = stages.find(&stage_name).is_some();
                ui.horizontal(|ui| {
                    ui.label(if known {
                        stage_name.clone()
                    } else {
                        format!("{stage_name} (missing)")
                    });
                    if ui
                        .small_button("Edit Stage…")
                        .on_hover_text("Open the stage editor (regions, layers, edge blends)")
                        .clicked()
                    {
                        edit_stage = Some(stage_name.clone());
                    }
                });
                ui.end_row();
            }
        });

        // --- Surface (QLab: Surface auf der Stage) – nur Video/Diashow ---
        if matches!(cue.cue_type, CueType::Video | CueType::SlideShow) {
            theme::section_label(right, "Geometry");
            // Vorschau: abstrahiert, wie der Inhalt auf der Stage liegt
            // (Fit/Fill/Stretch/Custom sichtbar, inkl. Verzerrung).
            let canvas = cue
                .stage
                .as_deref()
                .and_then(|name| stages.find(name))
                .map(|s| {
                    (
                        s.geometry.width.max(1.0) as u32,
                        s.geometry.height.max(1.0) as u32,
                    )
                })
                .unwrap_or((1920, 1080));
            if let Some(surface) = cue.surface.as_ref() {
                placement_preview(right, surface, canvas, content_size);
            }
            let surface = cue.surface.get_or_insert_with(SurfaceGeometry::fit);
            surface_editor(right, surface, content_size);
        }
    });
    if let Some(name) = edit_stage {
        if let Some(stage) = app.stages.find(&name) {
            app.stage_editor_open = Some(stage.id);
        }
    }
}

/// Editor für eine Cue-Surface (QLab: Surface-Geometrie auf der Stage):
/// Position/Größe, Crop (Quellen-Zuschnitt), Hard-Edge-Blending,
/// Deckkraft und Sichtbarkeit.
fn surface_editor(ui: &mut egui::Ui, surface: &mut SurfaceGeometry, content: Option<(u32, u32)>) {
    use crate::core::{BlendCurve, Crop, EdgeSide, HardEdge};
    theme::form_grid("surface_grid").show(ui, |ui| {
        theme::field_label(ui, "Mode");
        egui::ComboBox::from_id_salt("surface_mode")
            .selected_text(surface.mode.label())
            .show_ui(ui, |ui| {
                for mode in crate::core::GeometryMode::ALL {
                    ui.selectable_value(&mut surface.mode, mode, mode.label());
                }
            })
            .response
            .on_hover_text(
                "Fit: whole image, not distorted. Fill: fills the stage, edges are cut off. \
                     Stretch: fills the stage, distorted. Custom: your own position and size.",
            );
        ui.end_row();
        if surface.mode == crate::core::GeometryMode::Custom {
            theme::field_label(ui, "Position");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut surface.x).speed(1.0).prefix("X "));
                ui.add(egui::DragValue::new(&mut surface.y).speed(1.0).prefix("Y "));
            });
            ui.end_row();
            theme::field_label(ui, "Size");
            // Seitenverhältnis-Sperre: Basis ist das Medienformat (falls das
            // erste Frame schon da ist), sonst das aktuelle Rechteck.
            let ratio = content
                .filter(|(w, h)| *w > 0 && *h > 0)
                .map(|(w, h)| h as f32 / w as f32)
                .or_else(|| (surface.width > 0.0).then_some(surface.height / surface.width));
            let old_w = surface.width;
            let old_h = surface.height;
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut surface.width)
                        .speed(1.0)
                        .range(1.0..=16384.0)
                        .prefix("W "),
                );
                ui.add(
                    egui::DragValue::new(&mut surface.height)
                        .speed(1.0)
                        .range(1.0..=16384.0)
                        .prefix("H "),
                );
                if ui
                    .checkbox(&mut surface.lock_aspect, "Lock")
                    .on_hover_text("Lock aspect ratio (media format)")
                    .changed()
                    && surface.lock_aspect
                {
                    // Beim Einschalten: Verhältnis auf das aktuelle Rechteck festziehen.
                    if surface.width > 0.0 {
                        surface.height = surface.width
                            * content
                                .filter(|(w, h)| *w > 0 && *h > 0)
                                .map(|(w, h)| h as f32 / w as f32)
                                .unwrap_or(surface.height / surface.width);
                    }
                }
            });
            // Lock aktiv: geänderte Achse zieht die andere proportional mit.
            if surface.lock_aspect {
                if let Some(r) = ratio {
                    if surface.width != old_w && surface.height == old_h {
                        surface.height = (surface.width * r).clamp(1.0, 16384.0);
                    } else if surface.height != old_h && surface.width == old_w {
                        surface.width = (surface.height / r).clamp(1.0, 16384.0);
                    }
                }
            }
            ui.end_row();
        }
        theme::field_label(ui, "Opacity");
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut surface.opacity, 0.0..=1.0));
            ui.checkbox(&mut surface.visible, "Visible");
        });
        ui.end_row();

        // --- Crop (Quellen-Zuschnitt, normalisiert 0..1) ---
        theme::field_label(ui, "Crop");
        let mut aoi = surface.area_of_interest.is_some();
        if ui.checkbox(&mut aoi, "Crop").changed() {
            surface.area_of_interest = aoi.then_some(Crop {
                left: 0.0,
                top: 0.0,
                right: 1.0,
                bottom: 1.0,
            });
        }
        ui.end_row();
        if let Some(crop) = &mut surface.area_of_interest {
            theme::field_label(ui, "Left");
            ui.add(
                egui::DragValue::new(&mut crop.left)
                    .speed(0.01)
                    .range(0.0..=1.0),
            );
            ui.end_row();
            theme::field_label(ui, "Top");
            ui.add(
                egui::DragValue::new(&mut crop.top)
                    .speed(0.01)
                    .range(0.0..=1.0),
            );
            ui.end_row();
            theme::field_label(ui, "Right");
            ui.add(
                egui::DragValue::new(&mut crop.right)
                    .speed(0.01)
                    .range(0.0..=1.0),
            );
            ui.end_row();
            theme::field_label(ui, "Bottom");
            ui.add(
                egui::DragValue::new(&mut crop.bottom)
                    .speed(0.01)
                    .range(0.0..=1.0),
            );
            ui.end_row();
        }

        // --- Hard Edge (Kanten-Blending für Projektor-Overlaps) ---
        theme::field_label(ui, "Hard Edge");
        let mut edge = surface.hard_edge.is_some();
        if ui.checkbox(&mut edge, "Blend edge").changed() {
            surface.hard_edge =
                edge.then_some(HardEdge::new(EdgeSide::Right, 100.0, BlendCurve::Cosine));
        }
        ui.end_row();
        if let Some(hard_edge) = &mut surface.hard_edge {
            theme::field_label(ui, "Side");
            let mut side = hard_edge.side;
            egui::ComboBox::from_id_salt("hard_edge_side")
                .selected_text(side.label())
                .show_ui(ui, |ui| {
                    for s in EdgeSide::ALL {
                        ui.selectable_value(&mut side, s, s.label());
                    }
                });
            hard_edge.side = side;
            ui.end_row();
            theme::field_label(ui, "Width");
            ui.add(
                egui::DragValue::new(&mut hard_edge.width)
                    .speed(1.0)
                    .range(1.0..=4096.0),
            );
            ui.end_row();
            theme::field_label(ui, "Curve");
            let mut curve = hard_edge.curve;
            egui::ComboBox::from_id_salt("hard_edge_curve")
                .selected_text(curve.label())
                .show_ui(ui, |ui| {
                    for c in BlendCurve::ALL {
                        ui.selectable_value(&mut curve, c, c.label());
                    }
                });
            hard_edge.curve = curve;
            ui.end_row();
        }
    });
}

/// Abstrahierte Geometrie-Vorschau: das Stage-Rechteck im echten
/// Seitenverhältnis, darin das Ziel-Rechteck des Inhalts aus
/// [`SurfaceGeometry::placement`] – so sieht man, was die Einstellungen
/// tun (Fit = Balken, Fill = beschnitten, Stretch = verzerrt, Custom =
/// eigene Lage). Die Content-Größe kommt aus dem dekodierten Frame,
/// solange unbekannt wird 1920×1080 angenommen (beschriftet). Der
/// Hard-Edge-Bereich erscheint als abgestufte Fläche an der Kante.
fn placement_preview(
    ui: &mut egui::Ui,
    surface: &SurfaceGeometry,
    canvas: (u32, u32),
    content: Option<(u32, u32)>,
) {
    let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
    let (sw, sh) = content.unwrap_or((1920, 1080));
    let assumed = content.is_none();
    let avail_width = ui.available_width().min(360.0);
    let height = (avail_width * ch / cw).clamp(80.0, 200.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let scale = (rect.width() / cw).min(rect.height() / ch);
    let stage = egui::Rect::from_center_size(rect.center(), egui::vec2(cw * scale, ch * scale));
    let stage_color = egui::Color32::from_rgb(40, 44, 52);
    ui.painter().rect_filled(stage, 2.0, stage_color);
    ui.painter().rect_stroke(
        stage,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(150, 150, 160)),
    );
    // Ziel-Rechteck des Inhalts auf der Stage.
    let ((x, y, w, h), _) = surface.placement((sw, sh), (cw as u32, ch as u32));
    let content_rect = egui::Rect::from_min_size(
        stage.min + egui::vec2(x * scale, y * scale),
        egui::vec2(w * scale, h * scale),
    );
    let accent = egui::Color32::from_rgb(255, 176, 0);
    ui.painter()
        .rect_filled(content_rect, 1.0, accent.gamma_multiply(0.18));
    ui.painter()
        .rect_stroke(content_rect, 1.0, egui::Stroke::new(1.5, accent));
    // Hard-Edge-Bereich: abgestufte Flaeche an der blendenden Kante.
    if let Some(edge) = &surface.hard_edge {
        let band = (edge.width * scale).min(content_rect.width() / 2.0);
        let edge_rect = match edge.side {
            crate::core::EdgeSide::Left => {
                egui::Rect::from_min_size(content_rect.min, egui::vec2(band, content_rect.height()))
            }
            crate::core::EdgeSide::Right => egui::Rect::from_min_size(
                egui::pos2(content_rect.right() - band, content_rect.top()),
                egui::vec2(band, content_rect.height()),
            ),
            crate::core::EdgeSide::Top => {
                egui::Rect::from_min_size(content_rect.min, egui::vec2(content_rect.width(), band))
            }
            crate::core::EdgeSide::Bottom => egui::Rect::from_min_size(
                egui::pos2(content_rect.left(), content_rect.bottom() - band),
                egui::vec2(content_rect.width(), band),
            ),
        };
        for (i, rect) in split_band(edge_rect, 6).into_iter().enumerate() {
            let alpha = 0.30 * (1.0 - i as f32 / 6.0);
            ui.painter()
                .rect_filled(rect, 0.0, accent.gamma_multiply(alpha));
        }
    }
    // Beschriftung: Stage unten links, Content in/unter dem Rechteck.
    ui.painter().text(
        stage.left_bottom() + egui::vec2(3.0, -3.0),
        egui::Align2::LEFT_BOTTOM,
        format!("Stage {}x{}", cw as u32, ch as u32),
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(150, 150, 160),
    );
    let label = if assumed {
        "Content 1920x1080 (assumed)".to_string()
    } else {
        format!("Content {sw}x{sh}")
    };
    ui.painter().text(
        content_rect.left_top() + egui::vec2(3.0, 3.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(10.0),
        accent,
    );
    ui.add_space(theme::size::SPACE_SMALL);
}

/// Teilt ein Rechteck in `n` Streifen entlang der laengeren Seite.
fn split_band(rect: egui::Rect, n: usize) -> Vec<egui::Rect> {
    let n = n.max(1) as f32;
    if rect.width() >= rect.height() {
        let step = rect.width() / n;
        (0..n as usize)
            .map(|i| {
                let x0 = rect.left() + step * i as f32;
                let x1 = if i as f32 == n - 1.0 {
                    rect.right()
                } else {
                    x0 + step
                };
                egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom()))
            })
            .collect()
    } else {
        let step = rect.height() / n;
        (0..n as usize)
            .map(|i| {
                let y0 = rect.top() + step * i as f32;
                let y1 = if i as f32 == n - 1.0 {
                    rect.bottom()
                } else {
                    y0 + step
                };
                egui::Rect::from_min_max(egui::pos2(rect.left(), y0), egui::pos2(rect.right(), y1))
            })
            .collect()
    }
}
