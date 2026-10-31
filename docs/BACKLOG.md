# Backlog: offene Funktionen

**Stand:** 2026-10-31. Sammlung noch nicht gebauter Funktionen, ohne Reihenfolge und ohne
Zusage. Geplante größere Umbauten stehen in `docs/KONZEPT-html-templates.md` und
`docs/KONZEPT-slideshow.md`.

## Offen

**Bedienung und Werkzeuge**
- Launcher-Fenster (Cue über Nummer oder Namen suchen und starten)
- Timecode-Status-Fenster
- Override Controls (Ausgänge global unterdrücken, z. B. DMX, Netzwerk, MIDI)
- Renumber für ausgewählte Cues
- Dateiziele in der File Target Search umschreiben (bisher nur Lesezugriff)
- Tastenkürzel frei belegen (Key Map zeigt sie bisher nur an)
- Einstellungen auf App-Ebene, unabhängig von der Show
- Network und MIDI in den Workspace Settings bearbeitbar machen (bisher Umgebungsvariablen)
- Theme-Auswahl (Palette ist bisher fest im Code)
- Versionen und Backups von Show-Dateien

**Audio und Video**
- Audio Map Editor
- Object Audio
- Mic-Cues und Camera-Cues (Live-Eingänge; bisher bewusst zurückgestellt)

**Licht**
- Licht-Pult mit Fades über Cues, Dashboard
- Licht-Befehlssprache, Patch-Editor und Bibliothek

**DMX-Fernsteuerung (gewünscht)**

KuhPlay lässt sich vom Lichtpult über Art-Net und sACN (E1.31) steuern.
- **Empfang:** beide Protokolle, Universe und Startkanal einstellbar, sACN auch per Multicast.
- **Steuerkanäle:** Zuordnung frei einstellbar, mindestens GO, Panic, Pause, Resume, Reset und ein
  Kanal für die Cue-Nummer.
- **Auslösen nur bei steigender Flanke:** Ein Befehl feuert einmal beim Überschreiten von etwa
  50 %, ein stehender Wert löst nichts erneut aus.
- **Sicherheit:** Beim Start, bei Signalverlust und beim Wiederverbinden lösen springende Werte
  nichts aus. Panic bekommt eine zusätzliche Absicherung, die Priorität von sACN und mehrere Sender
  werden berücksichtigt.
- **Bedienung:** Einstellungen in Workspace Settings und Web-Remote, mit Anzeige der empfangenen
  Werte zum Testen. Im Show-Mode bleibt der Eingang aktiv.
- **Cues gezielt abfeuern, wie bei einem Medienserver:**
  - Jeder Cue bekommt im Tab „Triggers“ einen festen **DMX-Slot** aus Bank und Slot, unabhängig von
    seiner Nummer. Verschieben oder Umnummerieren ändert also nichts am Pult.
  - Das Pult setzt Kanäle für Bank, Slot und Trigger: Bank und Slot wählen den Cue, die steigende
    Flanke am Trigger feuert ihn. Bank und Slot arbeiten in **Prozent-Schritten**, damit die Werte
    am Pult direkt lesbar sind: Slot 34 ist 34 %. Umrechnung `prozent = round(dmx * 100 / 255)`,
    das verträgt auch abweichendes Runden der Pulte. Das ergibt 100 Bänke mit je 100 Slots,
    also 10.000 Cues.
  - Slot 0 % bedeutet „kein Cue“. Ohne eigenen Trigger-Kanal würde ein Fade am Pult beim
    Durchlaufen alle Cues dazwischen auslösen.
  - Vorschlag zusätzlich: ein festes Layer-Profil pro Stage mit Intensity, Stop und Pause, dazu
    eine exportierbare Kanalliste oder ein Fixture-Profil für das Pult.

**Steuerung und Netzwerk**
- LTC- und MTC-Empfang über Hardware (bisher Timecode über OSC)
- MIDI Show Control, MIDI-File-Cues
- Zusammenarbeit mehrerer Rechner an einer Show
- OSC: Rückfragen und vollständige Befehlsreferenz

**Dokumentation**
- Benutzerhandbuch und Tutorials

## Bewusst nicht geplant

- NDI: HTML5 mit Live-Daten deckt den Bedarf ab
- AppleScript: nur auf dem Mac verfügbar, OSC und Web-Remote decken die Steuerung ab
