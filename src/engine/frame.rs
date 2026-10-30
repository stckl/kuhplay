//! Video-Frames: RGBA-Puffer, die die Engine für die GUI (Stage-Fenster)
//! bereitstellt.
//!
//! Die Real-Engine dekodiert Video (ffmpeg) und Diashows (`image`) in
//! RGBA-Frames und legt den jeweils neuesten Frame hier ab. Die GUI liest
//! den Store **direkt** (shared `Arc<Mutex<…>>`) – so gibt es keine
//! Kanal-Spam bei 25/30 fps, und der UI-Thread blockiert nie (ein Lock,
//! eine `Arc`-Kopie).
//!
//! Die GUI wandelt [`VideoFrame`] per `egui::ColorImage::from_rgba_unmultiplied`
//! in eine Textur um (`Context::load_texture` mit stabilem Namen aktualisiert
//! die Textur in-place).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use uuid::Uuid;

/// Ein dekodiertes RGBA-Frame (Row-Major, 4 Bytes/Pixel, Alpha = 255).
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Breite in Pixeln.
    pub width: u32,
    /// Höhe in Pixeln.
    pub height: u32,
    /// RGBA-Puffer (`width * height * 4` Bytes).
    pub rgba: Vec<u8>,
}

impl VideoFrame {
    /// Erzeugt ein Frame (asserted die Pufferlänge – Programmierfehler früh).
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            rgba,
        }
    }

    /// Schwarzes Frame (Fallback, wenn noch kein Frame dekodiert wurde).
    pub fn black(width: u32, height: u32) -> Self {
        Self::new(width, height, vec![0u8; (width * height * 4) as usize])
    }

    /// Blendet zwei Frames linear (`t` = 0.0 → `a`, 1.0 → `b`).
    /// Für Diashow-Crossfades. Gleiche Größe vorausgesetzt.
    pub fn blend(a: &VideoFrame, b: &VideoFrame, t: f32) -> VideoFrame {
        let t = t.clamp(0.0, 1.0);
        let len = a.rgba.len().min(b.rgba.len());
        let mut out = vec![0u8; len];
        for ((out, &av), &bv) in out.iter_mut().zip(&a.rgba).zip(&b.rgba) {
            let (av, bv) = (av as f32, bv as f32);
            *out = (av + (bv - av) * t).clamp(0.0, 255.0) as u8;
        }
        VideoFrame {
            width: a.width,
            height: a.height,
            rgba: out,
        }
    }
}

/// Geteilter Frame-Store: Cue-ID → neuester Frame.
///
/// Wird zwischen Engine-Thread (schreibend) und GUI (lesend) geteilt.
pub type FrameStore = Arc<Mutex<HashMap<Uuid, Arc<VideoFrame>>>>;

/// Erzeugt einen leeren Frame-Store.
pub fn new_frame_store() -> FrameStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Deckkraft (Opacity) pro Cue-ID – wird von Fade-Cues (Opacity) aktualisiert.
pub type OpacityStore = Arc<Mutex<HashMap<Uuid, f32>>>;

/// Erzeugt einen leeren Opacity-Store.
pub fn new_opacity_store() -> OpacityStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Kompositioniertes Stage-Canvas pro Stage-Name (Screens/Compositor).
pub type StageFrameStore = Arc<Mutex<HashMap<String, Arc<VideoFrame>>>>;

/// Erzeugt einen leeren Stage-Frame-Store.
pub fn new_stage_frame_store() -> StageFrameStore {
    Arc::new(Mutex::new(HashMap::new()))
}
