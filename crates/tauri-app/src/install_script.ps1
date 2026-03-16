$ErrorActionPreference = 'Continue'
$log = @()

# ============================================================
# Phaselith APO Install Script (runs elevated)
# ============================================================
# Key facts:
# - audiodg.exe runs as LocalService → cannot access user dirs
# - {d04e05a6...},13 = PKEY_CompositeFX_StreamEffectClsid (CLSIDs)
# - {d04e05a6...},5  = PKEY_FX_StreamEffectClsid V1 (single CLSID)
# - {d3993a3f...},5  = PKEY_SFX_ProcessingModes (modes, NOT CLSIDs!)
# - FxProperties keys are owned by TrustedInstaller
# ============================================================

$dllSource = '__DLL_PATH__'
$installDir = '__INSTALL_DIR__'
$apoClsid = '__APO_CLSID__'
$renderBase = '__RENDER_BASE__'
$renderPath = "HKLM:\$renderBase"

# Property key for CompositeFX Stream Effect CLSIDs
$pkeyCompositeSfx = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13'
# Property key for V1 Stream Effect CLSID (single)
$pkeyV1Sfx = '{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},5'

# ---- Step 0: Set permissions on Phaselith data dir for LocalService (audiodg) ----
$phaselithDataDir = 'C:\ProgramData\Phaselith'
if (-not (Test-Path $phaselithDataDir)) {
    New-Item -ItemType Directory -Path $phaselithDataDir -Force | Out-Null
}
# Grant LocalService full control so audiodg can write debug logs + mmap
$acl = Get-Acl $phaselithDataDir
$localServiceRule = New-Object System.Security.AccessControl.FileSystemAccessRule(
    'NT AUTHORITY\LOCAL SERVICE', 'FullControl',
    'ContainerInherit,ObjectInherit', 'None', 'Allow')
$acl.AddAccessRule($localServiceRule)
# Also grant Everyone read/write for mmap IPC
$everyoneRule = New-Object System.Security.AccessControl.FileSystemAccessRule(
    'Everyone', 'FullControl',
    'ContainerInherit,ObjectInherit', 'None', 'Allow')
$acl.AddAccessRule($everyoneRule)
Set-Acl $phaselithDataDir $acl

# Clear old debug log so we get clean counts after this install
$apoLogFile = Join-Path $phaselithDataDir 'apo_debug.log'
if (Test-Path $apoLogFile) { Remove-Item $apoLogFile -Force -ErrorAction SilentlyContinue }

# ---- Step 1: Copy DLL to system-accessible location ----
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}
$dllDest = Join-Path $installDir 'phaselith_apo.dll'
Copy-Item -Path $dllSource -Destination $dllDest -Force
# Also copy PDB for debugging
$pdbSource = $dllSource -replace '\.dll$', '.pdb'
if (Test-Path $pdbSource) {
    Copy-Item -Path $pdbSource -Destination (Join-Path $installDir 'phaselith_apo.pdb') -Force
}
$log += "dll copied to $dllDest"

# ---- Step 2: Register COM server via .NET Registry API ----
# NOTE: Both regsvr32 and PowerShell HKLM: drive are virtualized in sandbox.
# Use .NET Microsoft.Win32.Registry API directly (same as TakeOwnership — proven to work).
function Set-RegString {
    param([string]$SubKey, [string]$Name, [string]$Value)
    $key = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey($SubKey)
    if ($key) {
        $key.SetValue($Name, $Value, [Microsoft.Win32.RegistryValueKind]::String)
        $key.Close()
        return $true
    }
    return $false
}
function Set-RegDword {
    param([string]$SubKey, [string]$Name, [int]$Value)
    $key = [Microsoft.Win32.Registry]::LocalMachine.CreateSubKey($SubKey)
    if ($key) {
        $key.SetValue($Name, $Value, [Microsoft.Win32.RegistryValueKind]::DWord)
        $key.Close()
        return $true
    }
    return $false
}

# COM InprocServer32 — use reg.exe to bypass .NET registry virtualization
# .NET Registry API writes are visible to PowerShell but NOT to native COM (combase.dll)
$comSubKey = "SOFTWARE\Classes\CLSID\$apoClsid"
$inprocSubKey = "$comSubKey\InprocServer32"

# Method 1: reg.exe (native Win32, same code path as COM)
$regHklmClsid = "HKLM\SOFTWARE\Classes\CLSID\$apoClsid"
$regHklmInproc = "$regHklmClsid\InprocServer32"
$regHkcrClsid = "HKCR\CLSID\$apoClsid"
$regHkcrInproc = "$regHkcrClsid\InprocServer32"

# Write to BOTH HKLM and HKCR to maximize visibility
reg add $regHklmClsid /ve /d 'Phaselith Audio Enhancement' /f 2>&1 | Out-Null
reg add $regHklmInproc /ve /d $dllDest /f 2>&1 | Out-Null
reg add $regHklmInproc /v ThreadingModel /d Both /f 2>&1 | Out-Null
reg add $regHkcrClsid /ve /d 'Phaselith Audio Enhancement' /f 2>&1 | Out-Null
reg add $regHkcrInproc /ve /d $dllDest /f 2>&1 | Out-Null
reg add $regHkcrInproc /v ThreadingModel /d Both /f 2>&1 | Out-Null

# Method 2: Also write via .NET as backup
Set-RegString -SubKey $comSubKey -Name '' -Value 'Phaselith Audio Enhancement' | Out-Null
$comOk = Set-RegString -SubKey $inprocSubKey -Name '' -Value $dllDest
Set-RegString -SubKey $inprocSubKey -Name 'ThreadingModel' -Value 'Both' | Out-Null

# Verify via reg.exe query (native, same as COM would use)
$regQuery = reg query $regHkcrInproc /ve 2>&1
$comVerifyNative = $regQuery -match [regex]::Escape($dllDest)

# Verify via .NET API
$verifyKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($inprocSubKey)
$comVerifyNet = $false
if ($verifyKey) {
    $dllVal = $verifyKey.GetValue('')
    $comVerifyNet = ($dllVal -eq $dllDest)
    $verifyKey.Close()
}

# Test CoCreateInstance from this elevated process
$comCreateOk = $false
try {
    $type = [Type]::GetTypeFromCLSID([Guid]$apoClsid)
    if ($type) {
        $obj = [Activator]::CreateInstance($type)
        $comCreateOk = $true
    }
} catch {}

$log += "COM: reg.exe=$comVerifyNative .NET=$comVerifyNet CoCreate=$comCreateOk"

# --- COM checkpoint helper ---
function Check-COM {
    param([string]$Label)
    $ck = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($inprocSubKey)
    $exists = $false
    $val = ''
    if ($ck) { $exists = $true; $val = $ck.GetValue(''); $ck.Close() }
    $script:log += "COM-check($Label): exists=$exists val=$val"
}
Check-COM -Label 'after-step2'

# APO catalog — also use reg.exe for native visibility
$apoSubKey = "SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$apoClsid"
$regApo = "HKLM\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$apoClsid"

# reg.exe writes (native)
reg add $regApo /v CLSID /d $apoClsid /f 2>&1 | Out-Null
reg add $regApo /v FriendlyName /d 'Phaselith Audio Enhancement' /f 2>&1 | Out-Null
reg add $regApo /v Copyright /d 'Phaselith Project' /f 2>&1 | Out-Null
reg add $regApo /v MajorVersion /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v MinorVersion /t REG_DWORD /d 0 /f 2>&1 | Out-Null
reg add $regApo /v Flags /t REG_DWORD /d 14 /f 2>&1 | Out-Null
reg add $regApo /v MinInputConnections /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v MaxInputConnections /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v MinOutputConnections /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v MaxOutputConnections /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v MaxInstances /t REG_DWORD /d 2147483647 /f 2>&1 | Out-Null
reg add $regApo /v NumAPOInterfaces /t REG_DWORD /d 1 /f 2>&1 | Out-Null
reg add $regApo /v APOInterface0 /d '{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}' /f 2>&1 | Out-Null

# Verify via reg.exe query
$apoVerifyNative = (reg query $regApo /v CLSID 2>&1) -match 'A1B2C3D4'
$log += "APO catalog: native=$apoVerifyNative"
Check-COM -Label 'after-apo-catalog'

# ---- Step 3: Enable TakeOwnership privilege ----
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Priv
{
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr h, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool LookupPrivilegeValue(string s, string n, out long l);
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool AdjustTokenPrivileges(IntPtr t, bool d, ref TP n, int b, IntPtr p, IntPtr r);
    [StructLayout(LayoutKind.Sequential)]
    struct TP { public int C; public long L; public int A; }
    public static bool E(string p) {
        IntPtr t; if (!OpenProcessToken((IntPtr)(-1), 0x28, out t)) return false;
        long l; if (!LookupPrivilegeValue(null, p, out l)) return false;
        TP tp = new TP(); tp.C=1; tp.L=l; tp.A=2;
        return AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
    }
}
'@ -ErrorAction Stop

[Priv]::E("SeTakeOwnershipPrivilege") | Out-Null
[Priv]::E("SeRestorePrivilege") | Out-Null
Check-COM -Label 'after-step3-priv'

# ---- Step 4: Bind APO to active render endpoints ----
# NOTE: ALL registry operations use .NET Registry API to avoid PowerShell provider virtualization.
$boundCount = 0
$totalEps = 0
$details = @()
$pkeyModes = '{d3993a3f-99c2-4402-b5ec-a92a0367664b},5'

function Take-Ownership {
    param([string]$SubKeyPath)
    try {
        $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(
            $SubKeyPath,
            [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
            [System.Security.AccessControl.RegistryRights]::TakeOwnership)
        if (-not $key) { return $false }
        $acl = $key.GetAccessControl([System.Security.AccessControl.AccessControlSections]::Owner)
        $admin = [System.Security.Principal.NTAccount]'BUILTIN\Administrators'
        $acl.SetOwner($admin)
        $key.SetAccessControl($acl)
        $key.Close()
        $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(
            $SubKeyPath,
            [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
            [System.Security.AccessControl.RegistryRights]::ChangePermissions -bor
            [System.Security.AccessControl.RegistryRights]::ReadKey)
        if (-not $key) { return $false }
        $acl = $key.GetAccessControl()
        $rule = New-Object System.Security.AccessControl.RegistryAccessRule(
            'BUILTIN\Administrators', 'FullControl',
            'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
        $key.SetAccessControl($acl)
        $key.Close()
        return $true
    } catch { return $false }
}

# Helper: Read registry value via .NET API (avoids PS provider virtualization)
function Get-RegValue {
    param([string]$SubKey, [string]$Name)
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($SubKey)
    if (-not $k) { return $null }
    $val = $k.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = $null
    try { $kind = $k.GetValueKind($Name) } catch {}
    $k.Close()
    return @{ Value = $val; Kind = $kind }
}

# Helper: Write MultiString via .NET API
function Set-RegMultiString {
    param([string]$SubKey, [string]$Name, [string[]]$Values)
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($SubKey, $true)
    if (-not $k) { return $false }
    $k.SetValue($Name, $Values, [Microsoft.Win32.RegistryValueKind]::MultiString)
    $k.Close()
    return $true
}

# Helper: Write String via .NET API
function Set-RegSzValue {
    param([string]$SubKey, [string]$Name, [string]$Value)
    $k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($SubKey, $true)
    if (-not $k) { return $false }
    $k.SetValue($Name, $Value, [Microsoft.Win32.RegistryValueKind]::String)
    $k.Close()
    return $true
}

# Pre-pass: Clean corrupted ProcessingModes on ALL endpoints (active + inactive).
# Previous install scripts may have accidentally written APO CLSIDs into
# {d3993a3f...},5 (PKEY_SFX_ProcessingModes). This key should ONLY contain
# audio signal processing mode GUIDs (e.g. Default mode), never APO CLSIDs.
$cleanedModes = 0
$preCleanKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($renderBase)
if ($preCleanKey) {
    foreach ($epGuid in $preCleanKey.GetSubKeyNames()) {
        $fxSubKey = "$renderBase\$epGuid\FxProperties"
        $fxTest = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxSubKey)
        if (-not $fxTest) { continue }
        $fxTest.Close()
        try {
            $modesInfo = Get-RegValue -SubKey $fxSubKey -Name $pkeyModes
            if ($modesInfo -and $modesInfo.Value) {
                $modesArr = @($modesInfo.Value)
                $hasOurClsid = $false
                foreach ($m in $modesArr) { if ($m -ieq $apoClsid) { $hasOurClsid = $true; break } }
                if ($hasOurClsid) {
                    $owned = Take-Ownership -SubKeyPath $fxSubKey
                    if ($owned) {
                        $cleanModes = @($modesArr | Where-Object { $_ -ine $apoClsid })
                        if ($cleanModes.Count -gt 0) {
                            Set-RegMultiString -SubKey $fxSubKey -Name $pkeyModes -Values $cleanModes | Out-Null
                        }
                        $cleanedModes++
                    }
                }
            }
        } catch {}
    }
    $preCleanKey.Close()
}
if ($cleanedModes -gt 0) { $log += "pre-clean: removed CLSID from $cleanedModes modes keys" }

# Enumerate active render endpoints via .NET API
$renderKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($renderBase)
if ($renderKey) {
    foreach ($epGuid in $renderKey.GetSubKeyNames()) {
        $epKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("$renderBase\$epGuid")
        if (-not $epKey) { continue }
        $devState = $epKey.GetValue('DeviceState', 0)
        $epKey.Close()
        if ($devState -ne 1) { continue } # Skip inactive
        $totalEps++

        $fxSubKey = "$renderBase\$epGuid\FxProperties"
        $fxKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxSubKey)
        if (-not $fxKey) { continue }
        $fxKey.Close()

        $bound = $false

        # Step 4b: Try CompositeFX SFX first (takes priority over V1)
        try {
            $cfxInfo = Get-RegValue -SubKey $fxSubKey -Name $pkeyCompositeSfx
            if ($cfxInfo -and $cfxInfo.Value -and $cfxInfo.Kind -eq 'MultiString') {
                $cfxVal = @($cfxInfo.Value)
                if ($cfxVal.Count -gt 0) {
                    $already = $false
                    foreach ($s in $cfxVal) { if ($s -ieq $apoClsid) { $already = $true; break } }
                    if ($already) {
                        $details += "${epGuid}: CompositeFX(already)"
                        $bound = $true
                        $boundCount++
                    } else {
                        $owned = Take-Ownership -SubKeyPath $fxSubKey
                        if ($owned) {
                            $newList = $cfxVal + @($apoClsid)
                            $writeOk = Set-RegMultiString -SubKey $fxSubKey -Name $pkeyCompositeSfx -Values $newList
                            # Verify the write persisted
                            $verifyInfo = Get-RegValue -SubKey $fxSubKey -Name $pkeyCompositeSfx
                            $verified = $false
                            if ($verifyInfo -and $verifyInfo.Value) {
                                foreach ($v in @($verifyInfo.Value)) { if ($v -ieq $apoClsid) { $verified = $true; break } }
                            }
                            if ($verified) {
                                $details += "${epGuid}: CompositeFX(bound,verified)"
                                $bound = $true
                                $boundCount++
                            } else {
                                $details += "${epGuid}: CompositeFX(WRITE-FAILED)"
                            }
                        } else {
                            $details += "${epGuid}: CompositeFX(own-fail)"
                        }
                    }
                }
            }
        } catch {
            $details += "${epGuid}: CompositeFX-err=$_"
        }

        # Step 4c: If no CompositeFX, try V1 SFX
        if (-not $bound) {
            try {
                $v1Info = Get-RegValue -SubKey $fxSubKey -Name $pkeyV1Sfx
                if ($v1Info -and $v1Info.Value -and $v1Info.Value -is [string]) {
                    $v1Val = $v1Info.Value
                    if ($v1Val -ine $apoClsid -and $v1Val.Length -gt 0) {
                        # V1 has another CLSID — upgrade to CompositeFX with original + ours
                        $owned = Take-Ownership -SubKeyPath $fxSubKey
                        if ($owned) {
                            $newList = @($v1Val, $apoClsid)
                            $writeOk = Set-RegMultiString -SubKey $fxSubKey -Name $pkeyCompositeSfx -Values $newList
                            $verifyInfo = Get-RegValue -SubKey $fxSubKey -Name $pkeyCompositeSfx
                            $verified = $false
                            if ($verifyInfo -and $verifyInfo.Value) {
                                foreach ($v in @($verifyInfo.Value)) { if ($v -ieq $apoClsid) { $verified = $true; break } }
                            }
                            if ($verified) {
                                $details += "${epGuid}: V1->CompositeFX(bound,verified)"
                                $bound = $true
                                $boundCount++
                            } else {
                                $details += "${epGuid}: V1->CompositeFX(WRITE-FAILED)"
                            }
                        } else {
                            $details += "${epGuid}: V1(own-fail)"
                        }
                    } elseif ($v1Val -ieq $apoClsid) {
                        $details += "${epGuid}: V1(already)"
                        $bound = $true
                        $boundCount++
                    }
                }
            } catch {
                $details += "${epGuid}: V1-err=$_"
            }
        }

        if (-not $bound) {
            $details += "${epGuid}: no-sfx-keys"
        }
    }
    $renderKey.Close()
}

$log += "bound=$boundCount/$totalEps"
if ($details.Count -gt 0) {
    $log += "details: $($details -join ', ')"
}
Check-COM -Label 'after-step4-bind'

# ---- Step 5: Restart audio service ----
Restart-Service AudioEndpointBuilder -Force
Start-Sleep -Seconds 3
$log += "service restarted"
Check-COM -Label 'after-step5-restart'

# ---- Step 6: Wait for audiodg ----
$waited = 0
while ($waited -lt 10) {
    $proc = Get-Process audiodg -ErrorAction SilentlyContinue
    if ($proc) { break }
    Start-Sleep -Seconds 1
    $waited++
}
if ($waited -ge 10) { $log += "audiodg timeout" }
else { $log += "audiodg after ${waited}s" }

# ---- Step 7: Play test sound ----
try {
    [System.Media.SystemSounds]::Beep.Play()
    Start-Sleep -Milliseconds 500
    [System.Media.SystemSounds]::Beep.Play()
    Start-Sleep -Seconds 2
    $log += "test sound played"
} catch { $log += "test sound failed: $_" }

# ---- Step 8: Check APO debug log ----
$apoLog = 'C:\ProgramData\Phaselith\apo_debug.log'
if (Test-Path $apoLog) {
    $lines = Get-Content $apoLog -Tail 30
    $hasInit = ($lines | Select-String 'Initialize\(sr=').Count
    $hasLock = ($lines | Select-String 'LockForProcess called').Count
    $hasProcess = ($lines | Select-String 'APOProcess: first call').Count
    $hasDllGetClass = ($lines | Select-String 'DllGetClassObject called').Count
    $hasDllMain = ($lines | Select-String 'DllMain: DLL_PROCESS_ATTACH').Count
    $hasCreateInst = ($lines | Select-String 'CreateInstance called').Count
    $log += "apo_log: DllMain=$hasDllMain DllGetClass=$hasDllGetClass CreateInst=$hasCreateInst Init=$hasInit Lock=$hasLock Process=$hasProcess"
} else { $log += "apo_log: NOT FOUND" }
Check-COM -Label 'final'

# ---- Step 9: Write marker ----
$log -join '; ' | Out-File -FilePath '__MARKER__' -Encoding UTF8
