//! Gemeinsamer Helfer für „echte OS-Fenster" (egui-Viewports) statt Overlays
//! im Hauptfenster (Nutzerwunsch: Workspace Settings & Popups als eigene
//! Fenster).
//!
//! [`draw`] ruft jeden Frame [`egui::Context::show_viewport_immediate`] auf,
//! solange das Fenster geöffnet sein soll. Auf dem Desktop (winit/eframe)
//! entsteht damit ein eigenes OS-Fenster mit Titelzeile und Close-Button; im
//! eingebetteten Fallback (Headless-Tests ohne Backend, Web) wird ein normales
//! [`egui::Window`] gezeichnet, damit bestehende Tests/Assertions stabil bleiben.
//!
//! Schließen (OS-Close-Button, Esc im Fenster, Close im Overlay) setzt das
//! übergebene `open`-Flag auf `false`; der Aufrufer hält den Zustand.

use egui::{Context, ViewportBuilder, ViewportClass, ViewportId};

/// Fenstereigenschaften für [`draw`] (Viewport und Overlay-Fallback).
pub struct WindowSpec {
    /// Stabiler Viewport-Schlüssel (einmalig pro Fenstertyp).
    pub id: &'static str,
    pub title: String,
    pub default_size: [f32; 2],
    pub min_size: [f32; 2],
    pub resizable: bool,
    /// Zeigt im Fenster-Footer ein „SHOW MODE“-Badge (F1.6).
    pub show_mode_badge: bool,
}

impl WindowSpec {
    pub fn new(
        id: &'static str,
        title: impl Into<String>,
        default_size: [f32; 2],
        min_size: [f32; 2],
    ) -> Self {
        Self {
            id,
            title: title.into(),
            default_size,
            min_size,
            resizable: true,
            show_mode_badge: false,
        }
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// „SHOW MODE“-Badge im Footer des Nebenfensters (F1.6).
    pub fn show_mode_badge(mut self, on: bool) -> Self {
        self.show_mode_badge = on;
        self
    }
}

/// Footer-Badge „SHOW MODE“ für Nebenfenster im Show-Mode (F1.6).
fn show_mode_footer(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("SHOW MODE")
                .strong()
                .color(crate::gui::theme::palette::PANIC),
        );
    });
}

/// Zeichnet `content` in einem eigenen OS-Fenster, solange `*open` true ist.
/// `content` bekommt den Context, das Ui und `app` (für zustandsbehaftete
/// Fenster direkt `&mut ShowApp`).
pub fn draw<A>(
    ctx: &Context,
    spec: &WindowSpec,
    open: &mut bool,
    app: &mut A,
    content: impl FnOnce(&Context, &mut egui::Ui, &mut A),
) {
    if !*open {
        return;
    }
    let viewport_id = ViewportId::from_hash_of(spec.id);
    let mut close = false;
    let mut content = Some(content);
    ctx.show_viewport_immediate(
        viewport_id,
        ViewportBuilder::default()
            .with_title(&spec.title)
            .with_inner_size(spec.default_size)
            .with_min_inner_size(spec.min_size)
            .with_resizable(spec.resizable),
        |ctx, class| {
            let content = content.take();
            if class == ViewportClass::Embedded {
                // Fallback (Tests/Web): normales Overlay-Fenster.
                let mut still_open = true;
                egui::Window::new(&spec.title)
                    .open(&mut still_open)
                    .resizable(spec.resizable)
                    .default_size(spec.default_size)
                    .min_size(spec.min_size)
                    .constrain_to(ctx.screen_rect())
                    .show(ctx, |ui| {
                        if spec.show_mode_badge {
                            show_mode_footer(ui);
                        }
                        if let Some(content) = content {
                            content(ctx, ui, app);
                        }
                    });
                if !still_open {
                    close = true;
                }
            } else {
                // Echtes OS-Fenster: Close-Button des Fensters oder Esc.
                if ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape))
                {
                    close = true;
                }
                if spec.show_mode_badge {
                    egui::TopBottomPanel::bottom("show-mode-badge").show(ctx, |ui| {
                        show_mode_footer(ui);
                    });
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(content) = content {
                        content(ctx, ui, app);
                    }
                });
            }
        },
    );
    if close {
        *open = false;
    }
}
