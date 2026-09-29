# Builds GameSessionTracker.exe into .\publish
# Requires the .NET 8 SDK: https://dotnet.microsoft.com/download/dotnet/8.0
$ErrorActionPreference = 'Stop'
dotnet publish "$PSScriptRoot\src\GameSessionTracker" `
    -c Release -r win-x64 --self-contained true `
    -p:PublishSingleFile=true `
    -p:IncludeNativeLibrariesForSelfExtract=true `
    -p:EnableCompressionInSingleFile=true `
    -o "$PSScriptRoot\publish"
Write-Host "`nDone: $PSScriptRoot\publish\GameSessionTracker.exe"
