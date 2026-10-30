# Baut KuhPlay für Windows: dist\kuhplay-<version>-windows-<arch>.zip
# Aufruf: powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1 [-NoWebview]
param([switch]$NoWebview)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo fehlt (https://rustup.rs).'
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$arch = $env:PROCESSOR_ARCHITECTURE.ToLower()
$name = "kuhplay-$version-windows-$arch"

$cargoArgs = @('build', '--release', '--locked')
if (-not $NoWebview) { $cargoArgs += @('--features', 'webview') }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build fehlgeschlagen ($LASTEXITCODE)." }

$out = Join-Path 'dist' $name
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Path $out | Out-Null
Copy-Item 'target\release\kuhplay.exe', 'README.md', 'icon.png' $out
# Die Engine sucht den WebView-Prozess neben dem Haupt-Binary.
if (-not $NoWebview) { Copy-Item 'target\release\kuhplay-webview.exe' $out }

$zip = "$out.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $out -DestinationPath $zip

if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    Write-Warning 'ffmpeg fehlt im PATH, Video-Cues brauchen es zur Laufzeit.'
}
Write-Host "Fertig: $zip"
