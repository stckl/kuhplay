//! Tab „Triggers": Timecode-In (Chasing) für den selektierten Cue.
//!
//! MIDI-, OSC- und Network-Trigger sind geplant (siehe `docs/BACKLOG.md`).

use std::time::Duration;

use egui::RichText;

use crate::core::clock_trigger::{ClockTrigger, ALL_DAYS, FRI, MON, SAT, SUN, THU, TUE, WED};
use crate::core::{format_timecode, CueTrigger, Fps, Timecode};
use crate::engine::EngineCommand;

use crate::gui::app::ShowApp;
use crate::gui::theme;

/// Tab „Triggers".
pub fn draw_triggers(ui: &mut egui::Ui, app: &mut ShowApp) {
    let engine = &app.engine;
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
    ui.separator();
    ui.label(
        RichText::new("MIDI, OSC and network triggers are not available yet.")
            .weak()
            .small(),
    );

    // --- Cue-Trigger (Hotkey/OSC/MIDI/Timecode/Clock) ---
    ui.separator();
    ui.label(RichText::new("Triggers (start cue regardless of playhead)").strong());
    let mut remove: Option<usize> = None;
    for (i, t) in cue.triggers.iter().enumerate() {
        ui.horizontal(|ui| {
            let label = match t {
                CueTrigger::Hotkey(h) => format!("Hotkey {h}"),
                CueTrigger::Osc(a) => format!("OSC {a}"),
                CueTrigger::Midi { channel, note } => format!("MIDI ch{channel} note{note}"),
                CueTrigger::Timecode(tc) => {
                    format!("Timecode {}", format_timecode(tc.to_duration()))
                }
                CueTrigger::Clock(ct) => format!("Clock {}", ct.label()),
            };
            ui.label(label);
            if ui.small_button("✕").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        cue.triggers.remove(i);
    }
    ui.horizontal(|ui| {
        let mut kind = 0u8; // 0 Hotkey, 1 OSC, 2 MIDI, 3 Timecode, 4 Clock
        egui::ComboBox::from_id_salt("trigger_kind")
            .selected_text(["Hotkey", "OSC", "MIDI", "Timecode", "Clock"][kind as usize])
            .show_ui(ui, |ui| {
                for (k, l) in [
                    (0, "Hotkey"),
                    (1, "OSC"),
                    (2, "MIDI"),
                    (3, "Timecode"),
                    (4, "Clock"),
                ] {
                    ui.selectable_value(&mut kind, k, l);
                }
            });
        if ui.button("Add trigger").clicked() {
            let t = match kind {
                0 => CueTrigger::Hotkey("J".to_string()),
                1 => CueTrigger::Osc("/kuhplay/cue".to_string()),
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
    // Clock-Editor für Clock-Trigger
    for t in cue.triggers.iter_mut() {
        if let CueTrigger::Clock(ct) = t {
            theme::form_grid("trigger_clock_grid").show(ui, |ui| {
                theme::field_label(ui, "Time");
                let (mut h, mut m, mut s) = ct.time;
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut h).range(0..=23));
                    ui.add(egui::DragValue::new(&mut m).range(0..=59));
                    ui.add(egui::DragValue::new(&mut s).range(0..=59));
                });
                ct.time = (h, m, s);
                ui.end_row();
                theme::field_label(ui, "Date (one-shot)");
                let mut has_date = ct.date.is_some();
                if ui.checkbox(&mut has_date, "einmalig am").changed() {
                    ct.date = if has_date { Some((2026, 11, 2)) } else { None };
                }
                if let Some((y, mo, d)) = &mut ct.date {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(y).range(2026..=2100));
                        ui.add(egui::DragValue::new(mo).range(1..=12));
                        ui.add(egui::DragValue::new(d).range(1..=31));
                    });
                }
                ui.end_row();
                if ct.date.is_none() {
                    theme::field_label(ui, "Weekdays");
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
                            let mut on = wd & bit != 0 || wd == ALL_DAYS;
                            if ui.checkbox(&mut on, name).changed() {
                                if wd == ALL_DAYS {
                                    wd = 0;
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
                    ui.end_row();
                }
            });
        }
    }
}
