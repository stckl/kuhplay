//! „Workspace Settings"-Fenster (QLab 5: Workspace → Workspace Settings).
//!
//! Eigenes [`egui::Window`] mit linker Navigationsliste (General, Audio,
//! Video, Network, MIDI, Key Map). Die Stages/Screens-Verwaltung (QLab 5:
//! „Video → Stages/Screens") lebt auf der Seite „Video" – der Screen-Editor
//! ([`super::screen_editor`]) ist hierher aus der Sidebar gewandert
//! (TODO.md Phase 1). Network und MIDI zeigen die aktuelle
//! Konfiguration nur an, geändert wird sie über Umgebungsvariablen.
//!
//! Geöffnet wird das Fenster über das Zahnrad im Footer bzw. das Menü
//! „Workspace → Workspace Settings …".

use serde::{Deserialize, Serialize};

use super::app::ShowApp;
use super::app::{AudioDeviceList, OutputMode};
use super::icons::{self, Icon};
use super::theme;
use crate::engine::audio_backend::{default_output_name, output_devices};

/// Seite der Workspace Settings (linke Navigation, QLab-5-Reihenfolge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SettingsPage {
    /// Show-Name, Show Mode, Audition Mode.
    #[default]
    General,
    /// Audio: Auswahl des Ausgabegeräts.
    Audio,
    /// Video: Stages & Screens (Screen-Editor).
    Video,
    /// Netzwerk: Remote-Konfiguration.
    Network,
    /// MIDI: Status und Noten-Belegung.
    Midi,
    /// Key Map: Tastenkürzel.
    KeyMap,
}

impl SettingsPage {
    /// Alle Seiten (Navigations-Reihenfolge).
    pub const ALL: [SettingsPage; 6] = [
        SettingsPage::General,
        SettingsPage::Audio,
        SettingsPage::Video,
        SettingsPage::Network,
        SettingsPage::Midi,
        SettingsPage::KeyMap,
    ];

    /// Anzeige-Label (QLab-Begriffe, Englisch).
    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Audio => "Audio",
            SettingsPage::Video => "Video",
            SettingsPage::Network => "Network",
            SettingsPage::Midi => "MIDI",
            SettingsPage::KeyMap => "Key Map",
        }
    }
}

/// Unterseite von „Video".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoTab {
    #[default]
    Screens,
    Stages,
}

/// Mindestbreite des Seiteninhalts rechts der Navigation.
const CONTENT_MIN_WIDTH: f32 = 640.0;

/// Zeichnet das „Workspace Settings"-Fenster (falls in [`ShowApp::ui`]
/// geöffnet). Ruft der Aufrufer jedes Frame (nach den Panels).
pub fn draw(ctx: &egui::Context, app: &mut ShowApp) {
    if !app.ui.workspace_settings_open {
        return;
    }
    let mut open = true;
    let screen = ctx.screen_rect();
    egui::Window::new("Workspace Settings")
        .open(&mut open)
        .default_size([820.0, 620.0])
        .min_size([560.0, 360.0])
        .resizable(true)
        // Mittig und über den Panels öffnen: Ohne `max_height` begrenzt egui
        // das Fenster auf die Fläche über dem Inspector.
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(screen.center_top() + egui::vec2(0.0, 60.0))
        .constrain_to(screen)
        .max_height(screen.height() - 100.0)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // Linke Navigation (QLab: Seitenliste, docs/UI.md §5.7).
                ui.vertical(|ui| {
                    ui.set_width(136.0);
                    for page in SettingsPage::ALL {
                        let selected = app.ui.workspace_settings_page == page;
                        if theme::nav_row(ui, selected, page.label()).clicked() {
                            app.ui.workspace_settings_page = page;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(CONTENT_MIN_WIDTH);
                    match app.ui.workspace_settings_page {
                        // QLab 5: „Video → Stages/Screens" (Screen-Editor).
                        SettingsPage::Video => {
                            draw_output_mode(ui, app);
                            theme::tab_bar(
                                ui,
                                &mut app.ui.video_tab,
                                &[
                                    (VideoTab::Screens, "Screens".to_string()),
                                    (VideoTab::Stages, "Stages".to_string()),
                                ],
                            );
                            match app.ui.video_tab {
                                VideoTab::Screens => super::screen_editor::draw(ui, app),
                                VideoTab::Stages => draw_stages(ui, app),
                            }
                        }
                        SettingsPage::General => draw_general(ui, app),
                        SettingsPage::Audio => draw_audio(ui, app),
                        SettingsPage::Network => draw_network(ui, app),
                        SettingsPage::Midi => draw_midi(ui, app),
                        SettingsPage::KeyMap => {
                            egui::ScrollArea::vertical().show(ui, super::shortcuts::draw_table);
                        }
                    }
                });
            });
        });
    if !open {
        app.ui.workspace_settings_open = false;
    }
}

/// Seite „General": Show-Name, Show Mode, Audition Mode.
fn draw_general(ui: &mut egui::Ui, app: &mut ShowApp) {
    theme::form_grid("settings_general").show(ui, |ui| {
        theme::field_label(ui, "Show Name");
        let r = ui.text_edit_singleline(&mut app.show_name);
        app.text_field_ids.insert(r.id);
        ui.end_row();

        theme::field_label(ui, "Show Mode");
        let mut show_mode = app.show_mode;
        if ui.checkbox(&mut show_mode, "Editing disabled").changed() {
            app.set_show_mode(show_mode);
        }
        ui.end_row();

        theme::field_label(ui, "Audition Mode");
        let mut audition = app.audition_mode;
        if ui
            .checkbox(&mut audition, "Video cues play without a stage target")
            .changed()
        {
            app.set_audition_mode(audition);
        }
        ui.end_row();
    });
}

/// Seite „Audio": Auswahl des Ausgabegeräts.
fn draw_audio(ui: &mut egui::Ui, app: &mut ShowApp) {
    let current = app.audio_output().map(str::to_string);
    // Einmal beim Öffnen statt jedes Frame: Die Abfrage ist dafür zu teuer.
    let list = app.audio_devices.get_or_insert_with(|| AudioDeviceList {
        devices: output_devices(),
        default_name: default_output_name(),
    });
    let default_label = match &list.default_name {
        Some(name) => format!("System Default ({name})"),
        None => "System Default".to_string(),
    };
    let names: Vec<String> = list.devices.iter().map(|d| d.name.clone()).collect();
    let device_infos = list.devices.clone();
    let list_default = list.default_name.clone();
    let shown = current
        .as_deref()
        .or(list.default_name.as_deref())
        .and_then(|name| list.devices.iter().find(|d| d.name == name))
        .map(|d| format!("{} channels, {} Hz", d.channels, d.sample_rate));
    let current_label = match &current {
        None => default_label.clone(),
        Some(name) if names.contains(name) => name.clone(),
        Some(name) => format!("{name} (not connected)"),
    };

    let running = app.carts.running_count() > 0;
    let editable = !app.show_mode && !running;
    let mut picked: Option<Option<String>> = None;
    let mut refresh = false;
    theme::form_grid("settings_audio").show(ui, |ui| {
        theme::field_label(ui, "Output");
        ui.horizontal(|ui| {
            ui.add_enabled_ui(editable, |ui| {
                egui::ComboBox::from_id_salt("audio_output_combo")
                    .selected_text(current_label)
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current.is_none(), default_label)
                            .clicked()
                        {
                            picked = Some(None);
                        }
                        for name in &names {
                            if ui
                                .selectable_label(current.as_ref() == Some(name), name)
                                .clicked()
                            {
                                picked = Some(Some(name.clone()));
                            }
                        }
                    });
            });
            refresh = icons::button(
                ui,
                Icon::ArrowClockwise,
                theme::palette::TEXT,
                egui::vec2(22.0, 22.0),
                true,
            )
            .on_hover_text("Refresh devices (e.g. after connecting an interface)")
            .clicked();
        });
        ui.end_row();

        theme::field_label(ui, "Format");
        ui.label(shown.unwrap_or_else(|| "Unknown".to_string()));
        ui.end_row();
    });
    let device_channels = current
        .as_deref()
        .or(list_default.as_deref())
        .and_then(|name| device_infos.iter().find(|d| d.name == name))
        .map(|d| d.channels);
    if let Some(device) = picked {
        app.set_audio_output(device);
    }
    if refresh {
        app.audio_devices = None;
    }
    if running {
        config_hint(ui, "Stop all cues to change the device.");
    } else if app.show_mode {
        config_hint(ui, "Leave Show Mode to change the device.");
    }
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
    theme::section_label(ui, "Channels");
    draw_channel_patch(ui, app, device_channels);
    config_hint(
        ui,
        "Device, channel names and outputs are saved with the show.",
    );
}

/// Tabelle: Audio-Kanal (Name) → Geräte-Ausgang. Wirkt sofort, auch live.
fn draw_channel_patch(ui: &mut egui::Ui, app: &mut ShowApp, device_channels: Option<u16>) {
    let mut patch = app.audio_patch.clone();
    let max_output = device_channels.unwrap_or(16).clamp(1, 64);
    let output_label = |output: u8| {
        if output == 0 {
            "Off".to_string()
        } else {
            format!("Output {output}")
        }
    };
    ui.add_enabled_ui(!app.show_mode, |ui| {
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 40.0)
            .show(ui, |ui| {
                theme::form_grid("settings_audio_patch").show(ui, |ui| {
                    for (i, channel) in patch.channels.iter_mut().enumerate() {
                        theme::field_label(ui, &format!("Ch {}", i + 1));
                        ui.horizontal(|ui| {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut channel.name).desired_width(160.0),
                            );
                            app.text_field_ids.insert(r.id);
                            egui::ComboBox::from_id_salt(("audio_channel_output", i))
                                .selected_text(output_label(channel.output))
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut channel.output, 0, "Off");
                                    for output in 1..=max_output.min(255) as u8 {
                                        ui.selectable_value(
                                            &mut channel.output,
                                            output,
                                            output_label(output),
                                        );
                                    }
                                });
                            if u16::from(channel.output) > max_output {
                                ui.label(
                                    egui::RichText::new("Not on this device")
                                        .color(theme::palette::TEXT_DIM),
                                );
                            }
                        });
                        ui.end_row();
                    }
                });
            });
    });
    if patch != app.audio_patch {
        app.set_audio_patch(patch);
    }
}

/// Seite „Network": Remote-Konfiguration (nur Anzeige).
fn draw_network(ui: &mut egui::Ui, app: &mut ShowApp) {
    let config = &app.remote_config;
    let enabled = |on: bool, value: String| if on { value } else { "Off".to_string() };
    let osc_out = match &config.osc_out {
        Some((host, port)) => format!("{host}:{port}"),
        None => "Off".to_string(),
    };
    let rows = [
        (
            "Web Remote",
            enabled(config.web_enabled, format!("Port {}", config.web_port)),
        ),
        (
            "Companion",
            enabled(
                config.companion_enabled,
                format!("Port {}", config.companion_port),
            ),
        ),
        (
            "OSC In",
            enabled(config.osc_enabled, format!("Port {}", config.osc_port)),
        ),
        ("OSC Out", enabled(config.osc_enabled, osc_out)),
        (
            "OSC Prefix",
            enabled(config.osc_enabled, config.osc_prefix.clone()),
        ),
    ];
    theme::form_grid("settings_network").show(ui, |ui| {
        for (label, value) in rows {
            theme::field_label(ui, label);
            ui.label(value);
            ui.end_row();
        }
    });
    config_hint(
        ui,
        "Set at startup via KUHPLAY_REMOTE_* and KUHPLAY_OSC_* environment variables.",
    );
    draw_web_pin(ui, app);
}

/// Seite „MIDI": Verbindungsstatus und Noten-Belegung (nur Anzeige).
fn draw_midi(ui: &mut egui::Ui, app: &ShowApp) {
    let config = &app.remote_config;
    let status = if !config.midi_enabled {
        "Off"
    } else if app.midi_starting() {
        "Connecting…"
    } else if app.midi_connected() {
        "Connected"
    } else if app.midi_init_failed() {
        "Unavailable, restart KuhPlay to retry"
    } else {
        "No device connected"
    };
    let map = config.midi_map;
    theme::form_grid("settings_midi").show(ui, |ui| {
        theme::field_label(ui, "Input");
        ui.label(status);
        ui.end_row();
        theme::field_label(ui, "Device Filter");
        ui.label(config.midi_device.as_deref().unwrap_or("First device"));
        ui.end_row();
        for (label, note) in [
            ("Go", map.go),
            ("Panic", map.panic),
            ("Pause", map.pause),
            ("Resume", map.resume),
            ("Reset", map.reset),
        ] {
            theme::field_label(ui, label);
            ui.label(format!("Note {note}"));
            ui.end_row();
        }
    });
    config_hint(
        ui,
        "Set at startup via KUHPLAY_MIDI, KUHPLAY_MIDI_DEVICE and KUHPLAY_MIDI_MAP.",
    );
}

/// Hinweis unter einer Anzeige-Seite: woher die Werte kommen.
fn config_hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
    ui.label(egui::RichText::new(text).color(theme::palette::TEXT_DIM));
}

/// Auswahl der Video-Ausgabe über dem Screen-Editor.
fn draw_output_mode(ui: &mut egui::Ui, app: &mut ShowApp) {
    theme::form_grid("settings_output_mode").show(ui, |ui| {
        theme::field_label(ui, "Output");
        ui.horizontal(|ui| {
            for (mode, label, hover) in [
                (OutputMode::Off, "Off", "No automatic output"),
                (
                    OutputMode::Preview,
                    "Preview Windows",
                    "One window per screen, also reopens closed previews",
                ),
                (
                    OutputMode::Displays,
                    "Displays",
                    "Fullscreen on the assigned displays",
                ),
            ] {
                if ui
                    .selectable_label(app.output_mode == mode, label)
                    .on_hover_text(hover)
                    .clicked()
                {
                    app.set_output_mode(mode);
                }
            }
        });
        ui.end_row();
    });
    ui.label(
        egui::RichText::new("Displays are arranged in the macOS System Settings (Displays).")
            .color(theme::palette::TEXT_DIM),
    );
    ui.add_space(theme::size::SPACE_MEDIUM);
}

/// PIN der Web-Remote: anzeigen, ändern, neu würfeln.
fn draw_web_pin(ui: &mut egui::Ui, app: &mut ShowApp) {
    use crate::remote::web::{generate_pin, valid_pin};
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
    theme::section_label(ui, "Web Remote Access");
    let Some(pin) = app.web_pin() else {
        config_hint(ui, "No PIN: anyone on the network can edit this show.");
        return;
    };
    let edit_id = ui.id().with("web_pin_edit");
    let mut text = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(edit_id))
        .unwrap_or_else(|| pin.clone());
    let mut new_pin = None;
    theme::form_grid("settings_web_pin").show(ui, |ui| {
        theme::field_label(ui, "PIN");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
            app.text_field_ids.insert(r.id);
            let changed = text != pin;
            if ui
                .add_enabled(changed && valid_pin(&text), egui::Button::new("Apply"))
                .on_disabled_hover_text("4 to 8 digits")
                .clicked()
            {
                new_pin = Some(text.clone());
            }
            if ui.button("New PIN").clicked() {
                new_pin = Some(generate_pin());
            }
        });
        ui.end_row();
    });
    if let Some(pin) = new_pin {
        text = pin.clone();
        app.set_web_pin(pin);
    }
    ui.ctx().data_mut(|d| d.insert_temp(edit_id, text));
    config_hint(
        ui,
        "Browsers ask for this PIN once. Changing it signs out all browsers. \
         The connection is not encrypted, so use a trusted network.",
    );
}

/// Unterseite „Stages": anlegen, bearbeiten, löschen. Eine Stage ist die Bildfläche,
/// auf der Cues spielen; ein Screen zeigt sie auf einem Ausgang.
fn draw_stages(ui: &mut egui::Ui, app: &mut ShowApp) {
    let stages: Vec<crate::core::Stage> = app.stages.stages().to_vec();
    let screens: Vec<(uuid::Uuid, String, u32, u32)> = app
        .screens
        .screens()
        .iter()
        .map(|s| {
            let (w, h) = s.rotation.rotated_size(s.width, s.height);
            (s.id, s.name.clone(), w, h)
        })
        .collect();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "A stage is the picture cues play on. Assign it to a screen to show it on an output.",
                )
                .color(theme::palette::TEXT_DIM),
            );
            ui.add_space(theme::size::SPACE_MEDIUM);
            for stage in stages {
                let mut updated = stage.clone();
                let screen = stage
                    .screen
                    .and_then(|id| screens.iter().find(|s| s.0 == id));
                let name_id = ui.id().with(("stage_name", stage.id));
                let mut name = ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<String>(name_id))
                    .unwrap_or_else(|| stage.name.clone());
                let mut rename = false;
                let mut delete = false;
                let in_use = app.carts.stage_in_use(&stage.name);
                egui::Frame::none()
                    .fill(theme::palette::RAISED)
                    .rounding(egui::Rounding::same(5.0))
                    .inner_margin(egui::Margin::same(theme::size::SPACE_MEDIUM))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        theme::form_grid(&format!("stage_form_{}", stage.id)).show(ui, |ui| {
                            theme::field_label(ui, "Name");
                            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(220.0));
                            app.text_field_ids.insert(r.id);
                            rename = r.lost_focus() && name != stage.name;
                            ui.end_row();

                            theme::field_label(ui, "Canvas");
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut updated.geometry.width)
                                        .range(16.0..=16384.0)
                                        .prefix("W "),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut updated.geometry.height)
                                        .range(16.0..=16384.0)
                                        .prefix("H "),
                                );
                                if let Some((_, _, w, h)) = screen {
                                    let (w, h) = (*w as f32, *h as f32);
                                    let differs = (updated.geometry.width - w).abs() > 0.5
                                        || (updated.geometry.height - h).abs() > 0.5;
                                    if differs
                                        && ui
                                            .button("Match Screen")
                                            .on_hover_text("Use the resolution of the screen")
                                            .clicked()
                                    {
                                        updated.geometry.width = w;
                                        updated.geometry.height = h;
                                    }
                                }
                            });
                            ui.end_row();

                            theme::field_label(ui, "Screen");
                            let current = screen.map_or("None", |s| s.1.as_str());
                            egui::ComboBox::from_id_salt(("stage_screen", stage.id))
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut updated.screen, None, "None");
                                    for (id, name, _, _) in &screens {
                                        ui.selectable_value(&mut updated.screen, Some(*id), name);
                                    }
                                });
                            ui.end_row();

                            theme::field_label(ui, "Visible");
                            ui.checkbox(&mut updated.visible, "");
                            ui.end_row();
                        });
                        if let Some((_, _, w, h)) = screen {
                            let stage_ratio = updated.geometry.width / updated.geometry.height;
                            if (stage_ratio - *w as f32 / *h as f32).abs() > 0.01 {
                                ui.label(
                                    egui::RichText::new(
                                        "The shape differs from the screen: the picture will be stretched.",
                                    )
                                    .color(theme::palette::ACCENT),
                                );
                            }
                        }
                        let delete_button = ui
                            .add_enabled(!in_use, egui::Button::new("Delete Stage"))
                            .on_disabled_hover_text("Cues still use this stage");
                        delete = delete_button.clicked();
                    });
                ui.ctx().data_mut(|d| d.insert_temp(name_id, name.clone()));
                ui.add_space(theme::size::SPACE_SMALL);
                if delete {
                    app.remove_stage(&stage.name);
                } else if rename {
                    app.rename_stage(&stage.name, &name);
                    ui.ctx().data_mut(|d| d.remove_temp::<String>(name_id));
                } else if updated != stage {
                    app.configure_stage(updated);
                }
            }
            if ui.button("+ Add Stage").clicked() {
                let mut number = app.stages.len() + 1;
                while app.stages.find(&format!("Stage {number}")).is_some() {
                    number += 1;
                }
                app.ensure_stage(&format!("Stage {number}"));
            }
        });
}
