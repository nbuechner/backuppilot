#Requires -Version 5.1
<#
.SYNOPSIS
    Uninstalls BackupPilot.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$AppName    = 'BackupPilot'
$InstallDir = Join-Path $env:LOCALAPPDATA $AppName
$RegKey     = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run'
$RegValue   = 'BackupPilot Daemon'
$StartMenu  = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'

Write-Host "Uninstalling $AppName..."

# Stop daemon
$proc = Get-Process -Name 'backuppilot-daemon' -ErrorAction SilentlyContinue
if ($proc) {
    Write-Host "  Stopping daemon..."
    $proc | Stop-Process -Force
    Start-Sleep -Milliseconds 500
}

# Remove autostart entry
if ((Get-ItemProperty -Path $RegKey -Name $RegValue -ErrorAction SilentlyContinue)) {
    Remove-ItemProperty -Path $RegKey -Name $RegValue
    Write-Host "  Removed autostart entry."
}

# Remove Start Menu shortcut
$link = "$StartMenu\$AppName.lnk"
if (Test-Path $link) {
    Remove-Item $link -Force
    Write-Host "  Removed Start Menu shortcut."
}

# Remove install directory
if (Test-Path $InstallDir) {
    Remove-Item $InstallDir -Recurse -Force
    Write-Host "  Removed $InstallDir."
}

Write-Host ""
Write-Host "$AppName uninstalled."
