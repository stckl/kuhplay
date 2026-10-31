//! Decklink-Hardware-Ausgabe: RGBA-Frames vom Compositor werden als
//! `rawvideo`-Pipe an einen ffmpeg-Subprozess gegeben, der sie über den
//! `decklink`-Muxer auf eine Blackmagic-Decklink-Karte (SDI/HDMI) ausgibt.
//!
//! ```text
//! Compositor ──VideoFrame (RGBA)──▶ ffmpeg stdin ──▶ Decklink SDI/HDMI
//!
//! ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r FPS -i - \
//!        -f decklink -pix_fmt uyvy422 "DeckLink SDI"
//! ```
//!
//! ## Hinweise
//! * Erfordert ffmpeg mit dem `decklink`-Muxer (in den meisten
//!   Standard-Builds enthalten; Windows braucht ggf. einen Spezial-Build).
//! * Ohne angeschlossene Hardware startet ffmpeg dennoch – die Karte wird
//!   erst beim ersten Frame angesprochen.
//! * `Drop` killt den Subprozess (kein Zombie).

use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::core::{ShowError, ShowResult};
use crate::engine::frame::VideoFrame;

/// Konfiguration einer Decklink-Ausgabe.
#[derive(Debug, Clone)]
pub struct DecklinkConfig {
    /// Gerätename für den ffmpeg-decklink-Muxer (z. B. „DeckLink SDI").
    pub device: String,
    /// Frame-Breite in Pixeln.
    pub width: u32,
    /// Frame-Höhe in Pixeln.
    pub height: u32,
    /// Framerate (fps).
    pub fps: u32,
    /// Pixelformat des Decklink-Muxers (SDI-Karten: meist „uyvy422").
    pub pix_fmt: String,
}

impl Default for DecklinkConfig {
    fn default() -> Self {
        Self {
            device: "DeckLink SDI".to_string(),
            width: 1920,
            height: 1080,
            fps: 25,
            pix_fmt: "uyvy422".to_string(),
        }
    }
}

impl DecklinkConfig {
    /// Konfiguration aus Umgebungsvariablen (konsistent mit `KUHPLAY_ENGINE`):
    /// * `KUHPLAY_DECKLINK_DEVICE` – Gerätename; **nicht gesetzt → Decklink aus**
    /// * `KUHPLAY_DECKLINK_STAGE` – Stage-Name (Default „Stage 1")
    /// * `KUHPLAY_DECKLINK_WIDTH` / `KUHPLAY_DECKLINK_HEIGHT` / `KUHPLAY_DECKLINK_FPS`
    ///   (Default 1920×1080@25)
    /// * `KUHPLAY_DECKLINK_PIXFMT` – Pixelformat des Muxers (Default „uyvy422")
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
            pix_fmt: std::env::var("KUHPLAY_DECKLINK_PIXFMT")
                .unwrap_or_else(|_| "uyvy422".to_string()),
        };
        let stage =
            std::env::var("KUHPLAY_DECKLINK_STAGE").unwrap_or_else(|_| "Stage 1".to_string());
        Some((config, stage))
    }
}

/// Baut die ffmpeg-Kommandozeile für eine Decklink-Ausgabe:
/// rawvideo/rgba von stdin → decklink-Muxer auf das Gerät.
pub fn ffmpeg_decklink_args(config: &DecklinkConfig) -> Vec<String> {
    vec![
        // Input: rawvideo/rgba von stdin.
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-s".into(),
        format!("{}x{}", config.width, config.height),
        "-r".into(),
        config.fps.to_string(),
        "-i".into(),
        "-".into(),
        // Output: Decklink-Muxer auf das Gerät.
        "-f".into(),
        "decklink".into(),
        "-pix_fmt".into(),
        config.pix_fmt.clone(),
        config.device.clone(),
    ]
}

/// Skaliert ein RGBA-Frame per Nearest-Neighbor auf `width`×`height`
/// (Stage-Canvas ≠ Karten-Auflösung; CPU-Scaling, V1 – kein GPU-Upscale).
/// Wandelt ein RGBA-Frame in ein Key-Signal (Luma = Alpha): RGB = Alpha-Wert.
/// Für den Key-SDI-Ausgang beim 2-Kabel-Key/Fill (Fill auf anderem Port).
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

/// Laufende Decklink-Ausgabe (ffmpeg-Subprocess mit Frame-Pipe).
pub struct DecklinkOutput {
    child: Child,
    stdin: ChildStdin,
    /// Erwartete Frame-Größe (für Validierung in `push_frame`).
    width: u32,
    height: u32,
    /// Anzahl geschriebener Frames.
    frames_pushed: u64,
    /// Anzahl geschriebener Bytes.
    bytes_pushed: u64,
}

impl DecklinkOutput {
    /// Startet ffmpeg mit der Decklink-Konfiguration.
    pub fn start(config: &DecklinkConfig) -> ShowResult<Self> {
        let args = ffmpeg_decklink_args(config);
        tracing::info!(
            device = %config.device,
            "{}x{}@{} → decklink",
            config.width,
            config.height,
            config.fps
        );
        Self::spawn("ffmpeg", &args, config.width, config.height)
    }

    /// Startet ein beliebiges Programm mit Args und stdin-Pipe (für Tests:
    /// z. B. `cat` oder ffmpeg mit `-f null -` statt Decklink).
    pub fn spawn(program: &str, args: &[String], width: u32, height: u32) -> ShowResult<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                ShowError::Engine(format!("{program} konnte nicht gestartet werden: {e}"))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ShowError::Engine(format!("{program}: stdin-Pipe nicht verfügbar")))?;
        Ok(Self {
            child,
            stdin,
            width,
            height,
            frames_pushed: 0,
            bytes_pushed: 0,
        })
    }

    /// Schreibt ein RGBA-Frame in die ffmpeg-Pipe. Die Frame-Größe muss
    /// zur Konfiguration passen (sonst `Err` – kein stiller Datenmüll).
    pub fn push_frame(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        if frame.width != self.width || frame.height != self.height {
            return Err(ShowError::Engine(format!(
                "frame {}x{} passt nicht zur Decklink-Ausgabe {}x{}",
                frame.width, frame.height, self.width, self.height
            )));
        }
        self.stdin.write_all(&frame.rgba).map_err(|e| {
            ShowError::Engine(format!(
                "decklink: schreiben auf ffmpeg-stdin fehlgeschlagen: {e}"
            ))
        })?;
        self.frames_pushed += 1;
        self.bytes_pushed += frame.rgba.len() as u64;
        Ok(())
    }

    /// Anzahl geschriebener Frames.
    pub fn frames_pushed(&self) -> u64 {
        self.frames_pushed
    }

    /// Anzahl geschriebener Bytes.
    pub fn bytes_pushed(&self) -> u64 {
        self.bytes_pushed
    }

    /// Konfigurierte Frame-Breite.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Konfigurierte Frame-Höhe.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// PID des ffmpeg-Subprocess.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Läuft der Subprocess noch?
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for DecklinkOutput {
    fn drop(&mut self) {
        // ffmpeg beenden, sonst Zombie-Prozess.
        let _ = self.child.kill();
        let _ = self.child.wait();
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
            50 => 0x48703530,        // 'Hp50' (progressive 50)
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
        _ => return None,
    };
    Some(m)
}

/// BMD-Pixelformat FourCC: 8-Bit-YUV (UYVY, '2vuy') – das Standardformat der
/// SDI-Decklink-Karten.
pub const BMD_FORMAT_8BIT_YUV: u32 = 0x32767579;

/// Konvertiert ein RGBA-Frame in UYVY 4:2:2 (BT.601 Studio-Range).
/// `dst` muss `width*height*2` Bytes haben. Alpha wird ignoriert (Fill opaque);
/// für den Key-Ausgang vorher [`alpha_to_luma`] anwenden (Y = Alpha).
pub fn rgba_to_uyvy422(src: &VideoFrame, dst: &mut [u8]) {
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
            // BT.601 Studio: Y in [16,235], U/V in [16,240]; U/V gemittelt über
            // das Pixelpaar (4:2:2), Y pro Pixel.
            let lum = |r: f32, g: f32, b: f32| {
                16.0 + (65.481 * r + 128.553 * g + 24.966 * b) * (219.0 / 255.0)
            };
            let lum0 = lum(r0, g0, b0);
            let lum1 = lum(r1, g1, b1);
            let r = (r0 + r1) / 2.0;
            let g = (g0 + g1) / 2.0;
            let b = (b0 + b1) / 2.0;
            let u = 128.0 + (-37.797 * r - 74.203 * g + 112.0 * b) * (224.0 / 255.0);
            let v = 128.0 + (112.0 * r - 93.786 * g - 18.214 * b) * (224.0 / 255.0);
            let o = (y * w + x) * 2;
            dst[o] = u.clamp(16.0, 240.0) as u8;
            dst[o + 1] = lum0.clamp(16.0, 235.0) as u8;
            dst[o + 2] = v.clamp(16.0, 240.0) as u8;
            dst[o + 3] = lum1.clamp(16.0, 235.0) as u8;
        }
    }
}

/// Aktive Decklink-Senke: nativ per COM (Windows, bevorzugt) oder per
/// ffmpeg-Subprocess mit `decklink`-Muxer. Auswahl über
/// `KUHPLAY_DECKLINK_BACKEND` = `auto` (Default; Windows: COM zuerst) | `com` | `ffmpeg`.
pub enum DecklinkSink {
    /// ffmpeg-Subprocess mit `-f decklink` (Fallback / Nicht-Windows).
    Ffmpeg(DecklinkOutput),
    /// Native COM-Ausgabe direkt auf die Karte (nur Windows).
    #[cfg(target_os = "windows")]
    Com(crate::engine::decklink_com::ComDeckLinkOutput),
}

impl DecklinkSink {
    /// Startet die Senke passend zur Plattform/Konfiguration.
    pub fn start(config: &DecklinkConfig) -> ShowResult<Self> {
        #[cfg(target_os = "windows")]
        {
            let backend =
                std::env::var("KUHPLAY_DECKLINK_BACKEND").unwrap_or_else(|_| "auto".into());
            if backend == "auto" || backend == "com" {
                match crate::engine::decklink_com::ComDeckLinkOutput::start(config) {
                    Ok(out) => {
                        tracing::info!(device = %config.device, "decklink: nativ per COM geöffnet");
                        return Ok(Self::Com(out));
                    }
                    Err(err) => {
                        if backend == "com" {
                            return Err(err);
                        }
                        tracing::warn!(%err, "decklink COM nicht verfügbar – Fallback ffmpeg");
                    }
                }
            }
        }
        Ok(Self::Ffmpeg(DecklinkOutput::start(config)?))
    }

    /// Schreibt ein RGBA-Frame in die Senke.
    pub fn push_frame(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        match self {
            Self::Ffmpeg(out) => out.push_frame(frame),
            #[cfg(target_os = "windows")]
            Self::Com(out) => out.push_frame(frame),
        }
    }

    /// Anzahl geschriebener Frames.
    pub fn frames_pushed(&self) -> u64 {
        match self {
            Self::Ffmpeg(out) => out.frames_pushed(),
            #[cfg(target_os = "windows")]
            Self::Com(out) => out.frames_pushed(),
        }
    }

    /// Konfigurierte Frame-Breite.
    pub fn width(&self) -> u32 {
        match self {
            Self::Ffmpeg(out) => out.width(),
            #[cfg(target_os = "windows")]
            Self::Com(out) => out.width(),
        }
    }

    /// Konfigurierte Frame-Höhe.
    pub fn height(&self) -> u32 {
        match self {
            Self::Ffmpeg(out) => out.height(),
            #[cfg(target_os = "windows")]
            Self::Com(out) => out.height(),
        }
    }

    /// Läuft die Senke noch?
    pub fn is_running(&mut self) -> bool {
        match self {
            Self::Ffmpeg(out) => out.is_running(),
            #[cfg(target_os = "windows")]
            Self::Com(out) => out.is_running(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_args_enthalten_rawvideo_input_und_decklink_sink() {
        let config = DecklinkConfig {
            device: "DeckLink Mini".to_string(),
            width: 1280,
            height: 720,
            fps: 50,
            pix_fmt: "uyvy422".to_string(),
        };
        let args = ffmpeg_decklink_args(&config);
        let joined = args.join(" ");
        assert!(joined.contains("-f rawvideo"));
        assert!(joined.contains("-pix_fmt rgba"));
        assert!(joined.contains("-s 1280x720"));
        assert!(joined.contains("-r 50"));
        assert!(joined.contains("-f decklink"));
        assert!(joined.contains("-pix_fmt uyvy422"));
        assert_eq!(args.last().unwrap(), "DeckLink Mini");
        // Input kommt von stdin ("-").
        assert!(args.contains(&"-".to_string()));
    }

    #[test]
    fn default_config_ist_1080p25() {
        let config = DecklinkConfig::default();
        assert_eq!((config.width, config.height, config.fps), (1920, 1080, 25));
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
