# Varix 内核

> 一个用 Rust 从第一行代码开始写的 x86-64 `no_std` 内核。判据驱动：每一项能力都有可执行判据，每一个数字都可复核。
>
> 本 README 只讲内核本体，不夸大、不预告未完成的事。所有数字标注出处；未完成的事项在第 10 章《已知边界》里如实列出。

---

## 目录（15 章合订本：docs/Varix/README.md）

| 章 | 内容 | 章内直达 |
|---|---|---|
| 01 | 架构总览 | [docs/Varix/README.md](docs/Varix/README.md) |
| 02 | 引导与平台 | [docs/Varix/README.md](docs/Varix/README.md) |
| 03 | CPU 与中断 | [docs/Varix/README.md](docs/Varix/README.md) |
| 04 | 内存管理 | [docs/Varix/README.md](docs/Varix/README.md) |
| 05 | 进程与装载 | [docs/Varix/README.md](docs/Varix/README.md) |
| 06 | 图形与桌面 | [docs/Varix/README.md](docs/Varix/README.md) |
| 07 | 判据体系 | [docs/Varix/README.md](docs/Varix/README.md) |
| 08 | 移植线 | [docs/Varix/README.md](docs/Varix/README.md) |
| 09 | 兼容层 | [docs/Varix/README.md](docs/Varix/README.md) |
| 10 | 已知边界 | [docs/Varix/README.md](docs/Varix/README.md) |
| 11 | 工程纪律 | [docs/Varix/README.md](docs/Varix/README.md) |
| 12 | 构建与测试 | [docs/Varix/README.md](docs/Varix/README.md) |
| 13 | 路线图 | [docs/Varix/README.md](docs/Varix/README.md) |
| 14 | 附录·实测数据 | [docs/Varix/README.md](docs/Varix/README.md) |
| 15 | 语言与生态扩展 | [docs/Varix/README.md](docs/Varix/README.md) |

---

## 1. 这个项目是什么

Varix 是一个运行在 x86-64 裸机上的桌面操作系统内核，用 Rust 编写，`no_std` 起步、Limine 协议引导、higher-half 装载（基址 `0xffffffff80000000`，见 `kernel/varix/linker.ld:9`）。

它不是一个教学玩具。截至 2026-10-13，内核源码树 `kernel/varix/src/` 含 **1000+ 个 `.rs` 文件**，划入 **354 个判据域**，全量宿主测试基线为 **13689 passed / 26 failed**（2026-10-12 实测，全量约 364 秒）。26 条红项的归因与修复进度见第 10 章。

它有一个长期目标，但本 README 不用它开头：让 Windows 桌面软件在本体上达到 90% 兼容（判定协议见 `docs/Varix/CoRun Varix STAR II · Unxreal/Windows软件90%兼容总案（独立技术版·一册通读）.md` 第〇篇，让步阶梯见其第七篇）。目标写在总案里，进度写在本 README 第 10、13 章里，两处都不掺水。

## 2. 现状一览（实测）

| 项 | 数字 | 出处 |
|---|---|---|
| 内核源码 | 1000+ `.rs` 文件 | `kernel/varix/src/` |
| 判据域 | 354 | `kernel/varix/src/robust.rs` 域数组 |
| 全量测试基线 | 13689 passed / 26 failed | 2026-10-12，约 364 秒 |
| ring3 能力 | GDT 用户段 + TSS.RSP0 + `syscall`（STAR/LSTAR/EFER.SCE）+ `int 0x80` 双入口 | `proc/ring3.rs`（1567 行） |
| PE32+ 装载 | 头/节表/入口/W^X/导入表全解析 | `proc/pe.rs`（895 行） |
| Win32 服务台 | 33 项注册：Full 18 / Partial 9 / Stub 6 | `proc/winapi.rs:84` |
| 判据总闸 | `CheckSet` + `run_kernel_checkup()` 动态注册表 | `checks.rs` + `robust.rs` |
| 图形栈 | 纯软件 2D：矢量/位图/文字/AA/渐变/图层合成/脏矩形 | `aurora/render2d.rs` 模块文档 |
| 字体现状 | 仅 95 个 ASCII 字形（8×8 等宽点阵） | `font.rs:8` |
| 移植落点 | `bfport`（bitflags 2.13.2）、`ckport`（bytemuck 1.25.2） | `src/bfport.rs`、`src/ckport.rs` |

## 3. 三个设计立场

**立场一：判据先行。** 每个域一个 `CheckSet`，判据的分子分母、量纲、期望值必须可推导。写死数字漂移即红的判据会被归因为"判据写错"而非实现错——修复判据不是放松判据，是把"恒红的常量比较"换成"守得住的性质"。细节见第 7 章。

**立场二：单一真源。** 同一件事只允许一处实现。`bfport` 的 `BfBits` trait 因无使用者被删除；`memmove` 的重叠方向判定以 `unx101_vgafallback.rs:2058` 的纯函数为真源，任何新实现必须复用它或对拍。

**立场三：如实。** 未实现就是未实现。`winapi.rs` 的 `ApiStatus::Stub` 调用返回 `Enosys` 且计数上账，判据 `winapi.rs:499` 锁死 `stub == 6` 防止账面漂移；`winelaunch` 在运行时未部署时如实报 `Absent`，不冒充兼容。

## 4. 十秒跑起来

```bash
cd kernel
cargo kbuild    # 内核镜像（x86_64-unknown-none + rust-lld + linker.ld）
cargo ktest     # 宿主全量判据（x86_64-pc-windows-msvc）
cargo kcheck    # 真机 target 静态检查
```

QEMU 引导验证走 `kernel/_tools/qemu_boot_verify.py`；ISO 组装走 `kernel/_tools/make_iso.py`。启动关键文件 `boot-select.json`、`limine.conf` 不可删。

## 5. 源码导览

```
kernel/
├── varix/
│   ├── src/               ← 内核本体（1000+ .rs，354 判据域）
│   │   ├── main.rs        ← 引导入口（Limine base rev 1）
│   │   ├── checks/        ← 判据基建（CheckSet 及分册）
│   │   ├── robust.rs      ← 判据总闸：354 域动态注册表
│   │   ├── proc/          ← 进程/装载/ring3/Win32 服务台
│   │   ├── cpu/           ← SMP/IDT/GDT/syscall 机制
│   │   ├── aurora/        ← 桌面环境（18 个模块）
│   │   ├── cgpu/          ← 图形域（50 判据）
│   │   ├── unxstar/       ← 深化域（70+ 判据）
│   │   ├── svstar2/       ← 动画/资产域（452 项函数指针表）
│   │   ├── bfport.rs      ← bitflags 移植落点
│   │   ├── ckport.rs      ← bytemuck 移植落点
│   │   └── linker.ld      ← 62 行链接脚本（5 section + DISCARD）
│   └── Cargo.toml         ← 依赖仅 libm 0.2（纯 Rust 数学库）
├── userspace/varix-std/   ← 用户态 libc 雏形（no_std，syscall0_raw）
├── varix-svcabin/         ← 服务容器 ABI
└── _tools/                ← make_iso / qemu_boot_verify 等
```

## 6. 阅读顺序建议

- 想知道**内核怎么启动** → 第 02 章。
- 想知道**ring3 和 PE 装载** → 第 05 章（这是兼容线的地基）。
- 想知道**为什么有 354 个判据域、怎么写新判据** → 第 07 章。
- 想知道**现在哪些东西没做完** → 第 10 章，逐条带文件行号。
- 想动手写代码 → 第 11 章（纪律）+ 第 12 章（构建），两条纪律不读不要提交：判据数组长度必须同步 +1；新模块必须进 `run_kernel_checkup()` 注册表，否则判据永不执行。
- 想知道 **C/C++/Python/JS/CSS 怎么进内核** → 第 15 章（语言与生态扩展），cenv 四层 shim + Servo 组件提取的真实路径。
- 想核对任何数字 → 第 14 章（实测数据附录），全部数字带出处与测量口径。

## 7. 快速问答

**Q：这个内核能跑 Windows 软件吗？**
现在不能跑完整的。已有的地基：ring3 用户态（GDT/TSS/syscall/int 0x80 双入口）、PE32+ 装载器、Win32 服务台 33 项。离"跑真实软件"还差：PE 重定位、号位扩容、Win32 语义深化——都在第 13 章路线图里，每个条目带验收口径。目标 90% 兼容写在总案里，这里只报进度不报愿望。

**Q：能编译 C/C++ 进内核吗？**
能，路径是真实的：cenv 四层 shim 替代 libc（约 4700 行）+ Zig/clang 交叉编译 + `linker.ld` 扩展。C 第一版即可用；C++ 第一版 `-fno-exceptions`（STL 容器/模板可用，throw 待 unwind 桩）。明细见第 15 章。前置是编译器在位（路线图 D 组）。

**Q：支持 HTML/CSS 吗？**
不是现在。主路径已定案：Servo（Rust）的 style + layout 组件提取约 25 万行进 ring0，对接 aurora 渲染；Chromium 整装（2500 万行 C++）不可行，账在第 14 章第 8 节。Edge/Chrome 闭源部分不逆向——这是写在总案里的红线，不是临时决定。

**Q：U 盘上那 1.67 亿行代码都进内核吗？**
不都是。1.67 亿行是**原料总量**（188 个实体仓），其中大量与内核场景无关。"全部进内核"的工程含义是七层落法各取所需：内核算法走 L3、系统服务走 L4、工具链永远留在 L6 不进镜像。分层规则与数字口径见第 14 章第 5、6 节。

**Q：为什么这么强调判据？**
因为裸机内核没有"看起来能跑"这回事——崩溃没有弹窗，只有重启。354 个判据域、13689 条判据是这个项目对"能跑"的定义方式：一条性质守得住，才算有这个能力。判据怎么写、红项怎么归因，见第 7 章；判据纪律的三条血泪教训（注册表漏注册、标准夹具自相矛盾、注释承诺未实现）见第 11 章。

## 8. 数字速查（口径统一在 14 章）

| 常被引用的数字 | 值 | 性质 |
|---|---|---|
| 内核测试基线 | 13689 passed / 26 failed | 实测 2026-10-12 |
| U 盘实体仓总量 | ≈167,082,777 行 | 实测，原料口径 |
| 可进 ring0 的 C 库 | ≈170–230 万行 | 估算区间 |
| 三条移植路径最终目标 | T1 80–120 万 / T2 150–250 万 / Win32 17–20 万行 | 估算区间 |
| Chromium 全量 | ≈2500 万行 | 估算，不可整进 |
| Servo 可提取 | ≈25 万行 | 估算，CSS 主路径 |
| 兼容率自认上限 | 88–93% | 总案口径 |

引用规则：估算数字必须带区间，不许只取上限当事实——这条纪律本身写在第 14 章第 10 节。

## 9. 与旧版 README 的关系

旧版 README（83KB《VARIX-500 通俗介绍》）已被本版整体替换，git 历史保留可查。替换原因：旧版以叙事为主、缺少文件行号级出处、与判据体系的口径脱节。新版原则：**每个数字有出处、每个结论有账本、每个未完成的事在第 10 章如实列名**。如果新版哪里写得含糊或与代码不符，按第 11 章工程纪律处理：以代码为准改文档，而不是反过来。

## 10. 上手与贡献入口

- **跑起来**：第 12 章第 1 节三条命令（`kbuild` / `ktest` / `kcheck`），QEMU 验证走 `_tools/qemu_boot_verify.py`；
- **改代码前必读**：第 11 章工程纪律——特别是 `no_std` 三件套自查（`use alloc::vec` 宏与 `Vec` 类型是两套东西）和"补注册 `mod.rs` 后 cargo check 全部作废"两条，都是真实踩过的坑；
- **认领路线图条目**：第 13 章阶段一 P0 三项是当前投入产出比最高的改动（约 800 行解锁三个物理天花板），认领前先读第 12 章第 8 节验收口径——四条不齐不算完成；
- **报告问题**：带 `cargo ktest` 的红项输出（用 `CheckSet::red_items_text()` 的"标签(明细)"格式），没有判据输出的报告先补判据再讨论；
- **提交纪律**：判据数组长度同步 +1、新模块进 `run_kernel_checkup()` 注册表、输出先落盘再 grep、不并行 cargo——四条全在第 11、12 章，违者打回。

这个项目的门槛不在代码量，在纪律量。判据体系让每一个改动都可复核，也让每一个偷懒都无处藏身——这正是它存在的意义。
