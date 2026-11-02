//! Stage-/Route-Ansichten: Monitor-Fenster pro Stage, Vorschau-Fenster pro
//! Output Route und randlose Device-Ausgabe
//! (**Stage → Region → Output Route → Device**).
//!
//! * **Stage-Monitor** ([`draw_stage_windows`]): `egui::Window` pro Stage
//!   mit dem komponierten Canvas („Monitor“-Fenster).
//! * **Route-Vorschau** ([`draw_route_previews`]): ein Fenster pro Route
//!   (`Route: <Name>`) – für die Vorschau zählt der Inhalt, nicht der
//!   Bildschirm. Testbild-Linien werden bei Verkleinerung nachgezeichnet.
//! * **Device-Ausgabe** ([`draw_device_outputs`]): **ein randloses Fenster
//!   pro belegtem Display** (`KuhPlay – Output <Display-ID>`), das alle
//!   Routen des Displays zeigt (auch Teilbereiche an ihrer Stelle) – damit
//!   fällt die Spaces-Grenze von macOS weg. Esc schließt die Ausgabe
//!   (vorhandenes Verhalten, erhalten). Fenster-Rechteck aus dem Display
//!   (in Punkten), Route-Auflösung in echten Pixeln.
//!
//! ## Erweiterung
//!
//! * NDI-Out: Route-Bild zusätzlich als NDI-Stream senden.

use egui::{
    Color32, ColorImage, Context, RichText, TextureOptions, Vec2, ViewportBuilder, ViewportId,
};

use super::app::OutputMode;
use super::theme::{self, palette};
use crate::core::video_routing::{DeviceArea, DeviceRef};
use std::sync::Arc;

use crate::core::{OutputRoute, ScreenRotation};
use crate::engine::frame::VideoFrame;
use crate::engine::route_renderer::present_on_device;
use crate::engine::StageFrameStore;

use super::app::ShowApp;

/// Baut (oder aktualisiert) die Canvas-Textur einer Stage.
///
/// Gleicher Textur-Name → in-place Update (egui ersetzt den Inhalt).
/// Rückgabe: (TextureHandle, Breite, Höhe) oder `None` ohne Canvas.
pub fn stage_texture(
    ctx: &Context,
    stage_frames: &StageFrameStore,
    stage_name: &str,
) -> Option<(egui::TextureHandle, u32, u32)> {
    let frame = stage_frames.lock().unwrap().get(stage_name).cloned()?;
    let image = ColorImage::from_rgba_unmultiplied(
        [frame.width as usize, frame.height as usize],
        &frame.rgba,
    );
    let tex = ctx.load_texture(
        format!("kuhplay-stage-canvas-{stage_name}"),
        image,
        TextureOptions::LINEAR,
    );
    Some((tex, frame.width, frame.height))
}

/// UV-Koordinaten der vier Ziel-Rechteck-Ecken (TL, TR, BR, BL) für eine
/// Drehung. Konvention: 90° = Canvas 90° **im Uhrzeigersinn**
/// (obere Canvas-Kante erscheint an der rechten Kante).
fn rotation_uvs(rotation: ScreenRotation) -> [(f32, f32); 4] {
    match rotation {
        ScreenRotation::Deg0 => [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ScreenRotation::Deg90 => [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
        ScreenRotation::Deg180 => [(1.0, 1.0), (0.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
        ScreenRotation::Deg270 => [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)],
    }
}

/// Erzeugt ein egui-Mesh, das die Textur `tex_id` vollständig und gemäß
/// `rotation` gedreht in `rect` zeichnet (Rechteck wird exakt gefüllt).
pub fn rotated_mesh(
    rect: egui::Rect,
    rotation: ScreenRotation,
    tex_id: egui::TextureId,
) -> egui::Mesh {
    let corners = [
        rect.min,                           // TL
        egui::pos2(rect.max.x, rect.min.y), // TR
        rect.max,                           // BR
        egui::pos2(rect.min.x, rect.max.y), // BL
    ];
    let uvs = rotation_uvs(rotation);
    let mut mesh = egui::Mesh::with_texture(tex_id);
    for (corner, (u, v)) in corners.iter().zip(uvs.iter()) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: *corner,
            uv: egui::pos2(*u, *v),
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh
}

/// Zeichnet das Canvas gestreckt auf `rect` (exakter Screen-Footprint),
/// mit Screen-Drehung.
pub fn draw_canvas_fill_rotated(
    ui: &mut egui::Ui,
    tex: &egui::TextureHandle,
    rect: egui::Rect,
    rotation: ScreenRotation,
) {
    let mesh = rotated_mesh(rect, rotation, tex.id());
    ui.painter().add(egui::Shape::Mesh(mesh));
}

/// Zeichnet das Canvas aspekt-korrigiert in den verfügbaren Bereich,
/// mit Screen-Drehung. `allow_upscale` erlaubt Maßstäbe > 1 (Ausgabe).
pub fn draw_canvas_rotated(
    ui: &mut egui::Ui,
    tex: &egui::TextureHandle,
    w: u32,
    h: u32,
    rotation: ScreenRotation,
    allow_upscale: bool,
) {
    let avail = ui.available_size();
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let mut scale = (avail.x / w).min(avail.y / h).max(0.01);
    if !allow_upscale {
        scale = scale.min(1.0);
    }
    let size = Vec2::new(w * scale, h * scale);
    // Schwarzer Hintergrund + Canvas zentriert.
    let (rect, _response) = ui.allocate_exact_size(avail, egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    let canvas_rect = egui::Rect::from_center_size(rect.center(), size);
    draw_canvas_fill_rotated(ui, tex, canvas_rect, rotation);
}

/// Fallback: neustes Frame eines laufenden Video-/Diashow-Cues der Stage
/// (wenn noch kein komponiertes Canvas existiert).
fn fallback_frame(app: &ShowApp, stage_name: &str) -> Option<Arc<VideoFrame>> {
    let list = app.carts.active();
    for cue in list.cues() {
        if cue.stage.as_deref() != Some(stage_name) {
            continue;
        }
        if !matches!(
            cue.cue_type,
            crate::core::CueType::Video | crate::core::CueType::SlideShow
        ) {
            continue;
        }
        if let Some(frame) = app.engine.latest_frame(cue.id) {
            return Some(frame);
        }
    }
    None
}

/// Stage-Monitor-Fenster (ein Fenster pro geöffneter Stage, „Monitor“).
pub fn draw_stage_windows(app: &mut ShowApp, ctx: &Context) {
    let stage_names: Vec<String> = app.stage_windows.iter().cloned().collect();
    if !stage_names.is_empty() {
        tracing::debug!(?stage_names, "drawing stage windows");
    }
    for stage_name in stage_names {
        let Some(stage) = app.stages.find(&stage_name) else {
            app.stage_windows.remove(&stage_name);
            continue;
        };
        let (cw, ch) = (
            stage.geometry.width.max(1.0) as u32,
            stage.geometry.height.max(1.0) as u32,
        );
        let routes_on_stage: Vec<String> = app
            .routes
            .routes()
            .iter()
            .filter(|route| stage.regions.iter().any(|r| r.route == Some(route.id)))
            .map(|route| route.name.clone())
            .collect();
        let title = format!("Stage: {stage_name}");
        let mut open = true;
        egui::Window::new(RichText::new(&title).size(15.0))
            .id(egui::Id::new(("stage-window", &stage_name)))
            .default_size([cw as f32 * 0.5, ch as f32 * 0.5 + 28.0])
            .min_size([320.0, 200.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{cw}×{ch}")).weak());
                    ui.separator();
                    if routes_on_stage.is_empty() {
                        ui.label(RichText::new("no output routes").weak());
                    } else {
                        ui.label(
                            RichText::new(format!("routes: {}", routes_on_stage.join(", "))).weak(),
                        )
                        .on_hover_text("Output routes this stage feeds via its regions");
                    }
                });
                ui.separator();
                // Komponiertes Canvas (Compositor) oder Fallback (Cue-Frame).
                if let Some((tex, w, h)) = stage_texture(ctx, &app.engine.stage_frames, &stage_name)
                {
                    draw_canvas_rotated(ui, &tex, w, h, ScreenRotation::Deg0, false);
                } else if let Some(frame) = fallback_frame(app, &stage_name) {
                    let image = ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.rgba,
                    );
                    let tex = ctx.load_texture(
                        format!("kuhplay-stage-fallback-{stage_name}"),
                        image,
                        TextureOptions::LINEAR,
                    );
                    draw_canvas_rotated(
                        ui,
                        &tex,
                        frame.width,
                        frame.height,
                        ScreenRotation::Deg0,
                        false,
                    );
                } else {
                    ui.label(
                        RichText::new("No running video cue on this stage.")
                            .weak()
                            .italics(),
                    );
                }
            });
        if !open {
            app.stage_windows.remove(&stage_name);
        }
    }
}

/// Hardware-Display-Ausgabe: **ein randloses Fenster pro belegtem Display**,
/// das alle Routen des Displays zeigt (Listenreihenfolge – keine zufällige
/// Überdeckung mehr). Teilbereiche (`DeviceArea::Part`) liegen an ihrer
/// Stelle im Fenster. Fensterplatzierung in Punkten, Route-Auflösung in
/// echten Pixeln (`present_on_device` skaliert/rotiert/spiegelt).
pub fn draw_device_outputs(app: &mut ShowApp, ctx: &Context) {
    if app.output_mode != OutputMode::Displays {
        return;
    }
    let displays: Vec<crate::core::DisplayRefInfo> = app
        .displays
        .iter()
        .map(crate::core::DisplayRefInfo::from)
        .collect();
    for display in displays {
        // Nur belegte Displays: mind. eine Route zeigt auf dieses Gerät.
        let routes: Vec<OutputRoute> = app
            .routes
            .routes()
            .iter()
            .filter(|route| {
                matches!(
                    &route.device,
                    Some(DeviceRef::Display { id, .. }) if *id == display.id
                )
            })
            .cloned()
            .collect();
        if routes.is_empty() {
            continue;
        }
        let closed = app.closed_device_outputs.clone();
        if closed.lock().unwrap().contains(&display.id) {
            continue;
        }
        // Titel eindeutig (macos_output findet das Fenster über den Titel).
        let title = format!("KuhPlay – Output {}", display.id);
        let (sx, sy, sw, sh) = (
            display.x as f32,
            display.y as f32,
            display.width as f32,
            display.height as f32,
        );
        let (px_w, px_h) = display.pixel_size();
        let viewport_id = ViewportId::from_hash_of(("kuhplay-device-output", display.id));
        let builder = ViewportBuilder::default()
            .with_title(&title)
            .with_decorations(false)
            // Immer oben, kein Taskbar-Eintrag.
            .with_always_on_top()
            .with_taskbar(false)
            .with_position([sx, sy])
            .with_inner_size([sw, sh])
            .with_min_inner_size([sw, sh])
            .with_max_inner_size([sw, sh]);
        let route_frames = app.engine.route_frames.clone();
        let stage_frames = app.engine.stage_frames.clone();
        let stages: Vec<crate::core::Stage> = app.stages.stages().to_vec();
        let scale = display.scale_factor;
        let display_id = display.id;
        ctx.show_viewport_deferred(viewport_id, builder, move |ctx, _class| {
            // Beim Öffnen einmal den Tastaturfokus holen, damit Esc ohne
            // Klick ankommt (das Fenster liegt über allem – sonst käme man
            // mit nur einem Display nicht mehr an die App heran).
            let focus_id = egui::Id::new(("device_output_focused", display_id));
            if !ctx.data_mut(|d| d.get_temp::<bool>(focus_id).unwrap_or(false)) {
                ctx.data_mut(|d| d.insert_temp(focus_id, true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            // Playout: Der Mauszeiger bleibt auf der Ausgabe unsichtbar.
            ctx.set_cursor_icon(egui::CursorIcon::None);
            // Esc schließt die Ausgabe (Zusätzlich gilt: Esc in der App
            // schaltet die Device-Ausgabe aus, siehe handle_hotkeys).
            let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            if escape || ctx.input(|i| i.viewport().close_requested()) {
                closed.lock().unwrap().insert(display_id);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            // Exakte Fensterplatzierung über AppKit (macOS schiebt normale
            // Fenster unter die Menüleiste).
            #[cfg(target_os = "macos")]
            super::macos_output::place_output_window(
                &title,
                f64::from(sx),
                f64::from(sy),
                f64::from(sw),
                f64::from(sh),
            );
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::BLACK))
                .show(ctx, |ui| {
                    let window_rect = ui.available_rect_before_wrap();
                    ui.painter().rect_filled(window_rect, 0.0, Color32::BLACK);
                    // Schließen ohne Tastatur: dezentes × oben rechts, nur
                    // bei Mauskontakt sichtbar (Cursor ist versteckt).
                    let pointer_inside = ctx.input(|i| {
                        i.pointer
                            .hover_pos()
                            .is_some_and(|p| window_rect.contains(p))
                    });
                    if pointer_inside {
                        let size = 28.0;
                        let btn = egui::Rect::from_min_size(
                            window_rect.right_top() - egui::vec2(size, 0.0),
                            egui::vec2(size, size),
                        );
                        let hovered =
                            ctx.input(|i| i.pointer.hover_pos().is_some_and(|p| btn.contains(p)));
                        let resp = ui.interact(
                            btn,
                            egui::Id::new(("device_close", display_id)),
                            egui::Sense::click(),
                        );
                        if hovered {
                            ui.painter().rect_filled(
                                btn,
                                4.0,
                                theme::with_alpha(palette::ACCENT, 40),
                            );
                            ui.painter().text(
                                btn.center(),
                                egui::Align2::CENTER_CENTER,
                                "×",
                                egui::FontId::proportional(20.0),
                                palette::TEXT,
                            );
                        }
                        if resp.clicked() {
                            closed.lock().unwrap().insert(display_id);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        resp.on_hover_text("Close this output (Esc)");
                    }
                    // Listenreihenfolge der Routen: Die spätere Route liegt oben.
                    for route in &routes {
                        // Zielbereich auf dem Gerät (Gerätepixel).
                        let (ax, ay, aw, ah) = match route.device_area {
                            DeviceArea::Full => (0.0f32, 0.0f32, px_w as f32, px_h as f32),
                            DeviceArea::Part {
                                x,
                                y,
                                width,
                                height,
                            } => (x as f32, y as f32, width as f32, height as f32),
                        };
                        let frame = route_frames.lock().unwrap().get(&route.id).cloned();
                        let Some(frame) = frame else {
                            continue; // Noch kein Route-Bild (Engine startet).
                        };
                        // Key-Region: Das Display zeigt die Alpha als Graubild.
                        let frame = if route.alpha {
                            std::sync::Arc::new(crate::engine::decklink::alpha_to_luma(&frame))
                        } else {
                            frame
                        };
                        // Route-Bild auf den Zielbereich abbilden
                        // (Drehung, Spiegelung, Skalierung).
                        let presented = present_on_device(
                            &frame,
                            route,
                            aw.max(1.0) as u32,
                            ah.max(1.0) as u32,
                        );
                        let image = ColorImage::from_rgba_unmultiplied(
                            [presented.width as usize, presented.height as usize],
                            &presented.rgba,
                        );
                        // Zielbereich im Fenster (Fensterkoordinaten = Punkte).
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(
                                window_rect.min.x + ax / scale,
                                window_rect.min.y + ay / scale,
                            ),
                            egui::vec2(aw / scale, ah / scale),
                        );
                        // Pixelgleich (Scale 1: Textur = Fensterbereich) →
                        // NEAREST, sonst LINEAR (1-Pixel-Linien nur exakt).
                        let pixel_equal = (presented.width as f32 - rect.width()).abs() < 0.5
                            && (presented.height as f32 - rect.height()).abs() < 0.5;
                        let options = if pixel_equal {
                            TextureOptions::NEAREST
                        } else {
                            TextureOptions::LINEAR
                        };
                        let tex = ui.ctx().load_texture(
                            format!("kuhplay-device-{}", route.id),
                            image,
                            options,
                        );
                        ui.painter().add(egui::Shape::Mesh(rotated_mesh(
                            rect,
                            ScreenRotation::Deg0,
                            tex.id(),
                        )));
                    }
                    // Stage-Placements direkt auf dem Display (1:1 Gerätepixel,
                    // Clipping am Displayrand). Z-Reihenfolge: Stage-Layer
                    // (niedriger zuerst), gleicher Layer = alphabetisch
                    // früherer Name oben (male später).
                    let mut placements: Vec<(
                        i32,
                        String,
                        uuid::Uuid,
                        crate::core::StagePlacement,
                    )> = Vec::new();
                    for stage in &stages {
                        if !stage.visible {
                            continue;
                        }
                        for placement in &stage.placements {
                            if placement.display == display_id {
                                placements.push((
                                    stage.layer,
                                    stage.name.clone(),
                                    stage.id,
                                    placement.clone(),
                                ));
                            }
                        }
                    }
                    placements.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
                    for (_layer, name, sid, placement) in placements {
                        // Sichtbarer Teil = Schnitt Placement ∩ Display.
                        let Some((cx, cy, cw, ch)) = placement.clip_to_display(px_w, px_h) else {
                            continue; // liegt komplett außerhalb → nichts zeigen
                        };
                        // Stage-Frame (komponiertes Canvas) auf das Placement-Rechteck.
                        let Some(frame) = stage_frames.lock().unwrap().get(&name).cloned() else {
                            continue;
                        };
                        let image = ColorImage::from_rgba_unmultiplied(
                            [frame.width as usize, frame.height as usize],
                            &frame.rgba,
                        );
                        let tex = ui.ctx().load_texture(
                            format!("kuhplay-device-placement-{sid}"),
                            image,
                            TextureOptions::LINEAR,
                        );
                        // Sichtbarer Ausschnitt in der Textur (UV relativ zum
                        // Placement-Rechteck, das die ganze Stage zeigt).
                        let uv = egui::Rect::from_min_size(
                            egui::pos2(
                                (cx - placement.x) as f32 / placement.width as f32,
                                (cy - placement.y) as f32 / placement.height as f32,
                            ),
                            egui::vec2(
                                cw as f32 / placement.width as f32,
                                ch as f32 / placement.height as f32,
                            ),
                        );
                        let vis_rect = egui::Rect::from_min_size(
                            egui::pos2(
                                window_rect.min.x + cx as f32 / scale,
                                window_rect.min.y + cy as f32 / scale,
                            ),
                            egui::vec2(cw as f32 / scale, ch as f32 / scale),
                        );
                        let mut mesh = egui::Mesh::with_texture(tex.id());
                        let corners = [
                            vis_rect.min,
                            egui::pos2(vis_rect.max.x, vis_rect.min.y),
                            vis_rect.max,
                            egui::pos2(vis_rect.min.x, vis_rect.max.y),
                        ];
                        let uvs = [
                            uv.min,
                            egui::pos2(uv.max.x, uv.min.y),
                            uv.max,
                            egui::pos2(uv.min.x, uv.max.y),
                        ];
                        for (corner, uv) in corners.iter().zip(uvs.iter()) {
                            mesh.vertices.push(egui::epaint::Vertex {
                                pos: *corner,
                                uv: *uv,
                                color: Color32::WHITE,
                            });
                        }
                        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                        ui.painter().add(egui::Shape::Mesh(mesh));
                    }
                });
            ctx.request_repaint();
        });
    }
}

/// Liste der Hardware-Displays (für Dropdowns) – gecacht, mit
/// Refresh-Möglichkeit.
pub fn detect_displays() -> Vec<display_info::DisplayInfo> {
    // AppKit (NSScreen) verträgt keine parallelen Abfragen: In den Tests bauen
    // viele Threads gleichzeitig eine `ShowApp`, das führte zu SIGSEGV/SIGTRAP.
    static QUERY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    display_info::DisplayInfo::all().unwrap_or_default()
}

/// Vorschau-Fenster pro Route (`Route: <Name>`): dasselbe Bild wie die
/// Device-Ausgabe, aber als eigenes Fenster, das sich neben die App
/// schieben lässt – die Vorschau zählt den Inhalt, nicht den Bildschirm
/// (Arbeitsauftrag 3.3). Testbild-Linien werden nachgezeichnet, sonst
/// verschwinden sie beim Verkleinern.
pub fn draw_route_previews(app: &mut ShowApp, ctx: &Context) {
    if app.output_mode != OutputMode::Preview {
        return;
    }
    let routes: Vec<OutputRoute> = app.routes.routes().to_vec();
    for (index, route) in routes.into_iter().enumerate() {
        let closed = app.closed_previews.clone();
        if closed.lock().unwrap().contains(&route.id) {
            continue;
        }
        let aspect = route.height.max(1) as f32 / route.width.max(1) as f32;
        let offset = index as f32 * 32.0;
        let title = format!("Route: {}", route.name);
        let builder = ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([480.0, 480.0 * aspect])
            .with_position([80.0 + offset, 80.0 + offset]);
        let test_pattern = route.test_pattern;
        let route_id = route.id;
        let route_frames = app.engine.route_frames.clone();
        ctx.show_viewport_deferred(
            ViewportId::from_hash_of(("kuhplay-route-preview", route_id)),
            builder,
            move |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    closed.lock().unwrap().insert(route_id);
                    return;
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Color32::BLACK))
                    .show(ctx, |ui| {
                        // Seitenverhältnis der Route, auch im gezogenen Fenster.
                        let space = ui.available_rect_before_wrap();
                        let size = if space.width() * aspect <= space.height() {
                            egui::vec2(space.width(), space.width() * aspect)
                        } else {
                            egui::vec2(space.height() / aspect, space.height())
                        };
                        let rect = egui::Rect::from_center_size(space.center(), size);
                        let has_frame = route_frames.lock().unwrap().contains_key(&route_id);
                        if let Some(frame) = route_frames.lock().unwrap().get(&route_id).cloned() {
                            let image = ColorImage::from_rgba_unmultiplied(
                                [frame.width as usize, frame.height as usize],
                                &frame.rgba,
                            );
                            let tex = ui.ctx().load_texture(
                                format!("kuhplay-route-preview-{}", route_id),
                                image,
                                TextureOptions::LINEAR,
                            );
                            ui.painter().add(egui::Shape::Mesh(rotated_mesh(
                                rect,
                                ScreenRotation::Deg0,
                                tex.id(),
                            )));
                        }
                        // Verkleinert gingen die 1-px-Linien des Testbilds
                        // verloren.
                        if test_pattern {
                            let stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
                            ui.painter().rect_stroke(rect.shrink(0.5), 0.0, stroke);
                            ui.painter().circle_stroke(
                                rect.center(),
                                rect.width().min(rect.height()) / 2.0 - 1.0,
                                stroke,
                            );
                        }
                        if !has_frame {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "No output",
                                egui::FontId::proportional(12.0),
                                palette::TEXT_DIM,
                            );
                        }
                    });
                ctx.request_repaint();
            },
        );
    }
}
