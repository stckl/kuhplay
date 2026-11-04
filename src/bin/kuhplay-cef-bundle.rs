//! Legt die CEF-Runtime neben den HTML5-Renderer `kuhplay-cef`, damit er
//! ohne weitere Einrichtung startet – für Entwicklung und Release-Pakete.
//!
//! Ergebnis liegt in einem Ordner `cef/` (Standard: `target/<profil>/cef/`,
//! dort sucht die Engine; Pakete übernehmen den Ordner).
//!
//! * **macOS:** baut `cef/kuhplay-cef.app` (Layout nach
//!   <https://bitbucket.org/chromiumembedded/cef/wiki/GeneralUsage.md>):
//!   `Contents/MacOS/kuhplay-cef`, `Contents/Frameworks/Chromium Embedded
//!   Framework.framework` und die Helper-Apps (`kuhplay-cef Helper (GPU)` …),
//!   die dasselbe Binary enthalten – `kuhplay-cef` zweigt als Unterprozess
//!   per `execute_process` ab.
//! * **Windows/Linux:** kopiert `kuhplay-cef` und die CEF-Dateien (libcef,
//!   Ressourcen, Locales) nach `cef/`.
//!
//! Quelle ist die CEF-Distribution, die `cef-dll-sys` beim Build lädt
//! (oder `CEF_PATH`). Aufruf (nach `cargo build --features cef`):
//!
//! ```text
//! cargo run --features cef --bin kuhplay-cef-bundle -- [ZIELORDNER]
//! ```
//!
//! Ohne Zielordner: `target/<profil>/cef/` neben dem gebauten `kuhplay`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Name des Renderer-Binaries (und des macOS-Bundles).
const RENDERER: &str = "kuhplay-cef";

fn main() {
    if let Err(err) = run() {
        eprintln!("kuhplay-cef-bundle: {err}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let build_dir = std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("kein Build-Ordner"))?;
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| build_dir.join("cef"));
    let cef_dir = cef::sys::get_cef_dir()
        .ok_or_else(|| io::Error::other("CEF-Distribution nicht gefunden (CEF_PATH?)"))?;
    let renderer = build_dir.join(format!("{RENDERER}{}", std::env::consts::EXE_SUFFIX));
    if !renderer.is_file() {
        return Err(io::Error::other(format!(
            "{} fehlt – erst `cargo build --features cef` ausführen",
            renderer.display()
        )));
    }
    fs::create_dir_all(&out)?;
    let result = bundle(&cef_dir, &renderer, &out)?;
    println!("CEF-Renderer bereit: {}", result.display());
    Ok(())
}

#[cfg(target_os = "macos")]
fn bundle(cef_dir: &Path, renderer: &Path, out: &Path) -> io::Result<PathBuf> {
    const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
    // (Name-Zusatz, Bundle-ID-Zusatz) der Helper-Apps.
    const HELPERS: [(&str, &str); 5] = [
        ("Helper", "helper"),
        ("Helper (GPU)", "helper.gpu"),
        ("Helper (Renderer)", "helper.renderer"),
        ("Helper (Plugin)", "helper.plugin"),
        ("Helper (Alerts)", "helper.alerts"),
    ];
    let app = out.join(format!("{RENDERER}.app"));
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    let frameworks = app.join("Contents/Frameworks");
    create_app(&app, RENDERER, "de.kuhplay.cef", renderer)?;
    copy_dir(&cef_dir.join(FRAMEWORK), &frameworks.join(FRAMEWORK))?;
    for (helper, id) in HELPERS {
        let name = format!("{RENDERER} {helper}");
        let id = format!("de.kuhplay.cef.{id}");
        create_app(
            &frameworks.join(format!("{name}.app")),
            &name,
            &id,
            renderer,
        )?;
    }
    Ok(app)
}

/// Ein .app-Bundle mit Info.plist und dem Binary unter `Contents/MacOS/<name>`.
/// Alle Bundles ohne Dock-Icon und Menü (`LSUIElement`) – der Renderer
/// zeichnet nur offscreen.
#[cfg(target_os = "macos")]
fn create_app(app: &Path, name: &str, id: &str, bin: &Path) -> io::Result<()> {
    let macos = app.join("Contents/MacOS");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(app.join("Contents/Frameworks"))?;
    fs::create_dir_all(app.join("Contents/Resources"))?;
    fs::copy(bin, macos.join(name))?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>{name}</string>
	<key>CFBundleIdentifier</key>
	<string>{id}</string>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>LSEnvironment</key>
	<dict>
		<key>MallocNanoZone</key>
		<string>0</string>
	</dict>
	<key>LSUIElement</key>
	<string>1</string>
	<key>NSSupportsAutomaticGraphicsSwitching</key>
	<true/>
</dict>
</plist>
"#,
        version = env!("CARGO_PKG_VERSION"),
    );
    fs::write(app.join("Contents/Info.plist"), plist)
}

#[cfg(not(target_os = "macos"))]
fn bundle(cef_dir: &Path, renderer: &Path, out: &Path) -> io::Result<PathBuf> {
    // libcef + Ressourcen + Locales neben das Binary (CEF sucht sie dort).
    for entry in fs::read_dir(cef_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        // Nur Laufzeit-Dateien: kein Wrapper-Quellcode/Build-Kram, keine
        // Import-Lib und keine Sandbox-Starter (kuhplay-cef läuft ohne Sandbox).
        let skip = name.to_str().is_some_and(|n| {
            matches!(
                n,
                "archive.json" | "include" | "cmake" | "libcef_dll" | "tests" | "CMakeLists.txt"
            ) || n.ends_with(".lib")
                || n.starts_with("bootstrap")
        });
        if skip {
            continue;
        }
        let dst = out.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst)?;
        } else {
            fs::copy(entry.path(), &dst)?;
        }
    }
    let target = out.join(renderer.file_name().expect("Dateiname"));
    fs::copy(renderer, &target)?;
    Ok(target)
}

/// Kopiert einen Ordner rekursiv (Symlinks werden als Ziel kopiert).
fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}
