//! Show-Dokument: serialisierbarer Gesamtzustand der Anwendung
//! (QLab-Pendant: ein „Workspace"-Dokument mit Name + allen Cue-Listen +
//! Stages + Screens).
//!
//! Datei-IO liegt bewusst in der GUI-Schicht (native Dialoge via `rfd`);
//! hier nur (De-)Serialisierung, damit Persistenz testbar bleibt.

use serde::{Deserialize, Serialize};

use crate::core::audio::AudioPatch;
use crate::core::cart::CartManager;
use crate::core::error::{ShowError, ShowResult};
use crate::core::screen::ScreenManager;
use crate::core::stage::StageManager;

/// Ein komplettes Show-Dokument (Workspace): Name + alle Carts +
/// Stages + Screens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowDocument {
    /// Anzeigename der Show (QLab: Workspace-Name in der Titelleiste).
    pub name: String,
    /// Alle Cue-Listen/Carts der Show.
    pub carts: CartManager,
    /// Video-Stages (mit Screen-Zuordnung).
    #[serde(default)]
    pub stages: StageManager,
    /// Physische Ausgabebereiche (QLab „Screens").
    #[serde(default)]
    pub screens: ScreenManager,
    /// Audio-Ausgabegerät (Name); `None` = System-Standard.
    #[serde(default)]
    pub audio_output: Option<String>,
    /// Zuordnung der Audio-Kanäle zu Geräte-Ausgängen.
    #[serde(default)]
    pub audio_patch: AudioPatch,
}

impl ShowDocument {
    /// Neues, leeres Show-Dokument (eine leere Cue-Liste, Default-Stage).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            carts: CartManager::new(),
            stages: StageManager::new(),
            screens: ScreenManager::new(),
            audio_output: None,
            audio_patch: AudioPatch::default(),
        }
    }

    /// Serialisiert das Dokument als hübsches JSON.
    pub fn to_json(&self) -> ShowResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|err| ShowError::InvalidParameter(format!("serialize show: {err}")))
    }

    /// Deserialisiert ein Show-Dokument aus JSON.
    pub fn from_json(json: &str) -> ShowResult<Self> {
        serde_json::from_str(json)
            .map_err(|err| ShowError::InvalidParameter(format!("deserialize show: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Cue, CueType, LoopMode, Screen, ScreenRotation, StageGeometry};

    #[test]
    fn audio_output_is_optional_in_old_shows() {
        let json = ShowDocument::new("Alt").to_json().unwrap();
        let json = json.replace("\"audio_output\": null", "\"unused\": null");
        assert_eq!(ShowDocument::from_json(&json).unwrap().audio_output, None);
    }

    #[test]
    fn show_document_roundtrip_preserves_carts_and_cues() {
        let mut doc = ShowDocument::new("Meine Show");
        doc.carts.active_mut().add(
            Cue::new("1", "Intro", CueType::Audio)
                .with_target("/media/intro.wav")
                .with_loop(LoopMode::Count(2)),
        );
        doc.carts.add_cart("Cart 2");
        doc.carts.set_active(0).unwrap(); // Cue liegt in Cart 1

        let json = doc.to_json().expect("serialize");
        let back = ShowDocument::from_json(&json).expect("deserialize");
        assert_eq!(back.name, "Meine Show");
        assert_eq!(back.carts.cart_count(), 2);
        assert_eq!(back.carts.active().len(), 1);
        assert_eq!(back.carts.active().cues()[0].name, "Intro");
        assert_eq!(back.carts.active().cues()[0].loop_mode, LoopMode::Count(2));
    }

    #[test]
    fn show_document_rejects_invalid_json_without_panic() {
        let err = ShowDocument::from_json("{ not json").unwrap_err();
        assert!(matches!(err, ShowError::InvalidParameter(_)));
    }

    #[test]
    fn show_document_roundtrip_preserves_stages_and_screens() {
        let mut doc = ShowDocument::new("Screen-Show");
        // Screen anlegen (mit Drehung + Testbild).
        let mut screen = Screen::new("Beamer", 1920, 0, 1920, 1080);
        screen.rotation = ScreenRotation::Deg90;
        screen.test_pattern = true;
        let screen_id = screen.id;
        doc.screens.add(screen).unwrap();
        // Stage 1 dem Screen zuordnen.
        if let Some(stage) = doc.stages.find_mut("Stage 1") {
            stage.screen = Some(screen_id);
            stage.geometry = StageGeometry::new(1080.0, 1920.0);
        }

        let json = doc.to_json().expect("serialize");
        let back = ShowDocument::from_json(&json).expect("deserialize");

        // Screens überlebt.
        assert_eq!(back.screens.len(), 1);
        let s = back.screens.find(screen_id).expect("screen found");
        assert_eq!(s.name, "Beamer");
        assert_eq!(s.rect(), (1920, 0, 1920, 1080));
        assert_eq!(s.rotation, ScreenRotation::Deg90);
        assert!(s.test_pattern);

        // Stage-Zuordnung überlebt.
        let stage = back.stages.find("Stage 1").expect("stage found");
        assert_eq!(stage.screen, Some(screen_id));
        assert_eq!(stage.geometry.width, 1080.0);
        assert_eq!(stage.geometry.height, 1920.0);
    }

    #[test]
    fn show_document_legacy_without_stages_screens_uses_defaults() {
        // Alte Show-Datei ohne stages/screens-Felder → Defaults.
        let legacy = r#"{"name":"Old Show","carts":{"carts":[],"active":0}}"#;
        let doc = ShowDocument::from_json(legacy).expect("legacy deserialize");
        assert_eq!(doc.name, "Old Show");
        assert_eq!(doc.stages.len(), 0, "keine Stages (Default: leer)");
        assert_eq!(doc.screens.len(), 0, "keine Screens (Default: leer)");
    }

    #[test]
    fn show_document_new_has_default_stage() {
        let doc = ShowDocument::new("Test");
        assert_eq!(doc.stages.len(), 1, "Default: Stage 1");
        assert_eq!(doc.stages.find("Stage 1").unwrap().geometry.width, 1920.0);
        assert_eq!(doc.screens.len(), 0, "keine Screens per Default");
    }
}
