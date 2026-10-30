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
