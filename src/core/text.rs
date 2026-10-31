//! Text-Cue-Inhalt (QLab 5 „Text Cue"): reiner Text mit Schriftgröße,
//! Farbe, Hintergrund und Ausrichtung, der auf einer Video-Stage gerendert
//! wird.
//!
//! Die Engine bekommt den Text nicht direkt, sondern als **Inline-HTML**
//! ([`TextContent::to_html`]) über den bestehenden HTML5-Pipeline
//! ([`crate::engine::EngineCommand::PlayHtml`]) – WebView rendert, Snapshot
//! landet im Stage-Compositor. Damit braucht der Text-Cue keine eigene
//! Render-Engine.

use serde::{Deserialize, Serialize};

/// Horizontale Textausrichtung (QLab Text-Cue „Alignment").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [TextAlign; 3] = [TextAlign::Left, TextAlign::Center, TextAlign::Right];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            TextAlign::Left => "Left",
            TextAlign::Center => "Center",
            TextAlign::Right => "Right",
        }
    }
    /// CSS-Wert für `text-align`.
    fn css(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }
}

/// Vertikale Textausrichtung (QLab Text-Cue „Vertical Alignment").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextVAlign {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl TextVAlign {
    /// Alle Varianten (UI-ComboBoxen).
    pub const ALL: [TextVAlign; 3] = [TextVAlign::Top, TextVAlign::Middle, TextVAlign::Bottom];
    /// Anzeige-Label.
    pub fn label(self) -> &'static str {
        match self {
            TextVAlign::Top => "Top",
            TextVAlign::Middle => "Middle",
            TextVAlign::Bottom => "Bottom",
        }
    }
    /// CSS-Wert für `justify-content` (Flexbox-Spalte).
    fn css(self) -> &'static str {
        match self {
            TextVAlign::Top => "flex-start",
            TextVAlign::Middle => "center",
            TextVAlign::Bottom => "flex-end",
        }
    }
}

/// Inhalt eines Text-Cues ([`crate::core::CueType::Text`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextContent {
    /// Anzuzeigender Text (Zeilenumbrüche erlaubt, `white-space: pre-wrap`).
    #[serde(default)]
    pub text: String,
    /// Schriftgröße in px (Stage-Auflösung = HTML-Viewport).
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Textfarbe (RGB).
    #[serde(default = "default_text_color")]
    pub color: [u8; 3],
    /// Hintergrundfarbe (RGB); `None` = transparent.
    #[serde(default)]
    pub bg_color: Option<[u8; 3]>,
    /// Horizontale Ausrichtung.
    #[serde(default)]
    pub align: TextAlign,
    /// Vertikale Ausrichtung.
    #[serde(default)]
    pub valign: TextVAlign,
}

fn default_font_size() -> f32 {
    64.0
}

fn default_text_color() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for TextContent {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_size: default_font_size(),
            color: default_text_color(),
            bg_color: None,
            align: TextAlign::default(),
            valign: TextVAlign::default(),
        }
    }
}

impl TextContent {
    /// Erzeugt ein komplettes HTML-Dokument, das den Text rendert.
    ///
    /// Der Text wird HTML-escaped; das Dokument füllt den Viewport
    /// (Stage-Canvas) und zentriert den Text per Flexbox.
    pub fn to_html(&self) -> String {
        let fg = format!("rgb({},{},{})", self.color[0], self.color[1], self.color[2]);
        let bg = match self.bg_color {
            Some(c) => format!("rgb({},{},{})", c[0], c[1], c[2]),
            None => "transparent".to_string(),
        };
        format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>\
             html,body{{margin:0;padding:0;width:100%;height:100%;background:{bg};}}\
             body{{display:flex;flex-direction:column;justify-content:{valign};\
             align-items:{align_flex};}}\
             #t{{width:100%;box-sizing:border-box;padding:2%;\
             color:{fg};font-size:{size}px;line-height:1.2;text-align:{align};\
             white-space:pre-wrap;word-wrap:break-word;\
             font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;}}\
             </style></head><body><div id=\"t\">{text}</div></body></html>",
            bg = bg,
            valign = self.valign.css(),
            align_flex = match self.align {
                TextAlign::Left => "flex-start",
                TextAlign::Center => "center",
                TextAlign::Right => "flex-end",
            },
            fg = fg,
            size = self.font_size,
            align = self.align.css(),
            text = escape_html(&self.text),
        )
    }
}

/// Escaped die fünf HTML-Sonderzeichen.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_html_escapes_text_and_applies_style() {
        let t = TextContent {
            text: "<b>fett</b> & \"Anführungszeichen\"".to_string(),
            font_size: 120.0,
            color: [255, 0, 0],
            bg_color: Some([0, 0, 0]),
            align: TextAlign::Center,
            valign: TextVAlign::Middle,
        };
        let html = t.to_html();
        assert!(html.contains("&lt;b&gt;fett&lt;/b&gt; &amp; &quot;Anführungszeichen&quot;"));
        assert!(
            !html.contains("<b>fett</b>"),
            "Roh-HTML darf nicht durchgereicht werden"
        );
        assert!(html.contains("font-size:120px"));
        assert!(html.contains("color:rgb(255,0,0)"));
        assert!(html.contains("background:rgb(0,0,0)"));
        assert!(html.contains("text-align:center"));
        assert!(html.contains("justify-content:center"));
    }

    #[test]
    fn to_html_transparent_background_by_default() {
        let t = TextContent::default();
        assert!(t.to_html().contains("background:transparent"));
    }

    #[test]
    fn serde_roundtrip_and_legacy_default() {
        let t = TextContent {
            text: "Hallo".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: TextContent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // Legacy: nur `text` vorhanden → Rest defaultet.
        let back: TextContent = serde_json::from_str("{\"text\":\"Hi\"}").unwrap();
        assert_eq!(back.text, "Hi");
        assert_eq!(back.font_size, default_font_size());
        assert_eq!(back.align, TextAlign::Left);
    }
}
