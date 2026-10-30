//! Screens (QLab 5 „Screens"): physische Ausgabebereiche im globalen
//! Display-Koordinatenraum.
//!
//! Ein **Screen** ist ein Rechteck `(x, y, width, height)` in dem Koordinaten-
//! system, das `display-info` liefert (logische Punkte, Ursprung links oben
//! des primären Displays – identisch mit den winit/egui-Viewport-Koordinaten).
//!
//! Im Gegensatz zu einem ganzen Display kann ein Screen (QLab-Verhalten):
//!
//! * **nur einen Teil eines Displays** umfassen (z. B. rechte Hälfte für
//!   eine Soft-Edge-Projektion),
//! * **mehrere Displays umfassen** (ein Screen, der über zwei Monitore
//!   gespannt ist – das Canvas wird über die gesamte Fläche gestreckt),
//! * **neben anderen Screens auf demselben Display** liegen (z. B. zwei
//!   Screens à halbe Displaybreite).
//!
//! Eine **Stage** wird einem Screen zugeordnet ([`crate::core::Stage::screen`] =
//! Screen-ID) und das komponierte Stage-Canvas wird randlos/„immer oben"
//! exakt auf dem Screen-Rechteck ausgegeben (echtes Fullscreen-Verhalten
//! pro Screen, siehe `src/gui/stage_view.rs`).
//!
//! Zusätzlich kann pro Screen ein **Testbild** aktiviert werden
//! ([`Screen::test_pattern`]): Rahmen, Kreis, Raster, leichtes Schachbrett-
//! muster und eine Beschriftung mit Screen-Name und -Größe – zum Einrichten
//! und Prüfen der Screens (QLab „Test Pattern").
//!
//! ## Erweiterung
//!
//! * Pro-Screen Helligkeits-/Gamma-Korrektur (Soft-Edge-Setups).
//! * Persistenz der Screens in `ShowDocument` (aktuell: App-intern +
//!   serde-fähig).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ShowError, ShowResult};

/// Ein physischer Ausgabebereich (Screen) im globalen Display-Koordinatenraum.
///
/// QLab-Pendant: „Screen" (frei platzierbares Rechteck über einem oder
/// mehreren Displays).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    /// Stabile ID (Referenz von [`crate::core::Stage::screen`]).
    pub id: Uuid,
    /// Anzeigename (z. B. „Main", „Bühne links"). Wird im Testbild als
    /// Beschriftung angezeigt.
    pub name: String,
    /// X-Position in globalen Display-Koordinaten (logische Punkte).
    pub x: i32,
    /// Y-Position in globalen Display-Koordinaten.
    pub y: i32,
    /// Breite in logischen Punkten (> 0).
    pub width: u32,
    /// Höhe in logischen Punkten (> 0).
    pub height: u32,
    /// Drehung des Screens auf dem Output (0/90/180/270°).
    ///
    /// QLab-Pendant: „Screen Rotation". Das Stage-Canvas wird um diesen
    /// Winkel **im Uhrzeigersinn** gedreht auf dem Screen dargestellt –
    /// z. B. für einen um 90° gedreht montierten Projektor oder eine
    /// hochkant laufende LED-Wand. `90°` bedeutet: die obere Canvas-Kante
    /// erscheint an der **rechten** Screen-Kante.
    #[serde(default)]
    pub rotation: ScreenRotation,
    /// Testbild auf diesem Screen anzeigen (Rahmen/Kreis/Raster/Schachbrett/
    /// Beschriftung) – QLab „Test Pattern".
    #[serde(default)]
    pub test_pattern: bool,
    /// Art des Testbilds (Grid / Solid Color / Color Bars).
    #[serde(default)]
    pub test_pattern_kind: TestPatternKind,
    /// Farbe des Screens (automatisch codiert nach Index, für Solid-Color-
    /// Testbilder und Unterscheidung auf LED-Wänden/Projektionen).
    #[serde(default = "default_screen_color")]
    pub color: [u8; 3],
    /// Kachelgröße für das LED-Wand-Testbild (Schachbrett-Kacheln in Pixel,
    /// z. B. 64 für 64×64-LED-Module). Default: 64.
    #[serde(default = "default_checker_size")]
    pub checker_size: u32,
}

/// Standardfarbe für Screens ohne explizite Farbe (weiß).
fn default_screen_color() -> [u8; 3] {
    [255, 255, 255]
}

/// Default-Kachelgröße für LED-Wand-Testbilder (64×64 px).
fn default_checker_size() -> u32 {
    64
}

/// Art des Testbilds (QLab „Test Pattern" – verschiedene Varianten zum
/// Einrichten und Prüfen der Screens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TestPatternKind {
    /// Rahmen + Kreis + Raster + Schachbrett + Beschriftung (QLab-Standard).
    #[default]
    Grid,
    /// Einfarbige Fläche (Farbe des Screens) + Beschriftung – zum
    /// Ausrichten und Unterscheiden von Projektoren/LED-Modulen.
    SolidColor,
    /// SMPTE-Farbbalken (Weiß, Gelb, Cyan, Grün, Magenta, Rot, Blau) +
    /// Beschriftung – zum Kalibrieren von Farbe/Helligkeit.
    ColorBars,
    /// LED-Wand-Testbild: Schachbrettmuster mit durchnummerierten Kacheln
    /// (Größe in Pixel einstellbar), 1px-Rahmen am Rand, 1px-Kreis in der
    /// Mitte, Beschriftung zentriert – Hintergrund in der Screen-Farbe.
    /// Zum Einrichten und Prüfen von LED-Wänden (Pixel-Ausrichtung,
    /// Modul-Identifikation).
    LedWall,
}

impl TestPatternKind {
    /// Alle Pattern-Typen (für UI-ComboBoxen).
    pub const ALL: [TestPatternKind; 4] = [
        TestPatternKind::Grid,
        TestPatternKind::SolidColor,
        TestPatternKind::ColorBars,
        TestPatternKind::LedWall,
    ];

    /// UI-Label.
    pub fn label(&self) -> &'static str {
        match self {
            TestPatternKind::Grid => "Grid (Rahmen/Kreis/Raster)",
            TestPatternKind::SolidColor => "Solid Color",
            TestPatternKind::ColorBars => "Color Bars (SMPTE)",
            TestPatternKind::LedWall => "LED Wall (Schachbrett nummeriert)",
        }
    }
}

/// Palette für automatische Screen-Farbcodierung (gut unterscheidbar,
/// auch bei Projektion auf LED-Wände).
pub const SCREEN_COLOR_PALETTE: [[u8; 3]; 8] = [
    [255, 255, 255], // Weiß
    [255, 64, 64],   // Rot
    [64, 255, 64],   // Grün
    [64, 128, 255],  // Blau
    [255, 255, 64],  // Gelb
    [255, 64, 255],  // Magenta
    [64, 255, 255],  // Cyan
    [255, 128, 0],   // Orange
];

/// Farbe für einen Screen-Index (zyklisch durch die Palette).
pub fn screen_color_for_index(index: usize) -> [u8; 3] {
    SCREEN_COLOR_PALETTE[index % SCREEN_COLOR_PALETTE.len()]
}

/// Drehung eines Screens in 90°-Schritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScreenRotation {
    /// Keine Drehung.
    #[default]
    Deg0,
    /// 90° im Uhrzeigersinn (obere Canvas-Kante erscheint rechts).
    Deg90,
    /// 180°.
    Deg180,
    /// 270° im Uhrzeigersinn (= 90° gegen den Uhrzeigersinn).
    Deg270,
}

impl ScreenRotation {
    /// Alle Drehungen (für UI-ComboBoxen).
    pub const ALL: [ScreenRotation; 4] = [
        ScreenRotation::Deg0,
        ScreenRotation::Deg90,
        ScreenRotation::Deg180,
        ScreenRotation::Deg270,
    ];

    /// Drehung in Grad.
    pub fn degrees(&self) -> u16 {
        match self {
            ScreenRotation::Deg0 => 0,
            ScreenRotation::Deg90 => 90,
            ScreenRotation::Deg180 => 180,
            ScreenRotation::Deg270 => 270,
        }
    }

    /// UI-Label (z. B. „90°").
    pub fn label(&self) -> &'static str {
        match self {
            ScreenRotation::Deg0 => "0°",
            ScreenRotation::Deg90 => "90°",
            ScreenRotation::Deg180 => "180°",
            ScreenRotation::Deg270 => "270°",
        }
    }

    /// Nächste Drehung (zyklisch) – für „Drehen"-Buttons.
    pub fn next(&self) -> Self {
        match self {
            ScreenRotation::Deg0 => ScreenRotation::Deg90,
            ScreenRotation::Deg90 => ScreenRotation::Deg180,
            ScreenRotation::Deg180 => ScreenRotation::Deg270,
            ScreenRotation::Deg270 => ScreenRotation::Deg0,
        }
    }

    /// Größe des Canvas-Footprints nach der Drehung: Bei 90°/270° sind
    /// Breite und Höhe vertauscht (ein 1920×1080-Canvas belegt auf einem
    /// 90°-gedrehten Screen 1080×1920).
    pub fn rotated_size(&self, width: u32, height: u32) -> (u32, u32) {
        match self {
            ScreenRotation::Deg0 | ScreenRotation::Deg180 => (width, height),
            ScreenRotation::Deg90 | ScreenRotation::Deg270 => (height, width),
        }
    }
}

impl Screen {
    /// Neuer Screen mit Name und Rechteck.
    pub fn new(name: impl Into<String>, x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            x,
            y,
            width,
            height,
            rotation: ScreenRotation::Deg0,
            test_pattern: false,
            test_pattern_kind: TestPatternKind::Grid,
            color: default_screen_color(),
            checker_size: default_checker_size(),
        }
    }

    /// Rechteck als `(x, y, width, height)`.
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }

    /// Rechteck als `x1, y1` (rechts unten, exklusiv).
    pub fn rect_edges(&self) -> (i32, i32, i32, i32) {
        (
            self.x,
            self.y,
            self.x + self.width as i32,
            self.y + self.height as i32,
        )
    }

    /// Fläche in Punkten².
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Überschneidet sich dieser Screen mit dem Rechteck `(x, y, w, h)`?
    pub fn intersects(&self, x: i32, y: i32, w: u32, h: u32) -> bool {
        let (x1, y1, x2, y2) = self.rect_edges();
        let (ox1, oy1, ox2, oy2) = (x, y, x + w as i32, y + h as i32);
        x1 < ox2 && ox1 < x2 && y1 < oy2 && oy1 < y2
    }

    /// Überdeckt dieser Screen das Display `(x, y, w, h)` vollständig?
    pub fn covers(&self, x: i32, y: i32, w: u32, h: u32) -> bool {
        let (x1, y1, x2, y2) = self.rect_edges();
        x1 <= x && y1 <= y && x2 >= x + w as i32 && y2 >= y + h as i32
    }

    /// Beschriftung für das Testbild: `„Name WIDTHxHEIGHT"`.
    pub fn label(&self) -> String {
        format!("{} {}x{}", self.name, self.width, self.height)
    }

    /// Beschriftung mit Nummer: `„#N Name WIDTHxHEIGHT"` (für Testbild).
    pub fn label_numbered(&self, index: usize) -> String {
        format!(
            "#{} {} {}x{}",
            index + 1,
            self.name,
            self.width,
            self.height
        )
    }

    /// Spezifikation für den Engine-Testbild-Renderer.
    pub fn pattern_spec(&self) -> TestPatternSpec {
        TestPatternSpec {
            label: self.label(),
            width: self.width,
            height: self.height,
            kind: self.test_pattern_kind,
            color: self.color,
            index: 0,
            checker_size: self.checker_size,
        }
    }

    /// Spezifikation mit Screen-Nummer (für Testbild-Beschriftung).
    pub fn pattern_spec_numbered(&self, index: usize) -> TestPatternSpec {
        TestPatternSpec {
            label: self.label_numbered(index),
            width: self.width,
            height: self.height,
            kind: self.test_pattern_kind,
            color: self.color,
            index,
            checker_size: self.checker_size,
        }
    }

    /// Validiert den Screen.
    pub fn validate(&self) -> ShowResult<()> {
        if self.name.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "screen name must not be empty".to_string(),
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err(ShowError::InvalidParameter(format!(
                "screen size must be > 0, got {}x{}",
                self.width, self.height
            )));
        }
        Ok(())
    }
}

/// Spezifikation eines Testbilds für die Engine (Screen-Name + -Größe).
///
/// Wird per `EngineCommand::SetTestPattern` an die Engine gemeldet; der
/// Compositor zeichnet das Testbild über das Stage-Canvas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestPatternSpec {
    /// Beschriftung (z. B. `„#1 Main 1920x1080"`).
    pub label: String,
    /// Breite des Screens in Punkten (für Raster/Kreis-Proportionen).
    pub width: u32,
    /// Höhe des Screens in Punkten.
    pub height: u32,
    /// Art des Testbilds (Grid / Solid Color / Color Bars).
    #[serde(default)]
    pub kind: TestPatternKind,
    /// Farbe des Screens (für Solid-Color-Testbilder).
    #[serde(default = "default_screen_color")]
    pub color: [u8; 3],
    /// Screen-Nummer (0-basiert, für Beschriftung und Farbcodierung).
    #[serde(default)]
    pub index: usize,
    /// Kachelgröße für LED-Wand-Schachbrettmuster (in Pixel, z. B. 64 für
    /// 64×64-LED-Module). Default: 64.
    #[serde(default = "default_checker_size")]
    pub checker_size: u32,
}

/// Geometrische Display-Beschreibung (plattform-unabhängig).
///
/// Wird von [`ScreenManager`] genutzt, damit die Screen-Geometrie ohne
/// plattform-spezifische Typen getestet werden kann. Implementiert für
/// `display_info::DisplayInfo` (logische Punkte = egui-Viewport-Einheiten).
pub trait DisplayGeometry {
    /// Anzeigename des Displays.
    fn display_name(&self) -> &str;
    /// X-Position in globalen Display-Koordinaten (logische Punkte).
    fn display_x(&self) -> i32;
    /// Y-Position in globalen Display-Koordinaten (logische Punkte).
    fn display_y(&self) -> i32;
    /// Breite in logischen Punkten.
    fn display_width(&self) -> u32;
    /// Höhe in logischen Punkten.
    fn display_height(&self) -> u32;
}

impl DisplayGeometry for display_info::DisplayInfo {
    fn display_name(&self) -> &str {
        &self.name
    }
    fn display_x(&self) -> i32 {
        self.x
    }
    fn display_y(&self) -> i32 {
        self.y
    }
    fn display_width(&self) -> u32 {
        self.width
    }
    fn display_height(&self) -> u32 {
        self.height
    }
}

/// Verwaltet alle Screens der Show.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScreenManager {
    screens: Vec<Screen>,
}

impl ScreenManager {
    /// Leerer Manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Alle Screens (nur lesend).
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    /// Anzahl der Screens.
    pub fn len(&self) -> usize {
        self.screens.len()
    }

    /// `true`, wenn keine Screens existieren.
    pub fn is_empty(&self) -> bool {
        self.screens.is_empty()
    }

    /// Fügt einen Screen hinzu. Rückgabe: Index.
    ///
    /// Validiert den Screen vor dem Hinzufügen und weist automatisch eine
    /// Farbe aus der Palette zu (basierend auf dem Index), falls die Farbe
    /// noch die Default-Farbe (weiß) ist.
    pub fn add(&mut self, mut screen: Screen) -> ShowResult<usize> {
        screen.validate()?;
        let index = self.screens.len();
        if screen.color == default_screen_color() {
            screen.color = screen_color_for_index(index);
        }
        self.screens.push(screen);
        Ok(index)
    }

    /// Entfernt einen Screen per ID. Rückgabe: `true` bei Erfolg.
    ///
    /// Nach dem Entfernen werden die Farben aller verbleibenden Screens
    /// neu codiert (Lücken schließen).
    pub fn remove(&mut self, id: Uuid) -> bool {
        let before = self.screens.len();
        self.screens.retain(|s| s.id != id);
        let removed = self.screens.len() != before;
        if removed {
            self.recolor_all();
        }
        removed
    }

    /// Weist allen Screens automatisch Farben aus der Palette zu (nach Index).
    pub fn recolor_all(&mut self) {
        for (i, s) in self.screens.iter_mut().enumerate() {
            s.color = screen_color_for_index(i);
        }
    }

    /// Sucht einen Screen per ID.
    pub fn find(&self, id: Uuid) -> Option<&Screen> {
        self.screens.iter().find(|s| s.id == id)
    }

    /// Mutable Variante von [`ScreenManager::find`].
    pub fn find_mut(&mut self, id: Uuid) -> Option<&mut Screen> {
        self.screens.iter_mut().find(|s| s.id == id)
    }

    /// Sucht einen Screen per Name (erster Treffer).
    pub fn find_by_name(&self, name: &str) -> Option<&Screen> {
        self.screens.iter().find(|s| s.name == name)
    }

    /// Alle Screen-Namen in Reihenfolge.
    pub fn names(&self) -> Vec<String> {
        self.screens.iter().map(|s| s.name.clone()).collect()
    }

    /// Alle Screen-IDs in Reihenfolge.
    pub fn ids(&self) -> Vec<Uuid> {
        self.screens.iter().map(|s| s.id).collect()
    }

    /// Stellt sicher, dass ein Screen mit dem Namen existiert (legt ihn
    /// mit Default-Rechteck an). Rückgabe: ID des Screens.
    pub fn ensure_named(&mut self, name: &str) -> Uuid {
        if let Some(found) = self.find_by_name(name) {
            return found.id;
        }
        let index = self.screens.len();
        let mut screen = Screen::new(name, 0, 0, 1920, 1080);
        screen.color = screen_color_for_index(index);
        let id = screen.id;
        self.screens.push(screen);
        id
    }

    /// Erzeugt einen Screen, der ein Display vollständig überdeckt.
    pub fn for_display<G: DisplayGeometry>(name: impl Into<String>, display: &G) -> Screen {
        Screen::new(
            name,
            display.display_x(),
            display.display_y(),
            display.display_width(),
            display.display_height(),
        )
    }

    /// Erzeugt Default-Screens (einer pro Display, nach Display benannt,
    /// mit automatischer Farbcodierung).
    pub fn from_displays<G: DisplayGeometry>(displays: &[G]) -> Self {
        let screens = displays
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut s = Self::for_display(d.display_name().to_string(), d);
                s.color = screen_color_for_index(i);
                s
            })
            .collect();
        Self { screens }
    }

    /// Testbild-Spezifikation für einen Screen (mit Nummer/Index).
    pub fn pattern_spec_for(&self, id: Uuid) -> Option<TestPatternSpec> {
        self.screens
            .iter()
            .position(|s| s.id == id)
            .map(|index| self.screens[index].pattern_spec_numbered(index))
    }

    /// Rechteck des virtuellen Gesamtdesktops (Bounding Box aller Displays).
    /// Wird für den Screen-Editor verwendet (Karten-Maßstab).
    pub fn bounding_box<G: DisplayGeometry>(displays: &[G]) -> (i32, i32, u32, u32) {
        if displays.is_empty() {
            return (0, 0, 1920, 1080);
        }
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for d in displays {
            min_x = min_x.min(d.display_x());
            min_y = min_y.min(d.display_y());
            max_x = max_x.max(d.display_x() + d.display_width() as i32);
            max_y = max_y.max(d.display_y() + d.display_height() as i32);
        }
        (min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32)
    }
}

impl ScreenManager {
    /// Bereich der Screen-Karte: alle Displays plus alle Screens (mit
    /// gedrehtem Fußabdruck), damit nichts über die Karte hinausragt.
    pub fn map_bounds<G: DisplayGeometry>(&self, displays: &[G]) -> (i32, i32, u32, u32) {
        let (x, y, w, h) = Self::bounding_box(displays);
        let (mut min_x, mut min_y) = (x, y);
        let (mut max_x, mut max_y) = (x + w as i32, y + h as i32);
        for s in self.screens() {
            let (fw, fh) = s.rotation.rotated_size(s.width, s.height);
            min_x = min_x.min(s.x);
            min_y = min_y.min(s.y);
            max_x = max_x.max(s.x + fw as i32);
            max_y = max_y.max(s.y + fh as i32);
        }
        (min_x, min_y, (max_x - min_x) as u32, (max_y - min_y) as u32)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_bounds_includes_rotated_screens() {
        let mut manager = ScreenManager::new();
        let mut screen = Screen::new("S", 0, 0, 1920, 1080);
        screen.rotation = ScreenRotation::Deg90;
        manager.add(screen).unwrap();
        // Keine Displays: Standardbereich 1920×1080, Screen hochkant 1080×1920.
        let none: &[TestDisplay] = &[];
        assert_eq!(manager.map_bounds(none), (0, 0, 1920, 1920));
    }

    use super::*;

    /// Test-Display (plattform-unabhängig, siehe [`DisplayGeometry`]).
    struct TestDisplay {
        name: String,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    }

    impl DisplayGeometry for TestDisplay {
        fn display_name(&self) -> &str {
            &self.name
        }
        fn display_x(&self) -> i32 {
            self.x
        }
        fn display_y(&self) -> i32 {
            self.y
        }
        fn display_width(&self) -> u32 {
            self.w
        }
        fn display_height(&self) -> u32 {
            self.h
        }
    }

    fn display(name: &str, x: i32, y: i32, w: u32, h: u32) -> TestDisplay {
        TestDisplay {
            name: name.to_string(),
            x,
            y,
            w,
            h,
        }
    }

    #[test]
    fn screen_validation() {
        assert!(Screen::new("Main", 0, 0, 1920, 1080).validate().is_ok());
        assert!(Screen::new("  ", 0, 0, 1920, 1080).validate().is_err());
        assert!(Screen::new("Main", 0, 0, 0, 1080).validate().is_err());
        assert!(Screen::new("Main", 0, 0, 1920, 0).validate().is_err());
    }

    #[test]
    fn screen_intersects_and_covers() {
        // Rechte Hälfte eines Displays.
        let s = Screen::new("Right", 960, 0, 960, 1080);
        assert!(s.intersects(0, 0, 1920, 1080), "Display überlappt");
        assert!(s.intersects(960, 0, 960, 1080), "identisch");
        assert!(!s.intersects(0, 0, 960, 1080), "linke Hälfte nicht");
        assert!(s.covers(960, 0, 960, 1080));
        assert!(!s.covers(0, 0, 1920, 1080), "nur Teil des Displays");
    }

    #[test]
    fn screen_spanning_multiple_displays() {
        // Zwei Displays nebeneinander: 1920x1080 bei (0,0) und (1920,0).
        let s = Screen::new("Wide", 960, 0, 1920, 1080);
        assert!(s.intersects(0, 0, 1920, 1080));
        assert!(s.intersects(1920, 0, 1920, 1080));
        assert!(!s.covers(0, 0, 1920, 1080), "erstes Display nur halb");
        assert!(!s.covers(1920, 0, 1920, 1080), "zweites Display nur halb");
    }

    #[test]
    fn manager_crud() {
        let mut m = ScreenManager::new();
        assert!(m.is_empty());
        let a = Screen::new("A", 0, 0, 100, 100);
        let b = Screen::new("B", 200, 0, 100, 100);
        let ia = m.add(a.clone()).unwrap();
        let ib = m.add(b.clone()).unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(ia, 0);
        assert_eq!(ib, 1);
        assert_eq!(m.find(a.id).unwrap().name, "A");
        assert_eq!(m.find_by_name("B").unwrap().id, b.id);
        assert_eq!(m.names(), vec!["A".to_string(), "B".to_string()]);
        // Ungültiger Screen wird abgelehnt.
        assert!(m.add(Screen::new("", 0, 0, 10, 10)).is_err());
        assert_eq!(m.len(), 2);
        // Entfernen.
        assert!(m.remove(a.id));
        assert!(!m.remove(a.id), "doppelt entfernen");
        assert_eq!(m.len(), 1);
        assert_eq!(m.screens()[0].name, "B");
    }

    #[test]
    fn manager_from_displays_and_bounding_box() {
        let displays = vec![
            display("Left", 0, 0, 1920, 1080),
            display("Right", 1920, 0, 1920, 1080),
        ];
        let m = ScreenManager::from_displays(&displays);
        assert_eq!(m.len(), 2);
        assert_eq!(m.screens()[1].rect(), (1920, 0, 1920, 1080));
        // Bounding Box über beide Displays.
        assert_eq!(ScreenManager::bounding_box(&displays), (0, 0, 3840, 1080));
        // Leer → Default.
        let empty: [TestDisplay; 0] = [];
        assert_eq!(ScreenManager::bounding_box(&empty), (0, 0, 1920, 1080));
    }

    #[test]
    fn ensure_named_is_idempotent() {
        let mut m = ScreenManager::new();
        let id1 = m.ensure_named("Main");
        let id2 = m.ensure_named("Main");
        assert_eq!(id1, id2);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn screen_serde_roundtrip_and_legacy() {
        let mut s = Screen::new("Main", 10, 20, 1920, 1080);
        s.test_pattern = true;
        s.rotation = ScreenRotation::Deg90;
        let json = serde_json::to_string(&s).unwrap();
        let back: Screen = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // Legacy ohne test_pattern/rotation → Defaults.
        let legacy = format!(
            r#"{{"id":"{}","name":"Old","x":0,"y":0,"width":640,"height":480}}"#,
            s.id
        );
        let old: Screen = serde_json::from_str(&legacy).unwrap();
        assert!(!old.test_pattern);
        assert_eq!(old.rotation, ScreenRotation::Deg0);
        assert_eq!(old.label(), "Old 640x480");
        assert_eq!(old.pattern_spec().label, "Old 640x480");
    }

    #[test]
    fn rotation_basics() {
        assert_eq!(ScreenRotation::Deg90.degrees(), 90);
        assert_eq!(ScreenRotation::Deg270.label(), "270°");
        assert_eq!(ScreenRotation::Deg0.next(), ScreenRotation::Deg90);
        assert_eq!(ScreenRotation::Deg270.next(), ScreenRotation::Deg0);
        // 0°/180°: Größe bleibt; 90°/270°: Breite/Höhe vertauscht.
        assert_eq!(ScreenRotation::Deg0.rotated_size(1920, 1080), (1920, 1080));
        assert_eq!(
            ScreenRotation::Deg180.rotated_size(1920, 1080),
            (1920, 1080)
        );
        assert_eq!(ScreenRotation::Deg90.rotated_size(1920, 1080), (1080, 1920));
        assert_eq!(
            ScreenRotation::Deg270.rotated_size(1920, 1080),
            (1080, 1920)
        );
        assert_eq!(ScreenRotation::ALL.len(), 4);
    }

    #[test]
    fn auto_color_coding_per_index() {
        let mut m = ScreenManager::new();
        // Drei Screens hinzufügen → automatisch verschiedene Farben.
        let a = Screen::new("A", 0, 0, 100, 100);
        let b = Screen::new("B", 100, 0, 100, 100);
        let c = Screen::new("C", 200, 0, 100, 100);
        m.add(a).unwrap();
        m.add(b).unwrap();
        m.add(c).unwrap();
        let colors: Vec<[u8; 3]> = m.screens().iter().map(|s| s.color).collect();
        assert_eq!(colors[0], screen_color_for_index(0));
        assert_eq!(colors[1], screen_color_for_index(1));
        assert_eq!(colors[2], screen_color_for_index(2));
        assert_ne!(colors[0], colors[1], "verschiedene Farben");
        assert_ne!(colors[1], colors[2], "verschiedene Farben");
    }

    #[test]
    fn recolor_after_remove_closes_gaps() {
        let mut m = ScreenManager::new();
        m.add(Screen::new("A", 0, 0, 100, 100)).unwrap();
        m.add(Screen::new("B", 100, 0, 100, 100)).unwrap();
        m.add(Screen::new("C", 200, 0, 100, 100)).unwrap();
        // B entfernen → C rückt auf Index 1 vor und bekommt dessen Farbe.
        let b_id = m.find_by_name("B").unwrap().id;
        m.remove(b_id);
        assert_eq!(m.len(), 2);
        assert_eq!(m.screens()[0].name, "A");
        assert_eq!(m.screens()[1].name, "C");
        assert_eq!(
            m.screens()[1].color,
            screen_color_for_index(1),
            "neu codiert"
        );
    }

    #[test]
    fn pattern_spec_numbered_includes_index() {
        let s = Screen::new("Main", 0, 0, 1920, 1080);
        let spec = s.pattern_spec_numbered(2);
        assert_eq!(spec.label, "#3 Main 1920x1080");
        assert_eq!(spec.index, 2);
        assert_eq!(spec.kind, TestPatternKind::Grid);
    }

    #[test]
    fn pattern_spec_for_from_manager() {
        let mut m = ScreenManager::new();
        m.add(Screen::new("A", 0, 0, 100, 100)).unwrap();
        let id_b = m.add(Screen::new("B", 100, 0, 100, 100)).unwrap();
        let id_b = m.screens()[id_b].id;
        let spec = m.pattern_spec_for(id_b).unwrap();
        assert_eq!(spec.label, "#2 B 100x100");
        assert_eq!(spec.index, 1);
        // Unbekannte ID → None.
        assert!(m.pattern_spec_for(Uuid::new_v4()).is_none());
    }

    #[test]
    fn test_pattern_kind_labels() {
        assert_eq!(TestPatternKind::Grid.label(), "Grid (Rahmen/Kreis/Raster)");
        assert_eq!(TestPatternKind::SolidColor.label(), "Solid Color");
        assert_eq!(TestPatternKind::ColorBars.label(), "Color Bars (SMPTE)");
        assert_eq!(
            TestPatternKind::LedWall.label(),
            "LED Wall (Schachbrett nummeriert)"
        );
        assert_eq!(TestPatternKind::ALL.len(), 4);
    }

    #[test]
    fn screen_serde_with_pattern_kind_and_color() {
        let mut s = Screen::new("LED", 0, 0, 128, 64);
        s.test_pattern = true;
        s.test_pattern_kind = TestPatternKind::SolidColor;
        s.color = [255, 64, 64];
        let json = serde_json::to_string(&s).unwrap();
        let back: Screen = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.test_pattern_kind, TestPatternKind::SolidColor);
        assert_eq!(back.color, [255, 64, 64]);
    }
}
