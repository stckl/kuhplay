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
    /// Achtung: RGBA 0,0,0,0 = transparent schwarz (Alpha 0).
    pub fn black(width: u32, height: u32) -> Self {
        Self::new(width, height, vec![0u8; (width * height * 4) as usize])
    }

    /// Transparenter Frame (Alpha 0) – Key/Fill-Key-Canvas.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, vec![0u8; (width * height * 4) as usize])
    }

    /// Blendet zwei Frames linear (`t` = 0.0 → `a`, 1.0 → `b`).
    /// Für Diashow-Crossfades. Unterschiedlich große Frames werden vorher
    /// seitentreu mittig auf eine gemeinsame Fläche gelegt.
    pub fn blend(a: &VideoFrame, b: &VideoFrame, t: f32) -> VideoFrame {
        if (a.width, a.height) != (b.width, b.height) {
            let (w, h) = (a.width.max(b.width), a.height.max(b.height));
            return Self::blend(&a.fit_into(w, h), &b.fit_into(w, h), t);
        }
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

    /// Skaliert seitentreu und mittig auf `width`×`height` (Rand transparent).
    fn fit_into(&self, width: u32, height: u32) -> VideoFrame {
        let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
        let scale = (width as f32 / self.width.max(1) as f32)
            .min(height as f32 / self.height.max(1) as f32);
        let w = ((self.width as f32 * scale).round() as u32).clamp(1, width);
        let h = ((self.height as f32 * scale).round() as u32).clamp(1, height);
        let (x0, y0) = ((width - w) / 2, (height - h) / 2);
        for y in 0..h {
            let sy = (y as u64 * self.height as u64 / h as u64) as usize;
            for x in 0..w {
                let sx = (x as u64 * self.width as u64 / w as u64) as usize;
                let src = (sy * self.width as usize + sx) * 4;
                let dst = ((y0 + y) as usize * width as usize + (x0 + x) as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.rgba[src..src + 4]);
            }
        }
        VideoFrame::new(width, height, out)
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

/// Kompositioniertes Stage-Canvas pro Stage-Name (Compositor).
pub type StageFrameStore = Arc<Mutex<HashMap<String, Arc<VideoFrame>>>>;

/// Erzeugt einen leeren Stage-Frame-Store.
pub fn new_stage_frame_store() -> StageFrameStore {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Gerendertes Route-Bild pro Route-ID (Video-Routing, Route-Renderer).
pub type RouteFrameStore = Arc<Mutex<HashMap<Uuid, Arc<VideoFrame>>>>;

/// Erzeugt einen leeren Route-Frame-Store.
pub fn new_route_frame_store() -> RouteFrameStore {
    Arc::new(Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod blend_size_tests {
    use super::*;

    #[test]
    fn blend_of_different_sizes_yields_a_complete_frame() {
        // Wie in der Diashow: ein großes und ein kleines Bild.
        let big = VideoFrame::new(8, 4, [255, 0, 0, 255].repeat(32));
        let small = VideoFrame::new(2, 2, [0, 0, 255, 255].repeat(4));
        for (a, b) in [(&big, &small), (&small, &big)] {
            let out = VideoFrame::blend(a, b, 0.5);
            assert_eq!((out.width, out.height), (8, 4));
            assert_eq!(out.rgba.len(), 8 * 4 * 4, "Puffer muss zur Größe passen");
        }
    }

    #[test]
    fn composite_skips_a_frame_with_wrong_buffer_length() {
        let bad = std::sync::Arc::new(VideoFrame {
            width: 8,
            height: 8,
            rgba: vec![255; 16],
        });
        let surface = crate::core::SurfaceGeometry::fit();
        let entries = vec![(uuid::Uuid::new_v4(), 1, surface, Some(bad), 1.0)];
        let out = crate::engine::compositor::composite_canvas((8, 8), &entries);
        assert!(
            out.iter().all(|&v| v == 0),
            "kaputter Frame bleibt unsichtbar"
        );
    }
}
