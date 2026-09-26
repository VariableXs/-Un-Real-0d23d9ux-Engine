
$ErrorActionPreference = 'Stop'
$log = 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\hiberboot-off.log'
function Say($m) { Add-Content -Path $log -Value $m }
try {
  $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power'
  $val = 0
  Set-ItemProperty -Path $path -Name HiberbootEnabled -Value $val -Type DWord
  $back = (Get-ItemProperty -Path $path -Name HiberbootEnabled).HiberbootEnabled
  if ($back -ne $val) { throw "read-back mismatch: $back" }
  Say "HIBERBOOT-SET-OK HiberbootEnabled=$back"
} catch {
  Say "HIBERBOOT-SET-FAIL $($_.Exception.Message)"
}
