//! Inspector-Tab „Companion" (Companion-Cue): Host/Port der Companion-HTTP-
//! API, Seite/Zeile/Spalte des Buttons und Aktion (press/down/up). Die Engine
//! feuert `POST /api/location/<page>/<row>/<column>/<action>` asynchron
//! (`EngineCommand::SendHttpPost`).

use egui::RichText;

use crate::core::{CompanionAction, CompanionTrigger, CueType, COMPANION_DEFAULT_PORT};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Companion" (Companion-Cue): Ziel + Button + Aktion.
pub fn draw_companion(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Companion {
        ui.label(RichText::new("Companion only for companion cues").weak());
        return;
    }
    if cue.companion.is_none() {
        cue.companion = Some(CompanionTrigger::default());
    }
    let Some(trig) = &mut cue.companion else {
        return;
    };

    theme::form_grid("inspector_companion_grid").show(ui, |ui| {
        theme::field_label(ui, "Host");
        let mut host = trig.host.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut host).desired_width(220.0));
        app.text_field_ids.insert(r.id);
        if r.changed() {
            trig.host = host;
        }
        ui.end_row();

        theme::field_label(ui, "Port");
        ui.add(
            egui::DragValue::new(&mut trig.port)
                .speed(1.0)
                .range(1..=65535),
        );
        ui.end_row();

        theme::field_label(ui, "Page");
        ui.add(
            egui::DragValue::new(&mut trig.page)
                .speed(1.0)
                .range(1..=99),
        );
        ui.end_row();

        theme::field_label(ui, "Row / Col");
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut trig.row)
                    .speed(1.0)
                    .range(0..=63)
                    .prefix("r"),
            );
            ui.add(
                egui::DragValue::new(&mut trig.column)
                    .speed(1.0)
                    .range(0..=63)
                    .prefix("c"),
            );
        });
        ui.end_row();

        theme::field_label(ui, "Action");
        egui::ComboBox::from_id_salt("companion_action_combo")
            .selected_text(trig.action.label())
            .show_ui(ui, |ui| {
                for a in CompanionAction::ALL {
                    ui.selectable_value(&mut trig.action, a, a.label());
                }
            });
        ui.end_row();

        theme::field_label(ui, "Request");
        ui.label(
            RichText::new(format!(
                "POST http://{}:{}{}",
                trig.host,
                trig.port,
                trig.path()
            ))
            .monospace()
            .small()
            .color(theme::palette::TEXT_DIM),
        );
        ui.end_row();
    });

    ui.add_space(theme::size::SPACE_SMALL);
    ui.label(
        RichText::new(format!(
            "Companion Web-UI port (default {}): Settings → Companion HTTP remote control",
            COMPANION_DEFAULT_PORT
        ))
        .small()
        .color(theme::palette::TEXT_FAINT),
    );
    if trig.validate().is_err() {
        ui.label(
            RichText::new("Host and port required (cue would be broken)")
                .color(theme::palette::ERROR),
        );
    }
}
