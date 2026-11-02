//! Reiner UI-Zustand (Panel-Sichtbarkeit, aktive Tabs, Fenster) als
//! serde-fähiger Struct – Persistenz des Workspace-Layouts via
//! `eframe::Storage` (Workspace-Einstellungen bleiben erhalten).
//!
//! docs/TODO-offen.md Phase 0b („`ShowApp` ist ein God-Object"): Die UI-Felder wandern
//! aus `ShowApp` in diesen eigenen Struct; `ShowApp::ui` ist die eine Quelle.

use serde::{Deserialize, Serialize};

use super::app::{InspectorTab, SidebarTab};
use super::workspace_settings::SettingsPage;

/// Storage-Key des UI-Zustands in `eframe::Storage`.
pub const UI_STATE_STORAGE_KEY: &str = "kuhplay.ui_state";

/// Storage-Key der Workspace-Templates („Workspace Templates") in
/// `eframe::Storage` – benannte Layout-Vorlagen (Snapshots des [`UiState`]).
pub const UI_TEMPLATES_STORAGE_KEY: &str = "kuhplay.ui_templates";

/// Anzahl der Spalten der Cue-Tabelle (docs/TODO-offen.md Phase 3).
/// Reihenfolge: Status | Type | Q# | Name | Target | Pre-wait | Duration |
/// Post-wait | Continue.
pub const CUE_COLUMN_COUNT: usize = 9;

/// Spalten-Titel der Cue-Tabelle (Index = Spalte; leer = kein Text,
/// die Continue-Spalte zeigt ein Icon, siehe `cue_table`).
pub const CUE_COLUMN_TITLES: [&str; CUE_COLUMN_COUNT] = [
    "",
    "Type",
    "Q#",
    "Name",
    "Target",
    "Pre-wait",
    "Duration",
    "Post-wait",
    "",
];

/// Persistiertes Spalten-Layout der Cue-Tabelle (docs/TODO-offen.md Phase 3):
/// veränderbare Breiten + ein-/ausblendbare Spalten (Rechtsklick
/// auf den Header). Breiten werden pro Frame aus der Tabelle zurückgelesen
/// (`TableRow::widths`) und landen so in der Persistenz.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CueColumnLayout {
    /// Persistierte Breiten der festen Spalten (Index = Spalte). Die
    /// Name-Spalte (Index 3) ist `remainder` und wird nicht persistiert.
    pub widths: [f32; CUE_COLUMN_COUNT],
    /// Sichtbarkeit pro Spalte.
    pub hidden: [bool; CUE_COLUMN_COUNT],
    /// Sichtbarkeit des Vor-Frames (nur Laufzeit, nicht persistiert) –
    /// bei Änderung wird der egui_extras-Table-State zurückgesetzt, damit
    /// wieder eingeblendete Spalten ihre persistierte Breite bekommen.
    #[serde(skip)]
    pub prev_hidden: [bool; CUE_COLUMN_COUNT],
}

impl Default for CueColumnLayout {
    fn default() -> Self {
        Self {
            // Default-Breiten (Name = remainder, Breite hier ungenutzt).
            widths: [46.0, 38.0, 60.0, 0.0, 150.0, 84.0, 84.0, 84.0, 48.0],
            hidden: [false; CUE_COLUMN_COUNT],
            prev_hidden: [false; CUE_COLUMN_COUNT],
        }
    }
}

impl CueColumnLayout {
    /// Stellt die Default-Breiten wieder her und blendet alle Spalten ein.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Persistierter UI-Zustand (Workspace-Layout).
///
/// `#[serde(default)]` auf Container-Ebene: fehlende Felder laden als
/// Defaults (abwärtskompatibel zu älteren/leereren Persistenz-Daten).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
    /// Sichtbarkeit Masthead (ein-/ausblendbar).
    pub masthead_visible: bool,
    /// Höhe des Mastheads in px (F1.5: per Drag änderbar, persistiert).
    pub masthead_height: f32,
    /// Sichtbarkeit Toolbar im Masthead.
    pub toolbar_visible: bool,
    /// Eigene Reihenfolge der Cue-Typen in Toolbox und Toolbar.
    pub toolbox_order: Vec<crate::core::CueType>,
    /// Sichtbarkeit Toolbox (linke Leiste, default: sichtbar).
    pub toolbox_visible: bool,
    /// Sichtbarkeit Sidebar (rechte Leiste).
    pub sidebar_visible: bool,
    /// Sichtbarkeit Inspector (unteres Panel).
    pub inspector_visible: bool,
    /// Aktiver Sidebar-Tab.
    pub sidebar_tab: SidebarTab,
    /// Aktiver Inspector-Tab.
    pub inspector_tab: InspectorTab,
    /// „Workspace Settings"-Fenster geöffnet.
    pub workspace_settings_open: bool,
    /// Aktive Seite im Workspace-Settings-Fenster.
    pub workspace_settings_page: SettingsPage,
    /// About-Fenster geöffnet.
    pub about_open: bool,
    /// „Keyboard Shortcuts"-Fenster geöffnet (docs/TODO-offen.md Phase 5).
    pub shortcuts_open: bool,
    /// „File Target Search"-Fenster geöffnet (Tools-Menü).
    pub target_search_open: bool,
    /// Audition-Fenster geöffnet („Audition Window").
    pub audition_window_open: bool,
    /// „Workspace Status"-Fenster geöffnet (Tools-Menü).
    pub status_window_open: bool,
    /// Filter-Query des File-Target-Search-Fensters (reiner Laufzeit-Zustand).
    #[serde(skip)]
    pub target_search_query: String,
    /// Test-Grid über allen Regionen der Stages an (Footer-Icon; im Show-Mode
    /// immer aus).
    pub test_grid: bool,
    /// Find-Leiste über der Cue-Liste (docs/TODO-offen.md Phase 5; reiner
    /// Laufzeit-Zustand, wird nicht persistiert).
    #[serde(skip)]
    pub find: FindState,
    /// Spalten-Layout der Cue-Tabelle (Breiten + Sichtbarkeit, Phase 3).
    pub cue_columns: CueColumnLayout,
    /// Unterseite von Settings → Video.
    #[serde(skip)]
    pub video_tab: super::workspace_settings::VideoTab,
    /// Update-Dialog geöffnet (Block U4).
    #[serde(default)]
    pub update_dialog_open: bool,
    /// Automatische Update-Prüfung beim Start (Block U4; für Offline-Shows
    /// abschaltbar). Persistiert im eframe-Storage.
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
}

fn default_true() -> bool {
    true
}

/// Laufzeit-Zustand der Find-Leiste über der Cue-Liste (Phase 5).
///
/// Nicht persistiert (`#[serde(skip)]` auf dem UiState-Feld): Suchbegriff
/// und Treffer-Position sind Session-Zustand.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FindState {
    /// Leiste sichtbar (⌘F toggelt).
    pub open: bool,
    /// Suchbegriff (matcht Nummer, Name, Notes – case-insensitive).
    pub query: String,
    /// Wie viele Treffer bereits angesprungen wurden (0 = noch keiner);
    /// Enter/⌘G erhöht und wrappt via Modulo.
    pub current: usize,
    /// Query des letzten `find_next_hit`-Aufrufs – bei Änderung wird
    /// `current` zurückgesetzt.
    pub query_prev: String,
}

impl Default for UiState {
    /// Default-Layout: alle Panels sichtbar, Toolbox eingeblendet.
    fn default() -> Self {
        Self {
            video_tab: Default::default(),
            masthead_visible: true,
            masthead_height: crate::gui::theme::size::MASTHEAD_HEIGHT,
            toolbar_visible: true,
            toolbox_order: Vec::new(),
            toolbox_visible: true,
            sidebar_visible: true,
            inspector_visible: true,
            sidebar_tab: SidebarTab::ActiveCues,
            inspector_tab: InspectorTab::Basics,
            workspace_settings_open: false,
            workspace_settings_page: SettingsPage::General,
            about_open: false,
            shortcuts_open: false,
            target_search_open: false,
            audition_window_open: false,
            status_window_open: false,
            update_dialog_open: false,
            auto_check_updates: true,
            target_search_query: String::new(),
            test_grid: false,
            find: FindState::default(),
            cue_columns: CueColumnLayout::default(),
        }
    }
}

impl UiState {
    /// Serialisiert den Zustand als JSON (für `eframe::Storage`).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Lädt den Zustand aus JSON; ungültig/fehlend → Defaults.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }
}

/// Eine benannte Workspace-Vorlage („Workspace Templates"): Snapshot
/// des UI-Layouts, das jederzeit wieder angewendet werden kann.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceTemplate {
    /// Anzeigename der Vorlage.
    pub name: String,
    /// Gespeichertes Layout (Panel-Sichtbarkeit, Tabs, Spaltenbreiten …).
    pub layout: UiState,
}

/// Serialisiert die Template-Liste als JSON (für `eframe::Storage`).
pub fn templates_to_json(templates: &[WorkspaceTemplate]) -> String {
    serde_json::to_string(templates).unwrap_or_else(|_| "[]".to_string())
}

/// Lädt die Template-Liste aus JSON; ungültig/fehlend → leer.
pub fn templates_from_json(json: &str) -> Vec<WorkspaceTemplate> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_state_json_roundtrip() {
        let state = UiState {
            toolbox_visible: false,
            sidebar_tab: SidebarTab::ActiveCues,
            inspector_tab: InspectorTab::Triggers,
            workspace_settings_open: true,
            workspace_settings_page: SettingsPage::Video,
            ..Default::default()
        };
        let json = state.to_json();
        assert_eq!(UiState::from_json(&json), state);
    }

    #[test]
    fn workspace_templates_json_roundtrip_and_legacy() {
        let templates = vec![
            WorkspaceTemplate {
                name: "Bühne".to_string(),
                layout: UiState {
                    toolbox_visible: false,
                    ..Default::default()
                },
            },
            WorkspaceTemplate {
                name: "Einrichten".to_string(),
                layout: UiState::default(),
            },
        ];
        let json = templates_to_json(&templates);
        assert_eq!(templates_from_json(&json), templates);
        // Ungültig/fehlend → leer (abwärtskompatibel).
        assert!(templates_from_json("").is_empty());
        assert!(templates_from_json("kaputt").is_empty());
        assert!(templates_from_json("{}").is_empty());
    }

    /// Phase 5: Find-Zustand ist reiner Laufzeit-Zustand (`serde(skip)`) –
    /// landet nicht in der Persistenz, alte JSON-States bleiben gültig.
    #[test]
    fn ui_state_find_state_is_not_persisted() {
        let mut state = UiState::default();
        state.find.open = true;
        state.find.query = "haus".to_string();
        state.find.current = 3;
        let json = state.to_json();
        assert!(
            !json.contains("find"),
            "Find-Zustand darf nicht persistiert werden: {json}"
        );
        let loaded = UiState::from_json(&json);
        assert_eq!(
            loaded.find,
            FindState::default(),
            "Find startet frisch nach dem Laden"
        );
        assert!(!loaded.shortcuts_open);
        // Shortcuts-Fenster-Flag persistiert wie about_open.
        let state2 = UiState {
            shortcuts_open: true,
            test_grid: false,
            ..Default::default()
        };
        assert!(UiState::from_json(&state2.to_json()).shortcuts_open);
    }

    #[test]
    fn ui_state_from_invalid_json_falls_back_to_default() {
        assert_eq!(UiState::from_json("not json"), UiState::default());
        assert_eq!(UiState::from_json("{"), UiState::default());
        // Fehlende Felder → Defaults (abwärtskompatibel).
        let partial = UiState::from_json("{\"toolbox_visible\":false}");
        assert!(partial.masthead_visible);
        assert!(!partial.toolbox_visible);
        assert_eq!(partial.sidebar_tab, SidebarTab::ActiveCues);
    }

    /// Phase 4: neue Inspector-Tabs (Time & Loops, Geometry, …) serialisieren
    /// per Varianten-Name und roundtrippen (alte Zustände bleiben gültig).
    #[test]
    fn inspector_tab_phase4_variants_roundtrip() {
        for tab in [
            InspectorTab::TimeLoops,
            InspectorTab::Geometry,
            InspectorTab::AudioLevels,
            InspectorTab::Curve,
            InspectorTab::Source,
            InspectorTab::Slides,
            InspectorTab::Data,
        ] {
            let state = UiState {
                inspector_tab: tab,
                ..Default::default()
            };
            let json = state.to_json();
            assert_eq!(UiState::from_json(&json), state, "Roundtrip für {tab:?}");
        }
        // Alter gespeicherter Zustand (nur Basics/Triggers) deserialisiert weiter.
        let old = UiState::from_json("{\"inspector_tab\":\"Triggers\"}");
        assert_eq!(old.inspector_tab, InspectorTab::Triggers);
    }

    #[test]
    fn cue_column_layout_roundtrip_and_backwards_compat() {
        let mut layout = CueColumnLayout::default();
        layout.widths[2] = 90.0;
        layout.hidden[4] = true; // Target ausblenden
        let state = UiState {
            cue_columns: layout,
            ..Default::default()
        };
        let json = state.to_json();
        assert_eq!(UiState::from_json(&json), state);

        // Altes JSON ohne cue_columns → Default (abwärtskompatibel).
        let old = UiState::from_json("{\"toolbox_visible\":false}");
        assert_eq!(old.cue_columns, CueColumnLayout::default());
        assert!(!old.cue_columns.hidden[4]);

        // Reset stellt Default-Breiten + Sichtbarkeit wieder her.
        let mut layout = CueColumnLayout::default();
        layout.widths[2] = 12.0;
        layout.hidden[0] = true;
        layout.prev_hidden[0] = true;
        layout.reset();
        assert_eq!(layout, CueColumnLayout::default());
    }
}
