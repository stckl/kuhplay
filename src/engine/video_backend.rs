//! Echtes Video-Backend: Dekodierung über den externen **ffmpeg**-Prozess.
//!
//! ## Architektur
//!
//! * `ffprobe` liefert Metadaten (Auflösung, Framerate, Dauer).
//! * `ffmpeg -f rawvideo -pix_fmt rgba -` dekodiert auf stdout; ein
//!   **Reader-Thread** liest RGBA-Frames (frame-genau: `w*h*4` Bytes) und
//!   pacet sie über eine virtuelle Clock (`started_at + pausiert + n/fps`) –
//!   unabhängig vom Decode-Durchsatz (ffmpeg läuft so schnell wie möglich,
//!   der Reader taktet die Ausgabe).
//! * Jedes angezeigte Frame landet als `Arc<VideoFrame>` im [`FrameStore`]
//!   (GUI/Compositor lesen daraus).
//! * Steuerung (Pause/Resume/Seek/Stop) über ein Control-Channel; Seek =
//!   Kind-Prozess töten und mit `-ss` neu starten.
//!
//! ## Erweiterung
//!
//! * GPU-Dekodierung (`-hwaccel auto`) + Zero-Copy-Upload in eine Textur.
//! * Mehrere parallele Streams pro Cue (Slices/Play-Counts).
//! * NDI-Aufnahme als Quelle (statt Datei).

use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::frame::{FrameStore, VideoFrame};

/// Steuerbefehle an einen Video-Player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoControl {
    Pause,
    Resume,
    Stop,
    /// Springt an `position` (Kind-Prozess neu starten mit `-ss`).
    Seek(Duration),
}

/// Gemeinsamer Player-Zustand (Reader-Thread ↔ Engine).
struct PlayerState {
    /// Angezeigte Frames (für Position + EOF-Erkennung).
    frames_shown: u64,
    /// Pausiert?
    paused: bool,
    /// Virtuelle Uhr: Startzeit + akkumulierte Pausenzeit.
    started_at: Instant,
    paused_accum: Duration,
    paused_since: Option<Instant>,
    /// Reader hat EOF (Pipe geschlossen).
    eof: bool,
}

impl PlayerState {
    fn new() -> Self {
        Self {
            frames_shown: 0,
            paused: false,
            started_at: Instant::now(),
            paused_accum: Duration::ZERO,
            paused_since: None,
            eof: false,
        }
    }
}

/// Ein laufender Video-Player.
struct VideoPlayer {
    state: Arc<Mutex<PlayerState>>,
    control: crossbeam_channel::Sender<VideoControl>,
    // Kind-Prozess-Lebenszeit.
    child: Arc<Mutex<Option<Child>>>,
}

/// Videometadaten (aus ffprobe).
#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: Option<Duration>,
}

/// Das Video-Backend: ffmpeg-Prozesse + Frame-Store.
pub struct VideoBackend {
    players: HashMap<Uuid, VideoPlayer>,
    frames: FrameStore,
    /// Frames pro Sekunde des letzten Starts (für Position).
    fps: HashMap<Uuid, f64>,
}

impl VideoBackend {
    pub fn new(frames: FrameStore) -> Self {
        Self {
            players: HashMap::new(),
            frames,
            fps: HashMap::new(),
        }
    }

    /// Woher kommt das ffmpeg-Binary?
    /// 1. `KUHPLAY_FFMPEG` (voller Pfad),
    /// 2. `ffmpeg`/`ffmpeg.exe` **neben unserer eigenen exe** (gebundelt mit ausliefern),
    /// 3. `ffmpeg` aus dem PATH.
    pub fn ffmpeg_binary() -> String {
        if let Ok(path) = std::env::var("KUHPLAY_FFMPEG") {
            if !path.trim().is_empty() {
                return path;
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let name = if cfg!(windows) {
                    "ffmpeg.exe"
                } else {
                    "ffmpeg"
                };
                let sibling = dir.join(name);
                if sibling.is_file() {
                    return sibling.display().to_string();
                }
            }
        }
        "ffmpeg".to_string()
    }

    /// Startet einen Player für `path` (ffmpeg-Dekodierung).
    /// `seek` = Startposition. Liefert die Metadaten.
    pub fn play(&mut self, cue_id: Uuid, path: &str, seek: Duration) -> Result<VideoInfo, String> {
        self.stop(cue_id);
        let info = probe(path)?;
        let state = Arc::new(Mutex::new(PlayerState::new()));
        let (tx, rx) = crossbeam_channel::unbounded::<VideoControl>();
        let child = Arc::new(Mutex::new(None::<Child>));
        self.spawn_ffmpeg(cue_id, path, seek, &info, state.clone(), rx, child.clone());
        self.fps.insert(cue_id, info.fps.max(1.0));
        self.players.insert(
            cue_id,
            VideoPlayer {
                state,
                control: tx,
                child,
            },
        );
        Ok(info)
    }

    /// Startet (bzw. restartet) den ffmpeg-Prozess + Reader-Thread.
    // Viele Argumente: alles Pipe-/Thread-Plumbing, eine Params-Struct lohnt
    // für diese interne Hilfsfunktion nicht.
    #[allow(clippy::too_many_arguments)]
    fn spawn_ffmpeg(
        &self,
        cue_id: Uuid,
        path: &str,
        seek: Duration,
        info: &VideoInfo,
        state: Arc<Mutex<PlayerState>>,
        control: crossbeam_channel::Receiver<VideoControl>,
        child_slot: Arc<Mutex<Option<Child>>>,
    ) {
        let mut cmd = Command::new(Self::ffmpeg_binary());
        for arg in ffmpeg_decode_args(seek, path, hwaccel_enabled()) {
            cmd.arg(arg);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        let stdout = match cmd.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take();
                *child_slot.lock().unwrap() = Some(child);
                stdout
            }
            Err(err) => {
                tracing::warn!(path, %err, "ffmpeg spawn failed");
                state.lock().unwrap().eof = true;
                return;
            }
        };
        let frames = self.frames.clone();
        let w = info.width;
        let h = info.height;
        let fps = info.fps.max(1.0);
        std::thread::Builder::new()
            .name(format!("video-reader-{cue_id}"))
            .spawn(move || {
                reader_loop(
                    cue_id, stdout, w, h, fps, frames, state, control, child_slot,
                );
            })
            .expect("spawn video reader thread");
    }

    pub fn pause(&mut self, cue_id: Uuid) {
        if let Some(p) = self.players.get(&cue_id) {
            let mut s = p.state.lock().unwrap();
            if !s.paused {
                s.paused = true;
                s.paused_since = Some(Instant::now());
            }
        }
    }

    pub fn resume(&mut self, cue_id: Uuid) {
        if let Some(p) = self.players.get(&cue_id) {
            let mut s = p.state.lock().unwrap();
            if s.paused {
                if let Some(since) = s.paused_since.take() {
                    s.paused_accum += since.elapsed();
                }
                s.paused = false;
            }
        }
    }

    /// Stoppt einen Player (Kind-Prozess + Reader-Thread beenden).
    pub fn stop(&mut self, cue_id: Uuid) {
        if let Some(p) = self.players.remove(&cue_id) {
            let _ = p.control.send(VideoControl::Stop);
            if let Some(mut child) = p.child.lock().unwrap().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        self.fps.remove(&cue_id);
        self.frames.lock().unwrap().remove(&cue_id);
    }

    /// Springt an `position`: umgesetzt als `stop()` + `play()` (ffmpeg-Neustart
    /// mit `-ss`) – auf Engine-Ebene, damit der neue Reader-Thread sauber läuft.
    /// Medien-Position (aus den angezeigten Frames).
    pub fn position(&self, cue_id: Uuid) -> Option<Duration> {
        let fps = *self.fps.get(&cue_id)?;
        let frames = self
            .players
            .get(&cue_id)?
            .state
            .lock()
            .unwrap()
            .frames_shown;
        Some(Duration::from_secs_f64(frames as f64 / fps))
    }

    /// `true`, wenn der Reader EOF erreicht hat (Dateiende).
    pub fn is_eof(&self, cue_id: Uuid) -> bool {
        self.players
            .get(&cue_id)
            .map(|p| p.state.lock().unwrap().eof)
            .unwrap_or(false)
    }

    /// Pausiert?
    pub fn is_paused(&self, cue_id: Uuid) -> bool {
        self.players
            .get(&cue_id)
            .map(|p| p.state.lock().unwrap().paused)
            .unwrap_or(false)
    }

    /// Anzahl laufender Player.
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

/// Liest RGBA-Frames aus der ffmpeg-Pipe und pacet sie frame-genau.
// Viele Argumente: alles Thread-/Pipe-Plumbing (siehe `spawn_ffmpeg`).
#[allow(clippy::too_many_arguments)]
fn reader_loop(
    cue_id: Uuid,
    stdout: Option<std::process::ChildStdout>,
    w: u32,
    h: u32,
    fps: f64,
    frames: FrameStore,
    state: Arc<Mutex<PlayerState>>,
    control: crossbeam_channel::Receiver<VideoControl>,
    child_slot: Arc<Mutex<Option<Child>>>,
) {
    let mut stdout = match stdout {
        Some(stdout) => stdout,
        None => {
            state.lock().unwrap().eof = true;
            return;
        }
    };
    let frame_bytes = (w as usize) * (h as usize) * 4;
    let mut buf = vec![0u8; frame_bytes];
    let frame_dur = Duration::from_secs_f64(1.0 / fps);

    'session: loop {
        // Control bedienen (Seek → Session neu starten, Stop → raus).
        while let Ok(cmd) = control.try_recv() {
            match cmd {
                VideoControl::Stop => {
                    kill_child(&child_slot);
                    state.lock().unwrap().eof = true;
                    return;
                }
                // Seek wird von der Engine als stop()+play() umgesetzt
                // (ffmpeg-Neustart mit -ss); der Reader räumt hier nur auf.
                VideoControl::Seek(_) => {
                    kill_child(&child_slot);
                    state.lock().unwrap().eof = true;
                    return;
                }
                VideoControl::Pause => {
                    let mut s = state.lock().unwrap();
                    if !s.paused {
                        s.paused = true;
                        s.paused_since = Some(Instant::now());
                    }
                }
                VideoControl::Resume => {
                    let mut s = state.lock().unwrap();
                    if s.paused {
                        if let Some(since) = s.paused_since.take() {
                            s.paused_accum += since.elapsed();
                        }
                        s.paused = false;
                    }
                }
            }
        }

        // Nächstes Frame lesen (blockierend).
        match stdout.read_exact(&mut buf) {
            Ok(()) => {}
            Err(_) => break, // EOF oder Prozess beendet
        }

        // Frame-genaues Pacing: virtuelle Uhr.
        {
            let s = state.lock().unwrap();
            if s.paused {
                drop(s);
                // Pausiert: Frame verwerfen? Nein – wir halten das Frame und
                // warten, bis die Pause endet (Frame bleibt stehen).
                loop {
                    while let Ok(cmd) = control.try_recv() {
                        match cmd {
                            VideoControl::Stop => {
                                kill_child(&child_slot);
                                state.lock().unwrap().eof = true;
                                return;
                            }
                            VideoControl::Seek(_) => {
                                kill_child(&child_slot);
                                state.lock().unwrap().eof = true;
                                return;
                            }
                            // (Seek → Engine: stop()+play())
                            VideoControl::Pause => {}
                            VideoControl::Resume => {
                                let mut s = state.lock().unwrap();
                                if s.paused {
                                    if let Some(since) = s.paused_since.take() {
                                        s.paused_accum += since.elapsed();
                                    }
                                    s.paused = false;
                                }
                                break;
                            }
                        }
                    }
                    if !state.lock().unwrap().paused {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                continue 'session; // Frame neu lesen (nächstes)
            }
            let target = s.started_at + s.paused_accum + frame_dur.mul_f64(s.frames_shown as f64);
            drop(s);
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }
        }

        // Frame in den Store (einzige Allocation: RGBA→Vec).
        let frame = VideoFrame::new(w, h, buf.clone());
        frames.lock().unwrap().insert(cue_id, Arc::new(frame));
        let mut s = state.lock().unwrap();
        s.frames_shown += 1;
    }

    // EOF erreicht.
    kill_child(&child_slot);
    state.lock().unwrap().eof = true;
}

/// Argumente für den ffmpeg-Dekodierprozess (rawvideo/rgba auf stdout).
///
/// `hwaccel`: Hardware-Dekodierung versuchen (`-hwaccel auto` — je nach
/// Plattform VideoToolBox/NVDEC/VAAPI/DXVA, automatischer Fallback auf
/// Software). Die Frames werden für die rgba-Ausgabe automatisch vom GPU
/// heruntergeholt; das CPU-Compositing bleibt davon unberührt.
pub fn ffmpeg_decode_args(seek: Duration, path: &str, hwaccel: bool) -> Vec<String> {
    let mut args = vec![
        "-nostdin".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
    ];
    if hwaccel {
        // hwaccel als Input-Option, damit auch der Seek hw-beschleunigt ist.
        args.push("-hwaccel".to_string());
        args.push("auto".to_string());
    }
    args.extend([
        "-ss".to_string(),
        format!("{:.6}", seek.as_secs_f64()),
        "-i".to_string(),
        path.to_string(),
        "-an".to_string(),
        "-f".to_string(),
        "rawvideo".to_string(),
        "-pix_fmt".to_string(),
        "rgba".to_string(),
        // Frames mit Original-Timing durchreichen.
        "-fps_mode".to_string(),
        "passthrough".to_string(),
        "-".to_string(),
    ]);
    args
}

/// `KUHPLAY_HWACCEL=off` schaltet die Hardware-Dekodierung ab (Default: an,
/// ffmpeg fällt bei fehlender Hardware automatisch auf Software zurück).
pub fn hwaccel_enabled() -> bool {
    std::env::var("KUHPLAY_HWACCEL")
        .map(|v| !matches!(v.as_str(), "0" | "off" | "false"))
        .unwrap_or(true)
}

/// Tötet den Kind-Prozess (falls noch laufend).
fn kill_child(child_slot: &Arc<Mutex<Option<Child>>>) {
    if let Some(mut child) = child_slot.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// ffprobe: Auflösung, Framerate, Dauer des ersten Video-Streams.
pub fn probe(path: &str) -> Result<VideoInfo, String> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate,duration",
            "-of",
            "json",
            path,
        ])
        .output()
        .map_err(|e| format!("ffprobe spawn failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe failed for {path}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe json: {e}"))?;
    let stream = &json["streams"][0];
    let width = stream["width"].as_u64().ok_or("ffprobe: no width")? as u32;
    let height = stream["height"].as_u64().ok_or("ffprobe: no height")? as u32;
    let fps = parse_fps(
        stream["r_frame_rate"]
            .as_str()
            .ok_or("ffprobe: no r_frame_rate")?,
    );
    let duration = stream["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .map(Duration::from_secs_f64);
    Ok(VideoInfo {
        width,
        height,
        fps,
        duration,
    })
}

/// Parst ffprobe-FPS-Formate: `"30/1"`, `"30000/1001"`, `"25"`.
fn parse_fps(s: &str) -> f64 {
    if let Some((num, den)) = s.split_once('/') {
        let num: f64 = num.parse().unwrap_or(0.0);
        let den: f64 = den.parse().unwrap_or(1.0);
        if den > 0.0 {
            return num / den;
        }
        0.0
    } else {
        s.parse().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod hwaccel_tests {
    use super::*;

    #[test]
    fn decode_args_contain_hwaccel_when_enabled() {
        let args = ffmpeg_decode_args(Duration::from_secs(2), "/tmp/x.mp4", true);
        let pos = args.iter().position(|a| a == "-hwaccel").unwrap();
        assert_eq!(args[pos + 1], "auto");
        // hwaccel ist Input-Option: vor -i stehen.
        let i_pos = args.iter().position(|a| a == "-i").unwrap();
        assert!(pos < i_pos);
        // Ausgabe bleibt rawvideo/rgba auf stdout.
        assert!(args.contains(&"rawvideo".to_string()));
        assert!(args.contains(&"rgba".to_string()));
        assert_eq!(args.last().unwrap(), "-");
    }

    #[test]
    fn decode_args_without_hwaccel() {
        let args = ffmpeg_decode_args(Duration::ZERO, "/tmp/x.mp4", false);
        assert!(!args.contains(&"-hwaccel".to_string()));
        assert!(args.contains(&"-ss".to_string()));
    }
}

#[cfg(test)]
mod ffmpeg_binary_tests {
    use super::VideoBackend;

    #[test]
    fn ffmpeg_binary_env_override() {
        // Wenn KUHPLAY_FFMPEG gesetzt ist, gewinnt das.
        std::env::set_var("KUHPLAY_FFMPEG", "/opt/custom/ffmpeg");
        assert_eq!(VideoBackend::ffmpeg_binary(), "/opt/custom/ffmpeg");
        std::env::remove_var("KUHPLAY_FFMPEG");
        // Ohne env: sibling neben der exe ODER PATH-Fallback "ffmpeg".
        let b = VideoBackend::ffmpeg_binary();
        assert!(b == "ffmpeg" || b.ends_with("ffmpeg") || b.ends_with("ffmpeg.exe"));
    }
}
