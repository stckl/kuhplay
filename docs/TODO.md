# TODO – was noch offen ist (Stand 2026-11-02)

Zentrale Liste. Details: `docs/TODO-offen.md` ( technischer Übergabe-Stand),
`docs/BACKLOG.md` (Wunschliste ohne Zusage), `docs/FUNKTIONSUMFANG.md` (Lücken
vs. Referenz), `docs/AUFTRAG-fade.md` (Fade – umgesetzt).

## P0 – muss auf dem echten Mac/ZBOOK laufen (braucht Termin/Hardware)

- [ ] **F1.9 Sichtprüfung** am echten Bildschirm (Vergleich Look & Feel) – Termin absprechen.
- [ ] **CEF-Abnahme Windows/Linux:** Mac fertig (Bundle, 60 fps 1080p/4K, Alpha).
      Windows-Paket aus dem Release-Workflow auf dem ZBOOK testen; Installer
      (DMG/NSIS/deb) von Hand prüfen. Doku: `docs/TODO-offen.md`.
- [ ] **DeckLink-Hardware-Keying auf einem Port** (framesync in der Karte) – ZBOOK-Test.

## P1 – Produktlücken (aus Funktionsumfang-Vergleich)

- [ ] Rotation, Blend-Modes, Video-Effekte, freie Maske (nur rechteckiger Crop)
- [ ] OSC-Sende-Cue, MIDI-File-Wiedergabe, MSC
- [ ] Freie Fade-Kurven (Curve-Editor), Audio-FX (EQ/Kompressor), „Fade per Rate"
- [ ] Pro-Cue-Hotkey direkt editierbar (Hotkey-Trigger gehen, Key-Name per Textfeld)

## P2 – Backlog-Highlights (aus docs/BACKLOG.md)

- [ ] Launcher-Fenster (Cue per Nummer/Name suchen+starten), Override Controls
- [ ] Renumber, File-Targets umschreiben, Keymap frei belegen, App-Settings,
      Network/MIDI in Settings statt Env, Theme-Auswahl, Show-Backups
- [ ] Audio Map Editor, Object Audio; Licht-Pult/LCC/Patch; DMX-Steuerung vom Pult
- [ ] Mic/Camera-Cues: bewusst zurückgestellt (kein Live-Input)

## P3 – Technik/Sonstiges

- [x] Real-World-Tests (`tests/realworld.rs`): echte Medien via ffmpeg/hound durch
      die Real-Engine – Out-Point endet früh, Opacity-Fade am echten Stage-Canvas
      (Mitte ist Mix, nie schwarz), Hold-at-end hält letztes Frame; Audio-Fade
      opt-in via `KUHPLAY_REAL_AUDIO=1` (braucht Audio-Device). `KUHPLAY_AUDIO=off`
      schaltet das Audio-Backend ab (Video-only, z. B. Sandbox/CI ohne Soundkarte).

- [ ] Headless-Modus (`KUHPLAY_HEADLESS=1`, Remote ohne eframe-Fenster)
- [ ] GPU-Compositing (wgpu) statt CPU-RGBA-Roundtrip
- [ ] Bekannte Flakes: `real_engine`-Timing-Tests in Parallel-Läufen (pre-existing,
      dokumentiert in `docs/TODO-offen.md`)

## Erledigt (nicht mehr listen)

Video-Routing komplett (altes `docs/TODO-video-routing.md` → `docs/archiv/`),
Review-Blöcke A–U + W + M, Fade-Cue (Crossfade entfernt), Cue-Trigger,
Time&Loops In/Out + Hold-at-end, 60-fps-Rendering, CEF als einziger Renderer.
