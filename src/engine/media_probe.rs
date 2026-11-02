//! Medienlänge ohne Abspielen: Ein Hintergrund-Thread fragt ffprobe, damit die
//! Cue-Liste die Dauer von Anfang an zeigt und das Fenster dabei frei bleibt.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;

use super::video_backend::VideoBackend;

/// Pfad → Länge. `None` = noch unterwegs oder keine lesbare Mediendatei.
#[derive(Debug, Default)]
pub struct MediaDurations {
    known: Arc<Mutex<HashMap<String, Option<Duration>>>>,
    queue: Mutex<Option<Sender<String>>>,
}

impl MediaDurations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bekannte Länge; ein neuer Pfad wird im Hintergrund nachgefragt.
    pub fn request(&self, path: &str) -> Option<Duration> {
        let mut known = self.known.lock().unwrap();
        if let Some(duration) = known.get(path) {
            return *duration;
        }
        known.insert(path.to_string(), None);
        drop(known);
        self.send(path);
        None
    }

    fn send(&self, path: &str) {
        let mut queue = self.queue.lock().unwrap();
        let sender = queue.get_or_insert_with(|| {
            let (tx, rx) = crossbeam_channel::unbounded::<String>();
            let known = self.known.clone();
            // Endet von selbst, wenn der Sender mit der App verschwindet.
            std::thread::spawn(move || {
                for path in rx {
                    let duration = probe_duration(&path);
                    known.lock().unwrap().insert(path, duration);
                }
            });
            tx
        });
        let _ = sender.send(path.to_string());
    }
}

/// Länge laut Container, für Ton- und Videodateien.
fn probe_duration(path: &str) -> Option<Duration> {
    let output = Command::new(VideoBackend::ffprobe_binary())
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            path,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let seconds: f64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()?;
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_has_no_duration() {
        assert_eq!(probe_duration("/gibt/es/nicht.mp4"), None);
    }

    #[test]
    fn duration_arrives_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mkv");
        let made = Command::new(VideoBackend::ffmpeg_binary())
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=3")
            .arg(&path)
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return; // ffmpeg fehlt
        }
        let path = path.to_string_lossy().into_owned();
        let durations = MediaDurations::new();
        assert_eq!(durations.request(&path), None, "kehrt sofort zurück");
        let start = std::time::Instant::now();
        let found = loop {
            if let Some(found) = durations.request(&path) {
                break found;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "keine Dauer");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!((2.5..3.5).contains(&found.as_secs_f32()), "{found:?}");
    }
}
