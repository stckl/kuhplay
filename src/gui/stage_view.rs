//! Stage-Ansicht: In-App-Stage-Fenster (komponiertes Canvas) und
//! Hardware-Screen-Ausgabe (QLab 5 „Stage → Screen“).
//!
//! * **Stage-Fenster:** `egui::Window` pro Stage mit dem vom Engine-Compositor
//!   gerenderten Canvas (Layer, Area of Interest, Hard-Edge, Deckkraft).
//!   Ist die Stage einem Screen zugeordnet, wird die Vorschau im
//!   Screen-Footprint (inkl. Drehung) dargestellt – so sieht man exakt,
//!   was auf dem Hardware-Output läuft.
//! * **Screen-Ausgabe:** `ctx.show_viewport_deferred` legt ein **echtes
//!   Fullscreen-Fenster** (dekorationslos, immer oben, ohne Taskbar-Eintrag)
//!   exakt auf das Rechteck des zugeordneten Screens (`Stage::screen` →
//!   `ScreenManager`). Screens sind frei definierbare Rechtecke im globalen
//!   Display-Koordinatenraum (Teile eines Displays, über mehrere Displays
//!   gespannt, mehrere pro Display) – QLab 5 „Screens". egui 0.30 bietet
//!   kein Monitor-Handle für `with_fullscreen`; Always-on-Top + exakte
//!   Screen-Rect ist die portable Lösung.
//! * **Drehung:** `ScreenRotation` (0/90/180/270°) wird über ein rotiertes
//!   Textur-Mesh im Viewport umgesetzt (obere Canvas-Kante erscheint bei
//!   90° an der rechten Screen-Kante). Testbild und Inhalt drehen sich
//!   gemeinsam, weil das Testbild engine-seitig *ungedreht* ins Canvas
//!   gezeichnet wird.
//!
//! ## Erweiterung
//!
//! * `with_fullscreen(true)` pro Screen, sobald egui Monitor-Handles bietet.
//! * NDI-Out: Canvas zusätzlich als NDI-Stream senden.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{
    Color32, ColorImage, Context, RichText, TextureOptions, Vec2, ViewportBuilder, ViewportId,
};

use super::app::OutputMode;
use super::theme::palette;
use crate::core::{ScreenRotation, Stage};
use crate::engine::frame::VideoFrame;
use crate::engine::StageFrameStore;

use super::app::ShowApp;
use super::icons::{self, Icon};

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
/// Screen-Drehung. Konvention: 90° = Canvas 90° **im Uhrzeigersinn**
/// (obere Canvas-Kante erscheint an der rechten Screen-Kante).
fn rotation_uvs(rotation: ScreenRotation) -> [(f32, f32); 4] {
    match rotation {
        ScreenRotation::Deg0 => [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
        ScreenRotation::Deg90 => [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
        ScreenRotation::Deg180 => [(1.0, 1.0), (0.0, 1.0), (0.0, 0.0), (1.0, 0.0)],
        ScreenRotation::Deg270 => [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)],
    }
}

/// Erzeugt ein egui-Mesh, das die Textur `tex_id` vollständig und gemäß
/// `rotation` gedreht in `rect` zeichnet (Rechteck wird exakt gefüllt,
/// d. h. bei abweichendem Seitenverhältnis wird gestreckt).
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
/// mit Screen-Drehung. `(w, h)` ist die **Footprint-Größe** des Screens
/// (ggf. rotiert: `ScreenRotation::rotated_size`), nicht die Canvas-Größe.
/// `allow_upscale` erlaubt Maßstäbe > 1 (Screen-Ausgabe), sonst max. 1:1.
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

/// In-App-Stage-Fenster (ein Fenster pro geöffneter Stage).
pub fn draw_stage_windows(app: &mut ShowApp, ctx: &Context) {
    let stage_names: Vec<String> = app.stage_windows.iter().cloned().collect();
    if !stage_names.is_empty() {
        tracing::debug!(?stage_names, "drawing stage windows");
    }
    for stage_name in stage_names {
        let canvas = app
            .stages
            .find(&stage_name)
            .map(|s| (s.geometry.width as u32, s.geometry.height as u32));
        let (cw, ch) = canvas.unwrap_or((1920, 1080));
        // Zugeordneter Screen → Footprint + Drehung für die Vorschau.
        let assigned = app
            .stages
            .find(&stage_name)
            .and_then(|s| s.screen)
            .and_then(|id| app.screens.find(id))
            .map(|scr| {
                let (fw, fh) = scr.rotation.rotated_size(scr.width, scr.height);
                (scr.id, scr.label(), fw, fh, scr.rotation)
            });
        let (foot_w, foot_h, rotation) = match &assigned {
            Some((_, _, fw, fh, rot)) => (*fw, *fh, *rot),
            None => (cw, ch, ScreenRotation::Deg0),
        };
        let title = format!("Stage: {stage_name}");
        let mut open = true;
        egui::Window::new(RichText::new(&title).size(15.0))
            .id(egui::Id::new(("stage-window", &stage_name)))
            .default_size([cw as f32 * 0.5, ch as f32 * 0.5 + 28.0])
            .min_size([320.0, 200.0])
            .open(&mut open)
            .show(ctx, |ui| {
                // Toolbar: Screen-Info, Testbild, Screen-Ausgabe öffnen/schließen.
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{cw}×{ch}")).weak());
                    ui.separator();
                    if let Some((screen_id, screen_label, _, _, _)) = &assigned {
                        ui.label(RichText::new(format!("on {screen_label}")).weak())
                            .on_hover_text("Assigned screen");
                        let mut pattern =
                            app.screens.find(*screen_id).is_some_and(|s| s.test_pattern);
                        if ui
                            .checkbox(&mut pattern, "Test Pattern")
                            .on_hover_text(
                                "Testbild (Rahmen/Kreis/Raster/Schachbrett/Beschriftung) \
                                 auf diesem Screen – QLab „Test Pattern\"",
                            )
                            .changed()
                        {
                            app.set_screen_test_pattern(*screen_id, pattern);
                        }
                        ui.separator();
                    }
                    let color = ui.visuals().text_color();
                    let is_open = app
                        .stage_screen_open
                        .lock()
                        .unwrap()
                        .get(&stage_name)
                        .copied()
                        .unwrap_or(false);
                    if !is_open {
                        if icons::button(ui, Icon::Globe, color, egui::vec2(24.0, 24.0), true)
                            .on_hover_text("Open on hardware screen (fullscreen)")
                            .clicked()
                        {
                            app.open_stage_screen(&stage_name);
                        }
                    } else if icons::button(ui, Icon::Close, color, egui::vec2(24.0, 24.0), true)
                        .on_hover_text("Close screen output")
                        .clicked()
                    {
                        app.close_stage_screen(&stage_name);
                    }
                });
                ui.separator();
                // Komponiertes Canvas (Compositor) oder Fallback (Cue-Frame).
                if let Some((tex, w, h)) = stage_texture(ctx, &app.engine.stage_frames, &stage_name)
                {
                    let (dw, dh) = if assigned.is_some() {
                        (foot_w, foot_h)
                    } else {
                        (w, h)
                    };
                    draw_canvas_rotated(ui, &tex, dw, dh, rotation, false);
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
                    let (dw, dh) = if assigned.is_some() {
                        (foot_w, foot_h)
                    } else {
                        (frame.width, frame.height)
                    };
                    draw_canvas_rotated(ui, &tex, dw, dh, rotation, false);
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

/// Hardware-Screen-Ausgabe: echtes Fullscreen-Viewport exakt auf dem
/// Rechteck des zugeordneten Screens (Stage → Screen → ScreenManager).
/// Wird jeden Frame mit `show_viewport_deferred` aktualisiert
/// (egui-Mehr-Viewport). Das Canvas füllt den Screen-Footprint exakt
/// (gestreckt) und wird gemäß `ScreenRotation` gedreht.
pub fn draw_screen_viewports(app: &mut ShowApp, ctx: &Context) {
    let mut targets: Vec<(String, Stage)> = Vec::new();
    for stage in app.stages.stages() {
        if stage.screen.is_some() {
            targets.push((stage.name.clone(), stage.clone()));
        }
    }
    for (stage_name, stage) in targets {
        let Some(screen_id) = stage.screen else {
            continue;
        };
        // Screen im ScreenManager nachschlagen (UUID-Referenz).
        let Some(screen) = app.screens.find(screen_id) else {
            continue; // Screen gelöscht → nur In-App-Fenster
        };
        // Footprint des Screens nach Drehung (90°/270° vertauschen W/H).
        let (foot_w, foot_h) = screen.rotation.rotated_size(screen.width, screen.height);
        let (sx, sy) = (screen.x as f32, screen.y as f32);
        let (sw, sh) = (foot_w as f32, foot_h as f32);
        let rotation = screen.rotation;
        let open_flag = app.stage_screen_open.clone();
        let stage_frames = app.engine.stage_frames.clone();
        let is_open = app
            .stage_screen_open
            .lock()
            .unwrap()
            .get(&stage_name)
            .copied()
            .unwrap_or(false);
        if !is_open {
            continue;
        }
        let viewport_id = ViewportId::from_hash_of(("kuhplay-screen", &stage_name));
        let builder = ViewportBuilder::default()
            .with_title(format!("KuhPlay – {stage_name}"))
            .with_decorations(false)
            // Echtes Fullscreen: immer oben, kein Taskbar-Eintrag.
            // (egui 0.30 hat kein Monitor-Handle für `with_fullscreen`.)
            .with_always_on_top()
            .with_taskbar(false)
            .with_position([sx, sy])
            .with_inner_size([sw, sh])
            .with_min_inner_size([sw, sh])
            .with_max_inner_size([sw, sh]);
        let name = stage_name.clone();
        let flag = open_flag.clone();
        ctx.show_viewport_deferred(viewport_id, builder, move |ctx, _class| {
            // Fenster wurde geschlossen (z. B. Cmd+W / Alt+F4) → Flag zurück.
            if ctx.input(|i| i.viewport().close_requested()) {
                flag.lock().unwrap().insert(name.clone(), false);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none())
                .show(ctx, |ui| {
                    if let Some((tex, _w, _h)) = stage_texture(ctx, &stage_frames, &name) {
                        // Canvas füllt den Screen-Footprint exakt (ggf. gestreckt),
                        // mit Screen-Drehung.
                        let rect = ui.available_rect_before_wrap();
                        draw_canvas_fill_rotated(ui, &tex, rect, rotation);
                    } else {
                        ui.painter().rect_filled(
                            ui.available_rect_before_wrap(),
                            0.0,
                            Color32::BLACK,
                        );
                    }
                });
            ctx.request_repaint();
        });
    }
}

/// Liste der Hardware-Displays (für Screen-Dropdowns) – gecacht, mit
/// Refresh-Möglichkeit.
pub fn detect_displays() -> Vec<display_info::DisplayInfo> {
    // AppKit (NSScreen) verträgt keine parallelen Abfragen: In den Tests bauen
    // viele Threads gleichzeitig eine `ShowApp`, das führte zu SIGSEGV/SIGTRAP.
    static QUERY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = QUERY.lock().unwrap_or_else(|e| e.into_inner());
    display_info::DisplayInfo::all().unwrap_or_default()
}

/// Offene Screen-Status-Map (geteilt mit den Viewport-Closures).
pub type StageScreenOpen = Arc<Mutex<HashMap<String, bool>>>;

/// Erzeugt die Shared-Map für Screen-Open-States.
pub fn new_stage_screen_open() -> StageScreenOpen {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Vorschau-Fenster pro Screen: dasselbe Bild wie die Vollbild-Ausgabe, aber
/// als normales Fenster – für die Arbeit an einem einzelnen Display.
pub fn draw_screen_previews(app: &mut ShowApp, ctx: &Context) {
    if app.output_mode != OutputMode::Preview {
        return;
    }
    let screens: Vec<(uuid::Uuid, String, u32, u32, ScreenRotation)> = app
        .screens
        .screens()
        .iter()
        .map(|s| {
            let (w, h) = s.rotation.rotated_size(s.width, s.height);
            (s.id, s.name.clone(), w, h, s.rotation)
        })
        .collect();
    for (index, (id, name, width, height, rotation)) in screens.into_iter().enumerate() {
        if app.closed_previews.contains(&id) {
            continue;
        }
        let stages: Vec<String> = app
            .stages
            .stages()
            .iter()
            .filter(|stage| stage.screen == Some(id) && stage.visible)
            .map(|stage| stage.name.clone())
            .collect();
        let mut open = true;
        let offset = index as f32 * 32.0;
        egui::Window::new(format!("Screen: {name}"))
            .id(egui::Id::new(("kuhplay_screen_preview", id)))
            .open(&mut open)
            .default_width(360.0)
            .default_pos(
                ctx.screen_rect().right_top() + egui::vec2(-400.0 - offset, 150.0 + offset),
            )
            .show(ctx, |ui| {
                let aspect = height.max(1) as f32 / width.max(1) as f32;
                let size = egui::vec2(ui.available_width(), ui.available_width() * aspect);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
                let mut painted = false;
                for stage in &stages {
                    if let Some((tex, _, _)) = stage_texture(ctx, &app.engine.stage_frames, stage) {
                        let mesh = rotated_mesh(rect, rotation, tex.id());
                        ui.painter().add(egui::Shape::Mesh(mesh));
                        painted = true;
                    }
                }
                if !painted {
                    let hint = if stages.is_empty() {
                        "No stage assigned"
                    } else {
                        "No output"
                    };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        hint,
                        egui::FontId::proportional(12.0),
                        palette::TEXT_DIM,
                    );
                }
                ui.label(
                    RichText::new(format!("{width}×{height}"))
                        .size(11.0)
                        .color(palette::TEXT_DIM),
                );
            });
        if !open {
            app.closed_previews.insert(id);
        }
    }
}
