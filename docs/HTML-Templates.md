# HTML-Templates in KuhPlay (für KI-generierte Overlays)

Diese Datei beschreibt **verbindlich**, wie eine HTML/CSS/JS-Datei aufgebaut
sein muss, damit sie in KuhPlay als **HTML5-Cue** auf einer Stage läuft und
per **HTML-Data-Cue** mit JSON-Daten gefüttert wird – ohne Neu-Rendern als
Video. Gedacht als Prompt-Grundlage für KI-Tools („Baue mir ein Template nach
docs/HTML-Templates.md").

---

## 1. Lebenszyklus (warum das so läuft)

1. **HTML5-Cue** lädt die Quelle (Inline / Datei / URL) in eine **echte
   WebView** (Subprocess `kuhplay-webview`, Feature `webview`). Es öffnet sich
   **kein sichtbares Fenster** – die WebView rendert offscreen.
2. Die Engine fordert nach dem Laden **alle 33 ms einen Snapshot** an
   (`WEBVIEW_SNAPSHOT_INTERVAL`, ca. 30 Hz; bei Snapshot-Fehlern Backoff auf
   500 ms) und legt ihn in den Stage-Compositor (Stage → Region → Route →
   Device). Das HTML skaliert dadurch **automatisch auf die Stage-Auflösung**.
   33 ms ist der *angeforderte* Takt – die tatsächlich erzielte Bildrate
   hängt von Rendering-Aufwand und Hardware ab und ist nicht gemessen.
3. **HTML-Data-Cue** pusht ein JSON-Payload an die laufende WebView. Die
   Bridge ruft im Page-Kontext auf:
   ```js
   window.kuhplay.onShowControlData(<payload>)
   ```
4. **JS-Trigger** (EvalJs, z. B. „Test"-Button im Inspector oder Script-Cue)
   führt arbitrary JS in der WebView aus: `webview.evaluate_script(...)`.

**Konsequenz für Animationen:** DOM/CSS-Animationen werden mit **ca. 30 Hz**
(33 ms Snapshot-Takt, angefordert) in die Stage übernommen. Für flüssige
60-fps-Motion → Video-Cue. Für Overlays (Texte, Logos, Countdown, Timetable)
reicht das.

## 2. Die Bridge (fixer Vertragsbestandteil)

Jede geladene Seite bekommt vor dem eigenen JS injiziert:

```js
window.kuhplay = window.kuhplay || { version: "1", onShowControlData: null };
```

Das Template **muss** definieren:

```js
window.kuhplay.onShowControlData = (data) => {
  state.data = deepMerge(defaultData, data); // oder: ersetzen, je nach Use-Case
  render();
};
```

- Payload ist **exakt das JSON des HTML-Data-Cues** (Inspector-Tab „Data").
- Mehrere Data-Cues können auf **einen** HTML5-Cue zielen (Target im
  Data-Tab) – jedes GO pusht neu.
- Standalone-Vorschau im Browser: `window.kuhplay` existiert dort nicht →
  Default-Daten greifen (siehe §4).

## 3. Größen & Layout (sehr wichtig)

- Die Seite rendert in **exakt der Stage-Auflösung** (z. B. 1920×1080, auch
  4K/8K oder LED-Wand-Ausschnitte). **Nie feste Pixel** (`px`) für Layout,
  sondern `vw`/`vh`/`%`/`vmin`/`cqw` (Container-Query-Units).
- Root-Element füllt den Viewport:
  ```css
  html, body { margin: 0; padding: 0; width: 100vw; height: 100vh; overflow: hidden; }
  ```
- Alles, was "oben/unten/links/rechts" sitzen soll, über Flexbox/Grid mit
  `vh`/`vw`-Abständen. Beispiel Schriftgröße: `font-size: 6vh`.
- Hintergrund default **transparent** (Key/Fill: HTML liefert das Key-Signal;
  die WebView ist transparent geschaltet). Wer ein deckendes Overlay will, setzt
  explizit `background: #0d1117` o. ä. auf `html,body`.
- Text immer mit Kontrast/Schatten lesbar halten (`text-shadow`).

## 4. Datenmodell-Konvention (Default + Merge)

Im Template ein **eingebettetes Default-Dataset** mit der vollständigen
Struktur, damit die Seite allein im Browser testbar ist:

```js
const defaultData = {
  title: "…",
  items: [ { time: "09:00", name: "…", titleDE: "…", titleEN: "…", img: "" } ],
  lang: "DE"
};
let state = { data: structuredClone(defaultData) };
```

- `deepMerge(defaults, payload)`: Payload überschreibt nur vorhandene Felder
  (so kann ein Data-Cue z. B. nur `{"title":"Neu"}` schicken).
- `render()` ist eine **pure Funktion** `data → DOM` (keine Side-Effects
  außer DOM), damit Re-Renders idempotent sind.
- Bilder: relative Pfade neben der HTML-Datei (bei Datei-Quelle) oder
  `data:`-URIs / externe URLs. Keine lokalen absoluten Pfade des
  Entwicklungsrechners.

## 5. Self-Contained-Regeln

- **Eine HTML-Datei**, CSS und JS inline (`<style>`, `<script>`). Keine
  externen CDNs/fonts/scripts laden (Show-Netz ist evtl. offline; CSP).
  Fonts als base64-`data:`-URI einbetten, wenn nötig – ansonsten System-Fonts
  (`-apple-system, Segoe UI, Roboto, sans-serif`).
- Kein `localStorage`/`sessionStorage`/Cookies als Datenquelle (Persistence
  läuft über KuhPlay-Shows), kein `alert`/`prompt`, kein Scrollen, kein
  Fokus-Ring, keine Navigation.
- Kein `fetch()` auf externe Dienste zur Laufzeit (Offline-Show). Daten
  kommen **nur** über `onShowControlData`.

## 6. Capture-Canvas (Cross-Plattform)

- **macOS:** Snapshot nativ über WKWebView `takeSnapshot` – das DOM wird
  1:1 in Stage-Auflösung aufgenommen. Kein Extra nötig.
- **Windows/Linux:** Snapshot kooperativ über ein Page-Canvas. Das Template
  sollte zusätzlich malen:
  ```html
  <canvas data-kuhplay-capture width="1920" height="1080"></canvas>
  ```
  und seinen Inhalt (z. B. via Canvas-2D-API oder html2canvas-artig) in
  dieses Canvas in **Stage-Auflösung** zeichnen. Das erste Canvas mit
  `data-kuhplay-capture` wird bevorzugt, `toDataURL` liefert PNG.
  → Wer plattformübergreifend baut, zeichnet den Inhalt am besten direkt in
  ein Canvas (vektorbasiert, skaliert verlustfrei).

## 7. Debugging & Fehler

- JS-Fehler in der WebView landen als `EngineEvent::Error` → Footer „⚠“ in
  KuhPlay. Konsole gibt es nicht → **im Browser (Safari/Chrome) entwickeln
  und testen**, dann als Datei-/Inline-Quelle in KuhPlay laden.
- Snapshot-Fehler: 1× Error, dann Backoff (Engine spammt nicht).
- WebView-Binary fehlt (Build ohne `--features webview`): HTML5-Cues laufen
  virtuell (Events, aber kein Bild).

## 8. Minimales Beispiel-Template (Lower Third)

```html
<!DOCTYPE html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;width:100vw;height:100vh;overflow:hidden;
    font-family:-apple-system,'Segoe UI',Roboto,sans-serif;background:transparent}
  #box{position:absolute;left:4vw;bottom:6vh;padding:2.5vh 3vw;background:#0d1117ee;
    border-left:0.6vh solid #ffb000;border-radius:1vh;color:#fff}
  h1{margin:0;font-size:6vh} p{margin:0.5vh 0 0;font-size:3.2vh;color:#c9d1d9}
</style></head><body>
<div id="box"><h1 id="name"></h1><p id="title"></p></div>
<script>
const defaultData = { name: "Name", title: "Titel", lang: "DE" };
let data = structuredClone(defaultData);
function render(){ name.textContent = data.name; title.textContent = data.title; }
render();
window.kuhplay && (window.kuhplay.onShowControlData = (d) => {
  Object.assign(data, d); render();
});
</script></body></html>
```

Dazu in KuhPlay: HTML5-Cue (Inline oder Datei) auf Stage → HTML-Data-Cue mit
Target auf diesen Cue und Payload `{"name":"Dr. Koch","title":"Vortrag …"}`.

## 9. Checkliste vor dem Übernehmen (KI-Selbstcheck)

- [ ] Eine Datei, alles inline, kein externes Laden
- [ ] `100vw/100vh`, nur relative Einheiten (`vw/vh/%/vmin`), kein festes `px`-Layout
- [ ] `window.kuhplay.onShowControlData` gesetzt, Default-Daten eingebettet
- [ ] `render()` pure, idempotent
- [ ] `canvas[data-kuhplay-capture]` für Windows/Linux vorhanden (optional auf macOS)
- [ ] Läuft standalone im Browser ohne Fehler (Konsole sauber)
- [ ] Datenstruktur dokumentiert (Felder, Typen, Defaults) – am besten als
      JSON-Beispiel im Kommentar oder in der Show als Data-Cue
