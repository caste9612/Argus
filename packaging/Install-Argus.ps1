<#
  Installer per-utente di Argus — nessun privilegio di amministratore richiesto.

  Cosa fa:
    - copia argus.exe in %LOCALAPPDATA%\Programs\Argus
    - crea un collegamento "Argus" nel menu Start

  Uso: tasto destro su questo file -> "Esegui con PowerShell"
       (oppure:  powershell -ExecutionPolicy Bypass -File Install-Argus.ps1)

  Disinstallare: elimina la cartella %LOCALAPPDATA%\Programs\Argus e il
  collegamento "Argus" dal menu Start. Argus non scrive nel registro né altrove.
#>
$ErrorActionPreference = 'Stop'

$src = Join-Path $PSScriptRoot 'argus.exe'
if (-not (Test-Path $src)) {
    Write-Error "argus.exe non trovato accanto a questo script. Tienili nella stessa cartella."
    exit 1
}

$dest = Join-Path $env:LOCALAPPDATA 'Programs\Argus'
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item $src (Join-Path $dest 'argus.exe') -Force

$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$lnk = Join-Path $startMenu 'Argus.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($lnk)
$shortcut.TargetPath = Join-Path $dest 'argus.exe'
$shortcut.WorkingDirectory = $dest
$shortcut.IconLocation = Join-Path $dest 'argus.exe'
$shortcut.Description = 'Argus - profiler GPU-accelerato per Windows'
$shortcut.Save()

Write-Host ""
Write-Host "  Argus installato in:  $dest" -ForegroundColor Green
Write-Host "  Collegamento creato nel menu Start ('Argus')." -ForegroundColor Green
Write-Host "  Per profiling completo (flame/timeline/disco/memoria) avvialo come amministratore." -ForegroundColor DarkGray
Write-Host ""
