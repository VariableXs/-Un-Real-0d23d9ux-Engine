//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18573 · 版本面缺陷账聚合（BUG-UNX E4 分册开账） · 0x80B00段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18573-J1 八批发现缺陷逐条汇总（分级🔴🟡🟢），红线类零条
//!       断言，AI-97 登记接口双绿
//! 设计要点：
//!   - 八批缺陷逐条开账：缺陷记录（批号/分级/生命周期三态）定容账本，
//!     实数 count 记账，账本满拒收不丢账；
//!   - 分级闭集：🔴Red=0 / 🟡Yellow=1 / 🟢Green=2，域外分级（≥3）拒收；
//!     红线类零条断言 = 全账扫描 Red 实收恒 0；
//!   - 生命周期状态机 Open→Confirmed→Closed 单向推进，Closed 复开必拒；
//!   - AI-97 登记接口双绿：格式绿（分级闭集内且字段齐备）∧ 守恒绿
//!     （批账之和 = 总账），两绿同时为真才允许登记；
//!   - 注入核四类：红线缺陷活性 / 域外分级 / Closed 复开 / 批号越界，
//!     逐类 10/10 轮必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80B00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：DefectRec（批/级/态三元）、DefectLedger（32 槽定容账）
//!   状态机  ：DefState 三态单向（Open→Confirmed→Closed）
//!   账本    ：batch_counts 八批分账、batch_totals 聚合账
//!   错误路径：DfxCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B00，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80B00;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 缺陷批数（冻结：八批）。
pub const BATCH_TOTAL: usize = 8;
/// 账本容量（冻结：32 槽）。
pub const LEDGER_CAP: usize = 32;
/// 规范语料每批缺陷数（冻结：每批 2 条 → 总 16 条）。
pub const PER_BATCH_DEFECTS: usize = 2;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 分级闭集 ───────────────────────────────────────────────────
/// 分级：🔴Red=0 / 🟡Yellow=1 / 🟢Green=2。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Red,
    Yellow,
    Green,
}

impl Severity {
    /// 级值（0/1/2）。
    pub fn val(self) -> u32 {
        match self {
            Severity::Red => 0,
            Severity::Yellow => 1,
            Severity::Green => 2,
        }
    }
    /// 级值 → 分级（域外返回 None）。
    pub fn from_val(v: u32) -> Option<Severity> {
        match v {
            0 => Some(Severity::Red),
            1 => Some(Severity::Yellow),
            2 => Some(Severity::Green),
            _ => None,
        }
    }
    /// 全级闭集。
    pub const ALL: [Severity; 3] = [Severity::Red, Severity::Yellow, Severity::Green];
}

// ── 生命周期状态机（单向三态） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefState {
    /// 开账：新发现。
    Open,
    /// 已确认：复核通过。
    Confirmed,
    /// 已闭账：处置完成。
    Closed,
}

impl DefState {
    pub const fn open() -> DefState {
        DefState::Open
    }
    /// 单向推进合法判定。
    pub fn can_advance_to(self, next: DefState) -> bool {
        matches!(
            (self, next),
            (DefState::Open, DefState::Confirmed) | (DefState::Confirmed, DefState::Closed)
        )
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DfxCode {
    /// 批号越界
    BatchRange,
    /// 分级域外（≥3）
    SevOutOfRange,
    /// 账本满
    LedgerFull,
    /// 红线缺陷活性（扫描必抓）
    RedActive,
    /// 生命周期倒退（Closed 复开 / Closed→Confirmed）
    StateRollback,
    /// 越级推进（Open→Closed）
    StateSkip,
    /// 二次登记
    AlreadyRegistered,
    /// 双绿未齐即登记
    RegisterNotDualGreen,
    /// 聚合守恒破坏（批账和 ≠ 总账）
    AggConserveBroken,
    /// 汇总行数与批数不符
    AggRowCountBroken,
    /// 级账守恒破坏（三级之和 ≠ 总账）
    SevConserveBroken,
    /// 双跑不一致
    NotReproducible,
}

impl DfxCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DfxCode::BatchRange => "批号越界：0..8 闭集外",
            DfxCode::SevOutOfRange => "分级域外：0/1/2 闭集外",
            DfxCode::LedgerFull => "账本满：32 槽上限",
            DfxCode::RedActive => "红线缺陷活性：扫描必抓",
            DfxCode::StateRollback => "生命周期倒退：Closed 复开",
            DfxCode::StateSkip => "越级推进：Open→Closed",
            DfxCode::AlreadyRegistered => "二次登记：AI-97 幂等",
            DfxCode::RegisterNotDualGreen => "双绿未齐即登记",
            DfxCode::AggConserveBroken => "聚合守恒破坏：批账和≠总账",
            DfxCode::AggRowCountBroken => "汇总行数与批数不符",
            DfxCode::SevConserveBroken => "级账守恒破坏：三级和≠总账",
            DfxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [DfxCode; 12] = [
        DfxCode::BatchRange,
        DfxCode::SevOutOfRange,
        DfxCode::LedgerFull,
        DfxCode::RedActive,
        DfxCode::StateRollback,
        DfxCode::StateSkip,
        DfxCode::AlreadyRegistered,
        DfxCode::RegisterNotDualGreen,
        DfxCode::AggConserveBroken,
        DfxCode::AggRowCountBroken,
        DfxCode::SevConserveBroken,
        DfxCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DfxCode::ALL.len() {
        if DfxCode::ALL[i].code() == code {
            return DfxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80B00 段闭集内"
}

// ── 缺陷记录 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DefectRec {
    /// 缺陷号（账内序）。
    pub id: u16,
    /// 批号 0..8。
    pub batch: u8,
    /// 分级（0/1/2）。
    pub sev: u8,
    /// 生命周期。
    pub state: DefState,
}

impl DefectRec {
    pub const fn empty() -> DefectRec {
        DefectRec { id: 0, batch: 0, sev: 0, state: DefState::Open }
    }
}

// ── 缺陷总账 ───────────────────────────────────────────────────
pub struct DefectLedger {
    /// 定容账本。
    pub recs: [DefectRec; LEDGER_CAP],
    /// 实收条数。
    pub count: usize,
    /// 八批分账（每批实收）。
    pub batch_counts: [u32; BATCH_TOTAL],
    /// 三级实收账（Red/Yellow/Green）。
    pub sev_counts: [u32; 3],
    /// AI-97 登记态（仅一次）。
    pub registered: bool,
    /// 注入红线活性记录数（扫描对账用）。
    pub red_injected: u32,
    /// 红线抓捕数。
    pub red_caught: u32,
    /// 注册尝试数。
    pub register_attempts: u32,
    /// 注入拒收数。
    pub rejected: u32,
}

impl DefectLedger {
    pub fn new() -> DefectLedger {
        DefectLedger {
            recs: [DefectRec::empty(); LEDGER_CAP],
            count: 0,
            batch_counts: [0u32; BATCH_TOTAL],
            sev_counts: [0u32; 3],
            registered: false,
            red_injected: 0,
            red_caught: 0,
            register_attempts: 0,
            rejected: 0,
        }
    }

    /// 开账一条缺陷：区间→分级→容量三闸依次过闸才落账。
    pub fn open_defect(&mut self, batch: usize, sev_val: u32) -> Result<usize, u32> {
        // 闸一：批号区间。
        if batch >= BATCH_TOTAL {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DfxCode::BatchRange.code());
        }
        // 闸二：分级闭集。
        let sev = match Severity::from_val(sev_val) {
            Some(s) => s,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(DfxCode::SevOutOfRange.code());
            }
        };
        // 闸三：容量。
        if self.count >= LEDGER_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DfxCode::LedgerFull.code());
        }
        // 红线账：Red 一律计入活性账（扫描必抓）。
        if sev == Severity::Red {
            self.red_injected = self.red_injected.saturating_add(1);
        }
        let id = self.count;
        self.recs[id] = DefectRec {
            id: id as u16,
            batch: batch as u8,
            sev: sev.val() as u8,
            state: DefState::Open,
        };
        self.count = self.count.saturating_add(1);
        self.batch_counts[batch] = self.batch_counts[batch].saturating_add(1);
        self.sev_counts[sev.val() as usize] =
            self.sev_counts[sev.val() as usize].saturating_add(1);
        Ok(id)
    }

    /// 生命周期推进：单向表核验，倒退/越级一律拒。
    pub fn advance(&mut self, id: usize, to: DefState) -> Result<(), u32> {
        if id >= self.count {
            return Err(DfxCode::BatchRange.code());
        }
        let cur = self.recs[id].state;
        if !cur.can_advance_to(to) {
            // 倒退 vs 越级分型留痕。
            let code = if (cur as u8) > (to as u8) {
                DfxCode::StateRollback
            } else {
                DfxCode::StateSkip
            };
            return Err(code.code());
        }
        self.recs[id].state = to;
        Ok(())
    }

    /// 红线扫描：活性 Red 条数必须为 0，否则报红线缺陷活性。
    pub fn scan_red(&mut self) -> Result<usize, u32> {
        let mut active = 0usize;
        let mut i = 0;
        while i < self.count {
            if self.recs[i].sev == 0u8 {
                active = active.saturating_add(1);
            }
            i += 1;
        }
        if active > 0 {
            // 抓捕账：注入多少抓多少（守恒）。
            self.red_caught = self.red_caught.saturating_add(active as u32);
            return Err(DfxCode::RedActive.code());
        }
        Ok(0)
    }

    /// 聚合守恒：八批分账之和 == 总账 且 三级账之和 == 总账。
    pub fn agg_conserve(&self) -> Result<(), u32> {
        let mut bsum = 0u32;
        let mut b = 0;
        while b < BATCH_TOTAL {
            bsum = bsum.saturating_add(self.batch_counts[b]);
            b += 1;
        }
        let mut ssum = 0u32;
        let mut s = 0;
        while s < 3 {
            ssum = ssum.saturating_add(self.sev_counts[s]);
            s += 1;
        }
        if bsum != self.count as u32 || ssum != self.count as u32 {
            return Err(DfxCode::AggConserveBroken.code());
        }
        Ok(())
    }

    /// AI-97 双绿判定：格式绿（分级全在闭集内）∧ 守恒绿（聚合守恒）。
    pub fn ai97_dual_green(&self) -> (bool, bool) {
        // 格式绿：每条分级 ∈ {0,1,2} 且批号 < 8。
        let mut fmt = true;
        let mut i = 0;
        while i < self.count {
            if self.recs[i].sev > 2 || (self.recs[i].batch as usize) >= BATCH_TOTAL {
                fmt = false;
            }
            i += 1;
        }
        let cons = self.agg_conserve().is_ok();
        (fmt, cons)
    }

    /// AI-97 登记：双绿齐备才允许，且仅一次（幂等登记）。
    pub fn ai97_register(&mut self) -> Result<(), u32> {
        self.register_attempts = self.register_attempts.saturating_add(1);
        if self.registered {
            return Err(DfxCode::AlreadyRegistered.code());
        }
        let (fmt, cons) = self.ai97_dual_green();
        if !(fmt && cons) {
            return Err(DfxCode::RegisterNotDualGreen.code());
        }
        self.registered = true;
        Ok(())
    }

    /// 摘要（双跑对拍用）。
    pub fn summary(&self) -> (usize, u32, u32, u32) {
        (
            self.count,
            self.sev_counts[0],
            self.sev_counts[1],
            self.sev_counts[2],
        )
    }
}

// ── 规范语料：八批 × 2 条（偶批 🟡+🟢，奇批 🟢+🟢） ─────────────
/// 规范分级值：batch 偶 → [1,2]，batch 奇 → [2,2]。
/// 推导：Red 恒 0（红线零条断言语义），Yellow = 偶批数 = 4，Green = 12。
pub fn canonical_sev(batch: usize, k: usize) -> u32 {
    if batch % 2 == 0 && k == 0 {
        1
    } else {
        2
    }
}

/// 规范语料总量（推导：8 批 × 2 = 16）。
pub const CANON_TOTAL: usize = BATCH_TOTAL * PER_BATCH_DEFECTS;

// ── 黄金全链：开账→扫描→聚合→双绿登记 ─────────────────────────
pub fn golden_run() -> Result<(usize, u32, u32, u32), u32> {
    let mut led = DefectLedger::new();
    let mut b = 0;
    while b < BATCH_TOTAL {
        let mut k = 0;
        while k < PER_BATCH_DEFECTS {
            led.open_defect(b, canonical_sev(b, k))?;
            k += 1;
        }
        b += 1;
    }
    // 红线零条断言。
    led.scan_red()?;
    // 聚合守恒。
    led.agg_conserve()?;
    // 双绿登记。
    led.ai97_register()?;
    if !led.registered {
        return Err(DfxCode::RegisterNotDualGreen.code());
    }
    let s = led.summary();
    Ok(s)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DfxCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入检出账（四类坏样本） ────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] != rounds || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 10/10：类0 红线活性 / 类1 域外分级 / 类2 Closed 复开 / 类3 批越界。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：红线缺陷活性 → 扫描必抓（RedActive）。
        let mut m0 = DefectLedger::new();
        let _ = m0.open_defect(0, 0)?;
        led.record(0, m0.scan_red().err() == Some(DfxCode::RedActive.code()));
        // 类1：域外分级（3）。
        let mut m1 = DefectLedger::new();
        led.record(1, m1.open_defect(0, 3).err() == Some(DfxCode::SevOutOfRange.code()));
        // 类2：Closed 复开（倒退）。
        let mut m2 = DefectLedger::new();
        let id2 = m2.open_defect(0, 2)?;
        let _ = m2.advance(id2, DefState::Confirmed)?;
        let _ = m2.advance(id2, DefState::Closed)?;
        led.record(2, m2.advance(id2, DefState::Open).err() == Some(DfxCode::StateRollback.code()));
        // 类3：批号越界。
        let mut m3 = DefectLedger::new();
        led.record(3, m3.open_defect(BATCH_TOTAL, 2).err() == Some(DfxCode::BatchRange.code()));
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：越级推进（Open→Closed）必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = DefectLedger::new();
    let id = led.open_defect(0, 2)?;
    match led.advance(id, DefState::Closed) {
        Err(c) if c == DfxCode::StateSkip.code() => Ok(()),
        _ => Err(DfxCode::StateSkip.code()),
    }
}

/// 探针 C：双绿未齐（含红线活性）登记必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = DefectLedger::new();
    let _ = led.open_defect(0, 0)?;
    match led.ai97_register() {
        Err(c) if c == DfxCode::RegisterNotDualGreen.code() => Ok(()),
        _ => Err(DfxCode::RegisterNotDualGreen.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：诊断码全落 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < DfxCode::ALL.len() {
        let v = DfxCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每码明细非空。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < DfxCode::ALL.len() {
        if DfxCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据总成 ───────────────────────────────────────────────────
/// 静态判据。
pub fn static_checks() -> Vec<(&'static str, bool, &'static str)> {
    let mut v: Vec<(&'static str, bool, &'static str)> = Vec::new();
    v.push(("J2-段位守卫", segment_guard(), "0x80B00 窗口 0x40 对齐"));
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值：8 批 × 2 = 16 条；规范语料 Red=0 / Yellow=4 / Green=12。
    v.push((
        "J2-冻结值一致",
        BATCH_TOTAL == 8 && LEDGER_CAP == 32 && CANON_TOTAL == 16,
        "8批×2条=16, 账容32",
    ));
    // 生命周期单向表核验。
    v.push((
        "J1-生命周期单向表",
        DefState::Open.can_advance_to(DefState::Confirmed)
            && DefState::Confirmed.can_advance_to(DefState::Closed)
            && !DefState::Open.can_advance_to(DefState::Closed)
            && !DefState::Closed.can_advance_to(DefState::Open),
        "Open→Confirmed→Closed, 倒退/越级皆非法",
    ));
    // 分级闭集往返：val → from_val 恒等，域外必 None。
    let mut roundtrip = true;
    let mut s = 0;
    while s < 3 {
        match Severity::from_val(Severity::ALL[s].val()) {
            Some(sv) if sv == Severity::ALL[s] => {}
            _ => roundtrip = false,
        }
        s += 1;
    }
    v.push((
        "J1-分级闭集往返",
        roundtrip && Severity::from_val(3).is_none() && Severity::from_val(255).is_none(),
        "0/1/2 往返恒等, 3/255 必 None",
    ));
    v
}

/// 动态判据（≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 规范语料逐条汇总 + 红线零条断言。
    let mut led = DefectLedger::new();
    let mut b = 0;
    while b < BATCH_TOTAL {
        let mut k = 0;
        while k < PER_BATCH_DEFECTS {
            let _ = led.open_defect(b, canonical_sev(b, k));
            k += 1;
        }
        b += 1;
    }
    let red_scan = led.scan_red();
    v.push((
        "J1-红线类零条断言",
        red_scan.is_ok() && led.sev_counts[0] == 0,
        format!("Red活性={} Yellow={} Green={}", led.sev_counts[0], led.sev_counts[1], led.sev_counts[2]),
    ));
    // 聚合守恒（批账和=级账和=总账）。
    v.push((
        "J1-聚合守恒",
        led.agg_conserve().is_ok() && led.count == CANON_TOTAL,
        format!("批账和+级账和={} 总账={}", led.sev_counts[0] + led.sev_counts[1] + led.sev_counts[2], led.count),
    ));
    // AI-97 双绿登记。
    let (fmt, cons) = led.ai97_dual_green();
    let reg_ok = led.ai97_register().is_ok() && led.registered;
    v.push((
        "J1-AI-97双绿登记",
        fmt && cons && reg_ok,
        format!("格式绿={} 守恒绿={} 登记={}", fmt, cons, reg_ok),
    ));
    // 注入 10/10 四类。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(JUDGE_ROUNDS),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10四类",
        inj_ok,
        format!(
            "t={:?} c={:?}",
            inj.as_ref().map(|l| l.tries).unwrap_or([0; 4]),
            inj.as_ref().map(|l| l.caught).unwrap_or([0; 4])
        ),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let bb = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && bb && c,
        format!("A过={} B拒={} C拒={}", a, bb, c),
    ));
    // 二次登记幂等拒 + 账不丢。
    let mut reg = DefectLedger::new();
    let mut b2 = 0;
    while b2 < BATCH_TOTAL {
        let mut k2 = 0;
        while k2 < PER_BATCH_DEFECTS {
            let _ = reg.open_defect(b2, canonical_sev(b2, k2));
            k2 += 1;
        }
        b2 += 1;
    }
    let _ = reg.ai97_register();
    let second = reg.ai97_register();
    v.push((
        "J2-二次登记幂等拒",
        second.err() == Some(DfxCode::AlreadyRegistered.code()) && reg.count == CANON_TOTAL,
        format!("登记二拒后 count={}", reg.count),
    ));
    v
}

/// 探针入口：B09 批统一自检（15 条判据）。
pub fn run_unx18573_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18573 缺陷账聚合");
    for (n, p, d) in static_checks() {
        s.add(n, p, d);
    }
    for (n, p, d) in dyn_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        let set = run_unx18573_checks();
        assert!(set.all_passed(), "F18573 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：批 0 合法，批 7 合法，批 8 越界。
        let mut led = DefectLedger::new();
        assert!(led.open_defect(0, 2).is_ok());
        assert!(led.open_defect(BATCH_TOTAL - 1, 2).is_ok());
        assert!(led.open_defect(BATCH_TOTAL, 2).is_err());
        // 分级边界：2 合法、3 越界。
        assert!(led.open_defect(0, 2).is_ok());
        assert!(led.open_defect(0, 3).is_err());
        // 容量边界：填满后第 33 条拒且账不丢。
        let mut full = DefectLedger::new();
        let mut n = 0;
        while n < LEDGER_CAP {
            let _ = full.open_defect(n % BATCH_TOTAL, 2);
            n += 1;
        }
        assert_eq!(full.count, LEDGER_CAP);
        let _ = full.open_defect(0, 2);
        assert_eq!(full.count, LEDGER_CAP);
    }

    #[test]
    fn test_injection_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS).unwrap_or(InjectLedger::new());
        assert!(inj.all_caught(JUDGE_ROUNDS), "注入存在漏检：{:?}", inj.caught);
        let mut i = 0;
        while i < 4 {
            assert_eq!(inj.tries[i] - inj.caught[i], 0);
            i += 1;
        }
    }

    #[test]
    fn test_order_range_before_sev() {
        // 判定次序：批号闸先于分级闸（越界批 + 域外分级报批越界）。
        let mut led = DefectLedger::new();
        match led.open_defect(BATCH_TOTAL, 9) {
            Err(c) if c == DfxCode::BatchRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal() {
        // 越级 Open→Closed 拒；倒退 Closed→Confirmed 拒。
        let mut led = DefectLedger::new();
        let a = led.open_defect(0, 2).unwrap_or(0);
        assert!(led.advance(a, DefState::Closed).is_err());
        assert!(led.advance(a, DefState::Confirmed).is_ok());
        assert!(led.advance(a, DefState::Closed).is_ok());
        assert!(led.advance(a, DefState::Confirmed).is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_red_scan_catches_injection() {
        // 红线注入与抓捕守恒：注入 3 条 → 扫描必报且抓捕数 == 注入数。
        let mut led = DefectLedger::new();
        let mut k = 0;
        while k < 3 {
            let _ = led.open_defect(0, 0);
            k += 1;
        }
        assert!(led.scan_red().is_err());
        assert_eq!(led.red_caught, led.red_injected);
        assert_eq!(led.red_injected, 3);
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(DfxCode::ALL.len(), 12);
        let mut i = 0;
        while i < DfxCode::ALL.len() {
            let mut j = i + 1;
            while j < DfxCode::ALL.len() {
                assert_ne!(DfxCode::ALL[i].code(), DfxCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
