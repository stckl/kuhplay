//! Native macOS-Menüleiste (muda). Ersetzt dort die egui-Menüleiste
//! [`super::menu_bar`], Inhalt und Reihenfolge entsprechen ihr.
//!
//! ## Tastenkürzel
//!
//! Ein Menü-Kürzel greift vor der App, also auch beim Tippen in Textfeldern.
//! Echte Kürzel bekommen daher nur `⌘`-Kombinationen, die dort nichts bedeuten.
//! Der Rest (Space, Esc, `⌘⌫`, `⌘↑`, …) läuft weiter über
//! `ShowApp::handle_hotkeys`, das Textfelder berücksichtigt, und steht nur als
//! Text im Eintrag.
//!
//! Kein Cut/Copy/Paste im Edit-Menü: Die nativen Einträge schicken Selektoren
//! an das Fenster, das sie nicht kennt, und würden die Tasten für egui-Textfelder
//! wegnehmen.

use std::collections::HashMap;
use std::str::FromStr;

use muda::accelerator::Accelerator;
use muda::{
    AboutMetadata, CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};

use crate::core::CueType;

use super::app::ShowApp;
use super::shortcuts::{shortcut_for, ShortcutAction};

/// Aktion eines Menüeintrags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Shortcut(ShortcutAction),
    AddCue,
    ToggleArmed,
    ToggleMasthead,
    ToggleAudition,
    KeyboardShortcuts,
    About,
}

/// Kürzel, die im Textfeld eine eigene Bedeutung haben und deshalb nicht als
/// Menü-Kürzel registriert werden.
fn reserved_for_text_fields(action: ShortcutAction) -> bool {
    matches!(
        action,
        ShortcutAction::DeleteCue
            | ShortcutAction::MoveCueUp
            | ShortcutAction::MoveCueDown
            | ShortcutAction::PlayheadUp
            | ShortcutAction::PlayheadDown
            | ShortcutAction::SelectPrevious
            | ShortcutAction::SelectNext
            | ShortcutAction::ToggleFlag
    )
}

/// Menü-Kürzel einer Aktion; `None`, wenn sie nur über `handle_hotkeys` läuft.
fn accelerator_for(action: ShortcutAction) -> Option<Accelerator> {
    if reserved_for_text_fields(action) {
        return None;
    }
    let shortcut = shortcut_for(action)?;
    if !shortcut.modifiers.command {
        return None;
    }
    let mut text = String::from("CmdOrCtrl+");
    if shortcut.modifiers.alt {
        text.push_str("Alt+");
    }
    if shortcut.modifiers.shift {
        text.push_str("Shift+");
    }
    text.push_str(shortcut.key?.name());
    Accelerator::from_str(&text).ok()
}

/// Eintragstext: Kürzel ohne echtes Menü-Kürzel stehen im Titel.
fn title_for(label: &str, action: ShortcutAction) -> String {
    match shortcut_for(action) {
        Some(shortcut) if accelerator_for(action).is_none() => {
            format!("{label}    {}", shortcut.display())
        }
        _ => label.to_string(),
    }
}

/// Die aufgebaute Menüleiste samt Einträgen, die ihren Zustand ändern.
pub struct NativeMenu {
    // Muss leben, solange das Menü angezeigt wird.
    _menu: Menu,
    commands: HashMap<MenuId, Command>,
    /// Einträge, die nur im Edit-Mode aktiv sind.
    edit_only: Vec<MenuItem>,
    /// Einträge, die zusätzlich eine Selektion brauchen.
    needs_selection: Vec<MenuItem>,
    armed: MenuItem,
    toolbox: CheckMenuItem,
    masthead: CheckMenuItem,
    sidebar: CheckMenuItem,
    inspector: CheckMenuItem,
    audition: CheckMenuItem,
}

/// Baut die Einträge und merkt sich pro Eintrag die Aktion.
#[derive(Default)]
struct Builder {
    commands: HashMap<MenuId, Command>,
}

impl Builder {
    fn plain(&mut self, label: &str, command: Command) -> MenuItem {
        let item = MenuItem::new(label, true, None);
        self.commands.insert(item.id().clone(), command);
        item
    }

    fn action(&mut self, label: &str, action: ShortcutAction) -> MenuItem {
        let item = MenuItem::new(title_for(label, action), true, accelerator_for(action));
        self.commands
            .insert(item.id().clone(), Command::Shortcut(action));
        item
    }

    fn check(
        &mut self,
        label: &str,
        command: Command,
        action: Option<ShortcutAction>,
    ) -> CheckMenuItem {
        let item = CheckMenuItem::new(label, true, false, action.and_then(accelerator_for));
        self.commands.insert(item.id().clone(), command);
        item
    }
}

impl NativeMenu {
    /// Baut die Menüleiste und setzt sie als Menü der App. Muss auf dem
    /// Main-Thread laufen, nachdem das Fenster existiert.
    pub fn install() -> Result<Self, muda::Error> {
        use ShortcutAction as A;
        let mut b = Builder::default();
        let separator = PredefinedMenuItem::separator;

        let settings = b.action("Settings…", A::WorkspaceSettings);
        let quit = b.action("Quit KuhPlay", A::Quit);
        let app_menu = Submenu::with_items(
            "KuhPlay",
            true,
            &[
                &PredefinedMenuItem::about(
                    Some("About KuhPlay"),
                    Some(AboutMetadata {
                        name: Some("KuhPlay".to_string()),
                        version: Some(env!("CARGO_PKG_VERSION").to_string()),
                        ..Default::default()
                    }),
                ),
                &separator(),
                &settings,
                &separator(),
                &PredefinedMenuItem::services(None),
                &separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &separator(),
                &quit,
            ],
        )?;

        let file = Submenu::with_items(
            "File",
            true,
            &[
                &b.action("New Show", A::NewShow),
                &b.action("Open Show…", A::OpenShow),
                &separator(),
                &b.action("Save Show", A::SaveShow),
                &b.action("Save Show As…", A::SaveShowAs),
            ],
        )?;

        let add_cue = b.plain("Add Cue", Command::AddCue);
        let delete = b.action("Delete Cue", A::DeleteCue);
        let move_up = b.action("Move Up", A::MoveCueUp);
        let move_down = b.action("Move Down", A::MoveCueDown);
        let armed = b.plain("Disarm Cue", Command::ToggleArmed);
        let duplicate = b.action("Duplicate", A::DuplicateCues);
        let group = b.action("Group Cues", A::GroupCues);
        let delete_numbers = b.action("Delete Cue Numbers", A::DeleteCueNumbers);
        let flag = b.action("Flag Cue", A::ToggleFlag);
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &add_cue,
                &delete,
                &separator(),
                &move_up,
                &move_down,
                &separator(),
                &armed,
                &duplicate,
                &group,
                &delete_numbers,
                &flag,
                &separator(),
                &b.action("Find…", A::Find),
                &b.action("Find Next", A::FindNext),
            ],
        )?;

        let toolbox = b.check(
            "Toolbox",
            Command::Shortcut(A::ToggleToolbox),
            Some(A::ToggleToolbox),
        );
        let masthead = b.check("Masthead", Command::ToggleMasthead, None);
        let sidebar = b.check(
            "Sidebar",
            Command::Shortcut(A::ToggleSidebar),
            Some(A::ToggleSidebar),
        );
        let inspector = b.check(
            "Inspector",
            Command::Shortcut(A::ToggleInspector),
            Some(A::ToggleInspector),
        );
        let view = Submenu::with_items("View", true, &[&toolbox, &masthead, &sidebar, &inspector])?;

        let cues = Submenu::with_items(
            "Cues",
            true,
            &[
                &b.action("GO", A::Go),
                &b.action("Panic All", A::PanicAll),
                &separator(),
                &b.action("Pause All", A::PauseAll),
                &b.action("Resume All", A::ResumeAll),
                &b.action("Reset All", A::ResetAll),
                &separator(),
            ],
        )?;
        let mut new_cues = Vec::new();
        for cue_type in CueType::ALL {
            let item = b.action(
                &format!("New {} Cue", cue_type.label()),
                A::NewCue(cue_type),
            );
            cues.append(&item)?;
            new_cues.push(item);
        }

        let audition = b.check("Audition Mode", Command::ToggleAudition, None);
        let workspace = Submenu::with_items(
            "Workspace",
            true,
            &[
                &audition,
                &separator(),
                &b.plain(
                    "Workspace Settings…",
                    Command::Shortcut(A::WorkspaceSettings),
                ),
            ],
        )?;

        let window = Submenu::with_items(
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::fullscreen(None),
            ],
        )?;
        let help = Submenu::with_items(
            "Help",
            true,
            &[
                &b.plain("Keyboard Shortcuts", Command::KeyboardShortcuts),
                &separator(),
                &b.plain("About KuhPlay", Command::About),
            ],
        )?;

        let menu = Menu::with_items(&[
            &app_menu, &file, &edit, &view, &cues, &workspace, &window, &help,
        ])?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();

        let mut edit_only = vec![add_cue];
        edit_only.extend(new_cues);
        Ok(Self {
            _menu: menu,
            commands: b.commands,
            edit_only,
            needs_selection: vec![delete, move_up, move_down, duplicate, group],
            armed,
            toolbox,
            masthead,
            sidebar,
            inspector,
            audition,
        })
    }

    /// Gleicht Häkchen, Aktivierung und den Arm-Eintrag mit dem App-Zustand ab.
    fn sync(&self, app: &ShowApp) {
        fn enable(item: &MenuItem, enabled: bool) {
            if item.is_enabled() != enabled {
                item.set_enabled(enabled);
            }
        }
        fn check(item: &CheckMenuItem, checked: bool) {
            if item.is_checked() != checked {
                item.set_checked(checked);
            }
        }
        let editable = !app.show_mode;
        let selected = app.carts.selected();
        for item in &self.edit_only {
            enable(item, editable);
        }
        for item in &self.needs_selection {
            enable(item, editable && selected.is_some());
        }
        enable(&self.armed, editable && selected.is_some());
        let title = match selected {
            Some(cue) if !cue.is_armed => "Arm Cue",
            _ => "Disarm Cue",
        };
        if self.armed.text() != title {
            self.armed.set_text(title);
        }
        check(&self.toolbox, app.ui.toolbox_visible);
        check(&self.masthead, app.ui.masthead_visible);
        check(&self.sidebar, app.ui.sidebar_visible);
        check(&self.inspector, app.ui.inspector_visible);
        check(&self.audition, app.audition_mode);
    }
}

impl ShowApp {
    /// Installiert die Menüleiste. Schlägt das fehl, bleibt die egui-Menüleiste.
    pub fn install_native_menu(&mut self) {
        match NativeMenu::install() {
            Ok(menu) => self.native_menu = Some(menu),
            Err(err) => tracing::warn!(%err, "native menu unavailable"),
        }
    }

    /// Führt angeklickte Menüeinträge aus und aktualisiert den Menüzustand.
    pub(super) fn poll_native_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.native_menu.take() else {
            return;
        };
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(command) = menu.commands.get(event.id()) {
                self.run_menu_command(*command, ctx);
            }
        }
        menu.sync(self);
        self.native_menu = Some(menu);
    }

    fn run_menu_command(&mut self, command: Command, ctx: &egui::Context) {
        use ShortcutAction as A;
        let editable = !self.show_mode;
        match command {
            Command::AddCue if editable => self.add_cue(CueType::Audio),
            Command::ToggleArmed if editable => {
                if let Some(armed) = self.carts.selected().map(|cue| cue.is_armed) {
                    self.set_selected_armed(!armed);
                }
            }
            Command::AddCue | Command::ToggleArmed => {}
            Command::ToggleMasthead => self.ui.masthead_visible = !self.ui.masthead_visible,
            Command::ToggleAudition => self.audition_mode = !self.audition_mode,
            Command::KeyboardShortcuts => self.ui.shortcuts_open = true,
            Command::About => self.ui.about_open = true,
            Command::Shortcut(action) => match action {
                A::Go => self.go_now(),
                A::PanicAll => self.panic(),
                A::PauseAll => self.pause_all(),
                A::ResumeAll => self.resume_all(),
                A::ResetAll => self.reset_all(),
                A::NewShow => self.new_show(),
                A::OpenShow => self.open_show_dialog(ctx),
                A::SaveShow => self.save_show_dialog(ctx),
                A::SaveShowAs => self.save_show_as_dialog(ctx),
                A::Quit => self.quit(ctx),
                A::WorkspaceSettings => self.ui.workspace_settings_open = true,
                A::ToggleInspector => self.ui.inspector_visible = !self.ui.inspector_visible,
                A::ToggleSidebar => self.ui.sidebar_visible = !self.ui.sidebar_visible,
                A::ToggleToolbox => self.ui.toolbox_visible = !self.ui.toolbox_visible,
                A::Find => {
                    self.ui.find.open = true;
                    ctx.memory_mut(|m| m.request_focus(super::cue_table::find_bar_id()));
                }
                A::FindNext => {
                    self.ui.find.open = true;
                    self.find_next();
                }
                A::NewCue(cue_type) if editable => self.add_cue(cue_type),
                A::DeleteCue if editable => self.delete_selected(),
                A::MoveCueUp if editable => self.move_selected(-1),
                A::MoveCueDown if editable => self.move_selected(1),
                A::DuplicateCues if editable => {
                    let ids = self.carts.active().selection().to_vec();
                    self.duplicate_cues(&ids);
                }
                A::DeleteCueNumbers if editable => self.delete_selected_numbers(),
                A::ToggleFlag if editable => self.toggle_selected_flags(),
                A::GroupCues if editable => {
                    let ids = self.carts.active().selection().to_vec();
                    self.group_cues(&ids);
                }
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::shortcuts::ALL;

    #[test]
    fn command_shortcuts_become_accelerators() {
        for shortcut in ALL {
            if shortcut.key.is_some()
                && shortcut.modifiers.command
                && !reserved_for_text_fields(shortcut.action)
            {
                assert!(
                    accelerator_for(shortcut.action).is_some(),
                    "{} lässt sich nicht registrieren",
                    shortcut.display()
                );
            }
        }
    }

    #[test]
    fn plain_keys_and_text_keys_stay_out_of_the_menu() {
        for action in [
            ShortcutAction::Go,
            ShortcutAction::PanicAll,
            ShortcutAction::PauseAll,
            ShortcutAction::DeleteCue,
            ShortcutAction::MoveCueUp,
        ] {
            assert!(accelerator_for(action).is_none(), "{action:?}");
        }
        assert!(title_for("GO", ShortcutAction::Go).contains("Space"));
        assert_eq!(
            title_for("Save Show", ShortcutAction::SaveShow),
            "Save Show"
        );
    }
}
