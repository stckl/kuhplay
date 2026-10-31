//! Inspector-Tab „MIDI" (MIDI-Cue, QLab 5): Nachrichtentyp (Note On/Off,
//! Control Change, Program Change), Kanal (1–16), Datenbytes. Die Engine
//! sendet die Wire-Bytes an das MIDI-Ausgabegerät (`EngineCommand::SendMidi`,
//! Gerät via `KUHPLAY_MIDI_OUT_DEVICE`).

use egui::RichText;

use crate::core::{CueType, MidiMessage, MidiMessageKind};

use super::theme;
use crate::gui::app::ShowApp;

/// Tab „MIDI" (MIDI-Cue): Typ/Kanal/Daten + Hex-Vorschau der Wire-Bytes.
pub fn draw_midi(ui: &mut egui::Ui, app: &mut ShowApp) {
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    if cue.cue_type != CueType::Midi {
        ui.label(RichText::new("MIDI only for MIDI cues").weak());
        return;
    }
    if cue.midi.is_none() {
        cue.midi = Some(MidiMessage::default());
    }
    let Some(msg) = &mut cue.midi else {
        return;
    };

    theme::form_grid("inspector_midi_grid").show(ui, |ui| {
        theme::field_label(ui, "Message");
        let mut kind = msg.kind;
        egui::ComboBox::from_id_salt("midi_kind_combo")
            .selected_text(kind.label())
            .show_ui(ui, |ui| {
                for k in MidiMessageKind::ALL {
                    ui.selectable_value(&mut kind, k, k.label());
                }
            });
        if kind != msg.kind {
            msg.kind = kind;
        }
        ui.end_row();

        theme::field_label(ui, "Channel");
        // UI zeigt 1-basiert (wie QLab), gespeichert wird 0-basiert.
        let mut ch = msg.channel + 1;
        ui.add(egui::DragValue::new(&mut ch).speed(1.0).range(1..=16));
        msg.channel = (ch - 1).clamp(0, 15);
        ui.end_row();

        let label1 = match msg.kind {
            MidiMessageKind::NoteOn | MidiMessageKind::NoteOff => "Note",
            MidiMessageKind::ControlChange => "Controller",
            MidiMessageKind::ProgramChange => "Program",
        };
        theme::field_label(ui, label1);
        let mut d1 = msg.data1;
        ui.add(egui::DragValue::new(&mut d1).speed(1.0).range(0..=127));
        msg.data1 = d1;
        ui.end_row();

        if !matches!(msg.kind, MidiMessageKind::ProgramChange) {
            let label2 = match msg.kind {
                MidiMessageKind::NoteOn | MidiMessageKind::NoteOff => "Velocity",
                MidiMessageKind::ControlChange => "Value",
                MidiMessageKind::ProgramChange => unreachable!(),
            };
            theme::field_label(ui, label2);
            let mut d2 = msg.data2;
            ui.add(egui::DragValue::new(&mut d2).speed(1.0).range(0..=127));
            msg.data2 = d2;
            ui.end_row();
        }

        theme::field_label(ui, "Bytes");
        ui.label(
            RichText::new(
                msg.to_bytes()
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
            .monospace()
            .color(theme::palette::TEXT_DIM),
        );
        ui.end_row();
    });

    ui.add_space(theme::size::SPACE_SMALL);
    ui.label(RichText::new(format!("Sends: {}", msg.describe())).color(theme::palette::TEXT_DIM));
}
