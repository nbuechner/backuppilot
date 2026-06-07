; BackupPilot NSIS installer hooks
; Called by Tauri's NSIS template at customInstall / customUnInstall points.

!macro customInstall
  ; Register daemon to start automatically when the user logs in
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" \
    "BackupPilot Daemon" '"$INSTDIR\backuppilot-daemon.exe"'

  ; Launch the daemon immediately so the user doesn't need to reboot
  Exec '"$INSTDIR\backuppilot-daemon.exe"'
!macroend

!macro customUnInstall
  ; Stop the daemon before uninstalling
  nsExec::Exec 'taskkill /F /IM backuppilot-daemon.exe'

  ; Remove the autostart registry entry
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "BackupPilot Daemon"
!macroend
