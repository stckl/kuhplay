# KuhPlay

**Show-Control- und Cue-Playback-Software in Rust, nach dem Vorbild von QLab.**
Audio, Video, HTML5 und Diashows laufen als Cues in einer Liste und werden mit
GO abgefahren, vom Theater bis zur Eventtechnik. Die Software ist ausgelegt für
macOS, Windows und Linux und lässt sich komplett aus dem Browser bedienen.

> **Status: in Entwicklung.** Entwickelt und getestet wird auf macOS (Apple
> Silicon). Für Windows ist bisher nur `cargo check` gelaufen, für Linux nur das
> Build-Skript auf Syntax geprüft. Die Vollbild-Ausgabe über mehrere Displays ist
> noch nicht an echter Hardware erprobt.

## Was KuhPlay kann

| Bereich | Funktionen |
|---|---|
| **Cues** | Audio, Video, HTML5, Diashow, Fade, Group, Stop, Control, HTML-Data; Pre-Wait, Duration, Post-Wait, Auto-Continue und Auto-Follow |
| **Ablauf** | Playhead und GO, mehrere Cue-Listen (Carts), Gruppen (Simultaneous, Sequential, Timeline, Loop, Shuffle), Timecode-Chasing (LTC/MTC) |
| **Audio** | Dekodierung mit symphonia, Ausgabe mit cpal, Geräteauswahl, Fades (Volume, Rate, Opacity; linear oder dB), Slices mit Play-Count |
| **Audio-Routing** | Matrix pro Cue: Datei-Kanal → 16 benannte Kanäle → Geräte-Ausgänge; Umlegen wirkt live |
| **Video** | Playout über ffmpeg, Stages mit Layern, Surfaces, Area of Interest, Hard Edge, Screen-Editor mit Testbildern (auch LED-Wand) |
| **Ausgabe** | Vollbild auf Displays oder Vorschau-Fenster pro Screen, ideal für die Arbeit am Laptop |
| **HTML5** | Eigene WebView (Feature `webview`), JavaScript-Trigger und JSON-Push für Lower-Thirds, Spielstände, Countdowns |
| **Fernsteuerung** | Web-Remote, Bitfocus Companion, OSC, MIDI |
| **Sicherheit im Betrieb** | Show-Mode sperrt alle Bearbeitungen, Panic, Erkennung kaputter Cues |

## Schnellstart

Voraussetzungen: [Rust](https://rustup.rs) und für Video-Cues `ffmpeg` samt
`ffprobe` im `PATH`. Unter Linux zusätzlich die Bibliotheken aus
`scripts/build-linux.sh`.

```sh
git clone https://github.com/stckl/kuhplay.git
cd kuhplay
cargo run --release
```

Beim ersten Start öffnet sich eine Demo-Show. Ihre Medien liegen nicht bei,
Audio- und Video-Cues sind dort deshalb als kaputt markiert. Eigene Dateien
zieht man einfach aus dem Finder ins Fenster.

Wichtige Tasten: **Space** GO · **Esc** Panic · **↑/↓** Playhead ·
**⌘I / ⌘L / ⌘K** Inspector, Sidebar, Toolbox. Alle Kürzel stehen unter
*Help → Keyboard Shortcuts*.

## Web-Remote

Die Oberfläche läuft im Browser unter `http://<rechner>:8080`, auch am Handy:
Cue-Liste, Inspector mit allen Tabs, Audio-Routing, Screens und Stages.

Nur in der App gibt es Show öffnen, „Speichern unter", Datei-Dialoge (Pfade
werden im Browser eingetippt), Waveforms und die Screen-Karte zum Ziehen.

> **Achtung: Die Web-Remote hat keine Anmeldung.** Wer Port 8080 im Netz
> erreicht, kann die Show bearbeiten und abfahren. Companion, OSC und MIDI lösen
> ebenfalls ohne Anmeldung Aktionen aus. KuhPlay gehört deshalb in ein
> vertrauenswürdiges Show-Netz. Abschalten mit `KUHPLAY_REMOTE=off` und
> `KUHPLAY_OSC=off`.

## Konfiguration

Alles läuft über Umgebungsvariablen beim Start.

| Variable | Bedeutung | Standard |
|---|---|---|
| `KUHPLAY_ENGINE` | `mock` = virtuelle Engine ohne Hardware | echte Engine |
| `KUHPLAY_REMOTE` | `off` = Web-Remote und Companion aus | an |
| `KUHPLAY_REMOTE_WEB`, `KUHPLAY_REMOTE_COMPANION` | `off` schaltet einzeln ab | an |
| `KUHPLAY_REMOTE_WEB_PORT`, `KUHPLAY_REMOTE_COMPANION_PORT` | Ports | 8080, 9999 |
| `KUHPLAY_OSC`, `KUHPLAY_OSC_PORT` | OSC an/aus, Eingangsport | an, 9000 |
| `KUHPLAY_OSC_OUT`, `KUHPLAY_OSC_PREFIX` | Ziel `host:port` oder `off`, Adress-Präfix | `127.0.0.1:9001`, `/kuhplay` |
| `KUHPLAY_MIDI`, `KUHPLAY_MIDI_DEVICE` | MIDI an/aus, Gerätename (Teilstring) | an, erstes Gerät |
| `KUHPLAY_MIDI_MAP` | Noten, z. B. `go=36,panic=37,pause=38,resume=39,reset=40` | Noten 0–4 |
| `KUHPLAY_DECKLINK_DEVICE`, `_STAGE`, `_WIDTH`, `_HEIGHT`, `_FPS`, `_PIXFMT` | Decklink-Ausgabe über ffmpeg | aus |
| `KUHPLAY_WEBVIEW_BIN` | Pfad zum WebView-Prozess | neben dem Programm |

## Release bauen

Jedes Skript baut auf seiner eigenen Plattform nach `dist/`.

```sh
./scripts/build-mac.sh        # KuhPlay.app und Zip
./scripts/build-linux.sh      # tar.gz
.\scripts\build-windows.ps1   # Zip
```

Mit `--no-webview` (Windows: `-NoWebview`) entfällt die HTML5-WebView. Die
macOS-App ist nur ad hoc signiert: Auf fremden Macs braucht sie eine Developer-ID
und Notarisierung, sonst blockiert Gatekeeper den Start.

## Aufbau

```text
src/
├── core/      Domain ohne GUI: Cues, Cue-Listen, Stages, Screens, Routing, Show-Datei
├── engine/    Playout im eigenen Thread: Audio, Video, Compositor, WebView, Decklink
├── gui/       Oberfläche mit egui: Workspace, Inspector, Screen-Editor, macOS-Menü
├── remote/    Web-Remote (axum), Companion, OSC, MIDI
└── bin/       WebView-Prozess für HTML5-Cues
companion/     Modul für Bitfocus Companion
scripts/       Build-Skripte
tests/         Integrationstests
docs/          UI-Richtlinie und QLab-Abgleich
```

GUI und Remote schicken Befehle über Kanäle an den Engine-Thread und bekommen
Ereignisse zurück. Bedienung und Oberfläche blockieren das Playout dadurch nicht.
Das `MediaEngine`-Trait trennt die echte Engine von einer virtuellen, mit der die
Timing-Tests millisekundengenau und ohne Hardware laufen.

## Entwicklung

```sh
cargo test                  # rund 490 Tests, auch die GUI läuft headless
cargo clippy --all-targets
KUHPLAY_ENGINE=mock cargo run
```

Die MIDI-Tests überspringen sich mit `SKIP`, wenn CoreMIDI in diesem Prozess
keinen Client zulässt. Das passiert gelegentlich und ist kein Fehler im Code.

Weiterführend:

- [`docs/UI.md`](docs/UI.md): verbindliche UI-Richtlinie
- [`docs/qlab5-parity.md`](docs/qlab5-parity.md): Stand gegenüber QLab 5
- [`ROADMAP.md`](ROADMAP.md) und [`TODO.md`](TODO.md): Verlauf und Planung

## Lizenz

[MIT](LICENSE). Die Phosphor-Icons stehen ebenfalls unter MIT, siehe
[`assets/fonts/phosphor`](assets/fonts/phosphor). KuhPlay ist ein eigenständiges
Projekt und steht in keiner Verbindung zu Figure 53 oder QLab.
