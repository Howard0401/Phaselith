<#
.SYNOPSIS
    Release automation for Phaselith.

.PARAMETER Deploy
    Deploy APO DLL after building.

.PARAMETER Tag
    Create git tag and push after building. Format: v0.1.x

.PARAMETER Version
    Bump version across all manifests before building.

.EXAMPLE
    .\scripts\release.ps1
    .\scripts\release.ps1 -Deploy
    .\scripts\release.ps1 -Tag v0.1.19
    .\scripts\release.ps1 -Tag v0.1.19 -Deploy
    .\scripts\release.ps1 -Version 0.1.19 -Tag v0.1.19 -Deploy
#>
param(
    [switch]$Deploy,
    [string]$Tag,
    [string]$Version
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

Write-Host "`n=== Phaselith Release ===" -ForegroundColor Cyan

# ── Version bump ──
if ($Version) {
    Write-Host "`n[release] Bumping version to $Version..." -ForegroundColor Yellow
    & "$Root\scripts\build.ps1" -Target test -Version $Version
    # build.ps1 with -Target test will bump version AND run tests
} else {
    # ── Step 1: Tests ──
    Write-Host "`n[release] Running tests..." -ForegroundColor Yellow
    & "$Root\scripts\build.ps1" -Target test
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Tests failed. Aborting release." -ForegroundColor Red
    exit 1
}

# ── Step 2: Full release build ──
Write-Host "`n[release] Building all targets (release)..." -ForegroundColor Yellow
& "$Root\scripts\build.ps1" -Target apo -Profile release
if ($LASTEXITCODE -ne 0) { Write-Host "[FAIL] APO build failed." -ForegroundColor Red; exit 1 }

& "$Root\scripts\build.ps1" -Target tauri -Profile release
if ($LASTEXITCODE -ne 0) { Write-Host "[FAIL] Tauri build failed." -ForegroundColor Red; exit 1 }

& "$Root\scripts\build.ps1" -Target wasm -Profile release
if ($LASTEXITCODE -ne 0) { Write-Host "[FAIL] WASM build failed." -ForegroundColor Red; exit 1 }

# ── Step 3: Deploy APO (optional) ──
if ($Deploy) {
    Write-Host "`n[release] Deploying APO DLL..." -ForegroundColor Yellow
    & "$Root\scripts\deploy-apo.ps1"
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[FAIL] Deploy failed." -ForegroundColor Red
        exit 1
    }
}

# ── Step 4: Git tag (optional) ──
if ($Tag) {
    Write-Host "`n[release] Creating git tag: $Tag" -ForegroundColor Yellow

    # Check for uncommitted changes
    $gitStatus = git status --porcelain
    if ($gitStatus) {
        Write-Host "[WARN] Uncommitted changes detected. Commit before tagging." -ForegroundColor Yellow
        Write-Host $gitStatus -ForegroundColor Gray
        exit 1
    }

    # Check if tag already exists
    $existingTag = git tag -l $Tag
    if ($existingTag) {
        Write-Host "[FAIL] Tag $Tag already exists." -ForegroundColor Red
        exit 1
    }

    git tag -a $Tag -m "Release $Tag"
    if ($LASTEXITCODE -ne 0) { throw "Failed to create tag" }

    git push origin $Tag
    if ($LASTEXITCODE -ne 0) { throw "Failed to push tag" }

    Write-Host "  [ok] Tag $Tag created and pushed." -ForegroundColor Green
}

# ── Summary ──
Write-Host "`n=== Release Complete ===" -ForegroundColor Green
$profileDir = "release"
$artifacts = @(
    @{ Name = "APO DLL"; Path = "$Root\target\$profileDir\phaselith_apo.dll" },
    @{ Name = "Tauri App"; Path = "$Root\target\$profileDir\phaselith-tauri.exe" },
    @{ Name = "WASM"; Path = "$Root\chrome-ext\phaselith_wasm_bridge.wasm" }
)
foreach ($a in $artifacts) {
    if (Test-Path $a.Path) {
        $size = (Get-Item $a.Path).Length
        if ($size -gt 1MB) { $sizeStr = "{0:N1} MB" -f ($size / 1MB) }
        elseif ($size -gt 1KB) { $sizeStr = "{0:N0} KB" -f ($size / 1KB) }
        else { $sizeStr = "$size B" }
        Write-Host "  $($a.Name): $($a.Path) ($sizeStr)" -ForegroundColor Gray
    }
}
if ($Tag) { Write-Host "  Tag: $Tag" -ForegroundColor Gray }
if ($Deploy) { Write-Host "  APO deployed." -ForegroundColor Gray }
Write-Host ""
