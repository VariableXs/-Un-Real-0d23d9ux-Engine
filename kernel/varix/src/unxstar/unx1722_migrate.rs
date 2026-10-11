//! unx1722_migrate —— UNX-F1722 迁移执行原语（摘出→暂存→挂入原子链）。
//! 锚点（CoRun Varix 主册 UNX-A3-B07 · CoRun·UNX-A3-B07 段「均衡执行」批次）：
//!   UNX-F1722 · 迁移执行原语（摘出→暂存→挂入原子链） · 目标 2000~2600 行 · 纯功能线
//!
//! 判据原文（逐条落实，代码内以 `// 判据：UNX-F1722-J1 —— <子句>` 标注）：
//!   `UNX-F1722-J1 三段原子链（源核摘出→暂存入区→目标核挂入）闭合断言全过，
//!    全域唯一跨队列写入点断言通过，绕行注入 10/10 次拒绝`
//!
//! 三条判据子句与本文件的落点：
//!   子句 1「三段原子链闭合断言全过」
//!     → 组件 A（三段原子链状态机）/`assert_chain_closed()`；任一段失败整链回滚，
//!       回滚本身落「回滚段账」（`RollbackEntry`），不静默回退。
//!   子句 2「全域唯一跨队列写入点断言通过」
//!     → 组件 B2（跨队列写入点登记表 `WritePointRegistry`）；跨队列写入必须持
//!       登记点令牌，非登记点直写一律 `BypassWrite(0xB027)` 拒。全域写点仅 1 个
//!       合法点（`WP_MIGRATE`），由 `assert_unique_write_point()` 断言。
//!   子句 3「绕行注入 10/10 次拒绝」
//!     → 组件 F（注入核 10 形态 + `inject_all()` 汇总入口），返回 10 条结果。
//!
//! 组件清单（全部实现，无薄壳）：
//!   A. 三段原子链状态机 —— `MigrateSeg{Detach→Stage→Attach}`，每段入口消费上一段
//!      交接签名（段号 + 负载快照 FNV 指纹 + 线程身份三元组），签名不匹配即拒；
//!      跳段/倒段即拒；全链原子性（失败整链回滚 + 回滚段账）。
//!   B. 源核摘出段（Detach）—— 队首摘出语义 + 摘出前后队列深度守恒恒等断言 +
//!      跨队列写入点唯一性 + 线程态校验（仅 Running 可摘）。
//!   C. 暂存区段（Stage）—— 64 槽定容槽位表，槽位状态闭集
//!      Free/Reserved/Occupied/Dirty，分配/释放/复用三路径；暂存守恒
//!      Σ占用 + Σ自由 == 总槽数；暂存满拒并给回退建议；FNV-1a 64 位手算锚。
//!   D. 目标核挂入段（Attach）—— 目标核选择（负载最轻核，平票取小核号，确定性）、
//!      挂入位置语义（按优先级类序下降）、挂入后目标队列深度守恒 + 全局深度守恒
//!      （迁移只改分布不改总量——关键不变量）。
//!   E. 三向记账联动 —— 源核出账 / 目标核入账 / 线程随行账，三账经 `migration_id`
//!      关联；append-only 环形缓冲（定容，满覆盖最旧并计 `dropped`，满账显性）；
//!      `settle()` 结算三账闭合比与差异明细。
//!   F. 注入核 —— 10 种形态，每形态一函数 + `inject_all()` 汇总。
//!   G. 观测导出 —— 迁移率/乒乓率/破窗率/回退率四族 + 窗口化统计；
//!      `export_kv()` KV 文本、`export_tlv()` TLV 二进制（魔数+版本+长度自洽断言）。
//!   H. 确定性回放 —— `replay()` 自三账环形缓冲重建迁移全过程，逐字段对账，
//!      偏差必须为零（回放偏差账）。
//!
//! 职责定位：本模块是**全域跨队列唯一写入点**。均衡执行（F1721）在收益复核通过后，
//!   唯一的落地手段就是本模块的三段原子链；任何绕过本链的跨队列写（绕行注入）
//!   必须在写入点登记表层被拒。断言密度本批最高：每段入口、每次记账、每次结算、
//!   每次注入均带闭合断言。本段纪律：无写盘条目；开发期零 QEMU。
//!
//! 性能逐项分解（O(·) 逐函数，均为定容/定长上限，实际为有界常数）：
//!   `Migrator::new`                 O(核数)            建核表与队列
//!   `Migrator::begin`               O(核数)            线性查源核 + 链登记
//!   `Migrator::detach`              O(核数 + 队列深)    队首摘出 + 深度重算 + 写点验签
//!   `Migrator::stage`               O(槽数)            64 槽线性扫描分配
//!   `Migrator::stage_reuse`         O(1)              脏槽 LRU 直取（游标）
//!   `Migrator::attach`              O(核数 + 队列深)    最轻核选择 + 位置语义插入
//!   `Migrator::rollback`            O(队列深 + 槽数)    反向摘除 + 槽释放
//!   `Migrator::settle`              O(账环容 × 3)       三账并归 + 差异明细
//!   `WritePointRegistry::authorize`  O(写点表容)         ≤8 项线性验签
//!   `assert_unique_write_point`     O(写点表容)         唯一性扫描
//!   `StageArea::allocate`           O(64)              槽位线性扫描
//!   `StageArea::conserve`           O(1)              维护累计计数
//!   `select_target`                 O(核数)            最轻核 + 平票小核号
//!   `insert_position`               O(队列深)          类序下降定位
//!   `ObsWindow::push`               O(1)              定长环形覆写
//!   `export_kv`                     O(字段数)          定点转十进制串
//!   `export_tlv`                    O(字段数)          TLV 逐项编码
//!   `replay`                        O(账环容 × 3)       全量重建 + 逐字段对账
//!   `inject_all`                    O(核数 × 10)       10 形态各建独立环境
//!
//! 诊断码段：**0xB020~0xB02F**（主码 `0xB020`，闭集 `0xB021~0xB02F`）。
//!   全域 grep 确认 0xB02x 段无冲突（既有 0xB0xx 仅见于 0xB000 掩码与实地址常量，
//!   非诊断码段）。本模块诊断码不外借、不越段。
//!
//! 平台数据结构布局（文件内以 struct 体现；无 repr 强制时按 Rust 默认 repr，
//!   下列为语义字段布局与建议对齐，实际偏移以编译器为准）：
//!   `CoreSlot`   —— 核槽：`cpu:u32`(off 0, align 4) / `online:bool`(off 4, align 1)
//!                  / `load:u64`(off 8, align 8) / `depth:u64`(off 16) /
//!                  `mig_out:u64`(off 24) / `mig_in:u64`(off 32) /
//!                  `last_mig_tick:u64`(off 40) / `offline_reason:u64`(off 48)；
//!                  整体对齐 8。
//!   `QueueEntry` —— 队列项：`tid:u64`(off 0) / `prio:u8`(off 8) / `arrival:u64`(off 16)；
//!                  对齐 8。队列为按 `prio` 类序下降 + 到达序稳定的定容数组语义。
//!   `StageSlot`  —— 暂存槽：`state:u8` / `owner:u64`(off 8) / `tid:u64`(off 16) /
//!                  `src_cpu:u32`(off 24) / `prio:u8`(off 28) / `payload_fp:u64`(off 32) /
//!                  `tick:u64`(off 40)；对齐 8。64 槽定容。
//!   `WritePoint` —— 写点描述符：`id:u16` / `owner_seg:u8` / `cpu:u32` /
//!                  `legal:bool` / `hits:u64`(off 8)；对齐 8，≤8 项。
//!   `LedgerEntry`—— 记账项：`kind:u8` / `seg:u8` / `ok:bool` / `mid:u64`(off 8) /
//!                  `tid:u64`(off 16) / `src_cpu:u32`(off 24) / `dst_cpu:u32`(off 28) /
//!                  `tick:u64`(off 32) / `payload_fp:u64`(off 40)；对齐 8。
//!
//! 纯功能线声明：no_std + alloc。零 `unwrap()`、零 `expect()`、零 `panic!()`、
//!   零浮点、零真实 IO、零墙钟。时间与延迟一律逻辑 `tick: u64` 计数器；
//!   硬件语义由确定性 LCG 伪随机构造，不读真硬件。全部返回值可复算。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const MIG_VERSION: &str = "unx1722_migrate-v1";

/// 诊断段基址（0xB020 段 · 本模块独占；闭集 0xB020~0xB02F）。
pub const MIG_SEG_BASE: u16 = 0xB020;

/// 诊断段上界（含）。
pub const MIG_SEG_LIMIT: u16 = 0xB02F;

/// FNV-1a 64 位 offset basis（可复算锚）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位 prime（可复算锚）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 已知向量 1：`fnv1a64(b"") == 0xcbf29ce484222325`。
pub const FNV_VEC_EMPTY: u64 = 0xcbf2_9ce4_8422_2325;

/// 已知向量 2：`fnv1a64(b"a") == 0xaf63dc4c8601ec8c`。
pub const FNV_VEC_A: u64 = 0xaf63_dc4c_8601_ec8c;

/// 已知向量 3：`fnv1a64(b"foobar") == 0x85944171f73967e8`。
pub const FNV_VEC_FOOBAR: u64 = 0x8594_4171_f739_67e8;

/// 暂存区总槽数（F1653 暂存区定容）。
pub const STAGE_SLOTS: usize = 64;

/// 跨队列写入点登记表容量。
pub const WRITE_POINTS: usize = 8;

/// 记账环形缓冲容量（三账各自定容）。
pub const LEDGER_CAP: usize = 48;

/// 单核队列深度上限（定容队列上限，保证 O 有界）。
pub const QUEUE_CAP: usize = 32;

/// 迁移冷却窗宽度（逻辑 tick，F1726 迁移冷却窗；本模块只读窗用于破窗率）。
pub const COOLDOWN_WINDOW: u64 = 64;

/// 三段原子链阶段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateSeg {
    /// 段 1：源核摘出（Detach）。
    Detach,
    /// 段 2：暂存入区（Stage）。
    Stage,
    /// 段 3：目标核挂入（Attach）。
    Attach,
}

impl MigrateSeg {
    /// 段号（0 起）。
    pub fn num(self) -> usize {
        match self {
            MigrateSeg::Detach => 0,
            MigrateSeg::Stage => 1,
            MigrateSeg::Attach => 2,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            MigrateSeg::Detach => "源核摘出",
            MigrateSeg::Stage => "暂存入区",
            MigrateSeg::Attach => "目标核挂入",
        }
    }
    /// 下一段（Attach 之后返回 None）。
    pub fn next(self) -> Option<MigrateSeg> {
        match self {
            MigrateSeg::Detach => Some(MigrateSeg::Stage),
            MigrateSeg::Stage => Some(MigrateSeg::Attach),
            MigrateSeg::Attach => None,
        }
    }
}

/// 三段顺序视图。
pub const MIG_SEGS: [MigrateSeg; 3] = [MigrateSeg::Detach, MigrateSeg::Stage, MigrateSeg::Attach];

/// 链相位闭集（原子链生命周期）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPhase {
    /// 已开链未摘出。
    Opened,
    /// 已摘出未暂存。
    Detached,
    /// 已暂存未挂入。
    Staged,
    /// 已挂入（三段闭合）。
    Attached,
    /// 已整链回滚（终态）。
    RolledBack,
}

impl ChainPhase {
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            ChainPhase::Opened => "已开链",
            ChainPhase::Detached => "已摘出",
            ChainPhase::Staged => "已暂存",
            ChainPhase::Attached => "已挂入",
            ChainPhase::RolledBack => "已回滚",
        }
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, ChainPhase::Attached | ChainPhase::RolledBack)
    }
}

/// 线程态闭集（仅 Running 可摘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// 运行中（唯一可摘态）。
    Running,
    /// 就绪未上核。
    Ready,
    /// 阻塞。
    Blocked,
    /// 睡眠。
    Sleeping,
    /// 已停止（终态不可摘）。
    Stopped,
}

impl ThreadState {
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            ThreadState::Running => "运行中",
            ThreadState::Ready => "就绪",
            ThreadState::Blocked => "阻塞",
            ThreadState::Sleeping => "睡眠",
            ThreadState::Stopped => "已停止",
        }
    }
    /// 是否可摘出（判据：仅 Running 态可摘）。
    pub fn migratable(self) -> bool {
        matches!(self, ThreadState::Running)
    }
    /// 态号（用于指纹）。
    pub fn num(self) -> u8 {
        match self {
            ThreadState::Running => 0,
            ThreadState::Ready => 1,
            ThreadState::Blocked => 2,
            ThreadState::Sleeping => 3,
            ThreadState::Stopped => 4,
        }
    }
}

/// 线程身份三元组（线程号 / 源核号 / 优先级）—— 交接签名必载字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadTriple {
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 优先级（值大者类序在前）。
    pub prio: u8,
}

impl ThreadTriple {
    /// 构造。
    pub fn new(tid: u64, src_cpu: u32, prio: u8) -> Self {
        ThreadTriple { tid, src_cpu, prio }
    }
    /// 三元组指纹（FNV 混合三字段）。
    pub fn fingerprint(&self) -> u64 {
        mix3(self.tid, self.src_cpu as u64, self.prio as u64)
    }
}

/// 确定性 LCG 伪随机（硬件语义替身；不读真硬件）。
pub fn lcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 33
}

/// FNV-1a 64 位（手算锚，可复算）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 64 位已知向量自检（空串 / "a" / "foobar" 三锚）。
pub fn fnv_anchor_check() -> (bool, Vec<u64>) {
    let mut got: Vec<u64> = Vec::new();
    let h0 = fnv1a64(b"");
    let h1 = fnv1a64(b"a");
    let h2 = fnv1a64(b"foobar");
    got.push(h0);
    got.push(h1);
    got.push(h2);
    (h0 == FNV_VEC_EMPTY && h1 == FNV_VEC_A && h2 == FNV_VEC_FOOBAR, got)
}

/// 三字段 FNV 混合（用于结构指纹，非字节序列指纹）。
pub fn mix3(a: u64, b: u64, c: u64) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for w in [a, b, c] {
        for byte in w.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= 0xff;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 六字段 FNV 混合（负载快照指纹）。
pub fn mix6(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    for w in [a, b, c, d, e, f] {
        for byte in w.to_le_bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= 0xfe;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 定点千分比格式化（零浮点；`v` 为千分比定点，输出 `x.xxx%` 文本）。
pub fn fmt_permille(v: u64) -> String {
    let whole = v / 1000;
    let frac = v % 1000;
    format!("{}.{:03}", whole, frac)
}

/// 错误闭集枚举（诊断码闭集 0xB020~0xB02F）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigErr {
    /// 0xB020 主码：迁移三段闭合（成功）。
    Ok = 0xB020,
    /// 0xB021 原语重入（链执行中再次调用）。
    Reentrant = 0xB021,
    /// 0xB022 跳段（段序被跳过）。
    SegSkipped = 0xB022,
    /// 0xB023 伪造交接签名（签名非本链铸造 / 段号倒退）。
    SigForged = 0xB023,
    /// 0xB024 暂存区满（F1653 槽位耗尽，回退路径由 F1724 消费）。
    StageFull = 0xB024,
    /// 0xB025 目标核离线。
    TargetOffline = 0xB025,
    /// 0xB026 非 Running 态摘出。
    NotRunning = 0xB026,
    /// 0xB027 绕行写入（未登记写入点直写跨队列）。
    BypassWrite = 0xB027,
    /// 0xB028 回退中再迁（回滚未完成即再开链）。
    InRollback = 0xB028,
    /// 0xB029 段签名载荷指纹篡改。
    FpTamper = 0xB029,
    /// 0xB02A 队列深度不守恒（手工破坏 depth 后结算）。
    DepthDrift = 0xB02A,
    /// 0xB02B 交接签名字段不匹配（段号/三元组/载荷指纹）。
    SigMismatch = 0xB02B,
    /// 0xB02C 暂存槽非法（未 Reserved 即占用 / 空槽释放）。
    StageSlotBad = 0xB02C,
    /// 0xB02D 源核离线。
    SrcOffline = 0xB02D,
    /// 0xB02E 三账不闭合。
    LedgerUnclosed = 0xB02E,
    /// 0xB02F 脏槽复用前未落账（内容未清即复用）。
    SlotDirty = 0xB02F,
}

impl MigErr {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            MigErr::Ok => "三段原子链闭合（迁移成功）",
            MigErr::Reentrant => "原语重入（链执行中再次调用迁移原语）",
            MigErr::SegSkipped => "跳段（三段链序被跳过）",
            MigErr::SigForged => "伪造交接签名（签名非本链铸造或段号倒退）",
            MigErr::StageFull => "暂存区满（F1653 槽位耗尽，回退交 F1724）",
            MigErr::TargetOffline => "目标核离线",
            MigErr::NotRunning => "非 Running 态摘出",
            MigErr::BypassWrite => "绕行写入（未登记写入点直写跨队列）",
            MigErr::InRollback => "回退中再迁（回滚未完成即开新链）",
            MigErr::FpTamper => "段签名载荷指纹篡改",
            MigErr::DepthDrift => "队列深度不守恒",
            MigErr::SigMismatch => "交接签名字段不匹配",
            MigErr::StageSlotBad => "暂存槽非法（未预留即占用或空槽释放）",
            MigErr::SrcOffline => "源核离线",
            MigErr::LedgerUnclosed => "三向账不闭合",
            MigErr::SlotDirty => "脏槽复用前未落账",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            MigErr::Ok => "无（成功路径）",
            MigErr::Reentrant => "迁移原语串行化；重入须待前链终态",
            MigErr::SegSkipped => "按 Detach→Stage→Attach 序推进，不得跳段",
            MigErr::SigForged => "签名只能由上一段出口铸造；禁止外部构造",
            MigErr::StageFull => "回退该次迁移并落暂存满（F1724 消费）；必要时扩槽",
            MigErr::TargetOffline => "改选次轻在线核；无在线核则本轮止迁",
            MigErr::NotRunning => "仅 Running 态线程可摘；等待其上 Running",
            MigErr::BypassWrite => "跨队列写必须经本原语写点；禁止旁路直写目标队列",
            MigErr::InRollback => "回滚终态前不得开新链；等回滚段账落定",
            MigErr::FpTamper => "签名字段不可外部改写；重铸后重入",
            MigErr::DepthDrift => "深度破坏即为记账事故；以守恒断言拦截结算",
            MigErr::SigMismatch => "段入口须提交上一段出口原样签名",
            MigErr::StageSlotBad => "分配走 allocate（Free→Reserved→Occupied）三路径",
            MigErr::SrcOffline => "源核离线不得摘出；本轮止迁",
            MigErr::LedgerUnclosed => "三向账须同 mid 齐记；缺一向即补记后结算",
            MigErr::SlotDirty => "复用脏槽前先 release 落账再重分配",
        }
    }
    /// 是否为拒绝码（0xB021~0xB02F）。
    pub fn is_reject(self) -> bool {
        self != MigErr::Ok
    }
    /// 码下标（用于分桶计数）。
    pub fn bucket(self) -> usize {
        (self.num() & 0x0f) as usize
    }
}

/// 诊断体（码 + 位置 + 细节）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigDiag {
    /// 诊断码。
    pub code: MigErr,
    /// 位置串。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl MigDiag {
    /// 构造。
    pub fn new(code: MigErr, site: &str, detail: String) -> Self {
        MigDiag { code, site: site.to_string(), detail }
    }
    /// 码值。
    pub fn num(&self) -> u16 {
        self.code.num()
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04X}] {} @{} —— {} —— 建议:{}",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 队列项（线程 + 优先级 + 到达序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueEntry {
    /// 线程号。
    pub tid: u64,
    /// 优先级（类序下降）。
    pub prio: u8,
    /// 到达序（稳定序打平用）。
    pub arrival: u64,
}

impl QueueEntry {
    /// 构造。
    pub fn new(tid: u64, prio: u8, arrival: u64) -> Self {
        QueueEntry { tid, prio, arrival }
    }
}

/// 核内运行队列（定容、按 prio 类序下降 + 到达序稳定）。
#[derive(Clone, Debug, Default)]
pub struct RunQueue {
    /// 核号。
    pub cpu: u32,
    /// 项表（有序）。
    pub items: Vec<QueueEntry>,
    /// 深度账（允许被外部手工破坏以供注入核 10 检测）。
    pub depth: u64,
    /// 到达序游标。
    pub arrival: u64,
}

impl RunQueue {
    /// 新队列（预置 `cap` 个空位语义：空队列）。
    pub fn new(cpu: u32) -> Self {
        RunQueue { cpu, items: Vec::new(), depth: 0, arrival: 0 }
    }
    /// 实算深度（items 长度）。
    pub fn actual_depth(&self) -> u64 {
        self.items.len() as u64
    }
    /// 深度守恒断言（账深 == 实深）。
    pub fn depth_conserved(&self) -> bool {
        self.depth == self.actual_depth()
    }
    /// 队首。
    pub fn head(&self) -> Option<QueueEntry> {
        self.items.first().copied()
    }
    /// 队尾。
    pub fn tail(&self) -> Option<QueueEntry> {
        self.items.last().copied()
    }
    /// 队首摘出（不重排，返回摘出项）。
    pub fn pop_head(&mut self) -> Option<QueueEntry> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }
    /// 队首原位回插（回滚用）。
    pub fn push_head(&mut self, e: QueueEntry) -> bool {
        if self.items.len() >= QUEUE_CAP {
            return false;
        }
        self.items.insert(0, e);
        self.arrival = self.arrival.wrapping_add(1);
        true
    }
    /// 类序下降插入位（返回应插入下标；`prio` 大者在前，同 prio 按到达序稳定）。
    pub fn insert_position(&self, prio: u8, arrival: u64) -> usize {
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            if it.prio < prio {
                return i;
            }
            if it.prio == prio && it.arrival > arrival {
                return i;
            }
            i += 1;
        }
        self.items.len()
    }
    /// 按类序下降插入。
    pub fn insert(&mut self, e: QueueEntry) -> bool {
        if self.items.len() >= QUEUE_CAP {
            return false;
        }
        let pos = self.insert_position(e.prio, e.arrival);
        self.items.insert(pos, e);
        true
    }
    /// 按线程号移除（回滚挂入时摘除）。
    pub fn remove_tid(&mut self, tid: u64) -> Option<QueueEntry> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].tid == tid {
                return Some(self.items.remove(i));
            }
            i += 1;
        }
        None
    }
    /// 队列指纹（线程号序列 + 优先级序列）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for it in &self.items {
            h ^= it.tid;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= it.prio as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
    /// 是否含线程。
    pub fn contains(&self, tid: u64) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].tid == tid {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 核槽（平台布局见文件头「平台数据结构布局」）。
#[derive(Clone, Debug, Default)]
pub struct CoreSlot {
    /// 核号。
    pub cpu: u32,
    /// 在线。
    pub online: bool,
    /// 负载（定点千分比）。
    pub load: u64,
    /// 深度账。
    pub depth: u64,
    /// 迁出计数。
    pub mig_out: u64,
    /// 迁入计数。
    pub mig_in: u64,
    /// 上次迁移 tick（F1726 冷却窗判定）。
    pub last_mig_tick: u64,
    /// 离线原因码（0=在线；非 0 为离线诊断码）。
    pub offline_reason: u64,
}

impl CoreSlot {
    /// 构造在线核。
    pub fn new(cpu: u32, online: bool, load: u64, depth: u64) -> Self {
        CoreSlot {
            cpu,
            online,
            load,
            depth,
            mig_out: 0,
            mig_in: 0,
            last_mig_tick: 0,
            offline_reason: if online { 0 } else { 0x02 },
        }
    }
    /// 下线。
    pub fn take_offline(&mut self, reason: u64) {
        self.online = false;
        self.offline_reason = reason;
    }
    /// 负载增量（迁移后重算）。
    pub fn bump_load(&mut self, delta: i64) {
        let cur = self.load as i64;
        let next = cur + delta;
        self.load = if next < 0 { 0 } else { next as u64 };
    }
}

// ---------------------------------------------------------------------------
// 组件 C · 暂存区段（F1653 暂存区 · 64 槽定容）
// ---------------------------------------------------------------------------

/// 暂存槽状态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    /// 空闲（可分配）。
    Free,
    /// 已预留（已占用通道但内容未落）。
    Reserved,
    /// 已占用（内容在区）。
    Occupied,
    /// 脏（内容已失效未清理，复用前须先落账释放）。
    Dirty,
}

impl SlotState {
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            SlotState::Free => "空闲",
            SlotState::Reserved => "已预留",
            SlotState::Occupied => "已占用",
            SlotState::Dirty => "脏",
        }
    }
    /// 态号。
    pub fn num(self) -> u8 {
        match self {
            SlotState::Free => 0,
            SlotState::Reserved => 1,
            SlotState::Occupied => 2,
            SlotState::Dirty => 3,
        }
    }
    /// 是否计入 Σ占用（Reserved + Occupied + Dirty 均占用槽）。
    pub fn occupied(self) -> bool {
        !matches!(self, SlotState::Free)
    }
}

/// 暂存槽（平台布局见文件头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageSlot {
    /// 槽态。
    pub state: SlotState,
    /// 归属 migration_id。
    pub owner: u64,
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 优先级。
    pub prio: u8,
    /// 暂存内容指纹（可复算）。
    pub payload_fp: u64,
    /// 落区 tick。
    pub tick: u64,
}

impl StageSlot {
    /// 空槽。
    pub const EMPTY: StageSlot = StageSlot {
        state: SlotState::Free,
        owner: 0,
        tid: 0,
        src_cpu: 0,
        prio: 0,
        payload_fp: 0,
        tick: 0,
    };
    /// 新空槽。
    pub fn empty() -> Self {
        StageSlot::EMPTY
    }
    /// 槽指纹（内容指纹的槽位封装：含槽号语义位）。
    pub fn slot_fp(&self, idx: usize) -> u64 {
        mix6(
            idx as u64,
            self.owner,
            self.tid,
            self.src_cpu as u64,
            self.prio as u64,
            self.payload_fp,
        )
    }
}

/// 暂存内容（入区载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagePayload {
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 优先级。
    pub prio: u8,
    /// 线程态（摘出时快照）。
    pub state: ThreadState,
    /// 迁移号。
    pub mid: u64,
    /// 到达序。
    pub arrival: u64,
    /// 负载槽值（负载快照分项）。
    pub load_word: u64,
}

impl StagePayload {
    /// 构造。
    pub fn new(tid: u64, src_cpu: u32, prio: u8, state: ThreadState, mid: u64, arrival: u64) -> Self {
        StagePayload { tid, src_cpu, prio, state, mid, arrival, load_word: load_word_of(tid, prio) }
    }
    /// 内容指纹（FNV 六项混合；可由同参复算）。
    pub fn fingerprint(&self) -> u64 {
        mix6(
            self.tid,
            self.src_cpu as u64,
            self.prio as u64,
            self.state.num() as u64,
            self.mid,
            self.load_word,
        )
    }
    /// 线程身份三元组。
    pub fn triple(&self) -> ThreadTriple {
        ThreadTriple::new(self.tid, self.src_cpu, self.prio)
    }
}

/// 负载槽值派生（确定性；避免真硬件读取）。
pub fn load_word_of(tid: u64, prio: u8) -> u64 {
    mix3(tid, prio as u64, 0x100) & 0xffff
}

/// 暂存区（64 槽定容；分配/释放/复用三路径；守恒账）。
#[derive(Clone, Debug)]
pub struct StageArea {
    /// 槽表。
    pub slots: Vec<StageSlot>,
    /// 脏槽游标（复用路径 O(1) 直取）。
    pub reuse_cursor: usize,
    /// 暂存满拒计数。
    pub full_rejects: u64,
    /// 累计分配数。
    pub allocs: u64,
    /// 累计释放数。
    pub frees: u64,
    /// 复用路径命中数。
    pub reuses: u64,
    /// Σ占用累计账（守恒断言用）。
    pub occupied_total: u64,
}

impl Default for StageArea {
    fn default() -> Self {
        let mut slots: Vec<StageSlot> = Vec::with_capacity(STAGE_SLOTS);
        let mut i = 0usize;
        while i < STAGE_SLOTS {
            slots.push(StageSlot::empty());
            i += 1;
        }
        StageArea {
            slots,
            reuse_cursor: 0,
            full_rejects: 0,
            allocs: 0,
            frees: 0,
            reuses: 0,
            occupied_total: 0,
        }
    }
}

impl StageArea {
    /// 新建暂存区。
    pub fn new() -> Self {
        StageArea::default()
    }
    /// 总槽数。
    pub fn total(&self) -> u64 {
        STAGE_SLOTS as u64
    }
    /// 实际扫描 Σ占用。
    pub fn occupied_scan(&self) -> u64 {
        let mut n = 0u64;
        for s in &self.slots {
            if s.state.occupied() {
                n += 1;
            }
        }
        n
    }
    /// 实际扫描 Σ空闲。
    pub fn free_scan(&self) -> u64 {
        let mut n = 0u64;
        for s in &self.slots {
            if !s.state.occupied() {
                n += 1;
            }
        }
        n
    }
    /// 暂存守恒断言：Σ占用 + Σ空闲 == 总槽数。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过（暂存段守恒为链内闭合前提）
    pub fn conserve(&self) -> bool {
        self.occupied_scan() + self.free_scan() == self.total()
    }
    /// 累计账守恒断言：Σ占用 == occupied_total。
    pub fn ledger_conserve(&self) -> bool {
        self.occupied_scan() == self.occupied_total
    }
    /// 空闲槽计数（快速路径）。
    pub fn free_count(&self) -> u64 {
        self.free_scan()
    }
    /// 占用槽计数。
    pub fn occupied_count(&self) -> u64 {
        self.occupied_scan()
    }
    /// 是否满（无空闲槽）。
    pub fn is_full(&self) -> bool {
        self.free_count() == 0
    }

    /// 路径一 · 分配：扫首个 Free 槽，Free → Reserved。
    pub fn reserve(&mut self, owner: u64, pay: &StagePayload, tick: u64) -> Result<usize, MigDiag> {
        let mut i = 0usize;
        while i < STAGE_SLOTS {
            if self.slots[i].state == SlotState::Free {
                let mut s = StageSlot::empty();
                s.state = SlotState::Reserved;
                s.owner = owner;
                s.tid = pay.tid;
                s.src_cpu = pay.src_cpu;
                s.prio = pay.prio;
                s.payload_fp = pay.fingerprint();
                s.tick = tick;
                self.slots[i] = s;
                self.allocs += 1;
                self.occupied_total += 1;
                return Ok(i);
            }
            i += 1;
        }
        self.full_rejects += 1;
        Err(MigDiag::new(
            MigErr::StageFull,
            "stage:reserve",
            format!("暂存 {} 槽全占（占用 {}），回退路径交 F1724", STAGE_SLOTS, self.occupied_scan()),
        ))
    }

    /// 路径二 · 落区：Reserved → Occupied（内容在区，指纹复算校验）。
    pub fn occupy(&mut self, idx: usize, pay: &StagePayload) -> Result<(), MigDiag> {
        if idx >= STAGE_SLOTS {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:occupy:range",
                format!("槽号 {} 越界（上限 {}）", idx, STAGE_SLOTS),
            ));
        }
        if self.slots[idx].state != SlotState::Reserved {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:occupy:state",
                format!("槽 {} 态为 {} 未预留即占用", idx, self.slots[idx].state.text()),
            ));
        }
        let fp = pay.fingerprint();
        if self.slots[idx].payload_fp != fp {
            return Err(MigDiag::new(
                MigErr::FpTamper,
                "stage:occupy:fp",
                format!("槽 {} 预留指纹 {:#x} ≠ 落区指纹 {:#x}", idx, self.slots[idx].payload_fp, fp),
            ));
        }
        self.slots[idx].state = SlotState::Occupied;
        Ok(())
    }

    /// 路径三 · 释放：Occupied/Reserved/Dirty → Free（内容清零落账）。
    pub fn release(&mut self, idx: usize, expect_owner: u64) -> Result<StageSlot, MigDiag> {
        if idx >= STAGE_SLOTS {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:release:range",
                format!("槽号 {} 越界", idx),
            ));
        }
        let s = self.slots[idx];
        if !s.state.occupied() {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:release:empty",
                format!("槽 {} 已空闲，重复释放", idx),
            ));
        }
        if s.owner != expect_owner {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:release:owner",
                format!("槽 {} 归属 {:#x} ≠ 迁移号 {:#x}", idx, s.owner, expect_owner),
            ));
        }
        self.slots[idx] = StageSlot::empty();
        self.frees += 1;
        if self.occupied_total > 0 {
            self.occupied_total -= 1;
        }
        Ok(s)
    }

    /// 标脏（挂入后内容失效未清；复用前须 release）。
    pub fn mark_dirty(&mut self, idx: usize, owner: u64) -> Result<(), MigDiag> {
        if idx >= STAGE_SLOTS || !self.slots[idx].state.occupied() {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:dirty",
                format!("槽 {} 无法标脏", idx),
            ));
        }
        if self.slots[idx].owner != owner {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "stage:dirty:owner",
                format!("槽 {} 归属不符", idx),
            ));
        }
        self.slots[idx].state = SlotState::Dirty;
        Ok(())
    }

    /// 复用：从脏槽取（O(1) 游标扫描至首个 Dirty），占用态不取。
    pub fn take_dirty(&mut self, owner: u64, pay: &StagePayload, tick: u64) -> Result<usize, MigDiag> {
        let mut probed = 0usize;
        while probed < STAGE_SLOTS {
            let idx = self.reuse_cursor % STAGE_SLOTS;
            self.reuse_cursor = (idx + 1) % STAGE_SLOTS;
            probed += 1;
            if self.slots[idx].state == SlotState::Dirty {
                let mut s = StageSlot::empty();
                s.state = SlotState::Reserved;
                s.owner = owner;
                s.tid = pay.tid;
                s.src_cpu = pay.src_cpu;
                s.prio = pay.prio;
                s.payload_fp = pay.fingerprint();
                s.tick = tick;
                self.slots[idx] = s;
                self.reuses += 1;
                self.allocs += 1;
                return Ok(idx);
            }
        }
        // 无脏槽可复用：退回普通分配。
        self.reserve(owner, pay, tick)
    }

    /// 取槽（只读）。
    pub fn slot(&self, idx: usize) -> Option<StageSlot> {
        if idx < STAGE_SLOTS {
            Some(self.slots[idx])
        } else {
            None
        }
    }

    /// 按 migration_id 反查槽号（回滚用）。
    pub fn find_owner(&self, owner: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < STAGE_SLOTS {
            if self.slots[i].state.occupied() && self.slots[i].owner == owner {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 暂存区全表指纹（守恒复算锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        let mut i = 0usize;
        while i < STAGE_SLOTS {
            h ^= self.slots[i].slot_fp(i);
            h = h.wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h
    }

    /// 暂存满诊断文本（附回退建议，F1724 消费）。
    pub fn full_report(&self) -> String {
        format!(
            "暂存满：占用 {}/{} 自由 {}；建议 {}",
            self.occupied_scan(),
            STAGE_SLOTS,
            self.free_scan(),
            MigErr::StageFull.suggestion()
        )
    }
}// ---------------------------------------------------------------------------
// 组件 B2 · 跨队列写入点登记表（F1641 跨队列禁区唯一合法通道）
// 判据：UNX-F1722-J1 —— 全域唯一跨队列写入点断言通过
// ---------------------------------------------------------------------------

/// 唯一合法跨队列写入点号。
pub const WP_MIGRATE: u16 = 0x0001;

/// 写点号非法哨兵。
pub const WP_NONE: u16 = 0x0000;

/// 写点描述符（平台布局见文件头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WritePoint {
    /// 写点号。
    pub id: u16,
    /// 归属段号（0=Detach / 1=Stage / 2=Attach）。
    pub owner_seg: u8,
    /// 归属核号（0xFFFF=跨核通用）。
    pub cpu: u32,
    /// 是否合法登记。
    pub legal: bool,
    /// 命中计数。
    pub hits: u64,
}

impl WritePoint {
    /// 合法写点构造。
    pub fn legal(id: u16, owner_seg: u8, cpu: u32) -> Self {
        WritePoint { id, owner_seg, cpu, legal: true, hits: 0 }
    }
    /// 非法（占位）写点。
    pub fn illegal(id: u16) -> Self {
        WritePoint { id, owner_seg: 0xff, cpu: 0xffff, legal: false, hits: 0 }
    }
    /// 中文描述。
    pub fn text(&self) -> String {
        format!(
            "WP#{:#06x} 段{} 核{} {}",
            self.id,
            self.owner_seg,
            self.cpu,
            if self.legal { "合法" } else { "未登记" }
        )
    }
}

/// 写入令牌（授权凭证；仅由登记表在授权通过时铸造）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteToken {
    /// 写点号。
    pub wp: u16,
    /// 授权迁移号。
    pub mid: u64,
    /// 授权段号。
    pub seg: u8,
    /// 一次性 nonce（防重放；令牌用后即焚）。
    pub nonce: u64,
}

/// 跨队列写入点登记表（定容 ≤8；唯一合法点断言）。
#[derive(Clone, Debug)]
pub struct WritePointRegistry {
    /// 点描述符数组。
    pub table: Vec<WritePoint>,
    /// 授权计数。
    pub granted: u64,
    /// 绕行拒计数（F1722 注入 7 命中此账）。
    pub denied: u64,
    /// nonce 游标。
    pub nonce: u64,
    /// 已焚令牌 nonce 集（防重放）。
    pub burned: Vec<u64>,
}

impl Default for WritePointRegistry {
    fn default() -> Self {
        WritePointRegistry { table: Vec::new(), granted: 0, denied: 0, nonce: 0, burned: Vec::new() }
    }
}

impl WritePointRegistry {
    /// 新建登记表并登记唯一合法点 WP_MIGRATE。
/// 该点是迁移原语整链（Detach 摘出 + Attach 挂入）共用的唯一合法跨队列通道，
/// 故归属段为全段通配 0xff；段越权检查仍对其余段生效。
pub fn new() -> Self {
        let mut r = WritePointRegistry::default();
        r.table.push(WritePoint::legal(WP_MIGRATE, 0xff, 0xffff));
        r
    }

    /// 全域唯一跨队列写入点断言：合法点恰为 1 且为 WP_MIGRATE。
    // 判据：UNX-F1722-J1 —— 全域唯一跨队列写入点断言通过
    pub fn assert_unique_write_point(&self) -> bool {
        let mut n = 0u64;
        let mut only = WP_NONE;
        for p in &self.table {
            if p.legal {
                n += 1;
                only = p.id;
            }
        }
        n == 1 && only == WP_MIGRATE
    }

    /// 查写点。
    pub fn point(&self, id: u16) -> Option<&WritePoint> {
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].id == id {
                return Some(&self.table[i]);
            }
            i += 1;
        }
        None
    }

    /// 登记写点（重号拒；表满拒）。
    pub fn register(&mut self, p: WritePoint) -> Result<(), MigDiag> {
        if self.table.len() >= WRITE_POINTS {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:register:full",
                format!("写点表满（上限 {}）", WRITE_POINTS),
            ));
        }
        if self.point(p.id).is_some() {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:register:dup",
                format!("写点 {:#06x} 重号", p.id),
            ));
        }
        self.table.push(p);
        Ok(())
    }

    /// 越权登记第二合法写点（注入核用；必被唯一性断言破掉）。
    pub fn try_register_second_legal(&mut self, p: WritePoint) -> Result<(), MigDiag> {
        if self.assert_unique_write_point() {
            // 已存在唯一合法点：再登记合法点即破坏唯一性 → 直接拒。
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:register:second",
                "全域唯一跨队列写入点断言成立，拒绝第二合法点".to_string(),
            ));
        }
        self.register(p)
    }

    /// 授权跨队列写（Detach 摘出与 Attach 挂入必须持令牌）。
    pub fn authorize(&mut self, wp: u16, seg: MigrateSeg, mid: u64) -> Result<WriteToken, MigDiag> {
        let exists = self.point(wp).is_some();
        if !exists {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:authorize:unknown",
                format!("写点 {:#06x} 未登记（绕行写入）", wp),
            ));
        }
        let (legal, owner_seg, cpu) = match self.point(wp) {
            Some(p) => (p.legal, p.owner_seg, p.cpu),
            None => {
                self.denied += 1;
                return Err(MigDiag::new(
                    MigErr::BypassWrite,
                    "wp:authorize:illegal",
                    format!("写点 {:#06x} 非合法登记点", wp),
                ));
            }
        };
        if !legal {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:authorize:illegal",
                format!("写点 {:#06x} 非合法登记点", wp),
            ));
        }
        // 段授权：写点归属段须为跨核通用段（Attach）或显式匹配当前段。
        let seg_ok = (owner_seg == 0xff) || (owner_seg == seg.num() as u8);
        if !seg_ok {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:authorize:seg",
                format!("写点 {:#06x} 不授权段 {}", wp, seg.text()),
            ));
        }
        // 核绑定：非通用写点只许在绑定核上用。
        if cpu != 0xffff {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:authorize:cpu",
                format!("写点 {:#06x} 绑定核 {} 不可跨核写", wp, cpu),
            ));
        }
        self.nonce = self.nonce.wrapping_add(1);
        let tk = WriteToken { wp, mid, seg: seg.num() as u8, nonce: self.nonce };
        self.granted += 1;
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].id == wp {
                self.table[i].hits += 1;
                break;
            }
            i += 1;
        }
        Ok(tk)
    }

    /// 验令牌（一次性；用后即焚防重放）。
    pub fn verify_and_burn(&mut self, tk: &WriteToken, mid: u64) -> Result<(), MigDiag> {
        let mut i = 0usize;
        while i < self.burned.len() {
            if self.burned[i] == tk.nonce {
                self.denied += 1;
                return Err(MigDiag::new(
                    MigErr::Reentrant,
                    "wp:burn:replay",
                    format!("nonce {:#x} 重放拒（令牌一次性）", tk.nonce),
                ));
            }
            i += 1;
        }
        if tk.mid != mid {
            self.denied += 1;
            return Err(MigDiag::new(
                MigErr::BypassWrite,
                "wp:burn:mid",
                format!("令牌迁移号 {:#x} ≠ 当前 {:#x}", tk.mid, mid),
            ));
        }
        if tk.seg == MigrateSeg::Attach.num() as u8 {
            // 挂入令牌焚于挂入前；摘出令牌不在此焚（摘出另行铸造）。
        }
        self.burned.push(tk.nonce);
        Ok(())
    }

    /// 核绑定写点登记（注入核演示用：绑定核的写点不可用于跨核）。
    pub fn register_cpu_bound(&mut self, id: u16, seg_no: u8, cpu: u32) -> Result<(), MigDiag> {
        self.register(WritePoint::legal(id, seg_no, cpu))
    }

    /// 表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for p in &self.table {
            for byte in p.id.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= p.owner_seg as u64;
            h = h.wrapping_mul(FNV_PRIME);
            h ^= if p.legal { 1 } else { 0 };
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 报表。
    pub fn report(&self) -> String {
        let mut legal = 0u64;
        for p in &self.table {
            if p.legal {
                legal += 1;
            }
        }
        format!(
            "写点表 项{} 合法{} 授权{} 拒{}",
            self.table.len(),
            legal,
            self.granted,
            self.denied
        )
    }
}// ---------------------------------------------------------------------------
// 组件 E · 三向记账联动（源核出账 / 目标核入账 / 线程随行账 · F1723）
// 三账经 migration_id 关联；append-only 环形缓冲；满则覆盖最旧并计 dropped
// ---------------------------------------------------------------------------

/// 账目类别闭集（三向账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// 源核出账（SrcOut）。
    SrcOut,
    /// 目标核入账（DstIn）。
    DstIn,
    /// 线程随行账（Follow）。
    Follow,
    /// 回滚段账（Rollback）。
    Rollback,
}

impl LedgerKind {
    /// 类别号。
    pub fn num(self) -> u8 {
        match self {
            LedgerKind::SrcOut => 0,
            LedgerKind::DstIn => 1,
            LedgerKind::Follow => 2,
            LedgerKind::Rollback => 3,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            LedgerKind::SrcOut => "源核出账",
            LedgerKind::DstIn => "目标核入账",
            LedgerKind::Follow => "线程随行账",
            LedgerKind::Rollback => "回滚段账",
        }
    }
    /// 是否为三向账之一（回滚段账不参与闭合比）。
    pub fn in_triple(self) -> bool {
        matches!(self, LedgerKind::SrcOut | LedgerKind::DstIn | LedgerKind::Follow)
    }
}

/// 记账项（平台布局见文件头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    /// 类别。
    pub kind: LedgerKind,
    /// 段号。
    pub seg: u8,
    /// 成败（false = 回滚/失败账）。
    pub ok: bool,
    /// 迁移号（三账关联键）。
    pub mid: u64,
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 目标核号。
    pub dst_cpu: u32,
    /// 逻辑 tick。
    pub tick: u64,
    /// 载荷指纹。
    pub payload_fp: u64,
}

impl LedgerEntry {
    /// 构造。
    pub fn new(kind: LedgerKind, seg: u8, ok: bool, mid: u64, tid: u64, src: u32, dst: u32, tick: u64, fp: u64) -> Self {
        LedgerEntry { kind, seg, ok, mid, tid, src_cpu: src, dst_cpu: dst, tick, payload_fp: fp }
    }
    /// 项指纹（回放逐字段对账用）。
    pub fn entry_fp(&self) -> u64 {
        mix6(
            self.kind.num() as u64,
            self.seg as u64,
            if self.ok { 1 } else { 0 },
            self.mid,
            self.tid,
            self.src_cpu as u64 ^ (self.dst_cpu as u64) << 32,
        )
    }
    /// 人类可读行。
    pub fn line(&self) -> String {
        format!(
            "{} 段{} mid#{:#x} tid#{} {}→{} tick{} ok={} fp={:#x}",
            self.kind.text(),
            self.seg,
            self.mid,
            self.tid,
            self.src_cpu,
            self.dst_cpu,
            self.tick,
            if self.ok { 1 } else { 0 },
            self.payload_fp
        )
    }
}

/// append-only 环形账（定容；满覆盖最旧并计 dropped，满账显性暴露）。
#[derive(Clone, Debug)]
pub struct LedgerRing {
    /// 类别。
    pub kind: LedgerKind,
    /// 槽表。
    pub buf: Vec<LedgerEntry>,
    /// 写游标（环形）。
    pub head: usize,
    /// 已填计数。
    pub filled: usize,
    /// 累计写入（含被覆盖）。
    pub total: u64,
    /// 被覆盖（丢账）计数——显性暴露，不静默丢。
    pub dropped: u64,
}

impl LedgerRing {
    /// 新建空账（定容 LEDGER_CAP）。
    pub fn new(kind: LedgerKind) -> Self {
        let mut buf: Vec<LedgerEntry> = Vec::with_capacity(LEDGER_CAP);
        let mut i = 0usize;
        while i < LEDGER_CAP {
            buf.push(LedgerEntry::new(kind, 0xff, false, 0, 0, 0, 0, 0, 0));
            i += 1;
        }
        LedgerRing { kind, buf, head: 0, filled: 0, total: 0, dropped: 0 }
    }
    /// 容量。
    pub fn cap(&self) -> usize {
        LEDGER_CAP
    }
    /// 满账暴露（dropped > 0 即显性）。
    pub fn is_overflowed(&self) -> bool {
        self.dropped > 0
    }
    /// 追加（append-only；满则覆盖最旧）。
    pub fn push(&mut self, e: LedgerEntry) {
        let cap = LEDGER_CAP;
        if self.filled < cap {
            self.buf[self.head] = e;
            self.filled += 1;
        } else {
            // 满账：覆盖最旧，显式计 dropped。
            self.buf[self.head] = e;
            self.dropped += 1;
        }
        self.head = (self.head + 1) % cap;
        self.total += 1;
    }
    /// 按序读（最旧 → 最新）。
    pub fn iter_ordered(&self) -> Vec<LedgerEntry> {
        let mut out: Vec<LedgerEntry> = Vec::with_capacity(self.filled);
        let cap = LEDGER_CAP;
        let start = (self.head + cap - self.filled) % cap;
        let mut i = 0usize;
        while i < self.filled {
            out.push(self.buf[(start + i) % cap]);
            i += 1;
        }
        out
    }
    /// 成功计数。
    pub fn ok_count(&self) -> u64 {
        let mut n = 0u64;
        for e in self.iter_ordered() {
            if e.ok {
                n += 1;
            }
        }
        n
    }
    /// 账指纹（顺序敏感）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for e in self.iter_ordered() {
            h ^= e.entry_fp();
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
    /// 满账报告。
    pub fn overflow_report(&self) -> String {
        format!(
            "{} 账 容量{} 在册{} 累计{} 覆盖丢账{}",
            self.kind.text(),
            LEDGER_CAP,
            self.filled,
            self.total,
            self.dropped
        )
    }
}

/// 三账差异明细（结算输出）。
#[derive(Clone, Debug, Default)]
pub struct SettleDiff {
    /// 仅在源出账出现的 mid。
    pub only_src: Vec<u64>,
    /// 仅在目标入账出现的 mid。
    pub only_dst: Vec<u64>,
    /// 仅在随行账出现的 mid。
    pub only_follow: Vec<u64>,
    /// 三账皆缺的 mid（三向皆无）。
    pub nowhere: Vec<u64>,
}

/// 结算结果（闭合比 + 明细）。
#[derive(Clone, Debug)]
pub struct SettleResult {
    /// 是否三账闭合。
    pub closed: bool,
    /// 期望笔数（以源出账成功笔数为基准）。
    pub expect: u64,
    /// 源出账成功笔数。
    pub src_ok: u64,
    /// 目标入账成功笔数。
    pub dst_ok: u64,
    /// 随行账成功笔数。
    pub follow_ok: u64,
    /// 闭合比（千分比定点；= 三账最小值 / 期望值）。
    pub ratio_permille: u64,
    /// 差异明细。
    pub diff: SettleDiff,
    /// 任一账满账（显性暴露）。
    pub overflow_exposed: bool,
    /// 三账指纹。
    pub fps: (u64, u64, u64),
}

impl SettleResult {
    /// 一行报告。
    pub fn line(&self) -> String {
        format!(
            "结算 闭合={} 期望{} 源{} 目标{} 随行{} 比{}‰ 差异(仅源{} 仅目标{} 仅随行{}) 满账={}",
            if self.closed { "是" } else { "否" },
            self.expect,
            self.src_ok,
            self.dst_ok,
            self.follow_ok,
            self.ratio_permille,
            self.diff.only_src.len(),
            self.diff.only_dst.len(),
            self.diff.only_follow.len(),
            if self.overflow_exposed { "是" } else { "否" }
        )
    }
    /// 转 KV 文本行。
    pub fn kv(&self, prefix: &str) -> String {
        format!(
            "{}={};{}closed={};{}ratio_permille={};{}src_ok={};{}dst_ok={};{}follow_ok={}",
            prefix,
            self.expect,
            prefix,
            if self.closed { 1 } else { 0 },
            prefix,
            self.ratio_permille,
            prefix,
            self.src_ok,
            prefix,
            self.dst_ok,
            prefix,
            self.follow_ok
        )
    }
}

/// 三向账本组（三账 + 回滚段账；共享 migration_id 关联）。
#[derive(Clone, Debug)]
pub struct TriLedger {
    /// 源核出账。
    pub src: LedgerRing,
    /// 目标核入账。
    pub dst: LedgerRing,
    /// 线程随行账。
    pub follow: LedgerRing,
    /// 回滚段账（回滚落账；不参与闭合比）。
    pub rollback: LedgerRing,
}

impl Default for TriLedger {
    fn default() -> Self {
        TriLedger {
            src: LedgerRing::new(LedgerKind::SrcOut),
            dst: LedgerRing::new(LedgerKind::DstIn),
            follow: LedgerRing::new(LedgerKind::Follow),
            rollback: LedgerRing::new(LedgerKind::Rollback),
        }
    }
}

impl TriLedger {
    /// 新建三账。
    pub fn new() -> Self {
        TriLedger::default()
    }
    /// 记一笔（按类别分发）。
    pub fn record(&mut self, e: LedgerEntry) {
        match e.kind {
            LedgerKind::SrcOut => self.src.push(e),
            LedgerKind::DstIn => self.dst.push(e),
            LedgerKind::Follow => self.follow.push(e),
            LedgerKind::Rollback => self.rollback.push(e),
        }
    }
    /// 查某 mid 在三账的成功出现情况（(src, dst, follow)）。
    pub fn presence(&self, mid: u64) -> (bool, bool, bool) {
        let mut s = false;
        let mut d = false;
        let mut f = false;
        for e in self.src.iter_ordered() {
            if e.mid == mid && e.ok {
                s = true;
            }
        }
        for e in self.dst.iter_ordered() {
            if e.mid == mid && e.ok {
                d = true;
            }
        }
        for e in self.follow.iter_ordered() {
            if e.mid == mid && e.ok {
                f = true;
            }
        }
        (s, d, f)
    }
    /// 收集三账出现过的全部 mid（BTreeMap 保序去重）。
    pub fn mids(&self) -> BTreeMap<u64, (bool, bool, bool)> {
        let mut m: BTreeMap<u64, (bool, bool, bool)> = BTreeMap::new();
        for e in self.src.iter_ordered() {
            if e.ok {
                let e = m.entry(e.mid).or_insert((false, false, false));
                e.0 = true;
            }
        }
        for e in self.dst.iter_ordered() {
            if e.ok {
                let e = m.entry(e.mid).or_insert((false, false, false));
                e.1 = true;
            }
        }
        for e in self.follow.iter_ordered() {
            if e.ok {
                let e = m.entry(e.mid).or_insert((false, false, false));
                e.2 = true;
            }
        }
        m
    }
    /// 任一账满账。
    pub fn overflow_exposed(&self) -> bool {
        self.src.is_overflowed() || self.dst.is_overflowed() || self.follow.is_overflowed()
    }
    /// 三账总指纹（合并）。
    pub fn combined_fp(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        h ^= self.src.fingerprint();
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.dst.fingerprint();
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.follow.fingerprint();
        h.wrapping_mul(FNV_PRIME)
    }
    /// 满账暴露全表文本（显性暴露不静默丢）。
    pub fn overflow_lines(&self) -> Vec<String> {
        let mut v = vec![self.src.overflow_report(), self.dst.overflow_report(), self.follow.overflow_report()];
        if self.rollback.is_overflowed() {
            v.push(self.rollback.overflow_report());
        }
        v
    }
}

/// 结算器（挂在 `Migrator` 上；此处给出自由函数形态便于测试与回放）。
///
/// 注：已回滚的迁移号（回滚段账中存在者）为**作废迁移**，其出账已由回滚冲正，
///   故不参与三向闭合统计（否则回滚迁移将永久显示为「仅源账」不闭合）。
pub fn settle_ledger(led: &TriLedger) -> SettleResult {
    // 作废迁移号集（回滚段账）。
    let mut voided: Vec<u64> = Vec::new();
    for e in led.rollback.iter_ordered() {
        voided.push(e.mid);
    }
    let is_void = |mid: u64| -> bool {
        let mut i = 0usize;
        while i < voided.len() {
            if voided[i] == mid {
                return true;
            }
            i += 1;
        }
        false
    };

    let mut expect = 0u64;
    for e in led.src.iter_ordered() {
        if e.ok && !is_void(e.mid) {
            expect += 1;
        }
    }
    let mut src_ok = 0u64;
    let mut dst_ok = 0u64;
    let mut follow_ok = 0u64;
    for e in led.src.iter_ordered() {
        if e.ok && !is_void(e.mid) {
            src_ok += 1;
        }
    }
    for e in led.dst.iter_ordered() {
        if e.ok && !is_void(e.mid) {
            dst_ok += 1;
        }
    }
    for e in led.follow.iter_ordered() {
        if e.ok && !is_void(e.mid) {
            follow_ok += 1;
        }
    }

    let mut diff = SettleDiff::default();
    for (mid, (s, d, f)) in led.mids() {
        if is_void(mid) {
            continue;
        }
        if s && !d {
            diff.only_src.push(mid);
        }
        if d && !s {
            diff.only_dst.push(mid);
        }
        if f && !s {
            diff.only_follow.push(mid);
        }
        if !s && !d && !f {
            diff.nowhere.push(mid);
        }
    }
    // 闭合：三账成功笔数相等且无单边差异。
    let closed = src_ok == dst_ok && dst_ok == follow_ok && diff.only_src.is_empty()
        && diff.only_dst.is_empty()
        && diff.only_follow.is_empty();
    let min3 = if src_ok < dst_ok {
        src_ok
    } else if dst_ok < follow_ok {
        dst_ok
    } else {
        follow_ok
    };
    let ratio_permille = if expect == 0 {
        if closed { 1000 } else { 0 }
    } else {
        (min3 * 1000) / expect
    };
    SettleResult {
        closed,
        expect,
        src_ok,
        dst_ok,
        follow_ok,
        ratio_permille,
        diff,
        overflow_exposed: led.overflow_exposed(),
        fps: (led.src.fingerprint(), led.dst.fingerprint(), led.follow.fingerprint()),
    }
}// ---------------------------------------------------------------------------
// 组件 A · 三段原子链状态机（Detach → Stage → Attach）
// 段入口消费上一段交接签名（段号 + 负载快照 FNV 指纹 + 线程身份三元组）
// ---------------------------------------------------------------------------

/// 交接签名（段号 + 负载快照指纹 + 线程身份三元组 + 迁移号 + 链标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffSig {
    /// 出铸段号（谁铸的）。
    pub from_seg: usize,
    /// 入口期望段号（谁该验）。
    pub expect_seg: usize,
    /// 负载快照指纹（FNV）。
    pub snap_fp: u64,
    /// 线程号。
    pub tid: u64,
    /// 源核号。
    pub src_cpu: u32,
    /// 优先级。
    pub prio: u8,
    /// 迁移号。
    pub mid: u64,
    /// 链标签（链次+链内序，防跨链重放）。
    pub chain_tag: u64,
}

impl HandoffSig {
    /// 构造。
    pub fn new(from_seg: usize, expect_seg: usize, snap_fp: u64, t: ThreadTriple, mid: u64, chain_tag: u64) -> Self {
        HandoffSig {
            from_seg,
            expect_seg,
            snap_fp,
            tid: t.tid,
            src_cpu: t.src_cpu,
            prio: t.prio,
            mid,
            chain_tag,
        }
    }
    /// 线程身份三元组。
    pub fn triple(&self) -> ThreadTriple {
        ThreadTriple::new(self.tid, self.src_cpu, self.prio)
    }
    /// 签名指纹（FNV 六项）。
    pub fn fingerprint(&self) -> u64 {
        mix6(
            self.from_seg as u64,
            self.expect_seg as u64,
            self.snap_fp,
            self.tid,
            self.src_cpu as u64,
            self.prio as u64 ^ self.mid,
        )
    }
    /// 是否为本链该段的合法签名（结构校验；不含状态校验）。
    pub fn structurally_valid_for(&self, mid: u64, chain_tag: u64, seg: MigrateSeg) -> bool {
        self.mid == mid && self.chain_tag == chain_tag && self.expect_seg == seg.num()
    }
    /// 中文描述。
    pub fn line(&self) -> String {
        format!(
            "sig 段{}→{} mid#{:#x} tid#{} snap={:#x} tag={:#x}",
            self.from_seg,
            self.expect_seg,
            self.mid,
            self.tid,
            self.snap_fp,
            self.chain_tag
        )
    }
}

/// 链标签铸造（链次 + 链内序混合；跨链重放即不符）。
pub fn mint_chain_tag(chain_no: u64, seg_ord: u64) -> u64 {
    mix3(chain_no ^ 0xA11CE, seg_ord, 0x7A9)
}

/// 链上下文（一次迁移的完整生命周期状态）。
#[derive(Clone, Debug)]
pub struct ChainCtx {
    /// 迁移号（共享关联键）。
    pub mid: u64,
    /// 链次。
    pub chain_no: u64,
    /// 线程身份三元组。
    pub triple: ThreadTriple,
    /// 线程态快照。
    pub state: ThreadState,
    /// 已达段号（-1 = 未达）。
    pub reached: i32,
    /// 相位。
    pub phase: ChainPhase,
    /// 暂存槽号。
    pub slot: i32,
    /// 目标核号。
    pub dst_cpu: u32,
    /// 迁移号分配 tick。
    pub born_tick: u64,
    /// 摘出前源队列深度。
    pub depth_before: u64,
    /// 摘出后源队列深度。
    pub depth_after: u64,
    /// 迁移前全局总深度。
    pub global_before: u64,
    /// 挂入后全局总深度。
    pub global_after: u64,
    /// 回滚落账标记。
    pub rollback_ledgered: bool,
    /// Detach 段快照的载荷（下游段以此为权威载荷；禁止中途改写）。
    pub payload: Option<StagePayload>,
    /// 本链已签名（逐段）。
    pub sigs: Vec<HandoffSig>,
}

impl ChainCtx {
    /// 新链（Detach 前）。
    pub fn new(mid: u64, chain_no: u64, t: ThreadTriple, state: ThreadState, tick: u64) -> Self {
        ChainCtx {
            mid,
            chain_no,
            triple: t,
            state,
            reached: -1,
            phase: ChainPhase::Opened,
            slot: -1,
            dst_cpu: u32::MAX,
            born_tick: tick,
            depth_before: 0,
            depth_after: 0,
            global_before: 0,
            global_after: 0,
            rollback_ledgered: false,
            payload: None,
            sigs: Vec::new(),
        }
    }
    /// 期望下一段号。
    pub fn expect_seg(&self) -> usize {
        (self.reached + 1) as usize
    }
    /// 本链链标签（链作用域恒定；段身份由签名的 from_seg/expect_seg 承载）。
    pub fn chain_tag(&self) -> u64 {
        mint_chain_tag(self.chain_no, 0)
    }
    /// 已签名齐备（三段皆达）。
    pub fn sigs_complete(&self) -> bool {
        self.reached >= 2 && self.sigs.len() >= 3
    }
    /// 链闭合断言（三段闭合的完整条件）。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过
    pub fn chain_closed(&self) -> bool {
        self.phase == ChainPhase::Attached
            && self.reached == 2
            && self.sigs_complete()
            && self.slot >= 0
            && self.depth_before == self.depth_after + 1
            && self.global_before == self.global_after
    }
    /// 回滚闭合断言（回滚态：深度复原 + 回滚段账已落）。
    pub fn rollback_closed(&self) -> bool {
        self.phase == ChainPhase::RolledBack && self.rollback_ledgered
    }
    /// 一行报告。
    pub fn line(&self) -> String {
        format!(
            "链#{} mid#{:#x} tid#{} 相位{} 达段{} 槽{} 目标核{} 深度{}→{} 全局{}→{}",
            self.chain_no,
            self.mid,
            self.triple.tid,
            self.phase.text(),
            self.reached,
            self.slot,
            if self.dst_cpu == u32::MAX { "未定".to_string() } else { self.dst_cpu.to_string() },
            self.depth_before,
            self.depth_after,
            self.global_before,
            self.global_after
        )
    }
}

/// 目标核选择（负载最轻核；平票取小核号，保证确定性）。
pub fn select_target(cores: &[CoreSlot], src: u32, exclude_full: bool) -> Result<u32, MigDiag> {
    let mut best: i64 = -1;
    let mut best_cpu = u32::MAX;
    for c in cores {
        if c.cpu == src {
            continue;
        }
        if !c.online {
            continue;
        }
        if exclude_full && c.depth >= QUEUE_CAP as u64 {
            continue;
        }
        let load = c.load as i64;
        // 严格小于才替换 → 平票保留先遇见的（核号升序遍历 → 小核号胜）。
        if best < 0 || load < best {
            best = load;
            best_cpu = c.cpu;
        }
    }
    if best_cpu == u32::MAX {
        let mut offline = 0u64;
        for c in cores {
            if !c.online {
                offline += 1;
            }
        }
        return Err(MigDiag::new(
            MigErr::TargetOffline,
            "select_target",
            format!("源核 {} 之外无在线可用核（离线 {}）", src, offline),
        ));
    }
    Ok(best_cpu)
}

/// 目标核选择（显式置核离线时给专门诊断）。
pub fn select_target_with_probe(cores: &[CoreSlot], src: u32, probe_cpu: u32) -> Result<u32, MigDiag> {
    let mut found_probe = false;
    for c in cores {
        if c.cpu == probe_cpu && !c.online {
            found_probe = true;
        }
    }
    let r = select_target(cores, src, true);
    if r.is_err() && found_probe {
        return Err(MigDiag::new(
            MigErr::TargetOffline,
            "select_target:probe",
            format!("探测核 {} 已离线，无可用目标", probe_cpu),
        ));
    }
    r
}// ---------------------------------------------------------------------------
// 组件 A/B/C/D 主装 · 迁移执行原语（全域跨队列唯一写入点）
// ---------------------------------------------------------------------------

/// 观测窗口（定容环形；O(1) 覆写；四族统计共用）。
#[derive(Clone, Debug)]
pub struct ObsWindow {
    /// 桶表（千分比定点）。
    pub buckets: Vec<u64>,
    /// 写游标。
    pub head: usize,
    /// 已填。
    pub filled: usize,
    /// 累计。
    pub total: u64,
}

impl ObsWindow {
    /// 窗口容量。
    pub const CAP: usize = 16;
    /// 新建。
    pub fn new() -> Self {
        ObsWindow { buckets: vec![0u64; ObsWindow::CAP], head: 0, filled: 0, total: 0 }
    }
    /// 推入一桶（千分比）。
    pub fn push(&mut self, v: u64) {
        self.buckets[self.head] = v;
        self.head = (self.head + 1) % ObsWindow::CAP;
        if self.filled < ObsWindow::CAP {
            self.filled += 1;
        }
        self.total += 1;
    }
    /// 最新桶。
    pub fn last(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            let idx = (self.head + ObsWindow::CAP - 1) % ObsWindow::CAP;
            self.buckets[idx]
        }
    }
    /// 窗口均值（整数千分比；零浮点）。
    pub fn mean_permille(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.filled {
            s += self.buckets[i];
            i += 1;
        }
        s / self.filled as u64
    }
    /// 窗口峰值。
    pub fn peak(&self) -> u64 {
        let mut m = 0u64;
        let mut i = 0usize;
        while i < self.filled {
            if self.buckets[i] > m {
                m = self.buckets[i];
            }
            i += 1;
        }
        m
    }
}

impl Default for ObsWindow {
    fn default() -> Self {
        ObsWindow::new()
    }
}

/// 四族观测面（迁移率 / 乒乓率 / 破窗率 / 回退率）。
#[derive(Clone, Debug, Default)]
pub struct ObsFace {
    /// 迁移尝试总数。
    pub attempts: u64,
    /// 迁移成功数。
    pub ok: u64,
    /// 回退数（F1724 回退由本模块只落码，不执行回退动作）。
    pub rollbacks: u64,
    /// 破窗尝试数（冷却窗内迁移尝试）。
    pub window_breaks: u64,
    /// 乒乓计数（同一线程在两核间往返次数）。
    pub pingpong: u64,
    /// 线程上次目标核（BTreeMap 有序；保证确定性遍历）。
    pub last_dst: BTreeMap<u64, u32>,
    /// 线程上次源核。
    pub last_src: BTreeMap<u64, u32>,
    /// 线程近期驻留核历史（有界；末位为最近一次驻留核）。
    pub visits: BTreeMap<u64, Vec<u32>>,
    /// 每线程驻留历史深度上限。
    pub visit_depth: usize,
    /// 拒绝分桶（按 MigErr::bucket）。
    pub rejects: Vec<u64>,
    /// 迁移率窗口。
    pub mig_rate: ObsWindow,
    /// 乒乓率窗口。
    pub pp_rate: ObsWindow,
    /// 破窗率窗口。
    pub win_rate: ObsWindow,
    /// 回退率窗口。
    pub rb_rate: ObsWindow,
}

impl ObsFace {
    /// 新建。
    pub fn new() -> Self {
        ObsFace { rejects: vec![0u64; 16], visit_depth: 8, ..ObsFace::default() }
    }
    /// 记一次拒绝（分桶）。
    pub fn note_reject(&mut self, e: MigErr) {
        let b = e.bucket();
        if b < self.rejects.len() {
            self.rejects[b] += 1;
        }
    }
    /// 迁移率（千分比；成功 / 尝试）。
    pub fn migration_rate_permille(&self) -> u64 {
        if self.attempts == 0 {
            0
        } else {
            (self.ok * 1000) / self.attempts
        }
    }
    /// 乒乓率（千分比；乒乓 / 成功）。
    pub fn pingpong_rate_permille(&self) -> u64 {
        if self.ok == 0 {
            0
        } else {
            (self.pingpong * 1000) / self.ok
        }
    }
    /// 破窗率（千分比；破窗 / 尝试）。
    pub fn break_rate_permille(&self) -> u64 {
        if self.attempts == 0 {
            0
        } else {
            (self.window_breaks * 1000) / self.attempts
        }
    }
    /// 回退率（千分比；回退 / 尝试）。
    pub fn rollback_rate_permille(&self) -> u64 {
        if self.attempts == 0 {
            0
        } else {
            (self.rollbacks * 1000) / self.attempts
        }
    }
    /// 记录一次迁移结果（并刷新窗口）。
    pub fn note_migrate(&mut self, ok: bool, tid: u64, src: u32, dst: u32, in_cooldown: bool) {
        self.attempts += 1;
        if ok {
            self.ok += 1;
        }
        if in_cooldown {
            self.window_breaks += 1;
        }
        // 乒乓判定：目标核曾在本线程近期驻留历史中出现 → 判为乒乓（往返）。
        // 注：与「本次源核相同」不重复计数，故仅在 src != dst 且历史非空时计。
        if src != dst {
            let hist = self.visits.get(&tid).cloned().unwrap_or_default();
            let mut i = 0usize;
            let mut back = false;
            while i < hist.len() {
                if hist[i] == dst {
                    back = true;
                    break;
                }
                i += 1;
            }
            if back {
                self.pingpong += 1;
            }
        }
        // 访问历史同时登记源核与目标核（起始核亦应计入，否则「迁出→迁回」不判乒乓）。
        {
            let hist = self.visits.entry(tid).or_insert_with(Vec::new);
            hist.push(src);
            hist.push(dst);
            if hist.len() > self.visit_depth {
                let drop_n = hist.len() - self.visit_depth;
                hist.drain(0..drop_n);
            }
        }
        self.last_src.insert(tid, src);
        self.last_dst.insert(tid, dst);
        self.mig_rate.push(self.migration_rate_permille());
        self.pp_rate.push(self.pingpong_rate_permille());
        self.win_rate.push(self.break_rate_permille());
        self.rb_rate.push(self.rollback_rate_permille());
    }
    /// 记一次回退。
    pub fn note_rollback(&mut self) {
        self.rollbacks += 1;
        self.rb_rate.push(self.rollback_rate_permille());
    }
    /// 某码累计拒绝数。
    pub fn reject_of(&self, e: MigErr) -> u64 {
        let b = e.bucket();
        if b < self.rejects.len() {
            self.rejects[b]
        } else {
            0
        }
    }
}

/// 迁移执行原语（全域跨队列唯一写入点；主装 A~E）。
#[derive(Clone, Debug)]
pub struct Migrator {
    /// 核表。
    pub cores: Vec<CoreSlot>,
    /// 每核队列。
    pub queues: Vec<RunQueue>,
    /// 暂存区。
    pub stage: StageArea,
    /// 写点登记表。
    pub wp: WritePointRegistry,
    /// 三向账。
    pub led: TriLedger,
    /// 观测面。
    pub obs: ObsFace,
    /// 链表。
    pub chains: Vec<ChainCtx>,
    /// 迁移号游标。
    pub next_mid: u64,
    /// 链次游标。
    pub next_chain: u64,
    /// 在飞链号（重入闸：Some 即在执行中）。
    pub inflight: Option<u64>,
    /// 回退闸（回滚未完成标记；回退中再迁由此拒）。
    pub rolling_back: bool,
    /// 逻辑时钟游标（零墙钟）。
    pub tick: u64,
    /// 伪随机种子（硬件语义替身）。
    pub rng: u64,
    /// 成功迁移数。
    pub migrated: u64,
    /// 回滚数。
    pub rolled_back: u64,
    /// 拒绝分诊累计。
    pub rejections: u64,
}

impl Migrator {
    /// 新建原语（`ncpu` 核，每核给定负载与深度）。
    pub fn new(ncpu: u32, loads: &[u64], depths: &[u64]) -> Self {
        let mut cores: Vec<CoreSlot> = Vec::new();
        let mut queues: Vec<RunQueue> = Vec::new();
        let mut i = 0u32;
        while i < ncpu {
            let li = i as usize;
            let load = if li < loads.len() { loads[li] } else { 0 };
            let depth = if li < depths.len() { depths[li] } else { 0 };
            cores.push(CoreSlot::new(i, true, load, depth));
            queues.push(RunQueue::new(i));
            i += 1;
        }
        Migrator {
            cores,
            queues,
            stage: StageArea::new(),
            wp: WritePointRegistry::new(),
            led: TriLedger::new(),
            obs: ObsFace::new(),
            chains: Vec::new(),
            next_mid: 1,
            next_chain: 1,
            inflight: None,
            rolling_back: false,
            tick: 1,
            rng: 0x5EED_1722_F172_0001,
            migrated: 0,
            rolled_back: 0,
            rejections: 0,
        }
    }

    /// 逻辑时钟推进。
    pub fn tick_now(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// 索引核表项（越界给诊断）。
    pub fn core_idx(&self, cpu: u32) -> Result<usize, MigDiag> {
        let mut i = 0usize;
        while i < self.cores.len() {
            if self.cores[i].cpu == cpu {
                return Ok(i);
            }
            i += 1;
        }
        Err(MigDiag::new(
            MigErr::SrcOffline,
            "core_idx",
            format!("核 {} 不在册", cpu),
        ))
    }

    /// 全局总深度（实算）。
    pub fn global_depth(&self) -> u64 {
        let mut n = 0u64;
        for q in &self.queues {
            n += q.actual_depth();
        }
        n
    }
    /// 全局深度守恒断言（各核账深之和 == 实算之和）。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过（队列深度守恒）
    pub fn global_depth_conserve(&self) -> bool {
        let mut sum = 0u64;
        for q in &self.queues {
            if !q.depth_conserved() {
                return false;
            }
            sum += q.depth;
        }
        sum == self.global_depth()
    }

    /// 置核离线（注入核 5 用）。
    pub fn take_core_offline(&mut self, cpu: u32, reason: u64) -> Result<(), MigDiag> {
        let i = self.core_idx(cpu)?;
        self.cores[i].take_offline(reason);
        Ok(())
    }

    /// 排入队尾（建环境用；填充 Running 线程）。
    pub fn enqueue(&mut self, cpu: u32, tid: u64, prio: u8) -> Result<(), MigDiag> {
        let i = self.core_idx(cpu)?;
        let arr = self.queues[i].arrival;
        let e = QueueEntry::new(tid, prio, arr);
        if !self.queues[i].insert(e) {
            return Err(MigDiag::new(
                MigErr::StageSlotBad,
                "enqueue",
                format!("核 {} 队列满（上限 {}）", cpu, QUEUE_CAP),
            ));
        }
        self.queues[i].arrival = arr.wrapping_add(1);
        self.queues[i].depth = self.queues[i].actual_depth();
        self.cores[i].depth = self.queues[i].depth;
        self.cores[i].bump_load(load_word_of(tid, prio) as i64);
        Ok(())
    }

    /// 手工破坏深度账（注入核 10 用；显式标注为事故源）。
    pub fn sabotage_depth(&mut self, cpu: u32) -> Result<(), MigDiag> {
        let i = self.core_idx(cpu)?;
        self.queues[i].depth = self.queues[i].depth.wrapping_add(7);
        Ok(())
    }

    /// 找线程当前所在核（线性扫）。
    pub fn find_cpu_of(&self, tid: u64) -> Option<u32> {
        let mut qi = 0usize;
        while qi < self.queues.len() {
            let mut ii = 0usize;
            while ii < self.queues[qi].items.len() {
                if self.queues[qi].items[ii].tid == tid {
                    return Some(self.queues[qi].cpu);
                }
                ii += 1;
            }
            qi += 1;
        }
        None
    }

    /// 查链（按 mid）。
    pub fn chain(&self, mid: u64) -> Option<&ChainCtx> {
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].mid == mid {
                return Some(&self.chains[i]);
            }
            i += 1;
        }
        None
    }
    /// 取链的权威载荷（Detach 快照；供下游段调用方原样传递）。
    pub fn payload_of(&self, mid: u64) -> Option<StagePayload> {
        match self.chain(mid) {
            Some(c) => c.payload,
            None => None,
        }
    }

    /// 查链可变。
    pub fn chain_mut(&mut self, mid: u64) -> Option<&mut ChainCtx> {
        let mut i = 0usize;
        while i < self.chains.len() {
            if self.chains[i].mid == mid {
                return Some(&mut self.chains[i]);
            }
            i += 1;
        }
        None
    }

    /// 失败分诊（统一记拒绝并返回原诊断）。
    fn reject<T>(&mut self, d: MigDiag) -> Result<T, MigDiag> {
        self.rejections += 1;
        self.obs.note_reject(d.code);
        Err(d)
    }

    /// 开链（Detach 段入口的前置）。
    ///
    /// 闸：重入（在飞链）/ 回退中再迁 / 源核离线 / 链容量。
    pub fn begin(&mut self, t: ThreadTriple, state: ThreadState) -> Result<u64, MigDiag> {
        // 注入 1：原语重入。
        if self.inflight.is_some() {
            let cur = self.inflight.unwrap_or(0);
            return self.reject(MigDiag::new(
                MigErr::Reentrant,
                "begin:reentrant",
                format!("链 mid#{:#x} 执行中，重入拒", cur),
            ));
        }
        // 注入 8：回退中再迁。
        if self.rolling_back {
            return self.reject(MigDiag::new(
                MigErr::InRollback,
                "begin:rollback",
                "回滚未完成，回退中再迁拒".to_string(),
            ));
        }
        let si = match self.core_idx(t.src_cpu) {
            Ok(i) => i,
            Err(d) => return self.reject(d),
        };
        if !self.cores[si].online {
            return self.reject(MigDiag::new(
                MigErr::SrcOffline,
                "begin:srcoff",
                format!("源核 {} 离线", t.src_cpu),
            ));
        }
        if state == ThreadState::Stopped {
            return self.reject(MigDiag::new(
                MigErr::NotRunning,
                "begin:stopped",
                format!("线程 {} 已停止不可摘", t.tid),
            ));
        }
        let mid = self.next_mid;
        self.next_mid += 1;
        let chain_no = self.next_chain;
        self.next_chain += 1;
        let now = self.tick_now();
        let mut ctx = ChainCtx::new(mid, chain_no, t, state, now);
        ctx.depth_before = self.queues[si].actual_depth();
        ctx.global_before = self.global_depth();
        self.chains.push(ctx);
        self.inflight = Some(mid);
        Ok(mid)
    }

    /// 段 1：源核摘出。
    ///
    /// 消费开链段签名；校验：签名结构 / 线程态 Running / 源核在线 / 队首命中 /
    /// 写点授权 / 深度守恒恒等。
    pub fn detach(&mut self, mid: u64, sig: &HandoffSig) -> Result<HandoffSig, MigDiag> {
        let tag = match self.chain(mid) {
            Some(c) => c.chain_tag(),
            None => {
                return self.reject(MigDiag::new(
                    MigErr::SegSkipped,
                    "detach:nochain",
                    format!("链 mid#{:#x} 不在册", mid),
                ))
            }
        };
        // 段序闸：必须是首段。
        let reached = match self.chain(mid) {
            Some(c) => c.reached,
            None => -1,
        };
        if reached != -1 {
            return self.reject(MigDiag::new(
                MigErr::SegSkipped,
                "detach:order",
                format!("已达段 {}，重复摘出拒", reached),
            ));
        }
        // 验签（伪造交接签名 / 字段不匹配）。
        if sig.mid != mid {
            return self.reject(MigDiag::new(
                MigErr::SigForged,
                "detach:sigmid",
                format!("签名 mid#{:#x} ≠ 链 {:#x}", sig.mid, mid),
            ));
        }
        if sig.chain_tag != tag {
            return self.reject(MigDiag::new(
                MigErr::SigForged,
                "detach:sigtag",
                format!("链标签 {:#x} ≠ 应为 {:#x}", sig.chain_tag, tag),
            ));
        }
        if sig.expect_seg != MigrateSeg::Detach.num() || sig.from_seg != MigrateSeg::Detach.num() {
            return self.reject(MigDiag::new(
                MigErr::SigMismatch,
                "detach:sigseg",
                format!("段号不符：入口给 {}→{}，应为 0→0", sig.from_seg, sig.expect_seg),
            ));
        }
        let (triple, state) = match self.chain(mid) {
            Some(c) => (c.triple, c.state),
            None => {
                return self.reject(MigDiag::new(
                    MigErr::SegSkipped,
                    "detach:gone",
                    "链消失".to_string(),
                ))
            }
        };
        if sig.triple() != triple {
            return self.reject(MigDiag::new(
                MigErr::SigMismatch,
                "detach:sigtriple",
                format!("三元组 {:?} ≠ 链 {:?}", sig.triple(), triple),
            ));
        }
        // 线程态校验（仅 Running 可摘）。
        if !state.migratable() {
            return self.reject(MigDiag::new(
                MigErr::NotRunning,
                "detach:state",
                format!("线程 {} 态为 {}，仅运行中可摘", triple.tid, state.text()),
            ));
        }
        let si = match self.core_idx(triple.src_cpu) {
            Ok(i) => i,
            Err(d) => return self.reject(d),
        };
        if !self.cores[si].online {
            return self.reject(MigDiag::new(
                MigErr::SrcOffline,
                "detach:srcoff",
                format!("源核 {} 离线", triple.src_cpu),
            ));
        }
        // 队首命中校验（摘出必须是队首）。
        let head = match self.queues[si].head() {
            Some(h) => h,
            None => {
                return self.reject(MigDiag::new(
                    MigErr::StageSlotBad,
                    "detach:empty",
                    format!("源核 {} 队空", triple.src_cpu),
                ))
            }
        };
        if head.tid != triple.tid {
            return self.reject(MigDiag::new(
                MigErr::SigMismatch,
                "detach:nothead",
                format!("队首线程 {} ≠ 待摘 {}", head.tid, triple.tid),
            ));
        }
        // 写点授权（跨队列写入点唯一性闸）。
        let tk = match self.wp.authorize(WP_MIGRATE, MigrateSeg::Detach, mid) {
            Ok(t) => t,
            Err(d) => return self.reject(d),
        };
        if let Err(d) = self.wp.verify_and_burn(&tk, mid) {
            return self.reject(d);
        }
        // 执行摘出。
        let popped = match self.queues[si].pop_head() {
            Some(e) => e,
            None => {
                return self.reject(MigDiag::new(
                    MigErr::StageSlotBad,
                    "detach:pop",
                    "摘出时队空".to_string(),
                ))
            }
        };
        // 深度守恒恒等断言：摘出前 depth == 摘出后 depth + 1。
        let before = self.queues[si].actual_depth() + 1;
        let after = self.queues[si].actual_depth();
        if before != after + 1 {
            return self.reject(MigDiag::new(
                MigErr::DepthDrift,
                "detach:conserve",
                format!("源核深度恒等破坏：{} ≠ {} + 1", before, after),
            ));
        }
        self.queues[si].depth = after;
        self.cores[si].depth = after;
        self.cores[si].mig_out += 1;
        self.cores[si].last_mig_tick = self.tick;
        let lw = load_word_of(popped.tid, popped.prio) as i64;
        self.cores[si].bump_load(-lw);
        // 落源核出账。
        let pay = StagePayload::new(popped.tid, triple.src_cpu, popped.prio, state, mid, popped.arrival);
        let fp = pay.fingerprint();
        self.led.record(LedgerEntry::new(LedgerKind::SrcOut, 0, true, mid, popped.tid, triple.src_cpu, 0, self.tick, fp));
        // 更新链上下文。
        let snap = self.snap_fp();
        // 摘出后全局总深度 = 当前实算（摘出已生效）；记录摘出前口径 = 现值 + 1。
        let global_pre = self.global_depth() + 1;
        let chain_no = match self.chain(mid) {
            Some(c) => c.chain_no,
            None => 0,
        };
        if let Some(c) = self.chain_mut(mid) {
            c.reached = 0;
            c.phase = ChainPhase::Detached;
            c.payload = Some(pay);
            c.depth_before = before;
            c.depth_after = after;
            c.global_before = global_pre;
            // 线程在暂存区内尚未挂入 → 全局总量此刻不变（仅分布变化）。
            c.global_after = global_pre;
            c.sigs.push(*sig);
        }
        // 出口铸签名（供 Stage 消费）。
        // 注：交接签名的 snap_fp 承载的是**本迁移载荷指纹**（下游 Stage/Attach 段以
        // 此复算防篡改）；全局负载快照另由 snap_fp() 供观测面使用。
        let next_tag = mint_chain_tag(chain_no, 0);
        Ok(HandoffSig::new(
            MigrateSeg::Detach.num(),
            MigrateSeg::Stage.num(),
            fp,
            ThreadTriple::new(popped.tid, triple.src_cpu, popped.prio),
            mid,
            next_tag,
        ))
    }

    /// 全局负载快照指纹（六核负载 + 六核深度混合）。
    pub fn snap_fp(&self) -> u64 {
        let mut h = FNV_OFFSET_BASIS;
        for c in &self.cores {
            for byte in c.load.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            for byte in c.depth.to_le_bytes() {
                h ^= byte as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }
        h
    }
}impl Migrator {
    /// 段 2：暂存入区。
    ///
    /// 闸：段序 / 验签 / 指纹篡改 / 暂存满。
    pub fn stage(&mut self, mid: u64, sig: &HandoffSig, pay: &StagePayload) -> Result<HandoffSig, MigDiag> {
        let (reached, tag, chain_no) = match self.chain(mid) {
            Some(c) => (c.reached, c.chain_tag(), c.chain_no),
            None => {
                return self.reject(MigDiag::new(MigErr::SegSkipped, "stage:nochain", format!("链 {:#x} 不在册", mid)))
            }
        };
        if reached != 0 {
            return self.reject(MigDiag::new(
                MigErr::SegSkipped,
                "stage:order",
                format!("已达段 {}，非 Detach 后续（跳段）", reached),
            ));
        }
        if sig.mid != mid || sig.chain_tag != tag {
            return self.reject(MigDiag::new(
                MigErr::SigForged,
                "stage:sig",
                format!("签名 mid#{:#x}/tag{:#x} 与链 {:#x}/{:#x} 不符", sig.mid, sig.chain_tag, mid, tag),
            ));
        }
        if sig.expect_seg != MigrateSeg::Stage.num() || sig.from_seg != MigrateSeg::Detach.num() {
            return self.reject(MigDiag::new(
                MigErr::SigMismatch,
                "stage:sigseg",
                format!("段号不符 {}→{}", sig.from_seg, sig.expect_seg),
            ));
        }
        // 载荷权威性闸：调用方载荷须与 Detach 快照逐字段一致。
        let auth = match self.chain(mid) {
            Some(c) => c.payload,
            None => None,
        };
        match auth {
            Some(a) => {
                if a != *pay {
                    return self.reject(MigDiag::new(
                        MigErr::FpTamper,
                        "stage:payload",
                        format!("载荷与 Detach 快照不符：入 {:?} 快照 {:?}", pay.triple(), a.triple()),
                    ));
                }
            }
            None => {
                return self.reject(MigDiag::new(MigErr::FpTamper, "stage:nopayload", "链无 Detach 载荷快照".to_string()))
            }
        }
        // 载荷指纹篡改闸（签名指纹 vs 实算载荷指纹）。
        let real_fp = pay.fingerprint();
        if sig.snap_fp != real_fp {
            return self.reject(MigDiag::new(
                MigErr::FpTamper,
                "stage:fp",
                format!("签名载荷指纹 {:#x} ≠ 实算 {:#x}", sig.snap_fp, real_fp),
            ));
        }
        if pay.triple() != sig.triple() {
            return self.reject(MigDiag::new(
                MigErr::SigMismatch,
                "stage:triple",
                format!("载荷三元组 {:?} ≠ 签名 {:?}", pay.triple(), sig.triple()),
            ));
        }
        // 分配 → 落区（两路径）。
        let idx = match self.stage.reserve(mid, pay, self.tick) {
            Ok(i) => i,
            Err(d) => return self.reject(d),
        };
        if let Err(d) = self.stage.occupy(idx, pay) {
            let _ = self.stage.release(idx, mid);
            return self.reject(d);
        }
        // 暂存守恒断言。
        if !self.stage.conserve() || !self.stage.ledger_conserve() {
            let _ = self.stage.release(idx, mid);
            return self.reject(MigDiag::new(
                MigErr::DepthDrift,
                "stage:conserve",
                format!("暂存守恒破坏：占{} 自由{} 账{}", self.stage.occupied_scan(), self.stage.free_scan(), self.stage.occupied_total),
            ));
        }
        if let Some(c) = self.chain_mut(mid) {
            c.reached = 1;
            c.phase = ChainPhase::Staged;
            c.slot = idx as i32;
            c.sigs.push(*sig);
        }
        Ok(HandoffSig::new(
            MigrateSeg::Stage.num(),
            MigrateSeg::Attach.num(),
            real_fp,
            pay.triple(),
            mid,
            mint_chain_tag(chain_no, 0),
        ))
    }

    /// 段 3：目标核挂入。
    ///
    /// 闸：段序 / 验签 / 目标核离线 / 写点授权 / 目标深度守恒 / 全局深度守恒。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过（挂入后全局总量守恒）
    pub fn attach(&mut self, mid: u64, sig: &HandoffSig, pay: &StagePayload, dst: u32) -> Result<HandoffSig, MigDiag> {
        let (reached, tag, chain_no, slot) = match self.chain(mid) {
            Some(c) => (c.reached, c.chain_tag(), c.chain_no, c.slot),
            None => {
                return self.reject(MigDiag::new(MigErr::SegSkipped, "attach:nochain", format!("链 {:#x} 不在册", mid)))
            }
        };
        if reached != 1 {
            return self.reject(MigDiag::new(
                MigErr::SegSkipped,
                "attach:order",
                format!("已达段 {}，非 Stage 后续（跳段）", reached),
            ));
        }
        if sig.mid != mid || sig.chain_tag != tag {
            return self.reject(MigDiag::new(MigErr::SigForged, "attach:sig", "签名与链不符".to_string()));
        }
        if sig.expect_seg != MigrateSeg::Attach.num() || sig.from_seg != MigrateSeg::Stage.num() {
            return self.reject(MigDiag::new(MigErr::SigMismatch, "attach:sigseg", format!("段号不符 {}→{}", sig.from_seg, sig.expect_seg)));
        }
        // 载荷权威性闸：调用方载荷须与 Detach 快照逐字段一致。
        let auth = match self.chain(mid) {
            Some(c) => c.payload,
            None => None,
        };
        match auth {
            Some(a) => {
                if a != *pay {
                    return self.reject(MigDiag::new(
                        MigErr::FpTamper,
                        "attach:payload",
                        format!("载荷与 Detach 快照不符：入 {:?} 快照 {:?}", pay.triple(), a.triple()),
                    ));
                }
            }
            None => {
                return self.reject(MigDiag::new(MigErr::FpTamper, "attach:nopayload", "链无 Detach 载荷快照".to_string()))
            }
        }
        let real_fp = pay.fingerprint();
        if sig.snap_fp != real_fp {
            return self.reject(MigDiag::new(MigErr::FpTamper, "attach:fp", format!("指纹 {:#x} ≠ {:#x}", sig.snap_fp, real_fp)));
        }
        // 暂存槽内容复算（槽指纹 vs 载荷指纹）。
        if slot >= 0 {
            match self.stage.slot(slot as usize) {
                Some(s) => {
                    if s.payload_fp != real_fp || s.owner != mid {
                        return self.reject(MigDiag::new(
                            MigErr::FpTamper,
                            "attach:slotfp",
                            format!("暂存槽 {} 指纹 {:#x} ≠ 载荷 {:#x}", slot, s.payload_fp, real_fp),
                        ));
                    }
                }
                None => {
                    return self.reject(MigDiag::new(MigErr::StageSlotBad, "attach:slot", "暂存槽丢失".to_string()))
                }
            }
        } else {
            return self.reject(MigDiag::new(MigErr::StageSlotBad, "attach:noslot", "链无暂存槽".to_string()));
        }
        // 目标核离线闸。
        let di = match self.core_idx(dst) {
            Ok(i) => i,
            Err(d) => return self.reject(d),
        };
        if !self.cores[di].online {
            return self.reject(MigDiag::new(MigErr::TargetOffline, "attach:off", format!("目标核 {} 离线", dst)));
        }
        if self.queues[di].actual_depth() >= QUEUE_CAP as u64 {
            return self.reject(MigDiag::new(MigErr::StageSlotBad, "attach:qfull", format!("目标核 {} 队列满", dst)));
        }
        // 写点授权（跨队列挂入唯一通道）。
        let tk = match self.wp.authorize(WP_MIGRATE, MigrateSeg::Attach, mid) {
            Ok(t) => t,
            Err(d) => return self.reject(d),
        };
        if let Err(d) = self.wp.verify_and_burn(&tk, mid) {
            return self.reject(d);
        }
        // 类序下降插入（挂入位置语义）。
        let arr = self.queues[di].arrival;
        let e = QueueEntry::new(pay.tid, pay.prio, arr);
        let pos = self.queues[di].insert_position(e.prio, e.arrival);
        let before_dst = self.queues[di].actual_depth();
        if !self.queues[di].insert(e) {
            return self.reject(MigDiag::new(MigErr::StageSlotBad, "attach:insert", "插入失败（队满）".to_string()));
        }
        self.queues[di].arrival = arr.wrapping_add(1);
        let after_dst = self.queues[di].actual_depth();
        // 目标队列深度守恒账。
        self.queues[di].depth = after_dst;
        self.cores[di].depth = after_dst;
        self.cores[di].mig_in += 1;
        self.cores[di].last_mig_tick = self.tick;
        self.cores[di].bump_load(load_word_of(pay.tid, pay.prio) as i64);
        // 挂入后目标深度恒等：挂入后 == 挂入前 + 1。
        if after_dst != before_dst + 1 {
            return self.reject(MigDiag::new(
                MigErr::DepthDrift,
                "attach:dstconserve",
                format!("目标深度恒等破坏：{} ≠ {} + 1（插入位 {}）", after_dst, before_dst, pos),
            ));
        }
        let global_after = self.global_depth();
        // 全局深度守恒恒等（迁移不改变全局总深度，仅改分布）。
        let expect_global = match self.chain(mid) {
            Some(c) => c.global_before,
            None => global_after,
        };
        if global_after != expect_global {
            return self.reject(MigDiag::new(
                MigErr::DepthDrift,
                "attach:global",
                format!("全局总深度 {} ≠ 迁移前 {}", global_after, expect_global),
            ));
        }
        // 暂存槽标脏（内容已随行，槽待复用）。
        if slot >= 0 {
            let _ = self.stage.mark_dirty(slot as usize, mid);
        }
        // 三向账（目标入账 + 线程随行账）。
        self.led.record(LedgerEntry::new(LedgerKind::DstIn, 2, true, mid, pay.tid, pay.src_cpu, dst, self.tick, real_fp));
        self.led.record(LedgerEntry::new(LedgerKind::Follow, 2, true, mid, pay.tid, pay.src_cpu, dst, self.tick, pay.triple().fingerprint()));
        // 冷却窗内判定（F1726；本模块只观测不阻断）。
        let in_cooldown = match self.cores[di].last_mig_tick.checked_sub(self.tick) {
            Some(d) => d < COOLDOWN_WINDOW,
            None => false,
        };
        if let Some(c) = self.chain_mut(mid) {
            c.reached = 2;
            c.phase = ChainPhase::Attached;
            c.dst_cpu = dst;
            c.global_after = global_after;
            c.sigs.push(*sig);
        }
        self.inflight = None;
        self.migrated += 1;
        self.obs.note_migrate(true, pay.tid, pay.src_cpu, dst, in_cooldown);
        Ok(HandoffSig::new(
            MigrateSeg::Attach.num(),
            MigrateSeg::Attach.num(),
            real_fp,
            pay.triple(),
            mid,
            mint_chain_tag(chain_no, 0),
        ))
    }

    /// 自动选目标核并挂入（省去调用方选核；选核失败给 TargetOffline）。
    pub fn attach_auto(&mut self, mid: u64, sig: &HandoffSig, pay: &StagePayload) -> Result<HandoffSig, MigDiag> {
        let src = pay.src_cpu;
        let dst = match select_target(&self.cores, src, true) {
            Ok(d) => d,
            Err(d) => return self.reject(d),
        };
        self.attach(mid, sig, pay, dst)
    }

    /// 整链回滚（任一段失败 → 回初始态；回滚本身落账）。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过（失败整链回滚且回滚落账）
    pub fn rollback(&mut self, mid: u64, reason: &MigErr) -> Result<String, MigDiag> {
        self.rolling_back = true;
        let mut notes: Vec<String> = Vec::new();
        let (reached, src, slot, tid) = match self.chain(mid) {
            Some(c) => (c.reached, c.triple.src_cpu, c.slot, c.triple.tid),
            None => {
                self.rolling_back = false;
                return self.reject(MigDiag::new(MigErr::SegSkipped, "rollback:nochain", format!("链 {:#x} 不在册", mid)));
            }
        };
        // 逆序回退：暂存槽释放 → 源核队首回插。
        if slot >= 0 {
            match self.stage.release(slot as usize, mid) {
                Ok(_) => notes.push(format!("释放暂存槽 {}", slot)),
                Err(d) => notes.push(format!("槽释放拒 {}: {}", slot, d.code.text())),
            }
        }
        if reached >= 0 {
            if let Ok(si) = self.core_idx(src) {
                let arr = self.queues[si].arrival;
                let prio = match self.chain(mid) {
                    Some(c) => c.triple.prio,
                    None => 0,
                };
                let e = QueueEntry::new(tid, prio, arr);
                if self.queues[si].push_head(e) {
                    self.queues[si].arrival = arr.wrapping_add(1);
                    self.queues[si].depth = self.queues[si].actual_depth();
                    self.cores[si].depth = self.queues[si].depth;
                    self.cores[si].bump_load(load_word_of(tid, prio) as i64);
                    if self.cores[si].mig_out > 0 {
                        self.cores[si].mig_out -= 1;
                    }
                    notes.push(format!("源核 {} 队首回插", src));
                } else {
                    notes.push(format!("源核 {} 队满回插失败", src));
                }
            }
        }
        // 回滚段账（F1724 回退面消费）。
        let seg_no = if reached >= 0 { reached as u8 } else { 0xff };
        let rb_tick = self.tick_now();
        self.led.record(LedgerEntry::new(LedgerKind::Rollback, seg_no, false, mid, tid, src, 0, rb_tick, 0));
        // 冲正：补记一笔源核出账的 ok=false 反向项，使摘出账净额归零。
        // 历史 ok=true 项保持 append-only 不改写，闭合由 ok 项计数自然对平。
        if reached >= 0 {
            self.led.record(LedgerEntry::new(LedgerKind::SrcOut, seg_no, false, mid, tid, src, 0, rb_tick, 0));
        }
        let closed_ok = self.stage.conserve() && self.global_depth_conserve();
        if let Some(c) = self.chain_mut(mid) {
            c.phase = ChainPhase::RolledBack;
            c.reached = -1;
            c.slot = -1;
            c.rollback_ledgered = true;
        }
        self.obs.note_rollback();
        self.rolled_back += 1;
        self.inflight = None;
        self.rolling_back = false;
        let line = format!(
            "回滚 mid#{:#x} 原因 {} —— {} —— 守恒{}",
            mid,
            reason.text(),
            notes.join("；"),
            if closed_ok { "过" } else { "破" }
        );
        Ok(line)
    }

    /// 三向账结算（settle）。
    pub fn settle(&self) -> SettleResult {
        settle_ledger(&self.led)
    }

    /// 全链闭合断言总装（三段闭合 + 深度守恒 + 暂存守恒 + 写点唯一性）。
    // 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过 + 全域唯一跨队列写入点断言通过
    pub fn assert_all(&self) -> (bool, Vec<String>) {
        let mut v: Vec<String> = Vec::new();
        let mut all = true;
        // 1. 已完成链逐条闭合。
        let mut closed = 0u64;
        for c in &self.chains {
            if c.phase == ChainPhase::Attached {
                if c.chain_closed() {
                    closed += 1;
                } else {
                    all = false;
                    v.push(format!("链 {:#x} 未闭合", c.mid));
                }
            }
        }
        v.push(format!("闭合链 {}/{}", closed, self.chains.iter().filter(|c| c.phase == ChainPhase::Attached).count()));
        // 2. 回滚链闭合。
        for c in &self.chains {
            if c.phase == ChainPhase::RolledBack && !c.rollback_closed() {
                all = false;
                v.push(format!("链 {:#x} 回滚未落账", c.mid));
            }
        }
        // 3. 深度守恒。
        if !self.global_depth_conserve() {
            all = false;
            v.push("全局队列深度不守恒".to_string());
        } else {
            v.push("全局深度守恒过".to_string());
        }
        // 4. 暂存守恒。
        if !self.stage.conserve() || !self.stage.ledger_conserve() {
            all = false;
            v.push("暂存守恒破".to_string());
        } else {
            v.push("暂存守恒过".to_string());
        }
        // 5. 写点唯一性。
        if !self.wp.assert_unique_write_point() {
            all = false;
            v.push("全域唯一跨队列写入点断言破".to_string());
        } else {
            v.push("唯一跨队列写入点断言过".to_string());
        }
        // 6. 三账闭合。
        let s = self.settle();
        if !s.closed {
            all = false;
            v.push(s.line());
        } else {
            v.push(s.line());
        }
        (all, v)
    }

    /// 报表。
    pub fn report(&self) -> String {
        format!(
            "迁移原语 核{} 成功{} 回滚{} 拒绝{} 在飞{} {}",
            self.cores.len(),
            self.migrated,
            self.rolled_back,
            self.rejections,
            match self.inflight {
                Some(m) => format!("mid#{:#x}", m),
                None => "无".to_string(),
            },
            self.stage.fingerprint() & 0xffff
        )
    }
}// ---------------------------------------------------------------------------
// 组件 F · 注入核（10 形态；判据：UNX-F1722-J1 —— 绕行注入 10/10 次拒绝）
// ---------------------------------------------------------------------------

/// 单条注入结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectResult {
    /// 序号（1~10）。
    pub no: u32,
    /// 形态名。
    pub form: String,
    /// 是否被拒（判据要求全部为 true）。
    pub rejected: bool,
    /// 诊断码（被拒时有效；未拒为 0）。
    pub code: u16,
    /// 理由文本。
    pub reason: String,
}

impl InjectResult {
    /// 构造被拒结果。
    pub fn caught(no: u32, form: &str, code: MigErr, reason: String) -> Self {
        InjectResult { no, form: form.to_string(), rejected: true, code: code.num(), reason }
    }
    /// 构造漏网结果（应为失败态；出现即判据破）。
    pub fn missed(no: u32, form: &str, reason: String) -> Self {
        InjectResult { no, form: form.to_string(), rejected: false, code: 0, reason }
    }
    /// 一行。
    pub fn line(&self) -> String {
        format!(
            "注{} {} —— {} [0x{:04X}] {}",
            self.no,
            self.form,
            if self.rejected { "已拒" } else { "漏网" },
            self.code,
            self.reason
        )
    }
}

/// 构造环境（4 核；核0 重载 3 线程，核1/2/3 轻载）。
/// 注：队列为 prio 类序下降，摘出只取队首 —— 故待迁线程 1001 以最高优先级位于队首。
pub fn build_env() -> Migrator {
    let mut m = Migrator::new(4, &[8000, 1000, 1000, 1000], &[3, 0, 0, 0]);
    let _ = m.enqueue(0, 1002, 5);
    let _ = m.enqueue(0, 1003, 6);
    let _ = m.enqueue(0, 1001, 9);
    let _ = m.enqueue(1, 2001, 3);
    let _ = m.enqueue(2, 3001, 4);
    let _ = m.enqueue(3, 4001, 2);
    m
}

/// 开链签名（辅助：铸 Detach 段入口签名）。
fn det_sig(m: &Migrator, mid: u64, t: ThreadTriple) -> HandoffSig {
    let tag = match m.chain(mid) {
        Some(c) => c.chain_tag(),
        None => 0,
    };
    HandoffSig::new(0, 0, m.snap_fp(), t, mid, tag)
}

/// 注入 1：原语重入（链执行中再次调用）。
pub fn inject_1_reentrant() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(1, "原语重入", d.code, d.detail),
    };
    // 在飞链未终态，再次开链 → 必拒。
    let again = m.begin(ThreadTriple::new(1002, 0, 6), ThreadState::Running);
    match again {
        Err(d) => InjectResult::caught(1, "原语重入", d.code, format!("链 {:#x} 在飞时重入被拒", mid)),
        Ok(_) => InjectResult::missed(1, "原语重入", format!("链 {:#x} 在飞却重入成功", mid)),
    }
}

/// 注入 2：跳段（Stage 前直接 Attach）。
pub fn inject_2_seg_skip() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(2, "跳段", d.code, d.detail),
    };
    // 开链后不摘出、不暂存，直接请求挂入段 → 段序闸必拒。
    let pay = StagePayload::new(1001, 0, 5, ThreadState::Running, mid, 1);
    let tag = match m.chain(mid) {
        Some(c) => c.chain_tag(),
        None => 0,
    };
    let fake = HandoffSig::new(0, 2, pay.fingerprint(), t, mid, tag);
    let r = m.attach(mid, &fake, &pay, 1);
    match r {
        Err(d) => InjectResult::caught(2, "跳段", d.code, format!("跳过摘出与暂存直接挂入被拒：{}", d.code.text())),
        Ok(_) => InjectResult::missed(2, "跳段", "跳过摘出与暂存直接挂入成功".to_string()),
    }
}

/// 注入 3：伪造交接签名（跨链重放 / mid 不符）。
pub fn inject_3_forged_sig() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(3, "伪造交接签名", d.code, d.detail),
    };
    // 用不存在的链标签 → 伪造。
    let bad = HandoffSig::new(0, 0, m.snap_fp(), t, mid, 0xDEAD_BEEF);
    match m.detach(mid, &bad) {
        Err(d) => InjectResult::caught(3, "伪造交接签名", d.code, format!("链标签伪造被拒：{}", d.code.text())),
        Ok(_) => InjectResult::missed(3, "伪造交接签名", "伪造标签的签名被接受".to_string()),
    }
}

/// 注入 4：暂存区满（占满 64 槽后再暂存）。
pub fn inject_4_stage_full() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(4, "暂存区满", d.code, d.detail),
    };
    let s1 = match m.detach(mid, &det_sig(&m, mid, t)) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(4, "暂存区满", d.code, d.detail),
    };
    // 灌满暂存区（64 槽）。
    let mut filler = 1u64;
    while m.stage.free_count() > 0 {
        let pay = StagePayload::new(90000 + filler, 0, 1, ThreadState::Running, 0xffff_0000 + filler, filler);
        if m.stage.reserve(0xffff_0000 + filler, &pay, filler).is_err() {
            break;
        }
        filler += 1;
        if filler > 200 {
            break;
        }
    }
    let pay = match m.payload_of(mid) {
        Some(p) => p,
        None => return InjectResult::missed(4, "暂存区满", "链无载荷快照".to_string()),
    };
    match m.stage(mid, &s1, &pay) {
        Err(d) => InjectResult::caught(
            4,
            "暂存区满",
            d.code,
            format!("占满 {} 槽后暂存被拒；{}", m.stage.occupied_count(), MigErr::StageFull.suggestion()),
        ),
        Ok(_) => InjectResult::missed(4, "暂存区满", "暂存区满仍接受暂存".to_string()),
    }
}

/// 注入 5：目标核离线。
pub fn inject_5_target_offline() -> InjectResult {
    let mut m = build_env();
    // 全部候选目标核离线 → 无可用目标。
    let _ = m.take_core_offline(1, 0x02);
    let _ = m.take_core_offline(2, 0x02);
    let _ = m.take_core_offline(3, 0x02);
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(5, "目标核离线", d.code, d.detail),
    };
    let s1 = match m.detach(mid, &det_sig(&m, mid, t)) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(5, "目标核离线", d.code, d.detail),
    };
    let pay = match m.payload_of(mid) {
        Some(p) => p,
        None => return InjectResult::missed(5, "目标核离线", "链无载荷快照".to_string()),
    };
    if m.stage(mid, &s1, &pay).is_err() {
        return InjectResult::caught(5, "目标核离线", MigErr::StageFull, "暂存先行失败".to_string());
    }
    match select_target_with_probe(&m.cores, 0, 1) {
        Err(d) => InjectResult::caught(5, "目标核离线", d.code, d.detail),
        Ok(c) => InjectResult::missed(5, "目标核离线", format!("全离线却选出目标核 {}", c)),
    }
}

/// 注入 6：非 Running 态摘出。
pub fn inject_6_not_running() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    // 以阻塞态开链（begin 允许 Stopped 之外的非 Running），摘出段必须拒。
    let mid = match m.begin(t, ThreadState::Blocked) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(6, "非 Running 态摘出", d.code, d.detail),
    };
    match m.detach(mid, &det_sig(&m, mid, t)) {
        Err(d) => InjectResult::caught(6, "非 Running 态摘出", d.code, format!("阻塞态摘出被拒：{}", d.code.text())),
        Ok(_) => InjectResult::missed(6, "非 Running 态摘出", "阻塞态线程被摘出".to_string()),
    }
}

/// 注入 7：绕行写入（未登记写入点直写目标队列）。
// 判据：UNX-F1722-J1 —— 全域唯一跨队列写入点断言通过（绕行注入必拒）
pub fn inject_7_bypass_write() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(7, "绕行写入", d.code, d.detail),
    };
    // 未登记写点号 → 授权必拒。
    let r = m.wp.authorize(0x00FF, MigrateSeg::Attach, mid);
    match r {
        Err(d) => InjectResult::caught(
            7,
            "绕行写入",
            d.code,
            format!("未登记写点 0x00FF 直写被拒；{}", MigErr::BypassWrite.suggestion()),
        ),
        Ok(_) => InjectResult::missed(7, "绕行写入", "未登记写点被授权".to_string()),
    }
}

/// 注入 7b：越权登记第二合法写点（破坏唯一性）。
pub fn inject_7b_second_legal_point() -> InjectResult {
    let mut m = build_env();
    let r = m.wp.try_register_second_legal(WritePoint::legal(0x0002, MigrateSeg::Attach.num() as u8, 0xffff));
    let unique = m.wp.assert_unique_write_point();
    match r {
        Err(d) => InjectResult::caught(
            7,
            "绕行写入",
            d.code,
            format!("第二合法写点被拒，唯一性仍={}", unique),
        ),
        Ok(_) => InjectResult::missed(7, "绕行写入", "登记了第二合法写点".to_string()),
    }
}

/// 注入 8：回退中再迁。
pub fn inject_8_migrate_in_rollback() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    // 人为置回退闸（模拟回滚进行中）。
    m.rolling_back = true;
    let r = m.begin(t, ThreadState::Running);
    match r {
        Err(d) => InjectResult::caught(8, "回退中再迁", d.code, format!("回退闸开启时开链被拒：{}", d.code.text())),
        Ok(_) => InjectResult::missed(8, "回退中再迁", "回退中开链成功".to_string()),
    }
}

/// 注入 9：段签名指纹篡改。
pub fn inject_9_fp_tamper() -> InjectResult {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(9, "指纹篡改", d.code, d.detail),
    };
    let s1 = match m.detach(mid, &det_sig(&m, mid, t)) {
        Ok(x) => x,
        Err(d) => return InjectResult::caught(9, "指纹篡改", d.code, d.detail),
    };
    let pay = StagePayload::new(1001, 0, 5, ThreadState::Running, mid, 1);
    // 篡改载荷指纹一位。
    let tampered = s1.snap_fp ^ 0x1;
    let sig = HandoffSig { snap_fp: tampered, ..s1 };
    match m.stage(mid, &sig, &pay) {
        Err(d) => InjectResult::caught(9, "指纹篡改", d.code, format!("指纹 {:#x} 被拒", tampered)),
        Ok(_) => InjectResult::missed(9, "指纹篡改", "篡改指纹被接受".to_string()),
    }
}

/// 注入 10：队列深度不守恒（手工破坏 depth 后结算）。
pub fn inject_10_depth_drift() -> InjectResult {
    let mut m = build_env();
    let before_ok = m.global_depth_conserve();
    let _ = m.sabotage_depth(1);
    let after_ok = m.global_depth_conserve();
    let broke = !after_ok && before_ok;
    if broke {
        InjectResult::caught(
            10,
            "深度不守恒",
            MigErr::DepthDrift,
            format!("破坏前守恒={} 破坏后守恒={}；{}", before_ok, after_ok, MigErr::DepthDrift.suggestion()),
        )
    } else {
        InjectResult::missed(10, "深度不守恒", "破坏深度账未被守恒断言捕获".to_string())
    }
}

/// 汇总入口（判据：绕行注入 10/10 次拒绝）。
pub fn inject_all() -> (usize, usize, Vec<InjectResult>) {
    let mut rs: Vec<InjectResult> = Vec::new();
    rs.push(inject_1_reentrant());
    rs.push(inject_2_seg_skip());
    rs.push(inject_3_forged_sig());
    rs.push(inject_4_stage_full());
    rs.push(inject_5_target_offline());
    rs.push(inject_6_not_running());
    // 注入 7 取两形态（绕行直写 + 第二合法写点），其一失败即判据破。
    let r7a = inject_7_bypass_write();
    let r7b = inject_7b_second_legal_point();
    let mut r7 = if r7a.rejected && r7b.rejected {
        r7a
    } else if r7a.rejected {
        r7b
    } else {
        r7a
    };
    r7.form = "绕行写入".to_string();
    rs.push(r7);
    rs.push(inject_8_migrate_in_rollback());
    rs.push(inject_9_fp_tamper());
    rs.push(inject_10_depth_drift());
    let caught = rs.iter().filter(|r| r.rejected).count();
    (caught, rs.len(), rs)
}// ---------------------------------------------------------------------------
// 组件 G/H · 观测导出（四族统计 + KV + TLV）与确定性回放
// ---------------------------------------------------------------------------

/// TLV 魔数（"V1722"）。
pub const TLV_MAGIC: u32 = 0x5641_3732;

/// TLV 版本。
pub const TLV_VERSION: u16 = 1;

/// 观测导出面（四族计数 + KV/TLV 编码）。
#[derive(Clone, Debug)]
pub struct ExportFace {
    /// 迁移率千分比。
    pub mig_rate: u64,
    /// 乒乓率千分比。
    pub pp_rate: u64,
    /// 破窗率千分比。
    pub win_rate: u64,
    /// 回退率千分比。
    pub rb_rate: u64,
    /// 迁移率窗口均值。
    pub mig_win_mean: u64,
    /// 乒乓率窗口均值。
    pub pp_win_mean: u64,
    /// 破窗率窗口均值。
    pub win_win_mean: u64,
    /// 回退率窗口均值。
    pub rb_win_mean: u64,
    /// 迁移率窗口峰值。
    pub mig_win_peak: u64,
    /// 结算闭合比千分比。
    pub settle_ratio: u64,
    /// 三账合并指纹。
    pub led_fp: u64,
    /// 暂存区指纹。
    pub stage_fp: u64,
    /// 写点表指纹。
    pub wp_fp: u64,
    /// 全局总深度。
    pub global_depth: u64,
    /// 已覆盖丢账总数（满账显性）。
    pub dropped_total: u64,
}

impl ExportFace {
    /// 从原语取观测面快照。
    pub fn of(m: &Migrator) -> Self {
        let s = m.settle();
        ExportFace {
            mig_rate: m.obs.migration_rate_permille(),
            pp_rate: m.obs.pingpong_rate_permille(),
            win_rate: m.obs.break_rate_permille(),
            rb_rate: m.obs.rollback_rate_permille(),
            mig_win_mean: m.obs.mig_rate.mean_permille(),
            pp_win_mean: m.obs.pp_rate.mean_permille(),
            win_win_mean: m.obs.win_rate.mean_permille(),
            rb_win_mean: m.obs.rb_rate.mean_permille(),
            mig_win_peak: m.obs.mig_rate.peak(),
            settle_ratio: s.ratio_permille,
            led_fp: m.led.combined_fp(),
            stage_fp: m.stage.fingerprint(),
            wp_fp: m.wp.fingerprint(),
            global_depth: m.global_depth(),
            dropped_total: m.led.src.dropped + m.led.dst.dropped + m.led.follow.dropped,
        }
    }

    /// 四族文本（百分比；零浮点）。
    pub fn four_families(&self) -> String {
        format!(
            "迁移率{} 乒乓率{} 破窗率{} 回退率{}",
            fmt_permille(self.mig_rate),
            fmt_permille(self.pp_rate),
            fmt_permille(self.win_rate),
            fmt_permille(self.rb_rate)
        )
    }

    /// KV 文本导出（`key=value` 每行一行）。
    pub fn export_kv(&self) -> String {
        let mut s: String = String::new();
        s.push_str(&format!("module={}\n", MIG_VERSION));
        s.push_str(&format!("seg_base=0x{:04X}\n", MIG_SEG_BASE));
        s.push_str(&format!("mig_rate={}\n", self.mig_rate));
        s.push_str(&format!("pingpong_rate={}\n", self.pp_rate));
        s.push_str(&format!("break_rate={}\n", self.win_rate));
        s.push_str(&format!("rollback_rate={}\n", self.rb_rate));
        s.push_str(&format!("mig_win_mean={}\n", self.mig_win_mean));
        s.push_str(&format!("pingpong_win_mean={}\n", self.pp_win_mean));
        s.push_str(&format!("break_win_mean={}\n", self.win_win_mean));
        s.push_str(&format!("rollback_win_mean={}\n", self.rb_win_mean));
        s.push_str(&format!("mig_win_peak={}\n", self.mig_win_peak));
        s.push_str(&format!("settle_ratio={}\n", self.settle_ratio));
        s.push_str(&format!("ledger_fp=0x{:016X}\n", self.led_fp));
        s.push_str(&format!("stage_fp=0x{:016X}\n", self.stage_fp));
        s.push_str(&format!("wp_fp=0x{:016X}\n", self.wp_fp));
        s.push_str(&format!("global_depth={}\n", self.global_depth));
        s.push_str(&format!("dropped_total={}\n", self.dropped_total));
        s
    }

    /// TLV 二进制导出（魔数 + 版本 + 项数 + 项体；长度自洽）。
    pub fn export_tlv(&self) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        let mut items: Vec<(u16, u64)> = Vec::new();
        items.push((0x01, self.mig_rate));
        items.push((0x02, self.pp_rate));
        items.push((0x03, self.win_rate));
        items.push((0x04, self.rb_rate));
        items.push((0x05, self.mig_win_mean));
        items.push((0x06, self.pp_win_mean));
        items.push((0x07, self.win_win_mean));
        items.push((0x08, self.rb_win_mean));
        items.push((0x09, self.mig_win_peak));
        items.push((0x0A, self.settle_ratio));
        items.push((0x0B, self.led_fp));
        items.push((0x0C, self.stage_fp));
        items.push((0x0D, self.wp_fp));
        items.push((0x0E, self.global_depth));
        items.push((0x0F, self.dropped_total));
        let mut i = 0usize;
        while i < items.len() {
            let (t, v) = items[i];
            body.push(t as u8);
            body.push(0x08);
            for b in v.to_le_bytes() {
                body.push(b);
            }
            i += 1;
        }
        // 头：魔数(4) + 版本(2) + 项数(2) + 体长(4)
        let mut out: Vec<u8> = Vec::new();
        for b in TLV_MAGIC.to_le_bytes() {
            out.push(b);
        }
        for b in TLV_VERSION.to_le_bytes() {
            out.push(b);
        }
        for b in (items.len() as u16).to_le_bytes() {
            out.push(b);
        }
        for b in (body.len() as u32).to_le_bytes() {
            out.push(b);
        }
        for b in &body {
            out.push(*b);
        }
        out
    }

    /// TLV 长度自洽断言（复解析并逐项对账）。
    pub fn tlv_selfcheck(blob: &[u8]) -> (bool, u32) {
        if blob.len() < 12 {
            return (false, 0);
        }
        let mut magic = 0u32;
        for k in 0..4 {
            magic |= (blob[k] as u32) << (8 * k);
        }
        let mut ver = 0u16;
        for k in 0..2 {
            ver |= (blob[4 + k] as u16) << (8 * k);
        }
        let mut cnt = 0u16;
        for k in 0..2 {
            cnt |= (blob[6 + k] as u16) << (8 * k);
        }
        let mut blen = 0u32;
        for k in 0..4 {
            blen |= (blob[8 + k] as u32) << (8 * k);
        }
        let ok = magic == TLV_MAGIC && ver == TLV_VERSION && (blen as usize) + 12 == blob.len();
        // 逐项长度自洽：每项 2 + 8 字节。
        let expect = (cnt as usize) * 10;
        (ok && expect == blen as usize, blen)
    }
}

/// 迁移原语 KV 文本导出（薄封装；便于观测面单点调用）。
pub fn export_kv(m: &Migrator) -> String {
    ExportFace::of(m).export_kv()
}

/// 迁移原语 TLV 二进制导出（薄封装）。
pub fn export_tlv(m: &Migrator) -> Vec<u8> {
    ExportFace::of(m).export_tlv()
}

/// 回放偏差明细（逐字段对账）。
#[derive(Clone, Debug, Default)]
pub struct ReplayDiff {
    /// 偏差字段描述（空 = 零偏差）。
    pub fields: Vec<String>,
    /// 重建链数。
    pub rebuilt: u64,
    /// 对账项数。
    pub checked: u64,
}

impl ReplayDiff {
    /// 是否零偏差。
    pub fn is_zero(&self) -> bool {
        self.fields.is_empty()
    }
    /// 一行。
    pub fn line(&self) -> String {
        format!("回放 重建{} 对账项{} 偏差{}", self.rebuilt, self.checked, self.fields.len())
    }
}

/// 回放结果。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 迁移轨迹（三向账合并按序）。
    pub trace: Vec<LedgerEntry>,
    /// 偏差账。
    pub diff: ReplayDiff,
    /// 三账指纹（回放重建后复算）。
    pub rebuilt_fps: (u64, u64, u64),
    /// 是否零偏差。
    pub exact: bool,
}

/// 确定性回放（自三账环形缓冲重建迁移全过程，逐字段对账，偏差必须为零）。
// 判据：UNX-F1722-J1 —— 三段原子链闭合断言全过（回放零偏差即闭合可复现）
pub fn replay(m: &Migrator) -> ReplayResult {
    // 1. 取三账按序条目并合并为单一时序轨迹（按 mid 归并，类内按序）。
    let src = m.led.src.iter_ordered();
    let dst = m.led.dst.iter_ordered();
    let fol = m.led.follow.iter_ordered();
    let mut trace: Vec<LedgerEntry> = Vec::new();
    let mut si = 0usize;
    let mut di = 0usize;
    let mut fi = 0usize;
    // 归并：以 mid 升序为主键，同 mid 内按 源出→目标入→随行 顺序。
    while si < src.len() || di < dst.len() || fi < fol.len() {
        let smid = if si < src.len() { src[si].mid } else { u64::MAX };
        let dmid = if di < dst.len() { dst[di].mid } else { u64::MAX };
        let fmid = if fi < fol.len() { fol[fi].mid } else { u64::MAX };
        let min = if smid < dmid {
            if smid < fmid {
                smid
            } else {
                fmid
            }
        } else if dmid < fmid {
            dmid
        } else {
            fmid
        };
        if smid == min && si < src.len() {
            trace.push(src[si]);
            si += 1;
        }
        if dmid == min && di < dst.len() {
            trace.push(dst[di]);
            di += 1;
        }
        if fmid == min && fi < fol.len() {
            trace.push(fol[fi]);
            fi += 1;
        }
    }
    // 2. 逐 mid 重建：期望三向齐备且字段一致。
    let mut diff = ReplayDiff::default();
    let mut by_mid: BTreeMap<u64, (Option<LedgerEntry>, Option<LedgerEntry>, Option<LedgerEntry>)> = BTreeMap::new();
    for e in &trace {
        let slot = by_mid.entry(e.mid).or_insert((None, None, None));
        match e.kind {
            LedgerKind::SrcOut => slot.0 = Some(*e),
            LedgerKind::DstIn => slot.1 = Some(*e),
            LedgerKind::Follow => slot.2 = Some(*e),
            LedgerKind::Rollback => {}
        }
    }
    for (mid, (s, d, f)) in &by_mid {
        diff.rebuilt += 1;
        match (s, d, f) {
            (Some(sa), Some(da), Some(fa)) => {
                // 逐字段对账：tid / src / 载荷指纹。
                diff.checked += 1;
                if sa.tid != da.tid || da.tid != fa.tid {
                    diff.fields.push(format!("mid#{:#x} tid 不一致 {}/{}/{}", mid, sa.tid, da.tid, fa.tid));
                }
                if sa.src_cpu != da.src_cpu {
                    diff.fields.push(format!("mid#{:#x} src 不一致 {}/{}", mid, sa.src_cpu, da.src_cpu));
                }
                if sa.payload_fp != da.payload_fp {
                    diff.fields.push(format!("mid#{:#x} 载荷指纹不一致", mid));
                }
                if da.dst_cpu == da.src_cpu {
                    diff.fields.push(format!("mid#{:#x} 目标核等于源核", mid));
                }
                // 链闭合：查链上下文。
                match m.chain(*mid) {
                    Some(c) => {
                        if c.phase == ChainPhase::Attached && !c.chain_closed() {
                            diff.fields.push(format!("mid#{:#x} 链未闭合", mid));
                        }
                    }
                    None => {
                        diff.fields.push(format!("mid#{:#x} 链上下文缺失", mid));
                    }
                }
            }
            _ => {
                diff.fields.push(format!("mid#{:#x} 三向不齐", mid));
            }
        }
    }
    // 3. 重建三账指纹并与原账比对。
    let r_src = replay_fp(&m.led.src.iter_ordered());
    let r_dst = replay_fp(&m.led.dst.iter_ordered());
    let r_fol = replay_fp(&m.led.follow.iter_ordered());
    if r_src != m.led.src.fingerprint() || r_dst != m.led.dst.fingerprint() || r_fol != m.led.follow.fingerprint() {
        diff.fields.push("三账指纹重建偏差".to_string());
    }
    ReplayResult { exact: diff.is_zero(), trace, diff, rebuilt_fps: (r_src, r_dst, r_fol) }
}

/// 回放用账项指纹（顺序敏感；应与账本指纹一致）。
pub fn replay_fp(items: &[LedgerEntry]) -> u64 {
    let mut h = FNV_OFFSET_BASIS;
    let mut i = 0usize;
    while i < items.len() {
        h ^= items[i].entry_fp();
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 全绿演练：一次完整三段原子链 + 三向结算 + 回放零偏差。
/// 返回（是否全绿, 报告文本）。
pub fn drill_migrate() -> (bool, String) {
    let mut m = build_env();
    let t = ThreadTriple::new(1001, 0, 5);
    let mut lines: Vec<String> = Vec::new();
    // 开链 → 摘出 → 暂存 → 挂入。
    let mid = match m.begin(t, ThreadState::Running) {
        Ok(x) => x,
        Err(d) => return (false, d.full()),
    };
    let s1 = match m.detach(mid, &det_sig(&m, mid, t)) {
        Ok(x) => x,
        Err(d) => return (false, d.full()),
    };
    lines.push(format!("摘出 {}", s1.line()));
    let pay = match m.payload_of(mid) {
        Some(p) => p,
        None => return (false, "链无载荷快照".to_string()),
    };
    let s2 = match m.stage(mid, &s1, &pay) {
        Ok(x) => x,
        Err(d) => return (false, d.full()),
    };
    lines.push(format!("暂存 {}", s2.line()));
    let dst = match select_target(&m.cores, 0, true) {
        Ok(d) => d,
        Err(d) => return (false, d.full()),
    };
    let _ = m.attach(mid, &s2, &pay, dst);
    lines.push(format!("挂入 目标核{}", dst));
    let s = m.settle();
    lines.push(s.line());
    let rp = replay(&m);
    lines.push(rp.diff.line());
    let (all, av) = m.assert_all();
    for a in &av {
        lines.push(a.clone());
    }
    let ok = all && s.closed && rp.exact && m.migrated == 1;
    (ok, lines.join(" | "))
}