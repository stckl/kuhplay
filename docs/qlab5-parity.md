# KuhPlay – QLab 5 Parity-Dokumentation

Diese Datei dokumentiert kapitelweise, wie weit KuhPlay dem offiziellen
QLab-5-Handbuch (<https://qlab.app/docs/v5/>) entspricht. Sie wird bei
jedem Feature-Schritt aktualisiert.

**Legende**

| Symbol | Bedeutung |
|--------|-----------|
| ✅ | vollständig umgesetzt (1:1-Verhalten) |
| 🟡 | teilweise umgesetzt / funktional äquivalent, Abweichung dokumentiert |
| ⬜ | noch nicht umgesetzt (Roadmap) |

**GUI-Mapping (QLab 5 „The Workspace" → KuhPlay-Module)**

| QLab 5 Bereich | Position | KuhPlay-Modul |
|----------------|----------|---------------|
| Menüleiste (File/Edit/View/Cues/Workspace/Help) | oben | `src/gui/menu_bar.rs` |
| Masthead (GO, Standby-Display „Q# • Name", Notes volle Breite; **keine Toolbar**) | oben | `src/gui/masthead.rs` – Toolbox ist die einzige Cue-Typ-Button-Quelle |
| Toolbox (Cue-Typ-Buttons, gruppiert, Icon + Beschriftung) | linke Leiste | `src/gui/toolbox.rs` – 140 px, einklappbar (View ⌘K) |
| Cue-Liste (Status/Type/Q#/Name/Target/Pre-wait/Duration/Post-wait/Continue) | Zentrum | `src/gui/cue_table.rs` – **Phase 2:** Status-Pille über eine Farben-Quelle (`theme::status_color`; Standby neutral, nur Broken rot mit ✕ + Tooltip-Gründen), Cue-Farbe als Zeilen-Tönung + 3px-Streifen (statt Punkt), Cursor-Zeile hervorgehoben, Fortschrittsbalken in Action/Post-Wait, Pre-Wait-Countdown. **Phase 3 (QLab 5 Spaltensatz, gegen das QLab-5-Handbuch geprüft):** exakt die 9 QLab-Spalten (Status/Type/Q#/Name/Target/Pre-wait/Duration/Post-wait/Continue; „Duration" inkl. Duration-Spalte, Target-Spalte statt QLab-„Notes"-Ansicht – Abweichung notiert), Spaltenbreiten persistiert + per Header-Rechtsklick veränderbar (ein-/ausblenden, Reset), Timecode-Anzeige `MM:SS.mmm` mit Restzeit bei Playing und Countdown bei Post-Wait, Inline-Edit per Doppelklick (Q#/Name/Pre/Duration/Post; Enter/Fokusverlust übernimmt, Esc verwirft, Timecode-Eingabe `MM:SS.mmm`/`HH:MM:SS.mmm`/nackte Sekunden), Multi-Select (⌘-Klick, ⇧-Klick für Bereich), Drag&Drop-Umordnung (Drop auf Gruppe = hineinlegen), Gruppen per Dreieck ein-/ausklappen, Zeilen-Kontextmenü (Playhead/Arm/Group/Duplicate/Color/Move/Delete), Auto-Scroll zum Playhead |
| Sidebar (Transport-Icon-Leiste, Tabs: Lists & Carts / Active Cues) | rechte Leiste | `src/gui/sidebar.rs` – Reset/Pause/Resume/Panic als Icons mit Tooltips (Esc-Hinweis an Panic). **Phase 6:** Active Cues zeigt pro laufendem Cue Typ-Icon + „Q#  Name“ + elapsed/−remaining und Pause/Resume/Stop-Buttons (28×28 px) |
| Inspector (typabhängige Tabs wie QLab 5) | unteres Panel | `src/gui/inspector/` (Submodule, keine Datei > 400 Zeilen) – **Phase 4:** Kopfzeile (Typ-Icon + „Q# • Name“, bei Mehrfachauswahl „N cues selected"), typabhängige Tabs (`tabs_for`: alle Basics+Triggers; Audio/Video + „Time & Loops" (+ „Audio Levels", Video auch „Geometry"); Fade „Curve"; HTML5 „Geometry"+„Source"; Diashow „Geometry"+„Slides"; HTML-Data „Data"), Basics zweispaltig wie QLab (links Q#/Name/Target/Pre Wait/Duration/Post Wait/Continue, rechts Armed/Color/Notes/Type), Zeitfelder als Timecode `MM:SS.mmm` (Anzeige+Eingabe wie die Cue-Liste, DragValue-Ziehen bleibt), Slices-Editor + große Waveform in „Time & Loops", Show-Mode read-only |
| Footer (Edit/Show, ⚠ kaputte Cues, „N cues in M lists", Icon-Toggles + Zahnrad) | ganz unten | `src/gui/footer.rs` – inkl. nicht-blockierender Fehleranzeige (`last_error` + „×" zum Schließen); ⚠-Zähler springt per Klick zum ersten kaputten Cue |
| Workspace-Settings-Fenster (Nav: General/Audio/Video/Network/MIDI/Key Map) | schwebend | `src/gui/workspace_settings.rs` – Video-Seite mit drei Reitern wie QLab 5: "Video Outputs (Stages)" (Stages-Tabelle, New-Stage-Menü inkl. Multi-Output-Assistent), "Output Routing" (Routen-Tabelle + Route-Editor), "Output Devices" (Displays mit Auflösung/Hz); Stage-Editor als eigenes Fenster (`src/gui/stage_editor.rs`) |
| UI-Layout-Persistenz (Panel-Sichtbarkeit, Tabs, Settings-Seite) | global | `src/gui/state.rs` – `UiState` (serde) über `eframe::Storage` (`App::save` / `new_with_storage`) |
| Broken-Cue-Erkennung (fehlende Datei, unbekannte Stage, …) | global | `src/gui/problems.rs` – `cue_problems` + 2-s-Cache in `ShowApp` (`broken_count`, `jump_to_first_broken`) |
| Theme (Dark-Mode) + Layout-Konstanten | global | `src/gui/theme.rs` – Farben + `theme::size` (GO-Button, Panel-Breiten, Button-Größen); **Phase 2:** Palette aufgeräumt (Rot nur für Broken/Panic/Error), eine Status-Farben-Quelle `status_color()`, `SELECTION`/`CURSOR_ROW`/`ROW_TINT_ALPHA`, Textfarben WCAG AA (≥ 4,5:1); wird einmalig in `main.rs` gesetzt |
| Icons (alle UI-Icons, eingebetteter Font) | global | `src/gui/icons.rs` – **Phosphor Icons** (Regular, MIT, `assets/fonts/phosphor/`); inkl. Gear/Sidebar/Sliders (Phase 1) |
| Stage-Monitor-Fenster (QLab "Monitor") | schwebend | `src/gui/stage_view.rs::draw_stage_windows` – komponiertes Stage-Canvas (Layer, AoI, Hard Edge, Deckkraft) + Routen-Info |
| Route-Vorschau (ein Fenster pro Output Route) | schwebend | `src/gui/stage_view.rs::draw_route_previews` – gerendertes Route-Bild (Inhalt, nicht Bildschirm) |
| Device-Ausgabe (Route → Hardware-Display) | Vollbild-Viewport | `src/gui/stage_view.rs::draw_device_outputs` – **ein randloses Fenster pro belegtem Display**, zeigt alle Routen des Displays (`DeviceArea::Part` an ihrer Stelle), Esc schließt |

---

## General

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| What's New in QLab 5 | ⬜ | Meta-Kapitel |
| Change Log | ⬜ | siehe `CHANGELOG`-Einträge im Repo |
| System Recommendations | ⬜ | Doku |
| Preparing Your Mac | ⬜ | macOS-spezifisch |
| Keyboard Shortcuts | ✅ | **Phase 5:** zentrale Tabelle `src/gui/shortcuts.rs` (`Shortcut { group, action, key, modifiers, label }`, `ALL`) – Menüs, Shortcuts-Fenster (Help → „Keyboard Shortcuts“) und `handle_hotkeys` lesen dieselbe Quelle. Belegung aus dem QLab-5-Handbuch: Space = GO, Esc = Panic All, `[`/`]` = Pause All/Resume All, Reset All (Menü-only, QLab hat kein Kürzel), ↑/↓ = Playhead, ⌘↑/⌘↓ + ⌥⌘↑/⌥⌘↓ = Move Up/Down, ⌘0–⌘9 = neue Cues (Cues-Menü), ⌘I/⌘K/⌘L = Inspector/Toolbox/Sidebar, ⌘N/⌘O/⌘S/⇧⌘S/⌘,/⌘Q = Datei, ⌘F = Find (Leiste über der Cue-Liste, matcht Q#/Name/Notes, Enter/⌘G = nächster Treffer, Nicht-Treffer abgedunkelt), ⌘G = Find Next, ⌘⌫/⌫/Delete = Delete (nur Edit-Mode), ⌘D = Duplicate, Group Cues (Menü-only). Abweichungen siehe „Bekannte Abweichungen“ Nr. 8 |
| Licenses | ⬜ | kein Lizenzmodell in KuhPlay |
| Features by License | ⬜ | — |
| QLab Preferences | ⬜ | Einstellungen-Fenster geplant |

## Fundamentals

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| The Workspace | ✅ | Menüleiste, Masthead (GO links ≥120×64, Standby-Display „Q# • Name", Notes-Feld volle Breite, Audition als Pill-Label; **keine Toolbar**), Toolbox links (gruppiert, Icon + Beschriftung, **einzige Cue-Typ-Button-Quelle**, 140 px, einklappbar via View ⌘K), Cue-Liste Zentrum (QLab-Spalten), Sidebar rechts (Transport-Icon-Leiste + Tabs „Lists & Carts" / „Active Cues"), Inspector unten, Footer ganz unten (Edit/Show, ⚠-Indikator kaputte Cues, „N cues in M lists", Icon-Toggles Sidebar ⌘L / Inspector ⌘I / Zahnrad Workspace Settings). Panel-Positionen entsprechen QLab 5. Alle Panels mit Innenabständen (Padding), abgerundete Widgets, Vektor-Icons (`src/gui/icons.rs`). **Phase 6:** Leerzustände (leere Cue-Liste: „Drag files here or use the Toolbox to add cues"; Inspector ohne Selektion: „No cue selected" zentriert), Datei-Drop aus dem Finder legt Cues an (Endung → Audio/Video/Slideshow, `cue_type_for_file`), einheitliche Spacing-Konstanten 4/8/12/16 px (`theme::size`), Tooltips für alle Icon-only-Elemente (Playhead, Typ-Icons, Auto-Continue, Continue-Mode). **Phase 1:** Layout (Panel-Sichtbarkeit, aktive Tabs, Settings-Seite) persistiert via `UiState` + eframe storage. **Phase 2:** Farbsemantik – Standby/akzent neutral, Rot nur für Broken/Panic/Error, konsistente Status-Farben (`theme::status_color`), WCAG-AA-Textkontraste. |
| Workspace Settings | 🟡 | Fenster mit linker Nav (General/Audio/**Video**/Network/MIDI/Key Map); **Video mit drei Reitern wie QLab 5**: "Video Outputs (Stages)", "Output Routing", "Output Devices" (+ Stage-Editor als eigenes Fenster). Audio/Network/MIDI mit Inhalten. Offen: Key Map nur Anzeige, Theme-Auswahl |
| Workspace Templates | ✅ | Benannte Layout-Vorlagen (Snapshot des `UiState`): Settings → General „Workspace Templates" (Save current layout / Apply / Delete), Persistenz über eigenen eframe-Storage-Key (`kuhplay.ui_templates`), Rename bei gleichem Namen, Find-Laufzeitzustand bleibt beim Anwenden. Test: `workspace_templates_save_apply_delete_and_persist` |
| Saving/Managing/Moving Workspaces | 🟡 | Show speichern/laden als `.kuhplay` (JSON, serde) via File-Menü + `rfd`. Kein QLab-`.qlab5`-Format, kein Versions-/Backup-Management |
| Cues | ✅ | Cue-Typen (Audio, Video, Fade, Group, Stop, Script, Control, HTML5, Slideshow, HTML Data, **Wait, Memo, Text, Network, MIDI, Light**), Q#-Nummern, Namen, Armed/Disarmed, Notizen, Farben (Status-Farben). **Interaktion (QLab 5):** Klick auf Zeile = selektieren (Inspector), Klick in Status-Spalte = Playhead setzen, Doppelklick = Arm/Disarm toggeln, Rechtsklick = Kontextmenü (Playhead / Arm / Move / Delete). Kinder von Gruppen eingerückt. Disarmte Cues ausgegraut. **Phase 0:** Q-Nummern eindeutig (`next_free_number`, höchste Nummer + 1 inkl. Gruppen-Kinder); Löschen stoppt laufende Cues (`ShowApp::delete_cue_at` dispatcht `stop_running`); Selektion von Group-Kindern auf Phase 3 verschoben (UUID-Selektion). **Phase 1:** Broken-Cue-Erkennung (`src/gui/problems.rs`: fehlende Datei, unbekannte Stage, HTML ohne Quelle, Slideshow ohne/fehlende Slides, HtmlData ohne Target/ungültiges JSON, Control-Target nicht gefunden; Gruppen-Kinder rollen hoch) – gezählt im Footer „⚠ N" (Tooltip mit Gründen, Klick springt zum ersten kaputten Cue). Getestet headless in `tests/gui_interaction.rs` (simulierte Maus-Events). **Phase 2 (Status-Semantik wie QLab 5):** Standby ist neutral (kein Rot), nur Broken-Cues sind rot (✕ in der Status-Spalte, Tooltip nennt die Gründe via `problems_of`; bei aktiver Reihe zusätzlich ✕ am Pillenrand); Status-Farbe kommt aus einer Quelle (`theme::status_color`); Cue-Farbe färbt die Zeile dezent + 3px-Streifen am linken Rand (statt Punkt in der Name-Spalte); Cursor-Zeile (Playhead) dezent hervorgehoben; laufende Cues zeigen Fortschrittsbalken in der Action-Spalte, Post-Wait-Cues in der Post-Spalte, Pre-Wait-Cues einen Countdown in der Pre-Spalte (`prewait_started` in `ShowApp`). **Phase 3 (Interaktion wie QLab 5):** Selektion per UUID-Mehrfachauswahl (⌘-Klick toggelt, ⇧-Klick wählt Bereich, Selektion überlebt Cue-Operationen); Doppelklick auf Q#/Name/Pre-wait/Duration/Post-wait startet Inline-Edit (QLab: Zellen sind direkt editierbar), Enter/Fokusverlust übernimmt, Esc verwirft; Zeiten editieren als Timecode (`MM:SS.mmm`, `HH:MM:SS.mmm` oder Sekunden – QLab akzeptiert beides); Drag&Drop verschiebt Zeilen (QLab: „drag to reorder"), Drop auf eine Gruppe legt den Cue hinein; Gruppen klappen per Dreieck in der Type-Spalte ein/aus (QLab: disclosure triangle); Kontextmenü pro Zeile (Set Playhead Here, Arm/Disarm, Group, Duplicate, Set Color, Move Up/Down, Delete – QLab-Menüpunkte „Group Cues", „Duplicate Cue(s)" nachempfunden); Spalten per Header-Kontextmenü konfigurierbar (QLab: Spalten ein-/ausblenden via Header-Menü). Doppelklick toggelt **nicht** mehr Arm/Disarm (QLab 5: Doppelklick startet den Edit) – Abweichung von der V1-Annahme notiert. |
| The Inspector | ✅ | **Phase 4 (QLab 5 „The Inspector"):** Kopfzeile mit Typ-Icon, Q# und Name („N cues selected" bei Mehrfachauswahl); typabhängige Tabs wie im QLab-5-Handbuch pro Cue-Typ (Basics/Triggers überall; Audio/Video: Time & Loops + Audio Levels; Video/HTML5/Diashow: Geometry; Fade: Curve; HTML5: Source; Diashow: Slides; HTML-Data: Data) – nicht vorhandene Tabs werden weggelassen. Basics zweispaltig: links Q#, Name, Target (Datei + „…"-Button bzw. Control-Target), Pre Wait, Duration, Post Wait, Continue; rechts Armed-Checkbox, Color-Swatches, großes Notes-Feld, Type. Zeitfelder als `MM:SS.mmm`-Timecode (Eingabe `MM:SS.mmm`/`HH:MM:SS.mmm`/Sekunden, Drag-Ziehen bleibt). Time & Loops: Loop-Modus, Slices (Start/End/Play-Count als Timecode), Waveform groß. **Abweichung:** KuhPlay-Fades haben kein Ziel-Cue/-Stage (Parameter des eigenen Cues wird gefadet) – „Audio Levels/Geometry je nach Ziel" entfällt daher beim Fade; Volume/Rate/Opacity liegen im Curve-Tab. // TODO(qlab-parity): verifizieren |
| Cue Lists | ✅ | `CartManager` verwaltet mehrere Cue-Listen; aktive Liste steuerbar |
| Cue Carts | ✅ | Sidebar-Tab „Lists & Carts": Cart-Liste (auswählbar, mit Cue-Anzahl) + „New Cart". **Phase 0:** Engine-Events (Status, Auto-Follow, Stop/Pause/Resume) erreichen Cues in allen Carts (UUID-Suche über alle Listen via `find_cue_any`) |
| Group Cues | ✅ | Modi *Timeline* (zeitversetzt), *Fire all simultaneously*, *Sequential*, *Loop* (Re-Arm mit Iterationslimit), *Shuffle* (deterministisch) |
| Cue Sequences | 🟡 | Continue-Mode `Off` / `Auto-Continue` / `Auto-Follow` (QLab-Spalte „Cont." + Inspector „Continue"). **Abweichung V1:** Auto-Continue feuert den Folge-Cue sofort beim Start; eine Post-Wait-Verzögerung innerhalb der Kette ist noch nicht verkettet. Auto-Follow feuert beim `Completed` des Cues. |

## Workflow Tools

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Tools Menu | ⬜ | — |
| Find | ✅ | Find-Leiste über der Cue-Liste (⌘F, matcht Q#/Name/Notes, Enter/⌘G nächster Treffer, Treffer-Highlight + Playhead springt mit) |
| Paste Cue Properties | ✅ | Edit-Menü „Copy/Paste Cue Properties": Properties (ohne ID/Nummer/Status) in Zwischenablage, Paste merged auf alle selektierten Cues |
| Launcher Window | ⬜ | — |
| Workspace Status Window | ⬜ | — |
| File Target Search Tool | ⬜ | — |
| Monitor Windows | ⬜ | — |
| Timecode Status Window | ⬜ | — |
| Auditioning Cues | 🟡 | Audition Mode (Masthead-Pill + Workspace Settings): Video-Cues ohne Stage spielen trotzdem (Preview); noch kein separates Audition-Fenster |
| Override Controls | ⬜ | — |

## Audio

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Intro to Audio | ⬜ | Doku |
| Object Audio | ⬜ | — |
| Audio Cues | ✅ | **Echtes Playout** (symphonia-Dekodierung + cpal-Ausgabe, `src/engine/audio_backend.rs`): Mehrkanal-Routing (bis 16 Kanäle + Pan), Volume-Fades (dB-Interpolation), Playrate/Pitch-Fading, Slices mit Play-Counts. Getestet in `tests/real_engine.rs` (echte WAV-Wiedergabe, Pause/Resume/Stop, EOF) |
| Mic Cues | ⬜ | — |
| Audio Output Patch Editor | ✅ | Patch-Matrix in Workspace Settings → Audio: 16 Kanäle × (Gerät, Ausgang) mit Checkboxen; ein Kanal auf mehrere Ausgänge/Geräte parallel (Engine: Extra-Streams mit gemeinsamem Mixer); Umlegen wirkt live; Webremote zeigt die Matrix read-only |
| Audio Map Editor | ⬜ | — |
| Fading Audio and Audio Effects | ✅ | Fades für Volume/Rate/Opacity, lineare + dB-Kurve; Effekte-Architektur vorbereitet (Trait) |

## Video

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Intro to Video | ⬜ | Doku |
| Video Cues | ✅ | **Echtes Playout** (ffmpeg-Subprocess → rawvideo/rgba, `src/engine/video_backend.rs`): Stage + Layer + Surface-Geometrie (Position/Größe/Deckkraft). Hardwarebeschleunigtes Decoding vorbereitet über `MediaEngine`-Trait (GStreamer). Getestet in `tests/real_engine.rs` (echte MP4-Wiedergabe, Frames im Store, EOF) |
| Camera Cues | ⬜ | — |
| Text Cues | ✅ | Dedizierter **Text-Cue** (`CueType::Text`, `src/core/text.rs`): Text, Schriftgröße, Text-/Hintergrundfarbe, horizontale/vertikale Ausrichtung (Inspector-Tab „Text“). Rendert als Inline-HTML über die HTML5-Pipeline (WebView → Snapshot → Stage-Compositor), Ziel-Stage/Layer wie Video. HTML5-Cues (Inline/URL/Datei) bleiben für komplexe Overlays |
| Blend Mode | ✅ | `BlendMode` auf der Surface (SourceOver/Multiply/Screen/Overlay/Darken/Lighten/Add/Difference), Compositor mischt pro Kanal, UI im Geometry-Tab; Test multiply_blends_instead_of_covering |
| Video Output | ✅ | **Stage → Region → Output Route → Device** (QLab 5): Routen als logische Ausgänge (Auflösung in echten Pixeln, Device, Teilbereich, Testbild, Skalierung Center/Fit/Stretch/Fill); CPU-Renderer `src/engine/route_renderer.rs` (Edge Blend mit Blend-Gamma, Auto-Blend aus Regions-Überlappung); Engine rendert nur geänderte Routen in den `RouteFrameStore` (`ConfigureRoutes`); ein Fenster pro belegtem Display + Vorschau pro Route + Stage-Monitor (`src/gui/stage_view.rs`) |
| Stages / Video Outputs | ✅ | Stages mit **Regionen** (Ausschnitte auf Routen, Buchstaben A/B/…, Edge Blends, Auto-Blend, Grid), Layern (höher = oben, gleicher Layer: alphabetisch früherer Name oben), Keep Rendering, Blend-Gamma; Stage-Editor (`src/gui/stage_editor.rs`) mit Zeichenfläche (ziehen, Ecken, Einrasten, Pfeiltasten); New-Stage-Menü (leer / mit Output / Multi-Output-Assistent). Alte Shows (Screens) werden beim Laden migriert, das Screen-Modell ist Legacy |
| Area of Interest | ✅ | Surface-Crop als normalisiertes Keep-Rechteck (left/top/right/bottom 0–1), Pixel-Umrechnung + Clamping im Compositor (`SurfaceGeometry::source_rect`), Inspector-Editor. Getestet: `tests/real_engine.rs::stage_canvas_composites_surface_with_area_of_interest` |
| Hard Edge (Edge Blending) | ✅ | `HardEdge` (Seite Left/Right/Top/Bottom, Breite in px, Kurve Linear/Cosine) – Alpha-Rampe über die Überlappungszone zweier Surfaces im Software-Compositor (`src/engine/compositor.rs`), Inspector-Editor. Getestet: `composite_hard_edge_blends_overlap` |
| Using NDI | ⬜ | **bewusst weggelassen** (Nutzerentscheidung) – stattdessen HTML5/JS mit Live-Daten/Commands |
| Fading Video and Video Effects | ✅ | Opacity-Fades (Compositor-Deckkraft + Stage-Dirty-Tracking, getestet in `tests/real_engine.rs::stage_canvas_reflects_opacity_fade`); Effekte-Architektur vorbereitet |

## Lighting

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Intro to Lighting | 🟡 | generische Parameter-Fades (Fade-Cues: Volume/Rate/Opacity) vorhanden; eigenes Licht-Pult (Dashboard/LCC) ⬜ |
| Light Cues | 🟡 | **Light-Cue** (`CueType::Light`, `src/core/light.rs`): Art-Net-DMX-Paket (OpDmx, Port 6454) per UDP – Universe, Startkanal 1–512, Kanalwerte (Dez/Hex), Ziel-Host (Broadcast default), Sofort-Cue. Engine: fire-and-forget UDP-Thread (`EngineCommand::SendDmx`). Inspector-Tab „Light", Toolbox-Gruppe „Lighting“. **Abweichung:** keine Cue-übergreifenden Licht-Fades/Chaser (dafür Fade-Cues + Sequences); kein eigenes Licht-Pult |
| Light Dashboard | ⬜ | — |
| Lighting Command Language | ⬜ | — |
| Light Patch Editor | ⬜ | — |
| Light Library | ⬜ | — |

## Networking, MIDI, Show Control

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Collaboration | ⬜ | — |
| QLab Remote | ⬜ | — |
| Stream Deck | ⬜ | — |
| OSC | ⬜ | — |
| MIDI & MSC | ⬜ | — |
| Show control broadcast | ⬜ | — |
| Timecode | 🟡 | Timecode-Chasing (LTC/MTC) vorbereitet: `Cue::timecode_in`, `EngineCommand::ChaseTimecode`, präzise Zeitsprünge (Timecode-Chase-Tests). Kein echtes LTC/MTC-Empfangen in V1; Inspector-Tab „Triggers" mit FPS/Offset/„Start Timecode Chasing" |
| Network cues | ✅ | **Network-Cue** (`CueType::Network`, `src/core/network.rs`): UDP/TCP-Nachricht an `host:port`, Sofort-Cue (in der Liste direkt „Completed“). Real-Engine sendet fire-and-forget in einem eigenen Thread (blockierendes Ziel hält die Engine nicht auf), Fehler als `EngineEvent::Error` (Footer ⚠). Inspector-Tab „Network“, Toolbox-Gruppe „Network“, Web-Remote. Loopback-Tests: UDP/TCP-Empfang + Connect-Fehler (`tests/network_cue.rs`) |
| MIDI cues | ✅ | **MIDI-Cue** (`CueType::Midi`, `src/core/midi.rs`): Note On/Off, Control Change, Program Change (Kanal 1–16 in der UI, 0-basiert im Wire-Format), Sofort-Cue. Engine sendet über midir-Output (`src/engine/midi_out.rs`, lazy geöffnet, Gerät via `KUHPLAY_MIDI_OUT_DEVICE`), Fehler als `EngineEvent::Error`. Inspector-Tab „MIDI“ mit Hex-Vorschau, Toolbox-Gruppe „Network“, Web-Remote. MIDI-*Input* (Steuerung) bleibt `src/remote/midi.rs` |
| MIDI File cues | ⬜ | — |
| Timecode cues | 🟡 | Chasing-Trigger pro Cue (Inspector „Triggers"); dedizierter Timecode-Cue-Typ ⬜ |

## Scripting & Automation

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| OSC Dictionary | ⬜ | — |
| OSC Queries | ⬜ | — |
| AppleScript Dictionary | ⬜ | — |
| Script Cues | 🟡 | HTML5-Cues + bidirektionale JS-Bridge (`EvalJs`, `PushJson`, `HtmlDataCue`) decken Scripting ab; dedizierter Script-Cue-Typ ⬜ |
| Parameter Reference | ⬜ | — |
| Examples | ⬜ | — |

## Other Cue Types

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Transport cues (Start/Stop/Pause/Load/Reset) | ✅ | Control-Cues |
| Devamp | ✅ | sofortiges Loop-Ende → Outro |
| GoTo | ✅ | Playhead-Sprung (Target/GoTo) |
| Target | ✅ | Ziel-Cue-Kopplung (Control-Cues, HTML-Data-Cues) |
| Arm/Disarm | ✅ | disarmte Cues werden bei GO übersprungen und in der Liste ausgegraut |
| Wait | ✅ | Dedizierter **Wait-Cue** (`CueType::Wait` + `EngineCommand::Wait`): reine Timer-Unit in beiden Engines, `duration` = Wartezeit (0 = sofort), Pre-/Post-Wait, Pause/Resume/Stop wirken ( Fortschrittsbalken in der Action-Spalte). Pre-Wait/Post-Wait pro Cue bleiben zusätzlich |
| Memo | ✅ | Dedizierter **Memo-Cue** (`CueType::Memo`): kein Engine-Kommando, gilt beim GO sofort als abgeschlossen; Notiz im Notes-Feld (+ Notes des Standby-Cues im Masthead) |

## Tutorials

| Kapitel | Status | Anmerkung |
|---------|--------|-----------|
| Getting Started / Core Concepts / General Reference / Audio / Video / MIDI/Networking/Scripting | ⬜ | Doku geplant |

---

## Bekannte Abweichungen (V1)

1. **Auto-Continue-Kette:** QLab startet den Folge-Cue nach Ablauf des
   aktuellen Cues (inkl. Post-Wait). KuhPlay V1 feuert Auto-Continue
   sofort beim Start und Auto-Follow beim `Completed`.
2. **Show-Format:** KuhPlay speichert `.kuhplay` (JSON), nicht das
   QLab-`.qlab5`-Format. Import/Export von QLab-Shows ist ⬜.
3. **HTML5-WebView:** In der Real-Engine ist HTML5 noch virtuell
   (Events `HtmlLoaded`/`JsExecuted`/`JsonReceived` werden emittiert, aber
   es rendert keine echte WebView). Anbindung `wry` (WebView2/WKWebView/
   webkit2gtk) über das `MediaEngine`-Trait ist vorbereitet. HTML5 ersetzt
   bewusst NDI.
4. **Device-Ausgabe:** Ein Borderless-Fenster pro belegtem Display, exakt
   auf der Display-Rect, zeigt alle Routen des Displays; Routen mit
   Teilbereich liegen an ihrer Stelle. "Immer oben"/richtes Platzieren über
   AppKit (`src/gui/macos_output.rs`), Esc schließt.
5. **Timecode:** Chasing-Logik + Sprünge sind implementiert und getestet;
   das Empfangen externer LTC/MTC-Signale fehlt in V1.
6. **Show-Mode:** deaktiviert Editing (Toolbar, Inspector, Kontextmenü,
   Playhead-Klicks) – wie QLab; Sidebar-Transport (Pause/Resume/Panic)
   bleibt wie in QLab aktiv. **Phase 6:** sofort erkennbar durch 2px-Panic-Rahmen
   + Füllung am Masthead, eingefärbten Footer (`SHOW_MODE_BG`) und „SHOW"-Pill
   (statt „EDIT"). Edit-Funktionen werden wie in QLab 5 nur deaktiviert, nicht
   ausgeblendet. Datei-Drop legt im Show-Mode keine Cues an.
7. **Cue-Tabelle Click-Handling:** egui_extras-Tabellenzellen sind default
   `Sense::hover()`; ohne `TableBuilder::sense(Sense::click())` feuert
   `row.response().clicked()` nie (Regression „Klick auf Cue passiert
   nichts"). KuhPlay setzt `Sense::click()` und rekonstruiert den
   Doppelklick aus dem globalen Pointer-Input (`Response::union` überträgt
   das `double_clicked`-Feld in egui 0.30 nicht). Abgedeckt durch
   `tests/gui_interaction.rs`.

8. **Tastenkürzel-Abweichungen (Phase 5, gegen QLab-5-Handbuch geprüft):**
   - **⌘ → Ctrl auf Linux/Windows:** egui bildet `Modifiers::command` plattformadaptiv ab (⌘ auf macOS, Ctrl sonst) – alle ⌘-Kürzel der Tabelle liegen damit automatisch richtig.
   - **↑/↓ (nackt) = Playhead:** QLab 5 nutzt dafür ⌘↑/⌘↓ („Select previous/next“). KuhPlay behält die nackten Pfeile bei (TODO.md Phase 5 so gewünscht) und bindet zusätzlich ⌘↑/⌘↓ = Move Cue (QLab: ⌥⌘↑/⌥⌘↓, bei KuhPlay beides).
   - **⌘0–⌘9 = KuhPlay-Cue-Typen:** QLab belegt die Ziffern mit Group/Audio/Mic/Video/Camera/Text/Light/Fade/Network/MIDI. KuhPlay hat andere Typen (kein Mic/Camera/Light/MIDI): ⌘0=Group, ⌘1=Audio, ⌘2=Video, ⌘3=Fade, ⌘4=Stop, ⌘5=Script, ⌘6=Control, ⌘7=HTML5, ⌘8=Slideshow, ⌘9=HTML Data. Die später dazugekommenen Typen **Wait, Memo, Text, Network** haben – wie in QLab 5 die überzähligen Typen – **kein ⌘-Ziffern-Kürzel**, sondern Toolbox/Menü/Remote (Test: genau ⌘0–⌘9, jede Ziffer einmal).
   - **⌘D = Duplicate:** QLab 5 nutzt ⌘D für „Delete numbers of selected cues“ (in KuhPlay nicht implementiert). **⌘G = Find Next** (QLab 5), daher hat „Group Cues“ kein Kürzel (Menü/Kontextmenü). **Esc einfach = Panic** (QLab: Doppel-Esc = Hard Stop – in KuhPlay nicht abgebildet). **Reset All** hat kein Standard-Kürzel (QLab ebenfalls nicht) → Cues-Menü. QLab-Editierkürzel der Cue-Felder (N/Q/O/T/E/D/W/C/F) sind nicht belegt – Zellen editiert man per Doppelklick (Inline-Edit).

9. **Phase-6-Feinschliff:** Show-Mode bleibt bei deaktivierten (nicht ausgeblendeten) Edit-Funktionen – QLab 5 blendet im Show-Mode ebenfalls nur aus, wo es das tut; KuhPlay hält die Menüs konsistent sichtbar+deaktiviert. Datei-Drop ist auf Audio/Video/Bild-Endungen beschränkt (andere Endungen erzeugen keinen Cue – QLab 5 würde je nach Datei einen passenden Cue-Typ raten). // TODO(qlab-parity): verifizieren

## Behobene Bugs (TODO.md Phase 0, 2026-10-29)

1. **Cart-Events:** `Finished`/`Stopped`/Auto-Follow erreichten nur den
   aktiven Cart – Cues in anderen Listen blieben „Playing". Fix: UUID-Suche
   über alle Carts (`CartManager::find_cue_any`; `set_status`,
   `set_status_with_follow`, `stop_running`, `pause_running`, `resume_running`
   laufen cross-cart).
2. **Esc-Panic während Texteingabe:** Esc im Notes-Feld/Inspector stoppte die
   ganze Show. Fix: Panic nur ohne Textfeld-Fokus (Textfeld-Registry
   `text_field_ids` + gecachter Gate `text_field_active`, da egui 0.30 den
   Fokus in `begin_pass` bei Esc löscht, bevor Hotkeys laufen).
3. **⌘↑/⌘↓:** nicht implementiert und als plain ↑/↓ (Playhead) fehlgedeutet.
   Fix: Modifier-Prüfung; ⌘↑/⌘↓ = Move Up/Down, Selection folgt dem
   verschobenen Cue.
4. **`last_error` unsichtbar:** Fix: Fehlerzeile im Footer (`⚠ N` +
   Kurztext + „×" zum Schließen); Engine-Fehler landen im Zähler.
5. **Doppelte Q-Nummern:** Fix: `CueList::next_free_number()` (höchste vorhandene
   Nummer + 1, rekursiv inkl. Gruppen-Kinder).
6. **Delete stoppt laufende Cues nicht:** Fix: `ShowApp::delete_cue_at`
   dispatcht `stop_running(id)` vor dem Entfernen; Kontextmenü-Delete läuft
   nur noch über `ShowApp` (`cue_table::draw` → `Option<usize>`).
7. **Space-GO nach Button-Klick:** Fokus blieb auf Buttons/Slidern liegen und
   blockierte Space. Fix: nur Textfelder blockieren (Registry); das
   Space-Key-Event wird konsumiert, damit ein fokussierter Button nicht doppelt
   feuert.
8. **Group-Kinder selektierbar:** auf Phase 3 verschoben (UUID-Selektion) –
   noch offen.

## Roadmap (nächste Schritte)

1. ~~Echte Media-Engine~~ ✅ **erledigt:** Real-Engine produktiv
   (Audio: symphonia+cpal, Video: ffmpeg, Diashow: `image`, Compositor,
   Stages/Screens/Hard Edge/AoI). **Offen:** HTML5-WebView via `wry`
   anbinden (WebView2/WKWebView/webkit2gtk) hinter dem `MediaEngine`-Trait.
2. ~~Workspace-Settings persistieren (Panel-Layout)~~ ✅ **Phase 1 erledigt:** Panel-Sichtbarkeit, aktive Tabs und Settings-Seite persistieren via `UiState` + eframe storage. Offen: Theme-Auswahl.
3. Find / Paste Cue Properties / Auditioning.
4. OSC/MIDI-Netzwerk-Trigger (Inspector „Triggers" erweitern).
5. QLab-Show-Import (`.qlab5` lesen).
6. Hardware-beschleunigtes Video-Decoding (GStreamer) als Alternative zum
   ffmpeg-Subprocess; echtes Vollfullscreen pro Screen (egui-Monitor-Handles).
