# 双域 · 任务15 验收记录：进程控制块 + spawn/wait + ELF 装载器（映像装载原语抽象）

- **日期**：2026-09-17
- **执行**：AI-K（内核线）
- **前置**：任务14 ✅（ring3 切换 + 最小 syscall + 用户 hello）
- **结论**：**全部验收项通过**。宿主 ktest 2784 绿 / kcheck 0 告警 / kbuild 成功；QEMU q35 实机串口全链无 fatal。

---

## 交付物

| 交付 | 位置 | 说明 |
|---|---|---|
| 映像装载原语抽象 | `kernel/varix/src/proc/loader.rs`（新） | `SegmentView`/`ImageFormat`/`UserMapper` 三抽象 + 页账本引擎 + ELF 适配器 + KernelMapper(target)。引擎对**镜像格式与内存模型双零知识**——未来 PE/Wine 层只需实现 ImageFormat+UserMapper |
| PCB / spawn / wait 语义 | `kernel/varix/src/proc/uspace.rs`（扩展） | Pcb 全字段经核内 API（审计：零裸字段跨模块改）；句柄类型**运行期注册扩展**（`register_handle_kind`，16 封顶，同名幂等）；waitpid 1000 循环零泄漏用例 |
| 页表摘叶原语 | `kernel/varix/src/mem/pfh.rs` | `PageTableOps::unmap_user`（trait+RealPt+FakePt）——4KiB 用户叶摘除返回原帧，大叶/内核半区如实拒绝 |
| SpaceArena 槽位/表帧回收 | `kernel/varix/src/mem/addrspace.rs` `kernel/varix/src/mem/paging.rs` | destroy 真归还槽位；`TableArena::free_user_tree` 递归归还用户表树（内核半区共享表绝不回收） |
| 监护生命周期 | `kernel/varix/src/proc/ring3.rs`（重写演示段） | spawn→iretq→exit→resume_supervisor→wait→页账本归还→实例2→压力探针→汇总停机；SYS_WAIT 用户臂（`wait_user_in` 纯函数） |
| 错误码 | `kernel/varix/src/proc/entry.rs` | `ErrNo::Echild = 11`（from_i32/name 同步） |
| 生命周期状态图 | `docs/进程生命周期状态图-2026-09-17.md` | 状态机 mermaid + 迁移触发表 + 五条不变量 |

## 总案验收对照

| 验收项 | 结果 | 证据 |
|---|---|---|
| spawn 压力 64 槽打满/耗尽优雅拒绝 | ✅ | 实机：`pressure fill: +64 procs → next alloc rejected TableFull (exhausted=1)`；宿主 f007 |
| 父先死子孤儿被 init 收养 | ✅ | 宿主 f010（exit 过继+wait 收养孙辈）；状态图不变量 4 |
| waitpid 循环 1000 次无泄漏 | ✅ | 宿主 f027（表/空间/僵尸三清、exhausted=0）；实机 drain `exited=64 reaped=64 live=0` |
| PCB 字段访问全部经核内 API | ✅ | 审计：跨模块仅经 `alloc/spawn/exit/wait/reap/find/get/release_space`；Pcb 字段无非本模块写入 |
| 句柄表类型可注册扩展 | ✅ | 宿主 f026：注册/幂等/读回/开查关/空名超长满容拒绝 |
| 进程生命周期状态图入档 | ✅ | `docs/进程生命周期状态图-2026-09-17.md` |
| ELF 装载器 + PE 共用原语抽象 | ✅ | 引擎经 ImageFormat/UserMapper；宿主 `elf_adapter_equals_format_neutral_mirror`（hello.elf 与同段表格式中立镜像逐页逐字节一致） |

## 实机串口实录（QEMU q35，`kernel/qemu-serial-t15b.log`）

```text
[ INFO] ring3: supervised lifecycle demo — 2 轮 spawn/exit/wait，随后压力探针
[ INFO] ring3: instance#1 spawned pid=2 entry=0x400000 pages=18 — PCB + 逐页账本登记
[ INFO] ring3: iretq → user (rip=0x400000 rsp=0x7ffffffff000 rflags=0x3200)
hello from ring3                       ← int 0x80 通道
hello from ring3                       ← syscall 指令通道
[ INFO] ring3: exit(0) pid=2 → zombie=true — 控制权交还监护者，wait 收割
[ INFO] ring3: supervisor wait → pid=2 code=0 pages_released=18/18 — 空间随进程消亡，一页不留
[ INFO] ring3: instance#2 spawned pid=2 entry=0x400000 pages=18 — PCB + 逐页账本登记
hello from ring3 ×2（双通道一致）
[ INFO] ring3: exit(0) pid=2 → zombie=true …
[ INFO] ring3: supervisor wait → pid=2 code=0 pages_released=18/18 …
[ INFO] ring3: pressure fill: +64 procs → next alloc rejected TableFull (exhausted=1) — 64 槽打满，优雅拒绝
[ INFO] ring3: pressure drain: exited=64 reaped=64 live=0 — 表清空，无泄漏
[ INFO] ring3: task15 lifecycle complete — 2 轮 spawn/exit/wait + 压力探针全过 — halting
[ INFO] ring3: user program finished — halting
```

要点：实例2 的 pid **仍是 2**——Zombie→Vacant→alloc 最小槽复用的直接实证；两轮页账本 18/18 归还（页数从任务14 的 19 变 18 是因为旧计数把同页双段数了两次，引擎按唯一页记账）。

## 途中发现并修复的真实缺陷

1. **SpaceArena 永久耗尽**：`destroy` 只标记不复位槽位、`create` 只追加不回收——16 轮 spawn/destroy 后地址空间表永久耗尽，**"waitpid 1000 次无泄漏"验收当场暴露**。修复：槽位置 None+计数归还+create 找首个空槽复用；连带 `TableArena::free_user_tree` 递归归还用户表帧（内核半区共享表防御性跳过）。
2. **监护者 match 临时守卫自旋死锁**：`match PROCS.lock().alloc(..)` 的守卫临时值活到 match 结束，Err 臂内再 `PROCS.lock()` → 实机卡死在压力探针。修复：先 `let res = …` 绑定再分支。
3. **退出回收账本重复记账**：同页多段时旧逻辑每段每页都 push 账本——重复摘叶、数量对不上。修复：仅新帧落账；映射失败的新帧即时回收。

## 门禁

- `cargo ktest`：**2784 passed / 0 failed**（新增 loader 6 例 + wait 语义 1 例 + f026 + f027，计 8 例）
- `cargo kcheck`：**0 告警**
- `cargo kbuild`：成功（kernel 7807336 字节）
- 实机：QEMU 全链无 fatal / 无 EXCEPTION，优雅停机

## 诚实边界

- 调度器派发（Ready↔Running）与用户态多进程并发属任务16 PCB 全局化+调度接入；演示期监护者串行驱动生命周期。
- `SYS_WAIT` 用户臂的调用者身份取自唯一用户进程槽（DEMO_PID），多进程身份随任务16 stub 传参落地。
- PE 适配器本体未实现（属 Wine 兼容层远期通道）；本任务交付的是它必须走的共用原语，格式中立性由镜像等价用例结构性保证。
