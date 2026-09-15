param()
$exe = "D:\2\14\-Un-Real-0d23d9ux-Engine-main\src-tauri\target\release\variable.exe"
$p = Start-Process -FilePath $exe -PassThru
"STARTED pid=$($p.Id)"
while (-not $p.HasExited) { Start-Sleep -Milliseconds 500 }
"EXITED code=$($p.ExitCode) at $(Get-Date -Format 'HH:mm:ss.fff')"
