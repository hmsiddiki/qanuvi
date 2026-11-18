; Qanuvi's installer hooks (migration M8d).
;
; Qanuvi's background part (qanuvi-master.exe, This PC's master) runs beside the app and keeps its program file in
; use, so it has to be gone before the file is replaced or removed. An update from Qanuvi ends it first and only then
; starts this installer; this is the safety net for an installer started by hand while it runs. Only the copy in this
; install folder is stopped (Restart Manager, by its path), never another master on this computer.

!macro QanuviStopMaster
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 != ""
    !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\qanuvi-master.exe"
    ${If} $0 = 0
      System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'
      ${If} $0 = ${ERROR_MORE_DATA}
        System::Call 'RSTRTMGR::RmShutdown(p R0, i ${RmForceShutdown}, p 0) i .r0'
      ${EndIf}
    ${EndIf}
    !insertmacro RestartManager_EndSession $R0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro QanuviStopMaster
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro QanuviStopMaster
!macroend
