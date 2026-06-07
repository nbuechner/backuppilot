#Requires -Version 5.1
<#
.SYNOPSIS
    Installs BackupPilot (user-level, no administrator rights required).
.DESCRIPTION
    Copies binaries to %LOCALAPPDATA%\BackupPilot\, registers the daemon
    for autostart via the current user's Run registry key, creates a
    Start Menu shortcut, and launches the daemon immediately.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$AppName    = 'BackupPilot'
$InstallDir = Join-Path $env:LOCALAPPDATA $AppName
$RegKey     = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Run'
$RegValue   = 'BackupPilot Daemon'
$StartMenu  = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "Installing $AppName to $InstallDir ..."

# Stop existing daemon if running
$existing = Get-Process -Name 'backuppilot-daemon' -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "  Stopping existing daemon..."
    $existing | Stop-Process -Force
    Start-Sleep -Milliseconds 500
}

# Copy binaries
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item "$ScriptDir\backuppilot.exe"         "$InstallDir\" -Force
Copy-Item "$ScriptDir\backuppilot-daemon.exe"  "$InstallDir\" -Force
Copy-Item "$ScriptDir\backuppilot-cli.exe"     "$InstallDir\" -Force

# Register daemon for autostart on login
Set-ItemProperty -Path $RegKey -Name $RegValue `
    -Value "`"$InstallDir\backuppilot-daemon.exe`""
Write-Host "  Registered daemon for autostart."

# Start Menu shortcut
$WshShell  = New-Object -ComObject WScript.Shell
$Shortcut  = $WshShell.CreateShortcut("$StartMenu\$AppName.lnk")
$Shortcut.TargetPath    = "$InstallDir\backuppilot.exe"
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description   = 'BackupPilot - Proxmox Backup Server client'
$Shortcut.Save()
Write-Host "  Created Start Menu shortcut."

# Launch the daemon now (so user doesn't need to reboot)
Start-Process -FilePath "$InstallDir\backuppilot-daemon.exe" -WindowStyle Hidden
Write-Host "  Daemon started."

Write-Host ""
Write-Host "$AppName installed successfully."
Write-Host "Launch it from the Start Menu or run: $InstallDir\backuppilot.exe"
