//! Menüleiste (QLab-Style): File / Edit / View / Cues / Help.
//!
//! ## Erweiterung
//!
//! * Weitere Menüeinträge (z. B. „Paste Cue Properties") ergänzen –
//!   siehe `docs/qlab5-parity.md` (Kapitel „Workflow Tools").
//! * Tastenkürzel kommen ALLE aus der zentralen Tabelle
//!   [`super::shortcuts`] (`menu_label`) – Menüanzeige, Shortcuts-Fenster
//!   und `ShowApp::handle_hotkeys` lesen dieselbe Quelle (TODO.md
//!   Phase 5), sie können nie inkonsistent sein.

use egui::vec2;

use crate::core::CueType;

use super::app::ShowApp;
use super::icons::{self, Icon};
use super::shortcuts::{self, ShortcutAction};

/// Menü-Label mit Kürzel aus der zentralen Tabelle (z. B. „Open Show…  ⌘O“).
fn sc(label: &str, action: ShortcutAction) -> String {
    shortcuts::menu_label(label, action)
}

/// Zeichnet die Menüleiste (oberster Panel-Bereich, QLab-Position).
pub fn draw(ui: &mut egui::Ui, app: &mut ShowApp) {
    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button(sc("New Show", ShortcutAction::NewShow)).clicked() {
                app.new_show();
                ui.close_menu();
            }
            if ui
                .button(sc("Open Show…", ShortcutAction::OpenShow))
                .clicked()
            {
                app.open_show_dialog(ui.ctx());
                ui.close_menu();
            }
            ui.separator();
            if ui
                .button(sc("Save Show", ShortcutAction::SaveShow))
                .clicked()
            {
                app.save_show_dialog(ui.ctx());
                ui.close_menu();
            }
            if ui
                .button(sc("Save Show As…", ShortcutAction::SaveShowAs))
                .clicked()
            {
                app.save_show_as_dialog(ui.ctx());
                ui.close_menu();
            }
            ui.separator();
            if ui.button(sc("Quit", ShortcutAction::Quit)).clicked() {
                app.quit(ui.ctx());
                ui.close_menu();
            }
        });
        ui.menu_button("Edit", |ui| {
            let editable = !app.show_mode;
            let has_selection = app.carts.selected_index().is_some();
            if ui
                .add_enabled(editable, egui::Button::new("Add Cue"))
                .clicked()
            {
                app.add_cue(CueType::Audio);
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    editable && has_selection,
                    egui::Button::new(sc("Delete Cue", ShortcutAction::DeleteCue)),
                )
                .clicked()
            {
                app.delete_selected();
                ui.close_menu();
            }
            ui.separator();
            if ui
                .add_enabled(
                    editable && has_selection,
                    egui::Button::new(sc("Move Up", ShortcutAction::MoveCueUp)),
                )
                .clicked()
            {
                app.move_selected(-1);
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    editable && has_selection,
                    egui::Button::new(sc("Move Down", ShortcutAction::MoveCueDown)),
                )
                .clicked()
            {
                app.move_selected(1);
                ui.close_menu();
            }
            ui.separator();
            if let Some(cue) = app.carts.selected() {
                let armed = cue.is_armed;
                if ui
                    .add_enabled(
                        editable,
                        egui::Button::new(if armed { "Disarm Cue" } else { "Arm Cue" }),
                    )
                    .clicked()
                {
                    app.set_selected_armed(!armed);
                    ui.close_menu();
                }
            }
            if ui
                .add_enabled(
                    editable && has_selection,
                    egui::Button::new(sc("Duplicate", ShortcutAction::DuplicateCues)),
                )
                .clicked()
            {
                let ids = app.carts.active().selection().to_vec();
                app.duplicate_cues(&ids);
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    editable && has_selection,
                    egui::Button::new(sc("Group Cues", ShortcutAction::GroupCues)),
                )
                .clicked()
            {
                let ids = app.carts.active().selection().to_vec();
                app.group_cues(&ids);
                ui.close_menu();
            }
            ui.separator();
            if ui.button(sc("Find…", ShortcutAction::Find)).clicked() {
                app.ui.find.open = true;
                ui.ctx()
                    .memory_mut(|m| m.request_focus(super::cue_table::find_bar_id()));
                ui.close_menu();
            }
            if ui
                .button(sc("Find Next", ShortcutAction::FindNext))
                .clicked()
            {
                if !app.ui.find.open {
                    app.ui.find.open = true;
                }
                app.find_next();
                ui.close_menu();
            }
        });
        ui.menu_button("View", |ui| {
            // Panel-Sichtbarkeit (QLab: Checkboxes, Zustand bleibt über
            // Neustarts erhalten – Persistenz via UiState). Labels mit
            // Kürzel aus der zentralen Tabelle.
            ui.checkbox(
                &mut app.ui.toolbox_visible,
                sc("Toolbox", ShortcutAction::ToggleToolbox),
            );
            ui.checkbox(&mut app.ui.masthead_visible, "Masthead");
            ui.checkbox(
                &mut app.ui.sidebar_visible,
                sc("Sidebar", ShortcutAction::ToggleSidebar),
            );
            ui.checkbox(
                &mut app.ui.inspector_visible,
                sc("Inspector", ShortcutAction::ToggleInspector),
            );
        });
        ui.menu_button("Cues", |ui| {
            let editable = !app.show_mode;
            // Playback-Aktionen (sonst nur Masthead/Sidebar-Buttons).
            if ui
                .add_enabled(true, egui::Button::new(sc("GO", ShortcutAction::Go)))
                .clicked()
            {
                app.go_now();
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    true,
                    egui::Button::new(sc("Panic All", ShortcutAction::PanicAll)),
                )
                .clicked()
            {
                app.panic();
                ui.close_menu();
            }
            ui.separator();
            if ui
                .add_enabled(
                    true,
                    egui::Button::new(sc("Pause All", ShortcutAction::PauseAll)),
                )
                .clicked()
            {
                app.pause_all();
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    true,
                    egui::Button::new(sc("Resume All", ShortcutAction::ResumeAll)),
                )
                .clicked()
            {
                app.resume_all();
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    true,
                    egui::Button::new(sc("Reset All", ShortcutAction::ResetAll)),
                )
                .clicked()
            {
                app.reset_all();
                ui.close_menu();
            }
            ui.separator();
            for cue_type in CueType::ALL {
                let label = format!("New {} Cue", cue_type.label());
                let response = ui.add_enabled(
                    editable,
                    egui::Button::new(sc(&label, ShortcutAction::NewCue(cue_type)))
                        .min_size(vec2(210.0, 22.0)),
                );
                // Vektor-Icon links im Menü-Eintrag (QLab-Type-Icon).
                icons::paint_at(
                    ui,
                    Icon::for_cue_type(cue_type),
                    egui::Rect::from_min_size(response.rect.min + vec2(8.0, 3.5), vec2(15.0, 15.0)),
                    ui.visuals().text_color(),
                );
                if response.clicked() {
                    app.add_cue(cue_type);
                    ui.close_menu();
                }
            }
        });
        ui.menu_button("Workspace", |ui| {
            ui.checkbox(&mut app.audition_mode, "Audition Mode")
                .on_hover_text(
                    "Audition Mode: video cues play without a stage target, audio stays normal",
                );
            ui.separator();
            if ui
                .button(sc("Workspace Settings…", ShortcutAction::WorkspaceSettings))
                .clicked()
            {
                app.ui.workspace_settings_open = true;
                ui.close_menu();
            }
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Keyboard Shortcuts").clicked() {
                app.ui.shortcuts_open = true;
                ui.close_menu();
            }
            ui.separator();
            if ui.button("About KuhPlay").clicked() {
                app.ui.about_open = true;
                ui.close_menu();
            }
        });
    });
}
