//! „Workspace Settings"-Fenster (Workspace → Workspace Settings).
//!
//! Eigenes [`egui::Window`] mit linker Navigationsliste (General, Audio,
//! Video, Network, MIDI, Key Map). Die Stages/Screens-Verwaltung
//! lebt auf der Seite „Video" – der Screen-Editor
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
use crate::core::ChannelOutput;
use crate::engine::audio_backend::{default_output_name, output_devices};

/// Seite der Workspace Settings (linke Navigation).
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

    /// Anzeige-Label (Begriffe, Englisch).
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

/// Unterseite von "Video" (Stages, Output Routing, Devices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoTab {
    /// Video Outputs (Stages): Stages mit Regionen.
    #[default]
    Stages,
    /// Output Routing: Routen auf Devices.
    Routing,
    /// Output Devices: erkannte Displays/Blackmagic.
    Devices,
}

/// Mindestbreite des Seiteninhalts rechts der Navigation.
const CONTENT_MIN_WIDTH: f32 = 640.0;

/// Zeichnet das „Workspace Settings"-Fenster (falls in [`ShowApp::ui`]
/// geöffnet). Ruft der Aufrufer jedes Frame (nach den Panels).
pub fn draw(ctx: &egui::Context, app: &mut ShowApp) {
    let mut open = app.ui.workspace_settings_open;
    if !open {
        return;
    }
    // Echtes OS-Fenster (in Headless-Tests: Overlay-Fallback), siehe viewport_window.
    super::viewport_window::draw(
        ctx,
        &super::viewport_window::WindowSpec::new(
            "kuhplay-workspace-settings",
            "Workspace Settings",
            [820.0, 620.0],
            [560.0, 360.0],
        ),
        &mut open,
        app,
        |_ctx, ui, app| {
            ui.horizontal_top(|ui| {
                // Linke Navigation (Seitenliste, docs/UI.md §5.7).
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
                        // „Video → Stages/Screens" (Screen-Editor).
                        SettingsPage::Video => {
                            draw_output_mode(ui, app);
                            theme::tab_bar(
                                ui,
                                &mut app.ui.video_tab,
                                &[
                                    (VideoTab::Stages, "Video Outputs (Stages)".to_string()),
                                    (VideoTab::Routing, "Output Routing".to_string()),
                                    (VideoTab::Devices, "Output Devices".to_string()),
                                ],
                            );
                            match app.ui.video_tab {
                                VideoTab::Stages => draw_stages_table(ui, app),
                                VideoTab::Routing => draw_routes_table(ui, app),
                                VideoTab::Devices => draw_devices_page(ui, app),
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
        },
    );
    app.ui.workspace_settings_open = open;
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

    draw_workspace_templates(ui, app);
}

/// Abschnitt „Workspace Templates": aktuelles Layout als benannte
/// Vorlage speichern, wieder anwenden oder löschen. Persistenz über eigenen
/// Storage-Key (siehe [`super::state::UI_TEMPLATES_STORAGE_KEY`]).
fn draw_workspace_templates(ui: &mut egui::Ui, app: &mut ShowApp) {
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
    ui.horizontal(|ui| {
        ui.add_space(theme::size::SPACE_SMALL);
        theme::section_label(ui, "Workspace Templates");
    });
    ui.add_space(theme::size::SPACE_SMALL);

    if app.workspace_templates.is_empty() {
        ui.label(egui::RichText::new("No templates saved yet").color(theme::palette::TEXT_FAINT));
    }
    let mut apply = None;
    let mut delete = None;
    for (index, template) in app.workspace_templates.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(theme::size::SPACE_SMALL);
            ui.label(&template.name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Delete").clicked() {
                    delete = Some(index);
                }
                if ui
                    .small_button("Apply")
                    .on_hover_text("Restore this workspace layout")
                    .clicked()
                {
                    apply = Some(index);
                }
            });
        });
    }
    if let Some(index) = apply {
        app.apply_workspace_template(index);
    }
    if let Some(index) = delete {
        app.delete_workspace_template(index);
    }

    // Neues Template aus dem aktuellen Layout speichern.
    ui.horizontal(|ui| {
        ui.add_space(theme::size::SPACE_SMALL);
        let r = ui.add(
            egui::TextEdit::singleline(&mut app.template_name_buffer)
                .desired_width(220.0)
                .hint_text("Template name"),
        );
        app.text_field_ids.insert(r.id);
        if ui
            .button("Save current layout")
            .on_hover_text("Save the current workspace layout as a template")
            .clicked()
            && !app.template_name_buffer.trim().is_empty()
        {
            let name = app.template_name_buffer.trim().to_string();
            app.save_workspace_template(name);
            app.template_name_buffer.clear();
        }
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

/// Patch-Matrix: Zeilen = die 16 KuhPlay-Kanäle (Name editierbar), Spalten =
/// (Ausgabegerät, Ausgang). Checkbox = Kanal ist auf dieses Ziel gepatcht; ein
/// Kanal kann auf mehrere Ausgänge und mehrere Geräte gleichzeitig gehen.
/// Wirkt sofort (wird live an die Engine gemeldet).
fn draw_channel_patch(ui: &mut egui::Ui, app: &mut ShowApp, device_channels: Option<u16>) {
    // Geräteliste wie auf der Audio-Seite (einmalig laden).
    let list = app.audio_devices.get_or_insert_with(|| AudioDeviceList {
        devices: output_devices(),
        default_name: default_output_name(),
    });
    // Spalten: (Gerät, Ausgang 1-basiert). Pro Gerät max. 16 (u16-Bitmaske).
    let default_channels = list
        .default_name
        .as_deref()
        .and_then(|name| list.devices.iter().find(|d| d.name == name))
        .map(|d| d.channels)
        .or(device_channels)
        .unwrap_or(16)
        .clamp(1, 16);
    let mut columns: Vec<(Option<String>, u8)> =
        (1..=default_channels as u8).map(|o| (None, o)).collect();
    for device in &list.devices {
        let count = device.channels.clamp(1, 16);
        for output in 1..=count as u8 {
            columns.push((Some(device.name.clone()), output));
        }
    }
    let known: Vec<Option<String>> = std::iter::once(None)
        .chain(list.devices.iter().map(|d| Some(d.name.clone())))
        .collect();

    let mut patch = app.audio_patch.clone();
    let mut changed = false;
    ui.add_enabled_ui(!app.show_mode, |ui| {
        egui::ScrollArea::both()
            .max_height(ui.available_height() - 40.0)
            .show(ui, |ui| {
                egui::Grid::new("settings_audio_patch_matrix")
                    .num_columns(columns.len() + 1)
                    .spacing(egui::vec2(6.0, 3.0))
                    .striped(true)
                    .show(ui, |ui| {
                        // Kopfzeile 1: Gerätename (je Gruppe nur in erster Spalte).
                        ui.label(egui::RichText::new("Channel").strong());
                        let mut prev: Option<Option<&str>> = None;
                        for (device, _) in &columns {
                            let name = device.as_deref();
                            if prev == Some(name) {
                                ui.label("");
                            } else {
                                let label = match name {
                                    None => "System Default".to_string(),
                                    Some(name) => name.to_string(),
                                };
                                ui.label(egui::RichText::new(label).strong());
                            }
                            prev = Some(name);
                        }
                        ui.end_row();
                        // Kopfzeile 2: Ausgangsnummern.
                        ui.label(egui::RichText::new("Out").small());
                        for (_, output) in &columns {
                            ui.label(
                                egui::RichText::new(output.to_string())
                                    .size(theme::size::LABEL_TEXT)
                                    .color(theme::palette::TEXT_DIM),
                            );
                        }
                        ui.end_row();
                        // Eine Zeile pro KuhPlay-Kanal.
                        for (i, channel) in patch.channels.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("Ch {}", i + 1)).strong());
                                let r = ui.add(
                                    egui::TextEdit::singleline(&mut channel.name)
                                        .desired_width(140.0),
                                );
                                app.text_field_ids.insert(r.id);
                            });
                            for (device, output) in &columns {
                                let mut checked = channel
                                    .outputs
                                    .iter()
                                    .any(|o| o.device == *device && o.output == *output);
                                if ui.checkbox(&mut checked, "").changed() {
                                    if checked {
                                        channel.outputs.push(ChannelOutput {
                                            device: device.clone(),
                                            output: *output,
                                        });
                                    } else {
                                        channel.outputs.retain(|o| {
                                            !(o.device == *device && o.output == *output)
                                        });
                                    }
                                    changed = true;
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    });
    // Zuordnungen auf nicht verbundene Geräte bleiben erhalten (Show-Datei).
    let missing = patch
        .channels
        .iter()
        .flat_map(|c| c.outputs.iter())
        .filter(|o| o.output > 0 && !known.contains(&o.device))
        .count();
    if missing > 0 {
        config_hint(
            ui,
            &format!("{missing} assignment(s) target a device that is not connected."),
        );
    }
    if changed {
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

// ---------------------------------------------------------------------
// Video: Video Outputs (Stages) / Output Routing / Output Devices
// ---------------------------------------------------------------------

/// Reiter „Video Outputs (Stages)“: Tabelle Name, Auflösung, Devices,
/// Edit, Monitor, Löschen + „New Stage“-Menü.
fn draw_stages_table(ui: &mut egui::Ui, app: &mut ShowApp) {
    let stages: Vec<crate::core::Stage> = app.stages.stages().to_vec();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "A stage is the picture cues play on. Its regions show on output routes.",
                )
                .color(theme::palette::TEXT_DIM),
            );
            ui.add_space(theme::size::SPACE_MEDIUM);
            for stage in stages {
                let devices: Vec<String> = stage
                    .regions
                    .iter()
                    .filter_map(|r| r.route)
                    .filter_map(|id| app.routes.find(id))
                    .map(|route| route.name.clone())
                    .collect();
                let in_use = app.carts.stage_in_use(&stage.name);
                let mut edit = false;
                let mut monitor = app.stage_windows.contains(&stage.name);
                let mut delete = false;
                egui::Frame::none()
                    .fill(theme::palette::RAISED)
                    .rounding(egui::Rounding::same(5.0))
                    .inner_margin(egui::Margin::same(theme::size::SPACE_MEDIUM))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        theme::form_grid(&format!("stage_row_{}", stage.id)).show(ui, |ui| {
                            theme::field_label(ui, "Name");
                            ui.label(&stage.name);
                            ui.end_row();
                            theme::field_label(ui, "Resolution");
                            ui.label(format!(
                                "{}x{}",
                                stage.geometry.width as u32, stage.geometry.height as u32
                            ));
                            ui.end_row();
                            theme::field_label(ui, "Devices");
                            ui.label(if devices.is_empty() {
                                "-".to_string()
                            } else {
                                devices.join(", ")
                            })
                            .on_hover_text("Output routes fed by this stage");
                            ui.end_row();
                            theme::field_label(ui, "Actions");
                            ui.horizontal(|ui| {
                                if ui.button("Edit").clicked() {
                                    edit = true;
                                }
                                if ui
                                    .selectable_label(monitor, "Monitor")
                                    .on_hover_text("Open the stage monitor window")
                                    .clicked()
                                {
                                    monitor = !monitor;
                                }
                                let delete_button =
                                    ui.add_enabled(!in_use, egui::Button::new("Delete"));
                                let clicked = delete_button.clicked();
                                delete_button.on_disabled_hover_text("Cues still use this stage");
                                delete = clicked;
                            });
                            ui.end_row();
                        });
                    });
                if edit {
                    app.stage_editor_open = Some(stage.id);
                }
                if monitor {
                    app.stage_windows.insert(stage.name.clone());
                } else {
                    app.stage_windows.remove(&stage.name);
                }
                if delete {
                    app.remove_stage(&stage.name);
                }
                ui.add_space(theme::size::SPACE_SMALL);
            }
            // „New Stage“ mit Menü (leer / mit Output / Multi-Output).
            egui::ComboBox::from_id_salt("new_stage_menu")
                .selected_text("+ New Stage")
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(false, "Empty Stage (1920x1080)")
                        .clicked()
                    {
                        app.add_empty_stage();
                    }
                    if ui.selectable_label(false, "Stage with Output").clicked() {
                        app.add_stage_with_output();
                    }
                    if ui.selectable_label(false, "Multi-Output Stage…").clicked() {
                        app.multi_output_wizard = Some(Default::default());
                    }
                });
        });
}

/// Reiter „Output Routing“: Tabelle Name, Auflösung, Device-Auswahl,
/// Teilbereich, Testbild, Farbe, Edit, Löschen + „New Output Route“.
/// Device-Karte im Tab „Output Routing": alle Displays als Umrisse (Welt =
/// globale Display-Punkte), jede Route als farbiges Rechteck an ihrer
/// `DeviceArea`-Position (Part ist in Gerätepixeln, Umrechnung über
/// `scale_factor`). Ziehen verschiebt die Route auf dem Gerät, Ziehen an der
/// Ecke unten rechts skaliert den Ausschnitt, Doppelklick = „Full".
/// Einrasten an den Geräte-Rändern (Shift deaktiviert), Änderungen wirken
/// sofort (`configure_routes`).
fn draw_device_map(ui: &mut egui::Ui, app: &mut ShowApp) {
    use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};

    /// Drag-Zustand der Karte (im Context gespeichert, überdauert Frames).
    #[derive(Clone, Copy, Default)]
    enum MapDrag {
        #[default]
        Idle,

        Move {
            id: uuid::Uuid,
            grab: (f32, f32),
        },
        Resize {
            id: uuid::Uuid,
        },
    }

    let displays = app.displays.clone();
    if displays.is_empty() {
        return;
    }
    // Welt = Bounding-Box aller Displays in globalen Punkten.
    let min_x = displays.iter().map(|d| d.x).min().unwrap_or(0) as f32;
    let min_y = displays.iter().map(|d| d.y).min().unwrap_or(0) as f32;
    let bw = (displays
        .iter()
        .map(|d| d.x + d.width as i32)
        .max()
        .unwrap_or(0) as f32
        - min_x)
        .max(1.0);
    let bh = (displays
        .iter()
        .map(|d| d.y + d.height as i32)
        .max()
        .unwrap_or(0) as f32
        - min_y)
        .max(1.0);

    let canvas_height = 210.0_f32;
    let (canvas_rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), canvas_height),
        Sense::click_and_drag(),
    );
    ui.set_clip_rect(canvas_rect.intersect(ui.clip_rect()));
    ui.painter()
        .rect_filled(canvas_rect, 4.0, theme::palette::BACKGROUND);

    let margin = 20.0_f32;
    let scale = ((canvas_rect.width() - 2.0 * margin) / bw)
        .min((canvas_rect.height() - 2.0 * margin) / bh)
        .max(0.001);
    let offset = Vec2::new(
        canvas_rect.center().x - (min_x + bw / 2.0) * scale,
        canvas_rect.center().y - (min_y + bh / 2.0) * scale,
    );
    let to_screen = |x: f32, y: f32| Pos2::new(x * scale + offset.x, y * scale + offset.y);
    let to_world = |p: Pos2| ((p.x - offset.x) / scale, (p.y - offset.y) / scale);
    let pointer_world = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|p| canvas_rect.contains(*p))
        .map(to_world);

    // Route → Rechteck in Welt-Punkten (Part in Gerätepixeln → Punkte).
    let route_rect = |route: &crate::core::OutputRoute,
                      d: &display_info::DisplayInfo|
     -> Option<(f32, f32, f32, f32)> {
        let crate::core::DeviceRef::Display { id, .. } = route.device.as_ref()? else {
            return None;
        };
        if *id != d.id {
            return None;
        }
        Some(route_map_rect(
            &route.device_area,
            (d.width, d.height),
            d.scale_factor,
            (d.x as f32, d.y as f32),
        ))
    };

    let mut routes = app.routes.routes().to_vec();
    let drag_id = ui.id().with("device_map_drag");
    let sel_id = ui.id().with("device_map_selected");
    let mut selected: Option<uuid::Uuid> = ui.ctx().data_mut(|d| d.get_temp(sel_id));
    let mut changed = false;

    // --- Interaktion ---
    if response.drag_started() {
        if let Some(pw) = pointer_world {
            // Oberste Route am Pointer finden (zuletzt gezeichnete zuerst).
            let hit = routes.iter().rev().find_map(|r| {
                displays
                    .iter()
                    .find_map(|d| route_rect(r, d).map(|(x, y, w, h)| (r.id, x, y, w, h)))
                    .filter(|(_, x, y, w, h)| {
                        pw.0 >= *x && pw.0 <= *x + *w && pw.1 >= *y && pw.1 <= *y + *h
                    })
            });
            if let Some((id, x, y, w, _h)) = hit {
                selected = Some(id);
                let corner_tol = 10.0 / scale;
                let drag = if (pw.0 - (x + w)).abs() <= corner_tol
                    && (pw.1 - (y + _h)).abs() <= corner_tol
                {
                    MapDrag::Resize { id }
                } else {
                    MapDrag::Move {
                        id,
                        grab: (pw.0 - x, pw.1 - y),
                    }
                };
                ui.ctx().data_mut(|d| d.insert_temp(drag_id, drag));
            }
        }
    }
    if response.dragged() {
        let drag = ui.ctx().data_mut(|d| d.get_temp::<MapDrag>(drag_id));
        let snap = !ui.input(|i| i.modifiers.shift);
        if let (Some(drag), Some(pw)) = (drag, pointer_world) {
            let id = match drag {
                MapDrag::Move { id, .. } | MapDrag::Resize { id } => id,
                MapDrag::Idle => return,
            };
            let Some(route) = routes.iter_mut().find(|r| r.id == id) else {
                return;
            };
            let Some((d, (rx, ry, rw, rh))) = displays
                .iter()
                .find_map(|d| route_rect(route, d).map(|r| (d.clone(), r)))
            else {
                return;
            };
            let sf = d.scale_factor.max(0.001);
            let (pw_dev, ph_dev) = (
                (d.width as f32 * sf).round().max(1.0),
                (d.height as f32 * sf).round().max(1.0),
            );
            // Pointer in Gerätepixeln relativ zur Display-Kante.
            let px = (pw.0 - d.x as f32) * sf;
            let py = (pw.1 - d.y as f32) * sf;
            let (mut nx, mut ny, mut nw, mut nh) = match drag {
                MapDrag::Move { grab, .. } => {
                    let gx = grab.0 * sf;
                    let gy = grab.1 * sf;
                    (px - gx, py - gy, rw * sf, rh * sf)
                }
                MapDrag::Resize { .. } => (rx * sf, ry * sf, px - rx * sf, py - ry * sf),
                MapDrag::Idle => unreachable!(),
            };
            // An Geräte-Rändern einrasten (in Gerätepixeln).
            if snap {
                let tol = 12.0;
                if nx.abs() < tol {
                    nx = 0.0;
                }
                if ny.abs() < tol {
                    ny = 0.0;
                }
                if (nx + nw - pw_dev).abs() < tol {
                    nw = pw_dev - nx;
                }
                if (ny + nh - ph_dev).abs() < tol {
                    nh = ph_dev - ny;
                }
            }
            // Im Gerät halten, Mindestgröße 16 px.
            nw = nw.clamp(16.0, pw_dev - nx.min(pw_dev - 16.0).max(0.0));
            nh = nh.clamp(16.0, ph_dev - ny.min(ph_dev - 16.0).max(0.0));
            nx = nx.clamp(0.0, pw_dev - nw);
            ny = ny.clamp(0.0, ph_dev - nh);
            let new_area = crate::core::DeviceArea::Part {
                x: nx.round().max(0.0) as u32,
                y: ny.round().max(0.0) as u32,
                width: nw.round().max(1.0) as u32,
                height: nh.round().max(1.0) as u32,
            };
            if route.device_area != new_area {
                route.device_area = new_area;
                changed = true;
            }
        }
    }
    if response.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove_temp::<MapDrag>(drag_id));
    }
    if response.double_clicked() {
        if let Some(pw) = pointer_world {
            let hit = routes.iter().rev().find(|r| {
                displays.iter().any(|d| {
                    route_rect(r, d)
                        .map(|(x, y, w, h)| {
                            pw.0 >= x && pw.0 <= x + w && pw.1 >= y && pw.1 <= y + h
                        })
                        .unwrap_or(false)
                })
            });
            if let Some(hit_id) = hit.map(|r| r.id) {
                if let Some(route) = routes.iter_mut().find(|x| x.id == hit_id) {
                    if route.device_area != crate::core::DeviceArea::Full {
                        route.device_area = crate::core::DeviceArea::Full;
                        changed = true;
                    }
                }
            }
        }
    }
    if response.clicked() {
        if let Some(pw) = pointer_world {
            selected = routes
                .iter()
                .rev()
                .find(|r| {
                    displays.iter().any(|d| {
                        route_rect(r, d)
                            .map(|(x, y, w, h)| {
                                pw.0 >= x && pw.0 <= x + w && pw.1 >= y && pw.1 <= y + h
                            })
                            .unwrap_or(false)
                    })
                })
                .map(|r| r.id);
        }
    }
    ui.ctx().data_mut(|d| {
        if let Some(sel) = selected {
            d.insert_temp(sel_id, sel);
        } else {
            d.remove_temp::<uuid::Uuid>(sel_id);
        }
    });

    // --- Displays zeichnen ---
    for d in &displays {
        let r = Rect::from_min_size(
            to_screen(d.x as f32, d.y as f32),
            egui::vec2(d.width as f32 * scale, d.height as f32 * scale),
        );
        ui.painter()
            .rect_stroke(r, 2.0, Stroke::new(1.0_f32, theme::palette::COMPLETED));
        ui.painter().text(
            r.left_bottom() + egui::vec2(4.0, -2.0),
            Align2::LEFT_BOTTOM,
            format!("{}  {}×{} pt", d.name, d.width, d.height),
            FontId::proportional(10.0),
            theme::palette::COMPLETED,
        );
    }

    // --- Routen zeichnen ---
    for route in &routes {
        let Some((d, (x, y, w, h))) = displays
            .iter()
            .find_map(|d| route_rect(route, d).map(|r| (d, r)))
        else {
            continue;
        };
        let _ = d;
        let r = Rect::from_min_size(to_screen(x, y), egui::vec2(w * scale, h * scale));
        let is_sel = selected == Some(route.id);
        let color = egui::Color32::from_rgb(route.color[0], route.color[1], route.color[2]);
        let fill = if is_sel {
            egui::Color32::from_rgba_unmultiplied(
                route.color[0],
                route.color[1],
                route.color[2],
                96,
            )
        } else {
            egui::Color32::from_rgba_unmultiplied(
                route.color[0],
                route.color[1],
                route.color[2],
                56,
            )
        };
        ui.painter().rect_filled(r, 2.0, fill);
        ui.painter().rect_stroke(
            r,
            2.0,
            Stroke::new(if is_sel { 2.5_f32 } else { 1.0_f32 }, color),
        );
        let area_label = match route.device_area {
            crate::core::DeviceArea::Full => "Full".to_string(),
            crate::core::DeviceArea::Part { width, height, .. } => format!("{width}×{height} px"),
        };
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            format!("{} ({})", route.name, area_label),
            FontId::proportional(11.0),
            theme::palette::TEXT,
        );
        // Resize-Griff unten rechts.
        let grip = Rect::from_center_size(
            r.right_bottom() - egui::vec2(0.0, 0.0),
            egui::vec2(10.0, 10.0),
        );
        ui.painter().rect_filled(grip, 2.0, color);
    }

    if changed {
        for updated in routes {
            if let Some(r) = app.routes.find_mut(updated.id) {
                *r = updated;
            }
        }
        app.configure_routes();
    }
}

fn draw_routes_table(ui: &mut egui::Ui, app: &mut ShowApp) {
    draw_device_map(ui, app);
    ui.add_space(theme::size::SPACE_MEDIUM);
    let routes: Vec<crate::core::OutputRoute> = app.routes.routes().to_vec();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "An output route is a logical video output. Assign it to a display or a \
                     Blackmagic device; regions of stages show on it.",
                )
                .color(theme::palette::TEXT_DIM),
            );
            ui.add_space(theme::size::SPACE_MEDIUM);
            for route in routes {
                let mut edit = false;
                let mut delete = false;
                let mut updated = route.clone();
                egui::Frame::none()
                    .fill(theme::palette::RAISED)
                    .rounding(egui::Rounding::same(5.0))
                    .inner_margin(egui::Margin::same(theme::size::SPACE_MEDIUM))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        theme::form_grid(&format!("route_row_{}", route.id)).show(ui, |ui| {
                            theme::field_label(ui, "Name");
                            ui.horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(12.0, 12.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    2.0,
                                    egui::Color32::from_rgb(
                                        route.color[0],
                                        route.color[1],
                                        route.color[2],
                                    ),
                                );
                                ui.label(&route.name);
                            });
                            ui.end_row();
                            theme::field_label(ui, "Resolution");
                            ui.label(format!("{}x{}", route.width, route.height));
                            ui.end_row();
                            theme::field_label(ui, "Device");
                            device_label(ui, &route, app);
                            ui.end_row();
                            theme::field_label(ui, "Test Pattern");
                            let mut pattern = updated.test_pattern;
                            if ui.checkbox(&mut pattern, "").changed() {
                                updated.test_pattern = pattern;
                            }
                            ui.end_row();
                            theme::field_label(ui, "Key (Alpha)");
                            let mut alpha = updated.alpha;
                            if ui
                                .checkbox(&mut alpha, "")
                                .on_hover_text(
                                    "Key-Route für Key/Fill: Alpha bleibt im Route-Bild erhalten                                      (Stage-Canvas transparent, echtes Over-Alpha). Fill-Routen bleiben flach.",
                                )
                                .changed()
                            {
                                updated.alpha = alpha;
                            }
                            ui.end_row();
                            theme::field_label(ui, "Actions");
                            ui.horizontal(|ui| {
                                if ui.button("Edit").clicked() {
                                    edit = true;
                                }
                                if ui.button("Delete").clicked() {
                                    delete = true;
                                }
                            });
                            ui.end_row();
                        });
                    });
                if updated != route {
                    if let Some(r) = app.routes.find_mut(route.id) {
                        *r = updated;
                    }
                    app.configure_routes();
                }
                if edit {
                    app.route_editor_open = Some(route.id);
                }
                if delete {
                    app.remove_route(route.id);
                }
                ui.add_space(theme::size::SPACE_SMALL);
            }
            // „New Output Route“ mit Menü je Device.
            egui::ComboBox::from_id_salt("new_route_menu")
                .selected_text("+ New Output Route")
                .show_ui(ui, |ui| {
                    let displays: Vec<crate::core::DisplayRefInfo> = app
                        .displays
                        .iter()
                        .map(crate::core::DisplayRefInfo::from)
                        .collect();
                    for display in &displays {
                        let label = format!(
                            "Display: {} ({}x{})",
                            display.name,
                            display.pixel_size().0,
                            display.pixel_size().1
                        );
                        if ui.selectable_label(false, label).clicked() {
                            app.add_route_for_display(display);
                        }
                    }
                    if ui.selectable_label(false, "Without device").clicked() {
                        app.add_route();
                    }
                });
        });
}

/// Device-Spalte: Name oder kursives „(missing)“, wenn das Gerät fehlt.
fn device_label(ui: &mut egui::Ui, route: &crate::core::OutputRoute, app: &ShowApp) {
    match &route.device {
        None => {
            ui.label(egui::RichText::new("(no device)").weak());
        }
        Some(crate::core::DeviceRef::Display { id, name, .. }) => {
            let present = app.displays.iter().any(|d| d.id == *id);
            if present {
                ui.label(name);
            } else {
                ui.label(
                    egui::RichText::new(format!("{name} (missing)"))
                        .italics()
                        .color(theme::palette::ACCENT),
                );
            }
        }
        Some(crate::core::DeviceRef::Blackmagic { name }) => {
            // Ohne Hardware-Anbindung immer „(missing)“ (Phase 7: Schnittstelle).
            ui.label(
                egui::RichText::new(format!("{name} (missing)"))
                    .italics()
                    .color(theme::palette::ACCENT),
            );
        }
    }
}

/// Reiter „Output Devices“: Name, Auflösung, Info (Hz) + Refresh +
/// Hinweis auf die Systemeinstellungen.
fn draw_devices_page(ui: &mut egui::Ui, app: &mut ShowApp) {
    let mut refresh = false;
    ui.horizontal(|ui| {
        theme::section_label(ui, "Output Devices");
        if ui.button("Refresh").clicked() {
            refresh = true;
        }
    });
    ui.add_space(theme::size::SPACE_SMALL);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            theme::form_grid("devices_grid").show(ui, |ui| {
                for display in app.displays.iter().map(crate::core::DisplayRefInfo::from) {
                    let (w, h) = display.pixel_size();
                    theme::field_label(ui, &display.name);
                    ui.label(format!(
                        "{}x{} px ({}x{} pt)",
                        w, h, display.width, display.height
                    ));
                    ui.end_row();
                    theme::field_label(ui, "");
                    ui.label(format!(
                        "{}{}",
                        if display.frequency > 0.0 {
                            format!("{:.0} Hz", display.frequency)
                        } else {
                            "Hz unknown".to_string()
                        },
                        if display.scale_factor > 1.0 {
                            format!(", Retina x{}", display.scale_factor)
                        } else {
                            String::new()
                        }
                    ));
                    ui.end_row();
                }
            });
        });
    if refresh {
        app.refresh_displays();
    }
    config_hint(
        ui,
        "Arrange displays and change resolutions in the macOS System Settings (Displays).",
    );
}

/// Route-Editor (eigenes Fenster): Name, Größe, „Set from Device“,
/// Drehung, Rear projection, Skalierung, Device, Teilbereich.
pub fn draw_route_editor(ctx: &egui::Context, app: &mut ShowApp) {
    let Some(route_id) = app.route_editor_open else {
        return;
    };
    let Some(route) = app.routes.find(route_id).cloned() else {
        app.route_editor_open = None;
        return;
    };
    let mut open = true;
    let mut updated = route.clone();
    let mut set_from_device = false;
    let title = format!("Route Editor: {}", route.name);
    // Echtes OS-Fenster (in Headless-Tests: Overlay-Fallback).
    super::viewport_window::draw(
        ctx,
        &super::viewport_window::WindowSpec::new(
            "kuhplay-route-editor",
            &title,
            [420.0, 320.0],
            [320.0, 240.0],
        ),
        &mut open,
        app,
        |_ctx, ui, app| {
            theme::form_grid("route_editor").show(ui, |ui| {
                theme::field_label(ui, "Name");
                let r = ui.add(egui::TextEdit::singleline(&mut updated.name).desired_width(200.0));
                app.text_field_ids.insert(r.id);
                ui.end_row();
                theme::field_label(ui, "Size");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut updated.width)
                            .range(1..=30000)
                            .prefix("W "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut updated.height)
                            .range(1..=30000)
                            .prefix("H "),
                    );
                    if ui.button("Set from Device").clicked() {
                        set_from_device = true;
                    }
                });
                ui.end_row();
                theme::field_label(ui, "Rotation");
                let mut rot = updated.rotation;
                egui::ComboBox::from_id_salt("route_rotation")
                    .selected_text(rot.label())
                    .show_ui(ui, |ui| {
                        for r in crate::core::ScreenRotation::ALL {
                            ui.selectable_value(&mut rot, r, r.label());
                        }
                    });
                updated.rotation = rot;
                ui.end_row();
                theme::field_label(ui, "Rear Projection");
                ui.checkbox(&mut updated.rear_projection, "mirror horizontally");
                ui.end_row();
                theme::field_label(ui, "Scaling");
                let mut scaling = updated.scaling;
                egui::ComboBox::from_id_salt("route_scaling")
                    .selected_text(scaling.label())
                    .show_ui(ui, |ui| {
                        for s in crate::core::RouteScaling::ALL {
                            ui.selectable_value(&mut scaling, s, s.label());
                        }
                    });
                updated.scaling = scaling;
                ui.end_row();
                theme::field_label(ui, "Device");
                ui.horizontal(|ui| {
                    let selected = updated
                        .device
                        .as_ref()
                        .map(|d| d.label().to_string())
                        .unwrap_or_else(|| "None".to_string());
                    egui::ComboBox::from_id_salt("route_device")
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(updated.device.is_none(), "None")
                                .clicked()
                            {
                                updated.device = None;
                            }
                            for display in
                                app.displays.iter().map(crate::core::DisplayRefInfo::from)
                            {
                                if ui.selectable_label(false, &display.name).clicked() {
                                    updated.device = Some(crate::core::DeviceRef::Display {
                                        id: display.id,
                                        name: display.name.clone(),
                                        width: display.width,
                                        height: display.height,
                                    });
                                }
                            }
                            // G5: erkannte Karten zur Auswahl (statt festem Namen).
                            let (_avail, devices, hint) =
                                crate::engine::decklink_native::devices_with_hint();
                            let mut bm_name = match &updated.device {
                                Some(crate::core::DeviceRef::Blackmagic { name }) => name.clone(),
                                _ => String::new(),
                            };
                            let mut changed = false;
                            egui::ComboBox::from_id_salt("route-device-bm")
                                .selected_text(if bm_name.is_empty() {
                                    "Blackmagic…".to_string()
                                } else {
                                    bm_name.clone()
                                })
                                .show_ui(ui, |ui| {
                                    for dev in &devices {
                                        if ui.selectable_label(bm_name == *dev, dev).clicked() {
                                            bm_name = dev.clone();
                                            changed = true;
                                        }
                                    }
                                    if devices.is_empty() {
                                        ui.label(egui::RichText::new("(no device)").weak());
                                    }
                                });
                            if changed {
                                updated.device = Some(crate::core::DeviceRef::Blackmagic {
                                    name: bm_name.clone(),
                                });
                            }
                            // G8: Treiber da, aber 0 Geräte → klarer Hinweis.
                            if let Some(hint) = &hint {
                                ui.label(
                                    egui::RichText::new(hint)
                                        .small()
                                        .color(crate::gui::theme::palette::ERROR),
                                );
                            }
                        });
                });
                ui.end_row();
                theme::field_label(ui, "Device Area");
                if let crate::core::DeviceArea::Part {
                    x,
                    y,
                    width,
                    height,
                } = &mut updated.device_area
                {
                    let mut part = (*x, *y, *width, *height);
                    let mut make_full = false;
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut part.0).prefix("X "));
                        ui.add(egui::DragValue::new(&mut part.1).prefix("Y "));
                        ui.add(egui::DragValue::new(&mut part.2).prefix("W "));
                        ui.add(egui::DragValue::new(&mut part.3).prefix("H "));
                        if ui.button("Full").clicked() {
                            make_full = true;
                        }
                    });
                    if make_full {
                        updated.device_area = crate::core::DeviceArea::Full;
                    } else {
                        updated.device_area = crate::core::DeviceArea::Part {
                            x: part.0,
                            y: part.1,
                            width: part.2,
                            height: part.3,
                        };
                    }
                } else if ui.button("Full device").clicked() {
                    updated.device_area = crate::core::DeviceArea::Part {
                        x: 0,
                        y: 0,
                        width: updated.width,
                        height: updated.height,
                    };
                }
                ui.end_row();
            });
        },
    );
    if !open {
        app.route_editor_open = None;
    }
    if set_from_device {
        // Größe von assigned Display uebernehmen (Pixel), Part auf Full.
        if let Some(crate::core::DeviceRef::Display { id, .. }) = updated.device {
            if let Some(display) = app
                .displays
                .iter()
                .map(crate::core::DisplayRefInfo::from)
                .find(|d| d.id == id)
            {
                let (w, h) = display.pixel_size();
                updated.width = w;
                updated.height = h;
                updated.device_area = crate::core::DeviceArea::Full;
            }
        }
    }
    if updated != route {
        if let Some(r) = app.routes.find_mut(route_id) {
            *r = updated;
        }
        app.configure_routes();
    }
}

/// Multi-Output-Assistent: Anzahl, Auflösung und Anordnung der Projektoren,
/// Overlap in Pixeln.
pub fn draw_multi_output_wizard(ctx: &egui::Context, app: &mut ShowApp) {
    let Some(wizard) = app.multi_output_wizard.clone() else {
        return;
    };
    let mut create = false;
    let mut cancel = false;
    let mut w = wizard.clone();
    let mut open = true;
    // Echtes OS-Fenster (in Headless-Tests: Overlay-Fallback).
    super::viewport_window::draw(
        ctx,
        &super::viewport_window::WindowSpec::new(
            "kuhplay-multi-output-wizard",
            "New Multi-Output Stage",
            [380.0, 320.0],
            [300.0, 240.0],
        )
        .resizable(false),
        &mut open,
        app,
        |_ctx, ui, _app| {
            ui.label(
                egui::RichText::new(
                    "Creates one output route per projector (left to right) and a stage \
                     whose regions share the picture with a soft edge overlap.",
                )
                .color(theme::palette::TEXT_DIM),
            );
            theme::form_grid("multi_output_wizard").show(ui, |ui| {
                theme::field_label(ui, "Layout");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut w.cols)
                            .range(1..=6)
                            .prefix("Cols "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut w.rows)
                            .range(1..=6)
                            .prefix("Rows "),
                    );
                });
                ui.end_row();
                theme::field_label(ui, "Resolution");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut w.width)
                            .range(16..=30000)
                            .prefix("W "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut w.height)
                            .range(16..=30000)
                            .prefix("H "),
                    );
                });
                ui.end_row();
                theme::field_label(ui, "Overlap");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut w.overlap_x)
                            .range(0..=2048)
                            .prefix("X "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut w.overlap_y)
                            .range(0..=2048)
                            .prefix("Y "),
                    );
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Create Stage").clicked() {
                    create = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );
    if create {
        app.add_multi_output_stage(&w);
        app.multi_output_wizard = None;
    } else if open && !cancel {
        app.multi_output_wizard = Some(w);
    } else {
        app.multi_output_wizard = None;
    }
}

/// Rechteck einer Route in globalen Display-Punkten: `device_area` ist in
/// Gerätepixeln, `scale_factor` rechnet Punkte → Pixel, `origin` ist die
/// Display-Position in globalen Punkten. `Full` = gesamtes Display.
fn route_map_rect(
    area: &crate::core::DeviceArea,
    display_points: (u32, u32),
    scale_factor: f32,
    origin: (f32, f32),
) -> (f32, f32, f32, f32) {
    let sf = scale_factor.max(0.001);
    let (pw, ph) = (
        (display_points.0 as f32 * sf).round().max(1.0) as u32,
        (display_points.1 as f32 * sf).round().max(1.0) as u32,
    );
    let (ax, ay, aw, ah) = area.rect(pw, ph);
    (
        origin.0 + ax as f32 / sf,
        origin.1 + ay as f32 / sf,
        aw as f32 / sf,
        ah as f32 / sf,
    )
}

#[cfg(test)]
mod device_map_tests {
    use super::route_map_rect;
    use crate::core::DeviceArea;

    #[test]
    fn full_area_covers_whole_display_in_points() {
        // Retina-Display 1920x1080 pt @2x an (5120, 458).
        let r = route_map_rect(&DeviceArea::Full, (1920, 1080), 2.0, (5120.0, 458.0));
        assert_eq!(r, (5120.0, 458.0, 1920.0, 1080.0));
    }

    #[test]
    fn part_area_is_device_pixels_converted_to_points() {
        // LED-Wand-Szenario: SDI 1920x1080 px, genutzter Ausschnitt 800x600 px
        // bei scale_factor 1 → 800x600 pt auf dem Display.
        let area = DeviceArea::Part {
            x: 100,
            y: 50,
            width: 800,
            height: 600,
        };
        let r = route_map_rect(&area, (1920, 1080), 1.0, (0.0, 0.0));
        assert_eq!(r, (100.0, 50.0, 800.0, 600.0));
        // Retina @2x: Gerätepixel halb so viele Punkte.
        let r2 = route_map_rect(&area, (1920, 1080), 2.0, (5120.0, 458.0));
        assert_eq!(r2, (5120.0 + 50.0, 458.0 + 25.0, 400.0, 300.0));
    }
}
