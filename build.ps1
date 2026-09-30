# Builds PlayLedger (the tracker and the dashboard, both Rust) and its installer into .\publish
#   publish\PlayLedgerSetup.exe   (releases ship it as Setup.exe)
# Requires Rust (rustup; rust-toolchain.toml picks the version) and NSIS (winget install NSIS.NSIS).
param(
    # Optional: the version being released. It must match Cargo.toml, which also sets the exes' version resources.
    [ValidatePattern('^\d{1,5}\.\d{1,5}\.\d{1,5}$')]
    [string]$Version
)
$ErrorActionPreference = 'Stop'
$out = "$PSScriptRoot\publish"

$cargoVersion = (Select-String -Path "$PSScriptRoot\Cargo.toml" -Pattern '^version = "(\d+\.\d+\.\d+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $Version) { $Version = $cargoVersion }
if ($Version -ne $cargoVersion) {
    throw "Version $Version doesn't match Cargo.toml ($cargoVersion); update the workspace version first."
}

cargo build --release --locked -p playtime-tracker -p playtime-dashboard
if ($LASTEXITCODE) { exit $LASTEXITCODE }

# The dashboard installs as Dashboard\PlaytimeTracker.Dashboard.exe, the name the tray opens.
Remove-Item "$out\dashboard" -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$out\dashboard" | Out-Null
Copy-Item "$PSScriptRoot\target\release\playtime-dashboard.exe" "$out\dashboard\PlaytimeTracker.Dashboard.exe"

$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
if (-not $makensis) { $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (-not (Test-Path $makensis)) {
    Write-Warning "NSIS not found, skipping the installer. The programs are in target\release."
    exit 0
}

Push-Location "$PSScriptRoot\installer"
try {
    & $makensis /V2 "/DVERSION=$Version" "/DTRACKER_EXE=$PSScriptRoot\target\release\playtime-tracker.exe" `
        "/DDASHBOARD_DIR=$out\dashboard" "/DOUT_FILE=$out\PlayLedgerSetup.exe" PlayLedger.nsi
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
Write-Host "`nDone:`n  $out\PlayLedgerSetup.exe"
