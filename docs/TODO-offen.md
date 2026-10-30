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

## Aufgabe 1 (angefangen, nur Analyse): Audio-Routing-Matrix

**Wunsch des Nutzers:** In den Audio-Einstellungen eine Matrix, in der die 16
KuhPlay-Kanäle auf die Ausgänge des Computers gepatcht werden können —
(a) ein Kanal auf **mehrere** Ausgänge, (b) **mehrere Ausgabegeräte parallel**.

**Aktueller Stand (nur gelesen, noch nichts geändert):**
- `AudioChannel { name, output: u8 }` — genau EIN Ausgang, kein Gerät im Modell.
- `EngineCommand::SetAudioPatch { outputs: [u8; AUDIO_CHANNEL_COUNT] }` — eine
  Zuordnung für das eine Gerät.
- `AudioBackend::with_device` baut EINEN Stream; `set_channel_outputs([u8;16])`
  setzt atomare Channel→Geräteausgang-Maske; `mix()` (audio_backend.rs ~Z. 218)
  rendert den gemeinsamen Mixer in den einen Stream. Voices liegen pro Backend
  (`self.voices` + `self.mixer`), `mix_voice` nutzt `masks` (Cue-Route-Matrix)
  und `outputs` (Patch-Maske).
- Real-Engine: Feld `audio: Option<AudioBackend>`, `SetAudioOutput` wechselt das
  Gerät (nur wenn kein Audio läuft), `SetAudioPatch` → `set_channel_outputs`.
- UI: `workspace_settings.rs::draw_channel_patch` — pro Kanal EIN Dropdown.
- Web-Remote: `pageAudio` in `web_ui.html` — Patch-Formular mit je einem Select
  pro Kanal; Inspector „Audio Levels“ hat bereits eine Cue→Kanal-Matrix (`masks`).

**Geplanter Entwurf (abarbeiten):**
1. **Modell** `src/core/audio.rs`:
   - Neu: `ChannelOutput { device: Option<String>, output: u8 }` (`None`-Gerät =
     System-Standard/primäres Gerät, damit Default-Shows ohne Gerätenamen laufen).
   - `AudioChannel` ergänzen: `outputs: Vec<ChannelOutput>` mit
     `#[serde(default)]`; altes Feld `output: u8` zu `#[serde(default, skip_serializing)]`
     machen (Legacy-Dateien laden weiter: `#[serde(default)] fn migrate` oder
     nachträgliche Migration — wenn `outputs` leer und `output > 0`, ergibt
     `outputs = vec![ChannelOutput { device: None, output }]`).
   - `AudioPatch::default`: Kanal n → `outputs = [{device: None, output: n+1}]`.
   - Helper: `pub fn device_outputs(&self, device: &str) -> [u8; 16]` — Kanal→
     Ausgang **an diesem Gerät** (0 = aus), d. h. Zuordnungen mit anderem Gerät
     werden ignoriert; `pub fn devices(&self) -> Vec<Option<String>>` — alle im
     Patch referenzierten Geräte (eindeutig, Reihenfolge stabil).
2. **Engine-Befehl** `src/engine/command.rs`: `SetAudioPatch { outputs: [u8; 16] }`
   → ersetzen durch `SetAudioPatch { patch: Box<AudioPatch> }` (oder
   `assignments: Vec<(Option<String>, [u8; 16])>` — entscheiden; `Box<AudioPatch>`
   ist serde-freundlicher, da `AudioPatch` schon Serialize/Deserialize ist).
   Alle Aufrufer anpassen: `app.set_audio_patch`, Mock-Engine, Tests
   (`tests/audio_params.rs`, `tests/remote.rs`, evtl. `osc.rs`).
3. **Backend** `src/engine/audio_backend.rs`:
   - `with_device` um einen Parameter ergänzen, der einen **bestehenden Mixer-Arc**
     teilt (z. B. `with_shared_mixer(name, mixer)`), damit alle Geräte-Streams
     dieselben Voices mischen. `voices`-Verwaltung zieht in ein übergeordnetes
     Objekt (siehe 4) — Backend wird reiner „Stream + Maske“.
4. **Real-Engine** `src/engine/real_engine.rs`:
   - `audio: Option<AudioBackend>` → `audios: Vec<AudioBackend>` (je Gerät einer)
     + EIN gemeinsamer Mixer. `SetAudioPatch`: für jedes im Patch referenzierte
     Gerät fehlenden Stream öffnen, nicht mehr referenzierte Streams schließen
     (Fehler nur melden, nicht alles abschalten). `add_voice`/`stop_voice` auf den
     gemeinsamen Mixer. `SetAudioOutput` bleibt als „primäres Gerät“ (Kompat,
     Companion/Remote), wirkt auf `device: None`-Zuordnungen.
   - „Stop running audio cues before changing the device“-Logik überdenken: Bei
     Mehrgeräte-Patch darf live umgepatcht werden (Masken sind atomar).
5. **UI** `workspace_settings.rs::draw_channel_patch` → echte Matrix:
   - Zeilen = 16 Kanäle (Name editierbar), Spalten = **(Gerät, Ausgang)** über
     ALLE Geräte aus `output_devices()` (Spaltenkopf: Kurzname + Ausgang N);
   - Zelle = Checkbox (an/aus), horizontal scrollbar; ein Kanal kann mehrere
     Zellen (auch geräteübergreifend) aktivieren;
   - „Off“-Spalte zum Deaktivieren; Hinweis, dass Umlegen live wirkt.
   - Geräteliste einmalig cachen wie `audio_devices`.
6. **Web-Remote**: `pageAudio` Patch-Editor auf Read-only-Zusammenfassung
   (`device:out, …` je Kanal, Namen editierbar) umbauen + Hinweis „Full matrix in
   the app“ — NICHT per Einzelselect speichern, sonst werden Mehrfach-Zuordnungen
   beim Speichern aus der Remote gelöscht (Datenverlust!). `RemoteWorkspace`
   `audio_patch` serialisiert das neue Modell automatisch mit.
7. **Tests**: Serde-Rundlauf alt→neu (Datei mit `output: 3` lädt und ergibt
   `outputs=[{None,3}]`), `device_outputs()` je Gerät, Mock nimmt `SetAudioPatch`
   an, UI rendert headless (gui_smoke: Matrix-Text vorhanden), Remote-Test
   (`set_audio_patch` mit neuem Modell), Engine-Test: zwei Geräte-Patches →
   zwei Streams (ohne Hardware nur Fehlerpfad/Mock prüfbar).
8. **Doku**: README-Zeile Audio, `docs/UI.md` §5.7 Audio-Abschnitt, ggf.
   `docs/qlab5-parity.md` Audio-Patch-Zeile.

**Abgrenzung:** Cue-seitiges Routing (Route/Maske im Inspector „Audio Levels“)
bleibt, wie es ist — nur der Patch Kanal→Gerät wird zur Matrix.

---

## Aufgabe 2: Workspace Settings & Popups als eigene Fenster

**Wunsch:** Workspace-Settings-Fenster und alle anderen Popups/Modals sollen
**eigene OS-Fenster** sein, keine Overlays im Hauptfenster.

**Betroffene egui::Windows (derzeit Overlays):**
- `workspace_settings::draw` (Workspace Settings, mit Nav)
- `stage_editor::draw` (Stage Editor)
- `workspace_settings::draw_route_editor` (Route Editor)
- `workspace_settings::draw_multi_output_wizard` (Multi-Output-Assistent)
- `app.rs`: „About KuhPlay“-Fenster, `shortcuts::draw_window` (Keyboard Shortcuts)

**Entwurf:** egui-Viewports mit `ctx.show_viewport_immediate(...)` (direkter
Zugriff auf `&mut ShowApp` im Closure — `show_viewport_deferred` geht nicht, weil
der Closure 'static sein muss; die Fenster brauchen aber App-Zustand). Pro Fenster:
- `ViewportId::from_hash_of("workspace-settings")` usw., `ViewportBuilder` mit
  `.with_title(...)`, `.with_decorations(true)`, `.with_inner_size(...)`,
  `.with_resizable(true)`;
- Inhalt wie bisher (gleiche ui-Funktionen wiederverwenden, nur statt
  `egui::Window::new(...)` ein `CentralPanel` in den Viewport zeichnen);
- Schließen: `ctx.input(|i| i.viewport().close_requested())` → Flag zurücksetzen
  (`app.ui.workspace_settings_open = false` usw.); Esc zusätzlich.
- Vorsicht: Die egui-Window-Extras (`.constrain_to`, Pivot, max_height) entfallen —
  Layout in `ScrollArea` kapseln. Menüpunkt/zahnrad-Aufrufe bleiben, sie setzen nur
  die Flags. **Pro Fenster ein eigener Commit + headless gui_smoke-Test**
  (render_frame rendert Viewports in den Kontext eingebettet — Titel nicht findbar,
  nur Inhalte; die bestehenden Text-Assertions weiterführen).
- Ansatz-Reihenfolge: erst Workspace Settings (am meisten Wert), dann Stage
  Editor/Route Editor/Wizard, dann About/Shortcuts. Wenn ein Fenster Probleme
  macht (Fokus/Platzierung), Einzelfall dokumentieren statt die ganze Aufgabe abbrechen.

---

## Aufgabe 3: Routen frei auf Ausgabegeräten positionieren (Device-Karte)

**Wunsch/Nutzungsszenario (Nutzer-Zitat):** „LED-Wand per SDI: SDI ist 1920×1080,
die Wand aber 800×600 — wir nutzen nur einen Teil des SDI-Signals (Area of
Interest).“ Dazu: „Beim Output Routing will ich die Displays/Routen frei auf den
Output-Devices positionieren können.“

**Das Modell kann das schon** (`DeviceArea::Part { x, y, width, height }` in
**Gerätepixeln**); es fehlt die grafische Bedienung (früher hatte der gelöschte
`screen_editor.rs` genau so eine Karte — als Vorlage aus der Git-History holen:
`git show 5c1cc0f^:src/gui/screen_editor.rs`).

**Entwurf:** Im Reiter „Output Routing“ (workspace_settings.rs) über der Routen-
Tabelle eine Karte (`draw_device_map`) pro Device (oder eine Karte mit allen
Displays nebeneinander):
- SVG-artiges Zeichnen mit `ui.painter()` wie `stage_editor::draw_canvas`:
  Display-Umriss (Punkte→Pixel über `scale_factor`), Routen als farbige Rechtecke
  (Routenfarbe) an ihrer `DeviceArea::Part`-Position, `Full` = ganzes Display;
- **Ziehen** verschiebt (ändert `Part.x/y`), **Ecke unten rechts** skaliert
  (`Part.width/height`), Doppelklick oder „Full“-Button auf Full zurück;
  Einrasten an Display-Rändern (Toleranz, ⌘ schaltet aus — wie stage_editor);
- Nur Routen mit `DeviceRef::Display { id }` des angezeigten Devices;
  Blackmagic-Routen ohne Karte (nur Tabellen-Edit);
- Nach jeder Änderung `app.configure_routes()` aufrufen;
- Buttons „Set from Device“ (im Route-Editor vorhanden) bleibt.
- Tests: headless gui_smoke — Karte rendert ohne Panic, Routen mit Part erscheinen
  (Text/„Resolution“-Assertions erweitern); Interaktion nur manuell prüfbar
  (Screencapture live — siehe „Live-Test-Werkzeuge“ unten).
- Doku: `docs/UI.md` §5.7 + qlab5-parity („Output Routing“-Zeile).

---

## Aufgabe 4: Multi-Output-Assistent mit freier Anordnung (2×2 etc.)

**Nutzungsszenario (Nutzer):** „8K-Stage auf 4 4K-Displays — oben rechts, oben
links, unten rechts, unten links.“ Der Assistent (`add_multi_output_stage` in
`app.rs`, Wizard-UI `draw_multi_output_wizard`) legt derzeit nur **horizontal**
an.

**Entwurf:** `MultiOutputWizard` um `rows: u32`/`cols: u32` (statt nur `count`)
ergänzen; Regionen werden zeilenweise angeordnet (x = col*(w-overlap),
y = row*(h-overlap_v)), Overlap horizontal + vertikal getrennt;
`add_multi_output_stage` entsprechend umbauen (Stage-Breite/-Höhe aus dem Raster).
Serde/default der Wizard-Struktur ist nur UI-State (nicht persistent) — einfach.
Tests: `add_multi_output_stage` mit rows=2, cols=2 → 4 Regionen, Stage-Größe
`2*w-overlap × 2*h-overlap_v`, Regionen an den Rasterpositionen; Wizard rendert
headless. Danach in `docs/UI.md` erwähnen.

---

## Aufgabe 5: Windows-11-PC (Testrechner) — Build & Tests dort

**Zugang:** `ssh wolfb@192.168.88.132` (Zertifikat, BatchMode funktioniert).
Quelle liegt unter `C:\Users\wolfb\kuhplay` (tar-über-ssh-Kopie OHNE `.git`,
`target`, `dist`; `assets/` + `icon.png` wurden nachkopiert — build.rs braucht sie).
**Kein ffmpeg auf dem PC** (Video-Playout/real-engine-Video-Tests brauchen es —
entweder ffmpeg nachinstallieren (Rückfrage beim Nutzer) oder die betreffenden
Fehler als „fehlende Abhängigkeit“ im Bericht führen und nicht als Regression).

**Offen:**
1. Neuen Stand rüberspielen (auf dem Mac:
   `tar czf - --exclude target --exclude .git --exclude dist --exclude .DS_Store src tests docs Cargo.toml Cargo.lock README.md LICENSE assets icon.png | ssh wolfb@192.168.88.132 "tar xf - -C kuhplay"`)
   — ACHTUNG: Commits seit der letzten Kopie sind noch nicht drin.
2. `cargo build --release` auf dem PC (ein Hintergrundlauf von vorhin lief mit
   alten Dateien; Ergebnis in `kuhplay\build.log` — neu starten).
   Bekannte Warnungen des neueren rustc 1.99: „falling back to f32 as the trait
   bound `f32: From<f64>` …“ (28×, in Abhängigkeiten/unserem Code prüfen — beim
   letzten Stand wurden sie nur als Warnung geführt; MACHT-Tests nicht stören).
3. `cargo test --no-fail-fast` dort: Zu erwartende Abweichungen: MIDI-Tests
   (midir/WinMM) evtl. SKIP, OSC/Remote binden Ports (Windows-Firewall-Prompt
   möglich), Video-Tests ohne ffmpeg. Alles dokumentieren, keine „Heilung“ per
   Abschwächung der Tests.
4. App dort starten (via SSH: läuft in der SSH-Session, Fenster erscheint nicht
   auf dem Desktop — aber Web-Remote/Engine testen): Start mit
   `KUHPLAY_REMOTE_WEB_PORT=18080 ... kuhplay.exe`, dann vom Mac aus
   `curl http://192.168.88.132:18080/api/session` prüfen (cross-machine).
5. Bericht: was lief auf Windows, was nicht (ffmpeg, MIDI, …).

**Auch macOS-live noch offen:** Esc/Fokus/Cursor-Verhalten des Ausgabefensters
(Fix `14931e3`) ist gebaut, aber am Bildschirm noch nicht verifiziert; Nutzer
soll es bestätigen (Vollbild auf, Esc → weg, Mauszeiger unsichtbar). Zwei-Display-
Abnahme (5120×1440 + MacBook) war live begonnen (Fenster lagen exakt auf den
Display-Grenzen); nach Aufgabe 3/4 nochmal mit Nutzer zusammen durchgehen.

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
