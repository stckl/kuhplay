//! Remote-Steuerung: Web-Remote (HTTP/WebSocket) + Companion (TCP).
//!
//! Beide Protokolle teilen sich die gleichen [`RemoteCommand`] (eingehend)
//! und [`RemoteEvent`] (ausgehend, bidirektionales Feedback).
//!
//! ## Web-Remote
//! Browser-basierte Fernsteuerung über HTTP + WebSocket (Port 8080).
//! Siehe [`web`].
//!
//! ## Companion (Bitfocus)
//! TCP-Server (Port 9999) mit JSON-lines Protokoll – bidirektional:
//! Companion sendet Commands, die App pusht Status-Updates zurück.
//! Siehe [`companion`].

pub mod companion;
pub mod midi;
pub mod osc;
pub mod web;

pub use companion::start_companion_server;
pub use midi::{start_midi_input, MidiNoteMap};
pub use osc::start_osc_server;
pub use web::start_web_remote;

use crate::core::{AudioPatch, Cue, CueType, Stage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use uuid::Uuid;

/// Eingehende Remote-Commands (von Web-Remote oder Companion).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RemoteCommand {
    /// GO: nächsten armed Cue triggern.
    Go,
    /// Panic: alles sofort stoppen.
    Panic,
    /// Pause All: alle laufenden Cues pausieren.
    PauseAll,
    /// Resume All: alle pausierten Cues fortsetzen.
    ResumeAll,
    /// Reset: alle Cues stoppen + Playhead an den Anfang.
    Reset,
    /// Einzelnen Cue starten (per Q-Nummer, z. B. "5.1").
    GoTo {
        cue: String,
    },
    /// Cue armen/disarmen (per Q-Nummer).
    Arm {
        cue: String,
        armed: bool,
    },
    /// Einzelnen Cue pausieren/fortsetzen (per ID).
    PauseCue {
        id: Uuid,
    },
    ResumeCue {
        id: Uuid,
    },
    /// Einzelnen Cue stoppen (per ID).
    StopCue {
        id: Uuid,
    },
    /// Show-Mode umschalten.
    SetShowMode {
        enabled: bool,
    },
    /// Status anfordern (Antwort: [`RemoteEvent::Status`]).
    GetStatus,
    /// Rohe MIDI Note On (Channel 1–16, Note 0–127) – für Cue-MIDI-Trigger.
    MidiNote {
        channel: u8,
        note: u8,
    },
    /// OSC-Nachricht an eine nicht belegte Adresse (volle Adresse, z. B.
    /// „/kuhplay/cue/5") – für Cue-OSC-Trigger.
    OscAddress {
        address: String,
    },
    /// Externer Timecode (LTC/MTC/OSC-Eingang): chasing Cues springen mit,
    /// Timecode-Trigger-Cues feuern (siehe `CueList::receive_timecode`).
    ReceiveTimecode {
        tc: crate::core::Timecode,
    },
    /// Neuen Cue im aktiven Cart anlegen.
    NewCue {
        cue_type: CueType,
    },
    /// Felder eines Cues ändern (JSON-Objekt, Feldnamen wie im Show-Dokument).
    UpdateCue {
        id: Uuid,
        changes: serde_json::Value,
    },
    DeleteCue {
        id: Uuid,
    },
    DuplicateCues {
        ids: Vec<Uuid>,
    },
    GroupCues {
        ids: Vec<Uuid>,
    },
    /// Top-Level-Cue an Listenposition `to` verschieben.
    MoveCue {
        id: Uuid,
        to: usize,
    },
    /// Playhead auf einen Top-Level-Cue setzen.
    SetPlayhead {
        id: Uuid,
    },
    SetActiveCart {
        index: usize,
    },
    AddCart {
        name: String,
    },
    SetShowName {
        name: String,
    },
    NewShow,
    /// In die aktuelle Show-Datei speichern.
    SaveShow,
    /// Show aus einer `.kuhplay`-Datei auf dem KuhPlay-Rechner laden.
    OpenShow {
        path: String,
    },
    /// Show als `.kuhplay`-Datei auf dem KuhPlay-Rechner speichern.
    SaveShowAs {
        path: String,
    },
    SetAudition {
        enabled: bool,
    },
    /// Ausblendzeit von Panic in Millisekunden (0 = sofort stoppen).
    SetPanicDuration {
        ms: u64,
    },
    /// Audio-Gerät wählen (`None` = System-Standard).
    SetAudioOutput {
        device: Option<String>,
    },
    SetAudioPatch {
        patch: Box<AudioPatch>,
    },
    RefreshAudioDevices,
    /// Video-Ausgabe: `off`, `preview` oder `displays`.
    SetOutputMode {
        mode: String,
    },
    AddRoute {
        name: String,
        width: u32,
        height: u32,
    },
    UpdateRoute {
        id: Uuid,
        changes: serde_json::Value,
    },
    RemoveRoute {
        id: Uuid,
    },
    AddStage {
        name: String,
    },
    UpdateStage {
        name: String,
        changes: serde_json::Value,
    },
    RemoveStage {
        name: String,
    },
    RenameStage {
        old: String,
        new: String,
    },
    /// Region einer Stage anlegen (Felder wie im Show-Dokument).
    AddRegion {
        stage: String,
        changes: serde_json::Value,
    },
    UpdateRegion {
        stage: String,
        region: Uuid,
        changes: serde_json::Value,
    },
    RemoveRegion {
        stage: String,
        region: Uuid,
    },
    /// JavaScript in der laufenden WebView eines HTML5-Cues ausführen.
    EvalJs {
        id: Uuid,
        script: String,
    },
    /// JSON-Payload eines HTML-Data-Cues an sein Ziel senden.
    PushJson {
        id: Uuid,
    },
}

impl RemoteCommand {
    /// Playback-Befehle: alles, was Companion ohne Anmeldung darf. Bearbeiten
    /// und Dateizugriff bleiben der Web-Remote mit PIN vorbehalten.
    pub fn is_playback(&self) -> bool {
        matches!(
            self,
            Self::Go
                | Self::Panic
                | Self::PauseAll
                | Self::ResumeAll
                | Self::Reset
                | Self::GoTo { .. }
                | Self::Arm { .. }
                | Self::PauseCue { .. }
                | Self::ResumeCue { .. }
                | Self::StopCue { .. }
                | Self::SetShowMode { .. }
                | Self::GetStatus
        )
    }

    /// Greift auf Dateien des KuhPlay-Rechners zu: nur mit aktivem PIN.
    pub fn accesses_files(&self) -> bool {
        matches!(self, Self::OpenShow { .. } | Self::SaveShowAs { .. })
    }

    /// `true` für alles, was die Show verändert: Im Show-Mode gesperrt, wie in
    /// der App. Playback und das Umschalten des Show-Mode bleiben möglich.
    pub fn is_edit(&self) -> bool {
        !matches!(
            self,
            Self::Go
                | Self::Panic
                | Self::PauseAll
                | Self::ResumeAll
                | Self::Reset
                | Self::GoTo { .. }
                | Self::Arm { .. }
                | Self::PauseCue { .. }
                | Self::ResumeCue { .. }
                | Self::StopCue { .. }
                | Self::SetShowMode { .. }
                | Self::GetStatus
                | Self::SetActiveCart { .. }
                | Self::SetPlayhead { .. }
                | Self::ReceiveTimecode { .. }
                | Self::MidiNote { .. }
                | Self::OscAddress { .. }
                | Self::RefreshAudioDevices
                | Self::SaveShow
                | Self::SaveShowAs { .. }
                | Self::EvalJs { .. }
                | Self::PushJson { .. }
        )
    }
}

/// Ausgehende Remote-Events (Feedback an Web-Remote / Companion).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteEvent {
    /// Vollständiger Status-Snapshot (initial + auf Anfrage).
    Status(RemoteStatus),
    /// Einzel-Update: Cue gestartet.
    CueStarted {
        id: Uuid,
        number: String,
        name: String,
    },
    /// Einzel-Update: Cue pausiert.
    CuePaused { id: Uuid },
    /// Einzel-Update: Cue fortgesetzt.
    CueResumed { id: Uuid },
    /// Einzel-Update: Cue beendet.
    CueFinished { id: Uuid },
    /// Einzel-Update: Cue gestoppt.
    CueStopped { id: Uuid },
    /// Einzel-Update: Cue-Fehler.
    CueError { id: Uuid, message: String },
    /// Playhead hat sich bewegt.
    PlayheadMoved { cart: usize, index: usize },
    /// Standby-Cue hat sich geändert.
    StandbyChanged { number: String, name: String },
    /// Show-Mode geändert.
    ShowModeChanged { enabled: bool },
    /// Panic/Reset ausgeführt.
    Panicked,
    /// Pong (Antwort auf Ping).
    Pong,
    /// Vollständige Show für die Web-Remote (nur dorthin, nicht OSC/Companion).
    Workspace(Box<RemoteWorkspace>),
    /// Ein Remote-Command wurde abgelehnt oder ist fehlgeschlagen.
    Error { message: String },
}

/// Alles, was die Web-Remote zum Anzeigen und Bearbeiten braucht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkspace {
    pub show_name: String,
    /// Aktuelle Show-Datei (`None` = noch nie gespeichert).
    pub file: Option<String>,
    pub audition_mode: bool,
    /// Ausblendzeit von Panic in Millisekunden.
    pub panic_duration_ms: u64,
    /// `off`, `preview` oder `displays`.
    pub output_mode: String,
    pub audio_output: Option<String>,
    pub audio_patch: AudioPatch,
    pub audio_devices: Vec<RemoteAudioDevice>,
    pub default_audio_device: Option<String>,
    /// Alle Cues des aktiven Carts, vollständig.
    pub cues: Vec<Cue>,
    /// Kaputte Cues: ID → Gründe.
    pub problems: HashMap<Uuid, Vec<String>>,
    pub stages: Vec<Stage>,
    pub routes: Vec<crate::core::OutputRoute>,
    pub devices: Vec<RemoteDevice>,
    /// Erkannte Blackmagic-Ausgabegeräte (Namen), für die Geräteauswahl im
    /// Route-Editor (G5). Leer + hint gesetzt = Treiber da, aber 0 Geräte.
    #[serde(default)]
    pub decklink_devices: Vec<String>,
    /// Meldung für die UI: Treiber fehlt / 0 Geräte (z. B. nach macOS-Update:
    /// Desktop Video neu installieren und registrieren, G8).
    #[serde(default)]
    pub decklink_hint: Option<String>,
    /// Auswahlwerte: Feldname → (JSON-Wert, Anzeigename).
    pub options: BTreeMap<String, Vec<RemoteOption>>,
    pub shortcuts: Vec<RemoteShortcut>,
    pub network: Vec<(String, String)>,
    pub midi: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAudioDevice {
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDevice {
    /// Betriebssystem-ID des Displays.
    pub id: u32,
    pub name: String,
    /// Position in globalen Display-Punkten.
    pub x: i32,
    pub y: i32,
    /// Größe in logischen Punkten.
    pub width: u32,
    pub height: u32,
    /// Retina-Skalierung (Punkte → Pixel).
    pub scale_factor: f32,
    /// Bildwiederholrate (Hz, 0 = unbekannt).
    pub frequency: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteOption {
    pub value: serde_json::Value,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteShortcut {
    pub group: String,
    pub label: String,
    pub keys: String,
}

/// Vollständiger Status-Snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteStatus {
    /// Name der Show.
    pub show_name: String,
    /// Show-Mode aktiv?
    pub show_mode: bool,
    /// Aktiver Cart (Index).
    pub active_cart: usize,
    /// Name des aktiven Carts.
    pub active_cart_name: String,
    /// Playhead-Index im aktiven Cart.
    pub playhead: usize,
    /// Standby-Cue (nächster GO).
    pub standby: Option<StandbyCue>,
    /// Laufende Cues.
    pub running: Vec<RunningCue>,
    /// Alle Carts (Name + Cue-Anzahl).
    pub carts: Vec<CartInfo>,
    /// Alle Cues des aktiven Carts (für Liste in Remote-UI).
    pub cues: Vec<CueInfo>,
}

/// Standby-Cue (nächster GO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandbyCue {
    pub id: Uuid,
    pub number: String,
    pub name: String,
    pub armed: bool,
}

/// Laufender Cue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningCue {
    pub id: Uuid,
    pub number: String,
    pub name: String,
    pub status: String,
    pub elapsed_ms: u64,
    pub duration_ms: u64,
}

/// Cart-Info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartInfo {
    pub index: usize,
    pub name: String,
    pub cue_count: usize,
}

/// Cue-Info (für Remote-Liste).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueInfo {
    pub id: Uuid,
    pub number: String,
    pub name: String,
    pub cue_type: String,
    pub armed: bool,
    pub status: String,
    pub is_running: bool,
    pub duration_ms: u64,
}

impl RemoteStatus {
    /// Erstellt einen Status-Snapshot aus den App-Daten.
    /// Wird von der App bei jedem Status-Request oder Broadcast aufgerufen.
    #[allow(clippy::too_many_arguments)] // Status-Snapshot: alle Felder fachlich nötig
    pub fn from_app(
        show_name: &str,
        show_mode: bool,
        active_cart: usize,
        active_cart_name: &str,
        playhead: usize,
        standby: Option<StandbyCue>,
        running: Vec<RunningCue>,
        carts: Vec<CartInfo>,
        cues: Vec<CueInfo>,
    ) -> Self {
        Self {
            show_name: show_name.to_string(),
            show_mode,
            active_cart,
            active_cart_name: active_cart_name.to_string(),
            playhead,
            standby,
            running,
            carts,
            cues,
        }
    }
}

/// Port-Konfiguration für Remote-Server.
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// Web-Remote HTTP/WebSocket Port (Default: 8080).
    pub web_port: u16,
    /// Companion TCP Port (Default: 9999).
    pub companion_port: u16,
    /// Web-Remote aktivieren.
    pub web_enabled: bool,
    /// Companion aktivieren.
    pub companion_enabled: bool,
    /// OSC aktivieren (UDP-In + Event-Out).
    pub osc_enabled: bool,
    /// OSC UDP-Listen-Port (Default: 9000).
    pub osc_port: u16,
    /// OSC-Out-Ziel (`host`, port); `None` = kein Out.
    pub osc_out: Option<(String, u16)>,
    /// OSC-Adress-Präfix (Default: `/kuhplay`; z. B. `/show`).
    pub osc_prefix: String,
    /// PIN der Web-Remote; `None` = keine Anmeldung.
    pub web_pin: Option<String>,
    /// MIDI aktivieren (Controller-Input, siehe `midi`).
    pub midi_enabled: bool,
    /// MIDI-Input-Device (Namens-Substring); `None` = erstes Device.
    pub midi_device: Option<String>,
    /// MIDI Note-On → Command Mapping (Default: Noten 0–4).
    pub midi_map: MidiNoteMap,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            web_port: 8080,
            companion_port: 9999,
            web_enabled: true,
            companion_enabled: true,
            osc_enabled: true,
            osc_port: 9000,
            osc_out: Some(("127.0.0.1".to_string(), 9001)),
            osc_prefix: "/kuhplay".to_string(),
            web_pin: None,
            midi_enabled: true,
            midi_device: None,
            midi_map: MidiNoteMap::default(),
        }
    }
}

impl RemoteConfig {
    /// Konfiguration aus Umgebungsvariablen:
    /// * `KUHPLAY_REMOTE=off` – Web + Companion deaktiviert
    /// * `KUHPLAY_REMOTE_WEB=off` / `KUHPLAY_REMOTE_COMPANION=off`
    /// * `KUHPLAY_REMOTE_WEB_PORT` / `KUHPLAY_REMOTE_COMPANION_PORT`
    /// * `KUHPLAY_OSC=off`, `KUHPLAY_OSC_PORT`,
    ///   `KUHPLAY_OSC_OUT` = `host:port` oder `off`,
    ///   `KUHPLAY_OSC_PREFIX` = Adress-Präfix (Default `/kuhplay`)
    /// * `KUHPLAY_MIDI=off`, `KUHPLAY_MIDI_DEVICE` = Namens-Substring,
    ///   `KUHPLAY_MIDI_MAP` = `go=36,panic=37,pause=38,resume=39,reset=40`
    /// * `KUHPLAY_REMOTE_PIN` = PIN der Web-Remote (`off` = keine Anmeldung;
    ///   ungesetzt legt `main` einen an, siehe [`web::load_or_create_pin`])
    pub fn from_env() -> Self {
        fn disabled(var: &str) -> bool {
            std::env::var_os(var).is_some_and(|v| v == "0" || v == "off" || v == "false")
        }
        fn port(var: &str, default: u16) -> u16 {
            std::env::var(var)
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(default)
        }
        /// Liefert `None` = Variable unset (→ Default), `Some(None)` = „off",
        /// `Some(Some(..))` = `host:port`.
        fn osc_out(var: &str) -> Option<Option<(String, u16)>> {
            let value = std::env::var(var).ok()?;
            if matches!(value.as_str(), "0" | "off" | "false") {
                return Some(None);
            }
            let (host, port) = value.rsplit_once(':')?;
            Some(Some((host.to_string(), port.parse().ok()?)))
        }
        let web_pin = std::env::var("KUHPLAY_REMOTE_PIN")
            .ok()
            .filter(|pin| !pin.is_empty() && pin != "off");
        let all_off = disabled("KUHPLAY_REMOTE");
        Self {
            web_pin,
            web_port: port("KUHPLAY_REMOTE_WEB_PORT", 8080),
            companion_port: port("KUHPLAY_REMOTE_COMPANION_PORT", 9999),
            web_enabled: !all_off && !disabled("KUHPLAY_REMOTE_WEB"),
            companion_enabled: !all_off && !disabled("KUHPLAY_REMOTE_COMPANION"),
            osc_enabled: !disabled("KUHPLAY_OSC"),
            osc_port: port("KUHPLAY_OSC_PORT", 9000),
            osc_out: osc_out("KUHPLAY_OSC_OUT").unwrap_or(Some(("127.0.0.1".to_string(), 9001))),
            osc_prefix: std::env::var("KUHPLAY_OSC_PREFIX")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "/kuhplay".to_string()),
            midi_enabled: !disabled("KUHPLAY_MIDI"),
            midi_device: std::env::var("KUHPLAY_MIDI_DEVICE")
                .ok()
                .filter(|s| !s.is_empty()),
            midi_map: std::env::var("KUHPLAY_MIDI_MAP")
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| MidiNoteMap::from_map_str(&s))
                .unwrap_or_default(),
        }
    }
}

/// Hilfskonvertierung: Duration → Millisekunden.
pub fn duration_ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_command_serde_roundtrip() {
        let cmds = vec![
            RemoteCommand::Go,
            RemoteCommand::Panic,
            RemoteCommand::PauseAll,
            RemoteCommand::ResumeAll,
            RemoteCommand::Reset,
            RemoteCommand::GoTo {
                cue: "5.1".to_string(),
            },
            RemoteCommand::Arm {
                cue: "5".to_string(),
                armed: true,
            },
            RemoteCommand::SetShowMode { enabled: true },
            RemoteCommand::GetStatus,
        ];
        for cmd in cmds {
            let json = serde_json::to_string(&cmd).unwrap();
            let back: RemoteCommand = serde_json::from_str(&json).unwrap();
            assert_eq!(json, serde_json::to_string(&back).unwrap());
        }
    }

    #[test]
    fn remote_event_serde_roundtrip() {
        let events = vec![
            RemoteEvent::Pong,
            RemoteEvent::Panicked,
            RemoteEvent::CueStarted {
                id: Uuid::new_v4(),
                number: "1".to_string(),
                name: "Intro".to_string(),
            },
            RemoteEvent::PlayheadMoved { cart: 0, index: 5 },
            RemoteEvent::ShowModeChanged { enabled: true },
        ];
        for ev in events {
            let json = serde_json::to_string(&ev).unwrap();
            let back: RemoteEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(json, serde_json::to_string(&back).unwrap());
        }
    }

    #[test]
    fn remote_status_serde() {
        let status = RemoteStatus {
            show_name: "Test Show".to_string(),
            show_mode: false,
            active_cart: 0,
            active_cart_name: "Cart 1".to_string(),
            playhead: 3,
            standby: Some(StandbyCue {
                id: Uuid::new_v4(),
                number: "4".to_string(),
                name: "Next".to_string(),
                armed: true,
            }),
            running: vec![],
            carts: vec![CartInfo {
                index: 0,
                name: "Cart 1".to_string(),
                cue_count: 10,
            }],
            cues: vec![],
        };
        let json = serde_json::to_string_pretty(&status).unwrap();
        let back: RemoteStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.show_name, "Test Show");
        assert_eq!(back.playhead, 3);
        assert!(back.standby.is_some());
    }
}
