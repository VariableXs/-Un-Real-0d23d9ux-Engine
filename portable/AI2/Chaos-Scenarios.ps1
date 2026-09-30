<#
.SYNOPSIS
    扩充 21：打印/写入十场景验收表。默认不执行破坏性动作。
#>
[CmdletBinding()]
param(
    [string]$OutFile,
    [switch]$ExecuteDestructive
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($ExecuteDestructive) {
    throw "[AI-2] 拒绝自动执行破坏性混沌。请人工按清单操作，并把证据写入报告。"
}

& (Join-Path $PSScriptRoot "Test-VM.ps1") -ListScenarios
if ($OutFile) {
    & (Join-Path $PSScriptRoot "Test-VM.ps1") -ScenarioReport $OutFile -Vhdx "D:\Variable-USB\Variable-OS.vhdx"
}
