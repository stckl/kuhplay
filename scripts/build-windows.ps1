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
# H4: DeckLink gehört seit G6 zum Release (SDK liegt unter third_party/).
$features = if ($NoCef) { 'decklink' } else { 'cef,decklink' }
$cargoArgs += @('--features', $features)
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build fehlgeschlagen ($LASTEXITCODE)." }

$out = Join-Path 'dist' $name
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Path $out | Out-Null
Copy-Item 'target\release\kuhplay.exe', 'README.md', 'icon.png' $out
# Die Engine sucht den WebView-Prozess neben dem Haupt-Binary.
if (-not $NoCef) { Copy-Item 'target\release\kuhplay-cef.exe' $out }

# H3: ffmpeg/ffprobe gebündelt neben die exe (vor dem Zip) – schlägt das
# Mitliefern fehl (Download, Prüfsumme, ffmpeg-Checks), bricht der Release-Build ab.
& "$PSScriptRoot\vendor-ffmpeg.ps1" $out

$zip = "$out.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $out -DestinationPath $zip
Write-Host "Fertig: $zip"
