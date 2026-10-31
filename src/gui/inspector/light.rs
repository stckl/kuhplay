//! Inspector-Tab „Light" (Light-Cue, QLab 5 Minimal-Variante): DMX-Universe,
//! Startkanal, Kanalwerte und Ziel-Host. Die Engine baut das Art-Net-Paket
//! (`LightMessage::to_artnet`) und sendet es per UDP an Port 6454
//! (`EngineCommand::SendDmx`).

use egui::RichText;

use crate::core::{CueType, LightMessage, LightProtocol, DMX_CHANNELS};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „Light" (Light-Cue): Universe/Kanal/Werte/Host + Paket-Vorschau.
pub fn draw_light(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Light {
        ui.label(RichText::new("Light only for light cues").weak());
        return;
    }
    if cue.light.is_none() {
        cue.light = Some(LightMessage::default());
    }
    let Some(light) = &mut cue.light else {
        return;
    };

    theme::form_grid("inspector_light_grid").show(ui, |ui| {
        theme::field_label(ui, "Protocol");
        let mut protocol = light.protocol;
        egui::ComboBox::from_id_salt("light_protocol_combo")
            .selected_text(protocol.label())
            .show_ui(ui, |ui| {
                for p in LightProtocol::ALL {
                    ui.selectable_value(&mut light.protocol, p, p.label());
                }
            });
        let _ = &mut protocol;
        ui.end_row();

        theme::field_label(ui, "Universe");
        ui.add(
            egui::DragValue::new(&mut light.universe)
                .speed(1.0)
                .range(0..=0x7FFF),
        );
        ui.end_row();

        theme::field_label(ui, "Start Channel");
        ui.add(
            egui::DragValue::new(&mut light.start_channel)
                .speed(1.0)
                .range(1..=DMX_CHANNELS as u16),
        );
        ui.end_row();

        theme::field_label(ui, "Host");
        let mut host = light.host.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut host).desired_width(220.0));
        app.text_field_ids.insert(r.id);
        if r.changed() {
            light.host = host;
        }
        ui.end_row();

        theme::field_label(ui, "Values");
        // Kanalwerte als Text (dezimal oder 0x-Hex, Komma/Leerzeichen-getrennt).
        let mut text = light.format_values();
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(320.0));
        app.text_field_ids.insert(r.id);
        if r.changed() {
            match LightMessage::parse_values(&text) {
                Ok(values) => {
                    light.values = values;
                }
                Err(err) => {
                    ui.label(RichText::new(err).small().color(theme::palette::ERROR));
                }
            }
        }
        ui.end_row();

        theme::field_label(ui, "Packet");
        let (packet, port) = light.packet();
        ui.label(
            RichText::new(format!(
                "{} → {}:{} ({} Bytes, {} Kanäle ab Ch {})",
                light.protocol.label(),
                light.host,
                port,
                packet.len(),
                light.values.len(),
                light.start_channel
            ))
            .monospace()
            .small()
            .color(theme::palette::TEXT_DIM),
        );
        ui.end_row();
    });

    if light.validate().is_err() {
        ui.label(
            RichText::new("Universe 0-32767, Startkanal 1-512, Werte bis Kanal 512, Host nötig")
                .small()
                .color(theme::palette::ERROR),
        );
    }
}
