# Varix STAR I · 工作包台账（MD3 附录 B 执行面）

按 MD3 附录 B 模板逐包一行；状态与判据同记（MD3 1.5），台账与代码同库同提交（R4 恢复点纪律）。里程碑对账时逐行核。

| WP 编号 | 状态 | 判据状态 | 依赖状态 | schema 变更 | 收工日 |
| --- | --- | --- | --- | --- | --- |
| WP-101 | 收口（宿主侧） | B-101~106 六绿-宿主（实机/QEMU 依赖项=环境未就位类，随 WP-102 补测） | 无前置 | limine.conf 固化（interface_version + hash 元数据、default_entry、comment 字段；bootconf::render 单源） | 2026-09-24 |
| WP-103 | 未开工 | — | WP-101 | — | — |
| WP-104 | 未开工 | — | WP-101（与 WP-103 交错） | — | — |
| WP-102 | 未开工 | — | WP-101/104 | — | — |
| WP-105 | 未开工 | — | WP-101 | — | — |
| WP-106 | 未开工 | — | WP-101 | — | — |

## WP-101 收口明细（2026-09-24）

**交付面**：
- `kernel/varix/src/bootconf.rs`（新建）：limine.conf 唯一权威解析面——拥有型 `BootConf`（固定容量、零静态、并发安全）、`ConfIssue` 词表（8 类问题登记）、WD-003 `entry_render` 灰显判定、FNV-1a `conf_hash`（与 bootchain::hash_bytes 同源）、`render()` 安装器单源；`include_str!` 把仓库根 limine.conf 编译期嵌入测试，文件漂移即红。
- `kernel/varix/src/oneshot.rs`（新建）：OneShot 全链——40 字节帧编解码（魔法/schema/FNV 校验和）、`arm_conf_text`/`restore_conf_text` conf 改写（越界拒绝）、`on_boot_verify` 启动回读判定（双层消费兜底）、`choose_handoff_path` 三路径降级（Oneshot→BootNext→Menu→不交接）、`gate_check` 闸门三条件、20 组注入矩阵（B-105）。
- `kernel/varix/src/bootchain.rs`：BootStage 6→12 段（B-106）——`ELEVEN_CHECKPOINTS` 十一点位 + `checkpoints_complete` + `standard_timeline` 12 点（0→3100ms）。
- `kernel/varix/src/limine.rs`：B-101 BASE_REVISION 闸（volatile 读 + 三态判定）+ limine.conf 挂为第三可选模块（`../limine.conf`）。
- `kernel/varix/src/main.rs`：`refuse_boot` 拒绝启动出口（串口先行 + 帧缓冲人话屏 + hlt）+ 菜单前契约检查块（ContractMissing → 灰显注入）+ 灰显卡三路执行拦截（键盘 Enter / 倒计时归零默认项 / 鼠标点击）。
- `kernel/varix/src/bootselect.rs`：WD-003 灰显状态机（`WindowsGreyReason::{ConfigMissing, TargetMissing}` + 幂等 set/get）+ 灰显渲染（标题降暗/无高亮框/人话副标题）+ 并行测试串行闸（GREY_STATE_LOCK）。
- `limine.conf`（根目录，固化）：timeout 5 / serial yes / default_entry 0 / 双条目（varix + Windows 11 (USB)→WINESP GUID）/ 条目 comment / 头部元数据 `# interface_version: varix-bli-1` + `# hash: 8207912a`（口径：首个非注释非空行起至 EOF，FNV-1a 32，与内核同源；红线注释原样保留）。

**证据三件套**：数据 = 全量 `cargo +1.97.1 test`（kernel workspace）3179 项全绿（lib 3172 + fuzz 1 + parser fuzz 6），耗时 3m36s；复现命令 = `cd kernel && cargo +1.97.1 test`；日期 = 2026-09-24。

**红项处置**：无红项。过程记录（修复并锁定回归）：F178 过期断言 6→12（BootStage 扩容后未同步）；`gate_check` 对 Incomplete（协议/path 残缺）误放行为 Pass 的真缺陷（B-105 组 03 逮住）；spin_lock 争用断言时序 flaky（起跑 Barrier + 首临界区拉长，从概率变必然）。

**环境偏差登记（不阻断，随队跟踪）**：
1. 冻结工具链 1.97.1 的 clippy 组件在本机损坏（rustup 组件账本缺条目，add/remove 均异常）——clippy 门禁以 1.98.1 视图代跑：2425 条警告全为存量（1.98 新 lint 全开），非本包引入；按"手术式改动"纪律不在本包清理，留专项。
2. 1.97.1 rustc 存量警告 31 条（xhci/ahci/winapi/usrshell/ring3/exfat/msc/security/winsurf/bootcfg 等存量文件）；WP-101 触碰文件（bootconf/oneshot/bootselect/main/limine/bootchain）零警告——本轮引入的 5 条（unused import + 4 处冗余 mut）已在收工前清零。

**WP-101 最丑角落（m4 复盘用）**：`main.rs` 的 `boot()` 已接近 700 行，契约检查块与灰显拦截散在引导序列里——WP-102 交接状态机接线时应把「菜单决策面」收拢为独立函数，boot() 只留时序骨架。
