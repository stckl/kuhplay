//! Testbild-Renderer („Test Pattern").
//!
//! Das Testbild wird in der **Auflösung des Screens** gezeichnet, nicht in der
//! der Stage (siehe [`super::compositor`]). Ein Pixel im Bild ist also ein Pixel
//! am Ausgang, und der Kreis ist am Ausgang rund. Erscheint er auf der Leinwand
//! als Ellipse, verzerrt die Darstellung dahinter (Display, Projektor, Scaler).
//!
//! Alle Arten teilen dieselben Elemente: 1 px weißer Rand exakt am Bildrand,
//! 1 px Kreis in der Mitte und die Beschriftung im weißen Kasten.
//!
//! * **Grid** (Aufbau wie das Ausrichtungsraster): in der Screen-/Routenfarbe
//!   auf Schwarz (Standard Weiß), feines Raster mit Skala am Rand, Mittelachsen,
//!   Diagonalen und Kreise in den Ecken. Alles 1 px breit.
//! * **Solid Color**: Fläche in der Screen-Farbe.
//! * **Color Bars**: SMPTE-Balken.
//! * **LED Wall**: nummerierte Kacheln und Pixelskala.
//!
//! Das Testbild **überdeckt die Inhalte**. Die Drehung des Screens wird erst beim
//! Anzeigen angewendet. Texte nutzen die Ubuntu-Schrift, die egui mitbringt.
//!
//! **Wasserzeichen:** Die Kuh aus `kuh.svg` (einmalig gerastert, siehe
//! [`super::kuh_mask`]) liegt in der Screen-/Routenfarbe mit 25 % Deckkraft
//! **hinter** dem Grid – Reihenfolge: Hintergrund → Kuh → Grid-Linien/Kreis →
//! Beschriftung. Im normalen Bild (ohne Testbild/Grid) erscheint keine Kuh.
//!
//! **Testbild-Grid (bewusst so, nicht eigenmächtig ändern):** Ausrichtungsraster
//! in der Ausgangs-/Routenfarbe mit 1 px Linien, Mittelachsen, Diagonalen,
//! Eckenkreisen, Skala, weißem 1 px Rahmen, Mittelkreis und Beschriftung –
//! plus Kuh-Wasserzeichen im Hintergrund. Bewährt im Live-Betrieb (Ausrichten
//! von Projektoren/LED-Wänden); Änderungen nur nach Absprache.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

use super::kuh_mask::kuh_mask;
use crate::core::{TestPatternKind, TestPatternSpec};

/// Deckkraft des Kuh-Wasserzeichens (25 %, gut sichtbar hinter dem Grid).
const WATERMARK_STRENGTH: f32 = 0.25;

/// Größe der Kuh relativ zur kürzeren Canvas-Kante (~55 %).
const WATERMARK_REL_SIZE: f32 = 0.55;

/// Die Schrift aller Testbild-Texte.
fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| {
        FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
            .expect("mitgelieferte Schrift ist gültig")
    })
}

/// Breite und Höhe eines Textes in Pixeln.
fn measure_text(text: &str, px: f32) -> (f32, f32) {
    let scaled = font().as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut previous = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(previous) = previous {
            width += scaled.kern(previous, id);
        }
        width += scaled.h_advance(id);
        previous = Some(id);
    }
    (width, scaled.ascent() - scaled.descent())
}

/// Zeichnet Text kantengeglättet, `(x, y)` ist die obere linke Ecke.
#[allow(clippy::too_many_arguments)] // Zeichen-Helfer: alle Parameter fachlich nötig
fn draw_text(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    x: f32,
    y: f32,
    text: &str,
    px: f32,
    color: (u8, u8, u8),
) {
    let scaled = font().as_scaled(PxScale::from(px));
    let baseline = y + scaled.ascent();
    let mut pen = x;
    let mut previous = None;
    for ch in text.chars() {
        let id = scaled.glyph_id(ch);
        if let Some(previous) = previous {
            pen += scaled.kern(previous, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(pen, baseline));
        if let Some(outline) = font().outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px_x = bounds.min.x as i64 + i64::from(gx);
                let px_y = bounds.min.y as i64 + i64::from(gy);
                if px_x < 0 || px_y < 0 || px_x >= w as i64 || px_y >= h as i64 {
                    return;
                }
                let i = (px_y as usize * w + px_x as usize) * 4;
                let coverage = coverage.clamp(0.0, 1.0);
                for (channel, target) in [color.0, color.1, color.2].into_iter().enumerate() {
                    let current = f32::from(rgba[i + channel]);
                    rgba[i + channel] =
                        (current * (1.0 - coverage) + f32::from(target) * coverage) as u8;
                }
                rgba[i + 3] = 255;
            });
        }
        pen += scaled.h_advance(id);
        previous = Some(id);
    }
}

/// Grid für **eine Region** bzw. die ganze Stage („Region Grid“, Footer-Grid):
/// exakt dasselbe Bild wie das Grid-Testbild einer Route – Raster, Diagonalen,
/// Eck- und Mittelkreis, Skala, Rahmen, Beschriftung, Kuh dahinter – nur mit
/// eigenem Text und eigener Farbe (Stage-Grid: Weiß).
pub fn draw_region_grid(rgba: &mut [u8], width: u32, height: u32, label: &str, color: [u8; 3]) {
    let spec = TestPatternSpec {
        label: label.to_string(),
        width,
        height,
        kind: TestPatternKind::Grid,
        color,
        index: 0,
        checker_size: 64,
    };
    draw_test_pattern(rgba, width, height, &spec);
}

/// Zeichnet das Testbild vollständig in `rgba` (Größe `width`×`height`,
/// Row-Major, 4 Bytes/Pixel). Reihenfolge: Hintergrund → Kuh-Wasserzeichen
/// (Screen-Farbe) → Grid-Linien/Kreis → Beschriftung → Rand.
pub fn draw_test_pattern(rgba: &mut [u8], width: u32, height: u32, spec: &TestPatternSpec) {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    assert_eq!(
        rgba.len(),
        w * h * 4,
        "rgba buffer must match width*height*4"
    );

    // 1) Hintergrund je nach Testbild-Art.
    match spec.kind {
        TestPatternKind::Grid => draw_solid_color(rgba, w, h, [0, 0, 0]),
        TestPatternKind::SolidColor => draw_solid_color(rgba, w, h, spec.color),
        TestPatternKind::ColorBars => draw_color_bars(rgba, w, h),
        TestPatternKind::LedWall => draw_led_wall(rgba, w, h, spec),
    }
    // 2) Kuh-Wasserzeichen hinter Grid/Linien. Auf Solid Color in derselben
    //    Farbe leicht aufgehellt (helle Farben: abgedunkelt), sonst wäre die
    //    Kuh unsichtbar.
    let logo_color = match spec.kind {
        TestPatternKind::SolidColor => contrast_tint(spec.color, 0.30),
        _ => spec.color,
    };
    draw_logo(rgba, w, h, logo_color, WATERMARK_STRENGTH);
    // 3) Grid-Linien (in Screen-Farbe) bzw. Pixelskala ÜBER der Kuh.
    match spec.kind {
        TestPatternKind::Grid => {
            // VOLLSTÄNDIGES Ausrichtungsraster in der Ausgangs-/Routenfarbe
            // (1 px Linien, Mittelachsen, Diagonalen, Eckenkreise, Skala) –
            // dazu Kuh-Wasserzeichen Dahinter, dann Kreis, Beschriftung, Rand.
            // Nicht „vereinfachen“ – siehe Doku-Hinweis im Modul-Header.
            let (dim, major, diag, axis, scale) = grid_shades_of(spec.color);
            grid_lines(rgba, w, h, dim, major, diag, axis, scale);
        }
        TestPatternKind::LedWall => {
            let cell = (spec.checker_size.max(8) as usize).min(w.max(h));
            draw_pixel_scale(rgba, w, h, cell, (255, 255, 255));
        }
        _ => {}
    }
    // 4) Kreis, Beschriftung und Rand ganz oben (lesbar auf jedem Grund).
    //    Grid: Kreis und Rand in der Screen-/Routenfarbe (Weiß = klassisches
    //    Testbild); sonst weiß, damit sie auf jeder Fläche sichtbar bleiben.
    let accent = accent_color(spec);
    draw_circle(rgba, w, h, accent);
    draw_label(rgba, w, h, &spec.label);
    draw_frame(rgba, w, h, accent);
}

/// Farbe von Mittelkreis und Rand: beim Grid die Screen-/Routenfarbe, sonst Weiß.
fn accent_color(spec: &TestPatternSpec) -> [u8; 3] {
    match spec.kind {
        TestPatternKind::Grid => spec.color,
        _ => [255; 3],
    }
}

/// Mischt `color` um `f` Richtung Weiß, bei hellen Farben Richtung Schwarz
/// (Kuh auf Solid Color in derselben Farbe – Weiß aufgehellt bliebe Weiß).
fn contrast_tint(color: [u8; 3], f: f32) -> [u8; 3] {
    let f = f.clamp(0.0, 1.0);
    let luma = 0.2126 * color[0] as f32 + 0.7152 * color[1] as f32 + 0.0722 * color[2] as f32;
    let target = if luma > 160.0 { 0.0 } else { 255.0 };
    color.map(|c| (c as f32 + (target - c as f32) * f).round() as u8)
}

/// Zeichnet die Kuh als Wasserzeichen: zentriert, `WATERMARK_REL_SIZE` der
/// kürzeren Kante, Deckkraft `strength` (0..1), in `color`. Die Form kommt
/// als Alpha-Maske aus `kuh.svg` (einmalig gerastert, siehe [`kuh_mask`]).
fn draw_logo(rgba: &mut [u8], w: usize, h: usize, color: [u8; 3], strength: f32) {
    let mask = kuh_mask();
    let (mw, mh) = (mask.width as usize, mask.height as usize);
    let target = ((w.min(h) as f32 * WATERMARK_REL_SIZE) as usize).max(8);
    let scale = target as f32 / mw.max(mh) as f32;
    let dw = ((mw as f32 * scale) as usize).max(1);
    let dh = ((mh as f32 * scale) as usize).max(1);
    let x0 = (w.saturating_sub(dw)) / 2;
    let y0 = (h.saturating_sub(dh)) / 2;
    let (cr, cg, cb) = (color[0] as f32, color[1] as f32, color[2] as f32);
    let strength = strength.clamp(0.0, 1.0);
    for dy in 0..dh {
        let sy = (dy as f32 / dh as f32 * mh as f32) as usize;
        for dx in 0..dw {
            let sx = (dx as f32 / dw as f32 * mw as f32) as usize;
            let a = mask.alpha[sy * mw + sx] as f32 / 255.0 * strength;
            if a <= 0.0 {
                continue;
            }
            let di = ((y0 + dy) * w + (x0 + dx)) * 4;
            let dst = &mut rgba[di..di + 4];
            dst[0] = (cr * a + dst[0] as f32 * (1.0 - a)).min(255.0) as u8;
            dst[1] = (cg * a + dst[1] as f32 * (1.0 - a)).min(255.0) as u8;
            dst[2] = (cb * a + dst[2] as f32 * (1.0 - a)).min(255.0) as u8;
            dst[3] = 255;
        }
    }
}

/// LED-Wand-Testbild: Schachbrettmuster mit durchnummerierten Kacheln und
/// Pixelskala. Hintergrund: Schachbrett leicht in der Screen-Farbe eingefärbt
/// (abgedunkelt – nicht grell).
fn draw_led_wall(rgba: &mut [u8], w: usize, h: usize, spec: &TestPatternSpec) {
    let cell = (spec.checker_size.max(8) as usize).min(w.max(h));
    let (scr_r, scr_g, scr_b) = (spec.color[0], spec.color[1], spec.color[2]);
    // Schachbrett: Screen-Farbe abgedunkelt (70%) und stark abgedunkelt (18%).
    // Ergibt ein dezentes Muster in der Screen-Farbe, nicht grell.
    let (on_r, on_g, on_b) = (
        (scr_r as u16 * 70 / 100) as u8,
        (scr_g as u16 * 70 / 100) as u8,
        (scr_b as u16 * 70 / 100) as u8,
    );
    let (off_r, off_g, off_b) = (
        (scr_r as u16 * 18 / 100) as u8,
        (scr_g as u16 * 18 / 100) as u8,
        (scr_b as u16 * 18 / 100) as u8,
    );

    // Schachbrett mit durchnummerierten Kacheln.
    let cols = w.div_ceil(cell);
    let rows = h.div_ceil(cell);
    for row in 0..rows {
        for col in 0..cols {
            let x0 = col * cell;
            let y0 = row * cell;
            let x1 = (x0 + cell).min(w);
            let y1 = (y0 + cell).min(h);
            let on = (row + col).is_multiple_of(2);
            let (r, g, b) = if on {
                (on_r, on_g, on_b)
            } else {
                (off_r, off_g, off_b)
            };
            // Kachel füllen.
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    rgba[i] = r;
                    rgba[i + 1] = g;
                    rgba[i + 2] = b;
                    rgba[i + 3] = 255;
                }
            }
            // Kachel-Nummer zentriert (laufende Nummer ab 1).
            let num = row * cols + col + 1;
            let label = num.to_string();
            // Textfarbe: kontrastreich zur Kachel (weiß auf dunkler Kachel,
            // schwarz auf heller Kachel – aber beides in Screen-Farbe getönt).
            let (tr, tg, tb) = if on {
                // Helle Kachel → dunkler Text (Screen-Farbe, fast schwarz)
                (
                    (scr_r as u16 * 10 / 100) as u8,
                    (scr_g as u16 * 10 / 100) as u8,
                    (scr_b as u16 * 10 / 100) as u8,
                )
            } else {
                // Dunkle Kachel → heller Text (Screen-Farbe, fast weiß)
                (255u8, 255u8, 255u8)
            };
            draw_label_in_rect(rgba, w, h, x0, y0, x1, y1, &label, (tr, tg, tb));
        }
    }

    // Pixelskala umlaufend am Rand zeichnet draw_test_pattern NACH der Kuh.
}

/// Zeichnet eine umlaufende Pixelskala am Rand des Canvas.
/// Markierungen alle `step` Pixel (groß mit Zahl), dazwischen klein.
/// Oben/unten: horizontale Skala (x-Position).
/// Links/rechts: vertikale Skala (y-Position).
fn draw_pixel_scale(rgba: &mut [u8], w: usize, h: usize, step: usize, color: (u8, u8, u8)) {
    let step = step.max(16);
    let tick_large = 8usize; // Große Markierung (mit Zahl)
    let tick_small = 4usize; // Kleine Markierung (ohne Zahl)
    let half = step / 2;

    // --- Oben (y=1..tick_large+1) ---
    let mut x = 0usize;
    while x < w {
        let is_large = x.is_multiple_of(step);
        let len = if is_large { tick_large } else { tick_small };
        for yy in 1..=len.min(12) {
            if x < w && yy < h {
                let i = (yy * w + x) * 4;
                rgba[i] = color.0;
                rgba[i + 1] = color.1;
                rgba[i + 2] = color.2;
                rgba[i + 3] = 255;
            }
        }
        // Zahl bei großen Markierungen (oberhalb des Canvas nicht möglich,
        // also innerhalb: y=2..9).
        if is_large && x > 0 {
            let label = x.to_string();
            draw_label_at(rgba, w, h, x as isize + 2, 2, &label, color);
        }
        x += if is_large { step } else { half };
    }

    // --- Unten (y=h-1-len..h-1) ---
    x = 0;
    while x < w {
        let is_large = x.is_multiple_of(step);
        let len = if is_large { tick_large } else { tick_small };
        for yy in 0..len.min(12) {
            let y = h - 2 - yy;
            if x < w && y < h {
                let i = (y * w + x) * 4;
                rgba[i] = color.0;
                rgba[i + 1] = color.1;
                rgba[i + 2] = color.2;
                rgba[i + 3] = 255;
            }
        }
        if is_large && x > 0 {
            let label = x.to_string();
            // Text von rechts nach links zeichnen (damit er nicht über den Rand läuft).
            let text_w = label.chars().count() * 6;
            let tx = (x as isize - text_w as isize - 2).max(0);
            draw_label_at(rgba, w, h, tx, (h - 10) as isize, &label, color);
        }
        x += if is_large { step } else { half };
    }

    // --- Links (x=1..tick_large+1) ---
    let mut y = 0usize;
    while y < h {
        let is_large = y.is_multiple_of(step);
        let len = if is_large { tick_large } else { tick_small };
        for xx in 1..=len.min(12) {
            if xx < w && y < h {
                let i = (y * w + xx) * 4;
                rgba[i] = color.0;
                rgba[i + 1] = color.1;
                rgba[i + 2] = color.2;
                rgba[i + 3] = 255;
            }
        }
        if is_large && y > 0 {
            let label = y.to_string();
            draw_label_at(rgba, w, h, 2, y as isize + 2, &label, color);
        }
        y += if is_large { step } else { half };
    }

    // --- Rechts (x=w-1-len..w-1) ---
    y = 0;
    while y < h {
        let is_large = y.is_multiple_of(step);
        let len = if is_large { tick_large } else { tick_small };
        for xx in 0..len.min(12) {
            let x = w - 2 - xx;
            if x < w && y < h {
                let i = (y * w + x) * 4;
                rgba[i] = color.0;
                rgba[i + 1] = color.1;
                rgba[i + 2] = color.2;
                rgba[i + 3] = 255;
            }
        }
        if is_large && y > 0 {
            let label = y.to_string();
            let text_w = label.chars().count() * 6;
            let tx = (w as isize - text_w as isize - 4).max(0);
            draw_label_at(rgba, w, h, tx, y as isize + 2, &label, color);
        }
        y += if is_large { step } else { half };
    }
}

/// Zeichnet die kleinen Zahlen der Pixelskala an einer festen Position.
fn draw_label_at(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    x0: isize,
    y0: isize,
    text: &str,
    color: (u8, u8, u8),
) {
    draw_text(rgba, w, h, x0 as f32, y0 as f32 - 2.0, text, 11.0, color);
}

/// Zeichnet Text zentriert in einem Rechteck (für Kachel-Nummerierung).
#[allow(clippy::too_many_arguments)] // Zeichen-Helfer: alle Parameter fachlich nötig
fn draw_label_in_rect(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    text: &str,
    color: (u8, u8, u8),
) {
    let rect_w = (x1 - x0) as isize;
    let rect_h = (y1 - y0) as isize;
    if rect_w < 8 || rect_h < 8 {
        return; // Kachel zu klein für Text
    }
    // Text muss in die Kachel passen (höchstens 60 % der Kachel).
    let (unit_w, unit_h) = measure_text(text, 1.0);
    let px = (rect_w as f32 * 0.6 / unit_w)
        .min(rect_h as f32 * 0.6 / unit_h)
        .clamp(8.0, 64.0);
    let (text_w, text_h) = measure_text(text, px);
    let x = x0 as f32 + (rect_w as f32 - text_w) / 2.0;
    let y = y0 as f32 + (rect_h as f32 - text_h) / 2.0;
    draw_text(rgba, w, h, x, y, text, px, color);
}

/// Füllt das gesamte Canvas mit einer einfarbigen Fläche.
fn draw_solid_color(rgba: &mut [u8], _w: usize, _h: usize, color: [u8; 3]) {
    for px in rgba.chunks_exact_mut(4) {
        px[0] = color[0];
        px[1] = color[1];
        px[2] = color[2];
        px[3] = 255;
    }
}

/// SMPTE-Farbbalken: 7 vertikale Streifen (75% Sättigung).
fn draw_color_bars(rgba: &mut [u8], w: usize, h: usize) {
    // SMPTE 75% bars: Weiß, Gelb, Cyan, Grün, Magenta, Rot, Blau
    let bars: [[u8; 3]; 7] = [
        [191, 191, 191], // Grau (75% Weiß)
        [191, 191, 0],   // Gelb
        [0, 191, 191],   // Cyan
        [0, 191, 0],     // Grün
        [191, 0, 191],   // Magenta
        [191, 0, 0],     // Rot
        [0, 0, 191],     // Blau
    ];
    let bar_w = w / 7;
    for (i, bar) in bars.iter().enumerate() {
        let x0 = i * bar_w;
        let x1 = if i == 6 { w } else { (i + 1) * bar_w };
        for y in 0..h {
            for x in x0..x1 {
                let idx = (y * w + x) * 4;
                rgba[idx] = bar[0];
                rgba[idx + 1] = bar[1];
                rgba[idx + 2] = bar[2];
                rgba[idx + 3] = 255;
            }
        }
    }
}

// ---------------------------------------------------------------------
// Elemente
// ---------------------------------------------------------------------

/// Shades des Rasters: (minor, major, diagonal, axis, scale).
type GridShades = ([u8; 3], [u8; 3], [u8; 3], [u8; 3], [u8; 3]);

/// Shades des Rasters aus der Routen-/Screen-Farbe (Linien in der Layerfarbe).
fn grid_shades_of(color: [u8; 3]) -> GridShades {
    // Gleiche Helligkeitsstufen wie das klassische weiße Grid (15/35/60/80/100 %),
    // auf die Ausgangs-/Routenfarbe skaliert – nicht hart dividiert (zu dunkel).
    let scale = |f: u16| -> [u8; 3] {
        [
            (color[0] as u16 * f / 100) as u8,
            (color[1] as u16 * f / 100) as u8,
            (color[2] as u16 * f / 100) as u8,
        ]
    };
    let dim = scale(15);
    let major = scale(35);
    let diag = scale(60);
    let axis = scale(80);
    let scale_c = color;
    (dim, major, diag, axis, scale_c)
}

/// Nur die Rasterlinien (ohne Hintergrund-Füllung) – so zeichnet draw_test_pattern
/// das Grid ÜBER das Kuh-Wasserzeichen: kein Grid-Strich liegt unter der Kuh.
/// Quadratische Zellen von der Mitte aus (symmetrisch), jede fünfte Linie
/// heller, Skala am Rand, Diagonalen, Eckkreise.
#[allow(clippy::too_many_arguments)] // Zeichen-Helfer: Shades fachlich nötig
fn grid_lines(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    minor: [u8; 3],
    major: [u8; 3],
    diagonal: [u8; 3],
    axis: [u8; 3],
    scale: [u8; 3],
) {
    let cell = (w.min(h) / 40).max(8);
    let major_cell = cell * 5;
    let (cx, cy) = (w / 2, h / 2);
    let shade = |distance: usize| {
        if distance.is_multiple_of(major_cell) {
            major
        } else {
            minor
        }
    };
    for x in (0..w).filter(|x| x.abs_diff(cx).is_multiple_of(cell)) {
        for y in 0..h {
            put(rgba, w, x, y, shade(x.abs_diff(cx)));
        }
    }
    for y in (0..h).filter(|y| y.abs_diff(cy).is_multiple_of(cell)) {
        for x in 0..w {
            put(rgba, w, x, y, shade(y.abs_diff(cy)));
        }
    }
    // Diagonalen von Ecke zu Ecke.
    let steps = w.max(h);
    for step in 0..steps {
        let x = step * (w - 1) / steps.max(2).saturating_sub(1).max(1);
        let y = step * (h - 1) / steps.max(2).saturating_sub(1).max(1);
        put(rgba, w, x.min(w - 1), y.min(h - 1), diagonal);
        put(rgba, w, x.min(w - 1), (h - 1).saturating_sub(y), diagonal);
    }
    // Mittelachsen.
    for y in 0..h {
        put(rgba, w, cx, y, axis);
    }
    for x in 0..w {
        put(rgba, w, x, cy, axis);
    }
    // Skala: Striche am Rand, an den hellen Linien länger.
    let (long, short) = ((cell / 2).max(4), (cell / 4).max(2));
    for x in (0..w).filter(|x| x.abs_diff(cx).is_multiple_of(cell)) {
        let len = if x.abs_diff(cx).is_multiple_of(major_cell) {
            long
        } else {
            short
        };
        for offset in 0..len.min(h) {
            put(rgba, w, x, offset, scale);
            put(rgba, w, x, h - 1 - offset, scale);
        }
    }
    for y in (0..h).filter(|y| y.abs_diff(cy).is_multiple_of(cell)) {
        let len = if y.abs_diff(cy).is_multiple_of(major_cell) {
            long
        } else {
            short
        };
        for offset in 0..len.min(w) {
            put(rgba, w, offset, y, scale);
            put(rgba, w, w - 1 - offset, y, scale);
        }
    }
    // Kreise in den Ecken, an beiden Rändern anliegend.
    let radius = w.min(h) as f32 / 6.0;
    if radius >= 4.0 {
        let (right, bottom) = ((w - 1) as f32 - radius, (h - 1) as f32 - radius);
        for (x, y) in [
            (radius, radius),
            (right, radius),
            (radius, bottom),
            (right, bottom),
        ] {
            draw_circle_at(rgba, w, h, x, y, radius, scale);
        }
    }
}

/// Setzt ein deckendes Pixel.
fn put(rgba: &mut [u8], w: usize, x: usize, y: usize, color: [u8; 3]) {
    let i = (y * w + x) * 4;
    rgba[i..i + 3].copy_from_slice(&color);
    rgba[i + 3] = 255;
}

/// 1 px breiter weißer Rand exakt am Bildrand.
fn draw_frame(rgba: &mut [u8], w: usize, h: usize, color: [u8; 3]) {
    for x in 0..w {
        put(rgba, w, x, 0, color);
        put(rgba, w, x, h - 1, color);
    }
    for y in 0..h {
        put(rgba, w, 0, y, color);
        put(rgba, w, w - 1, y, color);
    }
}

/// 1 px breiter Kreis in der Mitte, kantengeglättet und immer rund.
fn draw_circle(rgba: &mut [u8], w: usize, h: usize, color: [u8; 3]) {
    // Pixelmitten: Der Mittelpunkt liegt bei gerader Größe zwischen zwei Pixeln.
    let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
    let radius = (w.min(h) as f32 - 1.0) / 2.0 - 1.0;
    if radius >= 4.0 {
        draw_circle_at(rgba, w, h, cx, cy, radius, color);
    }
}

/// Kreis um (`cx`, `cy`) in `color`. Der Abstand wird für jedes Pixel exakt
/// berechnet.
fn draw_circle_at(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    color: [u8; 3],
) {
    let mut blend = |x: usize, y: usize| {
        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
        let coverage = (1.0 - (d - radius).abs()).clamp(0.0, 1.0);
        let i = (y * w + x) * 4;
        for (c, target) in rgba[i..i + 3].iter_mut().zip(color) {
            *c = (f32::from(*c) * (1.0 - coverage) + f32::from(target) * coverage) as u8;
        }
        rgba[i + 3] = 255;
    };
    // Pro Zeile nur die beiden Ringstücke links und rechts der Mitte.
    for y in 0..h {
        let dy = y as f32 - cy;
        let outer = (radius + 1.0).powi(2) - dy * dy;
        if outer < 0.0 {
            continue;
        }
        let inner = ((radius - 1.0).powi(2) - dy * dy).max(0.0).sqrt();
        let outer = outer.sqrt();
        let left =
            ((cx - outer).floor().max(0.0) as usize)..=((cx - inner).ceil() as usize).min(w - 1);
        let last_left = *left.end();
        for x in left {
            blend(x, y);
        }
        let right_start = (((cx + inner).floor().max(0.0)) as usize).max(last_left + 1);
        for x in right_start..=((cx + outer).ceil() as usize).min(w - 1) {
            blend(x, y);
        }
    }
}

/// Zentrierte Beschriftung: schwarze Schrift im weißen Kasten, damit
/// sie auf jedem Hintergrund lesbar bleibt. Schriftgröße folgt dem Screen.
fn draw_label(rgba: &mut [u8], w: usize, h: usize, text: &str) {
    let (unit_w, unit_h) = measure_text(text, 1.0);
    let by_height = w.min(h) as f32 / 12.0 / unit_h;
    let by_width = w as f32 * 0.7 / unit_w.max(0.01);
    let px = by_height.min(by_width).max(8.0);
    let (text_w, text_h) = measure_text(text, px);
    let pad_x = (px * 0.6) as usize;
    let pad_y = (px * 0.25) as usize;
    let x0 = (w as f32 - text_w).max(0.0) / 2.0;
    let y0 = (h as f32 - text_h).max(0.0) / 2.0;
    let left = (x0 as usize).saturating_sub(pad_x);
    let top = (y0 as usize).saturating_sub(pad_y);
    let right = (x0 as usize + text_w.ceil() as usize + pad_x).min(w);
    let bottom = (y0 as usize + text_h.ceil() as usize + pad_y).min(h);
    for y in top..bottom {
        for x in left..right {
            let edge = y == top || y + 1 == bottom || x == left || x + 1 == right;
            put(rgba, w, x, y, if edge { [0; 3] } else { [255; 3] });
        }
    }
    draw_text(rgba, w, h, x0, y0, text, px, (0, 0, 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(label: &str, w: u32, h: u32) -> TestPatternSpec {
        TestPatternSpec {
            label: label.to_string(),
            width: w,
            height: h,
            kind: crate::core::TestPatternKind::Grid,
            color: [255, 255, 255],
            index: 0,
            checker_size: 64,
        }
    }

    fn px(rgba: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn frame_is_exactly_one_pixel_at_the_edge() {
        for (w, h) in [(1920usize, 1080usize), (1512, 982), (65, 41)] {
            let mut rgba = vec![0u8; w * h * 4];
            draw_solid_color(&mut rgba, w, h, [0, 0, 0]);
            let (dim, major, diag, axis, scale) = grid_shades_of([255; 3]);
            grid_lines(&mut rgba, w, h, dim, major, diag, axis, scale);
            draw_frame(&mut rgba, w, h, [255; 3]);
            let row = |y: usize| (0..w).filter(|x| rgba[(y * w + x) * 4] == 255).count();
            let col = |x: usize| (0..h).filter(|y| rgba[(y * w + x) * 4] == 255).count();
            assert_eq!(row(0), w, "oben durchgehend bei {w}x{h}");
            assert_eq!(row(h - 1), w, "unten durchgehend");
            assert_eq!(col(0), h, "links durchgehend");
            assert_eq!(col(w - 1), h, "rechts durchgehend");
            // Direkt daneben nur noch die Striche der Skala.
            assert!(row(1) < w / 2, "Rand oben ist 1 px breit");
            assert!(row(h - 2) < w / 2, "Rand unten ist 1 px breit");
            assert!(col(1) < h / 2, "Rand links ist 1 px breit");
            assert!(col(w - 2) < h / 2, "Rand rechts ist 1 px breit");
        }
    }

    #[test]
    fn circle_is_round_on_any_screen_shape() {
        for (w, h) in [
            (1920usize, 1080usize),
            (5120, 1440),
            (1512, 982),
            (1080, 1920),
            (801, 601),
        ] {
            let mut rgba = vec![0u8; w * h * 4];
            draw_circle(&mut rgba, w, h, [255; 3]);
            let lit: Vec<(usize, usize)> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|(x, y)| rgba[(y * w + x) * 4] > 127)
                .collect();
            let (x0, x1) = (
                lit.iter().map(|p| p.0).min().unwrap(),
                lit.iter().map(|p| p.0).max().unwrap(),
            );
            let (y0, y1) = (
                lit.iter().map(|p| p.1).min().unwrap(),
                lit.iter().map(|p| p.1).max().unwrap(),
            );
            assert_eq!(x1 - x0, y1 - y0, "gleich breit wie hoch bei {w}x{h}");
            assert_eq!(x0, w - 1 - x1, "waagerecht mittig bei {w}x{h}");
            assert_eq!(y0, h - 1 - y1, "senkrecht mittig bei {w}x{h}");
            // Lückenlos: Jede Zeile des Kreises trifft Pixel.
            for y in y0..=y1 {
                assert!(lit.iter().any(|p| p.1 == y), "Lücke in Zeile {y}");
            }
            // Jedes helle Pixel liegt auf dem Radius.
            let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
            let r = (w.min(h) as f32 - 1.0) / 2.0 - 1.0;
            for (x, y) in lit {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                assert!(
                    (d - r).abs() <= 0.6,
                    "Pixel {x},{y} weicht ab: {d} statt {r}"
                );
            }
        }
    }

    #[test]
    fn label_sits_in_white_box_and_scales_with_screen() {
        let (w, h) = (1920usize, 1080usize);
        let mut rgba = vec![200u8; w * h * 4];
        draw_label(&mut rgba, w, h, "#1 MAIN 1920X1080");
        let row = h / 2;
        let line: Vec<u8> = (0..w).map(|x| rgba[(row * w + x) * 4]).collect();
        assert!(line.contains(&255), "weißer Kasten");
        assert!(line.contains(&0), "schwarze Schrift");
        // Lesbare Größe: mindestens 8 Pixel-Blöcke hoch sind bei 1080p drin.
        let black_rows = (0..h).filter(|y| rgba[(y * w + w / 2) * 4] != 200).count();
        assert!(black_rows >= 80, "Feld nur {black_rows} Zeilen hoch");
    }

    #[test]
    fn grid_lines_are_drawn() {
        let (w, h) = (200, 100);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec("T", 200, 100));
        // Vertikale Linie bei x = w/2 (Mittelachse, heller).
        let axis = px(&rgba, w, 100, 15);
        assert!(axis[0] > 80, "Mittelachse hell: {axis:?}");
    }

    #[test]
    fn label_draws_text_pixels() {
        let (w, h) = (320, 240);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(
            &mut rgba,
            w as u32,
            h as u32,
            &spec("MAIN 320X240", 320, 240),
        );
        // Im Textbereich (Zentrum) müssen helle Pixel existieren.
        let mut bright = 0;
        for y in (h / 2 - 10)..(h / 2 + 10) {
            for x in (w / 2 - 80)..(w / 2 + 80) {
                if px(&rgba, w, x, y)[0] > 200 {
                    bright += 1;
                }
            }
        }
        assert!(bright > 50, "Beschriftung zeichnet helle Pixel: {bright}");
    }

    #[test]
    fn pattern_is_deterministic_and_opaque() {
        let (w, h) = (128, 96);
        let mut a = vec![0u8; w * h * 4];
        let mut b = vec![0u8; w * h * 4];
        draw_test_pattern(&mut a, w as u32, h as u32, &spec("S 128X96", 128, 96));
        draw_test_pattern(&mut b, w as u32, h as u32, &spec("S 128X96", 128, 96));
        assert_eq!(a, b, "Testbild ist deterministisch");
        assert!(
            a.chunks_exact(4).all(|c| c[3] == 255),
            "vollständig deckend"
        );
    }

    #[test]
    fn font_covers_label_characters() {
        for c in ('A'..='Z')
            .chain('a'..='z')
            .chain('0'..='9')
            .chain("#.-:()".chars())
        {
            assert_ne!(font().glyph_id(c).0, 0, "Glyphe für {c}");
        }
    }

    #[test]
    fn solid_color_fills_canvas() {
        use crate::core::TestPatternKind;
        let (w, h) = (64, 48);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "#1 LED 64x48".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::SolidColor,
            color: [255, 64, 64], // Rot
            index: 0,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Außerhalb von Rand, Kreis und Beschriftung: die Screen-Farbe.
        let center = px(&rgba, w, 4, 4);
        assert_eq!(
            [center[0], center[1], center[2]],
            [255, 64, 64],
            "Fläche hat Screen-Farbe"
        );
        // Rand ist weiß (überdeckt die Farbe).
        let frame_px = px(&rgba, w, w / 2, 0);
        assert_eq!(&frame_px[..3], &[255, 255, 255], "Rand weiß");
    }

    #[test]
    fn color_bars_draws_seven_stripes() {
        use crate::core::TestPatternKind;
        let (w, h) = (140, 48); // 140/7 = 20 px pro Balken
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "#2 BAR 140x48".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::ColorBars,
            color: [255, 255, 255],
            index: 1,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Pixel weit oben (y=2, nicht vom Label in der Mitte überdeckt).
        // Balken 0 (Grau): x=10
        let bar0 = px(&rgba, w, 10, 2);
        assert!(
            bar0[0] > 150 && bar0[1] > 150 && bar0[2] > 150,
            "Balken 0 grau: {bar0:?}"
        );
        // Balken 1 (Gelb): x=30
        let bar1 = px(&rgba, w, 30, 2);
        assert!(
            bar1[0] > 150 && bar1[1] > 150 && bar1[2] < 50,
            "Balken 1 gelb: {bar1:?}"
        );
        // Balken 6 (Blau): x=130
        let bar6 = px(&rgba, w, 130, 2);
        assert!(
            bar6[0] < 50 && bar6[1] < 50 && bar6[2] > 150,
            "Balken 6 blau: {bar6:?}"
        );
    }

    #[test]
    fn led_wall_checkerboard_numbered() {
        use crate::core::TestPatternKind;
        // 128×128 Canvas, Kachelgröße 64 → 2×2 Kacheln.
        let (w, h) = (128, 128);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "#1 WALL 128x128".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::LedWall,
            color: [255, 64, 64], // Rot
            index: 0,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Kachel (0,0) oben links: Screen-Farbe abgedunkelt (70% → [178,44,44]).
        // Pixel (12,12): außerhalb Pixelskala und Kachel-Nummer.
        let kachel_0_0 = px(&rgba, w, 12, 12);
        assert!(
            kachel_0_0[0] > 140 && kachel_0_0[0] < 220,
            "Kachel (0,0) abgedunkeltes Rot: {kachel_0_0:?}"
        );
        assert!(
            kachel_0_0[1] < 80,
            "Kachel (0,0) wenig Grün: {kachel_0_0:?}"
        );
        // Kachel (1,0) oben rechts: stark abgedunkelt (18% → [45,11,11]).
        let kachel_1_0 = px(&rgba, w, 76, 12);
        assert!(
            kachel_1_0[0] > 20 && kachel_1_0[0] < 80,
            "Kachel (1,0) dunkles Rot: {kachel_1_0:?}"
        );
        // Kachel (0,1) unten links: dunkel.
        let kachel_0_1 = px(&rgba, w, 12, 76);
        assert!(kachel_0_1[0] < 80, "Kachel (0,1) dunkel: {kachel_0_1:?}");
        // Kachel (1,1) unten rechts: heller.
        let kachel_1_1 = px(&rgba, w, 76, 76);
        assert!(kachel_1_1[0] > 140, "Kachel (1,1) heller: {kachel_1_1:?}");
        // 1px-Rahmen am Rand: Pixel (0,0) ist weiß (Rahmen überdeckt Kachel).
        let rahmen = px(&rgba, w, 0, 0);
        assert!(
            rahmen[0] > 200 && rahmen[1] > 200,
            "1px-Rahmen weiß: {rahmen:?}"
        );
        // Rahmen auch rechts/unten.
        assert!(px(&rgba, w, w - 1, h / 2)[0] > 200, "Rahmen rechts");
        assert!(px(&rgba, w, w / 2, h - 1)[1] > 200, "Rahmen unten");
    }

    #[test]
    fn led_wall_circle_1px() {
        use crate::core::TestPatternKind;
        let (w, h) = (128, 128);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "C 128x128".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::LedWall,
            color: [64, 255, 64],
            index: 0,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Kreis: suche weißes Pixel in der Nähe von (108, 108) (45° auf dem Kreis).
        // Toleranz: 3×3 Bereich (Midpoint-Algorithmus ist nicht pixelgenau).
        let mut found_white = false;
        for dy in -3..=3isize {
            for dx in -3..=3isize {
                let x = (108isize + dx) as usize;
                let y = (108isize + dy) as usize;
                if x < w && y < h {
                    let p = px(&rgba, w, x, y);
                    if p[0] > 200 && p[1] > 200 {
                        found_white = true;
                        break;
                    }
                }
            }
            if found_white {
                break;
            }
        }
        assert!(found_white, "1px-Kreis weiß nahe (108,108)");
    }

    #[test]
    fn led_wall_checker_size_adjustable() {
        use crate::core::TestPatternKind;
        // Kachelgröße 32 statt 64 → 4×4 Kacheln bei 128×128.
        let (w, h) = (128, 128);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "X".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::LedWall,
            color: [255, 255, 255],
            index: 0,
            checker_size: 32,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Bei 32px Kacheln: Kachel (0,0) heller (70% Weiß = 178).
        // Pixel (28,28): außerhalb Pixelskala und Kachel-Nummer (Text bei x=10..22, y=9..23).
        let k0 = px(&rgba, w, 28, 28);
        assert!(
            k0[0] > 140 && k0[0] < 220,
            "Kachel (0,0) bei 32px abgedunkeltes Weiß: {k0:?}"
        );
        // Kachel (1,0) dunkler (18% Weiß = 45): Pixel (60,28) in Kachel (1,0) = x=32..64.
        let k1 = px(&rgba, w, 60, 28);
        assert!(
            k1[0] > 20 && k1[0] < 80,
            "Kachel (1,0) bei 32px dunkles Weiß: {k1:?}"
        );
    }

    #[test]
    fn led_wall_pixel_scale_at_edges() {
        use crate::core::TestPatternKind;
        let (w, h) = (256, 256);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "SCALE".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::LedWall,
            color: [64, 128, 255],
            index: 0,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Pixelskala oben: bei x=64 (große Markierung, 8px lang ab y=1).
        let tick_top = px(&rgba, w, 64, 3);
        assert!(
            tick_top[0] > 200,
            "Pixelskala oben bei x=64 weiß: {tick_top:?}"
        );
        // Pixelskala links: bei y=64 (große Markierung, 8px lang ab x=1).
        let tick_left = px(&rgba, w, 3, 64);
        assert!(
            tick_left[1] > 200,
            "Pixelskala links bei y=64 weiß: {tick_left:?}"
        );
        // Pixelskala unten: bei x=128.
        let tick_bottom = px(&rgba, w, 128, h - 3);
        assert!(
            tick_bottom[0] > 200,
            "Pixelskala unten bei x=128 weiß: {tick_bottom:?}"
        );
        // Pixelskala rechts: bei y=128.
        let tick_right = px(&rgba, w, w - 3, 128);
        assert!(
            tick_right[2] > 200,
            "Pixelskala rechts bei y=128 weiß: {tick_right:?}"
        );
    }
}

#[cfg(test)]
mod region_grid_color_tests {
    use super::draw_region_grid;

    fn px(rgba: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let i = (y * w + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    }

    #[test]
    fn region_grid_uses_route_color_for_frame_and_axes() {
        let (w, h) = (200usize, 100usize);
        let mut rgba = vec![0u8; w * h * 4];
        draw_region_grid(&mut rgba, w as u32, h as u32, "A: Route X", [255, 0, 0]);
        // Rahmen oben links in Routenfarbe.
        assert_eq!(px(&rgba, w, 0, 0), [255, 0, 0], "Ecke = Routenfarbe");
        // Mittelachse (x = w/2, außerhalb des Labels) eingefärbt: dominant rot.
        let axis = px(&rgba, w, w / 2, h / 4);
        assert!(
            axis[0] > 120 && axis[1] < 60 && axis[2] < 60,
            "Achse rot: {axis:?}"
        );
        // Nebenlinie gedimmt, aber rötlich (nicht grau).
        let cell = (w.min(h) / 40).max(8);
        let minor = px(&rgba, w, w / 2 + cell, 10);
        assert!(
            minor[0] > minor[1] && minor[0] > minor[2],
            "Nebenlinie rötlich: {minor:?}"
        );
    }
}

#[cfg(test)]
mod watermark_tests {
    use super::{draw_test_pattern, TestPatternSpec};
    use crate::core::TestPatternKind;

    #[test]
    fn test_pattern_draws_faint_watermark_in_background() {
        let (w, h) = (400usize, 300usize);
        let mut rgba = vec![0u8; w * h * 4];
        let spec = TestPatternSpec {
            label: "T".to_string(),
            width: w as u32,
            height: h as u32,
            kind: TestPatternKind::SolidColor,
            color: [255, 0, 0], // Route rot
            index: 0,
            checker_size: 32,
        };
        // Baseline ohne Wasserzeichen: Muster + Kreis + Label + Rand (alles außer Kuh).
        let mut plain = vec![0u8; w * h * 4];
        super::draw_solid_color(&mut plain, w, h, spec.color);
        super::draw_circle(&mut plain, w, h, [255; 3]);
        super::draw_label(&mut plain, w, h, &spec.label);
        super::draw_frame(&mut plain, w, h, [255; 3]);
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Wasserzeichen dezent: auf reinen Hintergrundpixeln (plain == Grundton,
        // also ohne Rand/Kreis/Label) weicht das Ergebnis leicht ab — sichtbar,
        // aber nicht dominant.
        let mut max_diff = 0i32;
        for i in 0..w * h {
            if plain[i * 4] == spec.color[0]
                && plain[i * 4 + 1] == spec.color[1]
                && plain[i * 4 + 2] == spec.color[2]
            {
                let d = (rgba[i * 4] as i32 - plain[i * 4] as i32).abs()
                    + (rgba[i * 4 + 1] as i32 - plain[i * 4 + 1] as i32).abs()
                    + (rgba[i * 4 + 2] as i32 - plain[i * 4 + 2] as i32).abs();
                max_diff = max_diff.max(d);
            }
        }
        assert!(
            max_diff > 0 && max_diff < 250,
            "Wasserzeichen soll dezent sichtbar sein, max_diff={max_diff}"
        );
        // Ecke (außerhalb der Kuh): bleibt die Solid-Color-Farbe.
        let j = (5 * w + 5) * 4;
        assert_eq!([rgba[j], rgba[j + 1], rgba[j + 2]], [255, 0, 0]);
    }

    fn spec_with_kind(kind: TestPatternKind, color: [u8; 3]) -> TestPatternSpec {
        TestPatternSpec {
            label: "T".to_string(),
            width: 400,
            height: 300,
            kind,
            color,
            index: 0,
            checker_size: 32,
        }
    }

    /// Testbild ohne Kuh (Baseline): Hintergrund + Grid/Pixelskala + Kreis
    /// + Label + Rand – exakt draw_test_pattern ohne den Logo-Schritt.
    fn render_without_watermark(spec: &TestPatternSpec, w: usize, h: usize) -> Vec<u8> {
        let mut rgba = vec![0u8; w * h * 4];
        match spec.kind {
            TestPatternKind::Grid => {
                super::draw_solid_color(&mut rgba, w, h, [0, 0, 0]);
                let (dim, major, diag, axis, scale) = super::grid_shades_of(spec.color);
                super::grid_lines(&mut rgba, w, h, dim, major, diag, axis, scale);
            }
            TestPatternKind::SolidColor => super::draw_solid_color(&mut rgba, w, h, spec.color),
            TestPatternKind::ColorBars => super::draw_color_bars(&mut rgba, w, h),
            TestPatternKind::LedWall => {
                super::draw_led_wall(&mut rgba, w, h, spec);
                let cell = (spec.checker_size.max(8) as usize).min(w.max(h));
                super::draw_pixel_scale(&mut rgba, w, h, cell, (255, 255, 255));
            }
        }
        let accent = super::accent_color(spec);
        super::draw_circle(&mut rgba, w, h, accent);
        super::draw_label(&mut rgba, w, h, &spec.label);
        super::draw_frame(&mut rgba, w, h, accent);
        rgba
    }

    fn px4(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn watermark_uses_route_color_and_grid_lines_stay_on_top() {
        let (w, h) = (400usize, 300usize);
        let spec = spec_with_kind(TestPatternKind::Grid, [255, 0, 0]);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        let plain = render_without_watermark(&spec, w, h);
        // Kuh in Layerfarbe: Abweichung nur im R-Kanal gegen schwarzen Grund
        // (25 % von 255 ≈ 64).
        let mut found = false;
        for y in 100..200 {
            for x in 120..278 {
                let p = px4(&rgba, w, x, y);
                let b = px4(&plain, w, x, y);
                if p != b {
                    assert_eq!((p[1], p[2]), (b[1], b[2]), "nur R-Kanal: {p:?} vs {b:?}");
                    assert!(
                        p[0] > b[0] && p[0] <= 80,
                        "Deckkraft ~25 % von 255: {p:?} vs {b:?}"
                    );
                    found = true;
                }
            }
        }
        assert!(found, "Kuh in Routenfarbe (rot) sichtbar");
        // Mittelachse über der Kuh: 80 % Routenfarbe (altes Helligkeitsraster), unverändert.
        assert_eq!(
            px4(&rgba, w, w / 2, 120),
            [204, 0, 0, 255],
            "Achse über Kuh (80 % Routenfarbe)"
        );
        assert_eq!(px4(&rgba, w, w / 2, 120), px4(&plain, w, w / 2, 120));
        // Ecke außerhalb der Kuh: unverändert.
        assert_eq!(px4(&rgba, w, 5, 5), [0, 0, 0, 255]);
        assert_eq!(px4(&rgba, w, 5, 5), px4(&plain, w, 5, 5));
    }

    #[test]
    fn grid_circles_use_route_color() {
        let (w, h) = (400usize, 300usize);
        let spec = spec_with_kind(TestPatternKind::Grid, [255, 0, 0]);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Mittelkreis: Mitte (199.5, 149.5), Radius 148.5 → rechts bei x≈348.
        let p = px4(&rgba, w, 348, 149);
        assert!(
            p[0] > 200 && p[1] < 60 && p[2] < 60,
            "Mittelkreis rot: {p:?}"
        );
        // Eckkreis oben links: Mitte (50, 50), Radius 50 → oben bei (50, 0..1).
        let p = px4(&rgba, w, 100, 50);
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60, "Eckkreis rot: {p:?}");
    }

    #[test]
    fn region_grid_is_the_same_picture_as_the_grid_test_pattern() {
        let (w, h) = (400usize, 300usize);
        let spec = TestPatternSpec {
            label: "A: X".to_string(),
            ..spec_with_kind(TestPatternKind::Grid, [0, 128, 255])
        };
        let mut pattern = vec![0u8; w * h * 4];
        draw_test_pattern(&mut pattern, w as u32, h as u32, &spec);
        let mut region = vec![0u8; w * h * 4];
        super::draw_region_grid(&mut region, w as u32, h as u32, "A: X", [0, 128, 255]);
        assert!(
            pattern == region,
            "ein Testbild für Route, Region und Stage"
        );
    }

    #[test]
    fn region_grid_watermark_in_route_color() {
        let (w, h) = (200usize, 100usize);
        let mut rgba = vec![0u8; w * h * 4];
        super::draw_region_grid(&mut rgba, w as u32, h as u32, "A: X", [0, 255, 0]);
        // Kuh-Rechteck: 55 % von 100 = 55 px breit, zentriert → x 72..127, y 32..67.
        // Volle Kuh innen: 25 % von 255 ≈ 64 im G-Kanal, R/B = 0.
        let mut found = false;
        for y in 34..65 {
            for x in 74..125 {
                let p = px4(&rgba, w, x, y);
                if p[0] == 0 && p[2] == 0 && (55..=75).contains(&p[1]) {
                    found = true;
                }
            }
        }
        assert!(found, "Kuh in Routenfarbe (25 % Grün auf Schwarz ≈ 64)");
        // Rahmen bleibt Routenfarbe.
        assert_eq!(px4(&rgba, w, 0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn solid_color_watermark_is_lightened_so_it_stays_visible() {
        let (w, h) = (400usize, 300usize);
        let spec = spec_with_kind(TestPatternKind::SolidColor, [200, 100, 50]);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        // Kuh aufgehellt: im Kuh-Rechteck heller als die Grundfarbe.
        let mut found = false;
        for y in 100..130 {
            for x in 120..278 {
                let p = px4(&rgba, w, x, y);
                if p[0] > 200 && p[1] > 100 && p[2] > 50 {
                    found = true;
                }
            }
        }
        assert!(found, "Kuh auf Solid Color aufgehellt (sichtbar)");
        // Ecke außerhalb der Kuh: reine Screen-Farbe.
        assert_eq!(px4(&rgba, w, 5, 5), [200, 100, 50, 255]);
    }

    #[test]
    fn solid_white_watermark_is_darkened_so_it_stays_visible() {
        let (w, h) = (400usize, 300usize);
        let spec = spec_with_kind(TestPatternKind::SolidColor, [255, 255, 255]);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
        let plain = render_without_watermark(&spec, w, h);
        // Kuh abgedunkelt: im Kuh-Rechteck dunkler als das Muster ohne Kuh.
        let darker = (100..200)
            .flat_map(|y| (120..278).map(move |x| (x, y)))
            .any(|(x, y)| {
                let (p, b) = (px4(&rgba, w, x, y), px4(&plain, w, x, y));
                p[0] < b[0] && p[0] == p[1] && p[1] == p[2]
            });
        assert!(darker, "Kuh auf Weiß abgedunkelt (sichtbar)");
        assert_eq!(px4(&rgba, w, 5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn watermark_on_all_pattern_kinds_but_only_inside_kuh_rect() {
        let (w, h) = (400usize, 300usize);
        for kind in [
            TestPatternKind::Grid,
            TestPatternKind::SolidColor,
            TestPatternKind::ColorBars,
            TestPatternKind::LedWall,
        ] {
            let spec = spec_with_kind(kind, [255, 0, 0]);
            let mut rgba = vec![0u8; w * h * 4];
            draw_test_pattern(&mut rgba, w as u32, h as u32, &spec);
            let plain = render_without_watermark(&spec, w, h);
            // Kuh-Rechteck bei 400×300: 165×104 zentriert → x 117..282, y 98..202.
            let (kx0, ky0, kx1, ky1) = (117, 98, 282, 202);
            let mut inside_diff = false;
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    if rgba[i..i + 4] != plain[i..i + 4] {
                        let inner = x >= kx0 + 2 && x < kx1 - 2 && y >= ky0 + 2 && y < ky1 - 2;
                        assert!(
                            x >= kx0 && x < kx1 && y >= ky0 && y < ky1,
                            "Abweichung außerhalb der Kuh bei {kind:?} ({x},{y})"
                        );
                        if inner {
                            inside_diff = true;
                        }
                    }
                }
            }
            assert!(inside_diff, "Kuh sichtbar bei {kind:?}");
        }
    }
}

#[cfg(test)]
mod dump {
    use super::*;
    #[test]
    #[ignore]
    fn dump_test_pattern_png() {
        let (w, h) = (640u32, 360u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let spec = TestPatternSpec {
            label: "A: Route 1  640x360".to_string(),
            width: w,
            height: h,
            kind: crate::core::TestPatternKind::Grid,
            color: [255, 80, 0],
            index: 0,
            checker_size: 64,
        };
        draw_test_pattern(&mut rgba, w, h, &spec);
        let img = image::RgbaImage::from_raw(w, h, rgba).unwrap();
        img.save(std::env::temp_dir().join("grid_test.png"))
            .unwrap();
        // Region-Grid-Variante
        let mut rgba2 = vec![0u8; (w * h * 4) as usize];
        draw_region_grid(&mut rgba2, w, h, "A: Route X", [255, 80, 0]);
        image::RgbaImage::from_raw(w, h, rgba2)
            .unwrap()
            .save(std::env::temp_dir().join("region_grid.png"))
            .unwrap();
    }
}
