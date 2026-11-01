//! H1: Vergleicht die COM-Traits in `src/engine/decklink_com.rs` automatisch
//! gegen die IDL-Dateien des Blackmagic SDK 16.0 – IID und Methodenreihenfolge
//! jedes Interfaces muessen exakt uebereinstimmen (sonst falsche vtable-Slots).
//! Laeuft auf dem Host (macOS/Linux), die IDL-Dateien werden nur gelesen.

use std::collections::BTreeMap;
use std::path::Path;

struct IdlInterface {
    iid: String,
    methods: Vec<String>,
}

struct RustTrait {
    name: String,
    iid: String,
    methods: Vec<String>,
}

fn normalize_iid(iid: &str) -> String {
    iid.trim_matches(|c| c == '{' || c == '}').to_uppercase()
}

fn extract_uuid(attr_block: &str) -> Option<String> {
    let start = attr_block.find("uuid(")? + "uuid(".len();
    let end = attr_block[start..].find(')')? + start;
    Some(normalize_iid(&attr_block[start..end]))
}

/// Kommentare entfernen, damit Klammern in Kommentaren das Parsing nicht stoeren.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c2 in chars.by_ref() {
                    if c2 == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if prev == '*' && c2 == '/' {
                        break;
                    }
                    prev = c2;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn ident_len(s: &str) -> usize {
    s.find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len())
}

/// Parst `] interface <Name> : <Base> { ... };` Bloecke samt uuid aus dem
/// Attributblock davor und sammelt die HRESULT-Methoden in Reihenfolge.
fn parse_idl_interfaces(path: &Path) -> BTreeMap<String, IdlInterface> {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("IDL nicht lesbar {}: {e}", path.display()));
    let text = strip_comments(&raw.replace("\r\n", "\n"));
    let mut out = BTreeMap::new();
    let mut pos = 0;
    // "\n] interface " – so werden "[default] interface X;"-Eintraege in
    // coclass-Bodies (eingerueckt) nicht fälschlich mitgeparst.
    while let Some(rel) = text[pos..].find("\n] interface ") {
        let start = pos + rel + 1; // hinter dem '\n'
        let after = start + "] interface ".len();
        let rest = &text[after..];
        let name = &rest[..ident_len(rest)];
        assert!(
            !name.is_empty(),
            "interface ohne Namen in {}",
            path.display()
        );
        let brace = rest
            .find('{')
            .unwrap_or_else(|| panic!("kein Body fuer {name} in {}", path.display()));
        let rb = rest.as_bytes();
        let mut depth = 1usize;
        let mut i = brace + 1;
        while i < rb.len() && depth > 0 {
            match rb[i] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        assert!(
            depth == 0,
            "Body von {name} nicht geschlossen in {}",
            path.display()
        );
        let body = &rest[brace + 1..i - 1];
        let block_start = text[..start].rfind('[').unwrap_or(0);
        let iid = extract_uuid(&text[block_start..start])
            .unwrap_or_else(|| panic!("keine uuid fuer {name} in {}", path.display()));
        let mut methods = Vec::new();
        for line in body.lines() {
            // Methoden stehen einzeilig als `<RetType> <Name> (<args>);` –
            // der Name ist der Identifier direkt vor der ersten Klammer.
            let Some(paren) = line.find('(') else {
                continue;
            };
            let before = line[..paren].trim_end();
            if before.contains([';', '{', '}']) {
                continue;
            }
            // Methodenname = letztes Identifier-Token vor der Klammer
            // (davor stehen nur Attribute und der Return-Type).
            let id_start = before
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map(|i| i + 1)
                .unwrap_or(0);
            let tail = &before[id_start..];
            if tail.is_empty() {
                continue;
            }
            methods.push(tail.to_string());
        }
        out.insert(name.to_string(), IdlInterface { iid, methods });
        pos = after + name.len();
    }
    out
}

/// Parst `#[interface("IID")] unsafe trait <Name> { fn ... }` aus dem Rust-Source.
fn parse_rust_traits(src: &str) -> Vec<RustTrait> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = src[pos..].find("#[interface(\"") {
        let start = pos + rel;
        let iid_start = start + "#[interface(\"".len();
        let iid_end = src[iid_start..].find('"').unwrap() + iid_start;
        let iid = normalize_iid(&src[iid_start..iid_end]);
        let after = &src[iid_end + 2..]; // `")` ueberspringen
        let tpos = after
            .find("unsafe trait ")
            .expect("kein `unsafe trait` nach #[interface]");
        let rest = &after[tpos + "unsafe trait ".len()..];
        let name = &rest[..ident_len(rest)];
        let brace = rest.find('{').expect("kein Trait-Body");
        let body = &rest[brace + 1..];
        let end = body.find("\n}").expect("Trait-Ende nicht gefunden");
        let mut methods = Vec::new();
        for line in body[..end].lines() {
            if let Some(f) = line.trim_start().strip_prefix("fn ") {
                let n = f.find('(').expect("fn ohne Parameterliste");
                methods.push(f[..n].trim().to_string());
            }
        }
        out.push(RustTrait {
            name: name.to_string(),
            iid,
            methods,
        });
        pos = iid_end + 1;
    }
    out
}

fn extract_coclass_uuid(idl_text: &str, coclass: &str) -> String {
    let needle = format!("] coclass {coclass}");
    let pos = idl_text
        .find(&needle)
        .unwrap_or_else(|| panic!("coclass {coclass} nicht in der IDL"));
    let block_start = idl_text[..pos].rfind('[').unwrap_or(0);
    extract_uuid(&idl_text[block_start..pos]).expect("keine uuid an der coclass")
}

#[test]
fn com_traits_match_sdk16_idl() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let idl_dir = root.join("third_party/decklink-sdk-16.0/windows/include");
    let mut idl = parse_idl_interfaces(&idl_dir.join("DeckLinkAPI.idl"));
    for (name, iface) in parse_idl_interfaces(&idl_dir.join("DeckLinkAPIDiscovery.idl")) {
        idl.insert(name, iface);
    }

    let src_path = root.join("src/engine/decklink_com.rs");
    let src = std::fs::read_to_string(&src_path).unwrap();
    let traits = parse_rust_traits(&src);
    assert!(
        traits.len() >= 7,
        "mindestens 7 COM-Traits in decklink_com.rs erwartet, gefunden {}",
        traits.len()
    );

    for t in &traits {
        let idl_if = idl
            .get(&t.name)
            .unwrap_or_else(|| panic!("Interface {} fehlt in den SDK-16.0-IDLs", t.name));
        assert_eq!(
            t.iid, idl_if.iid,
            "IID von {} weicht vom SDK 16.0 ab (Rust {} vs IDL {})",
            t.name, t.iid, idl_if.iid
        );
        assert_eq!(
            t.methods, idl_if.methods,
            "Methodenliste/Reihenfolge von {} weicht ab\nRust: {:?}\nIDL:  {:?}",
            t.name, t.methods, idl_if.methods
        );
    }

    // Die CLSID des Iterators muss zur coclass im SDK passen.
    let api_idl = std::fs::read_to_string(idl_dir.join("DeckLinkAPI.idl")).unwrap();
    let coclass_uuid = extract_coclass_uuid(&api_idl, "CDeckLinkIterator");
    let hex_only = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>()
            .to_uppercase()
    };
    assert!(
        hex_only(&src).contains(&hex_only(&coclass_uuid)),
        "CLSID CDeckLinkIterator ({coclass_uuid}) fehlt in decklink_com.rs"
    );
}
