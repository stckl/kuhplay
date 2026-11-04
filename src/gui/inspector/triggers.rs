//! Tab „Triggers": Timecode-In (Chasing) und Start-Trigger (Hotkey, OSC,
//! MIDI, Timecode, Clock) für den selektierten Cue.

use std::time::Duration;

use egui::RichText;

use crate::core::clock_trigger::{
    today_local, ClockTrigger, ALL_DAYS, FRI, MON, SAT, SUN, THU, TUE, WED,
};
use crate::core::{format_timecode, CueTrigger, Fps, Timecode};
use crate::engine::EngineCommand;

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Triggers".
pub fn draw_triggers(ui: &mut egui::Ui, app: &mut ShowApp) {
    let engine = &app.engine;
    let text_field_ids = &mut app.text_field_ids;
    let Some(cue) = app.carts.selected_mut() else {
        ui.label(RichText::new("No cue selected").weak());
        return;
    };
    theme::form_grid("cue_triggers_grid").show(ui, |ui| {
        theme::field_label(ui, "Timecode In");
        let mut chasing = cue.timecode_in.is_some();
        if ui
            .checkbox(&mut chasing, "cue follows external timecode (LTC/MTC)")
            .changed()
        {
            if chasing {
                let tc = cue
                    .timecode_in
                    .unwrap_or_else(|| Timecode::from_duration(Duration::ZERO, Fps::Fps25));
                cue.timecode_in = Some(tc);
            } else {
                cue.timecode_in = None;
            }
        }
        ui.end_row();

        if let Some(tc) = &mut cue.timecode_in {
            theme::field_label(ui, "FPS");
            let mut fps = tc.fps;
            egui::ComboBox::from_id_salt("timecode_fps_combo")
                .selected_text(fps.label())
                .show_ui(ui, |ui| {
                    for f in Fps::ALL {
                        ui.selectable_value(&mut fps, f, f.label());
                    }
                });
            if fps != tc.fps {
                tc.fps = fps;
            }
            ui.end_row();

            theme::field_label(ui, "Offset");
            ui.label(format_timecode(tc.to_duration()));
            ui.end_row();

            theme::field_label(ui, "Chase");
            if ui
                .button("Start Timecode Chasing")
                .on_hover_text("Send EngineCommand::ChaseTimecode to the engine")
                .clicked()
            {
                let command = EngineCommand::ChaseTimecode {
                    cue_id: cue.id,
                    timecode_in: Some(*tc),
                };
                if let Err(err) = engine.send(command) {
                    tracing::warn!(%err, "inspector: chase_timecode send failed");
                }
            }
            ui.end_row();
        }
    });
    // --- Cue-Trigger (Hotkey/OSC/MIDI/Timecode/Clock) ---
    ui.separator();
    ui.label(RichText::new("Triggers (start cue regardless of playhead)").strong());
    let mut remove: Option<usize> = None;
    for (i, t) in cue.triggers.iter_mut().enumerate() {
        ui.push_id(("cue_trigger", i), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .small_button("✕")
                    .on_hover_text("Remove trigger")
                    .clicked()
                {
                    remove = Some(i);
                }
                draw_trigger_editor(ui, t, text_field_ids);
            });
        });
    }
    if let Some(i) = remove {
        cue.triggers.remove(i);
    }
    ui.horizontal(|ui| {
        // Auswahl über Frames merken (sonst springt die Combo sofort zurück).
        let kind_id = ui.id().with("trigger_kind");
        let mut kind: u8 = ui.data(|d| d.get_temp(kind_id)).unwrap_or(0);
        const KINDS: [&str; 5] = ["Hotkey", "OSC", "MIDI", "Timecode", "Clock"];
        egui::ComboBox::from_id_salt("trigger_kind")
            .selected_text(KINDS[kind as usize])
            .show_ui(ui, |ui| {
                for (k, l) in KINDS.iter().enumerate() {
                    ui.selectable_value(&mut kind, k as u8, *l);
                }
            });
        ui.data_mut(|d| d.insert_temp(kind_id, kind));
        if ui.button("Add trigger").clicked() {
            let t = match kind {
                0 => CueTrigger::Hotkey("J".to_string()),
                1 => CueTrigger::Osc(format!("/kuhplay/cue/{}", cue.number)),
                2 => CueTrigger::Midi {
                    channel: 1,
                    note: 60,
                },
                3 => CueTrigger::Timecode(Timecode::from_duration(Duration::ZERO, Fps::Fps25)),
                _ => CueTrigger::Clock(ClockTrigger::daily(8, 0, 0)),
            };
            cue.triggers.push(t);
        }
    });
}

/// Editor für einen Cue-Trigger (eine Zeile, Clock mehrzeilig).
fn draw_trigger_editor(
    ui: &mut egui::Ui,
    trigger: &mut CueTrigger,
    text_field_ids: &mut std::collections::HashSet<egui::Id>,
) {
    match trigger {
        CueTrigger::Hotkey(key) => {
            ui.label("Hotkey");
            let r = ui.add(egui::TextEdit::singleline(key).desired_width(60.0));
            text_field_ids.insert(r.id);
            r.on_hover_text("egui key name, e.g. J, F5, Num1");
        }
        CueTrigger::Osc(address) => {
            ui.label("OSC");
            let r = ui.add(egui::TextEdit::singleline(address).desired_width(180.0));
            text_field_ids.insert(r.id);
            r.on_hover_text("Full OSC address, e.g. /kuhplay/cue/5");
        }
        CueTrigger::Midi { channel, note } => {
            ui.label("MIDI ch");
            ui.add(egui::DragValue::new(channel).range(1..=16));
            ui.label("note");
            ui.add(egui::DragValue::new(note).range(0..=127));
        }
        CueTrigger::Timecode(tc) => {
            ui.label("Timecode");
            let mut at = tc.to_duration();
            if super::time_field(ui, &mut at).changed() {
                *tc = Timecode::from_duration(at, tc.fps);
            }
            let mut fps = tc.fps;
            egui::ComboBox::from_id_salt("trigger_tc_fps")
                .selected_text(fps.label())
                .show_ui(ui, |ui| {
                    for f in Fps::ALL {
                        ui.selectable_value(&mut fps, f, f.label());
                    }
                });
            if fps != tc.fps {
                *tc = Timecode::from_duration(tc.to_duration(), fps);
            }
        }
        CueTrigger::Clock(ct) => {
            ui.vertical(|ui| draw_clock_editor(ui, ct));
        }
    }
}

/// Editor für einen Clock-Trigger: Uhrzeit, optional Datum, sonst Wochentage.
fn draw_clock_editor(ui: &mut egui::Ui, ct: &mut ClockTrigger) {
    ui.horizontal(|ui| {
        ui.label("Clock")
            .on_hover_text("Local time of this computer");
        let (mut h, mut m, mut s) = ct.time;
        ui.add(egui::DragValue::new(&mut h).range(0..=23));
        ui.label(":");
        ui.add(egui::DragValue::new(&mut m).range(0..=59));
        ui.label(":");
        ui.add(egui::DragValue::new(&mut s).range(0..=59));
        ct.time = (h, m, s);
        let mut has_date = ct.date.is_some();
        if ui.checkbox(&mut has_date, "once on").changed() {
            ct.date = has_date.then(today_local);
        }
        if let Some((y, mo, d)) = &mut ct.date {
            ui.add(egui::DragValue::new(y).range(2000..=2100));
            ui.add(egui::DragValue::new(mo).range(1..=12));
            ui.add(egui::DragValue::new(d).range(1..=31));
        }
    });
    if ct.date.is_none() {
        ui.horizontal(|ui| {
            let mut wd = ct.weekdays;
            for (name, bit) in [
                ("Mo", MON),
                ("Di", TUE),
                ("Mi", WED),
                ("Do", THU),
                ("Fr", FRI),
                ("Sa", SAT),
                ("So", SUN),
            ] {
                let mut on = wd & bit != 0 || wd == 0;
                if ui.checkbox(&mut on, name).changed() {
                    if wd == 0 {
                        wd = ALL_DAYS;
                    }
                    if on {
                        wd |= bit;
                    } else {
                        wd &= !bit;
                    }
                    ct.weekdays = wd;
                }
            }
            if ui.small_button("daily").clicked() {
                ct.weekdays = ALL_DAYS;
            }
        });
    }
}
