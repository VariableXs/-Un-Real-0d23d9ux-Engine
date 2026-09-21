$ErrorActionPreference='Stop'
schtasks /Create /TN 'VarixW1Finalize' /TR "powershell.exe -NoProfile -ExecutionPolicy Bypass -File 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Finalize.ps1'" /SC ONCE /ST 23:59 /RL HIGHEST /F | Out-Null
if ($LASTEXITCODE -ne 0) { throw ('create failed ' + $LASTEXITCODE) }
schtasks /Run /TN 'VarixW1Finalize' | Out-Null
if ($LASTEXITCODE -ne 0) { throw ('run failed ' + $LASTEXITCODE) }
Write-Output 'SETUP-OK'
