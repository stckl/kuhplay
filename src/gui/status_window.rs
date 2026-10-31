//! Workspace Status Window (QLab 5 „Tools → Workspace Status"): kompaktes,
//! immeraktuelles Übersichtsfenster: Show-Name, Edit/Show-Mode, aktiver Cart,
//! Playhead/Standby, laufende Cues mit Restzeit, kaputte Cues, Engine.
//! Reiner Lesezugriff, aktualisiert sich jeden Frame.

use egui::{Context, RichText};

use super::app::ShowApp;
use super::theme::palette;
use super::viewport_window::{self, WindowSpec};

/// Zeichnet das Status-Fenster, solange `app.ui.status_window_open`.
pub fn draw(ctx: &Context, app: &mut ShowApp) {
    if !app.ui.status_window_open {
        return;
    }
    let mut open = true;
    viewport_window::draw(
        ctx,
        &WindowSpec::new(
            "kuhplay-status",
            "Workspace Status",
            [380.0, 420.0],
            [300.0, 260.0],
        ),
        &mut open,
        app,
        |_ctx, ui, app| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mode = if app.show_mode { "SHOW" } else { "EDIT" };
                ui.label(RichText::new(&app.show_name).size(16.0).strong());
                ui.label(
                    RichText::new(format!("Mode: {mode}   Engine: {}", app.engine_name()))
                        .color(palette::TEXT_DIM),
                );
                ui.separator();

                let active_cart = app.carts.active_index();
                let cart_name = app.carts.cart(active_cart).name().to_string();
                ui.label(format!("Active cart: {active_cart} ({cart_name})"));
                let playhead = app.carts.cursor();
                ui.label(format!("Playhead: #{playhead}"));
                match app.carts.active().standby() {
                    Some(c) => {
                        ui.label(format!("Standby: Q{} • {}", c.number, c.name));
                    }
                    None => {
                        ui.label(RichText::new("Standby: —").color(palette::TEXT_FAINT));
                    }
                }
                ui.separator();

                ui.label(RichText::new("Running cues").strong());
                let running: Vec<(String, String, Option<std::time::Duration>)> = {
                    let list = app.carts.active();
                    list.cues()
                        .iter()
                        .filter(|c| c.status.is_active())
                        .map(|c| {
                            let remaining = app
                                .elapsed_of(c)
                                .filter(|_| c.duration > std::time::Duration::ZERO)
                                .map(|e| c.duration.saturating_sub(e));
                            (c.number.clone(), c.name.clone(), remaining)
                        })
                        .collect()
                };
                if running.is_empty() {
                    ui.label(RichText::new("none").color(palette::TEXT_FAINT));
                }
                for (number, name, remaining) in running {
                    let time = remaining
                        .map(|r| format!("−{}", crate::core::format_timecode(r)))
                        .unwrap_or_default();
                    ui.label(format!("Q{number}  {name}  {time}"));
                }
                ui.separator();

                let broken = app.broken_count();
                if broken > 0 {
                    ui.label(
                        RichText::new(format!("⚠ {broken} broken cue(s)")).color(palette::PANIC),
                    );
                } else {
                    ui.label(RichText::new("No broken cues").color(palette::TEXT_DIM));
                }
                let total = app.carts.active().len();
                ui.label(
                    RichText::new(format!("{total} cues in active list")).color(palette::TEXT_DIM),
                );
            });
        },
    );
    if !open {
        app.ui.status_window_open = false;
    }
}
