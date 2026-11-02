//! Kuh-Wasserzeichen als Alpha-Maske – Quelle ist `kuh.svg` (potrace-Ausgabe:
//! genau ein `<path>` aus M/c/l/Z im viewBox-Koordinatensystem).
//!
//! Die Maske wird **einmalig zur Laufzeit** gerastert und gecacht ([`OnceLock`])
//! – nicht je Bild, und ohne externes SVG-Crate: potrace-Pfade bestehen nur
//! aus Geraden und kubischen Bézier-Segmenten, die hier flach gerechnet und
//! per Scanline (even-odd, 2×2-Supersampling) gefüllt werden.

use std::sync::OnceLock;

/// Gerasterte Kuh: Alpha-Maske in viewBox-Auflösung.
pub struct KuhMask {
    pub width: u32,
    pub height: u32,
    /// `width * height` Bytes, 0 = Hintergrund, 255 = voll deckende Kuh.
    pub alpha: Vec<u8>,
}

/// Eingebettetes Original-SVG (potrace).
static KUH_SVG: &str = include_str!("../../kuh.svg");

/// Einmalig gerasterte Kuh-Maske (Cache – nicht je Bild neu gerechnet).
pub fn kuh_mask() -> &'static KuhMask {
    static MASK: OnceLock<KuhMask> = OnceLock::new();
    MASK.get_or_init(|| rasterize(KUH_SVG))
}

/// Liest `name="wert"` aus einem XML-Tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let start = tag.find(&pat)? + pat.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Zerlegt „a,b c" in floats.
fn parse_floats(s: &str) -> Vec<f32> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f32>().unwrap_or(0.0))
        .collect()
}

/// `translate(tx,ty) scale(sx,sy)` – Default: Identität.
fn parse_transform(tr: &str) -> (f32, f32, f32, f32) {
    let mut t = (0.0, 0.0, 1.0, 1.0);
    if let Some(i) = tr.find("translate(") {
        let inner = &tr[i + "translate(".len()..];
        let end = inner.find(')').unwrap_or(inner.len());
        let v = parse_floats(&inner[..end]);
        if v.len() >= 2 {
            t.0 = v[0];
            t.1 = v[1];
        }
    }
    if let Some(i) = tr.find("scale(") {
        let inner = &tr[i + "scale(".len()..];
        let end = inner.find(')').unwrap_or(inner.len());
        let v = parse_floats(&inner[..end]);
        if v.len() >= 2 {
            t.2 = v[0];
            t.3 = v[1];
        } else if let Some(&s) = v.first() {
            t.2 = s;
            t.3 = s;
        }
    }
    t
}

enum Tok {
    Cmd(char),
    Num(f32),
}

/// Zerlegt SVG-Path-Daten in Kommandos und Zahlen (korrekt auch bei „1-2“).
fn tokenize(d: &str) -> Vec<Tok> {
    let chars: Vec<char> = d.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphabetic() {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c.is_whitespace() || c == ',' {
            i += 1;
        } else {
            let start = i;
            if chars[i] == '+' || chars[i] == '-' {
                i += 1;
            }
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let save = i;
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    i += 1;
                }
                if i < chars.len() && chars[i].is_ascii_digit() {
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Tok::Num(s.parse().unwrap_or(0.0)));
        }
    }
    out
}

/// Kubische Bézier flach gerechnet (24 Segmente).
fn flatten_cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> Vec<[f32; 4]> {
    const N: usize = 24;
    let mut out = Vec::with_capacity(N);
    let mut prev = p0;
    for k in 1..=N {
        let t = k as f32 / N as f32;
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.0
            + 3.0 * mt * mt * t * p1.0
            + 3.0 * mt * t * t * p2.0
            + t * t * t * p3.0;
        let y = mt * mt * mt * p0.1
            + 3.0 * mt * mt * t * p1.1
            + 3.0 * mt * t * t * p2.1
            + t * t * t * p3.1;
        out.push([prev.0, prev.1, x, y]);
        prev = (x, y);
    }
    out
}

/// Quadratische Bézier flach gerechnet (16 Segmente).
fn flatten_quad(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) -> Vec<[f32; 4]> {
    const N: usize = 16;
    let mut out = Vec::with_capacity(N);
    let mut prev = p0;
    for k in 1..=N {
        let t = k as f32 / N as f32;
        let mt = 1.0 - t;
        let x = mt * mt * p0.0 + 2.0 * mt * t * p1.0 + t * t * p2.0;
        let y = mt * mt * p0.1 + 2.0 * mt * t * p1.1 + t * t * p2.1;
        out.push([prev.0, prev.1, x, y]);
        prev = (x, y);
    }
    out
}

/// Path-Daten → Geraden-Segmente in viewBox-Koordinaten.
fn path_to_segments(d: &str) -> Vec<[f32; 4]> {
    let toks = tokenize(d);
    let mut segs: Vec<[f32; 4]> = Vec::new();
    let mut i = 0;
    let mut cur = (0.0f32, 0.0f32);
    let mut sub_start = (0.0f32, 0.0f32);
    let mut last_c2: Option<(f32, f32)> = None;
    let mut last_q: Option<(f32, f32)> = None;
    let mut cmd = ' ';
    fn num(toks: &[Tok], i: &mut usize) -> f32 {
        match toks.get(*i) {
            Some(Tok::Num(n)) => {
                *i += 1;
                *n
            }
            _ => 0.0,
        }
    }
    while i < toks.len() {
        match &toks[i] {
            Tok::Cmd(c) => {
                // Z schließt den Subpath sofort (es folgen keine Zahlen).
                if *c == 'Z' || *c == 'z' {
                    if cur != sub_start {
                        segs.push([cur.0, cur.1, sub_start.0, sub_start.1]);
                    }
                    cur = sub_start;
                    last_c2 = None;
                    last_q = None;
                }
                cmd = *c;
                i += 1;
            }
            Tok::Num(_) => match cmd {
                'M' | 'm' => {
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    cur = if cmd == 'M' {
                        (x, y)
                    } else {
                        (cur.0 + x, cur.1 + y)
                    };
                    sub_start = cur;
                    // Folge-Zahlenpaare nach M sind implizite L.
                    cmd = if cmd == 'M' { 'L' } else { 'l' };
                    last_c2 = None;
                    last_q = None;
                }
                'L' | 'l' => {
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let p = if cmd == 'L' {
                        (x, y)
                    } else {
                        (cur.0 + x, cur.1 + y)
                    };
                    segs.push([cur.0, cur.1, p.0, p.1]);
                    cur = p;
                    last_c2 = None;
                    last_q = None;
                }
                'H' | 'h' => {
                    let x = num(&toks, &mut i);
                    let p = if cmd == 'H' {
                        (x, cur.1)
                    } else {
                        (cur.0 + x, cur.1)
                    };
                    segs.push([cur.0, cur.1, p.0, p.1]);
                    cur = p;
                    last_c2 = None;
                    last_q = None;
                }
                'V' | 'v' => {
                    let y = num(&toks, &mut i);
                    let p = if cmd == 'V' {
                        (cur.0, y)
                    } else {
                        (cur.0, cur.1 + y)
                    };
                    segs.push([cur.0, cur.1, p.0, p.1]);
                    cur = p;
                    last_c2 = None;
                    last_q = None;
                }
                'C' | 'c' => {
                    let x1 = num(&toks, &mut i);
                    let y1 = num(&toks, &mut i);
                    let x2 = num(&toks, &mut i);
                    let y2 = num(&toks, &mut i);
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let (a, b, p) = if cmd == 'C' {
                        ((x1, y1), (x2, y2), (x, y))
                    } else {
                        (
                            (cur.0 + x1, cur.1 + y1),
                            (cur.0 + x2, cur.1 + y2),
                            (cur.0 + x, cur.1 + y),
                        )
                    };
                    segs.extend(flatten_cubic(cur, a, b, p));
                    last_c2 = Some(b);
                    cur = p;
                    last_q = None;
                }
                'S' | 's' => {
                    let x2 = num(&toks, &mut i);
                    let y2 = num(&toks, &mut i);
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let (b, p) = if cmd == 'S' {
                        ((x2, y2), (x, y))
                    } else {
                        ((cur.0 + x2, cur.1 + y2), (cur.0 + x, cur.1 + y))
                    };
                    let a = last_c2
                        .map(|c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1))
                        .unwrap_or(cur);
                    segs.extend(flatten_cubic(cur, a, b, p));
                    last_c2 = Some(b);
                    cur = p;
                    last_q = None;
                }
                'Q' | 'q' => {
                    let x1 = num(&toks, &mut i);
                    let y1 = num(&toks, &mut i);
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let (a, p) = if cmd == 'Q' {
                        ((x1, y1), (x, y))
                    } else {
                        ((cur.0 + x1, cur.1 + y1), (cur.0 + x, cur.1 + y))
                    };
                    segs.extend(flatten_quad(cur, a, p));
                    last_q = Some(a);
                    cur = p;
                    last_c2 = None;
                }
                'T' | 't' => {
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let p = if cmd == 'T' {
                        (x, y)
                    } else {
                        (cur.0 + x, cur.1 + y)
                    };
                    let a = last_q
                        .map(|c| (2.0 * cur.0 - c.0, 2.0 * cur.1 - c.1))
                        .unwrap_or(cur);
                    segs.extend(flatten_quad(cur, a, p));
                    last_q = Some(a);
                    cur = p;
                    last_c2 = None;
                }
                'A' | 'a' => {
                    // potrace generiert keine Bögen – als Gerade approximieren.
                    let _ = num(&toks, &mut i);
                    let _ = num(&toks, &mut i);
                    let _ = num(&toks, &mut i);
                    let _ = num(&toks, &mut i);
                    let _ = num(&toks, &mut i);
                    let x = num(&toks, &mut i);
                    let y = num(&toks, &mut i);
                    let p = if cmd == 'A' {
                        (x, y)
                    } else {
                        (cur.0 + x, cur.1 + y)
                    };
                    segs.push([cur.0, cur.1, p.0, p.1]);
                    cur = p;
                    last_c2 = None;
                    last_q = None;
                }
                _ => {
                    i += 1;
                }
            },
        }
    }
    segs
}

/// Rastert das SVG einmalig in eine Alpha-Maske (viewBox-Auflösung,
/// 2×2-Supersampling, Scanline even-odd mit Buckets pro Subpixel-Zeile).
fn rasterize(svg: &str) -> KuhMask {
    let svg_start = svg.find("<svg").expect("kuh.svg: <svg> fehlt");
    let svg_end = svg[svg_start..]
        .find('>')
        .map(|e| svg_start + e + 1)
        .unwrap_or(svg.len());
    let svg_tag = &svg[svg_start..svg_end];
    let v = parse_floats(attr(svg_tag, "viewBox").expect("kuh.svg: viewBox fehlt"));
    assert!(v.len() >= 4, "kuh.svg: viewBox unvollständig");
    let (vx, vy, vw, vh) = (v[0], v[1], v[2], v[3]);
    let (w, h) = (vw.round().max(1.0) as u32, vh.round().max(1.0) as u32);

    let g_start = svg.find("<g ").expect("kuh.svg: <g> fehlt");
    let g_end = svg[g_start..]
        .find('>')
        .map(|e| g_start + e + 1)
        .unwrap_or(svg.len());
    let g_tag = &svg[g_start..g_end];
    let (tx, ty, sx, sy) = g_tag
        .find("transform")
        .map(|_| parse_transform(attr(g_tag, "transform").unwrap_or("")))
        .unwrap_or((0.0, 0.0, 1.0, 1.0));

    let p_start = svg.find("<path").expect("kuh.svg: <path> fehlt");
    let p_end = svg[p_start..]
        .find('>')
        .map(|e| p_start + e + 1)
        .unwrap_or(svg.len());
    let d = attr(&svg[p_start..p_end], "d").expect("kuh.svg: d-Attribut fehlt");

    // In Zielpixel transformieren (lineare Transformation – Bézier bleibt Bézier).
    let segs: Vec<[f32; 4]> = path_to_segments(d)
        .iter()
        .map(|s| {
            [
                tx + sx * (s[0] - vx),
                ty + sy * (s[1] - vy),
                tx + sx * (s[2] - vx),
                ty + sy * (s[3] - vy),
            ]
        })
        .collect();

    const SS: usize = 2; // 2×2 Subpixel
    let (wu, hu) = (w as usize, h as usize);
    let sh = hu * SS;
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); sh];
    for (idx, s) in segs.iter().enumerate() {
        let y0 = s[1].min(s[3]);
        let y1 = s[1].max(s[3]);
        if y1 - y0 < 1e-6 {
            continue;
        }
        let b0 = (y0 * SS as f32).floor().max(0.0) as usize;
        let b1 = ((y1 * SS as f32).ceil() as usize).min(sh);
        for bucket in &mut buckets[b0..b1] {
            bucket.push(idx as u32);
        }
    }

    let mut count = vec![0u8; wu * hu]; // Subpixel-Treffer pro Pixel (0..4)
    for py in 0..hu {
        for syi in 0..SS {
            let y = py as f32 + (syi as f32 + 0.5) / SS as f32;
            let mut xs: Vec<f32> = Vec::new();
            for &si in &buckets[py * SS + syi] {
                let s = &segs[si as usize];
                let (y0, y1) = (s[1], s[3]);
                if (y0 <= y && y < y1) || (y1 <= y && y < y0) {
                    xs.push(s[0] + (y - y0) * (s[2] - s[0]) / (y1 - y0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in xs.chunks_exact(2) {
                let (xa, xb) = (pair[0], pair[1]);
                let px0 = (xa.floor() as i64 - 1).max(0) as usize;
                let px1 = ((xb.ceil() as i64 + 1).max(0) as usize).min(wu);
                for px in px0..px1 {
                    for sxi in 0..SS {
                        let x = px as f32 + (sxi as f32 + 0.5) / SS as f32;
                        if x >= xa && x < xb {
                            count[py * wu + px] += 1;
                        }
                    }
                }
            }
        }
    }
    let alpha: Vec<u8> = count.iter().map(|c| (*c as u16 * 255 / 4) as u8).collect();
    KuhMask {
        width: w,
        height: h,
        alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maske_kommt_aus_svg_viewbox_und_ist_innen_deckend() {
        let m = kuh_mask();
        assert_eq!((m.width, m.height), (1280, 808), "viewBox-Auflösung");
        assert_eq!(m.alpha.len(), (1280 * 808) as usize);
        assert_eq!(m.alpha[0], 0, "Ecke (0,0) liegt außerhalb der Kuh");
        let innen = m.alpha.iter().filter(|&&a| a > 200).count();
        assert!(
            innen > m.alpha.len() / 5,
            "Kuh füllt einen Großteil: {innen}/{}",
            m.alpha.len()
        );
        // Irgendwo in der Mittelzeile ein voll deckendes Pixel.
        let row = &m.alpha[404 * 1280..405 * 1280];
        assert!(row.contains(&255), "Mittelzeile enthält deckende Pixel");
    }

    #[test]
    fn path_parser_flacht_kurven_und_schliesst_subpaths() {
        let segs = path_to_segments("M0 0 c10 0 10 10 0 10 l5 5 Z");
        // 24 Bézier-Segmente + 1 Gerade + 1 Schließgerade.
        assert_eq!(segs.len(), 24 + 1 + 1, "Segmente: {}", segs.len());
        let last = segs.last().unwrap();
        assert_eq!([last[2], last[3]], [0.0, 0.0], "Z schließt zum Anfang");
    }

    #[test]
    fn tokenizer_trennt_vorzeichengetrennte_zahlen() {
        let toks = tokenize("M1-2 3.5e1");
        let nums: Vec<f32> = toks
            .iter()
            .filter_map(|t| match t {
                Tok::Num(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(nums, vec![1.0, -2.0, 35.0]);
    }
}
