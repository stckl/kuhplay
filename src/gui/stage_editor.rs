//! Stage-Editor („Video → Video Outputs → Edit Stage…“).
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

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};
use uuid::Uuid;

use crate::core::{
    region_letter, regions_in_reading_order, DeviceArea, DeviceRef, EdgeBlend, OutputRoute, Region,
    RouteScaling, ScreenRotation,
};

use super::app::ShowApp;
use super::theme::{self, palette};
use crate::engine::route_renderer::auto_blends;

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
        )
        .show_mode_badge(app.show_mode),
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
                        draw_stage_head(left, app, stage_id);
                        left.add_space(theme::size::SPACE_MEDIUM);
                        draw_region_list(left, app, stage_id);
                        draw_region_properties(right, app, stage_id);
                    });
                    ui.separator();
                    ui.scope(|ui| draw_display_view(ui, app, stage_id));
                });
        },
    );
    if !open {
        app.stage_editor_open = None;
    }
}

/// Stage: Name und Auflösung – mehr hat das Canvas nicht.
fn draw_stage_head(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = stage_of(app, stage_id) else {
        return;
    };
    let mut rename = None;
    let (mut width, mut height) = (stage.geometry.width as u32, stage.geometry.height as u32);
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

        theme::field_label(ui, "Resolution");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut width)
                    .range(16..=16384)
                    .prefix("W "),
            );
            ui.add(
                egui::DragValue::new(&mut height)
                    .range(16..=16384)
                    .prefix("H "),
            );
        });
        ui.end_row();
    });
    if (width, height) != (stage.geometry.width as u32, stage.geometry.height as u32) {
        let mut updated = stage.clone();
        let old = (stage.geometry.width, stage.geometry.height);
        let new = (width as f32, height as f32);
        updated.geometry.width = new.0;
        updated.geometry.height = new.1;
        // Der Crop je Seite bleibt, die Regionen wachsen mit der Stage mit.
        for region in &mut updated.regions {
            let crop = region_crop(region, old);
            apply_crop(region, crop, new);
        }
        app.configure_stage(updated);
    }
    if let Some(new_name) = rename {
        app.rename_stage(&stage.name, &new_name);
    }
}

/// Liste der Regionen (linke Spalte).
fn draw_region_list(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = stage_of(app, stage_id) else {
        return;
    };
    let letters = region_letters(&stage);
    let selected = selected_region_id(ui, &stage);
    let mut remove = None;
    let mut new_color: Option<(Uuid, [u8; 3])> = None;
    let mut updated_regions = stage.regions.clone();

    theme::section_label(ui, "Regions");
    for (index, region) in stage.regions.iter().enumerate() {
        let route = region.route.and_then(|id| app.routes.find(id));
        egui::Frame::none()
            .fill(palette::RAISED)
            .rounding(egui::Rounding::same(5.0))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let letter = letters.get(&region.id).cloned().unwrap_or_default();
                    if ui.selectable_label(selected == region.id, letter).clicked() {
                        set_selected_region(ui, &stage, region.id);
                    }
                    if let Some(route) = route {
                        let mut color =
                            Color32::from_rgb(route.color[0], route.color[1], route.color[2]);
                        ui.spacing_mut().interact_size = egui::vec2(16.0, 16.0);
                        let changed = egui::color_picker::color_edit_button_srgba(
                            ui,
                            &mut color,
                            egui::color_picker::Alpha::Opaque,
                        )
                        .on_hover_text("Change the color of this region")
                        .changed();
                        if changed {
                            new_color = Some((route.id, [color.r(), color.g(), color.b()]));
                        }
                        ui.label(
                            route
                                .device
                                .as_ref()
                                .map_or("No display", |device| device.label()),
                        );
                        if region.key {
                            ui.label(egui::RichText::new("KEY").small().strong());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Delete").clicked() {
                            remove = Some(region.id);
                        }
                        ui.checkbox(&mut updated_regions[index].grid, "Grid");
                    });
                });
            });
        ui.add_space(2.0);
    }
    if ui.button("+ Add Region").clicked() {
        add_region(ui, app, stage_id);
        return;
    }
    if let Some((id, color)) = new_color {
        app.set_route_color(id, color);
    }
    if let Some(id) = remove {
        remove_region(app, stage_id, id);
    } else if updated_regions != stage.regions {
        let mut updated = stage;
        updated.regions = updated_regions;
        app.configure_stage(updated);
    }
}

/// Neue Region: ganze Stage auf dem ganzen ersten Display.
fn add_region(ui: &egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(mut stage) = stage_of(app, stage_id) else {
        return;
    };
    let display = app
        .displays
        .iter()
        .map(crate::core::DisplayRefInfo::from)
        .next();
    let route_id = match display {
        Some(display) => app.add_route_for_display(&display),
        None => app.add_route(),
    };
    let mut region = Region::full_stage(
        stage.geometry.width.max(1.0),
        stage.geometry.height.max(1.0),
    );
    region.route = Some(route_id);
    let region_id = region.id;
    stage.regions.push(region);
    app.configure_stage(stage.clone());
    set_selected_region(ui, &stage, region_id);
}

/// Key-Gegenstück: gleicher Crop wie die Region, möglichst auf einem anderen
/// Display.
fn add_key_region(ui: &egui::Ui, app: &mut ShowApp, stage_id: Uuid, fill_id: Uuid) {
    let Some(mut stage) = stage_of(app, stage_id) else {
        return;
    };
    let Some(fill) = stage.regions.iter().find(|r| r.id == fill_id).cloned() else {
        return;
    };
    let fill_device = fill
        .route
        .and_then(|id| app.routes.find(id))
        .and_then(|route| route.device.clone());
    let displays = display_infos(app);
    let display = displays
        .iter()
        .find(|d| !matches!(&fill_device, Some(DeviceRef::Display { id, .. }) if *id == d.id))
        .or_else(|| displays.first());
    let route_id = match display {
        Some(display) => app.add_route_for_display(display),
        None => app.add_route(),
    };
    if let Some(route) = app.routes.find_mut(route_id) {
        route.alpha = true;
    }
    app.configure_routes();
    let mut key = fill;
    key.id = Uuid::new_v4();
    key.route = Some(route_id);
    key.key = true;
    key.grid = false;
    let key_id = key.id;
    stage.regions.push(key);
    app.configure_stage(stage.clone());
    set_selected_region(ui, &stage, key_id);
}

/// Löscht eine Region samt ihrem Ausgang, wenn keine andere Region ihn nutzt.
fn remove_region(app: &mut ShowApp, stage_id: Uuid, region_id: Uuid) {
    let Some(mut stage) = stage_of(app, stage_id) else {
        return;
    };
    let route = stage
        .regions
        .iter()
        .find(|r| r.id == region_id)
        .and_then(|r| r.route);
    stage.regions.retain(|r| r.id != region_id);
    app.configure_stage(stage);
    if let Some(route) = route {
        let used = app
            .stages
            .stages()
            .iter()
            .flat_map(|s| &s.regions)
            .any(|r| r.route == Some(route));
        if !used {
            app.remove_route(route);
        }
    }
}

/// Eigenschaften der gewählten Region (rechte Spalte), alles in echten Pixeln.
fn draw_region_properties(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = stage_of(app, stage_id) else {
        return;
    };
    let selected = selected_region_id(ui, &stage);
    theme::section_label(ui, "Selected Region");
    let Some(region) = stage.regions.iter().find(|r| r.id == selected).cloned() else {
        ui.label(egui::RichText::new("No region yet").color(palette::TEXT_FAINT));
        return;
    };
    let Some(route) = region.route.and_then(|id| app.routes.find(id)).cloned() else {
        return;
    };
    let displays = display_infos(app);
    let stage_size = (stage.geometry.width, stage.geometry.height);
    let mut r = route.clone();
    let mut reg = region.clone();
    let device = device_size(&route, &displays);
    let (mut x, mut y, mut w, mut h) = route.device_area.rect(device.0, device.1);
    let on_display = matches!(route.device, Some(DeviceRef::Display { .. }));
    let mut crop = region_crop(&region, stage_size);
    let old_crop = crop;
    let mut rotation = route.rotation;
    let (cards, hint) = {
        let (_available, cards, hint) = crate::engine::decklink_native::devices_with_hint();
        (cards, hint)
    };
    let mut pick: Option<DeviceRef> = None;

    theme::form_grid("region_properties").show(ui, |ui| {
        theme::field_label(ui, "Output");
        egui::ComboBox::from_id_salt(("region_output", region.id))
            .selected_text(route.device.as_ref().map_or("Choose…", |d| d.label()))
            .show_ui(ui, |ui| {
                for d in &displays {
                    let (pw, ph) = d.pixel_size();
                    if ui
                        .selectable_label(false, format!("{} ({pw}×{ph} px)", d.name))
                        .clicked()
                    {
                        pick = Some(DeviceRef::Display {
                            id: d.id,
                            name: d.name.clone(),
                            width: d.width,
                            height: d.height,
                        });
                    }
                }
                for card in &cards {
                    if ui.selectable_label(false, card).clicked() {
                        pick = Some(DeviceRef::Blackmagic { name: card.clone() });
                    }
                }
            });
        ui.end_row();
        theme::field_label(ui, "Signal");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut reg.key, false, "Fill");
            ui.selectable_value(&mut reg.key, true, "Key")
                .on_hover_text("Outputs the stage's alpha as a grayscale key signal");
        });
        ui.end_row();
        if on_display {
            theme::field_label(ui, "Position");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut x)
                        .range(0..=device.0)
                        .prefix("X "),
                );
                ui.add(
                    egui::DragValue::new(&mut y)
                        .range(0..=device.1)
                        .prefix("Y "),
                );
            });
            ui.end_row();
        }
        theme::field_label(ui, if on_display { "Size" } else { "Format" });
        ui.horizontal(|ui| {
            // Darf größer als das Display sein, der Rest wird dort abgeschnitten.
            ui.add(
                egui::DragValue::new(&mut w)
                    .range(MIN_AREA_SIZE..=30000)
                    .prefix("W "),
            );
            ui.add(
                egui::DragValue::new(&mut h)
                    .range(MIN_AREA_SIZE..=30000)
                    .prefix("H "),
            );
            if on_display
                && ui
                    .button("1:1")
                    .on_hover_text("Original pixel size at the top left of the display")
                    .clicked()
            {
                (x, y, w, h) = one_to_one(&reg, r.rotation);
            }
        });
        ui.end_row();

        theme::field_label(ui, "Rotation");
        ui.horizontal(|ui| {
            for (value, label) in [
                (ScreenRotation::Deg0, "0°"),
                (ScreenRotation::Deg90, "90°"),
                (ScreenRotation::Deg180, "180°"),
                (ScreenRotation::Deg270, "270°"),
            ] {
                ui.selectable_value(&mut rotation, value, label);
            }
        });
        ui.end_row();

        theme::field_label(ui, "Mirror");
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.rear_projection, "Horizontal");
            ui.checkbox(&mut r.mirror_vertical, "Vertical");
        });
        ui.end_row();

        theme::field_label(ui, "Crop");
        ui.horizontal(|ui| {
            let (sw, sh) = (stage_size.0 as u32, stage_size.1 as u32);
            for (value, prefix, limit) in crop
                .iter_mut()
                .zip(["L ", "T ", "R ", "B "])
                .zip([sw, sh, sw, sh])
                .map(|((v, p), l)| (v, p, l))
            {
                ui.add(egui::DragValue::new(value).range(0..=limit).prefix(prefix));
            }
        });
        ui.end_row();

        theme::field_label(ui, "Edge Blends");
        ui.horizontal(|ui| {
            ui.checkbox(&mut reg.auto_blend, "Auto");
            let shown = if reg.auto_blend {
                effective_blend(&stage, region.id)
            } else {
                reg.blend
            };
            let mut values = shown;
            ui.add_enabled_ui(!reg.auto_blend, |ui| {
                let row = [
                    (&mut values.left, "L "),
                    (&mut values.top, "T "),
                    (&mut values.right, "R "),
                    (&mut values.bottom, "B "),
                ];
                for (value, prefix) in row {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(0.0..=16384.0)
                            .prefix(prefix),
                    );
                }
            });
            if !reg.auto_blend {
                reg.blend = values;
            }
        });
        ui.end_row();

        theme::field_label(ui, "Test Pattern");
        ui.checkbox(&mut r.test_pattern, "");
        ui.end_row();
    });
    if let Some(hint) = &hint {
        ui.label(egui::RichText::new(hint).small().color(palette::TEXT_DIM));
    }

    if !region.key && ui.button("+ Add Key Region").clicked() {
        add_key_region(ui, app, stage_id, region.id);
        return;
    }
    r.alpha = reg.key;

    // Beim Crop bleibt der Maßstab gleich: 1:1 bleibt 1:1.
    if crop != old_crop {
        let before = (reg.width, reg.height);
        apply_crop(&mut reg, crop, stage_size);
        (w, h) = scaled_for_crop((w, h), before, (reg.width, reg.height), rotation);
    }
    if rotation != route.rotation {
        if quarter_turned(rotation) != quarter_turned(route.rotation) {
            (w, h) = (h, w);
        }
        r.rotation = rotation;
    }
    if let Some(device_ref) = pick {
        r.device = Some(device_ref);
        let size = match &r.device {
            Some(DeviceRef::Display { .. }) => device_size(&r, &displays),
            _ => (1920, 1080),
        };
        (x, y, w, h) = (0, 0, size.0, size.1);
    }
    // Die Region beginnt immer auf dem Display.
    if matches!(r.device, Some(DeviceRef::Display { .. })) {
        let size = device_size(&r, &displays);
        x = x.min(size.0.saturating_sub(w.min(size.0)));
        y = y.min(size.1.saturating_sub(h.min(size.1)));
    }
    let size = device_size(&r, &displays);
    set_placement(&mut r, (x, y, w, h), size);
    if r != route {
        if let Some(target) = app.routes.find_mut(route.id) {
            *target = r;
        }
        app.configure_routes();
    }
    if reg != region {
        let mut updated = stage;
        if let Some(target) = updated.regions.iter_mut().find(|t| t.id == reg.id) {
            *target = reg;
        }
        app.configure_stage(updated);
    }
}

/// Die eine Ansicht: alle Ausgänge nebeneinander, darauf jede Region an ihrer
/// echten Stelle mit Größe, Crop, Drehung und Edge Blends.
fn draw_display_view(ui: &mut egui::Ui, app: &mut ShowApp, stage_id: Uuid) {
    let Some(stage) = stage_of(app, stage_id) else {
        return;
    };
    theme::section_label(ui, "Placement");
    let displays = display_infos(app);
    let mut devices: Vec<(String, (u32, u32), DeviceRef)> = displays
        .iter()
        .map(|d| {
            (
                d.name.clone(),
                d.pixel_size(),
                DeviceRef::Display {
                    id: d.id,
                    name: d.name.clone(),
                    width: d.width,
                    height: d.height,
                },
            )
        })
        .collect();
    for route in app.routes.routes() {
        if let Some(device @ DeviceRef::Blackmagic { name }) = &route.device {
            if !devices.iter().any(|(_, _, d)| d == device) {
                devices.push((
                    name.clone(),
                    (route.width.max(1), route.height.max(1)),
                    device.clone(),
                ));
            }
        }
    }
    if devices.is_empty() {
        ui.label(
            egui::RichText::new("No displays detected in this session").color(palette::TEXT_FAINT),
        );
        return;
    }

    const GAP: f32 = 16.0;
    let (map, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(200.0), 280.0),
        Sense::click_and_drag(),
    );
    let total_w: f32 = devices.iter().map(|(_, s, _)| s.0 as f32).sum();
    let max_h = devices.iter().map(|(_, s, _)| s.1).max().unwrap_or(1) as f32;
    let free_w = map.width() - GAP * (devices.len() as f32 - 1.0) - 8.0;
    let scale = (free_w / total_w.max(1.0))
        .min((map.height() - 28.0) / max_h.max(1.0))
        .max(0.0001);

    struct Placed {
        region: Uuid,
        route: Uuid,
        rect: PixelRect,
        device: (u32, u32),
        origin: Pos2,
    }
    let selected = selected_region_id(ui, &stage);
    let letters = region_letters(&stage);
    let painter = ui.painter_at(map);
    let mut placed: Vec<Placed> = Vec::new();
    let mut left = map.left() + 4.0;
    for (name, size, device) in &devices {
        let device_rect = Rect::from_min_size(
            Pos2::new(left, map.top() + 20.0),
            egui::vec2(size.0 as f32 * scale, size.1 as f32 * scale),
        );
        left += device_rect.width() + GAP;
        painter.rect_filled(device_rect, 2.0, palette::LIST_BG);
        painter.rect_stroke(device_rect, 2.0, Stroke::new(1.0_f32, palette::BORDER));
        painter.text(
            device_rect.left_top() - egui::vec2(0.0, 3.0),
            Align2::LEFT_BOTTOM,
            format!("{name} ({}×{})", size.0, size.1),
            FontId::proportional(11.0),
            palette::TEXT_DIM,
        );
        // Regionen anderer Stages nur als Umriss.
        for other in app.stages.stages().iter().filter(|s| s.id != stage_id) {
            for region in &other.regions {
                let Some(route) = region.route.and_then(|id| app.routes.find(id)) else {
                    continue;
                };
                if route.device.as_ref() == Some(device) {
                    let rect = route.device_area.rect(size.0, size.1);
                    let area = screen_rect(device_rect.min, rect, scale);
                    painter.rect_stroke(area, 1.0, Stroke::new(1.0_f32, palette::TEXT_FAINT));
                }
            }
        }
        for region in &stage.regions {
            let Some(route) = region.route.and_then(|id| app.routes.find(id)) else {
                continue;
            };
            if route.device.as_ref() != Some(device) {
                continue;
            }
            let rect = route.device_area.rect(size.0, size.1);
            placed.push(Placed {
                region: region.id,
                route: route.id,
                rect,
                device: *size,
                origin: device_rect.min,
            });
        }
    }

    // Auswahl und Ziehen: von der Ausgangslage aus rechnen, damit nichts wegdriftet.
    let drag_id = egui::Id::new(("region_placement_drag", stage_id));
    let (origin, now) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()));
    let hit = |pos: Pos2| {
        placed.iter().rev().find(|p| {
            let area = screen_rect(p.origin, p.rect, scale);
            area.contains(pos) || handle_rect(area).contains(pos)
        })
    };
    let handle_of_selected = placed
        .iter()
        .find(|p| p.region == selected)
        .map(|p| handle_rect(screen_rect(p.origin, p.rect, scale)));
    if response.drag_started() {
        if let Some(pos) = origin {
            let on_handle = handle_of_selected.is_some_and(|h| h.contains(pos));
            let target = if on_handle {
                placed.iter().find(|p| p.region == selected)
            } else {
                hit(pos)
            };
            if let Some(p) = target {
                set_selected_region(ui, &stage, p.region);
                ui.ctx()
                    .data_mut(|d| d.insert_temp(drag_id, (p.region, p.rect, on_handle)));
            } else {
                ui.ctx()
                    .data_mut(|d| d.remove::<(Uuid, PixelRect, bool)>(drag_id));
            }
        }
    }
    if response.clicked() {
        if let Some(p) = origin.or(now).and_then(hit) {
            set_selected_region(ui, &stage, p.region);
        }
    }
    let mut change: Option<(Uuid, PixelRect, (u32, u32))> = None;
    if response.dragged() {
        let start = ui
            .ctx()
            .data_mut(|d| d.get_temp::<(Uuid, PixelRect, bool)>(drag_id));
        if let (Some((region, start, resize)), Some(origin), Some(now)) = (start, origin, now) {
            if let Some(p) = placed.iter().find(|p| p.region == region) {
                let delta = (now - origin) / scale;
                let rect = if resize {
                    resized_area(start, (delta.x, delta.y), p.device)
                } else {
                    moved_area(start, (delta.x, delta.y), p.device)
                };
                change = Some((p.route, rect, p.device));
            }
        }
    }
    if response.double_clicked() {
        if let Some(p) = origin.or(now).and_then(hit) {
            let rect = stage
                .regions
                .iter()
                .find(|r| r.id == p.region)
                .zip(app.routes.find(p.route))
                .map(|(region, route)| one_to_one(region, route.rotation));
            if let Some(rect) = rect {
                change = Some((p.route, rect, p.device));
            }
        }
    }

    // Regionen malen (die gewählte zuletzt, sie liegt oben).
    let selected_now = selected_region_id(ui, &stage);
    let mut order: Vec<&Placed> = placed.iter().collect();
    order.sort_by_key(|p| p.region == selected_now);
    for p in order {
        let Some(region) = stage.regions.iter().find(|r| r.id == p.region) else {
            continue;
        };
        let Some(route) = app.routes.find(p.route) else {
            continue;
        };
        let rect = match change {
            Some((id, rect, _)) if id == p.route => rect,
            _ => p.rect,
        };
        let area = screen_rect(p.origin, rect, scale);
        let is_selected = p.region == selected_now;
        let color = Color32::from_rgb(route.color[0], route.color[1], route.color[2]);
        painter.rect_filled(
            area,
            1.0,
            with_alpha(color, if is_selected { 80 } else { 40 }),
        );

        // Quell-Seiten auf die Display-Seiten abbilden (Drehung, Spiegelung).
        let turned = quarter_turned(route.rotation);
        let (src_w, src_h) = if turned {
            (region.height, region.width)
        } else {
            (region.width, region.height)
        };
        let to_x = area.width() / src_w.max(1.0);
        let to_y = area.height() / src_h.max(1.0);
        let sides = |values: [f32; 4]| {
            let [l, t, r, b] = displayed_sides(route, values);
            [l * to_x, t * to_y, r * to_x, b * to_y]
        };

        // Crop: Umriss der ganzen Stage in diesem Maßstab.
        let crop = region_crop(region, (stage.geometry.width, stage.geometry.height));
        let [cl, ct, cr, cb] = sides(crop.map(|v| v as f32));
        if cl + ct + cr + cb > 0.0 {
            let ghost =
                Rect::from_min_max(area.min - egui::vec2(cl, ct), area.max + egui::vec2(cr, cb));
            painter.rect_stroke(ghost, 0.0, Stroke::new(1.0_f32, with_alpha(color, 110)));
        }

        // Edge Blends: schraffierte Zone an der Kante, an der sie wirklich liegt.
        let blend = effective_blend(&stage, region.id);
        let [bl, bt, br, bb] = sides([blend.left, blend.top, blend.right, blend.bottom]);
        let zones = [
            Rect::from_min_size(area.min, egui::vec2(bl.min(area.width()), area.height())),
            Rect::from_min_size(area.min, egui::vec2(area.width(), bt.min(area.height()))),
            Rect::from_min_max(
                Pos2::new(area.right() - br.min(area.width()), area.top()),
                area.max,
            ),
            Rect::from_min_max(
                Pos2::new(area.left(), area.bottom() - bb.min(area.height())),
                area.max,
            ),
        ];
        for zone in zones {
            if zone.width() > 0.5 && zone.height() > 0.5 {
                painter.rect_filled(zone, 0.0, with_alpha(Color32::WHITE, 40));
                hatch(
                    &painter.with_clip_rect(zone.intersect(map)),
                    zone,
                    with_alpha(Color32::WHITE, 150),
                );
            }
        }

        // Rand; die dicke Linie markiert die Oberkante der Stage (Drehung).
        painter.rect_stroke(
            area,
            1.0,
            Stroke::new(if is_selected { 2.0_f32 } else { 1.0_f32 }, color),
        );
        let [ml, mt, mr, _] = displayed_sides(route, [0.0, 1.0, 0.0, 0.0]);
        let top_edge = if ml > 0.0 {
            [area.left_top(), area.left_bottom()]
        } else if mt > 0.0 {
            [area.left_top(), area.right_top()]
        } else if mr > 0.0 {
            [area.right_top(), area.right_bottom()]
        } else {
            [area.left_bottom(), area.right_bottom()]
        };
        painter.line_segment(top_edge, Stroke::new(4.0_f32, color));

        let rotation_label = match route.rotation {
            ScreenRotation::Deg0 => "",
            ScreenRotation::Deg90 => "  90°",
            ScreenRotation::Deg180 => "  180°",
            ScreenRotation::Deg270 => "  270°",
        };
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            format!(
                "{}{}  {}×{}{rotation_label}",
                letters.get(&region.id).cloned().unwrap_or_default(),
                if region.key { " KEY" } else { "" },
                rect.2,
                rect.3
            ),
            FontId::proportional(12.0),
            palette::TEXT,
        );
        if is_selected {
            painter.rect_filled(handle_rect(area).shrink(3.0), 1.0, color);
        }
    }
    response.on_hover_text("Drag to move, drag the corner to resize, double-click for 1:1");

    if let Some((route_id, rect, device)) = change {
        if let Some(route) = app.routes.find_mut(route_id) {
            set_placement(route, rect, device);
        }
        app.configure_routes();
    }
}

fn stage_of(app: &ShowApp, stage_id: Uuid) -> Option<crate::core::Stage> {
    app.stages
        .stages()
        .iter()
        .find(|s| s.id == stage_id)
        .cloned()
}

fn display_infos(app: &ShowApp) -> Vec<crate::core::DisplayRefInfo> {
    app.displays
        .iter()
        .map(crate::core::DisplayRefInfo::from)
        .collect()
}

fn region_letters(stage: &crate::core::Stage) -> std::collections::HashMap<Uuid, String> {
    regions_in_reading_order(stage.regions.iter())
        .iter()
        .enumerate()
        .map(|(i, region)| (region.id, region_letter(i)))
        .collect()
}

/// Größe des Ausgangs in Pixeln (Karte oder fehlendes Display: Format der Route).
fn device_size(route: &OutputRoute, displays: &[crate::core::DisplayRefInfo]) -> (u32, u32) {
    if let Some(DeviceRef::Display { id, .. }) = &route.device {
        if let Some(display) = displays.iter().find(|d| d.id == *id) {
            return display.pixel_size();
        }
    }
    (route.width.max(1), route.height.max(1))
}

/// Wirksame Edge Blends einer Region (Auto oder eigene Werte).
fn effective_blend(stage: &crate::core::Stage, region: Uuid) -> EdgeBlend {
    let Some(index) = stage.regions.iter().position(|r| r.id == region) else {
        return EdgeBlend::ZERO;
    };
    if stage.regions[index].auto_blend {
        auto_blends(&stage.regions)[index]
    } else {
        stage.regions[index].blend
    }
}

fn screen_rect(origin: Pos2, rect: PixelRect, scale: f32) -> Rect {
    Rect::from_min_size(
        origin + egui::vec2(rect.0 as f32 * scale, rect.1 as f32 * scale),
        egui::vec2(rect.2 as f32 * scale, rect.3 as f32 * scale),
    )
}

fn handle_rect(area: Rect) -> Rect {
    Rect::from_center_size(area.max, egui::vec2(14.0, 14.0))
}

fn quarter_turned(rotation: ScreenRotation) -> bool {
    matches!(rotation, ScreenRotation::Deg90 | ScreenRotation::Deg270)
}

/// Crop je Seite `[links, oben, rechts, unten]` in Stage-Pixeln: Eine Region ist
/// die Stage abzüglich ihres Crops.
fn region_crop(region: &Region, stage: (f32, f32)) -> [u32; 4] {
    [
        region.x.max(0.0).round() as u32,
        region.y.max(0.0).round() as u32,
        (stage.0 - region.x - region.width).max(0.0).round() as u32,
        (stage.1 - region.y - region.height).max(0.0).round() as u32,
    ]
}

/// Setzt den Crop; gegenüberliegende Seiten lassen mindestens 1 Pixel übrig.
fn apply_crop(region: &mut Region, crop: [u32; 4], stage: (f32, f32)) {
    let [l, t, r, b] = crop.map(|v| v as f32);
    region.x = l.min(stage.0 - 1.0).max(0.0);
    region.y = t.min(stage.1 - 1.0).max(0.0);
    region.width = (stage.0 - region.x - r).max(1.0);
    region.height = (stage.1 - region.y - b).max(1.0);
}

/// Größe auf dem Display nach einem Crop: Der Maßstab bleibt gleich.
fn scaled_for_crop(
    size: (u32, u32),
    before: (f32, f32),
    after: (f32, f32),
    rotation: ScreenRotation,
) -> (u32, u32) {
    let (ratio_w, ratio_h) = (after.0 / before.0.max(1.0), after.1 / before.1.max(1.0));
    let (ratio_w, ratio_h) = if quarter_turned(rotation) {
        (ratio_h, ratio_w)
    } else {
        (ratio_w, ratio_h)
    };
    (
        ((size.0 as f32 * ratio_w).round() as u32).max(MIN_AREA_SIZE),
        ((size.1 as f32 * ratio_h).round() as u32).max(MIN_AREA_SIZE),
    )
}

/// Bildet Stage-Seiten `[links, oben, rechts, unten]` auf die Display-Seiten ab:
/// erst Drehung im Uhrzeigersinn, dann Spiegelung (Reihenfolge wie im Renderer).
fn displayed_sides(route: &OutputRoute, [l, t, r, b]: [f32; 4]) -> [f32; 4] {
    let [mut dl, mut dt, mut dr, mut db] = match route.rotation {
        ScreenRotation::Deg0 => [l, t, r, b],
        ScreenRotation::Deg90 => [b, l, t, r],
        ScreenRotation::Deg180 => [r, b, l, t],
        ScreenRotation::Deg270 => [t, r, b, l],
    };
    if route.rear_projection {
        std::mem::swap(&mut dl, &mut dr);
    }
    if route.mirror_vertical {
        std::mem::swap(&mut dt, &mut db);
    }
    [dl, dt, dr, db]
}

/// Original-Pixelgröße der Region oben links auf dem Display. Passt die Stage
/// nicht zum Display, ragt sie hinaus oder lässt Platz.
fn one_to_one(region: &Region, rotation: ScreenRotation) -> PixelRect {
    let (w, h) = (
        (region.width.round() as u32).max(MIN_AREA_SIZE),
        (region.height.round() as u32).max(MIN_AREA_SIZE),
    );
    if quarter_turned(rotation) {
        (0, 0, h, w)
    } else {
        (0, 0, w, h)
    }
}

/// Setzt Lage und Größe auf dem Gerät; das Raster entspricht 1:1 diesen Pixeln.
fn set_placement(route: &mut OutputRoute, rect: PixelRect, device: (u32, u32)) {
    route.device_area = area_from_rect(rect, device);
    let (w, h) = (rect.2.max(1), rect.3.max(1));
    (route.width, route.height) = if quarter_turned(route.rotation) {
        (h, w)
    } else {
        (w, h)
    };
    route.scaling = RouteScaling::Stretch;
}

fn selected_region_id(ui: &egui::Ui, stage: &crate::core::Stage) -> Uuid {
    let id = egui::Id::new(("stage_editor_selected_region", stage.id));
    let stored = ui.ctx().data_mut(|d| d.get_temp::<Uuid>(id));
    stored
        .filter(|rid| stage.regions.iter().any(|r| r.id == *rid))
        .or_else(|| stage.regions.first().map(|r| r.id))
        .unwrap_or_else(Uuid::nil)
}

fn set_selected_region(ui: &egui::Ui, stage: &crate::core::Stage, region: Uuid) {
    let id = egui::Id::new(("stage_editor_selected_region", stage.id));
    ui.ctx().data_mut(|d| d.insert_temp(id, region));
}

/// Schraffur (Blend-Bereich).
fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let step = 6.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [
                Pos2::new(x, rect.bottom()),
                Pos2::new(x + rect.height(), rect.top()),
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

/// Rechteck `(x, y, Breite, Höhe)` in Gerätepixeln.
type PixelRect = (u32, u32, u32, u32);

/// Kleinste Größe beim Skalieren in Gerätepixeln.
const MIN_AREA_SIZE: u32 = 16;

/// Bereich aus Rechteck: deckt er das ganze Gerät ab, ist er wieder `Full`.
fn area_from_rect(rect: PixelRect, device: (u32, u32)) -> DeviceArea {
    let (x, y, width, height) = rect;
    if (x, y, width, height) == (0, 0, device.0, device.1) {
        DeviceArea::Full
    } else {
        DeviceArea::Part {
            x,
            y,
            width,
            height,
        }
    }
}

/// Verschiebt `rect` um `delta` Gerätepixel und bleibt auf dem Gerät.
fn moved_area(rect: PixelRect, delta: (f32, f32), device: (u32, u32)) -> PixelRect {
    let (x, y, w, h) = rect;
    let limit_x = device.0.saturating_sub(w) as f32;
    let limit_y = device.1.saturating_sub(h) as f32;
    (
        (x as f32 + delta.0).round().clamp(0.0, limit_x) as u32,
        (y as f32 + delta.1).round().clamp(0.0, limit_y) as u32,
        w,
        h,
    )
}

/// Skaliert `rect` an der Ecke unten rechts und bleibt auf dem Gerät.
fn resized_area(rect: PixelRect, delta: (f32, f32), device: (u32, u32)) -> PixelRect {
    let (x, y, w, h) = rect;
    let max_w = device.0.saturating_sub(x).max(MIN_AREA_SIZE);
    let max_h = device.1.saturating_sub(y).max(MIN_AREA_SIZE);
    (
        x,
        y,
        ((w as f32 + delta.0).round().max(0.0) as u32).clamp(MIN_AREA_SIZE, max_w),
        ((h as f32 + delta.1).round().max(0.0) as u32).clamp(MIN_AREA_SIZE, max_h),
    )
}

#[cfg(test)]
mod display_area_tests {
    use super::*;

    const DEVICE: (u32, u32) = (1920, 1080);

    #[test]
    fn whole_device_is_full_and_anything_else_a_part() {
        assert_eq!(area_from_rect((0, 0, 1920, 1080), DEVICE), DeviceArea::Full);
        assert!(matches!(
            area_from_rect((10, 0, 1000, 1080), DEVICE),
            DeviceArea::Part { x: 10, .. }
        ));
    }

    #[test]
    fn moving_stays_on_the_device() {
        let rect = (100, 100, 800, 600);
        assert_eq!(moved_area(rect, (50.0, -20.0), DEVICE), (150, 80, 800, 600));
        assert_eq!(moved_area(rect, (-500.0, -500.0), DEVICE), (0, 0, 800, 600));
        assert_eq!(
            moved_area(rect, (5000.0, 5000.0), DEVICE),
            (1120, 480, 800, 600)
        );
        // Der volle Bereich hat keinen Spielraum.
        assert_eq!(
            moved_area((0, 0, 1920, 1080), (50.0, 50.0), DEVICE),
            (0, 0, 1920, 1080)
        );
    }

    #[test]
    fn resizing_keeps_position_and_limits() {
        let rect = (100, 100, 800, 600);
        assert_eq!(
            resized_area(rect, (100.0, 50.0), DEVICE),
            (100, 100, 900, 650)
        );
        assert_eq!(
            resized_area(rect, (5000.0, 5000.0), DEVICE),
            (100, 100, 1820, 980)
        );
        assert_eq!(
            resized_area(rect, (-5000.0, -5000.0), DEVICE),
            (100, 100, MIN_AREA_SIZE, MIN_AREA_SIZE)
        );
    }

    fn route_turned(rotation: ScreenRotation) -> OutputRoute {
        let mut route = OutputRoute::new("R", 1920, 1080);
        route.rotation = rotation;
        route
    }

    #[test]
    fn crop_is_the_stage_minus_the_region() {
        let mut region = Region::full_stage(1920.0, 1080.0);
        assert_eq!(region_crop(&region, (1920.0, 1080.0)), [0, 0, 0, 0]);
        apply_crop(&mut region, [100, 20, 860, 60], (1920.0, 1080.0));
        assert_eq!(
            (region.x, region.y, region.width, region.height),
            (100.0, 20.0, 960.0, 1000.0)
        );
        assert_eq!(region_crop(&region, (1920.0, 1080.0)), [100, 20, 860, 60]);
    }

    #[test]
    fn crop_keeps_at_least_one_pixel() {
        let mut region = Region::full_stage(100.0, 100.0);
        apply_crop(&mut region, [500, 0, 500, 0], (100.0, 100.0));
        assert!(region.width >= 1.0 && region.x + region.width <= 100.0);
    }

    #[test]
    fn cropping_keeps_the_pixel_scale() {
        let half = (960.0, 1080.0);
        assert_eq!(
            scaled_for_crop((1920, 1080), (1920.0, 1080.0), half, ScreenRotation::Deg0),
            (960, 1080)
        );
        // Gedreht liegt die Stage-Breite auf der Display-Höhe.
        assert_eq!(
            scaled_for_crop((1080, 1920), (1920.0, 1080.0), half, ScreenRotation::Deg90),
            (1080, 960)
        );
    }

    #[test]
    fn sides_follow_rotation_and_mirroring() {
        let top = [0.0, 1.0, 0.0, 0.0];
        let side = |rotation| displayed_sides(&route_turned(rotation), top);
        assert_eq!(side(ScreenRotation::Deg0), [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(side(ScreenRotation::Deg90), [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(side(ScreenRotation::Deg180), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(side(ScreenRotation::Deg270), [1.0, 0.0, 0.0, 0.0]);

        let mut route = route_turned(ScreenRotation::Deg90);
        route.rear_projection = true;
        assert_eq!(displayed_sides(&route, top), [1.0, 0.0, 0.0, 0.0]);
        let mut route = route_turned(ScreenRotation::Deg0);
        route.mirror_vertical = true;
        assert_eq!(displayed_sides(&route, top), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn one_to_one_uses_the_regions_pixels_at_the_top_left() {
        let mut region = Region::full_stage(3840.0, 2160.0);
        region.x = 100.0;
        region.width = 2000.0;
        assert_eq!(
            one_to_one(&region, ScreenRotation::Deg0),
            (0, 0, 2000, 2160)
        );
        assert_eq!(
            one_to_one(&region, ScreenRotation::Deg90),
            (0, 0, 2160, 2000)
        );
    }

    #[test]
    fn oversized_region_stays_at_the_origin_when_moved() {
        assert_eq!(
            moved_area((0, 0, 3840, 2160), (500.0, 500.0), DEVICE),
            (0, 0, 3840, 2160)
        );
    }

    #[test]
    fn placement_sets_a_one_to_one_raster() {
        let mut route = route_turned(ScreenRotation::Deg0);
        set_placement(&mut route, (10, 20, 800, 600), DEVICE);
        assert_eq!((route.width, route.height), (800, 600));
        assert!(matches!(route.device_area, DeviceArea::Part { x: 10, .. }));
        assert_eq!(route.scaling, RouteScaling::Stretch);

        let mut route = route_turned(ScreenRotation::Deg90);
        set_placement(&mut route, (0, 0, 600, 800), DEVICE);
        assert_eq!((route.width, route.height), (800, 600), "Raster ungedreht");

        set_placement(&mut route, (0, 0, 1920, 1080), DEVICE);
        assert_eq!(route.device_area, DeviceArea::Full);
    }
}
