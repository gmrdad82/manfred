Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel admin

!ifndef VERSION
  !error "pass /DVERSION=X.Y.Z"
!endif
!ifndef VERSION_NUM
  !error "pass /DVERSION_NUM=X.Y.Z.0"
!endif
!ifndef SRC
  !error "pass /DSRC=<folder with manfred.exe, manfred-desk.exe, manfred.ico, LICENSE, NOTICE and licenses>"
!endif
!ifndef OUT
  !error "pass /DOUT=<Manfred-vX.Y.Z-Setup.exe>"
!endif

!define NAME "Manfred"
!define PUBLISHER "PITO MD"
!define URL "https://pitomd.com"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Manfred"
!define PROGID "Manfred.Document"
!define ENV_KEY "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"
!define /math PATH_ROOM ${NSIS_MAX_STRLEN} - 1

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"

Name "${NAME}"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\${NAME}"
BrandingText "${NAME} ${VERSION}"

VIProductVersion "${VERSION_NUM}"
VIFileVersion "${VERSION_NUM}"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${NAME} ${VERSION} Setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "© ${PUBLISHER}"

!define MUI_ICON "${SRC}\manfred.ico"
!define MUI_UNICON "${SRC}\manfred.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_NODESC

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "${NAME} needs 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext all
FunctionEnd

Function un.onInit
  SetRegView 64
  SetShellVarContext all
FunctionEnd

!macro PATH_FIND UN
Function ${UN}PathFind
  Push $R3
  Push $R4
  StrLen $R3 $R1
  StrCpy $R2 0
  loop:
    StrCpy $R4 $R0 $R3 $R2
    StrCmp $R4 "" missing
    StrCmp $R4 $R1 done
    IntOp $R2 $R2 + 1
    Goto loop
  missing:
    StrCpy $R2 -1
  done:
  Pop $R4
  Pop $R3
FunctionEnd
!macroend
!insertmacro PATH_FIND ""
!insertmacro PATH_FIND "un."

Section "${NAME}" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SRC}\manfred.exe"
  File "${SRC}\manfred-desk.exe"
  File "${SRC}\manfred.ico"
  File "${SRC}\LICENSE"
  File "${SRC}\NOTICE"
  File /r "${SRC}\licenses"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\manfred-desk.exe" "" "$INSTDIR\manfred.ico" 0

  WriteRegStr HKLM "Software\Classes\.manf" "" "${PROGID}"
  WriteRegStr HKLM "Software\Classes\.manf" "Content Type" "application/x-manfred"
  WriteRegStr HKLM "Software\Classes\${PROGID}" "" "Manfred scene"
  WriteRegStr HKLM "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\manfred.ico,0"
  WriteRegStr HKLM "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\manfred-desk.exe" "%1"'

  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\manfred.ico"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "URLInfoAbout" "${URL}"
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  ClearErrors
  ReadRegStr $0 HKLM "${ENV_KEY}" "Path"
  ${If} ${Errors}
    DetailPrint "The machine Path could not be read; add $INSTDIR to it by hand"
  ${Else}
    StrCpy $R0 ";$0;"
    StrCpy $R1 ";$INSTDIR;"
    Call PathFind
    ${If} $R2 < 0
      StrLen $1 $0
      StrLen $2 $INSTDIR
      IntOp $1 $1 + $2
      IntOp $1 $1 + 1
      ${If} $1 >= ${PATH_ROOM}
        DetailPrint "The machine Path is too long for the setup to change; add $INSTDIR to it by hand"
      ${Else}
        StrCpy $1 $0 1 -1
        ${If} $0 == ""
          StrCpy $0 "$INSTDIR"
        ${ElseIf} $1 == ";"
          StrCpy $0 "$0$INSTDIR"
        ${Else}
          StrCpy $0 "$0;$INSTDIR"
        ${EndIf}
        WriteRegExpandStr HKLM "${ENV_KEY}" "Path" $0
        SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
      ${EndIf}
    ${EndIf}
  ${EndIf}
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${NAME}.lnk" "$INSTDIR\manfred-desk.exe" "" "$INSTDIR\manfred.ico" 0
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\${NAME}.lnk"
  Delete "$DESKTOP\${NAME}.lnk"

  ReadRegStr $0 HKLM "Software\Classes\.manf" ""
  StrCmp $0 "${PROGID}" 0 +2
    DeleteRegKey HKLM "Software\Classes\.manf"
  DeleteRegKey HKLM "Software\Classes\${PROGID}"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"

  ClearErrors
  ReadRegStr $0 HKLM "${ENV_KEY}" "Path"
  StrLen $1 $0
  ${IfNot} ${Errors}
  ${AndIf} $1 < ${PATH_ROOM}
    StrCpy $R0 ";$0;"
    StrCpy $R1 ";$INSTDIR;"
    Call un.PathFind
    ${If} $R2 >= 0
      StrCpy $1 $R0 $R2
      StrLen $2 $INSTDIR
      IntOp $2 $2 + 1
      IntOp $2 $2 + $R2
      StrCpy $3 $R0 "" $2
      StrCpy $0 "$1$3"
      StrCpy $0 $0 "" 1
      StrCpy $0 $0 -1
      WriteRegExpandStr HKLM "${ENV_KEY}" "Path" $0
      SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
    ${EndIf}
  ${EndIf}

  Delete "$INSTDIR\manfred.exe"
  Delete "$INSTDIR\manfred-desk.exe"
  Delete "$INSTDIR\manfred.ico"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE"
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
