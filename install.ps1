#!/usr/bin/env pwsh
# Native Windows installer (PowerShell). Linux/macOS use install.sh instead.
# Requires the Rust toolchain plus a C compiler for the bundled SQLite
# (Visual Studio "Desktop development with C++" / Build Tools, or the
# x86_64-pc-windows-gnu toolchain).

$ErrorActionPreference = 'Stop'

Write-Host "Building dnews (release)..."
cargo build --release

$BinDir  = Join-Path $env:LOCALAPPDATA 'Programs\dnews'
$ConfDir = Join-Path $env:APPDATA 'dnews'
$BinSrc  = Join-Path 'target\release' 'dnews.exe'

if (-not (Test-Path $BinSrc)) {
    throw "Build did not produce $BinSrc"
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Copy-Item $BinSrc (Join-Path $BinDir 'dnews.exe') -Force
Write-Host "Installed: $(Join-Path $BinDir 'dnews.exe')"

# Seed the default feed list only if neither a repo-local nor an installed one exists.
if (-not (Test-Path '.\feeds.opml') -and -not (Test-Path (Join-Path $ConfDir 'feeds.opml'))) {
    New-Item -ItemType Directory -Force -Path $ConfDir | Out-Null
    Copy-Item '.\feeds.opml' (Join-Path $ConfDir 'feeds.opml')
    Write-Host "Copied default feeds.opml to $(Join-Path $ConfDir 'feeds.opml')"
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $BinDir) {
    Write-Host ""
    Write-Host "$BinDir is not on your PATH. Add it (one-time):"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$BinDir', 'User')"
    Write-Host "  # then restart the shell"
}

Write-Host ""
Write-Host "Run with: dnews"
Write-Host "Feeds are read from .\feeds.opml if present in the current directory,"
Write-Host "otherwise from $(Join-Path $ConfDir 'feeds.opml'). Edit it and restart to change feeds."
