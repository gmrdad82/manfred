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

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

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
