//! Sidebar (QLab 5): Tabs „Lists & Carts" und „Active Cues"
//! (docs/UI.md §5.4). Der globale Transport sitzt im Masthead.
//!
//! Screens/Stages liegen nicht mehr hier, sondern in den Workspace Settings
//! → Video (QLab 5, TODO.md Phase 1).
//!
//! ## Active Cues (QLab 5)
//!
//! Jede laufende Cue-Zeile zeigt:
//! * links einen Pause/Resume-Button,
//! * rechts einen Panic-Button (nur dieser Cue),
//! * einen Fortschrittsbalken über die ganze Zeilenbreite
//!   (grün = playing, gelb = paused),
//! * darunter Name und Restzeit.

use std::time::Duration;

use egui::RichText;
use uuid::Uuid;

use crate::core::{format_timecode, CueStatus, CueType};

use super::app::{ShowApp, SidebarTab};
use super::icons::{self, Icon};
use super::theme::{self, palette, size};

/// Zeichnet die Sidebar (rechtes SidePanel): Tab-Leiste + Inhalt
/// (docs/UI.md §5.4). Der globale Transport sitzt im Masthead.
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    let running = app.carts.running_count();
    theme::tab_bar(
        ui,
        &mut app.ui.sidebar_tab,
        &[
            (SidebarTab::ListsCarts, "Lists & Carts".to_string()),
            (SidebarTab::ActiveCues, format!("Active Cues ({running})")),
        ],
    );
    match app.ui.sidebar_tab {
        SidebarTab::ListsCarts => draw_lists_carts(ui, app),
        SidebarTab::ActiveCues => draw_active_cues(ui, app),
    }
}

/// Tab „Lists & Carts": Listenzeilen „Name … Anzahl" + „New Cart".
fn draw_lists_carts(ui: &mut egui::Ui, app: &mut ShowApp) {
    let count = app.carts.cart_count();
    let active = app.carts.active_index();
    for index in 0..count {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
        let fill = if index == active {
            palette::SELECTION
        } else if response.hovered() {
            palette::RAISED
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 5.0, fill);
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + size::SPACE_MEDIUM, rect.center().y - 7.0),
            egui::vec2(14.0, 14.0),
        );
        icons::paint_at(ui, Icon::Cards, icon_rect, palette::TEXT_DIM);
        ui.painter().text(
            egui::pos2(icon_rect.right() + size::SPACE_MEDIUM, rect.center().y),
            egui::Align2::LEFT_CENTER,
            app.carts.cart_name(index),
            egui::FontId::proportional(13.5),
            palette::TEXT,
        );
        ui.painter().text(
            egui::pos2(rect.right() - size::SPACE_MEDIUM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            app.carts.cart_cue_count(index).to_string(),
            egui::FontId::monospace(12.0),
            palette::TEXT_DIM,
        );
        if response
            .on_hover_text(format!("{} cues", app.carts.cart_cue_count(index)))
            .clicked()
        {
            let _ = app.carts.set_active(index);
        }
    }
    ui.add_space(size::SPACE_MEDIUM);
    if ui
        .add(egui::Button::new(RichText::new("+ New Cart").color(palette::TEXT_DIM)).frame(false))
        .clicked()
    {
        let n = app.carts.cart_count() + 1;
        app.carts.add_cart(format!("Cart {n}"));
    }
}

/// Snapshot einer laufenden Cue-Zeile (vermeidet Borrow-Konflikte zwischen
/// Zeilenaufbau und Engine-Kommandos).
struct ActiveCueRow {
    id: Uuid,
    number: String,
    name: String,
    cue_type: CueType,
    status: CueStatus,
    duration: Duration,
    elapsed: Option<Duration>,
    /// Medien-Dauer (aus Engine-Event), falls bekannt – Basis fürs Spulen.
    media: Option<Duration>,
}

/// Tab „Active Cues": laufende Cues mit Einzel-Steuerung.
fn draw_active_cues(ui: &mut egui::Ui, app: &mut ShowApp) {
    let mut rows: Vec<ActiveCueRow> = Vec::new();
    for cue in app.carts.active().cues() {
        if cue.status.is_active() {
            rows.push(ActiveCueRow {
                id: cue.id,
                number: cue.number.clone(),
                name: cue.name.clone(),
                cue_type: cue.cue_type,
                status: cue.status,
                duration: cue.duration,
                elapsed: app.elapsed_of(cue),
                media: app.media_duration_of(cue),
            });
        }
    }
    if rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(size::SPACE_LARGE);
            ui.label(RichText::new("No active cues").color(palette::TEXT_FAINT));
        });
        return;
    }
    for row in &rows {
        draw_active_cue_row(ui, app, row);
    }
}

/// Eine Zeile: [Pause/Resume] Name + Zeit [■ Panic], Balken im Hintergrund.
fn draw_active_cue_row(ui: &mut egui::Ui, app: &mut ShowApp, row: &ActiveCueRow) {
    let ActiveCueRow {
        id,
        number,
        name,
        cue_type,
        status,
        duration,
        elapsed,
        media,
    } = row;
    let (id, cue_type, status, duration, elapsed, media) =
        (*id, *cue_type, *status, *duration, *elapsed, *media);
    let progress = match (elapsed, duration) {
        (Some(e), d) if d > Duration::ZERO => {
            (e.as_secs_f64() / d.as_secs_f64()).clamp(0.0, 1.0) as f32
        }
        // Loops / unbekannte Dauer: Balken bleibt leer (QLab zeigt hier
        // ebenfalls keinen prozentualen Fortschritt).
        _ => 0.0,
    };
    let color = if status == CueStatus::Paused {
        palette::PAUSED
    } else {
        palette::PLAYING
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 48.0),
        egui::Sense::click_and_drag(),
    );
    // Fortschrittsbalken im Hintergrund (QLab: über die ganze Zeilenbreite).
    ui.painter().rect_filled(rect, 5.0, palette::RAISED);
    let bar_width = rect.width() * progress;
    if bar_width > 0.0 {
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(bar_width, rect.height()));
        ui.painter()
            .rect_filled(bar, 5.0, theme::progress_fill(color));
    }
    // Farb-Streifen am linken Rand (QLab: Statusfarbe als linker Balken).
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
        0.0,
        color,
    );
    // Widgets über dem Balken.
    // Volle Zeilenhoehe nutzbar (nur horizontal eingerückt): Die Uhr darf
    // nicht unten abgeschnitten werden.
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(6.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row_ui.horizontal(|ui| {
        // Links: Pause/Resume-Button (Vektor-Icon, TODO.md Phase 6:
        // min. 28 px hoch – gut klickbar).
        match status {
            CueStatus::Playing => {
                if icons::button(ui, Icon::Pause, color, size::ACTIVE_CUE_BUTTON, true)
                    .on_hover_text("Pause this cue")
                    .clicked()
                {
                    let commands = app.pause_running(id);
                    app.dispatch(commands);
                }
            }
            CueStatus::Paused => {
                if icons::button(ui, Icon::Resume, color, size::ACTIVE_CUE_BUTTON, true)
                    .on_hover_text("Resume this cue")
                    .clicked()
                {
                    let commands = app.resume_running(id);
                    app.dispatch(commands);
                }
            }
            _ => {}
        }
        ui.add_space(size::SPACE_SMALL);
        // Typ-Icon (QLab: jede Active-Cue-Zeile mit Typ-Icon).
        icons::draw(ui, Icon::for_cue_type(cue_type), 16.0, palette::TEXT_DIM)
            .on_hover_text(format!("{} cue", cue_type.label()));
        ui.add_space(size::SPACE_SMALL);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(format!("{}  {}", number, name)).strong());
            // Restzeit (docs/UI.md §5.4).
            let time = match (elapsed, duration) {
                (Some(e), d) if d > Duration::ZERO => {
                    let remaining = d.saturating_sub(e);
                    // Nur Restzeit: „elapsed / −remaining" passt nicht in
                    // eine Zeile der Sidebar.
                    format!("−{}", format_timecode(remaining))
                }
                (Some(e), _) => format_timecode(e),
                _ => String::new(),
            };
            ui.label(
                RichText::new(time)
                    .monospace()
                    .size(11.0)
                    .color(palette::TEXT_DIM),
            );
        });
        // Rechts: Stop-Button für diesen einzelnen Cue (QLab „Panic this cue").
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(
                ui,
                Icon::Stop,
                palette::PANIC,
                size::ACTIVE_CUE_BUTTON,
                true,
            )
            .on_hover_text("Stop this cue now")
            .clicked()
            {
                let commands = app.stop_running(id);
                app.dispatch(commands);
            }
        });
    });
    // Spulen (QLab „Scrub"): Ziehen/Klick auf der Zeile springt im Medium an
    // die Stelle – Bild und Ton bekommen im selben Engine-Command den Seek.
    let total = if duration > Duration::ZERO {
        Some(duration)
    } else {
        media
    };
    if let Some(total) = total.filter(|t| *t > Duration::ZERO) {
        let interact = (response.dragged() || response.clicked())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| rect.contains(p));
        if interact {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let frac = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                app.seek_running(id, total.mul_f64(f64::from(frac)));
            }
        }
        response.on_hover_text("Drag to scrub / click to jump to this position");
    }
    ui.add_space(size::SPACE_SMALL);
}
