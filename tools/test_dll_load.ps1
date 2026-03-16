Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class DllTest {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr LoadLibrary(string path);
    [DllImport("kernel32.dll")]
    public static extern bool FreeLibrary(IntPtr handle);
    [DllImport("kernel32.dll")]
    public static extern int GetLastError();
}
"@

$h = [DllTest]::LoadLibrary("C:\Program Files\Phaselith\phaselith_apo.dll")
if ($h -eq [IntPtr]::Zero) {
    $err = [System.Runtime.InteropServices.Marshal]::GetLastWin32Error()
    Write-Output "LOAD FAILED: Win32 error $err"
} else {
    Write-Output "LOAD OK: handle=$h"
    [DllTest]::FreeLibrary($h) | Out-Null
}

# Check if any new files were created
Write-Output "--- Files in Phaselith dir ---"
Get-ChildItem "C:\ProgramData\Phaselith" | Format-Table Name, Length, LastWriteTime
