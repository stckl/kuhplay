//! Waveform-Peaks für Audio-Cues (QLab „Waveform"-Vorschau im Inspector).
//!
//! Dekodiert eine Audiodatei mit **symphonia** (derselbe Stack wie das
//! Audio-Backend, siehe [`super::audio_backend`]), mischt auf Mono und
//! berechnet min/max-Peaks pro Bucket (normalisiert auf -1.0..=1.0).
//!
//! Die GUI hält die Peaks in einem [`WaveformCache`] (Pfad → Peaks), damit
//! eine Datei nur einmal dekodiert wird.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// Default-Anzahl Buckets (min/max-Paare) für die Vorschau.
pub const WAVEFORM_BUCKETS: usize = 512;

/// Min/Max-Peaks einer Audiodatei, normalisiert auf -1.0..=1.0.
#[derive(Debug, Clone)]
pub struct Waveform {
    /// (min, max) pro Bucket – für die Darstellung als vertikale Balken.
    pub buckets: Vec<(f32, f32)>,
    /// Dauer des Stücks (aus den Codec-Params, sonst aus der Sample-Anzahl).
    pub duration: Duration,
    /// Sample-Rate der Quelle (Hz).
    pub sample_rate: u32,
}

impl Waveform {
    /// Anzahl der Buckets.
    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// Lautestes Peak (max über alle Bucket-Maxima, 0.0..=1.0) – für die
    /// optionale Normalisierung in der Anzeige.
    pub fn peak(&self) -> f32 {
        self.buckets
            .iter()
            .fold(0.0f32, |acc, &(_, max)| acc.max(max.abs()))
            .max(
                self.buckets
                    .iter()
                    .fold(0.0f32, |acc, &(min, _)| acc.max(min.abs())),
            )
    }
}

/// Fehler bei der Peak-Berechnung.
#[derive(Debug, thiserror::Error)]
pub enum WaveformError {
    #[error("cannot open {0}: {1}")]
    Open(String, std::io::Error),
    #[error("probe {0}: {1}")]
    Probe(String, String),
    #[error("no audio track in {0}")]
    NoTrack(String),
    #[error("decoder error: {0}")]
    Decode(String),
}

/// Dekodiert eine Audiodatei und berechnet min/max-Peaks pro Bucket.
///
/// Alle Kanäle werden zu Mono gemittelt; die Buckets entstehen durch
/// gleichmäßige Aufteilung aller Samples (leere Buckets → `(0.0, 0.0)`).
/// Hinweis: dekodiert das ganze File in den Speicher (Mono-f32) – für die
/// Vorschau üblicher Dateien unkritisch.
pub fn compute_waveform(path: &Path, buckets: usize) -> Result<Waveform, WaveformError> {
    let buckets = buckets.max(1);
    let file = std::fs::File::open(path)
        .map_err(|e| WaveformError::Open(path.display().to_string(), e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| WaveformError::Probe(path.display().to_string(), e.to_string()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| WaveformError::NoTrack(path.display().to_string()))?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    // Dauer aus den Codec-Params (falls bekannt).
    let duration = track
        .codec_params
        .n_frames
        .and_then(|frames| track.codec_params.time_base.map(|tb| tb.calc_time(frames)))
        .map(|t: Time| Duration::from_secs_f64(t.seconds as f64 + t.frac))
        .unwrap_or(Duration::ZERO);
    let mut decoder = symphonia::default::get_codecs()
        .make(
            &track.codec_params,
            &symphonia::core::codecs::DecoderOptions::default(),
        )
        .map_err(|e| WaveformError::Decode(e.to_string()))?;

    // Decode-Loop: Mono-Mixdown aller Samples.
    let mut mono: Vec<f32> = Vec::new();
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => break, // Demuxer-/Decoder-Fehler → bisherige Samples nutzen
        };
        let spec = *decoded.spec();
        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }
        let mut buf = SampleBuffer::<f32>::new(frames as u64, spec);
        buf.copy_interleaved_ref(decoded);
        let samples = buf.samples();
        let src_channels = spec.channels.count().max(1);
        if src_channels == 1 {
            mono.extend_from_slice(samples);
        } else {
            // Kanal-Mixdown: Mittelwert pro Frame.
            for frame in samples.chunks(src_channels) {
                mono.push(frame.iter().sum::<f32>() / src_channels as f32);
            }
        }
    }

    // Buckets: gleichmäßige Aufteilung, min/max pro Bucket (geclampt).
    let total = mono.len();
    let mut out = Vec::with_capacity(buckets);
    for i in 0..buckets {
        let start = i * total / buckets;
        let end = ((i + 1) * total / buckets).max(start + 1).min(total);
        let chunk = if start < total {
            &mono[start..end]
        } else {
            &[]
        };
        if chunk.is_empty() {
            out.push((0.0, 0.0));
            continue;
        }
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for &s in chunk {
            let s = s.clamp(-1.0, 1.0);
            if s < min {
                min = s;
            }
            if s > max {
                max = s;
            }
        }
        out.push((min, max));
    }
    // Fallback: Dauer aus der Mono-Sample-Anzahl (total ist bereits Mono).
    let duration = if duration.is_zero() && sample_rate > 0 {
        Duration::from_secs_f64(total as f64 / sample_rate as f64)
    } else {
        duration
    };
    Ok(Waveform {
        buckets: out,
        duration,
        sample_rate,
    })
}

/// Thread-sicherer Cache: Pfad → Peaks (GUI-seitig für den Inspector).
///
/// Fehlgeschlagene Dekodierungen werden als `None` gecacht, damit die GUI
/// nicht bei jedem Frame erneut dekodiert.
#[derive(Debug, Default, Clone)]
pub struct WaveformCache {
    inner: Arc<Mutex<HashMap<PathBuf, Option<Arc<Waveform>>>>>,
}

impl WaveformCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Liefert gecachte Peaks oder berechnet sie (blocking – Dekodierung
    /// typischer Vorschau-Dateien ist schnell genug für den UI-Thread).
    /// `None` = Datei nicht dekodierbar (Fehler wird gecacht).
    pub fn get_or_compute(&self, path: &Path, buckets: usize) -> Option<Arc<Waveform>> {
        if let Some(w) = self.inner.lock().unwrap().get(path) {
            return w.clone();
        }
        let w = compute_waveform(path, buckets).ok().map(Arc::new);
        self.inner
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), w.clone());
        w
    }

    /// Anzahl gecachter Einträge (für Tests).
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// `true`, wenn der Cache leer ist.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Cache leeren.
    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Schreibt eine Mono-WAV (16-bit) mit Sinus, Amplitude `amp`, `secs` lang.
    fn write_sine_wav(path: &Path, secs: f32, amp: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        let n = (secs * 8000.0) as usize;
        for i in 0..n {
            let t = i as f32 / 8000.0;
            let s = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * amp;
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn peaks_aus_sinus_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sine.wav");
        write_sine_wav(&path, 0.5, 0.8);

        let w = compute_waveform(&path, 64).unwrap();
        assert_eq!(w.bucket_count(), 64);
        assert_eq!(w.sample_rate, 8000);
        // Sinus mit Amplitude 0.8 → Peaks nahe ±0.8.
        assert!(w.peak() > 0.7, "peak zu klein: {}", w.peak());
        let (min_all, max_all) = w.buckets.iter().fold(
            (f32::INFINITY, f32::NEG_INFINITY),
            |(lo, hi), &(min, max)| (lo.min(min), hi.max(max)),
        );
        assert!(min_all < -0.7, "min zu klein: {min_all}");
        assert!(max_all > 0.7, "max zu klein: {max_all}");
        // Dauer grob 0.5 s (aus n_frames oder Sample-Anzahl).
        assert!(
            w.duration.as_secs_f64() > 0.4 && w.duration.as_secs_f64() < 0.6,
            "duration: {:?}",
            w.duration
        );
        // Symmetrie: min ≈ -max.
        assert!((min_all + max_all).abs() < 0.05);
    }

    #[test]
    fn peaks_stille_und_buckets_clamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silence.wav");
        // 0.1 s Stille → alle Buckets (0, 0).
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..800 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();

        let w = compute_waveform(&path, 32).unwrap();
        assert_eq!(w.bucket_count(), 32);
        assert!(w.buckets.iter().all(|&(min, max)| min == 0.0 && max == 0.0));
        assert_eq!(w.peak(), 0.0);
    }

    #[test]
    fn stereo_wird_zu_mono_gemischt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // Links +1.0, rechts -1.0 → Mono-Mix = 0.
        for _ in 0..800 {
            writer.write_sample(i16::MAX).unwrap();
            writer.write_sample(i16::MIN).unwrap();
        }
        writer.finalize().unwrap();

        let w = compute_waveform(&path, 16).unwrap();
        assert!(w.peak() < 0.01, "mixdown nicht ~0: {}", w.peak());
    }

    #[test]
    fn datei_fehlt_ist_fehler() {
        let err = compute_waveform(Path::new("/nonexistent/xyz.wav"), 64).unwrap_err();
        assert!(matches!(err, WaveformError::Open(..)));
    }

    #[test]
    fn cache_liefert_gleiche_instanz_und_cacht_fehler() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sine.wav");
        write_sine_wav(&path, 0.2, 0.5);

        let cache = WaveformCache::new();
        let a = cache.get_or_compute(&path, 32).expect("dekodierbar");
        let b = cache.get_or_compute(&path, 32).expect("dekodierbar");
        assert!(Arc::ptr_eq(&a, &b), "Cache muss dieselbe Arc liefern");
        assert_eq!(cache.len(), 1);

        // Fehler wird gecacht (kein erneuter Decode-Versuch, trotzdem None).
        let missing = dir.path().join("missing.wav");
        assert!(cache.get_or_compute(&missing, 32).is_none());
        assert!(cache.get_or_compute(&missing, 32).is_none());
        assert_eq!(cache.len(), 2);

        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
