//! Footer (ganz unten im Fenster): Edit/Show-Segmentschalter,
//! Warnungs-Indikator (kaputte Cues), Fehlerzeile, Cue-Anzahl sowie
//! Icon-Toggles für Sidebar/Inspector und das Zahnrad für die
//! Workspace Settings (TODO.md Phase 1).
//!
//! Im Show-Mode werden alle Edit-Funktionen deaktiviert
//! (Sicherheitsfunktion gegen versehentliche Änderungen während der Show).

use egui::RichText;

use super::app::ShowApp;
use super::icons::{self, Icon};
use super::theme::{palette, size};

/// Zeichnet den Footer (unterster Panel-Bereich, fixe Höhe).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    ui.horizontal(|ui| {
        // Edit / Show (Segment-Schalter, links im Footer).
        if ui
            .selectable_label(!app.show_mode, "Edit")
            .on_hover_text("Edit mode – editing enabled")
            .clicked()
        {
            app.set_show_mode(false);
        }
        if ui
            .selectable_label(app.show_mode, "Show")
            .on_hover_text("Show mode – editing disabled (safety)")
            .clicked()
        {
            app.set_show_mode(true);
        }
        // F1.4: Toggles für Toolbox und Masthead links im Footer.
        ui.separator();
        if panel_toggle(
            ui,
            Icon::Layout,
            if app.ui.toolbox_visible {
                "Hide Toolbox"
            } else {
                "Show Toolbox"
            },
            app.ui.toolbox_visible,
        ) {
            app.ui.toolbox_visible = !app.ui.toolbox_visible;
        }
        if panel_toggle(
            ui,
            Icon::ArrowsOut,
            if app.ui.masthead_visible {
                "Hide Masthead"
            } else {
                "Show Masthead"
            },
            app.ui.masthead_visible,
        ) {
            app.ui.masthead_visible = !app.ui.masthead_visible;
        }
        let toolbar_tooltip = if app.ui.toolbar_visible {
            "Hide Toolbar"
        } else {
            "Show Toolbar"
        };
        if panel_toggle(ui, Icon::Group, toolbar_tooltip, app.ui.toolbar_visible) {
            app.ui.toolbar_visible = !app.ui.toolbar_visible;
        }
        // Warnungs-Indikator (Anzahl kaputter Cues) – F1.4: rechts im Footer.
        let broken = app.broken_count();
        // Fehleranzeige (nicht-blockierend): Warn-Icon + gekürzte Meldung +
        // Schließen-Button (zeigt Fehler im Footer-Bereich).
        if let Some(err) = app.last_error() {
            ui.separator();
            let short: String = err.chars().take(80).collect();
            warning(ui, &short).on_hover_text(err);
            if ui
                .small_button("×")
                .on_hover_text("Dismiss error")
                .clicked()
            {
                app.dismiss_error();
            }
        }
        ui.separator();
        // Mitte: Cue-Anzahl („N cues in M lists").
        let total: usize = (0..app.carts.cart_count())
            .map(|i| app.carts.cart_cue_count(i))
            .sum();
        let lists = app.carts.cart_count();
        let noun = |n: usize, one: &str, many: &str| {
            if n == 1 {
                one.to_string()
            } else {
                many.to_string()
            }
        };
        ui.label(
            RichText::new(format!(
                "{total} {} in {lists} {}",
                noun(total, "cue", "cues"),
                noun(lists, "list", "lists")
            ))
            .color(palette::TEXT_DIM),
        );
        // Rechte Seite (in right_to_left erscheint das Erste ganz rechts):
        // Zahnrad Workspace Settings, dann Inspector- und Sidebar-Toggles
        // (Icon-Buttons, keine Text-Toggles).
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // F1.4: Warn-Icon ganz rechts (vor dem Zahnrad in right_to_left …
            // right_to_left rendert das Erste zuletzt rechts – Reihenfolge unten).
            if broken > 0 {
                let tooltip = app
                    .first_broken_problems()
                    .map(|problems| {
                        format!(
                            "{broken} broken cue(s) – click to jump to the first:\n{}",
                            problems.join("\n")
                        )
                    })
                    .unwrap_or_default();
                if warning(ui, &format!("{broken} broken"))
                    .on_hover_text(tooltip)
                    .clicked()
                {
                    app.jump_to_first_broken();
                }
                ui.separator();
            }
            let gear_tooltip = if app.ui.workspace_settings_open {
                "Close Workspace Settings"
            } else {
                "Workspace Settings"
            };
            if panel_toggle(ui, Icon::Gear, gear_tooltip, app.ui.workspace_settings_open) {
                app.ui.workspace_settings_open = !app.ui.workspace_settings_open;
            }
            // Test-Grid an/aus (neben den Einstellungen); im Show-Mode immer aus.
            let grid_on = app.ui.test_grid && !app.show_mode;
            let grid_tooltip = if app.show_mode {
                "Test grid is always off in Show Mode".to_string()
            } else if grid_on {
                "Hide test grid on all stages".to_string()
            } else {
                "Show test grid on all stages".to_string()
            };
            if ui
                .add_enabled_ui(!app.show_mode, |ui| {
                    panel_toggle(ui, Icon::GridNine, &grid_tooltip, grid_on)
                })
                .inner
            {
                app.set_test_grid(!grid_on);
            }
            ui.separator();
            let inspector_tooltip = if app.ui.inspector_visible {
                "Hide Inspector (⌘I)"
            } else {
                "Show Inspector (⌘I)"
            };
            if panel_toggle(
                ui,
                Icon::Sliders,
                inspector_tooltip,
                app.ui.inspector_visible,
            ) {
                app.ui.inspector_visible = !app.ui.inspector_visible;
            }
            let sidebar_tooltip = if app.ui.sidebar_visible {
                "Hide Sidebar (⌘L)"
            } else {
                "Show Sidebar (⌘L)"
            };
            if panel_toggle(ui, Icon::Sidebar, sidebar_tooltip, app.ui.sidebar_visible) {
                app.ui.sidebar_visible = !app.ui.sidebar_visible;
            }
            if app.show_mode {
                // Show-Mode sofort erkennbar (TODO.md Phase 6): „SHOW"-Label
                // als Pill + der Footer selbst ist eingefärbt (Panel-Frame).
                ui.separator();
                egui::Frame::none()
                    .fill(palette::PANIC.gamma_multiply(0.25))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                    .show(ui, |ui| {
                        ui.colored_label(palette::TEXT, RichText::new("SHOW").strong())
                    })
                    .inner
                    .on_hover_text("Show mode active – editing is disabled (click Edit to leave)");
            }
        });
    });
}

/// Icon-Toggle für ein Panel (Footer, rechts): aktives Panel hervorgehoben
/// (Zustand an der Farbe ablesbar).
fn panel_toggle(ui: &mut egui::Ui, icon: Icon, tooltip: &str, active: bool) -> bool {
    let color = if active {
        palette::ACCENT
    } else {
        palette::TEXT_DIM
    };
    icons::button(ui, icon, color, size::FOOTER_ICON_BUTTON, true)
        .on_hover_text(tooltip)
        .clicked()
}

/// Warn-Hinweis: rotes Icon + Text, als eine klickbare Fläche.
fn warning(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::TextStyle::Body.resolve(ui.style()),
        palette::ERROR,
    );
    let icon = 14.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(
            icon + size::SPACE_SMALL + galley.size().x,
            ui.spacing().interact_size.y,
        ),
        egui::Sense::click(),
    );
    icons::paint_at(
        ui,
        Icon::Panic,
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.center().y - icon / 2.0),
            egui::Vec2::splat(icon),
        ),
        palette::ERROR,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + icon + size::SPACE_SMALL,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette::ERROR,
    );
    response
}
