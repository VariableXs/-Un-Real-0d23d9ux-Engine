# VARIABLE-200 · 系统调用 ABI 规范（一页纸）

> AI-03 交付物 · 对应 **F051~F075**
> 落点：`kernel/varix/src/syscall/`（内核侧）、`kernel/userspace/varix-std/`（用户侧）
> 状态：ABI 版本 `1`，最低兼容版本 `1`
>
> 本文是**号表的唯一人类可读投影**。内核自检（F074）会断言号表的每一项都满足
> 本文声明的形状（有名字、参数个数不超过 6、能力位在册、快路不参与审计），
> 因此本文与代码漂移会让 `cargo ktest` 变红——而不是等到用户程序跑飞。

---

## 1. 入口与调用约定

| 项 | 值 |
| --- | --- |
| 调用号寄存器 | `RAX` |
| 参数寄存器（顺序） | `RDI` · `RSI` · `RDX` · `R10` · `R8` · `R9` |
| 返回寄存器 | `RAX`（≥ 0 成功；`[-4095, -1]` 为错误，按 Linux 约定取负编码） |
| 被硬件破坏的寄存器 | `RCX` · `R11`（仅 `syscall` 路径；`int 0x80` 路径不破坏） |
| 其余寄存器 | 全部保留（含 FPU/SSE 状态） |

**两条入口**：

- **F051 MSR 路径**：`SYSCALL` → `LSTAR`。需配置 `EFER.SCE=1`、
  `STAR=[63:48]=user_cs, [47:32]=kernel_cs`、`FMASK` 清 `IF/TF/DF/AC/NT/RF/IOPL`。
  入口 stub **必须**先保存 `RCX`/`R11`，否则用户 RIP/RFLAGS 丢失。
- **F052 `int 0x80` 备路**：IDT 向量 `0x80`，DPL=3。语义与 MSR 路径**完全一致**
  （内核自检断言两条路解码出同一个 nr/args/rip/rsp）；差别只有 `RCX`/`R11` 不被破坏。

`RFLAGS` 净化：用户可控位在返回时被掩到 `CF|PF|AF|ZF|SF|OF|DF|IF`，`bit1` 恒置 1。

---

## 2. 号段划分（F053）

| 号段 | 范围 | 用途 |
| --- | --- | --- |
| 保留 | `0x0000`–`0x3FFF` | 永不发放；出现即为调用方 bug |
| **原生** | `0x4000`–`0x40FF` | Varix 调用面（本表） |
| 兼容 | `0x4100`–`0x41FF` | 旧 ABI / Linux 号经此转接（F070） |
| 厂商 | `0x4200`–`0x7FFF` | 实验位与设备扩展 |
| 越界 | `> 0x7FFF` | 直接 `ENOSYS` |

**未注册号一律 `ENOSYS`，且必须在触碰任何参数之前返回**。号表按 `nr` 升序排列，
查找是二分；内核自检断言排序不变式。

---

## 3. 调用面总表（F056~F062 / F069）

| 号 | 名称 | 参数 | 返回 | 能力要求 | 快路 | 审计 |
| --- | --- | --- | --- | --- | --- | --- |
| `0x4000` | `read` | `fd, buf, len` | 字节数（0=EOF） | `CAP_FS` | | |
| `0x4001` | `write` | `fd, buf, len` | 字节数（可短写） | `CAP_CONSOLE` | | ✔ |
| `0x4002` | `open` | `path, len, flags` | `fd` | `CAP_FS` | | ✔ |
| `0x4003` | `close` | `fd` | — | — | | |
| `0x4004` | `exit` | `code`（0..=255） | 不返回 | — | | ✔ |
| `0x4005` | `wait` | `pid`（0=任意） | 状态字 | `CAP_PROC` | | ✔ |
| `0x4006` | `clock_gettime` | `id, tp` | — | `CAP_TIME` | ✔ | |
| `0x4007` | `mmap` | `addr, len, prot, flags, fd, off` | 基址 | `CAP_MEM` | | ✔ |
| `0x4008` | `munmap` | `addr, len` | — | `CAP_MEM` | | |
| `0x4009` | `mprotect` | `addr, len, prot` | — | `CAP_MEM` | | ✔ |
| `0x400A` | `event_ctl` | `port, mask, op` | `port` | `CAP_EVENT` | | |
| `0x400B` | `event_wait` | `port, out, len` | — | `CAP_EVENT` | ✔ | |
| `0x400C` | `pipe` | `out[2]` | — | `CAP_PROC` | | |
| `0x400D` | `dup` | `fd, target` | `fd` | — | | |
| `0x400E` | `send_handle` | `pid, fd, rights` | — | **`CAP_ADMIN`** | | ✔ |
| `0x400F` | `abi_query` | `version, ptr_size` | 打包能力字 | — | ✔ | |
| `0x4010` | `yield` | — | — | — | ✔ | |
| `0x4011` | `log` | `level, buf, len` | — | `CAP_DEBUG` | | ✔ |

**参数个数上限 6**（存储约束）；超出即为 ABI 设计错误，内核自检会拦下。

---

## 4. 参数安全层（F054）

用户指针**永不**被内核直接解引用。所有搬运走同一层判定：

1. 整个 `[ptr, ptr+len)` 必须落在进程声明的用户窗口内（默认为
   `0x1000 ..= 0x7FFF_FFFF_FFFF`）；
2. `ptr + len` **检查加法**，溢出即 `EOVERFLOW`（不允许回绕掩盖越界）；
3. 部分拷贝必须**同时**返回已搬运字节数与错误，调用方不得把半次搬运当成成功；
4. `len == 0` 仅在指针本身合法时放行（使 `write(fd, NULL, 0)` 可用）；
5. NUL 结尾字符串在目标缓冲放不下终止符时返回 `ERANGE`，**绝不**返回未终止的串。

---

## 5. 错误码（F055）

错误集合共 **44** 个变体，声明顺序即编号，编号↔变体是**双射**（自检逐项验证）。
编码取 Linux 约定（负数），Varix 原生扩展 (`quota/seccomp/audit/busyns/denied`)
落在 `-1080..-1084` 的厂商带。

常用项：

| 名 | 值 | 语义 | 可重试 |
| --- | --- | --- | --- |
| `EPERM` | −1 | 能力不足 / W^X 违规 | |
| `ENOENT` | −2 | 目标不存在 | |
| `EAGAIN` | −11 | 队列空 / 速率超额 | ✔ |
| `ENOMEM` | −12 | 超出内存配额 | |
| `EFAULT` | −14 | 用户地址非法 | |
| `EBADF` | −9 | 描述符非法或权限位不足 | |
| `EINVAL` | −22 | 参数结构性错误 | |
| `ENOSYS` | −38 | 号未注册 | |
| `EPIPE` | −32 | 对端已关闭 | |
| `EQUOTA` | −1080 | 生命周期硬上限 | |
| `ESECCOMP` | −1081 | 被过滤器拦截 | |

返回通道解码规则：`raw ≥ 0` 即成功；`[-4095,-1]` 查表；表中无此项按 `EINVAL`
处理——**绝不**让一个无法识别的负数冒充巨大成功值。

---

## 6. 门禁顺序（F063~F067）

```
入口 → 号表(ENOSYS) → seccomp(F067) → 配额(F065) → 能力(F063) → 审计(F064) → 处理体
```

顺序是契约的一部分，内核自检逐段验证：

- **seccomp 先于配额**：沙箱进程不能靠探测自己被禁止的调用来消耗预算；
- **能力先于处理体**：被拒调用零成本，且拒绝原因可归因；
- **审计最后但无条件**：放行与拒绝都记录（仅敏感集记录放行）；
  未注册号**默认审计**——失败要看得见。

**seccomp 语义**：白名单模式默认动作由 `default_action` 决定（预设为 `KILL`）；
未启用时过滤器完全透明。黑名单模式只拦清单内的号。

**配额语义**：窗口内超额是 `EAGAIN`（可重试）；生命周期硬上限是 `EQUOTA`（终局）。
`limit == 0 && hard_limit == 0` 表示免计量进程（init 与内核线程）。

---

## 7. 快路与 vDSO（F066 / F068）

- **快路**：仅号表登记 `fast = true` 且**不参与审计**的调用可走无锁车道
  （`clock_gettime` / `event_wait` / `abi_query` / `yield`）。
  「快且敏感」是被自检禁止的组合——审计会取锁，会毁掉快路的意义。
- **vDSO**：只导出**无副作用**调用。当前导出三项：

  | 符号 | 号 | 偏移 | 大小 |
  | --- | --- | --- | --- |
  | `__vdso_clock_gettime` | `0x4006` | `0x000` | 96 |
  | `__vdso_event_poll` | `0x400B` | `0x080` | 64 |
  | `__vdso_abi` | `0x400F` | `0x100` | 32 |

  页面映射为**只读 + 可执行**，数据页（`0xF00` 起）由内核带外刷新。
  `yield` **不在**白名单：它看起来便宜，但它动运行队列。

---

## 8. ABI 版本与兼容（F069 / F070）

`abi_query(version, ptr_size)` 返回打包字：`[15:0]` 版本、`[31:16]` 版本下限、
`[47:32]` 号基址、`[63:48]` 页大小（本表另附能力位与号上限，见
`syscall::table::AbiInfo`）。

拒绝规则：

| 输入 | 结果 |
| --- | --- |
| `version < version_min` | `ENOTSUP` |
| `version > ABI_VERSION` | `EINVAL`（未来版本不许猜字段） |
| `ptr_size != 8` | `ENOTSUP` |

能力位（`FEAT_*`）：`CAPS` `AUDIT` `QUOTA` `SECCOMP` `VDSO` `COMPAT` `PIPES`
`EVENTPORT` `HANDLE_PASSING` `FASTPATH` `METER`。用户程序应在启动时探测，
按位选择代码路径，而不是先调用再猜为什么 `ENOSYS`。

**兼容层**（F070）：`0x4100` 段收到号后按映射表转接到原生号，并标注转接注意事项
（`Exact` / `ExtraArgsIgnored` / `Permuted` / `ReturnDiffers`）。
映射表**刻意不完整**——只有语义是原生调用的子集才转接，其余一律 `ENOSYS`，
绝不猜测。

| Linux/x86-64 | 原生 | 备注 |
| --- | --- | --- |
| 0 `read` | `0x4000` | |
| 1 `write` | `0x4001` | |
| 2 `open` | `0x4002` | |
| 3 `close` | `0x4003` | |
| 9 `mmap` | `0x4007` | |
| 10 `mprotect` | `0x4009` | |
| 11 `munmap` | `0x4008` | |
| 22 `pipe` | `0x400C` | |
| 24 `sched_yield` | `0x4010` | |
| 32 `dup` | `0x400D` | |
| 33 `dup2` | `0x400D` | 已弃用；参数语义需重排 |
| 60 `exit` | `0x4004` | |
| 61 `wait4` | `0x4005` | 多余参数忽略 |
| 228 `clock_gettime` | `0x4006` | |
| 231 `exit_group` | `0x4004` | |

---

## 9. 用户库契约（F071）

`kernel/userspace/varix-std/`（`no_std`，无 libc，无分配器）提供：

| 模块 | 内容 |
| --- | --- |
| `abi` | 全部号与常量（唯一允许写死号的用户侧文件） |
| `error` | `Error` 枚举 + `Result` + `from_raw` 解码（唯一解码返回通道处） |
| `io` | `read/write/write_all/open/close/pipe/dup/print*/eprint*` |
| `mem` | `Prot` 类型、`Mapping`（RAII 自动 `munmap`） |
| `time` | `ClockId` / `Timespec` / `monotonic_nanos` |
| `proc` | `ExitStatus` / `exit` / `wait` / `yield_now` |
| `event` | `EventPort` / `Event` / 订阅位掩码 |

入口桩（`syscall0_raw`）是**唯一**的内联汇编点。稳定性分级：`stable`
（12 项，永久承诺）/ `provisional` / `internal`。`send_handle` 与 `log`
**不提供**通用包装（前者是监管者接口，后者只属调试面）。

---

## 10. 质量门禁（F072 / F073 / F074）

- **F072 fuzz**：非法指针、越界号、全参数空间三路轰击，累计 **≥ 10⁶** 轮，
  全部走真实校验函数（不是复刻的判定），契约是**不 panic**。
  种子固定，失败可复现。自检跑 6 万轮保证构建速度，单测跑满 100.2 万轮。
- **F073 仪表**：每号计数/累计耗时/最坏值 + 32 条最近样本环形；
  `p50`/`p99` 由环形副本排序得出（无分配）；热门榜取前 N。
  越界号计入 `out_of_band` 而不是丢弃——看不见的调用才最危险。
- **F074 自检**：本域 CheckSet **恰好 25 项**（F051..F075 各一），
  经 `robust::run_kernel_checkup()` 纳入内核全局闭合回路。

---

## 11. 不变量清单（违反即为 ABI 缺陷）

1. 未注册号 → `ENOSYS`，且在任何参数检查之前。
2. 用户指针从不被直接解引用；`ptr + len` 从不回绕。
3. W^X 恒成立：`PROT_WRITE | PROT_EXEC` 任何路径都被拒。
4. 句柄权限只能收窄，永不放大；控制台不可转赠。
5. 管道：无写者且缓冲空 → 读返回 **0**（成功）；无读者 → 写 `EPIPE`。
6. 单调钟只增不减；墙钟步进不影响单调钟。
7. 队列满报 `EAGAIN` 并计入丢弃计数，绝不静默丢样本。
8. 敏感调用的放行与拒绝**都**进审计环；未知号默认审计。
9. 快路调用不得同时是审计调用。
10. vDSO 只导出无副作用调用，且页面可执行但不可写。
11. 两条入口路径对同一调用必须解码出同一帧。
12. 错误码编号与变体一一对应，且返回通道可无损解码。
