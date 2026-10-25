; hooks.nsh - NSIS custom installer hook for Streamy Desktop
; Copies libmp3lame.dll directly to $INSTDIR (alongside Streamy Desktop.exe)
; and removes it upon uninstall. Ensures LGPL §6 compliance and runtime dynamic linking.

!ifndef HOOK_FILE_DIR
  !define HOOK_FILE_DIR "${__FILEDIR__}"
!endif

!macro NSIS_HOOK_PREINSTALL
  CreateDirectory "$INSTDIR"
  SetOutPath "$INSTDIR"
  File "${HOOK_FILE_DIR}\libmp3lame.dll"
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\libmp3lame.dll"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
