# H3: ffmpeg + ffprobe als LGPL-Builds neben die App legen (Windows).
# Gepinnt auf BtbN/FFmpeg-Builds LGPL-Variante (statisch, kein --enable-gpl,
# --disable-libx264/-libx265), Release-Tag autobuild-2026-10-31-12-59,
# SHA-256 aus dessen checksums.sha256 – keine beweglichen "release"-Links.
# Nach dem Installieren: Hash-, Versions-, LGPL-Config-, HW-Decoder- und
# Decode-Smoke-Test. Schlägt etwas fehl, bricht das Skript ab (ErrorActionPreference
# Stop) – der Release-Build muss dann abbrechen.
# Aufruf: scripts\vendor-ffmpeg.ps1 <Zielverzeichnis>
$ErrorActionPreference = 'Stop'
param([Parameter(Mandatory = $true)][string]$Dest)

$BtbnTag  = 'autobuild-2026-10-31-12-59'
$BtbnVer  = 'n9.0.2-39-g7adfbe40dc'   # ffmpeg 9.0.2, LGPL-Variante
$Expected = @{
  'AMD64'  = @{ Asset = "ffmpeg-$BtbnVer-win64-lgpl-9.0.zip";    Sha256 = '9d37c564e0d01c18cde184a079fa6ef04d7d2783ada41da81d04165cc8f1a650' }
  'ARM64'  = @{ Asset = "ffmpeg-$BtbnVer-winarm64-lgpl-9.0.zip"; Sha256 = 'd1981518b99a1c05d6b864200b1ac9459b09e2ee174feffe84b6e6bb0bfa79e2' }
}
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'ARM64' } else { 'AMD64' }
if (-not $Expected.ContainsKey($arch)) { throw "Nicht unterstützte Architektur: $($env:PROCESSOR_ARCHITECTURE)" }
$pin = $Expected[$arch]

New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$work = Join-Path $env:TEMP ("ffmpeg-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
  $zip = Join-Path $work 'ffmpeg.zip'
  $url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$BtbnTag/$($pin.Asset)"
  Write-Host "Lade ffmpeg LGPL ($($pin.Asset)) …"
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $zip
  $got = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
  if ($got -ne $pin.Sha256) {
    throw "SHA-256-Prüfung fehlgeschlagen für $($pin.Asset): erwartet $($pin.Sha256), ist $got"
  }
  Expand-Archive -Path $zip -DestinationPath $work -Force
  $bin = Get-ChildItem -Recurse -Filter ffmpeg.exe -Path $work | Select-Object -First 1
  $probe = Get-ChildItem -Recurse -Filter ffprobe.exe -Path $work | Select-Object -First 1
  Copy-Item $bin.FullName (Join-Path $Dest 'ffmpeg.exe') -Force
  Copy-Item $probe.FullName (Join-Path $Dest 'ffprobe.exe') -Force

  $ff = Join-Path $Dest 'ffmpeg.exe'
  $out = & $ff -version
  if ($out[0] -notmatch '^ffmpeg version n?9\.0\.2') { throw "Unerwartete ffmpeg-Version: $($out[0])" }
  $cfg = ($out | Where-Object { $_ -match '^configuration:' }) -join ' '
  if ($cfg -match '--enable-gpl|--enable-nonfree|--enable-libx264|--enable-libx265') {
    throw "FEHLER: Build enthält GPL-/Nonfree-Komponenten – nicht als LGPL ausliefern!`n$cfg"
  }
  $hw = (& $ff -hide_banner -hwaccels 2>$null) | Where-Object { $_ -match '^[a-z0-9_]+$' }
  if (-not $hw) { throw 'Keine Hardware-Beschleuniger gemeldet (-hwaccels leer).' }
  $smoke = Join-Path $work 'smoke.avi'
  & $ff -hide_banner -loglevel error -f lavfi -i testsrc=duration=0.4:size=160x120:rate=10 -c:v mpeg4 -f avi -y $smoke
  if ($LASTEXITCODE -ne 0) { throw 'Smoke-Test (Encode) fehlgeschlagen.' }
  & $ff -hide_banner -loglevel error -hwaccel auto -i $smoke -f null -
  if ($LASTEXITCODE -ne 0) { throw 'Smoke-Test (Decode mit -hwaccel auto) fehlgeschlagen.' }
  ($out -join "`r`n") | Set-Content -Path (Join-Path $Dest 'BUILD-ffmpeg.txt') -Encoding UTF8

  @"
ffmpeg/ffprobe in diesem Verzeichnis: statischer LGPL-Build von BtbN/FFmpeg-Builds
($BtbnTag, $($pin.Asset)), SHA-256 geprüft.
Lizenz: GNU LGPL v2.1 oder später. Der Build enthält keine GPL- oder
Nonfree-Komponenten (kein --enable-gpl/--enable-nonfree, kein x264/x265/xvid;
wird vor dem Ausliefern per "ffmpeg -version" geprüft, siehe BUILD-ffmpeg.txt).
Quellen: https://github.com/FFmpeg/FFmpeg (Tag n9.0.2)
Lizenztext: https://www.gnu.org/licenses/lgpl-2.1.html
Hinweis: Hardware-Decoder (QSV/D3D11VA/CUDA/NVDEC) sind keine GPL-Komponenten
und in diesen LGPL-Builds enthalten.
"@ | Set-Content -Path (Join-Path $Dest 'LICENSE-ffmpeg.txt') -Encoding UTF8
  Write-Host "OK: ffmpeg.exe + ffprobe.exe (LGPL, geprüft) + LICENSE-ffmpeg.txt + BUILD-ffmpeg.txt nach $Dest"
}
finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
