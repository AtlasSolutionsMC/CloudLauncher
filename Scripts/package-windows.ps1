param([string]$SourceExe)

$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($SourceExe)) {
    $SourceExe = Join-Path $projectRoot 'target\x86_64-pc-windows-gnu\release\legacylauncher-rust.exe'
} else {
    $SourceExe = (Resolve-Path -LiteralPath $SourceExe -ErrorAction Stop).Path
}
if (-not (Test-Path -LiteralPath $SourceExe -PathType Leaf)) {
    throw "Release executable was not found at $SourceExe. Run Scripts\build-windows.ps1 or pass the built executable path."
}

$stage = Join-Path $projectRoot 'dist\LegacyLauncher-Rust'
$archive = Join-Path $projectRoot 'dist\LegacyLauncher-Rust-portable.zip'
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $SourceExe -Destination (Join-Path $stage 'LegacyLauncher-Rust.exe') -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $stage -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'PORTING_PLAN.md') -Destination $stage -Force

$runScript = @'
@echo off
start "" "%~dp0LegacyLauncher-Rust.exe" %*
'@
[System.IO.File]::WriteAllText(
    (Join-Path $stage 'Run-LegacyLauncher.cmd'),
    $runScript,
    [System.Text.Encoding]::ASCII
)

$buildInfo = @'
LegacyLauncher Rust portable build
Target: Windows x64

The launcher UI, language file, icons, background, and US English crash signature catalog
are embedded in the executable. No separate Rust or Java runtime is required to start it.
The selected Minecraft Java runtime is installed on demand when a game version is launched.
Network access is required for Microsoft sign-in, version catalogs, and game downloads.

This package was built locally from the Rust port workspace for evaluation.
'@
[System.IO.File]::WriteAllText(
    (Join-Path $stage 'BUILD-INFO.txt'),
    $buildInfo,
    [System.Text.Encoding]::UTF8
)

Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -Force
Write-Output "Portable package: $archive"
