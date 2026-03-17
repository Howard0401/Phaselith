<#
.SYNOPSIS
    Unified build script for Phaselith.

.PARAMETER Target
    Build target: all (default), test, apo, tauri, wasm

.PARAMETER Profile
    Build profile: release (default), debug

.PARAMETER Version
    Bump version across all manifests before building.

.EXAMPLE
    .\scripts\build.ps1
    .\scripts\build.ps1 -Target apo
    .\scripts\build.ps1 -Profile debug -Target tauri
    .\scripts\build.ps1 -Version 0.1.19
#>
param(
    [ValidateSet("all", "test", "apo", "tauri", "wasm")]
    [string]$Target = "all",

    [ValidateSet("release", "debug")]
    [string]$Profile = "release",

    [string]$Version
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

# ── Version bump ──
if ($Version) {
    Write-Host "`n[version] Bumping to $Version" -ForegroundColor Cyan

    # Cargo.toml workspace version
    $cargo = Get-Content "$Root\Cargo.toml" -Raw
    $cargo = $cargo -replace 'version = "[^"]*"', "version = `"$Version`""
    Set-Content "$Root\Cargo.toml" $cargo -NoNewline

    # tauri.conf.json
    $tauri = Get-Content "$Root\crates\tauri-app\tauri.conf.json" -Raw
    $tauri = $tauri -replace '"version": "[^"]*"', "`"version`": `"$Version`""
    Set-Content "$Root\crates\tauri-app\tauri.conf.json" $tauri -NoNewline

    # App.vue footer
    $vue = Get-Content "$Root\crates\tauri-app\frontend\src\App.vue" -Raw
    $vue = $vue -replace 'Phaselith v[0-9]+\.[0-9]+\.[0-9]+', "Phaselith v$Version"
    Set-Content "$Root\crates\tauri-app\frontend\src\App.vue" $vue -NoNewline

    # Chrome extension manifest.json
    $manifest = Get-Content "$Root\chrome-ext\manifest.json" -Raw
    $manifest = $manifest -replace '"version": "[^"]*"', "`"version`": `"$Version`""
    Set-Content "$Root\chrome-ext\manifest.json" $manifest -NoNewline

    Write-Host "  [ok] All manifests updated to $Version" -ForegroundColor Green
}

# ── Helpers ──
function Format-Size($bytes) {
    if ($bytes -gt 1MB) { return "{0:N1} MB" -f ($bytes / 1MB) }
    if ($bytes -gt 1KB) { return "{0:N0} KB" -f ($bytes / 1KB) }
    return "$bytes B"
}

$profileFlag = if ($Profile -eq "release") { "--release" } else { $null }
$profileDir = if ($Profile -eq "release") { "release" } else { "debug" }
$results = @()

function Run-Step($name, $scriptBlock) {
    Write-Host "`n[$name] Building..." -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        & $scriptBlock
        $sw.Stop()
        Write-Host "  [ok] $name completed in $($sw.Elapsed.TotalSeconds.ToString('F1'))s" -ForegroundColor Green
        $script:results += [PSCustomObject]@{ Target = $name; Status = "ok"; Time = $sw.Elapsed.TotalSeconds }
    }
    catch {
        $sw.Stop()
        Write-Host "  [FAIL] $name failed: $_" -ForegroundColor Red
        $script:results += [PSCustomObject]@{ Target = $name; Status = "FAIL"; Time = $sw.Elapsed.TotalSeconds }
        throw
    }
}

# ── Targets ──
function Build-Test {
    Run-Step "test" {
        cargo test -p phaselith-dsp-core
        if ($LASTEXITCODE -ne 0) { throw "Tests failed" }
    }
}

function Build-Apo {
    Run-Step "apo" {
        $args = @("build", "-p", "phaselith-apo")
        if ($profileFlag) { $args += $profileFlag }
        cargo @args
        if ($LASTEXITCODE -ne 0) { throw "APO build failed" }
        $dll = "$Root\target\$profileDir\phaselith_apo.dll"
        if (Test-Path $dll) {
            $size = Format-Size (Get-Item $dll).Length
            Write-Host "  -> $dll ($size)" -ForegroundColor Gray
        }
    }
}

function Build-Tauri {
    Run-Step "tauri" {
        $args = @("build", "-p", "phaselith-tauri")
        if ($profileFlag) { $args += $profileFlag }
        cargo @args
        if ($LASTEXITCODE -ne 0) { throw "Tauri build failed" }
        $exe = "$Root\target\$profileDir\phaselith-tauri.exe"
        if (Test-Path $exe) {
            $size = Format-Size (Get-Item $exe).Length
            Write-Host "  -> $exe ($size)" -ForegroundColor Gray
        }
    }
}

function Build-Wasm {
    Run-Step "wasm" {
        cargo build -p phaselith-wasm-bridge --target wasm32-unknown-unknown $profileFlag
        if ($LASTEXITCODE -ne 0) { throw "WASM build failed" }
        $wasm = "$Root\target\wasm32-unknown-unknown\$profileDir\phaselith_wasm_bridge.wasm"
        $dest = "$Root\chrome-ext\phaselith_wasm_bridge.wasm"
        if (Test-Path $wasm) {
            Copy-Item $wasm $dest -Force
            $size = Format-Size (Get-Item $dest).Length
            Write-Host "  -> $dest ($size)" -ForegroundColor Gray
        }
    }
}

# ── Dispatch ──
switch ($Target) {
    "test"  { Build-Test }
    "apo"   { Build-Apo }
    "tauri" { Build-Tauri }
    "wasm"  { Build-Wasm }
    "all"   {
        Build-Test
        Build-Apo
        Build-Tauri
        Build-Wasm
    }
}

# ── Summary ──
Write-Host "`n=== Build Summary ===" -ForegroundColor White
foreach ($r in $results) {
    $icon = if ($r.Status -eq "ok") { "[ok]" } else { "[FAIL]" }
    $color = if ($r.Status -eq "ok") { "Green" } else { "Red" }
    Write-Host ("  {0,-6} {1,-10} {2:F1}s" -f $icon, $r.Target, $r.Time) -ForegroundColor $color
}
Write-Host ""
