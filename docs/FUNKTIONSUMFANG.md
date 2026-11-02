# Funktionsumfang von KuhPlay im Branchenvergleich (Stand 2026-11-02)

Referenz: oeffentlich dokumentierter Funktionsumfang gaengiger Show-Control-Software
(Workspace, Cue-Typen, Inspector-Tabs, Tastenkuerzel). Verglichen gegen den Code (`src/core/cue.rs`,
`src/core/stage.rs`, `src/core/audio.rs`, `src/gui/inspector/*`).

Legende: ✅ vorhanden · ◐ teilweise · ❌ fehlt

## 1. Gemeinsame Cue-Attribute (Basics / Time & Loops / Triggers)

| Attribut (Referenz) | KuhPlay | Feld |
|---|---|---|
| Number / Name | ✅ | `Cue::number`, `Cue::name` |
| Notes | ✅ | `Cue::notes` |
| Color | ✅ | `Cue::color` (Zeilentönung + Randstreifen) |
| Flag (Taste F) | ✅ | `Cue::flagged` |
| Armed | ✅ | `Cue::is_armed` |
| Pre-Wait / Duration / Post-Wait | ✅ | `pre_wait`, `duration`, `post_wait` |
| Auto-Continue / Auto-Follow | ✅ | `ContinueMode` (Abweichung: feuert nach Cue-Ende, nicht sofort – BACKLOG Nr. 1) |
| Infinite Loop / Loop-Count | ✅ | `LoopMode::{Once,Count,Infinite}` |
| Triggers (Play/Pause/Resume/Stop) | ✅ | Inspector-Tab „Triggers" |
| Target-Cue | ◐ | `control_target` (Control), `fade_target` (Fade – Ausbau siehe AUFTRAG-fade) |
| Kill others / Stop previous | ✅ | `kill_others`, `stop_previous` |
| Hold last frame | ✅ | `hold_last_frame` |

## 2. Cue-Typen

| Referenz | KuhPlay | Status |
|---|---|---|
| Audio | `CueType::Audio` | ✅ Datei/Stream, Slices, Loops, Volume, Pan, Kanal-Matrix (`AudioRoute`) |
| Video | `CueType::Video` | ✅ Stage/Layer/Geometry/Opacity/Crop/Hard Edge, Tonspur mitlaufen, Hold last frame |
| Mic / Camera | — | ❌ bewusst weggelassen (kein Live-Input gewünscht, BACKLOG) |
| Text | `CueType::Text` | ✅ rendert als Inline-HTML (CEF) auf Stage |
| Script | `CueType::Script` | ✅ JS in CEF-Seite, `window.kuhplay.done()` |
| Group | `CueType::Group` | ✅ Modi: Simultaneous, Sequential, Timeline, Loop, Shuffle; Kinder selektierbar |
| Fade | `CueType::Fade` | ◐ **Lücke, siehe AUFTRAG-fade.md**: Ziel wählbar, Volume+Opacity gleichzeitig, Stop-when-done, Crossfade-Cue |
| Network | `CueType::Network` | ✅ UDP/TCP, Host:Port, Nachricht |
| MIDI | `CueType::Midi` | ✅ Note On/Off, CC, Program Change |
| OSC | (Empfang) | ✅ OSC-In (Remote), ❌ kein dedizierter OSC-Sende-Cue (Network-Cue via UDP deckt ab) |
| Timecode | `CueType::Timecode` | ✅ LTC/MTC/OSC-Chasing + manuelles GO |
| Light (DMX) | `CueType::Light` | ✅ Art-Net + sACN (Universe, Startkanal, Werte) |
| Wait / Memo | `CueType::Wait` / `Memo` | ✅ |
| HTML5 / HTML-Data | `CueType::Html` / `HtmlData` | ✅ CEF-Offscreen-Rendering, JSON-Push, JS-Bridge |
| Slideshow | `CueType::SlideShow` | ✅ Bilderliste, Hold, Crossfade, Auto-Advance, Next/Prev |
| Companion | `CueType::Companion` | ✅ Button press/down/up über HTTP-API |
| Control (Start/Stop/Pause/Load/Reset/Devamp/GoTo/Arm/Disarm/Next/Prev) | `CueType::Control` | ✅ `ControlAction` komplett |

## 3. Video-Geometrie / Attribute (Referenz „Geometry“/„Filters & Effects“)

| Referenz | KuhPlay | Status |
|---|---|---|
| Stage, Layer | ✅ | `Cue::stage`, `Cue::layer` |
| Position/Größe (x/y/w/h), Aspect-Lock, Fit-Modi | ✅ | `SurfaceGeometry` (x,y,width,height, `GeometryMode`, `lock_aspect`) |
| Opacity | ✅ | `SurfaceGeometry::opacity` + Fade |
| Crop / Area of Interest / Mask | ◐ | Crop ja (`area_of_interest`), ❌ keine freie Maske |
| Hard Edge / Soft Edge | ✅ | `hard_edge` (Cue), Stage-Edge + Region-Blend |
| Rotation | ❌ fehlt | – |
| Blend Mode (Normal/Multiply/Screen/…) | ❌ fehlt | – |
| Video-Filter/Effekte (Helligkeit, Kontrast, Fades als Effekt) | ❌ fehlt | – |
| Decklink-Key/Fill | ✅ | Key/Fill als Regionen, 2-Kabel + nativ |

## 4. Audio-Attribute (Referenz „Levels“)

| Referenz | KuhPlay | Status |
|---|---|---|
| Volume pro Cue | ✅ | `Cue::volume`, Fade |
| Pan | ✅ | `AudioRoute::pan` |
| Kanal-Routing / Matrix | ✅ | `AudioRoute::{channels, first_channel, matrix}` + Patch-Matrix in Settings |
| Slices (In/Out, Play-Count) | ✅ | `Cue::slices` (MediaSlice) |
| Loops | ✅ | `LoopMode` |
| Fade-Kurven linear/dB | ✅ | `FadeCurve` |

## 5. Bekannte Lücken (Priorität, Detail-Recherche lokal in `docs-lokal/`, git-ignored)

1. **Fade/Crossfade wie in der Referenz** – kompletter Auftrag in `docs/AUFTRAG-fade.md` (offen): Fade-Ziel wählbar, Volume+Opacity gleichzeitig, Stop-when-done, eigene Kurven, relative Fades, Crossfade-Cue mit EqualPower, Panic-Regel, Migration alter Shows.
2. **Inspector-Tabs der Referenz pro Cue-Typ** – Referenz kennt u. a. „Levels" (Volume/Pan/Routing + Dog-Ear-Anzeige), „Objects" (Video-Parameter faden), „Curve" (freie Kurven), „Audio FX" (EQ/Kompressor), „Time & Loops" mit Slices (In/Out + Play-Count pro Slice) und Loop je Slice. KuhPlay hat Slices/Loops/Levels, aber ❌ freie Kurven, ❌ Audio-Effekte, ❌ „Fade per Rate".
3. Rotation ❌, Blend-Modes ❌ (nur Alpha), Video-Effekte ❌, freie Maske ❌ (nur rechteckiger Crop).
4. OSC-Sende-Cue ❌ (über Network-Cue UDP möglich), MIDI-File-Wiedergabe ❌, MSC ❌.
5. Mic/Camera ❌ (bewusste Produktentscheidung, kein Live-Input).
6. Referenz-Triggers: Hotkey/OSC/MIDI/Timecode direkt am Cue, kombinierbar, unabhängig vom Playhead – KuhPlay: Triggers-Tab ✅, Timecode ✅, Hotkey pro Cue ◐ (globale Shortcuts ja, pro-Cue-Hotkey ❌).

## 6. Tastenkürzel

Zentrale Tabelle `src/gui/shortcuts.rs`; Referenz-Belegung weitgehend
übernommen (F2.1–F2.7 in `docs/TODO-review-2026-10-31.md`, erledigt).
Offen: F1.9 Sichtprüfung (Termin nötig).
