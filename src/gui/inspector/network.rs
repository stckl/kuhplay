//! Inspector-Tab „Network" (Network-Cue, QLab 5): Protokoll (UDP/TCP),
//! Ziel `host:port` und die zu sendende Nachricht. Der Cue feuert die
//! Nachricht asynchron in der Engine (`EngineCommand::SendNetwork`).

use egui::RichText;

use crate::core::{CueType, NetworkMessage, NetworkProtocol};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Network" (Network-Cue): Protokoll/Ziel/Nachricht.
pub fn draw_network(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Network {
        ui.label(RichText::new("Network only for network cues").weak());
        return;
    }
    if cue.network.is_none() {
        cue.network = Some(NetworkMessage::default());
    }
    let Some(msg) = &mut cue.network else {
        return;
    };

    theme::form_grid("inspector_network_grid").show(ui, |ui| {
        theme::field_label(ui, "Protocol");
        egui::ComboBox::from_id_salt("network_protocol_combo")
            .selected_text(msg.protocol.label())
            .show_ui(ui, |ui| {
                for p in NetworkProtocol::ALL {
                    ui.selectable_value(&mut msg.protocol, p, p.label());
                }
            });
        ui.end_row();

        theme::field_label(ui, "Host");
        let mut host = msg.host.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut host).desired_width(220.0));
        app.text_field_ids.insert(r.id);
        if r.changed() {
            msg.host = host;
        }
        ui.end_row();

        theme::field_label(ui, "Port");
        ui.add(
            egui::DragValue::new(&mut msg.port)
                .speed(1.0)
                .range(1..=65535),
        );
        ui.end_row();
    });

    ui.add_space(theme::size::SPACE_MEDIUM);
    theme::field_label(ui, "Message");
    let mut body = msg.message.clone();
    let response = ui.add(
        egui::TextEdit::multiline(&mut body)
            .desired_width(f32::INFINITY)
            .desired_rows(4)
            .font(egui::TextStyle::Monospace),
    );
    app.text_field_ids.insert(response.id);
    if response.changed() {
        msg.message = body;
    }
    if msg.validate().is_err() {
        ui.label(
            RichText::new("Host and port required (cue would be broken)")
                .color(theme::palette::ERROR),
        );
    }
}
