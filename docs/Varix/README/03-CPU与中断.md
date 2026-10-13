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
