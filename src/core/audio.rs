//! Audio-Parameter: Fades (linear & dB), Routing-Vorbereitung und Media-Slices.
//!
//! ## Erweiterung
//!
//! * **Mehrkanal-Routing:** [`AudioRoute`] patcht die Kanäle eines Cues auf
//!   konkrete Device-Kanäle (1-basierter Start) inkl. Constant-Power-Pan –
//!   umgesetzt von der produktiven Engine (cpal, `engine::audio_backend`).
//! * **Slices:** [`MediaSlice`] zerlegt ein Medium in Segmente mit eigenen
//!   Wiederholungszählern (Play Counts) – die Engine spielt den Plan ab.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Welcher Parameter eines Cues gefadet wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum FadeParam {
    /// Lautstärke (0.0 – 1.0).
    #[default]
    Volume,
    /// Wiedergaberate / Pitch-Faktor (0.5 – 2.0 typisch).
    Rate,
    /// Deckkraft/Opacity (0.0 – 1.0, Video-Layer).
    Opacity,
}

impl FadeParam {
    /// Alle Parameter (für UI-ComboBoxen).
    pub const ALL: [FadeParam; 3] = [FadeParam::Volume, FadeParam::Rate, FadeParam::Opacity];

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            FadeParam::Volume => "Volume",
            FadeParam::Rate => "Rate / Pitch",
            FadeParam::Opacity => "Opacity",
        }
    }
}

/// Fade-Kurve: linear oder dezibel (logarithmisch, gehörrichtig).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FadeCurve {
    /// Lineare Interpolation.
    #[default]
    Linear,
    /// dB-Interpolation über Zeit (gehörrichtig, Standard für Audio).
    Decibel,
    /// Equal-Power aufsteigend: sin(t·π/2) – Crossfade „Nach" (Ton).
    EqualPowerUp,
    /// Equal-Power absteigend: cos(t·π/2) – Crossfade „Von" (Ton).
    EqualPowerDown,
}

impl FadeCurve {
    /// Wählbare Kurven (für UI-ComboBoxen) – Linear oder dB laut
    /// docs/AUFTRAG-fade.md; Equal-Power stammt vom entfernten Crossfade-Cue
    /// und bleibt nur zum Laden alter Shows.
    pub const ALL: [FadeCurve; 2] = [FadeCurve::Linear, FadeCurve::Decibel];

    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            FadeCurve::Linear => "Linear",
            FadeCurve::EqualPowerUp => "Equal Power (up)",
            FadeCurve::EqualPowerDown => "Equal Power (down)",
            FadeCurve::Decibel => "dB (logarithmisch)",
        }
    }
}

/// Lineare Amplitude (0.0 – 1.0) → Dezibel. `v <= 0` ergibt `-∞`.
pub fn linear_to_db(v: f32) -> f32 {
    if v <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * v.log10()
    }
}

/// Dezibel → lineare Amplitude.
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// dB-Floor für Fades gegen 0 (verhindert `-∞`-Interpolation;
/// die Engine begrenzt hier auf -96 dB ≈ Stille).
pub const DB_FLOOR: f32 = -96.0;

/// Wie [`linear_to_db`], aber gegen [`DB_FLOOR`] begrenzt (interpolierbar).
pub fn linear_to_db_clamped(v: f32) -> f32 {
    linear_to_db(v).max(DB_FLOOR)
}

/// Anzahl der logischen Audio-Kanäle einer Show.
pub const AUDIO_CHANNEL_COUNT: usize = 16;

/// Eine einzelne Patch-Zuordnung: logischer Kanal → Ausgang eines Geräts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelOutput {
    /// Zielgerät (Name); `None` = System-Standard (primäres Gerät).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// 1-basierter Geräte-Ausgang; 0 = aus.
    pub output: u8,
}

/// Logischer Audio-Kanal: Cues routen auf ihn, er kann auf **mehrere**
/// Geräte-Ausgänge liegen — auch auf verschiedenen Geräten gleichzeitig
/// (Mehrgeräte-Betrieb parallel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioChannel {
    /// Anzeigename, z. B. „Mains L".
    pub name: String,
    /// Legacy-Feld (altes Show-Format, ein Ausgang, kein Gerät): wird gelesen,
    /// aber nicht mehr geschrieben; beim Laden in `outputs` migriert.
    #[serde(default, skip_serializing)]
    pub output: u8,
    /// Alle Zuordnungen des Kanals (mehrere Ausgänge/Geräte gleichzeitig).
    #[serde(default)]
    pub outputs: Vec<ChannelOutput>,
}

impl AudioChannel {
    /// Kurze Anzeige der Ziele, z. B. „Out 1+2, Interface Out 1" („off" wenn leer).
    pub fn output_summary(&self) -> String {
        let targets: Vec<String> = self
            .outputs
            .iter()
            .filter(|o| o.output > 0)
            .map(|o| match &o.device {
                None => format!("Out {}", o.output),
                Some(device) => format!("{device} Out {}", o.output),
            })
            .collect();
        if targets.is_empty() {
            "off".to_string()
        } else {
            targets.join(", ")
        }
    }
}

/// Zuordnung der logischen Kanäle zu Geräte-Ausgängen (Workspace-Einstellung).
/// Default: Kanal n → Ausgang n am Standardgerät.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioPatch {
    pub channels: [AudioChannel; AUDIO_CHANNEL_COUNT],
}

impl Default for AudioPatch {
    fn default() -> Self {
        Self {
            channels: std::array::from_fn(|i| AudioChannel {
                name: format!("Ch {}", i + 1),
                output: i as u8 + 1,
                outputs: vec![ChannelOutput {
                    device: None,
                    output: i as u8 + 1,
                }],
            }),
        }
    }
}

impl AudioPatch {
    /// Migriert das Legacy-Format: Kanäle mit leerem `outputs`, aber gesetztem
    /// `output`, bekommen eine Zuordnung am Standardgerät. Aufruf nach jedem
    /// Deserialisieren (Show-Datei, Remote-Command).
    pub fn migrate_legacy(&mut self) {
        for channel in &mut self.channels {
            if channel.outputs.is_empty() && channel.output > 0 {
                channel.outputs = vec![ChannelOutput {
                    device: None,
                    output: channel.output,
                }];
            }
        }
    }

    /// Alle im Patch referenzierten Geräte (`None` = Standardgerät), eindeutig,
    /// in stabiler Reihenfolge (erstes Vorkommen).
    pub fn devices(&self) -> Vec<Option<String>> {
        let mut devices: Vec<Option<String>> = Vec::new();
        for channel in &self.channels {
            for out in &channel.outputs {
                if out.output > 0 && !devices.contains(&out.device) {
                    devices.push(out.device.clone());
                }
            }
        }
        devices
    }

    /// Kanal→Ausgänge als Bitmaske **für ein Gerät** (Bit 0 = Ausgang 1,
    /// 0 = Kanal auf diesem Gerät aus). `device = None` = Standardgerät.
    /// Ein Kanal kann mehrere Ausgänge haben (mehrere Bits).
    pub fn device_output_mask(&self, device: Option<&str>) -> [u16; AUDIO_CHANNEL_COUNT] {
        let mut masks = [0u16; AUDIO_CHANNEL_COUNT];
        for (channel, entry) in self.channels.iter().enumerate() {
            for out in &entry.outputs {
                if out.output == 0 {
                    continue;
                }
                let same_device = match (&out.device, device) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                };
                if same_device && (out.output as usize) <= 16 {
                    masks[channel] |= 1 << (out.output - 1);
                }
            }
        }
        masks
    }

    /// Kanäle als altes 1-Ausgang-Array (nur für Kompatibilität; für die Engine
    /// bitte `device_output_mask` verwenden). Erster Ausgang gewinnt.
    pub fn outputs(&self) -> [u8; AUDIO_CHANNEL_COUNT] {
        std::array::from_fn(|i| {
            self.channels[i]
                .outputs
                .iter()
                .find(|o| o.output > 0)
                .map(|o| o.output)
                .unwrap_or(self.channels[i].output)
        })
    }
}

/// Audio-Routing eines Cues (Mehrkanal-Ausgabe).
///
/// `channels`: Ziel-Kanalanzahl (1 = Mono … 16), `pan`: -1.0 (links) bis 1.0
/// (rechts, Constant-Power, nur bei ≥ 2 Kanälen), `first_channel`: 1-basierter
/// Patch-Start im Ausgabegerät (0 = Default, Patch ab Kanal 1). Die produktive
/// Engine (cpal) routet die Voice-Kanäle 1:1 auf die Device-Kanäle
/// `first_channel … first_channel + channels - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioRoute {
    /// Anzahl der Ziel-Kanäle.
    pub channels: u8,
    /// Pan-Position (-1.0 links … 1.0 rechts).
    pub pan: f32,
    /// 1-basierter Start-Kanal des Patches (0 = Default ab Kanal 1).
    #[serde(default)]
    pub first_channel: u8,
    /// Matrix: pro Datei-Kanal eine Bitmaske der Ziel-Kanäle (Bit 0 = Kanal 1).
    /// `None` = fortlaufend ab `first_channel`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<[u16; AUDIO_CHANNEL_COUNT]>,
}

impl AudioRoute {
    /// Neue Route mit `channels` Kanälen, Pan mittig, Patch ab Kanal 1.
    pub fn new(channels: u8) -> Self {
        Self {
            channels,
            pan: 0.0,
            first_channel: 0,
            matrix: None,
        }
    }

    /// Route mit explizitem Patch-Start (`first_channel`, 1-basiert, 0 = Default).
    pub fn with_patch(channels: u8, first_channel: u8) -> Self {
        Self {
            channels,
            pan: 0.0,
            first_channel,
            matrix: None,
        }
    }

    /// Ziel-Kanäle von Datei-Kanal `source` (0-basiert) als Bitmaske.
    pub fn channel_mask(&self, source: usize) -> u16 {
        if source >= AUDIO_CHANNEL_COUNT {
            return 0;
        }
        if let Some(matrix) = &self.matrix {
            return matrix[source];
        }
        let target = usize::from(self.first_channel.saturating_sub(1)) + source;
        if target < AUDIO_CHANNEL_COUNT {
            1 << target
        } else {
            0
        }
    }

    /// Schaltet die Verbindung Datei-Kanal `source` → Kanal `channel` (beide
    /// 0-basiert) um. Der bisherige Patch wird dabei in die Matrix übernommen.
    pub fn toggle(&mut self, source: usize, channel: usize) {
        let mut matrix = self.matrix.unwrap_or_else(|| {
            let mut matrix = [0; AUDIO_CHANNEL_COUNT];
            for (i, mask) in matrix.iter_mut().enumerate() {
                *mask = self.channel_mask(i);
            }
            matrix
        });
        if source < AUDIO_CHANNEL_COUNT && channel < AUDIO_CHANNEL_COUNT {
            matrix[source] ^= 1 << channel;
        }
        self.matrix = Some(matrix);
    }

    /// Letzter belegter Kanal des Patches (1-basiert); 0 = Default-Patch.
    pub fn last_channel(&self) -> u8 {
        if self.first_channel == 0 {
            0
        } else {
            self.first_channel + self.channels - 1
        }
    }

    /// Validiert die Route.
    pub fn validate(&self) -> ShowResult<()> {
        if !(1..=16).contains(&self.channels) {
            return Err(ShowError::InvalidParameter(format!(
                "invalid channel count: {} (erlaubt: 1–16)",
                self.channels
            )));
        }
        if !(-1.0..=1.0).contains(&self.pan) {
            return Err(ShowError::InvalidParameter(format!(
                "invalid pan: {} (erlaubt: -1.0 … 1.0)",
                self.pan
            )));
        }
        if self.first_channel > 16 {
            return Err(ShowError::InvalidParameter(format!(
                "invalid patch start: {} (erlaubt: 0–16)",
                self.first_channel
            )));
        }
        if self.first_channel > 0 && self.last_channel() > 16 {
            return Err(ShowError::InvalidParameter(format!(
                "patch ch {}–{} exceeds 16 device channels",
                self.first_channel,
                self.last_channel()
            )));
        }
        Ok(())
    }
}

/// Media-Slice: Segment eines Mediums mit eigenem Wiederholungszähler.
///
/// Mehrere Slices bilden einen Abspiel-Plan (z. B. Slice A 2×, dann Slice B 1×).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaSlice {
    /// Start im Medium.
    #[serde(with = "crate::core::duration_ms")]
    pub start: Duration,
    /// Ende im Medium (exklusiv).
    #[serde(with = "crate::core::duration_ms")]
    pub end: Duration,
    /// Wie oft dieses Segment gespielt wird (Play Count).
    pub play_count: u32,
}

impl MediaSlice {
    /// Neuer Slice über `[start, end)`, `play_count` Wiederholungen.
    pub fn new(start: Duration, end: Duration, play_count: u32) -> Self {
        Self {
            start,
            end,
            play_count,
        }
    }

    /// Dauer des Segments.
    pub fn duration(&self) -> Duration {
        self.end.saturating_sub(self.start)
    }

    /// Validiert den Slice.
    pub fn validate(&self) -> ShowResult<()> {
        if self.end <= self.start {
            return Err(ShowError::InvalidParameter(format!(
                "invalid slice: end ({:?}) must be after start ({:?})",
                self.end, self.start
            )));
        }
        if self.play_count == 0 {
            return Err(ShowError::InvalidParameter(
                "invalid slice: play_count must be >= 1".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_without_matrix_keeps_sequential_patch() {
        let route = AudioRoute::with_patch(2, 3);
        assert_eq!(route.channel_mask(0), 1 << 2);
        assert_eq!(route.channel_mask(1), 1 << 3);
        // Überhang am Rand und ungültige Quelle: kein Ziel.
        assert_eq!(AudioRoute::with_patch(2, 16).channel_mask(1), 0);
        assert_eq!(route.channel_mask(AUDIO_CHANNEL_COUNT), 0);
    }

    #[test]
    fn toggle_starts_from_current_patch() {
        let mut route = AudioRoute::with_patch(2, 3);
        route.toggle(0, 7);
        assert_eq!(route.channel_mask(0), (1 << 2) | (1 << 7));
        assert_eq!(route.channel_mask(1), 1 << 3, "anderer Kanal bleibt");
        route.toggle(1, 3);
        assert_eq!(route.channel_mask(1), 0, "alles aus bleibt stumm");
    }

    #[test]
    fn old_route_json_has_no_matrix() {
        let route: AudioRoute =
            serde_json::from_str(r#"{"channels":2,"pan":0.0,"first_channel":3}"#).unwrap();
        assert_eq!(route, AudioRoute::with_patch(2, 3));
    }

    #[test]
    fn default_patch_maps_channel_n_to_output_n() {
        let outputs = AudioPatch::default().outputs();
        assert_eq!(outputs[0], 1);
        assert_eq!(outputs[15], 16);
    }

    #[test]
    fn db_conversion_known_values() {
        assert_eq!(linear_to_db(1.0), 0.0);
        assert!((linear_to_db(0.5) - -6.0206).abs() < 1e-3);
        assert_eq!(linear_to_db(0.1), -20.0);
        assert_eq!(linear_to_db(0.0), f32::NEG_INFINITY);
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn db_conversion_roundtrip() {
        for v in [0.01, 0.1, 0.25, 0.5, 0.75, 1.0] {
            let rt = db_to_linear(linear_to_db(v));
            assert!((rt - v).abs() < 1e-5, "roundtrip failed for {v}: {rt}");
        }
    }

    #[test]
    fn route_validation() {
        assert!(AudioRoute::new(2).validate().is_ok());
        assert!(AudioRoute::new(0).validate().is_err());
        assert!(AudioRoute::new(17).validate().is_err());
        assert!(AudioRoute {
            channels: 2,
            pan: 1.5,
            first_channel: 0,
            matrix: None,
        }
        .validate()
        .is_err());
        assert!(AudioRoute {
            channels: 2,
            pan: -1.0,
            first_channel: 0,
            matrix: None,
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn route_patch_validation() {
        // Patch-Start 1-basiert; 0 = Default (immer gültig).
        assert!(AudioRoute::with_patch(2, 3).validate().is_ok());
        assert!(AudioRoute::with_patch(1, 16).validate().is_ok());
        assert_eq!(AudioRoute::with_patch(2, 3).last_channel(), 4);
        assert_eq!(AudioRoute::new(2).last_channel(), 0);
        // Start jenseits der 16 Device-Kanäle.
        assert!(AudioRoute::with_patch(1, 17).validate().is_err());
        // Patch überhängt die 16 Device-Kanäle.
        assert!(AudioRoute::with_patch(2, 16).validate().is_err());
        assert!(AudioRoute::with_patch(16, 2).validate().is_err());
    }

    #[test]
    fn route_serde_backcompat_and_roundtrip() {
        // Alte Show-Dateien ohne `first_channel` laden als Default (0).
        let old_json = r#"{"channels":2,"pan":0.5}"#;
        let route: AudioRoute = serde_json::from_str(old_json).unwrap();
        assert_eq!(route.first_channel, 0);
        assert_eq!(route.channels, 2);

        let route = AudioRoute::with_patch(4, 3);
        let json = serde_json::to_string(&route).unwrap();
        let back: AudioRoute = serde_json::from_str(&json).unwrap();
        assert_eq!(route, back);
    }

    #[test]
    fn slice_validation_and_duration() {
        let slice = MediaSlice::new(Duration::from_millis(100), Duration::from_millis(250), 2);
        assert_eq!(slice.duration(), Duration::from_millis(150));
        assert!(slice.validate().is_ok());

        assert!(
            MediaSlice::new(Duration::from_millis(250), Duration::from_millis(100), 1)
                .validate()
                .is_err()
        );
        assert!(
            MediaSlice::new(Duration::from_millis(100), Duration::from_millis(250), 0)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn slice_serde_roundtrip() {
        let slice = MediaSlice::new(Duration::from_millis(1500), Duration::from_secs(2), 3);
        let json = serde_json::to_string(&slice).unwrap();
        let back: MediaSlice = serde_json::from_str(&json).unwrap();
        assert_eq!(slice, back);
    }
}

#[cfg(test)]
mod patch_matrix_tests {
    use super::*;

    #[test]
    fn legacy_output_field_migrates_to_outputs() {
        // Altes Show-Format: Kanal mit nur "output", ohne outputs.
        let channel: AudioChannel = serde_json::from_str(r#"{"name":"Ch 1","output":3}"#).unwrap();
        assert_eq!(channel.output, 3);
        assert!(channel.outputs.is_empty());
        let mut patch = AudioPatch::default();
        patch.channels[0] = channel;
        patch.migrate_legacy();
        assert_eq!(
            patch.channels[0].outputs,
            vec![ChannelOutput {
                device: None,
                output: 3
            }]
        );
    }

    #[test]
    fn device_output_mask_supports_multiple_outputs_and_devices() {
        let mut patch = AudioPatch::default();
        // Kanal 1 -> Ausgaenge 1+2 am Standardgeraet UND Ausgang 1 am Interface.
        patch.channels[0].outputs = vec![
            ChannelOutput {
                device: None,
                output: 1,
            },
            ChannelOutput {
                device: None,
                output: 2,
            },
            ChannelOutput {
                device: Some("Interface".to_string()),
                output: 1,
            },
        ];
        // Kanal 2 -> nur Ausgang 5 am Interface.
        patch.channels[1].outputs = vec![ChannelOutput {
            device: Some("Interface".to_string()),
            output: 5,
        }];
        let std_mask = patch.device_output_mask(None);
        assert_eq!(std_mask[0], 0b11, "Kanal 1 auf Ausgaenge 1+2");
        assert_eq!(std_mask[1], 0, "Kanal 2 nicht am Standardgeraet");
        let iface_mask = patch.device_output_mask(Some("Interface"));
        assert_eq!(iface_mask[0], 0b1, "Kanal 1 auf Interface-Ausgang 1");
        assert_eq!(iface_mask[1], 0b10000, "Kanal 2 auf Interface-Ausgang 5");
        assert_eq!(patch.devices(), vec![None, Some("Interface".to_string())]);
    }

    #[test]
    fn patch_serde_roundtrip_and_legacy_read() {
        let mut patch = AudioPatch::default();
        patch.channels[2].outputs.push(ChannelOutput {
            device: Some("Blackmagic".to_string()),
            output: 7,
        });
        let json = serde_json::to_string(&patch).unwrap();
        let back: AudioPatch = serde_json::from_str(&json).unwrap();
        for i in 0..AUDIO_CHANNEL_COUNT {
            assert_eq!(back.channels[i].name, patch.channels[i].name);
            assert_eq!(back.channels[i].outputs, patch.channels[i].outputs);
        }
    }
}
