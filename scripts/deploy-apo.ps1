<#
.SYNOPSIS
    Deploy APO DLL to system. Requires admin privileges.

.PARAMETER Debug
    Deploy debug build instead of release.

.PARAMETER Build
    Build the DLL before deploying.

.EXAMPLE
    .\scripts\deploy-apo.ps1
    .\scripts\deploy-apo.ps1 -Build
    .\scripts\deploy-apo.ps1 -Debug -Build
#>
param(
    [switch]$Debug,
    [switch]$Build
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$profileDir = if ($Debug) { "debug" } else { "release" }
$src = "$Root\target\$profileDir\phaselith_apo.dll"
$dst = "C:\Program Files\Phaselith\phaselith_apo.dll"

# ── Admin check ──
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[deploy] Elevating to admin..." -ForegroundColor Yellow
    $args = "-NoProfile -ExecutionPolicy Bypass -File `"$($MyInvocation.MyCommand.Path)`""
    if ($Debug) { $args += " -Debug" }
    if ($Build) { $args += " -Build" }
    Start-Process powershell -Verb RunAs -ArgumentList $args
    exit
}

Write-Host "`n[deploy] APO DLL Deploy ($profileDir)" -ForegroundColor Cyan

# ── Build if requested ──
if ($Build) {
    Write-Host "[deploy] Building APO DLL ($profileDir)..." -ForegroundColor Yellow
    Set-Location $Root
    $buildArgs = @("build", "-p", "phaselith-apo")
    if (-not $Debug) { $buildArgs += "--release" }
    cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "Build failed" }
}

# ── Check source exists ──
if (-not (Test-Path $src)) {
    Write-Host "[FAIL] DLL not found: $src" -ForegroundColor Red
    Write-Host "  Run with -Build flag or build manually first." -ForegroundColor Gray
    exit 1
}

$srcSize = (Get-Item $src).Length
Write-Host "[deploy] Source: $src ($('{0:N0}' -f ($srcSize / 1KB)) KB)" -ForegroundColor Gray

# ── Ensure destination directory ──
$dstDir = Split-Path $dst
if (-not (Test-Path $dstDir)) {
    New-Item -ItemType Directory -Path $dstDir -Force | Out-Null
}

# ── Stop audio services ──
Write-Host "[deploy] Stopping audio services..." -ForegroundColor Yellow
Stop-Service -Name Audiosrv -Force -ErrorAction SilentlyContinue
Stop-Service -Name AudioEndpointBuilder -Force -ErrorAction SilentlyContinue

# Wait for audiodg.exe to exit
$timeout = 10
$elapsed = 0
while ((Get-Process audiodg -ErrorAction SilentlyContinue) -and $elapsed -lt $timeout) {
    Start-Sleep -Seconds 1
    $elapsed++
}
if (Get-Process audiodg -ErrorAction SilentlyContinue) {
    Write-Host "[deploy] Force-killing audiodg.exe..." -ForegroundColor Yellow
    Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 1
}

# ── Copy DLL ──
Write-Host "[deploy] Copying DLL..." -ForegroundColor Yellow
try {
    Copy-Item $src $dst -Force
    $dstSize = (Get-Item $dst).Length
    Write-Host "[deploy] Destination: $dst ($('{0:N0}' -f ($dstSize / 1KB)) KB)" -ForegroundColor Gray

    if ($srcSize -ne $dstSize) {
        Write-Host "[FAIL] Size mismatch! src=$srcSize dst=$dstSize" -ForegroundColor Red
        exit 1
    }
}
catch {
    Write-Host "[FAIL] Copy failed: $_" -ForegroundColor Red
    # Restart services even on failure
    Start-Service -Name AudioEndpointBuilder -ErrorAction SilentlyContinue
    Start-Service -Name Audiosrv -ErrorAction SilentlyContinue
    exit 1
}

# ── Restart audio services ──
Write-Host "[deploy] Restarting audio services..." -ForegroundColor Yellow
Start-Service -Name AudioEndpointBuilder -ErrorAction SilentlyContinue
Start-Service -Name Audiosrv -ErrorAction SilentlyContinue

Write-Host "`n[ok] APO DLL deployed successfully." -ForegroundColor Green
Write-Host "  Audio services restarted. Test playback now." -ForegroundColor Gray
