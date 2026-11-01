# G7: ffmpeg + ffprobe als statische LGPL-Builds neben die App legen (Windows).
# Aufruf: scripts\vendor-ffmpeg.ps1 <Zielverzeichnis>
$ErrorActionPreference = 'Stop'
param([Parameter(Mandatory = $true)][string]$Dest)
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
$work = Join-Path $env:TEMP ("ffmpeg-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $work | Out-Null
try {
  $zip = Join-Path $work 'ffmpeg.zip'
  Write-Host 'Lade ffmpeg (LGPL, gyan.dev) …'
  Invoke-WebRequest -UseBasicParsing -Uri 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip' -OutFile $zip
  Expand-Archive -Path $zip -DestinationPath $work -Force
  $bin = Get-ChildItem -Recurse -Filter ffmpeg.exe -Path $work | Select-Object -First 1
  $probe = Get-ChildItem -Recurse -Filter ffprobe.exe -Path $work | Select-Object -First 1
  Copy-Item $bin.FullName (Join-Path $Dest 'ffmpeg.exe') -Force
  Copy-Item $probe.FullName (Join-Path $Dest 'ffprobe.exe') -Force
  @'
ffmpeg/ffprobe in diesem Verzeichnis: statische Builds von https://www.gyan.dev/ffmpeg/builds/
Lizenz: GNU LGPL v2.1 oder später (Build ohne GPL- und Nonfree-Komponenten).
Quellen: https://github.com/FFmpeg/FFmpeg
Lizenztext: https://www.gnu.org/licenses/lgpl-2.1.html
'@ | Set-Content -Path (Join-Path $Dest 'LICENSE-ffmpeg.txt') -Encoding UTF8
  Write-Host "OK: ffmpeg.exe + ffprobe.exe + LICENSE-ffmpeg.txt nach $Dest"
}
finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
