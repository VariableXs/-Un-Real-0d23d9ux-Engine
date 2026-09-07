<#
.SYNOPSIS
    假启动壳：50–100 ms 内弹出可取消进度，真进程在 Job 语义下由 isolation.rs 启动。

.DESCRIPTION
    本脚本不实现 Variable-Loading.exe 的 Win32 窗口；它提供可脚本化的进度节拍、
    30 秒熔断和取消通道，供 AI-2 验收与 Core 对接。不会 Terminate 任意宿主进程。
#>
[CmdletBinding()]
param(
    [string]$Name = "Blender",
    [int]$TimeoutSeconds = 30,
    [switch]$Cancel
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$started = Get-Date
Write-Host "[AI-2] Fake-Start shell for '$Name' at $($started.ToString('o'))" -ForegroundColor Cyan

if ($Cancel) {
    Write-Host "[AI-2] 用户取消：应对应 TerminateJobObject，而不是杀宿主 explorer。" -ForegroundColor Yellow
    exit 2
}

$deadline = $started.AddSeconds($TimeoutSeconds)
$tick = 0
while ((Get-Date) -lt $deadline) {
    $tick++
    $elapsed = ((Get-Date) - $started).TotalMilliseconds
    Write-Host ("[AI-2] progress tick={0} elapsedMs={1:N0}" -f $tick, $elapsed)
    Start-Sleep -Milliseconds 100
    if ($tick -ge 3) {
        Write-Host "[AI-2] 壳可拖动/关闭；真窗口替换由 SetParent 在 Windows 构建完成。" -ForegroundColor Green
        exit 0
    }
}

Write-Host "[AI-2] 30s 熔断：建议 /safe /no-plugins 重试，不杀宿主。" -ForegroundColor Yellow
exit 3
