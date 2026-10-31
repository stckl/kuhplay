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
