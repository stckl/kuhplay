//! Show-Dokument: serialisierbarer Gesamtzustand der Anwendung
//! (QLab-Pendant: ein „Workspace“-Dokument mit Name + allen Cue-Listen +
//! Stages + Output Routes).
//!
//! Datei-IO liegt bewusst in der GUI-Schicht (native Dialoge via `rfd`);
//! hier nur (De-)Serialisierung, damit Persistenz testbar bleibt.
//!
//! ## Migration alter Shows (Screens → Output Routes)
//!
//! Alte Shows (Screens + `Stage::screen`) laden weiter und sehen gleich
//! aus: [`migrate_legacy_screens`] wandelt die Altfelder beim Laden in
//! Routen + Regionen um. Die Altfelder bleiben lesbar
//! (`#[serde(default, skip_serializing)]`), werden beim Speichern aber
//! nicht mehr geschrieben.

use serde::{Deserialize, Serialize};

use crate::core::audio::AudioPatch;
use crate::core::cart::CartManager;
use crate::core::error::{ShowError, ShowResult};
use crate::core::screen::ScreenManager;
use crate::core::stage::StageManager;
use crate::core::video_routing::{
    regions_for_routes_proportional, DeviceArea, DeviceRef, DisplayRefInfo, OutputRoute,
    RouteManager,
};

/// Ein komplettes Show-Dokument (Workspace): Name + alle Carts +
/// Stages + Output Routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowDocument {
    /// Anzeigename der Show (QLab: Workspace-Name in der Titelleiste).
    pub name: String,
    /// Alle Cue-Listen/Carts der Show.
    pub carts: CartManager,
    /// Video-Stages (mit Regionen und Layern).
    #[serde(default)]
    pub stages: StageManager,
    /// Output Routes (logische Ausgänge auf Devices).
    #[serde(default)]
    pub routes: RouteManager,
    /// Altes Modell (QLab 4-artige „Screens“): nur noch zum Laden alter
    /// Shows; wird beim Speichern nicht mehr geschrieben (Migration siehe
    /// [`migrate_legacy_screens`]).
    #[serde(default, skip_serializing)]
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
            routes: RouteManager::new(),
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
        let mut doc: Self = serde_json::from_str(json)
            .map_err(|err| ShowError::InvalidParameter(format!("deserialize show: {err}")))?;
        // Legacy-Audio-Patch (ein Ausgang je Kanal) in die Matrix migrieren.
        doc.audio_patch.migrate_legacy();
        Ok(doc)
    }

    /// `true`, wenn das Dokument noch alte Screens bzw. `Stage::screen`-
    /// Zuordnungen enthält, die noch nicht migriert wurden.
    pub fn needs_screen_migration(&self) -> bool {
        self.routes.is_empty()
            && (!self.screens.is_empty()
                || self
                    .stages
                    .stages()
                    .iter()
                    .any(|stage| stage.screen.is_some()))
    }

    /// Migriert das alte Screen-Modell in Routen + Regionen (Abschnitt 6
    /// des Arbeitsauftrags). Aufrufer: `load_show` (mit den erkannten
    /// Displays); Test mit festen Display-Geometrien.
    ///
    /// * Jeder Screen wird zu Routen: eine pro überlappendem Display
    ///   (`Full`, wenn er es deckt, sonst `Part` in Gerätepixeln); ohne
    ///   passendes Display eine Route ohne Gerät („(missing)“).
    /// * Jede Stage mit `screen` bekommt Regionen, die die Stage
    ///   proportional auf diese Routen aufteilen.
    /// * Skalierung `Stretch`: Die Show sieht aus wie vorher, auch wenn
    ///   sie verzerrt war – nichts wird „verbessert“.
    /// * Cues bleiben unverändert.
    pub fn migrate_legacy_screens(&mut self, displays: &[DisplayRefInfo]) {
        if !self.needs_screen_migration() {
            return;
        }
        // Display-Reihenfolge nach Position (links → rechts, oben → unten)
        // für die proportionale Aufteilung der Stage.
        let mut displays: Vec<&DisplayRefInfo> = displays.iter().collect();
        displays.sort_by_key(|d| (d.x, d.y));

        // Screen-ID → Routen-IDs (in Device-Reihenfolge links nach rechts).
        let mut routes_per_screen: Vec<(uuid::Uuid, Vec<uuid::Uuid>)> = Vec::new();
        for screen in self.screens.screens() {
            let mut route_ids = Vec::new();
            let overlapping: Vec<&DisplayRefInfo> = displays
                .iter()
                .copied()
                .filter(|d| d.intersects(screen.x, screen.y, screen.width, screen.height))
                .collect();
            if overlapping.is_empty() {
                // Kein Display passt (anderer Rechner): Route ohne Gerät.
                let mut route = OutputRoute::new(
                    screen.name.clone(),
                    screen.width.max(1),
                    screen.height.max(1),
                );
                route.rotation = screen.rotation;
                route.color = screen.color;
                route.test_pattern = screen.test_pattern;
                route.test_pattern_kind = screen.test_pattern_kind;
                route.checker_size = screen.checker_size;
                let id = route.id;
                if self.routes.add(route).is_ok() {
                    route_ids.push(id);
                }
            } else {
                let multi = overlapping.len() > 1;
                for display in overlapping {
                    // Routengröße = Screen in Pixeln auf diesem Display.
                    let (w, h) = (
                        (screen.width as f32 * display.scale_factor)
                            .round()
                            .max(1.0) as u32,
                        (screen.height as f32 * display.scale_factor)
                            .round()
                            .max(1.0) as u32,
                    );
                    let mut route = OutputRoute::new(
                        if multi {
                            format!("{} ({})", screen.name, display.name)
                        } else {
                            screen.name.clone()
                        },
                        w,
                        h,
                    );
                    route.device = Some(DeviceRef::Display {
                        id: display.id,
                        name: display.name.clone(),
                        width: display.width,
                        height: display.height,
                    });
                    // Deckt der Screen das Display → Full, sonst Part
                    // (Sichtbarer Ausschnitt, in Gerätepixeln).
                    if screen.covers(display.x, display.y, display.width, display.height) {
                        route.device_area = DeviceArea::Full;
                    } else {
                        let scale = display.scale_factor;
                        let x = (screen.x - display.x).max(0) as f32 * scale;
                        let y = (screen.y - display.y).max(0) as f32 * scale;
                        let right = ((screen.x + screen.width as i32) - display.x)
                            .min(display.width as i32) as f32
                            * scale;
                        let bottom = ((screen.y + screen.height as i32) - display.y)
                            .min(display.height as i32) as f32
                            * scale;
                        route.device_area = DeviceArea::Part {
                            x: x.round() as u32,
                            y: y.round() as u32,
                            width: (right - x).round().max(1.0) as u32,
                            height: (bottom - y).round().max(1.0) as u32,
                        };
                    }
                    route.rotation = screen.rotation;
                    route.color = screen.color;
                    route.test_pattern = screen.test_pattern;
                    route.test_pattern_kind = screen.test_pattern_kind;
                    route.checker_size = screen.checker_size;
                    let id = route.id;
                    if self.routes.add(route).is_ok() {
                        route_ids.push(id);
                    }
                }
            }
            routes_per_screen.push((screen.id, route_ids));
        }

        // Stages mit Screen-Zuordnung → proportionale Regionen.
        for stage in self.stages.stages_mut() {
            let Some(screen_id) = stage.screen else {
                continue;
            };
            let Some((_, route_ids)) = routes_per_screen.iter().find(|(id, _)| *id == screen_id)
            else {
                continue;
            };
            let stage_w = stage.geometry.width.max(1.0);
            let stage_h = stage.geometry.height.max(1.0);
            stage.regions =
                regions_for_routes_proportional(stage_w, stage_h, route_ids, &self.routes);
            stage.screen = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        Cue, CueType, LoopMode, Region, Screen, ScreenRotation, StageGeometry, TestPatternKind,
    };

    fn display(id: u32, name: &str, x: i32, w: u32, h: u32, scale: f32) -> DisplayRefInfo {
        DisplayRefInfo {
            id,
            name: name.to_string(),
            x,
            y: 0,
            width: w,
            height: h,
            scale_factor: scale,
            frequency: 60.0,
        }
    }

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
    fn show_document_roundtrip_preserves_stages_and_routes() {
        let mut doc = ShowDocument::new("Route-Show");
        // Route anlegen (mit Drehung + Testbild).
        let mut route = OutputRoute::new("Beamer", 1920, 1080);
        route.rotation = ScreenRotation::Deg90;
        route.test_pattern = true;
        route.rear_projection = true;
        let route_id = route.id;
        doc.routes.add(route).unwrap();
        // Stage 1 mit Region auf die Route.
        if let Some(stage) = doc.stages.find_mut("Stage 1") {
            stage.geometry = StageGeometry::new(1080.0, 1920.0);
            let mut region = Region::full_stage(1080.0, 1920.0);
            region.route = Some(route_id);
            stage.regions.push(region);
            stage.layer = 3;
            stage.keep_rendering = true;
        }

        let json = doc.to_json().expect("serialize");
        // Alte Felder werden nicht mehr geschrieben.
        assert!(!json.contains("\"screens\""));
        assert!(!json.contains("\"screen\""));
        let back = ShowDocument::from_json(&json).expect("deserialize");

        // Route überlebt.
        assert_eq!(back.routes.len(), 1);
        let r = back.routes.find(route_id).expect("route found");
        assert_eq!(r.name, "Beamer");
        assert_eq!(r.rotation, ScreenRotation::Deg90);
        assert!(r.test_pattern);
        assert!(r.rear_projection);
        assert_eq!(back.screens.len(), 0, "Screens werden nicht gespeichert");

        // Stage mit Regionen überlebt.
        let stage = back.stages.find("Stage 1").expect("stage found");
        assert_eq!(stage.geometry.width, 1080.0);
        assert_eq!(stage.regions.len(), 1);
        assert_eq!(stage.regions[0].route, Some(route_id));
        assert_eq!(stage.layer, 3);
        assert!(stage.keep_rendering);
        assert_eq!(stage.screen, None);
    }

    #[test]
    fn show_document_legacy_without_stages_screens_uses_defaults() {
        // Alte Show-Datei ohne stages/screens-Felder → Defaults.
        let legacy = r#"{"name":"Old Show","carts":{"carts":[],"active":0}}"#;
        let doc = ShowDocument::from_json(legacy).expect("legacy deserialize");
        assert_eq!(doc.name, "Old Show");
        assert_eq!(doc.stages.len(), 0, "keine Stages (Default: leer)");
        assert_eq!(doc.routes.len(), 0, "keine Routen (Default: leer)");
    }

    #[test]
    fn show_document_new_has_default_stage() {
        let doc = ShowDocument::new("Test");
        assert_eq!(doc.stages.len(), 1, "Default: Stage 1");
        assert_eq!(doc.stages.find("Stage 1").unwrap().geometry.width, 1920.0);
        assert_eq!(doc.routes.len(), 0, "keine Routen per Default");
        assert_eq!(doc.screens.len(), 0, "keine Screens per Default");
    }

    // ------------------------------------------------------------------
    // Migration alter Shows (Screens → Routen + Regionen)
    // ------------------------------------------------------------------

    #[test]
    fn migration_screen_on_single_display_becomes_full_route() {
        let mut doc = ShowDocument::new("Alt");
        let screen = Screen::new("Beamer", 0, 0, 1920, 1080);
        let screen_id = screen.id;
        let mut screen = screen;
        screen.test_pattern = true;
        screen.test_pattern_kind = TestPatternKind::ColorBars;
        screen.rotation = ScreenRotation::Deg90;
        doc.screens.add(screen).unwrap();
        doc.stages.find_mut("Stage 1").unwrap().screen = Some(screen_id);
        doc.stages.find_mut("Stage 1").unwrap().geometry = StageGeometry::new(1920.0, 1080.0);

        doc.migrate_legacy_screens(&[display(1, "Main", 0, 1920, 1080, 1.0)]);
        assert_eq!(doc.routes.len(), 1);
        let route = doc.routes.routes()[0].clone();
        assert_eq!(route.name, "Beamer");
        assert_eq!((route.width, route.height), (1920, 1080));
        assert_eq!(route.device_area, DeviceArea::Full);
        assert!(route.test_pattern);
        assert_eq!(route.test_pattern_kind, TestPatternKind::ColorBars);
        assert_eq!(route.rotation, ScreenRotation::Deg90);
        assert!(matches!(
            route.device,
            Some(DeviceRef::Display { id: 1, .. })
        ));
        // Stage hat jetzt eine Region über die ganze Fläche auf die Route.
        let stage = doc.stages.find("Stage 1").unwrap();
        assert_eq!(stage.screen, None, "Screen-Referenz wird aufgelöst");
        assert_eq!(stage.regions.len(), 1);
        assert_eq!(stage.regions[0].route, Some(route.id));
        assert!((stage.regions[0].width - 1920.0).abs() < 0.5);
        // Nochmalige Migration ist eine No-Op (Routen vorhanden).
        doc.migrate_legacy_screens(&[]);
        assert_eq!(doc.routes.len(), 1);
    }

    #[test]
    fn migration_screen_over_two_displays_becomes_two_routes() {
        let mut doc = ShowDocument::new("Alt");
        // Screen über beide Displays (1920 breit je Display, Screen 2400 breit
        // mit 120 px Überlapp je Seite).
        let screen = Screen::new("Wide", 960, 0, 2400, 1080);
        let screen_id = screen.id;
        doc.screens.add(screen).unwrap();
        doc.stages.find_mut("Stage 1").unwrap().screen = Some(screen_id);
        doc.stages.find_mut("Stage 1").unwrap().geometry = StageGeometry::new(2400.0, 1080.0);

        let displays = vec![
            display(1, "Left", 0, 1920, 1080, 1.0),
            display(2, "Right", 1920, 1920, 1080, 1.0),
        ];
        doc.migrate_legacy_screens(&displays);
        assert_eq!(doc.routes.len(), 2, "eine Route pro Display");
        // Beide Routen Partial (kein Display wird vollständig gedeckt).
        assert!(matches!(
            doc.routes.routes()[0].device_area,
            DeviceArea::Part { .. }
        ));
        assert!(matches!(
            doc.routes.routes()[1].device_area,
            DeviceArea::Part { .. }
        ));
        // Stage: zwei Regionen, die die Stage vollständig abdecken.
        let stage = doc.stages.find("Stage 1").unwrap();
        assert_eq!(stage.regions.len(), 2);
        let covered: f32 = stage.regions.iter().map(|r| r.width).sum();
        assert!((covered - 2400.0).abs() < 1.0, "Stage komplett geteilt");
        assert_ne!(stage.regions[0].route, stage.regions[1].route);
    }

    #[test]
    fn migration_without_matching_display_keeps_route_missing() {
        let mut doc = ShowDocument::new("Alt");
        let screen = Screen::new("Gast", 5000, 0, 1920, 1080);
        let screen_id = screen.id;
        doc.screens.add(screen).unwrap();
        doc.stages.find_mut("Stage 1").unwrap().screen = Some(screen_id);

        // Displays, die den Screen nicht schneiden.
        doc.migrate_legacy_screens(&[display(1, "Main", 0, 1920, 1080, 1.0)]);
        assert_eq!(doc.routes.len(), 1);
        let route = doc.routes.routes()[0].clone();
        assert_eq!(route.device, None, "kein Gerät → (missing)");
        assert_eq!((route.width, route.height), (1920, 1080));
        // Stage bleibt an die Route gebunden.
        assert_eq!(doc.stages.find("Stage 1").unwrap().regions.len(), 1);
    }

    #[test]
    fn migration_retina_display_scales_route_to_pixels() {
        let mut doc = ShowDocument::new("Alt");
        let screen = Screen::new("Retina", 0, 0, 1512, 982);
        let screen_id = screen.id;
        doc.screens.add(screen).unwrap();
        doc.stages.find_mut("Stage 1").unwrap().screen = Some(screen_id);

        doc.migrate_legacy_screens(&[display(1, "MBP", 0, 1512, 982, 2.0)]);
        let route = doc.routes.routes()[0].clone();
        assert_eq!((route.width, route.height), (3024, 1964), "echte Pixel");
        assert_eq!(route.device_area, DeviceArea::Full);
    }

    #[test]
    fn migration_stage_without_screen_gets_no_region() {
        let mut doc = ShowDocument::new("Alt");
        doc.screens
            .add(Screen::new("Beamer", 0, 0, 1920, 1080))
            .unwrap();
        // Stage 1 hat keinen Screen.
        doc.migrate_legacy_screens(&[display(1, "Main", 0, 1920, 1080, 1.0)]);
        assert_eq!(doc.stages.find("Stage 1").unwrap().regions.len(), 0);
    }

    #[test]
    fn migrated_show_saves_and_reloads_without_legacy_fields() {
        // Kompletter Zyklus: alte Show laden → migrieren → speichern →
        // wieder laden, ohne dass Daten fehlen.
        let mut doc = ShowDocument::new("Old Show");
        let screen = Screen::new("Beamer", 1920, 0, 1920, 1080);
        let screen_id = screen.id;
        let mut screen = screen;
        screen.color = [64, 128, 255];
        doc.screens.add(screen).unwrap();
        doc.stages.find_mut("Stage 1").unwrap().screen = Some(screen_id);

        let displays = vec![display(1, "Main", 0, 1920, 1080, 1.0)];
        doc.migrate_legacy_screens(&displays);

        let json = doc.to_json().unwrap();
        assert!(!json.contains("\"screens\""), "Altfelder nicht schreiben");
        let reloaded = ShowDocument::from_json(&json).unwrap();
        assert_eq!(reloaded.routes.len(), 1);
        let route = reloaded.routes.routes()[0].clone();
        assert_eq!(route.name, "Beamer");
        assert_eq!(route.color, [64, 128, 255]);
        assert!(!reloaded.needs_screen_migration());
        let stage = reloaded.stages.find("Stage 1").unwrap();
        assert_eq!(stage.regions.len(), 1);
        assert_eq!(stage.regions[0].route, Some(route.id));
    }
}
