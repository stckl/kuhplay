//! Inspector-Tab „Script" (Script-Cue): JavaScript-
//! Editor. Das Script läuft beim GO in einer eigenen WebView-Instanz
//! (unsichtbar, ohne Stage – `EngineCommand::RunScript`); nach dem Laden
//! meldet die Engine `Finished`. Daten-Übergabe an laufende HTML5-Cues über
//! `window.kuhplay.onShowControlData(...)` bzw. `push_json` – siehe
//! docs/HTML-Templates.md.

use egui::RichText;

use crate::core::CueType;

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Script" (Script-Cue): mehrzeiliger JS-Editor.
pub fn draw_script(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Script {
        ui.label(RichText::new("Script only for script cues").weak());
        return;
    }

    theme::field_label(ui, "JavaScript");
    let mut body = cue.script.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut body)
            .desired_width(f32::INFINITY)
            .desired_rows(14)
            .font(egui::TextStyle::Monospace),
    );
    app.text_field_ids.insert(response.id);
    if response.changed() {
        cue.script = body;
    }

    ui.add_space(theme::size::SPACE_SMALL);
    ui.label(
        RichText::new(
            "Runs in its own WebView when the cue fires (no stage, invisible). \
             Talk to a running HTML5 cue via window.kuhplay.onShowControlData(payload) \
             or eval_js – see docs/HTML-Templates.md.",
        )
        .small()
        .color(theme::palette::TEXT_DIM),
    );
    if cue.script.trim().is_empty() {
        ui.label(RichText::new("Empty script (cue would be broken)").color(theme::palette::ERROR));
    }
}
