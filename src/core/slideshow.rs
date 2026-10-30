//! Diashow-Cue (`SlideShowCue`): Fotoliste mit Hold-Zeiten und Crossfades.
//!
//! Eine [`SlideShow`] ist die datenorientierte Definition eines
//! Foto-Cues: Liste von Bildern mit individuellen Anzeige-Dauern
//! (Hold Time), Crossfade-Zeit und Auto-Advance-Flag. Die Engine verwaltet
//! den *Laufzeit*-Zustand (aktueller Slide-Index, nächste Wechselzeit).
//!
//! ## Semantik
//!
//! * **Auto-Advance:** Die Engine schaltet nach jeder Hold-Zeit selbständig
//!   weiter und endet nach der letzten Slide (Gesamtdauer = Summe der
//!   Holds; Crossfades überlappen visuell, verlängern aber nicht).
//! * **Manuell:** Ohne Auto-Advance läuft die Show bis Stop/Devamp bzw.
//!   bis zur expliziten Cue-Duration; geschaltet wird über die
//!   Control-Aktionen [`crate::core::ControlAction::NextSlide`] /
//!   [`crate::core::ControlAction::PrevSlide`].
//! * **Loop:** `loop_slides` wrappt nach der letzten zurück zur ersten.
//!
//! ## Erweiterung
//!
//! * **Preloading:** Bilder-Preloading in der produktiven Engine (kein
//!   Ruckeln beim Wechsel).
//! * **Ken-Burns/Transitions:** Animierte Übergänge als eigene Transition-Typen.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Ein einzelnes Dia: Bildpfad + Anzeige-Dauer (Hold Time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slide {
    /// Bilddatei (PNG/JPG/WebP).
    pub path: PathBuf,
    /// Anzeige-Dauer dieses Dias.
    #[serde(default, with = "crate::core::duration_ms")]
    pub hold: Duration,
}

impl Slide {
    /// Neue Slide. `hold` muss > 0 sein.
    pub fn new(path: impl Into<PathBuf>, hold: Duration) -> ShowResult<Self> {
        let slide = Self {
            path: path.into(),
            hold,
        };
        slide.validate()?;
        Ok(slide)
    }

    /// Prüft die Slide (nicht-leerer Pfad, positive Hold-Zeit).
    pub fn validate(&self) -> ShowResult<()> {
        if self.path.as_os_str().is_empty() {
            return Err(ShowError::InvalidParameter(
                "slide: empty image path".to_string(),
            ));
        }
        if self.hold.is_zero() {
            return Err(ShowError::InvalidParameter(format!(
                "slide {}: hold time must be > 0",
                self.path.display()
            )));
        }
        Ok(())
    }
}

/// Definition eines Diashow-Cues ([`crate::core::CueType::SlideShow`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SlideShow {
    /// Die Dias in Abspielreihenfolge.
    #[serde(default)]
    pub slides: Vec<Slide>,
    /// Crossfade-Dauer zwischen zwei Dias (visuelle Überlappung).
    #[serde(default, with = "crate::core::duration_ms")]
    pub crossfade: Duration,
    /// `true`: Engine schaltet selbständig weiter (Auto-Advance).
    #[serde(default = "default_true")]
    pub auto_advance: bool,
    /// `true`: nach der letzten Slide zurück zur ersten (Loop).
    #[serde(default)]
    pub loop_slides: bool,
}

fn default_true() -> bool {
    true
}

impl SlideShow {
    /// Neue Diashow. `slides` darf nicht leer sein.
    pub fn new(slides: Vec<Slide>, crossfade: Duration, auto_advance: bool) -> ShowResult<Self> {
        let show = Self {
            slides,
            crossfade,
            auto_advance,
            loop_slides: false,
        };
        show.validate()?;
        Ok(show)
    }

    /// Prüft die gesamte Diashow (nicht leer, alle Slides gültig).
    pub fn validate(&self) -> ShowResult<()> {
        if self.slides.is_empty() {
            return Err(ShowError::InvalidParameter(
                "slideshow: at least one slide is required".to_string(),
            ));
        }
        for slide in &self.slides {
            slide.validate()?;
        }
        Ok(())
    }

    /// Anzahl der Dias.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// Gesamtdauer bei Auto-Advance (Summe der Holds; Crossfades überlappen).
    pub fn total_duration(&self) -> Duration {
        self.slides.iter().map(|s| s.hold).sum()
    }

    /// Nächster Slide-Index. `None` am Ende (ohne Loop) – sonst wrap/clamp.
    pub fn next_index(&self, current: usize) -> Option<usize> {
        let next = current + 1;
        if next < self.slides.len() {
            Some(next)
        } else if self.loop_slides {
            Some(0)
        } else {
            None
        }
    }

    /// Vorheriger Slide-Index (geclampt auf 0).
    pub fn prev_index(&self, current: usize) -> usize {
        current.saturating_sub(1)
    }

    /// Hold-Zeit eines Slides (Index außerhalb des Bereichs → `Duration::ZERO`).
    pub fn hold_at(&self, index: usize) -> Duration {
        self.slides.get(index).map(|s| s.hold).unwrap_or_default()
    }
}

/// Manuelle Schaltung einer Diashow (Control-Cues `NextSlide`/`PrevSlide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlideNav {
    /// Nächstes Dia (am Ende ohne Loop: bleibt auf dem letzten Dia).
    Next,
    /// Vorheriges Dia (am Anfang: bleibt auf dem ersten Dia).
    Prev,
}

impl SlideNav {
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            SlideNav::Next => "Next Slide",
            SlideNav::Prev => "Prev Slide",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(name: &str, hold_ms: u64) -> Slide {
        Slide::new(
            format!("/slides/{name}.jpg"),
            Duration::from_millis(hold_ms),
        )
        .unwrap()
    }

    #[test]
    fn slide_requires_positive_hold_and_path() {
        assert!(Slide::new("/a.jpg", Duration::ZERO).is_err());
        assert!(Slide::new("", Duration::from_secs(1)).is_err());
        assert!(Slide::new("/a.jpg", Duration::from_secs(1)).is_ok());
    }

    #[test]
    fn slideshow_requires_at_least_one_slide() {
        assert!(SlideShow::new(vec![], Duration::ZERO, true).is_err());
        assert!(SlideShow::new(vec![slide("a", 1000)], Duration::ZERO, true).is_ok());
    }

    #[test]
    fn total_duration_sums_holds() {
        let show = SlideShow::new(
            vec![slide("a", 1000), slide("b", 2500), slide("c", 500)],
            Duration::from_millis(300),
            true,
        )
        .unwrap();
        assert_eq!(show.total_duration(), Duration::from_millis(4000));
        assert_eq!(show.slide_count(), 3);
    }

    #[test]
    fn next_index_advances_wraps_and_stops() {
        let mut show =
            SlideShow::new(vec![slide("a", 100), slide("b", 100)], Duration::ZERO, true).unwrap();
        assert_eq!(show.next_index(0), Some(1));
        assert_eq!(show.next_index(1), None, "no loop → end of show");
        show.loop_slides = true;
        assert_eq!(show.next_index(1), Some(0), "loop → wrap to first");
    }

    #[test]
    fn prev_index_clamps_at_zero() {
        let show = SlideShow::new(
            vec![slide("a", 100), slide("b", 100)],
            Duration::ZERO,
            false,
        )
        .unwrap();
        assert_eq!(show.prev_index(1), 0);
        assert_eq!(show.prev_index(0), 0);
        assert_eq!(
            show.prev_index(99),
            98,
            "clamp via saturating_sub on given index"
        );
    }

    #[test]
    fn serde_roundtrip_preserves_slides_and_flags() {
        let mut show = SlideShow::new(
            vec![slide("a", 1000), slide("b", 2000)],
            Duration::from_millis(250),
            true,
        )
        .unwrap();
        show.loop_slides = true;
        let json = serde_json::to_string(&show).expect("serialize");
        let back: SlideShow = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(show, back);
        assert!(back.auto_advance);
        assert!(back.loop_slides);
        assert_eq!(back.crossfade, Duration::from_millis(250));
    }
}
