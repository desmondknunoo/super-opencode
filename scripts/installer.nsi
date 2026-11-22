; NSIS installer for Super OpenCode.
; Paths and version come from the build workflow via /D flags.

!ifndef APP_VERSION
  !define APP_VERSION "0.2.0"
!endif
!ifndef STAGE_DIR
  !define STAGE_DIR "dist\stage"
!endif
!ifndef OUTFILE
  !define OUTFILE "dist\Super-OpenCode-windows-x64.exe"
!endif

Name "Super OpenCode ${APP_VERSION}"
OutFile "${OUTFILE}"
Icon "${STAGE_DIR}\AppIcon.ico"
UninstallIcon "${STAGE_DIR}\AppIcon.ico"
InstallDir "$LOCALAPPDATA\Programs\Super OpenCode"
InstallDirRegKey HKCU "Software\Super OpenCode" "InstallDir"
RequestExecutionLevel user
ShowInstDetails show

Page directory
Page instfiles

UninstPage uninstConfirm
UninstPage instfiles

Section "Super OpenCode"
  SetOutPath "$INSTDIR"
  File "${STAGE_DIR}\super-opencode.exe"
  File "${STAGE_DIR}\WebView2Loader.dll"
  File "${STAGE_DIR}\AppIcon.ico"
  File "${STAGE_DIR}\README.md"

  CreateDirectory "$SMPROGRAMS\Super OpenCode"
  CreateShortcut "$SMPROGRAMS\Super OpenCode\Super OpenCode.lnk" "$INSTDIR\super-opencode.exe" "" "$INSTDIR\AppIcon.ico"
  CreateShortcut "$SMPROGRAMS\Super OpenCode\Uninstall Super OpenCode.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\AppIcon.ico"

  WriteRegStr HKCU "Software\Super OpenCode" "InstallDir" "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\super-opencode.exe"
  Delete "$INSTDIR\WebView2Loader.dll"
  Delete "$INSTDIR\AppIcon.ico"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\Super OpenCode\Super OpenCode.lnk"
  Delete "$SMPROGRAMS\Super OpenCode\Uninstall Super OpenCode.lnk"
  RMDir "$SMPROGRAMS\Super OpenCode"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Super OpenCode"
SectionEnd
