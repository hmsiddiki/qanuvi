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

; sl1 S3 (owner, 2026-11-22): direct links between paired computers at home. The master takes them on ports
; 47444-47454; Windows would otherwise ask about it with its own dialog. The rule is for this install's
; qanuvi-master.exe on private (home) networks only. Looking for the rule needs no admin rights, so it is only added
; (one Windows "allow changes" question) when it is missing, never again on each update. Said no to: nothing is
; added, and the computers keep using the relay.
!define QANUVI_FW_RULE "Qanuvi home links"

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack 'netsh advfirewall firewall show rule name="${QANUVI_FW_RULE}"'
  Pop $0
  Pop $1
  ${If} $0 != 0
    ExecShellWait "runas" "netsh" 'advfirewall firewall add rule name="${QANUVI_FW_RULE}" dir=in action=allow program="$INSTDIR\qanuvi-master.exe" enable=yes profile=private protocol=TCP localport=47444-47454' SW_HIDE
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; An uninstall that is part of an update leaves the rule (the new version keeps using it).
  ${If} $UpdateMode <> 1
    nsExec::ExecToStack 'netsh advfirewall firewall show rule name="${QANUVI_FW_RULE}"'
    Pop $0
    Pop $1
    ${If} $0 = 0
      ExecShellWait "runas" "netsh" 'advfirewall firewall delete rule name="${QANUVI_FW_RULE}"' SW_HIDE
    ${EndIf}
  ${EndIf}
!macroend
