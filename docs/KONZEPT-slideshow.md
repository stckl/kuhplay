# Konzept: Slideshow-Übergänge, Ken Burns und Fotowand

**Stand:** 2026-10-31. **Status:** Konzept, noch nicht umgesetzt und kein Arbeitsauftrag.

**Ziel:** dezente, professionelle Bildwechsel für Videoproduktionen, mehr als nur Überblendung.

## 1. Ausgangslage im Code

- Vorhanden: Crossfade, Cut, Dip to Black (`SlideTransition`, `src/core/slideshow.rs`).
- Der Übergang gilt für die ganze Slideshow, nicht pro Dia.
- Die Standarddauer ist 0, ein neuer Crossfade wirkt erst mit eingetragener Dauer.
- Gerechnet wird im Engine-Tick auf der CPU (`tick_slideshow`).

## 2. Übergänge

| Übergang | Wirkung | Paket |
|---|---|---|
| Dip to Color | blendet über eine wählbare Farbe | CPU |
| Push | neues Bild schiebt altes weg, vier Richtungen | CPU |
| Slide (Cover) | neues Bild gleitet über das stehende alte | CPU |
| Reveal | altes Bild gleitet weg und gibt das neue frei | CPU |
| Soft Wipe | Wischblende mit weicher Kante, Breite und Richtung einstellbar | CPU |
| Zoom Fade | Überblendung mit leichtem Zoom von etwa 3–5 % | CPU |
| Blur Fade | altes Bild wird unscharf, neues zieht scharf | GPU |
| Luma Fade | helle Bildbereiche wechseln zuerst | GPU |

**Optional:** Split (Blende öffnet sich von der Mitte), Custom-Maske (Graustufenbild als Blende,
z. B. Logo-Wipe).

**Bewusst nicht:** Würfel, Seite umblättern, Kreise, Sterne, Dreh- und Glitch-Effekte.

**Gilt für alle Übergänge:**
- Easing als Vorgabe statt linearer Bewegung
- Standard der Show, einzelne Dias können abweichen
- Standarddauer etwa 0,5–1 s; das Feld „Crossfade (s)“ heißt künftig „Dauer“
- leichter Motion Blur bei Push und Slide
- korrekt mit Alpha, damit Key/Fill sauber bleibt
- alte Shows laden unverändert

## 3. Ken Burns

Langsamer Zoom oder Schwenk während der Standzeit, mit Start- und Endausschnitt pro Dia. Läuft
auf der GPU, weil langsame Bewegung sonst sichtbar ruckelt (siehe 4.3).

## 4. Fotowand

### 4.1 Verhalten

Viele Fotos füllen die ganze Stage und laufen endlos durch. Jedes Foto bleibt in seinem nativen
Seitenverhältnis, die Abstände sind überall gleich, die Größen unterschiedlich und der Eindruck
organisch.

### 4.2 Layout: Justified Wall

```text
┌──────────┐ ┌─────┐ ┌──────────────┐ ┌───────┐
│          │ │     │ │              │ │       │
└──────────┘ └─────┘ └──────────────┘ └───────┘
┌────┐ ┌────────────┐ ┌────────┐ ┌────┐ ┌──────┐
│    │ │            │ │        │ │    │ │      │
│    │ │            │ │        │ │    │ │      │
│    │ │            │ │        │ │    │ │      │
└────┘ └────────────┘ └────────┘ └────┘ └──────┘
┌───────┐ ┌───┐ ┌─────────────┐ ┌──────────────┐
│       │ │   │ │             │ │              │
└───────┘ └───┘ └─────────────┘ └──────────────┘
```

- Nichts wird beschnitten: Jede Reihe wird so skaliert, dass sie exakt die Breite füllt.
- Keine Löcher und überall gleiche Lücken.
- Größen variieren durch schwankende Reihenhöhen und die Seitenverhältnisse.
- **Grenze:** Bilder können nicht über zwei Reihen ragen, ohne Löcher oder Beschnitt.

### 4.3 Einstellungen

- Laufrichtung hoch, runter, links, rechts; bei seitlichem Lauf Spalten statt Reihen
- Parallax: Reihen oder Spalten laufen unterschiedlich schnell. Sie müssen dafür quer zur
  Laufrichtung liegen, also z. B. Reihen bei seitlichem Lauf.
- Tempo, Abstand, Grundgröße, Stärke der Größenschwankung
- Hintergrund: Farbe oder transparent für Key/Fill
- Zufällige Reihenfolge ohne Wiederholung, bis alle Bilder einmal dran waren

### 4.4 Extras

- **Hero-Moment:** Ein Foto zoomt per Trigger groß heraus und fliegt zurück.
- **Live-Ordner:** Neue Fotos aus einem Ordner erscheinen automatisch in der Wand.

### 4.5 Technik

Die GPU ist hier fast Pflicht:
- Langsames Laufen braucht Subpixel-Bewegung und gute Filterung, sonst sieht man ein feines
  Ruckeln, besonders auf LED-Wänden.
- Fotos liegen als Texturen auf der Karte, statt pro Frame auf der CPU skaliert zu werden. Große
  Bilder vorab auf die benötigte Größe reduzieren, damit der Speicher reicht.
- Multi-Output-Stages wie 8K-Wände sind auf der CPU sehr teuer.

## 5. GPU-Pipeline

Compositor und Route-Renderer rechnen heute auf der CPU. Die GPU ist deshalb ein Umbau der
Render-Pipeline und wird **zusammen mit CEF** geplant (`docs/KONZEPT-html-templates.md`,
Abschnitt 9). Für DeckLink und Key/Fill müssen die Bilder schnell genug zurück in den
Arbeitsspeicher; das gehört in den Prototyp.

## 6. Reihenfolge

1. Übergänge des CPU-Pakets mit Easing, Übergang pro Dia und besserer Vorgabe
2. CEF- und GPU-Prototyp
3. Ken Burns, Blur Fade, Luma Fade und Fotowand direkt auf der GPU, damit nichts doppelt
   gebaut wird
4. Hero-Moment und Live-Ordner
