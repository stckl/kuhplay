//! Inspector (unteres Panel).
//!
//! ## Phase 4 (TODO.md): typabhängige Tabs + Kopfzeile
//!
//! * **Kopfzeile** über den Tabs: Typ-Icon, Cue-Nummer und Name des
//!   selektierten Cues (bei Mehrfachauswahl „N cues selected").
//! * **Typabhängige Tabs** (siehe [`tabs_for`]): alle Typen
//!   „Basics" + „Triggers"; Audio/Video zusätzlich „Time & Loops" (+ „Audio
//!   Levels", Video auch „Geometry"); Fade „Curve"; HTML5 „Geometry" +
//!   „Source"; Diashow „Geometry" + „Slides"; HTML-Data „Data". Tabs, die
//!   es in KuhPlay nicht gibt, werden weggelassen (nicht leer angezeigt).
//! * „Basics" ist **zweispaltig** (links Q#/Name/Target/Pre Wait/
//!   Duration/Post Wait/Continue, rechts Armed/Color/Notes/Type) –
//!   siehe `basics.rs`.
//! * **Zeitfelder** zeigen/akzeptieren Timecode `MM:SS.mmm` (wie die
//!   Cue-Liste, siehe [`time_field`]); DragValue-Ziehen bleibt erhalten.
//! * Aufgeteilt in Submodule (keine Datei über ~400 Zeilen):
//!   `basics.rs`, `triggers.rs`, `time_loops.rs`, `geometry.rs`,
//!   `audio_levels.rs`, `curve.rs`, `media.rs`.
//!
//! ## Datei-Dialoge
//!
//! `rfd` blockiert beim Öffnen kurz den UI-Thread (auf macOS laufen native
//! Dialoge zwingend auf dem Main-Thread und betreiben eine eigene modale
//! Event-Loop). Das Fenster bleibt während des Dialogs fokussiert; die
//! Engine arbeitet auf ihrem eigenen Thread unbeeindruckt weiter.
//!
//! **Show-Mode:** Der Inspector bleibt sichtbar, ist aber read-only
//! (`ui.disable()` in [`draw`] – Sicherheitsfunktion).

use std::path::PathBuf;
use std::time::Duration;

use egui::RichText;

use crate::core::{format_timecode, parse_timecode, CueType};

use super::app::{InspectorTab, ShowApp};
use super::icons::{self, Icon};
use super::theme;

mod audio_levels;
mod basics;
mod companion;
mod curve;
mod geometry;
mod light;
mod media;
mod midi;
mod network;
mod script;
mod text;
mod time_loops;
mod timecode_tab;
mod triggers;

/// Zeichnet den Inspector: Kopfzeile, typabhängige Tab-Leiste, Tab-Inhalt.
///
/// Ohne Selektion nur der zentrierte Hinweis „No cue selected" – keine
/// Kopfzeile, keine Tabs (docs/UI.md §5.5).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    // Höhe unabhängig vom Inhalt: sonst schrumpft egui das Panel auf den Tab
    // und die Cue-Liste springt beim Wechsel von Tab oder Cue.
    ui.set_min_height(ui.available_height());
    if app.carts.selection().is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(theme::size::SPACE_LARGE * 3.0);
            ui.label(
                RichText::new("No cue selected")
                    .color(theme::palette::TEXT_FAINT)
                    .size(16.0),
            );
        });
        return;
    }

    draw_header(ui, app);
    ui.add_space(theme::size::SPACE_SMALL);

    let cue_type = app.carts.selected().map(|c| c.cue_type);
    let tabs = tabs_for(cue_type);
    // Persistierter Tab evtl. für diesen Cue-Typ nicht verfügbar → erster Tab.
    let mut selected = if tabs.contains(&app.ui.inspector_tab) {
        app.ui.inspector_tab
    } else {
        tabs[0]
    };
    let labelled: Vec<(InspectorTab, String)> =
        tabs.iter().map(|t| (*t, t.label().to_string())).collect();
    theme::tab_bar(ui, &mut selected, &labelled);
    if selected != app.ui.inspector_tab {
        app.ui.inspector_tab = selected;
    }

    if app.show_mode {
        // Show-Mode: Inspector read-only (Sicherheitsfunktion).
        ui.disable();
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match selected {
            InspectorTab::Basics => basics::draw_basics(ui, app),
            InspectorTab::Triggers => triggers::draw_triggers(ui, app),
            InspectorTab::TimeLoops => time_loops::draw_time_loops(ui, app),
            InspectorTab::Geometry => geometry::draw_geometry(ui, app),
            InspectorTab::AudioLevels => audio_levels::draw_audio_levels(ui, app),
            InspectorTab::Curve => curve::draw_curve(ui, app),
            InspectorTab::Source => media::draw_source(ui, app),
            InspectorTab::Slides => media::draw_slides(ui, app),
            InspectorTab::Data => media::draw_data(ui, app),
            InspectorTab::Text => text::draw_text(ui, app),
            InspectorTab::Network => network::draw_network(ui, app),
            InspectorTab::Midi => midi::draw_midi(ui, app),
            InspectorTab::Light => light::draw_light(ui, app),
            InspectorTab::Companion => companion::draw_companion(ui, app),
            InspectorTab::Timecode => timecode_tab::draw_timecode(ui, app),
            InspectorTab::ScriptTab => script::draw_script(ui, app),
        });
}

/// Kopfzeile: Typ-Icon + Cue-Nummer + Name (bei Mehrfachauswahl
/// „N cues selected").
fn draw_header(ui: &mut egui::Ui, app: &ShowApp) {
    let count = app.carts.selection().len();
    ui.horizontal(|ui| {
        if count > 1 {
            icons::draw(ui, Icon::Cards, 18.0, theme::palette::TEXT_DIM);
            ui.label(
                RichText::new(format!("{count} cues selected"))
                    .size(15.0)
                    .strong(),
            );
        } else if let Some(cue) = app.carts.selected() {
            icons::draw(
                ui,
                Icon::for_cue_type(cue.cue_type),
                18.0,
                theme::palette::TEXT_DIM,
            );
            ui.label(
                RichText::new(format!("{} • {}", cue.number, cue.name))
                    .size(15.0)
                    .strong(),
            );
        }
    });
}

/// Typabhängige Tab-Liste (TODO.md Phase 4). Tabs, die es
/// in KuhPlay (noch) nicht gibt, fehlen statt leer angezeigt zu werden.
pub fn tabs_for(cue_type: Option<CueType>) -> &'static [InspectorTab] {
    use InspectorTab::*;
    match cue_type {
        Some(CueType::Audio) => &[Basics, Triggers, TimeLoops, AudioLevels],
        Some(CueType::Video) => &[Basics, Triggers, TimeLoops, Geometry, AudioLevels],
        Some(CueType::Fade) => &[Basics, Triggers, Curve],
        Some(CueType::Html) => &[Basics, Triggers, Geometry, Source],
        Some(CueType::SlideShow) => &[Basics, Triggers, Geometry, Slides],
        Some(CueType::HtmlData) => &[Basics, Triggers, Data],
        Some(CueType::Text) => &[Basics, Triggers, Geometry, Text],
        Some(CueType::Network) => &[Basics, Triggers, Network],
        Some(CueType::Midi) => &[Basics, Triggers, Midi],
        Some(CueType::Light) => &[Basics, Triggers, Light],
        Some(CueType::Companion) => &[Basics, Triggers, Companion],
        Some(CueType::Timecode) => &[Basics, Triggers, Timecode],
        Some(CueType::Script) => &[Basics, Triggers, ScriptTab],
        // Group, Stop, Script, Control, Wait, Memo, keine Auswahl.
        _ => &[Basics, Triggers],
    }
}

/// Zeitfeld: Anzeige und Eingabe als Timecode `MM:SS.mmm` (wie die
/// Cue-Liste). Akzeptiert `MM:SS.mmm`, `HH:MM:SS.mmm` und nackte
/// Sekunden (siehe [`parse_timecode`]); ungültige Eingabe wird verworfen.
/// DragValue-Ziehen bleibt erhalten (Schrittweite in Sekunden).
pub fn time_field(ui: &mut egui::Ui, value: &mut Duration) -> egui::Response {
    let mut secs = value.as_secs_f64();
    let response = ui.add_sized(
        egui::vec2(theme::size::FORM_NUMBER_WIDTH, ui.spacing().interact_size.y),
        egui::DragValue::new(&mut secs)
            .speed(0.05)
            .range(0.0..=86_400.0)
            .custom_formatter(|v, _precision| format_timecode(Duration::from_secs_f64(v)))
            .custom_parser(|s| parse_timecode(s).map(|d| d.as_secs_f64())),
    );
    if response.changed() {
        *value = Duration::from_secs_f64(secs.max(0.0));
    }
    response
}

/// Nativer Datei-Dialog (rfd) für Medien-Dateien (Audio/Video).
/// Rückgabe: Pfad als String; `None` bei Abbruch.
pub(super) fn pick_media_file() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter(
            "Media",
            &[
                "wav", "mp3", "aac", "flac", "ogg", "m4a", "mp4", "mov", "mkv", "webm", "avi",
            ],
        )
        .set_title("Select media file")
        .pick_file()
        .map(|p| p.display().to_string())
}

/// Nativer Multi-Select-Dialog (rfd) für Slide-Bilder.
pub(super) fn pick_image_files() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
        .set_title("Select slide images")
        .pick_files()
        .unwrap_or_default()
}

/// Nativer Datei-Dialog (rfd) für HTML-Dateien (HTML5-Cue, Quelle „File").
pub(super) fn pick_html_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("HTML", &["html", "htm"])
        .set_title("Select HTML file")
        .pick_file()
}

/// Meldet eine Stage-Änderung an die Engine (`ConfigureStage`).
pub(super) fn push_stage(
    engine: &crate::engine::EngineHandle,
    stages: &crate::core::StageManager,
    name: &str,
) {
    if let Some(stage) = stages.find(name) {
        let _ = engine.send(crate::engine::EngineCommand::ConfigureStage {
            stage: stage.clone(),
        });
    }
}
