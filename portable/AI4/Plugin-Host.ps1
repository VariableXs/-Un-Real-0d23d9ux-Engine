param(
  [string]$PluginRoot = "D:\Data\Plugins",
  [string]$MarketFile = "$PSScriptRoot\Config\plugin-market.json",
  [string]$PermissionsFile = "D:\Data\Config\permissions.json",
  [string]$Filter = "*",
  [switch]$VerifyOnly,
  [switch]$AuthorizeAll,
  [switch]$Sandbox,       # 14.1 权限按 JobObject 限额: 插件不进宿主进程, 在受限子进程中运行
  [int]$SandboxMemMB = 512
)
# AI-4 拓展核 / 第8.3章 插件化 Variable Engine + 第14.1章 插件市场
#   热加载: LoadLibrary + GetProcAddress("VariablePluginInit")
#   安全:   SHA256 + Authenticode 签名 + 权限白名单 + (可选)JobObject 限额沙箱
# 用法:
#   .\Plugin-Host.ps1                     # 校验并热加载全部已授权插件
#   .\Plugin-Host.ps1 -VerifyOnly         # 只校验不加载
#   .\Plugin-Host.ps1 -Filter wallpaper   # 只加载 id/entry 含 wallpaper 的插件
#   .\Plugin-Host.ps1 -AuthorizeAll       # 首次授权全部网络插件(交互; AI5_NONINTERACTIVE=1 时自动拒绝)
#   .\Plugin-Host.ps1 -Sandbox            # 插件在 rundll32 子进程运行, 512MB 内存上限, 崩溃不传染宿主
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# 若插件 DLL 要求 C 导出变量插件初始化入口, 使用 P/Invoke 调用。
# Variable Engine Core 在实际产品中由 Rust 实现; 这里提供跨语言可复现的宿主验证。
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class NativePluginHost {
  [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern IntPtr LoadLibrary(string lpFileName);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool FreeLibrary(IntPtr hModule);
  [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
  public static extern IntPtr GetProcAddress(IntPtr hModule, string lpProcName);
  [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
  public delegate int VariablePluginInit(IntPtr hostCtx);
  public static VariablePluginInit GetInit(IntPtr h, string name = "VariablePluginInit") {
    IntPtr p = GetProcAddress(h, name);
    if (p == IntPtr.Zero) return null;
    return (VariablePluginInit)Marshal.GetDelegateForFunctionPointer(p, typeof(VariablePluginInit));
  }
}
"@

# JobObject 限额(14.1: 权限按 JobObject 限额) —— 沙箱子进程用, 与 AI-2 isolation.rs 同一套语义
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class JobLimitNative {
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool SetInformationJobObject(IntPtr hJob, int infoClass, ref JOBOBJECT_EXTENDED_LIMIT_INFORMATION lpInfo, int cbInfo);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool AssignProcessToJobObject(IntPtr hJob, IntPtr hProcess);
  [DllImport("kernel32.dll", SetLastError = true)]
  public static extern bool CloseHandle(IntPtr h);
  [StructLayout(LayoutKind.Sequential)]
  public struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
    public long PerProcessUserTimeLimit; public long PerJobUserTimeLimit; public uint LimitFlags;
    public UIntPtr MinimumWorkingSetSize; public UIntPtr MaximumWorkingSetSize;
    public uint ActiveProcessLimit; public UIntPtr Affinity; public uint PriorityClass; public uint SchedulingClass;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct IO_COUNTERS {
    public ulong ReadOperationCount; public ulong WriteOperationCount; public ulong OtherOperationCount;
    public ulong ReadTransferCount; public ulong WriteTransferCount; public ulong OtherTransferCount;
  }
  [StructLayout(LayoutKind.Sequential)]
  public struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
    public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation; public IO_COUNTERS IoInfo;
    public UIntPtr ProcessMemoryLimit; public UIntPtr JobMemoryLimit;
    public UIntPtr PeakProcessMemoryUsed; public UIntPtr PeakJobMemoryUsed;
  }
  public const int JobObjectExtendedLimitInformation = 9;
  public const uint JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x100;
  public const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000;
}
"@

function Read-JsonFile {
  param([string]$Path, [hashtable]$Defaults)
  if (Test-Path $Path) {
    return Get-Content $Path -Raw | ConvertFrom-Json
  }
  return $Defaults
}

function Write-JsonFile {
  param([string]$Path, $Object)
  New-Item -ItemType Directory -Force -Path (Split-Path $Path -Parent) | Out-Null
  $Object | ConvertTo-Json -Depth 8 | Set-Content -Path $Path -Encoding UTF8
}

function Get-Market {
  $market = Read-JsonFile $MarketFile @{ Plugins = @() }
  if (-not ($market.PSObject.Properties.Name -contains "Plugins")) { $market | Add-Member -NotePropertyName Plugins -NotePropertyValue @() -Force }
  if (-not $market.Plugins) { $market.Plugins = @() }
  return $market
}

function Get-PluginPermissions {
  $p = Read-JsonFile $PermissionsFile @{ AuthorizedPlugins = @{} }
  if (-not ($p.PSObject.Properties.Name -contains "AuthorizedPlugins")) { $p | Add-Member -NotePropertyName AuthorizedPlugins -NotePropertyValue @{} -Force }
  return $p
}

function Save-PluginPermissions {
  param($Permissions)
  Write-JsonFile $PermissionsFile $Permissions
}

function Resolve-PluginEntry {
  param($Plugin, [string]$Root)
  $entry = $Plugin.entry
  if (-not $entry) {
    $candidates = @("plugin.dll", "$($Plugin.id).dll", "$($Plugin.id).exe")
    $found = $candidates | ForEach-Object { Join-Path $Root $_ } | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $found) { throw "插件 $($Plugin.id) 缺少 entry 且候选 DLL 均不存在" }
    $entry = [IO.Path]::GetFileName($found)
  }
  return $entry
}

function Confirm-Plugin {
  param($Plugin, [string]$Root)
  $entry = Resolve-PluginEntry $Plugin $Root
  $dll = Join-Path $Root $entry
  if (-not (Test-Path $dll)) { throw "插件入口缺失: $dll" }

  # 1) SHA256 清单校验
  $hash = (Get-FileHash $dll -Algorithm SHA256).Hash
  $expected = $Plugin.sha256
  if ($expected) {
    if ($hash -ne $expected) {
      Write-Warning ">>> SHA256 不匹配: $entry  期望 $expected  实际 $hash"
      return $false
    }
  } else {
    Write-Warning ">>> $entry 无 sha256 清单, 仅 Authenticode 校验"
  }

  # 2) Authenticode 签名
  $sig = Get-AuthenticodeSignature $dll
  if ($sig.Status -ne "Valid") {
    Write-Warning ">>> Authenticode 签名无效: $($sig.Status) $($sig.StatusMessage)"
    return $false
  }
  $subject = $sig.SignerCertificate.Subject
  Write-Host ">>> 校验通过: $entry  SHA256=$($hash.Substring(0,12))...  签名=$subject" -ForegroundColor Green
  $pluginPermissions = if ($Plugin.PSObject.Properties.Name -contains "permissions") { @($Plugin.permissions) } else { @() }
  if ($pluginPermissions -contains "network") {
    Write-Host "    插件声明 network 权限, 需用户授权" -ForegroundColor Yellow
  }
  return $true
}

function Test-NetworkAuthorized {
  param($Plugin)
  $pluginPermissions = if ($Plugin.PSObject.Properties.Name -contains "permissions") { @($Plugin.permissions) } else { @() }
  if ($pluginPermissions -notcontains "network") { return $true }
  $perms = Get-PluginPermissions
  $auth = $perms.AuthorizedPlugins
  if ($auth.PSObject.Properties.Name -contains $Plugin.id) { return $true }
  if ($AuthorizeAll) {
    $authorized = $true
  } elseif ($env:AI5_NONINTERACTIVE -eq "1") {
    # 与 AI-5 联调/CI 约定: 非交互环境不得弹提问, 默认拒绝(安全侧)
    Write-Host "    [non-interactive] 网络插件 $($Plugin.id) 未授权, 按策略拒绝" -ForegroundColor Yellow
    return $false
  } else {
    $ans = Read-Host "插件 $($Plugin.id) ($($Plugin.entry)) 请求网络权限, 是否授权? [y/N]"
    $authorized = $ans -match "^[Yy]"
  }
  if ($authorized) {
    $auth | Add-Member -NotePropertyName $Plugin.id -NotePropertyValue @{ network = $true; ts = (Get-Date -Format o) } -Force
    $perms.AuthorizedPlugins = $auth
    Save-PluginPermissions $perms
    Write-Host "    已授权网络权限: $($Plugin.id)" -ForegroundColor Green
  }
  return $authorized
}

function Invoke-NativePluginEntry {
  param([string]$DllPath)
  $h = [NativePluginHost]::LoadLibrary($DllPath)
  if ($h -eq [IntPtr]::Zero) { return $false }
  try {
    $init = [NativePluginHost]::GetInit($h)
    if ($null -ne $init) {
      # hostCtx 使用指针0表示"宿主上下文由 Core 注入"; 此处仅验证入口可解析
      Write-Host "    LoadLibrary 成功, VariablePluginInit 可解析" -ForegroundColor Green
      return $true
    }
  } catch {
    Write-Warning "    LoadLibrary 后 GetProcAddress 异常: $_"
  } finally {
    [NativePluginHost]::FreeLibrary($h) | Out-Null
  }
  return $false
}

function Invoke-SandboxedPluginEntry {
  # 14.1 "权限按 JobObject 限额" 的落地: 插件不 LoadLibrary 进宿主进程,
  # 而是在 rundll32 子进程中解析入口; 子进程被限制内存上限, 句柄关闭即整树终止(崩溃不传染)。
  param([string]$DllPath)
  $rundll = Join-Path $env:SystemRoot "System32\rundll32.exe"
  if (-not (Test-Path $rundll)) { Write-Warning "    rundll32 不可用, 回退进程内校验"; return (Invoke-NativePluginEntry $DllPath) }
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $rundll
  $psi.Arguments = '"' + $DllPath + '",VariablePluginInit'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $proc = [System.Diagnostics.Process]::new()
  $proc.StartInfo = $psi
  $job = [IntPtr]::Zero
  try {
    if (-not $proc.Start()) { return $false }
    $job = [JobLimitNative]::CreateJobObject([IntPtr]::Zero, $null)
    $info = New-Object JobLimitNative+JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    $info.BasicLimitInformation.LimitFlags = [JobLimitNative]::JOB_OBJECT_LIMIT_PROCESS_MEMORY -bor [JobLimitNative]::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    $info.ProcessMemoryLimit = [UIntPtr]([UInt64]($SandboxMemMB * 1024 * 1024))
    $info.JobMemoryLimit = [UIntPtr]([UInt64]($SandboxMemMB * 1024 * 1024))
    [void][JobLimitNative]::SetInformationJobObject($job, [JobLimitNative]::JobObjectExtendedLimitInformation, [ref]$info, [System.Runtime.InteropServices.Marshal]::SizeOf($info))
    [void][JobLimitNative]::AssignProcessToJobObject($job, $proc.Handle)
    Write-Host "    沙箱: rundll32 pid=$($proc.Id) 内存上限 ${SandboxMemMB}MB (JobObject)" -ForegroundColor DarkGray
    $ok = $true
    if (-not $proc.WaitForExit(10000)) {
      Write-Warning "    沙箱超时(10s), 终止进程树"
      $proc.Kill()
      $ok = $false
    }
    [void]$proc.WaitForExit(3000)
    if ($ok) {
      Write-Host "    沙箱退出码: $($proc.ExitCode) (0=入口执行完成; 非法入口时 rundll32 返回非0)" -ForegroundColor DarkGray
    }
    return $ok
  } finally {
    if (-not $proc.HasExited) { try { $proc.Kill() } catch { } }
    if ($job -ne [IntPtr]::Zero) { [void][JobLimitNative]::CloseHandle($job) }
    $proc.Dispose()
  }
}

function Load-Plugin {
  param($Plugin, [string]$Root)
  try {
    if (-not (Confirm-Plugin $Plugin $Root)) { return }
    if ($VerifyOnly) {
      # -VerifyOnly: 只校验(SHA256+签名)不加载, 不触发网络授权提问
      Write-Host "    [verified] $($Plugin.id) v$($Plugin.version) (仅校验)" -ForegroundColor Green
      $Plugin | Add-Member -NotePropertyName "verified" -NotePropertyValue $true -Force
      return
    }
    if (-not (Test-NetworkAuthorized $Plugin)) {
      Write-Warning ">>> 跳过未授权网络插件: $($Plugin.id)"
      $Plugin | Add-Member -NotePropertyName "skipped" -NotePropertyValue "unauthorized-network" -Force
      return
    }
    $entry = Resolve-PluginEntry $Plugin $Root
    $dll = Join-Path $Root $entry
    $loaded = if ($Sandbox) { Invoke-SandboxedPluginEntry $dll } else { Invoke-NativePluginEntry $dll }
    if ($loaded) {
      Write-Host "    [loaded] $($Plugin.id) v$($Plugin.version)$(if($Sandbox){' (sandboxed)'})" -ForegroundColor Green
      $Plugin | Add-Member -NotePropertyName "loaded" -NotePropertyValue $true -Force
    } else {
      Write-Warning "    [not-a-dll] $($Plugin.id), 可能为 PowerShell/解释器插件, 仅登记无需 LoadLibrary"
      $Plugin | Add-Member -NotePropertyName "loaded" -NotePropertyValue $false -Force
    }
  } catch {
    Write-Warning "    加载失败 $($Plugin.id): $_"
    $Plugin | Add-Member -NotePropertyName "error" -NotePropertyValue $_.Exception.Message -Force
  }
}

# 主流程
$market = Get-Market
$loadedPlugins = @()
New-Item -ItemType Directory -Force -Path $PluginRoot | Out-Null
foreach ($plugin in $market.Plugins) {
  $entry = Resolve-PluginEntry $plugin $PluginRoot
  $idMatch = $plugin.id -like "$Filter*"
  $entryMatch = $entry -like "*$Filter*"
  if (-not ($idMatch -or $entryMatch)) { continue }
  $plugin | Add-Member -NotePropertyName "resolvedEntry" -NotePropertyValue $entry -Force
  Load-Plugin $plugin $PluginRoot
  $loadedPlugins += $plugin
}
Write-Host ""
Write-Host "===== 插件热加载结果: $($loadedPlugins.Count) 匹配 / $($market.Plugins.Count) 清单 =====" -ForegroundColor Cyan
$loadedPlugins | Where-Object { ($_.PSObject.Properties.Name -contains "loaded") -or ($_.PSObject.Properties.Name -contains "verified") } |
  Select-Object id, version, resolvedEntry, permissions, loaded, verified, skipped, error | Format-Table -AutoSize
