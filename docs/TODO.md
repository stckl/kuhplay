# TODO – was noch offen ist (Stand 2026-11-02)

Zentrale Liste. Details: `docs/TODO-offen.md` ( technischer Übergabe-Stand),
`docs/BACKLOG.md` (Wunschliste ohne Zusage), `docs/FUNKTIONSUMFANG.md` (Lücken
vs. Referenz), `docs/AUFTRAG-fade.md` (Fade – umgesetzt).

## P0 – muss auf dem echten Mac/ZBOOK laufen (braucht Termin/Hardware)

- [ ] **F1.9 Sichtprüfung** am echten Bildschirm (Vergleich Look & Feel) – Termin absprechen.
- [ ] **CEF-Live-Abnahme:** volle CEF-Distribution ins .app-Bundle (~300 MB), 60 fps
      1080p/4K mit Alpha messen auf Mac und ZBOOK; Windows/Linux-CEF-Lauf auf dem
      ZBOOK bauen/testen (MSVC da); CI-Job mit CEF. Doku: `docs/TODO-offen.md`.
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

- [ ] Headless-Modus (`KUHPLAY_HEADLESS=1`, Remote ohne eframe-Fenster)
- [ ] GPU-Compositing (wgpu) statt CPU-RGBA-Roundtrip
- [ ] Bekannte Flakes: `real_engine`-Timing-Tests in Parallel-Läufen (pre-existing,
      dokumentiert in `docs/TODO-offen.md`)

## Erledigt (nicht mehr listen)

Video-Routing komplett (altes `docs/TODO-video-routing.md` → `docs/archiv/`),
Review-Blöcke A–U + W + M, Fade-Cue (Crossfade entfernt), Cue-Trigger,
Time&Loops In/Out + Hold-at-end, 60-fps-Rendering, CEF als einziger Renderer.
