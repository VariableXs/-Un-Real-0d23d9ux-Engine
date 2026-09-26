# update-varix-usb.ps1 (wrapper) - run in ADMIN terminal.
# Creates a one-shot SYSTEM scheduled task (SYSTEM can write the ESP),
# runs the real worker script, waits for DONE marker in the log.
$ErrorActionPreference = "Stop"
$repo   = "d:\2\14\-Un-Real-0d23d9ux-Engine-main"
$worker = Join-Path $repo "tools\update-varix-usb-sys.ps1"
$log    = Join-Path $repo "reports\usb-update-log.txt"

$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { Write-Host "NOT ADMIN - open Terminal (Admin) first"; exit 1 }

"=== wrapper run $(Get-Date -Format s) ===" | Out-File $log -Encoding utf8

schtasks /create /tn "VarixUsbUpdate" /tr "powershell -NoProfile -ExecutionPolicy Bypass -File `"$worker`"" /sc once /st 23:59 /ru SYSTEM /f | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Host "task create failed: $LASTEXITCODE"; exit 1 }
schtasks /run /tn "VarixUsbUpdate" | Out-Null
if ($LASTEXITCODE -ne 0) { Write-Host "task run failed: $LASTEXITCODE"; exit 1 }

$deadline = (Get-Date).AddSeconds(120)
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Seconds 2
  if (Test-Path $log) {
    $tail = Get-Content $log -Raw -ErrorAction SilentlyContinue
    if ($tail -match "DONE (OK|FAIL)") { break }
  }
}
schtasks /delete /tn "VarixUsbUpdate" /f | Out-Null

Get-Content $log
