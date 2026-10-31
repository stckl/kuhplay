//! Stage-Editor (QLab 5: „Video → Video Outputs → Edit Stage…“).
//!
//! Eigenes Fenster pro Stage:
//!
//! * Kopf: Name, Layer, Keep Rendering, Größe mit `Resize…`.
//! * Links: Regionsliste (Buchstabe mit Routenfarbe, Route-Auswahl,
//!   Grid-Häkchen, Löschen) + „New region with output…“ + Umschalter
//!   Move/Draw.
//! * Rechts: Größe/Position/Edge Blends/Auto/Blend-Gamma der gewählten
//!   Region.
//! * Zeichenfläche: Stage-Rechteck, Regionen in Routenfarbe (ziehbar,
//!   Griffe zur Größe), Einrasten an Rand, Mittellinien und andere
//!   Regionen; ⌘ schaltet Einrasten aus. Pfeiltasten: 1 Pixel, ⇧ 10 Pixel,
//!   ⌥ Größe. Blend-Bereich schraffiert, Region ohne Route gepunktet.
//!
//! Ausgangspunkt waren `screen_editor.rs::draw_map` und `snap_to_displays`
//! (vor der Umstellung auf Video-Routing).

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use uuid::Uuid;

use crate::core::{region_letter, regions_in_reading_order, Crop, Region, StagePlacement};

use super::app::ShowApp;
use super::theme::{self, palette};

/// Drag-Interaktion auf der Zeichenfläche.
#[derive(Debug, Clone, Copy, Default)]
enum Drag {
    #[default]
    Idle,
    /// Region verschieben (Topleft folgt dem Zeiger + Griff-Offset).
    Move { id: Uuid, grab: (f32, f32) },
    /// Region an einer Ecke skalieren.
    Resize { id: Uuid, corner: Corner },
    /// Rubber-Band: neue Region aufziehen (Draw-Modus).
    Rubber { start: (f32, f32) },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

/// Zeichnet das Stage-Editor-Fenster (falls geöffnet).
pub fn draw(ctx: &egui::Context, app: &mut ShowApp) {
    let Some(stage_id) = app.stage_editor_open else {
        return;
    };
    // Stage existiert nicht mehr → Editor schließen.
    let Some(stage) = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .cloned()
    else {
        app.stage_editor_open = None;
        return;
    };
    let mut open = true;
    let title = format!("Stage Editor: {}", stage.name);
    // Echtes OS-Fenster (in Headless-Tests: Overlay-Fallback).
    super::viewport_window::draw(
        ctx,
        &super::viewport_window::WindowSpec::new(
            "kuhplay-stage-editor",
            &title,
            [720.0, 580.0],
            [560.0, 420.0],
        ),
        &mut open,
        app,
        |_ctx, ui, app| {
            // Scrollbar: Im OS-Fenster darf die Vorschau unten nicht
            // abgeschnitten werden (Columns + Canvas zusammen > Fensterhoehe).
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.columns(2, |uis| {
                        let [left, right] = uis else {
                            return;
                        };
                        draw_regions_panel(left, app, stage_id);
                        draw_properties(right, app, stage_id);
                    });
                    ui.separator();
                    ui.scope(|ui| draw_placements(ui, app, stage_id));
                    ui.separator();
                    ui.scope(|ui| draw_key_fill(ui, app, stage_id));
                    ui.separator();
                    ui.scope(|ui| draw_canvas(ui, app, stage_id));
                });
        },
    );
    if !open {
        app.stage_editor_open = None;
    }
}

/// Kopf- und Regionsliste (linke Spalte).
fn draw_regions_panel(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .cloned()
    else {
        return;
    };
    let letters: std::collections::HashMap<Uuid, String> =
        regions_in_reading_order(stage.regions.iter())
            .iter()
            .enumerate()
            .map(|(i, region)| (region.id, region_letter(i)))
            .collect();
    let mut configure = false;
    let mut rename: Option<String> = None;
    let mut remove_region: Option<Uuid> = None;
    let mut add_region: Option<Option<Uuid>> = None;
    let mut open_resize = false;

    theme::section_label(ui, "Stage");
    theme::form_grid("stage_editor_head").show(ui, |ui| {
        theme::field_label(ui, "Name");
        let id = ui.id().with("stage_name");
        let mut name = ui
            .ctx()
            .data_mut(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| stage.name.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
        app.text_field_ids.insert(r.id);
        if r.lost_focus() && name.trim() != stage.name {
            rename = Some(name.trim().to_string());
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, name));
        ui.end_row();

        theme::field_label(ui, "Layer");
        let mut layer = stage.layer;
        ui.add(egui::DragValue::new(&mut layer).range(-99..=99));
        if layer != stage.layer {
            app.stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
                .unwrap()
                .layer = layer;
            configure = true;
        }
        ui.end_row();

        theme::field_label(ui, "Keep Rendering");
        let mut keep = stage.keep_rendering;
        if ui.checkbox(&mut keep, "").changed() {
            app.stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
                .unwrap()
                .keep_rendering = keep;
            configure = true;
        }
        ui.end_row();

        // --- Stage-Crop (Sichtfenster der Stage, normalisiert 0..1) ---
        theme::field_label(ui, "Crop");
        let mut crop_on = stage.crop.is_some();
        let mut crop = stage.crop.unwrap_or_else(Crop::none);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut crop_on, "Crop stage").changed() && crop_on {
                crop = Crop::none();
            }
            if crop_on {
                ui.add(
                    egui::DragValue::new(&mut crop.left)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .prefix("L "),
                );
                ui.add(
                    egui::DragValue::new(&mut crop.top)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .prefix("T "),
                );
                ui.add(
                    egui::DragValue::new(&mut crop.right)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .prefix("R "),
                );
                ui.add(
                    egui::DragValue::new(&mut crop.bottom)
                        .speed(0.01)
                        .range(0.0..=1.0)
                        .prefix("B "),
                );
            }
        });
        if crop_on || crop != Crop::none() {
            let new_crop = crop_on.then_some(crop);
            if let Some(s) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                s.crop = new_crop;
            }
            configure = true;
        }
        ui.end_row();

        // --- Stage-Edge (Randabblendung am Route-Ausgang) ---
        theme::field_label(ui, "Edge");
        let mut edge = stage.edge;
        let mut soft = stage.soft_edge;
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut edge.left)
                    .speed(1.0)
                    .range(0.0..=4096.0)
                    .prefix("L "),
            );
            ui.add(
                egui::DragValue::new(&mut edge.right)
                    .speed(1.0)
                    .range(0.0..=4096.0)
                    .prefix("R "),
            );
            ui.add(
                egui::DragValue::new(&mut edge.top)
                    .speed(1.0)
                    .range(0.0..=4096.0)
                    .prefix("T "),
            );
            ui.add(
                egui::DragValue::new(&mut edge.bottom)
                    .speed(1.0)
                    .range(0.0..=4096.0)
                    .prefix("B "),
            );
            if ui
                .checkbox(&mut soft, "Soft Edge")
                .on_hover_text("Soft Edge: gamma-blended ramp; Hard Edge: linear ramp")
                .changed()
            {
                if let Some(s) = app
                    .stages
                    .stages_mut()
                    .iter_mut()
                    .find(|s| s.id == stage_id)
                {
                    s.soft_edge = soft;
                }
                configure = true;
            }
        });
        if edge != stage.edge {
            if let Some(s) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                s.edge = edge;
            }
            configure = true;
        }
        ui.end_row();

        theme::field_label(ui, "Canvas");
        ui.horizontal(|ui| {
            ui.label(format!(
                "{}x{}",
                stage.geometry.width as u32, stage.geometry.height as u32
            ));
            if ui.button("Resize…").clicked() {
                open_resize = true;
            }
        });
        ui.end_row();
    });
    ui.add_space(theme::size::SPACE_MEDIUM);

    theme::section_label(ui, "Regions");
    if let Some(letter) = letters.get(&selected_region_id(ui, &stage)) {
        ui.label(
            egui::RichText::new(format!("selected: {letter}"))
                .weak()
                .small(),
        );
    }
    for (index, region) in stage.regions.iter().enumerate() {
        let mut updated = region.clone();
        let letter = letters.get(&region.id).cloned().unwrap_or_default();
        let route_color = region
            .route
            .and_then(|id| app.routes.find(id))
            .map(|r| Color32::from_rgb(r.color[0], r.color[1], r.color[2]));
        let mut select = false;
        egui::Frame::none()
            .fill(palette::RAISED)
            .rounding(egui::Rounding::same(5.0))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(selected_region_id(ui, &stage) == region.id, &letter)
                        .clicked()
                    {
                        select = true;
                    }
                    // Farbfleck in Routenfarbe.
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 2.0, route_color.unwrap_or(palette::TEXT_DIM));
                    // Route-Auswahl.
                    let current = region
                        .route
                        .and_then(|id| app.routes.find(id))
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| "None".to_string());
                    egui::ComboBox::from_id_salt(("region_route", region.id))
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut updated.route, None, "None");
                            for route in app.routes.routes() {
                                ui.selectable_value(
                                    &mut updated.route,
                                    Some(route.id),
                                    &route.name,
                                );
                            }
                        });
                    let mut grid = updated.grid;
                    if ui.checkbox(&mut grid, "Grid").changed() {
                        updated.grid = grid;
                    }
                    if ui.button("Delete").clicked() {
                        remove_region = Some(region.id);
                    }
                });
                let _ = index;
            });
        if select {
            set_selected_region(ui, region.id);
        }
        if updated != *region {
            if let Some(s) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                if let Some(r) = s.regions.iter_mut().find(|r| r.id == region.id) {
                    *r = updated;
                }
            }
            configure = true;
        }
        ui.add_space(2.0);
    }
    if stage.regions.is_empty() {
        ui.label(egui::RichText::new("no regions yet").weak().small());
    }
    ui.horizontal(|ui| {
        // „New region with output…“: Menü der Routen.
        let menu = egui::ComboBox::from_id_salt("new_region_output")
            .selected_text("+ New region with output…");
        menu.show_ui(ui, |ui| {
            for route in app.routes.routes() {
                if ui.selectable_label(false, &route.name).clicked() {
                    add_region = Some(Some(route.id));
                }
            }
            if ui.selectable_label(false, "Without route").clicked() {
                add_region = Some(None);
            }
        });
        if ui.button("+ Empty region").clicked() {
            add_region = Some(None);
        }
    });
    ui.add_space(theme::size::SPACE_SMALL);
    // Umschalter Move/Draw (Draw: neue Regionen auf der Fläche aufziehen).
    let mode_id = ui.id().with("stage_editor_draw");
    let mut draw_mode = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(mode_id))
        .unwrap_or(false);
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!draw_mode, "Move")
            .on_hover_text("Drag moves regions")
            .clicked()
        {
            draw_mode = false;
        }
        if ui
            .selectable_label(draw_mode, "Draw")
            .on_hover_text("Drag on the canvas draws a new region")
            .clicked()
        {
            draw_mode = true;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(mode_id, draw_mode));

    if let Some(Some(route_id)) = add_region {
        // Neue Region: halbe Stage-Größe in der Mitte.
        let (sw, sh) = (stage.geometry.width, stage.geometry.height);
        let mut region = Region::new(sw / 4.0, sh / 4.0, sw / 2.0, sh / 2.0);
        region.route = Some(route_id);
        region.auto_blend = true;
        if let Some(s) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            s.regions.push(region);
        }
        configure = true;
    } else if let Some(None) = add_region {
        let (sw, sh) = (stage.geometry.width, stage.geometry.height);
        let region = Region::new(sw / 4.0, sh / 4.0, sw / 2.0, sh / 2.0);
        if let Some(s) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            s.regions.push(region);
        }
        configure = true;
    }
    if let Some(id) = remove_region {
        if let Some(s) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            s.regions.retain(|r| r.id != id);
        }
        configure = true;
    }
    if let Some(new_name) = rename {
        app.rename_stage(&stage.name, &new_name);
    }
    if open_resize {
        ui.ctx()
            .data_mut(|d| d.insert_temp(ui.id().with("stage_resize_open"), true));
    }
    // Resize-Popup.
    let resize_open = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(ui.id().with("stage_resize_open")))
        .unwrap_or(false);
    if resize_open {
        let mut size = (stage.geometry.width as i32, stage.geometry.height as i32);
        egui::Window::new("Resize Stage")
            .id(egui::Id::new(("stage_resize", stage_id)))
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                theme::form_grid("stage_resize").show(ui, |ui| {
                    theme::field_label(ui, "Width");
                    ui.add(egui::DragValue::new(&mut size.0).range(16..=16384));
                    ui.end_row();
                    theme::field_label(ui, "Height");
                    ui.add(egui::DragValue::new(&mut size.1).range(16..=16384));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        if let Some(s) = app
                            .stages
                            .stages_mut()
                            .iter_mut()
                            .find(|s| s.id == stage_id)
                        {
                            s.geometry.width = size.0.max(16) as f32;
                            s.geometry.height = size.1.max(16) as f32;
                            // Regionen auf die neue Größe begrenzen.
                            for r in &mut s.regions {
                                r.width = r.width.min(s.geometry.width);
                                r.height = r.height.min(s.geometry.height);
                            }
                        }
                        configure = true;
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                egui::Id::new(("stage-editor", stage_id)).with("stage_resize_open"),
                                false,
                            )
                        });
                    }
                    if ui.button("Cancel").clicked() {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                egui::Id::new(("stage-editor", stage_id)).with("stage_resize_open"),
                                false,
                            )
                        });
                    }
                });
            });
    }
    if configure {
        if let Some(s) = app
            .stages
            .stages()
            .iter()
            .find(|s| s.id == stage_id)
            .cloned()
        {
            app.configure_stage(s);
        }
    }
}

/// ID der gewählten Region (Temp-Speicher des Canvas-Contexts).
fn selected_region_id(ui: &egui::Ui, stage: &crate::core::Stage) -> Uuid {
    let id = egui::Id::new(("stage_editor_selected_region", stage.id));
    let stored = ui.ctx().data_mut(|d| d.get_temp::<Uuid>(id));
    stored
        .filter(|rid| stage.regions.iter().any(|r| r.id == *rid))
        .or_else(|| stage.regions.first().map(|r| r.id))
        .unwrap_or_else(Uuid::new_v4)
}

fn set_selected_region(ui: &egui::Ui, region: Uuid) {
    // Beim Canvas-Aufruf wird dieselbe Id benutzt (Stage-ID fehlt dort):
    // wir speichern global die letzte Auswahl.
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("stage_editor_selected_region"), region));
}

/// Eigenschaften der gewählten Region (rechte Spalte): Größe, Position,
/// Edge Blends, Auto, Blend-Gamma.
fn draw_properties(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .cloned()
    else {
        return;
    };
    let Some(region_id) = stage
        .regions
        .iter()
        .map(|r| r.id)
        .find(|id| {
            let sel = ui
                .ctx()
                .data_mut(|d| d.get_temp::<Uuid>(egui::Id::new("stage_editor_selected_region")));
            sel == Some(*id)
        })
        .or_else(|| stage.regions.first().map(|r| r.id))
    else {
        ui.label(egui::RichText::new("no region selected").weak());
        return;
    };
    let Some(region) = stage.regions.iter().find(|r| r.id == region_id).cloned() else {
        return;
    };
    let mut updated = region.clone();
    let letters: std::collections::HashMap<Uuid, String> =
        regions_in_reading_order(stage.regions.iter())
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, region_letter(i)))
            .collect();
    theme::section_label(
        ui,
        &format!(
            "Region {}",
            letters.get(&region_id).cloned().unwrap_or_default()
        ),
    );
    let mut changed = false;
    theme::form_grid("stage_editor_region").show(ui, |ui| {
        theme::field_label(ui, "Position");
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut updated.x).speed(1.0).prefix("X "))
                .changed();
            changed |= ui
                .add(egui::DragValue::new(&mut updated.y).speed(1.0).prefix("Y "))
                .changed();
        });
        ui.end_row();
        theme::field_label(ui, "Size");
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.width)
                        .speed(1.0)
                        .range(1.0..=stage.geometry.width)
                        .prefix("W "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.height)
                        .speed(1.0)
                        .range(1.0..=stage.geometry.height)
                        .prefix("H "),
                )
                .changed();
        });
        ui.end_row();
        theme::field_label(ui, "Edge Blends");
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.blend.left)
                        .speed(1.0)
                        .range(0.0..=4096.0)
                        .prefix("L "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.blend.right)
                        .speed(1.0)
                        .range(0.0..=4096.0)
                        .prefix("R "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.blend.top)
                        .speed(1.0)
                        .range(0.0..=4096.0)
                        .prefix("T "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut updated.blend.bottom)
                        .speed(1.0)
                        .range(0.0..=4096.0)
                        .prefix("B "),
                )
                .changed();
        });
        ui.end_row();
        theme::field_label(ui, "Auto Blend");
        let mut auto = updated.auto_blend;
        if ui.checkbox(&mut auto, "").changed() {
            updated.auto_blend = auto;
            changed = true;
        }
        ui.end_row();
        theme::field_label(ui, "Blend Gamma");
        let mut gamma = stage.blend_gamma;
        if ui
            .add(
                egui::DragValue::new(&mut gamma)
                    .speed(0.05)
                    .range(0.5..=5.0),
            )
            .changed()
        {
            if let Some(s) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                s.blend_gamma = gamma;
            }
            changed = true;
        }
        ui.end_row();
    });
    if changed {
        if let Some(s) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            if let Some(r) = s.regions.iter_mut().find(|r| r.id == region_id) {
                *r = updated;
            }
        }
        if let Some(s) = app
            .stages
            .stages()
            .iter()
            .find(|s| s.id == stage_id)
            .cloned()
        {
            app.configure_stage(s);
        }
    }
}

/// Zeichenfläche: Stage, Regionen in Routenfarbe, ziehen/skalieren,
/// Einrasten, Pfeiltasten.
fn draw_canvas(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .cloned()
    else {
        return;
    };
    let canvas_height = 240.0f32;
    let (canvas_rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), canvas_height),
        Sense::click_and_drag(),
    );
    ui.set_clip_rect(canvas_rect.intersect(ui.clip_rect()));
    ui.painter()
        .rect_filled(canvas_rect, 4.0, palette::BACKGROUND);

    // Welt (Stage-Pixel) → Bildschirm.
    let (sw, sh) = (
        stage.geometry.width.max(1.0),
        stage.geometry.height.max(1.0),
    );
    let margin = 24.0f32;
    let scale = (canvas_rect.width() / (sw + 2.0 * margin))
        .min(canvas_rect.height() / (sh + 2.0 * margin))
        .max(0.0001);
    let offset = Vec2::new(canvas_rect.left() + margin, canvas_rect.top() + margin);
    let to_screen = |x: f32, y: f32| Pos2::new(offset.x + x * scale, offset.y + y * scale);
    let to_world = |p: Pos2| {
        (
            ((p.x - offset.x) / scale).round(),
            ((p.y - offset.y) / scale).round(),
        )
    };

    let pointer_world = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| canvas_rect.contains(*p))
        .map(to_world);
    let drag_id = ui.id().with(("stage_editor_drag", stage_id));

    // --- Tastatur: Pfeiltasten für die gewählte Region ---
    if !app.text_field_focused(ui.ctx()) {
        let (up, down, left, right, shift, alt) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.modifiers.shift,
                i.modifiers.alt,
            )
        });
        if up || down || left || right {
            let step = if shift { 10.0 } else { 1.0 };
            let selected = selected_region_of(ui, &stage);
            if let Some(id) = selected {
                let mut changed = false;
                if let Some(s) = app
                    .stages
                    .stages_mut()
                    .iter_mut()
                    .find(|s| s.id == stage_id)
                {
                    if let Some(r) = s.regions.iter_mut().find(|r| r.id == id) {
                        // ⌥ ändert die Größe statt der Position.
                        if alt {
                            if left {
                                r.width = (r.width - step).max(1.0);
                                changed = true;
                            }
                            if right {
                                r.width += step;
                                changed = true;
                            }
                            if up {
                                r.height = (r.height - step).max(1.0);
                                changed = true;
                            }
                            if down {
                                r.height += step;
                                changed = true;
                            }
                        } else if up {
                            r.y -= step;
                            changed = true;
                        } else if down {
                            r.y += step;
                            changed = true;
                        } else if left {
                            r.x -= step;
                            changed = true;
                        } else if right {
                            r.x += step;
                            changed = true;
                        }
                    }
                }
                if changed {
                    if let Some(s) = app
                        .stages
                        .stages()
                        .iter()
                        .find(|s| s.id == stage_id)
                        .cloned()
                    {
                        app.configure_stage(s);
                    }
                }
            }
        }
    }

    // --- Interaktion ---
    if response.drag_started() {
        if let Some(pw) = pointer_world {
            let hit = stage
                .regions
                .iter()
                .rev()
                .find(|r| {
                    pw.0 >= r.x && pw.0 < r.x + r.width && pw.1 >= r.y && pw.1 < r.y + r.height
                })
                .map(|r| r.id);
            let draw_mode = ui
                .ctx()
                .data_mut(|d| d.get_temp::<bool>(ui.id().with("stage_editor_draw")))
                .unwrap_or(false);
            if let Some(id) = hit {
                set_selected_region(ui, id);
                if !draw_mode {
                    let region = stage.regions.iter().find(|r| r.id == id).unwrap();
                    let corner = corner_at(region, pw, scale);
                    let drag = match corner {
                        Some(c) => Drag::Resize { id, corner: c },
                        None => Drag::Move {
                            id,
                            grab: (pw.0 - region.x, pw.1 - region.y),
                        },
                    };
                    ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
                } else {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(drag_id, Drag::Rubber { start: pw }));
                }
            } else {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(drag_id, Drag::Rubber { start: pw }));
            }
        }
    }
    if response.dragged() {
        let drag = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
        // ⌘ schaltet das Einrasten aus.
        let snap_enabled = !ui.input(|i| i.modifiers.command);
        if let (Some(drag), Some(pw)) = (drag, pointer_world) {
            let mut changed = false;
            match drag {
                Drag::Move { id, grab } => {
                    let (mut nx, mut ny) = (pw.0 - grab.0, pw.1 - grab.1);
                    if snap_enabled {
                        (nx, ny) = snap_to_stage_and_regions(&stage, id, nx, ny, 16.0 / scale);
                    }
                    if let Some(s) = app
                        .stages
                        .stages_mut()
                        .iter_mut()
                        .find(|s| s.id == stage_id)
                    {
                        if let Some(r) = s.regions.iter_mut().find(|r| r.id == id) {
                            r.x = nx.clamp(0.0, (s.geometry.width - r.width).max(0.0));
                            r.y = ny.clamp(0.0, (s.geometry.height - r.height).max(0.0));
                        }
                    }
                    changed = true;
                }
                Drag::Resize { id, corner } => {
                    if let Some(s) = app
                        .stages
                        .stages_mut()
                        .iter_mut()
                        .find(|s| s.id == stage_id)
                    {
                        if let Some(r) = s.regions.iter_mut().find(|r| r.id == id) {
                            apply_corner_resize(r, corner, pw, s.geometry.width, s.geometry.height);
                        }
                    }
                    changed = true;
                }
                Drag::Rubber { .. } | Drag::Idle => {}
            }
            if changed {
                if let Some(s) = app
                    .stages
                    .stages()
                    .iter()
                    .find(|s| s.id == stage_id)
                    .cloned()
                {
                    app.configure_stage(s);
                }
            }
        }
    }
    if response.drag_stopped() {
        let drag = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
        ui.ctx().data_mut(|d| d.remove_temp::<Drag>(drag_id));
        if let (Some(Drag::Rubber { start }), Some(pw)) = (drag, pointer_world) {
            let x = start.0.min(pw.0);
            let y = start.1.min(pw.1);
            let w = (start.0 - pw.0).abs().max(16.0);
            let h = (start.1 - pw.1).abs().max(16.0);
            if let Some(s) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                let mut region =
                    Region::new(x, y, w.min(s.geometry.width), h.min(s.geometry.height));
                region.auto_blend = true;
                s.regions.push(region);
            }
            if let Some(s) = app
                .stages
                .stages()
                .iter()
                .find(|s| s.id == stage_id)
                .cloned()
            {
                app.configure_stage(s);
            }
        }
    }
    if response.clicked() {
        if let Some(pw) = pointer_world {
            let hit = stage
                .regions
                .iter()
                .rev()
                .find(|r| {
                    pw.0 >= r.x && pw.0 < r.x + r.width && pw.1 >= r.y && pw.1 < r.y + r.height
                })
                .map(|r| r.id);
            if let Some(id) = hit {
                set_selected_region(ui, id);
            }
        }
    }

    // --- Stage-Umriss + Mittellinien ---
    let stage_rect = Rect::from_min_size(to_screen(0.0, 0.0), egui::vec2(sw * scale, sh * scale));
    ui.painter()
        .rect_stroke(stage_rect, 2.0, Stroke::new(1.0_f32, palette::COMPLETED));
    let center = stage_rect.center();
    ui.painter().line_segment(
        [
            Pos2::new(center.x, stage_rect.top()),
            Pos2::new(center.x, stage_rect.bottom()),
        ],
        Stroke::new(0.5_f32, with_alpha(palette::COMPLETED, 40)),
    );
    ui.painter().line_segment(
        [
            Pos2::new(stage_rect.left(), center.y),
            Pos2::new(stage_rect.right(), center.y),
        ],
        Stroke::new(0.5_f32, with_alpha(palette::COMPLETED, 40)),
    );
    ui.painter().text(
        stage_rect.left_bottom() + egui::vec2(4.0, -2.0),
        Align2::LEFT_BOTTOM,
        format!("Stage {}x{}", sw as u32, sh as u32),
        FontId::proportional(10.0),
        palette::COMPLETED,
    );

    // --- Regionen zeichnen ---
    let letters: std::collections::HashMap<Uuid, String> =
        regions_in_reading_order(stage.regions.iter())
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, region_letter(i)))
            .collect();
    let selected = selected_region_of(ui, &stage);
    for region in &stage.regions {
        let r = Rect::from_min_size(
            to_screen(region.x, region.y),
            egui::vec2(region.width * scale, region.height * scale),
        );
        let color = region
            .route
            .and_then(|id| app.routes.find(id))
            .map(|route| Color32::from_rgb(route.color[0], route.color[1], route.color[2]))
            .unwrap_or(palette::TEXT_DIM);
        let is_sel = selected == Some(region.id);
        ui.painter().rect_filled(
            r,
            2.0,
            theme::with_alpha(color, if is_sel { 72 } else { 36 }),
        );
        // Ohne Route gepunktet, mit Route durchgezogen.
        let stroke = if region.route.is_some() {
            Stroke::new(
                if is_sel { 2.0 } else { 1.0 },
                theme::with_alpha(color, 220),
            )
        } else {
            Stroke::new(1.0_f32, theme::with_alpha(color, 160))
        };
        ui.painter().rect_stroke(r, 2.0, stroke);
        // Blend-Bereiche schraffiert (diagonale Linien).
        for (rect, _width) in blend_rects(region, r) {
            hatch(ui, rect, theme::with_alpha(color, 90));
        }
        // Beschriftung: Buchstabe.
        ui.painter().text(
            r.min + egui::vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            letters.get(&region.id).cloned().unwrap_or_default(),
            FontId::proportional(10.0),
            palette::TEXT,
        );
        // Griffe an den Ecken (sichtbar bei Auswahl).
        if is_sel {
            for corner in corner_rects(r) {
                ui.painter().rect_filled(corner, 1.5, palette::ACCENT);
            }
        }
    }

    // --- Rubber-Band ---
    let rubber = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
    if let (Some(Drag::Rubber { start }), Some(pw)) = (rubber, pointer_world) {
        let rect = Rect::from_two_pos(to_screen(start.0, start.1), to_screen(pw.0, pw.1));
        ui.painter()
            .rect_filled(rect, 2.0, with_alpha(palette::ACCENT, 30));
        ui.painter()
            .rect_stroke(rect, 2.0, Stroke::new(1.5_f32, palette::ACCENT));
    }
    ui.label(
        egui::RichText::new(
            "Drag: move · corner: resize · arrows: 1 px (Shift 10 px, Alt size) · Cmd: no snap",
        )
        .weak()
        .small(),
    );
}

/// Gewählte Region (konsistent mit `set_selected_region`).
fn selected_region_of(ui: &egui::Ui, stage: &crate::core::Stage) -> Option<Uuid> {
    let sel = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Uuid>(egui::Id::new("stage_editor_selected_region")));
    sel.filter(|id| stage.regions.iter().any(|r| r.id == *id))
}

/// Ecke unter dem Zeiger (Toleranz in Bildschirm-Pixeln).
fn corner_at(region: &Region, pw: (f32, f32), scale: f32) -> Option<Corner> {
    let tol = 12.0 / scale;
    let near_left = (pw.0 - region.x).abs() <= tol;
    let near_right = (pw.0 - (region.x + region.width)).abs() <= tol;
    let near_top = (pw.1 - region.y).abs() <= tol;
    let near_bottom = (pw.1 - (region.y + region.height)).abs() <= tol;
    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(Corner::TopLeft),
        (_, true, true, _) => Some(Corner::TopRight),
        (_, true, _, true) => Some(Corner::BottomRight),
        (true, _, _, true) => Some(Corner::BottomLeft),
        _ => None,
    }
}

/// Ecke auf Region anwenden (min. 16 px, auf die Stage begrenzt).
fn apply_corner_resize(
    region: &mut Region,
    corner: Corner,
    pw: (f32, f32),
    stage_w: f32,
    stage_h: f32,
) {
    let (mut x0, mut y0) = (region.x, region.y);
    let (mut x1, mut y1) = (region.x + region.width, region.y + region.height);
    match corner {
        Corner::TopLeft => {
            x0 = pw.0.min(x1 - 16.0).max(0.0);
            y0 = pw.1.min(y1 - 16.0).max(0.0);
        }
        Corner::TopRight => {
            x1 = pw.0.max(x0 + 16.0).min(stage_w);
            y0 = pw.1.min(y1 - 16.0).max(0.0);
        }
        Corner::BottomRight => {
            x1 = pw.0.max(x0 + 16.0).min(stage_w);
            y1 = pw.1.max(y0 + 16.0).min(stage_h);
        }
        Corner::BottomLeft => {
            x0 = pw.0.min(x1 - 16.0).max(0.0);
            y1 = pw.1.max(y0 + 16.0).min(stage_h);
        }
    }
    region.x = x0;
    region.y = y0;
    region.width = (x1 - x0).max(1.0);
    region.height = (y1 - y0).max(1.0);
}

/// Snap: Region-Kanten an Stage-Rand, Mittellinien und andere Regionen
/// einrasten (Toleranz in Welt-Pixeln). (Nach screen_editor::snap_to_displays.)
fn snap_to_stage_and_regions(
    stage: &crate::core::Stage,
    own_id: Uuid,
    x: f32,
    y: f32,
    tol: f32,
) -> (f32, f32) {
    let own = stage.regions.iter().find(|r| r.id == own_id);
    let (w, h) = own.map(|r| (r.width, r.height)).unwrap_or((1.0, 1.0));
    let (sw, sh) = (stage.geometry.width, stage.geometry.height);
    let mut candidates_x = vec![0.0, sw - w, sw / 2.0 - w / 2.0, sw / 2.0, sw];
    let mut candidates_y = vec![0.0, sh - h, sh / 2.0 - h / 2.0, sh / 2.0, sh];
    for r in &stage.regions {
        if r.id == own_id {
            continue;
        }
        candidates_x.extend([r.x, r.x + r.width, r.x - w, r.x + r.width - w]);
        candidates_y.extend([r.y, r.y + r.height, r.y - h, r.y + r.height - h]);
    }
    let snap = |v: f32, candidates: &[f32]| -> f32 {
        candidates
            .iter()
            .filter(|c| (v - **c).abs() <= tol)
            .min_by(|a, b| (v - **a).abs().partial_cmp(&(v - **b)).unwrap())
            .copied()
            .unwrap_or(v)
    };
    (snap(x, &candidates_x), snap(y, &candidates_y))
}

/// Blend-Bereiche einer Region als Bildschirm-Rechtecke.
fn blend_rects(region: &Region, rect: Rect) -> Vec<(Rect, f32)> {
    let b = &region.blend;
    let scale_x = rect.width() / region.width.max(1.0);
    let scale_y = rect.height() / region.height.max(1.0);
    let mut out = Vec::new();
    if b.left > 0.0 {
        out.push((
            Rect::from_min_size(rect.min, egui::vec2(b.left * scale_x, rect.height())),
            b.left,
        ));
    }
    if b.right > 0.0 {
        out.push((
            Rect::from_min_size(
                Pos2::new(rect.right() - b.right * scale_x, rect.top()),
                egui::vec2(b.right * scale_x, rect.height()),
            ),
            b.right,
        ));
    }
    if b.top > 0.0 {
        out.push((
            Rect::from_min_size(rect.min, egui::vec2(rect.width(), b.top * scale_y)),
            b.top,
        ));
    }
    if b.bottom > 0.0 {
        out.push((
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - b.bottom * scale_y),
                egui::vec2(rect.width(), b.bottom * scale_y),
            ),
            b.bottom,
        ));
    }
    out
}

/// Griff-Rechtecke an den vier Ecken.
fn corner_rects(r: Rect) -> Vec<Rect> {
    let s = 7.0;
    [
        Rect::from_min_size(r.min, egui::vec2(s, s)),
        Rect::from_min_size(Pos2::new(r.right() - s, r.top()), egui::vec2(s, s)),
        Rect::from_min_size(Pos2::new(r.right() - s, r.bottom() - s), egui::vec2(s, s)),
        Rect::from_min_size(Pos2::new(r.left(), r.bottom() - s), egui::vec2(s, s)),
    ]
    .to_vec()
}

/// Schraffur (Blend-Bereich).
fn hatch(ui: &mut egui::Ui, rect: Rect, color: Color32) {
    let step = 8.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let x1 = (x + rect.height()).min(rect.right());
        ui.painter().line_segment(
            [
                Pos2::new(x.max(rect.left()), rect.bottom()),
                Pos2::new(
                    x1,
                    rect.top() + (x1 - x).max(0.0) - (x.max(rect.left()) - x),
                ),
            ],
            Stroke::new(1.0_f32, color),
        );
        x += step;
    }
}

/// Farbe mit Alpha (gamma-korrigiert, siehe docs/TODO-video-routing.md §7).
fn with_alpha(c: Color32, a: u8) -> Color32 {
    c.gamma_multiply(f32::from(a) / 255.0)
}

// ------------------------------------------------------------------
// Display-Placements: Stage direkt auf Displays legen (1:1 Gerätepixel,
// Clipping am Displayrand, Z-Order über Stage-Layer) + Mini-Map.
// ------------------------------------------------------------------

/// Section „Displays“: Platzierung der Stage pro Display (in Gerätepixeln
/// des Displays) inkl. Mini-Map, die das ganze Display UND die ganze
/// Stagefläche (auch Überstand über den Displayrand hinaus) zeigt, plus
/// alle anderen Stages auf demselben Display und die Z-Reihenfolge.
fn draw_placements(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    theme::section_label(ui, "Displays (Stage 1:1 in Gerätepixeln, Clipping am Rand)");
    ui.label(
        egui::RichText::new(
            "Display bleibt voll/native. Stage-Rechteck in Display-Pixeln; Überstand wird abgeschnitten. Z-Order über Stage-Layer.",
        )
        .small()
        .color(palette::TEXT_DIM),
    );
    ui.add_space(theme::size::SPACE_SMALL);

    let displays: Vec<crate::core::DisplayRefInfo> = app
        .displays
        .iter()
        .map(crate::core::DisplayRefInfo::from)
        .collect();
    if displays.is_empty() {
        ui.label(
            egui::RichText::new("No displays detected in this session").color(palette::TEXT_FAINT),
        );
        return;
    }

    // Gewähltes Display für die Map (egui-Temp pro Stage).
    let map_id = egui::Id::new(("stage_placement_map_display", stage_id));
    let mut map_display: u32 = ui
        .ctx()
        .data_mut(|d| d.get_temp::<u32>(map_id).unwrap_or(0));
    if !displays.iter().any(|d| d.id == map_display) {
        map_display = displays[0].id;
    }
    let display = displays.iter().find(|d| d.id == map_display).unwrap();
    let (disp_px_w, disp_px_h) = display.pixel_size();

    // Display-Picker + Placement-Zeilen der eigenen Stage auf diesem Display.
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("stage_placement_display_combo")
            .selected_text(format!("{} ({}×{} px)", display.name, disp_px_w, disp_px_h))
            .show_ui(ui, |ui| {
                for d in &displays {
                    let (w, h) = d.pixel_size();
                    ui.selectable_value(
                        &mut map_display,
                        d.id,
                        format!("{} ({}×{} px)", d.name, w, h),
                    );
                }
            });
        ui.ctx().data_mut(|d| d.insert_temp(map_id, map_display));

        let has_placement = app
            .stages
            .stages()
            .iter()
            .find(|s| s.id == stage_id)
            .map(|s| s.placements.iter().any(|p| p.display == map_display))
            .unwrap_or(false);
        if !has_placement && ui.button("Place stage on this display").clicked() {
            let stage_w = app
                .stages
                .stages()
                .iter()
                .find(|s| s.id == stage_id)
                .map(|s| s.geometry.width.max(1.0) as u32)
                .unwrap_or(disp_px_w);
            let stage_h = app
                .stages
                .stages()
                .iter()
                .find(|s| s.id == stage_id)
                .map(|s| s.geometry.height.max(1.0) as u32)
                .unwrap_or(disp_px_h);
            if let Some(stage) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                stage.placements.push(StagePlacement {
                    display: map_display,
                    x: 0,
                    y: 0,
                    width: stage_w.min(disp_px_w),
                    height: stage_h.min(disp_px_h),
                });
            }
        }
    });

    // Eigene Placement(s) auf diesem Display editieren (X/Y/W/H in Gerätepixeln).
    let mut remove = false;
    {
        let stage = app
            .stages
            .stages()
            .iter()
            .find(|s| s.id == stage_id)
            .cloned();
        if let Some(stage) = stage {
            if let Some(p) = stage
                .placements
                .iter()
                .find(|p| p.display == map_display)
                .cloned()
            {
                let mut p = p;
                theme::form_grid("stage_placement_fields").show(ui, |ui| {
                    theme::field_label(ui, "Position (px)");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut p.x).speed(1.0).prefix("X "));
                        ui.add(egui::DragValue::new(&mut p.y).speed(1.0).prefix("Y "));
                    });
                    ui.end_row();
                    theme::field_label(ui, "Size (px)");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut p.width)
                                .speed(1.0)
                                .range(1..=30000)
                                .prefix("W "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut p.height)
                                .speed(1.0)
                                .range(1..=30000)
                                .prefix("H "),
                        );
                    });
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.small_button("Remove placement").clicked() {
                        remove = true;
                    }
                    if p.x
                        != stage
                            .placements
                            .iter()
                            .find(|q| q.display == map_display)
                            .unwrap()
                            .x
                        || p.y
                            != stage
                                .placements
                                .iter()
                                .find(|q| q.display == map_display)
                                .unwrap()
                                .y
                        || p.width
                            != stage
                                .placements
                                .iter()
                                .find(|q| q.display == map_display)
                                .unwrap()
                                .width
                        || p.height
                            != stage
                                .placements
                                .iter()
                                .find(|q| q.display == map_display)
                                .unwrap()
                                .height
                    {
                        if let Some(st) = app
                            .stages
                            .stages_mut()
                            .iter_mut()
                            .find(|s| s.id == stage_id)
                        {
                            if let Some(q) =
                                st.placements.iter_mut().find(|q| q.display == map_display)
                            {
                                *q = p;
                            }
                        }
                    }
                });
            }
        }
    }
    if remove {
        if let Some(st) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            st.placements.retain(|q| q.display != map_display);
        }
    }

    // Mini-Map: ganzes Display + alle Stage-Placements darauf (eigene Stage
    // akzentuiert, andere gedimmt), Überstand über den Displayrand hinaus
    // sichtbar (gestrichelt), Drag verschiebt die eigene Stage.
    let map_size = egui::vec2(ui.available_width(), 190.0);
    let (map_rect, map_response) = ui.allocate_exact_size(map_size, Sense::click_and_drag());
    let painter = ui.painter();
    painter.rect_filled(map_rect, 4.0, palette::PANEL);

    // Transform Display-Pixel → Map: fit + zentriert.
    let scale = (map_rect.width() - 24.0) / disp_px_w.max(1) as f32;
    let scale = scale.min((map_rect.height() - 24.0) / disp_px_h.max(1) as f32);
    let disp_rect = Rect::from_center_size(
        map_rect.center(),
        egui::vec2(disp_px_w as f32 * scale, disp_px_h as f32 * scale),
    );
    let to_map = |x: i32, y: i32| -> Pos2 {
        egui::pos2(
            disp_rect.min.x + x as f32 * scale,
            disp_rect.min.y + y as f32 * scale,
        )
    };
    // Display-Fläche (volle native Größe).
    painter.rect_filled(disp_rect, 2.0, Color32::from_rgb(16, 20, 28));
    painter.rect_stroke(disp_rect, 2.0, Stroke::new(1.5, palette::ACCENT));
    painter.text(
        disp_rect.left_bottom() + egui::vec2(4.0, -4.0),
        Align2::LEFT_BOTTOM,
        format!(
            "{} {}×{} px (Display, voll)",
            display.name, disp_px_w, disp_px_h
        ),
        FontId::proportional(10.0),
        palette::TEXT_DIM,
    );

    // Alle Placements aller Stages auf diesem Display, Z-Order = Layer.
    let mut on_display: Vec<(i32, String, StagePlacement, bool)> = Vec::new();
    for st in app.stages.stages() {
        for p in &st.placements {
            if p.display == map_display {
                on_display.push((st.layer, st.name.clone(), p.clone(), st.id == stage_id));
            }
        }
    }
    // Unten zuerst malen: Layer aufsteigend, gleicher Layer: Name absteigend.
    on_display.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
    let mut drag_delta: Option<(i32, i32)> = None;
    for (layer, name, p, is_own) in &on_display {
        let full = Rect::from_min_size(
            to_map(p.x, p.y),
            egui::vec2(p.width as f32 * scale, p.height as f32 * scale),
        );
        let color = if *is_own {
            palette::ACCENT
        } else {
            palette::TEXT_DIM
        };
        // Überstand außerhalb des Displays: gestrichelt (sichtbar in der Map).
        painter.rect_stroke(full, 2.0, Stroke::new(1.0, color));
        // Sichtbarer Teil (Schnitt mit Display) füllen.
        if let Some((cx, cy, cw, ch)) = p.clip_to_display(disp_px_w, disp_px_h) {
            let vis = Rect::from_min_size(
                to_map(cx, cy),
                egui::vec2(cw as f32 * scale, ch as f32 * scale),
            );
            painter.rect_filled(
                vis,
                2.0,
                theme::with_alpha(color, if *is_own { 70 } else { 30 }),
            );
        }
        painter.text(
            full.left_top() + egui::vec2(3.0, 2.0),
            Align2::LEFT_TOP,
            format!(
                "{name} (L{layer}) {}×{} @ {},{}",
                p.width, p.height, p.x, p.y
            ),
            FontId::proportional(10.0),
            color,
        );
        // Drag auf eigener Stage verschiebt sie (relativ via drag_delta).
        if *is_own && map_response.dragged() {
            let press_origin = ui.ctx().input(|i| i.pointer.press_origin());
            if press_origin.is_some_and(|o| full.contains(o)) {
                let d = map_response.drag_delta();
                drag_delta = Some(((d.x / scale).round() as i32, (d.y / scale).round() as i32));
            }
        }
    }
    if let Some((dx, dy)) = drag_delta {
        if dx != 0 || dy != 0 {
            if let Some(st) = app
                .stages
                .stages_mut()
                .iter_mut()
                .find(|s| s.id == stage_id)
            {
                if let Some(p) = st.placements.iter_mut().find(|q| q.display == map_display) {
                    p.x += dx;
                    p.y += dy;
                }
            }
        }
    }
    map_response.on_hover_text("Map: Display (voll) + Stage-Rechtecke. Ziehen verschiebt die eigene Stage (in Display-Pixeln). Überstand über den Displayrand ist gestrichelt sichtbar und wird abgeschnitten.");

    // Z-Reihenfolge auf diesem Display (oben zuerst).
    if !on_display.is_empty() {
        let mut z: Vec<&(i32, String, StagePlacement, bool)> = on_display.iter().collect();
        z.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Z-Order (oben zuerst):")
                    .small()
                    .color(palette::TEXT_DIM),
            );
            for (i, (_, name, _, is_own)) in z.iter().enumerate() {
                ui.label(
                    egui::RichText::new(format!("{}{}", if i > 0 { " ‹ " } else { "" }, name))
                        .small()
                        .color(if *is_own {
                            palette::ACCENT
                        } else {
                            palette::TEXT
                        }),
                );
            }
        });
    }
}

// ------------------------------------------------------------------
// Key/Fill pro Stage: Fill-Route + Key-Route (Alpha) wählen.
// ------------------------------------------------------------------

/// Section „Key/Fill“: pro Stage aktivierbar; Fill- und Key-Route per Combo.
/// Die Key-Route bekommt das Flag `alpha` (Route rendert mit Alpha).
fn draw_key_fill(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    theme::section_label(ui, "Key/Fill (Alpha-Ausgabe)");
    ui.label(
        egui::RichText::new(
            "Pro Stage separat aktiv. Fill = Farbbild (flach), Key = Alpha-Bild (Route-Flag „Key (Alpha)“).              Alpha kommt aus Videos (z. B. ProRes 4444), Bildern (PNG mit Alpha) und HTML (default transparent).              Geräte: zwei Decklink-Ports (Fill/Key als 2× SDI) oder ein key/fill-fähiger Ausgang.",
        )
        .small()
        .color(palette::TEXT_DIM),
    );
    ui.add_space(theme::size::SPACE_SMALL);

    let routes: Vec<(uuid::Uuid, String)> = app
        .routes
        .routes()
        .iter()
        .map(|r| (r.id, format!("{} ({}×{})", r.name, r.width, r.height)))
        .collect();
    if routes.is_empty() {
        ui.label(
            egui::RichText::new("No routes yet – create one in Settings → Video → Output Routing")
                .color(palette::TEXT_FAINT),
        );
        return;
    }

    let mut enabled = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .and_then(|s| s.key_fill)
        .is_some();
    if ui
        .checkbox(&mut enabled, "Key/Fill output for this stage")
        .changed()
    {
        if let Some(st) = app
            .stages
            .stages_mut()
            .iter_mut()
            .find(|s| s.id == stage_id)
        {
            if enabled {
                // Default: erste zwei Routen (bzw. eine doppelt ist verboten → nimm erste + zweite).
                let fill = routes[0].0;
                let key = routes.get(1).map(|r| r.0).unwrap_or(routes[0].0);
                st.key_fill = if fill == key {
                    None
                } else {
                    Some(crate::core::KeyFillRoutes { fill, key })
                };
                if st.key_fill.is_some() {
                    // Key-Route als Alpha markieren.
                    if let Some(r) = app.routes.find_mut(key) {
                        r.alpha = true;
                    }
                }
            } else {
                // Beim Deaktivieren Alpha-Flag der Key-Route zurücksetzen.
                if let Some(kf) = st.key_fill {
                    if let Some(r) = app.routes.find_mut(kf.key) {
                        r.alpha = false;
                    }
                }
                st.key_fill = None;
            }
        }
    }

    if !enabled {
        return;
    }
    let mut kf = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .and_then(|s| s.key_fill)
        .unwrap_or(crate::core::KeyFillRoutes {
            fill: routes[0].0,
            key: routes[0].0,
        });
    theme::form_grid("stage_key_fill").show(ui, |ui| {
        theme::field_label(ui, "Fill route");
        egui::ComboBox::from_id_salt("kf_fill")
            .selected_text(
                routes
                    .iter()
                    .find(|r| r.0 == kf.fill)
                    .map(|r| r.1.clone())
                    .unwrap_or_default(),
            )
            .show_ui(ui, |ui| {
                for (id, label) in &routes {
                    ui.selectable_value(&mut kf.fill, *id, label);
                }
            });
        ui.end_row();
        theme::field_label(ui, "Key route (alpha)");
        egui::ComboBox::from_id_salt("kf_key")
            .selected_text(
                routes
                    .iter()
                    .find(|r| r.0 == kf.key)
                    .map(|r| r.1.clone())
                    .unwrap_or_default(),
            )
            .show_ui(ui, |ui| {
                for (id, label) in &routes {
                    ui.selectable_value(&mut kf.key, *id, label);
                }
            });
        ui.end_row();
    });
    if kf.fill == kf.key {
        ui.label(
            egui::RichText::new("Fill and key route must differ (two outputs)")
                .color(palette::ERROR),
        );
    }
    // Änderungen zurückschreiben + Key-Route alpha setzen.
    if let Some(st) = app
        .stages
        .stages_mut()
        .iter_mut()
        .find(|s| s.id == stage_id)
    {
        if st.key_fill != Some(kf) && kf.fill != kf.key {
            st.key_fill = Some(kf);
        }
    }
    if let Some(r) = app.routes.find_mut(kf.key) {
        r.alpha = true;
    }
    if let Some(kf_old) = app
        .stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .and_then(|s| s.key_fill)
    {
        if kf_old.key != kf.key {
            if let Some(r) = app.routes.find_mut(kf_old.key) {
                r.alpha = false;
            }
        }
    }
}
