# Auftrag: UI-Überarbeitung von KuhPlay (Rust/egui)

Du bist ein erfahrener Rust-Entwickler mit Erfahrung in `egui`/`eframe` und kennst
Show-Control-Software aus der Praxis im Theater- und Veranstaltungsbetrieb. Deine
Aufgabe ist es, die Oberfläche von **KuhPlay** übersichtlicher zu machen und
gleichzeitig so vertraut zu gestalten, dass sich erfahrene Operator **nicht
umgewöhnen müssen**: klare Positionen, branchenübliche Begriffe, Interaktionen,
Tastenkürzel und Farbbedeutungen.

Arbeite **inkrementell in den unten beschriebenen Phasen**. Nach jeder Phase muss
`cargo test` und `cargo clippy --all-targets` grün sein, bevor du weitermachst.

---

## 1. Projektkontext (bitte zuerst lesen)

- Crate `kuhplay`, Rust 2021, GUI mit **egui/eframe 0.30** + `egui_extras` 0.30
  (`TableBuilder` für die Cue-Liste). Keine neuen GUI-Frameworks einführen.
- Relevante Dateien:
  - `src/gui/app.rs` – `ShowApp`, `eframe::App::update`, Panel-Aufbau, Hotkeys (`handle_hotkeys`)
  - `src/gui/masthead.rs` – GO-Button, Standby-Anzeige, Notes, Cue-Typ-Toolbar
  - `src/gui/toolbox.rs` – linke Leiste (dupliziert aktuell die Toolbar)
  - `src/gui/cue_table.rs` – Cue-Liste (Spalten, Klick-/Doppelklick-Logik, Kontextmenü)
  - `src/gui/inspector.rs` – Inspector (Tabs Basics/Triggers, ~1200 Zeilen, ein langes Grid)
  - `src/gui/sidebar.rs` – Reset/Pause/Resume/Panic + Tabs „Lists & Carts", „Active Cues", „Screens"
  - `src/gui/screen_editor.rs` – Screen-Editor (steckt derzeit in der Sidebar)
  - `src/gui/footer.rs` – Edit/Show, Text-Toggles „Toolbox: on" etc., Cue-Anzahl
  - `src/gui/theme.rs` – Farbpalette + Status-Farben
  - `src/gui/icons.rs` – Phosphor-Icons (eingebetteter Font) – **für alle Icons verwenden**
  - `src/gui/menu_bar.rs` – File/Edit/View/Cues/Workspace/Help
  - `ROADMAP.md` – Projektstand
- Tests, die die GUI headless prüfen: `tests/gui_smoke.rs`, `tests/gui_interaction.rs`.
  Sie simulieren Maus-Events. Wenn du Interaktionen bewusst änderst, **passe die
  Tests an und ergänze neue** – lösche keine Tests ersatzlos.
- Code-Stil: Doku-Kommentare und Kommentare **auf Deutsch**, Modul-Header mit
  `//!` erklären das Layout. Halte diesen Stil ein.

### Technische Fallstricke, die du kennen musst

1. **egui-Panel-Reihenfolge ist zwingend**: `top → bottom → side → CentralPanel zuletzt`.
   Bei mehreren Bottom-Panels ist das zuerst hinzugefügte das unterste (Footer vor Inspector).
2. **Tabellen-Klicks**: `egui_extras`-Zellen sind standardmäßig `Sense::hover()`.
   Ohne `TableBuilder::sense(Sense::click())` feuert `row.response().clicked()` nie.
   Doppelklick wird aus dem globalen Pointer-Input rekonstruiert, weil
   `Response::union` in egui 0.30 `double_clicked` nicht überträgt. Nicht kaputt machen.
3. **Borrow-Splitting im Inspector**: `draw_basics` borgt disjunkte Felder von
   `ShowApp` vor `carts.selected_mut()`. Beim Aufteilen in mehrere Tabs/Funktionen
   dieses Muster beibehalten (oder einen kleinen Kontext-Struct mit `&mut`-Feldern einführen).
4. **Engine nicht anfassen**: `src/engine/*`, `src/core/*` und `src/remote/*` nur ändern,
   wenn ein UI-Feature zwingend ein neues Feld braucht (dann `#[serde(default)]`,
   abwärtskompatibel zu bestehenden `.kuhplay`-Dateien).
5. Hotkeys dürfen nicht feuern, solange ein Textfeld Fokus hat (`ctx.wants_keyboard_input()`).

---

## 2. Leitprinzipien

1. **Muskelgedächtnis der Operator vor eigener Kreativität.** Branchenübliche Abläufe
   bleiben erhalten, auch wenn eine andere Lösung „schöner" wäre. Wenn du dir nicht
   sicher bist, markiere die Stelle und dokumentiere sie.
2. **Englische UI-Begriffe, einheitlich** (GO, Standby, Pre Wait, Action,
   Post Wait, Continue, Auto-continue, Auto-follow, Inspector, Basics, Triggers,
   Time & Loops, Audio Levels, Geometry, Lists, Carts, Active Cues, Workspace
   Settings, Panic, Pause All, Resume All, Reset All …).
3. **Weniger, aber klarer**: keine doppelten Bedienelemente, keine Text-Toggles
   wo Icons reichen, keine Funktionen in Panels, wo sie nicht hingehören.
4. **Live-Show-Tauglichkeit**: Während der Show muss auf einen Blick erkennbar sein:
   was steht in Standby, was läuft, wie lange noch, was ist kaputt.
5. **Farben haben Bedeutung, nicht Dekoration** (siehe Phase 2).

---

## 3. Phasen

### Phase 0 – Gefundene Fehler beheben (zuerst!) — ERLEDIGT 2026-11-02

Alle 8 Punkte sind umgesetzt (find_cue_any über alle Carts, Esc-Gate über
text_field_active, ⌘↑/⌘↓ nach shortcuts.rs, last_error im Footer, next_free_number,
stop_running vor dem Löschen, Gruppen-Kinder per UUID selektierbar, Space-Gate
nur Textfelder). Siehe Commits/Review-Blöcke; Regressionstests in tests/.

Diese Fehler wurden beim Code-Review gefunden. Für **jeden** zuerst einen
fehlschlagenden Test schreiben, dann fixen.

1. **Engine-Events landen nur im aktiven Cart** (`src/core/cart.rs`,
   `CartManager::set_status` / `set_status_with_follow` rufen nur
   `active_mut()` auf; `poll_engine_events` in `app.rs` nutzt sie).
   Szenario: Cue in Liste A starten, auf Liste B / einen Cart wechseln, Cue endet
   → `Finished`/`Stopped` wird von der State-Machine abgelehnt, der Cue bleibt
   für immer „Playing", Auto-Follow feuert nicht, Active Cues und Remote-Status
   sind falsch. Fix: Cue per UUID **über alle Carts** suchen (es gibt bereits
   `find_cue`), Status dort setzen und Auto-Follow in der Liste des Cues auslösen.
2. **Esc löst Panic aus, auch während ein Textfeld bearbeitet wird**
   (`handle_hotkeys`: der Esc-Zweig prüft `text_input` nicht). Wer im Notes-Feld
   oder Inspector Esc drückt, um die Eingabe abzubrechen, stoppt die ganze Show.
   Fix: Esc nur dann als Panic werten, wenn kein Textfeld Fokus hat (bzw. das
   das übliche Verhalten übernehmen).
3. **Menü zeigt „Move Up ⌘↑ / Move Down ⌘↓", die Kürzel existieren aber nicht.**
   Schlimmer: ⌘↑/⌘↓ werden vom Pfeiltasten-Handler als normales ↑/↓ verarbeitet
   (Modifier werden ignoriert) und verschieben den **Playhead** statt den Cue.
   Fix: Modifier prüfen, Kürzel implementieren (→ Phase 5, zentrale Shortcut-Tabelle).
4. **`last_error` wird gesetzt, aber nirgends angezeigt.** Fehler beim Laden/
   Speichern einer Show und Engine-Fehler (`EngineEvent::Error`) sind für den
   Nutzer unsichtbar. Fix: nicht-blockierende Fehleranzeige (Toast oder
   Fehlerzeile im Footer mit Schließen-Button), Fehler auch im Warnungs-Zähler.
5. **`add_cue` erzeugt doppelte Cue-Nummern**: Nummer = `list.len() + 1`.
   Beispiel: Cues 1, 2, 3 → Cue 2 löschen → neuer Cue bekommt „3" (doppelt).
   Vergeben wird die nächste freie Nummer nach dem selektierten Cue (bzw. höchste
   numerische Nummer + Increment). Fix: eindeutige Nummernvergabe, Test dazu.
6. **Löschen eines laufenden Cues stoppt ihn nicht** (`delete_selected` in
   `app.rs` und „Delete Cue" im Kontextmenü von `cue_table.rs` entfernen nur aus
   der Liste). Die Engine spielt weiter, der Cue ist nicht mehr steuerbar (nur
   noch Panic). Fix: vorher `stop_running(id)` dispatchen – und dafür das Löschen
   **nur** über `ShowApp` laufen lassen (siehe Struktur-Punkt „Aktionen" unten).
7. **Group-Kinder sind nicht selektierbar**: Klick auf eine Kind-Zeile selektiert
   die Gruppe. Jeder Cue kann in einer Gruppe selektiert und im Inspector
   bearbeitet werden. Fix im Zuge von Phase 3 (Selektion über Cue-UUID statt Index).
8. **Zu prüfen (nicht verifiziert)**: `handle_hotkeys` blockiert Space-GO, solange
   irgendein Widget Fokus hat (`m.focused().is_some()`). Prüfe, ob nach einem
   Mausklick auf GO, einen Sidebar-Button o. ä. der Fokus auf dem Button bleibt –
   dann funktioniert Space danach nicht mehr, was im Live-Betrieb fatal wäre.
   Falls ja: nur Textfelder blockieren lassen, und Buttons nicht per Space auslösbar
   machen, wenn das globale GO gemeint ist. Test dazu schreiben.

### Phase 0b – Code-Struktur & Wartbarkeit

Diese Punkte beim Umbau mit erledigen (nicht als separates Big-Bang-Refactoring,
sondern jeweils in der Phase, die die Datei ohnehin anfasst):

- **`ShowApp` ist ein God-Object** (`src/gui/app.rs`, ~1400 Zeilen, 65 `pub`-
  Elemente, alle Felder öffentlich). Aufteilen in:
  - `src/gui/state.rs` – reiner UI-Zustand (Panel-Sichtbarkeit, Tabs, Fenster,
    Spaltenbreiten) als eigener, serde-fähiger Struct `UiState` (→ Persistenz).
  - `src/gui/actions.rs` – ein `enum UiAction` (Go, Panic, AddCue(CueType),
    DeleteSelected, MoveSelected(i32), SetArmed, SelectCue(Uuid), SetPlayhead(Uuid),
    ToggleInspector, …). Panels **geben Actions zurück bzw. pushen sie in eine
    Queue** statt `ShowApp` direkt zu mutieren; `ShowApp::apply(action)` ist die
    einzige Stelle mit Seiteneffekten. Damit verschwinden die doppelten
    Codepfade (z. B. Arm/Disarm in `cue_table.rs` *und* `menu_bar.rs` *und*
    `app.rs`; Delete in `cue_table.rs` *und* `app.rs`), Shortcut, Menü,
    Kontextmenü und Remote lösen exakt dieselbe Logik aus, und GUI-Tests können
    Actions direkt prüfen.
  - Felder privat machen, gezielte Getter/Setter; Tests über die öffentliche API.
- **Remote-Server-Start gehört nicht in die GUI**: `RemoteServers` (inkl. tokio-
  Runtime, MIDI-Guard) aus `app.rs` nach `src/remote/` verschieben; die App bekommt
  nur ein Handle mit `poll_commands()` und `broadcast(event)`.
- **`demo_cues()` aus `app.rs`** nach `src/core/demo.rs` (oder `src/gui/demo.rs`)
  verschieben.
- **Doppelte Status-Farblogik**: `cue_table::status_color` und
  `theme::status_color` haben unterschiedliche Semantik. Eine einzige Quelle in
  `theme.rs` (→ Phase 2).
- **Copy-Paste im Inspector**: Pre-Wait/Duration/Post-Wait sind je ~12 Zeilen
  identischer DragValue-Code. Einen Helfer `duration_field(ui, &mut Duration)`
  schreiben (mit `mm:ss.ss`-Format), ebenso für Combo-Boxen über `ALL`-Arrays
  (`enum_combo(ui, id, &mut value, &[…])`). `draw_basics` ist eine einzige
  Funktion über mehrere hundert Zeilen → in Phase 4 aufteilen.
- **`theme::apply_theme` läuft jeden Frame** (`set_visuals` + Font-Check in
  `update`). Einmalig in `ShowApp::new` bzw. über `eframe::CreationContext`
  setzen.
- **Dauer-Repaint**: `request_repaint_after(50ms)` hält die App auch im Leerlauf
  auf ~20 fps (unnötige CPU-Last auf Show-Rechnern). Nur dann eng takten, wenn
  Cues laufen / Pre-/Post-Wait aktiv sind, sonst z. B. 500 ms (Uhr) – Engine- und
  Remote-Events können `ctx.request_repaint()` auslösen.
- **Gemischte UI-Sprache**: Die Oberfläche ist Englisch , einzelne
  Strings sind Deutsch (`"— keine —"` im Inspector, `"Noch keine Screens…"`,
  `"Keine Displays erkannt"`, `"keine Stage zugeordnet"` im Screen-Editor).
  Alle sichtbaren UI-Strings auf Englisch vereinheitlichen; Code-Kommentare
  bleiben Deutsch.
- **Magic Numbers** (Spaltenbreiten, Button-Größen `vec2(38.0, 34.0)`, Abstände)
  als benannte Konstanten in `theme.rs` bündeln.
- **Selektion/Playhead über Indizes** (`selected_index`, `cursor: usize`) bricht
  bei Gruppen-Kindern und wird bei Drag & Drop/Mehrfachauswahl fehleranfällig.
  Für die UI auf `Uuid`-basierte Selektion (`HashSet<Uuid>` + Anker) umstellen;
  der Index-Playhead im Core darf bleiben, wenn die Umrechnung zentral passiert.

### Phase 1 – Workspace-Grundlayout bereinigen

Ziel: klares Workspace-Layout.

```
┌──────────────────────────────────────────────────────────────────────┐
│ Menüleiste                                                           │
├──────────────────────────────────────────────────────────────────────┤
│ [  GO  ]  Q 3 · Musik Einlass                       (Show-Uhr/Status) │  ← Masthead
│           [ Notes des Standby-Cues …                               ] │
├──────┬────────────────────────────────────────────────┬──────────────┤
│Tool- │  Cue-Liste                                     │ Lists │Carts │
│box   │                                                │ Active Cues  │
│(opt.)│                                                │              │
├──────┴────────────────────────────────────────────────┴──────────────┤
│ Inspector: [Cue-Header]  Basics | Triggers | <typ-spezifische Tabs>  │
├──────────────────────────────────────────────────────────────────────┤
│ Edit|Show   ⚠ 2      12 cues in 2 lists       [Sidebar][Insp.][⚙]   │  ← Footer
└──────────────────────────────────────────────────────────────────────┘
```

Konkret:
- **Doppelte Cue-Typ-Buttons entfernen**: Masthead-Toolbar und Toolbox zeigen
  heute dieselben Buttons. Cues legt man über die **Toolbox (links,
  ein-/ausblendbar)**, das Menü „Cues", Tastenkürzel oder Drag & Drop an. Mach die
  Toolbox zur einzigen Button-Quelle: Icons **mit Beschriftung** (Typ-Name),
  gruppiert (Audio/Video/…, Fade, Control-Cues wie Start/Stop/Pause/GoTo/Arm/Disarm,
  Group, Memo/Wait sofern vorhanden). Breite etwa 140 px, einklappbar.
  Die Masthead-Toolbar entfällt – der Masthead wird dadurch kompakter.
- **Masthead**: GO-Button links (groß, mindestens 120×64), rechts daneben Standby
  „Q-Nummer · Name" groß und gut lesbar, darunter das Notes-Feld über die volle
  restliche Breite (nicht fix 420 px). Ganz rechts optional eine schlichte
  Uhrzeit/Laufzeit-Anzeige. Audition-Badge bleibt, aber als dezentes Pill-Label.
- **Footer**: links Edit/Show-Segmentschalter; daneben ein
  **Warnungs-Indikator** (Anzahl kaputter Cues, Klick springt zum ersten);
  Mitte „N cues in M lists"; rechts **Icon-Toggle-Buttons** für Sidebar und
  Inspector sowie ein **Zahnrad** für „Workspace Settings". Die Text-Buttons
  „Toolbox: on/off" usw. ersetzen (Toolbox-Toggle nur noch über View-Menü + Kürzel).
- **Screens aus der Sidebar entfernen**: Screens/Stages liegen in den
  **Workspace Settings → Video**. Lege ein eigenes `egui::Window` (oder
  Viewport) „Workspace Settings" an, mit linker Navigationsliste
  (General, Audio, Video → Stages/Screens, Network, MIDI, Key Map/Remotes)
  und verschiebe `screen_editor` dorthin. Der Editor bekommt dort deutlich mehr
  Platz. Seiten, die es noch nicht gibt, zeigen einen Platzhalter.
- **Sidebar** (rechts) hat nur noch die Tabs **„Lists" / „Carts" / „Active Cues"**
  (bzw. „Lists & Carts" + „Active Cues").
  Die globalen Transport-Buttons (Reset/Pause All/Resume All/Panic) als kompakte
  Icon-Leiste mit Tooltips und Shortcuts-Hinweis; Panic klar rot, aber nicht dominanter als GO.
- **Layout-Persistenz**: Sichtbarkeit von Toolbox/Sidebar/Inspector, Panel-Breiten
  und -Höhen, aktive Tabs über `eframe`-Persistenz (`App::save` / `storage`)
  speichern. Fehlende Werte → Defaults.

### Phase 2 – Farben und Status-Semantik

Aktuell wird ein armierter Cue in Standby **rot** dargestellt (`theme::status_color`).
Rot soll aber **„kaputt / Problem"** bedeuten – das ist für Operator
irreführend. Neue Semantik:

| Zustand | Darstellung |
|---|---|
| Idle / bereit (armed) | neutral, keine Statusfarbe |
| Disarmed | ausgegraut, kursiv oder gedimmt (wie heute) |
| Playhead / Standby | Playhead-Pfeil in der Statusspalte + gut sichtbarer Rahmen/Hintergrund der Zeile |
| Pre Wait läuft | Pre-Wait-Zelle zählt sichtbar herunter, Status-Icon orange/gelb |
| Running | grünes Play-Icon, Fortschrittsbalken in der Action-Spalte |
| Paused | Pause-Icon (gelb/blau), Fortschritt eingefroren |
| Post Wait | Fortschrittsbalken in der Post-Wait-Zelle |
| **Broken** (fehlende Datei, fehlendes Target, ungültige Stage) | **rotes ✕-Icon + Tooltip mit Grund**, im Footer gezählt |
| Selektiert | blaue Zeilenhervorhebung, unabhängig vom Playhead |

- Füge eine reine UI-Funktion `cue_problems(&Cue, &ShowApp) -> Vec<String>`
  (oder ähnlich) hinzu, die Broken-Gründe ermittelt (Datei existiert nicht,
  Target-UUID nicht gefunden, Stage unbekannt). Ergebnis cachen, nicht jeden Frame
  das Dateisystem abfragen (z. B. Neuberechnung bei Änderung oder alle 2 s).
- **Cue-Farben** (`Cue.color`): als **Tönung der ganzen Zeile**
  (niedrige Deckkraft) plus farbiger Streifen am linken Rand – nicht nur als Punkt
  in der Name-Spalte.
- Palette in `theme.rs` aufräumen; alle Farben als benannte Konstanten,
  ausreichender Kontrast (WCAG AA für Text auf Hintergrund).

### Phase 3 – Cue-Liste (wichtigster Bereich)

- **Spalten**: Status | Type | Number | Name | Target | Pre Wait |
  Action (Duration) | Post Wait | Continue. Die Spaltenüberschrift „Duration"
  in „Action" umbenennen (prüfen).
- Spalten **in der Breite veränderbar** (`Column::initial(..).resizable(true)`),
  Breiten persistieren; über Rechtsklick auf den Header ein-/ausblendbar.
- **Live-Zeiten**: laufende Cues zeigen in der Action-Spalte die **verbleibende
  Zeit** und einen Fortschrittsbalken als Zellenhintergrund; Pre/Post Wait zählen
  in ihrer Zelle herunter. Daten: vorhandene `elapsed_of()`/Engine-Positionen.
- **Interaktion vereinheitlichen** (das ist eine bewusste Verhaltensänderung –
  Tests anpassen):
  - Einfachklick = selektieren (bleibt).
  - **Doppelklick auf Number/Name/Pre Wait/Duration/Post Wait = Inline-Bearbeitung
    dieser Zelle** (Enter übernimmt, Esc bricht ab). Heute toggelt Doppelklick
    „armed" – das ist nicht üblich. Arm/Disarm über Inspector, Kontextmenü
    und Tastenkürzel.
  - Playhead setzen: Klick in die Statusspalte (bleibt) **und** Tastatur.
  - **Mehrfachauswahl** mit ⌘-Klick und ⇧-Klick (für Löschen, Verschieben, Gruppieren).
  - **Drag & Drop** zum Umsortieren; Drag auf eine Gruppe = hineinlegen.
  - **Group-Cues** mit Aufklapp-Dreieck (ein-/ausklappen), Kinder eingerückt.
  - Kontextmenü um „Group selected cues", „Duplicate", „Arm/Disarm",
    „Set Color ▸" erweitern.
- **Auto-Scroll**: Bei GO/Playhead-Wechsel scrollt die Liste so, dass der
  Standby-Cue sichtbar bleibt (`scroll_to_row`).
- **Continue-Spalte**: Icons (Auto-continue / Auto-follow) statt Text,
  und zwischen verketteten Zeilen ein dezenter Verbindungs-Indikator.
- Zeilenhöhe so wählen, dass ca. 25–30 Zeilen auf einen 1080p-Bildschirm passen,
  Zahlen in den Zeit-Spalten mit **Monospace/tabular figures** und rechtsbündig,
  damit herunterzählende Zeiten nicht springen.

### Phase 4 – Inspector neu strukturieren

Heute ist „Basics" ein einziges, langes, scrollendes Grid mit allen
typ-spezifischen Feldern – das ist der größte Übersichtlichkeits-Verlust.

- **Kopfzeile** über den Tabs: Typ-Icon, Cue-Nummer und Name des selektierten Cues
  (bei Mehrfachauswahl „N cues selected").
- **Typabhängige Tabs**, z. B.:
  - alle: **Basics**, **Triggers**
  - Audio: **Time & Loops** (Start/End, Loops, Slices, Waveform groß),
    **Audio Levels** (Routing/Kanäle/Pan), ggf. **Audio Trim/Effects**
  - Video: **Time & Loops**, **Geometry** (Stage, Layer, Position/Größe, Opacity,
    Area of Interest, Hard Edge), **Audio Levels** falls Ton
  - Fade: **Curve**, **Audio Levels**/**Geometry** je nach Ziel
  - HTML5/Slideshow/Data: eigene Tabs, die die heutigen Felder sinnvoll gruppieren
  - Control-Cues: Target-Auswahl prominent im Basics-Tab
  Lege die Tab-Liste pro Cue-Typ fest; Tabs, die es in
  KuhPlay (noch) nicht gibt, weglassen statt leer anzeigen.
- **Basics zweispaltig**: links Number, Name, Target (mit Dateiname/
  Target-Cue + „…"-Button), Pre Wait, Duration, Post Wait, Continue; rechts Color
  und ein großes **Notes**-Feld (mehrzeilig). Armed als Checkbox.
- Zeitfelder im Format `mm:ss.ss` eingeben/anzeigen (wie die Liste), nicht als
  nackte Sekunden-DragValue; DragValue-Ziehen darf bleiben.
- `inspector.rs` in ein Modul `src/gui/inspector/` aufteilen (`mod.rs`, `basics.rs`,
  `triggers.rs`, `time_loops.rs`, `geometry.rs`, `audio_levels.rs`, …).
  Keine Datei über ~400 Zeilen.
- Show-Mode: Inspector bleibt sichtbar, aber read-only (wie heute).

### Phase 5 – Tastenkürzel & Menüs

- Alle Shortcuts in **einer zentralen Tabelle** (z. B. `src/gui/shortcuts.rs`:
  `struct Shortcut { action, key, modifiers, label }`) definieren; Menüeinträge
  zeigen ihren Shortcut aus dieser Tabelle an, `handle_hotkeys` liest ebenfalls
  daraus. So sind Menü und Verhalten nie inkonsistent.
- Belegung vereinheitlichen. Mindestens: Space = GO, Esc = Panic/Stop All,
  ↑/↓ = Selektion/Playhead bewegen, ⌘-Zahlen für neue Cue-Typen, Pause All /
  Resume All / Reset All, ⌘I Inspector?, Toolbox, Sidebar, Find (⌘F),
  Delete/⌫ = Cue löschen (nur Edit-Mode), ⌘D Duplicate, ⌘G Group.
  **Lege die genaue Belegung zentral fest**
  und weiche nur ab, wenn es auf Linux/Windows nicht anders geht (⌘ → Ctrl).
  Abweichungen in `docs/BACKLOG.md` dokumentieren.
- Ein „Keyboard Shortcuts"-Fenster unter Help, das die Tabelle anzeigt.
- **Find** (⌘F): kleine Suchleiste über der Cue-Liste, filtert/markiert nach
  Nummer, Name, Notes; Enter springt zum nächsten Treffer.

### Phase 6 – Show-Mode & Feinschliff

- Show-Mode muss **sofort erkennbar** sein: z. B. farbiger Rahmen um den Masthead
  oder eingefärbter Footer + „SHOW" Label; Edit-Funktionen ausgeblendet statt nur
  deaktiviert, wo das zur Sicherheit nötig ist.
- **Active Cues**-Tab: pro laufendem Cue Typ-Icon, Nummer/Name, Fortschrittsbalken,
  elapsed / remaining, Buttons Pause/Resume/Stop – gut klickbar (min. 28 px hoch).
- Leerzustände: leere Cue-Liste zeigt einen Hinweis („Drag files here or use the
  Toolbox to add cues"); Inspector ohne Auswahl zeigt „No cue selected" zentriert.
- **Drag & Drop von Dateien** aus dem Finder in die Cue-Liste legt passende Cues an
  (Audio-/Video-/Bild-Endungen) – `ctx.input(|i| i.raw.dropped_files)`.
- Einheitliche Abstände: definiere in `theme.rs` Konstanten für Spacing
  (z. B. 4/8/12/16 px) und verwende nur diese.
- Tooltips für jedes Icon-only-Element.

---

## 4. Was du NICHT tun sollst

- Keine Änderungen an Engine-, Remote- oder Persistenz-Logik ohne zwingenden Grund.
- Keine neuen Abhängigkeiten außer, wenn unvermeidbar, kleine egui-Ökosystem-Crates
  (vorher begründen). Kein Wechsel auf eine andere egui-Version.
- Keine erfundenen Features oder Begriffe. Keine eigenen „Verbesserungen",
  die erfahrene Operator umlernen müssten.
- Keine Tests löschen, um grün zu werden.
- Nicht alles in einem riesigen Commit/Diff – pro Phase ein abgeschlossener Stand.

---

## 5. Abnahmekriterien

Pro Phase:
1. `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` sind grün
   (auch `cargo check --features webview`).
2. Neue/angepasste GUI-Tests in `tests/gui_interaction.rs` bzw. `tests/gui_smoke.rs`
   für jede geänderte Interaktion (z. B. Doppelklick = Inline-Edit, Mehrfachauswahl,
   Drag-Reorder, Broken-Cue-Erkennung, Shortcut-Tabelle ↔ Menü).
3. `docs/BACKLOG.md` aktualisiert (GUI-Mapping-Tabelle, Kapitel „The Workspace",
   „The Inspector", „Keyboard Shortcuts", bekannte Abweichungen) und `ROADMAP.md`
   ergänzt.
4. Kurzer Bericht: was geändert wurde, welches Verhalten du verifiziert
   hast und welches offen ist.

Gesamtziel: Ein erfahrener Operator öffnet KuhPlay, findet GO, Standby, Cue-Liste,
Inspector-Tabs, Sidebar und Shortcuts dort, wo er sie erwartet, und sieht während
einer Show sofort, was läuft, was als Nächstes kommt und was kaputt ist.

Beginne mit Phase 0 (Bugfixes). Lies vorher die genannten Dateien vollständig und fasse in
3–5 Sätzen zusammen, wie du Phase 0 umsetzen willst, bevor du Code schreibst.
