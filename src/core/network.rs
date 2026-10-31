//! Network-Cue (QLab 5 „Network Cue"): sendet eine Nachricht per UDP oder
//! TCP an `host:port`, wenn der Cue feuert. Use-Case: andere Geräte im
//! Show-Netz triggern (Medienserver, Projektoren, Companion-Ziele …).
//!
//! Der Cue gilt sofort als abgeschlossen (Sofort-Cue wie QLab); das
//! eigentliche Senden erledigt die Engine asynchron
//! ([`crate::engine::EngineCommand::SendNetwork`]) – ein blockierendes Ziel
//! darf den Engine-Thread nicht aufhalten.

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Transport-Protokoll eines Network-Cues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NetworkProtocol {
    /// UDP (fire-and-forget, kein Verbindungsaufbau).
    #[default]
    Udp,
    /// TCP (Verbindung aufbauen, Nachricht schreiben, Verbindung schließen).
    Tcp,
}

impl NetworkProtocol {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [NetworkProtocol; 2] = [NetworkProtocol::Udp, NetworkProtocol::Tcp];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            NetworkProtocol::Udp => "UDP",
            NetworkProtocol::Tcp => "TCP",
        }
    }
}

/// Netzwerk-Nachricht eines Network-Cues ([`crate::core::CueType::Network`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkMessage {
    /// Transport-Protokoll.
    #[serde(default)]
    pub protocol: NetworkProtocol,
    /// Ziel-Host (IP oder Hostname).
    #[serde(default)]
    pub host: String,
    /// Ziel-Port.
    #[serde(default)]
    pub port: u16,
    /// Zu sendende Nachricht (UTF-8, wird 1:1 gesendet).
    #[serde(default)]
    pub message: String,
}

impl Default for NetworkMessage {
    fn default() -> Self {
        Self {
            protocol: NetworkProtocol::Udp,
            host: String::new(),
            port: 0,
            message: String::new(),
        }
    }
}

impl NetworkMessage {
    /// Prüft die Konfiguration (GO setzt den Cue sonst auf `Error`).
    pub fn validate(&self) -> ShowResult<()> {
        if self.host.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "network cue: no host".to_string(),
            ));
        }
        if self.port == 0 {
            return Err(ShowError::InvalidParameter(
                "network cue: no port".to_string(),
            ));
        }
        Ok(())
    }

    /// Ziel als `host:port` (für Anzeigen).
    pub fn target(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_requires_host_and_port() {
        let msg = NetworkMessage::default();
        assert!(msg.validate().is_err());
        let msg = NetworkMessage {
            host: "192.168.1.10".to_string(),
            port: 7000,
            ..Default::default()
        };
        assert!(msg.validate().is_ok());
        assert_eq!(msg.target(), "192.168.1.10:7000");
        assert_eq!(msg.protocol.label(), "UDP");
        assert_eq!(NetworkProtocol::Tcp.label(), "TCP");
    }

    #[test]
    fn serde_roundtrip_and_legacy_default() {
        let msg = NetworkMessage {
            protocol: NetworkProtocol::Tcp,
            host: "10.0.0.5".to_string(),
            port: 9000,
            message: "GO".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: NetworkMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        // Legacy: nur message → Rest defaultet.
        let back: NetworkMessage = serde_json::from_str("{\"message\":\"x\"}").unwrap();
        assert_eq!(back.protocol, NetworkProtocol::Udp);
        assert_eq!(back.port, 0);
    }
}
