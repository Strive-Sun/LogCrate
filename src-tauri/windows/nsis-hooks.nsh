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
