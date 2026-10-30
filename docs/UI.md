# KuhPlay – UI-Richtlinie

**Vorbild:** QLab 5 (Workspace-Fenster). **Ziel:** übersichtlich. Ein
Operator muss in einer dunklen Regie in **unter einer Sekunde** sehen:
*Was kommt als Nächstes? Was läuft gerade? Ist etwas kaputt?*

Diese Datei ist verbindlich. Neue UI-Elemente richten sich danach; wer
davon abweicht, ändert zuerst dieses Dokument.

---

## 1. Grundprinzipien

1. **Ruhe vor Information.** Standardzustand = neutral und leise. Farbe
   bekommt nur, was Aufmerksamkeit verdient (läuft, wartet, kaputt,
   Selektion, GO/Panic).
2. **Eine Sache, ein Ort.** Jede Funktion gibt es genau einmal sichtbar.
   (Cue anlegen → Toolbox. Transport → Masthead. Eigenschaften →
   Inspector. Status der App → Footer.)
3. **Leere Werte treten zurück.** `00:00.000`, „–", fehlende Targets
   werden gedimmt dargestellt, damit echte Werte herausstechen.
4. **Flächen statt Linien.** Bereiche werden durch unterschiedliche
   Hintergrundtöne getrennt, nicht durch dicke Rahmen. Linien sind 1 px
   und kaum sichtbar.
5. **Feste Raster.** Abstände nur 4 / 8 / 12 / 16 px. Zeilenhöhen fest.
6. **Nichts überlappt.** Icon und Text haben jeweils ihren eigenen Platz.
7. **Leerzustände sind explizit.** Ein leeres Panel sagt *einmal*, warum
   es leer ist – keine leeren Tabs, keine doppelten Hinweise.

---

## 2. Layout

Auf macOS liegt das Menü in der System-Menüleiste (`gui/native_menu.rs`), die
Zeile „Menüleiste" unten entfällt dort. Echte Menü-Kürzel gibt es nur für
`⌘`-Kombinationen, die in Textfeldern nichts bedeuten; Space, Esc, `⌘⌫` und
`⌘↑` laufen über `handle_hotkeys` und stehen als Text im Eintrag.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ Menüleiste                                                               │
├──────────────────────────────────────────────────────────────────────────┤
│ MASTHEAD                                                                 │
│ ┌──────┐  STANDBY                                   [⏸] [▶] [↺] [■ Panic]│
│ │  GO  │  1   House to Half                                              │
│ └──────┘  [ Notes … ]                                                    │
├─────────┬──────────────────────────────────────────────┬─────────────────┤
│ TOOLBOX │ CUE-LISTE                                    │ SIDEBAR         │
│ Media   │      Q   Name          Target  Pre  Act  Post│ Lists | Active  │
│ ▸ Audio │ ▶ ♫  1   House to Half   –     …    …    …   │ ▸ Cart 1   11   │
│ ▸ Video │   ▣  2   Intro Video  intro… …    …    …   │                 │
│ …       │                                              │                 │
├─────────┴──────────────────────────────────────────────┴─────────────────┤
│ INSPECTOR   ♫ 3 · Welcome Jingle                                         │
│ Basics  Triggers  Time & Loops  Audio Levels                             │
│ ──────                                                                   │
│ (Inhalt)                                                                 │
├──────────────────────────────────────────────────────────────────────────┤
│ FOOTER  Edit|Show  ⚠ 5   11 cues in 1 list            [▯] [≡] │ [⚙]      │
└──────────────────────────────────────────────────────────────────────────┘
```

* Der **Footer ist immer die unterste Zeile** des Fensters – unter dem
  Inspector, nie dazwischen.
* Der **Inspector** sitzt unter Toolbox/Liste/Sidebar und geht über die
  volle Breite (QLab 5).
* Toolbox (links) und Sidebar (rechts) sind optional (⌘K / ⌘L).
* Masthead und Footer haben eine **feste Höhe**, Inspector ist in der
  Höhe ziehbar (Default 260 px).

### Panel-Reihenfolge (egui)

`menu_bar (top) → masthead (top) → footer (bottom) → inspector (bottom)
→ toolbox (left) → sidebar (right) → cue list (central)`.
Bei egui ist das zuerst hinzugefügte Bottom-Panel das unterste – deshalb
**Footer vor Inspector**. Die Reihenfolge steht genau einmal im Code:
`ShowApp::draw_workspace`.

---

## 3. Farben

Die App ist **immer dunkel**, unabhängig vom Erscheinungsbild des Systems.

Alle Farben stehen in `src/gui/theme.rs` (`palette`). Keine Farbliterale
in Panel-Modulen.

### Flächen (dunkel → hell)

| Token          | Verwendung                                   | RGB          |
|----------------|----------------------------------------------|--------------|
| `BACKGROUND`   | Fenster, Eingabefelder                       | 18, 18, 21   |
| `LIST_BG`      | Cue-Liste (dunkelste große Fläche = Fokus)   | 22, 22, 25   |
| `ROW_STRIPE`   | jede zweite Cue-Zeile                        | 27, 27, 31   |
| `PANEL`        | Masthead, Toolbox, Sidebar, Inspector, Footer| 33, 33, 38   |
| `RAISED`       | Buttons, Tabs (inaktiv), Karten              | 45, 45, 52   |
| `BORDER`       | 1-px-Trennlinien                             | 52, 52, 60   |

### Text

| Token        | Verwendung                                      |
|--------------|-------------------------------------------------|
| `TEXT`       | Primärtext (Namen, Werte ≠ 0)                   |
| `TEXT_DIM`   | Sekundärtext: Spaltenköpfe, Labels, Targets     |
| `TEXT_FAINT` | leere Werte (`00:00.000`, `–`), deaktiviert     |

`TEXT` und `TEXT_DIM` erfüllen WCAG AA (≥ 4,5 : 1) auf `PANEL`
(Unit-Test). `TEXT_FAINT` ist bewusst schwächer – nur für „nichts da".

### Semantische Farben (QLab-Bedeutung)

| Farbe     | Bedeutung                       | Wo                                 |
|-----------|---------------------------------|------------------------------------|
| Grün      | läuft / GO bereit               | GO-Rahmen, Status, Fortschritt     |
| Amber     | Pre-Wait (wartet)               | Status, Countdown                  |
| Blau      | pausiert                        | Status, Fortschritt                |
| Türkis    | Post-Wait                       | Status, Countdown                  |
| Rot       | **kaputt** oder **Panic** – sonst nie | ✕ in der Liste, Footer-⚠, Panic |
| QLab-Blau | Selektion                       | selektierte Zeile                  |
| Akzent (Gelb) | aktiver Toggle / Badge      | Footer-Toggles, Audition-Badge     |

* **Standby ist neutral**, nicht rot. Rot heißt in QLab „kaputt".
* Transparenz nur über `gamma_multiply` (siehe `theme::progress_fill`).
* Cue-Farbe des Benutzers = 3-px-Streifen links + sehr leichte Tönung der
  Zeile. Nie Textfarbe.

---

## 4. Typografie

| Rolle                         | Größe | Stil                    |
|-------------------------------|-------|-------------------------|
| GO-Button                     | 30    | fett, weiß              |
| Standby-Name (Masthead)       | 22    | fett                    |
| Standby-Nummer (Masthead)     | 22    | Monospace, `TEXT_DIM`   |
| Fließtext, Cue-Namen          | 13.5  | normal                  |
| Zeiten / Q# in der Liste      | 13    | Monospace, rechtsbündig (Zeiten) |
| Spaltenköpfe, Gruppen-Labels  | 11    | `TEXT_DIM`, GROSSBUCHSTABEN bei Gruppen-Labels |

Zeiten sind immer Monospace, damit herunterzählende Werte nicht springen.

---

## 5. Bereiche im Detail

### 5.1 Masthead (feste Höhe 108 px, Fläche `PANEL`)

* **GO** links: 132 × 64 px, Radius 8. Grüner 3-px-Rahmen, wenn ein Cue
  bereitsteht, sonst 1 px grau. Space = GO.
* Daneben **Standby**: kleines Label „STANDBY" (11, `TEXT_DIM`), darunter
  Nummer (Monospace, gedimmt) + Name groß. Ohne Standby: „No cue standing
  by" gedimmt.
* Darunter **Notes** des Standby-Cues: sichtbares Eingabefeld (Fläche
  `BACKGROUND`), volle Breite bis zum Transport-Block.
* **Transport rechts**, gleich hoch ausgerichtet: Pause All, Resume All,
  Reset (Icon-Buttons 32 px, neutral weiß) und **Panic** als beschrifteter
  Button mit rotem Rahmen. Esc = Panic. GO und Panic sind die beiden
  einzigen „lauten" Buttons im Fenster.
* Audition-Badge: Pill rechts vom Standby-Namen, nur wenn aktiv.
* Show-Mode: roter 2-px-Rahmen um den Masthead.

### 5.2 Cue-Liste (Fläche `LIST_BG`)

Spalten (QLab): **Status · Type · Q · Name · Target · Pre-wait · Action ·
Post-wait · Continue**.

* Zeilenhöhe **28 px**, Kopfzeile 24 px. Zebra-Streifen `ROW_STRIPE`.
* **Keine sichtbaren Spaltentrenner** im Ruhezustand – nur beim Hover über
  der Grenze erscheint die Resize-Linie.
* Kopfzeile: 11 px, `TEXT_DIM`, darunter eine 1-px-Linie `BORDER`.
* **Status-Spalte**: Playhead-Pfeil (weiß) auf der Standby-Zeile;
  Status-Icon in Statusfarbe bei aktiven Cues; **kaputt = rotes ✕ ohne
  Hintergrund-Kasten** (Tooltip nennt den Grund).
* **Type**: Icon 16 px, `TEXT_DIM`. Gruppen mit Aufklapp-Dreieck.
* **Q**: Monospace, fett.
* **Name**: Primärtext; Gruppen-Kinder um 20 px eingerückt.
* **Target**: gleiche Schriftgröße wie der Rest, `TEXT_DIM`; leer = „–"
  in `TEXT_FAINT`.
* **Zeiten**: Monospace, rechtsbündig. **Wert 0 → `TEXT_FAINT`**.
  Laufend: Restzeit in Statusfarbe + dezenter Fortschrittsbalken dahinter
  (`theme::progress_fill`), der Text bleibt lesbar.
* **Continue**: Icon nur bei Auto-Continue / Auto-Follow; „Do not
  continue" = leere Zelle.
* Selektion: ganze Zeile `SELECTION`. Playhead-Zeile: `CURSOR_ROW`.
* Disarmte Cues: gesamte Zeile in `TEXT_FAINT`.

### 5.3 Toolbox (links, 136 px, Fläche `PANEL`)

* Gruppen-Label (MEDIA, FADE, CONTROL, DATA) 11 px, `TEXT_DIM`.
* Jeder Cue-Typ ist eine **Listenzeile** (26 px): Icon 16 px links mit
  eigenem Platz, 8 px Abstand, Label. Kein Button-Kasten; Hover hellt die
  Zeile auf. Keine Trennlinien zwischen Gruppen, nur 12 px Abstand.
* Show-Mode: alle Zeilen gedimmt, nicht klickbar.

### 5.4 Sidebar (rechts, 240 px, Fläche `PANEL`)

* Oben eine **Tab-Leiste** (siehe 6.2): „Lists & Carts" | „Active Cues (N)".
* Lists & Carts: Listenzeilen „Name … Anzahl" (Anzahl rechtsbündig,
  gedimmt), aktive Liste = Selektion. Darunter dezent „+ New Cart".
* Active Cues: Karte pro laufendem Cue (44 px): Statusstreifen links,
  Fortschritt im Hintergrund, Pause/Resume links, Stop rechts, Name +
  Restzeit „−MM:SS.mmm" in Monospace.
* Leer: „No active cues" gedimmt, zentriert.

### 5.5 Inspector (unten, volle Breite, Fläche `PANEL`)

* **Ohne Selektion:** nur der zentrierte Hinweis „No cue selected".
  Keine Kopfzeile, keine Tabs.
* **Mit Selektion:** Kopfzeile (Icon + „Q · Name", 15 px fett), darunter
  die Tab-Leiste mit den für den Typ relevanten Tabs (siehe
  `inspector::tabs_for`), dann der Inhalt.
* **Basics dreispaltig**, damit bei Default-Höhe nichts wegscrollt:
  links Q#/Name/Target/Datei, Mitte Zeiten/Continue, rechts
  Type+Armed/Color/Notes.
* **Geometry zweispaltig:** links Stage/Layer, rechts Lage des Cues. Mode wie in
  QLab: Fit (Standard), Fill, Stretch, Custom (eigene Position und Größe).
* **Slides zweispaltig:** links Slide-Liste, rechts Crossfade/Optionen.
* Alle Felder folgen §6.4.
* Die Höhe hängt nicht vom Inhalt ab (nur vom Ziehen): Beim Wechsel von Cue
  oder Tab bleibt die Cue-Liste stehen; langer Inhalt scrollt im Inspector.
* Show-Mode: sichtbar, aber read-only.

### 5.6 Footer (unterste Zeile, 28 px, Fläche `PANEL`)

* Links: Segment „Edit | Show".
* Daneben: rotes Warn-Icon + „N broken" nur wenn Cues kaputt sind – Klick springt zum
  ersten kaputten Cue. Engine-Fehler als gekürzte rote Zeile mit ×.
* Mitte: „N cues in M lists" gedimmt.
* Rechts: Toggles Sidebar / Inspector (aktiv = Akzentfarbe), Trenner,
  Zahnrad (Workspace Settings).
* Show-Mode: Footer-Fläche dunkelrot + „SHOW"-Pill.

### 5.7 Workspace Settings (Fenster)

* Öffnet mittig über den Panels; Inhalt nie breiter als das Fenster.
* Links Navigation (136 px, Listenzeilen wie Toolbox), rechts die Seite.
* Bereichs-Überschriften als kleines Label (`theme::section_label`).
* Audio: Dropdown mit allen Ausgabegeräten, wird mit der Show gespeichert und
  ist gesperrt, solange Cues laufen oder Show Mode aktiv ist.
* Vollbild-Ausgabe: Esc im Ausgabefenster schließt es. Es liegt über allem, auch
  über der App und der Menüleiste.
* Audio-Kanäle: Patch-Matrix mit 16 benannten Kanälen (Zeilen) × (Gerät,
  Ausgang) (Spalten: System Default + jedes verbundene Ausgabegerät, max. 16
  Ausgänge pro Gerät). Checkbox = Kanal auf dieses Ziel gepatcht; ein Kanal
  kann auf mehrere Ausgänge und mehrere Geräte gleichzeitig gehen. Der
  Signalweg ist Cue (Tab „Audio Levels", Matrix) → Kanal → Ausgänge/Geräte.
  Umlegen wirkt sofort, auch live; nur Show Mode sperrt es. Zuordnungen auf
  nicht verbundene Geräte bleiben in der Show erhalten (Hinweis darunter).
* Network: PIN der Web-Remote anzeigen und ändern (4 bis 8 Ziffern).
* Video (QLab 5 „Workspace → Video“) mit drei Reitern:
  - **Video Outputs (Stages):** Tabelle pro Stage (Name, Auflösung, Devices,
    Edit, Monitor, Delete). „New Stage“-Menü: leere Stage (1920×1080), Stage
    mit Ausgabe (neue Route + Full-Canvas-Region) oder Multi-Output-Stage
    (Anzahl/Auflösung/Überlappung der Projektoren; erzeugt je Projektor eine
    Route und eine Stage mit überlappenden Regionen). Löschen nur, wenn kein
    Cue die Stage nutzt.
  - **Output Routing:** Tabelle der Routen (logische Ausgänge): Name,
    Auflösung in echten Pixeln, Device (Display oder Blackmagic), Testbild,
    Farbe, Edit, Delete. Fehlende Geräte erscheinen kursiv mit „(missing)“.
    Route-Editor: Name, Größe, „Set from Device“, Drehung, Rear Projection,
    Skalierung (Center/Fit/Stretch/Fill), Device, Teilbereich (Full/Part).
  - **Output Devices:** erkannte Displays mit Auflösung (Pixel und Punkte)
    und Wiederholrate; Hinweis, dass Anordnung/Auflösung in den
    Systemeinstellungen geändert werden.
* Stage-Editor (eigenes Fenster, „Edit Stage…“): Name, Layer, Keep Rendering,
  Größe mit „Resize…“; links Regionsliste (Buchstabe in Routenfarbe,
  Route-Auswahl, Grid, Delete, „New region with output…“) mit Move/Draw-
  Umschalter; rechts Größe, Position, Edge Blends, Auto, Blend-Gamma;
  Zeichenfläche mit Stage, Regionen in Routenfarbe: ziehen verschiebt,
  Ecken-Griffe ändern die Größe, Einrasten an Rand, Mittellinien und anderen
  Regionen (⌘ schaltet Einrasten aus); Pfeiltasten: 1 Pixel, ⇧ 10 Pixel, ⌥
  Größe; Blend-Bereich schraffiert, Region ohne Route gepunktet.
* Video-Ausgabe: Off, Preview Windows (ein eigenes Fenster pro Route, frei
  verschiebbar) oder Displays (ein randloses Fenster pro belegtem Display,
  das alle Routen des Displays zeigt, Teilbereiche an ihrer Stelle). Mit nur
  einem Display startet die App mit Preview Windows, weil ein Vollbild sonst
  die App selbst verdeckt. Stage-Monitor („Monitor“) zeigt das Canvas.
* Testbild: gehört zur Route und entsteht in Route-Auflösung (1 Bildpunkt =
  1 Ausgangspixel), mit Aufbau wie das QLab-Raster: monochrom, 1 px Linien,
  Skala, Diagonalen, Kreise und Beschriftung im weißen Kasten. Ist der Kreis
  auf der Leinwand eine Ellipse, verzerrt die Darstellung dahinter. Das
  Region-Grid liegt nur über der Fläche der jeweiligen Region.
* Alte Shows (Screens, `Stage::screen`) laden weiter: Beim Laden werden sie
  in Routen und Regionen migriert ( proportionale Aufteilung, `Stretch`),
  gespeichert wird nur das neue Modell.

---

## 6. Komponenten

### 6.1 Buttons

* Normaler Button: Fläche `RAISED`, Radius 5, Padding 8 × 4.
* Icon-Button: quadratisch, kein Hintergrund, Hover = `RAISED`.
* Primär-Buttons (GO, Panic) haben einen farbigen Rahmen, keine farbige
  Fläche.

### 6.2 Tabs (`theme::tab_bar`)

* Text-Tabs nebeneinander, 12 px Abstand. Aktiv: `TEXT` + 2-px-Unterstrich
  in `TEXT`. Inaktiv: `TEXT_DIM`, kein Kasten. Darunter 1-px-Linie
  `BORDER` über die volle Breite.
* Ein Stil für **alle** Tab-Leisten (Sidebar, Inspector, Settings).

### 6.3 Icons

* Phosphor-Icons (`src/gui/icons.rs`), nie Emoji/Unicode-Symbole (Pfeile
  wie „→" fehlen im UI-Font und erscheinen als Kästchen).
* Größen: 14 (inline), 16 (Liste/Toolbox), 18 (Kopfzeilen).
* Icons sind standardmäßig `TEXT_DIM`; Farbe nur mit Bedeutung (Status).

### 6.4 Formulare (`theme::form_grid`, `theme::field_label`)

* Label rechtsbündig in fester Spalte (96 px), `TEXT_DIM`; Feld daneben.
* Zeit- und Q#-Felder 88 px breit. Zusammengehöriges in einer Zeile
  (Position X/Y, Size W/H).
* Beschriftung Englisch, wie der Rest der UI.

---

## 7. Checkliste für neue UI

- [ ] Liegt die Funktion am einzigen dafür vorgesehenen Ort (Abschnitt 1.2)?
- [ ] Nur Farben/Größen aus `theme.rs`?
- [ ] Leere Werte gedimmt, Leerzustand mit genau einem Hinweis?
- [ ] Keine Überlappung von Icon und Text, auch bei schmalem Panel?
- [ ] Show-Mode bedacht (read-only / ausgegraut)?
- [ ] Rot nur für „kaputt" oder Panic?

## 8. Bekannte Grenzen

* Tastenkürzel nutzen nur Zeichen, die der UI-Font enthält: `⌘` als Symbol,
  sonst Wörter (`Shift+⌘S`, `Opt+⌘Up`, `Backspace`). Der Test
  `shortcut_labels_use_available_glyphs` sichert das ab.
* CoreMIDI verweigert gelegentlich den MIDI-Client für den ganzen Prozess.
  Dann meldet der Footer den Ausfall, und nur ein Neustart hilft.
* Network und MIDI in den Workspace Settings zeigen die Konfiguration
  nur an. Geändert wird sie über Umgebungsvariablen beim Start.
