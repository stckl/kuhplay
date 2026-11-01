# Lizenzen von KuhPlay und mitgelieferten Komponenten

## KuhPlay selbst
- KuhPlay (dieses Projekt): **MIT License** – siehe `LICENSE`.
- GUI-Framework **egui / eframe** und Abhängigkeiten im Crate-Tree: **MIT** (bzw. Apache-2.0, dual lizenziert).
- Icons: **Phosphor Icons** – MIT License, <https://phosphoricons.com>.

## Mitgelieferte Binärdateien (im Paket/Installer)
- **ffmpeg / ffprobe**: GNU **LGPL v2.1 oder später**, Build ohne GPL- und
  Nonfree-Komponenten (kein `--enable-gpl`, kein x264/x265/xvid). Quelle und
  Build-Konfiguration siehe `LICENSE-ffmpeg.txt` und `BUILD-ffmpeg.txt` im
  Paketverzeichnis. Lizenztext: <https://www.gnu.org/licenses/lgpl-2.1.html>.
  Quellen: <https://github.com/FFmpeg/FFmpeg>.

## Blackmagic Desktop Video (DeckLink-Treiber)
- Wird **nicht mitgeliefert** (Weitergabe nicht geklärt). KuhPlay prüft beim
  Start, ob der Treiber fehlt oder älter als 16.0 ist, und verlinkt dann den
  Download: <https://www.blackmagicdesign.com/de/support/download>.
- Das Blackmagic SDK 16.0 liegt nur als Include-Header unter
  `third_party/` (Lizenz siehe Blackmagic SDK, nicht im Paket enthalten).

## WebView
- `kuhplay-webview` nutzt **wry/tao** (MIT/Apache-2.0). Auf Windows wird
  WebView2 verwendet (Microsoft-Lizenz, Runtime-Komponente des Systems).
