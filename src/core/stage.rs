//! Video-Stages: Ausgabeflächen mit Layer-Verwaltung und Geometrie
//! (Position, Skalierung, Cropping).
//!
//! Die Stage-Metadaten sind das UI-/Domain-Modell; die produktive Video-Engine
//! (GStreamer `videomixer`/Compositor oder native Window-Layer) bildet
//! [`StageGeometry`] auf konkrete Render-Pipelines ab.
//!
//! ## Funktionen
//!
//! * **Stages & Screens:** Eine [`Stage`] ist die virtuelle Ausgabefläche
//!   (Canvas-Größe); [`Stage::screen`] mappt sie per ID auf einen Screen
//!   ([`crate::core::Screen`], Rechteck auf einem Hardware-Output) –
//!   „Stage → Screen"-Zuordnung.
//! * **Surfaces:** Jeder Video-/Diashow-Cue trägt eine [`SurfaceGeometry`]
//!   (Position/Größe auf der Stage, Layer, Deckkraft, Sichtbarkeit).
//! * **Area of Interest:** [`SurfaceGeometry::area_of_interest`] ist der
//!   Zuschnitt der Quelle („Area of Interest“ / Crop) – als normalisiertes
//!   Keep-Rechteck ([`Crop`]), live editierbar.
//! * **Hard Edge Blending:** [`SurfaceGeometry::hard_edge`] weicht die Kante
//!   einer Surface über eine Breite in Pixeln auf (linear oder Cosinus-Kurve)
//!   und blendet so die darunterliegende Surface in der Überlappung ein –
//!   „Hard Edge“-Blending zwischen benachbarten Projektionen.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::error::{ShowError, ShowResult};
use crate::core::video_routing::{EdgeBlend, Region};

fn default_scale() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_opacity() -> f32 {
    1.0
}

/// Default-Blend-Gamma (sRGB-artige Kurve, „Blend Gamma“).
fn default_gamma() -> f32 {
    2.2
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

/// Kurvenform des Hard-Edge-Fades (Linear / Cosine).
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

    /// UI-Label .
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

    /// UI-Label .
    pub fn label(&self) -> &'static str {
        match self {
            BlendCurve::Linear => "Linear",
            BlendCurve::Cosine => "Cosine",
        }
    }
}

/// Hard-Edge-Blending einer Surface („Hard Edge“).
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
            // Cosinus: weicher S-förmiger Verlauf (Cosine).
            BlendCurve::Cosine => 0.5 - 0.5 * (std::f32::consts::PI * t).cos(),
        }
    }
}

/// Blend-Modus einer Surface (Blend Mode, z. B. Multiply/Screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    /// Normale Alpha-Kompositing (Default).
    #[default]
    SourceOver,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    Add,
    Difference,
}

impl BlendMode {
    pub const ALL: [BlendMode; 8] = [
        BlendMode::SourceOver,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Add,
        BlendMode::Difference,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SourceOver => "Source Over",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Add => "Add",
            Self::Difference => "Difference",
        }
    }

    /// Mischfunktion pro Kanal (0..=255).
    pub fn mix(self, dst: f32, src: f32) -> f32 {
        match self {
            Self::SourceOver => src,
            Self::Multiply => dst * src / 255.0,
            Self::Screen => 255.0 - (255.0 - dst) * (255.0 - src) / 255.0,
            Self::Overlay => {
                if dst < 128.0 {
                    2.0 * dst * src / 255.0
                } else {
                    255.0 - 2.0 * (255.0 - dst) * (255.0 - src) / 255.0
                }
            }
            Self::Darken => dst.min(src),
            Self::Lighten => dst.max(src),
            Self::Add => (dst + src).min(255.0),
            Self::Difference => (dst - src).abs(),
        }
    }
}

/// Ziel auf der Stage: `x`, `y`, Breite, Höhe.
pub type SurfaceRect = (f32, f32, f32, f32);
/// Ausschnitt der Quelle: `x0`, `y0`, `x1`, `y1`.
pub type SourceRect = (u32, u32, u32, u32);

/// Wie ein Cue auf der Stage liegt (Geometry-Tab, Mode und Style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GeometryMode {
    /// Ganz sichtbar, Seitenverhältnis bleibt; frei bleibende Fläche ist leer.
    Fit,
    /// Füllt die Stage, Seitenverhältnis bleibt; Überstehendes wird abgeschnitten.
    Fill,
    /// Füllt die Stage, Breite und Höhe werden getrennt skaliert.
    Stretch,
    /// Eigene Position und Größe. Default für ältere Shows, die nur das kannten.
    #[default]
    Custom,
}

impl GeometryMode {
    pub const ALL: [GeometryMode; 4] = [
        GeometryMode::Fit,
        GeometryMode::Fill,
        GeometryMode::Stretch,
        GeometryMode::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GeometryMode::Fit => "Fit",
            GeometryMode::Fill => "Fill",
            GeometryMode::Stretch => "Stretch",
            GeometryMode::Custom => "Custom",
        }
    }
}

/// Geometrie einer Surface (eines Video-/Diashow-Cues) auf einer Stage.
///
/// Surface-Zuweisung eines Video-Cues (Position/Größe auf der
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
    /// („Area of Interest“). `None` = volle Quelle.
    #[serde(default)]
    pub area_of_interest: Option<Crop>,
    /// Hard-Edge-Blending an einer Kante („Hard Edge“).
    #[serde(default)]
    pub hard_edge: Option<HardEdge>,
    /// Deckkraft der Surface (0.0 – 1.0; 1.0 = voll deckend).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Sichtbarkeit (unsichtbare Surfaces werden nicht komponiert).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Lage auf der Stage. `x`/`y`/`width`/`height` gelten nur bei `Custom`.
    #[serde(default)]
    pub mode: GeometryMode,
    /// Seitenverhältnis-Sperre im Custom-Mode: Größenänderung skaliert die
    /// andere Achse mit (Basis ist das Medienformat, sonst das aktuelle Rechteck).
    #[serde(default)]
    pub lock_aspect: bool,
    /// Blend-Modus der Surface (Blend Mode).
    #[serde(default)]
    pub blend_mode: BlendMode,
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
            mode: GeometryMode::Custom,
            lock_aspect: false,
            blend_mode: BlendMode::default(),
        }
    }

    /// Standard für neue Cues: ganz sichtbar, unverzerrt (Fill Stage, Fit).
    pub fn fit() -> Self {
        Self {
            mode: GeometryMode::Fit,
            ..Self::full_canvas()
        }
    }

    /// Ziel-Rechteck auf der Stage (`x`, `y`, Breite, Höhe) und Quell-Rechteck
    /// (`x0`, `y0`, `x1`, `y1`) für ein Bild der Größe `src` auf der Stage `canvas`.
    pub fn placement(&self, src: (u32, u32), canvas: (u32, u32)) -> (SurfaceRect, SourceRect) {
        let (sx0, sy0, sx1, sy1) = self.source_rect(src.0, src.1);
        let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
        let (src_w, src_h) = ((sx1 - sx0).max(1) as f32, (sy1 - sy0).max(1) as f32);
        match self.mode {
            GeometryMode::Custom => (
                (self.x, self.y, self.width, self.height),
                (sx0, sy0, sx1, sy1),
            ),
            GeometryMode::Stretch => ((0.0, 0.0, cw, ch), (sx0, sy0, sx1, sy1)),
            GeometryMode::Fit => {
                let scale = (cw / src_w).min(ch / src_h);
                let (w, h) = (src_w * scale, src_h * scale);
                (((cw - w) / 2.0, (ch - h) / 2.0, w, h), (sx0, sy0, sx1, sy1))
            }
            GeometryMode::Fill => {
                // Vom Bild nur den mittigen Ausschnitt mit dem Format der Stage.
                let scale = (cw / src_w).max(ch / src_h);
                let (keep_w, keep_h) = (cw / scale, ch / scale);
                let x0 = sx0 + ((src_w - keep_w) / 2.0).max(0.0) as u32;
                let y0 = sy0 + ((src_h - keep_h) / 2.0).max(0.0) as u32;
                let x1 = (x0 + keep_w.round().max(1.0) as u32).min(sx1);
                let y1 = (y0 + keep_h.round().max(1.0) as u32).min(sy1);
                ((0.0, 0.0, cw, ch), (x0, y0, x1, y1))
            }
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
    /// „Stage → Screen“-Zuordnung. Die Ausgabe erfolgt randlos
    /// und „immer oben" exakt auf dem Screen-Rechteck (ggf. gedreht, siehe
    /// [`crate::core::ScreenRotation`]).
    /// Altes Modell: zugeordneter Screen. Wird nur noch zum Laden alter
    /// Shows gelesen (beim Speichern nicht mehr geschrieben) und beim
    /// Laden via `migrate_legacy_screens` in Regionen/Routen überführt.
    #[serde(default, skip_serializing)]
    pub screen: Option<Uuid>,
    /// Sichtbarkeit der Stage (ausgeblendete Stages werden nicht
    /// ausgegeben, laufende Cues bleiben aber aktiv).
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Regionen: Ausschnitte der Stage, die auf Output Routes zeigen
    /// (leer = Stage ohne Ausgabe, z. B. nur Vorschau/Audition).
    #[serde(default)]
    pub regions: Vec<Region>,
    /// Stage-Layer: höher = oben („Stage Layer“). Bei gleichem Layer
    /// liegt der alphabetisch frühere Name oben.
    #[serde(default)]
    pub layer: i32,
    /// Blend-Gamma der Region-Edge-Blends (Default 2.2).
    #[serde(default = "default_gamma")]
    pub blend_gamma: f32,
    /// Stage zwischen Cues schwarz halten („Keep Rendering“).
    #[serde(default)]
    pub keep_rendering: bool,
    /// Stage-Crop (Sichtfenster, normalisiert wie [`Crop`]): Nur dieser
    /// Ausschnitt des Stage-Canvas geht auf die Regionen. Regionen
    /// bleiben in absoluten Stage-Koordinaten, der Crop beschneidet sie.
    #[serde(default)]
    pub crop: Option<Crop>,
    /// Stage-Randabblendung: Die Kanten der Stage werden am Route-Ausgang
    /// abgeblendet (Breiten in Stage-Pixeln, wie Region-Edge-Blends).
    #[serde(default)]
    pub edge: EdgeBlend,
    /// `true` = Soft Edge (Gamma-Kurve wie bei Regionen), `false` = Hard
    /// Edge (linearer Rampe).
    #[serde(default = "default_true")]
    pub soft_edge: bool,
    /// Direkte Platzierung der Stage auf Displays (in Gerätepixeln des
    /// jeweiligen Displays, 1:1 – das Display bleibt immer voll/native).
    /// Was über den Displayrand hinausragt, wird abgeschnitten (Clipping).
    /// Z-Reihenfolge über mehreren Stages auf demselben Display: `layer`.
    /// Unabhängig von den Region→Route-Mappings (additiv bei der Ausgabe).
    #[serde(default)]
    pub placements: Vec<StagePlacement>,
    /// Key/Fill-Ausgabe dieser Stage (pro Stage separat aktivierbar):
    /// Fill-Route (Farbe, flach) + Key-Route (Alpha; Route-Flag `alpha`).
    /// `None` = kein Key/Fill. Geräte: z. B. zwei Decklink-Ports (Fill/Key
    /// als zwei SDI-Ausgänge) oder ein key/fill-fähiger Ausgang (Hardware
    /// keyt intern, wenn Frames mit Alpha an einem Ausgang anliegen).
    #[serde(default)]
    pub key_fill: Option<KeyFillRoutes>,
}

/// Fill- + Key-Route einer Stage für Key/Fill-Ausgabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFillRoutes {
    /// Route mit dem Fill-Bild (Farbe, Alpha wird flachgerechnet).
    pub fill: Uuid,
    /// Route mit dem Key-Bild (Alpha bleibt erhalten, Route-Flag `alpha`).
    pub key: Uuid,
}

/// Platzierung einer Stage auf einem Display.
///
/// Koordinaten sind **Gerätepixel des Displays** (Ursprung oben links);
/// `x`/`y` dürfen negativ sein (Stage ragt dann links/oben hinaus und wird
/// am Displayrand abgeschnitten). `width`/`height` = Größe, auf die die
/// Stage auf dem Display abgebildet wird (bei Stage-Größe = 1:1 Pixel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagePlacement {
    /// OS-ID des Displays (siehe [`crate::core::DeviceRef::Display`]).
    pub display: u32,
    /// X-Position auf dem Display in Gerätepixeln (negativ = Überstand links).
    pub x: i32,
    /// Y-Position auf dem Display in Gerätepixeln (negativ = Überstand oben).
    pub y: i32,
    /// Breite der Stage-Abbildung auf dem Display in Gerätepixeln (> 0).
    pub width: u32,
    /// Höhe der Stage-Abbildung auf dem Display in Gerätepixeln (> 0).
    pub height: u32,
}

impl StagePlacement {
    /// Prüft die Platzierung (Breite/Höhe > 0).
    pub fn validate(&self) -> ShowResult<()> {
        if self.width == 0 || self.height == 0 {
            return Err(ShowError::InvalidParameter(
                "stage placement: width/height must be > 0".to_string(),
            ));
        }
        Ok(())
    }

    /// Rechteck der Platzierung in Gerätepixeln: `(x, y, w, h)`.
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }

    /// Schnitt mit dem Display `(0, 0, disp_w, disp_h)`: `(x, y, w, h)` des
    /// sichtbaren Teils; `None`, wenn die Stage das Display komplett
    /// verfehlt. Der Überstand bleibt in `self` sichtbar (Visualisierung).
    pub fn clip_to_display(&self, disp_w: u32, disp_h: u32) -> Option<(i32, i32, u32, u32)> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.width as i32).min(disp_w as i32);
        let y1 = (self.y + self.height as i32).min(disp_h as i32);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }
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
            regions: Vec::new(),
            layer: 0,
            blend_gamma: default_gamma(),
            keep_rendering: false,
            crop: None,
            edge: EdgeBlend::ZERO,
            soft_edge: true,
            placements: Vec::new(),
            key_fill: None,
        }
    }

    /// Validiert die Stage (inkl. Geometrie).
    pub fn validate(&self) -> ShowResult<()> {
        if self.name.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "stage name must not be empty".to_string(),
            ));
        }
        self.geometry.validate()?;
        for region in &self.regions {
            region.validate(self.geometry.width, self.geometry.height)?;
        }
        if let Some(crop) = &self.crop {
            crop.validate()?;
        }
        self.edge.validate()?;
        for placement in &self.placements {
            placement.validate()?;
        }
        if let Some(kf) = &self.key_fill {
            if kf.fill == kf.key {
                return Err(ShowError::InvalidParameter(
                    "key/fill: fill and key route must differ".to_string(),
                ));
            }
        }
        if !self.blend_gamma.is_finite() || !(0.1..=10.0).contains(&self.blend_gamma) {
            return Err(ShowError::InvalidParameter(format!(
                "stage blend_gamma must be within 0.1-10.0, got {}",
                self.blend_gamma
            )));
        }
        Ok(())
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

    /// Alle Stages (mutable, z. B. für die Migration).
    pub fn stages_mut(&mut self) -> &mut [Stage] {
        &mut self.stages
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
    /// Entfernt eine Stage. `true`, wenn es sie gab.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.stages.len();
        self.stages.retain(|stage| stage.name != name);
        self.stages.len() != before
    }

    /// Alle Stage-Namen.
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

    /// Stages in Zeichenreihenfolge (unten zuerst): Layer aufsteigend;
    /// bei gleichem Layer der alphabetisch **spätere** Name zuerst, damit
    /// der frühere Name oben liegt (Layer-Regel).
    pub fn sorted_back_to_front(&self) -> Vec<&Stage> {
        let mut sorted: Vec<&Stage> = self.stages.iter().collect();
        sorted.sort_by(|a, b| a.layer.cmp(&b.layer).then_with(|| b.name.cmp(&a.name)));
        sorted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_modes_fit_fill_stretch_custom() {
        let surface = |mode| SurfaceGeometry {
            mode,
            ..SurfaceGeometry::full_canvas()
        };
        // 16:9-Bild auf quadratischer Stage.
        let (src, canvas) = ((1920, 1080), (1000, 1000));
        let (dest, source) = surface(GeometryMode::Fit).placement(src, canvas);
        assert_eq!(source, (0, 0, 1920, 1080), "ganzes Bild");
        assert_eq!((dest.2.round(), dest.3.round()), (1000.0, 563.0));
        assert_eq!((dest.0.round(), dest.1.round()), (0.0, 219.0), "mittig");

        let (dest, source) = surface(GeometryMode::Fill).placement(src, canvas);
        assert_eq!(dest, (0.0, 0.0, 1000.0, 1000.0));
        assert_eq!(source, (420, 0, 1500, 1080), "Mitte, Seiten abgeschnitten");

        let (dest, source) = surface(GeometryMode::Stretch).placement(src, canvas);
        assert_eq!(dest, (0.0, 0.0, 1000.0, 1000.0));
        assert_eq!(source, (0, 0, 1920, 1080));

        let custom = SurfaceGeometry {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
            ..SurfaceGeometry::full_canvas()
        };
        assert_eq!(custom.placement(src, canvas).0, (10.0, 20.0, 30.0, 40.0));
    }

    #[test]
    fn old_surface_json_stays_custom() {
        let json = r#"{"x":1.0,"y":2.0,"width":3.0,"height":4.0}"#;
        let surface: SurfaceGeometry = serde_json::from_str(json).unwrap();
        assert_eq!(surface.mode, GeometryMode::Custom);
    }

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
            mode: GeometryMode::Custom,
            lock_aspect: false,
            blend_mode: Default::default(),
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

        // Screen ist ein Legacy-Feld: wird gelesen, aber nicht mehr
        // gespeichert (Migration in `migrate_legacy_screens`).
        let json = serde_json::to_string(&stage).unwrap();
        let back: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.screen, None, "Legacy-Feld nicht mehr schreiben");
        assert!(!back.visible);

        // Alte Show-Dateien mit screen-Zuordnung laden weiter.
        let legacy = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"Old","geometry":{"x":0.0,"y":0.0,"width":1920.0,"height":1080.0,"scale":1.0},"layers":0,"screen":"00000000-0000-0000-0000-000000000001"}"#;
        let old: Stage = serde_json::from_str(legacy).unwrap();
        assert!(old.screen.is_some(), "Legacy-screen wird gelesen");
        assert!(old.visible);
    }

    #[test]
    fn stage_regions_layer_and_defaults_serde() {
        let mut stage = Stage::new("Main", StageGeometry::new(1920.0, 1080.0));
        let mut region = crate::core::Region::full_stage(1920.0, 1080.0);
        region.route = Some(Uuid::new_v4());
        stage.regions = vec![region];
        stage.layer = 5;
        stage.blend_gamma = 1.8;
        stage.keep_rendering = true;
        let json = serde_json::to_string(&stage).unwrap();
        let back: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, stage);

        // Alte Shows ohne die neuen Felder laden (Abwärtskompatibilität).
        let legacy = r#"{"id":"00000000-0000-0000-0000-000000000000","name":"Old","geometry":{"x":0.0,"y":0.0,"width":1920.0,"height":1080.0,"scale":1.0},"layers":0}"#;
        let old: Stage = serde_json::from_str(legacy).unwrap();
        assert!(old.regions.is_empty());
        assert_eq!(old.layer, 0);
        assert!((old.blend_gamma - 2.2).abs() < 1e-6);
        assert!(!old.keep_rendering);
    }

    #[test]
    fn stage_validate_checks_regions_and_gamma() {
        let mut stage = Stage::new("Main", StageGeometry::new(1920.0, 1080.0));
        assert!(stage.validate().is_ok());
        // Region außerhalb der Stage.
        stage.regions = vec![crate::core::Region::new(1000.0, 0.0, 1000.0, 100.0)];
        assert!(stage.validate().is_err(), "Region ragt über die Stage");
        stage.regions = vec![crate::core::Region::full_stage(1920.0, 1080.0)];
        assert!(stage.validate().is_ok());
        // Blend-Gamma außerhalb des sinnvollen Bereichs.
        stage.blend_gamma = 0.0;
        assert!(stage.validate().is_err());
        stage.blend_gamma = 11.0;
        assert!(stage.validate().is_err());
        stage.blend_gamma = 2.2;
        assert!(stage.validate().is_ok());
    }

    #[test]
    fn stage_layer_sorting_back_to_front() {
        let mut m = StageManager::new();
        // Layer 1: "Beta" und "Alpha"; Layer 0: "Bottom".
        let mut beta = Stage::new("Beta", StageGeometry::new(100.0, 100.0));
        beta.layer = 1;
        let mut alpha = Stage::new("Alpha", StageGeometry::new(100.0, 100.0));
        alpha.layer = 1;
        let mut bottom = Stage::new("Bottom", StageGeometry::new(100.0, 100.0));
        bottom.layer = 0;
        m.stages_mut()[0] = beta;
        m.add(alpha);
        m.add(bottom);
        let order: Vec<&str> = m
            .sorted_back_to_front()
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        // Unten zuerst: Layer 0, dann bei Layer 1 der alphabetisch spätere
        // Name ("Beta") zuerst – "Alpha" liegt oben.
        assert_eq!(order, vec!["Bottom", "Beta", "Alpha"]);
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
            mode: GeometryMode::Custom,
            lock_aspect: false,
            blend_mode: Default::default(),
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

#[cfg(test)]
mod placement_tests {
    use super::*;

    #[test]
    fn placement_clip_to_display_inside_outside_and_full_miss() {
        // Komplett innen.
        let p = StagePlacement {
            display: 1,
            x: 100,
            y: 50,
            width: 800,
            height: 600,
        };
        assert_eq!(p.clip_to_display(1920, 1080), Some((100, 50, 800, 600)));
        // Teils links/oben hinaus → nur der innen liegende Teil.
        let p = StagePlacement {
            display: 1,
            x: -200,
            y: -100,
            width: 800,
            height: 600,
        };
        assert_eq!(p.clip_to_display(1920, 1080), Some((0, 0, 600, 500)));
        // Teils rechts/unten hinaus.
        let p = StagePlacement {
            display: 1,
            x: 1820,
            y: 1030,
            width: 800,
            height: 600,
        };
        assert_eq!(p.clip_to_display(1920, 1080), Some((1820, 1030, 100, 50)));
        // Komplett außerhalb → None (nichts sichtbar).
        let p = StagePlacement {
            display: 1,
            x: 5000,
            y: 0,
            width: 100,
            height: 100,
        };
        assert_eq!(p.clip_to_display(1920, 1080), None);
        // Genau am Rand → leer.
        let p = StagePlacement {
            display: 1,
            x: 1920,
            y: 0,
            width: 100,
            height: 100,
        };
        assert_eq!(p.clip_to_display(1920, 1080), None);
    }

    #[test]
    fn placement_validate_and_stage_serde_legacy() {
        let bad = StagePlacement {
            display: 1,
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        };
        assert!(bad.validate().is_err());
        let ok = StagePlacement {
            display: 1,
            x: -5,
            y: 0,
            width: 100,
            height: 100,
        };
        assert!(ok.validate().is_ok());

        // Stage mit Placement roundtrip; alte Stage ohne placements → leer.
        let mut stage = Stage::new("Main", StageGeometry::new(1920.0, 1080.0));
        stage.placements.push(StagePlacement {
            display: 7,
            x: -100,
            y: 20,
            width: 2000,
            height: 1080,
        });
        let json = serde_json::to_string(&stage).unwrap();
        let back: Stage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.placements.len(), 1);
        assert_eq!(back.placements[0].x, -100);
        assert!(back.validate().is_ok());
        let old_json = json.replace(
            ",\"placements\":[{\"display\":7,\"x\":-100,\"y\":20,\"width\":2000,\"height\":1080}]",
            "",
        );
        let back_old: Stage = serde_json::from_str(&old_json).unwrap();
        assert!(back_old.placements.is_empty(), "fehlendes Feld → Default");
    }
}
