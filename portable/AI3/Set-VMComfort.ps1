# V-1：VM 档体验调优检查（分辨率/剪贴板/驱动器/USB）——只检查+报告，不静默改系统
param([string]$VmName = "VariableOS")
Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"
$report = [ordered]@{}

# 1) 分辨率 / DPI：VMConnect 全屏自适应由 RDP 会话决定，此处检查 VM 显存与增强会话模式
$vm = Get-VM -Name $VmName -ErrorAction SilentlyContinue
if ($vm) {
  $report["VM状态"] = $vm.State
  $report["增强会话模式(宿主侧)"] = (Get-VMHost).EnableEnhancedSessionMode
  $report["内存(动态)"] = "{0:N0}-{1:N0} MB" -f ($vm.MemoryMinimum/1MB), ($vm.MemoryMaximum/1MB)
} else { $report["VM状态"] = "未找到 $VmName（未启动/未创建）" }

# 2) 剪贴板 / 驱动器桥接：Hyper-V 增强会话（vmconnect -UseEvent? 用 RDP 通道）
#    HV 服务（VM 内需启用）：VmRdpUid / IC 剪贴板 = "Guest Services Interface"
if ($vm) {
  $gsi = Get-VMIntegrationService -VM $vm | Where-Object { $_.Name -match "Guest Services|剪贴板|Clipboard" }
  $report["集成服务"] = ($gsi | ForEach-Object { "$($_.Name)=$($_.Enabled)" }) -join "; "
}

# 3) USB 重定向：Hyper-V 无原生 USB 直通（DDA 需服务器级硬件）→ 诚实声明
$report["USB直通"] = "Hyper-V 不支持通用 USB 直通（DDA 除外）；USB 设备请在轻量直跑档使用，或经网络桥接"

$report | Format-List | Out-String | ForEach-Object {
  Write-Host $_
  $_ | Out-File -FilePath (Join-Path $PSScriptRoot "comfort-report.txt") -Encoding utf8 -Append
}
Write-Host "报告已追加到 comfort-report.txt" -ForegroundColor Green
