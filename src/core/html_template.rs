//! Datenstruktur eines HTML5-Templates (docs/KONZEPT-html-templates.md §4):
//! Das Template beschreibt seine Felder als JSON direkt im HTML,
//!
//! ```html
//! <script type="application/kuhplay+json">
//! { "name": "Timeline", "languages": ["de", "en"],
//!   "fields": { "talks": { "type": "list", "required": true, "minItems": 1,
//!     "item": { "start": { "type": "time", "required": true },
//!               "title": { "type": "text", "localized": true } } } } }
//! </script>
//! ```
//!
//! KuhPlay liest das, ohne die Seite zu starten: Feldübersicht im Inspector,
//! Beispiel-JSON zum Ausfüllen und Prüfung der Daten (fehlende Pflichtfelder
//! → Cue kaputt).
//!
//! Feld-Typen: `text`, `multiline`, `number`, `bool`, `time` (`HH:MM`),
//! `timestamp`, `duration`, `image`, `url`, `color`, `select` (mit
//! `options`), `list` (mit `item`) und `group` (mit `fields`). Weitere
//! Angaben: `label`, `required`, `localized` (ein Wert je Sprache aus
//! `languages`), `minItems`, `default`, `example`, `description`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::{json, Map, Value};

/// Öffnendes Tag der Felddefinition.
const SCHEMA_TAG: &str = "<script type=\"application/kuhplay+json\">";

/// Felddefinition eines Templates (das JSON aus dem `<script>`-Block).
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateSchema {
    /// Anzeigename (`name`), falls angegeben.
    pub name: Option<String>,
    /// Sprachen für `localized`-Felder (Standard: keine Unterteilung).
    pub languages: Vec<String>,
    /// Felder auf oberster Ebene (`fields`).
    pub fields: Map<String, Value>,
}

/// Eine Zeile der Feldübersicht (für den Inspector).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    /// Pfad im JSON, z. B. `speakers[].time`.
    pub path: String,
    pub kind: String,
    pub label: Option<String>,
    pub required: bool,
    pub localized: bool,
    pub description: Option<String>,
}

impl TemplateSchema {
    /// Liest die Felddefinition aus einem HTML-Dokument. `None` = das
    /// Template beschreibt keine Daten; `Some(Err)` = Block kaputt.
    pub fn from_html(html: &str) -> Option<Result<Self, String>> {
        let start = html.find(SCHEMA_TAG)? + SCHEMA_TAG.len();
        let end = html[start..].find("</script>")? + start;
        Some(Self::from_json(&html[start..end]))
    }

    /// Liest die Felddefinition aus dem JSON des `<script>`-Blocks.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|err| format!("Felddefinition: {err}"))?;
        let fields = value
            .get("fields")
            .and_then(Value::as_object)
            .cloned()
            .ok_or("Felddefinition: \"fields\" fehlt")?;
        let languages = value
            .get("languages")
            .and_then(Value::as_array)
            .map(|langs| {
                langs
                    .iter()
                    .filter_map(|l| l.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            name: value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            languages,
            fields,
        })
    }

    /// Beispiel-JSON zum Ausfüllen: jedes Feld mit `default`/`example` bzw.
    /// einem Platzhalter seines Typs, Listen mit einem Eintrag.
    pub fn example(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|(name, spec)| (name.clone(), self.example_of(spec)))
                .collect(),
        )
    }

    fn example_of(&self, spec: &Value) -> Value {
        if let Some(v) = spec.get("default").or_else(|| spec.get("example")) {
            return v.clone();
        }
        let localized = spec.get("localized").and_then(Value::as_bool) == Some(true);
        let single = match kind_of(spec) {
            "number" => json!(0),
            "bool" => json!(false),
            "time" => json!("09:00"),
            "timestamp" => json!("2026-01-01T09:00:00"),
            "duration" => json!("00:30"),
            "color" => spec
                .get("options")
                .and_then(Value::as_array)
                .and_then(|o| o.first())
                .map(option_value)
                .unwrap_or_else(|| json!("#ffffff")),
            "select" => spec
                .get("options")
                .and_then(Value::as_array)
                .and_then(|o| o.first())
                .map(option_value)
                .unwrap_or(Value::Null),
            "list" => {
                let item = spec.get("item").cloned().unwrap_or(json!({}));
                return json!([self.example_of(&json!({ "type": "group", "fields": item }))]);
            }
            "group" => {
                let fields = spec.get("fields").and_then(Value::as_object);
                return Value::Object(
                    fields
                        .into_iter()
                        .flatten()
                        .map(|(name, spec)| (name.clone(), self.example_of(spec)))
                        .collect(),
                );
            }
            _ => json!(""),
        };
        if localized && !self.languages.is_empty() {
            Value::Object(
                self.languages
                    .iter()
                    .map(|lang| (lang.clone(), single.clone()))
                    .collect(),
            )
        } else {
            single
        }
    }

    /// Prüft Daten gegen die Felder. Leere Liste = in Ordnung.
    pub fn validate(&self, data: &Value) -> Vec<String> {
        let mut problems = Vec::new();
        match data.as_object() {
            Some(obj) => self.check_fields(&self.fields, obj, "", &mut problems),
            None => problems.push("Daten müssen ein JSON-Objekt sein".to_string()),
        }
        problems
    }

    fn check_fields(
        &self,
        fields: &Map<String, Value>,
        data: &Map<String, Value>,
        prefix: &str,
        problems: &mut Vec<String>,
    ) {
        for (name, spec) in fields {
            let path = format!("{prefix}{name}");
            let required = spec.get("required").and_then(Value::as_bool) == Some(true);
            match data.get(name) {
                None | Some(Value::Null) => {
                    if required {
                        problems.push(format!("{path}: Pflichtfeld fehlt"));
                    }
                }
                Some(value) => self.check_value(spec, value, &path, required, problems),
            }
        }
    }

    fn check_value(
        &self,
        spec: &Value,
        value: &Value,
        path: &str,
        required: bool,
        problems: &mut Vec<String>,
    ) {
        let localized = spec.get("localized").and_then(Value::as_bool) == Some(true);
        if localized {
            if let Some(per_lang) = value.as_object() {
                for (lang, v) in per_lang {
                    self.check_single(spec, v, &format!("{path}.{lang}"), required, problems);
                }
                if required {
                    if let Some(first) = self.languages.first() {
                        if !per_lang.contains_key(first) {
                            problems.push(format!("{path}.{first}: Pflichtfeld fehlt"));
                        }
                    }
                }
                return;
            }
        }
        self.check_single(spec, value, path, required, problems);
    }

    fn check_single(
        &self,
        spec: &Value,
        value: &Value,
        path: &str,
        required: bool,
        problems: &mut Vec<String>,
    ) {
        let kind = kind_of(spec);
        let ok = match kind {
            "number" => value.is_number(),
            "bool" => value.is_boolean(),
            "list" => {
                let Some(items) = value.as_array() else {
                    problems.push(format!("{path}: Liste erwartet"));
                    return;
                };
                let min = spec.get("minItems").and_then(Value::as_u64).unwrap_or(0);
                if (items.len() as u64) < min {
                    problems.push(format!("{path}: mindestens {min} Einträge"));
                }
                let item = spec.get("item").and_then(Value::as_object);
                for (i, entry) in items.iter().enumerate() {
                    match (entry.as_object(), item) {
                        (Some(obj), Some(item)) => {
                            self.check_fields(item, obj, &format!("{path}[{i}]."), problems)
                        }
                        (None, Some(_)) => problems.push(format!("{path}[{i}]: Objekt erwartet")),
                        _ => {}
                    }
                }
                return;
            }
            "group" => {
                let Some(obj) = value.as_object() else {
                    problems.push(format!("{path}: Objekt erwartet"));
                    return;
                };
                if let Some(fields) = spec.get("fields").and_then(Value::as_object) {
                    self.check_fields(fields, obj, &format!("{path}."), problems);
                }
                return;
            }
            "select" => {
                let options: Vec<Value> = spec
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|o| o.iter().map(option_value).collect())
                    .unwrap_or_default();
                if !options.is_empty() && !options.contains(value) {
                    problems.push(format!("{path}: kein erlaubter Wert"));
                }
                return;
            }
            "time" => value.as_str().is_some_and(|t| t.is_empty() || is_time(t)),
            _ => value.is_string(),
        };
        if !ok {
            problems.push(format!("{path}: {} erwartet", kind_label(kind)));
        } else if required && value.as_str().is_some_and(|t| t.trim().is_empty()) {
            problems.push(format!("{path}: Pflichtfeld ist leer"));
        }
    }

    /// Feldübersicht (verschachtelt als Pfade) für den Inspector.
    pub fn describe(&self) -> Vec<FieldInfo> {
        let mut out = Vec::new();
        describe_fields(&self.fields, "", &mut out);
        out
    }
}

fn describe_fields(fields: &Map<String, Value>, prefix: &str, out: &mut Vec<FieldInfo>) {
    for (name, spec) in fields {
        let path = format!("{prefix}{name}");
        let kind = kind_of(spec);
        out.push(FieldInfo {
            path: path.clone(),
            kind: kind.to_string(),
            label: spec
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_string),
            required: spec.get("required").and_then(Value::as_bool) == Some(true),
            localized: spec.get("localized").and_then(Value::as_bool) == Some(true),
            description: spec
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
        match kind {
            "list" => {
                if let Some(item) = spec.get("item").and_then(Value::as_object) {
                    describe_fields(item, &format!("{path}[]."), out);
                }
            }
            "group" => {
                if let Some(sub) = spec.get("fields").and_then(Value::as_object) {
                    describe_fields(sub, &format!("{path}."), out);
                }
            }
            _ => {}
        }
    }
}

/// Typ eines Felds (`type`, Standard `text`).
fn kind_of(spec: &Value) -> &str {
    spec.get("type").and_then(Value::as_str).unwrap_or("text")
}

/// Wert einer Option: `"rot"` oder `{"value": "#c00", "label": "Rot"}`.
fn option_value(option: &Value) -> Value {
    option
        .get("value")
        .cloned()
        .unwrap_or_else(|| option.clone())
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "number" => "Zahl",
        "bool" => "Ja/Nein",
        "time" => "Uhrzeit (HH:MM)",
        _ => "Text",
    }
}

/// `HH:MM` oder `HH:MM:SS`.
fn is_time(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
}

/// Cache der Felddefinitionen von Template-Dateien (neu gelesen, wenn sich
/// die Datei ändert) – der Inspector fragt jeden Frame.
#[derive(Default)]
pub struct TemplateSchemaCache {
    entries: Mutex<HashMap<PathBuf, CachedSchema>>,
}

/// Zeitstempel der Datei + gelesene Felddefinition.
type CachedSchema = (Option<SystemTime>, Option<Result<TemplateSchema, String>>);

impl TemplateSchemaCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Felddefinition der Datei `path` (`None` = keine oder unlesbar).
    pub fn for_file(&self, path: &Path) -> Option<Result<TemplateSchema, String>> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let mut entries = self.entries.lock().unwrap();
        if let Some((at, schema)) = entries.get(path) {
            if *at == modified {
                return schema.clone();
            }
        }
        let schema = std::fs::read_to_string(path)
            .ok()
            .and_then(|html| TemplateSchema::from_html(&html));
        entries.insert(path.to_path_buf(), (modified, schema.clone()));
        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &str = r##"<html><head>
<script type="application/kuhplay+json">
{ "name": "Timeline", "languages": ["de", "en"],
  "fields": {
    "color": { "type": "color", "options": [{"value": "#c6b597", "label": "Gold"}] },
    "speakers": { "type": "list", "required": true, "minItems": 1,
      "item": {
        "time":  { "type": "time", "required": true },
        "name":  { "type": "text", "required": true },
        "title": { "type": "text", "localized": true, "required": true },
        "photo": { "type": "image" } } } } }
</script></head><body></body></html>"##;

    fn schema() -> TemplateSchema {
        TemplateSchema::from_html(HTML).unwrap().unwrap()
    }

    #[test]
    fn reads_the_schema_block_from_html() {
        let s = schema();
        assert_eq!(s.name.as_deref(), Some("Timeline"));
        assert_eq!(s.languages, vec!["de", "en"]);
        assert!(TemplateSchema::from_html("<html></html>").is_none());
        let broken = r#"<script type="application/kuhplay+json">{oops</script>"#;
        assert!(TemplateSchema::from_html(broken).unwrap().is_err());
    }

    #[test]
    fn example_follows_types_languages_and_lists() {
        let example = schema().example();
        assert_eq!(example["color"], "#c6b597");
        assert_eq!(example["speakers"][0]["time"], "09:00");
        assert_eq!(example["speakers"][0]["title"], json!({"de": "", "en": ""}));
        assert_eq!(example["speakers"][0]["photo"], "");
    }

    #[test]
    fn validation_reports_missing_and_wrong_fields() {
        let s = schema();
        let good = json!({ "speakers": [
            { "time": "09:15", "name": "Dr. Koch", "title": { "de": "Arthrose", "en": "Arthritis" } }
        ]});
        assert!(s.validate(&good).is_empty(), "{:?}", s.validate(&good));
        let bad = json!({ "speakers": [ { "time": "9 Uhr", "title": { "en": "x" } } ] });
        let problems = s.validate(&bad);
        assert!(
            problems
                .iter()
                .any(|p| p == "speakers[0].name: Pflichtfeld fehlt"),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.starts_with("speakers[0].time:")),
            "{problems:?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p == "speakers[0].title.de: Pflichtfeld fehlt"),
            "{problems:?}"
        );
        assert_eq!(s.validate(&json!({})), vec!["speakers: Pflichtfeld fehlt"]);
        assert_eq!(
            s.validate(&json!({ "speakers": [] })),
            vec!["speakers: mindestens 1 Einträge"]
        );
    }

    #[test]
    fn describe_lists_nested_paths() {
        let paths: Vec<String> = schema().describe().into_iter().map(|f| f.path).collect();
        assert_eq!(
            paths,
            vec![
                "color",
                "speakers",
                "speakers[].time",
                "speakers[].name",
                "speakers[].title",
                "speakers[].photo"
            ]
        );
    }
}
