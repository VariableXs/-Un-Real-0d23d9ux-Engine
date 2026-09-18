& "D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Manage-Versions.ps1" -Action Snapshot -Root "$env:TEMP\mv-hist\usbroot" -Label first-burn
Set-Content "$env:TEMP\mv-hist\usbroot\kernel\varix" 'k2' -Encoding ASCII
& "D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\AI-P\Manage-Versions.ps1" -Action Snapshot -Root "$env:TEMP\mv-hist\usbroot"
Write-Host '--- history.json ---'
Get-Content "$env:TEMP\mv-hist\usbroot\_versions\history.json" -Raw | Write-Host
$h = @(Get-Content "$env:TEMP\mv-hist\usbroot\_versions\history.json" -Raw | ConvertFrom-Json)
Write-Host ("hist count=" + $h.Count)
$last = $h | Select-Object -Last 1
Write-Host ("last type=" + $last.GetType().Name + " ver=" + $last.version)
