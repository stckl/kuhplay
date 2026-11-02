# Arbeitsauftrag: Fade und Crossfade

**Stand:** 2026-11-02. **Status:** umgesetzt (Commit siehe Git-Log), siehe „Umgesetzt“-Notizen unten.

**Ziel:** Ein Fade-Cue blendet Ton und Bild eines laufenden Cues. Ein Crossfade-Cue macht mit einem GO den Übergang von einem Cue zum nächsten.

## 1. Ausgangslage im Code

Der Fade-Cue ist in der Bedienung heute praktisch wirkungslos:

- **Kein Ziel wählbar.** `Cue::fade_target` existiert, aber weder Inspector noch Web Remote setzen es. Ohne Ziel fadet der Cue sich selbst (`real_engine.rs`, `target_cue.unwrap_or(cue_id)`). Nur Panic setzt `target_cue`.
- **Nur ein Parameter pro Cue** (`fade_param`). Ton und Bild gemeinsam gehen nicht.
- **Fester Startwert.** `from_volume` ist standardmäßig 1.0. Steht das Ziel auf 0,3, springt es beim Start des Fades auf 1,0.
- **Stummer Fehlschlag.** Ohne `target_volume` erzeugt `Cue::play_command` kein Kommando, der Cue tut nichts und meldet nichts.
- **`FadeParam::Rate`** wird angeboten, wirkt aber nicht (`apply_fade_value`: nur Events).
- **`self.fades` hat `cue_id` als Schlüssel.** Ein zweiter Fade desselben Cues überschreibt den ersten. Das blockiert Ton und Bild im selben Cue.
- **Gleicher Layer:** `surfaces_of_stage` sortiert bereits nach Layer und Startzeit, später gestartet liegt oben. Darauf baut der Crossfade auf.
- **Deckkraft wird nur in einem Pfad zurückgesetzt** (`real_engine.rs`, um Zeile 1417). Prüfen, ob ein erneut gestarteter Cue wieder bei voller Deckkraft beginnt.

## 2. Fade-Cue

### Felder

| Feld | Bedeutung | Standard |
|---|---|---|
| Ziel | ein Cue, Auswahl über die Cue-Nummer | keins |
| Volume | Zielwert 0–1, oder aus | aus |
| Opacity | Zielwert 0–1, oder aus | aus |
| Duration | Länge des Übergangs, vorhandenes Feld `duration` | 3 s |
| Curve | gilt nur für Volume: Linear oder Decibel | Decibel |
| Stop when done | stoppt das Ziel am Ende des Fades | aus |

Opacity läuft immer linear. Einen eigenen Startwert gibt es nicht mehr.

### Verhalten

1. Der Fade startet beim **aktuellen Wert** des Ziels, nie bei einem festen Wert. Es darf nicht springen.
2. Volume und Opacity laufen gleichzeitig und enden gemeinsam. Der Fade-Cue meldet genau einmal `Finished`.
3. Ein neuer Fade auf dasselbe Ziel und denselben Parameter ersetzt den laufenden und übernimmt dessen aktuellen Wert. Ein Fade auf einen anderen Parameter läuft daneben weiter.
4. Bei Duration 0 gilt der Zielwert sofort.
5. Läuft das Ziel nicht, endet der Fade sofort ohne Wirkung und ohne Engine-Fehler.
6. Das Ziel läuft während des Fades normal weiter, auch beim Pausieren, Spulen und in Schleifen. Der Fade läuft nach der Wanduhr weiter.
7. Stop when done stoppt das Ziel, bevor der Fade-Cue `Finished` meldet. Auto-Continue und Auto-Follow des Fade-Cues funktionieren wie bei anderen Cues.
8. Wird das Ziel neu gestartet, beginnt es wieder bei seinen eigenen Werten: Volume aus `Cue::volume`, Opacity 1.
9. Ist das Ziel eine Gruppe, wirkt der Fade auf alle ihre laufenden Kinder.
10. Panic hat Vorrang und bricht laufende Fades ab. Das heutige Panic-Verhalten muss unverändert bleiben.

### Was wirkt worauf

| Ziel | Volume | Opacity |
|---|---|---|
| Audio | ja | nein |
| Video | ja | ja |
| SlideShow, Html, Text | nein | ja |
| Gruppe | je Kind | je Kind |
| anderes | nein | nein |

### Warnungen im Inspector und in `problems.rs`

Der Cue gilt als kaputt, wenn:
- kein Ziel gesetzt ist oder das Ziel nicht mehr existiert
- weder Volume noch Opacity aktiv ist
- nur ein Parameter aktiv ist, den das Ziel nicht hat

## 3. Crossfade-Cue

Neuer `CueType::Crossfade`, in der Toolbox in der Gruppe „Fade“.

### Felder

| Feld | Bedeutung | Standard |
|---|---|---|
| Von | laufender Cue, der endet | keins |
| Nach | Cue, der startet | keins |
| Duration | Länge | 3 s |

### Verhalten

1. GO startet „Nach“ mit Volume 0 und Opacity 0. **Vom ersten Bild an** darf nichts aufblitzen; die Werte müssen vor dem ersten Frame gesetzt sein.
2. **Bild, gleiche Stage:** Nur der obere Cue wird geblendet. Liegt „Nach“ oben, blendet es linear von 0 auf 1, während „Von“ voll deckend bleibt. Liegt „Nach“ unten, startet es sofort voll deckend und „Von“ blendet auf 0. Blendet man beide gleichzeitig, wird das Bild in der Mitte dunkler, weil unten Schwarz durchscheint. Das ist nicht erlaubt.
3. **Bild, verschiedene Stages:** Beide blenden unabhängig, „Nach“ auf 1 und „Von“ auf 0.
4. **Gleicher Layer:** „Nach“ liegt oben, weil es später startet. Das bestehende Sortieren reicht dafür.
5. **Ton:** Beide Cues blenden gleichzeitig mit neuer Kurve `FadeCurve::EqualPower`: „Von“ mit `cos(t·π/2)`, „Nach“ mit `sin(t·π/2)`, jeweils mal Ausgangswert. Die Lautstärke bleibt damit gleich. „Nach“ endet bei seinem eigenen `Cue::volume`.
6. Am Ende wird „Von“ gestoppt und sein Wert zurückgesetzt. Der Crossfade-Cue meldet einmal `Finished`.
7. Läuft „Von“ nicht, startet „Nach“ trotzdem und blendet normal ein. Ein Übergang darf eine Show nie anhalten.
8. Läuft „Nach“ schon, blendet es von seinem aktuellen Wert weiter und wird nicht neu gestartet.
9. Der Playhead springt nicht: Der Crossfade verhält sich wie jeder andere Cue in der Liste. „Nach“ wird dadurch nicht übersprungen; steht es als Nächstes in der Liste, gilt es wie heute als aktiv und GO geht weiter.

Kaputt ist der Cue, wenn „Nach“ fehlt, nicht existiert, gleich „Von“ ist oder kein Medien-Cue ist.

## 4. Umsetzung

- **Kommando:** `EngineCommand::Fade.from` wird `Option<f32>`, `None` heißt aktueller Wert. Panic nutzt das ebenfalls.
- **Aktuelle Werte:** Die Engine liefert die Lautstärke aus der Voice und die Deckkraft aus `opacities`.
- **Schlüssel:** `self.fades` wird `(Ziel-Cue, FadeParam)`. Der Fade-Cue merkt sich, wie viele seiner Fades noch laufen, und meldet erst danach `Finished`.
- **Rate:** `FadeParam::Rate` aus Oberfläche und Web Remote entfernen. Sie wirkt nicht. Den Engine-Zweig entfernen, wenn nichts mehr daran hängt.
- **Mock-Engine:** Verhalten in `mock_engine.rs` gleich nachziehen, die Tests hängen daran.
- **Alte Shows:** `fade_param` und `target_volume` werden beim Laden auf die neuen Felder abgebildet. `from_volume` entfällt. Felder per `serde(default)` lesbar halten.
- **Web Remote:** gleiche Felder wie der Inspector (`app.rs`, Abschnitt `fade_param`; `web_ui.html`, um Zeile 647).
- **Doku:** `docs/UI.md` und das veraltete Modulkommentar in `src/gui/inspector/curve.rs` anpassen.

- **Zusammenspiel:** `StopWhenVisible` und das Halten des letzten Bilds (`held` in `real_engine.rs`) gibt es schon. Beim Stoppen von „Von“ nicht daran vorbei arbeiten.

## 5. Abnahme

Für jeden Punkt ein Test, der ohne die Änderung fehlschlägt:

1. Video bei Deckkraft 0,3: Der Fade auf 1 startet bei 0,3, ohne Sprung.
2. Ton und Bild enden gemeinsam, `Finished` kommt genau einmal.
3. Zweiter Fade auf denselben Parameter ersetzt den ersten ohne Sprung.
4. Stop when done beendet das Ziel, Fade auf einen nicht laufenden Cue endet ohne Fehler.
5. Neu gestartetes Ziel hat wieder Deckkraft 1 und seine eigene Lautstärke.
6. Crossfade mit deckenden Videos: Kein Bild ist in der Mitte dunkler als beide Quellen. Gemessen wird am Stage-Canvas, nicht an Fade-Werten.
7. Crossfade: Das erste Bild von „Nach“ erscheint nicht mit voller Deckkraft.
8. Ton: Bei `t = 0,5` ist `von² + nach²` gleich 1 (innerhalb von 1 %).
9. Gleicher Layer: Beim Crossfade liegt „Nach“ über die ganze Dauer oben.
10. Crossfade ohne laufendes „Von“ startet „Nach“.
11. Show aus dem alten Format lädt, der alte Fade behält seinen Zielwert.
12. Panic während eines Fades und eines Crossfades: Beide enden, kein Cue bleibt hängen.
13. Bedienung: Ziel und Zielwerte lassen sich im Inspector setzen und wirken beim GO.

## 6. Regeln

- Vor jedem Commit: `cargo test`, `cargo clippy --all-targets` und `cargo fmt --check` müssen sauber sein, auch für Windows (`cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets`). Bei einem gescheiterten Schritt nicht committen.
- Bei unerklärlichen Timing-Fehlern der Engine-Tests einmal in einem frischen `CARGO_TARGET_DIR` gegenprüfen. Der normale `target`-Ordner steht unter Verdacht, sie auszulösen.
- Tests erzeugen ihre Medien selbst mit ffmpeg. Echte Medien kommen nicht ins Repo.
- Keine Fremdprodukte in Code, Doku und Oberfläche nennen.
- Nichts pushen, kein Tag, kein Release ohne Freigabe.

## 7. Nicht Teil dieses Auftrags

- Fade von Geometrie, Rate oder Effekten
- Eigene Kurven zum Zeichnen
- Übergänge zwischen mehr als zwei Cues
- Stop-Cue mit eigenem Ziel (`CueType::Stop` stoppt heute alles)

---

## Umgesetzt (2026-11-02)

- **Fade-Cue:** Felder `fade_volume`/`fade_opacity` (unabhängig), `stop_when_done`, Ziel via `fade_target` (Inspector + Web-Remote). `EngineCommand::Fade.from` ist `Option<f32>` (`None` = aktueller Wert, kein Sprung). `fades`-Key ist `(Ziel-Cue, FadeParam)` – neuer Fade ersetzt laufenden; `fade_pending` zählt, Finished erst wenn alle Fades des Cues fertig. Ziel nicht laufend → sofort Finished ohne Fehler. Duration 0 → sofortiger Zielwert. Panic fadet mit `from: None` auf 0 (Volume dB + Opacity in einem Kommando). Rate-Faden entfernt (UI + Engine-Zweig). Gruppen-Ziel: cue_list fächert auf laufende Kinder auf.
- **Crossfade-Cue:** neuer `CueType::Crossfade` (Toolbox-Gruppe „Fade“, Inspector Von/Nach). GO startet „Nach" (cue_list pusht Play + to_volume), Engine setzt Volume/Opacity 0 vor dem ersten Frame. Gleiche Stage: „Nach" oben blendet 0→1, „Von" bleibt deckend; „Nach" unten startet sofort deckend und „Von" blendet aus; verschiedene Stages blenden beide. Ton: `FadeCurve::EqualPowerUp/Down` (sin/cos). Ende: „Von" stoppen + Werte zurücksetzen, genau einmal Finished. „Von" nicht laufend → „Nach" startet trotzdem.
- **Migration:** `Cue::migrate_fade_fields()` beim Laden (show.rs, rekursiv): `fade_param`/`target_volume` → `fade_volume`/`fade_opacity`, `from_volume` entfällt. Serde abwärtskompatibel (`#[serde(default)]`).
- **Broken-Cues:** Fade kaputt bei fehlendem/ungueltigem Ziel, keinem Volume/Opacity, oder Parameter der am Ziel nicht existiert; Crossfade kaputt bei fehlendem/gleichem/nicht-Medien-„Nach" (src/gui/problems.rs).
- **Tests:** `tests/fade_crossfade.rs` (9 Tests: kein Sprung, Finished genau 1×, Ersetzen, Stop-when-done, nicht-laufendes Ziel, Equal-Power bei t=0.5, alte-Show-Migration, Real-Engine, Crossfade-Command) + angepasste audio_params/timing_deterministic/gui_smoke/toolbox.
- **Offen (bewusst):** Abnahme 6/9 am echten Stage-Canvas (deckende Videos, „Nach" oben) braucht Live-Lauf auf dem Mac; Curve-Tab (freie Kurven) und Audio-FX sind eigene Aufträge (siehe docs/FUNKTIONSUMFANG.md §5).
