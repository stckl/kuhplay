//! GUI-Layer (egui/eframe) – QLab 5 Workspace-Layout.
//!
//! **Verbindliche UI-Richtlinie: `docs/UI.md`** (Layout, Farben,
//! Typografie, Komponenten).
//!
//! Panel-Module (jedes zeichnet einen Bereich des Workspaces):
//! * [`menu_bar`] – Menüleiste (File / Edit / View / Cues / Workspace / Help)
//! * `native_menu` – macOS-Menüleiste (ersetzt dort [`menu_bar`])
//! * [`masthead`] – GO-Button, Standby-Display, Notes, Transport + Panic
//! * [`toolbox`] – linke Leiste, einzige Cue-Typ-Button-Quelle (gruppiert)
//! * [`sidebar`] – rechte Leiste (Tabs: Lists & Carts, Active Cues)
//! * [`inspector`] – unteres Panel, volle Breite (typabhängige Tabs)
//! * [`footer`] – unterster Panel-Bereich (Edit/Show, ⚠ kaputte Cues,
//!   „N cues in M lists", Icon-Toggles, Zahnrad Workspace Settings)
//! * [`cue_table`] – zentrale Cue-Liste (QLab-Spaltenlayout)
//! * [`screen_editor`] – Screen-Editor (in den Workspace Settings → Video)
//! * [`workspace_settings`] – Fenster „Workspace Settings" (Nav + Seiten)
//! * [`stage_view`] – Stage-Fenster & Hardware-Screen-Ausgabe (Vollbild)
//! * [`app`] – `ShowApp` (eframe::App) mit Panel-Orchestrierung
//! * [`state`] – `UiState`: persistiertes UI-Layout (serde)
//! * [`problems`] – Broken-Cue-Erkennung (QLab „broken cue")
//! * [`demo`] – Demo-Show für den ersten Start
//! * [`theme`] – Dark-Theme, Flächen-/Status-Farben, Layout-Konstanten,
//!   `tab_bar`/`section_label`/`panel_frame`

pub mod app;
pub mod cue_table;
pub mod demo;
pub mod footer;
pub mod icons;
pub mod inspector;
#[cfg(target_os = "macos")]
mod macos_output;
pub mod masthead;
pub mod menu_bar;
#[cfg(target_os = "macos")]
pub mod native_menu;
pub mod problems;
pub mod screen_editor;
pub mod shortcuts;
pub mod sidebar;
pub mod stage_view;
pub mod state;
pub mod theme;
pub mod toolbox;
pub mod workspace_settings;

pub use app::{InspectorTab, OutputMode, ShowApp, SidebarTab};
pub use demo::demo_cues;
pub use state::FindState;
pub use workspace_settings::SettingsPage;

/// Programm-Icon: `icon.png` aus dem Projekt-Root, via `include_bytes!`
/// eingebettet (unabhängig vom CWD; fehlt die Datei, schlägt der Build fehl).
/// PNG → RGBA für `egui::ViewportBuilder::with_icon`.
pub fn app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../icon.png"))
        .ok()?
        .to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_icon_decodes_embedded_png() {
        let icon = super::app_icon().expect("icon.png must decode");
        assert!(icon.width > 0 && icon.height > 0);
        assert_eq!(
            icon.rgba.len(),
            (icon.width * icon.height * 4) as usize,
            "RGBA-Pufferlänge muss zu width×height passen"
        );
    }
}
