//! Fehler-Typen des Domain-Layers (via `thiserror`, keine `panic!`s).

use thiserror::Error;

/// Zentrale Fehlerart der Anwendung.
#[derive(Debug, Error)]
pub enum ShowError {
    /// Cue mit der angegebenen ID/Nummer wurde nicht gefunden.
    #[error("cue not found: {0}")]
    CueNotFound(String),

    /// Ungültiger Zustandsübergang (z. B. Pause eines Standby-Cues).
    #[error("invalid cue state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },

    /// Fehler der Playback-Engine bzw. der Engine-Kommunikation.
    #[error("engine error: {0}")]
    Engine(String),

    /// Ungültiger Parameter (z. B. Slice-, Routing-, Stage- oder Cart-Validierung).
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    /// I/O-Fehler (Persistenz, Medien-Dateien).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result-Alias für den Domain-Layer.
pub type ShowResult<T> = Result<T, ShowError>;
