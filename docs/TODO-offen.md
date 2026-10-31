# Übergabe: Offene TODOs nach dem Video-Routing-Umbau

**Stand:** Branch `feature/video-routing` (kein Push!), 541 Tests grün (Baseline vor
dem Umbau: 503), Clippy ohne Warnung, `cargo check --target x86_64-pc-windows-msvc
--features webview` OK. Arbeitsbaum sauber, alles committet (HEAD `c42d405`).

Der ursprüngliche Arbeitsauftrag `docs/TODO-video-routing.md` (QLab-5-Video-Routing:
Stage → Region → Output Route → Device) ist **komplett umgesetzt und getestet** —
Phasen 1–8, Abschlussbericht siehe Git-History (Commits ab `45c5e8c`). Diese Datei
listet alles, was danach **offen** ist, in abarbeitbarer Reihenfolge. Jede Aufgabe
endet mit grünem `cargo test`, Clippy ohne Warnung, Windows-Check und eigenem Commit.

```sh
# Prüfliste pro Aufgabe (Abschnitt 8 des alten Auftrags, immer noch gültig):
cargo fmt
cargo test --no-fail-fast          # Testzahl mit dem Stand davor vergleichen (z.Zt. 541)
cargo clippy --all-targets         # keine Warnungen
cargo check --target x86_64-pc-windows-msvc --features webview
```

Stolperfallen (aus `docs/TODO-video-routing.md` §7, gelten weiter): macOS kennt kein
`timeout` → `perl -e 'alarm 240; exec @ARGV' cargo test`. Testzahl prüfen, nicht nur
„failed: 0“ — eine abgestürzte Testdatei fehlt in der Summe. `detect_displays` nur
über die vorhandene Mutex-Funktion (AppKit-SIGSEGV). `python3 - <<EOF`-Heredocs sind
in dieser Umgebung blockiert — Skripte nach `/tmp/xxx.py` schreiben, dann
`python3 /tmp/xxx.py` ausführen. Farbtöne mit Transparenz nur über `gamma_multiply`.
Sonderzeichen (→ ⌘ …) fehlen im UI-Font, umformulieren oder Phosphor-Icon.

---

## Architektur-Kurzreferenz (wichtig für alle Aufgaben)

| Baustein | Ort | Inhalt |
|---|---|---|
| Datenmodell | `src/core/video_routing.rs` | `OutputRoute`, `Region`, `EdgeBlend`, `RouteManager`, `DeviceRef` (Display/Blackmagic), `DeviceArea` (Full/Part), `RouteScaling`, `DisplayRefInfo`, `default_routes_for_displays`, Migration |
| Stage-Modell | `src/core/stage.rs` | `Stage` mit `regions`, `layer`, `blend_gamma`, `keep_rendering`, `crop` (Sichtfenster), `edge`+`soft_edge` (Randabblendung); `SurfaceGeometry` (Cue-Geometrie, Fit/Fill/Stretch/Custom, Hard Edge, Crop) |
| Renderer | `src/engine/route_renderer.rs` | `render_region_into` (Region→Route-Raster, Edge-Blend (x/breite)^(1/gamma), Stage-Crop/-Edge), `render_route`, `present_on_device` (Drehung→Spiegelung→Center/Fit/Stretch/Fill), `auto_blends` |
| Engine | `src/engine/real_engine.rs` | `ConfigureRoutes` → `RouteFrameStore` (`EngineHandle.route_frames`), Dirty-Tracking (keine Stage geändert → keine Arbeit), `tick_decklink` pusht Route-Bild über `OutputSink` |
| Fenster | `src/gui/stage_view.rs` | `draw_stage_windows` (Monitor), `draw_route_previews` (Vorschau pro Route), `draw_device_outputs` (ein randloses Fenster pro belegtem Display, Esc/Close-Button/Fokus/Cursor-None) |
| Settings | `src/gui/workspace_settings.rs` | Video-Seite mit 3 Reitern: „Video Outputs (Stages)“ (Tabelle, New-Stage-Menü, Multi-Output-Assistent), „Output Routing“ (Routen-Tabelle + `draw_route_editor`-Fenster), „Output Devices“ |
| Stage-Editor | `src/gui/stage_editor.rs` | Fenster pro Stage: Name/Layer/Keep/Crop/Edge/Resize, Regionsliste (Buchstabe, Route, Grid), Move/Draw, Zeichenfläche (ziehen/Ecken/Einrasten/Pfeiltasten) |
| Inspector | `src/gui/inspector/geometry.rs` | Stage/Layer + **`placement_preview`** (abstrahierte Vorschau), Labels heißen jetzt „Crop“ |
| Web-Remote | `src/remote/mod.rs`, `web_ui.html` | Befehle AddRoute/UpdateRoute/RemoveRoute/AddStage/UpdateStage/RemoveStage/RenameStage/AddRegion/UpdateRegion/RemoveRegion; Video-Seite mit stageMap |
| Audio | `src/core/audio.rs`, `src/engine/audio_backend.rs` | `AudioPatch` (16 Kanäle, je EIN `output: u8` am EINEN Gerät), `AudioBackend` = 1 cpal-Stream + gemeinsamer `mixer: Arc<Mutex<HashMap<Uuid, VoiceRef>>>`, `ChannelOutputs` = atomare [u8;16]-Zuordnung, `mix()` pro Stream |

Wichtige Verdrahtung: Jede Änderung an `app.routes`/Regionen **muss** `app.configure_routes()`
bzw. `app.configure_stage(stage)` aufrufen, sonst rendert die Engine nichts (das war
der Live-Bug `50f6d1b`: Engine hatte Stage ohne Regionen).

---

## ERLEDIGT nach dem Uebergabe-Dokument (Commits nach 14dfe4c)

- **Hardware-Dekodierung:** ffmpeg-Dekoder bekommt -hwaccel auto
  (VideoToolBox/NVDEC/VAAPI/DXVA je nach Plattform, automatischer Fallback auf
  Software; Frames werden fuer die rgba-Ausgabe automatisch vom GPU geholt,
  CPU-Compositing unveraendert). Abschaltbar mit KUHPLAY_HWACCEL=off.
  ffmpeg_decode_args() testbar; real_engine-Tests (echte Dekodierung) gruen.
  GPU-Compositing (wgpu o.ae.) ist NICHT umgesetzt - CPU-Renderer reicht gemessen
  (1080p ca. 226 fps), bei Bedarf eigener Auftrag.
- **Stabile Stage->Device-Bindung:** RouteManager::resolve_device_refs() loest
  Display-IDs per Name+Aufloesung (Punkte) neu auf, wenn ein Screen wegfaellt;
  bleibt es (missing), bleibt die UUID-Verknuepfung Region->Route stabil - das
  restliche Patching verschiebt sich nicht. Wird bei refresh_displays(),
  load_show() und setup_default_routes() aufgerufen. Tests in video_routing.rs.

---

## Aufgabe 1: Audio-Routing-Matrix — ERLEDIGT (Commit 65c101d)

**Umgesetzt:** 16 KuhPlay-Kanäle patchbar auf mehrere Ausgänge UND mehrere
Ausgabegeräte parallel.
- Modell `src/core/audio.rs`: `ChannelOutput { device: Option<String>, output }`,
  `AudioChannel.outputs: Vec<ChannelOutput>`, Legacy-`output` wird nicht mehr
  serialisiert und per `migrate_legacy()` beim Laden migriert;
  `AudioPatch::devices()` + `device_output_mask()` (u16-Bitmaske pro Gerät,
  mehrere Ausgänge pro Kanal); `AudioChannel::output_summary()` für Anzeigen.
- Engine: `SetAudioPatch { patch: Box<AudioPatch> }`; `apply_audio_patch()` in
  real_engine.rs verwaltet pro referenziertem Gerät einen Extra-Stream
  (`AudioBackend::with_shared_mixer`, gemeinsamer Mixer — dieselben Voices auf
  allen Geräten), schließt nicht mehr referenzierte Streams, meldet fehlende
  Geräte als Error (Show läuft weiter). `SetAudioOutput` baut das Primärgerät
  neu und nimmt Mixer + Patch mit (live nur ohne laufende Audio-Cues).
- `mix_voice` routet jedes gesetzte Bit der Ausgangsmaske auf den jeweiligen
  Geräteausgang (über Bit 16 hinaus abgeschnitten).
- UI: `workspace_settings.rs::draw_channel_patch` ist jetzt eine echte Matrix
  (Zeilen 16 Kanäle mit Namensfeld, Spalten = System Default + jedes Gerät ×
  Ausgang, Checkbox pro Zelle, horizontal scrollbar, live via set_audio_patch;
  Zuordnungen auf nicht verbundene Geräte bleiben erhalten + Hinweis).
  Inspector „Audio Levels“ zeigt pro Kanal die Ziel-Zusammenfassung.
- Web-Remote `pageAudio`: Matrix read-only (Namen editierbar), kein
  Überschreiben von Mehrfach-Zuordnungen mehr.
- Tests: 548 gruen (Serde alt→neu, Bitmaske, GUI-Smoke Matrix, Remote).
  clippy 0 Warnungen, Windows-Check (x86_64-pc-windows-msvc --features webview) ok.

---

## Aufgabe 2: Workspace Settings & Popups als eigene OS-Fenster — ERLEDIGT

**Umgesetzt:** Neuer Helfer `src/gui/viewport_window.rs` (`WindowSpec` +
`draw()`): ruft jeden Frame `ctx.show_viewport_immediate` auf → auf dem
Desktop echte OS-Fenster (Titelzeile, Close-Button, Esc schließt). Im
Headless-Fallback (Tests/Web, `ViewportClass::Embedded`) wird ein normales
`egui::Window` gezeichnet, damit bestehende Smoke-Tests stabil bleiben.
Umgestellt: Workspace Settings, Stage-Editor, Route-Editor,
Multi-Output-Assistent, About KuhPlay, Keyboard Shortcuts. Close-Request
setzt das jeweilige `open`-Flag zurück.
Hinweis: Das „Resize Stage"-Dialog im Stage-Editor bleibt ein Overlay IM
Stage-Editor-Fenster (verschachtelte Immediate-Viewports sind riskant).
Test: `viewport_window_embedded_fallback_renders` (gui_smoke).

---

## Aufgabe 3: Routen frei auf Ausgabegeräten positionieren (Device-Karte) — ERLEDIGT

**Umgesetzt:** `draw_device_map` oben im Tab „Output Routing"
(workspace_settings.rs): alle Displays als Umrisse (Welt = globale
Display-Punkte), Routen als farbige Rechtecke an ihrer `DeviceArea`-Position
(Part in Gerätepixeln, Umrechnung Punkte↔Pixel über `scale_factor`).
Interaktion: Ziehen verschiebt (Part.x/y), Ziehen an der Ecke unten rechts
skaliert (Part.width/height), Doppelklick setzt auf „Full“, Einrasten an
Geräterändern (Shift = ohne Snap), Änderungen wirken sofort über
`configure_routes()`. LED-Wand-Szenario abgedeckt: 800×600-Ausschnitt aus
1920×1080-SDI als `DeviceArea::Part`.
Pure Geometrie als `route_map_rect()` + 2 Unit-Tests (Full/Part, Retina-Skalierung);
Smoke-Test `output_routing_device_map_renders_route_area` (rendert nur, wenn im
Testprozess Displays erkannt werden). Doku: docs/UI.md §5.7.

---

## Aufgabe 4: Multi-Output-Assistent mit Raster (2×2 etc.) — ERLEDIGT

**Umgesetzt:** `MultiOutputWizard` jetzt mit `cols`/`rows` (statt `count`) und
getrennter Überlappung `overlap_x`/`overlap_y`; `add_multi_output_stage` legt
eine Route je Projektor an (zeilenweise row-major) und eine Stage mit
Gesamtgröße `cols·w−(cols−1)·ox × rows·h−(rows−1)·oy`, Regionen an den
Raster-Positionen mit auto_blend. Wizard-UI: Layout Cols/Rows + Overlap X/Y.
Test: `multi_output_wizard_2x2_grid` (4 Regionen an (0,0)/(1820,0)/(0,1030)/
(1820,1030), Stage 3740×2110 bei 1920×1080, ox=100, oy=50). Doku: docs/UI.md.

---

## Aufgabe 6: macOS Live-Check — ERLEDIGT (Commit 794851e, Live-Test mit Release-Build)

- **Farbiges Region-Grid:** `draw_region_grid` in Routenfarbe (Grid/Rahmen/
  Achse), Testbild bleibt neutral. Live verifiziert via pattern-Test.
- **Stage-Editor:** Inhalt jetzt in ScrollArea, Fenster 720×580 — Vorschau
  wird nicht mehr abgeschnitten.
- **WebView:** Subprocess-Fenster jetzt `with_visible(false)` — HTML5-Cues
  rendern offscreen in die Stage (Snapshot via WKWebView), kein eigenes Fenster.
- **Ruckeln:** Ursache war Engine-TICK 50 ms (nur 20 Hz Re-Render) + Debug-Build.
  Jetzt TICK 16 ms (~60 Hz) und Release-Build für Live-Betrieb. Video-Cue läuft
  live flüssig (Captures 1 s apart unterscheiden sich, Release).
- **Video-Ton:** Video-Cues legen jetzt zusätzlich eine Audio-Voice an
  (symphonia dekodiert die Tonspur aus dem Container, Pace gegen cpal-Callback
  = Echtzeit; Start im selben Tick wie Video → synchron). Stop/Pause/Resume/
  Seek/Finish führen die Voice mit. `EngineHandle::audio_voices` +
  `has_audio_voice()`; Test `video_cue_with_audio_track_registers_audio_voice`.
  webm/opus-Vorbis wird von symphonia nicht dekodiert (Feature fehlt) → webm
  bleibt stumm (Video läuft), mp4/AAC hat Ton.
- **Footage-Setup live:** Cart „Footage" mit 181 Video-Cues aus
  ~/sync/Geschäftlich/Footage (Name=Datei, stage=Stage 1), alte Cues gelöscht;
  Route auf großes Display entfernt, Stage 1 → Route „Display 2" (MacBook,
  device_area Full), output_mode=displays → Ausgabe NUR auf dem Notebook
  (5120,458 / 1512×982), App-Fenster liegt auf dem großen Display (1840,150).
- **Offen für manuelle Bestätigung durch den Nutzer:** Esc ohne Klick schließt
  die Vollbild-Ausgabe (Code in stage_view.rs + handle_hotkeys), Mauszeiger
  auf der Ausgabe unsichtbar (set_cursor_icon(None)).

---

## Aufgabe 5: Windows-11-PC — ERLEDIGT

- Quelle per tar/ssh nach `C:\Users\wolfb\kuhplay` (inkl. assets/ + icon.png für build.rs).
- `cargo build --release` dort: OK (2m18s; nur bekannte rustc-1.99-f32-Warnungen,
  die eigenen davon in `Stroke::new` wurden auf `_f32` gefixt).
- `cargo test --no-fail-fast --release` dort: 28 Test-Binaries, 546 passed, 0 failed,
  2 SKIP (ffmpeg fehlt auf dem PC → Video-Real-Tests springen sauber, kein Fehler).
- App dort gestartet (GUI-Session nötig; aus ssh-Session läuft sie headless weiter):
  Web-Remote auf `KUHPLAY_REMOTE_WEB_PORT=18080`. Cross-Check vom Mac:
  `curl http://192.168.88.132:18080/api/session` → HTTP 200. Dafür Firewall-Regel
  nötig gewesen: `netsh advfirewall firewall add rule name="KuhPlay WebRemote 18080"
  dir=in action=allow protocol=TCP localport=18080` (Port 8080 war schon freigegeben).
- Achtung cmd: `set VAR=18080` (Leerzeichen!) landet als "18080 " → Port-Parsing schlägt
  fehl → default 8080. Immer `set "VAR=18080"` in .bat-Dateien.

---

## ERLEDIGT nach dem Übergabe-Dokument (neuere Commits)

- **Neue QLab-Cue-Typen (Commit 406419e):** Wait (Timer-Unit, `EngineCommand::Wait`
  in beiden Engines), Memo (Sofort-Cue), Text (rendert als Inline-HTML über die
  HTML5-Pipeline, `src/core/text.rs`), Network (UDP/TCP, `EngineCommand::SendNetwork`,
  fire-and-forget-Thread in der Real-Engine). Toolbox/Inspector/Icons/Web-Remote
  verdrahtet; ⌘0–⌘9 bleiben den 10 Original-Typen vorbehalten (wie QLab).
- **MIDI-Cue (Commit da78e33):** `CueType::Midi` + `src/core/midi.rs` (Note On/Off,
  CC, Program Change, Wire-Bytes), `EngineCommand::SendMidi`, midir-Output lazy in
  der Real-Engine (`src/engine/midi_out.rs`, Gerät via `KUHPLAY_MIDI_OUT_DEVICE`).
- **Workspace Templates (Commit 8a5726b):** benannte UiState-Snapshots, eigener
  Storage-Key, Settings → General (Save/Apply/Delete).
- **Light-Cue (Art-Net DMX, Commit siehe Git-Log):** `CueType::Light` +
  `src/core/light.rs` (ArtDmx-Paket OpDmx 0x5000, Port 6454), `EngineCommand::SendDmx`,
  Sofort-Cue. Camera/Mic-Cues bewusst **weggelassen** (kein Live-Input gewünscht).
- **Offen aus qlab5-parity.md:** Lighting-Pult/LCC (nur DMX-Senden umgesetzt),
  Timecode-Cue mit LTC/MTC-Empfang (Chasing pro Cue ist da), Audio Map Editor
  (Patch-Matrix deckt Kanal→Ausgang ab), Script-Cue dediziert (HTML5/JS-Bridge
  deckt ab), File Target Search Tool, Monitor/Status-Fenster, QLab-`.qlab5`-Import.

---

## Sonstiges Kleinvolumen (nicht vergessen)

- **docs/TODO-video-routing.md** als „abgearbeitet“ markieren oder ins Archiv
  verschieben; Abschlussbericht war nur in Chat/Git-Commits.
- Remote-Web-UI: Blackmagic-Gerät nur im App-Route-Editor wählbar (bekannte
  Abweichung, dokumentiert).
- Region-Grid-Beschriftung „A: Route“ (Pfeil fehlt im Font) — dokumentiert.
- Windows-Remote: `is_playback` enthält keine Edit-Befehle (Companion bleibt
  reines Playback) — bei neuen Remote-Befehlen weiter beachten.

## Live-Test-Werkzeuge (bereits vorhanden, in /tmp/kuhplay-verify — nicht im Repo)

- `list-windows.swift`: eigene Fenster via CGWindowListCopyWindowInfo (id, Titel,
  Bounds, Layer) — `swift /tmp/kuhplay-verify/list-windows.swift`.
- `analyze/`: `analyze_pngs` (Helligkeit/Differenz zweier Screenshots) und
  `region` (Pixel-Region dumpen) — Basis für Fenster-Inhaltsprüfungen.
- `screencapture -x -o -l<window-id> datei.png` — NUR eigene Fenster aufnehmen,
  nie den ganzen Bildschirm (§7!). Fenster-IDs werden schnell stale — direkt vor
  dem Aufnehmen neu listen.
- App live steuern: Web-Remote-Befehle per
  `POST /api/command` (z. B. `go_to`, `set_output_mode`, `update_cue`,
  `add_region` …); App-Start mit
  `KUHPLAY_REMOTE_WEB_PORT=18090 KUHPLAY_REMOTE_COMPANION=off KUHPLAY_OSC=off
  KUHPLAY_MIDI=off KUHPLAY_REMOTE_PIN=off target/release/kuhplay`.
  Aktuell läuft evtl. noch eine Instanz auf Port 8080 (Nutzer nutzt die Remote!) —
  vor Neustarts den Nutzer fragen bzw. Port 8080 in Ruhe lassen.

## Nutzungskontext des Auftraggebers (für Entscheidungen)

- Bühnenpraxis: LED-Wand über SDI 1920×1080, Wand selbst z. B. 800×600 → Route mit
  `DeviceArea::Part` auf dem SDI-Gerät („Area of Interest“ der Ausgabe).
- Große Stages (z. B. 8K) auf mehrere 4K-Displays (2×2) verteilen; **Stages heißen
  bei ihnen „Layer“** (wie in anderen Medienservern) und sollen frei über den
  Outputs angeordnet werden — Stage.layer + Regionen decken das ab, die Freiform-
  Bedienung ist Aufgabe 3/4.
- „Hard Edge“ beim Cue-Geometry = weiche Kante einer Cue-Surface (Blend über
  Breite in px, Linear/Cosine) für Übergänge zwischen Projektionen; das
  Stage-Pendant sind jetzt Stage-Edge + Region-Edge-Blends (Soft Edge = Gamma-
  Kurve, Hard Edge = linear).

## Bekannte umweltbedingte Test-Flakes (Stand: aktueller Mac, kein Regressionsfehler)

- `detect_displays()` liefert im Testprozess derzeit 0 Displays (WindowServer-
  Session) → `gui_smoke::workspace_settings_video_page_renders_and_edits_stages_and_routes`
  fällt dadurch (tritt identisch auf dem letzten Commit auf).
- real_engine-Timing-Tests (`audio_play_started_then_finished_at_eof` u.a.)
  flaken je nach Audio-Device-/CPU-Zustand; ebenfalls nicht durch Codeänderungen
  verursacht (Stash-Vergleich). Auf dem Windows-PC dokumentieren, nicht schwächen.
