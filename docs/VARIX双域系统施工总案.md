# VARIX + Variable 双域系统施工总案

> 本文件是整场架构讨论的定稿施工文档：目标形态、八大阶段的详细逻辑（数据结构/算法/事件流/决策规则）、每阶段的开放改进点与验收门禁。
> 配套的任务勾选清单见 `docs/VARIX双域系统总施工清单.md`；本文负责"逻辑怎么转"，清单负责"进度怎么勾"。

---

## 0. 大致步骤总览（先看这张图）

```
阶段0 引导选择页          ← 内核画的第一个界面，纯内核功能，无前置依赖
   ↓
阶段1 U盘五分区便携基建    ← 复用 portable/ 现有脚本体系
   ↓
阶段2 内核欠账清零        ← 缺页接线/用户态/真驱动/真盘FS（能跑的前提）
   ↓
阶段3 Variable界面上内核  ← Servo 承载现有 React 前端 + Tauri IPC 垫片
   ↓
阶段4 数据总线与白名单    ← 共享内存/消息通道/共享分区，默认拒绝
   ↓                    ↘
阶段5 Wine兼容层通道       阶段6 隐形Windows引擎通道   ← 二者可并行施工
   ↘                    ↙
    阶段7 资源统管与多机打磨（设置页/配额/拔出保护/多机型验收）
```

- **并行性**：阶段5 与阶段6 互不依赖，可两条线并行；阶段0/1 可在阶段2 之前先行（不碰内核）。
- **可停性**：任一阶段做完系统都处于"可用的中间态"，随时可停不尴尬。
- **门禁**：每阶段过三线全绿（V: tsc+vitest / C: ca-core / K: ktest+kcheck）+ 实机验收报告归档 docs/。

### 终局形态一句话

一块 1TB U 盘插任意电脑：开机默认 5 秒进 VARIX+Variable 桌面；日常软件走 Wine 兼容层（Windows 完全没开机）；高兼容软件走隐形 Windows 引擎（实时无缝窗口）；游戏/反作弊重启进纯 Windows（同一块盘上的镜像）；拔盘即走、零残留。

---

## 阶段 0：引导选择页

### 目标
内核引导早期画出系统选择界面；这是 VARIX 画的第一个 UI。

### 详细逻辑
- **时机**：Limine 移交后、`boot()` 域初始化完成前，在 GOP 帧缓冲上直接绘制（不依赖任何驱动子系统，只依赖 framebuffer）。
- **数据结构**：引导配置文件 `boot-select.json` 放共享分区（两系统都能读写）：
  - `default_entry`: "variable" | "windows" | "last"
  - `timeout_sec`: 默认 5
  - `show_menu`: bool（false=静默走默认项）
  - `last_boot`: 上次实际进入的系统，每次进入时写回
- **交互逻辑**：
  1. 画三行选项 + 倒计时进度条；
  2. 键盘（此阶段只需 PS/2 最小轮询读键，不需要完整驱动栈）上下选择、Enter 确认；
  3. 倒计时归零 → 执行 default_entry；
  4. 选 "variable" → 继续正常域初始化；
  5. 选 "windows" → 通过 UEFI `BootNext` 变量指向 U 盘上 Windows 引导项，`ResetSystem()` 重启。
- **BootNext 写入逻辑**：引导期仍在 UEFI Boot Services 存活窗口内，直接调 `SetVariable(L"BootNext", ...)` 指向 Windows 引导项号；该项号由部署脚本预先探测写入配置文件。
- **倒计时可打断**：任何按键立即暂停倒计时，进入手动选择态。

### 开放改进点
- 选择页视觉（可做成 Variable 风格的动画选择页，后期升级）
- 键盘去抖与 USB 键盘在引导期的支持策略
- 多语言（中/英）显示

### 验收门禁
- 真机 U 盘引导：5 秒无操作自动走默认项；选 Windows 后真机重启进入 U 盘 VHDX 里的 Windows；
- 配置文件改动（改默认项/秒数）下次引导生效。

---

## 阶段 1：U 盘五分区便携基建

### 目标
一键脚本从零产出可引导 U 盘；复用项目现有便携体系。

### 详细逻辑
- **分区布局**（1TB 盘定版）：
  1. `ESP`（EFI 系统分区，1GB）：VARIX 引导器 + Limine + Windows 引导文件（BCD 等，供纯 Windows 模式）
  2. `VARIX_SYS`（64GB，EXT 变体或自研只读映像）：内核 ELF + Variable 运行时 + Servo + Wine 前缀模板
  3. `WIN_ENGINE`（300GB，NTFS）：Windows 引擎 VHDX（差分链 Base→Apps→User，复用 `Create-VHDX.ps1 -Chain`）
  4. `SHARED`（600GB，exFAT）：共享数据分区——唯一互通面
  5. `SNAPSHOT`（余量，NTFS）：快照/回收/误删恢复
- **SHARED 目录规范**（两系统共同遵守的契约）：
  - `apps.json`：软件登记总表（名称/图标/通道归属/分级：wine|engine|native-only）
  - `handoff/`：意图接力队列（预留，阶段6 前的过渡机制）
  - `whitelist/`：白名单目录清单
  - `boot-select.json`：引导配置
- **脚本改造**：`Deploy-To-USB.ps1` 新增 `Deploy-Varix` 段；`Create-VHDX.ps1` 参数化 Windows 引擎盘容量。
- **拔出保护**：完全复用 `usb.rs` 既有逻辑（1s 轮询 + 每 2s `wal_checkpoint(PASSIVE)` + 卷消失广播），挂到新分区布局上。

### 开放改进点
- 分区容量比例按实际软件规模调参
- VARIX_SYS 用 squash 式只读压缩映像减小占用
- 多 U 盘版本管理（Base 镜像 + 用户差分）

### 验收门禁
- 从零执行一键脚本 → VM（`Test-VM.ps1` 挂 U 盘）可引导到选择页；
- 拔出保护对五分区任一被占用卷均正确拦截。

---

## 阶段 2：内核欠账清零

### 目标
内核从"引导完就停"变成"能跑程序、能摸硬件、能存数据"。

### 详细逻辑
- **2.1 缺页处理接线**（最高优先）：
  - 现状：`paging::decide()` 决策纯函数已完成（guard→Fault / reserved-write→Fault / 栈未触碰→GrowStack / present+write+cow→CopyOnWrite / !present→SwapIn|MapZero），但 `idt::isr_dispatch` 对 vector<32 一律 fatal。
  - 改造：IDT 0x0E（#PF）注册专用 handler → 从 CR2 取故障地址 + 错误码 → 组装请求 → 调 `decide()` → 执行四路动作；decide 返回 Fault 才走 fatal。
  - COW 语义：fork 前置条件——写时复制位 + 引用计数页框；GrowStack 栈上限保护。
- **2.2 用户态进程壳**：
  - ring3 切换：代码段/数据段选择子 + RPL=3；syscall/sysret 或 int 0x80 双入口（号表已就绪：`syscall/table.rs`，18 个调用需扩充：spawn/exit/wait/mmap/brk/open/read/write 等）；
  - 进程模型：复用 sched 的 64 槽线程 + 新增进程控制块（地址空间、句柄表、父子关系）；
  - 第一个用户进程：init（从 VARIX_SYS 读 ELF 并 spawn）。
- **2.3 真驱动**：
  - 显示：Limine GOP 帧缓冲（已有）→ 封装为内核显示服务（双缓冲）；
  - 键鼠：PS/2 轮询起步（引导选择页已用），xHCI 最小栈后补；
  - 存储：NVMe 优先（提交队列/完成队列最小实现），AHCI 兜底；块设备抽象层统一接口；
  - 文件系统：`fs23_journal` 换块设备后端（现在是内存态 64 槽模型）；VARIX_SYS 用只读简单 FS，SHARED 走 exFAT 读写实现（或先只读+写走快照区过渡）。
- **2.4 掉电安全保留**：journal 重放语义不变（torn 丢弃该点及之后全部 + 幂等重放），掉电注入 100 轮测试沿用。

### 开放改进点
- 调度器 6 调度类的权重参数（Isolated>RT>Interactive>Normal>Batch>Idle）针对桌面负载调优
- syscall 号表扩充节奏与 Linux 兼容号段的取舍
- xHCI 栈的完成度（PS/2 机器越来越少）

### 验收门禁
- ktest/kcheck 0 告警全绿；掉电注入 0 数据丢失；
- 真机：启动到可交互 shell；ELF 用户程序可 spawn/exit；断电重上电数据完好。

---

## 阶段 3：Variable 界面上内核

### 目标
U 盘引导默认进入 Variable 桌面；前端代码复用最大化。

### 详细逻辑
- **架构**：`内核系统服务 ← Tauri IPC 垫片 ← Servo(渲染 Variable React 前端) ← GOP 帧缓冲`
- **Servo 集成**：
  - 作为 VARIX 用户态进程运行，渲染输出走共享帧缓冲；
  - 输入事件由内核输入服务 → 消息通道 → Servo DOM 事件。
- **Tauri IPC 垫片**（关键复用件）：
  - Variable 前端通过 `lib/ipc.ts`（约 554 方法，仅 4 处裸 invoke）调后端——垫片实现同一 invoke 协议；
  - 每个 invoke 名映射到三种后端之一：内核原生服务（文件/窗口/输入）/ 共享分区服务（apps.json 等）/ 暂不可用（返回统一降级错误 + 前端如实提示）；
  - 垫片先做"命令面审计"：用 `tools/audit.cjs` 的 IPC 清单逐个标注三色（可用/可映射/暂缺），缺失清单驱动后续补齐顺序。
- **前端裁剪原则**：不重写 UI；能力缺失时如实降级提示（i18n zh/en 已有体系）；视觉与操作手感保持一致（行为等价约束沿用）。
- **状态迁移**：vwmStore 几何持久化从 localStorage 迁到内核存储服务（KV 接口）。
- **启动链**：选择页选 Variable → 域初始化 → spawn init → spawn Servo(Variable) → 桌面亮起。bootPhase（loading→exit→done）逻辑在垫片上以内核服务进度事件喂给 BootScreen，保持既有启动仪式体验。

### 开放改进点
- Servo 缺失网页特性的补齐清单（按 Variable 前端实际用量裁剪优先级）
- 垫片的命令面覆盖率目标（第一阶段建议只保桌面壳+文件管理器+设置页三件套）
- 渲染性能：软件渲染起步，后期 GPU 加速（阶段7）

### 验收门禁
- U 盘真机引导 → Variable 桌面亮起 → 开始菜单/任务栏/窗口管理可用；
- vitest 中与 UI 逻辑无关的测试套件保持全绿（证明前端逻辑未被破坏）。

---

## 阶段 4：数据总线与白名单

### 目标
数据共享完全受控：白名单内自由，白名单外默认拒绝且留审计。

### 详细逻辑
- **三个原语**（内核服务）：
  1. 共享内存块：创建/映射/授权（只对授权进程可见），用于大块数据；
  2. 消息通道：进程间事件通知（复用 sched 的 16 通道等待队列思路扩展）；
  3. 共享分区文件访问：一切 SHARED 分区读写经 VFS 白名单裁决。
- **白名单规则**：目录级 + 剪贴板 + 拖放三类；规则存 SHARED/whitelist/，用户可在 Variable 设置页增删；
- **审计**：所有越权尝试记审计日志（时间/进程/路径/动作），设置页可查；
- **Uxv 接入**：跨系统大文件交换用 Uxv 容器（BLAKE3 校验 + journal 掉电安全已现成）。

### 开放改进点
- 规则粒度（进程级 × 目录级矩阵）
- 审计日志的检索 UI
- 剪贴板格式过滤（防恶意格式注入）

### 验收门禁
- 白名单外访问 100% 被拒且有日志；白名单内两通道互通；
- 掉电时白名单配置与共享数据零丢失。

---

## 阶段 5：Wine 兼容层通道

### 目标
Windows 完全不运行的前提下，.exe 在 VARIX 用户态跑起来。

### 详细逻辑
- **为什么不重写**：Wine 30 年积累的 Win32 API 实现（几千个）是最大资产；移植对接面即可。
- **三层结构**：
  1. PE 装载器：解析 PE 头/节表/导入表 → 映射进进程地址空间 → 解析符号绑定 Wine 的 DLL 实现；
  2. Win32 服务台：Kernel32/User32/GDI 等核心 DLL 的 VARIX 移植版，底层调内核 syscall；
  3. 进程模型：每个 exe = VARIX 独立内核进程 + Job 级资源限额（内存/CPU rate，语义同 src-tauri 的 isolation.rs 设计）；
- **图形路径**：GDI → 软件渲染到帧缓冲窗口服务；DirectX → WineD3D→Vulkan（后期）→ 软件兜底；
- **分级登记**（写入 SHARED/apps.json）：
  - ✅ 可跑：办公/工具/多数桌面软件
  - 🔶 逐步补：大型专业软件（缺哪个 API 补哪个，登记缺失清单）
  - ❌ 不可跑：内核驱动类、反作弊（永远走纯 Windows 通道）
- **物理隔离语义**：Windows 进程不存在 → 内存中零 Windows 代码；进程间地址空间由内核隔离；与 Variable 之间只经白名单通道。

### 开放改进点
- Wine 版本跟进策略与 prefix 模板管理（WINEPREFIX 放 VARIX_SYS 模板区）
- glibc/MSVCRT 运行库的打包
- 每软件适配数据库（社区化潜力）

### 验收门禁
- 记事本级目标：Windows 断电状态下实机打开、编辑、保存；
- 办公/工具类 ≥5 款实机通过；每进程资源限额生效验证。

---

## 阶段 6：隐形 Windows 引擎通道

### 目标
需要原生兼容性时，Windows 作为后台引擎被拉起，应用窗口无缝出现在 Variable 桌面。

### 详细逻辑
- **引擎生命周期**：关闭（默认）→ 拉起中（首次 20-40s，UI 如实提示）→ 就绪（常驻，几乎零占用）→ 休眠（内存快照到 WIN_ENGINE 差分盘）；
- **VM 管理**：过渡期用 Hyper-V 底座（WinEngine VHDX 直接挂），终局评估内核自带 VMX 管理；VM 可见资源 = 配额内 vCPU + 内存 + SHARED 白名单 + 独占 VHDX；
- **无缝窗口**：
  - 画面流：应用窗口级抓取 → 流协议（RDP RemoteApp 或 Spice）→ Variable 侧解码为窗口内容；
  - 输入注入：键盘鼠标事件反向注入；
  - VWM 接管：外来窗口注册进 vwmStore，贴靠/最小化/Z 序复用现有状态机（`vwm.ts` 1082 行逻辑原样复用）；
  - 延迟预算：≤50ms；竞技游戏场景提示走纯 Windows 模式；
- **ramcache**：热数据（VHDX 随机读热点）缓存进宿主 RAM；**只缓不落盘、关机即清**——提速同时不破坏"拔盘无痕"的物理隔离；
- **拉起协议**：Variable 点击通道=engine 的软件 → 若引擎未就绪：写请求 + 唤醒引擎 + 就绪后自动打开；窗口出现前显示占位卡（复用 embed 占位卡视觉语言）。

### 开放改进点
- 引擎休眠/恢复速度优化
- 窗口流码率与画质档位
- 多引擎实例（隔离需求更强的场景开第二个引擎）

### 验收门禁
- 引擎冷启动→窗口出现在 Variable 桌面，全流程可用；
- 无缝窗口支持拖动/贴靠/最小化；延迟实测 ≤50ms（办公场景）；
- ramcache 关机后 U 盘上零缓存残留（验证"只缓不落盘"）。

---

## 阶段 7：资源统管与多机打磨

### 目标
内核成为资源总管；设置页管全系统；多机型随插随用。

### 详细逻辑
- **资源配额**（内核服务）：CPU 核分配矩阵（Variable 前台 / 引擎 / Wine 三方）、内存水位与压力回收、GPU 通道（软件渲染 → Vulkan 硬件加速过渡）；
- **设置页新增**：
  - 引导行为：默认项/倒计时/是否显示菜单
  - 软件通道规则：每个软件 wine|engine|native-only
  - 共享白名单管理 + 审计日志查看
  - 性能档位：省电/均衡/性能（影响 ramcache 尺寸与配额）
- **拔出全链**：五分区任一使用中被拔 → 优雅中断 + 快照区恢复（复用 boot.rs 备份恢复逻辑）；
- **多机适配**：UEFI 兼容清单、Secure Boot 处理（关或签名）、首次引导的固件选择引导流程。

### 开放改进点
- GPU 硬件加速（Vulkan 驱动）是独立长线
- 引导页/桌面的多主题适配（复用 Variable 主题系统）
- 版本升级协议：U 盘系统自我更新（差分升级 + 失败回滚）

### 验收门禁
- ≥3 台不同厂商机型随插随用全流程通过；
- 三通道混跑压力测试无死锁、无数据损坏；
- 三线门禁全绿 + 实机验收报告归档。

---

## 全局不变量（任何阶段不得违背）

1. **物理隔离四层**：盘级（Windows 封在 VHDX）/ 机级（宿主机零残留）/ VM 级（引擎边界）/ 进程级（内核独立进程）——任何新功能不得打穿。
2. **默认拒绝**：一切跨域数据访问走白名单，未配置即拒绝。
3. **如实文化**：能力缺失如实降级提示，不伪造成功；todo 与实测分开标注。
4. **行为等价**：Variable 既有视觉与操作语义不因迁移而改变。
5. **可停性**：每阶段交付都是可用的中间态。
6. **零残留**：宿主机任何时刻不落任何数据；缓存只入易失内存。

---

## 附：关键现状坐标（施工时的地图）

- 内核：`kernel/varix/src/main.rs`（引导链，末尾 halt 待替换）、`paging.rs decide()`（缺页决策已写未接线）、`sched/engine.rs`（64 槽 6 类调度器）、`syscall/table.rs`（18 调用号表）、`fs/fs23_journal.rs`（内存态 journal）
- 界面栈：`src/lib/ipc.ts`（554 方法，垫片协议基准）、`src/system/windows/vwm.ts`（窗口状态机，引擎窗口复用）、`src/App.tsx` bootPhase（启动仪式）
- 便携基建：`portable/AI1/Create-VHDX.ps1`（差分链）、`portable/AI5/Deploy-To-USB.ps1`（部署编排）、`src-tauri/src/shell/usb.rs`（拔出看护）、`src-tauri/src/shell/isolation.rs`（Job 限额语义参照）
- 审计：`tools/audit.cjs`（IPC 清单，垫片覆盖率基准）
