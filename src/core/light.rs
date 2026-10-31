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

/// sACN (E1.31)-UDP-Port (Standard).
pub const SACN_PORT: u16 = 5568;

/// DMX512: 512 Kanäle pro Universe.
pub const DMX_CHANNELS: usize = 512;

/// Protokoll des Light-Cues: Art-Net (ArtDmx) oder sACN (E1.31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum LightProtocol {
    /// Art-Net (Port 6454, OpDmx 0x5000).
    #[default]
    ArtNet,
    /// sACN / E1.31 (Port 5568).
    Sacn,
}

impl LightProtocol {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [LightProtocol; 2] = [LightProtocol::ArtNet, LightProtocol::Sacn];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            LightProtocol::ArtNet => "Art-Net",
            LightProtocol::Sacn => "sACN (E1.31)",
        }
    }
    /// UDP-Zielport des Protokolls.
    pub fn port(self) -> u16 {
        match self {
            LightProtocol::ArtNet => ARTNET_PORT,
            LightProtocol::Sacn => SACN_PORT,
        }
    }
}

/// Licht-Nachricht eines Light-Cues ([`crate::core::CueType::Light`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightMessage {
    /// Protokoll (Art-Net oder sACN).
    #[serde(default)]
    pub protocol: LightProtocol,
    /// DMX-Universe (0–32767; Art-Net Net:SubUni bzw. sACN Universe 1–63999).
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
            protocol: LightProtocol::ArtNet,
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
        let uni_ok = match self.protocol {
            LightProtocol::ArtNet => self.universe <= 0x7FFF,
            // sACN-Universen sind 1-basiert (1–63999).
            LightProtocol::Sacn => (1..=63999).contains(&self.universe),
        };
        if !uni_ok {
            return Err(ShowError::InvalidParameter(format!(
                "light cue: invalid universe for {} (Art-Net 0-32767, sACN 1-63999)",
                self.protocol.label()
            )));
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

    /// Mischt die Kanalwerte ab `start_channel` in einen DMX-Universe-Stand
    /// (512 Kanäle, ab Kanal 1). Aufruf in der Engine pro Universe, damit ein
    /// Cue mit `start_channel > 1` nicht alle Kanäle davor auf 0 setzt.
    /// Panicfrei für beliebige (auch ungültige) Eingaben.
    pub fn apply_to(&self, state: &mut [u8; DMX_CHANNELS]) {
        let start = (self.start_channel.clamp(1, DMX_CHANNELS as u16) - 1) as usize;
        let end = (start + self.values.len()).min(DMX_CHANNELS);
        if start < end {
            state[start..end].copy_from_slice(&self.values[..end - start]);
        }
    }

    /// Baut das Art-Net-ArtDmx-Paket (OpCode 0x5000, Protokoll v14) aus einem
    /// vollständigen Universe-Stand (immer 512 Datenbytes – gerade Länge,
    /// Daten ab Kanal 1; `start_channel` ist im Stand bereits eingemischt).
    ///
    /// Layout: `"Art-Net\0"`, OpCode LE, ProtVer 14 LE, Sequence,
    /// Physical 0, SubUni, Net, Length (BE, 512), Data.
    pub fn to_artnet(&self) -> Vec<u8> {
        let mut state = [0u8; DMX_CHANNELS];
        self.apply_to(&mut state);
        artnet_packet(self.universe, &state, 0)
    }

    /// Baut das sACN-(E1.31)-DMP-Paket (UDP-Port 5568).
    ///
    /// Root-Layer: Preamble 0x0010, Postamble 0x0000, ACN-Identifier
    /// „ASC-E1.31\0\0\0", Flags+Length 0x7000|root_len, Vector
    /// VECTOR_ROOT_E131_DATA (0x00000004), CID (16 B, Null).
    /// Daten-Layer: Flags+Length 0x7000|len, VECTOR_E131_DATA_PACKET
    /// (0x00000004), Source-Name (64 B, leer), Priority 100, Sync 0,
    /// Sequence 0, Options 0, Universe (BE u16, 1-basiert).
    /// DMP-Layer: Flags+Length 0x7200|len, Vector 0x02, Addr-Type 0xA1,
    /// First-Property 0, Increment 1, Count = dmx_len, Startcode 0x00,
    /// DMX-Daten.
    pub fn to_sacn(&self) -> Vec<u8> {
        let mut state = [0u8; DMX_CHANNELS];
        self.apply_to(&mut state);
        sacn_packet(self.universe, &state, 0, &[0u8; 16], "KuhPlay")
    }

    /// Baut das Paket zum gewählten Protokoll und liefert den Zielport.
    pub fn packet(&self) -> (Vec<u8>, u16) {
        match self.protocol {
            LightProtocol::ArtNet => (self.to_artnet(), ARTNET_PORT),
            LightProtocol::Sacn => (self.to_sacn(), SACN_PORT),
        }
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

/// Art-Net-ArtDmx-Paket aus einem vollständigen Universe-Stand (512 B, ab Kanal 1).
pub fn artnet_packet(universe: u16, state: &[u8; DMX_CHANNELS], sequence: u8) -> Vec<u8> {
    let mut packet = Vec::with_capacity(18 + DMX_CHANNELS);
    packet.extend_from_slice(b"Art-Net\0");
    packet.extend_from_slice(&0x5000u16.to_le_bytes()); // OpDmx
    packet.extend_from_slice(&14u16.to_le_bytes()); // Protocol version
    packet.push(sequence); // Sequence
    packet.push(0); // Physical input
    packet.push((universe & 0xFF) as u8); // SubUni (low)
    packet.push((universe >> 8) as u8); // Net (high)
    packet.extend_from_slice(&(DMX_CHANNELS as u16).to_be_bytes()); // Data length (512, gerade)
    packet.extend_from_slice(state);
    packet
}

/// sACN-(E1.31)-DMP-Paket (UDP-Port 5568) aus einem vollständigen Universe-Stand.
///
/// Wie bisher, aber: Sequenz läuft (Engine vergibt pro Universe), CID ist fest
/// (nicht Null) und der Source-Name ist gesetzt.
#[allow(clippy::too_many_arguments)]
pub fn sacn_packet(
    universe: u16,
    state: &[u8; DMX_CHANNELS],
    sequence: u8,
    cid: &[u8; 16],
    source_name: &str,
) -> Vec<u8> {
    let dmx_len = (DMX_CHANNELS + 1) as u16; // 512 Daten + Startcode = 513
    let dmp_len = 10 + dmx_len as u32; // 2 + 8 (DMP-Header) + dmx_len
    let data_len = 77 + dmp_len; // 2 + 75 (E1.31-Header) + dmp_len
    let root_len = 22 + data_len; // 2 + 4 (Vector) + 16 (CID) + data_len
    let mut p = Vec::with_capacity(126 + dmx_len as usize);
    p.extend_from_slice(&0x0010u16.to_be_bytes()); // Preamble
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // Postamble
    p.extend_from_slice(b"ASC-E1.31\0\0\0"); // ACN-Identifier (12 B)
    p.extend_from_slice(&(0x7000 | root_len as u16).to_be_bytes());
    p.extend_from_slice(&0x00000004u32.to_be_bytes()); // VECTOR_ROOT_E131_DATA
    p.extend_from_slice(cid); // CID (16 B, fest pro KuhPlay-Instanz)
    p.extend_from_slice(&(0x7000 | data_len as u16).to_be_bytes());
    p.extend_from_slice(&0x00000004u32.to_be_bytes()); // VECTOR_E131_DATA_PACKET
    let mut name = [0u8; 64];
    let src = source_name.as_bytes();
    let n = src.len().min(64);
    name[..n].copy_from_slice(&src[..n]);
    p.extend_from_slice(&name); // Source-Name (64 B)
    p.push(100); // Priority
    p.extend_from_slice(&0u16.to_be_bytes()); // Sync-Address
    p.push(sequence); // Sequence (läuft pro Universe)
    p.push(0); // Options
    p.extend_from_slice(&universe.to_be_bytes()); // Universe
    p.extend_from_slice(&(0x7000 | dmp_len as u16).to_be_bytes());
    p.push(0x02); // VECTOR_DMP_DATA
    p.push(0xA1); // Address-Type & Data-Type
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // First Property Address
    p.extend_from_slice(&0x0001u16.to_be_bytes()); // Address Increment
    p.extend_from_slice(&dmx_len.to_be_bytes()); // Property Value Count
    p.push(0x00); // DMX-Startcode
    p.extend_from_slice(state);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artnet_packet_layout() {
        let msg = LightMessage {
            protocol: LightProtocol::ArtNet,
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
        // Vollständiges Universe (512 B, gerade Länge); start_channel=5 wird
        // eingemischt: Werte landen ab Daten-Index 4.
        assert_eq!(u16::from_be_bytes([p[16], p[17]]), 512, "Data length");
        assert_eq!(&p[18 + 4..18 + 7], &[255, 128, 0]);
        assert_eq!(p.len(), 18 + 512);
    }

    #[test]
    fn packet_panicfrei_fuer_jede_eingabe() {
        // Regression: früher panikte to_artnet/to_sacn bei < 2 Werten.
        for values in [vec![], vec![255], vec![1, 2, 3]] {
            for start in [1u16, 100, 512] {
                let msg = LightMessage {
                    start_channel: start,
                    values: values.clone(),
                    ..Default::default()
                };
                let _ = msg.packet();
                let _ = msg.to_artnet();
                let msg = LightMessage {
                    protocol: LightProtocol::Sacn,
                    universe: 1,
                    start_channel: start,
                    values: values.clone(),
                    ..Default::default()
                };
                let _ = msg.packet();
                let _ = msg.to_sacn();
            }
        }
    }

    #[test]
    fn start_channel_wird_eingemischt() {
        let msg = LightMessage {
            start_channel: 100,
            values: vec![7],
            ..Default::default()
        };
        let p = msg.to_artnet();
        assert_eq!(p[18 + 99], 7, "Wert landet auf Kanal 100 (Index 99)");
        assert_eq!(p[18], 0, "Kanal 1 bleibt 0 (Zero-State)");
        // apply_to mischt in vorhandenen Stand ein:
        let mut state = [0u8; DMX_CHANNELS];
        state[0] = 9;
        msg.apply_to(&mut state);
        assert_eq!(state[0], 9, "bestehender Kanal 1 bleibt erhalten");
        assert_eq!(state[99], 7);
    }

    #[test]
    fn artnet_datenlaenge_immer_gerade() {
        let msg = LightMessage {
            values: vec![1],
            ..Default::default()
        };
        let p = msg.to_artnet();
        let len = u16::from_be_bytes([p[16], p[17]]);
        assert_eq!(len % 2, 0, "Art-Net verlangt gerade Länge");
        assert!((2..=512).contains(&len));
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
    fn sacn_packet_layout() {
        let msg = LightMessage {
            protocol: LightProtocol::Sacn,
            universe: 5,
            start_channel: 1,
            values: vec![10, 20, 30],
            host: "127.0.0.1".to_string(),
        };
        let p = msg.to_sacn();
        // dmx_len = 512 + Startcode = 513 → Paket = 125 + 513 = 638 Bytes.
        assert_eq!(p.len(), 638);
        assert_eq!(&p[4..16], b"ASC-E1.31\0\0\0");
        let root_len = u16::from_be_bytes([p[16], p[17]]) & 0x0FFF;
        assert_eq!(root_len as usize, 22 + 77 + 10 + 513);
        assert_eq!(u32::from_be_bytes([p[18], p[19], p[20], p[21]]), 0x00000004);
        // CID + Source-Name gesetzt (Legacy-Test hatte Null-CID/leeren Namen).
        let cid = [0x11u8; 16];
        let p = sacn_packet(5, &[0u8; DMX_CHANNELS], 7, &cid, "KuhPlay");
        assert_eq!(&p[22..38], &cid, "CID fest");
        assert_eq!(&p[44..51], b"KuhPlay", "Source-Name");
        assert_eq!(p[111], 7, "Sequence");
        let data_len = u16::from_be_bytes([p[38], p[39]]) & 0x0FFF;
        assert_eq!(data_len as usize, 77 + 10 + 513);
        assert_eq!(u32::from_be_bytes([p[40], p[41], p[42], p[43]]), 0x00000004);
        assert_eq!(p[108], 100, "Priority");
        assert_eq!(u16::from_be_bytes([p[113], p[114]]), 5, "Universe BE");
        let dmp_len = u16::from_be_bytes([p[115], p[116]]) & 0x0FFF;
        assert_eq!(dmp_len as usize, 10 + 513);
        assert_eq!(p[117], 0x02);
        assert_eq!(p[118], 0xA1);
        assert_eq!(u16::from_be_bytes([p[123], p[124]]), 513);
        assert_eq!(p[125], 0x00, "Startcode");
        // Werte landen ab Kanal 1 im DMX-Block (msg.to_sacn mit Zero-State):
        let p = msg.to_sacn();
        assert_eq!(&p[126..129], &[10, 20, 30]);
        // Protokoll-Port + Paket-Auswahl.
        assert_eq!(LightProtocol::Sacn.port(), SACN_PORT);
        let (packet, port) = msg.packet();
        assert_eq!(port, SACN_PORT);
        assert_eq!(packet, p);
        assert_eq!(LightProtocol::ArtNet.port(), ARTNET_PORT);
    }

    #[test]
    fn sacn_universe_must_be_1_based() {
        let msg = LightMessage {
            protocol: LightProtocol::Sacn,
            universe: 0,
            ..Default::default()
        };
        assert!(msg.validate().is_err());
        let msg = LightMessage {
            protocol: LightProtocol::Sacn,
            universe: 64000,
            ..Default::default()
        };
        assert!(msg.validate().is_err());
        let msg = LightMessage {
            protocol: LightProtocol::Sacn,
            universe: 1,
            ..Default::default()
        };
        assert!(msg.validate().is_ok());
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
            protocol: LightProtocol::ArtNet,
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
