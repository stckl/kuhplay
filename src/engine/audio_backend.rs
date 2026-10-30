//! Echtes Audio-Backend: Dekodierung mit **symphonia** (mp3/aac/flac/ogg/wav/
//! m4a/…, pure Rust) und Ausgabe über **cpal** (CoreAudio auf macOS).
//!
//! ## Architektur
//!
//! * Ein einzelner `cpal`-Output-Stream läuft für die gesamte Engine.
//! * Pro Cue läuft ein **Decoder-Thread** (symphonia), der PCM → f32
//!   dekodiert, auf Stream-Rate resampelt (lineare Interpolation) und in eine
//!   pro-Voice-FIFO (`VecDeque`) einspeist (Füllstands-gesteuert, Cap ≈ 2 s).
//!   Die FIFO liegt im **Cue-Kanal-Layout** (Voice-Kanäle, nicht Device-Kanäle).
//! * Der cpal-Callback **mischt** alle Voices sample-genau (`mix_voice`):
//!   Voice-Kanal `i` → logische Kanäle (Matrix/Patch der [`AudioRoute`]) →
//!   Geräte-Ausgänge (`set_channel_outputs`, Ausgänge jenseits des Geräts
//!   bleiben stumm), bei ≥ 2 Voice-Kanälen mit Constant-Power-Pan auf den
//!   Datei-Kanälen 1+2. Der Mixer zählt die
//!   gedrainten Frames (→ Medien-Position) und markiert eine Voice als
//!   `finished`, sobald EOF erreicht und die FIFO leer ist.
//! * Pause/Resume/Seek/Stop laufen über Flags bzw. ein Control-Channel an den
//!   Decoder-Thread – der cpal-Callback blockiert nie (ein kurzes Lock).
//!
//! ## Erweiterung
//!
//! * Besseres Resampling (rubato) statt linearer Interpolation.
//! * Low-Latency-Puffer konfigurierbar (`StreamConfig::buffer_size`).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::core::{AudioRoute, AUDIO_CHANNEL_COUNT};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;
use uuid::Uuid;

/// Maximal gepufferte Samples pro Voice (≈ 2 s Puffer gegen Ruckler).
const QUEUE_CAP_SECS: f32 = 2.0;

/// Steuerbefehle an einen Decoder-Thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioControl {
    /// Springt an `position` (Re-Open + Seek).
    Seek(Duration),
    /// Beendet den Decoder-Thread (und damit die Voice).
    Stop,
}

/// Gemeinsamer Voice-Zustand (Mixer-Callback ↔ Decoder-Thread ↔ Engine).
struct VoiceShared {
    /// Interleaved f32-Samples (Stream-Rate, **Cue-Kanal-Layout**), FIFO.
    queue: VecDeque<f32>,
    /// Kanalanzahl des Cue-Layouts (Voice-Queue-Layout).
    channels: usize,
    /// Pro Voice-Kanal die Ziel-Kanäle als Bitmaske (Bit 0 = Kanal 1).
    masks: [u16; AUDIO_CHANNEL_COUNT],
    /// Pan-Position (-1.0 … 1.0, nur bei `channels >= 2` wirksam).
    pan: f32,
    /// Lautstärke (0.0 – 1.0), vom Engine-Thread gesetzt.
    volume: f32,
    /// Pausiert: Mixer gibt Stille aus, FIFO wird nicht gedraint.
    paused: bool,
    /// Decoder hat EOF erreicht.
    eof: bool,
    /// Fertig: EOF und FIFO leer (vom Mixer gesetzt).
    finished: bool,
    /// Bereits an das Device gedrainte Frames (für die Position).
    drained_frames: u64,
}

type VoiceRef = Arc<Mutex<VoiceShared>>;
type ChannelOutputs = Arc<[AtomicU8; AUDIO_CHANNEL_COUNT]>;

/// Ein laufender Audio-Player (Decoder-Thread + Control-Channel).
struct AudioVoice {
    voice: VoiceRef,
    control: crossbeam_channel::Sender<AudioControl>,
}

/// Das Audio-Backend: ein cpal-Stream + Voice-Verwaltung.
pub struct AudioBackend {
    voices: HashMap<Uuid, AudioVoice>,
    mixer: Arc<Mutex<HashMap<Uuid, VoiceRef>>>,
    /// Geräte-Ausgang je logischem Kanal (1-basiert, 0 = aus). Atomar, damit
    /// der Callback nicht blockiert und Umlegen auch live wirkt.
    channel_outputs: ChannelOutputs,
    stream_sample_rate: u32,
    stream_channels: usize,
    // Stream-Lebenszeit: muss gehalten werden (Drop stoppt die Ausgabe).
    _stream: cpal::Stream,
}

/// Ein Ausgabegerät für die Auswahl in den Workspace Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// Anzeigename, zugleich Schlüssel für [`AudioBackend::with_device`].
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
}

/// Anzeigename eines Geräts.
fn device_name(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

/// Alle Ausgabegeräte des Systems (leer, wenn keine verfügbar sind).
pub fn output_devices() -> Vec<OutputDevice> {
    let Ok(devices) = cpal::default_host().output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let name = device_name(&device)?;
            let config = device.default_output_config().ok()?;
            Some(OutputDevice {
                name,
                channels: config.channels(),
                sample_rate: config.sample_rate(),
            })
        })
        .collect()
}

/// Name des aktuellen System-Standardgeräts.
pub fn default_output_name() -> Option<String> {
    device_name(&cpal::default_host().default_output_device()?)
}

impl AudioBackend {
    /// Öffnet das Default-Ausgabegerät und startet den Stream.
    pub fn new() -> Result<Self, String> {
        Self::with_device(None)
    }

    /// Öffnet das Ausgabegerät `name` (`None` = System-Standard) und startet
    /// den Stream. Fehler (Gerät nicht da / kein f32-Support) → `Err`.
    pub fn with_device(name: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match name {
            Some(name) => host
                .output_devices()
                .map_err(|e| format!("output_devices: {e}"))?
                .find(|device| device_name(device).as_deref() == Some(name))
                .ok_or_else(|| format!("audio output \"{name}\" not found"))?,
            None => host
                .default_output_device()
                .ok_or_else(|| "no audio output device available".to_string())?,
        };

        // f32-Konfiguration finden (Default bevorzugt).
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no default output config: {e}"))?;
        let stream_config: cpal::StreamConfig =
            if supported.sample_format() == cpal::SampleFormat::F32 {
                supported.into()
            } else {
                let rate = supported.sample_rate();
                device
                    .supported_output_configs()
                    .map_err(|e| format!("supported_output_configs: {e}"))?
                    .find(|c| {
                        c.sample_format() == cpal::SampleFormat::F32
                            && c.min_sample_rate() <= rate
                            && c.max_sample_rate() >= rate
                    })
                    .ok_or_else(|| "no f32 output config available".to_string())?
                    .with_sample_rate(rate)
                    .config()
            };
        let stream_sample_rate = stream_config.sample_rate;
        let stream_channels = stream_config.channels as usize;

        let mixer: Arc<Mutex<HashMap<Uuid, VoiceRef>>> = Arc::new(Mutex::new(HashMap::new()));
        let mixer_cb = mixer.clone();
        let channel_outputs: ChannelOutputs =
            Arc::new(std::array::from_fn(|i| AtomicU8::new(i as u8 + 1)));
        let outputs_cb = channel_outputs.clone();
        let channels = stream_channels;
        let stream = device
            .build_output_stream(
                stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    Self::mix(&mixer_cb, &outputs_cb, data, channels);
                },
                |err| tracing::error!("cpal stream error: {err}"),
                None,
            )
            .map_err(|e| format!("build_output_stream: {e}"))?;
        stream.play().map_err(|e| format!("stream.play(): {e}"))?;

        Ok(Self {
            voices: HashMap::new(),
            mixer,
            channel_outputs,
            stream_sample_rate,
            stream_channels,
            _stream: stream,
        })
    }

    /// Legt die logischen Kanäle auf Geräte-Ausgänge (1-basiert, 0 = aus).
    pub fn set_channel_outputs(&self, outputs: [u8; AUDIO_CHANNEL_COUNT]) {
        for (slot, output) in self.channel_outputs.iter().zip(outputs) {
            slot.store(output, Ordering::Relaxed);
        }
    }

    /// Mixer-Callback: summiert alle Voices sample-genau in den Output-Puffer.
    fn mix(
        mixer: &Arc<Mutex<HashMap<Uuid, VoiceRef>>>,
        outputs: &ChannelOutputs,
        data: &mut [f32],
        channels: usize,
    ) {
        for sample in data.iter_mut() {
            *sample = 0.0;
        }
        if channels == 0 {
            return;
        }
        let frames = data.len() / channels;
        let outputs: [u8; AUDIO_CHANNEL_COUNT] =
            std::array::from_fn(|i| outputs[i].load(Ordering::Relaxed));
        let voices = mixer.lock().unwrap();
        for voice in voices.values() {
            let mut v = voice.lock().unwrap();
            if v.paused || v.finished {
                continue;
            }
            // Skalare vorab kopieren (sonst kollidiert &mut v.queue mit den
            // Feld-Reads im selben Aufruf).
            let (voice_channels, masks, pan, volume) = (v.channels, v.masks, v.pan, v.volume);
            let drained = mix_voice(
                data,
                channels,
                &mut v.queue,
                voice_channels,
                &masks,
                &outputs,
                pan,
                volume,
                frames,
            );
            v.drained_frames += drained as u64;
            if v.eof && v.queue.is_empty() {
                v.finished = true;
            }
        }
    }

    /// Startet eine Voice für `path` (Decoder-Thread). `seek` = Startposition.
    /// `route`: Optionales Mehrkanal-Routing (Patch/Pan); `None` = Device-Layout
    /// (bisheriges Verhalten, Voice-Queue im Stream-Kanal-Layout).
    pub fn add_voice(
        &mut self,
        cue_id: Uuid,
        path: &str,
        seek: Duration,
        route: Option<AudioRoute>,
    ) -> Result<(), String> {
        self.stop_voice(cue_id);
        // Cue-Layout: Route (Kanäle/Patch/Pan) oder Stream-Layout als Default.
        // Ohne Route: Stream-Layout, Kanal n → Kanal n (höchstens 16 Kanäle).
        let route = route.unwrap_or_else(|| {
            AudioRoute::new(self.stream_channels.min(AUDIO_CHANNEL_COUNT) as u8)
        });
        let voice_channels = usize::from(route.channels);
        let pan = route.pan;
        let masks = std::array::from_fn(|i| route.channel_mask(i));
        let voice: VoiceRef = Arc::new(Mutex::new(VoiceShared {
            queue: VecDeque::new(),
            channels: voice_channels,
            masks,
            pan,
            volume: 1.0,
            paused: false,
            eof: false,
            finished: false,
            drained_frames: 0,
        }));
        let (tx, rx) = crossbeam_channel::unbounded::<AudioControl>();
        let voice_dec = voice.clone();
        let path = path.to_string();
        let rate = self.stream_sample_rate;
        std::thread::Builder::new()
            .name(format!("audio-decoder-{cue_id}"))
            .spawn(move || {
                decode_loop(&path, seek, &voice_dec, &rx, rate, voice_channels);
            })
            .map_err(|e| format!("spawn decoder thread: {e}"))?;
        self.mixer.lock().unwrap().insert(cue_id, voice.clone());
        self.voices
            .insert(cue_id, AudioVoice { voice, control: tx });
        Ok(())
    }

    /// Pausiert eine Voice (Mixer gibt Stille aus, Position friert ein).
    pub fn pause(&mut self, cue_id: Uuid) {
        if let Some(v) = self.voices.get(&cue_id) {
            v.voice.lock().unwrap().paused = true;
        }
    }

    /// Setzt eine pausierte Voice fort.
    pub fn resume(&mut self, cue_id: Uuid) {
        if let Some(v) = self.voices.get(&cue_id) {
            v.voice.lock().unwrap().paused = false;
        }
    }

    /// Stoppt eine Voice (Decoder-Thread beendet sich, FIFO geleert).
    pub fn stop_voice(&mut self, cue_id: Uuid) {
        if let Some(voice) = self.voices.remove(&cue_id) {
            let _ = voice.control.send(AudioControl::Stop);
            self.mixer.lock().unwrap().remove(&cue_id);
        }
    }

    /// Setzt die Lautstärke einer Voice (0.0 – 1.0).
    pub fn set_volume(&mut self, cue_id: Uuid, volume: f32) {
        if let Some(v) = self.voices.get(&cue_id) {
            v.voice.lock().unwrap().volume = volume.clamp(0.0, 1.0);
        }
    }

    /// Springt in einer Voice an `position`.
    pub fn seek(&mut self, cue_id: Uuid, position: Duration) {
        if let Some(v) = self.voices.get(&cue_id) {
            // Queue leeren + Position zurücksetzen, dann Decoder re-seeken.
            {
                let mut shared = v.voice.lock().unwrap();
                shared.queue.clear();
                shared.drained_frames = 0;
                shared.eof = false;
                shared.finished = false;
            }
            let _ = v.control.send(AudioControl::Seek(position));
        }
    }

    /// Medien-Position (sample-genau aus den gedrainten Frames).
    pub fn position(&self, cue_id: Uuid) -> Option<Duration> {
        let v = self.voices.get(&cue_id)?;
        let shared = v.voice.lock().unwrap();
        Some(Duration::from_secs_f64(
            shared.drained_frames as f64 / self.stream_sample_rate as f64,
        ))
    }

    /// `true`, wenn die Voice ihr Medium komplett ausgegeben hat.
    pub fn is_finished(&self, cue_id: Uuid) -> bool {
        self.voices
            .get(&cue_id)
            .map(|v| v.voice.lock().unwrap().finished)
            .unwrap_or(false)
    }

    /// Anzahl laufender Voices.
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }
}

/// Decoder-Status für den Control-Loop.
const RUN: u8 = 0;
const SEEKING: u8 = 1;
const STOPPED: u8 = 2;

/// Decoder-Thread: symphonia → f32 → (resample) → Voice-FIFO.
///
/// `voice_channels` = Kanalanzahl des Cue-Layouts (aus der `AudioRoute` oder
/// das Stream-Layout als Default) – die FIFO liegt in diesem Layout, der
/// Mixer patcht es auf die Device-Kanäle.
///
/// Endet nicht bei EOF, sondern wartet auf Control (Seek/Stop) – so kann die
/// Engine Loops über `Seek(0)` nahtlos neu starten.
fn decode_loop(
    path: &str,
    mut seek: Duration,
    voice: &VoiceRef,
    control: &crossbeam_channel::Receiver<AudioControl>,
    out_rate: u32,
    voice_channels: usize,
) {
    let state = Arc::new(AtomicU8::new(RUN));
    let current_path = path.to_string();
    'outer: loop {
        // Control-Channel bedienen (Seek/Stop zwischen den Paketen).
        while let Ok(cmd) = control.try_recv() {
            match cmd {
                AudioControl::Stop => {
                    state.store(STOPPED, Ordering::Relaxed);
                    break 'outer;
                }
                AudioControl::Seek(pos) => {
                    seek = pos;
                    state.store(SEEKING, Ordering::Relaxed);
                }
            }
        }

        // (Re-)Open + Probe + Decoder aufsetzen.
        let open = open_decoder(&current_path, seek, out_rate, voice_channels, voice);
        let mut session = match open {
            Ok(session) => session,
            Err(err) => {
                tracing::warn!(path = %current_path, %err, "audio decode failed");
                // Fehler melden und auf Control warten (Engine stoppt/restartet).
                voice.lock().unwrap().eof = true;
                wait_for_control(control, &state);
                if state.load(Ordering::Relaxed) == STOPPED {
                    break;
                }
                if state.load(Ordering::Relaxed) == SEEKING {
                    state.store(RUN, Ordering::Relaxed);
                    continue;
                }
                break;
            }
        };

        // Decode-Loop einer Session.
        loop {
            // Pro Durchlauf ein Control-Kommando bedienen (Stop/Seek beenden
            // die Session; der nächste Tick der äußeren Schleife holt das
            // nächste Kommando ab).
            if let Ok(cmd) = control.try_recv() {
                match cmd {
                    AudioControl::Stop => {
                        state.store(STOPPED, Ordering::Relaxed);
                        break 'outer;
                    }
                    AudioControl::Seek(pos) => {
                        seek = pos;
                        state.store(SEEKING, Ordering::Relaxed);
                        break; // Session verwerfen, neu öffnen
                    }
                }
            }
            if state.load(Ordering::Relaxed) == SEEKING {
                state.store(RUN, Ordering::Relaxed);
                break; // Re-Open mit neuem Seek
            }

            let packet = match session.format.next_packet() {
                Ok(packet) => packet,
                Err(_) => break, // EOF (oder Demuxer-Fehler) → Session-Ende
            };
            if packet.track_id() != session.track_id {
                continue;
            }
            let decoded = match session.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(err) => {
                    tracing::warn!(%err, "audio decode error");
                    break;
                }
            };
            let spec = *decoded.spec();
            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            let mut buf = SampleBuffer::<f32>::new(frames as u64, spec);
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            let src_rate = spec.rate;
            let src_channels = spec.channels.count().max(1);

            // Kanäle → Cue-Kanäle (Voice-Layout), dann Rate → Stream-Rate.
            let mono = convert_channels(samples, src_channels, voice_channels);
            let resampled = if src_rate == out_rate {
                mono
            } else {
                resample_linear(&mono, src_rate, out_rate, voice_channels)
            };

            // In die FIFO einspeisen (Füllstands-gesteuert).
            let cap = (out_rate as f32 * voice_channels as f32 * QUEUE_CAP_SECS) as usize;
            let data = resampled;
            let mut offset = 0;
            while offset < data.len() {
                // Control auch während langer Files bedienen (ein Kommando
                // pro Schleifendurchlauf).
                if let Ok(cmd) = control.try_recv() {
                    match cmd {
                        AudioControl::Stop => {
                            state.store(STOPPED, Ordering::Relaxed);
                            break 'outer;
                        }
                        AudioControl::Seek(pos) => {
                            seek = pos;
                            state.store(SEEKING, Ordering::Relaxed);
                            break; // Session verwerfen (Fill-Loop endet)
                        }
                    }
                }
                if state.load(Ordering::Relaxed) != RUN {
                    break;
                }
                let mut v = voice.lock().unwrap();
                let room = cap.saturating_sub(v.queue.len());
                if room == 0 {
                    drop(v);
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                let take = room.min(data.len() - offset);
                v.queue.extend(&data[offset..offset + take]);
                offset += take;
                drop(v);
            }
        }

        // EOF: Flag setzen und auf Control warten (Loop via Seek(0)).
        voice.lock().unwrap().eof = true;
        wait_for_control(control, &state);
        match state.load(Ordering::Relaxed) {
            STOPPED => break,
            SEEKING => {
                state.store(RUN, Ordering::Relaxed);
            }
            _ => break,
        }
    }
}

/// Wartet auf das nächste Control-Kommando (EOF-Phase, CPU-schonend).
fn wait_for_control(control: &crossbeam_channel::Receiver<AudioControl>, state: &Arc<AtomicU8>) {
    while state.load(Ordering::Relaxed) == RUN {
        match control.recv_timeout(Duration::from_millis(10)) {
            Ok(AudioControl::Stop) => {
                state.store(STOPPED, Ordering::Relaxed);
                return;
            }
            Ok(AudioControl::Seek(_)) => {
                state.store(SEEKING, Ordering::Relaxed);
                return;
            }
            Err(_) => {}
        }
    }
}

/// Eine geöffnete Decoder-Session.
struct DecoderSession {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
}

/// Öffnet Datei, wählt den Audio-Track und seeked an `seek`.
fn open_decoder(
    path: &str,
    seek: Duration,
    _out_rate: u32,
    _out_channels: usize,
    _voice: &VoiceRef,
) -> Result<DecoderSession, String> {
    let file =
        std::fs::File::open(Path::new(path)).map_err(|e| format!("cannot open {path}: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe {path}: {e}"))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("no audio track in {path}"))?;
    let track_id = track.id;
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("make decoder: {e}"))?;

    // Seek (demuxer-level, coarse – schnell).
    if !seek.is_zero() {
        let time = Time {
            seconds: seek.as_secs(),
            frac: seek.subsec_nanos() as f64 / 1e9,
        };
        let _ = format.seek(
            SeekMode::Coarse,
            SeekTo::Time {
                time,
                track_id: Some(track_id),
            },
        );
    }

    Ok(DecoderSession {
        format,
        decoder,
        track_id,
    })
}

/// Konvertiert Interleaved-Samples von `src_ch` auf `dst_ch` Kanäle.
///
/// V1-Regeln: mono → Duplizieren; stereo → 1:1; >2 → erste beiden (Rest verworfen);
/// Ziel Mono → Durchschnitt.
fn convert_channels(input: &[f32], src_ch: usize, dst_ch: usize) -> Vec<f32> {
    if src_ch == dst_ch {
        return input.to_vec();
    }
    let frames = input.len() / src_ch;
    let mut out = vec![0.0f32; frames * dst_ch];
    for f in 0..frames {
        let src = &input[f * src_ch..(f + 1) * src_ch];
        for (c, o) in out[f * dst_ch..(f + 1) * dst_ch].iter_mut().enumerate() {
            *o = if dst_ch == 1 {
                src.iter().sum::<f32>() / src_ch as f32
            } else if src_ch == 1 {
                src[0]
            } else if c < src_ch {
                src[c]
            } else {
                (src[0] + src.get(1).copied().unwrap_or(src[0])) * 0.5
            };
        }
    }
    out
}

/// Lineare Resampling-Interpolation (V1; rubato für V2).
fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32, channels: usize) -> Vec<f32> {
    if in_rate == out_rate || input.is_empty() || channels == 0 {
        return input.to_vec();
    }
    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }
    let ratio = in_rate as f64 / out_rate as f64;
    let out_frames = ((in_frames as f64) / ratio).floor() as usize;
    let mut out = vec![0.0f32; out_frames * channels];
    for of in 0..out_frames {
        let pos = of as f64 * ratio;
        let i0 = (pos.floor() as usize).min(in_frames - 1);
        let i1 = (i0 + 1).min(in_frames - 1);
        let t = (pos - i0 as f64) as f32;
        for c in 0..channels {
            let a = input[i0 * channels + c];
            let b = input[i1 * channels + c];
            out[of * channels + c] = a + (b - a) * t;
        }
    }
    out
}

/// Constant-Power-Pan-Gains für `pan` (-1.0 links … 1.0 rechts).
///
/// θ = (pan + 1) · π/4; Gains = (cos θ, sin θ) / √½ – auf **Unity in der
/// Mitte** normalisiert (kein Center-Dip), konstante Leistung über den
/// Pan-Verlauf (L² + R² = 2).
fn pan_gains(pan: f32) -> (f32, f32) {
    let theta = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let norm = std::f32::consts::FRAC_1_SQRT_2;
    (theta.cos() / norm, theta.sin() / norm)
}

/// Mischt bis zu `frames` Frames einer Voice in den Device-Puffer und
/// verbraucht die FIFO entsprechend. Rückgabe: Anzahl gedrainter Voice-Frames.
///
/// Routing: Voice-Kanal `vc` → alle logischen Kanäle aus `masks[vc]` → deren
/// Geräte-Ausgang aus `outputs` (0 oder jenseits des Geräts = stumm). `pan`
/// wirkt nur bei `voice_channels >= 2` (Constant-Power auf den Voice-Kanälen
/// 1+2). Reine Funktion – im Mixer-Callback ohne weitere Locks nutzbar.
#[allow(clippy::too_many_arguments)] // Mixer-Signatur: alle Parameter fachlich nötig
fn mix_voice(
    data: &mut [f32],
    device_channels: usize,
    queue: &mut VecDeque<f32>,
    voice_channels: usize,
    masks: &[u16; AUDIO_CHANNEL_COUNT],
    outputs: &[u8; AUDIO_CHANNEL_COUNT],
    pan: f32,
    volume: f32,
    frames: usize,
) -> usize {
    if device_channels == 0 || voice_channels == 0 {
        return 0;
    }
    let avail = queue.len() / voice_channels;
    let take = avail.min(frames);
    let (gain_l, gain_r) = if voice_channels >= 2 {
        pan_gains(pan)
    } else {
        (1.0, 1.0)
    };
    for f in 0..take {
        for vc in 0..voice_channels.min(AUDIO_CHANNEL_COUNT) {
            let mut s = queue[f * voice_channels + vc] * volume;
            if voice_channels >= 2 && vc < 2 {
                s *= if vc == 0 { gain_l } else { gain_r };
            }
            for (channel, output) in outputs.iter().enumerate() {
                let output = usize::from(*output);
                if masks[vc] & (1 << channel) != 0 && output > 0 && output <= device_channels {
                    data[f * device_channels + output - 1] += s;
                }
            }
        }
    }
    // FIFOverbrauch: VecDeque-Drain vorne.
    queue.drain(..take * voice_channels);
    take
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: [u8; AUDIO_CHANNEL_COUNT] =
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

    /// Fortlaufender Patch ab Kanal `first` (0-basiert), Kanal n → Ausgang n.
    #[allow(clippy::too_many_arguments)]
    fn mix_patch(
        data: &mut [f32],
        device_channels: usize,
        queue: &mut VecDeque<f32>,
        voice_channels: usize,
        first: u8,
        pan: f32,
        volume: f32,
        frames: usize,
    ) -> usize {
        let route = AudioRoute::with_patch(1, first + 1);
        let masks = std::array::from_fn(|i| route.channel_mask(i));
        mix_voice(
            data,
            device_channels,
            queue,
            voice_channels,
            &masks,
            &IDENTITY,
            pan,
            volume,
            frames,
        )
    }

    #[test]
    fn mix_voice_routes_matrix_through_channels_to_outputs() {
        // Datei-Kanal 1 → Kanäle 1+3, Datei-Kanal 2 → Kanal 2.
        // Kanal 1 → Ausgang 4, Kanal 2 → aus, Kanal 3 → Ausgang 1.
        let mut route = AudioRoute::new(2);
        route.toggle(0, 2);
        let masks = std::array::from_fn(|i| route.channel_mask(i));
        let mut outputs = IDENTITY;
        outputs[0] = 4;
        outputs[1] = 0;
        outputs[2] = 1;
        let mut queue: VecDeque<f32> = vec![0.5, 0.25].into();
        let mut data = vec![0.0f32; 4];
        mix_voice(&mut data, 4, &mut queue, 2, &masks, &outputs, 0.0, 1.0, 1);
        assert_eq!(data, vec![0.5, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn mix_voice_mutes_output_beyond_device() {
        let route = AudioRoute::new(1);
        let masks = std::array::from_fn(|i| route.channel_mask(i));
        let mut outputs = IDENTITY;
        outputs[0] = 5;
        let mut queue: VecDeque<f32> = vec![1.0].into();
        let mut data = vec![0.0f32; 2];
        assert_eq!(
            mix_voice(&mut data, 2, &mut queue, 1, &masks, &outputs, 0.0, 1.0, 1),
            1,
            "FIFO läuft weiter, auch wenn der Ausgang fehlt"
        );
        assert_eq!(data, vec![0.0, 0.0]);
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn pan_gains_unity_center_hard_pan_constant_power() {
        let (l, r) = pan_gains(0.0);
        assert!(approx(l, 1.0) && approx(r, 1.0), "center must be unity");
        let (l, r) = pan_gains(-1.0);
        assert!(approx(l, std::f32::consts::SQRT_2) && approx(r, 0.0));
        let (l, r) = pan_gains(1.0);
        assert!(approx(l, 0.0) && approx(r, std::f32::consts::SQRT_2));
        // Out-of-Range wird geclampt.
        assert_eq!(pan_gains(5.0), pan_gains(1.0));
        assert_eq!(pan_gains(-5.0), pan_gains(-1.0));
        // Constant Power: L² + R² konstant über den Pan-Verlauf.
        for pan in [-0.75, -0.25, 0.25, 0.75] {
            let (l, r) = pan_gains(pan);
            assert!(approx(l * l + r * r, 2.0), "power not constant at {pan}");
        }
    }

    #[test]
    fn mix_voice_patches_stereo_onto_device_channels() {
        // Stereo-Voice → Device-Kanäle 3+4 (0-based 2+3) eines 4-Kanal-Devices.
        let mut queue: VecDeque<f32> = vec![0.5, -0.5, 1.0, 1.0].into();
        let mut data = vec![0.0f32; 4 * 2]; // 2 Frames × 4 Kanäle
        let drained = mix_patch(&mut data, 4, &mut queue, 2, 2, 0.0, 1.0, 2);
        assert_eq!(drained, 2);
        assert!(queue.is_empty());
        // Kanäle 1+2 (Index 0,1) bleiben leer, der Patch landet auf 3+4.
        assert_eq!(&data[0..2], &[0.0, 0.0]);
        assert!(approx(data[2], 0.5));
        assert!(approx(data[3], -0.5));
        assert!(approx(data[6], 1.0));
        assert!(approx(data[7], 1.0));
    }

    #[test]
    fn mix_voice_mono_patch_applies_volume_but_no_pan() {
        // Mono-Voice auf Kanal 2 (0-based 1), Volume 0.5 – Pan wirkt nicht.
        let mut queue: VecDeque<f32> = vec![0.8, -0.4].into();
        let mut data = vec![0.0f32; 3 * 2]; // 2 Frames × 3 Kanäle
        let drained = mix_patch(&mut data, 3, &mut queue, 1, 1, -1.0, 0.5, 2);
        assert_eq!(drained, 2);
        assert!(queue.is_empty());
        assert!(approx(data[1], 0.4));
        assert!(approx(data[4], -0.2));
        assert!(data[0] == 0.0 && data[2] == 0.0 && data[3] == 0.0 && data[5] == 0.0);
    }

    #[test]
    fn mix_voice_pan_hard_left_mutes_right_patch_channel() {
        // Stereo, Pan hart links: rechter Patch-Kanal bleibt stumm.
        let mut queue: VecDeque<f32> = vec![0.5, 0.5].into();
        let mut data = vec![0.0f32; 2];
        let drained = mix_patch(&mut data, 2, &mut queue, 2, 0, -1.0, 1.0, 1);
        assert_eq!(drained, 1);
        assert!(approx(data[0], 0.5 * std::f32::consts::SQRT_2));
        assert!(approx(data[1], 0.0));
    }

    #[test]
    fn mix_voice_clips_patch_overhang_at_device_edge() {
        // 4-Kanal-Voice ab Kanal 4 (0-based 3) auf 4-Kanal-Device: nur der
        // erste Voice-Kanal passt (Index 3), der Rest wird abgeschnitten.
        let mut queue: VecDeque<f32> = vec![1.0, 2.0, 3.0, 4.0].into();
        let mut data = vec![0.0f32; 4];
        let drained = mix_patch(&mut data, 4, &mut queue, 4, 3, 0.0, 1.0, 1);
        assert_eq!(drained, 1);
        assert_eq!(&data[..3], &[0.0, 0.0, 0.0]);
        assert!(approx(data[3], 1.0));
    }

    #[test]
    fn mix_voice_take_limited_by_queue_and_buffer() {
        // Queue kürzer als die Puffer-Frames: nur verfügbare Frames gemischt.
        let mut queue: VecDeque<f32> = vec![0.25, 0.25].into(); // 1 Frame stereo
        let mut data = vec![0.0f32; 2 * 4]; // 4 Frames
        let drained = mix_patch(&mut data, 2, &mut queue, 2, 0, 0.0, 1.0, 4);
        assert_eq!(drained, 1);
        assert!(approx(data[0], 0.25));
        assert!(approx(data[1], 0.25));
        assert!(data[2..].iter().all(|s| *s == 0.0));

        // Leere Queue / ungültige Layouts: nichts passiert.
        let mut empty: VecDeque<f32> = VecDeque::new();
        let mut out = vec![0.0f32; 4];
        assert_eq!(mix_patch(&mut out, 2, &mut empty, 2, 0, 0.0, 1.0, 2), 0);
        assert_eq!(mix_patch(&mut out, 0, &mut empty, 2, 0, 0.0, 1.0, 2), 0);
        assert_eq!(mix_patch(&mut out, 2, &mut empty, 0, 0, 0.0, 1.0, 2), 0);
    }
}
