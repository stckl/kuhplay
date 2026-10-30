//! Zentrale Tastenkürzel-Tabelle (TODO.md Phase 5).
//!
//! **Eine Quelle der Wahrheit:** Menüs zeigen ihre Kürzel über
//! [`menu_label`] aus dieser Tabelle an, [`ShowApp::handle_hotkeys`]
//! (in `app.rs`) liest ebenfalls daraus, und das „Keyboard
//! Shortcuts"-Fenster (Help-Menü) rendert die Tabelle 1:1. Menüanzeige
//! und Verhalten können so nie auseinanderlaufen.
//!
//! Die Belegung folgt dem QLab-5-Handbuch („Keyboard Shortcuts",
//! https://qlab.app/docs/v5/general/keyboard-shortcuts/):
//!
//! * Space = GO, Esc = Panic All, `[` = Pause All, `]` = Resume All
//! * ⌘0–⌘9 = neue Cues (QLab: Group/Audio/Mic/Video/Camera/Text/Light/
//!   Fade/Network/MIDI – KuhPlay belegt die Ziffern mit seinen eigenen
//!   Cue-Typen, Abweichungen siehe `docs/qlab5-parity.md`)
//! * ⌘I/⌘K/⌘L = Inspector/Toolbox/Lists & Carts, ⌘F = Find, ⌘G = Find Next
//! * ⌘N/⌘O/⌘S/⇧⌘S/⌘, /⌘Q = Datei-Menü
//!
//! Abweichungen (z. B. ⌘→Ctrl auf Linux/Windows – egui bildet
//! `Modifiers::command` plattformadaptiv ab, siehe `matches`) sind in
//! `docs/qlab5-parity.md` dokumentiert.

use egui::{Key, Modifiers, RichText};

use crate::core::CueType;

/// Eine Aktion, die über ein Tastenkürzel ausgelöst werden kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    /// GO (nächster armed Cue).
    Go,
    /// Panic: alles sofort stoppen (QLab: Esc, Doppel-Esc = Hard Stop –
    /// in KuhPlay nicht abgebildet, siehe `docs/qlab5-parity.md`).
    PanicAll,
    /// Pause All (QLab: `[`).
    PauseAll,
    /// Resume All (QLab: `]`).
    ResumeAll,
    /// Reset All (QLab hat keine Standard-Taste – Menü-only, `key: None`).
    ResetAll,
    /// Playhead eine Zeile nach oben (nackte Pfeiltaste; QLab nutzt dafür
    /// ⌘↑ – Abweichung dokumentiert).
    PlayheadUp,
    /// Playhead eine Zeile nach unten.
    PlayheadDown,
    /// Selektierten Cue nach oben verschieben (⌘↑ und ⌥⌘↑, QLab: ⌥⌘↑).
    MoveCueUp,
    /// Selektierten Cue nach unten verschieben (⌘↓ und ⌥⌘↓).
    MoveCueDown,
    /// Neuen Cue des Typs erzeugen (QLab Cues-Menü: ⌘0–⌘9).
    NewCue(CueType),
    /// Inspector ein-/ausblenden (⌘I).
    ToggleInspector,
    /// Sidebar ein-/ausblenden (⌘L, QLab: „Lists / Carts & Active Cues").
    ToggleSidebar,
    /// Toolbox ein-/ausblenden (⌘K).
    ToggleToolbox,
    /// Neuer Show (⌘N).
    NewShow,
    /// Show öffnen (⌘O).
    OpenShow,
    /// Show speichern (⌘S).
    SaveShow,
    /// Show speichern unter… (⇧⌘S).
    SaveShowAs,
    /// Workspace Settings öffnen (⌘,).
    WorkspaceSettings,
    /// Programm beenden (⌘Q).
    Quit,
    /// Find-Leiste öffnen (⌘F).
    Find,
    /// Nächster Find-Treffer (⌘G; zusätzlich Enter in der Find-Leiste).
    FindNext,
    /// Selektierten Cue löschen – nur Edit-Mode (⌘⌫ sowie ⌫/Delete;
    /// QLab: ⌘⌫).
    DeleteCue,
    /// Selektion duplizieren (⌘D; QLab 5 nutzt ⌘D für „Delete numbers" –
    /// KuhPlay hat diesen Befehl nicht, Abweichung dokumentiert).
    DuplicateCues,
    /// Selektion zu Group-Cue gruppieren (QLab 5: kein Standard-Kürzel,
    /// ⌘G ist Find Next – daher Menü-only, `key: None`).
    GroupCues,
}

/// Ein Tastenkürzel: Aktion + Taste + Modifikatoren + Menü-Label.
///
/// `key: None` = kein Standard-Kürzel (Eintrag erscheint nur im Menü und
/// im Shortcuts-Fenster mit „–“).
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    /// Gruppe für das Shortcuts-Fenster (z. B. „Playback“).
    pub group: &'static str,
    /// Ausgelöste Aktion.
    pub action: ShortcutAction,
    /// Taste (`None` = kein Standard-Kürzel).
    pub key: Option<Key>,
    /// Modifikatoren (⌘ = `command`, plattformadaptiv Ctrl auf
    /// Linux/Windows; Vergleich in [`Shortcut::matches`]).
    pub modifiers: Modifiers,
    /// Anzeige-Label (Menü und Shortcuts-Fenster).
    pub label: &'static str,
}

/// Modifikatoren-Combination als const fn (für die statische Tabelle).
const fn mods(command: bool, alt: bool, shift: bool) -> Modifiers {
    Modifiers {
        alt,
        ctrl: false,
        shift,
        mac_cmd: false,
        command,
    }
}

/// Die zentrale Tabelle aller Tastenkürzel (Reihenfolge = Anzeige im
/// Shortcuts-Fenster, gruppiert über `group`).
pub const ALL: &[Shortcut] = &[
    // --- Playback -----------------------------------------------------
    Shortcut {
        group: "Playback",
        action: ShortcutAction::Go,
        key: Some(Key::Space),
        modifiers: mods(false, false, false),
        label: "GO",
    },
    Shortcut {
        group: "Playback",
        action: ShortcutAction::PanicAll,
        key: Some(Key::Escape),
        modifiers: mods(false, false, false),
        label: "Panic All",
    },
    Shortcut {
        group: "Playback",
        action: ShortcutAction::PauseAll,
        key: Some(Key::OpenBracket),
        modifiers: mods(false, false, false),
        label: "Pause All",
    },
    Shortcut {
        group: "Playback",
        action: ShortcutAction::ResumeAll,
        key: Some(Key::CloseBracket),
        modifiers: mods(false, false, false),
        label: "Resume All",
    },
    Shortcut {
        group: "Playback",
        action: ShortcutAction::ResetAll,
        key: None,
        modifiers: mods(false, false, false),
        label: "Reset All",
    },
    // --- Navigation ---------------------------------------------------
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::PlayheadUp,
        key: Some(Key::ArrowUp),
        modifiers: mods(false, false, false),
        label: "Playhead Up",
    },
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::PlayheadDown,
        key: Some(Key::ArrowDown),
        modifiers: mods(false, false, false),
        label: "Playhead Down",
    },
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::MoveCueUp,
        key: Some(Key::ArrowUp),
        modifiers: mods(true, false, false),
        label: "Move Cue Up",
    },
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::MoveCueUp,
        key: Some(Key::ArrowUp),
        modifiers: mods(true, true, false),
        label: "Move Cue Up",
    },
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::MoveCueDown,
        key: Some(Key::ArrowDown),
        modifiers: mods(true, false, false),
        label: "Move Cue Down",
    },
    Shortcut {
        group: "Navigation",
        action: ShortcutAction::MoveCueDown,
        key: Some(Key::ArrowDown),
        modifiers: mods(true, true, false),
        label: "Move Cue Down",
    },
    // --- Panels (View) -------------------------------------------------
    Shortcut {
        group: "Panels",
        action: ShortcutAction::ToggleInspector,
        key: Some(Key::I),
        modifiers: mods(true, false, false),
        label: "Inspector",
    },
    Shortcut {
        group: "Panels",
        action: ShortcutAction::ToggleToolbox,
        key: Some(Key::K),
        modifiers: mods(true, false, false),
        label: "Toolbox",
    },
    Shortcut {
        group: "Panels",
        action: ShortcutAction::ToggleSidebar,
        key: Some(Key::L),
        modifiers: mods(true, false, false),
        label: "Sidebar (Lists / Carts)",
    },
    // --- File ----------------------------------------------------------
    Shortcut {
        group: "File",
        action: ShortcutAction::NewShow,
        key: Some(Key::N),
        modifiers: mods(true, false, false),
        label: "New Show",
    },
    Shortcut {
        group: "File",
        action: ShortcutAction::OpenShow,
        key: Some(Key::O),
        modifiers: mods(true, false, false),
        label: "Open Show…",
    },
    Shortcut {
        group: "File",
        action: ShortcutAction::SaveShow,
        key: Some(Key::S),
        modifiers: mods(true, false, false),
        label: "Save Show",
    },
    Shortcut {
        group: "File",
        action: ShortcutAction::SaveShowAs,
        key: Some(Key::S),
        modifiers: mods(true, false, true),
        label: "Save Show As…",
    },
    Shortcut {
        group: "File",
        action: ShortcutAction::WorkspaceSettings,
        key: Some(Key::Comma),
        modifiers: mods(true, false, false),
        label: "Workspace Settings…",
    },
    Shortcut {
        group: "File",
        action: ShortcutAction::Quit,
        key: Some(Key::Q),
        modifiers: mods(true, false, false),
        label: "Quit",
    },
    // --- Edit ----------------------------------------------------------
    Shortcut {
        group: "Edit",
        action: ShortcutAction::Find,
        key: Some(Key::F),
        modifiers: mods(true, false, false),
        label: "Find…",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::FindNext,
        key: Some(Key::G),
        modifiers: mods(true, false, false),
        label: "Find Next",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::DeleteCue,
        key: Some(Key::Backspace),
        modifiers: mods(true, false, false),
        label: "Delete Cue",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::DeleteCue,
        key: Some(Key::Backspace),
        modifiers: mods(false, false, false),
        label: "Delete Cue",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::DeleteCue,
        key: Some(Key::Delete),
        modifiers: mods(false, false, false),
        label: "Delete Cue",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::DuplicateCues,
        key: Some(Key::D),
        modifiers: mods(true, false, false),
        label: "Duplicate",
    },
    Shortcut {
        group: "Edit",
        action: ShortcutAction::GroupCues,
        key: None,
        modifiers: mods(false, false, false),
        label: "Group Cues",
    },
    // --- Cues (neue Cue-Typen; QLab 5 Cues-Menü: ⌘0–⌘9) ----------------
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Group),
        key: Some(Key::Num0),
        modifiers: mods(true, false, false),
        label: "New Group Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Audio),
        key: Some(Key::Num1),
        modifiers: mods(true, false, false),
        label: "New Audio Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Video),
        key: Some(Key::Num2),
        modifiers: mods(true, false, false),
        label: "New Video Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Fade),
        key: Some(Key::Num3),
        modifiers: mods(true, false, false),
        label: "New Fade Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Stop),
        key: Some(Key::Num4),
        modifiers: mods(true, false, false),
        label: "New Stop Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Script),
        key: Some(Key::Num5),
        modifiers: mods(true, false, false),
        label: "New Script Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Control),
        key: Some(Key::Num6),
        modifiers: mods(true, false, false),
        label: "New Control Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::Html),
        key: Some(Key::Num7),
        modifiers: mods(true, false, false),
        label: "New HTML5 Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::SlideShow),
        key: Some(Key::Num8),
        modifiers: mods(true, false, false),
        label: "New Slideshow Cue",
    },
    Shortcut {
        group: "Cues",
        action: ShortcutAction::NewCue(CueType::HtmlData),
        key: Some(Key::Num9),
        modifiers: mods(true, false, false),
        label: "New HTML Data Cue",
    },
];

impl Shortcut {
    /// Passt dieses Kürzel auf das gedrückte `key` mit Modifikatoren `m`?
    /// Vergleicht nur command/alt/shift/ctrl – `command` ist in egui
    /// plattformadaptiv (⌘ auf macOS, Ctrl auf Linux/Windows).
    pub fn matches(&self, key: Key, m: &Modifiers) -> bool {
        self.key == Some(key)
            && m.command == self.modifiers.command
            && m.alt == self.modifiers.alt
            && m.shift == self.modifiers.shift
            && m.ctrl == self.modifiers.ctrl
    }

    /// Anzeige des Kürzels, z. B. „Shift+⌘S“, „Space“, „Backspace“, „Up“ („–“ bei `None`).
    pub fn display(&self) -> String {
        let Some(key) = self.key else {
            return "–".to_string();
        };
        let mut s = String::new();
        if self.modifiers.ctrl {
            s.push('⌃');
        }
        if self.modifiers.alt {
            s.push_str("Opt+");
        }
        if self.modifiers.shift {
            s.push_str("Shift+");
        }
        if self.modifiers.command {
            s.push('⌘');
        }
        s.push_str(&key_name(key));
        s
    }
}

/// Anzeigename einer Taste. Nur Symbole, die der UI-Font enthält (Test
/// `shortcut_labels_use_available_glyphs`) – sonst erscheinen Kästchen.
fn key_name(key: Key) -> String {
    match key {
        Key::Space => "Space".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::ArrowUp => "Up".to_string(),
        Key::ArrowDown => "Down".to_string(),
        Key::ArrowLeft => "Left".to_string(),
        Key::ArrowRight => "Right".to_string(),
        Key::Comma => ",".to_string(),
        Key::OpenBracket => "[".to_string(),
        Key::CloseBracket => "]".to_string(),
        Key::Num0 => "0".to_string(),
        Key::Num1 => "1".to_string(),
        Key::Num2 => "2".to_string(),
        Key::Num3 => "3".to_string(),
        Key::Num4 => "4".to_string(),
        Key::Num5 => "5".to_string(),
        Key::Num6 => "6".to_string(),
        Key::Num7 => "7".to_string(),
        Key::Num8 => "8".to_string(),
        Key::Num9 => "9".to_string(),
        k if (k as u8) >= (Key::A as u8) && (k as u8) <= (Key::Z as u8) => {
            let n = k as u8 - Key::A as u8;
            ((b'A' + n) as char).to_string()
        }
        other => format!("{other:?}"),
    }
}

/// Erstes Kürzel (mit Taste) zu einer Aktion – für die Menüanzeige.
pub fn shortcut_for(action: ShortcutAction) -> Option<&'static Shortcut> {
    ALL.iter().find(|s| s.action == action && s.key.is_some())
}

/// Menü-Label mit angehängtem Kürzel aus der Tabelle, z. B.
/// „Open Show…  ⌘O“ (zweispaltig wie in QLab-Menüs; ohne Kürzel nur
/// das Label).
pub fn menu_label(label: &str, action: ShortcutAction) -> String {
    match shortcut_for(action).map(|s| s.display()) {
        Some(display) => format!("{label}  {display}"),
        None => label.to_string(),
    }
}

/// „Keyboard Shortcuts"-Fenster (Help-Menü, TODO.md Phase 5): rendert
/// die Tabelle gruppiert – dieselbe Quelle wie Menüs und `handle_hotkeys`.
pub fn draw_window(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    let mut still_open = true;
    egui::Window::new("Keyboard Shortcuts")
        .collapsible(false)
        .resizable(true)
        .default_width(430.0)
        // Zentriert öffnen statt in die freie Ecke unter den Panels
        // (dort wäre die Liste unten abgeschnitten).
        .pivot(egui::Align2::CENTER_TOP)
        .default_pos(ctx.screen_rect().center_top() + egui::vec2(0.0, 60.0))
        .constrain_to(ctx.screen_rect())
        .default_height(660.0)
        .max_height(ctx.screen_rect().height() - 100.0)
        .open(&mut still_open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(620.0)
                .show(ui, draw_table);
        });
    *open = still_open;
}

/// Tabelle aller Tastenkürzel (Help-Fenster und Workspace Settings, Key Map).
pub fn draw_table(ui: &mut egui::Ui) {
    let mut group = "";
    for (n, sc) in ALL.iter().enumerate() {
        if sc.group != group {
            group = sc.group;
            ui.add_space(4.0);
            ui.label(RichText::new(group).strong().size(13.0));
            ui.separator();
        }
        // Eindeutige Grid-ID pro Zeile (sonst kollidieren die Widget-IDs und
        // egui stapelt die Zeilen übereinander).
        egui::Grid::new(ui.id().with(("sc_grid", n)))
            .num_columns(2)
            .spacing([24.0, 3.0])
            .show(ui, |ui| {
                ui.label(sc.label);
                ui.label(RichText::new(sc.display()).weak());
                ui.end_row();
            });
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new("⌘ = Command (macOS), Ctrl (Linux/Windows)")
            .weak()
            .small(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_has_label_and_group() {
        for sc in ALL {
            assert!(!sc.label.is_empty(), "Label fehlt bei {:?}", sc.action);
            assert!(!sc.group.is_empty(), "Gruppe fehlt bei {:?}", sc.action);
        }
    }

    #[test]
    fn key_modifier_combinations_are_unique() {
        // Dieselbe Taste+Modifikatoren darf nur eine Aktion auslösen –
        // sonst wäre die Tabelle uneindeutig.
        let mut seen = std::collections::HashSet::new();
        for sc in ALL {
            if let Some(key) = sc.key {
                let m = &sc.modifiers;
                let k = (key, m.command, m.alt, m.shift, m.ctrl);
                assert!(
                    seen.insert(k),
                    "doppeltes Kürzel: {} {:?}",
                    sc.display(),
                    sc.action
                );
            }
        }
    }

    #[test]
    fn display_renders_qlab_style_symbols() {
        assert_eq!(shortcut_for(ShortcutAction::Go).unwrap().display(), "Space");
        assert_eq!(
            shortcut_for(ShortcutAction::PanicAll).unwrap().display(),
            "Esc"
        );
        assert_eq!(
            shortcut_for(ShortcutAction::OpenShow).unwrap().display(),
            "⌘O"
        );
        assert_eq!(
            shortcut_for(ShortcutAction::SaveShowAs).unwrap().display(),
            "Shift+⌘S"
        );
        assert_eq!(
            shortcut_for(ShortcutAction::PauseAll).unwrap().display(),
            "["
        );
        assert_eq!(
            shortcut_for(ShortcutAction::ResumeAll).unwrap().display(),
            "]"
        );
        assert_eq!(shortcut_for(ShortcutAction::Find).unwrap().display(), "⌘F");
        assert_eq!(
            shortcut_for(ShortcutAction::FindNext).unwrap().display(),
            "⌘G"
        );
        assert_eq!(
            shortcut_for(ShortcutAction::NewCue(CueType::Audio))
                .unwrap()
                .display(),
            "⌘1"
        );
        assert_eq!(
            shortcut_for(ShortcutAction::NewCue(CueType::Group))
                .unwrap()
                .display(),
            "⌘0"
        );
        // Reset All hat kein Standard-Kürzel (QLab 5 ebenfalls nicht) → „–“.
        let reset = ALL
            .iter()
            .find(|s| s.action == ShortcutAction::ResetAll)
            .unwrap();
        assert_eq!(reset.display(), "–");
        assert!(shortcut_for(ShortcutAction::ResetAll).is_none());
    }

    #[test]
    fn matches_respects_exact_modifiers() {
        let go = shortcut_for(ShortcutAction::Go).unwrap();
        assert!(go.matches(Key::Space, &Modifiers::NONE));
        assert!(!go.matches(Key::Space, &Modifiers::COMMAND));
        assert!(!go.matches(Key::Enter, &Modifiers::NONE));
        let save_as = shortcut_for(ShortcutAction::SaveShowAs).unwrap();
        assert!(save_as.matches(
            Key::S,
            &Modifiers {
                command: true,
                shift: true,
                ..Modifiers::NONE
            }
        ));
        assert!(!save_as.matches(Key::S, &Modifiers::COMMAND));
        let move_up_alt = ALL
            .iter()
            .find(|s| s.action == ShortcutAction::MoveCueUp && s.modifiers.alt)
            .unwrap();
        assert!(move_up_alt.matches(
            Key::ArrowUp,
            &Modifiers {
                command: true,
                alt: true,
                ..Modifiers::NONE
            }
        ));
    }

    #[test]
    fn menu_label_appends_shortcut() {
        assert_eq!(
            menu_label("Open Show…", ShortcutAction::OpenShow),
            "Open Show…  ⌘O"
        );
        // Ohne Standard-Kürzel bleibt das Label unverändert (kein „–“ im Menü).
        assert_eq!(
            menu_label("Reset All", ShortcutAction::ResetAll),
            "Reset All"
        );
        assert_eq!(
            menu_label("Group Cues", ShortcutAction::GroupCues),
            "Group Cues"
        );
    }

    #[test]
    fn qlab5_minimum_set_is_covered() {
        // TODO.md Phase 5: Space=GO, Esc=Panic, ↑/↓=Playhead, ⌘-Zahlen,
        // Pause/Resume/Reset All, ⌘I/⌘K/⌘L, ⌘F, Delete, ⌘D, Group.
        for action in [
            ShortcutAction::Go,
            ShortcutAction::PanicAll,
            ShortcutAction::PlayheadUp,
            ShortcutAction::PlayheadDown,
            ShortcutAction::PauseAll,
            ShortcutAction::ResumeAll,
            ShortcutAction::ResetAll,
            ShortcutAction::ToggleInspector,
            ShortcutAction::ToggleToolbox,
            ShortcutAction::ToggleSidebar,
            ShortcutAction::Find,
            ShortcutAction::DeleteCue,
            ShortcutAction::DuplicateCues,
            ShortcutAction::GroupCues,
        ] {
            assert!(ALL.iter().any(|s| s.action == action), "fehlt: {action:?}");
        }
        for t in CueType::ALL {
            assert!(
                ALL.iter()
                    .any(|s| s.action == ShortcutAction::NewCue(t) && s.key.is_some()),
                "fehlende ⌘-Ziffer für {t:?}"
            );
        }
    }
}
