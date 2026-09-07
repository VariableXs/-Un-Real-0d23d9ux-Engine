# AI-2 隔离防崩核

> 对应《PORTABLE_VIRTUAL_SYSTEM_PLAN.md》第 4、5、15、21、25 章。本文只描述 AI-2 的隔离与防崩实现；存储造盘、兼容层、拓展层和交付层仍由各自模块负责。

## 1. 实现边界与原则

AI-2 的目标不是“把一个进程包起来就宣称安全”，而是把**母盘、资源、进程、边界、网络、注册表和痕迹**拆成可以检查的七层。每一层有失败关闭行为：配置不满足时拒绝启动，不自动退回到宿主路径、不把失败静默成“隔离成功”。

A 模式由 Hyper-V 测试脚本创建一个标准会话；B 模式仍需要 UEFI、VHDX 引导和宿主的磁盘策略。`portable/AI2/Test-VM.ps1` 只改它创建的 VM 和 Data 目录，默认不会删除已有 VM、不会执行全局 `automount disable`，也不会启用 Enhanced Session 的剪贴板/驱动器映射。脚本在标准 NAT 下会阻断桥接和入站，但 Windows NAT 的宿主虚拟网关仍可能存在，因此文案不会虚构“网络绝对不可见”。

Rust 模块位于 `src-tauri/src/shell/isolation.rs`。它不拼接 shell 命令，而使用参数化的 `std::process::Command`、Windows Job Object 和受控文件路径。非 Windows 构建保留同一 API 供测试，但明确只是进程生命周期测试替身，不把 POSIX 进程伪装成 Hyper-V 隔离。

## 2. 七层隔离

### 2.1 硬盘层

`IsolationPolicy` 的 `DiskLayer` 强制记录 `parent_vhdx`、`differencing_vhdx`、`parent_read_only`、`copy_on_write` 和 B 模式的 `automount_disabled`。Test-VM 启动前调用 `Set-VHD -ReadOnly $true`，同时设置文件只读属性，再用 `New-VHD -ParentPath ... -Differencing` 生成 `Data/PortableVM/Differencing/` 下的子盘。VM 的写入点只有子盘。`-Action Discard` 只允许删除 Data 内的子盘，发现硬盘路径在 Data 外会立即拒绝。

B 模式的 automount 是宿主全局设置，脚本必须同时收到 `-BMode -IUnderstandAutomountChange` 才会执行，恢复命令为 `diskpart -> automount enable`。这样不会因为一次普通 A 模式测试而让宿主所有新插入磁盘都失去自动挂载。

### 2.2 内存与 CPU 层

VM 级预算为启动 4 GB、动态最小 2 GB、最大 4 GB、4 个 vCPU、CPU 最大 30%，宿主保留 2 GB。进程级默认 `IsolationLimits` 为 4 GiB 内存、30% Job CPU hard cap、4 核亲和性、Very Low 后台 I/O，并在 Windows 上请求 Low Integrity。Job Object 使用：

- `JOB_OBJECT_LIMIT_PROCESS_MEMORY` 和 `JOB_OBJECT_LIMIT_JOB_MEMORY`；
- `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`；
- `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`；
- `JOBOBJECT_CPU_RATE_CONTROL_INFORMATION` 的 hard cap。

所以 `LimitedChild` 被丢弃时，Windows 会关闭 Job handle 并清理整个子进程树；点取消时则调用 `TerminateJobObject`，不会用模糊的进程名杀宿主同名软件。Low Integrity 设置失败会终止刚启动的 Job 并返回错误，而不是继续以普通完整性运行。

### 2.3 进程层

每一个受管软件获得一个 Job。`spawn_limited` 先创建并配置 Job，再 spawn，成功后立刻分配进程、应用 4 核亲和性和后台优先级。`Watchdog` 不通过进程名猜测状态，而持有这个 Job 的 `LimitedChild`。进程树继承 Job 限额，应用崩溃不会带走 Shell。

### 2.4 文件摆渡层

剪贴板、拖放、共享文件夹和 USB 直通默认均为 false。需要跨边界时只能使用 `Data/Exchange`。Rust 的 `controlled_exchange_path` 拒绝绝对路径、空名和 `..`，再检查规范化的 Data 前缀；PowerShell 也只把差分盘和 VM 配置写到 Data。Exchange 的杀毒和用户确认属于上层安全/交付模块，AI-2 不会把“复制成功”冒充“已经扫描”。

### 2.5 网络层

默认只接受 NAT、入站阻断、非桥接。Test-VM 拒绝 External switch，并把 VM 限制为一张网卡，开启 DHCP Guard、Router Guard、关闭 MAC spoofing。桥接模式没有隐式降级：`IsolationPolicy::validate` 直接失败。域名白名单、代理和 kill-switch 由现有网络层负责；AI-2 只保证进程/VM 不因错误的网络模式启动。

### 2.6 注册表层

每个会话的 hive 文件必须在 `Data/Registry` 且文件名以 `.dat` 结尾。Windows 版本通过 `RegLoadKeyW(HKU, ...)` 加载 `VariablePortable_<name>`，`LoadedRegistryHive` 的 Drop 实现 `RegUnLoadKeyW`。加载失败时绝不回退到宿主 HKCU。管理员权限不足是明确错误，需要上层显示“无法加载随盘注册表”，而不是悄悄写宿主注册表。

### 2.7 痕迹层

`trace_free_environment` 将 `HOME`、`USERPROFILE`、`APPDATA`、`LOCALAPPDATA`、`TEMP`、`TMP` 和 `PROGRAMDATA` 指到 Data/User、Data/Cache/Temp、Data/ProgramData。`ensure_data_layout` 只创建 Exchange、Registry、Cache、Dumps、PortableVM 和 Tests。这个保障只对经执行档启动的子进程成立；没有走执行档的 `.lnk`、ShellExecute 或第三方自启动项不能被 Rust 代码夸大为零残留，需用现有残留扫描器验收。

## 3. “永不卡死”六件套的落地

计划标题写作六件套，但 5.1–5.7 实际包含七个动作；实现全部保留：

1. **假启动壳**：VMConnect 和上层 Shell 先显示可取消的加载状态，真实 Worker 由 Job 启动。AI-2 不把慢的 `spawn` 放在前端渲染线程。
2. **按需分页**：`DemandPagedFile` 在 Windows 使用 `CreateFileMappingW` / `MapViewOfFile`，每次只映射 64 KiB；非 Windows 用等价的 seek-read 替身。调用 `read_range` 不会把 10 GB 文件整体读入内存。
3. **资源限额**：Job 内存 4 GB、CPU hard cap 30%、4 核亲和；Very Low 是后台优先级提示，硬限制仍以 Job Object 为准。
4. **读写分离**：实体文件和启动缓存放 Data，COW 子盘只承载会话写入；符号链接由存储核/Guest 内配置负责，AI-2 不修改 AI-1 文件。
5. **256 MiB LRU**：DemandPagedFile 的 ChunkCache 以 64 KiB 为单位统计命中/未命中并回收最旧块。缓存上限只是缓存上限，不会挤占宿主 2 GB 保留内存。
6. **可取消**：`LimitedChild::cancel` 只终止所属 Job；VM 测试的 `-Action Discard` 只删除 Data 中的子盘。写操作失败时保留源文件和日志。
7. **800 ms 熔断/30 s 启动熔断**：`timed_call` 和中文兼容 API `call_with熔断` 在超时后返回 `None`；`StartupFuse` 默认给大软件 30 秒总启动预算，加载器在分页边界检查过期并调用 `LimitedChild::cancel()`；`CircuitBreaker` 连续 3 次失败打开 60 秒。看门狗每 3 秒检查一次，Worker 的恢复有 3 秒退避，Shell 有 5 秒退避，System 层只发“回滚差分盘”事件，不自动破坏用户数据。

超时函数会在线程中执行阻塞调用，所以只适合可安全放弃结果的只读/IPC 工作；可写工作应使用本身支持取消的 API。这是为了避免“800 ms 后 UI 返回、后台仍在写文件”的伪熔断。

## 4. 崩溃四级与十场景

- **L1 应用崩**：关闭该 App Job，发 `Exited`，桌面继续工作。
- **L2 Worker 崩**：3 秒看门狗尝试在新 Job 中重启；60 秒内重启失败三次进入熔断。
- **L3 Shell 崩**：5 秒退避重启 Shell，失败路径保留占位卡，不杀其他软件。
- **L4 系统崩**：标记 `SystemRollbackRequired`，由上层丢弃差分盘/恢复 Checkpoint；Rust 看门狗不直接删除母盘。

`Test-VM.ps1 -ListScenarios` 打印扩充 21 的十个场景，`-ScenarioReport Data/Tests/ai2.md` 生成不执行破坏动作的验收表：拔盘、宿主掉电、Guest 删除 C、宿主恶意软件、空间低、反作弊、驱动回退、BitLocker 恢复密钥、4K 对齐和五台宿主切换。必须由实机测试人员把日志/录像/哈希填回表格；脚本不会为了“全过”自动勾选。

## 5. 验收命令

管理员 PowerShell：

```powershell
# 只检查，不启动、不删除
.\portable\AI2\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Check

# 创建 AI-2 专用差分 VM；不打开增强会话
.\portable\AI2\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx

# 生成十场景空白记录
.\portable\AI2\Test-VM.ps1 -ListScenarios
.\portable\AI2\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx `
    -ScenarioReport D:\Variable-USB\Data\Tests\ai2.md

# 显式丢弃本次 VM 的 Data 差分盘；母盘不动
.\portable\AI2\Test-VM.ps1 -Vhdx D:\Variable-USB\Variable-OS.vhdx -Action Discard
```

验收不能只看 VM 窗口：要检查 `Get-VHD` 的 ParentPath、母盘只读属性、`Get-VMProcessor` 的 Count/Maximum、`Get-VMMemory` 的上限、网卡不是 External、Data 目录是否出现 Registry/Exchange/Dumps，以及进程取消后宿主同名进程是否仍在。`del C:\`、libcef 断点和拔盘测试只在可丢弃的子盘/专用测试宿主执行，绝不在生产母盘上演练。

## 6. 已知边界

Hyper-V 的 NAT 不是物理空气隔离；Windows 内核漏洞、宿主管理员恶意操作、Guest 主动把数据导出到 Exchange 都不在本模块承诺内。大软件“热启动 6 秒”还依赖固态盘、Guest 缓存和软件自身初始化，AI-2 提供 64 KiB 分页、缓存和可取消，不伪造基准数字。Low Integrity、Job CPU 和 Hyper-V 参数必须在 Windows 实机验证；Linux CI 只验证路径护栏、缓存、熔断和非 Windows 进程生命周期。
