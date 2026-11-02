//! Software-Compositor für Video-Stages (Surfaces auf einer
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
    /// Zuletzt komponiertes Frame pro Cue, pro Stage (sonst löscht die
    /// Aufräumlogik einer Stage die Einträge der anderen → die gelten dann
    /// immer als geändert, Review E13).
    last_frames: HashMap<String, HashMap<Uuid, Arc<VideoFrame>>>,
    /// Zuletzt verwendete Deckkraft pro Cue, pro Stage.
    last_opacities: HashMap<String, HashMap<Uuid, f32>>,
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
        let frames_of_stage = self.last_frames.entry(stage_name.to_string()).or_default();
        let opacities_of_stage = self
            .last_opacities
            .entry(stage_name.to_string())
            .or_default();
        for (cue_id, _, _, _, _) in surfaces {
            if let Some(frame) = frames.get(cue_id) {
                frames_of_stage.insert(*cue_id, frame.clone());
            } else {
                frames_of_stage.remove(cue_id);
            }
            let opacity = opacities.get(cue_id).copied().unwrap_or(1.0);
            opacities_of_stage.insert(*cue_id, opacity);
        }
        // Verwaiste Einträge dieser Stage aufräumen (andere Stages bleiben unangetastet).
        let live: HashSet<Uuid> = surfaces.iter().map(|(id, _, _, _, _)| *id).collect();
        frames_of_stage.retain(|id, _| live.contains(id));
        opacities_of_stage.retain(|id, _| live.contains(id));
    }

    /// Prüft, ob sich ein Cue-Frame oder eine Opacity seit der letzten
    /// Komposition geändert hat → Stage dirty.
    pub fn is_dirty(
        &self,
        stage: &str,
        cue_id: Uuid,
        frame: Option<&Arc<VideoFrame>>,
        opacity: f32,
    ) -> bool {
        let frames_of_stage = self.last_frames.get(stage);
        let frame_changed = match (frame, frames_of_stage.and_then(|m| m.get(&cue_id))) {
            (Some(frame), Some(last)) => !Arc::ptr_eq(last, frame),
            (Some(_), None) => true,
            (None, Some(_)) => true,
            (None, None) => false,
        };
        let opacity_changed =
            self.last_opacities.get(stage).and_then(|m| m.get(&cue_id)) != Some(&opacity);
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
        let out = composite_canvas(canvas, surfaces);
        let (cw, ch) = (canvas.0.max(1), canvas.1.max(1));
        let canvas_frame = Arc::new(VideoFrame::new(cw, ch, out));
        stage_frames
            .lock()
            .unwrap()
            .insert(stage_name.to_string(), canvas_frame);
    }
}

/// Reine Compositing-Funktion: rendert ein Stage-Canvas (transparenter
/// Hintergrund, echtes Over-Alpha mit Fast Paths) als RGBA-Puffer.
/// Thread-sicher/parallel aufrufbar (keine &self-Abhängigkeit).
pub fn composite_canvas(canvas: (u32, u32), surfaces: &[SurfaceEntry]) -> Vec<u8> {
    {
        let (cw, ch) = (canvas.0.max(1), canvas.1.max(1));
        let mut out = vec![0u8; (cw as usize) * (ch as usize) * 4];
        // Hintergrund: transparent (Alpha 0) – Key/Fill braucht echte Alpha-
        // Werte; Fill-Routen flatten später auf Schwarz (render_region_into).
        // (vec![0u8; …] ist bereits RGBA 0,0,0,0 = transparent.)

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
            // Ein Frame mit falscher Pufferlänge darf die Engine nicht abstürzen lassen.
            if frame.rgba.len() != frame.width as usize * frame.height as usize * 4 {
                tracing::error!(
                    width = frame.width,
                    height = frame.height,
                    len = frame.rgba.len(),
                    "frame buffer does not match its size, skipped"
                );
                continue;
            }
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
            // Quell-Fenster an das sichtbare (geclampte) Dest-Rechteck koppeln:
            // Abgeschnittene Surface-Teile fallen aus der Quelle heraus (Crop),
            // statt dass das gesamte Bild auf den sichtbaren Teil gestaucht wird
            // (Review C9).
            let fx0 = ((dx0 as f32 - x) / width.max(1.0)).clamp(0.0, 1.0);
            let fy0 = ((dy0 as f32 - y) / height.max(1.0)).clamp(0.0, 1.0);
            let fx1 = ((dx1 as f32 - x) / width.max(1.0)).clamp(0.0, 1.0);
            let fy1 = ((dy1 as f32 - y) / height.max(1.0)).clamp(0.0, 1.0);
            let src_x0 = sx0 as f32 + fx0 * (sx1 - sx0) as f32;
            let src_y0 = sy0 as f32 + fy0 * (sy1 - sy0) as f32;
            let src_x1 = sx0 as f32 + fx1 * (sx1 - sx0) as f32;
            let src_y1 = sy0 as f32 + fy1 * (sy1 - sy0) as f32;
            let src_w = (src_x1 - src_x0).max(1.0) as usize;
            let src_h = (src_y1 - src_y0).max(1.0) as usize;
            let dest_w = (dx1 - dx0) as usize;
            let dest_h = (dy1 - dy0) as usize;
            let src_stride = frame.width as usize;
            // Obere Sampling-Grenzen (AoI-Ende bzw. Frame-Ende).
            let src_x_cap = ((src_x1.ceil() as usize).max(src_x0 as usize + 1) - 1)
                .min(frame.width as usize - 1);
            let src_y_cap = ((src_y1.ceil() as usize).max(src_y0 as usize + 1) - 1)
                .min(frame.height as usize - 1);

            // Fast Path: Quelle deckt die gesamte Canvas 1:1 und ist voll
            // deckend → kompletten Puffer per memcpy (kein Per-Pixel-Loop).
            // Hard Edge darf hier nicht gesetzt sein – sonst gingen rechte/
            // untere Kanten im Fast Path verloren (Review C10).
            if alpha_base >= 1.0
                && surface.blend_mode == crate::core::BlendMode::SourceOver
                && dx0 == 0
                && dy0 == 0
                && dest_w == cw as usize
                && dest_h == ch as usize
                && src_w == frame.width as usize
                && src_h == frame.height as usize
                && frame.rgba.len() == out.len()
                && src_x0 == 0.0
                && src_y0 == 0.0
                && surface.hard_edge.is_none()
            {
                // noch prüfen, ob die Quelle überall alpha=255 → dann copy,
                // sonst pro Pixel (Alpha < 255 → Over-Pfad unten greift eh).
                if frame.rgba.chunks_exact(4).all(|px| px[3] == 255) {
                    out.copy_from_slice(&frame.rgba);
                    continue;
                }
            }

            // Hard-Edge-Faktor: ohne gesetzte Kante überall 1.0 → pro Pixel sparen.
            let has_edge = surface.hard_edge.is_some();
            // Schrittweiten einmal pro Surface (statt Division pro Pixel).
            let step_x = src_w as f32 / dest_w as f32;
            let step_y = src_h as f32 / dest_h as f32;
            let mut src_y_f = 0.5f32;
            for dy in 0..dest_h {
                let py = (dy0 + dy as u32) as f32 + 0.5;
                // Quell-Y (Nearest-Neighbor innerhalb des AoI).
                let sy = (src_y0 as usize + (src_y_f * step_y) as usize).min(src_y_cap);
                src_y_f += 1.0;
                let mut src_x_f = 0.5f32;
                for dx in 0..dest_w {
                    let px = (dx0 + dx as u32) as f32 + 0.5;
                    // Hard-Edge-Faktor an dieser Canvas-Position (nur wenn gesetzt).
                    let edge = if has_edge {
                        surface.hard_edge_factor(px, py)
                    } else {
                        1.0
                    };
                    if edge <= 0.0 {
                        src_x_f += 1.0;
                        continue;
                    }
                    let a = alpha_base * edge;
                    let sx = (src_x0 as usize + (src_x_f * step_x) as usize).min(src_x_cap);
                    src_x_f += 1.0;
                    let src = &frame.rgba[(sy * src_stride + sx) * 4..][..4];
                    let dst = &mut out
                        [((dy0 as usize + dy) * cw as usize + dx0 as usize + dx) * 4..][..4];
                    let mode = surface.blend_mode;
                    // Effektive Source-Alpha (Surface-Deckkraft × Frame-Alpha × Edge).
                    let sa = (src[3] as f32 / 255.0) * a;
                    if sa <= 0.0 {
                        continue;
                    }
                    if a >= 1.0 && mode == crate::core::BlendMode::SourceOver && sa >= 1.0 {
                        // Vollauf deckend + SourceOver: direkter Copy (häufigster Fall).
                        dst.copy_from_slice(src);
                        continue;
                    }
                    // Dest-Alpha (Stage-Canvas startet transparent → echtes Over).
                    let da = dst[3];
                    if da == 0 {
                        // Fast Path: noch transparente Dest → keine Division nötig.
                        for c in 0..3 {
                            let s = src[c] as f32;
                            let blended = mode.mix(0.0, s);
                            // da == 0 → oa = sa → rgb = blended (keine Division).
                            dst[c] = blended.round().clamp(0.0, 255.0) as u8;
                        }
                        dst[3] = (sa * 255.0).round().clamp(0.0, 255.0) as u8;
                    } else if da == 255 {
                        // Fast Path: deckende Dest → oa = 1, alpha bleibt 255.
                        let da_f = 1.0 - sa;
                        for c in 0..3 {
                            let s = src[c] as f32;
                            let d = dst[c] as f32;
                            let blended = mode.mix(d, s);
                            dst[c] = (blended * sa + d * da_f).round().clamp(0.0, 255.0) as u8;
                        }
                        dst[3] = 255;
                    } else {
                        // General Path: echte Over-Alpha mit Division.
                        let da = da as f32 / 255.0;
                        let oa = sa + da * (1.0 - sa);
                        for c in 0..3 {
                            let s = src[c] as f32;
                            let d = dst[c] as f32;
                            let blended = mode.mix(d, s);
                            dst[c] = if oa > 0.0 {
                                ((blended * sa + d * da * (1.0 - sa)) / oa).round()
                            } else {
                                0.0
                            }
                            .clamp(0.0, 255.0) as u8;
                        }
                        dst[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }

        out
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
            lock_aspect: false,
            blend_mode: Default::default(),
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
            lock_aspect: false,
            blend_mode: Default::default(),
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
            lock_aspect: false,
            blend_mode: Default::default(),
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
            lock_aspect: false,
            blend_mode: Default::default(),
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
        assert_eq!(
            edge_px,
            &[64, 0, 191, 255],
            "x=4: alpha 0.25 über blau (gerundet)"
        );
        // x=5: Mittelpunkt 5.5 → 1.5 px innen → factor 0.75.
        let mid = px_at(5);
        assert_eq!(
            mid,
            &[191, 0, 64, 255],
            "x=5: alpha 0.75 über blau (gerundet)"
        );
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
            &[0, 0, 0, 0],
            "unsichtbar = transparenter Canvas (Key/Fill-Alpha)"
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
        // 50 % Weiß über transparent = Weiß mit Alpha 127 (un-premultiplied,
        // Key/Fill braucht die echte Alpha).
        let px = &canvas.rgba[..4];
        assert_eq!(px[0], 255, "Farbe bleibt un-premultiplied: {px:?}");
        assert!(
            (px[3] as i32 - 127).abs() <= 1,
            "Deckkraft 0.5 → Alpha 127: {px:?}"
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
        assert!(comp.is_dirty("S", id, Some(&f1), 1.0));
        let mut frames = HashMap::new();
        frames.insert(id, f1.clone());
        comp.note_composited(
            "S",
            &[(id, 0, SurfaceGeometry::full_canvas(), Some(f1.clone()), 1.0)],
            &frames,
            &opacities,
        );
        // Gleiches Frame + Opacity = nicht dirty.
        assert!(!comp.is_dirty("S", id, Some(&f1), 1.0));
        // Neues Frame (anderer Ptr) = dirty.
        assert!(comp.is_dirty("S", id, Some(&f2), 1.0));
        // Opacity geändert = dirty.
        assert!(comp.is_dirty("S", id, Some(&f1), 0.5));
    }
}

#[cfg(test)]
mod blend_mode_tests {
    use super::super::frame::new_stage_frame_store;
    use super::*;
    use crate::core::BlendMode;
    use std::sync::Arc;

    fn frame(w: u32, h: u32, rgba: Vec<u8>) -> Arc<VideoFrame> {
        Arc::new(VideoFrame::new(w, h, rgba))
    }

    #[test]
    fn multiply_blends_instead_of_covering() {
        let blue = frame(2, 2, [0u8, 0, 255, 255].repeat(4));
        let white = frame(2, 2, [255u8, 255, 255, 255].repeat(4));
        // Erst blau (SourceOver, Layer 0), dann Weiß mit Multiply (Layer 1).
        let under = SurfaceGeometry::full_canvas();
        let mut over = SurfaceGeometry::full_canvas();
        over.blend_mode = BlendMode::Multiply;
        let surfaces = vec![
            (Uuid::new_v4(), 0, under, Some(blue), 1.0),
            (Uuid::new_v4(), 1, over, Some(white), 1.0),
        ];
        let store = new_stage_frame_store();
        let comp = Compositor::new();
        comp.composite("S", (2, 2), &surfaces, &store);
        let out = store.lock().unwrap().get("S").cloned().unwrap();
        assert_eq!(
            &out.rgba[0..4],
            &[0, 0, 255, 255],
            "Multiply mit Weiß ändert Blau nicht"
        );

        // Screen mit Schwarz über Blau ändert nichts.
        let blue2 = frame(2, 2, [0u8, 0, 255, 255].repeat(4));
        let black = frame(2, 2, [0u8, 0, 0, 255].repeat(4));
        let under = SurfaceGeometry::full_canvas();
        let mut over = SurfaceGeometry::full_canvas();
        over.blend_mode = BlendMode::Screen;
        let surfaces = vec![
            (Uuid::new_v4(), 0, under, Some(blue2), 1.0),
            (Uuid::new_v4(), 1, over, Some(black), 1.0),
        ];
        let store = new_stage_frame_store();
        comp.composite("S2", (2, 2), &surfaces, &store);
        let out = store.lock().unwrap().get("S2").cloned().unwrap();
        assert_eq!(
            &out.rgba[0..4],
            &[0, 0, 255, 255],
            "Screen mit Schwarz ändert Blau nicht"
        );
    }
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn composite_1080p_three_layers_stays_fast() {
        // 3 volle 1080p-Surfaces (opaque, SourceOver → Copy-Fast-Path).
        let frame = Arc::new(VideoFrame::new(
            1920,
            1080,
            [200u8, 200, 200, 255].repeat(1920 * 1080),
        ));
        let surfaces: Vec<SurfaceEntry> = (0..3)
            .map(|layer| {
                (
                    Uuid::new_v4(),
                    layer,
                    SurfaceGeometry::full_canvas(),
                    Some(frame.clone()),
                    1.0,
                )
            })
            .collect();
        // Warmup + Timing (großzügige Grenze – nur gegen Pathological-Regression).
        let t = Instant::now();
        let out = composite_canvas((1920, 1080), &surfaces);
        let elapsed = t.elapsed();
        assert_eq!(out.len(), 1920 * 1080 * 4);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "1080p-Composite mit 3 Layern zu langsam: {elapsed:?}"
        );
    }

    #[test]
    fn composite_1080p_alpha_blend_stays_fast() {
        // Halbtransparente Quelle (Alpha 128) → Over-Alpha-Pfad.
        let frame = Arc::new(VideoFrame::new(
            1920,
            1080,
            [255u8, 0, 0, 128].repeat(1920 * 1080),
        ));
        let surface = SurfaceGeometry {
            opacity: 0.8,
            ..SurfaceGeometry::full_canvas()
        };
        let surfaces: Vec<SurfaceEntry> = vec![(Uuid::new_v4(), 0, surface, Some(frame), 1.0)];
        let t = Instant::now();
        let out = composite_canvas((1920, 1080), &surfaces);
        let elapsed = t.elapsed();
        assert_eq!(out.len(), 1920 * 1080 * 4);
        // Alpha des Ergebnisses: 128/255 * 0.8 ≈ 102.
        assert!(
            (out[3] as i32 - 102).abs() <= 2,
            "alpha ≈ 102, got {}",
            out[3]
        );
        assert!(
            elapsed < std::time::Duration::from_millis(800),
            "1080p-Alpha-Composite zu langsam: {elapsed:?}"
        );
    }
}

#[cfg(test)]
mod review_c_tests {
    use super::*;
    use crate::core::{BlendCurve, EdgeSide, GeometryMode, HardEdge};

    fn surface_at(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        frame: Arc<VideoFrame>,
        hard_edge: Option<HardEdge>,
    ) -> SurfaceEntry {
        let surface = SurfaceGeometry {
            x,
            y,
            width: w,
            height: h,
            area_of_interest: None,
            hard_edge,
            opacity: 1.0,
            visible: true,
            mode: GeometryMode::Custom,
            lock_aspect: false,
            blend_mode: Default::default(),
        };
        (Uuid::new_v4(), 0, surface, Some(frame), 1.0)
    }

    fn half_red_right_blue() -> Arc<VideoFrame> {
        let mut rgba = Vec::new();
        for _y in 0..100 {
            for x in 0..100 {
                if x < 50 {
                    rgba.extend_from_slice(&[255, 0, 0, 255]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 255, 255]);
                }
            }
        }
        Arc::new(VideoFrame::new(100, 100, rgba))
    }

    fn px(out: &[u8], x: u32, y: u32) -> &[u8] {
        let i = ((y * 100 + x) * 4) as usize;
        &out[i..i + 4]
    }

    #[test]
    fn c9_surface_clipped_left_is_cropped_not_squeezed() {
        // Custom x=-50: Canvas-Spalten 0..50 zeigen Quell-Spalten 50..100 (blau).
        let surfaces = vec![surface_at(
            -50.0,
            0.0,
            100.0,
            100.0,
            half_red_right_blue(),
            None,
        )];
        let out = composite_canvas((100, 100), &surfaces);
        assert_eq!(
            px(&out, 0, 0),
            &[0, 0, 255, 255],
            "Pixel 0 = Blau (Quell-Spalte 50)"
        );
        assert_eq!(px(&out, 49, 0), &[0, 0, 255, 255], "Pixel 49 = Blau");
        assert_eq!(
            px(&out, 60, 0),
            &[0, 0, 0, 0],
            "Pixel 60 außerhalb der Surface → transparent"
        );
    }

    #[test]
    fn c9_surface_clipped_right_top_bottom_crops() {
        // x=+50: Canvas-Spalten 50..100 zeigen Quell-Spalten 0..50 (rot).
        let surfaces = vec![surface_at(
            50.0,
            0.0,
            100.0,
            100.0,
            half_red_right_blue(),
            None,
        )];
        let out = composite_canvas((100, 100), &surfaces);
        assert_eq!(
            px(&out, 50, 0),
            &[255, 0, 0, 255],
            "Pixel 50 = Rot (Quell-Spalte 0)"
        );
        assert_eq!(
            px(&out, 99, 99),
            &[255, 0, 0, 255],
            "Pixel 99 = Rot (Quell-Spalte 49)"
        );
        assert_eq!(
            px(&out, 0, 0),
            &[0, 0, 0, 0],
            "Pixel 0 außerhalb → transparent"
        );
        // y=-50: Canvas-Zeile 0 zeigt Quell-Zeile 50.
        let surfaces = vec![surface_at(
            0.0,
            -50.0,
            100.0,
            100.0,
            half_red_right_blue(),
            None,
        )];
        let out = composite_canvas((100, 100), &surfaces);
        assert_eq!(
            px(&out, 0, 0),
            &[255, 0, 0, 255],
            "Zeile 0 = Quell-Zeile 50, Spalte 0 = Rot"
        );
        // y=+50: Canvas-Zeile 99 zeigt Quell-Zeile 49.
        let surfaces = vec![surface_at(
            0.0,
            50.0,
            100.0,
            100.0,
            half_red_right_blue(),
            None,
        )];
        let out = composite_canvas((100, 100), &surfaces);
        assert_eq!(
            px(&out, 0, 99),
            &[255, 0, 0, 255],
            "letzte Zeile = Quell-Zeile 49"
        );
    }

    #[test]
    fn c10_hard_edge_right_survives_fast_path() {
        // Vollbild mit Hard Edge rechts (50 px): Pixel 99 fast transparent
        // (vorher Fast Path memcpy → Alpha 255, Kante verloren).
        let frame = Arc::new(VideoFrame::new(
            100,
            100,
            [255, 255, 255, 255].repeat(100 * 100),
        ));
        let edge = HardEdge::new(EdgeSide::Right, 50.0, BlendCurve::Linear);
        let surfaces = vec![surface_at(0.0, 0.0, 100.0, 100.0, frame, Some(edge))];
        let out = composite_canvas((100, 100), &surfaces);
        assert!(
            px(&out, 99, 99)[3] < 10,
            "Pixel 99 an rechter Kante fast transparent, got {}",
            px(&out, 99, 99)[3]
        );
        assert_eq!(
            px(&out, 25, 25)[3],
            255,
            "Pixel 25 in der Mitte voll deckend"
        );
    }
}

#[cfg(test)]
mod review_e_tests {
    use super::*;
    use crate::core::{GeometryMode, SurfaceGeometry};

    fn entry(id: Uuid, frame: Arc<VideoFrame>) -> SurfaceEntry {
        let surface = SurfaceGeometry {
            mode: GeometryMode::Custom,
            ..SurfaceGeometry::full_canvas()
        };
        (id, 0, surface, Some(frame), 1.0)
    }

    /// Review E13: note_composited einer Stage darf die Dirty-Stände einer
    /// anderen Stage nicht löschen (sonst gilt deren Cue immer als geändert).
    #[test]
    fn e13_dirty_tracking_pro_stage_getrennt() {
        let mut comp = Compositor::new();
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let f = Arc::new(VideoFrame::new(2, 2, [255, 0, 0, 255].repeat(4)));
        let mut frames = HashMap::new();
        frames.insert(id_a, f.clone());
        frames.insert(id_b, f.clone());
        let opacities: HashMap<Uuid, f32> = HashMap::new();

        comp.note_composited("A", &[entry(id_a, f.clone())], &frames, &opacities);
        // Stage B komponieren – darf Stage A nicht wipen.
        comp.note_composited("B", &[entry(id_b, f.clone())], &frames, &opacities);
        assert!(
            !comp.is_dirty("A", id_a, Some(&f), 1.0),
            "Stage A darf nach Komposition von Stage B nicht dirty sein (E13)"
        );
        assert!(!comp.is_dirty("B", id_b, Some(&f), 1.0));
        // Neuer Frame in Stage A → dirty; Stage B bleibt clean.
        let f2 = Arc::new(VideoFrame::new(2, 2, [0, 255, 0, 255].repeat(4)));
        assert!(comp.is_dirty("A", id_a, Some(&f2), 1.0));
        assert!(!comp.is_dirty("B", id_b, Some(&f), 1.0));
    }
}
