!macro NSIS_HOOK_PREINSTALL
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  IfFileExists "$INSTDIR\logcrate_index_service.exe" 0 logcrate_preinstall_done

  nsExec::ExecToLog '"$SYSDIR\sc.exe" query "LogCrateIndex"'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Stopping the existing LogCrate Index Service before replacing its executable"
    nsExec::ExecToLog '"$INSTDIR\logcrate_index_service.exe" --uninstall'
    Pop $0
    ${If} $0 == "error"
      DetailPrint "LogCrate Index Service pre-upgrade stop could not be executed"
      MessageBox MB_OK|MB_ICONSTOP "The existing LogCrate Index Service could not be stopped before the application files were updated.$\r$\n$\r$\nLogCrate setup cannot continue. Close LogCrate, check Windows Security or endpoint protection, then run the installer again."
      Abort
    ${ElseIf} $0 != 0
      DetailPrint "LogCrate Index Service pre-upgrade stop returned $0"
      MessageBox MB_OK|MB_ICONSTOP "The existing LogCrate Index Service could not be stopped before the application files were updated (exit result $0).$\r$\n$\r$\nLogCrate setup cannot continue. Close LogCrate, then run the installer again."
      Abort
    ${EndIf}
  ${EndIf}

  logcrate_preinstall_done:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToLog '"$INSTDIR\logcrate_index_service.exe" --install'
  Pop $0
  ${If} $0 == "error"
    DetailPrint "LogCrate Index Service install/repair could not be executed"
    MessageBox MB_OK|MB_ICONSTOP "LogCrate Index Service could not be installed because the service installer could not be executed.$\r$\n$\r$\nLogCrate setup cannot continue. Check Windows Security or endpoint protection, then run the installer again."
    Abort
  ${ElseIf} $0 != 0
    DetailPrint "LogCrate Index Service install/repair returned $0"
    MessageBox MB_OK|MB_ICONSTOP "LogCrate Index Service installation failed with exit result $0.$\r$\n$\r$\nLogCrate setup cannot continue. Check Windows Security or endpoint protection, then run the installer again."
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog '"$INSTDIR\logcrate_index_service.exe" --uninstall'
  Pop $0
  DetailPrint "LogCrate Index Service uninstall returned $0"
!macroend
