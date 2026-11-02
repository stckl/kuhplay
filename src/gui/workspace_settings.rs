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

/// Unterseite von "Video" (Stages, Devices).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoTab {
    /// Video Outputs (Stages): Stages mit Regionen.
    #[default]
    Stages,
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
        )
        .show_mode_badge(app.show_mode),
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
                                    (VideoTab::Devices, "Output Devices".to_string()),
                                ],
                            );
                            match app.ui.video_tab {
                                VideoTab::Stages => draw_stages_table(ui, app),
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

        theme::field_label(ui, "Panic Fade");
        let mut seconds = app.panic_duration.as_secs_f32();
        if ui
            .add(
                egui::DragValue::new(&mut seconds)
                    .speed(0.05)
                    .range(0.0..=30.0)
                    .suffix(" s"),
            )
            .on_hover_text("Esc fades out over this time and stops. Esc twice stops at once.")
            .changed()
        {
            app.panic_duration = std::time::Duration::from_secs_f32(seconds);
        }
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
                    "One window per stage, also reopens closed previews",
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
    // Block U4: Auto-Update-Check beim Start (abschaltbar für Offline-Shows).
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
    theme::section_label(ui, "Updates");
    ui.checkbox(
        &mut app.ui.auto_check_updates,
        "Automatisch beim Start nach Updates suchen (GitHub Releases, signiert)",
    );
    if ui.button("Jetzt auf Updates prüfen…").clicked() {
        app.start_update_check();
        app.ui.update_dialog_open = true;
    }
    ui.add_space(theme::size::SPACE_MEDIUM_LARGE);
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
// Video: Video Outputs (Stages) / Output Devices
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
        .show_mode_badge(app.show_mode)
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
