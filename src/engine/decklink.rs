//! DeckLink-Hardware-Ausgabe direkt über das Blackmagic SDK (Review G3/G6):
//! RGBA-Frames vom Compositor werden nach UYVY konvertiert und über die native
//! Schnittstelle (`decklink_native.rs`) auf die Karte gepusht – kein ffmpeg-
//! Ausgabepfad mehr (ffmpeg dient nur noch zum Dekodieren, G7).
//!
//! ```text
//! Compositor ──VideoFrame (RGBA)──▶ UYVY-Konvertierung ──▶ SDK 16.0 ──▶ SDI/HDMI
//! ```

use crate::core::ShowResult;
use crate::engine::frame::VideoFrame;

/// Konfiguration einer Decklink-Ausgabe.
#[derive(Debug, Clone)]
pub struct DecklinkConfig {
    /// Gerätename (Substring, case-insensitive; leer/„auto“ = erstes Gerät).
    pub device: String,
    /// Frame-Breite in Pixeln.
    pub width: u32,
    /// Frame-Höhe in Pixeln.
    pub height: u32,
    /// Framerate (fps; 2997/5994 = 29,97/59,94, siehe fps_frame_interval).
    pub fps: u32,
}

impl Default for DecklinkConfig {
    fn default() -> Self {
        Self {
            device: "DeckLink SDI".to_string(),
            width: 1920,
            height: 1080,
            fps: 25,
        }
    }
}

impl DecklinkConfig {
    /// Konfiguration aus Umgebungsvariablen (konsistent mit `KUHPLAY_ENGINE`):
    /// * `KUHPLAY_DECKLINK_DEVICE` – Gerätename; **nicht gesetzt → Decklink aus**
    /// * `KUHPLAY_DECKLINK_STAGE` – Stage-Name (Default „Stage 1")
    /// * `KUHPLAY_DECKLINK_WIDTH` / `KUHPLAY_DECKLINK_HEIGHT` / `KUHPLAY_DECKLINK_FPS`
    ///   (Default 1920×1080@25)
    /// * `KUHPLAY_DECKLINK_KEY_DEVICE` – zweiter Decklink-Port für den Key-SDI
    ///   (Key/Fill): Alpha der Key-Route wird als Luma darauf ausgegeben.
    ///
    /// Rückgabe: `None` wenn deaktiviert, sonst `(Config, Stage-Name)`.
    pub fn from_env() -> Option<(Self, String)> {
        let device = std::env::var("KUHPLAY_DECKLINK_DEVICE").ok()?;
        let u32_var = |name: &str, default: u32| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        let config = Self {
            device,
            width: u32_var("KUHPLAY_DECKLINK_WIDTH", 1920),
            height: u32_var("KUHPLAY_DECKLINK_HEIGHT", 1080),
            fps: u32_var("KUHPLAY_DECKLINK_FPS", 25),
        };
        let stage =
            std::env::var("KUHPLAY_DECKLINK_STAGE").unwrap_or_else(|_| "Stage 1".to_string());
        Some((config, stage))
    }
}

/// BMD-DisplayMode-FourCC für gängige Auflösungen/Frameraten
/// (Werte aus Blackmagic DeckLinkAPIModes.idl). `None` = unbekannt.
pub fn bmd_display_mode(width: u32, height: u32, fps: u32) -> Option<u32> {
    let m = match (width, height) {
        (1920, 1080) => match fps {
            24 => 0x32347073,        // '24ps'
            25 => 0x48703235,        // 'Hp25'
            30 => 0x48703330,        // 'Hp30'
            50 => 0x48703530,        // 'Hp50'
            60 => 0x48703630,        // 'Hp60'
            2997 | 29 => 0x48703239, // 'Hp29' (29.97)
            5994 | 59 => 0x48703539, // 'Hp59' (59.94)
            2398 | 23 => 0x32337073, // '23ps' (23.98)
            _ => return None,
        },
        (1280, 720) => match fps {
            50 => 0x68703530,        // 'hp50'
            60 => 0x68703630,        // 'hp60'
            5994 | 59 => 0x68703539, // 'hp59'
            _ => return None,
        },
        (3840, 2160) => match fps {
            2398 | 23 => 0x346B3233, // '4k23'
            24 => 0x346B3234,        // '4k24'
            25 => 0x346B3235,        // '4k25'
            2997 | 29 => 0x346B3239, // '4k29'
            30 => 0x346B3330,        // '4k30'
            50 => 0x346B3530,        // '4k50'
            5994 | 59 => 0x346B3539, // '4k59'
            60 => 0x346B3630,        // '4k60'
            _ => return None,
        },
        _ => return None,
    };
    Some(m)
}

/// Frame-Intervall aus der konfigurierten fps-Zahl (Review G2): 29/2997 →
/// 30000/1001 (29,97), 59/5994 → 60000/1001 (59,94), sonst exakt 1/fps.
pub fn fps_frame_interval(fps: u32) -> std::time::Duration {
    match fps {
        29 | 2997 => std::time::Duration::from_nanos(1_000_000_000 * 1001 / 30_000),
        59 | 5994 => std::time::Duration::from_nanos(1_000_000_000 * 1001 / 60_000),
        other => std::time::Duration::from_secs_f64(1.0 / f64::from(other.max(1))),
    }
}

/// (time_scale, frame_dur) für Scheduled Playback aus der fps-Zahl (H2), analog
/// zur Mode-Tabelle in `decklink_bridge.cpp`: 25 → (25000, 1000),
/// 29,97 → (30000, 1001), 50 → (50000, 1000), 59,94 → (60000, 1001).
pub fn scheduled_time_params(fps: u32) -> (i64, i64) {
    match fps {
        2398 => (24_000, 1_001),
        24 => (24_000, 1_000),
        25 => (25_000, 1_000),
        29 | 2997 => (30_000, 1_001),
        30 => (30_000, 1_000),
        50 => (50_000, 1_000),
        59 | 5994 => (60_000, 1_001),
        60 => (60_000, 1_000),
        other => {
            let f = i64::from(other.max(1));
            (f * 1_000, 1_000)
        }
    }
}

/// Wandelt ein RGBA-Frame in ein Key-Signal (Luma = Alpha): RGB = Alpha-Wert.
/// Für den Key-SDI-Ausgang beim 2-Kabel-Key/Fill.
pub fn alpha_to_luma(src: &VideoFrame) -> VideoFrame {
    let mut out = vec![0u8; src.rgba.len()];
    for (dst, src4) in out.chunks_exact_mut(4).zip(src.rgba.chunks_exact(4)) {
        let a = src4[3];
        dst[0] = a;
        dst[1] = a;
        dst[2] = a;
        dst[3] = 255;
    }
    VideoFrame::new(src.width, src.height, out)
}

/// Skaliert ein RGBA-Frame per Nearest-Neighbor auf `width`×`height`.
pub fn scale_nearest(src: &VideoFrame, width: u32, height: u32) -> VideoFrame {
    if src.width == width && src.height == height {
        return VideoFrame::new(width, height, src.rgba.clone());
    }
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let sy = (y as u64 * src.height as u64 / height as u64) as u32;
        for x in 0..width {
            let sx = (x as u64 * src.width as u64 / width as u64) as u32;
            let src_i = ((sy * src.width + sx) * 4) as usize;
            let dst_i = ((y * width + x) * 4) as usize;
            out[dst_i..dst_i + 4].copy_from_slice(&src.rgba[src_i..src_i + 4]);
        }
    }
    VideoFrame::new(width, height, out)
}

/// Wie [`rgba_to_uyvy422_cs`] mit BT.601.
pub fn rgba_to_uyvy422(src: &VideoFrame, dst: &mut [u8]) {
    rgba_to_uyvy422_cs(src, dst, ColorSpace::Bt601)
}

/// BMD-Pixelformat FourCC: 8-Bit-YUV (UYVY, '2vuy') – das Standardformat der
/// SDI-DeckLink-Karten.
pub const BMD_FORMAT_8BIT_YUV: u32 = 0x32767579;

/// Farbraum für die RGB→YUV-Konvertierung (Review G1): BT.709 für HD/UHD,
/// BT.601 nur für SD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    /// BT.601 (SD, bis 576 Zeilen).
    Bt601,
    /// BT.709 (HD ab 720 Zeilen und UHD).
    Bt709,
}

impl ColorSpace {
    /// Farbraum aus der Frame-Höhe: SD (< 720 Zeilen) → BT.601, sonst BT.709.
    pub fn from_height(height: u32) -> Self {
        if height < 720 {
            ColorSpace::Bt601
        } else {
            ColorSpace::Bt709
        }
    }
}

/// Konvertiert ein RGBA-Frame in UYVY 4:2:2 (Studio-Range, Farbraum über `cs`).
/// `dst` muss `width*height*2` Bytes haben. Alpha wird ignoriert (Fill opaque);
/// für den Key-Ausgang vorher [`alpha_to_luma`] anwenden (Y = Alpha).
pub fn rgba_to_uyvy422_cs(src: &VideoFrame, dst: &mut [u8], cs: ColorSpace) {
    let w = src.width as usize;
    let h = src.height as usize;
    assert!(dst.len() >= w * h * 2, "UYVY-Zielpuffer zu klein");
    for y in 0..h {
        for x in (0..w).step_by(2) {
            let i0 = (y * w + x) * 4;
            let i1 = (y * w + x + 1) * 4;
            let (r0, g0, b0) = (
                f32::from(src.rgba[i0]),
                f32::from(src.rgba[i0 + 1]),
                f32::from(src.rgba[i0 + 2]),
            );
            let (r1, g1, b1) = (
                f32::from(src.rgba[i1]),
                f32::from(src.rgba[i1 + 1]),
                f32::from(src.rgba[i1 + 2]),
            );
            // Studio-Range: Y in [16,235], U/V in [16,240]; U/V gemittelt über
            // das Pixelpaar (4:2:2), Y pro Pixel. Eingaben normiert 0..1,
            // Koeffizienten je Farbraum (Review G1): BT.601 (SD) vs. BT.709 (HD/UHD).
            let (kr, kg, kb) = match cs {
                ColorSpace::Bt601 => (0.299, 0.587, 0.114),
                ColorSpace::Bt709 => (0.2126, 0.7152, 0.0722),
            };
            let lum = |r: f32, g: f32, b: f32| {
                let (rn, gn, bn) = (r / 255.0, g / 255.0, b / 255.0);
                16.0 + 219.0 * (kr * rn + kg * gn + kb * bn)
            };
            let lum0 = lum(r0, g0, b0);
            let lum1 = lum(r1, g1, b1);
            let (rn, gn, bn) = ((r0 + r1) / 510.0, (g0 + g1) / 510.0, (b0 + b1) / 510.0);
            let u = 128.0
                + 224.0 * (-kr / (2.0 * (1.0 - kb)) * rn - kg / (2.0 * (1.0 - kb)) * gn + 0.5 * bn);
            let v = 128.0
                + 224.0 * (0.5 * rn - kg / (2.0 * (1.0 - kr)) * gn - kb / (2.0 * (1.0 - kr)) * bn);
            let o = (y * w + x) * 2;
            dst[o] = u.clamp(16.0, 240.0) as u8;
            dst[o + 1] = lum0.clamp(16.0, 235.0) as u8;
            dst[o + 2] = v.clamp(16.0, 240.0) as u8;
            dst[o + 3] = lum1.clamp(16.0, 235.0) as u8;
        }
    }
}

/// Geteilte DeckLink-Statistik (Engine → GUI/Remote), siehe EngineHandle.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecklinkStats {
    pub frames: u64,
    pub late: u64,
    pub dropped: u64,
    pub queued: u64,
}

/// Aktive DeckLink-Senke (G6: nur noch nativ über das SDK; Capture nur für Tests).
pub enum DecklinkSink {
    /// Native Ausgabe direkt über das Blackmagic SDK (alle Plattformen, G3).
    Native(crate::engine::decklink_native::NativeOutput),
    /// Nur Tests: zählt gepushte Frames, ohne Hardware.
    Capture(std::sync::Arc<std::sync::Mutex<u64>>),
}

impl DecklinkSink {
    /// Startet die native Senke.
    pub fn start(config: &DecklinkConfig) -> ShowResult<Self> {
        Ok(Self::Native(
            crate::engine::decklink_native::NativeOutput::open(config)?,
        ))
    }

    /// Test-Senke ohne Hardware: zählt Frames.
    pub fn capture() -> (Self, std::sync::Arc<std::sync::Mutex<u64>>) {
        let counter = std::sync::Arc::new(std::sync::Mutex::new(0u64));
        (Self::Capture(counter.clone()), counter)
    }

    /// Schreibt ein RGBA-Frame in die Senke.
    pub fn push_frame(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        match self {
            Self::Native(out) => out.push_frame(frame),
            Self::Capture(counter) => {
                *counter.lock().unwrap() += 1;
                Ok(())
            }
        }
    }

    /// Anzahl geschriebener Frames.
    pub fn frames_pushed(&self) -> u64 {
        match self {
            Self::Native(out) => out.frames_pushed(),
            Self::Capture(counter) => *counter.lock().unwrap(),
        }
    }

    /// Konfigurierte Frame-Breite.
    pub fn width(&self) -> u32 {
        match self {
            Self::Native(out) => out.width(),
            Self::Capture(_) => 0,
        }
    }

    /// Konfigurierte Frame-Höhe.
    pub fn height(&self) -> u32 {
        match self {
            Self::Native(out) => out.height(),
            Self::Capture(_) => 0,
        }
    }

    /// (late, dropped, queued) der nativen Senke.
    pub fn stats(&self) -> (u64, u64, u64) {
        match self {
            Self::Native(out) => out.stats(),
            Self::Capture(_) => (0, 0, 0),
        }
    }

    /// Startet Scheduled Playback auf der nativen Senke (G4), sonst Ok(()).
    pub fn start_scheduled(&mut self) -> ShowResult<()> {
        match self {
            Self::Native(out) => out.start_scheduled(),
            Self::Capture(_) => Ok(()),
        }
    }

    /// Läuft die Senke noch?
    pub fn is_running(&mut self) -> bool {
        match self {
            Self::Native(_) => true,
            Self::Capture(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_ist_1080p25() {
        let config = DecklinkConfig::default();
        assert_eq!((config.width, config.height, config.fps), (1920, 1080, 25));
    }

    #[test]
    fn scheduled_time_params_aus_bildrate() {
        assert_eq!(scheduled_time_params(25), (25_000, 1_000));
        assert_eq!(scheduled_time_params(2997), (30_000, 1_001));
        assert_eq!(scheduled_time_params(29), (30_000, 1_001));
        assert_eq!(scheduled_time_params(30), (30_000, 1_000));
        assert_eq!(scheduled_time_params(50), (50_000, 1_000));
        assert_eq!(scheduled_time_params(5994), (60_000, 1_001));
        assert_eq!(scheduled_time_params(59), (60_000, 1_001));
        assert_eq!(scheduled_time_params(60), (60_000, 1_000));
        assert_eq!(scheduled_time_params(2398), (24_000, 1_001));
        assert_eq!(scheduled_time_params(24), (24_000, 1_000));
        // Unbekannte Rate: ganzzahlig über fps*1000/1000 als Fallback.
        assert_eq!(scheduled_time_params(120), (120_000, 1_000));
        // Mit fps_frame_interval konsistent: frame_dur/time_scale == Intervall.
        for fps in [25u32, 2997, 50, 5994] {
            let (ts, dur) = scheduled_time_params(fps);
            let interv = fps_frame_interval(fps).as_nanos() as f64;
            let frame = dur as f64 / ts as f64 * 1e9;
            assert!(
                (frame - interv).abs() < 1.0,
                "fps {fps}: frame {frame} vs interval {interv}"
            );
        }
    }

    #[test]
    fn bmd_display_mode_gängige_modi() {
        assert_eq!(bmd_display_mode(1920, 1080, 25), Some(0x48703235)); // 'Hp25'
        assert_eq!(bmd_display_mode(1920, 1080, 50), Some(0x48703530)); // 'Hp50'
        assert_eq!(bmd_display_mode(1920, 1080, 60), Some(0x48703630)); // 'Hp60'
        assert_eq!(bmd_display_mode(1920, 1080, 2997), Some(0x48703239)); // 'Hp29'
        assert_eq!(bmd_display_mode(1280, 720, 50), Some(0x68703530)); // 'hp50'
        assert_eq!(bmd_display_mode(640, 480, 25), None);
        assert_eq!(bmd_display_mode(1920, 1080, 100), None);
        // G5: UHD 3840×2160 ergänzt.
        assert_eq!(bmd_display_mode(3840, 2160, 25), Some(0x346B3235)); // '4k25'
        assert_eq!(bmd_display_mode(3840, 2160, 50), Some(0x346B3530)); // '4k50'
        assert_eq!(bmd_display_mode(3840, 2160, 2997), Some(0x346B3239)); // '4k29'
        assert_eq!(bmd_display_mode(3840, 2160, 60), Some(0x346B3630)); // '4k60'
    }

    #[test]
    fn rgba_to_uyvy422_schwarz_weiss_und_grau() {
        // 2×1: links schwarz, rechts weiß → U/V ≈ 128, Y ≈ 16 bzw. 235.
        let rgba = vec![
            0, 0, 0, 255, 255, 255, 255, 255, // Pixel 0 schwarz, Pixel 1 weiß
        ];
        let frame = VideoFrame::new(2, 1, rgba);
        let mut dst = vec![0u8; 4];
        rgba_to_uyvy422(&frame, &mut dst);
        let avg_y = (f32::from(dst[1]) + f32::from(dst[3])) / 2.0;
        let u = f32::from(dst[0]);
        let v = f32::from(dst[2]);
        // Schwarz-Weiß-Mittel = Grau: Y ≈ (16+235)/2 ≈ 125.5, U/V ≈ 128.
        assert!((avg_y - 125.5).abs() < 2.0, "Y-Mittel {avg_y}");
        assert!((u - 128.0).abs() < 2.0, "U {u}");
        assert!((v - 128.0).abs() < 2.0, "V {v}");
        // Reines Weiß allein: Y ≈ 235, U/V ≈ 128.
        let white = VideoFrame::new(2, 1, vec![255, 255, 255, 255, 255, 255, 255, 255]);
        rgba_to_uyvy422(&white, &mut dst);
        assert!((f32::from(dst[1]) - 235.0).abs() < 2.0, "weiß Y {}", dst[1]);
    }

    #[test]
    fn scale_nearest_upscale_downscale() {
        // 2×2 mit vier eindeutigen Pixeln.
        let mut rgba = Vec::new();
        for px in [
            [255u8, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [9, 9, 9, 255],
        ] {
            rgba.extend_from_slice(&px);
        }
        let src = VideoFrame::new(2, 2, rgba);

        // Upscale 2×2 → 4×4: jeder Pixel wird 2×2.
        let up = scale_nearest(&src, 4, 4);
        assert_eq!((up.width, up.height), (4, 4));
        fn px(f: &VideoFrame, x: u32, y: u32) -> [u8; 4] {
            let i = ((y * f.width + x) * 4) as usize;
            [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2], f.rgba[i + 3]]
        }
        assert_eq!(px(&up, 0, 0), px(&up, 1, 1));
        assert_eq!(px(&up, 2, 0), px(&up, 3, 1));
        assert_eq!(px(&up, 0, 2), px(&up, 1, 3));
        assert_ne!(px(&up, 0, 0), px(&up, 2, 0));
        assert_ne!(px(&up, 0, 0), px(&up, 0, 2));

        // Downscale 4×4 → 2×2: Ecke links oben bleibt erhalten.
        let down = scale_nearest(&up, 2, 2);
        assert_eq!((down.width, down.height), (2, 2));
        assert_eq!(px(&down, 0, 0), px(&src, 0, 0));

        // Gleiche Größe → Inhalt identisch.
        let same = scale_nearest(&src, 2, 2);
        assert_eq!(same.rgba, src.rgba);
    }
}

#[cfg(test)]
mod keyfill_tests {
    use super::*;

    #[test]
    fn alpha_to_luma_maps_alpha_to_rgb() {
        let frame = VideoFrame::new(2, 1, vec![10, 20, 30, 0, 200, 100, 50, 255]);
        let key = alpha_to_luma(&frame);
        assert_eq!(&key.rgba[..4], &[0, 0, 0, 255], "Alpha 0 → schwarz");
        assert_eq!(&key.rgba[4..], &[255, 255, 255, 255], "Alpha 255 → weiß");
    }

    #[test]
    fn key_fill_routes_serde_and_validate() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let kf = crate::core::KeyFillRoutes { fill: a, key: b };
        let json = serde_json::to_string(&kf).unwrap();
        let back: crate::core::KeyFillRoutes = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kf);

        // Stage: key_fill default None (alte Shows), validate bei fill==key → Fehler.
        let stage = crate::core::Stage::new("S", crate::core::StageGeometry::new(100.0, 100.0));
        assert!(stage.key_fill.is_none());
        assert!(stage.validate().is_ok());
        let mut stage = stage;
        stage.key_fill = Some(crate::core::KeyFillRoutes { fill: a, key: a });
        assert!(stage.validate().is_err(), "fill == key ist ungültig");
    }
}

#[cfg(test)]
mod review_g1_tests {
    use super::*;

    fn uyvy_of(rgb: [u8; 3], cs: ColorSpace) -> [u8; 4] {
        let frame = VideoFrame::new(
            2,
            1,
            vec![rgb[0], rgb[1], rgb[2], 255, rgb[0], rgb[1], rgb[2], 255],
        );
        let mut dst = vec![0u8; 4];
        rgba_to_uyvy422_cs(&frame, &mut dst, cs);
        dst.try_into().unwrap()
    }

    #[test]
    fn g1_reines_rot_bt601_vs_bt709() {
        // BT.601: Y≈81, U≈90, V≈240 – BT.709: Y≈63, U≈102, V≈240 (Studio-Range).
        let bt601 = uyvy_of([255, 0, 0], ColorSpace::Bt601);
        let bt709 = uyvy_of([255, 0, 0], ColorSpace::Bt709);
        assert!(
            (bt601[1] as i32 - 81).abs() <= 2,
            "BT.601 Y rot = 81, got {}",
            bt601[1]
        );
        assert!(
            (bt601[0] as i32 - 90).abs() <= 2,
            "BT.601 U rot = 90, got {}",
            bt601[0]
        );
        assert!(
            (bt709[1] as i32 - 63).abs() <= 2,
            "BT.709 Y rot = 63, got {}",
            bt709[1]
        );
        assert!(
            (bt709[0] as i32 - 102).abs() <= 2,
            "BT.709 U rot = 102, got {}",
            bt709[0]
        );
        assert!(
            (bt601[2] as i32 - 240).abs() <= 2 && (bt709[2] as i32 - 240).abs() <= 2,
            "V rot = 240"
        );
    }

    #[test]
    fn g1_weiss_und_schwarz_gleich_in_beiden_farbraeumen() {
        for cs in [ColorSpace::Bt601, ColorSpace::Bt709] {
            let w = uyvy_of([255, 255, 255], cs);
            assert!(
                (w[1] as i32 - 235).abs() <= 2
                    && (w[0] as i32 - 128).abs() <= 2
                    && (w[2] as i32 - 128).abs() <= 2
            );
            let b = uyvy_of([0, 0, 0], cs);
            assert!((b[1] as i32 - 16).abs() <= 2 && (b[0] as i32 - 128).abs() <= 2);
        }
    }

    #[test]
    fn g1_from_height_sd_vs_hd() {
        assert_eq!(ColorSpace::from_height(576), ColorSpace::Bt601);
        assert_eq!(ColorSpace::from_height(720), ColorSpace::Bt709);
        assert_eq!(ColorSpace::from_height(1080), ColorSpace::Bt709);
        assert_eq!(ColorSpace::from_height(2160), ColorSpace::Bt709);
    }
}

#[cfg(test)]
mod review_g2_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn g2_frame_interval_bruchbildraten() {
        assert_eq!(fps_frame_interval(25), Duration::from_millis(40));
        // 29 und 2997 sind beide 29,97 fps → 30000/1001 s ≈ 33,367 ms.
        let expect_2997 = Duration::from_nanos(1_000_000_000 * 1001 / 30_000);
        assert_eq!(fps_frame_interval(29), expect_2997);
        assert_eq!(fps_frame_interval(2997), expect_2997);
        let expect_5994 = Duration::from_nanos(1_000_000_000 * 1001 / 60_000);
        assert_eq!(fps_frame_interval(59), expect_5994);
        assert_eq!(fps_frame_interval(5994), expect_5994);
        // pre-fix: 1/2997 s ≈ 0,333 ms – deutlich falsch.
        assert!(expect_2997 > Duration::from_millis(33));
        assert_eq!(fps_frame_interval(50), Duration::from_millis(20));
    }
}
