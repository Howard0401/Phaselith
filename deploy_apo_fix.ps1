$ErrorActionPreference = "Stop"
$src = "C:\Users\Howard\Desktop\claude\target\release\phaselith_apo.dll"
$dst = "C:\Program Files\Phaselith\phaselith_apo.dll"
$log = "C:\Users\Howard\Desktop\claude\deploy_fix_log.txt"

"Starting deploy at $(Get-Date)" | Out-File $log
"Source size: $((Get-Item $src).Length)" | Out-File $log -Append

try {
    Stop-Service -Name Audiosrv -Force -ErrorAction SilentlyContinue
    Stop-Service -Name AudioEndpointBuilder -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 3

    # Kill audiodg if still running
    Get-Process audiodg -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Seconds 2

    "Before copy - dest size: $((Get-Item $dst -ErrorAction SilentlyContinue).Length)" | Out-File $log -Append

    Copy-Item $src $dst -Force

    "After copy - dest size: $((Get-Item $dst).Length)" | Out-File $log -Append
    "Copy SUCCESS" | Out-File $log -Append
} catch {
    "Copy FAILED: $_" | Out-File $log -Append
}

Start-Service -Name AudioEndpointBuilder -ErrorAction SilentlyContinue
Start-Service -Name Audiosrv -ErrorAction SilentlyContinue
"Services restarted" | Out-File $log -Append
