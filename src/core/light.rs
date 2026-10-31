//! Light-Cue (QLab 5 „Light Cue", Minimal-Variante): sendet ein **Art-Net
//! DMX-Paket** (OpDmx) per UDP an ein Lighting-Netz (Port 6454). Damit lassen
//! sich DMX-Universen an Nodes/Gateways triggern – ohne eigenes DMX-Interface.
//!
//! Das Paket wird zentral in [`LightMessage::to_artnet`] gebaut; die Engine
//! schickt es fire-and-forget ([`crate::engine::EngineCommand::SendDmx`]).

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Art-Net-UDP-Port (Standard).
pub const ARTNET_PORT: u16 = 6454;

/// DMX512: 512 Kanäle pro Universe.
pub const DMX_CHANNELS: usize = 512;

/// Licht-Nachricht eines Light-Cues ([`crate::core::CueType::Light`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightMessage {
    /// DMX-Universe (0–32767; Art-Net Net:SubUni).
    #[serde(default)]
    pub universe: u16,
    /// Erster DMX-Kanal (1-basiert, 1–512) – QLab „channel".
    #[serde(default = "default_start_channel")]
    pub start_channel: u16,
    /// Kanalwerte (0–255), ab `start_channel`.
    #[serde(default = "default_values")]
    pub values: Vec<u8>,
    /// Ziel-Host (Broadcast für das lokale Licht-Netz, z. B.
    /// „255.255.255.255“ oder „2.255.255.255“).
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_start_channel() -> u16 {
    1
}
fn default_values() -> Vec<u8> {
    vec![255]
}
fn default_host() -> String {
    "255.255.255.255".to_string()
}

impl Default for LightMessage {
    fn default() -> Self {
        Self {
            universe: 0,
            start_channel: default_start_channel(),
            values: default_values(),
            host: default_host(),
        }
    }
}

impl LightMessage {
    /// Prüft die Konfiguration (GO setzt den Cue sonst auf `Error`).
    pub fn validate(&self) -> ShowResult<()> {
        if self.universe > 0x7FFF {
            return Err(ShowError::InvalidParameter(
                "light cue: universe must be 0-32767".to_string(),
            ));
        }
        if self.start_channel == 0 || self.start_channel as usize > DMX_CHANNELS {
            return Err(ShowError::InvalidParameter(
                "light cue: start channel must be 1-512".to_string(),
            ));
        }
        if self.values.is_empty() {
            return Err(ShowError::InvalidParameter(
                "light cue: no channel values".to_string(),
            ));
        }
        if self.start_channel as usize - 1 + self.values.len() > DMX_CHANNELS {
            return Err(ShowError::InvalidParameter(
                "light cue: values exceed DMX channel 512".to_string(),
            ));
        }
        if self.host.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "light cue: no host".to_string(),
            ));
        }
        Ok(())
    }

    /// Baut das Art-Net-ArtDmx-Paket (OpCode 0x5000, Protokoll v14).
    ///
    /// Layout: `"Art-Net\0"`, OpCode LE, ProtVer 14 LE, Sequence 0,
    /// Physical 0, SubUni, Net, Length (BE, 2–512), Data.
    pub fn to_artnet(&self) -> Vec<u8> {
        let len = self.values.len().clamp(2, DMX_CHANNELS) as u16;
        let mut packet = Vec::with_capacity(18 + len as usize);
        packet.extend_from_slice(b"Art-Net\0");
        packet.extend_from_slice(&0x5000u16.to_le_bytes()); // OpDmx
        packet.extend_from_slice(&14u16.to_le_bytes()); // Protocol version
        packet.push(0); // Sequence (0 = nicht sequenziert)
        packet.push(0); // Physical input
        packet.push((self.universe & 0xFF) as u8); // SubUni (low)
        packet.push((self.universe >> 8) as u8); // Net (high)
        packet.extend_from_slice(&len.to_be_bytes()); // Data length
        packet.extend_from_slice(&self.values[..len as usize]);
        packet
    }

    /// Anzeige-Text (z. B. „DMX u0 ch1: 255, 128“).
    pub fn describe(&self) -> String {
        let vals: Vec<String> = self.values.iter().map(|v| v.to_string()).collect();
        let shown = if vals.len() > 4 {
            format!("{} … ({} Kanäle)", vals[..4].join(","), vals.len())
        } else {
            vals.join(",")
        };
        format!("DMX u{} ch{}: {}", self.universe, self.start_channel, shown)
    }

    /// Parst eine Kanalwert-Eingabe („255, 128, 0x40“ – Dezimal oder 0x-Hex).
    pub fn parse_values(text: &str) -> Result<Vec<u8>, String> {
        let mut values = Vec::new();
        for part in text.split([',', ' ', ';']) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let value = if let Some(hex) =
                part.strip_prefix("0x").or_else(|| part.strip_prefix("0X"))
            {
                u8::from_str_radix(hex, 16).map_err(|_| format!("ungültiger Hex-Wert: {part}"))?
            } else {
                part.parse::<u8>()
                    .map_err(|_| format!("ungültiger Kanalwert (0-255): {part}"))?
            };
            values.push(value);
        }
        Ok(values)
    }

    /// Formatiert die Werte als Eingabe-Text (dezimal, Komma-getrennt).
    pub fn format_values(&self) -> String {
        self.values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artnet_packet_layout() {
        let msg = LightMessage {
            universe: 1,
            start_channel: 5,
            values: vec![255, 128, 0],
            host: "127.0.0.1".to_string(),
        };
        let p = msg.to_artnet();
        assert_eq!(&p[0..8], b"Art-Net\0");
        assert_eq!(u16::from_le_bytes([p[8], p[9]]), 0x5000, "OpDmx");
        assert_eq!(u16::from_le_bytes([p[10], p[11]]), 14, "ProtVer");
        assert_eq!(p[14], 1, "SubUni = universe low");
        assert_eq!(p[15], 0, "Net = universe high");
        assert_eq!(u16::from_be_bytes([p[16], p[17]]), 3, "Data length");
        assert_eq!(&p[18..], &[255, 128, 0]);
        assert_eq!(p.len(), 21);
    }

    #[test]
    fn artnet_universe_high_byte() {
        let msg = LightMessage {
            universe: 0x0102,
            values: vec![1, 2],
            ..Default::default()
        };
        let p = msg.to_artnet();
        assert_eq!(p[14], 0x02);
        assert_eq!(p[15], 0x01);
    }

    #[test]
    fn validate_ranges() {
        assert!(LightMessage::default().validate().is_ok());
        let bad = LightMessage {
            start_channel: 0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = LightMessage {
            start_channel: 511,
            values: vec![0, 0, 0],
            ..Default::default()
        };
        assert!(bad.validate().is_err(), "511 + 3 Werte > 512");
        let bad = LightMessage {
            values: vec![],
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = LightMessage {
            universe: 0x8000,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn parse_and_format_values() {
        assert_eq!(
            LightMessage::parse_values("255, 128, 0x40").unwrap(),
            vec![255, 128, 64]
        );
        assert_eq!(
            LightMessage::parse_values("10;20 30").unwrap(),
            vec![10, 20, 30]
        );
        assert!(LightMessage::parse_values("300").is_err());
        assert!(LightMessage::parse_values("0xZZ").is_err());
        let msg = LightMessage {
            values: vec![1, 2, 255],
            ..Default::default()
        };
        assert_eq!(
            LightMessage::parse_values(&msg.format_values()).unwrap(),
            vec![1, 2, 255]
        );
        assert!(msg.describe().contains("DMX u0 ch1: 1,2,255"));
    }

    #[test]
    fn serde_roundtrip_and_legacy_default() {
        let msg = LightMessage {
            universe: 3,
            start_channel: 17,
            values: vec![10, 20],
            host: "10.0.0.255".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: LightMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        let back: LightMessage = serde_json::from_str("{}").unwrap();
        assert_eq!(back, LightMessage::default());
    }
}
