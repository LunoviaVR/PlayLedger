# Builds the next Playtime Tracker (Rust tracker + WinUI 3 dashboard) and its preview installer into .\publish
#   publish\PlaytimeTrackerSetup-Preview.exe
# Requires Rust (rustup; rust-toolchain.toml picks the version), the .NET 8 SDK and NSIS (winget install NSIS.NSIS).
# Releases still ship the C# app built by build.ps1 until the new app has been verified; see
# docs/architecture/rust-migration.md.
param(
    # Plain x.y.z only: the value ends up in the installer's version resource.
    [ValidatePattern('^\d{1,5}\.\d{1,5}\.\d{1,5}$')]
    [string]$Version = "2.2.0"
)
$ErrorActionPreference = 'Stop'
$out = "$PSScriptRoot\publish"

cargo build --release --locked -p playtime-tracker
if ($LASTEXITCODE) { exit $LASTEXITCODE }

dotnet publish "$PSScriptRoot\dashboard\PlaytimeTracker.Dashboard" -c Release -r win-x64 --self-contained true `
    -p:Platform=x64 -p:DebugType=none -p:Version=$Version -o "$out\dashboard"
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
if (-not $makensis) { $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (-not (Test-Path $makensis)) {
    Write-Warning "NSIS not found, skipping the installer. The tracker is at target\release\playtime-tracker.exe"
    exit 0
}

Push-Location "$PSScriptRoot\installer"
try {
    & $makensis /V2 "/DVERSION=$Version" "/DTRACKER_EXE=$PSScriptRoot\target\release\playtime-tracker.exe" `
        "/DDASHBOARD_DIR=$out\dashboard" "/DOUT_FILE=$out\PlaytimeTrackerSetup-Preview.exe" PlaytimeTracker-Next.nsi
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
Write-Host "`nDone:`n  $out\PlaytimeTrackerSetup-Preview.exe"
