//! Cue-Liste als Tabelle (Spalten-Layout) – der „Cue Editor".
//!
//! Spalten (links → rechts):
//! **Status** (Playhead + Status-Pillen + Broken-✕) | Type (Icon +
//! Aufklapp-Dreieck bei Gruppen) | Q# | Name | Target | Pre-wait |
//! Duration (Action) | Post-wait | Continue-Mode.
//!
//! ## Interaktion (TODO.md Phase 3)
//!
//! * Klick auf eine Zeile: selektiert den Cue (auch Group-Kinder,
//!   UUID-Selektion – Item 7).
//! * ⌘-Klick: Cue zur Mehrfachauswahl hinzufügen/entfernen; ⇧-Klick:
//!   Bereich vom Anker bis zur geklickten Zeile.
//! * Klick in die **Status-Spalte**: Playhead auf diese Zeile setzen.
//! * **Doppelklick** auf Q#/Name/Pre-wait/Duration/Post-wait: Inline-Edit
//!   der Zelle (Enter oder Klick daneben übernimmt, Esc bricht ab).
//!   Arm/Disarm läuft NICHT mehr über Doppelklick, sondern über
//!   Kontextmenü und Inspector.
//! * **Drag & Drop**: Top-Level-Cues lassen sich umsortieren; Drop auf
//!   eine Gruppen-Zeile legt den Cue als letztes Kind hinein.
//! * Group-Cues: Aufklapp-Dreieck (ein-/ausklappen), Kinder eingerückt.
//! * Rechtsklick auf den **Header**: Spalten ein-/ausblenden,
//!   Breiten sind veränderbar und werden persistiert (`CueColumnLayout`).
//! * Rechtsklick auf eine Zeile: Kontextmenü (Set Playhead / Arm / Group /
//!   Duplicate / Set Color ▸ / Move Up / Move Down / Delete). Bei
//!   Mehrfachauswahl wirken die Aktionen auf die ganze Selektion.
//! * Auto-Scroll: Bei Playhead-Wechsel (GO, Pfeiltasten, Klick) scrollt
//!   die Liste zur Standby-Zeile.
//! * Disarmte Cues sind ausgegraut.
//! * `editable == false` (Show-Mode): Playhead-Klicks, Doppelklick,
//!   Drag & Drop und Kontextmenü sind deaktiviert (Sicherheitsfunktion).
//!
//! ## Status-Semantik (TODO.md Phase 2)
//!
//! * Standby (auch armiert): **neutral** – die Zeile ist nur über den
//!   Playhead-Pfeil + dezente Zeilenhervorhebung markiert. Rot bedeutet
//!   „kaputt", nicht „armiert".
//! * Pre-Wait: amber Status-Icon + die Pre-wait-Zelle zählt sichtbar herunter.
//! * Playing: grünes Play-Icon + Fortschrittsbalken + **Restzeit** in der
//!   Duration-Spalte.
//! * Paused: blaues Pause-Icon + Fortschrittsbalken (eingefroren).
//! * Post-Wait: türkises Icon + Fortschrittsbalken + Countdown in der
//!   Post-wait-Zelle.
//! * **Broken** (fehlende Datei, unbekannte Stage, …): rotes ✕-Icon +
//!   Tooltip mit Grund (Daten: [`CueRowMeta`], gecacht in `ShowApp`).
//! * Selektiert: blaue Zeilenhervorhebung (unabhängig vom Playhead).
//! * Cue-Color: Tönung der ganzen Zeile (niedrige Deckkraft) + farbiger
//!   Streifen am linken Rand („Cue Color").
//! * Cue-Sequences: dezenter Verbindungs-Strich zwischen verketteten Zeilen.
//!
//! ## Technischer Hinweis (Click-Handling)
//!
//! egui_extras-Tabellenzellen sind default `Sense::hover()` – `Response`'s
//! `clicked`-Feld wird nur für click-sensed Widgets gesetzt. Ohne
//! [`egui_extras::TableBuilder::sense`] mit `Sense::click()` würde
//! `row.response().clicked()` **nie** feuern (Regression: „Klick auf Cue
//! passiert nichts"). Abgedeckt von `tests/gui_interaction.rs`.
//!
//! ## Technischer Hinweis (Zeilen-Hintergründe)
//!
//! Zell-Hintergründe (Streifen/Selektion) malt egui_extras pro Zelle, VOR
//! dem Zell-Inhalt. Die Cue-Color-Tönung muss ebenfalls unter ALLEN Texten
//! der Zeile landen, wird aber pro Zelle gemalt (lückenlos wie egui_extras'
//! eigene Hintergründe: um halbes `item_spacing` expandiert) – mit dem
//! Painter des äußeren Ui, damit die Expansion nicht am Zell-Clip scheitert.
//!
//! ## Technischer Hinweis (Aktionen nach dem Rendern)
//!
//! Die Tabelle iteriert über Zeilen-Indizes. Alle Aktionen, die die Liste
//! struktuell ändern (Löschen, Duplizieren, Gruppieren, Drag & Drop), werden
//! daher in [`CueTableActions`] gesammelt und NACH dem Rendern vom Aufrufer
//! (`ShowApp`) ausgeführt – sonst würde die laufende Iteration springen.

use std::collections::HashMap;
use std::time::Duration;

use egui::{RichText, Sense};
use uuid::Uuid;

use crate::core::{
    format_timecode, parse_timecode, ContinueMode, Cue, CueList, CueStatus, CueType, CUE_COLORS,
};

use super::icons::{self, Icon};
use super::state::{CueColumnLayout, FindState, CUE_COLUMN_COUNT, CUE_COLUMN_TITLES};
use super::theme::{self, palette, size};

/// Zeilenhöhe der Cue-Tabelle (komfortabel; ~30 Zeilen passen auf
/// einen 1080p-Bildschirm inkl. Masthead/Footer/Menüleiste).
const ROW_HEIGHT: f32 = 28.0;
/// Höhe der Spalten-Header-Zeile.
const HEADER_HEIGHT: f32 = 24.0;

/// Spalten-Indizes (Reihenfolge, siehe `CUE_COLUMN_TITLES`).
mod col {
    pub const STATUS: usize = 0;
    pub const TYPE: usize = 1;
    pub const NUMBER: usize = 2;
    pub const NAME: usize = 3;
    pub const TARGET: usize = 4;
    pub const PRE_WAIT: usize = 5;
    pub const DURATION: usize = 6;
    pub const POST_WAIT: usize = 7;
    pub const CONTINUE: usize = 8;
}

/// Eine anzeigbare Tabellenzeile: Top-Level-Cue oder eingerücktes Kind.
#[derive(Debug, Clone, Copy)]
struct RowRef {
    /// Index des Cues in der Liste.
    list_index: usize,
    /// `Some(c)` = c-tes Kind der Gruppe an `list_index` (eingerückt).
    child: Option<usize>,
}

/// Pro Cue vorberechnete Anzeige-Daten (TODO.md Phase 2).
///
/// Wird vom Aufrufer (`ShowApp::cue_row_meta`) einmal pro Frame gebaut –
/// so bleibt diese Tabelle ohne App-Borrow headless testbar (die
/// Interaktions-Tests in `tests/gui_interaction.rs` rendern sie mit einer
/// leeren Map).
#[derive(Debug, Clone, Default)]
pub struct CueRowMeta {
    /// Broken-Gründe (leer = Cue heil) – rotes ✕ + Tooltip mit Grund.
    pub problems: Vec<String>,
    /// Hinweise, die den Cue nicht kaputt machen.
    pub warnings: Vec<String>,
    /// Die Engine hat den Cue vorgeladen.
    pub loaded: bool,
    /// Vergangene Zeit seit Phasenbeginn (Playing/Paused/Post-Wait) –
    /// Fortschrittsbalken + Restzeit in der Action-/Post-Wait-Spalte.
    pub elapsed: Option<Duration>,
    /// Verbleibende Pre-Wait-Zeit (nur während Pre-Wait) – die Pre-wait-
    /// Zelle zählt sichtbar herunter.
    pub pre_wait_remaining: Option<Duration>,
    /// Medien-Dauer aus dem Engine-Started-Event (für Duration/Restzeit bei
    /// Cues mit duration=0).
    pub media_duration: Option<Duration>,
}

/// Vom Nutzer ausgelöste Tabellen-Aktionen (Kontextmenü, Drag & Drop).
///
/// Werden NACH dem Rendern der Tabelle ausgeführt (siehe Modul-Doku).
/// Alle Cue-IDs (nicht Indizes): Selektion und Aktionen überleben
/// Umordnungen; Group-Kinder sind per ID adressierbar (Item 7).
#[derive(Debug, Default)]
pub struct CueTableActions {
    /// Cues löschen (IDs; Top-Level oder Group-Kinder).
    pub delete: Vec<Uuid>,
    /// Cues duplizieren (IDs; Top-Level).
    pub duplicate: Vec<Uuid>,
    /// Cues gruppieren (IDs; Top-Level).
    pub group: Vec<Uuid>,
    /// Armed-Zustand setzen: (IDs, armed).
    pub arm: Option<(Vec<Uuid>, bool)>,
    /// Farbe setzen: (IDs, Some(Farbe) = setzen / None = löschen).
    pub color: Option<(Vec<Uuid>, Option<[u8; 3]>)>,
    /// Drag & Drop umsortieren: (von Index, auf Index).
    pub move_to: Option<(usize, usize)>,
    /// Drag & Drop in Gruppe: (Cue-Index, Gruppen-Index).
    pub move_into_group: Option<(usize, usize)>,
}

/// Inline-Edit-Feld einer Zelle (Doppelklick, TODO.md Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EditField {
    Number,
    Name,
    PreWait,
    Duration,
    PostWait,
}

/// Anzahl der inline-editierbaren Felder (Index in `cell_rects`).
const EDIT_FIELD_COUNT: usize = 5;

/// Editierbare Felder in `cell_rects`-Reihenfolge.
const EDIT_FIELDS: [EditField; EDIT_FIELD_COUNT] = [
    EditField::Number,
    EditField::Name,
    EditField::PreWait,
    EditField::Duration,
    EditField::PostWait,
];

/// Zustand einer laufenden Zellen-Editor-Session (egui temp data).
#[derive(Debug, Clone)]
struct CellEdit {
    /// Editierter Cue (auch Group-Kind möglich).
    cue_id: Uuid,
    /// Editiertes Feld.
    field: EditField,
    /// Text-Puffer.
    text: String,
    /// Widget-ID des TextEdit (Fokus + Esc-Gate, siehe `cell_edit_active_id`).
    widget_id: egui::Id,
    /// Fokus noch nicht erhalten – pro Frame erneut anfordern.
    focus_requested: bool,
}

/// Drag&Drop-Payload: Index des gezogenen Top-Level-Cues.
#[derive(Debug, Clone)]
struct DragPayload {
    index: usize,
}

/// Bekannte egui-ID: Widget-ID des aktiven Zellen-Editors (leer, wenn
/// keiner aktiv). `ShowApp::text_field_focused` nutzt sie für das
/// Esc-Hotkey-Gate (Esc bricht den Edit ab statt Panic auszulösen).
pub fn cell_edit_active_id() -> egui::Id {
    egui::Id::new("kuhplay/cue_cell_edit_active")
}

/// Zeichnet die Cue-Tabelle des aktiven Carts.
///
/// `meta` liefert pro Cue-ID die Anzeige-Daten (Broken-Gründe, Zeiten) –
/// siehe [`CueRowMeta`] bzw. `ShowApp::cue_row_meta`. `columns` ist das
/// persistierte Spalten-Layout (Breiten + Sichtbarkeit); die Tabelle
/// schreibt die aktuellen Breiten pro Frame zurück.
///
/// Rückgabe: [`CueTableActions`] – der Aufrufer führt strukturelle
/// Änderungen nach dem Rendern aus (Löschen stoppt laufende Cues vorher,
/// siehe `ShowApp::delete_cue_by_id`).
pub fn draw(
    ui: &mut egui::Ui,
    cue_list: &mut CueList,
    editable: bool,
    meta: &HashMap<Uuid, CueRowMeta>,
    columns: &mut CueColumnLayout,
    find: &FindState,
) -> CueTableActions {
    // Zeilentext ist nicht markierbar: Selektierbare Labels würden Klick und
    // Rechtsklick abfangen, bevor die Zeile sie sieht.
    ui.style_mut().interaction.selectable_labels = false;
    let ctx = ui.ctx().clone();
    // Painter des äußeren Ui: Zeilen-Hintergründe (Cue-Color-Tönung) werden
    // pro Zelle damit gemalt, landen so unter dem Zell-Text (siehe Modul-Doku).
    let painter = ui.painter().clone();
    let cursor = cue_list.cursor();
    let hidden = columns.hidden;
    let mut actions = CueTableActions::default();

    // Find (TODO.md Phase 5): bei aktiver Suche werden Nicht-Treffer-Zeilen
    // abgedunkelt ( „markiert“ ). `hit_ids` = IDs aller treffenden Cues.
    let hit_ids: Option<std::collections::HashSet<Uuid>> = {
        let q = if find.open { find.query.trim() } else { "" };
        if q.is_empty() {
            None
        } else {
            Some(find_hits(cue_list, q).into_iter().map(|h| h.id).collect())
        }
    };

    // Anzeige-Liste flach: Top-Level-Cues + Group-Kinder (eingerückt).
    // Eingeklappte Gruppen zeigen keine Kinder-Zeilen.
    let mut rows: Vec<RowRef> = Vec::new();
    for (i, cue) in cue_list.cues().iter().enumerate() {
        rows.push(RowRef {
            list_index: i,
            child: None,
        });
        if cue.cue_type == CueType::Group && !cue.collapsed {
            for c in 0..cue.children.len() {
                rows.push(RowRef {
                    list_index: i,
                    child: Some(c),
                });
            }
        }
    }

    // Inline-Edit-Zustand (egui temp data, über Frames stabil).
    // WICHTIG: Lesen und Schreiben müssen denselben Typ verwenden –
    // `IdTypeMap` bildet den Key aus `TypeId ^ Id`; liest man mit `CellEdit`
    // und schreibt mit `Option<CellEdit>`, landet der Zustand unter zwei
    // verschiedenen Keys und überlebt den Frame-Wechsel nicht.
    let edit_id = egui::Id::new("kuhplay_cue_cell_edit");
    let mut edit: Option<CellEdit> = ctx
        .data_mut(|d| d.get_temp::<Option<CellEdit>>(edit_id))
        .flatten();
    // Pro Frame: Editor übernehmen (Enter/Fokusverlust), verwerfen (Esc)
    // oder neu starten (Doppelklick) – angewendet nach der Tabelle.
    let mut commit_edit: Option<CellEdit> = None;
    let mut cancel_edit = false;
    let mut start_edit: Option<CellEdit> = None;
    // Widget-IDs der Zellen-Editoren (stabil pro Cue+Feld).
    let base_id = ui.id();
    let edit_widget_id =
        move |cue_id: Uuid, field: EditField| base_id.with(("cell_edit", cue_id, field));

    // Auto-Scroll: Bei Playhead-Wechsel die Standby-Zeile sichtbar machen
    // (TODO.md Phase 3; `scroll_to_row` ist eine TableBuilder-Option und
    // wird nur auf Wechsel-Frames gesetzt).
    let scroll_state_id = egui::Id::new("kuhplay_cue_table_scroll_cursor");
    let mut scroll_to: Option<usize> = None;
    {
        let last_cursor = ctx.data_mut(|d| d.get_temp::<usize>(scroll_state_id));
        if last_cursor != Some(cursor) {
            scroll_to = rows
                .iter()
                .position(|r| r.child.is_none() && r.list_index == cursor);
            ctx.data_mut(|d| d.insert_temp(scroll_state_id, cursor));
        }
    }

    // Spalten ein-/ausgeblendet seit dem letzten Frame → Table-State
    // zurücksetzen, damit wieder eingeblendete Spalten ihre persistierte
    // Breite aus `columns.widths` bekommen (sonst kleben sie an 0).
    let reset_columns = columns.hidden != columns.prev_hidden;

    // Keine sichtbaren Spaltentrenner im Ruhezustand – egui_extras zeichnet
    // die Resize-Linie mit `noninteractive.bg_stroke`; beim Hover erscheint
    // sie weiterhin (`hovered.bg_stroke`). docs/UI.md §5.2.
    ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::NONE;

    let mut builder = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        // WICHTIG: Zellen müssen Klicks sensen, sonst feuert
        // `row.response().clicked()` nie (siehe Modul-Doku). Drag-Sense ist
        // nötig für Zeile-per-Drag-&-Drop: Ohne `Sense::drag` landen die Zellen
        // nie in `hits.drag`, `Response::drag_started()` bleibt false und
        // `dnd_set_drag_payload` setzt nie ein Payload.
        .sense(egui::Sense::click_and_drag())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .min_scrolled_height(0.0);
    if let Some(row) = scroll_to {
        builder = builder.scroll_to_row(row, Some(egui::Align::Center));
    }
    if reset_columns {
        builder.reset();
    }
    for i in 0..CUE_COLUMN_COUNT {
        builder = builder.column(column_spec(i, columns));
    }

    // Verbindungs-Indikatoren (Continue-Ketten): (x, Zeilen-Grenze y) –
    // nach der Tabelle malen, damit sie über beiden Zeilen liegen.
    let mut connectors: Vec<(f32, f32)> = Vec::new();

    builder
        .header(HEADER_HEIGHT, |mut header| {
            for i in 0..CUE_COLUMN_COUNT {
                header.col(|ui| {
                    if hidden[i] {
                        return;
                    }
                    ui.add_space(6.0);
                    if i == col::CONTINUE {
                        // Continue-Spalte mit Icon beschriftet.
                        icons::draw(ui, Icon::AutoContinue, 11.0, palette::TEXT_DIM)
                            .on_hover_text("Continue mode");
                    } else if !CUE_COLUMN_TITLES[i].is_empty() {
                        ui.label(
                            RichText::new(CUE_COLUMN_TITLES[i])
                                .size(size::LABEL_TEXT)
                                .color(palette::TEXT_DIM),
                        );
                    }
                    // 1-px-Linie unter der Kopfzeile (docs/UI.md §5.2).
                    let r = ui
                        .max_rect()
                        .expand2(egui::vec2(ui.spacing().item_spacing.x * 0.5, 0.0));
                    painter.line_segment(
                        [r.left_bottom(), r.right_bottom()],
                        egui::Stroke::new(1.0_f32, palette::BORDER),
                    );
                    // Rechtsklick auf den Header: Spalten ein-/ausblenden.
                    let resp = ui.interact(
                        ui.max_rect(),
                        ui.id().with(("cue_col_ctx", i)),
                        egui::Sense::click(),
                    );
                    resp.context_menu(|ui| {
                        ui.label(RichText::new("Columns").strong());
                        ui.separator();
                        for j in 0..CUE_COLUMN_COUNT {
                            ui.checkbox(&mut columns.hidden[j], column_menu_title(j));
                        }
                        ui.separator();
                        if ui.button("Reset Column Widths").clicked() {
                            columns.reset();
                            ui.close_menu();
                        }
                    });
                });
            }
        })
        .body(|body| {
            // Aktuelle Breiten zurück in das Layout persistieren (nur
            // sichtbare feste Spalten; Name ist `remainder`).
            let widths = body.widths().to_vec();
            for (i, w) in widths.iter().enumerate() {
                if i < CUE_COLUMN_COUNT && !hidden[i] && i != col::NAME {
                    columns.widths[i] = *w;
                }
            }
            body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                let index = row.index();
                let row_ref = &rows[index];
                let is_child = row_ref.child.is_some();
                let list_index = row_ref.list_index;

                // Einmalig borgen; alle Spalten lesen daraus (NLL beendet
                // den Borrow vor den mutierenden Aktionen am Zeilenende).
                let cue: &Cue = match row_ref.child {
                    Some(c) => &cue_list.cues()[list_index].children[c],
                    None => &cue_list.cues()[list_index],
                };
                // ID kopieren: Nach den Zellen darf `cue` nicht mehr benutzt
                // werden (die Klick-/Menü-Aktionen borgen cue_list mutabel).
                let cue_id = cue.id;
                // Find aktiv und dieser Cue ist kein Treffer → abdunkeln.
                let dimmed = hit_ids.as_ref().is_some_and(|hits| !hits.contains(&cue_id));
                let is_cursor_row = !is_child && list_index == cursor;
                let is_selected = cue_list.is_selected(cue_id);
                row.set_selected(is_selected);

                // Anzeige-Daten dieses Cues (Broken-✕, Zeiten; Phase 2).
                let row_meta = meta.get(&cue.id);
                let broken = row_meta.is_some_and(|m| !m.problems.is_empty());
                let problems: &[String] = row_meta.map(|m| m.problems.as_slice()).unwrap_or(&[]);
                let elapsed = row_meta.and_then(|m| m.elapsed);
                let pre_wait_remaining = row_meta.and_then(|m| m.pre_wait_remaining);

                let is_armed = cue.is_armed;
                // Disarmte Cues ausgegraut; Kinder zusätzlich abgedunkelt.
                let text_color = if is_armed {
                    palette::TEXT
                } else {
                    palette::TEXT_FAINT
                };
                // Sekundärtext (Type-Icon, Target) und leere Werte.
                let dim_color = if is_armed {
                    palette::TEXT_DIM
                } else {
                    palette::TEXT_FAINT
                };
                // Zeiten: Wert 0 tritt zurück (docs/UI.md §1.3).
                let time_color = |d: Duration| {
                    if d.is_zero() {
                        // Auf der Selektions-Fläche wäre FAINT unlesbar.
                        if is_selected {
                            palette::TEXT_DIM
                        } else {
                            palette::TEXT_FAINT
                        }
                    } else {
                        text_color
                    }
                };
                let name_color = if is_child {
                    text_color.gamma_multiply(0.75)
                } else {
                    text_color
                };

                // Zell-Rechtecke für spalten-bewusste Klick-Behandlung.
                let mut status_rect: Option<egui::Rect> = None;
                let mut cell_rects = [None::<egui::Rect>; EDIT_FIELD_COUNT];
                // Aufklapp-Dreieck angeklickt (Mutation erst am Zeilenende).
                let mut triangle_clicked = false;
                let is_group = !is_child && cue.cue_type == CueType::Group;
                let collapsed = cue.collapsed;

                // --- Status-Spalte: Playhead / Broken-✕ / Status-Pille ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if !hidden[col::STATUS] {
                        status_rect = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    // „Cue Color": farbiger Streifen am linken Zeilenrand.
                    if let Some([r, g, b]) = cue.color {
                        let strip = egui::Rect::from_min_size(
                            status_rect
                                .expect("Status-Rechteck wurde gerade gesetzt")
                                .min,
                            egui::vec2(
                                size::CUE_COLOR_STRIP,
                                status_rect
                                    .expect("Status-Rechteck wurde gerade gesetzt")
                                    .height(),
                            ),
                        );
                        painter.rect_filled(strip, 0.0, egui::Color32::from_rgb(r, g, b));
                    }
                    // Tooltip-Text: Broken-Gründe bzw. Status-Label.
                    let tooltip = if broken {
                        format!("Broken: {}", problems.join("; "))
                    } else {
                        cue.status.label().to_string()
                    };
                    draw_status_markers(ui, cue.flagged, row_meta);
                    if is_cursor_row {
                        // Playhead = gefülltes Dreieck am Zeilenanfang (Klick setzt
                        // den Playhead, s. u.).
                        let icon_rect = egui::Rect::from_min_size(
                            ui.max_rect().left_top() + egui::vec2(4.0, 6.0),
                            egui::vec2(16.0, 16.0),
                        );
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![
                                icon_rect.left_top(),
                                icon_rect.right_center(),
                                icon_rect.left_bottom(),
                            ],
                            palette::ACCENT,
                            egui::Stroke::NONE,
                        ));
                        ui.interact(icon_rect, ui.id().with("playhead_tip"), Sense::hover())
                            .on_hover_text("Playhead – click the status column to move it here");
                        if broken {
                            // Kaputter Cue auf der Playhead-Zeile: kleines
                            // rotes ✕ neben dem Pfeil.
                            let x = egui::Rect::from_min_size(
                                ui.max_rect().left_top() + egui::vec2(27.0, 8.0),
                                egui::vec2(13.0, 13.0),
                            );
                            icons::paint_at(ui, Icon::Broken, x, palette::ERROR);
                            ui.interact(x, ui.id().with("broken"), Sense::hover())
                                .on_hover_text(tooltip);
                        }
                    } else if broken && !cue.status.is_active() {
                        // Kaputter Cue: rotes ✕ statt Status-Pille
                        // („broken cue").
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(30.0, 20.0), Sense::hover());
                        icons::paint_at(
                            ui,
                            Icon::Broken,
                            egui::Rect::from_center_size(rect.center(), egui::vec2(13.0, 13.0)),
                            palette::ERROR,
                        );
                        response.on_hover_text(tooltip);
                    } else if let Some(icon) = Icon::for_status(cue.status) {
                        // Status-Pille: dezenter Farb-Hintergrund + Icon
                        // (eine Status-Farben-Quelle: theme::status_color).
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(30.0, 20.0), Sense::hover());
                        let color = theme::status_color(cue.status);
                        icons::paint_at(
                            ui,
                            icon,
                            egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0)),
                            color,
                        );
                        let tip = if broken {
                            format!("{} – Broken: {}", cue.status.label(), problems.join("; "))
                        } else {
                            cue.status.label().to_string()
                        };
                        response.on_hover_text(tip);
                        if broken {
                            // Läuft trotz Problem: kleines ✕ am rechten Rand.
                            let x = egui::Rect::from_min_size(
                                ui.max_rect().left_top() + egui::vec2(33.0, 8.0),
                                egui::vec2(12.0, 12.0),
                            );
                            icons::paint_at(ui, Icon::Broken, x, palette::ERROR);
                            ui.interact(x, ui.id().with("broken"), Sense::hover())
                                .on_hover_text(tooltip);
                        }
                    }
                    // Standby (nicht Playhead, heil): leere Zelle (neutral).
                });
                // --- Type-Spalte: Icon (+ Aufklapp-Dreieck bei Gruppen) ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if hidden[col::TYPE] {
                        return;
                    }
                    if is_group {
                        // Aufklapp-Dreieck links vom Icon.
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::click());
                        let icon = if collapsed {
                            Icon::CaretRight
                        } else {
                            Icon::CaretDown
                        };
                        icons::paint_at(ui, icon, rect, dim_color);
                        if resp.clicked() {
                            triangle_clicked = true;
                        }
                        resp.on_hover_text(if collapsed {
                            "Expand group"
                        } else {
                            "Collapse group"
                        });
                        ui.add_space(2.0);
                        icons::draw(ui, Icon::for_cue_type(cue.cue_type), 16.0, dim_color)
                            .on_hover_text(cue_type_tooltip(cue));
                    } else {
                        ui.add_space(10.0);
                        icons::draw(ui, Icon::for_cue_type(cue.cue_type), 16.0, dim_color)
                            .on_hover_text(cue_type_tooltip(cue));
                    }
                });
                // --- Q#-Spalte: Monospace, fett; Doppelklick = Inline-Edit ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if !hidden[col::NUMBER] {
                        cell_rects[EditField::Number as usize] = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    if render_cell_editor(
                        ui,
                        &mut edit,
                        &mut commit_edit,
                        &mut cancel_edit,
                        cue_id,
                        EditField::Number,
                    ) {
                        return;
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(&cue.number)
                            .monospace()
                            .strong()
                            .color(text_color),
                    );
                });
                // --- Name: Kinder eingerückt; Doppelklick = Inline-Edit ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if !hidden[col::NAME] {
                        cell_rects[EditField::Name as usize] = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    if render_cell_editor(
                        ui,
                        &mut edit,
                        &mut commit_edit,
                        &mut cancel_edit,
                        cue_id,
                        EditField::Name,
                    ) {
                        return;
                    }
                    ui.add_space(6.0);
                    if is_child {
                        ui.add_space(20.0);
                    }
                    ui.label(RichText::new(&cue.name).color(name_color));
                });
                // --- Target ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if hidden[col::TARGET] {
                        return;
                    }
                    ui.add_space(6.0);
                    let target = target_display(cue);
                    let color = if target == "–" {
                        palette::TEXT_FAINT
                    } else {
                        dim_color
                    };
                    ui.label(RichText::new(target).color(color));
                });
                // --- Pre-wait: zählt während Pre-Wait sichtbar herunter ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if !hidden[col::PRE_WAIT] {
                        cell_rects[EditField::PreWait as usize] = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    if render_cell_editor(
                        ui,
                        &mut edit,
                        &mut commit_edit,
                        &mut cancel_edit,
                        cue_id,
                        EditField::PreWait,
                    ) {
                        return;
                    }
                    // Zeiten rechtsbündig + Monospace (herunterzählende
                    // Zeiten springen nicht).
                    let (text, color) = if cue.status == CueStatus::PreWait {
                        let remaining = pre_wait_remaining.unwrap_or(cue.pre_wait);
                        (format_timecode(remaining), palette::PRE_WAIT)
                    } else {
                        (format_timecode(cue.pre_wait), time_color(cue.pre_wait))
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text).monospace().color(color));
                    });
                });
                // --- Duration (Action): Fortschrittsbalken + Restzeit ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    // Fortschrittsbalken in der Action-Spalte, hinter
                    // dem Text; bei Paused eingefroren (Phase 2).
                    if let Some(elapsed) = elapsed {
                        let progress = cue_progress(elapsed, cue.duration);
                        if progress > 0.0
                            && matches!(cue.status, CueStatus::Playing | CueStatus::Paused)
                        {
                            let bar = egui::Rect::from_min_size(
                                ui.max_rect().min,
                                egui::vec2(
                                    ui.max_rect().width() * progress,
                                    ui.max_rect().height(),
                                ),
                            );
                            ui.painter().rect_filled(
                                bar,
                                0.0,
                                theme::progress_fill(theme::status_color(cue.status)),
                            );
                        }
                    }
                    if !hidden[col::DURATION] {
                        cell_rects[EditField::Duration as usize] = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    if render_cell_editor(
                        ui,
                        &mut edit,
                        &mut commit_edit,
                        &mut cancel_edit,
                        cue_id,
                        EditField::Duration,
                    ) {
                        return;
                    }
                    // laufende Cues zeigen die verbleibende Zeit. Basis ist
                    // die Cue-Duration, sonst die erkannte Medien-Dauer (duration=0).
                    let row_meta = meta.get(&cue_id);
                    let media_dur = row_meta.and_then(|m| m.media_duration);
                    let total = if cue.duration.is_zero() {
                        media_dur.unwrap_or(cue.duration)
                    } else {
                        cue.duration
                    };
                    let (text, color) = match elapsed {
                        Some(el)
                            if matches!(cue.status, CueStatus::Playing | CueStatus::Paused)
                                && !total.is_zero() =>
                        {
                            (
                                format_timecode(total.saturating_sub(el)),
                                theme::status_color(cue.status),
                            )
                        }
                        _ => (format_timecode(total), time_color(total)),
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text).monospace().color(color));
                    });
                });
                // --- Post-wait: Fortschrittsbalken + Countdown ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if cue.status == CueStatus::PostWait && !cue.post_wait.is_zero() {
                        if let Some(elapsed) = elapsed {
                            let post_elapsed = elapsed.saturating_sub(cue.duration);
                            let progress = cue_progress(post_elapsed, cue.post_wait);
                            if progress > 0.0 {
                                let bar = egui::Rect::from_min_size(
                                    ui.max_rect().min,
                                    egui::vec2(
                                        ui.max_rect().width() * progress,
                                        ui.max_rect().height(),
                                    ),
                                );
                                ui.painter().rect_filled(
                                    bar,
                                    0.0,
                                    theme::progress_fill(palette::POST_WAIT),
                                );
                            }
                        }
                    }
                    if !hidden[col::POST_WAIT] {
                        cell_rects[EditField::PostWait as usize] = Some(ui.max_rect());
                    } else {
                        return;
                    }
                    if render_cell_editor(
                        ui,
                        &mut edit,
                        &mut commit_edit,
                        &mut cancel_edit,
                        cue_id,
                        EditField::PostWait,
                    ) {
                        return;
                    }
                    // Während Post-Wait zählt die Zelle herunter.
                    let (text, color) =
                        if cue.status == CueStatus::PostWait && !cue.post_wait.is_zero() {
                            let post_elapsed = elapsed
                                .map(|e| e.saturating_sub(cue.duration))
                                .unwrap_or_default();
                            (
                                format_timecode(cue.post_wait.saturating_sub(post_elapsed)),
                                palette::POST_WAIT,
                            )
                        } else {
                            (format_timecode(cue.post_wait), time_color(cue.post_wait))
                        };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(6.0);
                        ui.label(RichText::new(text).monospace().color(color));
                    });
                });
                // --- Continue-Mode: Icon (Spalte „Continue") ---
                row.col(|ui| {
                    paint_row_bg(
                        &painter,
                        ui.max_rect(),
                        ui.spacing().item_spacing.x,
                        is_cursor_row,
                        is_selected,
                        cue.color,
                        dimmed,
                    );
                    if hidden[col::CONTINUE] {
                        return;
                    }
                    let mode = cue.continue_mode;
                    // „Do not continue" = leere Zelle (docs/UI.md §5.2).
                    if mode != ContinueMode::Off {
                        ui.add_space(12.0);
                        icons::draw(ui, Icon::for_continue_mode(mode), 14.0, palette::ACCENT)
                            .on_hover_text(format!("Continue: {}", mode.label()));
                    }
                    // Verbindungs-Indikator: Dieser Cue ketten zu einem
                    // nächsten Top-Level-Cue („Cue Sequence").
                    if !is_child
                        && mode != ContinueMode::Off
                        && rows[index + 1..].iter().any(|r| r.child.is_none())
                    {
                        let r = ui.max_rect();
                        connectors.push((r.center().x, r.bottom()));
                    }
                });

                // Aufklapp-Dreieck: nach allen Spalten anwenden (borrow-sicher).
                if triangle_clicked {
                    cue_list.set_collapsed(list_index, !collapsed);
                }

                // Nach allen Spalten: Union aller Zellen = ganze Zeile.
                // WICHTIG: `row.response()` darf erst nach dem ersten `col()`
                // gelesen werden – egui_extras panict sonst.
                let response = row.response();
                let clicked = response.clicked();
                // `Response::union` (egui 0.30) überträgt das `double_clicked`-
                // Feld der Zellen NICHT auf die Zeilen-Union. Doppelklick daher
                // mit derselben Formel rekonstruieren, die egui intern in
                // `Response::double_clicked_by` verwendet.
                let double_clicked = clicked
                    && ctx.input(|i| {
                        i.pointer
                            .button_double_clicked(egui::PointerButton::Primary)
                    });

                // --- Drag & Drop (nur Top-Level, Edit-Mode, kein Zell-Edit) ---
                if editable && !is_child && edit.is_none() {
                    response.dnd_set_drag_payload(DragPayload { index: list_index });
                }
                if editable && !is_child {
                    // Drop-Ziel: dezenter Rahmen während des Hover.
                    if response.dnd_hover_payload::<DragPayload>().is_some() {
                        painter.rect_stroke(
                            response.rect,
                            0.0,
                            egui::Stroke::new(1.5_f32, palette::ACCENT),
                        );
                    }
                    // Drop: Aktion vormerken (Ausführung nach der Tabelle).
                    if let Some(payload) = response.dnd_release_payload::<DragPayload>() {
                        let from = payload.index;
                        if from != list_index && from < cue_list.len() {
                            if is_group {
                                // Drop auf eine Gruppe = hineinlegen.
                                actions.move_into_group = Some((from, list_index));
                            } else {
                                actions.move_to = Some((from, list_index));
                            }
                        }
                    }
                }

                // Playhead per Drag in der Status-Spalte verschieben.
                if response.drag_started() || response.dragged() {
                    if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                        if status_rect.is_some_and(|r| r.contains(pos)) && editable {
                            cue_list.set_cursor(list_index);
                        }
                    }
                }
                if clicked || double_clicked {
                    // In welcher Spalte wurde geklickt? (Status-Spalte
                    // setzt den Playhead, der Rest selektiert/editiert.)
                    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
                    let in_status_column =
                        pointer_pos.is_some_and(|pos| status_rect.is_some_and(|r| r.contains(pos)));
                    // WICHTIG: egui setzt bei einem Doppelklick `clicked` UND
                    // `double_clicked` gleichzeitig – der Doppelklick muss
                    // zuerst geprüft werden.
                    if double_clicked && editable {
                        // Doppelklick auf Q#/Name/Pre/Duration/Post = Inline-Edit.
                        if let Some(field) =
                            pointer_pos.and_then(|pos| editable_field_at(&cell_rects, pos))
                        {
                            // Anderer aktiver Editor wird übernommen.
                            if let Some(e) = &edit {
                                if e.cue_id != cue_id || e.field != field {
                                    commit_edit = Some(e.clone());
                                }
                            }
                            // Cue frisch borgen (der Zellen-Borrow `cue` darf
                            // die mutablen Aktionen davor nicht überdauern).
                            let cue_ref = match row_ref.child {
                                Some(c) => cue_list
                                    .cues()
                                    .get(list_index)
                                    .and_then(|g| g.children.get(c)),
                                None => cue_list.cues().get(list_index),
                            };
                            if let Some(cue_ref) = cue_ref {
                                start_edit = Some(CellEdit {
                                    cue_id,
                                    field,
                                    text: initial_edit_text(field, cue_ref),
                                    widget_id: edit_widget_id(cue_id, field),
                                    focus_requested: true,
                                });
                            }
                        } else if !in_status_column {
                            // Doppelklick außerhalb editierbarer Zellen: selektieren.
                            cue_list.select_id(Some(cue_id));
                        }
                    } else if clicked {
                        if in_status_column {
                            // Status-Spalte: Playhead setzen.
                            if editable {
                                cue_list.set_cursor(list_index);
                            }
                        } else {
                            let modifiers = ctx.input(|i| i.modifiers);
                            if modifiers.command {
                                // ⌘-Klick: Cue zur Selektion hinzufügen/entfernen.
                                cue_list.toggle_select_id(cue_id);
                            } else if modifiers.shift {
                                // ⇧-Klick: Bereich vom Anker bis hier.
                                cue_list.select_range_to(cue_id);
                            } else {
                                // Einfachklick: selektieren (auch Group-Kinder, Item 7).
                                cue_list.select_id(Some(cue_id));
                                // Klick auf die Q#-Spalte bewegt zusätzlich
                                // den Playhead (GO triggert dann ab dieser Zeile).
                                if editable
                                    && pointer_pos.is_some_and(|pos| {
                                        cell_rects[EditField::Number as usize]
                                            .is_some_and(|r| r.contains(pos))
                                    })
                                {
                                    cue_list.set_cursor(list_index);
                                }
                            }
                        }
                    }
                }

                // Show-Mode: nur der Playhead, Bearbeiten bleibt gesperrt.
                if !editable && !is_child {
                    response.context_menu(|ui| {
                        if ui.button("Set Playhead Here").clicked() {
                            cue_list.set_cursor(list_index);
                            ui.close_menu();
                        }
                    });
                }

                if editable {
                    // Effektive Auswahl: Zeile Teil einer Mehrfachauswahl →
                    // Aktionen wirken auf die ganze Selektion.
                    let effective: Vec<Uuid> =
                        if cue_list.is_selected(cue_id) && cue_list.selection_len() > 1 {
                            cue_list.selection().to_vec()
                        } else {
                            vec![cue_id]
                        };
                    response.context_menu(|ui| {
                        let multi = effective.len() > 1;
                        let noun = if multi { "Cues" } else { "Cue" };
                        let any_cue = effective.iter().any(|id| cue_list.find_cue(*id).is_some());
                        if ui.button("Set Playhead Here").clicked() {
                            cue_list.set_cursor(list_index);
                            ui.close_menu();
                        }
                        ui.separator();
                        // Arm/Disarm auf die effektive Auswahl.
                        let all_armed = effective
                            .iter()
                            .all(|id| cue_list.find_cue(*id).is_some_and(|c| c.is_armed));
                        let label = format!("{} {noun}", if all_armed { "Disarm" } else { "Arm" });
                        if ui.add_enabled(any_cue, egui::Button::new(label)).clicked() {
                            actions.arm = Some((effective.clone(), !all_armed));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !is_child && any_cue,
                                egui::Button::new(format!("Group {noun}")),
                            )
                            .clicked()
                        {
                            actions.group = effective.clone();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                !is_child && any_cue,
                                egui::Button::new(format!("Duplicate {noun}")),
                            )
                            .clicked()
                        {
                            actions.duplicate = effective.clone();
                            ui.close_menu();
                        }
                        ui.menu_button("Set Color", |ui| {
                            for color in CUE_COLORS {
                                let (rect, resp) =
                                    ui.allocate_exact_size(egui::vec2(110.0, 16.0), Sense::click());
                                ui.painter().rect_filled(
                                    rect,
                                    3.0,
                                    egui::Color32::from_rgb(color[0], color[1], color[2]),
                                );
                                if resp.on_hover_text("Set cue color").clicked() {
                                    actions.color = Some((effective.clone(), Some(color)));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if ui.button("No Color").clicked() {
                                actions.color = Some((effective.clone(), None));
                                ui.close_menu();
                            }
                        });
                        ui.separator();
                        if ui
                            .add_enabled(!is_child, egui::Button::new("Move Up"))
                            .clicked()
                        {
                            cue_list.move_up(list_index);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(!is_child, egui::Button::new("Move Down"))
                            .clicked()
                        {
                            cue_list.move_down(list_index);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(format!("Delete {noun}")).clicked() {
                            // Nur vormerken – der Aufrufer löscht über
                            // `ShowApp::delete_cue_by_id` (stoppt laufende Cues).
                            actions.delete = effective.clone();
                            ui.close_menu();
                        }
                    });
                }
            });
        });

    // Nach der Tabelle: Verbindungs-Indikatoren der Continue-Ketten malen
    // (dezenter Strich über die Zeilengrenze, „Cue Sequence").
    for (x, y) in connectors {
        painter.line_segment(
            [
                egui::pos2(x, y - ROW_HEIGHT * 0.25),
                egui::pos2(x, y + ROW_HEIGHT * 0.25),
            ],
            egui::Stroke::new(1.5_f32, palette::ACCENT.gamma_multiply(0.55)),
        );
    }

    // Leerzustand (TODO.md Phase 6): leere Cue-Liste zeigt einen Hinweis
    // (Dateien per Drag & Drop oder Cues über die Toolbox hinzufügen).
    if rows.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(size::SPACE_LARGE * 2.0);
            ui.label(
                RichText::new("Drag files here or use the Toolbox to add cues")
                    .weak()
                    .size(15.0),
            );
        });
    }

    // Inline-Edit abschließen (nach der Tabelle: cue_list ist frei borrowbar).
    if let Some(e) = commit_edit {
        apply_cell_edit(cue_list, &e);
        if edit
            .as_ref()
            .is_some_and(|x| x.cue_id == e.cue_id && x.field == e.field)
        {
            edit = None;
        }
    }
    if cancel_edit {
        edit = None;
    }
    if let Some(e) = start_edit {
        edit = Some(e);
    }
    // Edit-Zustand zurückspeichern (temp) + Esc-Gate-ID + Fokus anfordern.
    ctx.data_mut(|d| d.insert_temp(edit_id, edit.clone()));
    match &edit {
        Some(e) => {
            ctx.data_mut(|d| d.insert_temp(cell_edit_active_id(), e.widget_id));
            if e.focus_requested {
                let widget_id = e.widget_id;
                if ctx.memory(|m| m.has_focus(widget_id)) {
                    if let Some(e) = &mut edit {
                        e.focus_requested = false;
                    }
                } else {
                    ctx.memory_mut(|m| m.request_focus(widget_id));
                }
            }
        }
        None => {
            ctx.data_mut(|d| d.remove::<egui::Id>(cell_edit_active_id()));
        }
    }

    // Sichtbarkeit für den nächsten Frame merken (Reset-Erkennung).
    columns.prev_hidden = columns.hidden;

    actions
}

/// Spalten-Definition aus dem persistierten Layout: feste Spalten
/// veränderbar (`initial` + `resizable`, TODO.md Phase 3), Name füllt den
/// Rest (ausdrücklich NICHT resizable – sonst würde die Breite nach dem
/// ersten Frame „einfrieren"), versteckte Spalten kollabieren auf 0.
fn column_spec(index: usize, layout: &CueColumnLayout) -> egui_extras::Column {
    use egui_extras::Column;
    /// Minimale Breiten beim Verkleinern (Index = Spalte).
    const MIN_WIDTHS: [f32; CUE_COLUMN_COUNT] =
        [24.0, 24.0, 28.0, 0.0, 60.0, 56.0, 56.0, 56.0, 24.0];
    if layout.hidden[index] {
        return Column::exact(0.0).resizable(false);
    }
    match index {
        col::NAME => Column::remainder().at_least(170.0).resizable(false),
        _ => Column::initial(layout.widths[index])
            .resizable(true)
            .at_least(MIN_WIDTHS[index]),
    }
}

/// Titel einer Spalte im Header-Kontextmenü (auch für die textlosen
/// Spalten Status/Continue).
fn column_menu_title(index: usize) -> &'static str {
    match index {
        col::STATUS => "Status",
        col::CONTINUE => "Continue",
        _ => CUE_COLUMN_TITLES[index],
    }
}

/// Rendert den Zellen-Editor, wenn diese Zelle editiert wird.
/// Rückgabe: `true`, wenn der Editor gerendert wurde (Zelle ist im Edit).
fn render_cell_editor(
    ui: &mut egui::Ui,
    edit: &mut Option<CellEdit>,
    commit_edit: &mut Option<CellEdit>,
    cancel_edit: &mut bool,
    cue_id: Uuid,
    field: EditField,
) -> bool {
    let Some(e) = edit.as_mut() else {
        return false;
    };
    if e.cue_id != cue_id || e.field != field {
        return false;
    }
    let monospace = matches!(
        field,
        EditField::PreWait | EditField::Duration | EditField::PostWait
    );
    let response = ui.add(
        egui::TextEdit::singleline(&mut e.text)
            .id(e.widget_id)
            .desired_width(f32::INFINITY)
            .font(if monospace {
                egui::TextStyle::Monospace
            } else {
                egui::TextStyle::Body
            }),
    );
    // Enter oder Fokusverlust (Klick daneben) übernimmt, Esc bricht ab.
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if enter || (!esc && response.lost_focus()) {
        *commit_edit = Some(e.clone());
    } else if esc {
        *cancel_edit = true;
    }
    true
}

/// Kleine Markierungen am rechten Rand der Status-Spalte: Flag oben, darunter
/// Warnung oder geladen.
fn draw_status_markers(ui: &mut egui::Ui, flagged: bool, meta: Option<&CueRowMeta>) {
    let right = ui.max_rect().right_top();
    let top = egui::Rect::from_min_size(right + egui::vec2(-12.0, 3.0), egui::vec2(10.0, 10.0));
    let bottom = egui::Rect::from_min_size(right + egui::vec2(-12.0, 15.0), egui::vec2(10.0, 10.0));
    if flagged {
        let painter = ui.painter();
        let stroke = egui::Stroke::new(1.5, palette::ACCENT);
        painter.line_segment([top.left_top(), top.left_bottom()], stroke);
        painter.add(egui::Shape::convex_polygon(
            vec![
                top.left_top(),
                top.right_top() + egui::vec2(0.0, 2.5),
                top.left_center(),
            ],
            palette::ACCENT,
            egui::Stroke::NONE,
        ));
        ui.interact(top, ui.id().with("flag"), Sense::hover())
            .on_hover_text("Flagged");
    }
    let Some(meta) = meta else {
        return;
    };
    if !meta.warnings.is_empty() {
        icons::paint_at(ui, Icon::Panic, bottom, palette::ACCENT);
        ui.interact(bottom, ui.id().with("warning"), Sense::hover())
            .on_hover_text(format!("Warning: {}", meta.warnings.join("; ")));
    } else if meta.loaded {
        ui.painter()
            .circle_filled(bottom.center(), 4.0, palette::ACCENT);
        ui.interact(bottom, ui.id().with("loaded"), Sense::hover())
            .on_hover_text("Loaded");
    }
}

/// Start-Text eines Zellen-Editors (aktueller Wert, Zeiten als
/// Timecode `MM:SS.mmm`).
fn initial_edit_text(field: EditField, cue: &Cue) -> String {
    match field {
        EditField::Number => cue.number.clone(),
        EditField::Name => cue.name.clone(),
        EditField::PreWait => format_timecode(cue.pre_wait),
        EditField::Duration => format_timecode(cue.duration),
        EditField::PostWait => format_timecode(cue.post_wait),
    }
}

/// Wendet einen abgeschlossenen Zellen-Edit an (Enter/Fokusverlust).
/// Ungültige Zeit-Eingaben werden verworfen (alter Wert bleibt).
fn apply_cell_edit(cue_list: &mut CueList, edit: &CellEdit) {
    let Some(cue) = cue_list.find_cue_mut(edit.cue_id) else {
        return;
    };
    match edit.field {
        EditField::Number => {
            let text = edit.text.trim();
            if !text.is_empty() {
                cue.number = text.to_string();
            }
        }
        EditField::Name => cue.name = edit.text.clone(),
        EditField::PreWait => {
            if let Some(d) = parse_timecode(&edit.text) {
                cue.pre_wait = d;
            }
        }
        EditField::Duration => {
            if let Some(d) = parse_timecode(&edit.text) {
                cue.duration = d;
            }
        }
        EditField::PostWait => {
            if let Some(d) = parse_timecode(&edit.text) {
                cue.post_wait = d;
            }
        }
    }
}

/// Welche editierbare Zelle liegt an `pos`? (erste passende Spalte)
fn editable_field_at(
    rects: &[Option<egui::Rect>; EDIT_FIELD_COUNT],
    pos: egui::Pos2,
) -> Option<EditField> {
    EDIT_FIELDS
        .iter()
        .find(|f| rects[**f as usize].is_some_and(|r| r.contains(pos)))
        .copied()
}

/// Malt den Zeilen-Hintergrund einer Zelle: Playhead-Zeile hervorheben
/// (unter der Selektion – Selektion unabhängig vom Playhead) und
/// Cue-Color als Tönung über die ganze Zeile (TODO.md Phase 2).
///
/// Lückenlos wie egui_extras' eigene Zell-Hintergründe: um halbes
/// `item_spacing` expandiert. Mit dem Painter des äußeren Ui malen, damit
/// die Expansion nicht am Zell-Clip scheitert und der Hintergrund unter
/// dem Zell-Text landet.
/// Tooltip-Text für das Typ-Icon einer Zeile (TODO.md Phase 6: Tooltips
/// für jedes Icon-only-Element).
fn cue_type_tooltip(cue: &Cue) -> String {
    format!("{} cue", cue.cue_type.label())
}

fn paint_row_bg(
    painter: &egui::Painter,
    cell: egui::Rect,
    item_spacing_x: f32,
    is_cursor_row: bool,
    is_selected: bool,
    cue_color: Option<[u8; 3]>,
    dimmed: bool,
) {
    let gapless = cell.expand2(egui::vec2(item_spacing_x * 0.5, 0.0));
    if is_cursor_row && !is_selected {
        painter.rect_filled(gapless, 0.0, palette::CURSOR_ROW);
    }
    if let Some([r, g, b]) = cue_color {
        painter.rect_filled(
            gapless,
            0.0,
            egui::Color32::from_rgb(r, g, b).gamma_multiply(palette::ROW_TINT),
        );
    }
    // Find (Phase 5): Nicht-Treffer abdunkeln, damit Treffer hervorstechen.
    if dimmed {
        painter.rect_filled(gapless, 0.0, egui::Color32::from_black_alpha(110));
    }
}

/// Fortschritt 0..=1 für die Fortschrittsbalken (Phase 2).
fn cue_progress(elapsed: Duration, total: Duration) -> f32 {
    if total.is_zero() {
        return 0.0;
    }
    (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0) as f32
}

/// Ziel-Anzeige für die Target-Spalte (gekürzt, Dateiname bei Medien).
fn target_display(cue: &Cue) -> String {
    match cue.cue_type {
        CueType::Audio | CueType::Video => cue
            .target_path
            .as_deref()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string())
            })
            .unwrap_or_else(|| "–".to_string()),
        CueType::Control => cue
            .control_target
            .as_deref()
            .map(|t| format!("Q {t}"))
            .unwrap_or_else(|| "all cues".to_string()),
        CueType::Html => "HTML".to_string(),
        CueType::HtmlData => "JSON".to_string(),
        CueType::Network => cue
            .network
            .as_ref()
            .map(|n| format!("{} {}", n.protocol.label().to_lowercase(), n.target()))
            .unwrap_or_else(|| "–".to_string()),
        CueType::Midi => cue
            .midi
            .as_ref()
            .map(|m| m.describe())
            .unwrap_or_else(|| "–".to_string()),
        CueType::Light => cue
            .light
            .as_ref()
            .map(|l| l.describe())
            .unwrap_or_else(|| "–".to_string()),
        CueType::Companion => cue
            .companion
            .as_ref()
            .map(|c| c.describe())
            .unwrap_or_else(|| "–".to_string()),
        CueType::Timecode => cue
            .tc_trigger
            .map(|t| format!("TC {t}"))
            .unwrap_or_else(|| "–".to_string()),
        CueType::Script => {
            if cue.script.trim().is_empty() {
                "–".to_string()
            } else {
                format!("JS ({} chars)", cue.script.len())
            }
        }
        CueType::Text => cue
            .text_content
            .as_ref()
            .map(|t| {
                let first = t.text.lines().next().unwrap_or("").trim();
                if first.is_empty() {
                    "–".to_string()
                } else {
                    let short: String = first.chars().take(24).collect();
                    if first.chars().count() > 24 {
                        format!("{short}…")
                    } else {
                        short
                    }
                }
            })
            .unwrap_or_else(|| "–".to_string()),
        CueType::SlideShow => cue
            .slideshow
            .as_ref()
            .map(|s| format!("{} slides", s.slides.len()))
            .unwrap_or_else(|| "–".to_string()),
        _ => "–".to_string(),
    }
}

// ------------------------------------------------------------------
// Find (TODO.md Phase 5): Suchleiste über der Cue-Liste
// ------------------------------------------------------------------

/// Stabile Widget-ID der Find-Leiste (⌘F requested den Fokus darauf).
pub fn find_bar_id() -> egui::Id {
    egui::Id::new("kuhplay_find_bar")
}

/// Ein Find-Treffer: flacher Zeilen-Bezug (Top-Level oder Group-Kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindHit {
    /// Index in `CueList::cues()` (Top-Level; bei Kindern die Gruppe).
    pub list_index: usize,
    /// Kind-Index innerhalb der Gruppe (`None` = Top-Level-Cue).
    pub child: Option<usize>,
    /// Cue-ID (für Selektion).
    pub id: Uuid,
}

/// Matcht ein Cue den Suchbegriff? Case-insensitive `contains` über
/// Q-Nummer, Name und Notes (TODO.md Phase 5). Leerer Begriff matcht
/// nichts (damit die Liste nicht komplett „leer markiert" wird).
pub fn cue_matches(cue: &Cue, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return false;
    }
    cue.number.to_lowercase().contains(&q)
        || cue.name.to_lowercase().contains(&q)
        || cue.notes.to_lowercase().contains(&q)
}

/// Alle Treffer in Anzeigereihenfolge (Top-Level + sichtbare Group-Kinder,
/// wie die Tabellenzeilen in [`draw`]).
pub fn find_hits(list: &CueList, query: &str) -> Vec<FindHit> {
    let mut hits = Vec::new();
    for (i, cue) in list.cues().iter().enumerate() {
        if cue_matches(cue, query) {
            hits.push(FindHit {
                list_index: i,
                child: None,
                id: cue.id,
            });
        }
        if cue.cue_type == CueType::Group && !cue.collapsed {
            for (c, child) in cue.children.iter().enumerate() {
                if cue_matches(child, query) {
                    hits.push(FindHit {
                        list_index: i,
                        child: Some(c),
                        id: child.id,
                    });
                }
            }
        }
    }
    hits
}

/// Nächster Treffer: Playhead (Cursor) + Selektion auf den Treffer setzen
/// (die Auto-Scroll-Logik in [`draw`] scrollt zur Cursor-Zeile). Wrappt
/// am Ende; bei geändertem Suchbegriff wird wieder vorne begonnen.
pub fn find_next_hit(list: &mut CueList, find: &mut FindState) {
    if find.query != find.query_prev {
        find.current = 0;
        find.query_prev = find.query.clone();
    }
    let hits = find_hits(list, &find.query);
    if hits.is_empty() {
        find.current = 0;
        return;
    }
    let idx = find.current % hits.len();
    let hit = hits[idx];
    list.set_cursor(hit.list_index);
    list.select_id(Some(hit.id));
    find.current += 1;
}

/// Zeichnet die Find-Leiste über der Cue-Liste (TODO.md Phase 5).
///
/// Enter = nächster Treffer (⌘G = Find Next; ⌘G landet in
/// `handle_hotkeys` auf derselben Funktion), Esc schließt die Leiste.
/// Die Textfeld-ID wird in `text_field_ids` registriert, damit das
/// Hotkey-Gate (Space/Esc/Panic) tippen in der Leiste nicht blockiert.
pub fn draw_find_bar(
    ui: &mut egui::Ui,
    list: &mut CueList,
    find: &mut FindState,
    text_field_ids: &mut std::collections::HashSet<egui::Id>,
) {
    // Key-Status VOR dem Widget lesen (ein TextEdit kann Enter konsumieren).
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(0.0, 2.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                icons::draw(ui, Icon::MagnifyingGlass, 13.0, palette::TEXT);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut find.query)
                        .id(find_bar_id())
                        .hint_text("Find cue by number, name or notes…  (⌘F)")
                        .desired_width(280.0),
                );
                text_field_ids.insert(resp.id);
                if resp.has_focus() && enter {
                    find_next_hit(list, find);
                }
                if escape && (resp.has_focus() || resp.lost_focus()) {
                    find.open = false;
                }
                let q = find.query.trim();
                if !q.is_empty() {
                    let hits = find_hits(list, q);
                    let text = if hits.is_empty() {
                        "No matches".to_string()
                    } else {
                        format!("{}/{}", find.current.min(hits.len()), hits.len())
                    };
                    ui.label(RichText::new(text).weak().small());
                }
                let close = ui.add(
                    egui::Button::new(RichText::new("✕").small()).min_size(egui::vec2(20.0, 20.0)),
                );
                if close.clicked() {
                    find.open = false;
                }
                close.on_hover_text("Close find bar (Esc)");
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CueType, GroupMode};

    #[test]
    fn cue_progress_clamps_to_unit_interval() {
        assert_eq!(cue_progress(Duration::ZERO, Duration::from_secs(10)), 0.0);
        assert_eq!(
            cue_progress(Duration::from_secs(5), Duration::from_secs(10)),
            0.5
        );
        assert_eq!(
            cue_progress(Duration::from_secs(20), Duration::from_secs(10)),
            1.0
        );
        // Unbekannte Dauer (Loops): kein Fortschritt.
        assert_eq!(cue_progress(Duration::from_secs(5), Duration::ZERO), 0.0);
    }

    #[test]
    fn initial_edit_text_formats_times_as_timecode() {
        let mut cue = Cue::new("7", "Intro", CueType::Audio);
        cue.pre_wait = Duration::from_millis(1500);
        cue.duration = Duration::from_secs(12);
        cue.post_wait = Duration::ZERO;
        assert_eq!(initial_edit_text(EditField::Number, &cue), "7");
        assert_eq!(initial_edit_text(EditField::Name, &cue), "Intro");
        assert_eq!(initial_edit_text(EditField::PreWait, &cue), "00:01.500");
        assert_eq!(initial_edit_text(EditField::Duration, &cue), "00:12.000");
        assert_eq!(initial_edit_text(EditField::PostWait, &cue), "00:00.000");
    }

    #[test]
    fn apply_cell_edit_updates_fields_and_keeps_invalid_times() {
        let mut list = CueList::new();
        let mut cue = Cue::new("1", "Alt", CueType::Audio);
        cue.pre_wait = Duration::from_secs(2);
        list.add(cue);
        let id = list.cues()[0].id;

        let widget_id = egui::Id::new("test");
        // Name + Nummer.
        apply_cell_edit(
            &mut list,
            &CellEdit {
                cue_id: id,
                field: EditField::Name,
                text: "Neu".to_string(),
                widget_id,
                focus_requested: false,
            },
        );
        apply_cell_edit(
            &mut list,
            &CellEdit {
                cue_id: id,
                field: EditField::Number,
                text: " 42 ".to_string(),
                widget_id,
                focus_requested: false,
            },
        );
        assert_eq!(list.cues()[0].name, "Neu");
        assert_eq!(list.cues()[0].number, "42");

        // Zeitfelder: gültig wird übernommen, ungültig verworfen.
        apply_cell_edit(
            &mut list,
            &CellEdit {
                cue_id: id,
                field: EditField::Duration,
                text: "01:04".to_string(),
                widget_id,
                focus_requested: false,
            },
        );
        assert_eq!(list.cues()[0].duration, Duration::from_secs(64));
        apply_cell_edit(
            &mut list,
            &CellEdit {
                cue_id: id,
                field: EditField::PreWait,
                text: "kaputt".to_string(),
                widget_id,
                focus_requested: false,
            },
        );
        assert_eq!(
            list.cues()[0].pre_wait,
            Duration::from_secs(2),
            "ungültige Zeit-Eingabe bleibt beim alten Wert"
        );

        // Unbekannte ID: kein Panic.
        apply_cell_edit(
            &mut list,
            &CellEdit {
                cue_id: Uuid::new_v4(),
                field: EditField::Name,
                text: "x".to_string(),
                widget_id,
                focus_requested: false,
            },
        );
    }

    #[test]
    fn editable_field_at_picks_the_hit_cell() {
        let rects = [
            Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(60.0, 30.0),
            )),
            Some(egui::Rect::from_min_size(
                egui::pos2(60.0, 0.0),
                egui::vec2(200.0, 30.0),
            )),
            Some(egui::Rect::from_min_size(
                egui::pos2(260.0, 0.0),
                egui::vec2(84.0, 30.0),
            )),
            Some(egui::Rect::from_min_size(
                egui::pos2(344.0, 0.0),
                egui::vec2(84.0, 30.0),
            )),
            Some(egui::Rect::from_min_size(
                egui::pos2(428.0, 0.0),
                egui::vec2(84.0, 30.0),
            )),
        ];
        assert_eq!(
            editable_field_at(&rects, egui::pos2(10.0, 15.0)),
            Some(EditField::Number)
        );
        assert_eq!(
            editable_field_at(&rects, egui::pos2(100.0, 15.0)),
            Some(EditField::Name)
        );
        assert_eq!(
            editable_field_at(&rects, egui::pos2(300.0, 15.0)),
            Some(EditField::PreWait)
        );
        assert_eq!(
            editable_field_at(&rects, egui::pos2(400.0, 15.0)),
            Some(EditField::Duration)
        );
        assert_eq!(
            editable_field_at(&rects, egui::pos2(450.0, 15.0)),
            Some(EditField::PostWait)
        );
        // Außerhalb aller Zellen → kein Edit.
        assert_eq!(editable_field_at(&rects, egui::pos2(1000.0, 15.0)), None);
        // Versteckte Spalte (kein Rect) → kein Edit.
        let mut hidden_rects = rects;
        hidden_rects[EditField::Name as usize] = None;
        assert_eq!(
            editable_field_at(&hidden_rects, egui::pos2(100.0, 15.0)),
            None
        );
    }

    #[test]
    fn column_spec_hides_and_sizes_columns() {
        let mut layout = CueColumnLayout::default();
        // Sichtbar + fest: initial + resizable (TODO.md Phase 3).
        assert!(is_resizable(&column_spec(col::STATUS, &layout)));
        assert!(is_resizable(&column_spec(col::DURATION, &layout)));
        // Name: remainder und NICHT resizable (füllt den Restplatz).
        assert!(!is_resizable(&column_spec(col::NAME, &layout)));
        // Versteckt: exact(0), nicht resizable.
        layout.hidden[col::TARGET] = true;
        assert!(!is_resizable(&column_spec(col::TARGET, &layout)));
    }

    /// Hilfsfunktion: Resizable-Flag einer Spalte lesen (egui_extras::Column
    /// kapselt die Felder privat; hier über den Debug-Output).
    fn is_resizable(column: &egui_extras::Column) -> bool {
        format!("{column:?}").contains("resizable: Some(true)")
    }

    #[test]
    fn group_rows_are_detected_for_triangle() {
        // Deckt die Zeilenmodell-Logik ab: eingeklappte Gruppen ohne Kinder.
        let mut list = CueList::new();
        list.add(Cue::new_group(
            "1",
            "g",
            GroupMode::Simultaneous,
            vec![Cue::new("1.1", "kind", CueType::Audio)],
        ));
        list.add(Cue::new("2", "solo", CueType::Audio));
        // (Direkte rows-Logik ist in draw() – hier nur die Vorbedingung.)
        assert_eq!(list.cues()[0].cue_type, CueType::Group);
        assert!(!list.cues()[0].collapsed);
    }
}
