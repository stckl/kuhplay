//! Software-Compositor für Video-Stages (QLab-Pendant: Surfaces auf einer
//! Stage mit **Area of Interest** und **Hard-Edge-Blending**).
//!
//! Pro Stage wird ein Canvas (Größe aus [`Stage::geometry`]) komponiert:
//! alle laufenden Video-/Diashow-Cues der Stage als Surfaces, sortiert nach
//! Layer (niedrigste zuerst), mit:
//!
//! * **Area of Interest** ([`SurfaceGeometry::area_of_interest`]): Zuschnitt
//!   der Quelle (normalisiertes Keep-Rechteck) – zur Kompositionszeit
//!   angewendet (live editierbar, ohne Neu-Dekodierung).
//! * **Hard-Edge-Blending** ([`SurfaceGeometry::hard_edge`]): Kanten-Übergang
//!   über `width` px (linear/Cosinus) – die darunterliegende Surface scheint
//!   in der Überlappung durch.
//! * **Deckkraft** (Surface-Opacity × Fade-Opacity).
//!
//! Das Ergebnis landet im [`StageFrameStore`] – die GUI zeigt es als
//! Stage-Fenster und/oder auf einem Hardware-Display (Screen-Zuordnung).
//!
//! ## Erweiterung
//!
//! * GPU-Compositor (wgpu-Texturen, bilineare Skalierung) statt CPU-Nearest.
//! * Keying/Chroma-Keying pro Surface.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;

use crate::core::{Stage, SurfaceGeometry};

use super::frame::{StageFrameStore, VideoFrame};

/// Eine zu komponierende Surface: (Cue-ID, Layer, Geometrie, Frame, Fade-Opacity).
pub type SurfaceEntry = (Uuid, u32, SurfaceGeometry, Option<Arc<VideoFrame>>, f32);

/// Default-Canvas-Größe, wenn eine Stage nicht konfiguriert wurde.
pub const DEFAULT_CANVAS: (u32, u32) = (1920, 1080);

/// Canvas-Größe einer Stage (konfiguriert oder Default).
pub fn canvas_size(stages: &HashMap<String, Stage>, stage_name: &str) -> (u32, u32) {
    stages
        .get(stage_name)
        .map(|s| {
            (
                s.geometry.width.max(1.0) as u32,
                s.geometry.height.max(1.0) as u32,
            )
        })
        .unwrap_or(DEFAULT_CANVAS)
}

/// Software-Compositor mit Dirty-Tracking (Ptr-Vergleich der Quell-Frames).
pub struct Compositor {
    /// Zuletzt komponiertes Frame pro Cue (Ptr-Vergleich → Dirty-Erkennung).
    last_frames: HashMap<Uuid, Arc<VideoFrame>>,
    /// Zuletzt verwendete Deckkraft pro Cue.
    last_opacities: HashMap<Uuid, f32>,
    /// Stages, die seit dem letzten Tick neu komponiert werden müssen.
    dirty: HashSet<String>,
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compositor {
    pub fn new() -> Self {
        Self {
            last_frames: HashMap::new(),
            last_opacities: HashMap::new(),
            dirty: HashSet::new(),
        }
    }

    /// Markiert eine Stage als „neu komponieren".
    pub fn mark_dirty(&mut self, stage: &str) {
        self.dirty.insert(stage.to_string());
    }

    /// Nimmt die Dirty-Menge (leert sie).
    pub fn take_dirty(&mut self) -> Vec<String> {
        self.dirty.drain().collect()
    }

    /// Aktualisiert das Dirty-Tracking nach einer Komposition.
    pub fn note_composited(
        &mut self,
        stage_name: &str,
        surfaces: &[SurfaceEntry],
        frames: &HashMap<Uuid, Arc<VideoFrame>>,
        opacities: &HashMap<Uuid, f32>,
    ) {
        for (cue_id, _, _, _, _) in surfaces {
            if let Some(frame) = frames.get(cue_id) {
                self.last_frames.insert(*cue_id, frame.clone());
            } else {
                self.last_frames.remove(cue_id);
            }
            let opacity = opacities.get(cue_id).copied().unwrap_or(1.0);
            self.last_opacities.insert(*cue_id, opacity);
        }
        // Verwaiste Einträge aufräumen.
        let live: HashSet<Uuid> = surfaces.iter().map(|(id, _, _, _, _)| *id).collect();
        self.last_frames.retain(|id, _| live.contains(id));
        self.last_opacities.retain(|id, _| live.contains(id));
        let _ = stage_name;
    }

    /// Prüft, ob sich ein Cue-Frame oder eine Opacity seit der letzten
    /// Komposition geändert hat → Stage dirty.
    pub fn is_dirty(&self, cue_id: Uuid, frame: Option<&Arc<VideoFrame>>, opacity: f32) -> bool {
        let frame_changed = match (frame, self.last_frames.get(&cue_id)) {
            (Some(frame), Some(last)) => !Arc::ptr_eq(last, frame),
            (Some(_), None) => true,
            (None, Some(_)) => true,
            (None, None) => false,
        };
        let opacity_changed = self.last_opacities.get(&cue_id) != Some(&opacity);
        frame_changed || opacity_changed
    }

    /// Kompiliert das Canvas einer Stage und legt es im Store ab.
    ///
    /// (Das Route-Testbild gehoert zum Route-Renderer, nicht mehr zum
    /// Stage-Compositor - siehe route_renderer::render_route.)
    pub fn composite(
        &self,
        stage_name: &str,
        canvas: (u32, u32),
        surfaces: &[SurfaceEntry],
        stage_frames: &StageFrameStore,
    ) {
        let (cw, ch) = (canvas.0.max(1), canvas.1.max(1));
        let mut out = vec![0u8; (cw as usize) * (ch as usize) * 4];
        // Hintergrund: undurchsichtiges Schwarz.
        for px in out.chunks_exact_mut(4) {
            px[3] = 255;
        }

        // Layer aufsteigend (niedrigste zuerst).
        let mut ordered: Vec<&SurfaceEntry> = surfaces.iter().collect();
        ordered.sort_by_key(|(_, layer, _, _, _)| *layer);

        for (_cue_id, _layer, surface, frame, fade_opacity) in ordered {
            if !surface.visible {
                continue;
            }
            let frame = match frame {
                Some(frame) => frame,
                None => continue,
            };
            let alpha_base =
                (surface.opacity.clamp(0.0, 1.0) * fade_opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            if alpha_base <= 0.0 {
                continue;
            }
            // Lage auf der Stage (Fit, Fill, Stretch oder eigene Position/Größe).
            let ((x, y, width, height), (sx0, sy0, sx1, sy1)) =
                surface.placement((frame.width, frame.height), (cw, ch));
            // Dest-Rechteck auf die Canvas clampen.
            let dx0 = x.max(0.0) as u32;
            let dy0 = y.max(0.0) as u32;
            let dx1 = ((x + width).ceil() as u32).min(cw);
            let dy1 = ((y + height).ceil() as u32).min(ch);
            if dx1 <= dx0 || dy1 <= dy0 {
                continue;
            }
            let src_w = (sx1 - sx0).max(1) as usize;
            let src_h = (sy1 - sy0).max(1) as usize;
            let dest_w = (dx1 - dx0) as usize;
            let dest_h = (dy1 - dy0) as usize;
            let src_stride = frame.width as usize;

            for dy in 0..dest_h {
                let py = (dy0 + dy as u32) as f32 + 0.5;
                // Quell-Y (Nearest-Neighbor innerhalb des AoI).
                let sy = (sy0 as usize
                    + ((dy as f32 + 0.5) / dest_h as f32 * src_h as f32) as usize)
                    .min((sy1 - 1) as usize);
                for dx in 0..dest_w {
                    let px = (dx0 + dx as u32) as f32 + 0.5;
                    // Hard-Edge-Faktor an dieser Canvas-Position.
                    let edge = surface.hard_edge_factor(px, py);
                    if edge <= 0.0 {
                        continue;
                    }
                    let a = alpha_base * edge;
                    let sx = (sx0 as usize
                        + ((dx as f32 + 0.5) / dest_w as f32 * src_w as f32) as usize)
                        .min((sx1 - 1) as usize);
                    let src = &frame.rgba[(sy * src_stride + sx) * 4..][..4];
                    let dst = &mut out
                        [((dy0 as usize + dy) * cw as usize + dx0 as usize + dx) * 4..][..4];
                    if a >= 1.0 {
                        // Deckend: direkter Copy.
                        dst.copy_from_slice(src);
                    } else {
                        // Source-Over-Blending.
                        for c in 0..3 {
                            let s = src[c] as f32;
                            let d = dst[c] as f32;
                            dst[c] = (s * a + d * (1.0 - a)).clamp(0.0, 255.0) as u8;
                        }
                        dst[3] = dst[3].max((src[3] as f32 * a).clamp(0.0, 255.0) as u8);
                    }
                }
            }
        }

        let canvas_frame = Arc::new(VideoFrame::new(cw, ch, out));
        stage_frames
            .lock()
            .unwrap()
            .insert(stage_name.to_string(), canvas_frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BlendCurve, Crop, EdgeSide, GeometryMode, HardEdge};
    use std::collections::HashMap;

    #[test]
    fn composite_places_surface_full_canvas() {
        let red = Arc::new(VideoFrame::new(4, 4, [255, 0, 0, 255].repeat(4 * 4)));
        let surface = SurfaceGeometry {
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 8.0,
            area_of_interest: None,
            hard_edge: None,
            opacity: 1.0,
            visible: true,
            mode: GeometryMode::Custom,
        };
        let surfaces = vec![(Uuid::new_v4(), 0, surface, Some(red), 1.0)];
        let store = super::super::frame::new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("Stage 1", (8, 8), &surfaces, &store);
        let canvas = store.lock().unwrap().get("Stage 1").cloned().unwrap();
        assert_eq!(canvas.width, 8);
        assert_eq!(canvas.height, 8);
        // Mitte = skaliertes Rot.
        let px = &canvas.rgba[(4 * 8 + 4) * 4..][..4];
        assert_eq!(px, &[255, 0, 0, 255]);
    }

    #[test]
    fn composite_applies_area_of_interest_crop() {
        // 4×4 Quelle: linke Hälfte rot, rechte Hälfte grün.
        let mut rgba = Vec::new();
        for _y in 0..4 {
            for x in 0..4 {
                if x < 2 {
                    rgba.extend_from_slice(&[255, 0, 0, 255]);
                } else {
                    rgba.extend_from_slice(&[0, 255, 0, 255]);
                }
            }
        }
        let frame = Arc::new(VideoFrame::new(4, 4, rgba));
        // AoI: nur rechte Hälfte (grün) der Quelle.
        let surface = SurfaceGeometry {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            area_of_interest: Some(Crop {
                left: 0.5,
                top: 0.0,
                right: 1.0,
                bottom: 1.0,
            }),
            hard_edge: None,
            opacity: 1.0,
            visible: true,
            mode: GeometryMode::Custom,
        };
        let surfaces = vec![(Uuid::new_v4(), 0, surface, Some(frame), 1.0)];
        let store = super::super::frame::new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("S", (4, 4), &surfaces, &store);
        let canvas = store.lock().unwrap().get("S").cloned().unwrap();
        // Jedes Pixel muss grün sein (nur rechte Quellenhälfte landet).
        for px in canvas.rgba.chunks_exact(4) {
            assert_eq!(px, &[0, 255, 0, 255], "AoI crop must show only green half");
        }
    }

    #[test]
    fn composite_hard_edge_blends_overlap() {
        // Zwei Surfaces überlappen: links blau (Layer 0), rechts rot (Layer 1)
        // mit Hard-Edge an der linken Kante (Breite = gesamte Überlappung).
        let blue = Arc::new(VideoFrame::new(4, 4, [0, 0, 255, 255].repeat(4 * 4)));
        let red = Arc::new(VideoFrame::new(4, 4, [255, 0, 0, 255].repeat(4 * 4)));
        let left = SurfaceGeometry {
            x: 0.0,
            y: 0.0,
            width: 6.0,
            height: 4.0,
            area_of_interest: None,
            hard_edge: None,
            opacity: 1.0,
            visible: true,
            mode: GeometryMode::Custom,
        };
        let right = SurfaceGeometry {
            x: 4.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            area_of_interest: None,
            hard_edge: Some(HardEdge::new(EdgeSide::Left, 2.0, BlendCurve::Linear)),
            opacity: 1.0,
            visible: true,
            mode: GeometryMode::Custom,
        };
        let surfaces = vec![
            (Uuid::new_v4(), 0, left, Some(blue), 1.0),
            (Uuid::new_v4(), 1, right, Some(red), 1.0),
        ];
        let store = super::super::frame::new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("S", (8, 4), &surfaces, &store);
        let canvas = store.lock().unwrap().get("S").cloned().unwrap();
        let px_at = |x: u32| &canvas.rgba[((2 * 8) + x) as usize * 4..][..4];
        // x=0..3: blau (nur linke Surface).
        assert_eq!(px_at(0), &[0, 0, 255, 255]);
        // Pixel-Center-Sampling: Pixel x=4 liegt mit seinem Mittelpunkt bei
        // 4.5 → 0.5 px inside the 2 px blend → factor 0.25 (linear).
        let edge_px = px_at(4);
        assert_eq!(edge_px, &[63, 0, 191, 255], "x=4: alpha 0.25 über blau");
        // x=5: Mittelpunkt 5.5 → 1.5 px innen → factor 0.75.
        let mid = px_at(5);
        assert_eq!(mid, &[191, 0, 63, 255], "x=5: alpha 0.75 über blau");
        // Ab x=6 (2.5 px ≥ width): voll deckend rot.
        assert_eq!(px_at(6), &[255, 0, 0, 255]);
        assert_eq!(px_at(7), &[255, 0, 0, 255]);
        // Monotonie: Rot-Anteil steigt über die Blende.
        assert!(edge_px[0] < mid[0]);
    }

    #[test]
    fn composite_respects_layer_order_and_visibility() {
        let blue = Arc::new(VideoFrame::new(2, 2, [0, 0, 255, 255].repeat(2 * 2)));
        let red = Arc::new(VideoFrame::new(2, 2, [255, 0, 0, 255].repeat(2 * 2)));
        let mut hidden = SurfaceGeometry::full_canvas();
        hidden.width = 4.0;
        hidden.height = 4.0;
        hidden.visible = false;
        let visible = SurfaceGeometry {
            width: 4.0,
            height: 4.0,
            ..SurfaceGeometry::full_canvas()
        };
        // Rot auf Layer 5 (oben), blau auf Layer 1 – rot gewinnt.
        let surfaces = vec![
            (Uuid::new_v4(), 1, visible, Some(blue), 1.0),
            (Uuid::new_v4(), 5, visible, Some(red.clone()), 1.0),
        ];
        let store = super::super::frame::new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("S", (4, 4), &surfaces, &store);
        let canvas = store.lock().unwrap().get("S").cloned().unwrap();
        assert_eq!(
            &canvas.rgba[..4],
            &[255, 0, 0, 255],
            "höherer Layer gewinnt"
        );

        // Unsichtbare Surface wird nicht komponiert.
        let surfaces = vec![(Uuid::new_v4(), 0, hidden, Some(red.clone()), 1.0)];
        let store2 = super::super::frame::new_stage_frame_store();
        comp.composite("S2", (4, 4), &surfaces, &store2);
        let canvas2 = store2.lock().unwrap().get("S2").cloned().unwrap();
        assert_eq!(
            &canvas2.rgba[..4],
            &[0, 0, 0, 255],
            "unsichtbar = schwarzer Canvas"
        );
    }

    #[test]
    fn composite_opacity_blends() {
        let white = Arc::new(VideoFrame::new(2, 2, [255, 255, 255, 255].repeat(2 * 2)));
        let surface = SurfaceGeometry {
            width: 2.0,
            height: 2.0,
            opacity: 0.5,
            ..SurfaceGeometry::full_canvas()
        };
        let surfaces = vec![(Uuid::new_v4(), 0, surface, Some(white), 1.0)];
        let store = super::super::frame::new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("S", (2, 2), &surfaces, &store);
        let canvas = store.lock().unwrap().get("S").cloned().unwrap();
        // 50 % Weiß über Schwarz = Grau.
        let px = &canvas.rgba[..4];
        assert!(
            (px[0] as i32 - 128).abs() <= 1,
            "Deckkraft 0.5 → Grau: {px:?}"
        );
    }

    #[test]
    fn dirty_tracking_detects_frame_and_opacity_changes() {
        let mut comp = Compositor::new();
        let id = Uuid::new_v4();
        let f1 = Arc::new(VideoFrame::black(2, 2));
        let f2 = Arc::new(VideoFrame::new(2, 2, vec![255; 16]));
        let opacities = HashMap::new();
        // Erstes Mal = dirty.
        assert!(comp.is_dirty(id, Some(&f1), 1.0));
        let mut frames = HashMap::new();
        frames.insert(id, f1.clone());
        comp.note_composited(
            "S",
            &[(id, 0, SurfaceGeometry::full_canvas(), Some(f1.clone()), 1.0)],
            &frames,
            &opacities,
        );
        // Gleiches Frame + Opacity = nicht dirty.
        assert!(!comp.is_dirty(id, Some(&f1), 1.0));
        // Neues Frame (anderer Ptr) = dirty.
        assert!(comp.is_dirty(id, Some(&f2), 1.0));
        // Opacity geändert = dirty.
        assert!(comp.is_dirty(id, Some(&f1), 0.5));
    }
}
