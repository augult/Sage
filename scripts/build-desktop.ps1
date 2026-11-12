# Builds the desktop app as one Sage.exe, puts it where it can stay, and gives it a shortcut
# on the Desktop and in the Start menu, so it opens like any other program: no terminal.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-desktop.ps1 [-NoShortcuts]
#
# Or double-click "Build Sage.cmd" in the repository's root.

param(
    # Build and install the exe only.
    [switch]$NoShortcuts
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host 'Building Sage (release). The first build takes a few minutes...' -ForegroundColor Cyan
# custom-protocol: the window loads the front end built into the exe, as an installed app does.
cargo build --release -p sage-desktop --target-dir target/desktop --features tauri/custom-protocol
if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }

$built = Join-Path $root 'target\desktop\release\sage-desktop.exe'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\Sage'
$exe = Join-Path $dest 'Sage.exe'
New-Item -ItemType Directory -Force -Path $dest | Out-Null
try {
    Copy-Item -Force $built $exe
} catch {
    throw "Could not replace $exe. Close Sage if it is open, then run this again."
}
Write-Host "Installed $exe" -ForegroundColor Green

# The shortcuts take their icon from a copy of icon.ico whose name holds a fingerprint of it.
# Windows keeps the icon it has shown for a path; a new icon under a new name is shown at once,
# where one built into the same Sage.exe would keep the old picture until the cache is cleared.
$source = Join-Path $root 'desktop\src-tauri\icons\icon.ico'
$fingerprint = (Get-FileHash -Algorithm SHA256 $source).Hash.Substring(0, 8).ToLower()
$icon = Join-Path $dest "Sage-$fingerprint.ico"
Copy-Item -Force $source $icon

$shell = New-Object -ComObject WScript.Shell
$places = @(
    [Environment]::GetFolderPath('Desktop'),
    (Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs')
)
foreach ($place in $places) {
    # The app was called Presage before: its shortcut goes, so the two do not sit side by side.
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $place 'Presage.lnk')
    $path = Join-Path $place 'Sage.lnk'
    # With -NoShortcuts none is made, but those already there still get the current icon.
    if ($NoShortcuts -and -not (Test-Path $path)) { continue }
    $link = $shell.CreateShortcut($path)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $env:USERPROFILE
    $link.IconLocation = "$icon,0"
    $link.Description = 'Sage'
    $link.Save()
    Write-Host "Shortcut: $path" -ForegroundColor Green
}
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $env:LOCALAPPDATA 'Programs\Presage')
Get-ChildItem $dest -Filter 'Sage-*.ico' | Where-Object { $_.FullName -ne $icon } | Remove-Item -Force
# Ask Explorer to draw icons again.
& (Join-Path $env:WINDIR 'System32\ie4uinit.exe') -show 2>$null

Write-Host 'Done. Open Sage from the Desktop or the Start menu.' -ForegroundColor Cyan
