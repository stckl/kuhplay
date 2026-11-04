//! Reine Render-Funktionen für das Video-Routing:
//! Stage → Region → Output Route → Device.
//!
//! Alles läuft auf der CPU in reinen Funktionen – ohne GPU testbar, und
//! Decklink braucht die Pixel ohnehin (Arbeitsauftrag Abschnitt 3.1).
//!
//! * [`render_region_into`]: malt den Ausschnitt einer Region aus dem
//!   Stage-Frame in den Raster einer Route (Region füllt die Route,
//!   Abschnitt 3.5) inkl. Edge-Blend mit Blend-Gamma.
//! * [`render_route`]: schwarzer Raster; alle Stages (unten zuerst) malen
//!   ihre Regionen auf die Route; Route-Testbild überdeckt alles,
//!   Region-Grid liegt über der einzelnen Region.
//! * [`present_on_device`]: Drehung, Spiegelung (Rückprojektion), dann
//!   Skalierung (Center/Fit/Stretch/Fill) auf das Device.
//! * [`auto_blends`]: Blendbreiten aus der Überlappung benachbarter
//!   Regionen.
//!
//! Interne Koordinaten: Pixel, Ursprung oben links. Nearest-Neighbor-
//! Sampling wie im Compositor (1-Pixel-Linien des Testbilds bleiben
//! erhalten).

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::video_routing::{
    edge_factor, region_letter, regions_in_reading_order, EdgeBlend, OutputRoute, Region,
    RouteScaling,
};

/// `f32::from(u32)` gibt es nicht – zentraler Helper.
fn f(v: u32) -> f32 {
    v as f32
}
use crate::core::{ScreenRotation, Stage};
use crate::engine::frame::VideoFrame;

/// Zeichnet den Ausschnitt `region` aus `stage_frame` in den Route-Raster
/// `target` (`route_w`×`route_h`, muss zur Ziel-Route passen).
///
/// Die Region wird auf den **vollen Raster** der Route abgebildet
/// (Arbeitsauftrag 3.5): Region und Route mit gleichem Format verzerren
/// nicht. `blend` sind die wirksamen Blendbreiten (Stage-Pixel, siehe
/// [`auto_blends`]); der Faktorverlauf folgt `(x/breite)^(1/gamma)`, damit
/// die Summe beider Projektoren im Überlapp konstant hell bleibt.
///
/// `stage_crop`: Sichtfenster der Stage (Stage-Pixel, `(x0, y0, x1, y1)`);
/// die Region wird auf das Fenster beschritten (leer = nichts), kein Crop
/// = die ganze Stage. `stage_edge`/`stage_soft_edge`: Randabblendung der
/// Stage am Route-Ausgang (Soft Edge = Gamma-Kurve, Hard Edge = linear).
#[allow(clippy::too_many_arguments)]
pub fn render_region_into(
    target: &mut VideoFrame,
    route_w: u32,
    route_h: u32,
    stage_frame: &VideoFrame,
    region: &Region,
    blend: &EdgeBlend,
    blend_gamma: f32,
    stage_crop: Option<(f32, f32, f32, f32)>,
    stage_edge: &EdgeBlend,
    stage_soft_edge: bool,
    keep_alpha: bool,
) {
    debug_assert_eq!((target.width, target.height), (route_w, route_h));
    let (tw, th) = (route_w.max(1), route_h.max(1));
    let (sw, sh) = (
        stage_frame.width.max(1) as f32,
        stage_frame.height.max(1) as f32,
    );
    let stride = stage_frame.width.max(1) as usize;
    // Regions-Rechteck auf die Stage-Frame-Grenzen clamen (Region liegt
    // laut Validierung in der Stage; der Frame kann minimal kleiner sein).
    let mut rx = region.x.max(0.0).min(sw - 1.0);
    let mut ry = region.y.max(0.0).min(sh - 1.0);
    let mut rw = (region.x + region.width).clamp(rx + 1.0, sw) - rx;
    let mut rh = (region.y + region.height).clamp(ry + 1.0, sh) - ry;
    // Stage-Crop: Region auf das Sichtfenster beschneiden (das Fenster
    // bleibt in absoluten Stage-Koordinaten; was draussen liegt, ist weg).
    if let Some((cx0, cy0, cx1, cy1)) = stage_crop {
        let nx0 = rx.max(cx0.max(0.0));
        let ny0 = ry.max(cy0.max(0.0));
        let nx1 = (rx + rw).min(cx1.min(sw));
        let ny1 = (ry + rh).min(cy1.min(sh));
        if nx1 <= nx0 || ny1 <= ny0 {
            return;
        }
        rx = nx0;
        ry = ny0;
        rw = nx1 - nx0;
        rh = ny1 - ny0;
    }
    if rw <= 0.0 || rh <= 0.0 {
        return;
    }

    // Stage-Randabblendung: Faktor am Punkt (u, v) im Stage-Raum
    // (Soft Edge = Gamma-Kurve wie bei Regionen, Hard Edge = linear).
    let stage_edge_factor = |u: f32, v: f32| -> f32 {
        let ramp = |t: f32| {
            if stage_soft_edge {
                edge_factor(t, blend_gamma)
            } else {
                t.clamp(0.0, 1.0)
            }
        };
        let mut factor = 1.0f32;
        if stage_edge.left > 0.0 {
            factor *= ramp(u / stage_edge.left);
        }
        if stage_edge.right > 0.0 {
            factor *= ramp((sw - u) / stage_edge.right);
        }
        if stage_edge.top > 0.0 {
            factor *= ramp(v / stage_edge.top);
        }
        if stage_edge.bottom > 0.0 {
            factor *= ramp((sh - v) / stage_edge.bottom);
        }
        factor
    };
    let stage_edge_active = stage_edge.left > 0.0
        || stage_edge.right > 0.0
        || stage_edge.top > 0.0
        || stage_edge.bottom > 0.0;

    for py in 0..th {
        // Quell-Koordinate in Stage-Pixeln (Pixelmitten).
        let v = ry + (f(py) + 0.5) * rh / f(th);
        let sy = (v.floor().max(0.0) as u32).min(stage_frame.height.saturating_sub(1)) as usize;
        for px in 0..tw {
            let u = rx + (f(px) + 0.5) * rw / f(tw);
            let sx = (u.floor().max(0.0) as u32).min(stage_frame.width.saturating_sub(1)) as usize;
            // Edge-Blend-Faktoren an den vier Kanten (Stage-Raum).
            let mut factor = 1.0f32;
            if blend.left > 0.0 {
                factor *= edge_factor((u - rx) / blend.left, blend_gamma);
            }
            if blend.right > 0.0 {
                factor *= edge_factor((rx + rw - u) / blend.right, blend_gamma);
            }
            if blend.top > 0.0 {
                factor *= edge_factor((v - ry) / blend.top, blend_gamma);
            }
            if blend.bottom > 0.0 {
                factor *= edge_factor((ry + rh - v) / blend.bottom, blend_gamma);
            }
            if stage_edge_active {
                factor *= stage_edge_factor(u, v);
            }
            let src = &stage_frame.rgba[(sy * stride + sx) * 4..][..4];
            let dst = &mut target.rgba[(py as usize * tw as usize + px as usize) * 4..][..4];
            let a = factor.clamp(0.0, 1.0);
            if keep_alpha {
                // Key-Route: echte Alpha-Komposition (over), Alpha bleibt erhalten.
                let sa = (f32::from(src[3]) / 255.0) * a;
                if sa <= 0.0 {
                    continue;
                }
                let da = dst[3];
                if da == 0 {
                    // Fast Path: transparente Dest → rgb = src, alpha = sa.
                    dst[0] = src[0];
                    dst[1] = src[1];
                    dst[2] = src[2];
                    dst[3] = (sa * 255.0).round().clamp(0.0, 255.0) as u8;
                } else if da == 255 || sa >= 1.0 {
                    // Deckende Dest (oder vollauf deckende Source) → oa = 1.
                    let inv = 1.0 - sa;
                    for c in 0..3 {
                        let s = f32::from(src[c]);
                        let d = f32::from(dst[c]);
                        dst[c] = (s * sa + d * inv).round().clamp(0.0, 255.0) as u8;
                    }
                    dst[3] = 255;
                } else {
                    let da_f = f32::from(da) / 255.0;
                    let oa = sa + da_f * (1.0 - sa);
                    for c in 0..3 {
                        let s = f32::from(src[c]);
                        let d = f32::from(dst[c]);
                        dst[c] = if oa > 0.0 {
                            ((s * sa + d * da_f * (1.0 - sa)) / oa).round()
                        } else {
                            0.0
                        }
                        .clamp(0.0, 255.0) as u8;
                    }
                    dst[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            } else {
                // Fill-Route: effektive Deckkraft = Source-Alpha × Kantenfaktor
                // (Review C8: vorher wurde das Quell-Alpha ignoriert – eine
                // halbtransparente obere Stage wurde schwarz deckend statt
                // die untere durchscheinen zu lassen; Opacity wirkte nicht).
                // Ergebnis-Alpha bleibt 255 (Fill komponiert auf Schwarz).
                let sa = (f32::from(src[3]) / 255.0) * a;
                if sa <= 0.0 {
                    continue;
                }
                let inv = 1.0 - sa;
                for c in 0..3 {
                    let s = f32::from(src[c]);
                    let d = f32::from(dst[c]);
                    dst[c] = (s * sa + d * inv).round().clamp(0.0, 255.0) as u8;
                }
                dst[3] = 255;
            }
        }
    }
}

/// Komponiert das Bild einer Route: schwarzer Raster, dann alle Stages
/// von unten nach oben (Layer, bei gleichem Layer alphabetisch späterer
/// Name zuerst – der frühere liegt oben), je Region der Ausschnitt auf
/// den vollen Raster.
///
/// `stage_frames`: Stage-Name → komponiertes Stage-Canvas (Compositor).
/// Route-Testbild überdeckt den Inhalt; `Region::grid` legt ein Raster
/// nur über den Bereich dieser Region (Beschriftung „B: Route“, Pfeil-
/// Zeichen fehlen im Testbild-Font).
pub fn render_route(
    route: &OutputRoute,
    stages: &[Stage],
    stage_frames: &HashMap<String, Arc<VideoFrame>>,
) -> VideoFrame {
    let (rw, rh) = (route.width.max(1), route.height.max(1));
    // Key-Route (alpha=true): Canvas startet transparent, Alpha bleibt erhalten.
    let mut out = if route.alpha {
        VideoFrame::transparent(rw, rh)
    } else {
        // Fill-Route: deckendes Schwarz (Alpha 255) als Hintergrund –
        // VideoFrame::black() ist transparent-schwarz, Alpha hier auf 255.
        let mut f = VideoFrame::black(rw, rh);
        for px in f.rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        f
    };
    // Unten zuerst malen (Layer-Regel aus Abschnitt 4).
    let mut ordered: Vec<&Stage> = stages.iter().collect();
    ordered.sort_by(|a, b| a.layer.cmp(&b.layer).then_with(|| b.name.cmp(&a.name)));

    for stage in ordered {
        if !stage.visible || stage.regions.is_empty() {
            continue;
        }
        let Some(frame) = stage_frames.get(&stage.name) else {
            continue;
        };
        // Stage-Grid: läuft über alle Regionen hinweg, deshalb auf dem Canvas.
        let gridded;
        let frame: &VideoFrame = if stage.grid {
            let mut copy = (**frame).clone();
            let (w, h) = (copy.width, copy.height);
            super::pattern::draw_region_grid(&mut copy.rgba, w, h, &stage.name, [255, 255, 255]);
            gridded = copy;
            &gridded
        } else {
            frame
        };
        // Buchstaben der Regionen (Lesefolge) für das Region-Grid.
        let letters: std::collections::HashMap<uuid::Uuid, String> =
            regions_in_reading_order(stage.regions.iter())
                .iter()
                .enumerate()
                .map(|(i, region)| (region.id, region_letter(i)))
                .collect();
        let auto = auto_blends(&stage.regions);
        for (index, region) in stage.regions.iter().enumerate() {
            if region.route != Some(route.id) {
                continue;
            }
            let effective = if region.auto_blend {
                auto[index]
            } else {
                region.blend
            };
            // Stage-Crop als Pixel-Fenster (normalisiert -> Stage-Pixel).
            let stage_crop = stage.crop.map(|crop| {
                let (fw, fh) = (frame.width.max(1) as f32, frame.height.max(1) as f32);
                (
                    crop.left * fw,
                    crop.top * fh,
                    crop.right * fw,
                    crop.bottom * fh,
                )
            });
            render_region_into(
                &mut out,
                rw,
                rh,
                frame,
                region,
                &effective,
                stage.blend_gamma,
                stage_crop,
                &stage.edge,
                stage.soft_edge,
                route.alpha,
            );
            if region.grid {
                let label = format!(
                    "{}: {}",
                    letters.get(&region.id).cloned().unwrap_or_default(),
                    route.name
                );
                super::pattern::draw_region_grid(&mut out.rgba, rw, rh, &label, route.color);
            }
        }
    }

    // Route-Testbild überdeckt den Inhalt („Test Pattern“).
    if route.test_pattern {
        let spec = crate::core::TestPatternSpec {
            label: route.label(),
            width: rw,
            height: rh,
            kind: route.test_pattern_kind,
            color: route.color,
            index: 0,
            checker_size: route.checker_size,
        };
        super::pattern::draw_test_pattern(&mut out.rgba, rw, rh, &spec);
    }
    out
}

/// Vorschau einer Stage (Preview-Fenster) in `out_w`×`out_h` Pixeln: das
/// Stage-Bild verkleinert, darüber dieselben Testbilder wie auf den
/// Ausgängen – Stage-Grid (Footer, Weiß) über die ganze Stage, Region-Grid
/// bzw. Route-Testbild (überdeckt den Inhalt) in der jeweiligen Region.
/// Gezeichnet wird in Vorschau-Pixeln, damit die 1-px-Linien nicht beim
/// Verkleinern verloren gehen.
pub fn stage_preview(
    stage: &Stage,
    frame: &VideoFrame,
    routes: &[OutputRoute],
    out_w: u32,
    out_h: u32,
) -> VideoFrame {
    let (out_w, out_h) = (out_w.max(1), out_h.max(1));
    let mut out = match image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()) {
        Some(img) => {
            let small =
                image::imageops::resize(&img, out_w, out_h, image::imageops::FilterType::Triangle);
            VideoFrame::new(out_w, out_h, small.into_raw())
        }
        None => VideoFrame::black(out_w, out_h),
    };
    if stage.grid {
        super::pattern::draw_region_grid(&mut out.rgba, out_w, out_h, &stage.name, [255; 3]);
    }
    let (sx, sy) = (
        f(out_w) / stage.geometry.width.max(1.0),
        f(out_h) / stage.geometry.height.max(1.0),
    );
    let letters: HashMap<uuid::Uuid, String> = regions_in_reading_order(stage.regions.iter())
        .iter()
        .enumerate()
        .map(|(i, region)| (region.id, region_letter(i)))
        .collect();
    for region in &stage.regions {
        let Some(route) = region
            .route
            .and_then(|id| routes.iter().find(|r| r.id == id))
        else {
            continue;
        };
        if !route.test_pattern && !region.grid {
            continue;
        }
        // Region in Vorschau-Pixeln (am Bildrand abgeschnitten).
        let x0 = (region.x * sx).round().clamp(0.0, f(out_w)) as u32;
        let y0 = (region.y * sy).round().clamp(0.0, f(out_h)) as u32;
        let x1 = ((region.x + region.width) * sx)
            .round()
            .clamp(0.0, f(out_w)) as u32;
        let y1 = ((region.y + region.height) * sy)
            .round()
            .clamp(0.0, f(out_h)) as u32;
        let (rw, rh) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if rw < 2 || rh < 2 {
            continue;
        }
        let mut tile = vec![0u8; (rw * rh * 4) as usize];
        if route.test_pattern {
            let spec = crate::core::TestPatternSpec {
                label: route.label(),
                width: rw,
                height: rh,
                kind: route.test_pattern_kind,
                color: route.color,
                index: 0,
                checker_size: route.checker_size,
            };
            super::pattern::draw_test_pattern(&mut tile, rw, rh, &spec);
        } else {
            let label = format!(
                "{}: {}",
                letters.get(&region.id).cloned().unwrap_or_default(),
                route.name
            );
            super::pattern::draw_region_grid(&mut tile, rw, rh, &label, route.color);
        }
        let row = (rw * 4) as usize;
        for y in 0..rh as usize {
            let dst = ((y0 as usize + y) * out_w as usize + x0 as usize) * 4;
            out.rgba[dst..dst + row].copy_from_slice(&tile[y * row..(y + 1) * row]);
        }
    }
    out
}

/// Bildet ein Route-Bild auf das Device ab: **Drehung**, dann
/// **Spiegelung** (Rückprojektion), dann **Skalierung** (Center/Fit/
/// Stretch/Fill, Arbeitsauftrag Phase 2). Hintergrund außerhalb der
/// Skalierung bleibt schwarz.
pub fn present_on_device(
    route_frame: &VideoFrame,
    route: &OutputRoute,
    device_w: u32,
    device_h: u32,
) -> VideoFrame {
    let (dw, dh) = (device_w.max(1), device_h.max(1));
    // 1) Drehung (90° im Uhrzeigersinn usw.).
    let rotated = rotate(route_frame, route.rotation);
    // 2) Rückprojektion: horizontal spiegeln.
    let oriented: VideoFrame = if route.rear_projection {
        mirror_horizontal(&rotated)
    } else {
        rotated
    };
    let oriented = if route.mirror_vertical {
        mirror_vertical(&oriented)
    } else {
        oriented
    };
    // 3) Skalierung.
    let (sw, sh) = (oriented.width.max(1) as f32, oriented.height.max(1) as f32);
    let (dwf, dhf) = (f(dw), f(dh));
    let (scale, ox, oy) = match route.scaling {
        RouteScaling::Stretch => (1.0f32, 0.0f32, 0.0f32), // eigenes Mapping unten
        RouteScaling::Center => (1.0, (dwf - sw) / 2.0, (dhf - sh) / 2.0),
        RouteScaling::Fit => {
            let scale = (dwf / sw).min(dhf / sh);
            let (w, h) = (sw * scale, sh * scale);
            (scale, (dwf - w) / 2.0, (dhf - h) / 2.0)
        }
        RouteScaling::Fill => {
            let scale = (dwf / sw).max(dhf / sh);
            let (w, h) = (sw * scale, sh * scale);
            (scale, (dwf - w) / 2.0, (dhf - h) / 2.0)
        }
    };
    let mut out = VideoFrame::black(dw, dh);
    if route.scaling == RouteScaling::Stretch {
        // Breite und Höhe getrennt auf das Device ziehen.
        for y in 0..dh {
            let sy = (f(y) * sh / dhf).floor() as u32 % oriented.height;
            for x in 0..dw {
                let sx = (f(x) * sw / dwf).floor() as u32 % oriented.width;
                copy_pixel(&oriented, sx, sy, &mut out, x, y);
            }
        }
        return out;
    }
    for y in 0..dh {
        for x in 0..dw {
            // Gerätepixel-Mitte zurück in die Quelle.
            let u = (f(x) + 0.5 - ox) / scale - 0.5;
            let v = (f(y) + 0.5 - oy) / scale - 0.5;
            let sx = u.round().floor() as i64;
            let sy = v.round().floor() as i64;
            if sx < 0 || sy < 0 || sx >= oriented.width as i64 || sy >= oriented.height as i64 {
                continue; // Überstand abschneiden / Balken bleiben schwarz.
            }
            copy_pixel(&oriented, sx as u32, sy as u32, &mut out, x, y);
        }
    }
    out
}

/// Kopiert ein Pixel (RGBA, deckend).
fn copy_pixel(src: &VideoFrame, sx: u32, sy: u32, dst: &mut VideoFrame, x: u32, y: u32) {
    let s = (sy as usize * src.width as usize + sx as usize) * 4;
    let d = (y as usize * dst.width as usize + x as usize) * 4;
    dst.rgba[d..d + 4].copy_from_slice(&src.rgba[s..s + 4]);
}

/// Dreht ein Frame um 90°-Schritte im Uhrzeigersinn (obere Kante
/// erscheint bei 90° an der rechten Kante).
pub fn rotate(frame: &VideoFrame, rotation: ScreenRotation) -> VideoFrame {
    match rotation {
        ScreenRotation::Deg0 => VideoFrame::new(frame.width, frame.height, frame.rgba.clone()),
        ScreenRotation::Deg90 => {
            let (w, h) = (frame.height, frame.width);
            let mut out = VideoFrame::black(w, h);
            for y in 0..h {
                for x in 0..w {
                    // CW: (sx, sy) -> (h_src-1-sy, sx)
                    let sx = y;
                    let sy = frame.height - 1 - x;
                    copy_pixel(frame, sx, sy, &mut out, x, y);
                }
            }
            out
        }
        ScreenRotation::Deg180 => {
            let mut out = VideoFrame::black(frame.width, frame.height);
            for y in 0..frame.height {
                for x in 0..frame.width {
                    let sx = frame.width - 1 - x;
                    let sy = frame.height - 1 - y;
                    copy_pixel(frame, sx, sy, &mut out, x, y);
                }
            }
            out
        }
        ScreenRotation::Deg270 => {
            let (w, h) = (frame.height, frame.width);
            let mut out = VideoFrame::black(w, h);
            for y in 0..h {
                for x in 0..w {
                    // CCW: (sx, sy) -> (sy, w_src-1-sx)
                    let sx = frame.width - 1 - y;
                    let sy = x;
                    copy_pixel(frame, sx, sy, &mut out, x, y);
                }
            }
            out
        }
    }
}

/// Spiegelt ein Frame vertikal.
pub fn mirror_vertical(frame: &VideoFrame) -> VideoFrame {
    let stride = frame.width as usize * 4;
    let mut rgba = Vec::with_capacity(frame.rgba.len());
    for row in frame.rgba.chunks_exact(stride.max(1)).rev() {
        rgba.extend_from_slice(row);
    }
    VideoFrame::new(frame.width, frame.height, rgba)
}

/// Spiegelt ein Frame horizontal (Rückprojektion).
pub fn mirror_horizontal(frame: &VideoFrame) -> VideoFrame {
    let mut out = VideoFrame::black(frame.width, frame.height);
    for y in 0..frame.height {
        for x in 0..frame.width {
            let sx = frame.width - 1 - x;
            copy_pixel(frame, sx, y, &mut out, x, y);
        }
    }
    out
}

/// Auto-Edge-Blend: Überlappt Region A die Region B auf derselben Stage,
/// ist die Überlappbreite die Blendbreite beider zugewandten Kanten
/// (mehrere Überlappungen: die breiteste gewinnt). Rückgabe parallel zur
/// Eingabe.
pub fn auto_blends(regions: &[Region]) -> Vec<EdgeBlend> {
    let mut out = vec![EdgeBlend::ZERO; regions.len()];
    for (i, a) in regions.iter().enumerate() {
        for (j, b) in regions.iter().enumerate() {
            // Key und Fill zeigen dasselbe Bild, sie blenden nicht ineinander.
            if i == j || a.key != b.key {
                continue;
            }
            let overlap_w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
            let overlap_h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
            if overlap_w <= 0.0 || overlap_h <= 0.0 {
                continue;
            }
            // Blende nur bei *teilweiser* Überlappung je Achse:
            // Deckt die x-Überlappung die volle Breite einer Region (z. B.
            // übereinander gestapelt, gleiche Spalte), gibt es keine
            // links/rechts-Blende – und umgekehrt. Voll ineinander
            // geschachtelte Regionen bekommen so automatisch keine Blende.
            let side_left = a.x <= b.x;
            let above = a.y <= b.y;
            let w = if overlap_w < a.width.min(b.width) {
                overlap_w
            } else {
                0.0
            };
            let h = if overlap_h < a.height.min(b.height) {
                overlap_h
            } else {
                0.0
            };
            // Horizontal: zugewandte Seiten (links/rechts).
            if w > 0.0 {
                if side_left {
                    out[i].right = out[i].right.max(w);
                    out[j].left = out[j].left.max(w);
                } else {
                    out[i].left = out[i].left.max(w);
                    out[j].right = out[j].right.max(w);
                }
            }
            // Vertikal: oben/unten.
            if h > 0.0 {
                if above {
                    out[i].bottom = out[i].bottom.max(h);
                    out[j].top = out[j].top.max(h);
                } else {
                    out[i].top = out[i].top.max(h);
                    out[j].bottom = out[j].bottom.max(h);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::video_routing::Region;
    use crate::core::{ScreenRotation, StageGeometry};

    /// Frame mit einheitlicher Farbe.
    fn solid(w: u32, h: u32, color: [u8; 3]) -> Arc<VideoFrame> {
        let [r, g, b] = color;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        Arc::new(VideoFrame::new(w, h, rgba))
    }

    /// Stage-Frame mit Farbverlauf: x < split → links, sonst rechts.
    fn split_frame(w: u32, h: u32, split: u32, left: [u8; 3], right: [u8; 3]) -> Arc<VideoFrame> {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                let c = if x < split { left } else { right };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        Arc::new(VideoFrame::new(w, h, rgba))
    }

    fn make_stage(name: &str, layer: i32, regions: Vec<Region>) -> Stage {
        let mut stage = Stage::new(name, StageGeometry::new(1920.0, 1080.0));
        stage.layer = layer;
        stage.regions = regions;
        stage
    }

    fn pixel(frame: &VideoFrame, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * frame.width as usize + x as usize) * 4;
        [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
    }

    /// Zwei Regionen nebeneinander (mit Überlappung `overlap` px).
    fn two_regions(overlap: f32) -> (Region, Region) {
        let width = 1200.0;
        let a = Region::new(0.0, 0.0, width, 1080.0);
        let b = Region::new(width - overlap, 0.0, width, 1080.0);
        (a, b)
    }

    #[test]
    fn two_regions_side_by_side_show_their_half() {
        // Stage 2400 breit: Region A links (0..1200), B rechts (1200..2400),
        // je eigene Route mit der Breite der Region.
        let (mut a, mut b) = two_regions(0.0);
        let mut stage = Stage::new("S", StageGeometry::new(2400.0, 1080.0));
        let route_a = OutputRoute::new("L", 1200, 1080);
        let route_b = OutputRoute::new("R", 1200, 1080);
        a.route = Some(route_a.id);
        b.route = Some(route_b.id);
        stage.regions = vec![a, b];
        // Stage-Frame: linke Hälfte rot, rechte blau.
        let frames: HashMap<String, Arc<VideoFrame>> = std::iter::once((
            "S".to_string(),
            split_frame(2400, 1080, 1200, [255, 0, 0], [0, 0, 255]),
        ))
        .collect();

        let left = render_route(&route_a, std::slice::from_ref(&stage), &frames);
        let right = render_route(&route_b, std::slice::from_ref(&stage), &frames);
        assert_eq!((left.width, left.height), (1200, 1080));
        // Linke Route zeigt nur linke Stage-Hälfte (rot) …
        assert_eq!(pixel(&left, 10, 10), [255, 0, 0]);
        assert_eq!(pixel(&left, 1190, 540), [255, 0, 0]);
        // … rechte nur die rechte (blau).
        assert_eq!(pixel(&right, 10, 10), [0, 0, 255]);
        assert_eq!(pixel(&right, 1190, 540), [0, 0, 255]);
    }

    #[test]
    fn edge_blend_overlap_keeps_constant_brightness() {
        // 200 px Überlappung: Region A 0..1200, B 1000..2200. Weiße
        // Stage; in der Mitte des Blends muss die Summe beider Routen
        // (im linearen Licht) der Ausgangshelligkeit entsprechen.
        let (mut a, mut b) = two_regions(200.0);
        let mut stage = Stage::new("S", StageGeometry::new(2200.0, 1080.0));
        let route_a = OutputRoute::new("L", 1200, 1080);
        let route_b = OutputRoute::new("R", 1200, 1080);
        a.route = Some(route_a.id);
        b.route = Some(route_b.id);
        stage.regions = vec![a.clone(), b.clone()];
        stage.blend_gamma = 2.2;
        let frames: HashMap<String, Arc<VideoFrame>> =
            std::iter::once(("S".to_string(), solid(2200, 1080, [255, 255, 255]))).collect();

        let left = render_route(&route_a, std::slice::from_ref(&stage), &frames);
        let right = render_route(&route_b, std::slice::from_ref(&stage), &frames);

        // Auto-Blend: Überlappbreite 200 an den zugewandten Kanten.
        let blends = auto_blends(&stage.regions);
        assert_eq!(blends[0].right, 200.0);
        assert_eq!(blends[1].left, 200.0);
        assert_eq!(blends[0].left, 0.0);
        assert_eq!(blends[1].right, 0.0);
        assert_eq!(
            blends[0].bottom, 0.0,
            "volle Höhenüberlappung: keine Vertikalblende"
        );
        assert_eq!(blends[1].bottom, 0.0);

        // Mitte des Blends: Stage x = 1100 → Route A Pixel 1100,
        // Route B Pixel 100 (Stage 1100 - 1000).
        let (pa, pb) = (pixel(&left, 1100, 540), pixel(&right, 100, 540));
        // Projektoren addieren Licht: c^gamma. Die Summe muss der
        // vollen Helligkeit (255) entsprechen (±2 von 255).
        let gamma = 2.2f32;
        let la = (f32::from(pa[0]) / 255.0).powf(gamma);
        let lb = (f32::from(pb[0]) / 255.0).powf(gamma);
        let sum = (la + lb).powf(1.0 / gamma) * 255.0;
        assert!(
            (sum - 255.0).abs() <= 2.0,
            "Summe im Überlapp muss 255 sein, ist {sum} ({pa:?} + {pb:?})"
        );
        // Kern der Routen bleibt voll hell.
        assert_eq!(pixel(&left, 10, 540), [255, 255, 255]);
        assert_eq!(pixel(&right, 1190, 540), [255, 255, 255]);
    }

    #[test]
    fn scaling_center_fit_stretch_fill() {
        // 1920×1080 auf 1000×1000, wie `placement_modes_fit_fill_stretch_custom`.
        // Quelle: links von 960 rot, rechts blau.
        let frame = split_frame(1920, 1080, 960, [255, 0, 0], [0, 0, 255]);
        let mut route = OutputRoute::new("D", 1920, 1080);
        route.device = Some(crate::core::video_routing::DeviceRef::Display {
            id: 1,
            name: "D".to_string(),
            width: 1000,
            height: 1000,
        });

        // Fit: 1000×563 zentriert, Balken oben/unten.
        route.scaling = RouteScaling::Fit;
        let out = present_on_device(&frame, &route, 1000, 1000);
        assert_eq!(pixel(&out, 500, 100), [0, 0, 0], "Balken oben schwarz");
        assert_eq!(pixel(&out, 500, 900), [0, 0, 0], "Balken unten schwarz");
        assert_eq!(pixel(&out, 250, 400), [255, 0, 0], "links rot");
        assert_eq!(pixel(&out, 750, 400), [0, 0, 255], "rechts blau");

        // Fill: voll gefüllt, Mitte der Quelle 420..1500 sichtbar.
        route.scaling = RouteScaling::Fill;
        let out = present_on_device(&frame, &route, 1000, 1000);
        assert_eq!(pixel(&out, 250, 500), [255, 0, 0], "Quelle x≈690 rot");
        assert_eq!(pixel(&out, 750, 500), [0, 0, 255], "Quelle x≈1230 blau");
        assert_eq!(pixel(&out, 500, 500), [0, 0, 255]);

        // Stretch: 1920 auf 1000 → Grenze bei x=500.
        route.scaling = RouteScaling::Stretch;
        let out = present_on_device(&frame, &route, 1000, 1000);
        assert_eq!(pixel(&out, 250, 500), [255, 0, 0]);
        assert_eq!(pixel(&out, 750, 500), [0, 0, 255]);

        // Center: unskaliert zentriert, Überstand abgeschnitten
        // (Quelle 460..1460 sichtbar).
        route.scaling = RouteScaling::Center;
        let out = present_on_device(&frame, &route, 1000, 1000);
        assert_eq!(pixel(&out, 250, 500), [255, 0, 0], "Quelle x≈710 rot");
        assert_eq!(pixel(&out, 750, 500), [0, 0, 255], "Quelle x≈1210 blau");
    }

    #[test]
    fn key_and_fill_regions_do_not_blend_into_each_other() {
        let fill = Region::new(0.0, 0.0, 100.0, 100.0);
        let mut key = Region::new(50.0, 0.0, 100.0, 100.0);
        key.key = true;
        let blends = auto_blends(&[fill.clone(), key]);
        assert_eq!(blends[0], EdgeBlend::ZERO);

        let neighbour = Region::new(50.0, 0.0, 100.0, 100.0);
        assert_ne!(auto_blends(&[fill, neighbour])[0], EdgeBlend::ZERO);
    }

    #[test]
    fn stage_grid_is_drawn_across_the_stage_canvas() {
        let mut stage = Stage::new("S", crate::core::StageGeometry::new(64.0, 64.0));
        let route = OutputRoute::new("R", 64, 64);
        let mut region = crate::core::Region::full_stage(64.0, 64.0);
        region.route = Some(route.id);
        stage.regions = vec![region];
        let mut frames = HashMap::new();
        frames.insert(
            "S".to_string(),
            Arc::new(VideoFrame::new(64, 64, [0, 0, 0, 255].repeat(64 * 64))),
        );
        let plain = render_route(&route, std::slice::from_ref(&stage), &frames);
        stage.grid = true;
        let with_grid = render_route(&route, std::slice::from_ref(&stage), &frames);
        assert_ne!(plain.rgba, with_grid.rgba);
    }

    #[test]
    fn mirror_vertical_swaps_rows() {
        let frame = VideoFrame::new(1, 2, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(mirror_vertical(&frame).rgba, [5, 6, 7, 8, 1, 2, 3, 4]);
    }

    #[test]
    fn rotation_and_mirror_map_known_pixels() {
        // 4×2-Bild mit eindeutigen Farben pro Spalte.
        let mut rgba = Vec::new();
        for y in 0..2 {
            for x in 0..4 {
                rgba.extend_from_slice(&[(x * 40) as u8, (y * 80) as u8, 7, 255]);
            }
        }
        let frame = VideoFrame::new(4, 2, rgba);
        let mut route = OutputRoute::new("D", 4, 2);

        // 90° im Uhrzeigersinn: Quelle (sx, sy) -> Ziel (1-sy, sx);
        // die obere Quellkante erscheint an der rechten Zielkante.
        route.rotation = ScreenRotation::Deg90;
        let out = rotate(&frame, ScreenRotation::Deg90);
        assert_eq!((out.width, out.height), (2, 4));
        assert_eq!(
            pixel(&out, 1, 0),
            pixel(&frame, 0, 0),
            "oben links -> oben rechts"
        );
        assert_eq!(
            pixel(&out, 0, 0),
            pixel(&frame, 0, 1),
            "unten links -> oben links"
        );
        assert_eq!(
            pixel(&out, 0, 3),
            pixel(&frame, 3, 1),
            "unten rechts -> unten links"
        );
        assert_eq!(
            pixel(&out, 1, 3),
            pixel(&frame, 3, 0),
            "oben rechts -> unten rechts"
        );

        // 180°: komplett gedreht.
        let out = rotate(&frame, ScreenRotation::Deg180);
        assert_eq!(pixel(&out, 0, 0), pixel(&frame, 3, 1));

        // Spiegelung (Rückprojektion): horizontal.
        let mirrored = mirror_horizontal(&frame);
        assert_eq!(pixel(&mirrored, 0, 0), pixel(&frame, 3, 0));
        assert_eq!(pixel(&mirrored, 3, 0), pixel(&frame, 0, 0));

        // present_on_device kombiniert beides (2×4-Device, Stretch).
        route.rear_projection = true;
        route.scaling = RouteScaling::Stretch;
        let out = present_on_device(&frame, &route, 2, 4);
        assert_eq!((out.width, out.height), (2, 4));
        // Erst gedreht, dann gespiegelt.
        let rotated = rotate(&frame, ScreenRotation::Deg90);
        let mirrored = mirror_horizontal(&rotated);
        assert_eq!(pixel(&out, 0, 0), pixel(&mirrored, 0, 0));
        assert_eq!(pixel(&out, 1, 3), pixel(&mirrored, 1, 3));
    }

    #[test]
    fn two_stages_stack_by_layer_then_name() {
        // Zwei Stages auf derselben Route: Beta Layer 0 (blau, unten),
        // Alpha Layer 1 (rot, oben). Reihenfolge egal – render_route sortiert.
        let route = OutputRoute::new("R", 100, 100);
        let mut alpha_region = Region::full_stage(1920.0, 1080.0);
        alpha_region.route = Some(route.id);
        let mut beta_region = Region::full_stage(1920.0, 1080.0);
        beta_region.route = Some(route.id);
        let alpha = make_stage("Alpha", 1, vec![alpha_region.clone()]);
        let beta = make_stage("Beta", 0, vec![beta_region.clone()]);
        let frames: HashMap<String, Arc<VideoFrame>> = [
            ("Alpha".to_string(), solid(1920, 1080, [255, 0, 0])),
            ("Beta".to_string(), solid(1920, 1080, [0, 0, 255])),
        ]
        .into_iter()
        .collect();
        // Absichtlich falsch sortiert übergeben.
        let out = render_route(&route, &[alpha.clone(), beta.clone()], &frames);
        assert_eq!(pixel(&out, 50, 50), [255, 0, 0], "höherer Layer oben");

        // Gleicher Layer: alphabetisch früherer Name oben (Alpha über Beta).
        let alpha0 = make_stage("Alpha", 0, vec![alpha_region.clone()]);
        let beta0 = make_stage("Beta", 0, vec![beta_region.clone()]);
        let frames2: HashMap<String, Arc<VideoFrame>> = [
            ("Alpha".to_string(), solid(1920, 1080, [255, 0, 0])),
            ("Beta".to_string(), solid(1920, 1080, [0, 0, 255])),
        ]
        .into_iter()
        .collect();
        let out = render_route(&route, &[beta0, alpha0], &frames2);
        assert_eq!(pixel(&out, 50, 50), [255, 0, 0], "Alpha über Beta");

        // Unsichtbare Stage wird nicht gemalt.
        let mut hidden = make_stage("Hidden", 5, vec![alpha_region]);
        hidden.visible = false;
        let out = render_route(&route, &[hidden], &frames2);
        assert_eq!(pixel(&out, 50, 50), [0, 0, 0], "unsichtbar = schwarz");
    }

    #[test]
    fn stage_preview_shows_the_same_test_pattern_as_the_output() {
        let mut route = OutputRoute::new("Display 1", 1920, 1080);
        route.test_pattern = true;
        route.color = [255, 80, 0];
        let mut region = Region::full_stage(1920.0, 1080.0);
        region.route = Some(route.id);
        let stage = make_stage("S", 0, vec![region]);
        let frame = solid(1920, 1080, [255, 0, 0]);
        let preview = stage_preview(&stage, &frame, std::slice::from_ref(&route), 960, 540);
        let mut expected = vec![0u8; 960 * 540 * 4];
        let spec = crate::core::TestPatternSpec {
            label: route.label(),
            width: 960,
            height: 540,
            kind: route.test_pattern_kind,
            color: route.color,
            index: 0,
            checker_size: route.checker_size,
        };
        super::super::pattern::draw_test_pattern(&mut expected, 960, 540, &spec);
        assert!(preview.rgba == expected, "Vorschau = Testbild des Ausgangs");
    }

    #[test]
    fn stage_preview_draws_footer_grid_with_label() {
        let stage = {
            let mut s = make_stage("Stage 1", 0, Vec::new());
            s.grid = true;
            s
        };
        let frame = solid(1920, 1080, [0, 0, 0]);
        let preview = stage_preview(&stage, &frame, &[], 960, 540);
        let mut expected = vec![0u8; 960 * 540 * 4];
        super::super::pattern::draw_region_grid(&mut expected, 960, 540, "Stage 1", [255; 3]);
        assert!(
            preview.rgba == expected,
            "Footer-Grid = volles Testbild in Weiß"
        );
    }

    #[test]
    fn route_test_pattern_stays_round_on_differently_shaped_stage() {
        // Route 800×600 (4:3), Stage 1920×1080 (16:9): Das Testbild wird
        // im Raster der Route gezeichnet, der Kreis bleibt rund.
        let mut route = OutputRoute::new("T", 800, 600);
        route.test_pattern = true;
        // Grid samt Kreisen liegt in Routenfarbe – hier Weiß (klassisches
        // Testbild): Nur der Kreis in voller Farbe trifft die Helligkeitssuche,
        // die Rasterlinien bleiben gedimmt.
        let mut region = Region::full_stage(1920.0, 1080.0);
        region.route = Some(route.id);
        let stage = make_stage("S", 0, vec![region]);
        let frames: HashMap<String, Arc<VideoFrame>> =
            std::iter::once(("S".to_string(), solid(1920, 1080, [255, 0, 0]))).collect();
        let out = render_route(&route, std::slice::from_ref(&stage), &frames);
        assert_eq!((out.width, out.height), (800, 600));
        assert!(
            !out.rgba
                .chunks_exact(4)
                .any(|c| c[0] == 255 && c[1] == 0 && c[2] == 0 && c[3] == 255),
            "Testbild überdeckt den Inhalt (kein reines Rot)"
        );

        // Kreis: Mittelpunkt (399.5, 299.5), Radius 298.5. Der am weitesten
        // rechts bzw. unten liegende helle Kreis-Pixel muss von der Mitte
        // gleich weit entfernt sein (±2 px) – rund, nicht oval.
        let bright = |x: u32, y: u32| {
            let i = (y as usize * 800 + x as usize) * 4;
            out.rgba[i] > 240 && out.rgba[i + 1] > 240 && out.rgba[i + 2] > 240
        };
        // Zeile 299 (nicht auf einer Rasterlinie): hellster Kreis-Punkt.
        // Bildrand (immer weiß) und Skalenstriche ausschließen: bis
        // wenige Pixel vor dem Rand scannen.
        let mut right_edge = 0u32;
        for x in (400..=794).rev() {
            if bright(x, 299) {
                right_edge = x;
                break;
            }
        }
        let mut bottom_edge = 0u32;
        for y in (300..=598).rev() {
            if bright(401, y) {
                bottom_edge = y;
                break;
            }
        }
        assert!(right_edge > 600, "Kreis gefunden (rechts), x={right_edge}");
        assert!(bottom_edge > 500, "Kreis gefunden (unten), y={bottom_edge}");
        let dx = f(right_edge) - 399.5;
        let dy = f(bottom_edge) - 299.5;
        assert!((dx - dy).abs() <= 2.0, "Kreis ist rund: dx={dx}, dy={dy}");
    }

    #[test]
    fn region_grid_overlays_only_that_region() {
        // Region mit Grid auf einer Route von zwei Regionen der Stage:
        // nur die eigene Route bekommt das Raster (andere bleibt Inhalt).
        let (mut a, mut b) = two_regions(0.0);
        let route_a = OutputRoute::new("L", 1200, 1080);
        let route_b = OutputRoute::new("R", 1200, 1080);
        a.route = Some(route_a.id);
        a.grid = true;
        b.route = Some(route_b.id);
        let mut stage = Stage::new("S", StageGeometry::new(2400.0, 1080.0));
        stage.regions = vec![a, b];
        let frames: HashMap<String, Arc<VideoFrame>> =
            std::iter::once(("S".to_string(), solid(2400, 1080, [10, 20, 30]))).collect();
        let left = render_route(&route_a, std::slice::from_ref(&stage), &frames);
        let right = render_route(&route_b, std::slice::from_ref(&stage), &frames);
        // Linke Route: Raster (weißer Rahmen am Rand).
        assert!(pixel(&left, 0, 0)[0] > 200, "Grid-Rahmen links oben");
        // Rechte Route: unveränderter Inhalt.
        assert_eq!(pixel(&right, 5, 5), [10, 20, 30]);
    }

    #[test]
    fn auto_blends_vertical_and_multiple_overlaps() {
        // Vertikale Überlappung (oben/unten).
        let a = Region::new(0.0, 0.0, 100.0, 150.0);
        let b = Region::new(0.0, 100.0, 100.0, 150.0);
        let blends = auto_blends(&[a, b]);
        assert_eq!(blends[0].bottom, 50.0);
        assert_eq!(blends[1].top, 50.0);
        assert_eq!(blends[0].right, 0.0, "keine horizontale Überlappung");
        // Drei Regionen nebeneinander: A-B überlappen 100, A-C nur 20,
        // B-C überlappen 120 → je Kante gewinnt die breiteste Überlappung.
        let a = Region::new(0.0, 0.0, 300.0, 100.0);
        let b = Region::new(200.0, 0.0, 200.0, 100.0);
        let c = Region::new(280.0, 0.0, 220.0, 100.0);
        let blends = auto_blends(&[a.clone(), b, c]);
        assert_eq!(
            blends[0].right, 100.0,
            "breiteste Überlappung gewinnt (100 > 20)"
        );
        assert_eq!(blends[1].left, 100.0);
        assert_eq!(blends[1].right, 120.0);
        assert_eq!(blends[2].left, 120.0);
        // Voll ineinander geschachtelte Region: keine Blende.
        let outer = Region::new(0.0, 0.0, 400.0, 300.0);
        let inner = Region::new(100.0, 50.0, 100.0, 100.0);
        let blends = auto_blends(&[outer, inner]);
        assert_eq!(blends[0], EdgeBlend::ZERO);
        assert_eq!(blends[1], EdgeBlend::ZERO);
        // Keine Überlappung → keine Blende.
        let a = Region::new(0.0, 0.0, 100.0, 100.0);
        let b = Region::new(100.0, 0.0, 100.0, 100.0);
        let blends = auto_blends(&[a, b]);
        assert_eq!(blends[0], EdgeBlend::ZERO);
        assert_eq!(blends[1], EdgeBlend::ZERO);
    }

    #[test]
    fn render_region_into_maps_one_to_one_without_distortion() {
        // Region-Größe == Route-Größe: Pixel-identisch (keine Verzerrung).
        let frame = solid(100, 50, [1, 2, 3]);
        let region = Region::new(0.0, 0.0, 100.0, 50.0);
        let mut target = VideoFrame::black(100, 50);
        render_region_into(
            &mut target,
            100,
            50,
            &frame,
            &region,
            &EdgeBlend::ZERO,
            2.2,
            None,
            &EdgeBlend::ZERO,
            true,
            false,
        );
        assert_eq!(target.rgba, frame.rgba);
    }
}

#[cfg(test)]
mod stage_crop_edge_tests {
    use super::*;
    use crate::core::video_routing::{EdgeBlend, OutputRoute, Region};
    use crate::core::{Crop, Stage, StageGeometry};

    fn solid(w: u32, h: u32, color: [u8; 3]) -> Arc<VideoFrame> {
        let [r, g, b] = color;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        Arc::new(VideoFrame::new(w, h, rgba))
    }

    fn pixel(frame: &VideoFrame, x: u32, y: u32) -> u8 {
        let i = (y as usize * frame.width as usize + x as usize) * 4;
        frame.rgba[i]
    }

    #[test]
    fn stage_crop_limits_the_visible_window() {
        // Stage: linke Haelfte rot, rechte blau. Crop auf die linke Haelfte:
        // Die Region (ueber die ganze Stage) zeigt nur noch den Crop-Inhalt.
        let mut stage = Stage::new("S", StageGeometry::new(200.0, 100.0));
        let route = OutputRoute::new("R", 100, 100);
        let mut region = Region::full_stage(200.0, 100.0);
        region.route = Some(route.id);
        stage.regions = vec![region];
        stage.crop = Some(Crop {
            left: 0.0,
            top: 0.0,
            right: 0.5,
            bottom: 1.0,
        });
        let frame = solid(200, 100, [255, 0, 0]);
        let mut target = VideoFrame::black(100, 100);
        render_region_into(
            &mut target,
            100,
            100,
            &frame,
            &stage.regions[0],
            &EdgeBlend::ZERO,
            2.2,
            Some((0.0, 0.0, 100.0, 100.0)),
            &EdgeBlend::ZERO,
            true,
            false,
        );
        assert!(pixel(&target, 50, 50) > 200, "Crop zeigt die rote Haelfte");
    }

    #[test]
    fn stage_crop_outside_region_renders_nothing() {
        // Crop-Deckel liegt vollstaendig neben der Region: Route bleibt schwarz.
        let mut stage = Stage::new("S", StageGeometry::new(400.0, 100.0));
        let route = OutputRoute::new("R", 100, 100);
        let mut region = Region::new(0.0, 0.0, 100.0, 100.0);
        region.route = Some(route.id);
        stage.regions = vec![region];
        let frame = solid(400, 100, [255, 0, 0]);
        let mut target = VideoFrame::black(100, 100);
        render_region_into(
            &mut target,
            100,
            100,
            &frame,
            &stage.regions[0],
            &EdgeBlend::ZERO,
            2.2,
            Some((300.0, 0.0, 400.0, 100.0)),
            &EdgeBlend::ZERO,
            true,
            false,
        );
        assert_eq!(pixel(&target, 50, 50), 0, "Crop daneben = schwarz");
    }

    #[test]
    fn stage_edge_fades_borders_on_route() {
        // Weisse Stage, Stage-Edge rechts 100 px (Hard Edge = linear):
        // Am rechten Rand Faktor ~0, 100 px davor ~1.
        let mut stage = Stage::new("S", StageGeometry::new(200.0, 100.0));
        stage.edge = EdgeBlend {
            right: 100.0,
            ..EdgeBlend::ZERO
        };
        stage.soft_edge = false;
        let route = OutputRoute::new("R", 200, 100);
        let mut region = Region::full_stage(200.0, 100.0);
        region.route = Some(route.id);
        stage.regions = vec![region];
        let frame = solid(200, 100, [255, 255, 255]);
        let mut target = VideoFrame::black(200, 100);
        render_region_into(
            &mut target,
            200,
            100,
            &frame,
            &stage.regions[0],
            &EdgeBlend::ZERO,
            2.2,
            None,
            &stage.edge,
            false,
            false,
        );
        // x=0: volle Helligkeit; x=190 (10 px vor dem Rand): Faktor ~0.1.
        assert!(pixel(&target, 5, 50) > 240, "links unveraendert");
        assert!(
            pixel(&target, 190, 50) < 40,
            "rechter Rand abgeblendet, got {}",
            pixel(&target, 190, 50)
        );
        // Soft Edge (Gamma-Kurve): Mitte der Blende ist heller als linear.
        let mut soft_target = VideoFrame::black(200, 100);
        render_region_into(
            &mut soft_target,
            200,
            100,
            &frame,
            &stage.regions[0],
            &EdgeBlend::ZERO,
            2.2,
            None,
            &stage.edge,
            true,
            false,
        );
        assert!(
            pixel(&soft_target, 150, 50) > pixel(&target, 150, 50),
            "Soft Edge liegt ueber Hard Edge"
        );
    }

    #[test]
    fn key_route_preserves_alpha_fill_route_flattens() {
        // Stage-Frame: Weiß mit Alpha 128 (halbtransparentes Overlay).
        let frame = Arc::new(VideoFrame::new(4, 4, [255, 255, 255, 128].repeat(16)));
        let mut stage = Stage::new("S", StageGeometry::new(4.0, 4.0));
        let mut region = Region::full_stage(4.0, 4.0);
        let route = OutputRoute::new("R", 4, 4);
        region.route = Some(route.id);
        stage.regions = vec![region];

        let mut frames = HashMap::new();
        frames.insert("S".to_string(), frame);

        // Key-Route (alpha = true): Alpha bleibt erhalten.
        let mut key_route = OutputRoute::new("K", 4, 4);
        key_route.alpha = true;
        let mut kstage = stage.clone();
        kstage.regions[0].route = Some(key_route.id);
        let out = render_route(&key_route, &[kstage], &frames);
        assert_eq!(
            &out.rgba[..4],
            &[255, 255, 255, 128],
            "Key: Alpha 128 bleibt"
        );

        // Fill-Route (alpha = false): Alpha flach auf 255, aber RGB mit
        // effektiver Deckkraft (Quell-Alpha × Faktor) gemischt (Review C8) –
        // halbtransparentes Weiß auf Schwarz ergibt Grau 128, nicht Weiß.
        let mut fstage = stage.clone();
        fstage.regions[0].route = Some(route.id);
        let out = render_route(&route, &[fstage], &frames);
        assert_eq!(
            &out.rgba[..4],
            &[128, 128, 128, 255],
            "Fill: Alpha flach 255, RGB = 255×(128/255) ≈ 128"
        );
    }

    #[test]
    fn c8_fill_route_uses_source_alpha_not_opaque_copy() {
        // 25 % Opacity: Canvas-Pixel [255,255,255,64] → Fill-Route über Schwarz
        // muss RGB ≈ 64 liefern (vorher: 255, weil Alpha ignoriert wurde).
        let frame = Arc::new(VideoFrame::new(
            2,
            2,
            vec![255, 255, 255, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ));
        let mut stage = Stage::new("S", StageGeometry::new(2.0, 2.0));
        let mut region = Region::full_stage(2.0, 2.0);
        let route = OutputRoute::new("R", 2, 2);
        region.route = Some(route.id);
        stage.regions = vec![region];
        let mut frames = HashMap::new();
        frames.insert("S".to_string(), frame);
        let out = render_route(&route, &[stage], &frames);
        let p0 = &out.rgba[..4];
        assert_eq!(p0[3], 255, "Fill-Alpha bleibt 255");
        assert!(
            (p0[0] as i32 - 64).abs() <= 2,
            "RGB muss 255×(64/255)≈64 sein, got {}",
            p0[0]
        );
        // Transparenter Quell-Pixel (Alpha 0): wird übersprungen, Ziel bleibt
        // deckendes Schwarz des Fill-Hintergrunds (vorher: RGB kopiert, egal).
        assert_eq!(&out.rgba[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn c8_transparent_upper_stage_does_not_cover_lower() {
        // Untere Stage rot (deckend), obere Stage komplett transparent:
        // Fill-Route muss Rot zeigen (vorher: Schwarz, Alpha wurde auf 255 gesetzt).
        let lower = Arc::new(VideoFrame::new(2, 2, [255, 0, 0, 255].repeat(4)));
        let upper = Arc::new(VideoFrame::new(2, 2, [0, 0, 0, 0].repeat(4)));
        let mut s1 = Stage::new("lower", StageGeometry::new(2.0, 2.0));
        s1.layer = 0;
        let mut r1 = Region::full_stage(2.0, 2.0);
        let route = OutputRoute::new("R", 2, 2);
        r1.route = Some(route.id);
        s1.regions = vec![r1];
        let mut s2 = Stage::new("upper", StageGeometry::new(2.0, 2.0));
        s2.layer = 1;
        let mut r2 = Region::full_stage(2.0, 2.0);
        r2.route = Some(route.id);
        s2.regions = vec![r2];
        let mut frames = HashMap::new();
        frames.insert("lower".to_string(), lower);
        frames.insert("upper".to_string(), upper);
        let out = render_route(&route, &[s1, s2], &frames);
        assert_eq!(
            &out.rgba[..4],
            &[255, 0, 0, 255],
            "transparente obere Stage darf untere rote nicht verdecken"
        );
    }
}
