# Arbeitsauftrag: Fehler aus dem Code-Review vom 2026-10-31

**Geprüfter Stand:** `cd913c8` (`main`). Zeilennummern beziehen sich auf diesen Stand
und können sich verschoben haben, deshalb sind auch die Funktionsnamen genannt.

**Bitte erst nach dem laufenden Decklink-Thema (Windows) anfangen** und beides nicht in
einem Commit mischen.

**Belege:** Die Punkte 1–11 wurden mit Prüftests auf einem Snapshot reproduziert, außer
wo „aus dem Code“ steht. Die Prüftests liegen nicht im Repo, die Beobachtungen stehen
unter „Beleg“. Die Punkte 12–14 sind nur aus Code oder Spezifikation abgeleitet: erst
nachweisen, dann beheben. Die Fix-Hinweise sind Vorschläge; wenn der Code etwas Besseres
hergibt, gilt das.

## Vorgehen

- Reihenfolge wie unten, Abhaken in dieser Datei.
- Pro Block ein eigener Commit mit grünem Lauf der Prüfliste.
- **Zu jedem Fix ein Regressionstest**, der vor dem Fix nachweislich fehlschlägt.
- Einige Tests schreiben das falsche Verhalten fest und müssen bewusst angepasst werden,
  z. B. `artnet_packet_layout` (`src/core/light.rs`) und
  `real_engine_sends_artnet_packet_via_udp` (`tests/light_cue.rs`). Keinen Test nur
  anpassen, damit er wieder grün wird, sondern im Commit begründen.
- Die App nicht im Vordergrund starten und keine Vollbild-Screenshots machen; headless
  egui-Tests bevorzugen.
- In Code, Kommentaren, Tests, Commit-Nachrichten und Doku QLab nicht mehr neu erwähnen,
  sondern das Verhalten beschreiben. Nur dieser Arbeitsauftrag nennt es noch als Referenz; die
  vorhandenen Erwähnungen werden nach Abschluss in einem eigenen Durchgang bereinigt, bitte
  nicht vorher.

```sh
cargo fmt
cargo test --no-fail-fast          # Testzahl mit dem Stand davor vergleichen
cargo clippy --all-targets         # keine Warnungen
cargo check --target x86_64-pc-windows-msvc --features webview
```

---

## Block A: Light-Cues (ein Commit)

- [x] **1. Absturz bei einem oder keinem Kanalwert** (kritisch)
  - Ort: `LightMessage::to_artnet` (`src/core/light.rs:135`, `:145`), `to_sacn` (`:162`, `:189`).
  - Problem: Die Länge wird auf mindestens 2 geklemmt, dann wird `self.values[..len]`
    gelesen. Die Vorgabe ist `vec![255]`, also ein Wert. Der Inspector ruft `packet()` bei
    jedem Frame auf (`src/gui/inspector/light.rs:86`), auch `build_trigger_command` tut das.
  - Beleg: `LightMessage::default().packet()` paniced; mit leeren Werten panicen Art-Net
    und sACN.
  - Fix: Fehlende Werte mit 0 auffüllen statt zu slicen. `packet()` darf für keine
    Eingabe panicen, weil das Werte-Feld im Inspector auch leer sein kann.
- [x] **2. Startkanal wird ignoriert**
  - Ort: `to_artnet`, `to_sacn`.
  - Beleg: Pakete mit `start_channel` 1 und 100 sind identisch; die Werte landen ab
    Kanal 1, der Inspector zeigt aber „ab Ch X“.
  - **Vorsicht:** Einfaches Auffüllen mit Nullen setzt alle Kanäle davor auf 0 und kann
    fremde Lampen im selben Universe ausschalten. Ein DMX-Paket beschreibt das Universe
    immer ab Kanal 1. Saubere Lösung: letzten Stand pro Universe in der Engine halten und
    die Werte hineinmischen. Falls das zu groß ist, das Verhalten im Inspector klar
    anzeigen und im Commit nennen.
- [x] **3. Vorgabe-Host `255.255.255.255` schlägt immer fehl**
  - Ort: `spawn_udp_send` (`src/engine/real_engine.rs:1107`).
  - Beleg: `send_to` liefert `Permission denied (os error 13)`.
  - Fix: `socket.set_broadcast(true)` vor dem Senden. Der Network-Cue über UDP nutzt
    dieselbe Funktion.
- [x] **4. Art-Net: ungerade Datenlänge** (nur Spezifikation, nicht an Hardware getestet)
  - Die Art-Net-Spezifikation verlangt eine gerade Länge von 2–512; aktuell sind ungerade
    Längen möglich. Mit einem Nullbyte auf gerade Länge auffüllen.
- [x] **5. sACN: Sequenznummer immer 0** (nur Spezifikation, nicht an Hardware getestet)
  - Ort: `to_sacn` (`src/core/light.rs:179`). Empfänger dürfen wiederholte Sequenznummern
    derselben Quelle verwerfen, außerdem sind CID und Source-Name leer.
  - Fix: Laufende Sequenz pro Universe in der Engine (`to_sacn` selbst ist zustandslos),
    feste CID, sinnvoller Source-Name.

## Block B: Follow-Logik (ein Commit)

- [x] **6. Auto-Continue startet zwei Cues statt einem** (kritisch)
  - Ort: `CueList::set_status_with_follow` (`src/core/cue_list.rs:927`, `:943`),
    `poll_engine_events` (`src/gui/app.rs:2626`).
  - Problem: Bei Post-Wait 0 kommt `Finished` (→ `Completed`, GO) und danach
    `PostWaitElapsed` (→ `Completed`, nochmal GO).
  - Beleg: Mock-Engine, Cues 1 (Wait 30 ms, AutoContinue), 2, 3 (Wait 30 s): nach GO stehen
    1=Completed, **2=Playing, 3=Playing**. Erwartet ist 3=Standby.
  - Fix: Follow nur, wenn der Status sich wirklich geändert hat. Dasselbe für Auto-Follow
    bei wiederholtem `Started` prüfen. Mit Mock- und Real-Engine testen.
- [x] **7. Auto-Continue/Auto-Follow bleiben bei Sofort-Cues stehen** (kritisch)
  - Ort: `go_single`, `go_group`, `trigger_by_number` (`src/core/cue_list.rs:628`, `:665`,
    `:853`); `finish_send` (`src/engine/real_engine.rs:1087`) meldet bei Erfolg nichts.
  - Beleg: Memo und MIDI mit AutoContinue: Der Folge-Cue bleibt `Standby`. Für Network,
    Light und Companion gilt es aus dem Code, Memo-Cues erzeugen gar kein Engine-Kommando.
  - Fix: Die Kette in der Cue-Liste weiterführen, wenn ein Cue sofort `Completed` wird, mit
    Schutz vor Endlosketten (der Kommentar von `go_chained` nennt früher max. 100). Zusammen
    mit Punkt 6 lösen, damit nichts doppelt feuert. Auch prüfen: Post-Wait bei Sofort-Cues,
    und dass ein später gemeldeter Sendefehler den Cue noch auf `Error` setzen kann.

## Block C: Video-Rendering (ein Commit)

- [x] **8. Opacity/Fades wirken auf Fill-Routen nicht, obere Stage verdeckt untere** (kritisch)
  - Ort: `render_region_into`, Zweige ohne `keep_alpha` (`src/engine/route_renderer.rs:190-201`).
  - Problem: Das Stage-Canvas ist transparent und nicht vormultipliziert, Fill-Routen
    kopieren aber RGB und setzen Alpha auf 255, ohne das Quell-Alpha zu beachten.
  - Beleg: 25 % Opacity: Canvas `[255,255,255,64]`, Route `[255,255,255,255]` (erwartet
    etwa 64). Untere Stage rot, obere transparent: Route `[0,0,0,255]` (erwartet rot).
  - Fix: `src[3] / 255 * factor` als Deckkraft auf das Ziel mischen, Ergebnis-Alpha 255.
    Prüfen, wer das Canvas sonst noch direkt anzeigt (Stage-Fenster in
    `src/gui/stage_view.rs`). Der Test `key_route_preserves_alpha_fill_route_flattens`
    deckt das nicht ausreichend ab, ebenso wenig opake Testbilder.
- [x] **9. Surfaces werden am Canvas-Rand gestaucht statt abgeschnitten**
  - Ort: `composite_canvas` (`src/engine/compositor.rs:177-187`, `:216-217`).
  - Beleg: Canvas 100×100, Bild 100×100 (links rot, rechts blau), Custom `x=-50`: Pixel 0
    ist rot, Pixel 49 blau; erwartet ist blau ab Pixel 0.
  - Fix: Skalierung aus dem ungeklemmten Rechteck berechnen und den Quell-Offset um den
    abgeschnittenen Teil verschieben. Links, rechts, oben und unten testen.
- [x] **10. Hard Edge rechts/unten geht im Fast Path verloren**
  - Ort: `composite_canvas` (`src/engine/compositor.rs:203`).
  - Beleg: Hard Edge rechts, 50 px Breite, Vollbild: Pixel 99 hat Alpha 255 (erwartet nahe 0).
  - Fix: Bedingung durch `surface.hard_edge.is_none()` ersetzen.

## Block D: Remote und Trigger (ein Commit)

- [x] **11a. Timecode wird im Show-Mode abgelehnt**
  - Ort: `RemoteCommand::is_edit` (`src/remote/mod.rs:217`), Prüfung in
    `handle_remote_command` (`src/gui/app.rs:1674`).
  - Beleg: `ReceiveTimecode.is_edit()` ist `true`, OSC `/timecode` wird also während der
    Show verworfen.
  - Fix: `ReceiveTimecode` in die Ausnahmeliste aufnehmen und einen Test über
    `handle_remote_command` im Show-Mode ergänzen.
- [x] **11b. Gruppen starten bei `GoTo` und Timecode ihre Kinder nicht**
  - Ort: `trigger_by_number` (`src/core/cue_list.rs:853`), `receive_timecode` (`:579`).
  - Beleg: Beide setzen die Gruppe auf `Completed`, Kind bleibt `Standby`, 0 Kommandos.
  - Fix: Wie `go()` nach Cue-Typ verzweigen (`go_group`, `go_control`, `go_single`), am
    besten über eine gemeinsame Funktion. Aus dem Code gilt dasselbe für Control-Cues; mit
    testen. `go_single` und `trigger_by_number` enthalten dieselbe Logik doppelt.

- [x] **11c. Web-Remote: Inspector-Tabs neuer Cue-Typen fehlen**
  - Ort: `renderInsp` (`src/remote/web_ui.html:502`), Tab-Liste `TABS` (`:185`).
  - Problem (aus dem Code, nicht im Browser getestet): `TABS` nennt Text, Network, MIDI,
    Light, Companion, Timecode und Script, die Zuordnung in Zeile 502 kennt sie aber nicht und
    es gibt keine passenden `tab…`-Funktionen. Der Aufruf von `undefined(c)` wirft einen
    TypeError, sobald so ein Tab offen ist.
  - Fix: Die fehlenden Tabs mit denselben Feldern wie die GUI bauen. Die Web-Remote soll alle
    Funktionen der GUI bieten. Der Test sollte jeden Eintrag in `TABS` gegen die Zuordnung prüfen.

## Block E: Erst nachweisen, dann beheben

- [x] **12. Script-Cues werden vermutlich zu früh beendet**
  - Ort: `tick` (`src/engine/real_engine.rs:2288-2315`), `handle_message` in
    `src/bin/kuhplay-webview.rs`.
  - Vermutung: `loaded` heißt nur „Load-Befehl angenommen“. Die Instanz wird danach
    entfernt und `Drop` beendet den Prozess, längeres oder asynchrones JS würde abgebrochen.
  - Nachweis: Test mit `--features webview` und einem Script, das verzögert ein Ergebnis
    meldet. Das öffnet ein Fenster, daher nur nach Absprache lokal ausführen.
- [x] **13. Dirty-Tracking bei mehreren Stages**
  - Ort: `Compositor::note_composited` (`src/engine/compositor.rs:105-107`).
  - Vermutung: Das `retain` löscht die Einträge der anderen Stages, die dadurch immer als
    geändert gelten. Mit zwei Stages messen, dann pro Stage verwalten.
- [x] **14. Doku korrigieren**
  - `docs/TODO-offen.md` nennt `feature/video-routing` als Stand und 541 statt 636 Tests.
  - `docs/HTML-Templates.md` §1 nennt 100 ms / 10 Hz, im Code sind es 33 ms
    (`WEBVIEW_SNAPSHOT_INTERVAL`). §1 nennt außerdem nicht, dass die Rate nur der angeforderte
    Takt ist; die tatsächliche Bildrate ist nicht gemessen.

## Block G: DeckLink direkt über das SDK, ffmpeg mitliefern (vor Block F)

**Ziel:** DeckLink läuft auf Windows, macOS und Linux direkt über das Blackmagic SDK, ohne
ffmpeg dazwischen. ffmpeg wird nur noch zum Dekodieren genutzt und mit der App ausgeliefert.

**Grund:** `decklink` steht in ffmpeg auf der Nonfree-Liste, solche Builds dürfen nicht
weitergegeben werden. Das lokale Homebrew-ffmpeg 9.0.2 enthält es auch nicht.

**Voraussetzung:** Das Blackmagic DeckLink SDK liegt lokal in `third_party/blackmagic-sdk/`
(per `.gitignore` ausgeschlossen). Fehlt es dort, stoppen und melden, keine Header aus dem
Gedächtnis nachbauen. Verwendete Version: **Desktop Video SDK 16.0**; das ist auch die
Mindestversion für Desktop Video, in der README nennen und in der App bei älterem Treiber klar
melden. Laufzeittest gegen den installierten Treiber 16.5 auf dem Mac.

**Lizenz (SDK-EULA, Klausel 0.1, 1.4, 1.7, 6):**
- Einchecken dürfen nur die Dateien aus `Mac/Include`, `Win/Include`, `Linux/Include`, und zwar
  in `third_party/decklink-sdk-16.0/`. Der Lizenzkopf der Header erlaubt das (geprüft an
  `Mac/Include/DeckLinkAPI.h`) und muss in allen Kopien erhalten bleiben.
- **Nicht einchecken oder ausliefern:** Beispielcode (Klausel 1.7: nur intern), Dokumentation und
  alle übrigen SDK-Dateien; Beispielcode auch nicht in unseren Code übernehmen.
- Desktop Video selbst wird nicht mitgeliefert.
- Produktname: Marken wie „DeckLink“ nicht in unserem Produktnamen. Als Kompatibilitätsaussage nur
  „KuhPlay compatible with Blackmagic Design DeckLink“ oder „KuhPlay for Blackmagic Design
  DeckLink“ (Klausel 6.2).

- [x] **G0. Absturz bei Control-Cue-Zyklen beheben** (kritisch, zuerst und eigener Commit)
  - Ort: `go_control` → `trigger_by_number` → `trigger_at` (`src/core/cue_list.rs`), seit Block D.
  - Beleg: Cue 1 (Control, Start → „2“) und Cue 2 (Control, Start → „1“): GO endet mit `stack
    overflow, aborting`, die ganze App stürzt ab. Der Control-Cue bleibt während seiner Aktion
    `Standby`, die Prüfung in `trigger_by_number` greift daher nicht. Auch ein Cue, der sich
    selbst startet, und längere Ketten testen.
  - Fix: Cue vor der Aktion als ausgelöst markieren oder die Tiefe begrenzen; ein Zyklus darf nur
    einen Fehlerzustand oder einen Hinweis erzeugen, keinen Absturz. Regressionstests dazu.
  - **Hinweis zu Block E:** Script-Cues enden ohne Done-Event nach 1000 ms
    (`KUHPLAY_SCRIPT_SETTLE_MS`). Längeres JS kann damit weiterhin abgeschnitten werden, und der Fix
    ist nur mit einer Fake-WebView getestet. Das ist mit dem CEF-Umbau neu zu prüfen.
- [x] **G1. Farbraum korrigieren** (zuerst, klein und eigener Commit)
  - Ort: `rgba_to_uyvy422` (`src/engine/decklink.rs`) rechnet immer mit BT.601.
  - Fix: BT.709 für HD und UHD, BT.601 nur für SD. Tests mit bekannten Farbwerten.
- [x] **G2. 29,97 und 59,94 fps korrigieren**
  - Ort: `bmd_display_mode`, `DecklinkConfig.fps`, `frame_interval`
    (`src/engine/real_engine.rs:447`).
  - Problem (aus dem Code): `fps = 2997` wählt den richtigen Modus, die Engine taktet dann aber
    mit `1.0 / 2997` Sekunden. `fps = 29` taktet mit 29 statt 29,97.
  - Fix: Bildrate als Bruch führen (30000/1001), Test für das Intervall.
- [x] **G3. Native Ausgabe auf macOS und Linux, Windows auf SDK 16.0 heben** (macOS/Linux: C++-Bridge via FFI mit SDK 16.0, live getestet auf UltraStudio Monitor 3G; Windows: COM auf SDK-16.0-GUIDs/Vtable inkl. IDeckLinkVideoBuffer; Feature `decklink`, Build ohne SDK läuft durch)
  - Windows: `decklink_com.rs` nutzt laut Vergleich mit dem SDK 16.0 alte Schnittstellen.
    `IDeckLinkOutput` steht in `DeckLinkAPI_v10_11.idl`, `IDeckLinkVideoFrame` und
    `IDeckLinkMutableVideoFrame` in `DeckLinkAPI_v14_2_1.idl`, und `GetBytes` liegt jetzt nicht mehr
    am Frame. Die vier CLSIDs stammen aus `_v10_8` und `_v10_11`; aktuell ist
    `BA6C6F44-6DA5-4DCE-94AA-EE2D1372A676`. Auf die aktuellen IDs und Layouts aus `DeckLinkAPI.idl`
    umstellen, die vtable-Reihenfolge dagegen prüfen und auf echter Karte testen.
  - Kleine C++-Brücke mit der `DeckLinkAPIDispatch` des SDK, die den Treiber zur Laufzeit lädt.
    Die App muss ohne installierten Treiber starten und dann eine klare Meldung zeigen.
  - Der Build muss auch ohne SDK durchlaufen, z. B. über ein Cargo-Feature, und die Build-Skripte
    für die Releases aktivieren es.
  - Gemeinsame Schnittstelle mit `decklink_com.rs`, Funktionen wie unter Windows: Geräteliste,
    Ausgabe, UYVY-Frames. Keine Plattformweiche außerhalb des DeckLink-Moduls.
- [x] **G4. Scheduled Playback auf allen Systemen** (Bridge + Windows-COM: Frame-Queue, Karte als Taktgeber via StartScheduledPlayback/ScheduleVideoFrame/Callback; next_time gegen Stream-Zeit geclampt; late/dropped/queued gezaehlt und im Workspace-Status + EngineHandle; live auf Mac: 125 Frames, dropped=0)
  - `DisplayVideoFrameSync` durch geplante Ausgabe mit der Karte als Taktgeber ersetzen, verspätete
    und verlorene Bilder zählen und im Status anzeigen. Key und Fill laufen synchron.
- [x] **G5. Formate und Geräteauswahl** (UHD 3840×2160 in bmd_display_mode; Geräte-Combo im GUI-Route-Editor mit erkannten Karten; Web-Remote-Routing bekommt decklink_devices/decklink_hint; Hint „Treiber da, 0 Geräte → Desktop Video neu installieren/registrieren“ in GUI + Web-Remote)
  - Fehlende Modi ergänzen, mindestens UHD (3840×2160); Modi und Geräte von der Karte abfragen
    statt nur fester Tabelle.
  - Karten im Route-Editor zur Auswahl anbieten statt festem Namen
    (`src/gui/workspace_settings.rs:1358`) und Umgebungsvariablen. Klare Meldungen für fehlenden
    Treiber und nicht unterstützte Formate.
  - Im Stil von `docs/UI.md`; die Web-Remote bekommt dieselbe Auswahl.
- [x] **G6. ffmpeg-Ausgabeweg entfernt** (DecklinkOutput/ffmpeg_decklink_args/KUHPLAY_DECKLINK_BACKEND/pix_fmt samt Tests raus; DecklinkSink nur noch Native + Capture für Tests; README + Modul-Doku angepasst)
  - Erst nach G3 und G4: `DecklinkOutput`, `ffmpeg_decklink_args`, `KUHPLAY_DECKLINK_BACKEND`,
    `pix_fmt`, den Fallback in `DecklinkSink` und die Tests dazu entfernen. README und falschen
    Kommentar in `decklink.rs` anpassen.
- [x] **G7. ffmpeg/ffprobe mitliefern** (ffprobe_binary() wie ffmpeg_binary(): env KUHPLAY_FFPROBE, Sibling neben der App, PATH; probe() nutzt sie + klare Fehlermeldung; scripts/vendor-ffmpeg.{sh,ps1} legen LGPL-Builds + LICENSE-ffmpeg.txt neben die App; build-mac/windows/linux rufen das Vendor-Skript auf; Test ffprobe_binary_env_override)
  - `probe` nutzt fest `ffprobe` aus dem `PATH` (`src/engine/video_backend.rs:448`). Die Suche aus
    `ffmpeg_binary()` auch für ffprobe verwenden, mit Test.
  - Build-Skripte legen beide Programme neben die App, auf dem Mac in das App-Bundle: LGPL-Build
    ohne GPL- und Nonfree-Teile, Lizenztext und Quellenhinweis liegen bei. Die Hardware-Decoder
    müssen weiter funktionieren.
  - Fehlende Programme klar melden, und ein Video-Test läuft ohne ffmpeg im `PATH`.
- [ ] **G8. Test auf echter Hardware**
  - Am Mac ist eine **UltraStudio Monitor 3G** angeschlossen und wird vom SDK mit Ausgang erkannt
    (Treiber 16.5, SDK 16.0). Dort mit einem standardmäßig deaktivierten Test prüfen: Dauerlauf,
    Farben, Bildaussetzer. Key/Fill nur, soweit diese Karte es kann; sonst offen nennen.
  - Fehlt die Karte plötzlich, besonders nach einem macOS-Update: Der Auftraggeber installiert
    Desktop Video neu und registriert es im System. Das Setup kann dabei aktiv aussehen. Die App
    soll in diesem Fall (Treiber da, 0 Geräte) genau darauf hinweisen, siehe G5.
  - Die Windows-Tests auf dem ZBOOK macht der Auftraggeber am 2026-11-01, die Karte hängt dann
    dort. Vorher muss alles für Windows bauen.
  - Linux bleibt ohne Hardware-Test, falls keine Karte verfügbar ist; das im Commit und im README
    nennen.

## Block F: Look and Feel wie QLab 5 (zum Schluss, nach den Blöcken A–E)

**Ziel:** Wer QLab 5 kennt, findet sich ohne Umlernen zurecht, vor allem blind per Tastatur.

**Maßstab:** das offizielle Handbuch, nicht `docs/qlab5-parity.md`, denn das Dokument
markiert hier Abweichungen als ✅ oder sogar als QLab-Verhalten:
<https://qlab.app/docs/v5/fundamentals/workspace/> und
<https://qlab.app/docs/v5/general/keyboard-shortcuts/>

**Stand der Prüfung:** Handbuch gegen Code und Doku, die App lief dabei nicht. Farben,
Abstände und Schriften am echten Bild sind damit **noch nicht beurteilt**. Punkte, die nur
aus Tabelle und Doku folgen, vor der Umsetzung kurz im Code bestätigen.

### F1: Direkt umsetzen

- [ ] **F1.1 Pause-Farbe:** QLab zeigt Pausiert gelb (gelbe Doppelbalken, gelber
  Fortschrittsbalken), KuhPlay blau (`palette::PAUSED`, dort als TODO vermerkt). Die Tabelle
  in `docs/UI.md` §3 mit anpassen.
- [ ] **F1.2 Transport-Buttons:** Pause All gelb, solange Cues laufen; Resume All grün,
  solange Cues pausiert sind. Aktuell neutral weiß.
- [ ] **F1.3 GO-Button:** Roter Rahmen bei aktivem Double-GO-Schutz, „Preview“ bei aktivem
  Cart, „Audition“ im Audition-Modus. Jeweils prüfen, ob vorhanden.
- [ ] **F1.4 Footer:** QLab hat links Edit/Show und Toggles für Toolbox und Masthead, rechts
  Settings, Sidebar, Inspector und das Warn-Icon. KuhPlay hat keine Toolbox-/Masthead-Toggles
  und das Warn-Icon links.
- [ ] **F1.5 Masthead** per Ziehen in der Höhe änderbar und einklappbar (`docs/UI.md` §2
  schreibt feste Höhe vor, also dort mit ändern).
- [ ] **F1.6 Show-Mode:** Nebenfenster zeigen „SHOW MODE“ im Footer; Find ist gesperrt.
- [ ] **F1.7 Fehlende Kürzel ergänzen**, sofern die Funktion existiert oder klein ist; alles
  über die Tabelle `src/gui/shortcuts.rs`:
  - Wiedergabe: Esc doppelt = Hard Stop, `S` Panic Selected, `P` Pause/Resume Selected,
    `V` Preview, `L` Load, ⌥Space Audition GO.
  - Felder: `N` Nummer, `Q` Name, `O` Notes, `T` Target, `E` Pre-Wait, `D` Duration,
    `W` Post-Wait, `C` Continue-Modus.
  - Navigation: ⌘→/⌘← Tab, ⌘J Select Cue, ⇧⌘↓/⇧⌘↑ Playhead, `=`/`-` Sequenz, `>`/`<`
    Gruppen auf-/zuklappen.
  - Bearbeiten: ⌘X/⌘C/⌘V, ⇧⌘V, ⌘A, ⌘R Renumber, ⌘T Load to Time.
  - Ansicht: ⇧⌘I, ⇧⌘L, ⌘B, ⇧⌘F; Modus ⇧⌘[ und ⇧⌘].
  - Einzelbuchstaben dürfen in Textfeldern nicht auslösen (Registry `text_field_ids`).
- [x] **F1.8 entfällt:** `docs/qlab5-parity.md` wird nicht mehr gepflegt und nach diesem
  Auftrag entfernt. Offene Funktionen stehen in `docs/BACKLOG.md`.
- [ ] **F1.9 Sichtprüfung:** Erst nach Absprache einen Termin finden, an dem der Rechner
  frei ist; nur das App-Fenster aufnehmen und nicht in den Vordergrund holen. Vergleich mit
  QLab 5: Zeilenhöhen, Schriften, Kontraste, Statussymbole.

### F2: Erst nach Rückfrage (bewusste oder folgenreiche Abweichungen)

Nicht eigenmächtig umsetzen. Empfehlung steht jeweils dabei.

- [ ] **F2.1 Pfeiltasten:** In QLab wählt ⌘↑/⌘↓ den Cue, ⇧⌘↑/⇧⌘↓ bewegt den Playhead. In
  KuhPlay **verschiebt ⌘↑/⌘↓ den Cue in der Liste**, wer QLab gewohnt ist, ändert also
  unbemerkt die Reihenfolge. Empfehlung: QLab-Belegung übernehmen, ⌥⌘↑/⌥⌘↓ zum Verschieben,
  die nackten Pfeile (laut Parity-Doku so gewünscht) zusätzlich behalten.
- [ ] **F2.2 ⌘0–⌘9:** QLab: 0 Group, 1 Audio, 2 Mic, 3 Video, 4 Camera, 5 Text, 6 Light,
  7 Fade, 8 Network, 9 MIDI. KuhPlay: 0 Group, 1 Audio, 2 Video, 3 Fade, 4 Stop, 5 Script,
  6 Control, 7 HTML5, 8 Slideshow, 9 HTML Data. Empfehlung: QLab-Belegung, Mic und Camera
  bleiben frei oder werden wie in QLab über die Toolbox-Reihenfolge neu vergeben.
- [ ] **F2.3 ⌘D:** In QLab „Delete numbers of selected cues“, in KuhPlay Duplicate.
  Empfehlung: QLab-Belegung und Duplicate auf ein freies Kürzel legen.
- [ ] **F2.4 Panic:** QLab blendet über eine einstellbare Panic-Duration aus, Doppel-Esc
  stoppt hart. KuhPlay stoppt laut Code-Kommentar sofort. Empfehlung: QLab-Verhalten mit
  einstellbarer Dauer. Das ändert das Verhalten im Notfall, also vor der Umsetzung klären.
- [ ] **F2.5 Transport-Ort:** QLab hat Reset/Pause/Resume/Panic oben in der Sidebar, KuhPlay
  im Masthead (`docs/UI.md` §5.1). Ist die Sidebar zu, fehlt in QLab der Panic-Button, im
  Masthead bleibt er sichtbar. Empfehlung: QLab folgen, außer der Masthead war bewusst gewählt.
- [ ] **F2.6 Toolbar im Masthead:** QLab hat Toolbar und Toolbox, KuhPlay nur die Toolbox
  (`docs/UI.md` §1.2 „Eine Sache, ein Ort“). Empfehlung: Toolbar ergänzen und per Footer-Toggle
  ausblendbar machen; die Toolbox-Reihenfolge wie in QLab frei sortierbar machen.
- [ ] **F2.7 Statussymbole:** QLab kennt zusätzlich Loaded, Warnung, Flag, Override,
  Tail-out. Empfehlung: Loaded, Warnung und Flag (`F`) zuerst.

## Nicht umsetzen ohne Rückfrage

Das sind Produktentscheidungen. Bitte nichts daran ändern, sondern nur melden, falls es
beim Abarbeiten relevant wird:

- OSC und Companion lauschen ohne Anmeldung auf `0.0.0.0` und erlauben GO, Panic und das
  Umschalten des Show-Mode.
- Der Web-Remote-PIN läuft über unverschlüsseltes HTTP.
- Fünf falsche PIN-Eingaben sperren den Login 30 Sekunden für alle.
