//! Companion-Cue: triggert **Bitfocus Companion** über dessen HTTP-Remote-
//! Control-API (POST `/api/location/<page>/<row>/<column>/<action>` auf dem
//! Companion-Web-Port, Default 8000). Damit kann ein KuhPlay-Cue z. B. einen
//! Button auf einem Companion-Surface drücken (Gegenrichtung zum
//! KuhPlay-Companion-Modul, das Companion → KuhPlay steuert).
//!
//! Der Cue gilt sofort als abgeschlossen (Sofort-Cue); das HTTP-POST erledigt
//! die Engine asynchron ([`crate::engine::EngineCommand::SendHttpPost`]).

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Default-Port des Companion-Web-UI (dort läuft die HTTP-API).
pub const COMPANION_DEFAULT_PORT: u16 = 8000;

/// Aktion auf einem Companion-Button (HTTP-Remote-Control).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompanionAction {
    /// Button drücken + loslassen (down+up) – Standard.
    #[default]
    Press,
    /// Button drücken und halten (down).
    Down,
    /// Button loslassen (up).
    Up,
}

impl CompanionAction {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [CompanionAction; 3] = [
        CompanionAction::Press,
        CompanionAction::Down,
        CompanionAction::Up,
    ];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            CompanionAction::Press => "press",
            CompanionAction::Down => "down",
            CompanionAction::Up => "up",
        }
    }
}

/// Companion-Trigger eines Companion-Cues ([`crate::core::CueType::Companion`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionTrigger {
    /// Companion-Host (IP/Name).
    #[serde(default = "default_host")]
    pub host: String,
    /// Web-Port der Companion-HTTP-API (Default 8000).
    #[serde(default = "default_port")]
    pub port: u16,
    /// Seite (1-basiert, Companion-Page).
    #[serde(default = "default_one")]
    pub page: u8,
    /// Zeile (0-basiert, Companion-Row).
    #[serde(default)]
    pub row: u8,
    /// Spalte (0-basiert, Companion-Column).
    #[serde(default)]
    pub column: u8,
    /// Aktion (press/down/up).
    #[serde(default)]
    pub action: CompanionAction,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    COMPANION_DEFAULT_PORT
}
fn default_one() -> u8 {
    1
}

impl Default for CompanionTrigger {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            page: 1,
            row: 0,
            column: 0,
            action: CompanionAction::Press,
        }
    }
}

impl CompanionTrigger {
    /// Prüft die Konfiguration (GO setzt den Cue sonst auf `Error`).
    pub fn validate(&self) -> ShowResult<()> {
        if self.host.trim().is_empty() {
            return Err(ShowError::InvalidParameter(
                "companion cue: no host".to_string(),
            ));
        }
        if self.port == 0 {
            return Err(ShowError::InvalidParameter(
                "companion cue: no port".to_string(),
            ));
        }
        Ok(())
    }

    /// HTTP-Pfad der Remote-Control-API, z. B. `/api/location/1/0/0/press`.
    pub fn path(&self) -> String {
        format!(
            "/api/location/{}/{}/{}/{}",
            self.page,
            self.row,
            self.column,
            self.action.label()
        )
    }

    /// Anzeige-Text (z. B. „companion 1/0/0 press“).
    pub fn describe(&self) -> String {
        format!(
            "companion {}:{} p{} r{}/c{} {}",
            self.host,
            self.port,
            self.page,
            self.row,
            self.column,
            self.action.label()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_and_describe() {
        let t = CompanionTrigger {
            host: "192.168.88.50".to_string(),
            port: 8000,
            page: 2,
            row: 3,
            column: 5,
            action: CompanionAction::Down,
        };
        assert_eq!(t.path(), "/api/location/2/3/5/down");
        assert!(t.describe().contains("p2 r3/c5 down"));
        assert!(CompanionTrigger::default().validate().is_ok());
        assert_eq!(
            CompanionTrigger::default().path(),
            "/api/location/1/0/0/press"
        );
        let bad = CompanionTrigger {
            host: " ".to_string(),
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn serde_roundtrip_and_legacy_default() {
        let t = CompanionTrigger {
            page: 4,
            column: 7,
            action: CompanionAction::Up,
            ..Default::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: CompanionTrigger = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // Legacy: leeres Objekt → Defaults.
        let back: CompanionTrigger = serde_json::from_str("{}").unwrap();
        assert_eq!(back, CompanionTrigger::default());
    }
}
