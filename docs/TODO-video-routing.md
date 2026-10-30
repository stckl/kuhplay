# TODO: Video-Zuordnung wie QLab 5 (Stage → Region → Output Route → Device)

Arbeitsauftrag für die Umsetzung. Wer das hier abarbeitet, soll keine Grundsatz-
entscheidungen neu treffen: Abschnitt 3 legt sie fest. Passt etwas davon nicht zum
Code, nicht still abweichen, sondern im Abschlussbericht nennen.

**Vorlage:** <https://qlab.app/docs/v5/video/video-output/> und
<https://qlab.app/docs/v5/video/video-cues/> (Geometry-Tab). Vor dem Start lesen.

---

## 1. Ausgangslage

Aufbau heute (`src/core/screen.rs`, `src/core/stage.rs`):

```text
Cue → Stage (Canvas) → Stage.screen → Screen (x, y, w, h im Desktop) → ein Fenster
```

Die Probleme, die dieser Umbau löst:

| Problem | Ursache |
|---|---|
| Inhalt wird verzerrt, wenn Stage und Screen verschiedene Formate haben | Stage wird immer auf den Screen gestreckt, keine Skalierung |
| Screen über zwei Displays erscheint nur auf einem | Ein einziges Fenster; macOS trennt Spaces pro Display |
| Hard Edge über mehrere Projektoren fehlt | Keine Regionen, nur eine Kante pro Cue-Surface |
| Zwei Stages auf einem Screen überdecken sich zufällig | Ein Fenster pro Stage, keine Stage-Layer |
| Nur Displays als Ziel | Kein Device-Begriff; Decklink läuft separat über Umgebungsvariablen |

Schon vorhanden und zu erhalten:

- Fit/Fill/Stretch/Custom pro Cue (`GeometryMode`, `SurfaceGeometry::placement`).
- Testbild in `src/engine/pattern.rs`. **Das Aussehen des Grid ist vom Auftraggeber
  abgenommen, nicht umgestalten.**
- Exakte Fensterplatzierung auf macOS (`src/gui/macos_output.rs`).
- Vorschau-Fenster pro Screen (`stage_view::draw_screen_previews`).
- Web-Remote mit PIN (`src/remote/web.rs`, `src/remote/web_ui.html`).

---

## 2. Zielbild

```text
Cue ──▶ Stage ──▶ Region ──▶ Output Route ──▶ Device
       Bildfläche  Ausschnitt   logischer       Display oder
       mit Layern  der Stage    Ausgang         Blackmagic
```

Begriffe in der UI exakt wie in QLab: **Stage, Region, Output Route, Device**. Das
Wort „Screen" verschwindet aus der UI.

- Ein Cue spielt auf **einer** Stage.
- Eine Stage hat **0 bis n** Regionen. Jede Region zeigt auf **0 oder 1** Route.
- Eine Route hat **0 oder 1** Device und kann nur einen Teil davon belegen.
- Mehrere Regionen (auch verschiedener Stages) dürfen dieselbe Route nutzen.

---

## 3. Festgelegte Entscheidungen

1. **Rendern auf der CPU in reinen Funktionen.** `render_route(...)` nimmt Stage-Frames
   und liefert ein `VideoFrame`. Grund: ohne GPU testbar, und Decklink braucht die Pixel
   ohnehin. Performance wird gemessen (Phase 3), nicht vorab optimiert.
2. **Ein Fenster pro belegtem Display**, nicht pro Stage oder Route. Dieses Fenster
   zeigt alle Routen des Displays. Damit fällt die Spaces-Grenze von macOS weg.
3. **Vorschau: ein Fenster pro Route.** Ausdrücklicher Wunsch: Für die Vorschau zählt
   der Inhalt, nicht der Bildschirm. Zusätzlich pro Stage ein Monitor-Fenster (QLab:
   „Monitor"-Button). Vorhanden ist `draw_stage_windows`, weiterverwenden.
4. **Auflösung in echten Pixeln.** `DisplayInfo.width/height` sind auf Retina Punkte
   (MacBook: 1512×982 bei 3024×1964 Pixeln). Route-Auflösung = Punkte × `scale_factor`.
   Die Fensterplatzierung bleibt in Punkten. Sonst ist das Testbild am Ausgang keine
   1-Pixel-Linie. Das Texturfiltern im Ausgabefenster auf `NEAREST` stellen, wenn Textur
   und Fenster pixelgleich sind.
5. **Regionen füllen ihre Route:** Eine Region wird auf den ganzen Raster der Route
   abgebildet. Region und Route mit gleichem Format verzerren nicht. Warping ist nicht
   Teil dieses Auftrags.
6. **Interne Koordinaten: Pixel, Ursprung oben links.** QLab zeigt Regionen relativ zur
   Stage-Mitte an. Hier oben links, in UI und Datei gleich.
7. **Alte Shows laden weiter und sehen gleich aus** (Abschnitt 6).
8. **Keine neue externe Abhängigkeit** ohne Rückfrage.
9. **Kein `ffmpeg` mit Decklink ausliefern.** Decklink ist dort `nonfree`, solche Builds
   sind nicht weitergabefähig.

### Bewusst NICHT in diesem Auftrag

Warping, Masken, Syphon, NDI, Rotation und Anker von Cues, Keying, Audio pro Gerät,
Blackmagic-Hardware-Ausgabe (nur die Schnittstelle, siehe Phase 7).

---

## 4. Datenmodell

Neue Datei `src/core/video_routing.rs`, in `src/core/mod.rs` exportieren. Alles `serde`
mit `#[serde(default)]` an neuen Feldern. Vorschlag:

```rust
pub enum DeviceRef {
    /// Display. `id` kann sich ändern, deshalb auch Name und Auflösung als Rückfall.
    Display { id: u32, name: String, width: u32, height: u32 },
    /// Blackmagic-Ausgang (Name aus Desktop Video).
    Blackmagic { name: String },
}

pub enum RouteScaling { Center, Fit, #[default] Stretch, Fill }

pub enum DeviceArea {
    #[default] Full,
    /// Teil des Geräts in Gerätepixeln (QLab: Partial Output).
    Part { x: u32, y: u32, width: u32, height: u32 },
}

pub struct OutputRoute {
    pub id: Uuid,
    pub name: String,
    pub width: u32,          // 1 bis 30000
    pub height: u32,
    pub device: Option<DeviceRef>,
    pub device_area: DeviceArea,
    pub rotation: ScreenRotation,   // aus screen.rs weiterverwenden
    pub rear_projection: bool,      // horizontal spiegeln
    pub scaling: RouteScaling,
    pub color: [u8; 3],             // screen_color_for_index
    pub test_pattern: bool,         // Testbild über dem vollen Raster
    pub test_pattern_kind: TestPatternKind,
    pub checker_size: u32,
}

pub struct EdgeBlend { pub left: f32, pub right: f32, pub top: f32, pub bottom: f32 }

pub struct Region {
    pub id: Uuid,
    pub route: Option<Uuid>,
    pub x: f32, pub y: f32, pub width: f32, pub height: f32, // Stage-Pixel
    pub blend: EdgeBlend,
    pub auto_blend: bool,  // Default true
    pub grid: bool,        // Raster nur über dieser Region
}

pub struct RouteManager { routes: Vec<OutputRoute> }
```

`Stage` (in `src/core/stage.rs`) bekommt:

```rust
#[serde(default)] pub regions: Vec<Region>,
#[serde(default)] pub layer: i32,               // höher = oben; gleich = Name alphabetisch
#[serde(default = "default_gamma")] pub blend_gamma: f32,   // 2.2
#[serde(default)] pub keep_rendering: bool,     // Schwarz halten zwischen Cues
```

Regeln:

- Region-Buchstabe (A, B, …) ergibt sich aus der Position, wird nicht gespeichert.
- Regionen müssen innerhalb der Stage liegen (`validate`, UI begrenzt).
- Stage-Größe bis 16384×16384, wie heute.
- Reihenfolge laut QLab: höherer Stage-Layer liegt oben. Gleicher Layer: der alphabetisch
  frühere Name liegt oben.

`ShowDocument` (`src/core/show.rs`) bekommt `routes: RouteManager`. Siehe Abschnitt 6
zum Umgang mit dem alten Feld `screens`.

**Tests:** Serde-Rundlauf, Validierung, Layer-Sortierung inklusive Alphabet-Regel,
Buchstaben A…Z, alte JSON-Datei ohne neue Felder.

---

## 5. Umsetzung in Phasen

Jede Phase endet mit grünem `cargo test`, Clippy ohne Warnung und einem eigenen Commit.

### Phase 1: Datenmodell und Migration
- Abschnitt 4 umsetzen, Abschnitt 6 umsetzen.
- `ShowApp` (`src/gui/app.rs`): Feld `routes`, Speichern und Laden in `save_show`,
  `load_show`, `new_show`.
- **Neue Show wie QLab:** pro Display eine Route (`Display n`) und eine Stage in dessen
  Pixelgröße mit einer Region über die ganze Fläche. `Stage 1` bleibt für die Demo-Cues
  bestehen und liegt auf dem ersten Display.
- Fertig, wenn: Eine alte Show lädt, speichert und wieder lädt, ohne dass Daten fehlen.

### Phase 2: Reine Render-Funktionen
Neue Datei `src/engine/route_renderer.rs`:

```rust
pub fn render_region_into(target, route_w, route_h, stage_frame, region, blend_gamma);
pub fn render_route(route, stages_on_route_sorted, frames) -> VideoFrame;
pub fn present_on_device(route_frame, route, device_w, device_h) -> VideoFrame;
pub fn auto_blends(regions_of_stage) -> Vec<EdgeBlend>;
```

- `render_route`: schwarzer Raster; Stages nach Layer (unten zuerst); pro Region der
  Ausschnitt auf den vollen Raster. Stage-Alpha respektieren: Was nicht deckt, zeigt die
  Stage darunter.
- **Edge Blend:** Faktor an den Kanten über `(x/breite)^(1/gamma)`, damit die Summe im
  Überlapp konstant hell bleibt. `auto_blends`: Überlappt Region A die Region B auf
  derselben Stage, ist die Überlappbreite der Blend beider Kanten.
- `present_on_device`: Drehung, Spiegelung, dann Skalierung:
  Center (Mitte, ungeskaliert, Überstand abschneiden), Fit (Balken), Stretch, Fill
  (Überstand abschneiden, Mitte).
- Testbild: `route.test_pattern` → `draw_test_pattern` im Raster der Route, überdeckt den
  Inhalt. `region.grid` → Raster nur im Bereich dieser Region, Beschriftung
  `A → <Routenname>`.
- Die Testbild-Spec heute hängt am Screen (`Screen::pattern_spec_numbered`,
  `EngineCommand::SetTestPattern`, `stage_patterns`, Sonderfall in `Compositor::composite`).
  Das Testbild gehört künftig zur Route; den Sonderfall im Compositor entfernen.

**Pflicht-Tests (ohne Fenster):**
- Zwei Regionen nebeneinander: linke Route zeigt nur linke Hälfte, rechte nur rechte.
- 200 Pixel Überlappung: In der Mitte des Blends ergibt die Summe beider Routen die
  Ausgangshelligkeit (Toleranz ±2 von 255, unter Berücksichtigung des Gamma).
- Jede Skalierung: 1920×1080 auf 1000×1000 mit exakten Rechtecken wie im vorhandenen Test
  `placement_modes_follow_qlab`.
- Drehung 90° und Spiegelung auf einem kleinen Bild mit bekannten Pixeln.
- Zwei Stages auf einer Route: Layer und Alphabet-Regel.
- Testbild-Kreis bleibt rund, wenn Route und Stage verschiedene Formate haben.

### Phase 3: Engine anbinden
- `EngineCommand` (`src/engine/command.rs`): `ConfigureRoutes { routes }` statt
  `SetTestPattern`. `ConfigureStage` trägt die Regionen bereits über `Stage`.
- `RealMediaEngine` (`src/engine/real_engine.rs`): nach `composite_dirty_stages` die
  betroffenen Routen neu rendern, in `RouteFrameStore` legen (Muster: `StageFrameStore`
  in `src/engine/frame.rs`).
- **Nur bei Änderung rendern.** Keine Stage geändert → keine Arbeit.
- `MockMediaEngine` nimmt die Kommandos an, ohne zu rendern (wie heute bei Stages).
- **Messen:** Eine Route 1920×1080 und eine 5120×1440 mit laufendem Video. Bericht mit
  Bildern pro Sekunde im Release-Build. Unter 25 fps bei 1080p melden, nicht heimlich
  Qualität senken.
- Fertig, wenn: Ein Integrationstest mit `spawn_real_engine` ein Route-Bild liefert.

### Phase 4: Fenster
`src/gui/stage_view.rs`:
- `draw_screen_viewports` ersetzen durch `draw_device_outputs`: ein randloses Fenster pro
  belegtem Display. Titel eindeutig halten (`KuhPlay – Output <display-id>`), weil
  `macos_output::place_output_window` das Fenster über den Titel findet. Rechteck aus dem
  Display, nicht aus der Route.
- Routen mit `DeviceArea::Part` an ihrer Stelle im Fenster zeichnen.
- Esc im Ausgabefenster schließt die Ausgabe (vorhanden, erhalten).
- `draw_screen_previews` → ein Fenster pro Route, Titel `Route: <Name>`. Testbild-Linien
  weiter nachzeichnen, sonst verschwinden sie beim Verkleinern (vorhanden).
- `OutputMode` (Off, Preview, Displays) bleibt. `apply_startup_output` bleibt: Mit nur
  einem Display startet die App mit Vorschau.
- Stage-Monitor-Fenster behalten.
- Keine zufällige Reihenfolge mehr, wenn sich Routen überlappen: Listenreihenfolge.
- `stage_screen_open` wird zu „Device offen" oder entfällt zugunsten von `OutputMode`.

### Phase 5: Einstellungen in der App
`src/gui/workspace_settings.rs`, Seite Video, drei Reiter wie QLab:

**Video Outputs (Stages):** Tabelle Name, Auflösung, Devices, `Edit`, `Monitor`, Löschen.
„New Stage" mit Menü: Leer (1920×1080), Stage mit Output (je Route), Multi-Output Stage
(Anzahl, Auflösung und Anordnung der Projektoren abfragen, Overlap in Pixeln).
Löschen wie heute nur, wenn kein Cue die Stage nutzt.

**Stage-Editor** (eigenes Fenster):
- Name, Layer, Keep rendering, Größe mit `Resize…`.
- Regionsliste: Buchstabe mit Farbe, Route-Auswahl, Grid-Häkchen, Löschen.
- „New region with output…" und Umschalter Move/Draw.
- Rechts: Größe, Position, Edge Blends, Auto, Blend-Gamma.
- Zeichenfläche: Stage, Regionen in Routenfarbe, ziehbar, Griffe zur Größe, Einrasten an
  Rand, Mittellinien und anderen Regionen; ⌘ schaltet das Einrasten aus. Pfeiltasten:
  1 Pixel, ⇧ 10 Pixel, ⌥ Größe. Blend-Bereich schraffiert, Region ohne Route gepunktet.
  `screen_editor.rs::draw_map` und `snap_to_displays` sind als Ausgangspunkt nutzbar.

**Output Routing:** Tabelle Name, Auflösung, Device-Auswahl, Teilbereich, Testbild,
Farbe, `Edit`, Löschen. Fehlendes Gerät kursiv mit „(missing)". Route-Editor: Name, Größe,
„Set from Device", Drehung, Rear projection, Skalierung, Device, Teilbereich.
„New Output Route" mit Menü je Device.

**Output Devices:** Name, Auflösung, Info (Hz, soweit verfügbar). Refresh-Button. Hinweis,
dass Anordnung und Auflösung der Displays in den Systemeinstellungen geändert werden.

- `src/gui/screen_editor.rs` danach entfernen, nichts Brauchbares verlieren.
- Der Bereich Stages vom letzten Stand in `draw_stages` geht im Reiter Video Outputs auf.
- `src/gui/inspector/geometry.rs`: Stage-Auswahl bleibt, „Stage Settings" (Canvas, Screen)
  entfällt zugunsten eines Buttons „Edit Stage…".
- Farben, Größen und Formulare nach `docs/UI.md` (`theme::form_grid`, `field_label` usw.).

### Phase 6: Web-Remote
`src/remote/mod.rs`, `src/remote/web_ui.html`, Handler in `apply_remote_command`:
- `RemoteWorkspace`: `routes`, Stages mit Regionen, Devices statt `screens`/`displays`.
- Befehle: `AddRoute`, `UpdateRoute`, `RemoveRoute`, `AddRegion`, `UpdateRegion`,
  `RemoveRegion`, `AddStage`, `UpdateStage`, `RemoveStage`, `RenameStage`. Alle zählen als
  Bearbeiten (`is_edit`), also Show-Mode-Sperre. **Nicht** in `is_playback` aufnehmen, sonst
  sind sie über Companion ohne PIN erreichbar.
- Die drei Reiter und den Stage-Editor nachbauen; Karte `screenMap()` wird zur Zeichenfläche.
- Fehlt der PIN, bleiben `accesses_files`-Befehle gesperrt. Nichts daran lockern.
- Prüfung im Browser wie in Abschnitt 8.

### Phase 7: Schnittstelle für Blackmagic (ohne Hardware-Pflicht)
- `DeviceRef::Blackmagic` im Modell und in der Device-Liste, ohne Gerät als „(missing)".
- Trait `OutputSink { fn push(&mut self, frame: &VideoFrame) }` in `src/engine/`.
  `tick_decklink` so umbauen, dass es ein Route-Bild statt eines Stage-Bilds bekommt.
- Die `KUHPLAY_DECKLINK_*`-Umgebungsvariablen funktionieren weiter.
- **Nicht** versuchen, das Gerät anzusprechen. Das UltraStudio ist auf dem Testrechner
  wegen eines alten Treibers noch nicht sichtbar; die Anbindung ist ein eigener Auftrag.

### Phase 8: Doku
- `docs/UI.md`: Abschnitte zu Screens und Stages neu; „Screen" raus.
- `docs/qlab5-parity.md`: Zeilen Video Output und Screens aktualisieren.
- `README.md`: Funktionsübersicht und Konfiguration.
- `src/gui/mod.rs`, Modul-Kommentare, `tests/screens.rs` umbenennen, falls der Inhalt jetzt
  Routen betrifft.

---

## 6. Migration alter Shows

Beim Laden, wenn `routes` leer und `screens` oder `Stage.screen` vorhanden sind:

- Alte Felder bleiben lesbar (`#[serde(default, skip_serializing)]`), werden beim Speichern
  nicht mehr geschrieben. Die Bestands-Tests in `src/core/show.rs` entsprechend anpassen,
  nicht löschen.
- **Jeder Screen wird zu Routen:**
  - Liegt er in einem Display: eine Route auf diesem Display, `DeviceArea::Full`, wenn er es
    deckt, sonst `Part`. Routengröße = Screen in Pixeln.
  - Liegt er über mehreren Displays: pro Display eine Route.
  - Kein Display passt (anderer Rechner): Route ohne Device, Anzeige „(missing)".
  - Name, Drehung, Farbe, Testbild übernehmen.
- **Jede Stage mit `screen`** bekommt Regionen, die die Stage proportional auf diese Routen
  aufteilen. Skalierung `Stretch`: So sieht die Show aus wie vorher, auch wenn sie verzerrt
  war. Nichts „verbessern".
- Stage ohne Screen: keine Region.
- Cues bleiben unverändert (`Cue.stage`, `Cue.surface`, `GeometryMode`).
- **Tests:** Je ein Fall aus der Liste, plus die Datei aus einem festen JSON-Beispiel in
  `tests/`, damit die Migration auch künftig geprüft bleibt.

---

## 7. Stolperfallen aus diesem Projekt

Das sind echte Vorfälle, keine Theorie.

- **Bildschirme des Nutzers:** App nur nach Rückfrage starten, wenn dabei das Vollbild
  aufgeht oder der Fokus wechselt. Während des Tippens landeten Tasten schon in der App.
  Nie den ganzen Bildschirm aufnehmen (private Mails waren zu sehen), nur das eigene Fenster:
  `screencapture -x -o -l <window-id>`. Wo möglich Bilder als PNG aus Tests erzeugen.
- **Mock-Engine zeichnet keine Stages.** Ein schwarzes Bild mit `KUHPLAY_ENGINE=mock` ist
  kein Fehler. Für Bildprüfungen die echte Engine nehmen.
- **Headless-Tests:** `show_viewport_deferred` wird dort in den Hauptkontext eingebettet,
  ohne Fenstertitel. Tests können den Titel also nicht finden, nur den Inhalt.
- **`detect_displays` nur durch die vorhandene Mutex-Funktion aufrufen.** AppKit stürzt bei
  parallelen Aufrufen ab (SIGSEGV), Tests bauen viele `ShowApp` gleichzeitig.
- **Testzahl prüfen, nicht nur „failed: 0".** Eine abgestürzte Testdatei fehlt einfach in der
  Summe. Stand heute: 503 Tests. Fällt die Zahl, stimmt etwas nicht.
- **macOS kennt kein `timeout`.** Stattdessen `perl -e 'alarm 240; exec @ARGV' cargo test`.
- **MIDI-Tests** überspringen sich mit `SKIP`, wenn CoreMIDI keinen Client zulässt. Das ist
  kein Fehler. OSC- und Remote-Tests binden echte Ports.
- **Farben mit Transparenz nur über `gamma_multiply`.** `linear_multiply` und
  `from_rgba_unmultiplied` wirken auf Dunkel viel zu hell.
- **Sonderzeichen im UI:** `→ ↑ ↓ ⌥ ⇧ ⌫` fehlen im Font und erscheinen als Kästchen, auch in
  neuen Texten wie „A → Route". In der App ein Phosphor-Icon nehmen oder umformulieren. Im
  Testbild-Renderer (`ab_glyph`) ebenfalls prüfen.
- **Web-UI:** kein `confirm()`, sondern `ask(...)`. Verschachtelte Arrays in `h(...)` sind
  erlaubt. Texte wie `[object …]`, `undefined` und `NaN` im Seitentext sind Fehler.
- **`cargo fmt` formatiert um:** Skripte, die Quelltext per exaktem Text ersetzen, brechen
  danach gern ab. Nach jedem Skript prüfen, ob wirklich alles angekommen ist.
- **Show-Mode gilt überall**, auch für neue Remote-Befehle.
- **Windows nicht vergessen:** `macos_output` und `native_menu` sind `cfg(target_os = "macos")`.
  Auf anderen Systemen muss das Fenster auch ohne sie funktionieren, dort aber ohne die
  Platzierung über AppKit.

---

## 8. Prüfung vor dem Abschluss

Bei jedem Commit:

```sh
cargo fmt
cargo test --no-fail-fast      # Zahl der Tests mit dem Stand davor vergleichen
cargo clippy --all-targets     # keine Warnungen
cargo check --target x86_64-pc-windows-msvc --features webview
```

Am Ende zusätzlich:

1. **Mit echtem Material** aus `/Users/stocke/sync/Geschäftlich/Streaming Testcontent`, nur
   lesen, nichts ins öffentliche Repo kopieren: `porsche-testvideo.mp4` (1920×1080),
   `BigBuckBunny.mov` (1280×720), `testbild.png`.
2. **Bilder aus Tests als PNG** ansehen: zwei Regionen mit Overlap auf zwei Routen; jede
   Skalierung; Grid auf einer Region.
3. **Web-Remote im unsichtbaren Browser** (Chrome `--headless=new` mit
   `--remote-debugging-port`, per Skript steuern, App ohne Fenster in einem Test laufen lassen:
   `poll_engine_events`, `poll_remote_commands`, `pump_remote` in einer Schleife). Jede Seite
   durchklicken, Ergebnis über `/api/workspace` prüfen, Screenshot ansehen.
4. **Nur nach Rückfrage, einmal am Ende:** zwei echte Displays (5120×1440 und 1512×982 bei
   Retina), Fensterposition über `CGWindowListCopyWindowInfo` mit den Display-Grenzen vergleichen,
   nur die eigenen Fenster aufnehmen.

### Abnahme

- [ ] Eine Stage läuft über zwei Displays, mit weichem Blend im Überlapp.
- [ ] Zwei Stages auf einer Route liegen in der richtigen Reihenfolge übereinander.
- [ ] Mehrere Routen auf einem Display liegen an ihrer Stelle.
- [ ] Route mit anderer Auflösung als das Gerät: Center, Fit, Stretch und Fill stimmen.
- [ ] Testbild pro Route, Raster pro Region; Kreis rund; 1 px am Ausgang ist 1 Gerätepixel.
- [ ] Fehlendes Gerät zeigt „(missing)", die Show läuft weiter und die Route lässt sich neu
      zuweisen.
- [ ] Vorschau pro Route funktioniert mit nur einem Display.
- [ ] Alte Show sieht nach dem Laden gleich aus.
- [ ] Alles auch in der Web-Remote bedienbar, Show-Mode sperrt es dort.
- [ ] Companion kann weiterhin nur Playback.

### Abschlussbericht

Ehrlich und konkret: was fertig ist, was getestet wurde und wie, was nur angenommen ist,
gemessene Bilder pro Sekunde, jede Abweichung von Abschnitt 3.

---

## 9. Arbeitsregeln

- Kommentare auf Deutsch, UI-Texte auf Englisch, Stil des umgebenden Codes.
- Commits nur auf einem eigenen Branch, zum Beispiel `feature/video-routing`, nicht direkt auf
  `main`. **Nicht pushen**, außer es ist ausdrücklich beauftragt.
- Vor dem Start muss der aktuelle Arbeitsstand committet sein (Abschnitt 1 setzt ihn voraus).
- Den Inhalt von `docs/UI.md` befolgen: Änderungen an den Regeln zuerst dort.
