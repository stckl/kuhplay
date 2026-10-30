//! Screen-Editor (QLab 5: „Video → Screens").
//!
//! Visueller Editor für die Ausgabe-**Screens** – die physischen
//! Ausgabe-Rechtecke, auf denen Stages wiedergegeben werden:
//!
//! * **Karte:** alle erkannten Displays (Umriss) und alle Screens
//!   (gefüllte Rechtecke) im globalen Display-Koordinatenraum, maßstäblich
//!   eingepasst. Screens können Teile eines Displays, über mehrere
//!   Displays gespannt oder mehrfach pro Display liegen.
//! * **Interaktionen:** Klick wählt einen Screen; Ziehen *innerhalb*
//!   verschiebt ihn; Ziehen an der **unteren rechten Ecke** skaliert ihn
//!   (pixelgenau, Fußabdruck folgt dem Zeiger); Ziehen auf leerer Fläche
//!   legt per Rubber-Band einen neuen Screen an.
//! * **Eigenschaften** des ausgewählten Screens: Name, X/Y (i32) und
//!   Breite/Höhe (u32) als pixelgenaue DragValues, Drehung (0/90/180/270°)
//!   und Testbild-Schalter.
//! * **Testbild:** QLab „Test Pattern" (Rahmen, Kreis, Raster,
//!   Schachbrettmuster, Beschriftung mit Name & Größe) – zum Einrichten
//!   und Ausrichten von Projektoren und LED-Wänden.
//!
//! ## Erweiterung
//!
//! * Zoom/Pan der Karte (aktuell: fit-to-view).
//! * Screens per Drag auf ein anderes Display eingeschränkt („snap to
//!   display") – QLab-Screen-Editor.
//! * Vorschau der zugeordneten Stage direkt in der Karte.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use uuid::Uuid;

use crate::core::{Screen, ScreenManager, ScreenRotation};
use crate::engine::{EngineCommand, EngineHandle};

use super::app::ShowApp;
use super::icons::{self, Icon};
use super::stage_view;
use super::theme::{self, palette, size};

/// Drag-Interaktion auf der Editor-Karte.
#[derive(Debug, Clone, Copy, Default)]
enum Drag {
    /// Keine Drag-Interaktion aktiv.
    #[default]
    Idle,
    /// Screen verschieben (Fußabdruck-Topleft folgt dem Zeiger + Griff-Offset).
    Move { id: Uuid, grab: (i32, i32) },
    /// Screen skalieren (an einer Ecke ziehen).
    Resize { id: Uuid, corner: ResizeCorner },
    /// Rubber-Band: neuer Screen wird aufgezogen.
    Rubber { start: (i32, i32) },
}

/// Ecke, an der ein Screen skaliert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeCorner {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

/// Zeichnet den Screen-Editor (Sidebar-Tab „Screens").
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Hotkey-Gate zuerst berechnen (braucht `&app`, bevor die
    // Feld-Borrows unten starten).
    let text_focused = app.text_field_focused(ui.ctx());
    // Disjunkte Feld-Borrows (wie im Inspector).
    let screens = &mut app.screens;
    let displays = &mut app.displays;
    let stages = &mut app.stages;
    let engine = &app.engine;
    let stage_screen_open = app.stage_screen_open.clone();
    let selected = &mut app.screen_editor_selected;
    let text_fields = &mut app.text_field_ids;

    // --- Tastenkürzel (R=Drehen, T=Testbild, Del=Löschen) ---
    // Nur wenn kein Textfeld fokussiert ist (Slider dürfen nicht blockieren).
    let kb = text_focused;
    if !kb {
        let (rot, del, tog) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::T),
            )
        });
        if let Some(sel_id) = *selected {
            if rot {
                if let Some(s) = screens.find_mut(sel_id) {
                    s.rotation = s.rotation.next();
                }
            }
            if tog {
                let new_val = !screens.find(sel_id).is_some_and(|s| s.test_pattern);
                set_pattern(screens, engine, sel_id, new_val);
            }
            if del {
                // Zugeordnete Stages entziehen.
                let names: Vec<String> = stages
                    .stages()
                    .iter()
                    .filter(|st| st.screen == Some(sel_id))
                    .map(|st| st.name.clone())
                    .collect();
                for name in names {
                    if let Some(stage) = stages.find_mut(&name) {
                        stage.screen = None;
                        let stage = stage.clone();
                        let _ = engine.send(EngineCommand::ConfigureStage { stage });
                    }
                }
                screens.remove(sel_id);
                *selected = None;
            }
        }
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // --- Kopf: Titel + globale Buttons ---
            ui.horizontal(|ui| {
                theme::section_label(ui, "Screens");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let color = ui.visuals().text_color();
                    let any_pattern = screens.screens().iter().any(|s| s.test_pattern);
                    if icons::button(ui, Icon::TestPattern, color, egui::vec2(22.0, 22.0), true)
                        .on_hover_text(if any_pattern {
                            "Turn off test patterns on all screens"
                        } else {
                            "Turn on test patterns on all screens (frame/circle/grid/checkerboard)"
                        })
                        .clicked()
                    {
                        set_all_patterns(screens, engine, !any_pattern);
                    }
                    if icons::button(
                        ui,
                        Icon::ArrowClockwise,
                        color,
                        egui::vec2(22.0, 22.0),
                        true,
                    )
                    .on_hover_text("Refresh displays (e.g. after connecting a projector)")
                    .clicked()
                    {
                        *displays = stage_view::detect_displays();
                    }
                });
            });
            ui.add_space(4.0);

            // --- Screen über einem Display anlegen ---
            let mut add_over: Option<usize> = None;
            egui::ComboBox::from_id_salt("screen_add_display")
                .selected_text("+ Screen over Display …")
                .show_ui(ui, |ui| {
                    for (i, d) in displays.iter().enumerate() {
                        let label =
                            format!("{}  ({}×{} @ {}, {})", d.name, d.width, d.height, d.x, d.y);
                        if ui.selectable_label(false, label).clicked() {
                            add_over = Some(i);
                        }
                    }
                });
            if let Some(i) = add_over {
                if let Some(d) = displays.get(i) {
                    let name = next_screen_name(screens);
                    let screen = ScreenManager::for_display(name, d);
                    if let Err(e) = screens.add(screen) {
                        tracing::warn!(error = %e, "could not create screen");
                    } else if let Some(s) = screens.screens().last() {
                        *selected = Some(s.id);
                    }
                }
            }
            ui.add_space(4.0);

            // --- Karte (Displays + Screens) ---
            ui.scope(|ui| draw_map(ui, screens, displays, selected));

            // --- Eigenschaften des ausgewählten Screens ---
            if let Some(sel_id) = *selected {
                if screens.find(sel_id).is_none() {
                    // Screen wurde gelöscht → Auswahl zurücksetzen.
                    *selected = None;
                } else {
                    draw_properties(
                        ui,
                        screens,
                        stages,
                        engine,
                        &stage_screen_open,
                        selected,
                        sel_id,
                        text_fields,
                    );
                }
            }

            if screens.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "No screens yet. Add one via \"+ Screen over Display …\" \
                         or drag a rectangle on the map.",
                    )
                    .weak()
                    .small(),
                );
            }
        });
}

/// Zeichnet die Karte (Display-Umrisse + Screen-Rechtecke) und behandelt
/// die Maus-Interaktionen (Wählen / Verschieben / Skalieren / Rubber-Band).
fn draw_map(
    ui: &mut egui::Ui,
    screens: &mut ScreenManager,
    displays: &[display_info::DisplayInfo],
    selected: &mut Option<Uuid>,
) {
    let canvas_height = 220.0_f32;
    let (canvas_rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), canvas_height),
        Sense::click_and_drag(),
    );
    // Nichts außerhalb der Karte zeichnen: Gedrehte oder verschobene Screens
    // können über die Display-Fläche hinausragen (Aufrufer kapselt per `scope`).
    ui.set_clip_rect(canvas_rect.intersect(ui.clip_rect()));
    ui.painter()
        .rect_filled(canvas_rect, 4.0, palette::BACKGROUND);

    // Welt → Bildschirm: Bounding-Box aller Displays, maßstäblich zentriert.
    // Während eines Drags bleibt der Maßstab fest, sonst zoomt die Karte unter
    // dem Mauszeiger weg.
    let bounds_id = ui.id().with("screen_map_bounds");
    let dragging = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Drag>(ui.id().with("screen_map_drag")))
        .is_some();
    let stored = ui
        .ctx()
        .data_mut(|d| d.get_temp::<(i32, i32, u32, u32)>(bounds_id));
    let (bx, by, bw, bh) = match stored.filter(|_| dragging) {
        Some(bounds) => bounds,
        None => screens.map_bounds(displays),
    };
    ui.ctx()
        .data_mut(|d| d.insert_temp(bounds_id, (bx, by, bw, bh)));
    let margin = 24.0_f32;
    let world_w = (bw as f32 + 2.0 * margin).max(1.0);
    let world_h = (bh as f32 + 2.0 * margin).max(1.0);
    let scale = (canvas_rect.width() / world_w)
        .min(canvas_rect.height() / world_h)
        .max(0.001);
    let offset = Vec2::new(
        canvas_rect.center().x - (bx as f32 + bw as f32 / 2.0) * scale,
        canvas_rect.center().y - (by as f32 + bh as f32 / 2.0) * scale,
    );
    let to_screen = |x: i32, y: i32| -> Pos2 {
        Pos2::new(x as f32 * scale + offset.x, y as f32 * scale + offset.y)
    };
    let to_world = |p: Pos2| -> (i32, i32) {
        (
            ((p.x - offset.x) / scale).round() as i32,
            ((p.y - offset.y) / scale).round() as i32,
        )
    };

    let pointer_world = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| canvas_rect.contains(*p))
        .map(to_world);
    let drag_id = ui.id().with("screen_map_drag");

    // --- Interaktionen ---
    if response.drag_started() {
        if let Some(pw) = pointer_world {
            let hit = hit_test(screens, pw);
            let drag = match hit {
                Some(id) => {
                    *selected = Some(id);
                    match screens.find(id) {
                        Some(s) => {
                            let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
                            let corner =
                                resize_corner_at(s.x, s.y, fw as i32, fh as i32, pw, scale);
                            match corner {
                                Some(c) => Drag::Resize { id, corner: c },
                                None => Drag::Move {
                                    id,
                                    grab: (pw.0 - s.x, pw.1 - s.y),
                                },
                            }
                        }
                        None => Drag::Rubber { start: pw },
                    }
                }
                None => Drag::Rubber { start: pw },
            };
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
        }
    }
    if response.dragged() {
        let drag = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
        // Shift deaktiviert Snap-to-Display.
        let snap_enabled = !ui.input(|i| i.modifiers.shift);
        if let (Some(drag), Some(pw)) = (drag, pointer_world) {
            match drag {
                Drag::Move { id, grab } => {
                    if let Some(s) = screens.find_mut(id) {
                        let mut nx = pw.0 - grab.0;
                        let mut ny = pw.1 - grab.1;
                        // Snap-to-Display: Nahegelegene Display-Kanten einrasten.
                        if snap_enabled {
                            let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
                            let (sx, sy) =
                                snap_to_displays(nx, ny, fw as i32, fh as i32, displays, 16);
                            nx = sx;
                            ny = sy;
                        }
                        s.x = nx;
                        s.y = ny;
                    }
                }
                Drag::Resize { id, corner } => {
                    if let Some(s) = screens.find_mut(id) {
                        apply_resize(s, corner, pw);
                    }
                }
                Drag::Rubber { .. } | Drag::Idle => {}
            }
        }
    }
    if response.drag_stopped() {
        let drag = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
        ui.ctx().data_mut(|d| d.remove_temp::<Drag>(drag_id));
        if let (Some(Drag::Rubber { start }), Some(pw)) = (drag, pointer_world) {
            let x = start.0.min(pw.0);
            let y = start.1.min(pw.1);
            let w = (start.0 - pw.0).unsigned_abs().max(16);
            let h = (start.1 - pw.1).unsigned_abs().max(16);
            let name = next_screen_name(screens);
            let screen = Screen::new(name, x, y, w, h);
            match screens.add(screen) {
                Ok(_) => {
                    if let Some(s) = screens.screens().last() {
                        *selected = Some(s.id);
                    }
                }
                Err(e) => tracing::warn!(error = %e, "could not create screen"),
            }
        }
    }
    if response.clicked() {
        if let Some(pw) = pointer_world {
            *selected = hit_test(screens, pw);
        }
    }

    // --- Displays zeichnen (Umrisse + Beschriftung) ---
    for d in displays {
        let r = Rect::from_min_size(
            to_screen(d.x, d.y),
            egui::vec2(d.width as f32 * scale, d.height as f32 * scale),
        );
        ui.painter()
            .rect_stroke(r, 2.0, Stroke::new(1.0, palette::COMPLETED));
        ui.painter().text(
            // Unten links: oben links steht die Nummer eines Screens darauf.
            r.left_bottom() + egui::vec2(4.0, -2.0),
            Align2::LEFT_BOTTOM,
            format!("{}  {}×{}", d.name, d.width, d.height),
            FontId::proportional(10.0),
            palette::COMPLETED,
        );
    }

    // --- Screens zeichnen (gefüllte Rechtecke + Name + Nummer + Farbe + Drehung) ---
    for (idx, s) in screens.screens().iter().enumerate() {
        let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
        let r = Rect::from_min_size(
            to_screen(s.x, s.y),
            egui::vec2(fw as f32 * scale, fh as f32 * scale),
        );
        let is_sel = *selected == Some(s.id);
        let screen_color = Color32::from_rgb(s.color[0], s.color[1], s.color[2]);
        let (fill, stroke) = if is_sel {
            (with_alpha(screen_color, 72), Stroke::new(2.0, screen_color))
        } else {
            (
                with_alpha(screen_color, 42),
                Stroke::new(1.0, with_alpha(screen_color, 200)),
            )
        };
        ui.painter().rect_filled(r, 2.0, fill);
        ui.painter().rect_stroke(r, 2.0, stroke);

        // Nummer oben links (#1, #2, …)
        ui.painter().text(
            r.min + egui::vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            format!("#{}", idx + 1),
            FontId::proportional(9.0),
            screen_color,
        );

        // Name zentriert
        let label = if s.rotation == ScreenRotation::Deg0 {
            s.name.clone()
        } else {
            format!("{} ({})", s.name, s.rotation.label())
        };
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            palette::TEXT,
        );

        // Drehungs-Indikator: gefülltes Dreieck innen an der Kante, die die
        // Oberseite des Canvas ist (0° oben, 90° rechts, …).
        let (dir, edge) = match s.rotation {
            ScreenRotation::Deg0 => (egui::vec2(0.0, -1.0), r.center_top()),
            ScreenRotation::Deg90 => (egui::vec2(1.0, 0.0), r.right_center()),
            ScreenRotation::Deg180 => (egui::vec2(0.0, 1.0), r.center_bottom()),
            ScreenRotation::Deg270 => (egui::vec2(-1.0, 0.0), r.left_center()),
        };
        let tip = edge - dir * 5.0;
        let base = tip - dir * 7.0;
        let side = egui::vec2(-dir.y, dir.x) * 6.0;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![tip, base + side, base - side],
            screen_color,
            Stroke::NONE,
        ));

        // Testbild-Indikator oben rechts
        if s.test_pattern {
            ui.painter().text(
                // Links neben dem Dreh-Button (16 px, oben rechts).
                r.right_top() + egui::vec2(-22.0, 2.0),
                Align2::RIGHT_TOP,
                Icon::TestPattern.glyph(),
                icons::font_id(11.0),
                palette::ACCENT,
            );
        }
    }

    // --- Dreh-Buttons über jedem Screen (kleine ⟳-Schaltfläche oben rechts) ---
    // IDs + Positionen zuerst sammeln (Borrow-Checker).
    let rotate_btns: Vec<(Uuid, Rect, [u8; 3], &'static str)> = screens
        .screens()
        .iter()
        .map(|s| {
            let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
            let r = Rect::from_min_size(
                to_screen(s.x, s.y),
                egui::vec2(fw as f32 * scale, fh as f32 * scale),
            );
            (s.id, r, s.color, s.rotation.label())
        })
        .collect();
    for (id, r, color, rot_label) in rotate_btns {
        let btn_size = 16.0_f32;
        if r.width() < btn_size * 2.0 || r.height() < btn_size * 2.0 {
            continue;
        }
        let btn_rect = Rect::from_min_size(
            r.right_top() + egui::vec2(-btn_size - 2.0, 2.0),
            egui::vec2(btn_size, btn_size),
        );
        let btn_id = ui.id().with(("screen_rotate_btn", id));
        let btn_resp = ui.interact(btn_rect, btn_id, Sense::click());
        let screen_color = Color32::from_rgb(color[0], color[1], color[2]);
        let bg = if btn_resp.hovered() {
            with_alpha(screen_color, 180)
        } else {
            with_alpha(screen_color, 100)
        };
        ui.painter().rect_filled(btn_rect, 3.0, bg);
        ui.painter().text(
            btn_rect.center(),
            Align2::CENTER_CENTER,
            Icon::ArrowClockwise.glyph(),
            icons::font_id(11.0),
            palette::TEXT,
        );
        if btn_resp.clicked() {
            if let Some(scr) = screens.find_mut(id) {
                scr.rotation = scr.rotation.next();
            }
        }
        btn_resp.on_hover_text(format!("Rotate (current: {rot_label})"));
    }

    // --- Rubber-Band-Vorschau ---
    let rubber = ui.ctx().data_mut(|d| d.get_temp::<Drag>(drag_id));
    if let (Some(Drag::Rubber { start }), Some(pw)) = (rubber, pointer_world) {
        let r = Rect::from_two_pos(to_screen(start.0, start.1), to_screen(pw.0, pw.1));
        ui.painter()
            .rect_filled(r, 2.0, with_alpha(palette::ACCENT, 30));
        ui.painter()
            .rect_stroke(r, 2.0, Stroke::new(1.5, palette::ACCENT));
    }

    // --- Hinweis ---
    if screens.is_empty() && displays.is_empty() {
        ui.painter().text(
            canvas_rect.center(),
            Align2::CENTER_CENTER,
            "No displays detected",
            FontId::proportional(11.0),
            palette::COMPLETED,
        );
    }
}

/// Eigenschaften-Panel des ausgewählten Screens (Name, pixelgenaue
/// Geometrie, Drehung, Testbild, Löschen) + zugeordnete Stages.
#[allow(clippy::too_many_arguments)]
fn draw_properties(
    ui: &mut egui::Ui,
    screens: &mut ScreenManager,
    stages: &mut crate::core::StageManager,
    engine: &EngineHandle,
    stage_screen_open: &stage_view::StageScreenOpen,
    selected: &mut Option<Uuid>,
    sel_id: Uuid,
    text_fields: &mut std::collections::HashSet<egui::Id>,
) {
    ui.add_space(size::SPACE_MEDIUM_LARGE);
    theme::section_label(ui, "Screen Properties");

    // --- Name + Geometrie + Drehung (alles über `find_mut`) ---
    let mut pattern_toggle: Option<bool> = None;
    let mut pattern_kind_changed = false;
    // Index vor dem mutablen Borrow ermitteln.
    let sel_idx = screens
        .screens()
        .iter()
        .position(|scr| scr.id == sel_id)
        .unwrap_or(0);
    if let Some(s) = screens.find_mut(sel_id) {
        theme::form_grid("screen_props_grid").show(ui, |ui| {
            theme::field_label(ui, "Name");
            ui.horizontal(|ui| {
                // Farbfleck + Nummer: dieselbe Farbe wie auf der Karte.
                let c = Color32::from_rgb(s.color[0], s.color[1], s.color[2]);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, c);
                ui.label(RichText::new(format!("#{}", sel_idx + 1)).color(palette::TEXT_DIM));
                let r = ui.text_edit_singleline(&mut s.name);
                text_fields.insert(r.id);
            });
            ui.end_row();

            theme::field_label(ui, "Position");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut s.x).speed(1.0).prefix("X "));
                ui.add(egui::DragValue::new(&mut s.y).speed(1.0).prefix("Y "));
            });
            ui.end_row();

            theme::field_label(ui, "Size");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut s.width)
                        .speed(1.0)
                        .range(1..=65535)
                        .prefix("W "),
                );
                ui.add(
                    egui::DragValue::new(&mut s.height)
                        .speed(1.0)
                        .range(1..=65535)
                        .prefix("H "),
                );
            });
            ui.end_row();

            theme::field_label(ui, "Rotation");
            ui.horizontal(|ui| {
                let mut rot = s.rotation;
                egui::ComboBox::from_id_salt("screen_rotation_combo")
                    .selected_text(rot.label())
                    .show_ui(ui, |ui| {
                        for r in ScreenRotation::ALL {
                            ui.selectable_value(&mut rot, r, r.label());
                        }
                    });
                if rot != s.rotation {
                    s.rotation = rot;
                }
                let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
                ui.label(RichText::new(format!("Footprint {fw}×{fh}")).color(palette::TEXT_DIM));
            });
            ui.end_row();
        });
        let mut pattern = s.test_pattern;
        if ui
            .checkbox(&mut pattern, "Show test pattern")
            .on_hover_text("Test pattern for aligning projectors and LED walls")
            .changed()
        {
            pattern_toggle = Some(pattern);
        }
        // Pattern-Type-Auswahl (nur sichtbar wenn Testbild aktiv).
        if s.test_pattern {
            ui.horizontal(|ui| {
                ui.label("Test pattern type");
                let mut kind = s.test_pattern_kind;
                egui::ComboBox::from_id_salt("screen_pattern_kind_combo")
                    .selected_text(kind.label())
                    .show_ui(ui, |ui| {
                        for k in crate::core::TestPatternKind::ALL {
                            ui.selectable_value(&mut kind, k, k.label());
                        }
                    });
                if kind != s.test_pattern_kind {
                    s.test_pattern_kind = kind;
                    pattern_kind_changed = true;
                }
            });
            // Kachelgröße (nur für LED-Wand-Testbild).
            if s.test_pattern_kind == crate::core::TestPatternKind::LedWall {
                ui.horizontal(|ui| {
                    ui.label("Tile size");
                    let mut changed_cs = false;
                    let resp = ui.add(
                        egui::DragValue::new(&mut s.checker_size)
                            .speed(1.0)
                            .range(8..=1024)
                            .suffix(" px"),
                    );
                    changed_cs |= resp.changed();
                    ui.label(RichText::new("(LED module size)").weak().small());
                    if changed_cs {
                        pattern_kind_changed = true;
                    }
                });
            }
        }
    }
    if let Some(enabled) = pattern_toggle {
        set_pattern(screens, engine, sel_id, enabled);
    } else if pattern_kind_changed {
        // Pattern-Typ geändert → Engine aktualisieren (Testbild neu zeichnen).
        set_pattern(screens, engine, sel_id, true);
    }

    // --- Löschen ---
    ui.horizontal(|ui| {
        let color = palette::PANIC;
        if icons::button(ui, Icon::Trash, color, egui::vec2(24.0, 24.0), true)
            .on_hover_text("Delete screen (stage assignments are removed)")
            .clicked()
        {
            // Zugeordnete Stages entziehen + Engine melden.
            let names: Vec<String> = stages
                .stages()
                .iter()
                .filter(|st| st.screen == Some(sel_id))
                .map(|st| st.name.clone())
                .collect();
            for name in names {
                if let Some(stage) = stages.find_mut(&name) {
                    stage.screen = None;
                    let stage = stage.clone();
                    let _ = engine.send(EngineCommand::ConfigureStage { stage });
                }
            }
            screens.remove(sel_id);
            *selected = None;
        }
        ui.label(
            RichText::new("Deleting also removes the stage assignments.")
                .weak()
                .small(),
        );
    });

    // --- Stages auf diesem Screen (Ausgabe öffnen/schließen) ---
    ui.add_space(size::SPACE_MEDIUM_LARGE);
    theme::section_label(ui, "Stages on this screen");
    let assigned: Vec<String> = stages
        .stages()
        .iter()
        .filter(|st| st.screen == Some(sel_id))
        .map(|st| st.name.clone())
        .collect();
    if assigned.is_empty() {
        ui.label(RichText::new("no stage assigned").weak());
        ui.label(
            RichText::new("Assign one in the Inspector: video cue, Geometry tab, Stage Settings.")
                .weak()
                .small(),
        );
    } else {
        for name in assigned {
            ui.horizontal(|ui| {
                ui.label(&name);
                let is_open = stage_screen_open
                    .lock()
                    .unwrap()
                    .get(&name)
                    .copied()
                    .unwrap_or(false);
                let color = ui.visuals().text_color();
                if !is_open {
                    if icons::button(ui, Icon::Globe, color, egui::vec2(20.0, 20.0), true)
                        .on_hover_text("Open screen output (fullscreen on this screen)")
                        .clicked()
                    {
                        stage_screen_open.lock().unwrap().insert(name.clone(), true);
                    }
                } else if icons::button(ui, Icon::Close, color, egui::vec2(20.0, 20.0), true)
                    .on_hover_text("Close screen output")
                    .clicked()
                {
                    stage_screen_open
                        .lock()
                        .unwrap()
                        .insert(name.clone(), false);
                }
            });
        }
    }
}

/// Testbild eines Screens an/aus schalten (an die Engine melden).
fn set_pattern(screens: &mut ScreenManager, engine: &EngineHandle, id: Uuid, enabled: bool) {
    if let Some(s) = screens.find_mut(id) {
        s.test_pattern = enabled;
    }
    let pattern = enabled.then(|| screens.pattern_spec_for(id)).flatten();
    let _ = engine.send(EngineCommand::SetTestPattern {
        screen: id,
        pattern,
    });
}

/// Testbilder aller Screens an/aus schalten.
fn set_all_patterns(screens: &mut ScreenManager, engine: &EngineHandle, enabled: bool) {
    for id in screens.ids() {
        set_pattern(screens, engine, id, enabled);
    }
}

/// Topmostester Screen unter dem Welt-Punkt `(x, y)` (Fußabdruck-Hitbox).
fn hit_test(screens: &ScreenManager, pw: (i32, i32)) -> Option<Uuid> {
    for s in screens.screens().iter().rev() {
        let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
        if pw.0 >= s.x && pw.0 < s.x + fw as i32 && pw.1 >= s.y && pw.1 < s.y + fh as i32 {
            return Some(s.id);
        }
    }
    None
}

/// Nächster freier Screen-Name („Screen N").
fn next_screen_name(screens: &ScreenManager) -> String {
    let mut n = screens.len() + 1;
    loop {
        let name = format!("Screen {n}");
        if screens.find_by_name(&name).is_none() {
            return name;
        }
        n += 1;
    }
}

/// Farbe mit neuem Alpha (unge-premultiplied).
fn with_alpha(c: Color32, a: u8) -> Color32 {
    c.gamma_multiply(f32::from(a) / 255.0)
}

/// Prüft, ob der Welt-Punkt `(pw)` nahe an einer der 4 Ecken des Rechtecks
/// `(x, y, w, h)` liegt (in Bildschirm-Pixeln, `scale` = Welt→Screen).
/// Rückgabe: Welche Ecke, oder `None` (Innenbereich → Move).
fn resize_corner_at(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    pw: (i32, i32),
    scale: f32,
) -> Option<ResizeCorner> {
    let tol = 12.0 / scale; // 12 Bildschirm-Pixel Toleranz in Welt-Koordinaten
    let near_left = (pw.0 - x).abs() as f32 <= tol;
    let near_right = (pw.0 - (x + w)).abs() as f32 <= tol;
    let near_top = (pw.1 - y).abs() as f32 <= tol;
    let near_bottom = (pw.1 - (y + h)).abs() as f32 <= tol;
    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(ResizeCorner::TopLeft),
        (_, true, true, _) => Some(ResizeCorner::TopRight),
        (true, _, _, true) => Some(ResizeCorner::BottomLeft),
        (_, true, _, true) => Some(ResizeCorner::BottomRight),
        _ => None,
    }
}

/// Snap-to-Display: Rastet die Position `(x, y)` mit Größe `(w, h)` an
/// nahegelegene Display-Kanten ein (Toleranz `tol` in Welt-Pixeln).
fn snap_to_displays(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    displays: &[display_info::DisplayInfo],
    tol: i32,
) -> (i32, i32) {
    let mut nx = x;
    let mut ny = y;
    for d in displays {
        let dx = d.x;
        let dy = d.y;
        let dr = d.x + d.width as i32;
        let db = d.y + d.height as i32;
        // Linke Kante des Screens an linke/rechte Display-Kante
        if (nx - dx).abs() <= tol {
            nx = dx;
        }
        if (nx - dr).abs() <= tol {
            nx = dr;
        }
        // Rechte Kante des Screens an Display-Kante
        if (nx + w - dx).abs() <= tol {
            nx = dx - w;
        }
        if (nx + w - dr).abs() <= tol {
            nx = dr - w;
        }
        // Obere/untere Kante
        if (ny - dy).abs() <= tol {
            ny = dy;
        }
        if (ny - db).abs() <= tol {
            ny = db;
        }
        if (ny + h - dy).abs() <= tol {
            ny = dy - h;
        }
        if (ny + h - db).abs() <= tol {
            ny = db - h;
        }
    }
    (nx, ny)
}

/// Wendet Resize an einer Ecke auf den Screen an (min. 16×16, Fußabdruck
/// folgt der Drehung).
fn apply_resize(s: &mut Screen, corner: ResizeCorner, pw: (i32, i32)) {
    let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
    let (mut x0, mut y0) = (s.x, s.y);
    let (mut x1, mut y1) = (s.x + fw as i32, s.y + fh as i32);
    match corner {
        ResizeCorner::BottomRight => {
            x1 = pw.0.max(x0 + 16);
            y1 = pw.1.max(y0 + 16);
        }
        ResizeCorner::BottomLeft => {
            x0 = pw.0.min(x1 - 16);
            y1 = pw.1.max(y0 + 16);
        }
        ResizeCorner::TopRight => {
            x1 = pw.0.max(x0 + 16);
            y0 = pw.1.min(y1 - 16);
        }
        ResizeCorner::TopLeft => {
            x0 = pw.0.min(x1 - 16);
            y0 = pw.1.min(y1 - 16);
        }
    }
    let new_fw = (x1 - x0).max(16) as u32;
    let new_fh = (y1 - y0).max(16) as u32;
    // Rückrechnung auf nominelle Breite/Höhe (vor Drehung).
    let (w, h) = match s.rotation {
        ScreenRotation::Deg0 | ScreenRotation::Deg180 => (new_fw, new_fh),
        ScreenRotation::Deg90 | ScreenRotation::Deg270 => (new_fh, new_fw),
    };
    s.x = x0;
    s.y = y0;
    s.width = w;
    s.height = h;
}
