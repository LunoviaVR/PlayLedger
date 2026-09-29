; Installer for Game Session Tracker.
; Build: makensis -DVERSION=1.0.0 -DEXE_PATH=..\publish\GameSessionTracker.exe GameSessionTracker.nsi
;
; Two flavours:
;   default    EXE_PATH is the self-contained exe; works offline, nothing else needed.
;   -DONLINE   EXE_PATH is the small framework-dependent exe; setup downloads and installs the
;              .NET 8 Desktop Runtime from Microsoft if the PC doesn't already have it.
; Installs per-user (no admin prompt) to %LocalAppData%\Programs\Game Session Tracker.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "1.2.0"
!endif
!ifndef EXE_PATH
  !define EXE_PATH "..\publish\GameSessionTracker.exe"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "..\publish\GameSessionTrackerSetup.exe"
!endif

!define APP_NAME "Game Session Tracker"
!define APP_EXE "GameSessionTracker.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GameSessionTracker"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"

Name "${APP_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME} Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" ""

!define MUI_ICON "..\src\GameSessionTracker\app.ico"
!define MUI_UNICON "..\src\GameSessionTracker\app.ico"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TEXT "This will install ${APP_NAME} ${VERSION}.$\r$\n$\r$\nIt runs quietly in the system tray, notices when you open a game, and keeps a file with how many times you've played each game and how long every session lasted.$\r$\n$\r$\nNo administrator rights are needed.$\r$\n$\r$\nClick Next to continue."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME} now"
!define MUI_FINISHPAGE_TEXT "${APP_NAME} has been installed.$\r$\n$\r$\nIt will start automatically with Windows and live in the system tray (the controller icon, possibly behind the ^ arrow next to the clock).$\r$\n$\r$\nYour stats are saved in Documents\Game Session Tracker."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Close a running tracker so its files can be replaced. It logs any game in progress before exiting.
!ifdef ONLINE
!define DOTNET_URL "https://aka.ms/dotnet/8.0/windowsdesktop-runtime-win-x64.exe"

; Installs the .NET 8 Desktop Runtime if missing (one Windows admin prompt).
!macro EnsureDotNetRuntime
  FindFirst $0 $1 "$PROGRAMFILES64\dotnet\shared\Microsoft.WindowsDesktop.App\8.*"
  FindClose $0
  StrCmp $1 "" 0 dotnet_done

  DetailPrint "Downloading the .NET 8 Desktop Runtime from Microsoft..."
  InitPluginsDir
  nsExec::ExecToLog '"$SYSDIR\curl.exe" -L -f -s -S -o "$PLUGINSDIR\dotnet-runtime.exe" "${DOTNET_URL}"'
  Pop $0
  StrCmp $0 "0" 0 dotnet_failed

  DetailPrint "Installing the .NET 8 Desktop Runtime (Windows will ask for permission)..."
  ExecWait '"$PLUGINSDIR\dotnet-runtime.exe" /install /passive /norestart' $0
  StrCmp $0 "0" dotnet_done
  StrCmp $0 "3010" dotnet_done ; installed, restart recommended

dotnet_failed:
  MessageBox MB_ICONEXCLAMATION|MB_OK "Setup couldn't install the .NET 8 Desktop Runtime automatically.$\r$\n$\r$\nGame Session Tracker will still be installed. When you first start it, Windows will offer a link to download the runtime; or get it from https://dotnet.microsoft.com/download/dotnet/8.0 ($\".NET Desktop Runtime$\", x64)." /SD IDOK
dotnet_done:
!macroend
!endif

!macro CloseRunningTracker
  IfFileExists "$INSTDIR\${APP_EXE}" 0 +2
    ExecWait '"$INSTDIR\${APP_EXE}" --exit'
  ; Fallback for a copy running from somewhere else (e.g. the Downloads folder).
  nsExec::Exec 'taskkill /IM "${APP_EXE}"'
  Pop $0
  Sleep 1000
  nsExec::Exec 'taskkill /F /IM "${APP_EXE}"'
  Pop $0
!macroend

Section "${APP_NAME}" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
!ifdef ONLINE
  !insertmacro EnsureDotNetRuntime
!endif
  !insertmacro CloseRunningTracker

  File "${EXE_PATH}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  ; Start with Windows (can be turned off later from the tray menu).
  WriteRegStr HKCU "${RUN_KEY}" "GameSessionTracker" '"$INSTDIR\${APP_EXE}" --startup'
  DeleteRegValue HKCU "${APPROVED_KEY}" "GameSessionTracker"

  ; Apps & features entry
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The tracker itself, a Start menu shortcut, and starting with Windows."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Put a shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---------------- Uninstaller ----------------

Section "un.${APP_NAME}" UnSecApp
  SectionIn RO
  !insertmacro CloseRunningTracker

  DeleteRegValue HKCU "${RUN_KEY}" "GameSessionTracker"
  DeleteRegValue HKCU "${APPROVED_KEY}" "GameSessionTracker"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"

  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd

Section /o "un.Delete my play history and settings" UnSecData
  RMDir /r "$DOCUMENTS\Game Session Tracker"
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecApp} "Remove the program, its shortcuts and its startup entry."
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecData} "Also delete Documents\Game Session Tracker (your stats, session log and settings). Leave unticked to keep them."
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
