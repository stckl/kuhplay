# Konzept: HTML5-Renderer und Daten-Templates

**Stand:** 2026-10-31. **Status:** Konzept, noch nicht umgesetzt.

**Start erst nach** `docs/TODO-review-2026-10-31.md`. Dieses Dokument ist kein Arbeitsauftrag:
Formate und Namen unten sind Vorschläge und werden beim Bauen konkretisiert. Bereits
getroffene Entscheidungen stehen in Abschnitt 2.

## 1. Ziel

Aus KuhPlay heraus lassen sich beliebige HTML/CSS/JS-Grafiken für Videoproduktionen
steuern: Bauchbinden, Programme, Countdowns und alles andere, was Kunden angezeigt haben
wollen. **KuhPlay kennt keine Vorträge oder Bauchbinden**, sondern nur allgemeine Bausteine.
Die Timeline (`assets/demo/timetable.html`) ist ein Beispiel, kein Sonderfall.

## 2. Entscheidungen

| Thema | Entscheidung |
|---|---|
| Renderer | **CEF mit Offscreen-Rendering**, wie OBS und CasparCG |
| Bildrate | Ausgabe-Bildrate, **mindestens 30 fps**, auch auf Windows und mit Alpha für Key/Fill |
| Felder | Das Template legt Felder und Typen fest, KuhPlay erzeugt die Eingabefelder |
| Trigger | Feste Trigger für rein/raus plus eigene Trigger des Templates |
| Panic | **Einmal Esc spielt `out` schnell ab, zweimal Esc beendet sofort** |
| Datenort | Standarddaten am HTML-Cue; der Data-Cue zeigt dasselbe Formular und überschreibt nur geänderte Felder; rohes JSON bleibt als Expertenansicht und für alte Shows |
| Datentabelle | Zeilen sind Datensätze, die einzeln abgefeuert werden |
| Import/Export | CSV und Excel, **in beide Richtungen** |
| Projektdaten | **Alle** Projektdaten liegen im Projektordner, wie bei QLab |
| Web-Remote | Bietet dieselben Funktionen wie die GUI, also auch die neuen Formulare |
| Außerdem gewünscht | Live-Vorschau im Inspector, Presets, Sprachumschaltung, Automatik nach Uhrzeit, Variablen (Uhr, Timer, Countdown) |

**Keine Kompatibilität zu anderen Systemen:** Wir bauen ein schlüssiges eigenes System nach
unseren Anforderungen, auch nicht zu CasparCG-Templates.

**Alpha wie in OBS:** HTML5 rendert standardmäßig auf transparentem Grund, und die Transparenz der
Seite bleibt bis zum Key/Fill-Ausgang erhalten. Weiche Kanten, Schatten und halbtransparente Flächen
müssen im Key und Fill korrekt ankommen. Ein deckendes Bild entsteht nur durch einen eigenen
Hintergrund der Seite.

**Overlays:** Dieselbe Transparenz gilt innerhalb von KuhPlay. Eine HTML-Seite liegt als Layer über
Videos, Bildern oder anderen Stages, und darunter bleibt alles sichtbar, auf jedem Ausgang: Display,
Vorschau und DeckLink. Mehrere Overlays lassen sich übereinander stapeln.

**Freiheit im Template:** Alles, was CEF kann, ist erlaubt, also eigenes JavaScript, Bibliotheken,
Canvas, WebGL, SVG und Video. Felddefinition, Trigger und Variablen sind Angebote: Sie schreiben
weder Framework noch Aufbau vor, und eine Seite ohne Felddefinition läuft trotzdem.

## 3. Ausgangslage im Code

- Rendering über System-WebViews im Subprocess `kuhplay-webview`, ein Prozess pro Cue.
- Bilder laufen einzeln als PNG/base64 über eine Pipe. Angefordert wird alle 33 ms
  (`WEBVIEW_SNAPSHOT_INTERVAL`); die tatsächliche Bildrate ist nicht gemessen.
- Außerhalb von macOS wird nur ein Page-Canvas erfasst (`js_snapshot`). Normales HTML/CSS
  liefert dort den Fehler `no canvas element` und kein Bild.
- `EngineCommand::Stop` beendet die Seite sofort, eine Out-Animation kann nicht laufen.
- Daten sind rohes JSON im Inspector (`draw_data`), gesendet an
  `window.kuhplay.onShowControlData`.
- Eine Show speichert nur Dateipfade, Medien werden nicht mitgenommen.
- Eine Live-Vorschau im Inspector gibt es nicht, nur Audition-Fenster und Stage-Monitor.
- Compositor und Route-Renderer rechnen auf der CPU.

## 4. Bausteine

1. **Template:** beliebiges HTML/CSS/JS, das Felder und Trigger selbst festlegt.
2. **Datentabelle:** Spalten kommen aus den Feldern, jede Zeile ist ein Datensatz. Eine Zeile
   wird per Klick, Cue, OSC oder Companion abgefeuert (`update`, dann `in`). Alternativ bekommt
   das Template die ganze Tabelle, wie bei der Timeline.
3. **Trigger:** siehe Abschnitt 5.
4. **Variablen:** Live-Werte aus KuhPlay, siehe Abschnitt 7.

### Felddefinition im Template

Die Felder stehen als JSON direkt im HTML, damit KuhPlay das Formular bauen kann, ohne die
Seite zu starten:

```html
<script type="application/kuhplay+json">
{
  "name": "Timeline",
  "languages": ["de", "en"],
  "fields": {
    "eventTitle": { "type": "text", "required": true },
    "talks": {
      "type": "list", "label": "Vorträge", "minItems": 1,
      "item": {
        "start":   { "type": "time", "required": true },
        "speaker": { "type": "text", "required": true },
        "title":   { "type": "text", "localized": true },
        "photo":   { "type": "image" }
      }
    }
  },
  "triggers": {
    "in":   { "timeout": "2s" },
    "out":  { "timeout": "1.5s" },
    "next": { "label": "Nächster Vortrag" }
  }
}
</script>
```

Fehlt ein Pflichtfeld, gilt der Cue wie in QLab als kaputt (rotes X).

### Datentypen

- **Gewünscht:** Text, Zahl, Uhrzeit, Zeitstempel, Bild, Liste
- **Vorschlag zusätzlich:** mehrsprachiger Text, mehrzeiliger Text, Video oder Logo, URL,
  Dauer, Ja/Nein, Farbe (auch aus festem Set), Auswahlliste, Gruppe (verschachtelte Felder)

## 5. Trigger

| Trigger | Wann | Was passiert |
|---|---|---|
| `in` | GO | Seite ist vorgeladen und befüllt, die Animation startet sofort |
| `update` | Data-Cue, Formular, Zeile | neue Daten, optional animiert |
| `out` | Stop | KuhPlay wartet auf das Ende der Animation, höchstens bis `timeout`, dann wird die Seite geschlossen |
| eigene (`next`, `highlight`) | Control-Cue, Button, OSC, Companion | im Template frei definiert |

Für einfache Grafiken reicht CSS:

```css
body[data-kuhplay-state="in"]  #box { animation: reinfliegen .6s both; }
body[data-kuhplay-state="out"] #box { animation: rausfliegen .6s both; }
```

Mit JavaScript; gibt der Handler ein Promise zurück, wartet KuhPlay darauf:

```js
kuhplay.on("out", async () => { /* Animation */ });
```

## 6. Import, Export und Projektordner

- CSV und Excel in beide Richtungen, Spalten entsprechen den Feldern.
- Mehrsprachige Felder bekommen je eine Spalte (`title.de`, `title.en`), Bilder stehen als
  Dateiname in der Tabelle.
- Listen innerhalb einer Zeile passen nicht in eine Zelle: In Excel kommen sie auf ein zweites
  Blatt, in CSV sind solche Felder nicht möglich.
- Vor dem Übernehmen zeigt eine Vorschau fehlende Pflichtfelder und unbekannte Dateien.
- Show, Medien, HTML und Bilder liegen zusammen in einem Ordner mit relativen Pfaden. Beim
  Öffnen alter Shows bietet KuhPlay an, alles dorthin zu kopieren. Das gilt für alle
  Cue-Typen, nicht nur HTML.

## 7. Variablen

| Variable | Inhalt |
|---|---|
| `clock.now` | aktuelle Uhrzeit |
| `cue.elapsed`, `cue.remaining` | Laufzeit und Restzeit des Cues, z. B. für einen Countdown aufs Cue-Ende |
| `timer.<name>` | frei benannte Timer, rauf oder runter, per Trigger start/stop/reset |
| `countdown.to` | Countdown auf eine Uhrzeit |
| `show.*`, `row.index` | Show-Daten und aktuelle Tabellenzeile |

```html
<span data-kuhplay-bind="cue.remaining" data-format="mm:ss"></span>
```

## 8. Reihenfolge

1. **CEF- und GPU-Prototyp** auf Mac und ZBOOK (Abschnitt 9).
2. **Trigger:** `in`, `out` und Panic.
3. **Projektordner**, damit Bilder von Anfang an im Projekt landen.
4. **Felder und Formulare** mit Inspector-Vorschau.
5. **Datentabelle** mit Zeilen abfeuern, Presets, CSV und Excel.
6. **Variablen:** Uhr, Timer, Countdowns, Automatik nach Uhrzeit, Sprachumschaltung.
7. **Web-Remote und Beispiel-Templates:** mindestens Timeline, Bauchbinde und Countdown, damit
   das System von Anfang an breit getestet ist.

## 9. Prototyp: Abnahme und Risiken

Der Prototyp kommt vor allem anderen, weil alles darauf aufbaut. Zusammen planen mit der
GPU-Pipeline aus `docs/KONZEPT-slideshow.md`.

**Muss nachweisen:**
- mindestens 30 fps in 1080p und 4K mit Alpha, auf Mac und ZBOOK
- normales HTML/CSS ohne Canvas wird erfasst
- Alpha stimmt bis zum Ausgang: Testseite mit Verlauf, Schatten und weicher Kante, geprüft an Fill
  und Key. CEF liefert die Bilder vormultipliziert, die Render-Kette rechnet heute nicht
  vormultipliziert, also einmal sauber umrechnen und festlegen, welches Format am Ausgang anliegt.
  Sonst entstehen dunkle oder helle Säume an den Kanten.
- Overlay über laufendem Video: dieselbe Testseite als Layer über einem Video und über einer zweiten
  Stage, ohne Säume und mit voller Bildrate beider Ebenen
- Bilder kommen schnell genug für DeckLink und Key/Fill in den Arbeitsspeicher

**Zu klären:**
- Reife der Rust-Anbindung `cef-rs`; falls nötig eine direkte Anbindung an C++
- zusätzliche App-Größe (Schätzung 150–300 MB), Build und Paket auf beiden Systemen
- Standard-Builds von CEF enthalten oft keine proprietären Codecs wie H.264; das betrifft nur
  Videos innerhalb von HTML-Seiten, Video-Cues laufen weiter über ffmpeg
- Folgen für Script-Cues, die heute ebenfalls über `kuhplay-webview` laufen

Scheitert der Prototyp an einem Punkt, wird neu entschieden, kein stiller Rückfall auf die
System-WebViews. Danach `docs/HTML-Templates.md` komplett auf den neuen Stand bringen.
