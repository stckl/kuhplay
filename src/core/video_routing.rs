//! Video-Routing: **Stage → Region → Output Route → Device**.
//!
//! * Eine **Stage** ist die Bildfläche, auf der Cues spielen (mit Layern).
//! * Eine **Region** ist ein Ausschnitt der Stage, der auf genau **eine**
//!   Output Route gezeigt wird (Soft Edge: mehrere Regionen mit Überlappung).
//! * Eine **Output Route** ist ein logischer Ausgang mit eigener Auflösung
//!   (echte Pixel, Ursprung oben links) und zeigt auf 0 oder 1 **Device**
//!   (Display oder Blackmagic), ggf. nur einen Teil davon („Partial Output“).
//! * Mehrere Regionen – auch verschiedener Stages – dürfen dieselbe Route
//!   nutzen; der Renderer stapelt die Stages nach Layer.
//!
//! Interne Koordinaten sind durchgehend Pixel, Ursprung oben links (auch in
//! UI und Datei). Das alte Modell (Screens) wird beim Laden migriert (siehe
//! [`migrate_legacy_screens`]); die Altfelder bleiben lesbar, werden aber
//! nicht mehr geschrieben.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ShowError, ShowResult};
use super::screen::{screen_color_for_index, ScreenRotation, TestPatternKind};

/// Referenz auf ein Ausgabegerät (Display oder Blackmagic).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeviceRef {
    /// Display. `id` kann sich zwischen Rechner-Starts ändern, deshalb
    /// zusätzlich Name und Auflösung (logische Punkte) als Rückfall, wenn
    /// das Display fehlt (Anzeige „(missing)“).
    Display {
        /// Betriebssystem-ID des Displays.
        id: u32,
        /// Anzeigename (Rückfall für fehlende Displays).
        name: String,
        /// Breite in logischen Punkten (Rückfall).
        width: u32,
        /// Höhe in logischen Punkten (Rückfall).
        height: u32,
    },
    /// Blackmagic-Ausgang (Name aus Desktop Video; ohne Gerät „(missing)“).
    Blackmagic { name: String },
}

impl DeviceRef {
    /// UI-Label (Name des Geräts).
    pub fn label(&self) -> &str {
        match self {
            Self::Display { name, .. } => name,
            Self::Blackmagic { name } => name,
        }
    }
}

/// Wie eine Route auf ihr Device abgebildet wird, wenn die Auflösungen
/// nicht übereinstimmen (Route-Skalierung auf dem Device).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RouteScaling {
    /// Unskaliert zentriert; Überstand wird abgeschnitten.
    Center,
    /// Vollständig sichtbar, Seitenverhältnis bleibt; Ränder bleiben frei
    /// (Default: Content wird nie verzerrt auf das Device gezogen).
    #[default]
    Fit,
    /// Füllt das Device, Breite und Höhe werden getrennt skaliert.
    Stretch,
    /// Füllt das Device, Seitenverhältnis bleibt; Überstand wird abgeschnitten.
    Fill,
}

impl RouteScaling {
    /// Alle Skalierungen (für UI-ComboBoxen).
    pub const ALL: [RouteScaling; 4] = [
        RouteScaling::Center,
        RouteScaling::Fit,
        RouteScaling::Stretch,
        RouteScaling::Fill,
    ];

    /// UI-Label .
    pub fn label(self) -> &'static str {
        match self {
            Self::Center => "Center",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
            Self::Fill => "Fill",
        }
    }
}

/// Welchen Teil des Geräts die Route belegt („Partial Output“).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DeviceArea {
    /// Das ganze Gerät.
    #[default]
    Full,
    /// Teil des Geräts, in **Gerätepixeln** (Ursprung oben links).
    Part {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

impl DeviceArea {
    /// Rechteck `(x, y, Breite, Höhe)` in Gerätepixeln (`Full` = 0/0/Gerät).
    pub fn rect(&self, device_w: u32, device_h: u32) -> (u32, u32, u32, u32) {
        match *self {
            Self::Full => (0, 0, device_w, device_h),
            Self::Part {
                x,
                y,
                width,
                height,
            } => (x, y, width, height),
        }
    }
}

/// Ein logischer Video-Ausgang („Output Route“).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputRoute {
    /// Stabile ID (Referenz von [`Region::route`]).
    pub id: Uuid,
    /// Anzeigename (z. B. „Stage Links“, „Beamer 1“).
    pub name: String,
    /// Auflösung in echten Pixeln (1 bis 30000).
    pub width: u32,
    /// Höhe in echten Pixeln (1 bis 30000).
    pub height: u32,
    /// Key-Route (Key/Fill): Alpha bleibt im gerenderten Route-Bild erhalten
    /// (Canvas startet transparent, Compositor rechnet echtes Over-Alpha).
    /// Fill-Routen bleiben flach auf Schwarz. `false` = normal (Fill).
    #[serde(default)]
    pub alpha: bool,
    /// Zielgerät (`None` = fehlt bzw. noch nicht zugewiesen).
    #[serde(default)]
    pub device: Option<DeviceRef>,
    /// Teilbereich auf dem Gerät (Default: ganzes Gerät).
    #[serde(default)]
    pub device_area: DeviceArea,
    /// Drehung auf dem Gerät (0/90/180/270°, im Uhrzeigersinn).
    #[serde(default)]
    pub rotation: ScreenRotation,
    /// Horizontal spiegeln (Rückprojektion).
    #[serde(default)]
    pub rear_projection: bool,
    /// Vertikal spiegeln (z. B. Deckenmontage).
    #[serde(default)]
    pub mirror_vertical: bool,
    /// Skalierung auf dem Gerät bei abweichender Auflösung.
    #[serde(default)]
    pub scaling: RouteScaling,
    /// Farbe der Route (automatisch codiert nach Index, für UI und Testbild).
    #[serde(default = "default_route_color")]
    pub color: [u8; 3],
    /// Testbild über dem vollen Raster der Route („Test Pattern“).
    #[serde(default)]
    pub test_pattern: bool,
    /// Art des Testbilds (Grid / Solid Color / Color Bars / LED Wall).
    #[serde(default)]
    pub test_pattern_kind: TestPatternKind,
    /// Kachelgröße für das LED-Wand-Testbild (Pixel, Default 64).
    #[serde(default = "default_checker_size")]
    pub checker_size: u32,
}

/// Standardfarbe für Routen ohne explizite Farbe (weiß).
fn default_route_color() -> [u8; 3] {
    [255, 255, 255]
}

/// Default-Kachelgröße für LED-Wand-Testbilder.
fn default_checker_size() -> u32 {
    64
}

impl OutputRoute {
    /// Neue Route mit Name und Auflösung (ohne Device).
    pub fn new(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            width,
            height,
            alpha: false,
            device: None,
            device_area: DeviceArea::Full,
            rotation: ScreenRotation::Deg0,
            rear_projection: false,
            mirror_vertical: false,
            scaling: RouteScaling::Fit,
            color: default_route_color(),
            test_pattern: false,
            test_pattern_kind: TestPatternKind::Grid,
            checker_size: default_checker_size(),
        }
    }

    /// Beschriftung für das Testbild: `„Name WIDTHxHEIGHT“`.
    pub fn label(&self) -> String {
        format!("{} {}x{}", self.name, self.width, self.height)
    }

    /// `true`, wenn die Route auf ein vorhandenes Device zeigt.
    pub fn has_device(&self) -> bool {
        self.device.is_some()
    }

    /// Validiert die Route.
    pub fn validate(&self) -> ShowResult<()> {
        if self.name.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "route name must not be empty".to_string(),
            ));
        }
        if !(1..=30000).contains(&self.width) || !(1..=30000).contains(&self.height) {
            return Err(ShowError::InvalidParameter(format!(
                "route size must be 1..=30000, got {}x{}",
                self.width, self.height
            )));
        }
        if let DeviceArea::Part { width, height, .. } = self.device_area {
            if width == 0 || height == 0 {
                return Err(ShowError::InvalidParameter(
                    "route device area must not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Edge-Blend einer Region: Blendbreiten an den vier Kanten, in Pixeln
/// auf der Stage (0 = keine Blende). Der Faktorverlauf folgt
/// `(x/breite)^(1/gamma)` (siehe [`edge_factor`]), damit die Summe beider
/// Projektoren im Überlapp konstant hell bleibt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeBlend {
    /// Blende an der linken Kante (Pixel).
    pub left: f32,
    /// Blende an der rechten Kante (Pixel).
    pub right: f32,
    /// Blende an der oberen Kante (Pixel).
    pub top: f32,
    /// Blende an der unteren Kante (Pixel).
    pub bottom: f32,
}

impl Default for EdgeBlend {
    fn default() -> Self {
        Self::ZERO
    }
}

impl EdgeBlend {
    /// Keine Blende an keiner Kante.
    pub const ZERO: EdgeBlend = EdgeBlend {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    /// Validiert die Blendbreiten.
    pub fn validate(&self) -> ShowResult<()> {
        for (side, v) in [
            ("left", self.left),
            ("right", self.right),
            ("top", self.top),
            ("bottom", self.bottom),
        ] {
            if v < 0.0 || !v.is_finite() {
                return Err(ShowError::InvalidParameter(format!(
                    "edge blend {side} must be >= 0 and finite, got {v}"
                )));
            }
        }
        Ok(())
    }
}

/// Blenden-Faktor am Punkt `t` (0..=1) innerhalb einer Blendbreite:
/// `t^(1/gamma)`. Zwei entgegengesetzte Kanten summieren sich im Überlapp
/// auf konstante Helligkeit, weil der Projektor das Signal mit `gamma`
/// in Licht umsetzt: `(t^(1/γ))^γ + ((1-t)^(1/γ))^γ = t + (1-t) = 1`.
pub fn edge_factor(t: f32, gamma: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let gamma = gamma.max(0.1);
    t.powf(1.0 / gamma)
}

/// Ein Ausschnitt einer Stage, der auf eine Output Route gezeigt wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// Stabile ID.
    pub id: Uuid,
    /// Route, auf die diese Region zeigt (`None` = noch nicht zugewiesen).
    #[serde(default)]
    pub route: Option<Uuid>,
    /// X-Position auf der Stage in Pixeln (Ursprung oben links).
    pub x: f32,
    /// Y-Position auf der Stage in Pixeln.
    pub y: f32,
    /// Breite in Stage-Pixeln (> 0).
    pub width: f32,
    /// Höhe in Stage-Pixeln (> 0).
    pub height: f32,
    /// Edge-Blend-Breiten an den vier Kanten (Stage-Pixel).
    #[serde(default)]
    pub blend: EdgeBlend,
    /// Auto-Blend gegen die Nachbar-Region (Default: an).
    #[serde(default = "default_true")]
    pub auto_blend: bool,
    /// Raster-Overlay (Testbild) nur über dieser Region.
    #[serde(default)]
    pub grid: bool,
}

fn default_true() -> bool {
    true
}

impl Region {
    /// Neue Region (Rechteck auf der Stage, ohne Route).
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            id: Uuid::new_v4(),
            route: None,
            x,
            y,
            width,
            height,
            blend: EdgeBlend::ZERO,
            auto_blend: true,
            grid: false,
        }
    }

    /// Region über die ganze Stage-Fläche.
    pub fn full_stage(stage_w: f32, stage_h: f32) -> Self {
        Self::new(0.0, 0.0, stage_w.max(1.0), stage_h.max(1.0))
    }

    /// Validiert die Region gegen die Stage-Größe (`stage_w`×`stage_h`).
    ///
    /// Regionen müssen vollständig innerhalb der Stage liegen.
    pub fn validate(&self, stage_w: f32, stage_h: f32) -> ShowResult<()> {
        for (name, v) in [
            ("x", self.x),
            ("y", self.y),
            ("width", self.width),
            ("height", self.height),
        ] {
            if !v.is_finite() {
                return Err(ShowError::InvalidParameter(format!(
                    "region {name} must be finite, got {v}"
                )));
            }
        }
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(ShowError::InvalidParameter(format!(
                "region size must be > 0, got {}x{}",
                self.width, self.height
            )));
        }
        let eps = 0.5;
        if self.x < -eps
            || self.y < -eps
            || self.x + self.width > stage_w + eps
            || self.y + self.height > stage_h + eps
        {
            return Err(ShowError::InvalidParameter(format!(
                "region must lie within the stage ({}x{}), got {:.0},{:.0} {}x{}",
                stage_w, stage_h, self.x, self.y, self.width, self.height
            )));
        }
        self.blend.validate()
    }
}

/// Buchstabe einer Region nach Position (Lesefolge oben links → unten
/// rechts): A, B, … Z, dann AA, AB, … Ergibt sich aus der Position und
/// wird nicht gespeichert.
pub fn region_letter(index: usize) -> String {
    let mut n = index + 1;
    let mut out = String::new();
    loop {
        n -= 1;
        out.insert(0, char::from(b'A' + (n % 26) as u8));
        n /= 26;
        if n == 0 {
            break;
        }
    }
    out
}

/// Regionen in Lesefolge (oben nach unten, links nach rechts) sortiert –
/// ergibt die Buchstaben-Reihenfolge A, B, …
pub fn regions_in_reading_order<'a>(
    regions: impl IntoIterator<Item = &'a Region>,
) -> Vec<&'a Region> {
    let mut sorted: Vec<&Region> = regions.into_iter().collect();
    sorted.sort_by(|a, b| {
        (a.y, a.x)
            .partial_cmp(&(b.y, b.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
}

/// Verwaltet alle Output Routes der Show.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteManager {
    routes: Vec<OutputRoute>,
}

impl RouteManager {
    /// Leerer Manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Alle Routen (nur lesend).
    pub fn routes(&self) -> &[OutputRoute] {
        &self.routes
    }

    /// Anzahl der Routen.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// `true`, wenn keine Routen existieren.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Fügt eine Route hinzu (nach Validierung, mit automatischer
    /// Farbcodierung, falls noch die Default-Farbe gesetzt ist).
    /// Rückgabe: Index.
    pub fn add(&mut self, mut route: OutputRoute) -> ShowResult<usize> {
        route.validate()?;
        let index = self.routes.len();
        if route.color == default_route_color() {
            route.color = screen_color_for_index(index);
        }
        self.routes.push(route);
        Ok(index)
    }

    /// Entfernt eine Route per ID. Rückgabe: `true` bei Erfolg.
    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.routes.len();
        self.routes.retain(|r| r.id != id);
        let removed = self.routes.len() != before;
        if removed {
            self.recolor_all();
        }
        removed
    }

    /// Weist allen Routen automatisch Farben aus der Palette zu.
    pub fn recolor_all(&mut self) {
        for (i, r) in self.routes.iter_mut().enumerate() {
            r.color = screen_color_for_index(i);
        }
    }

    /// Sucht eine Route per ID.
    pub fn find(&self, id: Uuid) -> Option<&OutputRoute> {
        self.routes.iter().find(|r| r.id == id)
    }

    /// Mutable Variante von [`RouteManager::find`].
    pub fn find_mut(&mut self, id: Uuid) -> Option<&mut OutputRoute> {
        self.routes.iter_mut().find(|r| r.id == id)
    }

    /// Sucht eine Route per Name (erster Treffer).
    pub fn find_by_name(&self, name: &str) -> Option<&OutputRoute> {
        self.routes.iter().find(|r| r.name == name)
    }

    /// Alle Routen mutable (z. B. Device-Referenzen auflösen).
    pub fn routes_mut(&mut self) -> &mut [OutputRoute] {
        &mut self.routes
    }

    /// Löst Display-Referenzen gegen die aktuell erkannten Displays auf:
    /// Ist die gespeicherte ID nicht mehr da (z. B. abgesteckter Screen),
    /// wird über Name + Auflösung (Punkte) das passende Display wiedergefunden
    /// und die ID aktualisiert. Gibt es keinen Treffer, bleibt die Referenz
    /// als „(missing)“ stehen — die UUID-Verknüpfung Region→Route bleibt davon
    /// unberührt und das restliche Patching verschiebt sich nicht.
    /// Rückgabe: Anzahl aktualisierter Routen.
    pub fn resolve_device_refs(&mut self, displays: &[DisplayRefInfo]) -> usize {
        let mut changed = 0;
        for route in &mut self.routes {
            if let Some(DeviceRef::Display {
                id,
                name,
                width,
                height,
            }) = &mut route.device
            {
                if displays.iter().any(|d| d.id == *id) {
                    continue; // ID noch gültig
                }
                // Rückfall: gleiches Gerät anhand Name + Auflösung finden.
                if let Some(found) = displays
                    .iter()
                    .find(|d| d.name == *name && d.width == *width && d.height == *height)
                {
                    *id = found.id;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Alle Route-Namen in Reihenfolge.
    pub fn names(&self) -> Vec<String> {
        self.routes.iter().map(|r| r.name.clone()).collect()
    }

    /// Alle Route-IDs in Reihenfolge.
    pub fn ids(&self) -> Vec<Uuid> {
        self.routes.iter().map(|r| r.id).collect()
    }

    /// Stellt sicher, dass eine Route mit dem Namen existiert (Default
    /// 1920×1080). Rückgabe: ID der Route.
    pub fn ensure_named(&mut self, name: &str) -> Uuid {
        if let Some(found) = self.find_by_name(name) {
            return found.id;
        }
        let index = self.routes.len();
        let mut route = OutputRoute::new(name, 1920, 1080);
        route.color = screen_color_for_index(index);
        let id = route.id;
        self.routes.push(route);
        id
    }
}

/// Geometrische Display-Info für Migration und Default-Setups (Routen in
/// **Pixeln**: Punkte × `scale_factor`; Fensterplatzierung bleibt in Punkten).
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRefInfo {
    /// Betriebssystem-ID des Displays (für [`DeviceRef::Display`]).
    pub id: u32,
    /// Anzeigename.
    pub name: String,
    /// X-Position in globalen Display-Punkten.
    pub x: i32,
    /// Y-Position in globalen Display-Punkten.
    pub y: i32,
    /// Breite in logischen Punkten.
    pub width: u32,
    /// Höhe in logischen Punkten.
    pub height: u32,
    /// Retina-Skalierung (Punkte → Pixel, z. B. 2.0).
    pub scale_factor: f32,
    /// Bildwiederholrate in Hz (soweit verfügbar, 0 = unbekannt).
    pub frequency: f32,
}

impl DisplayRefInfo {
    /// Auflösung in echten Pixeln (Punkte × `scale_factor`, gerundet).
    pub fn pixel_size(&self) -> (u32, u32) {
        let w = (self.width as f32 * self.scale_factor).round().max(1.0) as u32;
        let h = (self.height as f32 * self.scale_factor).round().max(1.0) as u32;
        (w, h)
    }

    /// `true`, wenn das Rechteck `(x, y, w, h)` (in Punkten) das Display
    /// schneidet.
    pub fn intersects(&self, x: i32, y: i32, w: u32, h: u32) -> bool {
        let (ox1, oy1) = (x, y);
        let (ox2, oy2) = (x + w as i32, y + h as i32);
        self.x < ox2
            && ox1 < self.x + self.width as i32
            && self.y < oy2
            && oy1 < self.y + self.height as i32
    }
}

impl From<&display_info::DisplayInfo> for DisplayRefInfo {
    fn from(d: &display_info::DisplayInfo) -> Self {
        Self {
            id: d.id,
            name: d.name.clone(),
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
            scale_factor: d.scale_factor,
            frequency: d.frequency,
        }
    }
}

/// Default-Setup einer neuen Show: pro Display eine Route
/// („Display n“, Auflösung in Pixeln) und eine Stage in dessen Pixelgröße
/// mit einer Region über die ganze Fläche. `Stage 1` bleibt für die
/// Demo-Cues bestehen und liegt auf dem ersten Display.
///
/// Rückgabe: (Routen, zusätzliche Stages für Displays ab dem zweiten).
/// Die Aufrufer-Stage „Stage 1“ wird vom Aufrufer selbst ergänzt (sie
/// existiert bereits und behält ihre Größe für die Demo-Cues).
pub fn default_routes_for_displays(displays: &[DisplayRefInfo]) -> RouteManager {
    let mut routes = RouteManager::new();
    for (i, d) in displays.iter().enumerate() {
        let (w, h) = d.pixel_size();
        let mut route = OutputRoute::new(format!("Display {}", i + 1), w, h);
        route.device = Some(DeviceRef::Display {
            id: d.id,
            name: d.name.clone(),
            width: d.width,
            height: d.height,
        });
        route.color = screen_color_for_index(i);
        let _ = routes.add(route);
    }
    routes
}

/// Ordnet einer Stage (Canvas `stage_w`×`stage_h`) Regionen zu, die sie
/// proportional auf die Routen aufteilen. `route_ids` muss in der
/// gewünschten Reihenfolge übergeben werden (Links-nach-rechts nach
/// Device-Position, siehe `migrate_legacy_screens`). Skalierung
/// `Stretch`: So sieht eine migrierte Show aus wie vorher, auch wenn sie
/// verzerrt war.
pub fn regions_for_routes_proportional(
    stage_w: f32,
    stage_h: f32,
    route_ids: &[Uuid],
    routes: &RouteManager,
) -> Vec<Region> {
    let widths: Vec<(Uuid, u32)> = route_ids
        .iter()
        .filter_map(|id| routes.find(*id).map(|r| (*id, r.width)))
        .collect();
    let total: u32 = widths.iter().map(|(_, w)| *w).sum();
    let mut regions = Vec::new();
    if total == 0 || widths.is_empty() {
        return regions;
    }
    let mut covered = 0.0f32;
    for (id, width) in &widths {
        let frac = *width as f32 / total as f32;
        let x = covered;
        covered += frac * stage_w;
        let mut region = Region::new(x, 0.0, (covered - x).max(1.0), stage_h);
        region.route = Some(*id);
        regions.push(region);
    }
    regions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: f32, y: f32, w: f32, h: f32) -> Region {
        Region::new(x, y, w, h)
    }

    #[test]
    fn route_validation() {
        assert!(OutputRoute::new("Main", 1920, 1080).validate().is_ok());
        assert!(OutputRoute::new(" ", 1920, 1080).validate().is_err());
        assert!(OutputRoute::new("Main", 0, 1080).validate().is_err());
        assert!(OutputRoute::new("Main", 1920, 30001).validate().is_err());
        let mut r = OutputRoute::new("Main", 1920, 1080);
        r.device_area = DeviceArea::Part {
            x: 0,
            y: 0,
            width: 0,
            height: 100,
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn route_manager_crud_and_colors() {
        let mut m = RouteManager::new();
        assert!(m.is_empty());
        let a = OutputRoute::new("A", 100, 100);
        let b = OutputRoute::new("B", 200, 100);
        m.add(a.clone()).unwrap();
        m.add(b.clone()).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m.names(), vec!["A".to_string(), "B".to_string()]);
        assert_eq!(m.find(a.id).unwrap().name, "A");
        // Farben automatisch codiert nach Index.
        assert_eq!(m.routes()[0].color, screen_color_for_index(0));
        assert_eq!(m.routes()[1].color, screen_color_for_index(1));
        // Entfernen → Farben rücken nach.
        assert!(m.remove(a.id));
        assert_eq!(m.routes()[0].name, "B");
        assert_eq!(m.routes()[0].color, screen_color_for_index(0));
        assert!(!m.remove(a.id), "doppelt entfernen geht nicht");
        // Ungültige Route wird abgelehnt.
        assert!(m.add(OutputRoute::new("", 10, 10)).is_err());
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn ensure_named_is_idempotent() {
        let mut m = RouteManager::new();
        let id1 = m.ensure_named("Beamer");
        let id2 = m.ensure_named("Beamer");
        assert_eq!(id1, id2);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn region_validation_within_stage() {
        assert!(region(0.0, 0.0, 1920.0, 1080.0)
            .validate(1920.0, 1080.0)
            .is_ok());
        // Über die Stage hinaus.
        assert!(region(100.0, 0.0, 1920.0, 1080.0)
            .validate(1920.0, 1080.0)
            .is_err());
        assert!(region(0.0, 0.0, 1921.0, 1080.0)
            .validate(1920.0, 1080.0)
            .is_err());
        // Negativ.
        assert!(region(-10.0, 0.0, 100.0, 100.0)
            .validate(1920.0, 1080.0)
            .is_err());
        // Größe 0 / unendlich.
        assert!(region(0.0, 0.0, 0.0, 100.0)
            .validate(1920.0, 1080.0)
            .is_err());
        assert!(region(0.0, 0.0, f32::NAN, 100.0)
            .validate(1920.0, 1080.0)
            .is_err());
        // Negative Blendbreite.
        let mut r = region(0.0, 0.0, 100.0, 100.0);
        r.blend = EdgeBlend {
            left: -1.0,
            ..EdgeBlend::ZERO
        };
        assert!(r.validate(1920.0, 1080.0).is_err());
    }

    #[test]
    fn region_letters_a_to_z_then_aa() {
        assert_eq!(region_letter(0), "A");
        assert_eq!(region_letter(1), "B");
        assert_eq!(region_letter(25), "Z");
        assert_eq!(region_letter(26), "AA");
        assert_eq!(region_letter(27), "AB");
        assert_eq!(region_letter(52), "BA");
    }

    #[test]
    fn reading_order_sorts_by_y_then_x() {
        let a = region(0.0, 0.0, 100.0, 100.0);
        let b = region(200.0, 0.0, 100.0, 100.0);
        let c = region(100.0, 200.0, 100.0, 100.0);
        let sorted = regions_in_reading_order([&c, &b, &a]);
        let ids: Vec<Uuid> = sorted.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![a.id, b.id, c.id]);
    }

    #[test]
    fn edge_factor_is_gamma_curve() {
        let gamma = 2.2;
        // Mitte der Blende: 0.5^(1/2.2) ≈ 0.730.
        let mid = edge_factor(0.5, gamma);
        assert!((mid - 0.5f32.powf(1.0 / gamma)).abs() < 1e-6);
        // Summe im linearen Licht bleibt konstant 1.
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            // Im linearen Licht (Projektor dekodiert mit gamma) summiert
            // die Blende auf konstante Helligkeit.
            let sum = edge_factor(t, gamma).powf(gamma) + edge_factor(1.0 - t, gamma).powf(gamma);
            assert!(
                (sum - 1.0).abs() < 0.01,
                "Blend-Summe im Licht: t={t}, Summe={sum}"
            );
        }
        assert_eq!(edge_factor(0.0, gamma), 0.0);
        assert_eq!(edge_factor(1.0, gamma), 1.0);
    }

    #[test]
    fn route_scaling_and_device_area_basics() {
        assert_eq!(RouteScaling::default(), RouteScaling::Fit);
        assert_eq!(RouteScaling::ALL.len(), 4);
        assert_eq!(RouteScaling::Fill.label(), "Fill");
        assert_eq!(
            DeviceArea::Full.rect(1920, 1080),
            (0, 0, 1920, 1080),
            "Full deckt das ganze Gerät"
        );
        assert_eq!(
            DeviceArea::Part {
                x: 100,
                y: 50,
                width: 800,
                height: 600
            }
            .rect(1920, 1080),
            (100, 50, 800, 600)
        );
    }

    #[test]
    fn serde_roundtrip_and_legacy_defaults() {
        let mut route = OutputRoute::new("Beamer", 1920, 1080);
        route.device = Some(DeviceRef::Display {
            id: 42,
            name: "Projector".to_string(),
            width: 1920,
            height: 1080,
        });
        route.rear_projection = true;
        route.test_pattern = true;
        route.rotation = ScreenRotation::Deg90;
        let json = serde_json::to_string(&route).unwrap();
        let back: OutputRoute = serde_json::from_str(&json).unwrap();
        assert_eq!(back, route);

        // Alte/kompatible Datei ohne neue Felder → Defaults.
        let legacy = format!(
            r#"{{"id":"{}","name":"Old","width":640,"height":480}}"#,
            route.id
        );
        let old: OutputRoute = serde_json::from_str(&legacy).unwrap();
        assert_eq!(old.device, None);
        assert_eq!(old.device_area, DeviceArea::Full);
        // Default ist Fit (Verhältnis bleibt); explizit gespeichertes Stretch bleibt.
        assert_eq!(old.scaling, RouteScaling::Fit);
        assert!(!old.rear_projection);
        assert_eq!(old.color, [255, 255, 255]);

        // Region-Rundlauf.
        let mut region = Region::full_stage(1920.0, 1080.0);
        region.blend = EdgeBlend {
            left: 100.0,
            ..EdgeBlend::ZERO
        };
        region.grid = true;
        let json = serde_json::to_string(&region).unwrap();
        let back: Region = serde_json::from_str(&json).unwrap();
        assert_eq!(back, region);
        // Legacy-Region ohne blend/auto_blend/grid.
        let legacy = format!(
            r#"{{"id":"{}","x":0.0,"y":0.0,"width":100.0,"height":100.0}}"#,
            region.id
        );
        let old: Region = serde_json::from_str(&legacy).unwrap();
        assert_eq!(old.blend, EdgeBlend::ZERO);
        assert!(old.auto_blend);
        assert!(!old.grid);
        assert_eq!(old.route, None);
    }

    #[test]
    fn default_routes_are_pixel_sized_per_display() {
        let displays = vec![
            DisplayRefInfo {
                id: 1,
                name: "MacBook".to_string(),
                x: 0,
                y: 0,
                width: 1512,
                height: 982,
                scale_factor: 2.0,
                frequency: 60.0,
            },
            DisplayRefInfo {
                id: 2,
                name: "Beamer".to_string(),
                x: 1512,
                y: 0,
                width: 1920,
                height: 1080,
                scale_factor: 1.0,
                frequency: 50.0,
            },
        ];
        let routes = default_routes_for_displays(&displays);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes.routes()[0].name, "Display 1");
        assert_eq!(
            (routes.routes()[0].width, routes.routes()[0].height),
            (3024, 1964)
        );
        assert_eq!(routes.routes()[1].name, "Display 2");
        assert_eq!(
            (routes.routes()[1].width, routes.routes()[1].height),
            (1920, 1080)
        );
        assert!(routes.routes()[0].has_device());
        assert!(matches!(
            routes.routes()[1].device,
            Some(DeviceRef::Display { id: 2, .. })
        ));
    }

    #[test]
    fn proportional_regions_split_stage_by_route_width() {
        let mut routes = RouteManager::new();
        let left = OutputRoute::new("L", 960, 1080);
        let right = OutputRoute::new("R", 1920, 1080);
        let left_id = left.id;
        let right_id = right.id;
        routes.add(left).unwrap();
        routes.add(right).unwrap();
        let regions =
            regions_for_routes_proportional(2880.0, 1080.0, &[left_id, right_id], &routes);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].route, Some(left_id));
        assert_eq!(regions[1].route, Some(right_id));
        // 960 : 1920 = 1 : 2 → linke Region 960, rechte 1920 px breit.
        assert!((regions[0].x - 0.0).abs() < 0.5);
        assert!((regions[0].width - 960.0).abs() < 0.5);
        assert!((regions[1].x - 960.0).abs() < 0.5);
        assert!((regions[1].width - 1920.0).abs() < 0.5);
        // Regionen füllen die Stage vollständig.
        let covered: f32 = regions.iter().map(|r| r.width).sum();
        assert!((covered - 2880.0).abs() < 0.5);
    }
}

#[cfg(test)]
mod resolve_device_refs_tests {
    use super::*;

    fn display(id: u32, name: &str, w: u32, h: u32) -> DisplayRefInfo {
        DisplayRefInfo {
            id,
            name: name.to_string(),
            x: 0,
            y: 0,
            width: w,
            height: h,
            scale_factor: 1.0,
            frequency: 60.0,
        }
    }

    #[test]
    fn device_ref_resolves_by_name_and_size_when_id_is_gone() {
        let mut routes = RouteManager::new();
        let mut route = OutputRoute::new("Beamer", 1920, 1080);
        route.device = Some(DeviceRef::Display {
            id: 42,
            name: "Beamer".to_string(),
            width: 1920,
            height: 1080,
        });
        let route_id = route.id;
        routes.add(route).unwrap();
        // Display mit ID 42 ist weg; gleiches Gerät kommt mit neuer ID 7 zurück.
        let displays = vec![display(7, "Beamer", 1920, 1080)];
        let changed = routes.resolve_device_refs(&displays);
        assert_eq!(changed, 1);
        match &routes.find(route_id).unwrap().device {
            Some(DeviceRef::Display { id, .. }) => assert_eq!(*id, 7),
            other => panic!("Device-Ref verloren: {other:?}"),
        }
    }

    #[test]
    fn device_ref_stays_missing_when_nothing_matches() {
        let mut routes = RouteManager::new();
        let mut route = OutputRoute::new("Weg", 1920, 1080);
        route.device = Some(DeviceRef::Display {
            id: 42,
            name: "Beamer".to_string(),
            width: 1920,
            height: 1080,
        });
        let route_id = route.id;
        routes.add(route).unwrap();
        // Anderer Rechner: nur ein 1512×982-Display, anderer Name.
        let displays = vec![display(1, "MacBook", 1512, 982)];
        let changed = routes.resolve_device_refs(&displays);
        assert_eq!(changed, 0, "nichts verschoben");
        match &routes.find(route_id).unwrap().device {
            Some(DeviceRef::Display { id, .. }) => assert_eq!(*id, 42, "alte ID bleibt (missing)"),
            other => panic!("Device-Ref verloren: {other:?}"),
        }
    }

    #[test]
    fn same_name_but_other_size_is_not_matched() {
        let mut routes = RouteManager::new();
        let mut route = OutputRoute::new("Beamer", 1920, 1080);
        route.device = Some(DeviceRef::Display {
            id: 42,
            name: "Beamer".to_string(),
            width: 1920,
            height: 1080,
        });
        let route_id = route.id;
        routes.add(route).unwrap();
        // Gleicher Name, aber andere Auflösung (anderes Gerät/Modus).
        let displays = vec![display(9, "Beamer", 3840, 2160)];
        assert_eq!(routes.resolve_device_refs(&displays), 0);
        match &routes.find(route_id).unwrap().device {
            Some(DeviceRef::Display { id, .. }) => assert_eq!(*id, 42),
            other => panic!("{other:?}"),
        }
    }
}
