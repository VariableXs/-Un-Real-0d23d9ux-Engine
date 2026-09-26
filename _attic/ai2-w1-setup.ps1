$ErrorActionPreference='Stop'
schtasks /Create /TN 'VarixW1Deploy' /TR "powershell.exe -NoProfile -ExecutionPolicy Bypass -File 'D:\2\14\-Un-Real-0d23d9ux-Engine-main\portable\engine\W1-Deploy.ps1' -IsoPath 'D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso'" /SC ONCE /ST 23:59 /RL HIGHEST /F | Out-Null
if ($LASTEXITCODE -ne 0) { throw ('schtasks create failed ' + $LASTEXITCODE) }
schtasks /Run /TN 'VarixW1Deploy' | Out-Null
if ($LASTEXITCODE -ne 0) { throw ('schtasks run failed ' + $LASTEXITCODE) }
Write-Output 'SETUP-OK task created and started'
