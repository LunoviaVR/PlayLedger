; Installer for the next Playtime Tracker: the Rust background tracker plus the WinUI 3 dashboard.
; Built by CI as a *preview* (not attached to releases) until the new app has been verified on real PCs;
; releases keep shipping PlaytimeTracker.nsi (the C# app) until then. See docs/architecture/rust-migration.md.
;
; Build: makensis -DVERSION=2.2.0 -DTRACKER_EXE=..\target\release\playtime-tracker.exe
;                 -DDASHBOARD_DIR=..\publish\dashboard -DOUT_FILE=..\publish\PlaytimeTrackerSetup-Preview.exe
;                 PlaytimeTracker-Next.nsi
;
; Installs per-user (no admin prompt) to %LocalAppData%\Programs\Playtime Tracker, replacing the C# app in place:
; same Apps entry, same Start menu shortcut, same "Start with Windows" entry, same data (untouched by setup).

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "2.2.0"
!endif
!ifndef TRACKER_EXE
  !define TRACKER_EXE "..\target\release\playtime-tracker.exe"
!endif
!ifndef DASHBOARD_DIR
  !define DASHBOARD_DIR "..\publish\dashboard"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "..\publish\PlaytimeTrackerSetup-Preview.exe"
!endif

!define APP_NAME "Playtime Tracker"
!define APP_EXE "playtime-tracker.exe"
!define DASHBOARD_EXE "PlaytimeTracker.Dashboard.exe"
; The C# app's exe, replaced by this install.
!define CSHARP_EXE "PlaytimeTracker.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PlaytimeTracker"
!define LEGACY_NAME "Game Session Tracker"
!define LEGACY_EXE "GameSessionTracker.exe"
!define LEGACY_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GameSessionTracker"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
; The dashboard's exe carries the app icon (the tracker has none of its own).
!define ICON_PATH "$INSTDIR\Dashboard\${DASHBOARD_EXE}"

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

!define MUI_ICON "..\assets\app.ico"
!define MUI_UNICON "..\assets\app.ico"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TEXT "This will install ${APP_NAME} ${VERSION}.$\r$\n$\r$\nIt runs quietly in the system tray, notices when you open a game, and keeps track of how long you play. Open the dashboard from the tray icon or the Start menu.$\r$\n$\r$\nIf an earlier version is installed, it's replaced and your history is kept.$\r$\n$\r$\nNo administrator rights are needed.$\r$\n$\r$\nClick Next to continue."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP_NAME} now"
!define MUI_FINISHPAGE_TEXT "${APP_NAME} has been installed.$\r$\n$\r$\nIt will start automatically with Windows and live in the system tray (the controller icon, possibly behind the ^ arrow next to the clock).$\r$\n$\r$\nYour history stays in Documents\Playtime Tracker."

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Close whichever tracker is running (the new one or the C# one; both answer --exit, which saves any game in
; progress first) and the dashboard, so their files can be replaced.
!macro CloseRunning
  IfFileExists "$INSTDIR\${APP_EXE}" 0 +2
    ExecWait '"$INSTDIR\${APP_EXE}" --exit'
  IfFileExists "$INSTDIR\${CSHARP_EXE}" 0 +2
    ExecWait '"$INSTDIR\${CSHARP_EXE}" --exit'
  ; Fallback for copies running from elsewhere. Full path to taskkill, so a file of that name next to the
  ; installer (e.g. in Downloads) can't be run instead.
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM "${APP_EXE}"'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM "${CSHARP_EXE}"'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /IM "${DASHBOARD_EXE}"'
  Pop $0
  Sleep 1000
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${APP_EXE}"'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${CSHARP_EXE}"'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${DASHBOARD_EXE}"'
  Pop $0
!macroend

!macro RemoveLegacyInstall
  ReadRegStr $1 HKCU "${LEGACY_KEY}" "InstallLocation"
  StrCmp $1 "" legacy_done
  IfFileExists "$1\${LEGACY_EXE}" 0 +2
    ExecWait '"$1\${LEGACY_EXE}" --exit'
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM "${LEGACY_EXE}"'
  Pop $0
  Delete "$1\${LEGACY_EXE}"
  Delete "$1\Uninstall.exe"
  RMDir "$1"
  Delete "$SMPROGRAMS\${LEGACY_NAME}.lnk"
  Delete "$DESKTOP\${LEGACY_NAME}.lnk"
  DeleteRegValue HKCU "${RUN_KEY}" "GameSessionTracker"
  DeleteRegValue HKCU "${APPROVED_KEY}" "GameSessionTracker"
  DeleteRegKey HKCU "${LEGACY_KEY}"
legacy_done:
!macroend

Section "${APP_NAME}" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  !insertmacro CloseRunning
  !insertmacro RemoveLegacyInstall

  ; The C# app this replaces (its data format is the same; nothing to convert).
  Delete "$INSTDIR\${CSHARP_EXE}"

  File "${TRACKER_EXE}"
  ; A fresh copy of the dashboard each time, so no files from an older version linger.
  RMDir /r "$INSTDIR\Dashboard"
  SetOutPath "$INSTDIR\Dashboard"
  File /r "${DASHBOARD_DIR}\*.*"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "${ICON_PATH}" 0

  ; Start with Windows (can be turned off in the dashboard's Settings or in Task Manager).
  WriteRegStr HKCU "${RUN_KEY}" "PlaytimeTracker" '"$INSTDIR\${APP_EXE}" --startup'
  DeleteRegValue HKCU "${APPROVED_KEY}" "PlaytimeTracker"

  ; Apps & features entry (the same one the C# app used, so this is an upgrade, not a second app)
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "${ICON_PATH}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"
SectionEnd

; The tracker's updater runs this installer silently with /relaunch; start the updated tracker again afterwards.
; --updated keeps it in the tray and it shows an "updated" notification.
Function .onInstSuccess
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/relaunch" $R1
  IfErrors relaunch_done
  Exec '"$INSTDIR\${APP_EXE}" --updated'
relaunch_done:
FunctionEnd

Section /o "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "${ICON_PATH}" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The tracker, the dashboard, a Start menu shortcut, and starting with Windows."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Put a shortcut on the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ---------------- Uninstaller ----------------

Section "un.${APP_NAME}" UnSecApp
  SectionIn RO
  !insertmacro CloseRunning

  DeleteRegValue HKCU "${RUN_KEY}" "PlaytimeTracker"
  DeleteRegValue HKCU "${APPROVED_KEY}" "PlaytimeTracker"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"

  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\${CSHARP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  ; Only our own sub-folder is removed recursively; the install folder itself only if it's then empty.
  RMDir /r "$INSTDIR\Dashboard"
  RMDir "$INSTDIR"
  ; Downloaded and extracted artwork (a cache; nothing personal).
  RMDir /r "$LOCALAPPDATA\Playtime Tracker\Cache"
  RMDir "$LOCALAPPDATA\Playtime Tracker"
SectionEnd

Section /o "un.Delete my play history and settings" UnSecData
  ; The readable reports are kept read-only by the app; clear that so they can be removed.
  SetFileAttributes "$DOCUMENTS\Playtime Tracker\Game Stats.txt" NORMAL
  SetFileAttributes "$DOCUMENTS\Playtime Tracker\Sessions.csv" NORMAL
  RMDir /r "$DOCUMENTS\Playtime Tracker"
  ; The SteamGridDB key, if one was saved (Windows Credential Manager).
  nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:"Playtime Tracker/SteamGridDB API key"'
  Pop $0
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecApp} "Remove the program, its shortcuts, its startup entry and its artwork cache."
  !insertmacro MUI_DESCRIPTION_TEXT ${UnSecData} "Also delete Documents\Playtime Tracker (your history, backups and settings) and a saved SteamGridDB key. Leave unticked to keep them."
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
