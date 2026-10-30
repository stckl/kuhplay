//! Testbild-Renderer (QLab „Test Pattern").
//!
//! Zeichnet ein Testbild in ein RGBA-Canvas (Stage-Canvas des Compositors):
//!
//! * **Grid** (QLab-Standard): leichtes Schachbrettmuster, weißer Rahmen,
//!   10×10-Raster, einbeschriebener Kreis, zentrierte Beschriftung mit
//!   Screen-Nummer, Name und Auflösung (z. B. `„#1 MAIN 1920X1080"`).
//! * **Solid Color**: einfarbige Fläche in der Screen-Farbe (automatisch
//!   codiert nach Index) + Beschriftung – zum Ausrichten und
//!   Unterscheiden von Projektoren/LED-Modulen.
//! * **Color Bars** (SMPTE): Weiß, Gelb, Cyan, Grün, Magenta, Rot, Blau +
//!   Beschriftung – zum Kalibrieren von Farbe/Helligkeit.
//!
//! Das Testbild **überdeckt die Inhalte** (QLab-Verhalten: Screen-Test zum
//! Einrichten von Projektoren und LED-Wänden). Es wird ungedreht in
//! Canvas-Koordinaten gezeichnet; die Drehung des Screens
//! ([`crate::core::ScreenRotation`]) wird beim Anzeigen im Viewport
//! angewendet und dreht Testbild + Inhalt gleichermaßen.
//!
//! Die Beschriftung nutzt eine eingebaute 5×7-Bitmap-Schrift (Public-Domain-
//! Stil, klassische `font5x7`), damit der Renderer keine Font-Dependency
//! braucht und auf allen Plattformen identisch aussieht.

use crate::core::{TestPatternKind, TestPatternSpec};

/// Zeichnet das Testbild vollständig in `rgba` (Größe `width`×`height`,
/// Row-Major, 4 Bytes/Pixel).
pub fn draw_test_pattern(rgba: &mut [u8], width: u32, height: u32, spec: &TestPatternSpec) {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    assert_eq!(
        rgba.len(),
        w * h * 4,
        "rgba buffer must match width*height*4"
    );

    match spec.kind {
        TestPatternKind::Grid => {
            draw_checkerboard(rgba, w, h);
            draw_grid(rgba, w, h);
            let margin = (w.min(h) / 20).max(8);
            draw_frame(rgba, w, h, margin);
            draw_circle(rgba, w, h, margin);
            draw_label(rgba, w, h, &spec.label);
        }
        TestPatternKind::SolidColor => {
            draw_solid_color(rgba, w, h, spec.color);
            // Dezenter Rahmen + Beschriftung für Orientierung.
            let margin = (w.min(h) / 20).max(8);
            draw_frame(rgba, w, h, margin);
            draw_label(rgba, w, h, &spec.label);
        }
        TestPatternKind::ColorBars => {
            draw_color_bars(rgba, w, h);
            draw_label(rgba, w, h, &spec.label);
        }
        TestPatternKind::LedWall => {
            draw_led_wall(rgba, w, h, spec);
        }
    }
}

/// LED-Wand-Testbild: Schachbrettmuster mit durchnummerierten Kacheln,
/// 1px-Rahmen am Rand, 1px-Kreis in der Mitte, Beschriftung zentriert.
/// Hintergrund: Schachbrett leicht in der Screen-Farbe eingefärbt
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

    // 1px-Rahmen ganz am Rand (weiß).
    draw_frame_1px(rgba, w, h, (255, 255, 255));
    // Pixelskala umlaufend am Rand (Markierungen + Zahlen).
    draw_pixel_scale(rgba, w, h, cell, (255, 255, 255));
    // 1px-Kreis in der Mitte (weiß).
    draw_circle_1px(rgba, w, h, (255, 255, 255));
    // Beschriftung zentriert (mit Schatten).
    draw_label(rgba, w, h, &spec.label);
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

/// Zeichnet Text an einer absoluten Position (kein Zentrieren).
fn draw_label_at(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    x0: isize,
    y0: isize,
    text: &str,
    color: (u8, u8, u8),
) {
    let scale = 1usize;
    let mut cx = x0;
    for ch in text.chars() {
        let glyph = glyph_for(ch);
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..7usize {
                if bits & (1 << row) != 0 {
                    fill_block(
                        rgba,
                        w,
                        h,
                        cx + col as isize * scale as isize,
                        y0 + row as isize * scale as isize,
                        scale,
                        color,
                    );
                }
            }
        }
        cx += 6 * scale as isize;
    }
}

/// Zeichnet einen 1px-Rahmen exakt am Canvas-Rand.
fn draw_frame_1px(rgba: &mut [u8], w: usize, h: usize, color: (u8, u8, u8)) {
    // Oben und unten (y=0 und y=h-1).
    for x in 0..w {
        let i_top = x * 4;
        rgba[i_top] = color.0;
        rgba[i_top + 1] = color.1;
        rgba[i_top + 2] = color.2;
        rgba[i_top + 3] = 255;
        let i_bot = ((h - 1) * w + x) * 4;
        rgba[i_bot] = color.0;
        rgba[i_bot + 1] = color.1;
        rgba[i_bot + 2] = color.2;
        rgba[i_bot + 3] = 255;
    }
    // Links und rechts (x=0 und x=w-1).
    for y in 0..h {
        let i_left = (y * w) * 4;
        rgba[i_left] = color.0;
        rgba[i_left + 1] = color.1;
        rgba[i_left + 2] = color.2;
        rgba[i_left + 3] = 255;
        let i_right = (y * w + w - 1) * 4;
        rgba[i_right] = color.0;
        rgba[i_right + 1] = color.1;
        rgba[i_right + 2] = color.2;
        rgba[i_right + 3] = 255;
    }
}

/// Zeichnet einen 1px-Kreis (einbeschrieben) in der Mitte.
fn draw_circle_1px(rgba: &mut [u8], w: usize, h: usize, color: (u8, u8, u8)) {
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let radius = (w.min(h) / 2) as f32 - 1.0; // 1px Abstand zum Rand
    if radius < 4.0 {
        return;
    }
    // Midpoint-Circle-Algorithmus für 1px-Linie.
    let mut x = radius as i32;
    let mut y = 0i32;
    let mut err = 1 - x;
    while x >= y {
        let pts = [
            (cx as i32 + x, cy as i32 + y),
            (cx as i32 + y, cy as i32 + x),
            (cx as i32 - y, cy as i32 + x),
            (cx as i32 - x, cy as i32 + y),
            (cx as i32 - x, cy as i32 - y),
            (cx as i32 - y, cy as i32 - x),
            (cx as i32 + y, cy as i32 - x),
            (cx as i32 + x, cy as i32 - y),
        ];
        for (px, py) in pts {
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let i = (py as usize * w + px as usize) * 4;
                rgba[i] = color.0;
                rgba[i + 1] = color.1;
                rgba[i + 2] = color.2;
                rgba[i + 3] = 255;
            }
        }
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
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
    // Skalierung: Text muss in die Kachel passen (max. 60% der Kachel).
    let n_chars = text.chars().count().max(1) as isize;
    let max_scale_w = (rect_w * 6 / 10) / (n_chars * 6);
    let max_scale_h = (rect_h * 6 / 10) / 7;
    let scale = (max_scale_w.min(max_scale_h) as usize).clamp(1, 6);
    let text_w = text.chars().count() * 6 * scale;
    let text_h = 7 * scale;
    let cx0 = x0 as isize + (rect_w - text_w as isize) / 2;
    let cy0 = y0 as isize + (rect_h - text_h as isize) / 2;
    let mut cx = cx0;
    for ch in text.chars() {
        let glyph = glyph_for(ch);
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..7usize {
                if bits & (1 << row) != 0 {
                    fill_block(
                        rgba,
                        w,
                        h,
                        cx + col as isize * scale as isize,
                        cy0 + row as isize * scale as isize,
                        scale,
                        color,
                    );
                }
            }
        }
        cx += 6 * scale as isize;
    }
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

/// SMPTE-Farbbalken: 7 vertikale Streifen (75% Sättigung, QLab-Style).
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

/// Leichtes Schachbrettmuster (zwei dezente Graustufen).
fn draw_checkerboard(rgba: &mut [u8], w: usize, h: usize) {
    let cell = (w.max(h) / 16).clamp(16, 128);
    let (dark, light) = ((26u8, 26u8, 26u8), (44u8, 44u8, 44u8));
    for y in 0..h {
        for x in 0..w {
            let on = ((x / cell) + (y / cell)).is_multiple_of(2);
            let (r, g, b) = if on { dark } else { light };
            let i = (y * w + x) * 4;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
}

/// 10×10-Rasterlinien + hellere Mittelachsen.
fn draw_grid(rgba: &mut [u8], w: usize, h: usize) {
    let line = (72u8, 72u8, 72u8);
    let axis = (96u8, 96u8, 96u8);
    let put = |rgba: &mut [u8], x: usize, y: usize, c: (u8, u8, u8)| {
        if x < w && y < h {
            let i = (y * w + x) * 4;
            rgba[i] = c.0;
            rgba[i + 1] = c.1;
            rgba[i + 2] = c.2;
            rgba[i + 3] = 255;
        }
    };
    for i in 1..10 {
        let x = w * i / 10;
        let y = h * i / 10;
        let c = if i == 5 { axis } else { line };
        for yy in 0..h {
            put(rgba, x, yy, c);
        }
        for xx in 0..w {
            put(rgba, xx, y, c);
        }
    }
}

/// Weißer Rahmen, `margin` px eingezogen.
fn draw_frame(rgba: &mut [u8], w: usize, h: usize, margin: usize) {
    let t = (w.min(h) / 100).max(2);
    let white = (240u8, 240u8, 240u8);
    let mut fill_rect = |x0: usize, y0: usize, x1: usize, y1: usize| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                let i = (y * w + x) * 4;
                rgba[i] = white.0;
                rgba[i + 1] = white.1;
                rgba[i + 2] = white.2;
                rgba[i + 3] = 255;
            }
        }
    };
    fill_rect(margin, margin, w - margin, margin + t); // oben
    fill_rect(margin, h - margin - t, w - margin, h - margin); // unten
    fill_rect(margin, margin, margin + t, h - margin); // links
    fill_rect(w - margin - t, margin, w - margin, h - margin); // rechts
}

/// Einbeschriebener Kreis (weißer Umriss) innerhalb des Rahmens.
fn draw_circle(rgba: &mut [u8], w: usize, h: usize, margin: usize) {
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let radius = (w.min(h) / 2 - margin) as f32;
    if radius < 4.0 {
        return;
    }
    let t = ((w.min(h) / 200).max(1)) as f32;
    let white = (240u8, 240u8, 240u8);
    // Pro Pixel: Distanz zum Kreisring (nur Ring zeichnen).
    let r0 = radius - t;
    let r1 = radius + t;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d >= r0 && d <= r1 {
                let i = (y * w + x) * 4;
                rgba[i] = white.0;
                rgba[i + 1] = white.1;
                rgba[i + 2] = white.2;
                rgba[i + 3] = 255;
            }
        }
    }
}

/// Zentrierte Beschriftung (5×7-Bitmap-Schrift, weiß mit schwarzem Schatten).
fn draw_label(rgba: &mut [u8], w: usize, h: usize, text: &str) {
    let scale = (w.min(h) / 60).clamp(2, 12);
    let text_w = text.chars().count() * 6 * scale;
    let text_h = 7 * scale;
    let x0 = (w as isize - text_w as isize) / 2;
    let y0 = (h as isize - text_h as isize) / 2;
    let mut cx = x0;
    for ch in text.chars() {
        let glyph = glyph_for(ch);
        for (col, bits) in glyph.iter().enumerate() {
            for row in 0..7usize {
                if bits & (1 << row) != 0 {
                    // Schatten (+1/+1, schwarz).
                    fill_block(
                        rgba,
                        w,
                        h,
                        cx + scale as isize + 1,
                        y0 + row as isize * scale as isize + 1,
                        scale,
                        (0, 0, 0),
                    );
                    fill_block(
                        rgba,
                        w,
                        h,
                        cx + col as isize * scale as isize,
                        y0 + row as isize * scale as isize,
                        scale,
                        (245, 245, 245),
                    );
                }
            }
        }
        cx += 6 * scale as isize;
    }
}

/// Füllt einen `scale`×`scale`-Block (geclampt auf den Canvas).
fn fill_block(
    rgba: &mut [u8],
    w: usize,
    h: usize,
    x0: isize,
    y0: isize,
    scale: usize,
    c: (u8, u8, u8),
) {
    for dy in 0..scale as isize {
        for dx in 0..scale as isize {
            let x = x0 + dx;
            let y = y0 + dy;
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let i = (y as usize * w + x as usize) * 4;
                rgba[i] = c.0;
                rgba[i + 1] = c.1;
                rgba[i + 2] = c.2;
                rgba[i + 3] = 255;
            }
        }
    }
}

// ---------------------------------------------------------------------
// 5×7-Bitmap-Schrift (Public-Domain-Stil, klassische font5x7)
// ---------------------------------------------------------------------

/// 5×7-Glyphe für ein Zeichen (5 Spalten, Bit 0 = oberste Zeile).
///
/// Nur Großbuchstaben, Ziffern und wenige Zeichen – für Testbild-Labels
/// ausreichend. Unbekannte Zeichen → „?“.
fn glyph_for(c: char) -> [u8; 5] {
    match c.to_ascii_uppercase() {
        'A' => [0x7E, 0x11, 0x11, 0x11, 0x7E],
        'B' => [0x7F, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3E, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7F, 0x41, 0x41, 0x22, 0x1C],
        'E' => [0x7F, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7F, 0x09, 0x09, 0x09, 0x01],
        'G' => [0x3E, 0x41, 0x49, 0x49, 0x7A],
        'H' => [0x7F, 0x08, 0x08, 0x08, 0x7F],
        'I' => [0x00, 0x41, 0x7F, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3F, 0x01],
        'K' => [0x7F, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7F, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7F, 0x02, 0x0C, 0x02, 0x7F],
        'N' => [0x7F, 0x04, 0x08, 0x10, 0x7F],
        'O' => [0x3E, 0x41, 0x41, 0x41, 0x3E],
        'P' => [0x7F, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3E, 0x41, 0x51, 0x21, 0x5E],
        'R' => [0x7F, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7F, 0x01, 0x01],
        'U' => [0x3F, 0x40, 0x40, 0x40, 0x3F],
        'V' => [0x1F, 0x20, 0x40, 0x20, 0x1F],
        'W' => [0x3F, 0x40, 0x38, 0x40, 0x3F],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '0' => [0x3E, 0x51, 0x49, 0x45, 0x3E],
        '1' => [0x00, 0x42, 0x7F, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4B, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7F, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3C, 0x4A, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1E],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00],
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        ',' => [0x00, 0x70, 0x18, 0x00, 0x00],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '#' => [0x14, 0x7F, 0x14, 0x7F, 0x14],
        '%' => [0x23, 0x13, 0x08, 0x64, 0x62],
        '+' => [0x08, 0x08, 0x3E, 0x08, 0x08],
        '_' => [0x40, 0x40, 0x40, 0x40, 0x40],
        '(' => [0x00, 0x1C, 0x22, 0x41, 0x00],
        ')' => [0x00, 0x41, 0x22, 0x1C, 0x00],
        '?' => [0x02, 0x01, 0x51, 0x09, 0x06],
        _ => [0x02, 0x01, 0x51, 0x09, 0x06], // unbekannt → „?“
    }
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
    fn checkerboard_alternates() {
        let (w, h) = (64, 64);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec("T 64X64", 64, 64));
        // Zellgröße = clamp(64/16, 16..128) = 16 px → Zellgrenze bei x=16.
        let a = px(&rgba, w, 0, 0);
        let b = px(&rgba, w, 16, 0);
        assert_ne!(a, b, "Schachbrett wechselt die Farbe");
        // Beide sind die dezenten Graustufen.
        assert!(a[0] < 60 && b[0] < 60);
        // Diagonale Nachbarschaft wieder gleich.
        assert_eq!(a, px(&rgba, w, 16, 16));
    }

    #[test]
    fn frame_is_drawn_inside() {
        let (w, h) = (200, 100);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec("T", 200, 100));
        // margin = max(min(200,100)/20, 8) = 8; Dicke t = max(100/100, 2) = 2.
        // Rahmen oben bei y=8..10 weiß (überdeckt die Raster-Mittelachse).
        let frame_px = px(&rgba, w, 100, 8);
        assert!(frame_px[0] > 200, "Rahmen weiß: {frame_px:?}");
        // Direkt außerhalb des Rahmens (y=2) keine Rahmenfarbe.
        let outside = px(&rgba, w, 50, 2);
        assert!(
            outside[0] < 100,
            "außerhalb des Rahmens dunkel: {outside:?}"
        );
        // Rahmen auch unten (y=90..92) und rechts (x=190..192) vorhanden.
        assert!(px(&rgba, w, 100, 90)[0] > 200);
        assert!(px(&rgba, w, 190, 50)[0] > 200);
    }

    #[test]
    fn circle_is_drawn() {
        let (w, h) = (200, 200);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec("T", 200, 200));
        // Kreisring: Punkt rechts der Mitte auf Ringhöhe (cx + radius).
        // radius = 200/2 - margin(10) = 90 → Pixel (190, 100) liegt auf dem Ring.
        let ring = px(&rgba, w, 190, 100);
        assert!(ring[0] > 200, "Kreisring weiß: {ring:?}");
        // Innen (nahe Zentrum) kein Ring.
        let inside = px(&rgba, w, 100, 100);
        assert!(inside[0] < 100, "Kreis innen dunkel: {inside:?}");
    }

    #[test]
    fn grid_lines_are_drawn() {
        let (w, h) = (200, 100);
        let mut rgba = vec![0u8; w * h * 4];
        draw_test_pattern(&mut rgba, w as u32, h as u32, &spec("T", 200, 100));
        // Vertikale Linie bei x = w/2 (Mittelachse, heller).
        let axis = px(&rgba, w, 100, 10);
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
    fn glyph_covers_charset() {
        // Alle Label-Zeichen haben eine definierte Glyphe (kein „?“ für A-Z/0-9).
        for c in ('A'..='Z')
            .chain('0'..='9')
            .chain([' ', 'X', ':', '.', '-'])
        {
            let g = glyph_for(c);
            assert!(g.iter().any(|b| *b != 0) || c == ' ', "Glyphe für {c}");
        }
        assert_eq!(glyph_for('?'), glyph_for('§'), "unbekannt → Fragezeichen");
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
        // Innen (nicht am Rahmen): überall die Screen-Farbe.
        let center = px(&rgba, w, w / 2, h / 2);
        assert_eq!(
            [center[0], center[1], center[2]],
            [255, 64, 64],
            "Zentrum hat Screen-Farbe"
        );
        // Rahmen ist weiß (überdeckt die Farbe).
        let margin = (w.min(h) / 20).max(8);
        let frame_px = px(&rgba, w, w / 2, margin);
        assert!(frame_px[0] > 200, "Rahmen weiß überdeckt Farbe");
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
