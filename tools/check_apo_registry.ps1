$clsid = '{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}'
$basePath = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'
$found = $false

Get-ChildItem $basePath -ErrorAction SilentlyContinue | ForEach-Object {
    $fxPath = Join-Path $_.PSPath 'FxProperties'
    if (Test-Path $fxPath) {
        $props = Get-ItemProperty $fxPath -ErrorAction SilentlyContinue
        $props.PSObject.Properties | ForEach-Object {
            if ($_.Value -eq $clsid) {
                $found = $true
                Write-Output "FOUND: $($_.Name) = $($_.Value)"
            }
        }
    }
}

if (-not $found) {
    Write-Output "APO CLSID not found in any render endpoint FxProperties!"
    Write-Output "The APO is not bound to any audio endpoint."
}
