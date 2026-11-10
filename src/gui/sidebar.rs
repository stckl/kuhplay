//! Sidebar: Tabs „Active Cues" und „Lists & Carts"
//! (docs/UI.md §5.4). Der globale Transport sitzt im Masthead.
//!
//! Screens/Stages liegen nicht mehr hier, sondern in den Workspace Settings
//! → Video (docs/TODO-offen.md Phase 1).
//!
//! ## Active Cues
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

use crate::core::{Cue, CueStatus, CueType, GroupMode, LoopMode};

use super::app::{ShowApp, SidebarTab};
use super::icons::{self, Icon};
use super::theme::{self, palette, size};

/// Zeichnet die Sidebar (rechtes SidePanel): Tab-Leiste + Inhalt
/// (docs/UI.md §5.4). Der globale Transport sitzt ganz oben.
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    draw_transport(ui, app);
    ui.add_space(size::SPACE_MEDIUM);
    // Zeilen einmal sammeln: Tab-Zähler und Liste zeigen dasselbe
    // (inkl. der gerade spielenden Kinder laufender Gruppen).
    let rows = active_cue_rows(app);
    let running = rows.len();
    theme::tab_bar(
        ui,
        &mut app.ui.sidebar_tab,
        &[
            (SidebarTab::ActiveCues, format!("Active Cues ({running})")),
            (SidebarTab::ListsCarts, "Lists & Carts".to_string()),
        ],
    );
    match app.ui.sidebar_tab {
        SidebarTab::ListsCarts => draw_lists_carts(ui, app),
        SidebarTab::ActiveCues => draw_active_cues(ui, app, &rows),
    }
}

/// Globaler Transport: Pause All, Resume All, Reset und Panic.
fn draw_transport(ui: &mut egui::Ui, app: &mut ShowApp) {
    let any_with = |app: &ShowApp, status: CueStatus| {
        app.carts.active().cues().iter().any(|c| c.status == status)
    };
    let (any_running, any_paused) = (
        any_with(app, CueStatus::Playing),
        any_with(app, CueStatus::Paused),
    );
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::SPACE_SMALL;
        if icons::button(
            ui,
            Icon::Pause,
            if any_running {
                palette::PAUSED
            } else {
                palette::TEXT
            },
            size::MASTHEAD_ICON_BUTTON,
            true,
        )
        .on_hover_text("Pause All – pause all running cues")
        .clicked()
        {
            app.pause_all();
        }
        if icons::button(
            ui,
            Icon::Resume,
            if any_paused {
                palette::PLAYING
            } else {
                palette::TEXT
            },
            size::MASTHEAD_ICON_BUTTON,
            true,
        )
        .on_hover_text("Resume All – resume all paused cues")
        .clicked()
        {
            app.resume_all();
        }
        if icons::button(
            ui,
            Icon::Reset,
            palette::TEXT,
            size::MASTHEAD_ICON_BUTTON,
            true,
        )
        .on_hover_text("Reset All – stop all cues, playhead to start")
        .clicked()
        {
            app.reset_all();
        }
        if panic_button(ui)
            .on_hover_text("Panic – fade out and stop everything (Esc, twice: stop now)")
            .clicked()
        {
            app.panic();
        }
    });
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
    /// Beendet, das letzte Bild steht noch: Hier kann man es wegnehmen.
    holding: bool,
    id: Uuid,
    number: String,
    name: String,
    cue_type: CueType,
    status: CueStatus,
    duration: Duration,
    elapsed: Option<Duration>,
    /// Medien-Dauer (aus Engine-Event), falls bekannt – Basis fürs Spulen.
    media: Option<Duration>,
    /// Gesamtlaufzeit, wenn es ein Ende gibt – Basis für Restzeit und Balken.
    /// Bei Loops: Länge eines Durchlaufs (`elapsed` zählt dann darin).
    total: Option<Duration>,
    /// Loop-Cue: Restzeit/Balken gelten für den aktuellen Durchlauf (↻).
    looping: bool,
    /// Laufende Sequential/Shuffle-Gruppe: „eins weiter"-Button.
    skippable: bool,
    /// Verschachtelungstiefe (Kind einer laufenden Gruppe = 1, eingerückt).
    depth: usize,
}

/// Gesamtlaufzeit eines laufenden Cues: eingetragene Dauer, sonst die des Mediums.
/// Endlosschleifen haben kein Ende.
fn total_runtime(
    duration: Duration,
    media: Option<Duration>,
    loop_mode: LoopMode,
) -> Option<Duration> {
    let base = Some(duration)
        .filter(|d| *d > Duration::ZERO)
        .or(media.filter(|d| *d > Duration::ZERO))?;
    match loop_mode {
        LoopMode::Once => Some(base),
        LoopMode::Count(n) => Some(base.saturating_mul(n)),
        LoopMode::Infinite => None,
    }
}

/// Loop-Cue (n-mal/endlos) mit bekannter Durchlauf-Länge: (Länge, gelaufene
/// Zeit im aktuellen Durchlauf). Restzeit, Balken und Spulen beziehen sich
/// dann auf das Medium, nicht auf den ganzen Cue.
fn loop_window(
    duration: Duration,
    media: Option<Duration>,
    loop_mode: LoopMode,
    elapsed: Option<Duration>,
) -> Option<(Duration, Option<Duration>)> {
    if loop_mode == LoopMode::Once {
        return None;
    }
    let base = Some(duration)
        .filter(|d| *d > Duration::ZERO)
        .or(media.filter(|d| *d > Duration::ZERO))?;
    let within = elapsed.map(|e| Duration::from_nanos((e.as_nanos() % base.as_nanos()) as u64));
    Some((base, within))
}

/// Spielzeit eines In/Out- oder Slice-Plans.
fn planned_runtime(cue: &crate::core::Cue) -> Option<Duration> {
    (!cue.slices.is_empty()).then(|| {
        cue.slices
            .iter()
            .map(|slice| slice.duration().saturating_mul(slice.play_count.max(1)))
            .sum()
    })
}

/// Gut lesbare Zeit für Active Cues: `HH:MM:SS` bzw. `MM:SS` (immer
/// zweistellig), unter 10 s mit Zehnteln. Ganze Sekunden werden aufgerundet,
/// damit ein Countdown erst am Ende auf 00:00 steht.
fn countdown_text(d: Duration) -> String {
    if d < Duration::from_secs(10) {
        let tenths = d.as_millis().div_ceil(100);
        return format!("00:{:02}.{}", tenths / 10, tenths % 10);
    }
    let secs = d.as_millis().div_ceil(1000) as u64;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Laufende Cues des aktiven Carts; unter einer laufenden Gruppe ihre
/// gerade aktiven Kinder (eingerückt).
fn active_cue_rows(app: &ShowApp) -> Vec<ActiveCueRow> {
    let mut rows = Vec::new();
    collect_active_rows(app, app.carts.active().cues(), 0, &mut rows);
    rows
}

fn collect_active_rows(app: &ShowApp, cues: &[Cue], depth: usize, rows: &mut Vec<ActiveCueRow>) {
    for cue in cues {
        let holding = !cue.status.is_active() && app.is_holding(cue.id);
        if !cue.status.is_active() && !holding {
            continue;
        }
        let group = cue.cue_type == CueType::Group;
        let media = app.media_duration_of(cue);
        let elapsed = if group {
            app.group_elapsed(cue)
        } else {
            app.elapsed_of(cue)
        };
        let window = (cue.slices.is_empty() && !group)
            .then(|| loop_window(cue.duration, media, cue.loop_mode, elapsed))
            .flatten();
        rows.push(ActiveCueRow {
            holding,
            id: cue.id,
            number: cue.number.clone(),
            name: cue.name.clone(),
            cue_type: cue.cue_type,
            status: cue.status,
            duration: cue.duration,
            elapsed: window.map_or(elapsed, |(_, within)| within),
            media,
            total: match window {
                // Gruppe: Gesamtlaufzeit (Restzeit und Balken für die ganze Gruppe).
                _ if group => app.group_runtime(cue),
                Some((base, _)) => Some(base),
                None => planned_runtime(cue)
                    .or_else(|| total_runtime(cue.duration, media, cue.loop_mode)),
            },
            looping: window.is_some() && !group,
            skippable: group
                && matches!(cue.group_mode, GroupMode::Sequential | GroupMode::Shuffle)
                && matches!(cue.status, CueStatus::Playing | CueStatus::Paused),
            depth,
        });
        if cue.cue_type == CueType::Group {
            collect_active_rows(app, &cue.children, depth + 1, rows);
        }
    }
}

/// Tab „Active Cues": laufende Cues mit Einzel-Steuerung.
fn draw_active_cues(ui: &mut egui::Ui, app: &mut ShowApp, rows: &[ActiveCueRow]) {
    if rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(size::SPACE_LARGE);
            ui.label(RichText::new("No active cues").color(palette::TEXT_FAINT));
        });
        return;
    }
    for row in rows {
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
        total: runtime,
        holding,
        looping,
        skippable,
        depth,
    } = row;
    let holding = *holding;
    let (id, cue_type, status, duration, elapsed, media, runtime) = (
        *id, *cue_type, *status, *duration, *elapsed, *media, *runtime,
    );
    let progress = match (elapsed, runtime) {
        (Some(e), Some(t)) if t > Duration::ZERO => {
            (e.as_secs_f64() / t.as_secs_f64()).clamp(0.0, 1.0) as f32
        }
        // Loops / unbekannte Dauer: Balken bleibt leer
        // (kein prozentualer Fortschritt).
        _ => 0.0,
    };
    let color = if status == CueStatus::Paused {
        palette::PAUSED
    } else {
        palette::PLAYING
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 58.0),
        egui::Sense::click_and_drag(),
    );
    // Kinder laufender Gruppen eingerückt unter ihrer Gruppe.
    let rect = rect.with_min_x((rect.left() + 14.0 * *depth as f32).min(rect.right()));
    // Fortschrittsbalken im Hintergrund (über die ganze Zeilenbreite).
    ui.painter().rect_filled(rect, 5.0, palette::RAISED);
    let bar_width = rect.width() * progress;
    if bar_width > 0.0 {
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(bar_width, rect.height()));
        ui.painter()
            .rect_filled(bar, 5.0, theme::progress_fill(color));
    }
    // Farb-Streifen am linken Rand (Statusfarbe als linker Balken).
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
        0.0,
        color,
    );
    // Widgets über dem Balken, alle an der Zeilenmitte ausgerichtet. Der Text
    // bekommt den Platz zwischen den Icons und wird bei Bedarf gekürzt.
    let inner = rect.shrink2(egui::vec2(6.0, 0.0));
    let button = size::ACTIVE_CUE_BUTTON;
    let center_y = rect.center().y;
    let button_rect =
        |center_x: f32| egui::Rect::from_center_size(egui::pos2(center_x, center_y), button);
    let child = |ui: &mut egui::Ui, r: egui::Rect, layout: egui::Layout| {
        ui.new_child(egui::UiBuilder::new().max_rect(r).layout(layout))
    };
    let centered = egui::Layout::left_to_right(egui::Align::Center);

    // Links: Pause/Resume-Button (Vektor-Icon, docs/TODO-offen.md Phase 6:
    // min. 28 px hoch – gut klickbar).
    let mut left_ui = child(ui, button_rect(inner.left() + button.x / 2.0), centered);
    match status {
        CueStatus::Playing => {
            if icons::button(&mut left_ui, Icon::Pause, color, button, true)
                .on_hover_text("Pause this cue")
                .clicked()
            {
                let commands = app.pause_running(id);
                app.dispatch(commands);
            }
        }
        CueStatus::Paused => {
            if icons::button(&mut left_ui, Icon::Resume, color, button, true)
                .on_hover_text("Resume this cue")
                .clicked()
            {
                let commands = app.resume_running(id);
                app.dispatch(commands);
            }
        }
        _ => {}
    }

    // Typ-Icon (jede Active-Cue-Zeile mit Typ-Icon).
    let icon_x = inner.left() + button.x + size::SPACE_SMALL;
    let icon_rect =
        egui::Rect::from_min_size(egui::pos2(icon_x, center_y - 8.0), egui::vec2(16.0, 16.0));
    let mut icon_ui = child(ui, icon_rect, centered);
    icons::draw(
        &mut icon_ui,
        Icon::for_cue_type(cue_type),
        16.0,
        palette::TEXT_DIM,
    )
    .on_hover_text(format!("{} cue", cue_type.label()));

    // Rechts: Stop-Button für diesen einzelnen Cue („Panic this cue").
    let mut stop_ui = child(ui, button_rect(inner.right() - button.x / 2.0), centered);
    if icons::button(&mut stop_ui, Icon::Stop, palette::PANIC, button, true)
        .on_hover_text("Stop this cue now")
        .clicked()
    {
        let commands = if holding {
            vec![crate::engine::EngineCommand::Stop { cue_id: id }]
        } else {
            app.stop_running(id)
        };
        app.dispatch(commands);
    }

    // Gruppe: „eins weiter" (nächstes Kind) links neben dem Stop-Button.
    let mut right_edge = inner.right() - button.x;
    if *skippable {
        let mut skip_ui = child(
            ui,
            button_rect(right_edge - size::SPACE_SMALL - button.x / 2.0),
            centered,
        );
        if icons::button(
            &mut skip_ui,
            Icon::SkipForward,
            palette::TEXT_DIM,
            button,
            true,
        )
        .on_hover_text("Skip to the next cue in this group")
        .clicked()
        {
            let commands = app.skip_in_group(id);
            app.dispatch(commands);
        }
        right_edge -= size::SPACE_SMALL + button.x;
    }

    // Groß die Restzeit – das Wichtigste im Lauf.
    let text_left = icon_rect.right() + size::SPACE_SMALL;
    let text_right = (right_edge - size::SPACE_SMALL).max(text_left);
    let remaining = match (elapsed, runtime) {
        (Some(e), Some(t)) if !holding => Some(t.saturating_sub(e)),
        _ => None,
    };
    let (big, big_color) = if holding {
        ("HOLD".to_string(), palette::TEXT_DIM)
    } else if let Some(left) = remaining {
        let color = if left <= Duration::from_secs(5) {
            palette::PANIC
        } else if left <= Duration::from_secs(10) {
            palette::ACCENT
        } else {
            palette::TEXT
        };
        (format!("−{}", countdown_text(left)), color)
    } else {
        // Ohne bekanntes Ende: gelaufene Zeit (gedimmt, kein Countdown).
        (
            countdown_text(elapsed.unwrap_or_default()),
            palette::TEXT_DIM,
        )
    };
    let big_text =
        RichText::new(big)
            .monospace()
            .size(20.0)
            .strong()
            .color(if status == CueStatus::Paused {
                palette::PAUSED
            } else {
                big_color
            });
    // Oben der Name über die ganze Breite, darunter linksbündig groß nur
    // die Restzeit (bei Loops mit ↻).
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.top() + 4.0),
        egui::pos2(text_right, rect.top() + 22.0),
    );
    let lower = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.top() + 22.0),
        egui::pos2(text_right, rect.bottom() - 2.0),
    );
    let mut name_ui = child(ui, name_rect, centered);
    name_ui.add(
        egui::Label::new(RichText::new(format!("{}  {}", number, name)).strong())
            .selectable(false)
            .truncate(),
    );
    let mut big_ui = child(ui, lower, centered);
    if *looping && !holding {
        big_ui.label(RichText::new("↻").size(16.0).color(palette::TEXT_DIM));
    }
    big_ui.add(egui::Label::new(big_text).selectable(false).truncate());
    // Spulen („Scrub"): Ziehen/Klick auf der Zeile springt im Medium an
    // die Stelle – Bild und Ton bekommen im selben Engine-Command den Seek.
    let total = if duration > Duration::ZERO {
        Some(duration)
    } else {
        media
    };
    if let Some(total) = total.filter(|t| *t > Duration::ZERO && !holding) {
        let interact = (response.dragged() || response.clicked())
            && ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| rect.contains(p));
        if interact {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let frac = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                // Throttle beim Ziehen (150 ms / 2 %), Klick & Loslassen immer.
                let force = response.clicked() || response.drag_stopped();
                if app.scrub_throttle(id, frac, force) {
                    app.seek_running(id, total.mul_f64(f64::from(frac)));
                }
            }
        }
        response.on_hover_text("Drag to scrub / click to jump to this position");
    }
    ui.add_space(size::SPACE_SMALL);
}

/// Panic-Button: Icon + „Panic", roter Rahmen, keine rote Fläche
/// (docs/UI.md §6.1 – Primär-Buttons sind umrandet).
fn panic_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(96.0, 40.0), egui::Sense::click());
    let fill = if response.hovered() {
        palette::PANIC.gamma_multiply(0.18)
    } else {
        palette::RAISED
    };
    ui.painter()
        .rect(rect, 8.0, fill, egui::Stroke::new(2.0_f32, palette::PANIC));
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 14.0, rect.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    icons::paint_at(ui, Icon::Panic, icon_rect, palette::PANIC);
    ui.painter().text(
        egui::pos2(icon_rect.right() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Panic",
        egui::FontId::proportional(15.0),
        palette::TEXT,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod active_time_tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn video_without_own_duration_counts_down_from_its_media_length() {
        let total = total_runtime(Duration::ZERO, Some(SECOND * 100), LoopMode::Once);
        assert_eq!(total, Some(SECOND * 100));
        assert_eq!(countdown_text(total.unwrap() - SECOND * 40), "01:00");
    }

    #[test]
    fn loops_count_down_within_the_current_pass() {
        let media = Some(SECOND * 10);
        assert_eq!(
            loop_window(Duration::ZERO, media, LoopMode::Once, Some(SECOND)),
            None
        );
        let (base, within) =
            loop_window(Duration::ZERO, media, LoopMode::Infinite, Some(SECOND * 23)).unwrap();
        assert_eq!(
            (base, within),
            (SECOND * 10, Some(SECOND * 3)),
            "3. Durchlauf, 3 s"
        );
        assert_eq!(countdown_text(base - within.unwrap()), "00:07.0");
        // Ohne bekannte Länge kein Fenster (dann zählt die Uhr nur hoch).
        assert_eq!(
            loop_window(Duration::ZERO, None, LoopMode::Count(3), Some(SECOND)),
            None
        );
    }

    #[test]
    fn own_duration_wins_and_loops_multiply() {
        let once = total_runtime(SECOND * 5, Some(SECOND * 100), LoopMode::Once);
        assert_eq!(once, Some(SECOND * 5));
        let twice = total_runtime(Duration::ZERO, Some(SECOND * 10), LoopMode::Count(2));
        assert_eq!(twice, Some(SECOND * 20));
    }

    #[test]
    fn no_known_end_counts_up() {
        let endless = total_runtime(SECOND, Some(SECOND), LoopMode::Infinite);
        assert_eq!(endless, None);
        assert_eq!(total_runtime(Duration::ZERO, None, LoopMode::Once), None);
        assert_eq!(countdown_text(SECOND * 7), "00:07.0");
    }

    #[test]
    fn countdown_rounds_up_and_shows_hours() {
        assert_eq!(countdown_text(Duration::from_millis(59_200)), "01:00");
        assert_eq!(countdown_text(Duration::from_millis(9_850)), "00:09.9");
        assert_eq!(countdown_text(SECOND * 3725), "01:02:05");
        assert_eq!(countdown_text(Duration::ZERO), "00:00.0");
    }
}
