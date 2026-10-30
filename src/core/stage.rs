//! Video-Stages: Ausgabeflächen mit Layer-Verwaltung und Geometrie
//! (Position, Skalierung, Cropping).
//!
//! Die Stage-Metadaten sind das UI-/Domain-Modell; die produktive Video-Engine
//! (GStreamer `videomixer`/Compositor oder native Window-Layer) bildet
//! [`StageGeometry`] auf konkrete Render-Pipelines ab.
//!
//! ## QLab-Feature-Mapping (Kapitel „Video")
//!
//! * **Stages & Screens:** Eine [`Stage`] ist die virtuelle Ausgabefläche
//!   (Canvas-Größe); [`Stage::screen`] mappt sie per ID auf einen Screen
//!   ([`crate::core::Screen`], Rechteck auf einem Hardware-Output) –
//!   „Stage → Screen"-Zuordnung wie in QLab.
//! * **Surfaces:** Jeder Video-/Diashow-Cue trägt eine [`SurfaceGeometry`]
//!   (Position/Größe auf der Stage, Layer, Deckkraft, Sichtbarkeit).
//! * **Area of Interest:** [`SurfaceGeometry::area_of_interest`] ist der
//!   Zuschnitt der Quelle (QLab „Area of Interest“ / Crop) – als normalisiertes
//!   Keep-Rechteck ([`Crop`]), live editierbar.
//! * **Hard Edge Blending:** [`SurfaceGeometry::hard_edge`] weicht die Kante
//!   einer Surface über eine Breite in Pixeln auf (linear oder Cosinus-Kurve)
//!   und blendet so die darunterliegende Surface in der Überlappung ein –
//!   wie QLabs „Hard Edge“-Blending zwischen benachbarten Projektionen.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::error::{ShowError, ShowResult};

fn default_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_opacity() -> f32 {
    1.0
}

/// Seite einer Surface, an der ein Hard-Edge-Fade liegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeSide {
    /// Linke Kante der Surface.
    Left,
    /// Rechte Kante.
    Right,
    /// Obere Kante.
    Top,
    /// Untere Kante.
    Bottom,
}

/// Kurvenform des Hard-Edge-Fades (QLab: Linear / Cosine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendCurve {
    /// Lineare Rampe (hart → weich linear).
    Linear,
    /// Cosinus-Rampe (weicher, S-förmiger Übergang).
    #[default]
    Cosine,
}

impl EdgeSide {
    /// Alle Seiten (für UI-ComboBoxen).
    pub const ALL: [EdgeSide; 4] = [
        EdgeSide::Left,
        EdgeSide::Right,
        EdgeSide::Top,
        EdgeSide::Bottom,
    ];

    /// UI-Label (QLab-Terminologie).
    pub fn label(&self) -> &'static str {
        match self {
            EdgeSide::Left => "Left",
            EdgeSide::Right => "Right",
            EdgeSide::Top => "Top",
            EdgeSide::Bottom => "Bottom",
        }
    }
}

impl BlendCurve {
    /// Alle Kurven (für UI-ComboBoxen).
    pub const ALL: [BlendCurve; 2] = [BlendCurve::Linear, BlendCurve::Cosine];

    /// UI-Label (QLab-Terminologie).
    pub fn label(&self) -> &'static str {
        match self {
            BlendCurve::Linear => "Linear",
            BlendCurve::Cosine => "Cosine",
        }
    }
}

/// Hard-Edge-Blending einer Surface (QLab „Hard Edge“).
///
/// In der Überlappungszone zweier Surfaces wird die Kante dieser Surface
/// über `width` Pixel (auf der Stage-Canvas) von 0 auf 1 eingeblendet –
/// die darunterliegende Surface scheint durch (Edge Blending zwischen
/// benachbarten Projektionen).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HardEdge {
    /// Seite, an der geblendet wird.
    pub side: EdgeSide,
    /// Breite der Blende in Pixeln (auf der Stage-Canvas, > 0).
    pub width: f32,
    /// Kurvenform.
    #[serde(default)]
    pub curve: BlendCurve,
}

impl HardEdge {
    /// Neuer Hard-Edge-Fade an `side` über `width` px mit `curve`.
    pub fn new(side: EdgeSide, width: f32, curve: BlendCurve) -> Self {
        Self { side, width, curve }
    }

    /// Validiert die Blende.
    pub fn validate(&self) -> ShowResult<()> {
        if self.width <= 0.0 {
            return Err(ShowError::InvalidParameter(format!(
                "hard edge width must be > 0, got {}",
                self.width
            )));
        }
        if !self.width.is_finite() {
            return Err(ShowError::InvalidParameter(
                "hard edge width must be finite".to_string(),
            ));
        }
        Ok(())
    }

    /// Blenden-Faktor (0.0 – 1.0) für einen Punkt `dist` px **innen** von der
    /// Kante entfernt: 0 direkt an der Kante → 1 ab `width` px innen.
    pub fn factor(&self, dist: f32) -> f32 {
        if dist >= self.width {
            return 1.0;
        }
        if dist <= 0.0 {
            return 0.0;
        }
        let t = dist / self.width; // 0 an der Kante → 1 innen
        match self.curve {
            BlendCurve::Linear => t,
            // Cosinus: weicher S-förmiger Verlauf (QLab-Cosine).
            BlendCurve::Cosine => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
        }
    }
}

/// Geometrie einer Surface (eines Video-/Diashow-Cues) auf einer Stage.
///
/// QLab-Pendant: Surface-Zuweisung eines Video-Cues (Position/Größe auf der
/// Stage, Layer, Deckkraft) inkl. **Area of Interest** (Quellen-Zuschnitt)
/// und **Hard-Edge**-Blending.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SurfaceGeometry {
    /// X-Position auf der Stage-Canvas (Pixel).
    pub x: f32,
    /// Y-Position auf der Stage-Canvas (Pixel).
    pub y: f32,
    /// Breite auf der Stage-Canvas (Pixel, > 0).
    pub width: f32,
    /// Höhe auf der Stage-Canvas (Pixel, > 0).
    pub height: f32,
    /// Area of Interest: Quellen-Zuschnitt als normalisiertes Keep-Rechteck
    /// (QLab „Area of Interest“). `None` = volle Quelle.
    #[serde(default)]
    pub area_of_interest: Option<Crop>,
    /// Hard-Edge-Blending an einer Kante (QLab „Hard Edge“).
    #[serde(default)]
    pub hard_edge: Option<HardEdge>,
    /// Deckkraft der Surface (0.0 – 1.0; 1.0 = voll deckend).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Sichtbarkeit (unsichtbare Surfaces werden nicht komponiert).
    #[serde(default = "default_true")]
    pub visible: bool,
}

impl SurfaceGeometry {
    /// Neue Surface, die die gesamte Stage-Canvas ausfüllt.
    pub fn full_canvas() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            area_of_interest: None,
            hard_edge: None,
            opacity: 1.0,
            visible: true,
        }
    }

    /// Validiert die Surface-Geometrie.
    pub fn validate(&self) -> ShowResult<()> {
        if self.width <= 0.0 || !self.width.is_finite() {
            return Err(ShowError::InvalidParameter(format!(
                "surface width must be > 0 and finite, got {}",
                self.width
            )));
        }
        if self.height <= 0.0 || !self.height.is_finite() {
            return Err(ShowError::InvalidParameter(format!(
                "surface height must be > 0 and finite, got {}",
                self.height
            )));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(ShowError::InvalidParameter(format!(
                "surface opacity must be within 0.0–1.0, got {}",
                self.opacity
            )));
        }
        if let Some(crop) = &self.area_of_interest {
            crop.validate()?;
        }
        if let Some(edge) = &self.hard_edge {
            edge.validate()?;
        }
        Ok(())
    }

    /// Pixel-Rechteck der Quelle, das dem Area of Interest entspricht
    /// (`(x0, y0, x1, y1)`, halb-offen, auf `src_w`×`src_h` geclampt).
    pub fn source_rect(&self, src_w: u32, src_h: u32) -> (u32, u32, u32, u32) {
        match &self.area_of_interest {
            None => (0, 0, src_w, src_h),
            Some(crop) => {
                let x0 = (crop.left.clamp(0.0, 1.0) * src_w as f32) as u32;
                let y0 = (crop.top.clamp(0.0, 1.0) * src_h as f32) as u32;
                let x1 = (crop.right.clamp(0.0, 1.0) * src_w as f32).ceil() as u32;
                let y1 = (crop.bottom.clamp(0.0, 1.0) * src_h as f32).ceil() as u32;
                let x1 = x1.clamp(x0 + 1, src_w.max(x0 + 1));
                let y1 = y1.clamp(y0 + 1, src_h.max(y0 + 1));
                (x0, y0, x1, y1)
            }
        }
    }

    /// Blenden-Faktor des Hard-Edge an einem Canvas-Punkt `(px, py)`
    /// (nur innerhalb des Dest-Rechtecks sinnvoll; 1.0 außerhalb der Blende).
    pub fn hard_edge_factor(&self, px: f32, py: f32) -> f32 {
        let edge = match &self.hard_edge {
            Some(edge) => edge,
            None => return 1.0,
        };
        let dist = match edge.side {
            EdgeSide::Left => px - self.x,
            EdgeSide::Right => self.x + self.width - px,
            EdgeSide::Top => py - self.y,
            EdgeSide::Bottom => self.y + self.height - py,
        };
        edge.factor(dist)
    }
}

/// Cropping-Anteil (0.0 – 1.0 relativ zur Mediengröße).
///
/// `left`/`top` = beschnittener Anteil oben/links, `right`/`bottom` = Anteil
/// rechts/unten, der **beibehalten** wird. Beispiel: `left = 0.1` entfernt 10 %
/// links.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    /// Anteil links (0.0 – 1.0).
    pub left: f32,
    /// Anteil oben (0.0 – 1.0).
    pub top: f32,
    /// Anteil rechts, der erhalten bleibt (0.0 – 1.0).
    pub right: f32,
    /// Anteil unten, der erhalten bleibt (0.0 – 1.0).
    pub bottom: f32,
}

impl Crop {
    /// Volles Bild (kein Crop).
    pub fn none() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        }
    }

    /// Validiert die Crop-Anteile.
    pub fn validate(&self) -> ShowResult<()> {
        for (name, v) in [
            ("left", self.left),
            ("top", self.top),
            ("right", self.right),
            ("bottom", self.bottom),
        ] {
            if !(0.0..=1.0).contains(&v) {
                return Err(ShowError::InvalidParameter(format!(
                    "crop.{name} must be within 0.0–1.0, got {v}"
                )));
            }
        }
        if self.left >= self.right {
            return Err(ShowError::InvalidParameter(format!(
                "crop.left ({}) must be < crop.right ({})",
                self.left, self.right
            )));
        }
        if self.top >= self.bottom {
            return Err(ShowError::InvalidParameter(format!(
                "crop.top ({}) must be < crop.bottom ({})",
                self.top, self.bottom
            )));
        }
        Ok(())
    }
}

/// Geometrie einer Stage: Position, Größe, Skalierung und optionaler Crop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StageGeometry {
    /// X-Position (Pixel, links oben = 0/0).
    pub x: f32,
    /// Y-Position (Pixel).
    pub y: f32,
    /// Breite (Pixel, > 0).
    pub width: f32,
    /// Höhe (Pixel, > 0).
    pub height: f32,
    /// Skalierungsfaktor (1.0 = Originalgröße, > 0).
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// Optionaler Crop.
    #[serde(default)]
    pub crop: Option<Crop>,
}

impl Default for StageGeometry {
    fn default() -> Self {
        Self::new(1920.0, 1080.0)
    }
}

impl StageGeometry {
    /// Neue Geometrie mit Größe, Position (0/0), Scale 1.0, ohne Crop.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width,
            height,
            scale: 1.0,
            crop: None,
        }
    }

    /// Validiert die Geometrie.
    pub fn validate(&self) -> ShowResult<()> {
        if self.width <= 0.0 {
            return Err(ShowError::InvalidParameter(format!(
                "stage width must be > 0, got {}",
                self.width
            )));
        }
        if self.height <= 0.0 {
            return Err(ShowError::InvalidParameter(format!(
                "stage height must be > 0, got {}",
                self.height
            )));
        }
        if self.scale <= 0.0 {
            return Err(ShowError::InvalidParameter(format!(
                "stage scale must be > 0, got {}",
                self.scale
            )));
        }
        if let Some(crop) = &self.crop {
            crop.validate()?;
        }
        Ok(())
    }
}

/// Eine Ausgabe-Stage (Display/Fenster) mit Geometrie und Layer-Anzahl.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    /// Stabile ID.
    pub id: Uuid,
    /// Anzeigename (z. B. „Main", „Stage Left").
    #[serde(default)]
    pub name: String,
    /// Geometrie der Ausgabefläche.
    #[serde(default)]
    pub geometry: StageGeometry,
    /// Anzahl der Layer (0 = unbegrenzt).
    #[serde(default)]
    pub layers: u32,
    /// Zugeordneter Screen (ID aus [`crate::core::ScreenManager`]).
    ///
    /// `None` = nur virtuelle Vorschau im App-Fenster (kein Hardware-Output).
    /// QLab-Pendant: „Stage → Screen“-Zuordnung. Die Ausgabe erfolgt randlos
    /// und „immer oben" exakt auf dem Screen-Rechteck (ggf. gedreht, siehe
    /// [`crate::core::ScreenRotation`]).
    #[serde(default)]
    pub screen: Option<Uuid>,
    /// Sichtbarkeit der Stage (ausgeblendete Stages werden nicht
    /// ausgegeben, laufende Cues bleiben aber aktiv).
    #[serde(default = "default_true")]
    pub visible: bool,
}

impl Stage {
    /// Neue Stage mit Name und Geometrie.
    pub fn new(name: impl Into<String>, geometry: StageGeometry) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            geometry,
            layers: 0,
            screen: None,
            visible: true,
        }
    }

    /// Validiert die Stage (inkl. Geometrie).
    pub fn validate(&self) -> ShowResult<()> {
        if self.name.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "stage name must not be empty".to_string(),
            ));
        }
        self.geometry.validate()
    }
}

/// Verwaltet alle Stages der Show.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageManager {
    stages: Vec<Stage>,
}

impl StageManager {
    /// Mit einer Default-Stage („Stage 1", 1920×1080).
    pub fn new() -> Self {
        Self {
            stages: vec![Stage::new("Stage 1", StageGeometry::new(1920.0, 1080.0))],
        }
    }

    /// Alle Stages (nur lesend).
    pub fn stages(&self) -> &[Stage] {
        &self.stages
    }

    /// Anzahl der Stages.
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// `true`, wenn keine Stage existiert.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Fügt eine Stage hinzu. Rückgabe: Index.
    pub fn add(&mut self, stage: Stage) -> usize {
        self.stages.push(stage);
        self.stages.len() - 1
    }

    /// Sucht eine Stage nach Namen.
    pub fn find(&self, name: &str) -> Option<&Stage> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// Mutable Variante von [`StageManager::find`].
    pub fn find_mut(&mut self, name: &str) -> Option<&mut Stage> {
        self.stages.iter_mut().find(|s| s.name == name)
    }

    /// Alle Stage-Namen in Reihenfolge.
    pub fn names(&self) -> Vec<String> {
        self.stages.iter().map(|s| s.name.clone()).collect()
    }

    /// Stellt sicher, dass eine Stage mit dem Namen existiert.
    ///
    /// Rückgabe: `true`, wenn eine neue Stage angelegt wurde
    /// (Default-Geometrie 1920×1080).
    pub fn ensure(&mut self, name: &str) -> bool {
        if self.find(name).is_some() {
            return false;
        }
        self.add(Stage::new(name, StageGeometry::new(1920.0, 1080.0)));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_validation() {
        assert!(StageGeometry::new(1920.0, 1080.0).validate().is_ok());
        assert!(StageGeometry::new(0.0, 1080.0).validate().is_err());
        assert!(StageGeometry::new(1920.0, -1.0).validate().is_err());
        let mut g = StageGeometry::new(100.0, 100.0);
        g.scale = 0.0;
        assert!(g.validate().is_err());
    }

    #[test]
    fn crop_validation() {
        assert!(Crop::none().validate().is_ok());
        assert!(
            Crop {
                left: 0.5,
                top: 0.0,
                right: 0.4,
                bottom: 1.0
            }
            .validate()
            .is_err(),
            "left >= right"
        );
        assert!(
            Crop {
                left: 0.0,
                top: 0.0,
                right: 1.1,
                bottom: 1.0
            }
            .validate()
            .is_err(),
            "out of range"
        );
    }

    #[test]
    fn hard_edge_factor_ramp() {
        let edge = HardEdge::new(EdgeSide::Right, 100.0, BlendCurve::Linear);
        assert_eq!(edge.factor(0.0), 0.0, "direkt an der Kante = 0");
        assert_eq!(edge.factor(50.0), 0.5, "Mitte der Blende = 0.5");
        assert_eq!(edge.factor(100.0), 1.0, "innen = 1");
        assert_eq!(edge.factor(250.0), 1.0, "außerhalb = 1");
        assert_eq!(edge.factor(-5.0), 0.0, "außerhalb der Surface = 0");

        let cosine = HardEdge::new(EdgeSide::Left, 100.0, BlendCurve::Cosine);
        let mid = cosine.factor(50.0);
        assert!((mid - 0.5).abs() < 1e-6, "Cosinus-Mitte = 0.5, got {mid}");
        assert!(cosine.factor(25.0) < 0.5, "Cosinus startet weicher");
        assert!(cosine.factor(75.0) > 0.5, "Cosinus endet weicher");

        // factor() ist monoton steigend (keine Sprünge in der Blende).
        let mut prev = 0.0f32;
        for i in 0..=100 {
            let f = edge.factor(i as f32);
            assert!(f >= prev, "monoton steigend bei {i}");
            prev = f;
        }
    }

    #[test]
    fn surface_geometry_validation_and_aoi() {
        let mut s = SurfaceGeometry::full_canvas();
        assert!(s.validate().is_ok());

        s.width = 0.0;
        assert!(s.validate().is_err(), "Breite 0 ungültig");
        s.width = 1920.0;

        s.opacity = 1.5;
        assert!(s.validate().is_err(), "Opacity > 1 ungültig");
        s.opacity = 1.0;

        s.hard_edge = Some(HardEdge::new(EdgeSide::Top, -1.0, BlendCurve::Linear));
        assert!(s.validate().is_err(), "negative Blende ungültig");
        s.hard_edge = Some(HardEdge::new(EdgeSide::Top, 50.0, BlendCurve::Cosine));
        assert!(s.validate().is_ok());

        s.area_of_interest = Some(Crop {
            left: 0.9,
            top: 0.0,
            right: 0.1,
            bottom: 1.0,
        });
        assert!(s.validate().is_err(), "AoI left >= right ungültig");

        // AoI → Pixel-Rechteck der Quelle (halb-offen, geclampt).
        s.area_of_interest = Some(Crop {
            left: 0.25,
            top: 0.0,
            right: 0.75,
            bottom: 1.0,
        });
        assert_eq!(s.source_rect(1920, 1080), (480, 0, 1440, 1080));
        // Ohne AoI = volle Quelle.
        s.area_of_interest = None;
        assert_eq!(s.source_rect(1920, 1080), (0, 0, 1920, 1080));
        // Degenerierter Crop wird auf mindestens 1 px geclampt.
        s.area_of_interest = Some(Crop {
            left: 0.5,
            top: 0.5,
            right: 0.5,
            bottom: 0.5,
        });
        let _ = s.validate(); // validate() fängt left>=right — Crop hier bewusst knapp
        s.area_of_interest = Some(Crop {
            left: 0.5,
            top: 0.5,
            right: 0.5001,
            bottom: 0.5001,
        });
        let (x0, y0, x1, y1) = s.source_rect(100, 100);
        assert!(x1 > x0 && y1 > y0, "mindestens 1 px");
    }

    #[test]
    fn surface_hard_edge_factor_positions() {
        let s = SurfaceGeometry {
            x: 100.0,
            y: 50.0,
            width: 200.0,
            height: 100.0,
            area_of_interest: None,
            hard_edge: Some(HardEdge::new(EdgeSide::Right, 40.0, BlendCurve::Linear)),
            opacity: 1.0,
            visible: true,
        };
        // Rechte Kante bei x = 300: 20 px innen → Faktor 0.5.
        assert_eq!(s.hard_edge_factor(280.0, 100.0), 0.5);
        // Genau an der Kante → 0.
        assert_eq!(s.hard_edge_factor(300.0, 100.0), 0.0);
        // Ganz innen → 1.
        assert_eq!(s.hard_edge_factor(100.0, 100.0), 1.0);
        // Ohne Hard-Edge → immer 1.
        let plain = SurfaceGeometry::full_canvas();
        assert_eq!(plain.hard_edge_factor(0.0, 0.0), 1.0);
    }

    #[test]
    fn stage_screen_and_visibility_serde() {
        let mut stage = Stage::new("Main", StageGeometry::new(1920.0, 1080.0));
        assert_eq!(stage.screen, None);
        assert!(stage.visible);
        let screen_id = Uuid::new_v4();
        stage.screen = Some(screen_id);
        stage.visible = false;

        let json = serde_json::to_string(&stage).unwrap();
        let back: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.screen, Some(screen_id));
        assert!(!back.visible);

        // Alte Shows ohne die neuen Felder laden (Abwärtskompatibilität).
        let legacy = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"Old","geometry":{"x":0.0,"y":0.0,"width":1920.0,"height":1080.0,"scale":1.0},"layers":0}"#;
        let old: Stage = serde_json::from_str(legacy).unwrap();
        assert_eq!(old.screen, None);
        assert!(old.visible);
    }

    #[test]
    fn surface_geometry_serde_roundtrip() {
        let s = SurfaceGeometry {
            x: 10.0,
            y: 20.0,
            width: 640.0,
            height: 360.0,
            area_of_interest: Some(Crop {
                left: 0.1,
                top: 0.2,
                right: 0.9,
                bottom: 1.0,
            }),
            hard_edge: Some(HardEdge::new(EdgeSide::Left, 64.0, BlendCurve::Cosine)),
            opacity: 0.75,
            visible: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: SurfaceGeometry = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn stage_manager_find_and_serde() {
        let mut mgr = StageManager::new();
        assert!(mgr.find("Stage 1").is_some());
        mgr.add(Stage::new("Beamer", StageGeometry::new(1280.0, 720.0)));
        assert_eq!(mgr.len(), 2);
        assert!(mgr.find("Beamer").is_some());
        assert!(mgr.find("Missing").is_none());

        let json = serde_json::to_string(&mgr).unwrap();
        let back: StageManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back.find("Beamer").unwrap().geometry.width, 1280.0);
    }
}
