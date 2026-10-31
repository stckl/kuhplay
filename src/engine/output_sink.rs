//! Schnittstelle fuer Ausgabe-Senken (Phase 7 des Video-Routing-Auftrags):
//! fertige Bilder werden an ein Geraet gepusht, ohne die Engine an eine
//! konkrete Hardware zu binden. Implementiert von
//! [`DecklinkOutput`](super::decklink::DecklinkOutput) (ffmpeg-Subprocess);
//! die eigentliche Blackmagic-Anbindung ist ein eigener Auftrag – das
//! Geraet wird hier nicht angesprochen.

use crate::core::ShowResult;

use super::frame::VideoFrame;

/// Senke fuer fertige Ausgabe-Bilder (z. B. Decklink-Hardware).
pub trait OutputSink {
    /// Pusht ein Frame an das Geraet. Ein Fehler deaktiviert die Senke beim
    /// Aufrufer (Live-Betrieb: kein Panic, Ausgabe wird stillgelegt).
    fn push(&mut self, frame: &VideoFrame) -> ShowResult<()>;
}

impl OutputSink for super::decklink::DecklinkOutput {
    fn push(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        self.push_frame(frame)
    }
}

impl OutputSink for super::decklink::DecklinkSink {
    fn push(&mut self, frame: &VideoFrame) -> ShowResult<()> {
        self.push_frame(frame)
    }
}
