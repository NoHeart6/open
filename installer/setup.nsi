; OpenCommander NSIS installer — produces OpenCommanderSetup.exe
!include "MUI2.nsh"
Name "OpenCommander"
OutFile "OpenCommanderSetup.exe"
InstallDir "$PROGRAMFILES64\OpenCommander"
RequestExecutionLevel admin
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_LANGUAGE "English"
Section "Install"
  SetOutPath $INSTDIR
  File "..\agent-rust\target\release\OpenCommander.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "OpenCommander" "$INSTDIR\OpenCommander.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenCommander" "DisplayName" "OpenCommander"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenCommander" "UninstallString" "$INSTDIR\Uninstall.exe"
SectionEnd
Section "Uninstall"
  Delete "$INSTDIR\OpenCommander.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegValue HKLM "Software\Microsoft\Windows\CurrentVersion\Run" "OpenCommander"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenCommander"
SectionEnd
