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

```sh
cargo fmt
cargo test --no-fail-fast          # Testzahl mit dem Stand davor vergleichen
cargo clippy --all-targets         # keine Warnungen
cargo check --target x86_64-pc-windows-msvc --features webview
```

---

## Block A: Light-Cues (ein Commit)

- [ ] **1. Absturz bei einem oder keinem Kanalwert** (kritisch)
  - Ort: `LightMessage::to_artnet` (`src/core/light.rs:135`, `:145`), `to_sacn` (`:162`, `:189`).
  - Problem: Die Länge wird auf mindestens 2 geklemmt, dann wird `self.values[..len]`
    gelesen. Die Vorgabe ist `vec![255]`, also ein Wert. Der Inspector ruft `packet()` bei
    jedem Frame auf (`src/gui/inspector/light.rs:86`), auch `build_trigger_command` tut das.
  - Beleg: `LightMessage::default().packet()` paniced; mit leeren Werten panicen Art-Net
    und sACN.
  - Fix: Fehlende Werte mit 0 auffüllen statt zu slicen. `packet()` darf für keine
    Eingabe panicen, weil das Werte-Feld im Inspector auch leer sein kann.
- [ ] **2. Startkanal wird ignoriert**
  - Ort: `to_artnet`, `to_sacn`.
  - Beleg: Pakete mit `start_channel` 1 und 100 sind identisch; die Werte landen ab
    Kanal 1, der Inspector zeigt aber „ab Ch X“.
  - **Vorsicht:** Einfaches Auffüllen mit Nullen setzt alle Kanäle davor auf 0 und kann
    fremde Lampen im selben Universe ausschalten. Ein DMX-Paket beschreibt das Universe
    immer ab Kanal 1. Saubere Lösung: letzten Stand pro Universe in der Engine halten und
    die Werte hineinmischen. Falls das zu groß ist, das Verhalten im Inspector klar
    anzeigen und im Commit nennen.
- [ ] **3. Vorgabe-Host `255.255.255.255` schlägt immer fehl**
  - Ort: `spawn_udp_send` (`src/engine/real_engine.rs:1107`).
  - Beleg: `send_to` liefert `Permission denied (os error 13)`.
  - Fix: `socket.set_broadcast(true)` vor dem Senden. Der Network-Cue über UDP nutzt
    dieselbe Funktion.
- [ ] **4. Art-Net: ungerade Datenlänge** (nur Spezifikation, nicht an Hardware getestet)
  - Die Art-Net-Spezifikation verlangt eine gerade Länge von 2–512; aktuell sind ungerade
    Längen möglich. Mit einem Nullbyte auf gerade Länge auffüllen.
- [ ] **5. sACN: Sequenznummer immer 0** (nur Spezifikation, nicht an Hardware getestet)
  - Ort: `to_sacn` (`src/core/light.rs:179`). Empfänger dürfen wiederholte Sequenznummern
    derselben Quelle verwerfen, außerdem sind CID und Source-Name leer.
  - Fix: Laufende Sequenz pro Universe in der Engine (`to_sacn` selbst ist zustandslos),
    feste CID, sinnvoller Source-Name.

## Block B: Follow-Logik (ein Commit)

- [ ] **6. Auto-Continue startet zwei Cues statt einem** (kritisch)
  - Ort: `CueList::set_status_with_follow` (`src/core/cue_list.rs:927`, `:943`),
    `poll_engine_events` (`src/gui/app.rs:2626`).
  - Problem: Bei Post-Wait 0 kommt `Finished` (→ `Completed`, GO) und danach
    `PostWaitElapsed` (→ `Completed`, nochmal GO).
  - Beleg: Mock-Engine, Cues 1 (Wait 30 ms, AutoContinue), 2, 3 (Wait 30 s): nach GO stehen
    1=Completed, **2=Playing, 3=Playing**. Erwartet ist 3=Standby.
  - Fix: Follow nur, wenn der Status sich wirklich geändert hat. Dasselbe für Auto-Follow
    bei wiederholtem `Started` prüfen. Mit Mock- und Real-Engine testen.
- [ ] **7. Auto-Continue/Auto-Follow bleiben bei Sofort-Cues stehen** (kritisch)
  - Ort: `go_single`, `go_group`, `trigger_by_number` (`src/core/cue_list.rs:628`, `:665`,
    `:853`); `finish_send` (`src/engine/real_engine.rs:1087`) meldet bei Erfolg nichts.
  - Beleg: Memo und MIDI mit AutoContinue: Der Folge-Cue bleibt `Standby`. Für Network,
    Light und Companion gilt es aus dem Code, Memo-Cues erzeugen gar kein Engine-Kommando.
  - Fix: Die Kette in der Cue-Liste weiterführen, wenn ein Cue sofort `Completed` wird, mit
    Schutz vor Endlosketten (der Kommentar von `go_chained` nennt früher max. 100). Zusammen
    mit Punkt 6 lösen, damit nichts doppelt feuert. Auch prüfen: Post-Wait bei Sofort-Cues,
    und dass ein später gemeldeter Sendefehler den Cue noch auf `Error` setzen kann.

## Block C: Video-Rendering (ein Commit)

- [ ] **8. Opacity/Fades wirken auf Fill-Routen nicht, obere Stage verdeckt untere** (kritisch)
  - Ort: `render_region_into`, Zweige ohne `keep_alpha` (`src/engine/route_renderer.rs:190-201`).
  - Problem: Das Stage-Canvas ist transparent und nicht vormultipliziert, Fill-Routen
    kopieren aber RGB und setzen Alpha auf 255, ohne das Quell-Alpha zu beachten.
  - Beleg: 25 % Opacity: Canvas `[255,255,255,64]`, Route `[255,255,255,255]` (erwartet
    etwa 64). Untere Stage rot, obere transparent: Route `[0,0,0,255]` (erwartet rot).
  - Fix: `src[3] / 255 * factor` als Deckkraft auf das Ziel mischen, Ergebnis-Alpha 255.
    Prüfen, wer das Canvas sonst noch direkt anzeigt (Stage-Fenster in
    `src/gui/stage_view.rs`). Der Test `key_route_preserves_alpha_fill_route_flattens`
    deckt das nicht ausreichend ab, ebenso wenig opake Testbilder.
- [ ] **9. Surfaces werden am Canvas-Rand gestaucht statt abgeschnitten**
  - Ort: `composite_canvas` (`src/engine/compositor.rs:177-187`, `:216-217`).
  - Beleg: Canvas 100×100, Bild 100×100 (links rot, rechts blau), Custom `x=-50`: Pixel 0
    ist rot, Pixel 49 blau; erwartet ist blau ab Pixel 0.
  - Fix: Skalierung aus dem ungeklemmten Rechteck berechnen und den Quell-Offset um den
    abgeschnittenen Teil verschieben. Links, rechts, oben und unten testen.
- [ ] **10. Hard Edge rechts/unten geht im Fast Path verloren**
  - Ort: `composite_canvas` (`src/engine/compositor.rs:203`).
  - Beleg: Hard Edge rechts, 50 px Breite, Vollbild: Pixel 99 hat Alpha 255 (erwartet nahe 0).
  - Fix: Bedingung durch `surface.hard_edge.is_none()` ersetzen.

## Block D: Remote und Trigger (ein Commit)

- [ ] **11a. Timecode wird im Show-Mode abgelehnt**
  - Ort: `RemoteCommand::is_edit` (`src/remote/mod.rs:217`), Prüfung in
    `handle_remote_command` (`src/gui/app.rs:1674`).
  - Beleg: `ReceiveTimecode.is_edit()` ist `true`, OSC `/timecode` wird also während der
    Show verworfen.
  - Fix: `ReceiveTimecode` in die Ausnahmeliste aufnehmen und einen Test über
    `handle_remote_command` im Show-Mode ergänzen.
- [ ] **11b. Gruppen starten bei `GoTo` und Timecode ihre Kinder nicht**
  - Ort: `trigger_by_number` (`src/core/cue_list.rs:853`), `receive_timecode` (`:579`).
  - Beleg: Beide setzen die Gruppe auf `Completed`, Kind bleibt `Standby`, 0 Kommandos.
  - Fix: Wie `go()` nach Cue-Typ verzweigen (`go_group`, `go_control`, `go_single`), am
    besten über eine gemeinsame Funktion. Aus dem Code gilt dasselbe für Control-Cues; mit
    testen. `go_single` und `trigger_by_number` enthalten dieselbe Logik doppelt.

## Block E: Erst nachweisen, dann beheben

- [ ] **12. Script-Cues werden vermutlich zu früh beendet**
  - Ort: `tick` (`src/engine/real_engine.rs:2288-2315`), `handle_message` in
    `src/bin/kuhplay-webview.rs`.
  - Vermutung: `loaded` heißt nur „Load-Befehl angenommen“. Die Instanz wird danach
    entfernt und `Drop` beendet den Prozess, längeres oder asynchrones JS würde abgebrochen.
  - Nachweis: Test mit `--features webview` und einem Script, das verzögert ein Ergebnis
    meldet. Das öffnet ein Fenster, daher nur nach Absprache lokal ausführen.
- [ ] **13. Dirty-Tracking bei mehreren Stages**
  - Ort: `Compositor::note_composited` (`src/engine/compositor.rs:105-107`).
  - Vermutung: Das `retain` löscht die Einträge der anderen Stages, die dadurch immer als
    geändert gelten. Mit zwei Stages messen, dann pro Stage verwalten.
- [ ] **14. Doku korrigieren**
  - `docs/TODO-offen.md` nennt `feature/video-routing` als Stand und 541 statt 636 Tests.

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
- [ ] **F1.8 Parity-Doku korrigieren:**
  - „The Workspace“ und „Keyboard Shortcuts“ auf 🟡 setzen.
  - „keine Toolbar“ steht dort als QLab-Eigenschaft, das Handbuch beschreibt aber eine
    Toolbar im Masthead.
  - Sidebar-Transport steht in der Doku, liegt im Code aber im Masthead.
  - Veraltet: „kein Mic/Camera/Light/MIDI“ (Abweichung 8), „Fades haben kein Ziel“
    (The Inspector), HTML5 „noch virtuell“ (Abweichung 3).
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
