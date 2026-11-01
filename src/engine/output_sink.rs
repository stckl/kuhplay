//! Schnittstelle fuer Ausgabe-Senken: fertige Bilder werden an ein Geraet
//! gepusht, ohne die Engine an eine konkrete Hardware zu binden.
//! Implementiert von [`DecklinkSink`](super::decklink::DecklinkSink)
//! (native Blackmagic-SDK-Ausgabe, siehe decklink_native.rs).

use crate::core::ShowResult;

use super::frame::VideoFrame;

/// Senke fuer fertige Ausgabe-Bilder (z. B. Decklink-Hardware).
pub trait OutputSink {
    /// Pusht ein Frame an das Geraet. Ein Fehler deaktiviert die Senke beim
    /// Aufrufer (Live-Betrieb: kein Panic, Ausgabe wird stillgelegt).
    fn push(&mut self, frame: &VideoFrame) -> ShowResult<()>;
}

impl OutputSink for super::decklink::DecklinkSink {
    fn push(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        self.push_frame(frame)
    }
}
