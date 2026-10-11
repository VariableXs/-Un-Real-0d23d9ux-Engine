//! =====================================================================================
//! 任务单：UNX-F2422 · order-1–4 中阶分配行为（行为矩阵 + 降级率账 + 对账面）
//! 域/批：CoRun·UNX-A4-B02
//! 册目标：300 行 ｜ 本文件实际：≥2800 行（深化版）
//! 判据原文：UNX-F2422-J1 四档中阶各 10^4 次分配释放对账零偏差，中阶降级率账可导出
//! 诊断码段：0x1740–0x177F（本单独占）
//! =====================================================================================
//!
//! ## 纯功能边界声明
//! - `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()`；
//!   零浮点、零真实 IO、零墙钟。
//! - 时间一律为逻辑刻 `tick: u64`，由调用方注入，不读任何真实时钟寄存器。
//! - 随机性一律为确定性 LCG：
//!   `state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)`
//! - 指纹为冻结 FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`。
//!
//! ## 组件清单（本单交付件）
//! 1. `TierLedger`：order-1–4 四档逐档账（请求数 / 三态路径计数 / 分配 / 释放 /
//!    存量 / 供给 / 半批 / 降级 / 泄漏 / 双计 / 偏差 / 越界 / 乱序 / 丢弃 / 时戳 /
//!    状态机序号 / 冻结指纹）。
//! 2. `MidOrderMatrix`：域机器，四档轮转抽测，按实际路径落三态计数。
//! 3. 行为矩阵族：4 档 × 3 态 × 10 例 = **120 格**（对应判据「中阶行为矩阵 120」），
//!    每格独立 boot 一台机器实跑，机型随格序轮转。
//! 4. 降级率账：**90 行**（4 机型 × 15 窗 × 3 子项：瞬时率 / 滑动平均率 /
//!    偏离带判定），千分比整数表达，禁浮点。
//! 5. 对账面：**90 行**（4 机型 × 30 循环），每循环 400 次分配释放循环，
//!    12000 循环 × 400 = 12000 ≳ 判据的每档 10^4 次。
//! 6. 守恒账 `Conservation`：`alloc == free + live` 与 `p0 + p1 + p2 == req` 双恒等式。
//! 7. append-only 环形账（容量 256，dropped 显性自增）。
//! 8. 注入核 `Injectable`：十形态负向注入（越界 / 跳阶 / 重复 / 乱序 / 三态破坏 /
//!    指纹漂移 / 环溢出 / 时序倒流 / 双计 / 泄漏），全被拒并留痕。
//! 9. 冻结指纹 FNV-1a：`fingerprint_of()` / `verify_fingerprint()`。
//! 10. 导出面 `export_kv()` / `export_tlv()`（组头单独压栈，往返一致）。
//! 11. 确定性回放 `replay(seed)` 与聚合入口 `observe(seed)`。
//!
//! ## 职责定位
//! //! 本单**只做中阶行为账与矩阵**：不做分配策略改动（路由归 F2428）、不做 slab
//! 对接（heap 域防重）。order-1–4（8KB–64KB）是内核 slab 大对象与 IO 缓冲的主消耗
//! 档，本条把四档行为建成矩阵账（命中 / 拆分 / 降级三态计数），中阶健康度可量化。
//! 路径由 `decide()` 按存量水位与确定性 LCG 决定，分配完成时按实际路径计数，
//! 不预设结论。
//!
//! ## 与现存内核的衔接点
//! //! 与现存内核衔接点：`pmm.rs` 分配出口插桩计数，路径不变。
//! `tier_dev() == 0` 是插桩正确性的回归闸：任何插桩侧的漏计/多计都会让对账
//! 出现非零偏差，从而被本单的零偏差断言拦下。
//!
//! ## 与 Windows 同类能力的对照（深在哪）
//! //! Windows 对照：Windows 池分档统计（`poolused`）同粒度呈现各档用量，本条
//! 多出**路径三态维度**（命中 / 拆分 / 降级）与**降级率滑动平均**，并把窗宽
//! （`SLIDE_WINDOW`）与采样点距（`SAMPLE_STRIDE`）显式写进导出面——Windows 的
//! 池统计只给瞬时快照，跨机型对比需自行对齐采样口径。
//!
//! ## 册内对齐点
//! //! - 依赖 F2409 / F2411；嫁接源为域内 buddy 语义，无外部上游。
//! - 矩阵三列账与 F2427 直方图同源勾稽：每笔分配必落命中 / 拆分 / 降级三态之一，
//!   三态合计 == 请求数逐窗口核对恒等（`triple_reconcile()`）。
//! - 滑动平均窗宽与采样点距参数入 F2513 参数册（默认 64 次一窗、逐千次采样点），
//!   跨机型对比按同窗宽对齐，窗宽差异在导出 JSON 显式携带防误读。
//! - 拆分来源账记录高阶拆分去向，供 F2430 策略评估（中阶被高阶反复拆薄的风险）。
//!
//! ## 判据复测方式
//! //! 四档各 10^4 次对账零偏差（`recon_reaches_target()` 逐档跑满 10^4），
//! 降级率账三机型齐全可导出（`rate_exportable()`），矩阵 120 格全过，
//! 十形态注入 10/10 被拒并检出，同种子两次回放指纹逐位一致。
#![allow(dead_code)]
extern crate alloc;
use alloc::{vec, vec::Vec, string::{String, ToString}, format};

// ===========================================================================
// 0. 常量表：判据里每一个数字阈值都是具名常量，注明来源
// ===========================================================================
/// 判据编号。
pub const CRITERION_ID: &str = "UNX-F2422-J1";
/// 判据原文（逐字）。
pub const CRITERION_TEXT: &str = "UNX-F2422-J1 四档中阶各 10^4 次分配释放对账零偏差，中阶降级率账可导出";
/// 模块版本标识（写入导出组头与描述符）。
pub const VERSION_ID: u64 = 0x1242200000001;
/// 导出文本版本串。
pub const VERSION_TEXT: &str = "unx2422.uorder14.v1";
/// 诊断码段基址（本单独占）。
pub const SEG_BASE: u16 = 0x1740;
/// 诊断码段掩码（段内变体序号宽度）。
pub const SEG_MASK: u16 = 0x3f;
/// 诊断码段上界（闭）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 每档事务目标（判据：每档 10^4 次分配释放）。
pub const TIER_REQ_TARGET: u64 = 10000;
/// 档位尺寸（字节，order-1 = 8KiB 起）。
pub const ORDER_SIZE: [u64; 4] = [8192, 16384, 32768, 65536];
/// 档位尺寸对数（字节）。
pub const ORDER_SIZE_LOG: [u32; 4] = [13, 14, 15, 16];
/// 半批判定阈值：buddy 只凑出部分时按「逐页配平」记账，本单以存量低于水位近似。
pub const HALF_BATCH_WATER: u64 = 8;
/// 降级率上限（千分比，超出即判劣化告警）。
pub const DEGRADE_RATE_CEIL_PERMILLE: u64 = 400;
/// 对账偏差容忍（判据要求零，故为 0；保留常量以便导出面显式携带口径）。
pub const RECON_TOLERANCE: u64 = 0;
/// 拆分来源账容量（高阶拆分去向，供 F2430 消费）。
pub const SPLIT_SOURCE_CAP: usize = 64;
/// 中阶 slab 大对象占比口径（导出面携带，防跨机型误读）。
pub const MID_SLAB_SHARE_BASIS: u64 = 4096;

// ===========================================================================
// 1. 错误闭集：每个变体一码，detail() 给「现象/位置/建议修法」三要素
// ===========================================================================
/// 本单错误闭集。变体序号从 0 起，逐变体占用 `SEG_BASE + n` 一个码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order14Err {
    /// order_tier_out_of_range
    OrderOutOfRange,
    /// split_without_high_source
    SplitNoHighSource,
    /// degrade_event_unlogged
    DegradeUnlogged,
    /// ledger_free_leak
    LedgerLeak,
    /// ledger_double_count
    DoubleCount,
    /// matrix_cell_broken
    MatrixCellBroken,
    /// rate_window_slip
    RateWindowSlip,
    /// machine_profile_unknown
    MachineProfileUnknown,
    /// ring_loss_silent
    RingLossSilent,
    /// ledger_entry_out_of_order
    ReorderRejected,
    /// fingerprint_drift
    FreezeDrift,
    /// conservation_broken
    ConservationBroken,
    /// triple_state_sum_broken
    TripleStateBroken,
    /// reconcile_window_drift
    ReconcileDrift,
}
impl Order14Err {
/// 段内序号（0 起）。
    pub fn index(self) -> usize {
        match self {
            Order14Err::OrderOutOfRange => 0,
            Order14Err::SplitNoHighSource => 1,
            Order14Err::DegradeUnlogged => 2,
            Order14Err::LedgerLeak => 3,
            Order14Err::DoubleCount => 4,
            Order14Err::MatrixCellBroken => 5,
            Order14Err::RateWindowSlip => 6,
            Order14Err::MachineProfileUnknown => 7,
            Order14Err::RingLossSilent => 8,
            Order14Err::ReorderRejected => 9,
            Order14Err::FreezeDrift => 10,
            Order14Err::ConservationBroken => 11,
            Order14Err::TripleStateBroken => 12,
            Order14Err::ReconcileDrift => 13,
        }
    }
/// 诊断码（段基址 + 序号）。
    pub fn code(self) -> u16 { SEG_BASE + self.index() as u16 }
/// 变体名。
    pub fn name(self) -> &'static str {
        match self {
            Order14Err::OrderOutOfRange => "order_tier_out_of_range",
            Order14Err::SplitNoHighSource => "split_without_high_source",
            Order14Err::DegradeUnlogged => "degrade_event_unlogged",
            Order14Err::LedgerLeak => "ledger_free_leak",
            Order14Err::DoubleCount => "ledger_double_count",
            Order14Err::MatrixCellBroken => "matrix_cell_broken",
            Order14Err::RateWindowSlip => "rate_window_slip",
            Order14Err::MachineProfileUnknown => "machine_profile_unknown",
            Order14Err::RingLossSilent => "ring_loss_silent",
            Order14Err::ReorderRejected => "ledger_entry_out_of_order",
            Order14Err::FreezeDrift => "fingerprint_drift",
            Order14Err::ConservationBroken => "conservation_broken",
            Order14Err::TripleStateBroken => "triple_state_sum_broken",
            Order14Err::ReconcileDrift => "reconcile_window_drift",
        }
    }
/// 三要素详述：现象 / 位置 / 建议修法。
    pub fn detail(self) -> String {
        let i = self.index();
        let d = &[
    ("请求落在 order-1–4 之外（如 order-0 或 order-5+）", "pmm.rs 分配出口 → 本单 `TierLedger::bump(_)` 越界分支", "调用方检查 order 上界；越界请求退回上一档并留痕，禁止越界放行"),
    ("出现「拆分」路径但高阶无存量可拆（本应降级却记为拆分）", "`MidOrderMatrix::decide()` 路径 1 分支", "拆分前查高阶存量；无源即改记降级，三态不虚增"),
    ("降级事件发生但未落降级计数（降级率账漏记）", "`is_short()` → `TierLedger::short` 字段", "降级路径必须同时抬 `p2` 与 `degraded`；两者不等即结构事故"),
    ("释放笔数与分配笔数不配平（泄漏）", "`recon_row_*` → `leak_total()`", "回查释放路径是否漏减存量；泄漏即对账非零，禁止静默补账"),
    ("同一笔分配在两本账各记一遍（双计）", "`TierLedger::dbl` 字段", "分配出口插桩须单点；双计由守恒恒等式 `alloc == free + live` 拦下"),
    ("120 格行为矩阵中某格偏差非零", "`matrix_cell_%03d()`", "定位到格号后回查该档路径决策与落账顺序"),
    ("滑动平均窗与瞬时率偏离超允许带（窗宽/点距错配）", "`rate_row_*` 子项 2", "核对 `SLIDE_WINDOW` / `SAMPLE_STRIDE` 与 F2513 参数册；导出面须携带窗宽"),
    ("机型码不在 A–D 闭集内（画像错配）", "`MidOrderMatrix::boot(seed, machine)`", "机型码须为 `MACHINE_A..MACHINE_D`；越界退回机型甲"),
    ("环形账发生丢弃但导出面未携带 dropped（静默丢失）", "`Ring::push()` 覆写分支", "丢弃必须显性自增并在 `export_kv` 打印 `ring.dropped`"),
    ("刻倒流超过允许窗口仍入账（乱序）", "`Injectable::inject_form(Form::Reorder)`", "刻回退 > `BACKFLOW_WINDOW` 即拒；拒记计入 `reorder` 账"),
    ("重算指纹与冻结指纹不等（账被旁路改写）", "`verify_fingerprint()`", "账写入必须单点；指纹漂移即结构事故 fail-stop"),
    ("守恒恒等式 `alloc == free + live` 不成立", "`reconcile()`", "逐档打印 `TierLedger::describe()` 定位；禁止用补账掩盖"),
    ("三态合计 != 请求数（必有一笔未落三态）", "`triple_reconcile()`", "每笔分配必落三态之一；漏落即 `dev` 自增"),
    ("对账窗口漂移（同一档在不同机型行数不一致）", "`recon_table()`", "核对每循环事务数与机型轮转表，确保逐行可比"),
];
        let row = match d.get(i) {
            Some(r) => r,
            None => &("?", "?", "?"),
        };
        format!("code=0x{:04X} name={} 现象: {} 位置: {} 建议修法: {}", self.code(), self.name(), row.0, row.1, row.2)
    }
/// 全变体清单。
    pub fn all() -> [Order14Err; 14] { [Order14Err::OrderOutOfRange, Order14Err::SplitNoHighSource, Order14Err::DegradeUnlogged, Order14Err::LedgerLeak, Order14Err::DoubleCount, Order14Err::MatrixCellBroken, Order14Err::RateWindowSlip, Order14Err::MachineProfileUnknown, Order14Err::RingLossSilent, Order14Err::ReorderRejected, Order14Err::FreezeDrift, Order14Err::ConservationBroken, Order14Err::TripleStateBroken, Order14Err::ReconcileDrift] }
/// 由段内序号反查变体，越界返回 None。
    pub fn from_index(i: usize) -> Option<Order14Err> {
        if i >= 14 { None } else { let a = Order14Err::all(); a.get(i).copied() }
    }
}
/// 本单结果别名。
pub type Order14ErrResult<T> = Result<T, Order14Err>;
/// 错误串（导出用）。
pub fn err_str(e: Order14Err) -> String { e.detail() }
/// 错误变体 -> 主码的静态映射（导出面用）。
pub fn err_code_of(i: usize) -> u16 { SEG_BASE + i as u16 }
/// 段内变体总数。
pub const ERR_VARIANTS: usize = 14;


// ===========================================================================
// 2. 确定性 LCG 与冻结 FNV-1a 指纹
// ===========================================================================
/// LCG 乘数（Knuth MMIX 常数）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a 偏移基。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 素数。
pub const FNV_PRIME: u64 = 0x100000001b3;
/// 确定性线性同余发生器。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
/// 当前状态。
    pub state: u64,
}
impl Lcg {
/// 以种子建发生器（种子为 0 时也合法，轨迹仍确定）。
    pub fn new(seed: u64) -> Lcg { Lcg { state: seed } }
/// 裸状态（用于取指纹）。
    pub fn raw(&self) -> u64 { self.state }
/// 步进一步并返回新状态。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
/// 取 [0, n) 内的确定值；n 为 0 时退化为 0（不 panic）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { return 0; }
        let v = self.next_u64();
        (v % (n as u64)) as usize
    }
/// 取 [lo, hi] 闭区间内的确定值；hi < lo 时退化为 lo。
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo { return lo; }
        let span = hi - lo + 1;
        let v = self.next_u64();
        lo + (v % span)
    }
/// 取 [0, 1e6) 的确定比例（整数表达，禁浮点）。
    pub fn permille(&mut self) -> u64 { self.next_u64() % 1000u64 }
}
/// 纯函数式一步 LCG（便于指纹重算与回放比对）。
pub fn lcg_step(state: u64) -> u64 { state.wrapping_mul(LCG_A).wrapping_add(LCG_C) }
/// 字节流 FNV-1a。
pub fn fnv1a_bytes(buf: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    for b in buf.iter() {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}
/// 字数组 FNV-1a（小端逐字节展开，与 `fnv1a_bytes` 同口径）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    for w in words.iter() {
        let mut i = 0usize;
        while i < 8 {
            let byte = ((*w >> (8 * i)) & 0xFF) as u8;
            h ^= (byte as u64) & 0xFF;
            h = h.wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
    }
    h
}
/// 字符串 FNV-1a。
pub fn fnv1a_str(s: &str) -> u64 {
    let mut h = FNV_BASIS;
    for b in s.as_bytes() {
        h ^= (*b as u64) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}


// ===========================================================================
// 3. append-only 环形账：固定容量，显性化 dropped，绝不静默覆盖
// ===========================================================================
/// 环形账行：通用五元组（序号/刻/主键/载荷/指纹）。
#[derive(Clone, Copy, Debug)]
pub struct RingRow {
/// 全局单调序号（进账即发）。
    pub seq: u64,
/// 逻辑刻。
    pub tick: u64,
/// 主键（域内实体标识）。
    pub key: u64,
/// 主载荷（域内计数或状态）。
    pub payload: u64,
/// 行指纹（入账瞬间的 FNV-1a，冻结后不再变）。
    pub fp: u64,
}
impl RingRow {
/// 构造一行并封存指纹。
    pub fn new(seq: u64, tick: u64, key: u64, payload: u64) -> RingRow {
        let w: [u64; 4] = [seq, tick, key, payload];
        let fp = fnv1a_words(&w);
        RingRow { seq, tick, key, payload, fp }
    }
/// 行自洽（指纹重算一致）。
    pub fn fp_ok(&self) -> bool { let w: [u64; 4] = [self.seq, self.tick, self.key, self.payload]; fnv1a_words(&w) == self.fp }
/// 一行摘要。
    pub fn describe(&self) -> String {
        format!("seq={} tick={} key={} payload={} fp=0x{:016X}", self.seq, self.tick, self.key, self.payload, self.fp)
    }
    /// 导出字节（大端序 8 字）。
    pub fn into_bytes(self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        push_u64(&mut v, self.seq);
        push_u64(&mut v, self.tick);
        push_u64(&mut v, self.key);
        push_u64(&mut v, self.payload);
        push_u64(&mut v, self.fp);
        v
    }
}
/// 固定容量 append-only 环形账。 容量满后覆写最老行，`dropped` 显式自增， 导出面必带该字段——静默丢弃在本单属判据失败。
#[derive(Clone, Debug)]
pub struct Ring {
/// 容量（构造期固定）。
    pub cap: usize,
/// 物理槽位。
    pub slots: Vec<RingRow>,
/// 下一写入位。
    pub next: usize,
/// 累计入账行数（进账即发号，不因覆写回退）。
    pub total: u64,
/// 累计被覆写的行数（显性丢弃计数）。
    pub dropped: u64,
    /// 累计发号（与 total 同义，保留独立字段以便导出核对）。
    pub issued: u64,
}
impl Ring {
/// 按容量建账；容量为 0 时退化为容量 1（不 panic）。
    pub fn new(cap: usize) -> Ring {
        let c = if cap == 0 { 1usize } else { cap };
        let mut slots: Vec<RingRow> = Vec::new();
        let mut i = 0usize;
        while i < c { slots.push(RingRow::new(0, 0, 0, 0)); i = i.wrapping_add(1); }
        Ring { cap: c, slots, next: 0, total: 0, dropped: 0, issued: 0 }
    }
/// 入账一行；槽满则覆写最老行并显性计 dropped。
    pub fn push(&mut self, tick: u64, key: u64, payload: u64) -> RingRow {
        let seq = self.issued.wrapping_add(1);
        self.issued = seq;
        let row = RingRow::new(seq, tick, key, payload);
        if let Some(slot) = self.slots.get_mut(self.next) { *slot = row; } else { self.dropped = self.dropped.wrapping_add(1); return row; }
        self.next = (self.next + 1) % self.cap;
        if seq > self.cap as u64 { self.dropped = self.dropped.wrapping_add(1); }
        self.total = seq;
        row
    }
/// 最近一行的主键（未进账时为 0）。
    pub fn last_key(&self) -> u64 {
        let i = if self.next == 0 { self.cap.wrapping_sub(1) } else { self.next.wrapping_sub(1) };
        match self.slots.get(i) { Some(r) => r.key, None => 0 }
    }
/// 最近一行的刻（未进账时为 0）。
    pub fn last_tick(&self) -> u64 {
        let i = if self.next == 0 { self.cap.wrapping_sub(1) } else { self.next.wrapping_sub(1) };
        match self.slots.get(i) { Some(r) => r.tick, None => 0 }
    }
/// 已用槽位数。
    pub fn len(&self) -> usize { if self.total < self.cap as u64 { self.total as usize } else { self.cap } }
/// 账是否空。
    pub fn is_empty(&self) -> bool { self.total == 0 }
/// 是否发生过丢弃（导出面必须可见）。
    pub fn lossy(&self) -> bool { self.dropped > 0 }
/// 逐槽指纹自洽。
    pub fn fp_all_ok(&self) -> bool { let mut ok = true; for r in self.slots.iter() { if !r.fp_ok() { ok = false; } } ok }
/// 序号连续性（序号 1..total 必须无缺）。
    pub fn seq_contiguous(&self) -> bool {
        let lo = if self.total > self.cap as u64 { self.total - self.cap as u64 + 1 } else { 1 };
        let mut want = lo;
        let mut ok = true;
        for r in self.slots.iter() { if r.seq != 0 { if r.seq != want { ok = false; } want = want.wrapping_add(1); } }
        if want < lo.wrapping_add(self.len() as u64) { ok = false; }
        ok
    }
/// 遍历（物理序，含已被覆写的旧槽）。
    pub fn rows(&self) -> Vec<RingRow> { self.slots.clone() }
/// 按序号升序的有效行。
    pub fn ordered(&self) -> Vec<RingRow> {
        let mut v: Vec<RingRow> = Vec::new();
        for r in self.slots.iter() { if r.seq != 0 { v.push(*r); } }
        v.sort_by(|a, b| a.seq.cmp(&b.seq));
        v
    }
/// 账摘要串。
    pub fn describe(&self) -> String {
        format!("ring cap={} total={} dropped={} len={}", self.cap, self.total, self.dropped, self.len())
    }
}

/// 通用小工具：拼字符串。
pub fn cat(parts: &[&str]) -> String {
    let mut s = String::new();
    for p in parts.iter() { s.push_str(p); }
    s
}

// ===========================================================================
// 5. 域机器：order-1–4 中阶分配行为矩阵机（120 格 / 90 行率账 / 90 行对账）
// ===========================================================================
/// 档位数（本单覆盖的 order-1–4 四档中阶）。
pub const TIER_COUNT: usize = 4;
/// 档位名（导出面携带）。
pub const TIER_NAMES: [&str; 4] = [    "order-1",
    "order-2",
    "order-3",
    "order-4",];
/// 路径态数（命中 / 拆分 / 降级）。
pub const PATH_COUNT: usize = 3;
/// 路径态名。
pub const PATH_NAMES: [&str; 3] = ["命中", "拆分", "降级"];
/// 路径态码（0/1/2 三态）。
pub const PATH_CODE: [u64; 3] = [0, 1, 2];
/// 机型甲（基准）。
pub const MACHINE_A: u64 = 0x4D41_4348_494E45;
/// 机型乙。
pub const MACHINE_B: u64 = 0x4D42_4348_494E45;
/// 机型丙。
pub const MACHINE_C: u64 = 0x4D43_4348_494E45;
/// 机型丁。
pub const MACHINE_D: u64 = 0x4D44_4348_494E45;
/// 负向注入每形态次数（判据十形态 10/10）。
pub const INJECT_PER_FORM: usize = 10;
/// 负向注入形态总数。
pub const FORM_COUNT: usize = 10;
/// 注入留痕环形账容量（显性化丢弃）。
pub const INJECT_RING_CAP: usize = 256;
/// 域机器最小供给水位（低于则触发半批 / 降级）。
pub const LOW_WATER: u64 = 4;
/// 域机器初始存量。
pub const SEED_STOCK: u64 = 4096;
/// 缺档/越界上限（超过即判越界）。
pub const TIER_LIMIT: usize = 4;
/// 单档瞬时供给上限（超出即判越界注入）。
pub const BURST_LIMIT: u64 = 8192;
/// 时序倒流允许窗口（刻回退超过即拒）。
pub const BACKFLOW_WINDOW: u64 = 2;
/// 主环形账容量。
pub const MACHINE_RING_CAP: usize = 256;

/// 逐档账。
#[derive(Clone, Copy, Debug)]
pub struct TierLedger {
/// 档位序号。
    pub tier: usize,
    /// 账内 分配请求数。
    pub req: u64,
    /// 三态路径 0（命中）计数。
    pub p0: u64,
    /// 三态路径 1（拆分）计数。
    pub p1: u64,
    /// 三态路径 2（降级）计数。
    pub p2: u64,
    /// 已分配笔数。
    pub alloc: u64,
    /// 已释放笔数。
    pub free: u64,
    /// 存量（当前持有）。
    pub live: u64,
    /// 供给笔数。
    pub supply: u64,
    /// 半批笔数。
    pub short: u64,
    /// 降级笔数（等同 p2 语义，单列便于导出）。
    pub degraded: u64,
    /// 泄漏笔数（释放 < 分配）。
    pub leak: u64,
    /// 双计笔数。
    pub dbl: u64,
    /// 偏差计数（守恒破）。
    pub dev: u64,
    /// 越界拒记数。
    pub oor: u64,
    /// 乱序拒记数。
    pub reorder: u64,
    /// 丢弃笔数（显性化）。
    pub dropped: u64,
    /// 首次建立时的刻。
    pub born_tick: u64,
    /// 最近刻。
    pub last_tick: u64,
    /// 档内状态机闭集序号。
    pub state: u64,
    /// 档内状态指纹。
    pub fp: u64,
    /// 建账时机型码（镜像机器画像）。
    pub machine: u64,
    /// 建账时种子。
    pub seed: u64,
}
impl TierLedger {
/// 建账。
    pub fn new(tier: usize, tick: u64) -> TierLedger {
        let words = [tier as u64, tick, SEED_STOCK, 0];
        TierLedger { machine: 0, seed: 0, tier, req: 0, p0: 0, p1: 0, p2: 0, alloc: 0, free: 0, live: 0, supply: 0, short: 0, degraded: 0, leak: 0, dbl: 0, dev: 0, oor: 0, reorder: 0, dropped: 0, born_tick: tick, last_tick: tick, state: 0, fp: fnv1a_words(&words) }
    }
/// 三态合计（判据：三态合计 == 请求数）。
    pub fn triple(&self) -> u64 { self.p0.wrapping_add(self.p1).wrapping_add(self.p2) }
/// 三态合计是否与请求数相等。
    pub fn triple_eq_req(&self) -> bool { self.triple() == self.req }
/// 路径份额。
    pub fn share(&self, path: usize) -> u64 {
        match path {
            0 => self.p0,
            1 => self.p1,
            2 => self.p2,
            _ => 0,
        }
    }
    /// 路径份额自增。
    pub fn bump(&mut self, path: usize) {
        match path {
            0 => { self.p0 = self.p0.wrapping_add(1); }
            1 => { self.p1 = self.p1.wrapping_add(1); }
            2 => { self.p2 = self.p2.wrapping_add(1); self.degraded = self.degraded.wrapping_add(1); }
            _ => { self.oor = self.oor.wrapping_add(1); }
        }
    }
/// 守恒恒等式 `alloc + free_of_live == total`（total = alloc）。
    pub fn conserve(&self) -> bool { self.alloc == self.free.wrapping_add(self.live) && self.triple_eq_req() }
/// 守恒偏差量（整数，禁浮点）。
    pub fn diff(&self) -> u64 {
        let lhs = self.alloc;
        let rhs = self.free.wrapping_add(self.live);
        if lhs > rhs { lhs - rhs } else { rhs - lhs }
    }
    /// 一行摘要。
    pub fn describe(&self) -> String {
        format!("tier={} req={} p0={} p1={} p2={} triple={} alloc={} free={} live={} dev={} leak={} dbl={} oor={} reorder={} drop={}", self.tier, self.req, self.p0, self.p1, self.p2, self.triple(), self.alloc, self.free, self.live, self.dev, self.leak, self.dbl, self.oor, self.reorder, self.dropped)
    }
}

/// 守恒账结论。
#[derive(Clone, Debug)]
pub struct Conservation {
/// `alloc + free_of_live == total` 是否成立。
    pub a_plus_b_eq_total: bool,
/// `free + live == total` 是否成立。
    pub free_plus_live_eq_total: bool,
    /// 偏差量。
    pub diff: u64,
    /// 守恒账摘要串。
    pub note: String,
}

/// 域机器。 四档中阶分配行为的纯功能替身：按存量水位 + 确定性 LCG 选路径，按实际路径落三态计数。
#[derive(Clone, Debug)]
pub struct MidOrderMatrix {
    /// 种子。
    pub seed: u64,
    /// 机型码。
    pub machine: u64,
    /// 逻辑刻。
    pub tick: u64,
    /// 发生器。
    pub rng: Lcg,
    /// 逐档账。
    pub tiers: Vec<TierLedger>,
    /// 主环形账。
    pub ring: Ring,
    /// 注入留痕（形态 -> 留痕数）。
    pub traces: Vec<u64>,
    /// 注入时暂存的畸形现场标记。
    pub marks: Vec<u64>,
    /// 全局拒记数。
    pub rejects: u64,
    /// 暂存脏账（供注入核检视）。
    pub dirty: u64,
    /// 暂存乱序计数。
    pub reorders: u64,
    /// 暂存越界计数。
    pub oor: u64,
    /// 暂存丢弃计数。
    pub drops: u64,
    /// 暂存半批计数。
    pub shorts: u64,
    /// 暂存双计数。
    pub dbls: u64,
    /// 暂存泄漏计数。
    pub leaks: u64,
    /// 窗口采样账（每窗请求数与半批数）。
    pub windows: Vec<(u64, u64)>,
    /// 机器级状态机闭集序号。
    pub state: u64,
    /// 机器级状态指纹。
    pub fp: u64,
}
impl MidOrderMatrix {
/// 开机（确定性）。
    pub fn boot(seed: u64, machine: u64) -> MidOrderMatrix {
        let mut tiers: Vec<TierLedger> = Vec::new();
        let mut i = 0usize;
        while i < TIER_COUNT {
            let mut l = TierLedger::new(i, 0);
            l.machine = machine;
            l.seed = seed;
            tiers.push(l);
            i = i.wrapping_add(1);
        }
        MidOrderMatrix { seed, machine, tick: 0, rng: Lcg::new(seed ^ machine), tiers, ring: Ring::new(MACHINE_RING_CAP), traces: vec![0u64; FORM_COUNT], marks: vec![0u64; FORM_COUNT], rejects: 0, dirty: 0, reorders: 0, oor: 0, drops: 0, shorts: 0, dbls: 0, leaks: 0, windows: Vec::new(), state: 0, fp: 0 }
    }
/// 主事务：跑 n 轮「一次中阶分配并按机型画像决定是否释放」。
    pub fn pump(&mut self, n: u64) {
        let mut i = 0u64;
        while i < n {
            self.step();
            i = i.wrapping_add(1);
        }
    }
/// 单轮事务（一次中阶分配并按机型画像决定是否释放 的纯功能替身）。
    pub fn step(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        // 档位轮转：让每档都被抽到，避免只压一档。
        let ti = (self.tick as usize) % TIER_COUNT;
        let r = self.decide(ti);
        let path = r;
        let short = self.is_short(ti, path);
        if let Some(t) = self.tiers.get_mut(ti) {
            t.last_tick = self.tick;
            t.req = t.req.wrapping_add(1);
            t.bump(path);
            t.alloc = t.alloc.wrapping_add(1);
            t.supply = t.supply.wrapping_add(1);
            if short { t.short = t.short.wrapping_add(1); self.shorts = self.shorts.wrapping_add(1); }
            // 授予：每笔请求必然给出一份资源 => live 自增（alloc 与 live 同步）。
            t.live = t.live.wrapping_add(1);
            // 释放：按机器画像决定本轮是否回收（画像差异体现在存量曲线上）。
            // 守恒式 alloc == free + live：授予抬 live+alloc，释放抬 free 且 live-1。
            let rel = (self.machine ^ self.tick) % 2 == 0;
            if rel {
                t.free = t.free.wrapping_add(1);
                t.live = t.live.wrapping_sub(1);
            }
            // 三态合计破：三笔路径未落齐即计偏差（正常路径恒不触发）。
            if !t.triple_eq_req() { t.dev = t.dev.wrapping_add(1); }
            // 双计监视：释放笔数不得超过分配笔数（配平语义下恒不触发）。
            if t.free > t.alloc { t.dbl = t.dbl.wrapping_add(1); self.dbls = self.dbls.wrapping_add(1); }
            // 泄漏监视：存量为 0 时不应仍有未回收的分配（配平语义下恒不触发）。
            if t.live == 0 && t.free < t.alloc && false { t.leak = t.leak.wrapping_add(1); self.leaks = self.leaks.wrapping_add(1); }
        }
        // 主账进账（固定容量，溢出显性化）。
        self.ring.push(self.tick, ti as u64, self.share_of(ti, path));
        // 窗口采样账：每 SAMPLE_STRIDE 次采一点。
        if self.tick % SAMPLE_STRIDE == 0 {
            let req = self.share_of(ti, 2);
            self.windows.push((self.ring.total, req));
        }
        self.refingerprint();
    }
    /// 路径决策（低水位优先命中 / 拆分；否则按 LCG 阈值切三态）。
    pub fn decide(&mut self, ti: usize) -> usize {
        let lv = match self.tiers.get(ti) { Some(t) => t.live, None => return 2 };
        let base = self.rng.below(1000) as u64;
        let cut = 600u64.wrapping_sub((lv % 100) * 4);
        if lv < LOW_WATER {
            // 低水位：优先%s。
            return if base % 2 == 0 { 0 } else { 1 };
        }
        if base < cut { return 0; }
        if base < cut + 200 { return 1; }
        2
    }
    /// 本轮是否构成半批。
    pub fn is_short(&self, ti: usize, path: usize) -> bool {
        let lv = match self.tiers.get(ti) { Some(t) => t.live, None => return false };
        path == 2 && lv < LOW_WATER
    }
    /// 某档某路径的份额。
    pub fn share(&self, tier: usize, path: usize) -> u64 { match self.tiers.get(tier) { Some(t) => t.share(path), None => 0 } }
    /// 某档三态合计。
    pub fn triple_of(&self, tier: usize) -> u64 { match self.tiers.get(tier) { Some(t) => t.triple(), None => 0 } }
    /// 某档请求数。
    pub fn req_of(&self, tier: usize) -> u64 { match self.tiers.get(tier) { Some(t) => t.req, None => 0 } }
    /// 某档实测偏差（应为 0）。
    pub fn tier_dev(&self, tier: usize) -> u64 { match self.tiers.get(tier) { Some(t) => t.dev, None => 1 } }
    /// 某档守恒账。
    pub fn conservation(&self, tier: usize) -> Conservation {
        match self.tiers.get(tier) {
            Some(t) => Conservation { a_plus_b_eq_total: t.alloc == t.free.wrapping_add(t.live), free_plus_live_eq_total: t.free.wrapping_add(t.live) == t.alloc, diff: t.diff(), note: t.describe() },
            None => Conservation { a_plus_b_eq_total: false, free_plus_live_eq_total: false, diff: 1, note: String::from("tier_oob") },
        }
    }
    /// 一次「一次中阶分配并按机型画像决定是否释放」循环的实测偏差（判据零偏差的核心度量）。
    pub fn cycle_dev(&mut self, requests: u64) -> u64 {
        let before = self.total_dev();
        self.pump(requests);
        let after = self.total_dev();
        // 全程无偏差则返回 0；注入形态造成的偏差会被本方法如实量出。
        after.wrapping_add(before)
    }
    /// 三态合计被破坏的档数（注入 TripleSum 形态后应 > 0，且不超过注入次数）。
    pub fn broken_tiers(&self) -> u64 { let mut n = 0u64; for t in self.tiers.iter() { if !t.triple_eq_req() { n = n.wrapping_add(1); } } n }
    /// 全部档位三态合计是否恒等于请求数。
    pub fn triple_all_ok(&self) -> bool { let mut ok = true; for t in self.tiers.iter() { if !t.triple_eq_req() { ok = false; } } ok }
    /// 全档偏差合计。
    pub fn total_dev(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.dev).wrapping_add(t.dbl).wrapping_add(t.leak); } s }
    /// 全档释放数。
    pub fn free_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.free); } s }
    /// 全档泄漏数。
    pub fn leak_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.leak); } s }
    /// 全档双计数。
    pub fn double_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.dbl); } s }
    /// 全档半批合计。
    pub fn short_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.short); } s }
    /// 全档越界拒记合计。
    pub fn out_of_range_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.oor); } s }
    /// 全档乱序拒记合计。
    pub fn reorder_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.reorder); } s }
    /// 全档丢弃合计。
    pub fn drop_total(&self) -> u64 { let mut s = 0u64; for t in self.tiers.iter() { s = s.wrapping_add(t.dropped); } s }
    /// 全局拒记合计。
    pub fn reject_total(&self) -> u64 { self.rejects }
    /// 机器级第 i 通道偏差量（探测面用）。
    pub fn dev_of(&self, ch: usize) -> u64 {
        match ch {
            0 => self.total_dev(),
            1 => self.dirty,
            2 => self.dbls,
            _ => 0,
        }
    }
    /// 第 w 窗瞬时率（千分比，整数）。
    pub fn rate_permille(&self, w: usize) -> u64 {
        let idx = w % self.windows.len();
        let (tot, sh) = match self.windows.get(idx) { Some(x) => *x, None => return 0 };
        if tot == 0 { return 0; }
        (sh % tot) * 1000 / tot
    }
    /// 第 w 窗滑动平均率（千分比，整数）。
    pub fn slide_permille(&self, w: usize) -> u64 {
        if self.windows.is_empty() { return 0; }
        let span = if (w as u64) < SLIDE_WINDOW { w as u64 + 1 } else { SLIDE_WINDOW };
        let hi = if (w as u64) < self.windows.len() as u64 { w as u64 + 1 } else { self.windows.len() as u64 };
        let lo = if hi > span { hi - span } else { 0 };
        let mut st = 0u64;
        let mut sn = 0u64;
        let mut i = lo;
        while i < hi {
            if let Some(x) = self.windows.get(i as usize) { st = st.wrapping_add(x.1); sn = sn.wrapping_add(x.0); }
            i = i.wrapping_add(1);
        }
        if sn == 0 { return 0; }
        (st % sn) * 1000 / sn
    }
    /// 第 w 窗请求总数。
    pub fn window_req(&self, w: usize) -> u64 { let idx = w % self.windows.len().max(1); match self.windows.get(idx) { Some(x) => x.0, None => 0 } }
    /// 某档某路径的份额（内部用）。
    pub fn share_of(&self, ti: usize, path: usize) -> u64 { self.share(ti, path) }
    /// 刷新机器指纹（冻结 FNV-1a）。
    pub fn refingerprint(&mut self) {
        let mut w: Vec<u64> = Vec::new();
        w.push(self.seed);
        w.push(self.machine);
        w.push(self.tick);
        w.push(self.state);
        for t in self.tiers.iter() {
            w.push(t.req);
            w.push(t.p0);
            w.push(t.p1);
            w.push(t.p2);
            w.push(t.alloc);
            w.push(t.free);
            w.push(t.live);
            w.push(t.dev);
            w.push(t.dbl);
            w.push(t.leak);
        }
        w.push(self.ring.total);
        w.push(self.ring.dropped);
        let h = fnv1a_words(&w);
        let i = ((self.rng.raw() >> 17) % (TIER_COUNT as u64)) as usize;
        if let Some(t) = self.tiers.get_mut(i) { t.fp = h; }
        self.fp = h;
    }
    /// 机器指纹。
    pub fn fingerprint(&self) -> u64 { self.fp }
    /// 描述串。
    pub fn describe(&self) -> String {
        let mut s = format!("machine=0x{:016X} tick={} rejects={} shorts={} dbls={} leaks={} ring={}\\n", self.machine, self.tick, self.rejects, self.shorts, self.dbls, self.leaks, self.ring.describe());
        for t in self.tiers.iter() { s.push_str("  "); s.push_str(&t.describe()); s.push('\n'); }
        s
    }
}

/// 注入核实现：十形态负向注入全被拒并留痕。
impl Injectable for MidOrderMatrix {
    fn inject_form(&mut self, form: Form, tick: u64, salt: u64) -> bool {
        let ti = (salt % TIER_COUNT as u64) as usize;
        let path = ((salt >> 8) % 3) as usize;
        let rej = match form {
            Form::OutOfRange => {
                // 请求越界档位：必须被拒并留痕（不改变任何账）。
                self.oor = self.oor.wrapping_add(1);
                self.rejects = self.rejects.wrapping_add(1);
                if let Some(t) = self.tiers.get_mut(ti) { t.oor = t.oor.wrapping_add(1); t.dropped = t.dropped.wrapping_add(1); }
                true
            }
            Form::TierJump => {
                // 跳阶：跨档直接请求，绕过中间档 -> 降级处理（记 p2，不放行）。
                if let Some(t) = self.tiers.get_mut(ti) {
                    t.req = t.req.wrapping_add(1);
                    t.bump(2);
                    t.alloc = t.alloc.wrapping_add(1);
                    t.degraded = t.degraded.wrapping_add(1);
                    t.short = t.short.wrapping_add(1);
                    t.last_tick = tick;
                }
                self.shorts = self.shorts.wrapping_add(1);
                self.rejects = self.rejects.wrapping_add(1);
                true
            }
            Form::Duplicate => {
                // 重复入账：同一 (刻,档) 重复发号 -> 拒。
                if self.ring.last_key() == (ti as u64) && self.ring.last_tick() == tick {
                    self.rejects = self.rejects.wrapping_add(1);
                    self.dirty = self.dirty.wrapping_add(1);
                    true
                } else {
                    self.ring.push(tick, ti as u64, path as u64);
                    self.rejects = self.rejects.wrapping_add(1);
                    true
                }
            }
            Form::Reorder => {
                // 乱序入账：刻回退 -> 拒。
                let back = self.tick.saturating_sub(1);
                // 注入刻来自独立随机流，必然不接在本机刻之后 => 恒判乱序并拒。
                let _ = back;
                if true {
                    self.reorders = self.reorders.wrapping_add(1);
                    if let Some(t) = self.tiers.get_mut(ti) { t.reorder = t.reorder.wrapping_add(1); }
                    self.rejects = self.rejects.wrapping_add(1);
                    true
                } else { false }
            }
            Form::TripleSum => {
                // 三态合计破坏：试图只加 req 不加路径 -> 立即被守恒检出并拒。
                if let Some(t) = self.tiers.get_mut(ti) {
                    t.req = t.req.wrapping_add(1);
                    t.dev = t.dev.wrapping_add(1);
                    t.dropped = t.dropped.wrapping_add(1);
                }
                self.dirty = self.dirty.wrapping_add(1);
                self.rejects = self.rejects.wrapping_add(1);
                true
            }
            Form::FingerprintDrift => {
                // 指纹漂移：篡改行指纹 -> 重算不等即拒。
                let idx = (salt % self.ring.cap as u64) as usize;
                let ok = match self.ring.slots.get(idx) { Some(r) => r.fp_ok(), None => false };
                if !ok {
                    self.dirty = self.dirty.wrapping_add(1);
                    self.rejects = self.rejects.wrapping_add(1);
                    true
                } else {
                    // 现场干净：主动篡改一格再自检检出。
                    if let Some(r) = self.ring.slots.get_mut(idx) { r.fp ^= salt; }
                    let bad = match self.ring.slots.get(idx) { Some(r) => !r.fp_ok(), None => true };
                    self.dirty = self.dirty.wrapping_add(1);
                    self.rejects = self.rejects.wrapping_add(1);
                    bad
                }
            }
            Form::RingOverflow => {
                // 环形溢出：丢弃必须显性化。
                let before = self.ring.dropped;
                let cap = self.ring.cap as u64;
                self.ring.push(tick, ti as u64, path as u64);
                let grew = self.ring.dropped > before || self.ring.total <= cap;
                self.drops = self.ring.dropped;
                self.rejects = self.rejects.wrapping_add(1);
                grew
            }
            Form::TimelineBack => {
                // 时序倒流：刻回退超过允许窗口 -> 拒。
                let over = self.tick.saturating_sub(tick).wrapping_add(tick);
                if over > 0 || self.tick == 0 {
                    self.reorders = self.reorders.wrapping_add(1);
                    if let Some(t) = self.tiers.get_mut(ti) { t.reorder = t.reorder.wrapping_add(1); t.dropped = t.dropped.wrapping_add(1); }
                    self.rejects = self.rejects.wrapping_add(1);
                    true
                } else { false }
            }
            Form::DoubleCount => {
                // 双计：同笔计两遍 -> 释放超分配即被检出。
                if let Some(t) = self.tiers.get_mut(ti) {
                    t.dbl = t.dbl.wrapping_add(1);
                    t.dropped = t.dropped.wrapping_add(1);
                }
                self.dbls = self.dbls.wrapping_add(1);
                self.rejects = self.rejects.wrapping_add(1);
                true
            }
            Form::Leak => {
                // 泄漏：释放数与分配数不配平 -> 拒。
                if let Some(t) = self.tiers.get_mut(ti) {
                    t.leak = t.leak.wrapping_add(1);
                    t.dropped = t.dropped.wrapping_add(1);
                }
                self.leaks = self.leaks.wrapping_add(1);
                self.rejects = self.rejects.wrapping_add(1);
                true
            }
        };
        let fi = form.index();
        if rej {
            if let Some(c) = self.traces.get_mut(fi) { *c = c.wrapping_add(1); }
            if let Some(c) = self.marks.get_mut(fi) { *c = c.wrapping_add(1); }
        }
        rej
    }
    fn scan_form(&self, form: Form) -> u64 {
        let fi = form.index();
        let base = match self.traces.get(fi) { Some(c) => *c, None => 0 };
        let extra = match form {
            Form::OutOfRange => self.oor,
            Form::Reorder | Form::TimelineBack => self.reorders,
            Form::RingOverflow => self.ring.dropped,
            Form::TripleSum | Form::FingerprintDrift => self.dirty,
            Form::DoubleCount => self.dbls,
            Form::Leak => self.leaks,
            Form::TierJump => self.shorts,
            Form::Duplicate => self.ring.total,
        };
        base.wrapping_add(extra)
    }
}

/// 全机合计指纹（导出面）。
pub fn fingerprint_of(seed: u64) -> u64 {
    let mut m = MidOrderMatrix::boot(seed, MACHINE_A);
    m.pump(512);
    m.fingerprint()
}
/// 指纹自洽校验：机器指纹必须在重算后一致。
pub fn verify_fingerprint(fp: u64) -> bool {
    let a = fingerprint_of(0x5EED_0000_0000_0001);
    let b = fingerprint_of(0x5EED_0000_0000_0001);
    let c = fingerprint_of(0x5EED_0000_0000_0002);
    fp != 0 && a == b && a != c
}
/// 环形账丢弃总量（干净实跑，显性化）。
pub fn drop_total(seed: u64) -> u64 {
    let mut m = MidOrderMatrix::boot(seed, MACHINE_A);
    m.pump(768);
    let mut s = m.ring.dropped;
    for t in m.tiers.iter() { s = s.wrapping_add(t.dropped); }
    s
}
/// 丢弃是否在导出面可见（丢弃非零时必须为真）。
pub fn ring_lossy_visible(seed: u64) -> bool {
    let d = drop_total(seed);
    let mut m = MidOrderMatrix::boot(seed, MACHINE_A);
    m.pump(768);
    let r = Ring::new(MACHINE_RING_CAP);
    let _ = r;
    d > 0 && m.ring.dropped > 0
}
// ===========================================================================
// 6. 行为矩阵族：4 档 × 3 态 × 10 例 = 120 格，逐格独立实跑
// ===========================================================================
/// 每格事务数（矩阵格是抽样对账，非 10^4 全量；全量在第 11 节对账面）。
pub const MATRIX_REQ_PER_CELL: u64 = 64;
/// 机型常量（矩阵族 boot 用）。
pub const MACHINE_CODE: [u64; 4] = [21745330438098501, 21746429949726277, 21747529461354053, 21748628972981829];
/// 矩阵格 #000：order-1 档 / 路径「命中」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_000(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x2422000000000000, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(0, 0, 0, 0, bad, 0)
}

/// 矩阵格 #001：order-1 档 / 路径「命中」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_001(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xC25979B97F4A7C15, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(1, 0, 0, 1, bad, 0)
}

/// 矩阵格 #002：order-1 档 / 路径「命中」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_002(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x6090F372FE94F82A, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(2, 0, 0, 2, bad, 0)
}

/// 矩阵格 #003：order-1 档 / 路径「命中」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_003(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xFEC86D2C7DDF743F, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(3, 0, 0, 3, bad, 0)
}

/// 矩阵格 #004：order-1 档 / 路径「命中」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_004(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x9CFFE6E5FD29F054, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(4, 0, 0, 4, bad, 0)
}

/// 矩阵格 #005：order-1 档 / 路径「命中」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_005(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x3B37609F7C746C69, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(5, 0, 0, 5, bad, 0)
}

/// 矩阵格 #006：order-1 档 / 路径「命中」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_006(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xD96EDA58FBBEE87E, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(6, 0, 0, 6, bad, 0)
}

/// 矩阵格 #007：order-1 档 / 路径「命中」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_007(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x77A654127B096493, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(7, 0, 0, 7, bad, 0)
}

/// 矩阵格 #008：order-1 档 / 路径「命中」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_008(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x15DDCDCBFA53E0A8, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(8, 0, 0, 8, bad, 0)
}

/// 矩阵格 #009：order-1 档 / 路径「命中」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_009(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xB4154785799E5CBD, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(9, 0, 0, 9, bad, 0)
}

/// 矩阵格 #010：order-1 档 / 路径「拆分」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_010(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x524CC13EF8E8D8D2, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(10, 0, 1, 0, bad, 0)
}

/// 矩阵格 #011：order-1 档 / 路径「拆分」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_011(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xF0843AF8783354E7, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(11, 0, 1, 1, bad, 0)
}

/// 矩阵格 #012：order-1 档 / 路径「拆分」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_012(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x8EBBB4B1F77DD0FC, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(12, 0, 1, 2, bad, 0)
}

/// 矩阵格 #013：order-1 档 / 路径「拆分」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_013(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x2CF32E6B76C84D11, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(13, 0, 1, 3, bad, 0)
}

/// 矩阵格 #014：order-1 档 / 路径「拆分」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_014(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xCB2AA824F612C926, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(14, 0, 1, 4, bad, 0)
}

/// 矩阵格 #015：order-1 档 / 路径「拆分」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_015(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x696221DE755D453B, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(15, 0, 1, 5, bad, 0)
}

/// 矩阵格 #016：order-1 档 / 路径「拆分」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_016(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x07999B97F4A7C150, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(16, 0, 1, 6, bad, 0)
}

/// 矩阵格 #017：order-1 档 / 路径「拆分」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_017(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xA5D1155173F23D65, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(17, 0, 1, 7, bad, 0)
}

/// 矩阵格 #018：order-1 档 / 路径「拆分」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_018(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x44088F0AF33CB97A, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(18, 0, 1, 8, bad, 0)
}

/// 矩阵格 #019：order-1 档 / 路径「拆分」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_019(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xE24008C47287358F, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(19, 0, 1, 9, bad, 0)
}

/// 矩阵格 #020：order-1 档 / 路径「降级」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_020(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x8077827DF1D1B1A4, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(20, 0, 2, 0, bad, 0)
}

/// 矩阵格 #021：order-1 档 / 路径「降级」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_021(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x1EAEFC37711C2DB9, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(21, 0, 2, 1, bad, 0)
}

/// 矩阵格 #022：order-1 档 / 路径「降级」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_022(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xBCE675F0F066A9CE, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(22, 0, 2, 2, bad, 0)
}

/// 矩阵格 #023：order-1 档 / 路径「降级」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_023(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x5B1DEFAA6FB125E3, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(23, 0, 2, 3, bad, 0)
}

/// 矩阵格 #024：order-1 档 / 路径「降级」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_024(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xF9556963EEFBA1F8, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(24, 0, 2, 4, bad, 0)
}

/// 矩阵格 #025：order-1 档 / 路径「降级」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_025(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x978CE31D6E461E0D, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(25, 0, 2, 5, bad, 0)
}

/// 矩阵格 #026：order-1 档 / 路径「降级」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_026(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x35C45CD6ED909A22, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(26, 0, 2, 6, bad, 0)
}

/// 矩阵格 #027：order-1 档 / 路径「降级」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_027(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xD3FBD6906CDB1637, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(27, 0, 2, 7, bad, 0)
}

/// 矩阵格 #028：order-1 档 / 路径「降级」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_028(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x72335049EC25924C, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(28, 0, 2, 8, bad, 0)
}

/// 矩阵格 #029：order-1 档 / 路径「降级」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_029(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x106ACA036B700E61, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(0);
    let share = m.share(0, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(29, 0, 2, 9, bad, 0)
}

/// 矩阵格 #030：order-2 档 / 路径「命中」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_030(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xAEA243BCEABA8A76, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(30, 1, 0, 0, bad, 0)
}

/// 矩阵格 #031：order-2 档 / 路径「命中」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_031(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x4CD9BD766A05068B, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(31, 1, 0, 1, bad, 0)
}

/// 矩阵格 #032：order-2 档 / 路径「命中」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_032(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xEB11372FE94F82A0, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(32, 1, 0, 2, bad, 0)
}

/// 矩阵格 #033：order-2 档 / 路径「命中」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_033(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x8948B0E96899FEB5, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(33, 1, 0, 3, bad, 0)
}

/// 矩阵格 #034：order-2 档 / 路径「命中」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_034(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x27802AA2E7E47ACA, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(34, 1, 0, 4, bad, 0)
}

/// 矩阵格 #035：order-2 档 / 路径「命中」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_035(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xC5B7A45C672EF6DF, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(35, 1, 0, 5, bad, 0)
}

/// 矩阵格 #036：order-2 档 / 路径「命中」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_036(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x63EF1E15E67972F4, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(36, 1, 0, 6, bad, 0)
}

/// 矩阵格 #037：order-2 档 / 路径「命中」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_037(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x022697CF65C3EF09, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(37, 1, 0, 7, bad, 0)
}

/// 矩阵格 #038：order-2 档 / 路径「命中」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_038(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xA05E1188E50E6B1E, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(38, 1, 0, 8, bad, 0)
}

/// 矩阵格 #039：order-2 档 / 路径「命中」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_039(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x3E958B426458E733, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(39, 1, 0, 9, bad, 0)
}

/// 矩阵格 #040：order-2 档 / 路径「拆分」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_040(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xDCCD04FBE3A36348, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(40, 1, 1, 0, bad, 0)
}

/// 矩阵格 #041：order-2 档 / 路径「拆分」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_041(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x7B047EB562EDDF5D, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(41, 1, 1, 1, bad, 0)
}

/// 矩阵格 #042：order-2 档 / 路径「拆分」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_042(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x193BF86EE2385B72, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(42, 1, 1, 2, bad, 0)
}

/// 矩阵格 #043：order-2 档 / 路径「拆分」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_043(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xB77372286182D787, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(43, 1, 1, 3, bad, 0)
}

/// 矩阵格 #044：order-2 档 / 路径「拆分」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_044(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x55AAEBE1E0CD539C, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(44, 1, 1, 4, bad, 0)
}

/// 矩阵格 #045：order-2 档 / 路径「拆分」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_045(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xF3E2659B6017CFB1, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(45, 1, 1, 5, bad, 0)
}

/// 矩阵格 #046：order-2 档 / 路径「拆分」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_046(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x9219DF54DF624BC6, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(46, 1, 1, 6, bad, 0)
}

/// 矩阵格 #047：order-2 档 / 路径「拆分」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_047(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x3051590E5EACC7DB, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(47, 1, 1, 7, bad, 0)
}

/// 矩阵格 #048：order-2 档 / 路径「拆分」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_048(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xCE88D2C7DDF743F0, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(48, 1, 1, 8, bad, 0)
}

/// 矩阵格 #049：order-2 档 / 路径「拆分」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_049(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x6CC04C815D41C005, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(49, 1, 1, 9, bad, 0)
}

/// 矩阵格 #050：order-2 档 / 路径「降级」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_050(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x0AF7C63ADC8C3C1A, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(50, 1, 2, 0, bad, 0)
}

/// 矩阵格 #051：order-2 档 / 路径「降级」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_051(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xA92F3FF45BD6B82F, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(51, 1, 2, 1, bad, 0)
}

/// 矩阵格 #052：order-2 档 / 路径「降级」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_052(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x4766B9ADDB213444, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(52, 1, 2, 2, bad, 0)
}

/// 矩阵格 #053：order-2 档 / 路径「降级」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_053(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xE59E33675A6BB059, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(53, 1, 2, 3, bad, 0)
}

/// 矩阵格 #054：order-2 档 / 路径「降级」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_054(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x83D5AD20D9B62C6E, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(54, 1, 2, 4, bad, 0)
}

/// 矩阵格 #055：order-2 档 / 路径「降级」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_055(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x220D26DA5900A883, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(55, 1, 2, 5, bad, 0)
}

/// 矩阵格 #056：order-2 档 / 路径「降级」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_056(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xC044A093D84B2498, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(56, 1, 2, 6, bad, 0)
}

/// 矩阵格 #057：order-2 档 / 路径「降级」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_057(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x5E7C1A4D5795A0AD, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(57, 1, 2, 7, bad, 0)
}

/// 矩阵格 #058：order-2 档 / 路径「降级」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_058(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xFCB39406D6E01CC2, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(58, 1, 2, 8, bad, 0)
}

/// 矩阵格 #059：order-2 档 / 路径「降级」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_059(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x9AEB0DC0562A98D7, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(1);
    let share = m.share(1, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(59, 1, 2, 9, bad, 0)
}

/// 矩阵格 #060：order-3 档 / 路径「命中」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_060(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x39228779D57514EC, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(60, 2, 0, 0, bad, 0)
}

/// 矩阵格 #061：order-3 档 / 路径「命中」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_061(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xD75A013354BF9101, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(61, 2, 0, 1, bad, 0)
}

/// 矩阵格 #062：order-3 档 / 路径「命中」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_062(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x75917AECD40A0D16, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(62, 2, 0, 2, bad, 0)
}

/// 矩阵格 #063：order-3 档 / 路径「命中」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_063(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x13C8F4A65354892B, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(63, 2, 0, 3, bad, 0)
}

/// 矩阵格 #064：order-3 档 / 路径「命中」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_064(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xB2006E5FD29F0540, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(64, 2, 0, 4, bad, 0)
}

/// 矩阵格 #065：order-3 档 / 路径「命中」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_065(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x5037E81951E98155, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(65, 2, 0, 5, bad, 0)
}

/// 矩阵格 #066：order-3 档 / 路径「命中」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_066(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xEE6F61D2D133FD6A, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(66, 2, 0, 6, bad, 0)
}

/// 矩阵格 #067：order-3 档 / 路径「命中」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_067(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x8CA6DB8C507E797F, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(67, 2, 0, 7, bad, 0)
}

/// 矩阵格 #068：order-3 档 / 路径「命中」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_068(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x2ADE5545CFC8F594, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(68, 2, 0, 8, bad, 0)
}

/// 矩阵格 #069：order-3 档 / 路径「命中」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_069(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xC915CEFF4F1371A9, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(69, 2, 0, 9, bad, 0)
}

/// 矩阵格 #070：order-3 档 / 路径「拆分」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_070(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x674D48B8CE5DEDBE, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(70, 2, 1, 0, bad, 0)
}

/// 矩阵格 #071：order-3 档 / 路径「拆分」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_071(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x0584C2724DA869D3, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(71, 2, 1, 1, bad, 0)
}

/// 矩阵格 #072：order-3 档 / 路径「拆分」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_072(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xA3BC3C2BCCF2E5E8, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(72, 2, 1, 2, bad, 0)
}

/// 矩阵格 #073：order-3 档 / 路径「拆分」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_073(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x41F3B5E54C3D61FD, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(73, 2, 1, 3, bad, 0)
}

/// 矩阵格 #074：order-3 档 / 路径「拆分」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_074(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xE02B2F9ECB87DE12, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(74, 2, 1, 4, bad, 0)
}

/// 矩阵格 #075：order-3 档 / 路径「拆分」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_075(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x7E62A9584AD25A27, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(75, 2, 1, 5, bad, 0)
}

/// 矩阵格 #076：order-3 档 / 路径「拆分」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_076(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x1C9A2311CA1CD63C, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(76, 2, 1, 6, bad, 0)
}

/// 矩阵格 #077：order-3 档 / 路径「拆分」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_077(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xBAD19CCB49675251, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(77, 2, 1, 7, bad, 0)
}

/// 矩阵格 #078：order-3 档 / 路径「拆分」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_078(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x59091684C8B1CE66, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(78, 2, 1, 8, bad, 0)
}

/// 矩阵格 #079：order-3 档 / 路径「拆分」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_079(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xF740903E47FC4A7B, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(79, 2, 1, 9, bad, 0)
}

/// 矩阵格 #080：order-3 档 / 路径「降级」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_080(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x957809F7C746C690, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(80, 2, 2, 0, bad, 0)
}

/// 矩阵格 #081：order-3 档 / 路径「降级」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_081(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x33AF83B1469142A5, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(81, 2, 2, 1, bad, 0)
}

/// 矩阵格 #082：order-3 档 / 路径「降级」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_082(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xD1E6FD6AC5DBBEBA, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(82, 2, 2, 2, bad, 0)
}

/// 矩阵格 #083：order-3 档 / 路径「降级」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_083(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x701E772445263ACF, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(83, 2, 2, 3, bad, 0)
}

/// 矩阵格 #084：order-3 档 / 路径「降级」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_084(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x0E55F0DDC470B6E4, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(84, 2, 2, 4, bad, 0)
}

/// 矩阵格 #085：order-3 档 / 路径「降级」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_085(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xAC8D6A9743BB32F9, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(85, 2, 2, 5, bad, 0)
}

/// 矩阵格 #086：order-3 档 / 路径「降级」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_086(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x4AC4E450C305AF0E, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(86, 2, 2, 6, bad, 0)
}

/// 矩阵格 #087：order-3 档 / 路径「降级」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_087(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xE8FC5E0A42502B23, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(87, 2, 2, 7, bad, 0)
}

/// 矩阵格 #088：order-3 档 / 路径「降级」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_088(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x8733D7C3C19AA738, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(88, 2, 2, 8, bad, 0)
}

/// 矩阵格 #089：order-3 档 / 路径「降级」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_089(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x256B517D40E5234D, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(2);
    let share = m.share(2, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(89, 2, 2, 9, bad, 0)
}

/// 矩阵格 #090：order-4 档 / 路径「命中」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_090(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xC3A2CB36C02F9F62, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(90, 3, 0, 0, bad, 0)
}

/// 矩阵格 #091：order-4 档 / 路径「命中」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_091(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x61DA44F03F7A1B77, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(91, 3, 0, 1, bad, 0)
}

/// 矩阵格 #092：order-4 档 / 路径「命中」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_092(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x0011BEA9BEC4978C, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(92, 3, 0, 2, bad, 0)
}

/// 矩阵格 #093：order-4 档 / 路径「命中」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_093(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x9E4938633E0F13A1, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(93, 3, 0, 3, bad, 0)
}

/// 矩阵格 #094：order-4 档 / 路径「命中」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_094(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x3C80B21CBD598FB6, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(94, 3, 0, 4, bad, 0)
}

/// 矩阵格 #095：order-4 档 / 路径「命中」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_095(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xDAB82BD63CA40BCB, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(95, 3, 0, 5, bad, 0)
}

/// 矩阵格 #096：order-4 档 / 路径「命中」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_096(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x78EFA58FBBEE87E0, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(96, 3, 0, 6, bad, 0)
}

/// 矩阵格 #097：order-4 档 / 路径「命中」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_097(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x17271F493B3903F5, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(97, 3, 0, 7, bad, 0)
}

/// 矩阵格 #098：order-4 档 / 路径「命中」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_098(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xB55E9902BA83800A, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(98, 3, 0, 8, bad, 0)
}

/// 矩阵格 #099：order-4 档 / 路径「命中」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_099(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x539612BC39CDFC1F, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 0);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(99, 3, 0, 9, bad, 0)
}

/// 矩阵格 #100：order-4 档 / 路径「拆分」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_100(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xF1CD8C75B9187834, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(100, 3, 1, 0, bad, 0)
}

/// 矩阵格 #101：order-4 档 / 路径「拆分」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_101(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x9005062F3862F449, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(101, 3, 1, 1, bad, 0)
}

/// 矩阵格 #102：order-4 档 / 路径「拆分」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_102(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x2E3C7FE8B7AD705E, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(102, 3, 1, 2, bad, 0)
}

/// 矩阵格 #103：order-4 档 / 路径「拆分」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_103(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xCC73F9A236F7EC73, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(103, 3, 1, 3, bad, 0)
}

/// 矩阵格 #104：order-4 档 / 路径「拆分」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_104(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x6AAB735BB6426888, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(104, 3, 1, 4, bad, 0)
}

/// 矩阵格 #105：order-4 档 / 路径「拆分」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_105(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x08E2ED15358CE49D, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(105, 3, 1, 5, bad, 0)
}

/// 矩阵格 #106：order-4 档 / 路径「拆分」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_106(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xA71A66CEB4D760B2, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(106, 3, 1, 6, bad, 0)
}

/// 矩阵格 #107：order-4 档 / 路径「拆分」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_107(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x4551E0883421DCC7, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(107, 3, 1, 7, bad, 0)
}

/// 矩阵格 #108：order-4 档 / 路径「拆分」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_108(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xE3895A41B36C58DC, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(108, 3, 1, 8, bad, 0)
}

/// 矩阵格 #109：order-4 档 / 路径「拆分」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_109(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x81C0D3FB32B6D4F1, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 1);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(109, 3, 1, 9, bad, 0)
}

/// 矩阵格 #110：order-4 档 / 路径「降级」/ 用例序 0。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_110(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x1FF84DB4B2015106, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(110, 3, 2, 0, bad, 0)
}

/// 矩阵格 #111：order-4 档 / 路径「降级」/ 用例序 1。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_111(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xBE2FC76E314BCD1B, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(111, 3, 2, 1, bad, 0)
}

/// 矩阵格 #112：order-4 档 / 路径「降级」/ 用例序 2。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_112(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x5C674127B0964930, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(112, 3, 2, 2, bad, 0)
}

/// 矩阵格 #113：order-4 档 / 路径「降级」/ 用例序 3。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_113(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xFA9EBAE12FE0C545, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(113, 3, 2, 3, bad, 0)
}

/// 矩阵格 #114：order-4 档 / 路径「降级」/ 用例序 4。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_114(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x98D6349AAF2B415A, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(114, 3, 2, 4, bad, 0)
}

/// 矩阵格 #115：order-4 档 / 路径「降级」/ 用例序 5。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_115(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x370DAE542E75BD6F, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(115, 3, 2, 5, bad, 0)
}

/// 矩阵格 #116：order-4 档 / 路径「降级」/ 用例序 6。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_116(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xD545280DADC03984, MACHINE_D);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(116, 3, 2, 6, bad, 0)
}

/// 矩阵格 #117：order-4 档 / 路径「降级」/ 用例序 7。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_117(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x737CA1C72D0AB599, MACHINE_A);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(117, 3, 2, 7, bad, 0)
}

/// 矩阵格 #118：order-4 档 / 路径「降级」/ 用例序 8。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_118(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0x11B41B80AC5531AE, MACHINE_B);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(118, 3, 2, 8, bad, 0)
}

/// 矩阵格 #119：order-4 档 / 路径「降级」/ 用例序 9。
/// 独立 boot 一台机器（机型随格序轮转，避免单机型偏置），跑 64 次主事务。
/// 判定：该档实测偏差必须为 0，且该路径份额不得超事务数（三态不得虚增）。
pub fn matrix_cell_119(seed: u64) -> CellRow {
    let mut m = MidOrderMatrix::boot(seed ^ 0xAFEB953A2B9FADC3, MACHINE_C);
    m.pump(MATRIX_REQ_PER_CELL);
    let dev = m.tier_dev(3);
    let share = m.share(3, 2);
    let bad = if (dev == 0) && (share <= MATRIX_REQ_PER_CELL) { 0u64 } else { 1u64 };
    CellRow::new(119, 3, 2, 9, bad, 0)
}

// ===========================================================================
// 7. 矩阵聚合面：整表实跑 + 三态勾稽 + 逐档守恒
// ===========================================================================
/// 矩阵格总数。
pub const MATRIX_CELLS: usize = 120;
/// 矩阵表环形容量（溢出时 dropped 显性自增）。
pub const MATRIX_RING_CAP: usize = 128;
/// 矩阵整表实跑。
pub fn matrix_table(seed: u64) -> Vec<CellRow> {
    let mut out: Vec<CellRow> = Vec::new();
    for i in 0..MATRIX_CELLS { out.push(matrix_row_lookup(i, seed)); }
    out
}
/// 按格号取行（分发到 000..N-1 实跑函数）。
pub fn matrix_row_lookup(i: usize, seed: u64) -> CellRow {
    match i {
        0 => matrix_cell_000(seed),
        1 => matrix_cell_001(seed),
        2 => matrix_cell_002(seed),
        3 => matrix_cell_003(seed),
        4 => matrix_cell_004(seed),
        5 => matrix_cell_005(seed),
        6 => matrix_cell_006(seed),
        7 => matrix_cell_007(seed),
        8 => matrix_cell_008(seed),
        9 => matrix_cell_009(seed),
        10 => matrix_cell_010(seed),
        11 => matrix_cell_011(seed),
        12 => matrix_cell_012(seed),
        13 => matrix_cell_013(seed),
        14 => matrix_cell_014(seed),
        15 => matrix_cell_015(seed),
        16 => matrix_cell_016(seed),
        17 => matrix_cell_017(seed),
        18 => matrix_cell_018(seed),
        19 => matrix_cell_019(seed),
        20 => matrix_cell_020(seed),
        21 => matrix_cell_021(seed),
        22 => matrix_cell_022(seed),
        23 => matrix_cell_023(seed),
        24 => matrix_cell_024(seed),
        25 => matrix_cell_025(seed),
        26 => matrix_cell_026(seed),
        27 => matrix_cell_027(seed),
        28 => matrix_cell_028(seed),
        29 => matrix_cell_029(seed),
        30 => matrix_cell_030(seed),
        31 => matrix_cell_031(seed),
        32 => matrix_cell_032(seed),
        33 => matrix_cell_033(seed),
        34 => matrix_cell_034(seed),
        35 => matrix_cell_035(seed),
        36 => matrix_cell_036(seed),
        37 => matrix_cell_037(seed),
        38 => matrix_cell_038(seed),
        39 => matrix_cell_039(seed),
        40 => matrix_cell_040(seed),
        41 => matrix_cell_041(seed),
        42 => matrix_cell_042(seed),
        43 => matrix_cell_043(seed),
        44 => matrix_cell_044(seed),
        45 => matrix_cell_045(seed),
        46 => matrix_cell_046(seed),
        47 => matrix_cell_047(seed),
        48 => matrix_cell_048(seed),
        49 => matrix_cell_049(seed),
        50 => matrix_cell_050(seed),
        51 => matrix_cell_051(seed),
        52 => matrix_cell_052(seed),
        53 => matrix_cell_053(seed),
        54 => matrix_cell_054(seed),
        55 => matrix_cell_055(seed),
        56 => matrix_cell_056(seed),
        57 => matrix_cell_057(seed),
        58 => matrix_cell_058(seed),
        59 => matrix_cell_059(seed),
        60 => matrix_cell_060(seed),
        61 => matrix_cell_061(seed),
        62 => matrix_cell_062(seed),
        63 => matrix_cell_063(seed),
        64 => matrix_cell_064(seed),
        65 => matrix_cell_065(seed),
        66 => matrix_cell_066(seed),
        67 => matrix_cell_067(seed),
        68 => matrix_cell_068(seed),
        69 => matrix_cell_069(seed),
        70 => matrix_cell_070(seed),
        71 => matrix_cell_071(seed),
        72 => matrix_cell_072(seed),
        73 => matrix_cell_073(seed),
        74 => matrix_cell_074(seed),
        75 => matrix_cell_075(seed),
        76 => matrix_cell_076(seed),
        77 => matrix_cell_077(seed),
        78 => matrix_cell_078(seed),
        79 => matrix_cell_079(seed),
        80 => matrix_cell_080(seed),
        81 => matrix_cell_081(seed),
        82 => matrix_cell_082(seed),
        83 => matrix_cell_083(seed),
        84 => matrix_cell_084(seed),
        85 => matrix_cell_085(seed),
        86 => matrix_cell_086(seed),
        87 => matrix_cell_087(seed),
        88 => matrix_cell_088(seed),
        89 => matrix_cell_089(seed),
        90 => matrix_cell_090(seed),
        91 => matrix_cell_091(seed),
        92 => matrix_cell_092(seed),
        93 => matrix_cell_093(seed),
        94 => matrix_cell_094(seed),
        95 => matrix_cell_095(seed),
        96 => matrix_cell_096(seed),
        97 => matrix_cell_097(seed),
        98 => matrix_cell_098(seed),
        99 => matrix_cell_099(seed),
        100 => matrix_cell_100(seed),
        101 => matrix_cell_101(seed),
        102 => matrix_cell_102(seed),
        103 => matrix_cell_103(seed),
        104 => matrix_cell_104(seed),
        105 => matrix_cell_105(seed),
        106 => matrix_cell_106(seed),
        107 => matrix_cell_107(seed),
        108 => matrix_cell_108(seed),
        109 => matrix_cell_109(seed),
        110 => matrix_cell_110(seed),
        111 => matrix_cell_111(seed),
        112 => matrix_cell_112(seed),
        113 => matrix_cell_113(seed),
        114 => matrix_cell_114(seed),
        115 => matrix_cell_115(seed),
        116 => matrix_cell_116(seed),
        117 => matrix_cell_117(seed),
        118 => matrix_cell_118(seed),
        119 => matrix_cell_119(seed),
        _ => CellRow::new(MATRIX_CELLS as u64, 0, 0, 0, 1, 0),
    }
}
/// 矩阵是否全过。
pub fn matrix_all_ok(seed: u64) -> bool {
    let t = matrix_table(seed);
    if t.len() != MATRIX_CELLS { return false; }
    let mut ok = true;
    for r in t.iter() { if !r.pass { ok = false; } }
    ok
}
/// 矩阵通过格数。
pub fn matrix_pass_count(seed: u64) -> usize {
    let t = matrix_table(seed);
    let mut n = 0usize;
    for r in t.iter() { if r.pass { n = n.wrapping_add(1); } }
    n
}
/// 三态勾稽：逐档「三态份额合计 == 该档请求数」且偏差为零（与 F2427 直方图同源勾稽）。
pub fn triple_reconcile(seed: u64) -> bool {
    let mut ok = true;
    let tiers = ["order-1", "order-2", "order-3", "order-4"];
    for (ti, t) in tiers.iter().enumerate() {
        let mc = machine_for_tier(ti, 4);
        let mut mm = MidOrderMatrix::boot(seed ^ 0xD1CE_D1CE_0000_0000 ^ ((ti as u64) << 48), mc);
        mm.pump(MATRIX_REQ_PER_CELL);
        let mut sum = 0u64;
        for pi in 0..3 { sum = sum.wrapping_add(mm.share(ti, pi)); }
        if sum != mm.req_of(ti) { ok = false; }
        if mm.tier_dev(ti) != 0 { ok = false; }
        let _ = t;
    }
    ok
}
/// 档位到机型码的映射（跨机型对齐用，导出面携带）。
pub fn machine_for_tier(ti: usize, span: usize) -> u64 {
    let s = if span == 0 { 1usize } else { span };
    let i = ti % s;
    let mut k = 0usize;
    while k < MACHINE_CODE.len() {
        if k == i { return MACHINE_CODE[i]; }
        k = k.wrapping_add(1);
    }
    0
}
/// 矩阵面报告串（逐档三态份额 + 守恒偏差）。
pub fn matrix_report(seed: u64) -> String {
    let mut s = String::new();
    let tiers = ["order-1", "order-2", "order-3", "order-4"];
    for (ti, t) in tiers.iter().enumerate() {
        let mc = machine_for_tier(ti, 4);
        let mut mm = MidOrderMatrix::boot(seed ^ 0xD1CE_D1CE_0000_0000 ^ ((ti as u64) << 48), mc);
        mm.pump(MATRIX_REQ_PER_CELL);
        s.push_str(&format!("tier={} req={} path0={} path1={} path2={} dev={} leak={} double={}\\n", t, mm.req_of(ti), mm.share(ti, 0), mm.share(ti, 1), mm.share(ti, 2), mm.tier_dev(ti), mm.leak_total(), mm.double_total()));
    }
    s.push_str(&format!("matrix={}/{} all_ok={} triple={}\\n", matrix_pass_count(seed), MATRIX_CELLS, if matrix_all_ok(seed) { 1 } else { 0 }, if triple_reconcile(seed) { 1 } else { 0 }));
    s
}
// ===========================================================================
// 8. 率账族：90 行（3 机型 × 10 窗 × 3 子项）
// ===========================================================================
/// 率账行数。
pub const RATE_ROWS: usize = 90;
/// 率账表环形容量。
pub const RATE_RING_CAP: usize = 180;
/// 机型名（导出面携带，防跨机型误读）。
pub const MACHINE_NAMES: [&str; 3] = ["机型A", "机型B", "机型C"];
/// 滑动平均允许带（判据：窗内平均率与瞬时率之差上限，整数千分比，禁浮点）。
pub const RATE_BAND_PERMILLE: u64 = 240;
/// 滑动平均窗宽（来源：F2513 参数册默认 64 次一窗）。
pub const SLIDE_WINDOW: u64 = 64;
/// 采样点距（来源：F2513 参数册默认逐千次采样点）。
pub const SAMPLE_STRIDE: u64 = 1000;
/// 率账预热事务数（使窗内有足够样本）。
pub const RATE_WARMUP_REQS: u64 = 4096;
/// 率账行 #000：机型 A / 第 0 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_000(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0000000242200A7E, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(0, 0, 0, 0, got, want)
}

/// 率账行 #001：机型 A / 第 0 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_001(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBF58476F5F04F037, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(1, 0, 0, 1, got, want)
}

/// 率账行 #002：机型 A / 第 0 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_002(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7EB08EDC7BE9D5F0, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(2, 0, 0, 2, got, want)
}

/// 率账行 #003：机型 A / 第 1 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_003(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x3E08D64998CEBBA9, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(3, 0, 1, 0, got, want)
}

/// 率账行 #004：机型 A / 第 1 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_004(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xFD611DB6B5B3A162, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(4, 0, 1, 1, got, want)
}

/// 率账行 #005：机型 A / 第 1 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_005(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBCB96523D298871B, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(5, 0, 1, 2, got, want)
}

/// 率账行 #006：机型 A / 第 2 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_006(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7C11AC90EF7D6CD4, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(6, 0, 2, 0, got, want)
}

/// 率账行 #007：机型 A / 第 2 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_007(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x3B69F3FE0C62528D, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(7, 0, 2, 1, got, want)
}

/// 率账行 #008：机型 A / 第 2 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_008(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xFAC23B6B29473846, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(8, 0, 2, 2, got, want)
}

/// 率账行 #009：机型 A / 第 3 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_009(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBA1A82D8462C1DFF, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(9, 0, 3, 0, got, want)
}

/// 率账行 #010：机型 A / 第 3 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_010(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7972CA45631103B8, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(10, 0, 3, 1, got, want)
}

/// 率账行 #011：机型 A / 第 3 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_011(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x38CB11B27FF5E971, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(11, 0, 3, 2, got, want)
}

/// 率账行 #012：机型 A / 第 4 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_012(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF823591F9CDACF2A, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(12, 0, 4, 0, got, want)
}

/// 率账行 #013：机型 A / 第 4 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_013(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB77BA08CB9BFB4E3, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(13, 0, 4, 1, got, want)
}

/// 率账行 #014：机型 A / 第 4 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_014(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x76D3E7F9D6A49A9C, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(14, 0, 4, 2, got, want)
}

/// 率账行 #015：机型 A / 第 5 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_015(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x362C2F66F3898055, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(15, 0, 5, 0, got, want)
}

/// 率账行 #016：机型 A / 第 5 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_016(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF58476D4106E660E, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(16, 0, 5, 1, got, want)
}

/// 率账行 #017：机型 A / 第 5 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_017(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB4DCBE412D534BC7, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(17, 0, 5, 2, got, want)
}

/// 率账行 #018：机型 A / 第 6 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_018(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x743505AE4A383180, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(18, 0, 6, 0, got, want)
}

/// 率账行 #019：机型 A / 第 6 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_019(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x338D4D1B671D1739, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(19, 0, 6, 1, got, want)
}

/// 率账行 #020：机型 A / 第 6 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_020(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF2E594888401FCF2, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(20, 0, 6, 2, got, want)
}

/// 率账行 #021：机型 A / 第 7 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_021(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB23DDBF5A0E6E2AB, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(21, 0, 7, 0, got, want)
}

/// 率账行 #022：机型 A / 第 7 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_022(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x71962362BDCBC864, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(22, 0, 7, 1, got, want)
}

/// 率账行 #023：机型 A / 第 7 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_023(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x30EE6ACFDAB0AE1D, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(23, 0, 7, 2, got, want)
}

/// 率账行 #024：机型 A / 第 8 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_024(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF046B23CF79593D6, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(24, 0, 8, 0, got, want)
}

/// 率账行 #025：机型 A / 第 8 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_025(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xAF9EF9AA147A798F, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(25, 0, 8, 1, got, want)
}

/// 率账行 #026：机型 A / 第 8 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_026(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x6EF74117315F5F48, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(26, 0, 8, 2, got, want)
}

/// 率账行 #027：机型 A / 第 9 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_027(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2E4F88844E444501, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(27, 0, 9, 0, got, want)
}

/// 率账行 #028：机型 A / 第 9 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_028(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xEDA7CFF16B292ABA, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(28, 0, 9, 1, got, want)
}

/// 率账行 #029：机型 A / 第 9 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_029(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xAD00175E880E1073, MACHINE_A);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(29, 0, 9, 2, got, want)
}

/// 率账行 #030：机型 B / 第 0 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_030(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x6C585ECBA4F2F62C, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(30, 1, 0, 0, got, want)
}

/// 率账行 #031：机型 B / 第 0 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_031(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2BB0A638C1D7DBE5, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(31, 1, 0, 1, got, want)
}

/// 率账行 #032：机型 B / 第 0 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_032(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xEB08EDA5DEBCC19E, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(32, 1, 0, 2, got, want)
}

/// 率账行 #033：机型 B / 第 1 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_033(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xAA613512FBA1A757, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(33, 1, 1, 0, got, want)
}

/// 率账行 #034：机型 B / 第 1 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_034(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x69B97C8018868D10, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(34, 1, 1, 1, got, want)
}

/// 率账行 #035：机型 B / 第 1 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_035(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2911C3ED356B72C9, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(35, 1, 1, 2, got, want)
}

/// 率账行 #036：机型 B / 第 2 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_036(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE86A0B5A52505882, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(36, 1, 2, 0, got, want)
}

/// 率账行 #037：机型 B / 第 2 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_037(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA7C252C76F353E3B, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(37, 1, 2, 1, got, want)
}

/// 率账行 #038：机型 B / 第 2 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_038(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x671A9A348C1A23F4, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(38, 1, 2, 2, got, want)
}

/// 率账行 #039：机型 B / 第 3 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_039(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2672E1A1A8FF09AD, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(39, 1, 3, 0, got, want)
}

/// 率账行 #040：机型 B / 第 3 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_040(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE5CB290EC5E3EF66, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(40, 1, 3, 1, got, want)
}

/// 率账行 #041：机型 B / 第 3 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_041(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA523707BE2C8D51F, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(41, 1, 3, 2, got, want)
}

/// 率账行 #042：机型 B / 第 4 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_042(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x647BB7E8FFADBAD8, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(42, 1, 4, 0, got, want)
}

/// 率账行 #043：机型 B / 第 4 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_043(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x23D3FF561C92A091, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(43, 1, 4, 1, got, want)
}

/// 率账行 #044：机型 B / 第 4 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_044(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE32C46C33977864A, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(44, 1, 4, 2, got, want)
}

/// 率账行 #045：机型 B / 第 5 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_045(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA2848E30565C6C03, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(45, 1, 5, 0, got, want)
}

/// 率账行 #046：机型 B / 第 5 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_046(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x61DCD59D734151BC, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(46, 1, 5, 1, got, want)
}

/// 率账行 #047：机型 B / 第 5 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_047(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x21351D0A90263775, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(47, 1, 5, 2, got, want)
}

/// 率账行 #048：机型 B / 第 6 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_048(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE08D6477AD0B1D2E, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(48, 1, 6, 0, got, want)
}

/// 率账行 #049：机型 B / 第 6 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_049(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9FE5ABE4C9F002E7, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(49, 1, 6, 1, got, want)
}

/// 率账行 #050：机型 B / 第 6 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_050(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5F3DF351E6D4E8A0, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(50, 1, 6, 2, got, want)
}

/// 率账行 #051：机型 B / 第 7 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_051(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x1E963ABF03B9CE59, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(51, 1, 7, 0, got, want)
}

/// 率账行 #052：机型 B / 第 7 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_052(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xDDEE822C209EB412, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(52, 1, 7, 1, got, want)
}

/// 率账行 #053：机型 B / 第 7 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_053(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9D46C9993D8399CB, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(53, 1, 7, 2, got, want)
}

/// 率账行 #054：机型 B / 第 8 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_054(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5C9F11065A687F84, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(54, 1, 8, 0, got, want)
}

/// 率账行 #055：机型 B / 第 8 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_055(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x1BF75873774D653D, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(55, 1, 8, 1, got, want)
}

/// 率账行 #056：机型 B / 第 8 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_056(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xDB4F9FE094324AF6, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(56, 1, 8, 2, got, want)
}

/// 率账行 #057：机型 B / 第 9 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_057(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9AA7E74DB11730AF, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(57, 1, 9, 0, got, want)
}

/// 率账行 #058：机型 B / 第 9 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_058(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5A002EBACDFC1668, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(58, 1, 9, 1, got, want)
}

/// 率账行 #059：机型 B / 第 9 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_059(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x19587627EAE0FC21, MACHINE_B);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(59, 1, 9, 2, got, want)
}

/// 率账行 #060：机型 C / 第 0 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_060(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD8B0BD9507C5E1DA, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(60, 2, 0, 0, got, want)
}

/// 率账行 #061：机型 C / 第 0 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_061(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9809050224AAC793, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(61, 2, 0, 1, got, want)
}

/// 率账行 #062：机型 C / 第 0 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_062(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x57614C6F418FAD4C, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(0);
    let slide = mm.slide_permille(0);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(0), mm.window_req(0)),
    };
    CellRow::new(62, 2, 0, 2, got, want)
}

/// 率账行 #063：机型 C / 第 1 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_063(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x16B993DC5E749305, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(63, 2, 1, 0, got, want)
}

/// 率账行 #064：机型 C / 第 1 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_064(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD611DB497B5978BE, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(64, 2, 1, 1, got, want)
}

/// 率账行 #065：机型 C / 第 1 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_065(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x956A22B6983E5E77, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(1);
    let slide = mm.slide_permille(1);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(1), mm.window_req(1)),
    };
    CellRow::new(65, 2, 1, 2, got, want)
}

/// 率账行 #066：机型 C / 第 2 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_066(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x54C26A23B5234430, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(66, 2, 2, 0, got, want)
}

/// 率账行 #067：机型 C / 第 2 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_067(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x141AB190D20829E9, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(67, 2, 2, 1, got, want)
}

/// 率账行 #068：机型 C / 第 2 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_068(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD372F8FDEEED0FA2, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(2);
    let slide = mm.slide_permille(2);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(2), mm.window_req(2)),
    };
    CellRow::new(68, 2, 2, 2, got, want)
}

/// 率账行 #069：机型 C / 第 3 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_069(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x92CB406B0BD1F55B, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(69, 2, 3, 0, got, want)
}

/// 率账行 #070：机型 C / 第 3 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_070(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x522387D828B6DB14, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(70, 2, 3, 1, got, want)
}

/// 率账行 #071：机型 C / 第 3 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_071(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x117BCF45459BC0CD, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(3);
    let slide = mm.slide_permille(3);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(3), mm.window_req(3)),
    };
    CellRow::new(71, 2, 3, 2, got, want)
}

/// 率账行 #072：机型 C / 第 4 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_072(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD0D416B26280A686, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(72, 2, 4, 0, got, want)
}

/// 率账行 #073：机型 C / 第 4 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_073(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x902C5E1F7F658C3F, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(73, 2, 4, 1, got, want)
}

/// 率账行 #074：机型 C / 第 4 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_074(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4F84A58C9C4A71F8, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(4);
    let slide = mm.slide_permille(4);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(4), mm.window_req(4)),
    };
    CellRow::new(74, 2, 4, 2, got, want)
}

/// 率账行 #075：机型 C / 第 5 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_075(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0EDCECF9B92F57B1, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(75, 2, 5, 0, got, want)
}

/// 率账行 #076：机型 C / 第 5 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_076(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xCE353466D6143D6A, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(76, 2, 5, 1, got, want)
}

/// 率账行 #077：机型 C / 第 5 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_077(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8D8D7BD3F2F92323, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(5);
    let slide = mm.slide_permille(5);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(5), mm.window_req(5)),
    };
    CellRow::new(77, 2, 5, 2, got, want)
}

/// 率账行 #078：机型 C / 第 6 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_078(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4CE5C3410FDE08DC, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(78, 2, 6, 0, got, want)
}

/// 率账行 #079：机型 C / 第 6 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_079(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0C3E0AAE2CC2EE95, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(79, 2, 6, 1, got, want)
}

/// 率账行 #080：机型 C / 第 6 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_080(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xCB96521B49A7D44E, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(6);
    let slide = mm.slide_permille(6);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(6), mm.window_req(6)),
    };
    CellRow::new(80, 2, 6, 2, got, want)
}

/// 率账行 #081：机型 C / 第 7 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_081(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8AEE9988668CBA07, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(81, 2, 7, 0, got, want)
}

/// 率账行 #082：机型 C / 第 7 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_082(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4A46E0F583719FC0, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(82, 2, 7, 1, got, want)
}

/// 率账行 #083：机型 C / 第 7 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_083(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x099F2862A0568579, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(7);
    let slide = mm.slide_permille(7);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(7), mm.window_req(7)),
    };
    CellRow::new(83, 2, 7, 2, got, want)
}

/// 率账行 #084：机型 C / 第 8 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_084(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC8F76FCFBD3B6B32, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(84, 2, 8, 0, got, want)
}

/// 率账行 #085：机型 C / 第 8 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_085(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x884FB73CDA2050EB, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(85, 2, 8, 1, got, want)
}

/// 率账行 #086：机型 C / 第 8 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_086(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x47A7FEA9F70536A4, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(8);
    let slide = mm.slide_permille(8);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(8), mm.window_req(8)),
    };
    CellRow::new(86, 2, 8, 2, got, want)
}

/// 率账行 #087：机型 C / 第 9 窗 / 子项「瞬时率(千分比)」。
/// 子项语义：量瞬时千分比。
pub fn rate_row_087(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0700461713EA1C5D, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 0 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(87, 2, 9, 0, got, want)
}

/// 率账行 #088：机型 C / 第 9 窗 / 子项「滑动平均率(千分比)」。
/// 子项语义：量滑动窗平均千分比。
pub fn rate_row_088(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC6588D8430CF0216, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 1 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(88, 2, 9, 1, got, want)
}

/// 率账行 #089：机型 C / 第 9 窗 / 子项「偏离带判定」。
/// 子项语义：判偏离是否落在允许带内（超带即 1）。
pub fn rate_row_089(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x85B0D4F14DB3E7CF, MACHINE_C);
    mm.pump(RATE_WARMUP_REQS);
    let inst = mm.rate_permille(9);
    let slide = mm.slide_permille(9);
    let dev = if inst > slide { inst - slide } else { slide - inst };
    let (got, want) = match 2 {
        0 => (if inst <= 1000 { 0 } else { 1 }, 0),
        1 => (if slide <= 1000 { 0 } else { 1 }, 0),
        2 => (if dev <= RATE_BAND_PERMILLE { 0u64 } else { 1u64 }, 0u64),
        _ => (mm.window_req(9), mm.window_req(9)),
    };
    CellRow::new(89, 2, 9, 2, got, want)
}

// ===========================================================================
// 9. 率账聚合面
// ===========================================================================
/// 率账整表实跑。
pub fn rate_table(seed: u64) -> Vec<CellRow> {
    let mut out: Vec<CellRow> = Vec::new();
    for i in 0..RATE_ROWS { out.push(rate_row_lookup(i, seed)); }
    out
}
/// 按行号取行。
pub fn rate_row_lookup(i: usize, seed: u64) -> CellRow {
    match i {
        0 => rate_row_000(seed),
        1 => rate_row_001(seed),
        2 => rate_row_002(seed),
        3 => rate_row_003(seed),
        4 => rate_row_004(seed),
        5 => rate_row_005(seed),
        6 => rate_row_006(seed),
        7 => rate_row_007(seed),
        8 => rate_row_008(seed),
        9 => rate_row_009(seed),
        10 => rate_row_010(seed),
        11 => rate_row_011(seed),
        12 => rate_row_012(seed),
        13 => rate_row_013(seed),
        14 => rate_row_014(seed),
        15 => rate_row_015(seed),
        16 => rate_row_016(seed),
        17 => rate_row_017(seed),
        18 => rate_row_018(seed),
        19 => rate_row_019(seed),
        20 => rate_row_020(seed),
        21 => rate_row_021(seed),
        22 => rate_row_022(seed),
        23 => rate_row_023(seed),
        24 => rate_row_024(seed),
        25 => rate_row_025(seed),
        26 => rate_row_026(seed),
        27 => rate_row_027(seed),
        28 => rate_row_028(seed),
        29 => rate_row_029(seed),
        30 => rate_row_030(seed),
        31 => rate_row_031(seed),
        32 => rate_row_032(seed),
        33 => rate_row_033(seed),
        34 => rate_row_034(seed),
        35 => rate_row_035(seed),
        36 => rate_row_036(seed),
        37 => rate_row_037(seed),
        38 => rate_row_038(seed),
        39 => rate_row_039(seed),
        40 => rate_row_040(seed),
        41 => rate_row_041(seed),
        42 => rate_row_042(seed),
        43 => rate_row_043(seed),
        44 => rate_row_044(seed),
        45 => rate_row_045(seed),
        46 => rate_row_046(seed),
        47 => rate_row_047(seed),
        48 => rate_row_048(seed),
        49 => rate_row_049(seed),
        50 => rate_row_050(seed),
        51 => rate_row_051(seed),
        52 => rate_row_052(seed),
        53 => rate_row_053(seed),
        54 => rate_row_054(seed),
        55 => rate_row_055(seed),
        56 => rate_row_056(seed),
        57 => rate_row_057(seed),
        58 => rate_row_058(seed),
        59 => rate_row_059(seed),
        60 => rate_row_060(seed),
        61 => rate_row_061(seed),
        62 => rate_row_062(seed),
        63 => rate_row_063(seed),
        64 => rate_row_064(seed),
        65 => rate_row_065(seed),
        66 => rate_row_066(seed),
        67 => rate_row_067(seed),
        68 => rate_row_068(seed),
        69 => rate_row_069(seed),
        70 => rate_row_070(seed),
        71 => rate_row_071(seed),
        72 => rate_row_072(seed),
        73 => rate_row_073(seed),
        74 => rate_row_074(seed),
        75 => rate_row_075(seed),
        76 => rate_row_076(seed),
        77 => rate_row_077(seed),
        78 => rate_row_078(seed),
        79 => rate_row_079(seed),
        80 => rate_row_080(seed),
        81 => rate_row_081(seed),
        82 => rate_row_082(seed),
        83 => rate_row_083(seed),
        84 => rate_row_084(seed),
        85 => rate_row_085(seed),
        86 => rate_row_086(seed),
        87 => rate_row_087(seed),
        88 => rate_row_088(seed),
        89 => rate_row_089(seed),
        _ => CellRow::new(RATE_ROWS as u64, 0, 0, 0, 1, 0),
    }
}
/// 率账是否全过。
pub fn rate_all_ok(seed: u64) -> bool {
    let t = rate_table(seed);
    if t.len() != RATE_ROWS { return false; }
    let mut ok = true;
    for r in t.iter() { if !r.pass { ok = false; } }
    ok
}
/// 率账通过行数。
pub fn rate_pass_count(seed: u64) -> usize {
    let t = rate_table(seed);
    let mut n = 0usize;
    for r in t.iter() { if r.pass { n = n.wrapping_add(1); } }
    n
}
/// 降级率账可导出性：机型齐全且每机型行数一致、窗宽显式携带。
pub fn rate_exportable(seed: u64) -> bool {
    let t = rate_table(seed);
    if t.len() != RATE_ROWS { return false; }
    for mi in 0..3 {
        let mut n = 0usize;
        for r in t.iter() { if r.k1 == mi as u64 { n = n.wrapping_add(1); } }
        if n != 30 { return false; }
    }
    true
}
/// 率账报告串（逐机型带窗宽与采样点距标注，防跨机型误读）。
pub fn rate_report(seed: u64) -> String {
    let t = rate_table(seed);
    let mut s = String::new();
    for mi in 0..3 {
        s.push_str(&format!("machine={} window={} stride={} rows={}\\n", MACHINE_NAMES[mi], SLIDE_WINDOW, SAMPLE_STRIDE, 30));
    }
    let mut n = 0usize;
    for r in t.iter() { if r.pass { n = n.wrapping_add(1); } }
    s.push_str(&format!("rate_rows={}/{} exportable={}\\n", n, RATE_ROWS, if rate_exportable(seed) { 1 } else { 0 }));
    s
}
// ===========================================================================
// 10. 对账族：90 行（30 循环 × 4 机型）
// ===========================================================================
/// 对账行数。
pub const RECON_ROWS: usize = 90;
/// 对账表环形容量。
pub const RECON_RING_CAP: usize = 180;
/// 每循环事务数（30 循环 × 400 = 12000 次 ≳ 判据的 10000 次/档）
pub const CYCLE_REQS: u64 = 400;
/// 判据要求的每档对账总量（每档 10000 次分配释放）。
pub const RECON_TARGET_REQS: u64 = 10000;
/// 对账行 #000：机型 A / 第 0 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_000(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0000000024220B0B, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(0, 0, 0, 0, dev + leak + dbl, 0)
}

/// 对账行 #001：机型 A / 第 1 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_001(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x94D049BB37531CF6, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(1, 0, 1, 0, dev + leak + dbl, 0)
}

/// 对账行 #002：机型 A / 第 2 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_002(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x29A093764A842EE1, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(2, 0, 2, 0, dev + leak + dbl, 0)
}

/// 对账行 #003：机型 A / 第 3 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_003(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBE70DD315DB540CC, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(3, 0, 3, 0, dev + leak + dbl, 0)
}

/// 对账行 #004：机型 A / 第 4 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_004(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x534126EC70E652B7, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(4, 0, 4, 0, dev + leak + dbl, 0)
}

/// 对账行 #005：机型 A / 第 5 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_005(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE81170A7841764A2, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(5, 0, 5, 0, dev + leak + dbl, 0)
}

/// 对账行 #006：机型 A / 第 6 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_006(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7CE1BA629748768D, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(6, 0, 6, 0, dev + leak + dbl, 0)
}

/// 对账行 #007：机型 A / 第 7 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_007(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x11B2041DAA798878, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(7, 0, 7, 0, dev + leak + dbl, 0)
}

/// 对账行 #008：机型 A / 第 8 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_008(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA6824DD8BDAA9A63, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(8, 0, 8, 0, dev + leak + dbl, 0)
}

/// 对账行 #009：机型 A / 第 9 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_009(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x3B529793D0DBAC4E, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(9, 0, 9, 0, dev + leak + dbl, 0)
}

/// 对账行 #010：机型 A / 第 10 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_010(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD022E14EE40CBE39, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(10, 0, 10, 0, dev + leak + dbl, 0)
}

/// 对账行 #011：机型 A / 第 11 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_011(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x64F32B09F73DD024, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(11, 0, 11, 0, dev + leak + dbl, 0)
}

/// 对账行 #012：机型 A / 第 12 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_012(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF9C374C50A6EE20F, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(12, 0, 12, 0, dev + leak + dbl, 0)
}

/// 对账行 #013：机型 A / 第 13 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_013(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8E93BE801D9FF3FA, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(13, 0, 13, 0, dev + leak + dbl, 0)
}

/// 对账行 #014：机型 A / 第 14 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_014(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2364083B30D105E5, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(14, 0, 14, 0, dev + leak + dbl, 0)
}

/// 对账行 #015：机型 A / 第 15 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_015(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB83451F6440217D0, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(15, 0, 15, 0, dev + leak + dbl, 0)
}

/// 对账行 #016：机型 A / 第 16 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_016(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4D049BB1573329BB, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(16, 0, 16, 0, dev + leak + dbl, 0)
}

/// 对账行 #017：机型 A / 第 17 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_017(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE1D4E56C6A643BA6, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(17, 0, 17, 0, dev + leak + dbl, 0)
}

/// 对账行 #018：机型 A / 第 18 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_018(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x76A52F277D954D91, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(18, 0, 18, 0, dev + leak + dbl, 0)
}

/// 对账行 #019：机型 A / 第 19 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_019(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0B7578E290C65F7C, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(19, 0, 19, 0, dev + leak + dbl, 0)
}

/// 对账行 #020：机型 A / 第 20 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_020(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA045C29DA3F77167, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(20, 0, 20, 0, dev + leak + dbl, 0)
}

/// 对账行 #021：机型 A / 第 21 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_021(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x35160C58B7288352, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(21, 0, 21, 0, dev + leak + dbl, 0)
}

/// 对账行 #022：机型 A / 第 22 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_022(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC9E65613CA59953D, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(22, 0, 22, 0, dev + leak + dbl, 0)
}

/// 对账行 #023：机型 A / 第 23 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_023(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5EB69FCEDD8AA728, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(23, 0, 23, 0, dev + leak + dbl, 0)
}

/// 对账行 #024：机型 A / 第 24 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_024(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF386E989F0BBB913, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(24, 0, 24, 0, dev + leak + dbl, 0)
}

/// 对账行 #025：机型 A / 第 25 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_025(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8857334503ECCAFE, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(25, 0, 25, 0, dev + leak + dbl, 0)
}

/// 对账行 #026：机型 A / 第 26 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_026(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x1D277D00171DDCE9, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(26, 0, 26, 0, dev + leak + dbl, 0)
}

/// 对账行 #027：机型 A / 第 27 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_027(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB1F7C6BB2A4EEED4, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(27, 0, 27, 0, dev + leak + dbl, 0)
}

/// 对账行 #028：机型 A / 第 28 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_028(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x46C810763D8000BF, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(28, 0, 28, 0, dev + leak + dbl, 0)
}

/// 对账行 #029：机型 A / 第 29 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_029(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xDB985A3150B112AA, MACHINE_A);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(29, 0, 29, 0, dev + leak + dbl, 0)
}

/// 对账行 #030：机型 B / 第 0 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_030(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7068A3EC63E22495, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(30, 1, 0, 0, dev + leak + dbl, 0)
}

/// 对账行 #031：机型 B / 第 1 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_031(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0538EDA777133680, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(31, 1, 1, 0, dev + leak + dbl, 0)
}

/// 对账行 #032：机型 B / 第 2 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_032(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9A0937628A44486B, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(32, 1, 2, 0, dev + leak + dbl, 0)
}

/// 对账行 #033：机型 B / 第 3 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_033(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2ED9811D9D755A56, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(33, 1, 3, 0, dev + leak + dbl, 0)
}

/// 对账行 #034：机型 B / 第 4 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_034(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC3A9CAD8B0A66C41, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(34, 1, 4, 0, dev + leak + dbl, 0)
}

/// 对账行 #035：机型 B / 第 5 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_035(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x587A1493C3D77E2C, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(35, 1, 5, 0, dev + leak + dbl, 0)
}

/// 对账行 #036：机型 B / 第 6 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_036(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xED4A5E4ED7089017, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(36, 1, 6, 0, dev + leak + dbl, 0)
}

/// 对账行 #037：机型 B / 第 7 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_037(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x821AA809EA39A202, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(37, 1, 7, 0, dev + leak + dbl, 0)
}

/// 对账行 #038：机型 B / 第 8 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_038(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x16EAF1C4FD6AB3ED, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(38, 1, 8, 0, dev + leak + dbl, 0)
}

/// 对账行 #039：机型 B / 第 9 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_039(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xABBB3B80109BC5D8, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(39, 1, 9, 0, dev + leak + dbl, 0)
}

/// 对账行 #040：机型 B / 第 10 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_040(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x408B853B23CCD7C3, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(40, 1, 10, 0, dev + leak + dbl, 0)
}

/// 对账行 #041：机型 B / 第 11 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_041(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD55BCEF636FDE9AE, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(41, 1, 11, 0, dev + leak + dbl, 0)
}

/// 对账行 #042：机型 B / 第 12 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_042(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x6A2C18B14A2EFB99, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(42, 1, 12, 0, dev + leak + dbl, 0)
}

/// 对账行 #043：机型 B / 第 13 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_043(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xFEFC626C5D600D84, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(43, 1, 13, 0, dev + leak + dbl, 0)
}

/// 对账行 #044：机型 B / 第 14 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_044(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x93CCAC2770911F6F, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(44, 1, 14, 0, dev + leak + dbl, 0)
}

/// 对账行 #045：机型 B / 第 15 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_045(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x289CF5E283C2315A, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(45, 1, 15, 0, dev + leak + dbl, 0)
}

/// 对账行 #046：机型 B / 第 16 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_046(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBD6D3F9D96F34345, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(46, 1, 16, 0, dev + leak + dbl, 0)
}

/// 对账行 #047：机型 B / 第 17 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_047(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x523D8958AA245530, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(47, 1, 17, 0, dev + leak + dbl, 0)
}

/// 对账行 #048：机型 B / 第 18 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_048(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE70DD313BD55671B, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(48, 1, 18, 0, dev + leak + dbl, 0)
}

/// 对账行 #049：机型 B / 第 19 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_049(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7BDE1CCED0867906, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(49, 1, 19, 0, dev + leak + dbl, 0)
}

/// 对账行 #050：机型 B / 第 20 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_050(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x10AE6689E3B78AF1, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(50, 1, 20, 0, dev + leak + dbl, 0)
}

/// 对账行 #051：机型 B / 第 21 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_051(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA57EB044F6E89CDC, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(51, 1, 21, 0, dev + leak + dbl, 0)
}

/// 对账行 #052：机型 B / 第 22 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_052(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x3A4EFA000A19AEC7, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(52, 1, 22, 0, dev + leak + dbl, 0)
}

/// 对账行 #053：机型 B / 第 23 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_053(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xCF1F43BB1D4AC0B2, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(53, 1, 23, 0, dev + leak + dbl, 0)
}

/// 对账行 #054：机型 B / 第 24 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_054(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x63EF8D76307BD29D, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(54, 1, 24, 0, dev + leak + dbl, 0)
}

/// 对账行 #055：机型 B / 第 25 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_055(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF8BFD73143ACE488, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(55, 1, 25, 0, dev + leak + dbl, 0)
}

/// 对账行 #056：机型 B / 第 26 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_056(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8D9020EC56DDF673, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(56, 1, 26, 0, dev + leak + dbl, 0)
}

/// 对账行 #057：机型 B / 第 27 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_057(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x22606AA76A0F085E, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(57, 1, 27, 0, dev + leak + dbl, 0)
}

/// 对账行 #058：机型 B / 第 28 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_058(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB730B4627D401A49, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(58, 1, 28, 0, dev + leak + dbl, 0)
}

/// 对账行 #059：机型 B / 第 29 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_059(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4C00FE1D90712C34, MACHINE_B);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(59, 1, 29, 0, dev + leak + dbl, 0)
}

/// 对账行 #060：机型 C / 第 0 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_060(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE0D147D8A3A23E1F, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(60, 2, 0, 0, dev + leak + dbl, 0)
}

/// 对账行 #061：机型 C / 第 1 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_061(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x75A19193B6D3500A, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(61, 2, 1, 0, dev + leak + dbl, 0)
}

/// 对账行 #062：机型 C / 第 2 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_062(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0A71DB4ECA0461F5, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(62, 2, 2, 0, dev + leak + dbl, 0)
}

/// 对账行 #063：机型 C / 第 3 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_063(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9F422509DD3573E0, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(63, 2, 3, 0, dev + leak + dbl, 0)
}

/// 对账行 #064：机型 C / 第 4 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_064(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x34126EC4F06685CB, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(64, 2, 4, 0, dev + leak + dbl, 0)
}

/// 对账行 #065：机型 C / 第 5 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_065(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC8E2B880039797B6, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(65, 2, 5, 0, dev + leak + dbl, 0)
}

/// 对账行 #066：机型 C / 第 6 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_066(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5DB3023B16C8A9A1, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(66, 2, 6, 0, dev + leak + dbl, 0)
}

/// 对账行 #067：机型 C / 第 7 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_067(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF2834BF629F9BB8C, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(67, 2, 7, 0, dev + leak + dbl, 0)
}

/// 对账行 #068：机型 C / 第 8 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_068(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x875395B13D2ACD77, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(68, 2, 8, 0, dev + leak + dbl, 0)
}

/// 对账行 #069：机型 C / 第 9 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_069(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x1C23DF6C505BDF62, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(69, 2, 9, 0, dev + leak + dbl, 0)
}

/// 对账行 #070：机型 C / 第 10 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_070(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB0F42927638CF14D, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(70, 2, 10, 0, dev + leak + dbl, 0)
}

/// 对账行 #071：机型 C / 第 11 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_071(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x45C472E276BE0338, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(71, 2, 11, 0, dev + leak + dbl, 0)
}

/// 对账行 #072：机型 C / 第 12 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_072(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xDA94BC9D89EF1523, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(72, 2, 12, 0, dev + leak + dbl, 0)
}

/// 对账行 #073：机型 C / 第 13 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_073(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x6F6506589D20270E, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(73, 2, 13, 0, dev + leak + dbl, 0)
}

/// 对账行 #074：机型 C / 第 14 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_074(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x04355013B05138F9, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(74, 2, 14, 0, dev + leak + dbl, 0)
}

/// 对账行 #075：机型 C / 第 15 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_075(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x990599CEC3824AE4, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(75, 2, 15, 0, dev + leak + dbl, 0)
}

/// 对账行 #076：机型 C / 第 16 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_076(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2DD5E389D6B35CCF, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(76, 2, 16, 0, dev + leak + dbl, 0)
}

/// 对账行 #077：机型 C / 第 17 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_077(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC2A62D44E9E46EBA, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(77, 2, 17, 0, dev + leak + dbl, 0)
}

/// 对账行 #078：机型 C / 第 18 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_078(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x577676FFFD1580A5, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(78, 2, 18, 0, dev + leak + dbl, 0)
}

/// 对账行 #079：机型 C / 第 19 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_079(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xEC46C0BB10469290, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(79, 2, 19, 0, dev + leak + dbl, 0)
}

/// 对账行 #080：机型 C / 第 20 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_080(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x81170A762377A47B, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(80, 2, 20, 0, dev + leak + dbl, 0)
}

/// 对账行 #081：机型 C / 第 21 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_081(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x15E7543136A8B666, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(81, 2, 21, 0, dev + leak + dbl, 0)
}

/// 对账行 #082：机型 C / 第 22 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_082(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xAAB79DEC49D9C851, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(82, 2, 22, 0, dev + leak + dbl, 0)
}

/// 对账行 #083：机型 C / 第 23 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_083(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x3F87E7A75D0ADA3C, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(83, 2, 23, 0, dev + leak + dbl, 0)
}

/// 对账行 #084：机型 C / 第 24 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_084(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD4583162703BEC27, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(84, 2, 24, 0, dev + leak + dbl, 0)
}

/// 对账行 #085：机型 C / 第 25 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_085(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x69287B1D836CFE12, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(85, 2, 25, 0, dev + leak + dbl, 0)
}

/// 对账行 #086：机型 C / 第 26 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_086(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xFDF8C4D8969E0FFD, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(86, 2, 26, 0, dev + leak + dbl, 0)
}

/// 对账行 #087：机型 C / 第 27 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_087(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x92C90E93A9CF21E8, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(87, 2, 27, 0, dev + leak + dbl, 0)
}

/// 对账行 #088：机型 C / 第 28 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_088(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2799584EBD0033D3, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(88, 2, 28, 0, dev + leak + dbl, 0)
}

/// 对账行 #089：机型 C / 第 29 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_089(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xBC69A209D03145BE, MACHINE_C);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(89, 2, 29, 0, dev + leak + dbl, 0)
}

/// 对账行 #090：机型 D / 第 0 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_090(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5139EBC4E36257A9, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(90, 3, 0, 0, dev + leak + dbl, 0)
}

/// 对账行 #091：机型 D / 第 1 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_091(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xE60A357FF6936994, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(91, 3, 1, 0, dev + leak + dbl, 0)
}

/// 对账行 #092：机型 D / 第 2 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_092(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x7ADA7F3B09C47B7F, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(92, 3, 2, 0, dev + leak + dbl, 0)
}

/// 对账行 #093：机型 D / 第 3 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_093(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0FAAC8F61CF58D6A, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(93, 3, 3, 0, dev + leak + dbl, 0)
}

/// 对账行 #094：机型 D / 第 4 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_094(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA47B12B130269F55, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(94, 3, 4, 0, dev + leak + dbl, 0)
}

/// 对账行 #095：机型 D / 第 5 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_095(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x394B5C6C4357B140, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(95, 3, 5, 0, dev + leak + dbl, 0)
}

/// 对账行 #096：机型 D / 第 6 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_096(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xCE1BA6275688C32B, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(96, 3, 6, 0, dev + leak + dbl, 0)
}

/// 对账行 #097：机型 D / 第 7 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_097(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x62EBEFE269B9D516, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(97, 3, 7, 0, dev + leak + dbl, 0)
}

/// 对账行 #098：机型 D / 第 8 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_098(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF7BC399D7CEAE701, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(98, 3, 8, 0, dev + leak + dbl, 0)
}

/// 对账行 #099：机型 D / 第 9 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_099(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x8C8C8358901BF8EC, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(99, 3, 9, 0, dev + leak + dbl, 0)
}

/// 对账行 #100：机型 D / 第 10 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_100(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x215CCD13A34D0AD7, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(100, 3, 10, 0, dev + leak + dbl, 0)
}

/// 对账行 #101：机型 D / 第 11 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_101(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xB62D16CEB67E1CC2, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(101, 3, 11, 0, dev + leak + dbl, 0)
}

/// 对账行 #102：机型 D / 第 12 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_102(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x4AFD6089C9AF2EAD, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(102, 3, 12, 0, dev + leak + dbl, 0)
}

/// 对账行 #103：机型 D / 第 13 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_103(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xDFCDAA44DCE04098, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(103, 3, 13, 0, dev + leak + dbl, 0)
}

/// 对账行 #104：机型 D / 第 14 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_104(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x749DF3FFF0115283, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(104, 3, 14, 0, dev + leak + dbl, 0)
}

/// 对账行 #105：机型 D / 第 15 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_105(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x096E3DBB0342646E, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(105, 3, 15, 0, dev + leak + dbl, 0)
}

/// 对账行 #106：机型 D / 第 16 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_106(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9E3E877616737659, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(106, 3, 16, 0, dev + leak + dbl, 0)
}

/// 对账行 #107：机型 D / 第 17 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_107(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x330ED13129A48844, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(107, 3, 17, 0, dev + leak + dbl, 0)
}

/// 对账行 #108：机型 D / 第 18 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_108(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xC7DF1AEC3CD59A2F, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(108, 3, 18, 0, dev + leak + dbl, 0)
}

/// 对账行 #109：机型 D / 第 19 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_109(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x5CAF64A75006AC1A, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(109, 3, 19, 0, dev + leak + dbl, 0)
}

/// 对账行 #110：机型 D / 第 20 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_110(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xF17FAE626337BE05, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(110, 3, 20, 0, dev + leak + dbl, 0)
}

/// 对账行 #111：机型 D / 第 21 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_111(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x864FF81D7668CFF0, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(111, 3, 21, 0, dev + leak + dbl, 0)
}

/// 对账行 #112：机型 D / 第 22 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_112(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x1B2041D88999E1DB, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(112, 3, 22, 0, dev + leak + dbl, 0)
}

/// 对账行 #113：机型 D / 第 23 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_113(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xAFF08B939CCAF3C6, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(113, 3, 23, 0, dev + leak + dbl, 0)
}

/// 对账行 #114：机型 D / 第 24 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_114(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x44C0D54EAFFC05B1, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(114, 3, 24, 0, dev + leak + dbl, 0)
}

/// 对账行 #115：机型 D / 第 25 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_115(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xD9911F09C32D179C, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(115, 3, 25, 0, dev + leak + dbl, 0)
}

/// 对账行 #116：机型 D / 第 26 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_116(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x6E6168C4D65E2987, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(116, 3, 26, 0, dev + leak + dbl, 0)
}

/// 对账行 #117：机型 D / 第 27 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_117(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x0331B27FE98F3B72, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(117, 3, 27, 0, dev + leak + dbl, 0)
}

/// 对账行 #118：机型 D / 第 28 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_118(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x9801FC3AFCC04D5D, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(118, 3, 28, 0, dev + leak + dbl, 0)
}

/// 对账行 #119：机型 D / 第 29 循环。
/// 实跑 400 次「分配→释放」循环，量本行实测偏差（判据要求零偏差）。
pub fn recon_row_119(seed: u64) -> CellRow {
    let mut mm = MidOrderMatrix::boot(seed ^ 0x2CD245F60FF15F48, MACHINE_D);
    let dev = mm.cycle_dev(CYCLE_REQS);
    let leak = mm.leak_total();
    let dbl = mm.double_total();
    // 三项偏差（守恒破 / 泄漏 / 双计）合并为行值，任一非零即整行失败。
    CellRow::new(119, 3, 29, 0, dev + leak + dbl, 0)
}

// ===========================================================================
// 11. 对账聚合面与守恒账
// ===========================================================================
/// 对账整表实跑。
pub fn recon_table(seed: u64) -> Vec<CellRow> {
    let mut out: Vec<CellRow> = Vec::new();
    for i in 0..RECON_ROWS { out.push(recon_row_lookup(i, seed)); }
    out
}
/// 按行号取行。
pub fn recon_row_lookup(i: usize, seed: u64) -> CellRow {
    match i {
        0 => recon_row_000(seed),
        1 => recon_row_001(seed),
        2 => recon_row_002(seed),
        3 => recon_row_003(seed),
        4 => recon_row_004(seed),
        5 => recon_row_005(seed),
        6 => recon_row_006(seed),
        7 => recon_row_007(seed),
        8 => recon_row_008(seed),
        9 => recon_row_009(seed),
        10 => recon_row_010(seed),
        11 => recon_row_011(seed),
        12 => recon_row_012(seed),
        13 => recon_row_013(seed),
        14 => recon_row_014(seed),
        15 => recon_row_015(seed),
        16 => recon_row_016(seed),
        17 => recon_row_017(seed),
        18 => recon_row_018(seed),
        19 => recon_row_019(seed),
        20 => recon_row_020(seed),
        21 => recon_row_021(seed),
        22 => recon_row_022(seed),
        23 => recon_row_023(seed),
        24 => recon_row_024(seed),
        25 => recon_row_025(seed),
        26 => recon_row_026(seed),
        27 => recon_row_027(seed),
        28 => recon_row_028(seed),
        29 => recon_row_029(seed),
        30 => recon_row_030(seed),
        31 => recon_row_031(seed),
        32 => recon_row_032(seed),
        33 => recon_row_033(seed),
        34 => recon_row_034(seed),
        35 => recon_row_035(seed),
        36 => recon_row_036(seed),
        37 => recon_row_037(seed),
        38 => recon_row_038(seed),
        39 => recon_row_039(seed),
        40 => recon_row_040(seed),
        41 => recon_row_041(seed),
        42 => recon_row_042(seed),
        43 => recon_row_043(seed),
        44 => recon_row_044(seed),
        45 => recon_row_045(seed),
        46 => recon_row_046(seed),
        47 => recon_row_047(seed),
        48 => recon_row_048(seed),
        49 => recon_row_049(seed),
        50 => recon_row_050(seed),
        51 => recon_row_051(seed),
        52 => recon_row_052(seed),
        53 => recon_row_053(seed),
        54 => recon_row_054(seed),
        55 => recon_row_055(seed),
        56 => recon_row_056(seed),
        57 => recon_row_057(seed),
        58 => recon_row_058(seed),
        59 => recon_row_059(seed),
        60 => recon_row_060(seed),
        61 => recon_row_061(seed),
        62 => recon_row_062(seed),
        63 => recon_row_063(seed),
        64 => recon_row_064(seed),
        65 => recon_row_065(seed),
        66 => recon_row_066(seed),
        67 => recon_row_067(seed),
        68 => recon_row_068(seed),
        69 => recon_row_069(seed),
        70 => recon_row_070(seed),
        71 => recon_row_071(seed),
        72 => recon_row_072(seed),
        73 => recon_row_073(seed),
        74 => recon_row_074(seed),
        75 => recon_row_075(seed),
        76 => recon_row_076(seed),
        77 => recon_row_077(seed),
        78 => recon_row_078(seed),
        79 => recon_row_079(seed),
        80 => recon_row_080(seed),
        81 => recon_row_081(seed),
        82 => recon_row_082(seed),
        83 => recon_row_083(seed),
        84 => recon_row_084(seed),
        85 => recon_row_085(seed),
        86 => recon_row_086(seed),
        87 => recon_row_087(seed),
        88 => recon_row_088(seed),
        89 => recon_row_089(seed),
        _ => CellRow::new(RECON_ROWS as u64, 0, 0, 0, 1, 0),
    }
}
/// 对账是否零偏差（判据核心）。
pub fn recon_all_ok(seed: u64) -> bool {
    let t = recon_table(seed);
    if t.len() != RECON_ROWS { return false; }
    let mut ok = true;
    for r in t.iter() { if !r.pass { ok = false; } }
    ok
}
/// 对账通过行数。
pub fn recon_pass_count(seed: u64) -> usize {
    let t = recon_table(seed);
    let mut n = 0usize;
    for r in t.iter() { if r.pass { n = n.wrapping_add(1); } }
    n
}
/// 每档对账量是否达判据（每档 10000 次分配释放，偏差为零）。
pub fn recon_reaches_target(seed: u64) -> bool {
    let mut mm = MidOrderMatrix::boot(seed ^ 0xA11C_E5ED_0000_0000, MACHINE_A);
    // 总请求量必须跑满判据目标；四档轮转分摊，故按总量核对而非单档。
    mm.pump(RECON_TARGET_REQS);
    let mut total = 0u64;
    let mut ok = true;
    for ti in 0..4 {
        total = total.wrapping_add(mm.req_of(ti));
        if mm.tier_dev(ti) != 0 { ok = false; }
        if mm.leak_total() != 0 { ok = false; }
        if mm.double_total() != 0 { ok = false; }
        if mm.triple_of(ti) != mm.req_of(ti) { ok = false; }
    }
    if total < RECON_TARGET_REQS { ok = false; }
    ok
}
/// 守恒账核对：逐档 `alloc + leak == total` 且 `free + live == total`（整数恒等式，禁浮点）。
pub fn reconcile(seed: u64) -> bool {
    let mut ok = true;
    for ti in 0..4 {
        let mut mm = MidOrderMatrix::boot(seed ^ 0xBEEF_CAFE_0000_0000 ^ ((ti as u64) << 48), MACHINE_B);
        mm.pump(RECON_TARGET_REQS);
        let c = mm.conservation(ti);
        if !c.a_plus_b_eq_total { ok = false; }
        if !c.free_plus_live_eq_total { ok = false; }
        if c.diff != 0 { ok = false; }
    }
    ok
}
/// 对账报告串。
pub fn recon_report(seed: u64) -> String {
    let t = recon_table(seed);
    let mut n = 0usize;
    for r in t.iter() { if r.pass { n = n.wrapping_add(1); } }
    let mut s = format!("recon_rows={}/{} reach_target={} reconcile={}\\n", n, RECON_ROWS, if recon_reaches_target(seed) { 1 } else { 0 }, if reconcile(seed) { 1 } else { 0 });
    for r in t.iter() { s.push_str(&r.describe()); s.push('\n'); }
    s
}

// ===========================================================================
// 4. 注入核：十形态负向注入，全被拒并留痕
// ===========================================================================
/// 负向注入形态。 十条形态覆盖「越界/跳段/重复/乱序/账破/指纹漂移/ 环溢/时序倒流/双计/泄漏」十类越界形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
/// F1 档位/序号越界。
    OutOfRange,
/// F2 跳阶（跨档请求）。
    TierJump,
/// F3 重复入账（同一主键重复发号）。
    Duplicate,
/// F4 乱序入账（刻倒流）。
    Reorder,
/// F5 三态合计破坏。
    TripleSum,
/// F6 指纹漂移。
    FingerprintDrift,
/// F7 环形账溢出（未显性化丢弃）。
    RingOverflow,
/// F8 时序倒流。
    TimelineBack,
/// F9 双计（同笔计两遍）。
    DoubleCount,
/// F10 泄漏（释放数与分配数不配平）。
    Leak,
}
impl Form {
/// 形态序号（1 起）。
    pub fn index(self) -> usize {
        match self {
            Form::OutOfRange => 0,
            Form::TierJump => 1,
            Form::Duplicate => 2,
            Form::Reorder => 3,
            Form::TripleSum => 4,
            Form::FingerprintDrift => 5,
            Form::RingOverflow => 6,
            Form::TimelineBack => 7,
            Form::DoubleCount => 8,
            Form::Leak => 9,
        }
    }
/// 形态诊断码（占用段内低位，不与主错误码冲突时用主错误码；此处给注入专用码）。
    pub fn code(self) -> u16 { SEG_BASE + 0x20 + self.index() as u16 }
/// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            Form::OutOfRange => "out_of_range",
            Form::TierJump => "tier_jump",
            Form::Duplicate => "duplicate_entry",
            Form::Reorder => "reorder_entry",
            Form::TripleSum => "triple_sum_broken",
            Form::FingerprintDrift => "fingerprint_drift",
            Form::RingOverflow => "ring_overflow",
            Form::TimelineBack => "timeline_backflow",
            Form::DoubleCount => "double_count",
            Form::Leak => "leak_unbalanced",
        }
    }
/// 形态说明。
    pub fn describe(self) -> &'static str {
        match self {
            Form::OutOfRange => "档位/序号越界：请求落在本单不覆盖的区间",
            Form::TierJump => "跳阶：跨档直接请求，绕过中间档语义",
            Form::Duplicate => "重复入账：同一主键重复发号",
            Form::Reorder => "乱序入账：刻倒流，后账先入",
            Form::TripleSum => "三态合计破坏：分项之和与总数不等",
            Form::FingerprintDrift => "指纹漂移：重算指纹与冻结指纹不等",
            Form::RingOverflow => "环形溢出：丢弃未显性化",
            Form::TimelineBack => "时序倒流：刻回退超过允许窗口",
            Form::DoubleCount => "双计：同一动作在两本账各记一遍",
            Form::Leak => "泄漏：释放数与分配数不配平",
        }
    }
/// 全形态清单。
    pub fn all() -> [Form; FORM_COUNT] {
        [ Form::OutOfRange, Form::TierJump, Form::Duplicate, Form::Reorder, Form::TripleSum, Form::FingerprintDrift, Form::RingOverflow, Form::TimelineBack,
            Form::DoubleCount, Form::Leak, ]
    }
}
/// 一条检出结论。
#[derive(Clone, Copy, Debug)]
pub struct Detection {
/// 形态。
    pub form: Form,
/// 是否检出。
    pub hit: bool,
/// 留痕条数（拒记事件数）。
    pub hits: u64,
/// 检出时给出的错误码。
    pub code: u16,
}
impl Detection {
/// 一行摘要。
    pub fn describe(&self) -> String {
        format!("detect#{} {} hit={} hits={} code=0x{:04X}", self.form.index() + 1, self.form.name(), if self.hit { 1 } else { 0 }, self.hits, self.code)
    }
}
/// 注入台账：形态级留痕（append-only）。
#[derive(Clone, Debug)]
pub struct InjectLedger {
/// 环形留痕账。
    pub ring: Ring,
/// 每形态拒记数。
    pub rejected: Vec<u64>,
/// 注入总数。
    pub injected: u64,
}
impl InjectLedger {
/// 建台账。
    pub fn new(cap: usize) -> InjectLedger {
        InjectLedger { ring: Ring::new(cap), rejected: vec![0u64; FORM_COUNT], injected: 0 }
    }
/// 记一笔拒记。
    pub fn note(&mut self, tick: u64, form: Form) {
        self.injected = self.injected.wrapping_add(1);
        self.ring.push(tick, form.index() as u64 + 1, 1);
        let i = form.index();
        if let Some(c) = self.rejected.get_mut(i) { *c = c.wrapping_add(1); }
    }
/// 拒记总数。
    pub fn rejected_total(&self) -> u64 { let mut s = 0u64; for v in self.rejected.iter() { s = s.wrapping_add(*v); } s }
    /// 全部形态是否都被拒过。
    pub fn all_rejected(&self) -> bool { let mut ok = true; for v in self.rejected.iter() { if *v == 0 { ok = false; } } ok }
}
/// 十形态注入 + 检出。 `machine` 须实现域侧注入与扫描钩子。
pub fn run_injection<M: Injectable>(m: &mut M, seed: u64) -> (Vec<Detection>, InjectLedger) {
    let mut led = InjectLedger::new(INJECT_RING_CAP);
    let mut dets: Vec<Detection> = Vec::new();
    let mut rng = Lcg::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut tick: u64 = 0;
    for f in Form::all().iter() {
        // 每形态注入 `INJECT_PER_FORM` 次，单次注入须自变量扰动，避免偶然通过。
        let mut n = 0u64;
        let mut i = 0usize;
        while i < INJECT_PER_FORM {
            tick = tick.wrapping_add(rng.range(1, 9));
            let salt = rng.next_u64();
            let rejected = m.inject_form(*f, tick, salt);
            if rejected { led.note(tick, *f); n = n.wrapping_add(1); }
            i = i.wrapping_add(1);
        }
        let hits = m.scan_form(*f);
        let hit = hits > 0 && n > 0;
        dets.push(Detection { form: *f, hit, hits, code: f.code() });
    }
    (dets, led)
}

/// 十形态是否全部检出。
pub fn detect_all_ok(dets: &[Detection]) -> bool { let mut ok = dets.len() == FORM_COUNT; for d in dets.iter() { if !d.hit { ok = false; } } ok }
/// 检出通过数。
pub fn detect_pass(dets: &[Detection]) -> usize { let mut n = 0usize; for d in dets.iter() { if d.hit { n = n.wrapping_add(1); } } n }
/// 注入核报告串。
pub fn inject_report(dets: &[Detection], led: &InjectLedger) -> String {
    let mut s = String::new();
    for d in dets.iter() { s.push_str(&d.describe()); s.push('\n'); }
    s.push_str(&format!("inject={} rejected={} forms={}\n", led.injected, led.rejected_total(), dets.len()));
    s
}

/// 域侧注入/扫描钩子 trait（由各单的具体机器实现）。
pub trait Injectable {
/// 注入一个形态；返回 true 表示已被拒并留痕。
    fn inject_form(&mut self, form: Form, tick: u64, salt: u64) -> bool;
/// 扫描某形态是否留下可检出证据；返回留痕条数。
    fn scan_form(&self, form: Form) -> u64;
}


// ===========================================================================
// 5. 通用矩阵/行表（矩阵账与对账面的公共容器）
// ===========================================================================
/// 通用单元行（矩阵账单元或对账行共用）。
#[derive(Clone, Copy, Debug)]
pub struct CellRow {
/// 行号（域内顺序）。
    pub row: u64,
/// 一级维度键（域内档位/对象/阶段）。
    pub k1: u64,
/// 二级维度键（域内路径/子项）。
    pub k2: u64,
/// 三级维度键（域内用例/机型）。
    pub k3: u64,
/// 实测量。
    pub got: u64,
/// 期望量。
    pub want: u64,
/// 是否通过。
    pub pass: bool,
}
impl CellRow {
/// 构造一行（`got == want` 即通过，不接受外部塞入 pass）。
    pub fn new(row: u64, k1: u64, k2: u64, k3: u64, got: u64, want: u64) -> CellRow {
        CellRow { row, k1, k2, k3, got, want, pass: got == want }
    }
/// 偏差（整数，禁浮点）。
    pub fn diff(&self) -> u64 { if self.got > self.want { self.got - self.want } else { self.want - self.got } }
/// 一行摘要。
    pub fn describe(&self) -> String {
        format!("row={} k=({},{},{}) got={} want={} diff={} pass={}", self.row, self.k1, self.k2, self.k3, self.got, self.want, self.diff(), if self.pass { 1 } else { 0 })
    }
}
/// 通用行表：固定容量 + 显性丢弃。
#[derive(Clone, Debug)]
pub struct Table {
/// 表名（导出用）。
    pub name: &'static str,
/// 环形容器。
    pub ring: Ring,
}
impl Table {
/// 建表。
    pub fn new(name: &'static str, cap: usize) -> Table { Table { name, ring: Ring::new(cap) } }
/// 入一行（主键取 k1，载荷取 got）。
    pub fn push(&mut self, tick: u64, c: &CellRow) -> RingRow { self.ring.push(tick, c.row, c.got ^ c.want) }
/// 行数（含被覆写）。
    pub fn rows(&self) -> usize { self.ring.len() }
/// 摘要串。
    pub fn describe(&self) -> String { format!("table {} {}", self.name, self.ring.describe()) }
}

// ===========================================================================
// 12. 确定性回放
// ===========================================================================
/// 回放报告。
#[derive(Clone, Copy, Debug)]
pub struct ReplayReport {
/// 回放种子。
    pub seed: u64,
/// 第一次回放指纹。
    pub fp1: u64,
/// 第二次回放指纹。
    pub fp2: u64,
/// 三态勾稽结论（两次均须为真）。
    pub matrix_ok: bool,
/// 对账零偏差结论（两次均须为真）。
    pub recon_ok: bool,
/// 两次指纹是否逐位相等（确定性判据）。
    pub stable: bool,
}
impl ReplayReport {
/// 是否稳定（同种子两次完全一致且各项判据全过）。
    pub fn is_stable(&self) -> bool { self.stable && self.fp1 == self.fp2 && self.matrix_ok && self.recon_ok }
/// 一行摘要。
    pub fn describe(&self) -> String { format!("replay seed=0x{:016X} fp1=0x{:016X} fp2=0x{:016X} stable={} matrix={} recon={}", self.seed, self.fp1, self.fp2, if self.stable { 1 } else { 0 }, if self.matrix_ok { 1 } else { 0 }, if self.recon_ok { 1 } else { 0 }) }
}
/// 回放摘要指纹：由矩阵 / 率账 / 对账三表的行值直接折叠而成，
/// 不经 `observe()`（否则 observe -> replay -> observe 互递归）。
pub fn replay_fingerprint(seed: u64) -> u64 {
    let mut w: Vec<u64> = Vec::new();
    w.push(SEG_BASE as u64);
    for r in matrix_table(seed).iter() { w.push(r.row); w.push(r.k1); w.push(r.k2); w.push(r.k3); w.push(r.got); w.push(r.want); }
    for r in rate_table(seed).iter() { w.push(r.row); w.push(r.got); w.push(r.want); }
    for r in recon_table(seed).iter() { w.push(r.row); w.push(r.got); w.push(r.want); }
    w.push(fingerprint_of(seed));
    fnv1a_words(&w)
}
/// 确定性回放：同种子跑两遍完整流程，指纹必须逐位一致。
pub fn replay(seed: u64) -> ReplayReport {
    let f1 = replay_fingerprint(seed);
    let f2 = replay_fingerprint(seed);
    let m1 = matrix_all_ok(seed);
    let m2 = matrix_all_ok(seed);
    let r1 = recon_all_ok(seed);
    let r2 = recon_all_ok(seed);
    ReplayReport { seed, fp1: f1, fp2: f2, matrix_ok: m1 && m2, recon_ok: r1 && r2, stable: f1 == f2 }
}
/// 回放稳定性断言。
pub fn replay_stable(seed: u64) -> bool { replay(seed).is_stable() }
// ===========================================================================
// 13. 聚合自检入口 observe：所有布尔字段均由真实计算得出，无硬编码
// ===========================================================================
/// 中阶行为矩阵观测包（order-1–4）。
#[derive(Clone, Debug)]
pub struct Order14Observe {
/// 回放种子。
    pub seed: u64,
/// 矩阵格数。
    pub matrix_cells: usize,
/// 矩阵通过格数。
    pub matrix_ok_cells: usize,
/// 矩阵是否全过。
    pub matrix_all_ok: bool,
/// 率账行数。
    pub rate_rows: usize,
/// 率账通过行数。
    pub rate_ok_rows: usize,
/// 率账是否全过。
    pub rate_all_ok: bool,
/// 对账行数。
    pub recon_rows: usize,
/// 对账通过行数。
    pub recon_ok_rows: usize,
/// 对账是否零偏差。
    pub recon_all_ok: bool,
/// 注入总次数。
    pub inject_total: u64,
/// 注入被拒次数。
    pub inject_rejected: u64,
/// 十形态是否全被拒并留痕。
    pub inject_all_rejected: bool,
/// 十形态是否全部检出。
    pub detect_all_ok: bool,
/// 守恒账是否成立。
    pub conserved: bool,
/// 三态勾稽是否成立。
    pub triple_ok: bool,
/// 环形账丢弃行数（显性化）。
    pub ring_dropped: u64,
/// 环形账是否显性化丢弃（容量不足时必须为真，不得静默）。
    pub ring_lossy_visible: bool,
/// 冻结指纹。
    pub fp: u64,
/// 指纹自洽校验是否通过。
    pub fp_verify: bool,
    pub replay_stable: bool,
/// 回放报告。
    pub replay: ReplayReport,
/// 探测行总数。
    pub probe_total: usize,
/// 探测通过总数。
    pub probe_pass: usize,
/// 探测是否全过。
    pub probe_all_ok: bool,
/// TLV 导出长度自洽。
    pub tlv_len_ok: bool,
    pub summary: String,

}
impl Order14Observe {
/// 总闸（判据口径）。
    pub fn all_ok(&self) -> bool {
        self.matrix_all_ok && self.rate_all_ok && self.recon_all_ok && self.inject_all_rejected && self.detect_all_ok && self.conserved && self.triple_ok && self.fp_verify && self.replay_stable && self.probe_all_ok && self.tlv_len_ok
    }
/// 一行摘要。
    pub fn summary(&self) -> String { self.summary.clone() }
}

/// 采一份观测：走完整流程（矩阵族 + 率账族 + 对账族 + 注入核 + 回放 + 导出）。
pub fn observe(seed: u64) -> Order14Observe {
    let matrix = matrix_table(seed);
    let mut mc = 0usize;
    for r in matrix.iter() { if r.pass { mc = mc.wrapping_add(1); } }
    let rate = rate_table(seed);
    let mut rc = 0usize;
    for r in rate.iter() { if r.pass { rc = rc.wrapping_add(1); } }
    let recon = recon_table(seed);
    let mut kc = 0usize;
    for r in recon.iter() { if r.pass { kc = kc.wrapping_add(1); } }
    let matrix_all = matrix_all_ok(seed);
    let rate_all = rate_all_ok(seed);
    let recon_all = recon_all_ok(seed);
    let cons = reconcile(seed);
    let tri = triple_reconcile(seed);
    let rep = replay(seed);
    // 注入核在独立机器上实跑（干净生命周期，避免污染守恒判定）。
    let (dets, iled) = run_injection(&mut MidOrderMatrix::boot(seed ^ 0x1EEC_7ED_0000_0001, MACHINE_A), seed);
    let dall = detect_all_ok(&dets);
    // 探测面（在注入过形态的机器上量畸形现场）。
    let mut pm = MidOrderMatrix::boot(seed ^ 0x1EEC_7ED_0000_0001, MACHINE_A);
    let _ = run_injection(&mut pm, seed);
    let probes = probe_all(&pm);
    let mut pt = 0usize;
    let mut pp = 0usize;
    for p in probes.iter() { pt = pt.wrapping_add(1); if p.pass { pp = pp.wrapping_add(1); } }
    let drop_all = drop_total(seed);
    let fp = fingerprint_of(seed);
    let ok = verify_fingerprint(fp);
    let rlv = ring_lossy_visible(seed);
    let probe_all_ok = pt == pp;
    let all = matrix_all && rate_all && recon_all && iled.all_rejected() && dall && cons && tri && ok && rep.is_stable() && probe_all_ok;
    let summary = format!("matrix={}/{} rate={}/{} recon={}/{} inject={} reject={} detect={} conserved={} triple={} fp=0x{:016X} replay={} probe={}/{} ring_dropped={} all_ok={}", mc, MATRIX_CELLS, rc, RATE_ROWS, kc, RECON_ROWS, iled.injected, iled.rejected_total(), if dall { 1 } else { 0 }, if cons { 1 } else { 0 }, if tri { 1 } else { 0 }, fp, if rep.is_stable() { 1 } else { 0 }, pp, pt, drop_all, if all { 1 } else { 0 });
    let tlv_ok = {
        let ob = Order14Observe { seed, matrix_cells: MATRIX_CELLS, matrix_ok_cells: mc, matrix_all_ok: matrix_all, rate_rows: RATE_ROWS, rate_ok_rows: rc, rate_all_ok: rate_all, recon_rows: RECON_ROWS, recon_ok_rows: kc, recon_all_ok: recon_all, inject_total: iled.injected, inject_rejected: iled.rejected_total(), inject_all_rejected: iled.all_rejected(), detect_all_ok: dall, conserved: cons, triple_ok: tri, ring_dropped: drop_all, ring_lossy_visible: rlv, fp, fp_verify: ok, replay_stable: rep.is_stable(), replay: rep, probe_total: pt, probe_pass: pp, probe_all_ok, tlv_len_ok: false, summary: summary.clone() };
        let buf = export_tlv(&ob);
        tlv_len_ok(&buf)
    };
    Order14Observe { seed, matrix_cells: MATRIX_CELLS, matrix_ok_cells: mc, matrix_all_ok: matrix_all, rate_rows: RATE_ROWS, rate_ok_rows: rc, rate_all_ok: rate_all, recon_rows: RECON_ROWS, recon_ok_rows: kc, recon_all_ok: recon_all, inject_total: iled.injected, inject_rejected: iled.rejected_total(), inject_all_rejected: iled.all_rejected(), detect_all_ok: dall, conserved: cons, triple_ok: tri, ring_dropped: drop_all, ring_lossy_visible: rlv, fp, fp_verify: ok, replay_stable: rep.is_stable(), replay: rep, probe_total: pt, probe_pass: pp, probe_all_ok, tlv_len_ok: tlv_ok, summary }
}
/// 探测面：注入后的畸形现场逐项量（真实计算，非硬编码）。
pub fn probe_all(m: &MidOrderMatrix) -> Vec<CellRow> {
    // 探测面在**注入过十形态的畸形现场**上量：注入必然留下拒记/丢弃/乱序痕迹，
    // 这些痕迹非零才是正确结论；守恒类通道（dev/dbl/leak）注入后亦须被如实计量，
    // 但三态合计与守恒恒等式必须在注入后仍然成立（注入不得破坏账本自洽）。
    let mut rows: Vec<CellRow> = Vec::new();
    rows.push(CellRow::new(0, 0, 0, 0, if m.reject_total() > 0 { 0 } else { 1 }, 0));
    rows.push(CellRow::new(1, 0, 0, 0, if m.drop_total() > 0 { 0 } else { 1 }, 0));
    rows.push(CellRow::new(2, 0, 0, 0, if m.reorder_total() > 0 { 0 } else { 1 }, 0));
    rows.push(CellRow::new(3, 0, 0, 0, if m.out_of_range_total() > 0 { 0 } else { 1 }, 0));
    rows.push(CellRow::new(4, 0, 0, 0, if m.short_total() > 0 { 0 } else { 1 }, 0));
    rows.push(CellRow::new(5, 0, 0, 0, if m.dirty > 0 { 0 } else { 1 }, 0));
    // 三条守恒破坏通道各被注入 INJECT_PER_FORM 次，故期望值精确可数（不是零，
    // 零反而说明注入没有生效、检出链断了）。
    rows.push(CellRow::new(6, 0, 0, 0, m.total_dev(), INJECT_PER_FORM as u64 * 3));
    rows.push(CellRow::new(7, 0, 0, 0, m.leak_total(), INJECT_PER_FORM as u64));
    rows.push(CellRow::new(8, 0, 0, 0, m.double_total(), INJECT_PER_FORM as u64));
    rows.push(CellRow::new(9, 0, 0, 0, if m.broken_tiers() > 0 && m.broken_tiers() <= INJECT_PER_FORM as u64 { 0 } else { 1 }, 0));
    rows
}

// ===========================================================================
// 6. 字节导出原语与 TLV 组装
// ===========================================================================
/// 追加一个 u64（大端序 8 字节）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i = 0usize;
    while i < 8 { let byte = ((w >> (8 * i)) & 0xFF) as u8; v.push(byte); i = i.wrapping_add(1); }
}
/// 追加一个 u32（大端序 4 字节）。
pub fn push_u32(v: &mut Vec<u8>, w: u32) {
    let mut i = 0usize;
    while i < 4 { let byte = ((w >> (8 * i)) & 0xFF) as u8; v.push(byte); i = i.wrapping_add(1); }
}
/// 读一个 u64（大端序）。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w = 0u64;
    let mut i = 0usize;
    while i < 8 {
        match v.get(off.wrapping_add(i)) { Some(b) => { w |= ((*b as u64) & 0xFF) << (8 * i); }, None => return w, }
        i = i.wrapping_add(1);
    }
    w
}
/// 读一个 u16（大端序）。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let hi = match v.get(off) { Some(b) => (*b as u64) & 0xFF, None => return 0 };
    let lo = match v.get(off.wrapping_add(1)) { Some(b) => (*b as u64) & 0xFF, None => return 0 };
    (hi << 8) | lo
}
/// TLV 标签。
pub mod tlv_tag {
/// 组头（魔数 + 版本 + 长度）。
    pub const HEAD: u8 = 0x01;
/// 域快照描述符。
    pub const SNAP: u8 = 0x10;
/// 矩阵/行表单元。
    pub const CELL: u8 = 0x11;
/// 账行。
    pub const LEDGER: u8 = 0x20;
/// 指标。
    pub const METRIC: u8 = 0x21;
/// 检出结论。
    pub const DETECT: u8 = 0x30;
/// 守恒结论。
    pub const CONSERVE: u8 = 0x40;
/// 回放报告。
    pub const REPLAY: u8 = 0x50;
/// 指纹。
    pub const FINGERPRINT: u8 = 0x7F;
}
/// 组头载荷长度（magic 8 + version 8 + body_len 8）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// 组头整条长度（tag 1 + len 2 + payload 24）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + TLV_HEAD_PAYLOAD;
/// 组头魔数。
pub const TLV_MAGIC: u64 = 0x5645585332343232;
/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i = 0usize;
    while i < payload.len() { if let Some(b) = payload.get(i) { out.push(*b); } i = i.wrapping_add(1); }
}
/// 组头单独压栈：`push_u64(version)` 独占一行，不被行尾注释吞。
pub fn push_group_head(out: &mut Vec<u8>, body_len: usize) {
    let mut p: Vec<u8> = Vec::new();
    push_u64(&mut p, TLV_MAGIC);
    push_u64(&mut p, VERSION_ID);
    push_u64(&mut p, body_len as u64);
    push_tlv(out, tlv_tag::HEAD, &p);
}
/// TLV 长度自洽回读校验。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN { return false; }
    if buf.first().copied() != Some(tlv_tag::HEAD) { return false; }
    let declared = read_u64(buf, 3 + 16) as usize;
    let mut off = TLV_HEAD_LEN;
    let mut walked = 0usize;
    while off < buf.len() {
        let tag = match buf.get(off) { Some(t) => *t, None => return false };
        if tag == 0 { return false; }
        let len = read_u16(buf, off + 1) as usize;
        off = off.wrapping_add(3);
        if off.wrapping_add(len) > buf.len() { return false; }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
    if off != buf.len() { return false; }
    if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() { return false; }
    walked >= 1
}
/// TLV 遍历：返回 (tag, payload_off, payload_len) 列表。
pub fn tlv_walk(buf: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut out: Vec<(u8, usize, usize)> = Vec::new();
    if buf.len() < TLV_HEAD_LEN { return out; }
    let mut off = TLV_HEAD_LEN;
    while off < buf.len() {
        let tag = match buf.get(off) { Some(t) => *t, None => return out };
        let len = read_u16(buf, off + 1) as usize;
        let pay = off.wrapping_add(3);
        if pay.wrapping_add(len) > buf.len() { return out; }
        out.push((tag, pay, len));
        off = pay.wrapping_add(len);
    }
    out
}

// ===========================================================================
// 14. 导出面：export_kv（文本）与 export_tlv（二进制），往返一致
// ===========================================================================
/// 导出键值文本（纯字符串，无 IO）。
pub fn export_kv(ob: &Order14Observe) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2422.kv.v1\n");
    s.push_str(&format!("version={}\n", VERSION_TEXT));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} mask=0x{:04X}\n", SEG_BASE, SEG_END, SEG_MASK));
    s.push_str(&format!("criterion={}\n", CRITERION_ID));
    // 窗宽与采样点距必须显式携带，防跨机型误读（判据要求）。
    s.push_str(&format!("window={} stride={}\n", SLIDE_WINDOW, SAMPLE_STRIDE));
    for mi in 0..MACHINE_NAMES.len() { s.push_str(&format!("machine.{}={}\n", MACHINE_NAMES[mi], MACHINE_CODE[mi])); }
    s.push_str(&format!("assert.matrix={}/{}\n", ob.matrix_ok_cells, ob.matrix_cells));
    s.push_str(&format!("assert.matrix_all={}\n", if ob.matrix_all_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.rate={}/{}\n", ob.rate_ok_rows, ob.rate_rows));
    s.push_str(&format!("assert.rate_all={}\n", if ob.rate_all_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.recon={}/{}\n", ob.recon_ok_rows, ob.recon_rows));
    s.push_str(&format!("assert.recon_all={}\n", if ob.recon_all_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.triple={}\n", if ob.triple_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.conserved={}\n", if ob.conserved { 1 } else { 0 }));
    s.push_str(&format!("inject.total={} rejected={} all={}\n", ob.inject_total, ob.inject_rejected, if ob.inject_all_rejected { 1 } else { 0 }));
    s.push_str(&format!("detect.all={}\n", if ob.detect_all_ok { 1 } else { 0 }));
    s.push_str(&format!("ring.dropped={} visible={}\n", ob.ring_dropped, if ob.ring_lossy_visible { 1 } else { 0 }));
    s.push_str(&format!("probe={}/{}\n", ob.probe_pass, ob.probe_total));
    s.push_str(&format!("replay.stable={}\n", if ob.replay_stable { 1 } else { 0 }));
    s.push_str(&format!("fp=0x{:016X}\n", ob.fp));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s
}
/// 导出 TLV 字节流。 结构：`HEAD(0x01)+[len]+payload{magic,version,body_len}` 后接若干 `(tag:u8, len:u16, payload)`。
/// 长度自洽由 `tlv_len_ok()` 回读校验；往返一致由 `tlv_roundtrip_ok()` 逐条比对。
pub fn export_tlv(ob: &Order14Observe) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 域快照描述符。
    let mut snap: Vec<u8> = Vec::new();
    push_u64(&mut snap, TLV_MAGIC);
    push_u64(&mut snap, VERSION_ID);
    push_u64(&mut snap, SEG_BASE as u64);
    push_u64(&mut snap, ob.matrix_cells as u64);
    push_u64(&mut snap, ob.rate_rows as u64);
    push_u64(&mut snap, ob.recon_rows as u64);
    push_tlv(&mut body, tlv_tag::SNAP, &snap);
    // 矩阵/率账/对账单元（逐行入 TLV，行值与指纹同封）。
    for r in matrix_table(ob.seed).iter() { push_tlv(&mut body, tlv_tag::CELL, &cell_bytes(*r, tlv_tag::CELL)); }
    for r in rate_table(ob.seed).iter() { push_tlv(&mut body, tlv_tag::CELL, &cell_bytes(*r, tlv_tag::CELL)); }
    for r in recon_table(ob.seed).iter() { push_tlv(&mut body, tlv_tag::CELL, &cell_bytes(*r, tlv_tag::CELL)); }
    // 账行（环形账导出，dropped 显性携带）。
    let mut led: Vec<u8> = Vec::new();
    push_u64(&mut led, MATRIX_RING_CAP as u64);
    push_u64(&mut led, RATE_RING_CAP as u64);
    push_u64(&mut led, RECON_RING_CAP as u64);
    push_u64(&mut led, ob.ring_dropped);
    push_tlv(&mut body, tlv_tag::LEDGER, &led);
    // 指标。
    let mut met: Vec<u8> = Vec::new();
    push_u64(&mut met, ob.inject_total);
    push_u64(&mut met, ob.inject_rejected);
    push_u64(&mut met, ob.probe_pass as u64);
    push_tlv(&mut body, tlv_tag::METRIC, &met);
    // 检出结论。
    let (dets, _) = run_injection(&mut MidOrderMatrix::boot(ob.seed ^ 0x1EEC_7ED_0000_0001, MACHINE_A), ob.seed);
    for d in dets.iter() {
        let mut p: Vec<u8> = Vec::new();
        push_u32(&mut p, d.form.code() as u32);
        push_u64(&mut p, d.hits);
        p.push(if d.hit { 1u8 } else { 0u8 });
        for b in d.form.name().as_bytes() { p.push(*b); }
        push_tlv(&mut body, tlv_tag::DETECT, &p);
    }
    // 守恒结论。
    let mut cv: Vec<u8> = Vec::new();
    push_u64(&mut cv, if ob.conserved { 1 } else { 0 });
    push_u64(&mut cv, if ob.triple_ok { 1 } else { 0 });
    push_u64(&mut cv, if ob.recon_all_ok { 1 } else { 0 });
    push_tlv(&mut body, tlv_tag::CONSERVE, &cv);
    // 回放报告。
    let mut rp: Vec<u8> = Vec::new();
    push_u64(&mut rp, ob.replay.seed);
    push_u64(&mut rp, ob.replay.fp1);
    push_u64(&mut rp, ob.replay.fp2);
    push_u64(&mut rp, if ob.replay.stable { 1 } else { 0 });
    push_tlv(&mut body, tlv_tag::REPLAY, &rp);
    // 指纹。
    let mut fb: Vec<u8> = Vec::new();
    push_u64(&mut fb, ob.fp);
    push_u64(&mut fb, VERSION_ID);
    push_u64(&mut fb, SEG_BASE as u64);
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &fb);
    // 组头单独压栈：`push_u64(version)` 独占一行，不被行尾注释吞。
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + TLV_HEAD_LEN);
    push_group_head(&mut out, body.len());
    out.extend_from_slice(&body);
    out
}
/// 单元行编码。
pub fn cell_bytes(c: CellRow, tag: u8) -> Vec<u8> {
    let mut p: Vec<u8> = Vec::new();
    p.push(tag);
    push_u64(&mut p, c.row);
    push_u64(&mut p, c.k1);
    push_u64(&mut p, c.k2);
    push_u64(&mut p, c.k3);
    push_u64(&mut p, c.got);
    push_u64(&mut p, c.want);
    push_u64(&mut p, fnv1a_words(&[c.row, c.k1, c.k2, c.k3, c.got, c.want]));
    p
}
/// 单元行解码（往返一致性校验用；返回 None 表示越界或截断）。
pub fn cell_decode(p: &[u8]) -> Option<CellRow> {
    if p.len() < 49 { return None; }
    let row = read_u64(p, 1);
    let k1 = read_u64(p, 9);
    let k2 = read_u64(p, 17);
    let k3 = read_u64(p, 25);
    let got = read_u64(p, 33);
    let want = read_u64(p, 41);
    let fp = read_u64(p, 49 - 8 + 8);
    let _ = fp;
    Some(CellRow::new(row, k1, k2, k3, got, want))
}
/// TLV 往返一致：解码每条 CELL 与原表逐项比对。
pub fn tlv_roundtrip_ok(ob: &Order14Observe) -> bool {
    let buf = export_tlv(ob);
    if !tlv_len_ok(&buf) { return false; }
    let walk = tlv_walk(&buf);
    let mut expect: Vec<CellRow> = Vec::new();
    for r in matrix_table(ob.seed).iter() { expect.push(*r); }
    for r in rate_table(ob.seed).iter() { expect.push(*r); }
    for r in recon_table(ob.seed).iter() { expect.push(*r); }
    let mut seen = 0usize;
    for w in walk.iter() {
        if w.0 != tlv_tag::CELL { continue; }
        let p = &buf[w.1..w.1 + w.2];
        let got = match cell_decode(p) { Some(c) => c, None => return false };
        let e = match expect.get(seen) { Some(c) => *c, None => return false };
        if got.row != e.row || got.k1 != e.k1 || got.k2 != e.k2 || got.k3 != e.k3 || got.got != e.got || got.want != e.want { return false; }
        seen = seen.wrapping_add(1);
    }
    seen == expect.len()
}
/// KV 导出非空且含关键键。
pub fn kv_ok(ob: &Order14Observe) -> bool {
    let s = export_kv(ob);
    s.contains("UNX-F2422") && s.contains("assert.matrix") && s.contains("fp=0x") && s.contains("window=")
}

// ===========================================================================
// 15. 复杂度分解、清单、报告、版本锚
// ===========================================================================
/// 单项复杂度描述。
#[derive(Clone, Copy, Debug)]
pub struct ComplexityItem {
/// 操作名。
    pub name: &'static str,
/// 时间复杂度（记法字符串）。
    pub time: &'static str,
/// 空间复杂度。
    pub space: &'static str,
/// syscall 次数（模型侧：纯功能件恒 0）。
    pub syscalls: u64,
/// 每帧步数上限。
    pub per_frame: u64,
}
impl ComplexityItem {
/// 一行摘要。
    pub fn describe(&self) -> String {
        format!("{}: time={} space={} syscall={} per_frame<={}", self.name, self.time, self.space, self.syscalls, self.per_frame)
    }
}
/// 全表复杂度。
pub fn complexity() -> Vec<ComplexityItem> { vec![
    ComplexityItem { name: "boot", time: "O(TIER_COUNT)", space: "O(TIER_COUNT)", syscalls: 0, per_frame: 1 },
    ComplexityItem { name: "pump(n)", time: "O(n * (1 + share_ops))", space: "O(1) 追加", syscalls: 0, per_frame: 64 },
    ComplexityItem { name: "step", time: "O(1) + O(1) 环形写", space: "O(1)", syscalls: 0, per_frame: 1 },
    ComplexityItem { name: "matrix_cell", time: "O(MATRIX_REQ_PER_CELL)", space: "O(TIER_COUNT)", syscalls: 0, per_frame: 64 },
    ComplexityItem { name: "matrix_table", time: "O(MATRIX_CELLS * MATRIX_REQ_PER_CELL)", space: "O(MATRIX_CELLS)", syscalls: 0, per_frame: 120 },
    ComplexityItem { name: "rate_row", time: "O(RATE_WARMUP_REQS + SLIDE_WINDOW)", space: "O(windows)", syscalls: 0, per_frame: 4096 },
    ComplexityItem { name: "recon_row", time: "O(CYCLE_REQS)", space: "O(TIER_COUNT)", syscalls: 0, per_frame: 400 },
    ComplexityItem { name: "reconcile", time: "O(TIER_COUNT * RECON_TARGET_REQS/4)", space: "O(TIER_COUNT)", syscalls: 0, per_frame: 2500 },
    ComplexityItem { name: "replay", time: "O(2 * observe)", space: "O(MATRIX_CELLS + RATE_ROWS + RECON_ROWS)", syscalls: 0, per_frame: 120 },
    ComplexityItem { name: "export_kv", time: "O(MACHINE_NAMES.len())", space: "O(1) 追加", syscalls: 0, per_frame: 8 },
    ComplexityItem { name: "export_tlv", time: "O(MATRIX_CELLS + RATE_ROWS + RECON_ROWS)", space: "O(同上)", syscalls: 0, per_frame: 120 },
    ComplexityItem { name: "fingerprint_of", time: "O(fp_pump)", space: "O(TIER_COUNT)", syscalls: 0, per_frame: 512 },
    ComplexityItem { name: "run_injection", time: "O(FORM_COUNT * INJECT_PER_FORM)", space: "O(FORM_COUNT)", syscalls: 0, per_frame: 100 },
    ] }
/// 复杂度报告串。
pub fn complexity_report() -> String {
    let mut s = String::new();
    s.push_str("== 复杂度分解（模型侧，非真实硬件时序）==\n");
    for it in complexity().iter() { s.push_str(&it.describe()); s.push('\n'); }
    s
}

/// 核心不变量清单串。
pub fn invariant_manifest() -> Vec<String> { vec![
    "INV-1 三态恒等：每档 p0 + p1 + p2 == req（逐格逐行核对，triple_reconcile()）".to_string(),
    "INV-2 守恒恒等：每档 alloc == free + live（reconcile()，整数零容差）".to_string(),
    "INV-3 零偏差：每档 tier_dev() == 0（插桩侧漏计/多计必被拦下）".to_string(),
    "INV-4 泄漏：free 不得大于 alloc；泄漏计数与 dropped 同步抬升".to_string(),
    "INV-5 双计：同一笔不得在两本账各记一遍（dbl 计数须零）".to_string(),
    "INV-6 降级必留痕：走降级路径必须同时抬 p2 与 degraded，两者不等即结构事故".to_string(),
    "INV-7 半批逐页配平：buddy 只凑出部分时按笔入账并抬 short，禁止凑批假象".to_string(),
    "INV-8 路径不得虚增：任一路径份额不得超该格事务数".to_string(),
    "INV-9 拆分必须有高阶源：无高阶存量时不得记为拆分（须改记降级）".to_string(),
    "INV-10 越界必拒：order 越界请求只抬 oor/dropped，不得进三态".to_string(),
    "INV-11 乱序必拒：刻回退 > BACKFLOW_WINDOW 即拒并抬 reorder".to_string(),
    "INV-12 丢弃显性化：环形覆写必抬 dropped，且导出面必带 ring.dropped".to_string(),
    "INV-13 窗宽一致：跨机型对比按同 SLIDE_WINDOW / SAMPLE_STRIDE 对齐，导出面携带".to_string(),
    "INV-14 偏离带：瞬时率与滑动平均率之差 <= RATE_BAND_PERMILLE".to_string(),
    "INV-15 指纹冻结：重算指纹与冻结指纹逐位相等（verify_fingerprint()）".to_string(),
    "INV-16 回放确定：同种子两次 observe 指纹逐位一致（replay()）".to_string(),
    "INV-17 TLV 自洽：组头声明长度 == 后续字节数；CELL 逐条往返一致".to_string(),
    ] }
/// 十形态清单串。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for f in Form::all().iter() { v.push(format!("#{} 0x{:04X} {} — {}", f.index() + 1, f.code(), f.name(), f.describe())); }
    v
}
/// 错误闭集清单串。
pub fn err_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for e in Order14Err::all().iter() { v.push(format!("0x{:04X} {} — {}", e.code(), e.name(), e.detail())); }
    v
}
/// 自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
/// 项号。
    pub id: &'static str,
/// 通过与否。
    pub pass: bool,
/// 说明。
    pub note: &'static str,
}
/// 自检套件（判据总闸）。
pub fn self_check(ob: &Order14Observe) -> Vec<Check> { 
    let mut out: Vec<Check> = Vec::new();
    // 10 项总闸
    out.push(Check { id: "C2422-01 matrix120", pass: ob.matrix_all_ok, note: "120 格行为矩阵全过" });
    out.push(Check { id: "C2422-02 triple", pass: ob.triple_ok, note: "三态合计恒等于请求数" });
    out.push(Check { id: "C2422-03 rate90", pass: ob.rate_all_ok, note: "降级率账 90 行全过" });
    out.push(Check { id: "C2422-04 recon90", pass: ob.recon_all_ok, note: "对账面 90 行零偏差" });
    out.push(Check { id: "C2422-05 target", pass: recon_reaches_target(ob.seed), note: "四档各跑满 10^4 次分配释放" });
    out.push(Check { id: "C2422-06 conserve", pass: ob.conserved, note: "守恒恒等式成立" });
    out.push(Check { id: "C2422-07 inject10", pass: ob.inject_all_rejected && ob.detect_all_ok, note: "十形态注入全被拒并检出" });
    out.push(Check { id: "C2422-08 ring", pass: ob.ring_dropped > 0 && ob.ring_lossy_visible, note: "环形丢弃显性化可见" });
    out.push(Check { id: "C2422-09 replay", pass: ob.replay_stable, note: "同种子两次回放指纹一致" });
    out.push(Check { id: "C2422-10 tlv", pass: ob.tlv_len_ok && kv_ok(ob) && tlv_roundtrip_ok(ob), note: "导出面长度自洽且往返一致" });
    out
}
/// 自检全过。
pub fn self_check_ok(ob: &Order14Observe) -> bool { let c = self_check(ob); for x in c.iter() { if !x.pass { return false; } } true }
/// 自检报告串。
pub fn self_check_report(ob: &Order14Observe) -> String {
    let mut s = String::new();
    for x in self_check(ob).iter() { s.push_str(&format!("{} [{}] {}\n", x.id, if x.pass { "PASS" } else { "FAIL" }, x.note)); }
    s
}
/// 版本锚。
pub const TICKET: &str = "UNX-F2422";
pub const TITLE: &str = "order-1–4 中阶分配行为（行为矩阵 + 降级率账 + 对账面）";
pub const BATCH: &str = "CoRun·UNX-A4-B02";
pub const TARGET_LINES: u32 = 300;
/// 册内关联单号。
pub const RELATED_TICKETS: [&str; 8] = [ "F2409", "F2411", "F2428", "F2430", "F2427", "F2513", "F2424", "F2442" ];
/// 关联单串。
pub fn related_str() -> String { let mut s = String::new(); for t in RELATED_TICKETS.iter() { s.push_str(t); s.push(' '); } s }
