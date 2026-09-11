# BASELINE · AURORA-1000 基线快照（步骤 0010）

> 实测于 2026-09-11，rustc 1.97.1，Windows 11 x86_64。

| 项 | 值 |
|---|---|
| 内核 Rust 源码行数（kernel/varix/src，含 aurora 四十域） | 170,698 |
| 内核 ELF（kernel/target/x86_64-unknown-none/release/varix） | 7,668,912 字节（7.31 MiB） |
| cargo kbuild 全量耗时（重链接实测） | ~17 s（增量 <1 s） |
| cargo ktest 单测总数 | 1,831 passed / 0 failed |
| 启动路径 | Limine → kernel/varix（ELF，linker.ld 静态布局）→ banner → selftest |
| 工具链 | rust-stable 1.97.1（kernel/rust-toolchain.toml 冻结） |

## 待环境具备
- QEMU（步骤 0004/0014）：未安装，`scripts/run-qemu.sh` 已就绪，首启 golden（步骤 0015）待首启后固化。
- nasm / mtools / xorriso / limine（步骤 0005/0006）：未安装，`scripts/make-iso.sh` 已就绪。
- 真机 smoke（步骤 0024）：待启动介质与真机（见 `docs/真机清单.md`）。

## M1 环境归档（步骤 0276）
- 工具链：rustc 1.97.1（`kernel/rust-toolchain.toml` 冻结，与步骤 0010 基线一致）。
- 目标：x86_64-unknown-none（kbuild）/ x86_64-pc-windows-msvc（ktest 宿主）。
- QEMU / ISO 工具：仍未安装（同上「待环境具备」），M1 验收以主机套件为准。

## M4 性能基线归档（步骤 0990 · W4 系统服务与互联）

> 宿主侧确定性断言（无 QEMU，真机数据待补），判定入口 `perf::perf_selfcheck` / `w4_integration::perf_w4_budgets`。

| 项 | 预算 | 断言值（PASS） | 判定函数 |
|---|---|---|---|
| 帧耗时（60fps） | ≤ 16,700 µs | 14,000 µs（input 1k + sim 2k + render 8k + comp 3k） | `FrameSpans::fits_budget` |
| 启动链（firmware+kernel+session） | < 3,000 ms | 2,600 ms（900+700+1,000） | `BootStages::meets_instant_on` |
| 内存足迹 | 域预算 ≤ 128 MiB | 120 MiB（32+64+24 MiB） | `MemFootprint::within_budget` |
| 服务 IO 读延迟 | ≤ 10 ms | 8 ms（Nominal） | `perf::io_verdict` |
| 性能回归门禁 | 回退 >5% 即 FAIL | +4% → Pass（不回归） | `perf::regression_gate` |
| 唤醒延迟（S3→RTC） | < 1 s（红线 500 ms） | 900 ms PASS / 1500 ms FAIL | `apower::wake_latency_ok` |
| 每瓦性能 | ≥ 5 ops/W | `efficiency_verdict` | `apower::perf_per_watt_milli` |
| 文档/设置搜索 | < 100 ms | `search_budget_ok(80,100)` | `settings` |
| 打印单页 | 场景预算内 | `page_budget_ok` | `printing` |

- 可复现：`cargo ktest w4_integration::` + `cargo ktest perf::` + `cargo ktest apower::` 全绿即复现本表。
- 真机四类矩阵数据：待环境具备（步骤 1241~1290）后归档。
