//! Build-Skript: kompiliert die DeckLink-C++-Bridge (macOS/Linux), wenn das
//! Feature `decklink` aktiv ist und das SDK unter third_party/decklink-sdk-16.0
//! liegt. Ohne SDK läuft der Build normal durch (nativer Pfad deaktiviert,
//! ffmpeg-Fallback). Windows braucht keinen C++-Build (COM-Pfad in Rust).

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/engine/decklink_bridge.cpp");

    let decklink = std::env::var_os("CARGO_FEATURE_DECKLINK").is_some();
    if !decklink {
        return;
    }
    // Ziel-OS des Builds (build.rs läuft auf dem Host!).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // Windows: COM direkt aus Rust (decklink_com.rs) – kein C++ nötig.
    if target_os == "windows" {
        return;
    }
    let plat = match target_os.as_str() {
        "macos" => "macos",
        _ => "linux",
    };

    let inc = Path::new("third_party/decklink-sdk-16.0")
        .join(plat)
        .join("include");
    if !inc.join("DeckLinkAPI.h").exists() {
        println!("cargo:warning=decklink-Feature aktiv, aber SDK fehlt unter {} – nativer Pfad deaktiviert", inc.display());
        return;
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .include(&inc)
        .file("src/engine/decklink_bridge.cpp")
        .file(inc.join("DeckLinkAPIDispatch.cpp"));
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    } else {
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=pthread");
    }
    build.compile("kuhplay_decklink_bridge");
    println!("cargo:rerun-if-changed={}", inc.display());
}
