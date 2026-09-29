# Builds the app and both installers into .\publish
#   publish\PlaytimeTrackerSetup.exe          full installer, works offline
#   publish\PlaytimeTrackerSetup-Online.exe   small installer, downloads the .NET runtime if needed
# Requires the .NET 8 SDK (https://dotnet.microsoft.com/download/dotnet/8.0)
# and NSIS for the installers (https://nsis.sourceforge.io, or: winget install NSIS.NSIS).
param(
    # Plain x.y.z only: the value ends up in the exe's metadata and the installer's version resource.
    [ValidatePattern('^\d{1,5}\.\d{1,5}\.\d{1,5}$')]
    [string]$Version = "2.0.0"
)
$ErrorActionPreference = 'Stop'
$project = "$PSScriptRoot\src\GameSessionTracker"
$out = "$PSScriptRoot\publish"

dotnet publish $project -c Release -r win-x64 --self-contained true `
    -p:PublishSingleFile=true -p:IncludeNativeLibrariesForSelfExtract=true -p:DebugType=none `
    -p:Version=$Version -o "$out\full"
if ($LASTEXITCODE) { exit $LASTEXITCODE }

dotnet publish $project -c Release -r win-x64 --self-contained false `
    -p:PublishSingleFile=true -p:DebugType=none `
    -p:Version=$Version -o "$out\online"
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
if (-not $makensis) { $makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe" }
if (-not (Test-Path $makensis)) {
    Write-Warning "NSIS not found, skipping installers. The app is at $out\full\PlaytimeTracker.exe"
    exit 0
}

Push-Location "$PSScriptRoot\installer"
try {
    & $makensis /V2 "/DVERSION=$Version" "/DEXE_PATH=$out\full\PlaytimeTracker.exe" "/DOUT_FILE=$out\PlaytimeTrackerSetup.exe" PlaytimeTracker.nsi
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
    & $makensis /V2 /DONLINE "/DVERSION=$Version" "/DEXE_PATH=$out\online\PlaytimeTracker.exe" "/DOUT_FILE=$out\PlaytimeTrackerSetup-Online.exe" PlaytimeTracker.nsi
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
Write-Host "`nDone:`n  $out\PlaytimeTrackerSetup.exe`n  $out\PlaytimeTrackerSetup-Online.exe"
