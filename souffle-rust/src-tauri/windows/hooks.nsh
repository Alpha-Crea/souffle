; Avant l'installation : la version Electron de Souffle est fermée puis
; désinstallée. Les deux versions ne doivent jamais tourner ensemble : elles
; se disputeraient le même raccourci clavier.
!macro NSIS_HOOK_PREINSTALL
  nsExec::Exec 'taskkill /IM Souffle.exe /F'
  Sleep 800
  IfFileExists "$LOCALAPPDATA\Programs\souffle\Uninstall Souffle.exe" 0 +3
    ExecWait '"$LOCALAPPDATA\Programs\souffle\Uninstall Souffle.exe" /S _?=$LOCALAPPDATA\Programs\souffle'
    RMDir /r "$LOCALAPPDATA\Programs\souffle"
!macroend
