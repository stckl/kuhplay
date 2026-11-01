//! Build-Skript: kompiliert die DeckLink-C++-Bridge (macOS/Linux), wenn das
//! Feature `decklink` aktiv ist und das SDK unter third_party/decklink-sdk-16.0
//! liegt. Ohne SDK läuft der Build normal durch (nativer Pfad deaktiviert).
//! Windows braucht keinen C++-Build (COM-Pfad in Rust).

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // U1: Build-Info für About/Web-Remote (nie Secrets, nur öffentlicher Commit).
    let sha = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=KUH_GIT_SHA={sha}");
    let date = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unbekannt".to_string());
    println!("cargo:rustc-env=KUH_BUILD_DATE={date}");
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
