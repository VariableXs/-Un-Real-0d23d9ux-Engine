# AI-2 隔离核

对应主计划第 4、5、15、21、25 章。

## 文件

- `Test-VM.ps1`：Hyper-V 隔离验证器。默认创建只读母盘的 COW 差分子盘，4 GB 动态内存上限、4 vCPU、30% CPU、NAT、单网卡，并关闭增强会话/剪贴板/共享文件夹/USB 直通。已有 VM 不会自动删除；丢弃必须显式使用 `-Action Discard`。
- `src-tauri/src/shell/isolation.rs`：Rust 进程隔离底座，包含 Job Object 限额、Low Integrity、可取消 `LimitedChild`、800 ms 熔断、3 秒看门狗、64 KiB `CreateFileMappingW` 按需读取、256 MiB LRU、随盘环境/注册表护栏。
- `docs/AI2-隔离防崩.md`：7 层隔离、永不卡死组件、四级故障恢复和十场景验收说明。

## 快速验证

管理员 PowerShell：

```powershell
# 查看配置，不启动、不删除
.\portable\AI2\Test-VM.ps1 `
  -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Check

# 创建并启动隔离 VM
.\portable\AI2\Test-VM.ps1 `
  -Vhdx D:\Variable-USB\Variable-OS.vhdx

# 生成扩充 21 的十场景验收表，不执行破坏性动作
.\portable\AI2\Test-VM.ps1 -ListScenarios
.\portable\AI2\Test-VM.ps1 `
  -Vhdx D:\Variable-USB\Variable-OS.vhdx `
  -ScenarioReport D:\Variable-USB\Data\Tests\ai2.md

# 明确丢弃测试 VM 的 Data 差分盘；只读母盘保留
.\portable\AI2\Test-VM.ps1 `
  -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Discard
```

B 模式的全局 automount 策略不会被普通启动修改；只有明确理解后才可使用：

```powershell
.\portable\AI2\Test-VM.ps1 `
  -Vhdx D:\Variable-USB\Variable-OS.vhdx `
  -BMode -IUnderstandAutomountChange
```

恢复宿主策略：`diskpart` → `automount enable`。

## 边界

Hyper-V NAT 不是物理空气隔离，宿主虚拟网关仍可能存在；脚本阻止 External/桥接并关闭入站路径，不虚构“宿主绝对不可见”。Rust 在非 Windows 上只提供测试替身，真实 Job Object、Low Integrity、VHDX 和 `RegLoadKey` 必须在 Windows 管理员环境验证。
