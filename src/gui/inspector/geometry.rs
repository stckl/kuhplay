//! Tab „Geometry" (QLab 5, Video/HTML5/Diashow): Stage, Layer, Surface
//! (Position/Größe, Area of Interest, Hard Edge, Deckkraft) und
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
    // Stage-/Display-/Fenster-Felder ebenfalls disjunkt borgen.
    let stages = &mut app.stages;
    let screens = &mut app.screens;
    let stage_windows = &mut app.stage_windows;
    let stage_screen_open = app.stage_screen_open.clone();
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
                    if ui
                        .small_button("Open on Screen")
                        .on_hover_text("Fullscreen output on the assigned display")
                        .clicked()
                    {
                        if let Ok(mut open) = stage_screen_open.lock() {
                            open.insert(name, true);
                        }
                    }
                }
            });
            ui.end_row();

            theme::field_label(ui, "Layer");
            ui.add(egui::DragValue::new(&mut cue.layer).range(0..=99));
            ui.end_row();

            // --- Stage-Einstellungen (QLab: Stage → Screen) ---
            if let Some(stage_name) = cue.stage.clone() {
                theme::field_label(ui, "Stage Settings");
                let known = stages.find(&stage_name).is_some();
                egui::CollapsingHeader::new(if known {
                    format!("Canvas / Screen: {stage_name}")
                } else {
                    format!("Canvas / Screen: {stage_name} (missing)")
                })
                .id_salt("stage_settings")
                .default_open(false)
                .show(ui, |ui| {
                    if let Some(stage) = stages.find_mut(&stage_name) {
                        let mut changed = false;
                        theme::form_grid("stage_settings_grid").show(ui, |ui| {
                            theme::field_label(ui, "Canvas W");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut stage.geometry.width)
                                        .speed(1.0)
                                        .range(16.0..=16384.0),
                                )
                                .changed();
                            ui.end_row();
                            theme::field_label(ui, "Canvas H");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut stage.geometry.height)
                                        .speed(1.0)
                                        .range(16.0..=16384.0),
                                )
                                .changed();
                            ui.end_row();
                            theme::field_label(ui, "Screen");
                            let mut screen = stage.screen;
                            let selected = screen
                                .and_then(|id| screens.find(id))
                                .map(|s| s.label())
                                .unwrap_or_else(|| "— none —".to_string());
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("stage_screen_combo")
                                    .selected_text(selected)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(screen.is_none(), "— none —")
                                            .clicked()
                                        {
                                            screen = None;
                                        }
                                        for s in screens.screens() {
                                            if ui
                                                .selectable_label(screen == Some(s.id), s.label())
                                                .clicked()
                                            {
                                                screen = Some(s.id);
                                            }
                                        }
                                    });
                                if ui
                                    .small_button("…")
                                    .on_hover_text("Edit screens (Workspace Settings, Video)")
                                    .clicked()
                                {
                                    app.ui.workspace_settings_page =
                                        crate::gui::workspace_settings::SettingsPage::Video;
                                    app.ui.workspace_settings_open = true;
                                }
                            });
                            if screen != stage.screen {
                                stage.screen = screen;
                                changed = true;
                            }
                            ui.end_row();
                            theme::field_label(ui, "Visible");
                            changed |= ui.checkbox(&mut stage.visible, "").changed();
                            ui.end_row();
                        });
                        if changed {
                            push_stage(engine, stages, &stage_name);
                        }
                    } else {
                        ui.label(RichText::new("Stage does not exist (not created yet).").weak());
                    }
                });
                ui.end_row();
            }
        });

        // --- Surface (QLab: Surface auf der Stage) – nur Video/Diashow ---
        if matches!(cue.cue_type, CueType::Video | CueType::SlideShow) {
            theme::section_label(right, "Geometry");
            let surface = cue.surface.get_or_insert_with(SurfaceGeometry::fit);
            surface_editor(right, surface);
        }
    });
}

/// Editor für eine Cue-Surface (QLab: Surface-Geometrie auf der Stage):
/// Position/Größe, Area of Interest (Quellen-Crop), Hard-Edge-Blending,
/// Deckkraft und Sichtbarkeit.
fn surface_editor(ui: &mut egui::Ui, surface: &mut SurfaceGeometry) {
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
            });
            ui.end_row();
        }
        theme::field_label(ui, "Opacity");
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut surface.opacity, 0.0..=1.0));
            ui.checkbox(&mut surface.visible, "Visible");
        });
        ui.end_row();

        // --- Area of Interest (Quellen-Zuschnitt, normalisiert 0..1) ---
        theme::field_label(ui, "Area of Interest");
        let mut aoi = surface.area_of_interest.is_some();
        if ui.checkbox(&mut aoi, "Crop source").changed() {
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
