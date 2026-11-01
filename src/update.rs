//! Update-Funktion (Block U4): signierte Release-Manifeste von GitHub Releases.
//!
//! Ablauf: `latest.json` (Manifest mit Version, Notizen, Assets + SHA-256) und
//! `latest.json.sig` (Ed25519-Detached-Signatur über das Manifest) werden aus dem
//! neuesten GitHub-Release geladen. **Ohne gültige Signatur gegen den gepinnten
//! Verifizierungsschlüssel wird nie installiert** – unsignierte oder manipulierte
//! Updates werden abgelehnt. Das Paket selbst wird zusätzlich per SHA-256 geprüft.
//!
//! Der öffentliche Schlüssel kommt zur Build-Zeit über `KUH_UPDATE_PUBKEY_HEX`
//! (Hex, 32 Bytes) aus den CI-Secrets; fehlt er, gibt es keine verifizierten
//! Updates (Check/Anzeige ja, Installation nein). Private Schlüssel liegen nie
//! im Repo.
//!
//! Show-Sicherheit: Prüfung/Hinweise/Download/Installation nie im Show-Mode und
//! nie mientras Cues laufen – das gate't die GUI (`src/gui/app.rs`), nicht dieses
//! Modul.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// GitHub-Repo, dessen Releases als Update-Quelle dienen.
pub const REPO_OWNER: &str = "stckl";
pub const REPO_NAME: &str = "kuhplay";
/// Asset-Namen des Release-Manifests (latest Release → download/<name>).
pub const MANIFEST_ASSET: &str = "latest.json";
pub const SIGNATURE_ASSET: &str = "latest.json.sig";

/// Base-URL, aus der Manifest + Signatur geladen werden (neuester Release).
pub fn feed_base() -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest/download")
}

/// GitHub-API-Endpunkt des neuesten Release (nur für Versions-Check/Anzeige).
pub fn latest_release_api() -> String {
    format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest")
}

/// Aktuelle Version aus Cargo.toml.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Build-Info (Version, Commit, Datum) – Commit/Datum kommen aus build.rs.
pub fn build_info() -> String {
    let sha = option_env!("KUH_GIT_SHA").unwrap_or("dev");
    let date = option_env!("KUH_BUILD_DATE").unwrap_or("unbekannt");
    format!("Version {} (Commit {}, {})", current_version(), sha, date)
}

// ---------------------------------------------------------------------------
// Manifest + Signatur
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub assets: Vec<UpdateAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct UpdateCheck {
    pub manifest: UpdateManifest,
    /// true, wenn die Signatur gegen den gepinnten Schlüssel geprüft wurde.
    pub verified: bool,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<UpdateManifest, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("Manifest ungültig: {e}"))
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut nbits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    Some(out)
}

/// minisign-kompatible Signaturdatei parsen: letzte nicht-leere Zeile ist
/// base64 aus [sig_alg(2)][key_id(8)][ed25519-signatur(64)] – die Signatur
/// liegt über den exakten Datei-Bytes (kein Prehash).
pub fn parse_minisign_signature(sig_file: &[u8]) -> Result<[u8; 64], String> {
    let text = String::from_utf8_lossy(sig_file);
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .ok_or("Signaturdatei leer")?;
    let raw = base64_decode(line.trim()).ok_or("Signatur ist kein base64")?;
    if raw.len() < 64 {
        return Err(format!("Signatur zu kurz ({} Bytes)", raw.len()));
    }
    let sig: [u8; 64] = raw[raw.len() - 64..]
        .try_into()
        .map_err(|_| "Signatur-Bytes ungültig")?;
    Ok(sig)
}

/// minisign-kompatiblen öffentlichen Schlüssel parsen: base64 aus
/// [pk_alg(2) = "ED"][32-Byte-Ed25519-Pubkey].
pub fn minisign_pubkey_from_b64(line: &str) -> Option<[u8; 32]> {
    let raw = base64_decode(line.trim())?;
    if raw.len() < 32 {
        return None;
    }
    raw[raw.len() - 32..].try_into().ok()
}

/// Ed25519-Signatur des Manifests prüfen. 64-Byte-Rohsignatur.
pub fn verify_manifest_signature(
    manifest: &[u8],
    sig: &[u8],
    pubkey: &VerifyingKey,
) -> Result<(), String> {
    let sig = Signature::from_slice(sig).map_err(|e| format!("Signatur ungültig: {e}"))?;
    pubkey.verify(manifest, &sig).map_err(|_| {
        "Signatur-Prüfung fehlgeschlagen (manipuliert oder falscher Schlüssel)".to_string()
    })
}

/// Gepinnter Verifizierungsschlüssel (Hex) aus der Build-Umgebung, falls gesetzt.
pub fn pinned_pubkey() -> Option<VerifyingKey> {
    let hex = option_env!("KUH_UPDATE_PUBKEY_HEX")?.trim();
    let bytes = hex_decode(hex)?;
    VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// "v1.2.3" / "1.2.3" vergleichen – true, wenn `latest` echt neuer als `current`.
pub fn version_newer(latest: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let mut it = v.split('.');
        let maj = it.next()?.parse().ok()?;
        let min = it.next().unwrap_or("0").parse().ok()?;
        let pat = it.next().unwrap_or("0").parse().ok()?;
        Some((maj, min, pat))
    }
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Download + Verifikation
// ---------------------------------------------------------------------------

/// HTTP(S)-GET über das systemeigene `curl` (ab Windows 10 immer dabei).
/// Liefert den Body als Bytes. Reiner Rust-Anteil: Signatur + SHA-256.
fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--retry",
            "2",
            "-A",
            concat!("kuhplay/", env!("CARGO_PKG_VERSION")),
            url,
        ])
        .output()
        .map_err(|e| format!("curl nicht verfügbar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "Download {url} fehlgeschlagen: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Manifest + Signatur aus dem Feed laden und (falls Schlüssel gepinnt)
/// verifizieren. Liefert den Check – `verified=false` heißt: nur Anzeige,
/// keine Installation.
pub fn fetch_update_check(feed_base: &str) -> Result<UpdateCheck, String> {
    let manifest = http_get(&format!("{feed_base}/{MANIFEST_ASSET}"))?;
    let sig = http_get(&format!("{feed_base}/{SIGNATURE_ASSET}")).map_err(|e| {
        format!(
            "Signatur fehlt/fehlgeschlagen ({e}) – unsignierte Updates werden nicht installiert"
        )
    })?;
    let parsed = parse_manifest(&manifest)?;
    let verified = match pinned_pubkey() {
        Some(pk) => {
            let raw_sig = parse_minisign_signature(&sig)?;
            verify_manifest_signature(&manifest, &raw_sig, &pk)?;
            true
        }
        None => false, // kein Schlüssel gepinnt → nicht verifiziert
    };
    Ok(UpdateCheck {
        manifest: parsed,
        verified,
    })
}

/// Passendes Asset für die aktuelle Plattform wählen.
pub fn select_asset<'a>(manifest: &'a UpdateManifest, platform: &str) -> Option<&'a UpdateAsset> {
    manifest.assets.iter().find(|a| {
        let n = a.name.to_lowercase();
        match platform {
            "windows-x64" => n.contains("windows") && n.ends_with(".exe"),
            "macos-arm64" => n.contains("macos") && n.contains("arm64") && n.ends_with(".dmg"),
            "macos-x64" => n.contains("macos") && n.contains("x64") && n.ends_with(".dmg"),
            "linux-x64" => n.contains("linux") && n.ends_with(".deb"),
            _ => false,
        }
    })
}

/// Asset herunterladen, SHA-256 prüfen, ins Zielverzeichnis schreiben.
pub fn download_asset(asset: &UpdateAsset, dest_dir: &Path) -> Result<PathBuf, String> {
    let bytes = http_get(&asset.url)?;
    let got = sha256_hex(&bytes);
    if !got.eq_ignore_ascii_case(&asset.sha256) {
        return Err(format!(
            "SHA-256-Prüfung für {} fehlgeschlagen (erwartet {}, ist {got}) – Download abgebrochen",
            asset.name, asset.sha256
        ));
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("Zielverzeichnis: {e}"))?;
    let dest = dest_dir.join(&asset.name);
    std::fs::write(&dest, &bytes).map_err(|e| format!("Schreiben von {}: {e}", dest.display()))?;
    Ok(dest)
}

/// Plattform-Key für select_asset.
pub fn current_platform() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-arm64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos-x64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x64"
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64")
    )))]
    {
        "unknown"
    }
}

/// Installer/Paket nach erfolgreichem Download+Verify starten (best effort).
/// Danach muss die App neu gestartet werden (GUI führt exit durch).
pub fn run_installer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        // .deb: Installation über pkexec/apt mit Passwortabfrage.
        std::process::Command::new("pkexec")
            .args(["apt", "install", "-y", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| format!("pkexec apt install: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(path)
            .spawn()
            .map_err(|e| format!("Installer starten: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(format!(
            "Kein Installer-Launch für diese Plattform: {}",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_keypair() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::generate(&mut rand_core::OsRng);
        let pk = sk.verifying_key();
        (sk, pk)
    }

    #[test]
    fn version_vergleich() {
        assert!(version_newer("v0.2.0", "0.1.0"));
        assert!(version_newer("0.1.1", "0.1.0"));
        assert!(!version_newer("0.1.0", "0.1.0"));
        assert!(!version_newer("v0.1.0", "0.2.0")); // Downgrade nicht als Update
        assert!(!version_newer("kaputt", "0.1.0"));
    }

    #[test]
    fn manifest_parse() {
        let m = parse_manifest(
            br#"{"version":"0.2.0","notes":"Neu","assets":[{"name":"kuhplay-0.2.0-windows-x64.exe","url":"https://x/y.exe","sha256":"aa"}]}"#,
        )
        .unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.assets.len(), 1);
        assert!(parse_manifest(b"{kaputt").is_err());
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(T[(n >> 18 & 63) as usize] as char);
            out.push(T[(n >> 12 & 63) as usize] as char);
            out.push(if chunk.len() > 1 {
                T[(n >> 6 & 63) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                T[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn minisign_format_kompatibel() {
        let (sk, pk) = test_keypair();
        let manifest = br#"{"version":"0.2.0","assets":[]}"#;
        let sig = sk.sign(manifest);
        // minisign-Datei: Kommentarzeile + base64(["Ed"][key_id 8][sig 64])
        let mut boxbytes = b"Ed".to_vec();
        boxbytes.extend_from_slice(&[0u8; 8]);
        boxbytes.extend_from_slice(&sig.to_bytes());
        let sig_file = format!("untrusted comment: signature\n{}\n", b64(&boxbytes));
        let raw = parse_minisign_signature(sig_file.as_bytes()).unwrap();
        assert_eq!(raw, sig.to_bytes());
        verify_manifest_signature(manifest, &raw, &pk).unwrap();
        // Pubkey im minisign-Format
        let pk_box = format!("untrusted comment: public key\n{}\n", {
            let mut b = b"ED".to_vec();
            b.extend_from_slice(&pk.to_bytes());
            b64(&b)
        });
        let pk_line = pk_box.lines().last().unwrap();
        assert_eq!(minisign_pubkey_from_b64(pk_line).unwrap(), pk.to_bytes());
    }

    #[test]
    fn signatur_pruefung_ok_und_manipuliert() {
        let (sk, pk) = test_keypair();
        let manifest = br#"{"version":"0.2.0","assets":[]}"#;
        let sig = sk.sign(manifest);
        verify_manifest_signature(manifest, &sig.to_bytes(), &pk).unwrap();
        // manipuliertes Manifest → ablehnen
        let tampered = br#"{"version":"9.9.9","assets":[]}"#;
        assert!(verify_manifest_signature(tampered, &sig.to_bytes(), &pk).is_err());
        // falscher Schlüssel → ablehnen
        let (_sk2, pk2) = test_keypair();
        assert!(verify_manifest_signature(manifest, &sig.to_bytes(), &pk2).is_err());
        // kaputte Signatur → ablehnen
        assert!(verify_manifest_signature(manifest, &[7u8; 64], &pk).is_err());
    }

    #[test]
    fn sha256_bekannter_wert() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn asset_auswahl_pro_plattform() {
        let m = parse_manifest(
            br#"{"version":"0.2.0","assets":[
                {"name":"kuhplay-0.2.0-windows-x64.exe","url":"u1","sha256":"aa"},
                {"name":"kuhplay-0.2.0-macos-arm64.dmg","url":"u2","sha256":"bb"},
                {"name":"kuhplay-0.2.0-linux-x64.deb","url":"u3","sha256":"cc"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(select_asset(&m, "windows-x64").unwrap().url, "u1");
        assert_eq!(select_asset(&m, "macos-arm64").unwrap().url, "u2");
        assert_eq!(select_asset(&m, "linux-x64").unwrap().url, "u3");
        assert!(select_asset(&m, "macos-x64").is_none());
    }

    /// U7 (Host-Teil): kompletter Update-Flow gegen einen lokalen Test-Feed –
    /// Manifest signieren, als Server ausliefern, laden, Signatur prüfen,
    /// Asset laden + SHA-256 prüfen; manipulierte Signatur und manipuliertes
    /// Asset müssen abgelehnt werden.
    #[test]
    fn update_flow_gegen_lokalen_feed() {
        use axum::{routing::get, Router};
        let (sk, pk) = test_keypair();
        let asset_bytes = b"paket-inhalt".to_vec();
        let asset_sha = sha256_hex(&asset_bytes);
        // Port vor dem Signieren festlegen, damit Manifest und Signatur passen.
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let manifest = format!(
            r#"{{"version":"9.9.9","notes":"test","assets":[{{"name":"kuhplay-9.9.9-linux-x64.deb","url":"http://127.0.0.1:{port}/pkg.deb","sha256":"{asset_sha}"}}]}}"#
        );
        let sig = sk.sign(manifest.as_bytes());
        let sig_file = {
            let mut b = b"Ed".to_vec();
            b.extend_from_slice(&[0u8; 8]);
            b.extend_from_slice(&sig.to_bytes());
            format!("untrusted comment: signature\n{}\n", b64(&b))
        };

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
                    .await
                    .unwrap();
                let sig = sig_file.clone();
                let asset_bytes = asset_bytes.clone();
                let app = Router::new()
                    .route(
                        "/latest.json",
                        get(move || {
                            let m = manifest.clone();
                            async move { m }
                        }),
                    )
                    .route(
                        "/latest.json.sig",
                        get(move || {
                            let s = sig.clone().into_bytes();
                            async move { s }
                        }),
                    )
                    .route(
                        "/pkg.deb",
                        get(move || {
                            let a = asset_bytes.clone();
                            async move { a }
                        }),
                    );
                axum::serve(listener, app).await.unwrap();
            });
        });
        let feed = format!("http://127.0.0.1:{port}");
        std::thread::sleep(std::time::Duration::from_millis(200)); // Server hochfahren

        // 1) Manifest + Signatur laden und gegen Test-Schlüssel prüfen
        let m_bytes = http_get(&format!("{feed}/latest.json")).unwrap();
        let s_bytes = http_get(&format!("{feed}/latest.json.sig")).unwrap();
        let raw_sig = parse_minisign_signature(&s_bytes).unwrap();
        verify_manifest_signature(&m_bytes, &raw_sig, &pk).unwrap();
        let manifest = parse_manifest(&m_bytes).unwrap();
        assert!(version_newer(&manifest.version, current_version()));

        // 2) Asset-Download + SHA-256 ok
        let asset = select_asset(&manifest, "linux-x64").unwrap();
        let tmp = std::env::temp_dir().join("kuhplay-update-test");
        let path = download_asset(asset, &tmp).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"paket-inhalt");

        // 3) manipulierte Signatur (minisign-Format) → ablehnen
        let bad = sk.sign(b"anderes manifest");
        assert!(verify_manifest_signature(&m_bytes, &bad.to_bytes(), &pk).is_err());

        // 4) manipuliertes Asset (SHA passt nicht) → ablehnen
        let mut bad_asset = asset.clone();
        bad_asset.sha256 = sha256_hex(b"boese");
        assert!(download_asset(&bad_asset, &tmp).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
