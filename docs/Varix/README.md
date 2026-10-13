# Varix 内核 · README 合订本

> 本文件由原 15 章详册（`docs/Varix/README/01~15`）**逐字合并**而成，内容未作任何删改；原分章文件已移除，本文件为唯一真源。

## 目录

| 章 | 内容 | 章内直达 |
|---|---|---|
| 01 | 01 · 架构总览 | [↗](#01--架构总览) |
| 02 | 02 · 引导与平台 | [↗](#02--引导与平台) |
| 03 | 03 · CPU 与中断 | [↗](#03--cpu-与中断) |
| 04 | 04 · 内存管理 | [↗](#04--内存管理) |
| 05 | 05 · 进程与装载 | [↗](#05--进程与装载) |
| 06 | 06 · 图形与桌面 | [↗](#06--图形与桌面) |
| 07 | 07 · 判据体系 | [↗](#07--判据体系) |
| 08 | 08 · 移植线 | [↗](#08--移植线) |
| 09 | 09 · 兼容层 | [↗](#09--兼容层) |
| 10 | 10 · 已知边界（未完成事项清单） | [↗](#10--已知边界未完成事项清单) |
| 11 | 11 · 工程纪律 | [↗](#11--工程纪律) |
| 12 | 12 · 构建与测试 | [↗](#12--构建与测试) |
| 13 | 13 · 路线图 | [↗](#13--路线图) |
| 14 | 14 · 附录 · 实测数据 | [↗](#14--附录--实测数据) |
| 15 | 15 · 语言与生态扩展 | [↗](#15--语言与生态扩展) |

---

# 01 · 架构总览

## 1. 一句话定位

Varix 是一个 `no_std` 的 x86-64 桌面内核：Limine 引导、higher-half 静态映像、Rust 独占内核地址空间、ring3 用户态经 `syscall`/`int 0x80` 双入口进入。它同时维护 354 个判据域，测试即证据。

## 2. 编译形态

内核是**一个 crate 两种形态**：

| 形态 | target | 用途 |
|---|---|---|
| 宿主测试形态 | `x86_64-pc-windows-msvc`（std 可用） | `cargo ktest` 全量判据 |
| 内核镜像形态 | `x86_64-unknown-none`（no_std） | `cargo kbuild` 出 ELF |

条件属性是 `#![cfg_attr(not(test), no_std)]`——crate 根一处声明，**内层模块禁止再写 `#![no_std]`**（内层属性非法，是历史踩过的坑）。`panic = "abort"` 在 workspace 两个 profile 都锁死（`kernel/Cargo.toml:6,9`），宿主测试同样 abort，杜绝"测试吞 panic"的假绿。

链接配置（`kernel/.cargo/config.toml:17-25`）：

- 链接器 `rust-lld`，脚本 `varix/linker.ld`；
- `-C code-model=kernel`、`relocation-model=static`、`panic=abort`、`force-frame-pointers=yes`；
- **不链 libc**。这个事实决定了第 09 章兼容层的全部设计。

依赖面刻意极小：运行期只有 `libm = "0.2"`（musl 数学库的纯 Rust 移植，`float.rs` 实测与 std 最大 1 ULP，仅 `powf`/`exp`，其余 16 种方法逐位一致）；`dev-dependencies` 只有 `blake3`（宿主交叉验证用，不进镜像）。

## 3. 地址空间

`linker.ld` 第 9 行：`. = 0xffffffff80000000;`——higher-half。五个 section：

| section | 内容 | 特殊处理 |
|---|---|---|
| `.limine_requests` | 引导请求 | `KEEP()` 防 GC |
| `.text` | 代码 | 4K 对齐 |
| `.rodata` | 常量（含 `srodata`） | 4K 对齐 |
| `.stubs` | 中断入口桩（运行期生成） | 先可写、生成完由 `cpu::idt` 改 R-X |
| `.bss` | 清零区 | `__bss_start/__bss_end` 符号导出 |

`/DISCARD/` 丢弃 `.eh_frame*`、`.note`、`.comment`（`linker.ld:56-61`）。**这行是兼容线的硬约束**：C++ 异常展开表直接出局，改它是一行工程决策（第 13 章路线图有条目）。

用户半区与内核半区的分界、thunk 页基址 `0x7000_0000`、映像区基址 `0x1400_0000`、`USER_STACK_TOP`——这些常量在 `proc/winapi.rs` 与 `proc/` 各模块引用同一份定义，禁止旁路另算。

## 4. 工作区

`kernel/Cargo.toml` 是 workspace，`members = ["varix"]` 单成员。`userspace/varix-std` 与 `varix-svcabin` 是独立构建单元（用户态 `no_std` 库，`x86_64-unknown-none`，经 `syscall0_raw` 内联汇编进内核），**不进 workspace**——用户态与内核态的构建边界是刻意的，防止内核 target 配置污染用户态。

共享 `target/` 有过一次版本缓存污染事故（`libm` 元数据被 rustc 1.98.1 编译，而项目锁定 1.97.1），处置是删缓存重跑。现纪律：构建目录可整体删除重建，但**源码锁定版本不允许漂移**。

## 5. 判据域的物理布局

354 个域不是计数游戏，是目录结构的直接映射：

| 目录 | 域举例 | 判据规模 |
|---|---|---|
| `src/cgpu/` | 显示/EDID/拓扑/管线/功耗 | 50 项（全绿，2026-10-12） |
| `src/unxstar/` | 时长分布/零等式/负载追踪/平衡执行 | 70 项（全绿，2026-10-12） |
| `src/svstar2/` | 网遥测/音频资产/AST/调优双域 | 452 项值数组已改函数指针表 |
| `src/checks/` | 判据基建自身的判据 | 元判据 |
| 根模块 | bootchain/compatruntime/bfport/ckport… | 各域 10–40 项 |

每个域文件命名带语义编号（如 `unx1421_latdist.rs`），配套 `_checks.rs` 文件或文内 `run_*_checks()` 函数。注册表在 `robust.rs::run_kernel_checkup()`——**新域必须在此注册**，否则编译绿但判据永不执行（第 11 章纪律三）。

## 6. 一次调用的完整链路

以一次 Win32 调用为例（用户态记事本写文件）：

```
用户代码 call [iat:WriteFile]
  → thunk 页槽（0x7000_0000 起，每 API 32B）
  → syscall / int 0x80 进 ring0（ring3.rs:1258 syscall_entry）
  → winapi.rs dispatch（服务号 WIN32_NR_BASE=0x40 段）
  → 实现（winsrv.rs 语义 / winmem.rs 内存面 / 转发 SYS_*）
  → 回 ring3（ring3.rs:1315 enter_user）
```

链路上每一环都有判据：thunk 槽编码、服务号唯一性、三态计数、失败码语义、寄存器约定。断在哪一环，判据名直接指路。

## 7. 数据资产

仓库内与内核强相关的数据资产：

- `X:\OpenSource\_a4_real_lines.tsv`：188 个上游仓的实测行数/文件数/字节（总量 167,082,777 行），移植线（第 08 章）的台账底表；
- `docs/Varix/CoRun Varix STAR II · Unxreal/Windows软件90%兼容总案…md`：21 篇兼容总案，含 1000 基准应用清单与六级让步阶梯；
- `docs/Varix/7AI分工总案.md`：七个席位分工，AI-4 负责 207 项目定制化移植线。

这些资产与代码的关系：总案定目标，TSV 定台账，代码定进度，README 定如实汇报。

---

# 02 · 引导与平台

## 1. 引导协议

Varix 用 **Limine base revision 1**。`main.rs` 模块文档写明：Limine 把 `_start` 当普通 C 函数在 BSP 上调用，栈已就绪、请求应答已填充。`main.rs:16`：

```rust
#[no_mangle]
extern "C" fn _start() -> ! { boot(); }
```

引导器扫描整个可执行映像找请求魔数，所以 `linker.ld` 对 `.limine_requests` 不做排序要求，只用 `KEEP()` 防止 section GC。

## 2. 启动时序

`TOTAL_STAGES = 14`（`main.rs:21`）。每个阶段进 `timeline`（F024 打点），推进条 F014 按"阶段数 = 进度条总数"的真值映射前进，收尾跑自检 F025。14 段不是凑数：每一段对应一个必须成功的平台初始化（帧缓冲应答、内存映射应答、SMP 起核、HPET 校准、ACPI 解析等），少一段进度条就与真实状态脱钩。

## 3. 拒绝启动路径（B-101）

`main.rs:27 refuse_boot()` 是启动失败的唯一出口，双通道兜底：

1. **串口先行**：`serial::init()` 后直接写 COM1（16550 自初始化，绕开 logger——此时 logger 全局状态还是零初始化，走 logger 会二次故障）；
2. **屏幕人话**：若 Limine 帧缓冲应答在，用 `banner::paint_backdrop` 铺深空底 + `font::draw_text_scaled` 画三要素（发生了什么/为什么/FIX 提示），自动折行按列宽裁剪（`main.rs:50-55`）。

设计立场写在注释里：串口断在 QEMU 对练里会当场暴露，两路都断才算真黑屏。

## 4. bootchain 域

`src/bootchain.rs`（1700+ 行）+ 子模块：

| 文件 | 职责 |
|---|---|
| `bootchain.rs` | 启动链主体；`bootchain.rs:1734` 定义标准服务清单（init/log/fb 核心，网络/音频/打印/包管理非核心） |
| `audit.rs` | 启动链审计 |
| `health.rs` | 健康面 |
| `recovery.rs` | 恢复路径 |
| `secureboot.rs` | 安全引导（当前是结构面：`secureboot.rs:263` 只有符号结构，**无密码学实现**——见第 10 章已知边界 #31） |

## 5. 启动关键文件

仓库根 `boot-select.json` 与 `limine.conf` 是启动必需文件，**不可删**（项目记忆三条血坑之一）。ISO 组装工具 `make_iso.py` 会把它们放进镜像根；`pycdlib` 已装入隔离 venv，`build/isoroot` 为仓库根目录（不是 `kernel/build/`，历史上误判过一次）。

## 6. ACPI 与平台发现

`src/acpi.rs` 负责表解析。平台信息（CPU 拓扑、HPET、EDID）分属各自域：

- EDID → `cgpu/cgm03_edid.rs`（只读原始块，**不解 HDR 元数据**，见第 06 章）；
- HPET → `unxstar/unx1023_hpettick.rs`（丢拍注入测试与 ns→tick 换算方向判据）；
- CPU 拓扑 → `cgpu/cgm04_topology.rs`。

## 7. 真机与 QEMU

`kernel/_tools/`：

| 工具 | 作用 |
|---|---|
| `make_iso.py` | 组装可引导 ISO（含 pycdlib 路径） |
| `qemu_boot_verify.py` | QEMU 引导验证（串口输出对练） |

历史构建产物（旧 ISO、日志、ELF 备份）已归档到 `attic/build_artifacts_2026-10-12/`，仓库根不保留生成物。

## 8. 本域未完成事项

如实在账，全部详见第 10 章：

- `secureboot.rs` 无 RSA/ECDSA 签名验证实现（第 10 章条目 #31/#32）；
- TPM 2.0 平台信任根未接入（条目 #35）；
- 真机（非 QEMU）启动的定期验收未制度化——QEMU 对练在跑，真机抽验频次无判据锁定。

---

# 03 · CPU 与中断

## 1. SMP 启动与核间交接

`src/cpu/smp.rs`（900+ 行）负责 AP（应用处理器）启动。几个有判据背书的细节：

- `smp.rs:857 pub extern "C" fn varix_ap_entry() -> !`：AP 入口，C ABI，供 trampoline 跳转；
- `smp.rs:472` 与 `smp.rs:902` 两处 `extern "C"` 块：与汇编/引导侧的符号契约，**签名变更必须双侧同步**——历史上"六段交接补齐签名"（unx1721）修的就是这类账漏；
- 核间交接走统一验签口径（`bind_fp` 验签），`unxstar/unx1721_balexec.rs` 的判据锁死"段序记账 + 六段交接签名"。

## 2. 中断：IDT 与可写桩页

`src/cpu/idt.rs`（700+ 行）。设计要点：

- 中断入口桩**运行期生成**，所以 `.stubs` section 在链接脚本里单独成段且可写（`linker.ld:30-36`）；
- 生成完成后 `cpu::idt` 把该页权限**加固为 R-X**——W^X 纪律对内核自身同样执行，不只是用户态映像；
- 派发：`idt.rs:573 extern "C" fn isr_dispatch(vector: u64, rsp: u64)`，统一入口分发，向量号与栈指针进 Rust 世界；
- 通用栈框：`idt.rs:709 unsafe extern "C" fn common_entry()`。

## 3. syscall 机制（F102/F103）

`src/proc/syscall.rs`（608 行）。号表设计一句话（模块文档原文）："One table, one dispatcher, one numbering scheme"——每个调用先查"号存在吗、参数够吗"，再进 handler，handler 不自我防御畸形帧。

当前常量（**全部实测**）：

| 常量 | 值 | 备注 |
|---|---|---|
| `MAX_SYSCALLS` | **64** | `syscall.rs:10`。这是兼容线的号位瓶颈：Win32 段 `WIN32_NR_BASE = 0x40 = 64` 起，意味着 Win32 可用号位为 0 个（第 10 章边界 #1，扩容方案第 13 章 P0） |
| `STABLE_SYSCALL_BASE/COUNT` | 0 / 16 | F490 承诺不动的稳定 ABI 基座 |
| `MAX_SYSCALL_ARGS` | 6 | rdi/rsi/rdx/r10/r8/r9 |

稳定号 0–15：`SYS_EXIT/READ/WRITE/OPEN/CLOSE/MMAP/FORK/EXEC/WAIT/SIGNAL/SLEEP/TIME/CAPS/PROC_INFO/TRACE/SYNC`（`syscall.rs:18-33`）。

错误码语义（`SyscallError::errno()`）：`NotImplemented/BadNumber → -38 (ENOSYS)`、`TooFew/TooManyArgs → -22 (EINVAL)`、`Denied → -13 (EACCES)`、`Failed → -5 (EIO)`。

## 4. ring3 双入口

`src/proc/ring3.rs`（1567 行）是用户态执行的核心：

| 设施 | 位置 | 说明 |
|---|---|---|
| GDT 用户段（RPL3） | ring3.rs | 段描述符构造 |
| TSS.RSP0 | ring3.rs | ring0 切换栈 |
| `syscall` 快速入口 | `ring3.rs:1258 unsafe extern "C" fn syscall_entry() -> !` | MSR：STAR/LSTAR/EFER.SCE |
| 慢速入口 | `int 0x80` | 与 syscall 同语义表 |
| 回环 | `ring3.rs:1315 unsafe extern "C" fn enter_user(rip, rsp, rflags) -> !` | 进用户态 |

双入口不是冗余：`syscall` 走快路径，`int 0x80` 是兼容与调试面（单步跟踪、历史工具链）。两者共用同一号表与 dispatcher——**一份语义，两个门**，判据保证两门行为一致。

## 5. 调度与执行面

调度域散布在：

- `benchsix.rs`：启动/交接/应用/存储/网络五类基准，输出是**分布不是均值**（P50/P95），`benchsix.rs:375` 锁定 27 个点位齐且无重复（syscall/上下文切换/页分配释放/帧管线/块路径/网络环回）；
- `unxstar/unx1701_loadtrk.rs`：负载追踪，定点 IIR 带倍率与跨步余数（量纲正确性有专判据）；
- `unxstar/unx1721_balexec.rs`：平衡执行域，六段交接签名 + `bind_fp` 统一验签口径。

## 6. 与兼容层的接口点

Win32 段号位、thunk 页、服务台 dispatch 三者的关系在第 05 章展开。CPU 域对兼容层只承诺三件事：

1. 号表查得到、参数数得对（F102/F103）；
2. ring 进出栈帧完整（ring3 双入口判据）；
3. 失败返回负错误码，绝不静默（`winapi.rs:557` 判据：stub 与缺号必须返回 Enosys 且计数上账）。

---

# 04 · 内存管理

## 1. 分区总图

内核把用户半区的常驻区域划分成互相隔离的带，常量集中在 `proc/` 各模块（`winapi.rs:39` 等）：

| 区域 | 基址 | 用途 |
|---|---|---|
| 用户映像区 | `0x1400_0000` 起 | PE/ELF 映像装载 |
| thunk 页 | `0x7000_0000`（4KiB，128 槽 × 32B） | Win32 API 跳板 |
| 用户栈 | `USER_STACK_TOP` 附近 | ring3 栈 |

隔离规则（`winmem.rs` 判据 W441 锁定）：堆域起点必须在用户半区、不与 thunk 页/映像区/栈区重叠。三个 `overlaps_*` 函数是唯一判定真源，堆分配器与测试判据共用同一份。

## 2. 页账本与装载引擎

`proc/loader.rs`（605 行）是格式无关的装载引擎：

- `ImageFormat` trait：PE（`pe.rs:609 PeSource`）与 ELF（`elf.rs`）各自翻译成 `SegmentView`；
- **页账本/同页共享/回滚语义全部在引擎侧**，格式模块零知识——`pe.rs:14-17` 模块文档明言这是"结构保证"：引擎对格式不感知，格式出错不可能污染页账；
- W^X 口径统一：可写节强制 NX（`pe.rs:19`），映射层 `executable=false`；`pe.rs:692 map_user_frame(va, phys, w, nx)` 是唯一映射原语，测试夹具直接用它。

## 3. 用户地址空间 arena

`proc/uspace.rs` 提供 `SpaceArena`：用户半区的页分配面。判据在 `proc/pe.rs:853` 一带夹具中可见（内核半区 ImageBase 拒绝、入口不在可执行节拒绝、节越文件边界拒绝）。

**当前边界**（如实）：`pe.rs:32 MAX_IMAGE_SIZE = 256 MiB`，超出按损坏/恶意头拒绝。对 Wine/Chromium 级别的 DLL 这是硬顶，扩容方案在第 13 章。

## 4. 内核堆与 BSS

- `.bss` 由 Limine 装载清零（`proc/usrshell.rs:303` 注释确认），`__bss_start/__bss_end` 符号已导出（`linker.ld:47-51`），供调试器与未来 C 环境对账；
- 内核堆分配器供 `alloc` crate 系使用；`winmem.rs` 的用户堆（VirtualAlloc 语义）是**独立 bump 分配器**，与内核堆隔离，LIFO 释放（W444 判据锁定非栈顶块释放必须 EINVAL）。

## 5. 内存安全判据（实测通过）

2026-10-12 全量基线里，内存相关域全部绿：

| 判据 | 域 | 锁定的性质 |
|---|---|---|
| W441 | winmem | 堆域与 thunk/映像/栈互不重叠 |
| W442 | winmem | 分配地址 16B 对齐、单调前进、域内 |
| W445 | winmem | 新分配块内容全零（Win32 语义） |
| W446 | winmem | 块栈容量耗尽返回 ENOMEM（与堆域溢出是两条独立边界） |
| unx1421 | unxstar | 时长分布桶边界 CDF 逐字相等、分位估计不越桶 |
| unx1461 | unxstar | 零等式序号每向量独立、`engine_steps` 只计基线外开销 |
| unx1701 | unxstar | 定点 IIR 量纲：倍率与跨步余数 |
| unx101:2058 | unxstar | 重叠拷贝方向判定（memmove 语义纯函数真源） |

## 6. 测试栈预算

一个容易被忽略的内存事实：判据总闸是栈上定长数组——`[Option<CheckSet>; MAX_DOMAINS]`，354 域容量 ≈ 331KB；F475 叶条目与闭环嵌套两份 ≈ 662KB。默认 2MiB 测试栈曾触发 `STATUS_STACK_OVERFLOW`，现设 `RUST_MIN_STACK = "16777216"`（16MiB，`.cargo/config.toml:14-15`）。注释明确：**该变量只影响宿主测试进程，`kbuild` 内核镜像路径不读**。

## 7. 本域未完成事项

- `pe.rs:626-627`：PE 不做重定位，按首选基址静态装载——基址冲突即装不下（第 10 章 #2，`.reloc` 解析方案第 13 章）；
- `MAX_IMAGE_SIZE` 256 MiB 顶（第 13 章提级条目）；
- 大映像分页映射（当前节表 `MAX_SECTIONS = 96` 够用，但多 GB 映像的页账容量未实测）；
- 需求分页（demand paging）未实现——装载即全量占页。

---

# 05 · 进程与装载

## 1. 装载器家族

| 文件 | 行数 | 职责 |
|---|---|---|
| `proc/pe.rs` | 895 | PE32+（x86-64）解析与装载 |
| `proc/elf.rs` | — | ELF64 解析（与 PE 同一 `ImageFormat` 口径） |
| `proc/loader.rs` | 605 | 格式无关装载引擎（页账本/同页共享/回滚） |
| `proc/peblock.rs` | 277 | PEB 结构面（Windows 进程块等效物） |
| `proc/exec.rs` | — | exec 入口编排 |

装载器立场（`pe.rs:3-6` 模块文档）：装载器是决定"这个文件能不能跑"的人，**拒绝一切不能完全担保的东西**——非 AMD64、非 PE32+、节越文件边界、入口不在可执行节、ImageBase 落进内核半区。exec 时一次说清拒绝原因，好过三条指令后一场莫名故障。

## 2. PE 解析面（实测）

常量与校验（`pe.rs:24-46`）：

- `MACHINE_AMD64 = 0x8664`、`OPTIONAL_MAGIC_PE32P = 0x20B`；
- `MAX_SECTIONS = 96`、`MAX_IMAGE_SIZE = 256 MiB`；
- W^X：`SCN_MEM_EXECUTE/WRITE/READ` 三位独立判定，可写节强制 NX。

`PeImage` 暴露：节表（`PeSection`，含 `end()`/`contains(rva)`）、入口绝对 VA、导入目录（`import_dir`，任务 40 兑现）。

导入表解析（`pe.rs:485 parse_imports_into`）：IMAGE_IMPORT_DESCRIPTOR 数组 → INT/IAT thunk 链 → IMAGE_BY_NAME 记录 → 结构化 `ImportTable`。容量上限 `MAX_IMPORT_DLLS = 8`、`MAX_IMPORT_FUNCS = 64`；老式 bound imports 显式拒绝（fresh resolution only）。**这个结构就是兼容性诊断的数据源**——任何 Windows PE 丢进来，能报出"依赖哪些 DLL、哪些函数、其中多少有实现"。

## 3. ring3 与 Win32 服务台

执行链（第 03 章的 ring3 之后）：

```
ImportTable → winapi.rs 注册表查找 → IAT 补钉 + thunk 槽写入 → ring3 启动
```

服务台（`proc/winapi.rs`，777 行）：

- thunk 页 `0x7000_0000`，4KiB / 128 槽，每 API 32B（17B 编码 + 15B 余量，槽内偏移固定便于宿主/实机同址断言）；
- `API_TABLE` **33 项**（`winapi.rs:84`），四组：kernel32（进程/内存/文件）、ntdll（NTAPI 直呼）、user32（窗口面，多参 API 走 NT 风格参数块桥接）、gdi32（文本/绘制）+ comdlg32（文件对话框）；
- 三态：`Full 18 / Partial 9 / Stub 6`。Full = 内核侧语义完整；Partial = 映射到既有服务且边界如实声明（如 `GetStdHandle` 仅 `STD_OUTPUT_HANDLE=-11`）；Stub = 返回 Enosys，绝不崩溃；
- 判据 `winapi.rs:499`：`assert_eq!(stub, 6, "未实现面如实保持 Stub")`——**账面被锁死**，改表必须连判据一起改，防漂移。

`proc/winsrv.rs`（1136 行）承载语义实现：win32_dispatch 分发、虚拟文件槽（`VFiles`，内存态）、`read_user_bytes`/`write_user_bytes` 用户内存访问原语。user32 的多参 API（`MessageBoxW` 等）走 NT 风格参数块：用户态把参数打包进块，内核解包执行——32B thunk 槽装不下的签名用这条路。

## 4. winmem：内存面实现（任务 41 后半）

`proc/winmem.rs`（712 行）实现 Win32 内存 API 的内核侧：

| API | 状态 | 语义 |
|---|---|---|
| `VirtualAlloc` | 实现 | bump 分配，16B 对齐，新块交零页；零尺寸/指定地址/非法保护属性 → EINVAL；超域 → ENOMEM + last_error=8 |
| `VirtualFree` | 实现 | **仅 LIFO**：非栈顶块拒绝；非 `MEM_RELEASE` 拒绝 |
| `GetLastError` | 实现 | 成功清零/失败置码/读回一致（W447） |
| `CreateFileW` | 实现 | `CREATE_ALWAYS` 建/覆盖、`OPEN_EXISTING` 命中与未命中（W448，UTF-16 名经用户半区读入） |
| `NtAllocateVirtualMemory` | 实现 | 就地回填 BaseAddress/RegionSize 两槽（W449） |
| 失败计数 | — | 分配/释放失败独立计数（反证失败路径真的执行了） |

**如实声明**：该模块 2026-10-12 深夜完成编写，判据在 `run_winmem_checks()` 内，**最终复验结果未确认**（编译被构建锁阻塞，第 10 章 #12）。上述行为是设计意图 + 单元判据文本，不等于全绿实测。

## 5. 进程生命周期

- `SYS_EXIT(0)` → `ring3` sys_exit → 监护链收割（`winapi.rs:86` 注释）；
- `ExitProcess` / `RtlExitUserProcess` / `NtTerminateProcess` 三个 Full 条目同归一路；
- 句柄分类：任务 41 扩展了 `ReadFile`（handle 3 = 读取源）与 `CloseHandle`（句柄收尾）。

## 6. 用户态库

`kernel/userspace/varix-std/`：`#![no_std]`，`x86_64-unknown-none`，`abi.rs`/`io.rs`/`mem.rs`/`proc.rs`/`time.rs`/`event.rs`/`error.rs` 七个模块，`syscall0_raw` 内联汇编进内核。`varix-svcabin` 是服务容器 ABI。两者独立构建（第 01 章第 4 节），`userspace/codeanalysis/` 是第一个真实用户态程序（事件循环 + 清单 + VFS 桥）。

## 7. 本域未完成事项

- `pe.rs:626`：无重定位（`.reloc` 不解析），ASLR/基址冲突场景装不下；
- `MAX_IMPORT_DLLS = 8` / `MAX_IMPORT_FUNCS = 64`：对真实大型应用（msedge 导入 40+ DLL）不够，扩容方案第 13 章；
- 资源节（`VS_VERSION_INFO`/manifest）零解析——版本信息读不出；
- `winmem.rs` 判据待复验（第 10 章 #12）；
- Win32 覆盖面 33 项 vs 目标生态需要的 ~4000 项（路线图 P2–P5，第 13 章）。

---

# 06 · 图形与桌面

## 1. 桌面环境 AURORA

`src/aurora/` 是桌面环境主体，18 个模块：

| 模块 | 职责 |
|---|---|
| `compositor.rs` | 合成器 |
| `desktop.rs` / `window.rs` | 桌面与窗口管理 |
| `display.rs` / `gpu.rs` | 显示与图形抽象 |
| `render2d.rs` | 2D 渲染引擎（见下节） |
| `typography.rs` | 排版面 |
| `widgets.rs` / `demo.rs` | 组件与演示 |
| `input.rs` / `motion.rs` | 输入与动效 |
| `audio.rs` / `image.rs` | 音频与图像 |
| `clipboard.rs` / `session.rs` | 剪贴板与会话 |
| `appfw.rs` / `integration.rs` / `w2/w4_integration.rs` | 应用框架与集成 |

## 2. render2d：纯软件 2D 渲染内核

模块文档（`aurora/render2d.rs:1-5`）列出全部能力：**矢量路径 / 位图混合 / 文字光栅化 / 亚像素抗锯齿 / 裁剪遮罩 / 线性径向渐变 / 图层合成 / 离屏 / 脏矩形 / 批处理 / 快照 / 性能剖析 / 降级链**，全部固定容量数组 + 像素级逻辑，GPU 缺失时亦可跑。

关键常量：`CANVAS_W = 16`、`CANVAS_H = 16`、`BPP = 4`——**这是测试用小画布**，真机走 `fb::Surface`。像素结构 `Rgba` 直通 alpha（非预乘），`set_px` 是唯一写像素原语，越界自动裁剪。

这套能力恰好覆盖 CSS 渲染所需的底层（渐变/遮罩/合成/AA），是第 13 章布局引擎路线的既有地基。

## 3. 字体：现状与硬边界

`src/font.rs` 的现状（实测，行号可查）：

- `font.rs:8`：**"Glyphs for ASCII 0x20 (space) ..= 0x7E (~): 95 entries"**——只有 95 个 ASCII 字形；
- `font.rs:10-13`：`GLYPH_W = 8`、`GLYPH_H = 8`，8×8 等宽点阵，`CELL_H = GLYPH_H * 2`；
- `font.rs:116`：**非 ASCII / 控制码一律返回空白格**——渲染不出一个汉字；
- `font.rs:202`：文本宽度 = `字符数 × GLYPH_W × scale`——按字符数估宽，不按实际字形；
- 缩放只有整数倍 `scale`（`draw_text_scaled`）。

**这是全系统最大的单点能力缺口**：目标用户是中文桌面用户，而当前字库一个汉字都画不出。替代路径（skrifa/ttf-parser 轮廓 + HarfBuzz 整形）在第 13 章，是路线图第一优先。

`src/atlas.rs` 已备好字形缓存（glyph_id + size_px 键控的 pin/insert/lookup），新字体栈可直接对接。

## 4. 输入法候选窗（candwin）

`src/candwin.rs`：WP-202 · B-906 判据落点——候选窗是合成器浮层（popup_grab 抓取），定位跟随焦点窗口上报的光标锚点，**Wine 窗口与原生窗口同一路径**（`AnchorOrigin::{Native, WineBridge}`，定位算法不看来源）。规则：默认锚点下方，越下边界翻上方，四边夹紧回屏内，**永不越屏**（`place()` 有确定性判据：同锚点同尺寸同位置）。

## 5. cgpu 域

显示栈判定层，50 项判据（2026-10-12 全绿）：

| 组 | 内容 |
|---|---|
| cgm 系列 | display（M01/M02 双实现对拍）/ EDID / 拓扑 / 模式 |
| cge 系列 | domain / lifecycle |
| cgp 系列 | unified model / sampling / pipeline / tiers / query / jmerge |
| cgr 系列 | secure / threatmodel |
| vcj01 | 功耗架构 |
| cgd02 | frametimer |

显示协商回退链（hdr10 → sdr-high → sdr-native）在 `compatruntime.rs:96-108`，三档矩阵有专判据。

## 6. 帧缓冲

`src/fb.rs`：`Surface::from_limine` 接引导帧缓冲；控制台滚动是逐行 `memmove` 语义（`fb.rs:321`），1080p 单次全屏滚 8.1MB——这是 `unx101_vgafallback.rs:1870` 那条"memmove 翻车教训"的来源：**重叠拷贝方向选错 = 自我覆盖**，方向判定抽成纯函数后成为全系统真源。

## 7. 音频

`audio.rs` + `audio/tone.rs`：当前是**自研合成音**（tone generator），不是解码器。格式协商回退链（float32-192k → float32-48k → int16-48k → int16-44k）在 `compatruntime.rs:137`，四档判据齐（X08207 失败叙事 / X08208 空表回退净身）。真实解码（opus/flac/vorbis）属第 13 章多媒体路线。

## 8. 本域未完成事项

| 缺口 | 影响 | 依据 |
|---|---|---|
| 中文字形为零 | 桌面无法显示中文 | `font.rs:8/116` |
| 真字体轮廓缺失 | 任意磅值缩放/连字/字距全无 | 8×8 点阵 + 整数 scale |
| 文本宽度按字符数估 | 排版精度低 | `font.rs:202` |
| HDR 元数据不解析 | EDID 只读原始块 | `cgm03_edid.rs` |
| 色彩管理（ICC）无 | 色准不可控 | 全仓无 ICC 命中 |
| 摄像头/采集无 | `appmgr.rs:19` 权限位有名无实 | — |
| 真解码器无 | 音视频皆合成/占位 | `audio/tone.rs` |

---

# 07 · 判据体系

## 1. 核心结构：CheckSet

`src/checks.rs` 是判据基建真源：

- `CheckSet::new(domain)`：带域名的判据集；
- `set.add(标签, 布尔性质, 说明)` / `add_dyn`：性质必须写成**可推导的布尔表达式**，不允许写死会漂移的常量比较；
- `red_items()`：返回红项原始数组（给程序用）；
- `red_items_text()`：失败路径生成「标签(明细)」名单（给人看）——**写新聚合判据一律用它**，不要手写遍历。

判据写在 `run_*_checks()` 函数里（不是 `#[cfg(test)]` 才有），由总闸统一调度。判定成功的标准只有一种：`set.ok()` 全绿；失败输出 `red_items_text()` 直接点名，**不许只报"N/M 绿"**——那是把定位成本转嫁给下一轮调试（46 红项归因战役的最大教训）。

## 2. 总闸：run_kernel_checkup

`src/robust.rs`：

- `let domains: [fn() -> CheckSet; 354] = [...]`——**函数指针表**，354 个域的注册表；
- 每新增一个域，数组长度 +1，E0308 编译期强制同步（这是把"忘记注册"变成编译错误的设计）；
- `run_kernel_checkup()` 逐域调用、逐层诊断、二级红项穿透（`svstar2/mod.rs` 的 452 项值数组已改此表）。

**纪律**：新模块编译绿 ≠ 判据在跑。没进注册表的 `run_*_checks()` 永不执行（第 11 章纪律三）。补注册后，此前所有 `cargo check` 结果作废，必须全新量重跑。

## 3. 判据写作规范

从 46 红项归因战役提炼的五条：

1. **先问"判据红还是实现红"**。46 条红里只有 4 条是真实现缺陷，其余全是判据自身写错（写死数字漂移 / 期望值算错 / 用超出容量的样本 / 挑了会绕开被测路径的策略 / 踩了模块文档自己点名的坑）。
2. **注释承诺的性质，实现必须真的做到**。实例：`SignBook::sign()` 注释写"指纹当前"，实现却比的是"与已有签名一致"——首签恒放行。注释里出现"当前/最新/有效"这类词，必须去代码里找到那次比较，确认它比的是真源不是副本。
3. **标准环境必须真的能构造**。实例：`vea54_framelock::standard_scope()` 第 4 槽 UNDECLARED，而 `validate_scope` 要求四槽全 declared ⇒ `standard_lock()` 必然失败 ⇒ 调用它的 40+ 条判据全被挡在门外。凡"标准/夹具/范例"构造被 `.expect()` 包裹，先确认它真的能成。
4. **分子分母、量纲、期望值必须可推导**。实例：unx1421 的开销分子分母、unx1461 的 `engine_steps` 只计基线外开销、unx1701 的定点 IIR 带跨步余数——修的都是"账没对齐"而不是"算法不优"。
5. **修一条判据，顺着"谁依赖同一个巧合"排查到底**。改 `standard_scope()` 后，靠"标准态自带 UNDECLARED"造样本的判据跟着变绿——依赖链要一次清干净。

## 4. 测试栈与运行

- `RUST_MIN_STACK = 16MiB`（`.cargo/config.toml:14-15`）：判据总闸是栈上定长数组（354 域 ≈ 331KB + F475 双份 ≈ 662KB），默认 2MiB 曾 `STATUS_STACK_OVERFLOW`；
- 全量基线：**13689 passed / 26 failed**（2026-10-12，约 364 秒）；
- 过滤单域：`cargo test --lib <域名>`；两个移植落点一起跑：`cargo test --lib -- bfport ckport`；
- 构建锁教训：并行 cargo 互抢锁会假死——先 `taskkill //F //IM cargo.exe`，单跑、输出落盘、`tail`/grep 取结论，**不要在 6933 条 warning 里找 error**。

## 5. 域级样例（实测全绿，2026-10-12）

| 域 | 判据数 | 锁定的性质 |
|---|---|---|
| cgpu | 50 | 显示/EDID/拓扑/管线/功耗/安全 |
| unxstar | 70 | 桶边界 CDF 逐字相等、IIR 量纲、六段交接签名、丢拍注入从 `expected_beat + skip_each` 跳转 |
| vef62 | — | 采样期望：40 次 `record_bytes` 实际 80 事件 |
| veab07 | — | 音频时长 2 秒、压缩率不倒数、`u32/u64` 边界提升 |

## 6. 红项穿透设计

`svstar2/mod.rs` 的 452 项聚合采用**函数指针表 + 二级红项穿透**：表层红时，进入子集再点名到具体条目。折叠式域聚合（cgpu 50 / unxstar 127 / svstar2 数百子集）里，红项定位成本远高于修复成本——穿透诊断是为此设计的。

## 7. 元判据

`src/checks/` 目录下的分册（如 `ai27.rs`）是"判据的判据"：能力位掩码语义（b0 文件读…b5 通知）、判据计数对账（`winapi.rs:488` `full + partial + stub == API_COUNT`）等。元判据保证判据体系自身的完整性。

---

# 08 · 移植线

## 1. 方法论：逐文件精读重写

上游代码**不 vendor 拷贝**。每个落点的流程：

1. 读上游源码，理解算法本体（不是照抄行文）；
2. 用 Rust 重写为 `no_std` 内核形态：中文模块文档、锚点注释、固定容量、诊断码独占段；
3. 配独立判据，进 `robust.rs` 注册表（域数组 +1）；
4. 宿主测试全绿后，落点才算存在。

诊断码独占段现状：`bfport` 占 `0x1460`。

## 2. 已完成落点

### bfport（bitflags 2.13.2 → `src/bfport.rs`）

上游 8,396 行的算法层移植（**不移植过程宏**——内核不需要编译期宏展开，要的是位运算语义）：

- 定长位存储 `Flags<N>` + 闭集表 `T`；
- `from_bits_truncate` / `from_bits_retain` / **`from_bits_strict`（本模块新增：显性拒非法位，上游没有）**；
- `intersects` / `contains` / `union` / `intersection` / `difference` / `is_empty` / `is_all` / `truncate` / `count_ones` / 具名迭代；
- 接线：`lib.rs pub mod bfport;` + `robust.rs` 域数组 352→353。

红项修复实录（判据错不是实现错的实例）：原判据夹具 `T_A` 定义 bit0~bit4（`all_a == 0b1_1111`），raw 取 `0b1_1010`（bit1/3/4 全在闭集内），注释却说"bit1 未定义"——自相矛盾恒红。修法：raw 改 `0b10_1010`（bit5 越界），期望值改可推导式 `raw & 0b1_1111`，并补 strict 正向对照。

### ckport（bytemuck 1.25.2 → `src/ckport.rs`）

上游 8,785 行的类型双关/零初始化/布局检查内核：`cast`/`cast_mut`/`cast_slice` 语义族、对齐与大小检查、`PodCastError` 错误语义。13 条判据。接线同上，域数组 353→354。

**如实声明**：ckport 判据 2026-10-12 起**未复验**——编译被构建锁阻塞，最终结果未确认（第 10 章 #11）。代码已写完，绿不绿待验。

## 3. 台账：188 仓实测

普查脚本 `kernel/_tools/count_oss_lines.py`，结果 `X:\OpenSource\_a4_real_lines.tsv`：

| 指标 | 实测 |
|---|---|
| 实体仓 | 188（`_` 前缀状态文件不计；wine 两个副本实为一仓） |
| 源码总行数 | **167,082,777** |
| C/C++ | 142,167,920（85.1%） |
| Rust | 20,452,641（12.2%） |
| Python | 4,462,216（2.7%） |
| 文件数 / 体积 | 418,292 / 6.23 GB |

Top 5：linux-kernel 34.8M / llvm-project 17.9M / ui-visual 12.7M / reactos 9.3M / node 8.0M。完整 Top 表在第 14 章附录。

## 4. 七层落法

167M 行不等于 167M 行重写。七层归类：

| 层 | 代表仓 | 手段 | 需自研行数 |
|---|---|---|---|
| L1 内核本体 | varix | 自研 | — |
| L2 驱动 GPU 算法 | mesa/angle/VK-GL-CTS/edk2 | 挑算法层重写 | 40–70 万 |
| L3 内核算法 | rust 库生态（glam/nalgebra/smoltcp/heapless…） | 逐文件 `no_std` 重写 | 80–120 万 |
| L4 系统服务 | freetype/harfbuzz/icu/FFmpeg | C→Rust 或 freestanding C | 150–250 万 |
| L5 应用运行时 | wine/node/openjdk/dotnet/cpython/qtbase | **交叉编译 PE，零改动**，ring3 装载 | 0 |
| L6 工具链 | zig/llvm/mingw/rust | 构建期用，不进产品 | 0 |
| L7 资产 | 字体/主题/图标 | 直接打包 | 0 |

**关键结论**：真正需要精读重写的 ≈ 300–550 万行（占总量 2–3%）；其余 1.6 亿行靠"内核能装载 PE"承接——那需要 `pe.rs` 两个瓶颈先解（第 13 章）。

## 5. 下一批落点候选

按"贴合内核"排序（依据 TSV 筛选：纯 Rust、1.5k–30k 行、C++/Python 占比低）：

1. `heapless`——定长容器，`no_std` 天然匹配；
2. `smoltcp`——TCP/IP 栈，网络域真源（替代 `compatruntime` 的协商模拟）；
3. `x86_64`——CPU 表与结构体，补 CPU 域；
4. `ttf-parser` + `skrifa`——字体轮廓，解中文字形缺口（第一优先，见第 13 章）；
5. `glam`/`nalgebra`——向量数学，图形域。

## 6. 移植线与判据的耦合

每个落点消耗判据预算：域数组 354 → 每落点 +1。按 60–80 个落点估算，数组容量与 `RUST_MIN_STACK`（每域 +331KB 栈账）需要同批规划——这是第 13 章基建条目"判据容量扩容"的由来。

---

# 09 · 兼容层

## 1. 现有兼容面

| 模块 | 内容 |
|---|---|
| `compatruntime.rs` | 驱动拦截裁决 / 显示管线协商回退 / 音频管线独占守卫 / 网络协议回退与 VPN 代理共存。**全部确定性算法、固定容量、非法输入钳制** |
| `compatapi.rs` | 能力掩码：图形 1 / 文件 2 / 网络 4 / 输入 8 / 音频 16 |
| `proc/winapi.rs` + `winsrv.rs` + `winmem.rs` | Win32 服务台（第 05 章已详） |
| `wine*` 七模块 | 管控层：`wineshim` / `winelaunch` / `winepfx` / 前缀 / 看门狗 |

**如实标注**：`compatruntime` 的"网络协议回退"（`quic → http2 → http1.1`）是**枚举回退链，不是协议栈**——它决定"该用哪档"，不实现"那一档怎么收发包"。真协议栈是移植线 `smoltcp` 落点的职责（第 08 章候选 #2）。

## 2. Wine 管控层的如实边界

七个 `wine*` 模块全部是**管控层**（前缀管理/启动门/垫片/看门狗）。`winelaunch` 模块文档明写："Wine 用户态运行时未部署进镜像时如实报 Absent"。ISO 根 `build/isoroot`（11MB）当前只有引导文件，无任何运行时——所以现状就是 Absent。

Wine 本体（`wine-11.0`，6,014,074 行 C）交叉编译的硬依赖是 `x86_64-w64-mingw32`。本机 MSVC 19.51 + WinSDK 10.0.26100 完整可用，但 Wine 11.0 `configure` 实测只认 mingw 前缀（`x86_64-pe` 出现 0 次），MSVC 不能替代。`choco install mingw` 两次失败于 `C:\ProgramData\chocolatey\lib` ACL 拒绝。解法三条在第 13 章工具链条目。

## 3. C 环境规划（cenv）

C/C++ 进内核地址空间的前提是一层"内核 libc"（freestanding 环境）。规划结构：

```
kernel/varix/src/cenv/
├── mem.rs       memcpy/memset/memmove/memcmp      ~200 行
├── runtime.rs   __stack_chk_fail/abort/atexit      ~80 行
├── arith.rs     __udivti3/__umodti3/__muloti4…     ~300 行
├── heap.rs      malloc/free → 内核堆包装           ~150 行
├── iostream.rs  printf 家族 → serial::write_bytes  ~120 行
└── cenv.rs      C FFI 入口面
```

约 850 行，**零外部依赖**（不依赖任何 C 编译器即可先写 Rust 侧与判据）。设计依据：

- `main.rs:16` 已是 `extern "C" fn _start()`——内核入口本来就是 C ABI，SysV AMD64 下 C/Rust 直接互通；
- `memmove` 方向判定复用 `unx101_vgafallback.rs:2058` 纯函数真源；
- `float.rs:15` 的教训入档：`extern "C" { fn sqrt(...) }` 编译过、运行崩（GOT 间接调用）——**cenv 不允许"编译过但符号悬空"**，链接期必须对账。

第二层是头文件（`stdint.h`/`stddef.h`/`string.h` 等 freestanding 最小集，~3000–5000 行），第三层是 `linker.ld` 扩展（`.init_array`、保留 `.eh_frame`、`.got`），第四层才是编译器（Zig `zig cc -target x86_64-freestanding` 自带 compiler_rt，或自建 LLVM）。

## 4. freestanding C 的可达范围

有了 cenv，1.42 亿行 C 里能进 ring0 的是 **freestanding 子集**（估算 170–230 万行）：zlib/lz4/zstd/brotli、murmur3/xxhash/blake、mbedtls/tinycrypt/chacha、jpeg/png/webp、opus/flac 核心、freetype/harfbuzz、fribidi、lwIP。实测证据支持这个方向：FFmpeg `libavcodec` 的 `.c` 文件几乎只依赖自有 `libavutil/mem.h`（memcpy 包装），不碰系统 libc；wine kernel32 的 `.h` 里 `stdio/pthread/unistd` 命中为 **0**。

进不了 ring0 的（如实）：带 OS 抽象层的（V8/JVM/Qt/GTK）、C++ 异常/RTTI（`linker.ld:58` 丢 `.eh_frame`）、需 MMU 隔离的（DB 引擎）。它们走 L5 层 ring3 PE（第 08 章七层表）。

## 5. 判据对接

兼容层每个组件都有判据锁定：

- `compatruntime.rs:258` X08178 档位矩阵 / `:262` X08182 未知模式回退 / `:292-293` 音频失败叙事与空表回退 / `:322` X08232 网络未知协议回退 / `:340` X08250 收官复核；
- `winapi.rs:488` 三态计数对账 / `:499` stub 数锁死 / `:557` stub 与缺号必须 Enosys / `:647` 绑定计划安装计数对账（`(2,1,0)`）。

新增兼容面（cenv、smoltcp、字体栈）沿用同一模式：行为进 `CheckSet`，对账进元判据。

---

# 10 · 已知边界（未完成事项清单）

> 本章是全 README 的诚实核心。每条带文件行号与状态。这里没有的东西，别处不许声称有。

## A · 结构瓶颈（阻塞后续工作的）

| # | 事项 | 位置 | 影响 | 解法 |
|---|---|---|---|---|
| 1 | `MAX_SYSCALLS = 64`，Win32 段 `WIN32_NR_BASE = 0x40 = 64` 起 | `proc/syscall.rs:10`、`proc/winapi.rs:36` | **Win32 可用号位 0 个**，API 一个都加不进去 | 扩到 4096 + 编译期断言（~300 行，P0） |
| 2 | PE 无重定位 | `proc/pe.rs:626-627`（原文："pe.rs 不做重定位：静态映像按首选基址装载；任务40 引入动态映像时再上 reloc"） | 基址冲突/ASLR 场景装不下 | `.reloc` 解析 + 增量修正（~400 行） |
| 3 | `MAX_IMAGE_SIZE = 256 MiB` | `proc/pe.rs:32` | 大型 DLL（mesa/Chromium 级）装不下 | 提级 4GiB + 分页映射（~100 行） |
| 4 | 导入容量 `MAX_IMPORT_DLLS = 8` / `MAX_IMPORT_FUNCS = 64` | `proc/pe.rs:329,331` | 真实大型应用导入 40+ DLL，装不下 | 扩容 + 动态分配 |
| 5 | 中文字形为零 | `proc/../font.rs:8`（95 ASCII）、`font.rs:116`（非 ASCII 返空白格） | 桌面显示不出一个汉字 | 字体栈：skrifa/ttf-parser 轮廓 + HarfBuzz 整形（~1 万行） |
| 6 | `linker.ld:58` 丢弃 `.eh_frame*` | `linker.ld:56-61` | C++ 异常/RTTI/展开表出局 | 改保留（一行），需配套展开运行时 |
| 7 | `.init_array` 缺失 | `linker.ld` | C++ 静态构造不执行 | 加 section（~10 行） |

## B · 未复验的已完成工作

| # | 事项 | 状态 | 风险 |
|---|---|---|---|
| 11 | `ckport.rs`（bytemuck 落点，13 条判据） | 代码完，判据**未复验**（2026-10-12 编译被构建锁阻塞） | 可能存在编译错或判据红，未知 |
| 12 | `winmem.rs`（712 行，VirtualAlloc/Free/GetLastError/CreateFileW/NtAlloc） | 代码完 + 判据文本齐，**未复验** | 同上 |
| 13 | 207 仓 C/C++ 判定脚本 | 已提速改造（抽样估行 + 断点友好），**最终结果未回传** | 无 |
| 14 | bfport 红项修复（raw 改 `0b10_1010`） | 已修改，**修复后的复验结果未确认** | 低（归因明确） |

## C · 能力缺口（按第 06 章展开的 50 条归类）

| # | 缺口 | 依据 |
|---|---|---|
| 21 | 真文件系统缺失——`VFiles` 是内存槽，重启丢数据 | `winsrv.rs` |
| 22 | 真网络栈缺失——协商算法不是协议栈 | `compatruntime.rs:3-4,181` |
| 23 | 真解码器缺失——音频是合成音 | `audio/tone.rs` |
| 24 | SecureBoot 无密码学——只有结构面 | `secureboot.rs:263` |
| 31 | 真随机数、TPM、MTE、常数时间实现均无 | 全仓 grep |
| 39 | HDR 元数据、ICC 色彩管理无 | `cgm03_edid.rs` |

## D · 工具链缺口

| # | 事项 | 状态 |
|---|---|---|
| 41 | `x86_64-w64-mingw32` 缺失 | `choco install mingw` 两次失败于 ACL 拒绝；MSVC 19.51 + WinSDK 10.0.26100 在位但不能替代 Wine 构建 |
| 42 | `X:\OpenSource\zig` 仓残缺 | README 写 "This repository is not mirrored."，`lib/` 仅 5 文件；完整 Zig 需从 Codeberg 重拉 |
| 43 | clang 缺失 | `llvm-project`（17.9M 行）源码在盘，可自建，耗时长 |

## E · 判据体系自身的已知问题

| # | 事项 | 依据 |
|---|---|---|
| 51 | 全量基线 26 条红未清 | 2026-10-12：13689 passed / 26 failed |
| 52 | 域聚合判据曾只报 "N/M 绿" 无红项名单 | 已用 `red_items_text()` 统一修复，新判据须遵守 |
| 53 | `winapi.rs:499` 锁 `stub == 6`，填 Stub 需连判据同步改 | 设计如此（防账面漂移），但修改时易忘 |

## F · 明确不做（口径声明）

- **不以源码形式把 1.42 亿行 C 编进内核**——物理不可能（无 libc 的 `no_std` + C++ 异常被 DISCARD）；承诺的是"功能可用"（七层落法，第 08 章）。
- **不冒充兼容**——`ApiStatus::Stub` 返回 Enosys 且计数，`winelaunch` 未部署时报 Absent。诚实的不支持优于虚假的兼容。
- 兼容率目标 **90%**（总案口径），不承诺 100%：反作弊/加密狗/闭源 GPU 需 e-pod 兜底（总案第七篇 7.2），e-pod 不计入本体兼容率。

---

# 11 · 工程纪律

> 本章是提交前必读。六条纪律全部来自实战事故，每条有事故原型。

## 纪律一：条件 `no_std` 只在 crate 根声明

```rust
#![cfg_attr(not(test), no_std)]
```

写在 `lib.rs` 顶部一次。**内层模块再写 `#![no_std]` 是编译错**（属性重复合法性）。bfport 初版踩过。

## 纪律二：`alloc` 三件套自查

进内核链接的模块若用堆，必须带齐：

```rust
use alloc::vec;                    // 宏
use alloc::vec::Vec;               // 类型
use alloc::string::{String, ToString};
```

**宏与同名类型是两套东西**，缺任一项编译即炸；`alloc` 根下没有 `String`/`Vec`/`ToString` 这三个裸名。

## 纪律三：新模块必须进总闸注册表

两步缺一不可：

1. `lib.rs` 加 `pub mod xxx;`；
2. `robust.rs` 的 `domains: [fn() -> CheckSet; 354]` 数组加一行，**长度同步 +1**（E0308 会强制，但若先改长度忘加行会挂别处）。

**没进注册表的 `run_*_checks()` 永不执行**——编译绿 ≠ 判据绿。且补注册后，此前所有 `cargo check`/`test` 结果作废，全新量重跑。

## 纪律四：单源判定，不许旁路

- 重叠拷贝方向 → `unx101_vgafallback.rs:2058` 纯函数；
- 用户内存读写 → `winsrv::read_user_bytes` / `write_user_bytes`；
- 三态统计 → `winapi::coverage()`；
- 判据聚合红项名单 → `checks::red_items_text()`。

新实现要么复用真源，要么对拍判据证明一致。死抽象直接删（`BfBits` trait 无使用者，删除）。

## 纪律五：版本锁定与缓存卫生

- 工具链锁 Rust **1.97.1**。共享 `target/` 曾出现多版本缓存污染（`libm` 元数据被 1.98.1 编译）——删缓存重跑，不改锁；
- 构建产物（`nul.*` 等）是显式 `-o` 的产物，不改共享 Cargo 配置；
- 编译错误被海量 warning 淹没时：完整落盘 → `tail`/grep 提取，**禁止在流式输出里人眼捞**；
- 并行 cargo 抢锁会假死：先清进程（`taskkill //F //IM cargo.exe //T`），单跑。

## 纪律六：判据与实现同账

- 改 `winapi::API_TABLE` 必须同步改 `winapi.rs:499` 的 `stub == 6` 锁——这是刻意的强制同步点；
- 注释承诺的性质（"当前/最新/有效"），代码里必须有对应的真源比较（`SignBook` 事故：比了副本没比真源，首签恒放行）；
- "标准环境/夹具"构造若被 `.expect()` 包裹，必须验证真的能构造成功（`vea54` 事故：40+ 判据被一个必失败的构造挡在门外）。

## 纪律七：中文诊断、锚点注释

- 判据标签、模块文档、失败叙事一律中文；标识符与路径英文；
- 引号规范：代码/配置/命令里只用 ASCII 直引号（U+0022/U+0027），散文里用中文引号；
- 每个移植落点头部写明：上游名/版本/取舍（移植什么不移植什么）。

## 纪律八：仓库卫生

- 无用内容删除或归档 `attic/`（历史构建产物在 `attic/build_artifacts_2026-10-12/`）；
- 深化预写稿 `.deep_*.json`（105 个）已合并进 VE 正本，惯例保留勿删；
- `boot-select.json`、`limine.conf` 不可删；
- `kernel/.workbuddy/` 是项目数据目录，不是缓存，不删。

## 纪律九：如实汇报

- 未复验的代码不得声称"全绿"；
- 估算必须标注"估算"；实测必须带出处（文件：行号 / TSV / 命令输出）；
- 判据红 ≠ 失败：先归因（判据错 or 实现错），修判据不是降强度。

---

# 12 · 构建与测试

## 1. 三个命令

`kernel/.cargo/config.toml` 定义别名：

| 别名 | 展开为 | 用途 |
|---|---|---|
| `cargo kbuild` | `build --target x86_64-unknown-none --release --features kernel-image` | 内核镜像 ELF |
| `cargo ktest` | `test --target x86_64-pc-windows-msvc` | 宿主全量判据 |
| `cargo kcheck` | `check --target x86_64-unknown-none --features kernel-image` | 真机 target 静态检查 |

镜像 bin（`varix/Cargo.toml:13-19`）`required-features = ["kernel-image"]`，test/bench/doctest 全关——内核 bin 不参与宿主测试，宿主测试跑 lib。

## 2. 全量测试基线

```
cargo ktest
# 2026-10-12 实测：13689 passed / 26 failed，约 364 秒
```

- 26 条红的归因与状态见第 10 章 B/E 组；
- 单域过滤：`cargo test --lib <域关键词>`；
- 两个移植落点：`cargo test --lib -- bfport ckport`。

## 3. 栈预算

`.cargo/config.toml:14-15`：

```toml
[env]
RUST_MIN_STACK = "16777216"   # 16 MiB
```

原因（注释原文）：判据总闸是栈上定长数组 `[Option<CheckSet>; MAX_DOMAINS]`，WP-301 后 128 容量 ≈ 331KB，F475 leaf 条目与闭环嵌套两份 ≈ 662KB——默认 2MiB 测试栈在域数增长后触底（f475 `STATUS_STACK_OVERFLOW`）。**16MiB 只影响宿主测试进程，kbuild 内核镜像路径不读此变量。** 域数组扩到 354 后，此值需复测。

## 4. 依赖

| 依赖 | 版本 | 用途 | 纪律 |
|---|---|---|---|
| `libm` | 0.2 | 内核浮点层（musl 纯 Rust 移植） | 与 std 精度对拍：16 种方法逐位一致、`powf`/`exp` ≤1 ULP；判据 `kernel_math_matches_std` 持续复核（`float.rs:37`） |
| `blake3` | 1 | dev-dep，任务 35 测试交叉验证 | 仅宿主，不进镜像 |

**零 C 运行时纪律**：`x86_64-unknown-none` 不链 libc（`rust-lld` + `linker.ld`），任何需要 C 运行时的东西不得进依赖。`extern "C" { fn ... }` 声明外部符号会产生 GOT 间接调用——编译过、运行崩（`float.rs:15` 反汇编实证 `callq *%rax`），**禁止用声明外部 C 函数的方式"借用" libc**。

## 5. 工具链

| 工具 | 版本/状态 |
|---|---|
| Rust | 锁 1.97.1（缓存污染事故后铁律） |
| rust-lld | 系统随 rustc |
| 目标 `x86_64-unknown-none` | rustup 已装 |
| MSVC 19.51 + WinSDK 10.0.26100 | 在位（宿主测试链接用） |
| mingw-w64 | **缺**（第 10 章 #41） |

## 6. ISO 与 QEMU

```bash
python _tools/make_iso.py          # 组装 ISO（pycdlib 在隔离 venv）
python _tools/qemu_boot_verify.py  # QEMU 引导验证（串口对练）
```

- ISO 根：`build/isoroot/`（仓库根，不是 `kernel/build/`——历史上误判过）；
- `boot-select.json`、`limine.conf` 是启动关键文件；
- `nul.*` 等散落产物是显式 `-o` 的产物，与共享 Cargo 配置无关。

## 7. 输出处理纪律

- 编译输出先完整落盘再 grep：`cargo test ... > /tmp/log 2>&1; tail -30 /tmp/log`；
- 过滤模式避开 warning 洪流：`grep -vE "^\s+\||^\s+-->|^note:|^help:"`；
- 后台长任务（全量冷 target 约 6–15 分钟）用 run_in_background，**不要并行 cargo**（构建锁互斥会假死）。

## 8. 验收口径

- "编译通过" = `cargo kcheck` 0 error（warning 不拦）；
- "判据绿" = `cargo ktest` 全量 + 指定域红项为空；
- "落点存在" = 代码完 + 判据完 + **注册表已入** + 全量复验红项为零，四者缺一不可。

---

# 13 · 路线图

> 本册只列**已定案、已算过账**的事。每个阶段给出：做什么、为什么是它、改动落点、验收口径。不画的饼不写在这里；实现中的状态一律以第 10 章《已知边界》为准。

---

## 阶段一 · P0 三项解锁杠杆（合计约 800 行）

移植线现在卡在三个物理天花板。它们互不依赖，可以并行，但建议按 1→3→2 的顺序做（1 最小，2 最大）。

### P0-1 · 系统调用号位扩容

- **现状**：`kernel/varix/src/cpu/syscall.rs:10` 写死 `MAX_SYSCALLS = 64`。Win32 服务台 33 项已占用过半，cenv 与用户态 libc 一上来就要一次性占掉一批号位。
- **改动落点**：`syscall.rs` 号位表从定长 64 扩到 256 或 1024；派发函数从数组直查改为范围分支或二段表；`ring3.rs` 的 `syscall` / `int 0x80` 双入口同步。
- **量级**：约 100–150 行 + 判据更新。
- **验收**：新增判据"号位无冲突"（静态遍历注册表查重）+ 既有 33 项 Win32 判据全绿 + 双入口回归。

### P0-3 · PE 重定位

- **现状**：`kernel/varix/src/proc/pe.rs:626` 无重定位处理——映像必须落在首选基址，基址被占即装载失败。
- **改动落点**：解析 `.reloc` 节，按 `IMAGE_BASE_RELOCATION` 块逐条改写；装载器在首选基址不可用时走重定位路径；判据用合成 `.reloc` 块验证地址改写的正确性。
- **量级**：约 200–300 行。
- **验收**：合成的带 `.reloc` 的 PE 判据（首选基址可用时走直装、不可用时走重定位、改写后导入地址一致）三条全绿；ASLR 式随机基址装载成功。

### P0-2 · 映像上限扩容

- **现状**：`kernel/varix/src/proc/pe.rs:32` 写死 `MAX_IMAGE_SIZE = 256MiB`。 freetype / icu 级别的库静态链入后单映像会逼近或超过这个数。
- **改动落点**：上限改为配置量（编译期常量 + 判据锁定当前值）；同时复核物理页分配器能稳定供应大块连续/可映射区间。
- **量级**：约 50–100 行改动，大头在内存侧复核。
- **验收**：装载一个 >256MiB 的合成映像判据通过；内存账本（分配/释放计数）在失败路径不泄漏。

**为什么这三项先行**：它们是后续一切"把大东西装进来"的前置。没有号位，cenv 的系统调用没处注册；没有重定位，任何非平凡映像都可能装不上；没有上限，2MiB 级别的库链入即撞墙。800 行换掉三个天花板，是当前投入产出比最高的改动。

---

## 阶段二 · 判据复验（欠账清零）

- **范围**：`winmem.rs`、`ckport.rs` 等受构建锁阻塞未复验的判据，共 10 项左右待重跑；26 条全量红项按第 10 章 B/E 组归因逐条清账。
- **纪律**：复验前先读第 7 章"红项归因纪律"——先问"判据红还是实现红"。46 红批次的教训：只有 4 条是真实现缺陷，其余全是判据自身写错（写死数字漂移 / 期望值算错 / 样本超容量 / 挑了绕开被测路径的策略）。
- **工具**：失败路径一律用 `CheckSet::red_items_text()`（`checks.rs`）生成"标签(明细)"名单，不做折叠式二分重跑。
- **验收**：`cargo ktest` 全量 0 failed；`run_kernel_checkup()` 注册表与 354 域一一对应。

---

## 阶段三 · 中文字体栈

- **现状**：`kernel/varix/src/font.rs:8` 仅 95 个 ASCII 字形（8×8 等宽点阵）。这意味着界面只能显示英文——对中文用户这是第一优先级的体验缺口。
- **落法（四步）**：
  1. **TTF/OTF 解析器**：glyf/cmap/hmtx 最小集（读字形轮廓 + Unicode 映射 + advance width），Rust freestanding 实现，不依赖 std；
  2. **字形栅格化**：二次贝塞尔轮廓 → 扫描线填充 + AA，复用 `aurora/render2d.rs` 已有的抗锯齿能力；
  3. **CJK 码位覆盖**：优先从"思源黑体/文泉驿"级开源字库提取 GB2312 一级 3755 字子集，字库资产走 L7（资产层），不进内核映像，由引导/装载侧提供；
  4. **排版增强**（后置）：等宽假设破除后，文本度量接口从"像素宽 = 字形数 × 8"升级为按 advance 累加。
- **验收**：判据覆盖 cmap 查找（含 BMP 外）、轮廓栅格化与参考渲染对拍、字宽度量一致性；桌面端用中文渲染一个静态窗口文本并通过视觉判据。

---

## 阶段四 · cenv（C 兼容环境）

C 代码本身可以 freestanding 编译进 ring0，真正的障碍是 libc。cenv 用四层 shim 替代 libc，让 C 库源码不改或极少改地进内核：

| 层 | 内容 | 量级 |
|---|---|---|
| 1. mem | `memcpy/memset/memmove/memcmp/malloc/free` 对接到内核已有的堆与页分配器 | ~120 行 |
| 2. runtime | `strlen/strcmp/atoi/printf` 最小集；`printf` 走内核串口与 aurora 文本通道 | ~250 行 |
| 3. arith | 64 位除法/浮点软启动桩（x86-64 上多数已由硬件指令覆盖，保留兜底） | ~200 行 |
| 4. 头文件 | `stdint.h/string.h/stdlib.h/stdio.h` 等 freestanding 子集 | ~4000 行 |
| 5. 链接 | `linker.ld` 扩展 ~50 行（见第 15 章 C++ 一节） | ~50 行 |

- **验证门**：需要编译器（见阶段六 D 组）。在编译器在位前，cenv 只写代码与判据（判据可在宿主 target 跑，用 Rust 侧镜像实现对拍）。
- **首批目标库**（可进 ring0 的 C 库估算 ≈ 170–230 万行，明细见第 14 章）：zlib、lz4、murmur3、freetype、harfbuzz、icu 子集、lwIP。
- **验收**：zlib 静态链入后在内核态完成一次 deflate/inflate 往返，与宿主 zlib 输出逐字节一致（判据锁定）。

---

## 阶段五 · Win32 服务台五阶段（17–20 万行）

- **现状**：`proc/winapi.rs:84` 注册 33 项（Full 18 / Partial 9 / Stub 6）。
- **五个阶段**：
  1. **补 Stub → Partial**：把 6 个 Stub 中有真实语义空间的先升级（文件、内存、同步原语优先——它们直接对接内核已有子系统）；
  2. ** KERNEL32 深化**：进程/线程/内存管理/文件 IO 全语义，判据从"调用不返回 Enosys"升级到"语义与 Windows 文档行为一致"；
  3. **USER32/GDI32 基础面**：消息循环、窗口注册、DC、位图——对接 aurora 已有的窗口/2D 能力；
  4. **PE 装载生态**：P0-3 重定位就绪后，装载真实第三方 PE 并统计导入满足率，作为兼容率的实测口径；
  5. **注册表/服务/句柄命名空间**：`winreg` 式配置存储与句柄生命周期统一账本。
- **验收口径**：每阶段一个"真实 PE 用例集"通过率数字（用第 14 章的兼容性实测口径），不报"API 数量"这种虚数。

---

## 阶段六 · 工具链自建（D 组）

编译器不在位，cenv 的验证门开不了。两条腿：

- **D-1 · Zig 工具链重拉**：盘上 `zig/lib/` 残缺（仅 5 个文件，仓库 README 自证 `This repository is not mirrored.`），需从 Codeberg 官方镜像重拉完整 Zig；Zig 自带 clang（`zig cc` / `zig c++`），可以交叉编译 freestanding C/C++ 到 `x86_64-unknown-none`，一条工具链同时解决 C 与 C++ 的编译器问题。
- **D-2 · clang/LLVM 直装备选**：MSYS2 或 LLVM 官方预编译包（此前 choco 装 mingw 两次被 ACL 拒、便携包下载 22 分钟无进度已放弃，均记录在案；需要用户在管理员终端操作，或等 D-1）。
- **cargo 集成**：build.rs 或 `_tools/` 外部脚本调用 `zig cc -target x86_64-freestanding-none -ffreestanding -nostdlib`，产物以 `.o`/`.a` 形式参与链接；`linker.ld` 增补 C/C++ 需要的 section（见第 15 章）。
- **验收**：一个手写 `hello.c`（调 cenv 的 printf 到串口）经 `zig cc` 编译、链入内核、真机串口输出，全程判据锁定。

---

## 阶段七 · 图形栈与前端能力的真实落法

浏览器引擎不整装进内核（Chromium 约 2500 万行 C++，超出 ring0 承载力，账见第 14 章），但**渲染与样式能力可以按组件提取**：

- **Skia / ANGLE**：盘上已有 angle 源码 1,580,020 行。ANGLE 的 GLES→后端翻译层对接 aurora 的 2D/未来的 3D 能力；
- **CSS 引擎**：Servo（Rust，约 107 万行）的 style 与 layout 组件可提取量约 25 万行，纯 Rust、`no_std` 化改造成本相对最低——这是 Varix 界面获得 CSS 布局能力的**主路径**；Blink 的 CSS 部分作为 C++ 路线备选（依赖 cenv + `.eh_frame` 扩展成熟后）；
- **落点**：全部作为 L4/L5 层组件按第 8 章移植线流程进内核，不引入网络栈依赖（HTML/CSS 解析与渲染先行，网络后置）。
- **验收**：用提取后的 style+layout 渲染一个含 flex 布局的静态页面，与参考实现布局对拍（允许亚像素误差）。

---

## 阶段排序与依赖图

```
P0-1 号位 ─┐
P0-3 重定位 ─┼→ cenv 验证门开 → 首批 C 库（zlib…）→ freetype/harfbuzz → 中文字体栈加速
P0-2 上限 ─┘
判据复验（随时并行，不占主线）
D-1 Zig 重拉 ──→ cenv 编译器在位
Win32 五阶段 ←── P0-3（阶段四依赖重定位装真实 PE）
CSS/style 提取 ←── 中文字体栈 + 判据体系稳定
```

**每阶段通用验收口径**（沿用第 12 章第 8 节）：编译过（kcheck 0 error）+ 判据绿（ktest 全量 + 指定域红项为零）+ 注册表已入（`run_kernel_checkup()`）+ 落点复验，四者缺一不可。路线图上的任何条目在四条不齐时不算完成——第 10 章如实记账。

---

# 14 · 附录 · 实测数据

> 本册收录 README 体系引用的全部关键数字及其出处与测量口径。所有数字标注测量日期；无法精确测量的明确标注"估算"。**口径先行**：数字只有在口径一致时才可比较，本册第一节先立口径，再看数字。

---

## 1. 测量口径约定

- **行数**：`wc -l` 级统计，含注释与空行。纯逻辑行需另测，本册不冒充。
- **"已进内核"**：代码进入 `kernel/varix/src/` 且判据已注册进 `run_kernel_checkup()` 注册表且全量复验红项为零。只写完代码不进注册表的不算（编译绿 ≠ 判据绿）。
- **"估算"**：按已落点单模块实际行数外推的数字，标注区间，不给假精度。
- **日期**：所有实测数字标注测量日；README 体系引用时以本册为准。

## 2. 内核本体实测（2026-10-13）

| 项 | 数字 | 出处 |
|---|---|---|
| 内核源码文件 | 1000+ 个 `.rs` | `kernel/varix/src/` 目录实数 |
| 判据域 | 354 | `robust.rs` 域数组长度 |
| 全量测试基线 | 13689 passed / 26 failed | `cargo ktest`，2026-10-12，约 364 秒 |
| ring3 代码量 | 1567 行 | `proc/ring3.rs` 文件实数 |
| PE 装载器 | 895 行 | `proc/pe.rs` 文件实数 |
| Win32 服务台 | 33 项（Full 18 / Partial 9 / Stub 6） | `proc/winapi.rs:84`，判据锁死 stub==6 |
| 内核依赖 | 仅 `libm` 0.2（dev-dep blake3 不进镜像） | `varix/Cargo.toml` |
| 链接脚本 | 62 行，5 section + DISCARD | `kernel/varix/linker.ld` |
| 字体 | 95 个 ASCII 字形（8×8 等宽点阵） | `font.rs:8` |
| 移植落点 | bfport（bitflags 2.13.2）、ckport（bytemuck 1.25.2） | `src/bfport.rs`、`src/ckport.rs` |
| 用户态 | `userspace/varix-std/` libc 雏形（no_std，syscall0_raw） | 目录实数 |
| 桌面环境 | aurora 18 模块，纯软件 2D 全链路 | `aurora/render2d.rs` 模块文档 |

## 3. 物理天花板实测（当前生效值）

| 天花板 | 值 | 位置 | 解锁条目 |
|---|---|---|---|
| 系统调用号位 | `MAX_SYSCALLS = 64` | `cpu/syscall.rs:10` | 路线图 P0-1 |
| PE 映像上限 | `MAX_IMAGE_SIZE = 256MiB` | `proc/pe.rs:32` | 路线图 P0-2 |
| PE 重定位 | 无（首选基址直装，基址被占即失败） | `proc/pe.rs:626` | 路线图 P0-3 |
| 字形覆盖 | 95 ASCII，无 CJK | `font.rs:8` | 路线图阶段三 |
| `.eh_frame` | DISCARD 丢弃（C++ 异常/unwind 不可用） | `linker.ld:56-61` | 第 15 章 C++ 扩展 |

三项 P0 合计约 800 行改动，明细与验收口径见第 13 章阶段一。

## 4. U 盘实体仓盘点（2026-10 实测）

对 U 盘 **188 个实体仓**逐仓统计源码行数：

| 语言 | 行数 |
|---|---|
| C / C++ | ≈ 142,000,000 |
| Rust | ≈ 20,400,000 |
| Python | ≈ 4,500,000 |
| 其他（JS/TS/Zig/资产等） | ≈ 182,777 |
| **合计** | **≈ 167,082,777** |

**三条口径说明（引用这个数字前必读）**：

1. 这是**理论原料总量**，不是"已进内核的量"。已进内核的是其中被移植/重写/借用的部分（第 8 章移植线 + bfport/ckport 两个已落点）。
2. 1.67 亿行里大量代码与内核场景无关（应用源码、文档、测试夹具、构建产物）。"全部进内核"的真实含义是第 5 节七层落法中**各层各取所需**，不是把 1.67 亿行灌进 ring0——ring0 的承载力本身是有限资源，这也是判据域制度存在的原因。
3. 统计含注释与空行；如需纯逻辑行口径，需另测并在此登记。

## 5. 七层落法映射

| 层 | 名称 | 承接内容 | 当前实例 |
|---|---|---|---|
| L1 | 内核本体 | 调度、内存、中断、装载、判据体系 | 现有 1000+ 文件、354 域 |
| L2 | 驱动 | GPU/输入/存储驱动 | cgpu 域（50 判据） |
| L3 | 内核算法 | 压缩/哈希/排序/校验/字体算法 | zlib、lz4、murmur3、freetype（规划） |
| L4 | 系统服务 | Win32 服务台、网络栈、配置存储 | winapi.rs 33 项、lwIP（规划） |
| L5 | 应用运行时 | PE 装载、cenv、用户态 libc | pe.rs、varix-std |
| L6 | 工具链 | 构建期工具（**不进镜像**） | Zig/clang（规划 D 组）、make_iso.py |
| L7 | 资产 | 字库、图标、测试夹具 | CJK 字库子集（规划） |

**规则**：每个外来项目入内核前先定层；定不进七层的说明还没想清楚，不进。L6 永远不进镜像——工具链是造内核的，不是内核的一部分。

## 6. 三条移植路径估算（最终目标量级，估算）

| 路径 | 内容 | 估算量 |
|---|---|---|
| T1 | Rust 重写（逐模块翻译外来项目语义） | 80–120 万行 |
| T2 | C→Rust freestanding 移植（保持 C 结构，去 libc 依赖） | 150–250 万行 |
| T3 | Win32 API 面补全（shim 语义，非移植） | 17–20 万行 |

估算口径：按已落点（bfport/ckport）单模块实际行数外推，含判据行。**这是最终目标的量级，不是当前事实**；当前内核本体在 36–71 万行量级（视是否含判据与测试代码的口径浮动）。T1/T2 的划分标准：外来项目本身是 Rust 的优先直接借用（T1 的近亲）；C 项目看结构质量，结构好的走 T2 原样移植，结构差的走 T1 重写。

## 7. 可进 ring0 的 C 库清单（cenv 就绪后首批）

| 库 | 用途 | 量级 | 落层 |
|---|---|---|---|
| zlib | 压缩 | ~10 万行 | L3 |
| lz4 | 快速压缩 | ~1 万行 | L3 |
| murmur3 | 非加密哈希 | ~千行 | L3 |
| freetype | 字体解析/栅格化 | ~30 万行 | L3（字库资产在 L7） |
| harfbuzz | 文本整形 | ~20 万行 | L3 |
| icu 子集 | Unicode/编码转换 | ~50–100 万行（可裁） | L4 |
| lwIP | TCP/IP 栈 | ~10 万行 | L4 |

**合计 ≈ 170–230 万行**。这是 cenv 四层 shim（路线图阶段四）直接解锁的量，不含 Rust 生态。首批验收锚点：zlib 在内核态完成 deflate/inflate 往返，与宿主 zlib 输出逐字节一致。

## 8. 浏览器引擎实测账

| 项 | 数字 | 结论 |
|---|---|---|
| Chromium 全量 | ≈ 2500 万行 C++ | **不可整进 ring0**——超出内核承载力，且强绑完整网络栈/多进程/沙箱模型 |
| angle（盘上已有） | 1,580,020 行 | GLES→后端翻译层可对接 aurora 渲染能力 |
| Servo（Rust） | ≈ 107 万行 | style + layout 组件可提取约 **25 万行**，纯 Rust、`no_std` 化改造成本最低，是 CSS 布局能力的**主路径** |
| Blink CSS | Chromium 子集 | C++ 路线备选，前置依赖：cenv 成熟 + `.eh_frame` 扩展 |
| Edge / Chrome 闭源部分 | — | **不碰**。依据总案 7.6 法律红线：不逆向闭源浏览器二进制；开源替代按上文路径走 |

HTML/CSS/JS 的完整落法（含其他语言运行时的边界）见第 15 章。

## 9. 兼容性现实账（对 Windows 而言）

| 项 | 数字 | 出处 |
|---|---|---|
| 总案自认兼容上限 | 88–93% | 《Windows软件90%兼容总案》第七篇 |
| e-pod（不可兼容尾差） | ≤8%，年降 1.5% | 总案让步阶梯 |
| Win11 原生兼容 | 100%（定义使然） | 基准本身 |

结论如实写：**Varix 永远不会在"兼容率"这一个维度上超过 Win11**——Win11 是兼容性的定义基准，任何其它系统对它的兼容率不可能超过 100%。Varix 可以争取的是四个**结构性差异维度**的胜出：

1. **零遥测**：内核全源码在仓库，无任何回传通道，判据体系本身就是审计工具；
2. **零捆绑**：依赖只有 libm 一个 crate，没有广告/服务/账号体系的强制关联；
3. **可审计**：354 个判据域 + 13689 条判据构成机器可验证的行为快照，任何改动过判据闸；
4. **老硬件**：纯软件 2D 起步（不强制 GPU），Limine + x86-64 的引导面比现代 Windows 的硬件要求低。

这四条是设计立场带来的结构性收益，不是兼容率竞赛的安慰奖。90% 兼容目标写在总案里，进度以第 13 章阶段五的"真实 PE 用例集通过率"为实测口径，两处都不掺水。

## 10. 数字引用纪律

- README 任何章节引用数字，以本册为唯一出处；本册没有的不引用；
- 实测数字更新时先改本册、再改引用处，保持单一真源；
- 引用估算数字必须带"估算"字样与区间，不许去掉区间只取上限——"取区间上限当事实"是夸大，本项目不夸大。

---

# 15 · 语言与生态扩展

> 本册回答三个高频问题：**C/C++ 怎么进内核？其他语言呢？HTML/CSS/浏览器引擎呢？** 答案全部基于内核现状（第 14 章实测数字），不基于愿望。

---

## 1. 现状：内核是纯 Rust，零 C 运行时

- 依赖仅 `libm` 0.2（`varix/Cargo.toml`），`x86_64-unknown-none` 目标不链 libc（`rust-lld` + `linker.ld`）；
- **零 C 运行时纪律**（第 12 章第 4 节）：`extern "C" { fn ... }` 声明外部符号会产生 GOT 间接调用——编译过、运行崩（`float.rs:15` 反汇编实证 `callq *%rax`）。禁止用声明外部 C 函数的方式"借用" libc；
- 这条纪律不与"C 代码进内核"矛盾——矛盾的是**C 运行时（libc）**，不是 C 语言。

## 2. C 为什么进得来

C 本身是 freestanding 友好的：`-ffreestanding -nostdlib` 下，不碰 libc 的 C 程序可以编译为纯机器码。障碍只在 libc 提供的四类东西：

1. **内存**（malloc/free）→ 内核已有堆与页分配器，shim 对接即可；
2. **字符串/运行时**（strlen/printf/atoi）→ 最小集重实现；
3. **算术兜底**（64 位除法、软浮点）→ x86-64 硬件指令覆盖大半，留兜底桩；
4. **头文件**（stdint.h 等 freestanding 子集）→ 纯声明，无实现。

这四类就是 **cenv**（C 兼容环境）的内容，四层设计、量级与验收口径见第 13 章阶段四。cenv 就绪后，第 14 章第 7 节的 ≈170–230 万行 C 库（zlib/lz4/freetype/harfbuzz/icu 子集/lwIP）即可按第 8 章移植线进内核。

## 3. C++ 的额外门槛与三步扩链

C++ 在 C 之上多三样运行时需求，当前 `linker.ld` 全部不支持：

| 需求 | 现状 | 扩展 |
|---|---|---|
| `.eh_frame` / unwind | `linker.ld:56-61` DISCARD 丢弃 | 保留并给段地址；panic-abort 模式可先不接 unwind，但段必须保留否则链接报错 |
| `.init_array` / `.fini_array` | 未收集 | 增补 section 与 `__init_array_start/end` 符号，内核启动时遍历调用（静态构造函数） |
| itanium ABI 辅助 | 无 | `__cxa_pure_virtual` 等最小桩；异常全开需完整 unwind——**第一版只支持 `-fno-exceptions` 的 C++**，STL 容器/模板可用，throw 不可用 |

三步扩链合计约 50–80 行 `linker.ld` 改动 + ~200 行运行时桩。完成后：STL 容器、模板、虚函数可用；异常/RTTI 待 unwind 桩成熟后单独立项。**验收**：一个带静态构造函数与 `std::vector` 的 C++ 测试文件经编译链入内核并正确运行，判据锁定。

## 4. 工具链自建（编译器从哪来）

cenv 的验证门需要编译器（第 12 章工具链表：mingw 缺、choco 两次被 ACL 拒、LLVM 便携下载 22 分钟无进度已弃）：

- **D-1 主路径 · Zig**：盘上 `zig/lib/` 残缺（仅 5 文件，仓库自证 `This repository is not mirrored.`），从 Codeberg 官方镜像重拉。Zig 自带 clang，`zig cc -target x86_64-freestanding-none -ffreestanding -nostdlib` 一条命令交叉编译 C/C++，一条工具链同时解决 C 与 C++；
- **D-2 备选 · MSYS2/LLVM 预编译**：需用户管理员终端操作（ACL 限制记录在案）；
- **cargo 集成**：build.rs 或 `_tools/` 脚本调用 zig cc 产出 `.o`/`.a`，随内核镜像链接；C 代码的判据在宿主 target 先行（Rust 侧镜像实现对拍），真机验证走 QEMU。

**边界如实**：Rust 仍是内核第一语言；C/C++ 是"被内核承载的租户"，其代码质量与判据覆盖要求与 Rust 一致——租户不享受豁免。

## 5. 其他语言的落法与边界

| 语言 | 落法 | 边界 |
|---|---|---|
| Rust | 原生，第一语言 | — |
| C | cenv + zig cc（上文） | freestanding 子集，无 libc 全量 |
| C++ | 第 3 节扩链 | 第一版 `-fno-exceptions`，STL 容器可用 |
| Python | MicroPython 式嵌入解释器（~数万行 C，走 cenv） | 子集语法；CPython 全量（~百万行 + GIL 语义）不进 ring0 |
| JavaScript | QuickJS 级小型解释器嵌入 | 无 JIT 的解释执行；V8（JIT + 大运行时）不进 ring0 |
| Zig | 编译为 freestanding 目标直链，与 Rust 同等待遇 | 天然友好 |

**判据要求**：每个语言运行时进内核，必须带判据（解释器执行一段固定脚本、输出逐字节对拍）——运行时也是租户，同样过判据闸。

## 6. HTML/CSS/JS 与 Varix 界面

前端三样的分工：

- **JS**：由第 5 节的解释器路径承接（QuickJS 级），服务于界面脚本逻辑；
- **HTML**：解析器（tokenizer + DOM 树）为中等量级纯逻辑组件，Rust 重写或经 cenv 移植 html5ever 系；先行离线渲染（文件输入），网络栈后置（lwIP 就绪后再谈在线页面）;
- **CSS**：布局引擎是最重的一块，主路径是 **Servo 的 style + layout 组件提取（约 25 万行 Rust）**，`no_std` 化改造后作为 L5 组件进内核，对接 aurora 渲染（第 13 章阶段七）。C++ 路线（Blink CSS 子集）作为备选，前置依赖 cenv + `.eh_frame` 扩展成熟。

**为什么这样分工**：Servo 是 Rust 写的，与内核零 C 运行时纪律零冲突；Blink 虽然功能更全但拖家带口（依赖 Chromium 基础设施），提取成本与风险都高一个量级。先用 Servo 组件把 flex/grid 布局能力立起来，是投入产出比最高的顺序。

## 7. 浏览器引擎的完整账

| 问题 | 答案 |
|---|---|
| Chromium 能整装进 ring0 吗 | 不能。≈2500 万行 C++，强绑网络/多进程/沙箱模型（第 14 章第 8 节） |
| 能提取什么 | Skia/ANGLE（盘上 angle 1,580,020 行）→ 渲染；Blink CSS → 备选样式引擎 |
| Servo 能给什么 | style + layout ≈ 25 万行 Rust → CSS 主路径 |
| Edge / Chrome 闭源二进制 | 不逆向、不破解（总案 7.6 法律红线）；开源替代即上文路径 |
| Varix 上能跑浏览器吗 | 终态：能跑一个"Varix 原生浏览器"（Servo 组件 + QuickJS + aurora 渲染 + lwIP 网络），而非移植 Chromium 本体。这是能力等价路线，不是二进制兼容路线 |

## 8. 本册验收总口径

1. C：`hello.c` 经 zig cc 编译、cenv printf 输出、真机串口可见，判据锁定；
2. C++：静态构造 + `std::vector` 用例链入运行正确（`-fno-exceptions` 版）；
3. 语言运行时：固定脚本执行输出逐字节对拍；
4. CSS：flex 布局静态页与参考实现布局对拍（允许亚像素误差）；
5. 以上任一条在四条验收（kcheck / ktest / 注册表 / 复验）不齐时不记完成——沿用第 12 章第 8 节口径。

**一句话总结**：语言不是壁垒，libc 是；浏览器不是壁垒，2500 万行是。拆掉 libc（cenv）、拆整为零（组件提取），C/C++/Python/JS/CSS 都有真实进内核的路，每条路都有判据管着。
