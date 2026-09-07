param([string]$Src="D:\Variable-USB", [string]$Dst="E:\")
if (-not (Test-Path $Dst)) { throw "U盘 $Dst 不存在，请先插盘" }
Write-Host ">>> 部署 $Src -> $Dst (robocopy /MT:8)" -ForegroundColor Cyan
robocopy $Src $Dst /E /R:2 /W:2 /MT:8 /XD "Cache" "Temp" /XF "*.log"
Write-Host ">>> 完成，U盘已是成品盘，A模式双击 PortableVM/启动.exe，B模式重启F12" -ForegroundColor Green
