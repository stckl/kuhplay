//! SMPTE-Timecode (`HH:MM:SS:FF`) als reiner Werttyp.
//!
//! Grundlage für **Timecode-Chasing** (LTC/MTC): Cues koppeln sich an eine
//! externe Clock; bei Timecode-Sprüngen (vor-/rückwärts) springt das Medium
//! exakt auf die対応ende Millisekunde. Reine Mathematik, keine I/O –
//! daher vollständig unit-testbar.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Bildrate des Timecodes (Drop-Frame wird in V1 nicht unterstützt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fps {
    /// 24 fps (Film).
    Fps24,
    /// 25 fps (PAL) – Standard.
    #[default]
    Fps25,
    /// 30 fps (NTSC).
    Fps30,
}

impl Fps {
    /// Bilder pro Sekunde.
    pub fn frames_per_second(self) -> u64 {
        match self {
            Fps::Fps24 => 24,
            Fps::Fps25 => 25,
            Fps::Fps30 => 30,
        }
    }

    /// Dauer eines Frames.
    pub fn frame_duration(self) -> Duration {
        Duration::from_secs_f64(1.0 / self.frames_per_second() as f64)
    }

    /// Alle unterstützten Bildraten (für Combo-Boxen, z. B. Inspector „Triggers").
    pub const ALL: [Fps; 3] = [Fps::Fps24, Fps::Fps25, Fps::Fps30];

    /// Kurzes Label für die Bildrate (z. B. „25 fps").
    pub fn label(self) -> &'static str {
        match self {
            Fps::Fps24 => "24 fps",
            Fps::Fps25 => "25 fps",
            Fps::Fps30 => "30 fps",
        }
    }
}

/// Ein Timecode-Zeitpunkt: Gesamt-Frames seit 00:00:00:00 bei gegebener [`Fps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Timecode {
    /// Gesamtzahl der Frames.
    pub frames: u64,
    /// Bildrate.
    pub fps: Fps,
}

impl Timecode {
    /// Aus Stunden/Minuten/Sekunden/Frames.
    pub fn from_hmsf(hours: u64, minutes: u64, seconds: u64, frames: u64, fps: Fps) -> Self {
        let f = fps.frames_per_second();
        Self {
            frames: (hours * 3600 + minutes * 60 + seconds) * f + frames,
            fps,
        }
    }

    /// Aus einer Duration (rundet auf den nächsten Frame).
    pub fn from_duration(d: Duration, fps: Fps) -> Self {
        Self {
            frames: (d.as_secs_f64() * fps.frames_per_second() as f64).round() as u64,
            fps,
        }
    }

    /// Umrechnung in eine Duration (exakt: frames / fps).
    pub fn to_duration(self) -> Duration {
        Duration::from_secs_f64(self.frames as f64 / self.fps.frames_per_second() as f64)
    }

    /// Addiert Frames.
    pub fn add_frames(self, n: u64) -> Self {
        Self {
            frames: self.frames + n,
            ..self
        }
    }

    /// Subtrahiert Frames (saturierend).
    pub fn sub_frames(self, n: u64) -> Self {
        Self {
            frames: self.frames.saturating_sub(n),
            ..self
        }
    }

    /// Differenz `self - other` (nur bei gleicher Bildrate und positivem Ergebnis).
    pub fn checked_sub(self, other: Timecode) -> Option<Timecode> {
        if self.fps == other.fps && self.frames >= other.frames {
            Some(Timecode {
                frames: self.frames - other.frames,
                fps: self.fps,
            })
        } else {
            None
        }
    }
}

impl fmt::Display for Timecode {
    /// Format: `HH:MM:SS:FF`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fps = self.fps.frames_per_second();
        let frames = self.frames % fps;
        let total_seconds = self.frames / fps;
        let seconds = total_seconds % 60;
        let minutes = (total_seconds / 60) % 60;
        let hours = total_seconds / 3600;
        write!(f, "{hours:02}:{minutes:02}:{seconds:02}:{frames:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmsf_and_display() {
        let tc = Timecode::from_hmsf(1, 2, 3, 12, Fps::Fps25);
        assert_eq!(tc.to_string(), "01:02:03:12");
        assert_eq!(tc.frames, ((3600 + 2 * 60 + 3) * 25) + 12);
    }

    #[test]
    fn duration_roundtrip_is_frame_exact() {
        let tc = Timecode::from_hmsf(0, 0, 10, 0, Fps::Fps25);
        assert_eq!(tc.to_duration(), Duration::from_secs(10));
        assert_eq!(
            Timecode::from_duration(Duration::from_secs(10), Fps::Fps25),
            tc
        );
    }

    #[test]
    fn frame_duration_matches_fps() {
        assert_eq!(Fps::Fps25.frame_duration(), Duration::from_millis(40));
        assert_eq!(
            Fps::Fps24.frame_duration(),
            Duration::from_secs_f64(1.0 / 24.0)
        );
    }

    #[test]
    fn frame_arithmetic() {
        let tc = Timecode::from_hmsf(0, 0, 0, 10, Fps::Fps25);
        assert_eq!(tc.add_frames(15).to_string(), "00:00:01:00");
        assert_eq!(tc.add_frames(15).sub_frames(25).frames, 0);
        assert_eq!(tc.sub_frames(100).frames, 0, "saturating");
    }

    #[test]
    fn checked_sub_respects_fps() {
        let a = Timecode::from_hmsf(0, 0, 2, 0, Fps::Fps25);
        let b = Timecode::from_hmsf(0, 0, 1, 0, Fps::Fps25);
        assert_eq!(
            a.checked_sub(b).unwrap().to_duration(),
            Duration::from_secs(1)
        );
        assert!(a.checked_sub(a.add_frames(1)).is_none());
        let c = Timecode::from_hmsf(0, 0, 1, 0, Fps::Fps24);
        assert!(a.checked_sub(c).is_none(), "mixed fps not allowed");
    }
}
