//! HTML5-Cue-Quelle: Woher eine WebView ihren Inhalt lädt.
//!
//! Eine produktive Engine (z. B. `wry`-basiert) rendert die Quelle in einer
//! WebView auf einer Video-Stage. Die Mock-Engine verwaltet die Instanzen
//! virtuell und protokolliert JS-Trigger/JSON-Pushes mit exakten Zeitpunkten.
//!
//! ## Erweiterung
//!
//! * **Caching:** `Url`-Quellen lokal cachen (Offline-Shows ohne Netz).
//! * **Dev-Server:** `Url` auf `http://localhost:*` mit Live-Reload.
//! * **CSP/Permissions:** Quelle mit Content-Security-Policy-Headern kapseln.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::error::{ShowError, ShowResult};

/// Quelle eines HTML5-Cues ([`crate::core::CueType::Html`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HtmlSource {
    /// Lokale HTML/JS/CSS-Datei (Pfad relativ zur Show oder absolut).
    File(PathBuf),
    /// URL (http/https) – wird von der WebView geladen.
    Url(String),
    /// Inline-HTML (kein Dateizugriff; ideal für Lower-Thirds/Test-Cues).
    Inline(String),
}

impl HtmlSource {
    /// Alle Quellen-Arten (für UI-ComboBoxen).
    pub const ALL_KINDS: [HtmlSourceKind; 3] = [
        HtmlSourceKind::File,
        HtmlSourceKind::Url,
        HtmlSourceKind::Inline,
    ];

    /// Erzeugt eine Datei-Quelle.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        HtmlSource::File(path.into())
    }

    /// Erzeugt eine URL-Quelle.
    pub fn url(url: impl Into<String>) -> Self {
        HtmlSource::Url(url.into())
    }

    /// Erzeugt eine Inline-HTML-Quelle.
    pub fn inline(html: impl Into<String>) -> Self {
        HtmlSource::Inline(html.into())
    }

    /// Art der Quelle (für UI-ComboBoxen).
    pub fn kind(&self) -> HtmlSourceKind {
        match self {
            HtmlSource::File(_) => HtmlSourceKind::File,
            HtmlSource::Url(_) => HtmlSourceKind::Url,
            HtmlSource::Inline(_) => HtmlSourceKind::Inline,
        }
    }

    /// Anzeige-Text (Pfad/URL/HTML-Kurzvorschau).
    pub fn describe(&self) -> String {
        match self {
            HtmlSource::File(path) => path.display().to_string(),
            HtmlSource::Url(url) => url.clone(),
            HtmlSource::Inline(html) => {
                let flat: String = html.chars().filter(|c| !c.is_whitespace()).collect();
                flat.chars().take(60).collect()
            }
        }
    }

    /// Vollständiger Text zum Editieren (Pfad/URL/gesamtes Inline-HTML –
    /// im Gegensatz zu [`HtmlSource::describe`] ohne Kürzung).
    pub fn edit_text(&self) -> String {
        match self {
            HtmlSource::File(path) => path.display().to_string(),
            HtmlSource::Url(url) => url.clone(),
            HtmlSource::Inline(html) => html.clone(),
        }
    }

    /// Ersetzt den Inhalt (Pfad/URL/HTML) gemäß der aktuellen Quellen-Art.
    pub fn set_edit_text(&mut self, text: String) {
        match self {
            HtmlSource::File(path) => *path = PathBuf::from(text),
            HtmlSource::Url(url) => *url = text,
            HtmlSource::Inline(html) => *html = text,
        }
    }

    /// Prüft, ob die Quelle grundsätzlich gültig ist (Existenz von Dateien
    /// wird *engine-seitig* geprüft, damit Tests ohne Dateisystem laufen).
    pub fn validate(&self) -> ShowResult<()> {
        match self {
            HtmlSource::File(path) => {
                if path.as_os_str().is_empty() {
                    return Err(ShowError::InvalidParameter(
                        "html file source: empty path".to_string(),
                    ));
                }
                Ok(())
            }
            HtmlSource::Url(url) => {
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(ShowError::InvalidParameter(format!(
                        "invalid html url (http/https required): {url}"
                    )));
                }
                Ok(())
            }
            HtmlSource::Inline(html) => {
                if html.trim().is_empty() {
                    return Err(ShowError::InvalidParameter(
                        "inline html source: empty document".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Art einer [`HtmlSource`] (unabhängig vom Inhalt, für UI-ComboBoxen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlSourceKind {
    /// Lokale Datei.
    File,
    /// URL.
    Url,
    /// Inline-HTML.
    Inline,
}

impl HtmlSourceKind {
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            HtmlSourceKind::File => "File",
            HtmlSourceKind::Url => "URL",
            HtmlSourceKind::Inline => "Inline HTML",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_file_url_and_inline() {
        assert!(HtmlSource::file("/show/lower-third.html")
            .validate()
            .is_ok());
        assert!(HtmlSource::url("https://example.com/show.html")
            .validate()
            .is_ok());
        assert!(HtmlSource::url("http://localhost:8080/live")
            .validate()
            .is_ok());
        assert!(HtmlSource::inline("<h1>Hello</h1>").validate().is_ok());
    }

    #[test]
    fn validate_rejects_bad_sources() {
        assert!(HtmlSource::file("").validate().is_err());
        assert!(HtmlSource::url("ftp://example.com/x").validate().is_err());
        assert!(HtmlSource::url("example.com").validate().is_err());
        assert!(HtmlSource::inline("   ").validate().is_err());
    }

    #[test]
    fn describe_truncates_inline_html() {
        let long = "<html>\n  <body>very long document</body>\n</html>";
        let described = HtmlSource::inline(long).describe();
        assert!(described.len() <= 60);
        assert!(described.starts_with("<html><body>"));
    }

    #[test]
    fn serde_roundtrip_for_all_kinds() {
        for source in [
            HtmlSource::file("/a/b.html"),
            HtmlSource::url("https://x.test/"),
            HtmlSource::inline("<p>hi</p>"),
        ] {
            let json = serde_json::to_string(&source).expect("serialize");
            let back: HtmlSource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(source, back);
        }
    }
}
