# AI-2 隔离核 — 7 层隔离 + 永不卡死 6 件套 + 看门狗

> 对应 `PORTABLE_VIRTUAL_SYSTEM_PLAN.md` 第 4、5 章与扩充 15/21/25。  
> 代码：`portable/AI2/Test-VM.ps1`、`src-tauri/src/shell/isolation.rs`。  
> 日期：2026-09-07。实现状态：可验收（Windows 管理员 + Hyper-V 实机；Linux CI 跑 Rust 替身测试）。

## 1. 目标与非承诺

目标：任意软件崩溃不传染宿主；10GB 级软件 6 秒热路径可用、可取消；1TB 盘任意电脑隔离档可启动。

明确不承诺：

- Hyper-V NAT 不是空气隔离。宿主虚拟网关仍可能存在；脚本只拒绝 External/桥接并关闭入站路径。
- Job Object 不是虚拟机。Low Integrity 在权限不足时 **失败关闭**，不会假装已降权。
- 十场景报告不会自动拔盘、蓝屏或泄露 BitLocker 恢复密钥。

## 2. 第 4 章 — 7 层隔离

契约对象：`IsolationPolicy`。`validate()` 在启动前 fail-closed。

### 2.1 层1 硬盘

- 母盘只读（`Set-VHD -ReadOnly` + 文件只读属性）。
- 每次新建测试 VM 在 `Data/PortableVM/Differencing` 建差分 COW 子盘。
- 丢弃必须 `-Action Discard`，且只删除 Data 内路径。
- B 模式 `automount disable` 必须同时传 `-IUnderstandAutomountChange`。

### 2.2 层2 内存/CPU

- Guest：动态内存 2–4 GB，启动 4 GB；4 vCPU，Maximum 30%。
- 宿主可用内存 <2 GB 拒绝启动。
- 进程：Job 内存硬限 4 GB；CPU rate hard cap = `cpu_percent * 100`（百分之一百分之一单位）。

### 2.3 层3 进程

- 一软件一 Job；`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`。
- `Integrity Level = Low`（SID `S-1-16-4096`）；失败则取消 Job。
- IPC 走 800 ms 熔断，不卡 UI。

### 2.4 层4 文件摆渡

默认剪贴板/拖放/共享/USB 全关。唯一通道 `Data/Exchange`：`controlled_exchange_path` 拒绝 `..` 与绝对路径。

### 2.5 层5 网络

仅 NAT；External 交换机直接 Fail。DhcpGuard/RouterGuard On，Mac spoof Off。`host_visible` 必须为 false。

### 2.6 层6 注册表

`Data/Registry/*.dat` 经 `RegLoadKeyW` 挂 HKU，Drop 时 `RegUnLoadKeyW`。权限不足不回退写宿主 HKCU。

### 2.7 层7 痕迹

TEMP/APPDATA/USERPROFILE 重定向到 Data。配置在 `Data/PortableVM`。退出扫描由验收清单覆盖，不写 `C:\Users`。

## 3. 第 5 章 — 6 件套

### 3.1 假启动壳

双击后立即弹出独立壳（进度/取消）。真进程在 Job 中启动。取消 = `LimitedChild::cancel()` → `TerminateJobObject`。

PowerShell 侧：`portable/AI2/Fake-Start.ps1` 模拟 100 ms 进度与 30 s 熔断。

### 3.2 按需分页

`DemandPagedFile`：Windows `CreateFileMappingW` + `MapViewOfFile` 每次 64 KiB；非 Windows seek-read 替身。UI 不一次读 10 GB。

### 3.3 限额与优先级

Very Low IO（`PROCESS_MODE_BACKGROUND_BEGIN`，尽力而为）；亲和前 4 核；Job CPU 硬帽始终生效。

### 3.4 读写分离

实体在 Data，C 盘仅链接（AI-1 职责）。AI-2 保证 Exchange/Cache 不越界。

### 3.5 RAM LRU 256 MiB

`ChunkCache` 按 chunk 淘汰。命中率由 `CacheStats` 暴露。

### 3.6 可取消与 30 s 熔断

`StartupFuse`：过期或用户取消后由加载器调用 `cancel()`。超 30 s 提示安全模式重试，不杀宿主。

实测目标（人工验收，非本仓库伪造）：Blender 冷 18 s / 热 5.2 s，主线程冻结 0 ms。

## 4. 扩充 15 — 4 级崩溃

| 级 | 对象 | 行为 |
|---|---|---|
| L1 | Application | 关该 Job，Banner「已恢复」 |
| L2 | Worker | 3 s 重启 |
| L3 | Shell | 5 s 重启 |
| L4 | System | 不自动狂重启；事件 `SystemRollbackRequired`，丢弃差分盘 |

连续 3 次 IPC 超时（800 ms）→ 熔断 60 s。`libcef 0x80000003` 仅 Banner，不弹系统错误框（VEH 在 Windows 构建接入点预留）。

## 5. 扩充 21 — 10 场景

`Test-VM.ps1 -ListScenarios` / `-ScenarioReport` 只生成清单，不执行破坏动作。

1. 安装中拔盘 → 子盘可丢，母盘哈希不变  
2. 宿主蓝屏 → 不复用未完成快照  
3. Guest `del C:\` → 只伤子盘  
4. 宿主恶意软件 → 无桥接；NAT 入站阻断  
5. Data <5 GB → 事件，不自动删用户数据  
6. 反作弊 → 提示 B 模式，不伪造兼容  
7. 驱动不兼容 → 安全模式回退，不在宿主卸驱动  
8. BitLocker 忘密 → 只接受恢复密钥，不写日志  
9. 4K 对齐 → 偏移须为 4096 倍数  
10. 五台宿主 → 不共享宿主注册表  

脚本：`portable/AI2/Chaos-Scenarios.ps1`。

## 6. 扩充 25 — Rust API

- `call_with熔断` / `timed_call`：超时返回 `None`  
- `spawn_limited` / `LimitedChild::cancel`  
- `Watchdog` 3 s 节拍，`check_once` 可测  
- `CircuitBreaker` 3 次失败打开  

非 Windows 提供同一 API 的测试替身，不假装 Job Object 存在。

## 7. 验收

- [x] `IsolationPolicy::validate` fail-closed 单测  
- [x] Exchange 路径穿越拒绝  
- [x] 64 KiB 分页只读请求范围  
- [x] 三次超时打开熔断  
- [x] Test-VM 默认不删已有 VM、不改 automount  
- [ ] 实机 `del C:\` 宿主无影响（需 Hyper-V）  
- [ ] Blender 冷/热路径人工计时  

## 8. 边界

本核不修改 AI-1 存储脚本、AI-3 兼容层、AI-4 拓展、AI-5 交付。网络隔离是策略约束，不是密码学证明。
