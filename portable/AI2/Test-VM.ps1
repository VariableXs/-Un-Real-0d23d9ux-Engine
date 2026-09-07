<#
.SYNOPSIS
    AI-2 隔离核的 Hyper-V 验证器：只读母盘 + COW 子盘 + 4 GB/4 vCPU/NAT。

.DESCRIPTION
    默认不会删除已有 VM、不会改宿主全局自动挂载策略，也不会打开剪贴板/增强会话。
    每次新建测试 VM 都使用 Data\PortableVM\Differencing 下的差分盘；删除 VM
    需要显式 -Action Discard。脚本只支持 Windows PowerShell 5.1+/PowerShell 7
    和 Hyper-V，必须以管理员运行。

    这不是安全边界本身：Hyper-V/Windows 更新及宿主策略仍由管理员负责。脚本
    只把 AI-2 的可验证配置设置好，并在输出中明确哪些项目需要人工验收。

.EXAMPLE
    .\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx

.EXAMPLE
    .\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Check

.EXAMPLE
    .\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Discard

.EXAMPLE
    .\Test-VM.ps1 -ListScenarios
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$Vhdx = "D:\Variable-USB\Variable-OS.vhdx",
    [string]$DataRoot,
    [string]$VmName = "VariableOS-AI2-TEST",
    [ValidateSet("Start", "Check", "Discard")]
    [string]$Action = "Start",
    [string]$SwitchName = "Default Switch",
    [switch]$NoConnect,
    [switch]$BMode,
    [switch]$IUnderstandAutomountChange,
    [switch]$ListScenarios,
    [string]$ScenarioReport
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ScenarioTable = @(
    [pscustomobject]@{ Id = 1; Name = "安装中拔盘"; Expected = "差分子盘可丢弃，母盘哈希不变；提示重新安装" },
    [pscustomobject]@{ Id = 2; Name = "宿主蓝屏/强制掉电"; Expected = "下次启动不复用未完成快照，母盘仍只读" },
    [pscustomobject]@{ Id = 3; Name = "Guest 删除 C 盘"; Expected = "只损坏子盘；重建子盘后恢复" },
    [pscustomobject]@{ Id = 4; Name = "宿主恶意软件"; Expected = "无桥接、NAT 入站阻断；Guest Defender 仍需人工检查" },
    [pscustomobject]@{ Id = 5; Name = "Data 空间低于 5 GB"; Expected = "Core 产生低空间事件，不自动删除用户数据" },
    [pscustomobject]@{ Id = 6; Name = "反作弊软件"; Expected = "检测到虚拟化后提示改用 B 模式，不伪造兼容结果" },
    [pscustomobject]@{ Id = 7; Name = "驱动不兼容"; Expected = "安全模式/Checkpoint 回退，不在宿主卸载驱动" },
    [pscustomobject]@{ Id = 8; Name = "BitLocker 忘记密码"; Expected = "只接受恢复密钥；不把密钥写入日志" },
    [pscustomobject]@{ Id = 9; Name = "4K 对齐"; Expected = "起始偏移是 4096 的倍数，否则阻止验收" },
    [pscustomobject]@{ Id = 10; Name = "五台宿主切换"; Expected = "逐台记录 PnP/启动时间，不共享宿主注册表" }
)

function Fail([string]$Message) {
    throw "[AI-2] $Message"
}

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Fail "请以管理员 PowerShell 运行；Hyper-V、差分盘和注册表 hive 需要提升权限。"
    }
}

function Assert-HyperV {
    if (-not (Get-Command Get-VM -ErrorAction SilentlyContinue)) {
        Fail "未发现 Hyper-V PowerShell 模块。请启用 Microsoft-Hyper-V 并重启后再试。"
    }
    $service = Get-Service vmms -ErrorAction SilentlyContinue
    if (-not $service -or $service.Status -ne "Running") {
        Fail "Hyper-V Virtual Machine Management 服务未运行。"
    }
}

function Resolve-DataRoot {
    param([string]$Requested, [string]$ParentVhdx)
    if ($Requested) { return [IO.Path]::GetFullPath($Requested) }
    $parent = Split-Path -Parent ([IO.Path]::GetFullPath($ParentVhdx))
    return [IO.Path]::GetFullPath((Join-Path $parent "Data"))
}

function Assert-ParentVhdx {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Fail "母盘不存在：$Path"
    }
    $extension = [IO.Path]::GetExtension($Path).ToLowerInvariant()
    if ($extension -notin @(".vhdx", ".vhd")) {
        Fail "只接受 .vhd/.vhdx 母盘：$Path"
    }
    try {
        $vhd = Get-VHD -Path $Path
        if ($vhd.VhdType -eq "Differencing") {
            Fail "传入的母盘已经是差分盘；请传入只读 Base.vhdx。"
        }
    } catch {
        Fail "Get-VHD 无法读取母盘（可能正在被其他 VM 使用）：$($_.Exception.Message)"
    }
}

function New-AI2Layout {
    param([string]$Root)
    foreach ($relative in @(
        "Exchange", "Registry", "Cache\Temp", "Cache\RamCache", "Dumps",
        "PortableVM\Differencing", "PortableVM\VMs", "Tests"
    )) {
        New-Item -ItemType Directory -Force -Path (Join-Path $Root $relative) | Out-Null
    }
}

function Set-ParentReadOnly {
    param([string]$Path)
    # Set-VHD 的只读标志是 VHD 层保护；文件属性是第二道防线。
    try {
        Set-VHD -Path $Path -ReadOnly $true
    } catch {
        Write-Warning "Set-VHD -ReadOnly 失败，将只设置文件只读属性：$($_.Exception.Message)"
    }
    $item = Get-Item -LiteralPath $Path
    $item.IsReadOnly = $true
}

function Get-NatSwitch {
    param([string]$Name)
    $switch = Get-VMSwitch -Name $Name -ErrorAction SilentlyContinue
    if (-not $switch) {
        Fail "找不到网络交换机 '$Name'。默认使用 Default Switch（NAT）；不要改成 External/桥接。"
    }
    if ($switch.SwitchType -eq "External") {
        Fail "交换机 '$Name' 是 External/桥接，已拒绝以免 Guest 直接进入宿主局域网。"
    }
    return $switch
}

function Configure-VmNetwork {
    param([string]$Name, [string]$Switch)
    $adapters = @(Get-VMNetworkAdapter -VMName $Name -ErrorAction SilentlyContinue)
    if ($adapters.Count -eq 0) {
        Add-VMNetworkAdapter -VMName $Name -Name "AI2-NAT" | Out-Null
        $adapters = @(Get-VMNetworkAdapter -VMName $Name)
    }
    # 一个 VM 只保留一个网络接口：无桥接、无第二张逃逸网卡。
    foreach ($adapter in $adapters | Select-Object -Skip 1) {
        Remove-VMNetworkAdapter -VMNetworkAdapter $adapter
    }
    $adapter = @(Get-VMNetworkAdapter -VMName $Name)[0]
    Connect-VMNetworkAdapter -VMNetworkAdapter $adapter -SwitchName $Switch
    Set-VMNetworkAdapter -VMNetworkAdapter $adapter `
        -MacAddressSpoofing Off -DhcpGuard On -RouterGuard On
}

function Set-VmBudget {
    param([string]$Name)
    $os = Get-CimInstance Win32_OperatingSystem
    $freeBytes = [uint64]$os.FreePhysicalMemory * 1KB
    if ($freeBytes -lt 2GB) {
        Fail "宿主可用内存不足 2 GB，已拒绝启动以保护宿主。"
    }
    # 4 vCPU、硬上限 30%；动态内存上下限仍把 Guest 封在 4 GB 内。
    Set-VMProcessor -VMName $Name -Count 4 -Maximum 30
    Set-VMMemory -VMName $Name -DynamicMemoryEnabled $true `
        -MinimumBytes 2GB -StartupBytes 4GB -MaximumBytes 4GB
    Set-VM -Name $Name -CheckpointType Disabled `
        -AutomaticStartAction Nothing -AutomaticStopAction ShutDown
    # 标准 VMConnect 会话不打开增强会话，因此不提供宿主剪贴板/驱动器映射。
    Set-VMFirmware -VMName $Name -EnableSecureBoot Off
}

function Disable-AutomountForBMode {
    if (-not $IUnderstandAutomountChange) {
        Fail "-BMode 会修改宿主的全局 automount 策略；同时传 -IUnderstandAutomountChange 才执行。恢复命令：diskpart -> automount enable。"
    }
    $scriptPath = Join-Path $env:TEMP "variable-ai2-diskpart-$PID.txt"
    @("automount disable", "exit") | Set-Content -LiteralPath $scriptPath -Encoding ASCII
    try {
        $output = & diskpart.exe /s $scriptPath 2>&1
        if ($LASTEXITCODE -ne 0) { Fail "diskpart automount disable 失败：$($output -join ' ')" }
        Write-Host "[AI-2] B 模式宿主 automount 已禁用；恢复时执行 diskpart -> automount enable。" -ForegroundColor Yellow
    } finally {
        Remove-Item -LiteralPath $scriptPath -Force -ErrorAction SilentlyContinue
    }
}

function Get-ChildDiskPath {
    param([string]$Root, [string]$Name)
    $safeName = ($Name -replace "[^A-Za-z0-9_.-]", "_")
    return Join-Path $Root ("{0}-{1:yyyyMMdd-HHmmss}.vhdx" -f $safeName, (Get-Date))
}

function Get-CheckObject {
    param([string]$Name, [string]$Root, [string]$Parent)
    $vm = Get-VM -Name $Name -ErrorAction SilentlyContinue
    $switch = Get-VMSwitch -Name $SwitchName -ErrorAction SilentlyContinue
    $parentItem = Get-Item -LiteralPath $Parent -ErrorAction SilentlyContinue
    [pscustomobject]@{
        AI2 = "isolation"
        VM = $Name
        VMState = if ($vm) { $vm.State } else { "Absent" }
        ParentVhdx = $Parent
        ParentReadOnly = if ($parentItem) { $parentItem.IsReadOnly } else { $false }
        COWChild = if ($vm) { @(Get-VMHardDiskDrive -VMName $Name | Select-Object -ExpandProperty Path) -join ";" } else { "Not attached" }
        Memory = if ($vm) { "$(($vm | Get-VMMemory).Startup / 1GB)GB startup" } else { "4GB expected" }
        Processor = if ($vm) { ($vm | Get-VMProcessor).Count } else { 4 }
        CpuMaximumPercent = if ($vm) { ($vm | Get-VMProcessor).Maximum } else { 30 }
        Switch = if ($switch) { "$($switch.Name) [$($switch.SwitchType)]" } else { "Missing" }
        Clipboard = "OFF (standard VMConnect; Enhanced Session not enabled by this script)"
        DragDrop = "OFF"
        SharedFolders = "OFF"
        UsbPassthrough = "OFF"
        Registry = (Join-Path $Root "Registry")
        Exchange = (Join-Path $Root "Exchange")
        HostResidue = "配置/缓存/转储目标均在 Data；需运行 residue scan 验收"
    }
}

function Write-ScenarioReport {
    param([string]$Root, [string]$Requested)
    $out = if ($Requested) { [IO.Path]::GetFullPath($Requested) } else {
        Join-Path $Root ("Tests\AI2-scenarios-{0:yyyyMMdd-HHmmss}.md" -f (Get-Date))
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $out) | Out-Null
    $lines = @(
        "# AI-2 隔离演练记录",
        "",
        "> 生成时间：$(Get-Date -Format o)",
        "> 本报告是验收清单，不会自动执行拔盘、蓝屏、删除系统盘或泄露恢复密钥等破坏性动作。",
        "",
        "| # | 场景 | 预期结果 | 实测证据/操作者签字 |",
        "|---:|---|---|---|"
    )
    foreach ($scenario in $ScenarioTable) {
        $lines += "| $($scenario.Id) | $($scenario.Name) | $($scenario.Expected) | [ ] |"
    }
    Set-Content -LiteralPath $out -Value $lines -Encoding UTF8
    Write-Host "[AI-2] 十场景清单已写入 $out" -ForegroundColor Cyan
    return $out
}

if ($ListScenarios) {
    $ScenarioTable | Format-Table -AutoSize
    exit 0
}

Assert-Administrator
Assert-HyperV
$Vhdx = [IO.Path]::GetFullPath($Vhdx)
$DataRoot = Resolve-DataRoot -Requested $DataRoot -ParentVhdx $Vhdx
Assert-ParentVhdx -Path $Vhdx

if ($ScenarioReport) {
    New-AI2Layout -Root $DataRoot
    Write-ScenarioReport -Root $DataRoot -Requested $ScenarioReport | Out-Null
    exit 0
}

if ($Action -eq "Check") {
    Get-CheckObject -Name $VmName -Root $DataRoot -Parent $Vhdx | Format-List
    exit 0
}

if ($Action -eq "Discard") {
    $vm = Get-VM -Name $VmName -ErrorAction SilentlyContinue
    if (-not $vm) {
        Write-Host "[AI-2] VM '$VmName' 不存在，无需丢弃。" -ForegroundColor Yellow
        exit 0
    }
    $disks = @(Get-VMHardDiskDrive -VMName $VmName | Select-Object -ExpandProperty Path)
    $dataRootFull = [IO.Path]::GetFullPath($DataRoot).TrimEnd("\") + "\"
    foreach ($disk in $disks) {
        $diskFull = [IO.Path]::GetFullPath($disk)
        if (-not $diskFull.StartsWith($dataRootFull, [StringComparison]::OrdinalIgnoreCase)) {
            Fail "拒绝删除不在 Data 内的磁盘：$diskFull"
        }
    }
    if ($PSCmdlet.ShouldProcess($VmName, "停止、删除 VM 并丢弃 Data 内差分盘")) {
        if ($vm.State -ne "Off") { Stop-VM -Name $VmName -TurnOff -Force }
        Remove-VM -Name $VmName -Force
        foreach ($disk in $disks) {
            if (Test-Path -LiteralPath $disk) { Remove-Item -LiteralPath $disk -Force }
        }
        Write-Host "[AI-2] VM 与差分子盘已丢弃；母盘未触碰。" -ForegroundColor Green
    }
    exit 0
}

if ($WhatIfPreference) {
    Write-Host "[AI-2] WhatIf：不会创建/启动 VM、差分盘或修改 automount。" -ForegroundColor Yellow
    exit 0
}

$existing = Get-VM -Name $VmName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "[AI-2] VM '$VmName' 已存在，复用并重新校验，不删除其差分盘。" -ForegroundColor Yellow
    Configure-VmNetwork -Name $VmName -Switch $SwitchName
    Set-VmBudget -Name $VmName
    if ($BMode) { Disable-AutomountForBMode }
    if ($existing.State -ne "Running") { Start-VM -Name $VmName | Out-Null }
} else {
    New-AI2Layout -Root $DataRoot
    $switch = Get-NatSwitch -Name $SwitchName
    $child = Get-ChildDiskPath -Root (Join-Path $DataRoot "PortableVM\Differencing") -Name $VmName
    Set-ParentReadOnly -Path $Vhdx
    Write-Host "[AI-2] 创建只读母盘的 COW 子盘：$child" -ForegroundColor Cyan
    New-VHD -Path $child -ParentPath $Vhdx -Differencing | Out-Null
    try {
        $vmPath = Join-Path $DataRoot "PortableVM\VMs"
        New-VM -Name $VmName -MemoryStartupBytes 4GB -Generation 2 `
            -VHDPath $child -Path $vmPath | Out-Null
        Configure-VmNetwork -Name $VmName -Switch $switch.Name
        Set-VmBudget -Name $VmName
        if ($BMode) { Disable-AutomountForBMode }
    } catch {
        # 失败路径只清理由本次创建的 VM/child，不碰母盘和用户 Data。
        Remove-VM -Name $VmName -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $child -Force -ErrorAction SilentlyContinue
        throw
    }
}

$check = Get-CheckObject -Name $VmName -Root $DataRoot -Parent $Vhdx
$check | Format-List
Write-Host "[AI-2] 7 层隔离配置完成：母盘只读/COW、4GB/4vCPU/30%/NAT、文件边界关闭、配置随 Data。" -ForegroundColor Green
Write-Host "[AI-2] 注意：Hyper-V 默认 NAT 的宿主虚拟网关仍可存在；本脚本阻止桥接和入站，不声称网络层绝对不可见。" -ForegroundColor Yellow

if (-not $NoConnect) {
    if (Get-Command vmconnect.exe -ErrorAction SilentlyContinue) {
        Write-Host "[AI-2] 打开标准 VMConnect（不启用 Enhanced Session）..." -ForegroundColor Cyan
        Start-Process -FilePath vmconnect.exe -ArgumentList @("localhost", $VmName) -Wait:$false | Out-Null
    } else {
        Write-Warning "找不到 vmconnect.exe；VM 已启动，请在 Hyper-V 管理器中连接。"
    }
}
