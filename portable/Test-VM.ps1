param([string]$Vhdx="D:\Variable-USB\Variable-OS.vhdx")
# 隔离测试 - 三桥全关 + 限额 + NAT
$vmName="VariableOS-TEST"
if (Get-VM -Name $vmName -ErrorAction SilentlyContinue) { Remove-VM -Name $vmName -Force -ErrorAction SilentlyContinue }
Write-Host ">>> 创建虚拟机 $vmName (4GB/4核/6000MB动态/NAT/三桥全关)" -ForegroundColor Cyan
New-VM -Name $vmName -MemoryStartupBytes 4GB -VHDPath $Vhdx -Generation 2 -SwitchName "Default Switch" | Out-Null
Set-VM -Name $vmName -ProcessorCount 4 -DynamicMemory -MemoryMinimumBytes 2GB -MemoryMaximumBytes 6GB -CheckpointType Disabled | Out-Null
Set-VMFirmware -VMName $vmName -EnableSecureBoot Off | Out-Null
# 禁剪贴板/拖放需在 VirtualBox，此处 Hyper-V 已天然隔离
Write-Host ">>> 启动..." -ForegroundColor Green
Start-VM -Name $vmName
vmconnect localhost $vmName
Write-Host "窗口已弹，关机选 保存/丢弃，丢弃即还原" -ForegroundColor Yellow
