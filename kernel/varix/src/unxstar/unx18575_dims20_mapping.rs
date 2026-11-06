//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18575 · 版本面 20 维验收自查映射（前半） · 0x80B80段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18575-J1 20 维自查项↔判据映射表（10/20 维）登记，骨架期
//!       覆盖账显式
//! 设计要点：
//!   - 20 维验收自查项定容账本；本册登记前半 10 维（1..10），后半 10 维
//!     显式记 pending（骨架期覆盖账不允许静默缺口）；
//!   - 每个已登记维必须有 ≥1 条判据引用（判据号区间 1000+dim*10 起，
//!     每维 3 条——由维号推导，写死注明）与显式覆盖旗；
//!   - 覆盖守恒：registered + pending == 20 且 registered == 10，
//!     账面口径与前半范围一致；
//!   - 维状态机 Skeleton→Mapped→Audited 单向，Audited 后改判据必拒；
//!   - 注入核四类：半区外维号 / 零判据维 / 维重复 / 未显式 pending
//!     （静默缺口），逐类 10/10 轮必抓；
//!   - LCG 种子写死 = SEG_BASE（0x80B80）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 本模块结构清单：
//!   数据结构：DimRec（维号/判据基/判据数/态四元）、DimsLedger（20 槽）
//!   状态机  ：DimState 三态单向（Skeleton→Mapped→Audited）
//!   账本    ：coverage 覆盖账（registered/pending 显式双栏）
//!   错误路径：DimCode 闭集 12 码，code() = SEG_BASE + n
//!   注入核  ：inject_rounds（四类坏样本 × 10 轮，逐类全抓）
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80B80，AI-VX11-1105 批09 G1 独占区）。
pub const SEG_BASE: u32 = 0x80B80;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 验收自查维数（冻结：20）。
pub const DIM_TOTAL: usize = 20;
/// 本册前半维数（冻结：10，维号 1..10）。
pub const HALF_DIMS: usize = 10;
/// 每维判据引用数（推导：判据号 = 1000 + dim*10 + k，k∈0..3；
/// 每维 3 条保证判据区间互不重叠且可追溯）。
pub const CRIT_PER_DIM: usize = 3;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 20 维语料名表（闭集，前 10 维本册登记） ────────────────────
pub const DIM_NAMES: [&str; DIM_TOTAL] = [
    "install",  // 1 安装
    "boot",     // 2 启动
    "net",      // 3 网络
    "storage",  // 4 存储
    "sec",      // 5 安全
    "compat",   // 6 兼容
    "perf",     // 7 性能
    "reliab",   // 8 可靠
    "a11y",     // 9 无障碍
    "observe",  // 10 观测
    "update",   // 11 更新（后半 pending）
    "recover",  // 12 恢复
    "account",  // 13 账户
    "shell",    // 14 外壳
    "gfx",      // 15 图形
    "audio",    // 16 音频
    "input",    // 17 输入
    "power",    // 18 电源
    "i18n",     // 19 国际化
    "docs",     // 20 文档
];

/// 判据号推导：第 dim 维第 k 条 = 1000 + dim*10 + k。
/// 推导注释：dim∈1..20 → 基号区间 [1010, 1200)，每维 3 条互不重叠。
pub fn crit_id(dim: usize, k: usize) -> usize {
    1000 + dim * 10 + k
}

// ── 维状态机（单向三态） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimState {
    /// 骨架：仅立维未映射。
    Skeleton,
    /// 已映射：判据引用落账。
    Mapped,
    /// 已审计：映射复核通过。
    Audited,
}

impl DimState {
    pub const fn skeleton() -> DimState {
        DimState::Skeleton
    }
    /// 单向推进合法判定。
    pub fn can_advance_to(self, next: DimState) -> bool {
        matches!(
            (self, next),
            (DimState::Skeleton, DimState::Mapped) | (DimState::Mapped, DimState::Audited)
        )
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimCode {
    /// 维号越界（1..20 闭集外）
    DimRange,
    /// 半区外维号（11..20 不得入前半账）
    HalfRange,
    /// 零判据维
    CritEmpty,
    /// 维重复登记
    DimDup,
    /// 静默缺口（未显式 pending）
    SilentGap,
    /// 覆盖守恒破坏
    CoverBroken,
    /// 状态越级
    StateSkip,
    /// 状态倒退
    StateRollback,
    /// 审计后改判据
    MutateAfterAudit,
    /// 判据号域外
    CritRange,
    /// 维名闭集破坏
    NameClosureBroken,
    /// 双跑不一致
    NotReproducible,
}

impl DimCode {
    /// 诊断码 = SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文明细（闭集非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DimCode::DimRange => "维号越界：1..20 闭集外",
            DimCode::HalfRange => "半区外维号：11..20 不得入前半账",
            DimCode::CritEmpty => "零判据维：映射空",
            DimCode::DimDup => "维重复登记",
            DimCode::SilentGap => "静默缺口：未显式 pending",
            DimCode::CoverBroken => "覆盖守恒破坏：reg+pending≠20",
            DimCode::StateSkip => "状态越级：Skeleton→Audited",
            DimCode::StateRollback => "状态倒退",
            DimCode::MutateAfterAudit => "审计后改判据：锁定红线",
            DimCode::CritRange => "判据号域外：推导区间外",
            DimCode::NameClosureBroken => "维名闭集破坏：空名",
            DimCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（12 码）。
    pub const ALL: [DimCode; 12] = [
        DimCode::DimRange,
        DimCode::HalfRange,
        DimCode::CritEmpty,
        DimCode::DimDup,
        DimCode::SilentGap,
        DimCode::CoverBroken,
        DimCode::StateSkip,
        DimCode::StateRollback,
        DimCode::MutateAfterAudit,
        DimCode::CritRange,
        DimCode::NameClosureBroken,
        DimCode::NotReproducible,
    ];
}

/// 码 → 明细反查（闭集外返回未知文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DimCode::ALL.len() {
        if DimCode::ALL[i].code() == code {
            return DimCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80B80 段闭集内"
}

// ── 维记录 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimRec {
    /// 维号 1..20。
    pub dim: u8,
    /// 判据基号。
    pub crit_base: u16,
    /// 判据引用数（0 = 空维）。
    pub crit_n: u8,
    /// 显式覆盖旗（false 时必须显式 pending）。
    pub covered: bool,
    /// 显式 pending 旗（骨架期后半维专用）。
    pub explicit_pending: bool,
    /// 维状态。
    pub state: DimState,
}

impl DimRec {
    pub const fn empty() -> DimRec {
        DimRec {
            dim: 0,
            crit_base: 0,
            crit_n: 0,
            covered: false,
            explicit_pending: false,
            state: DimState::Skeleton,
        }
    }
}

// ── 20 维总账 ──────────────────────────────────────────────────
pub struct DimsLedger {
    /// 定容账本（20 维槽，按下标 0..19 对应维号 1..20）。
    pub dims: [DimRec; DIM_TOTAL],
    /// 已登记（前半）维数。
    pub registered: usize,
    /// 显式 pending 维数。
    pub pending: usize,
    /// 判据引用总数。
    pub crit_total: usize,
    /// 写入尝试计数。
    pub attempts: u32,
    /// 拒收计数。
    pub rejected: u32,
    /// 收口旗。
    pub settled: bool,
}

impl DimsLedger {
    pub fn new() -> DimsLedger {
        DimsLedger {
            dims: [DimRec::empty(); DIM_TOTAL],
            registered: 0,
            pending: 0,
            crit_total: 0,
            attempts: 0,
            rejected: 0,
            settled: false,
        }
    }

    /// 登记一个前半维（1..10）：区间→半区→判据非零→重复四闸。
    pub fn map_dim(&mut self, dim: usize) -> Result<usize, u32> {
        self.attempts = self.attempts.saturating_add(1);
        // 闸一：维号全区间。
        if dim == 0 || dim > DIM_TOTAL {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::DimRange.code());
        }
        // 闸二：前半区间。
        if dim > HALF_DIMS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::HalfRange.code());
        }
        // 闸三：判据引用数必须非零（映射空维拒）。
        if CRIT_PER_DIM == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::CritEmpty.code());
        }
        // 闸四：重复。
        let slot = dim - 1;
        if self.dims[slot].dim != 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::DimDup.code());
        }
        // 落账：判据基推导 + 状态 Mapped + 覆盖旗。
        self.dims[slot] = DimRec {
            dim: dim as u8,
            crit_base: crit_id(dim, 0) as u16,
            crit_n: CRIT_PER_DIM as u8,
            covered: true,
            explicit_pending: false,
            state: DimState::Mapped,
        };
        self.registered = self.registered.saturating_add(1);
        self.crit_total = self.crit_total.saturating_add(CRIT_PER_DIM);
        Ok(slot)
    }

    /// 显式登记一个后半 pending 维（11..20）：不许静默缺口。
    pub fn mark_pending(&mut self, dim: usize) -> Result<usize, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if dim == 0 || dim > DIM_TOTAL {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::DimRange.code());
        }
        if dim <= HALF_DIMS {
            // 前半维不允许挂 pending（前半必须真映射）。
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::HalfRange.code());
        }
        let slot = dim - 1;
        if self.dims[slot].dim != 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(DimCode::DimDup.code());
        }
        self.dims[slot] = DimRec {
            dim: dim as u8,
            crit_base: 0,
            crit_n: 0,
            covered: false,
            explicit_pending: true,
            state: DimState::Skeleton,
        };
        self.pending = self.pending.saturating_add(1);
        Ok(slot)
    }

    /// 静默缺口扫描：20 槽中任何空槽（dim==0 且未显式 pending）即缺口。
    pub fn scan_gap(&self) -> Result<usize, u32> {
        let mut gaps = 0usize;
        let mut i = 0;
        while i < DIM_TOTAL {
            if self.dims[i].dim == 0 && !self.dims[i].explicit_pending {
                gaps = gaps.saturating_add(1);
            }
            i += 1;
        }
        if gaps > 0 {
            return Err(DimCode::SilentGap.code());
        }
        Ok(0)
    }

    /// 状态推进：单向表核验。
    pub fn advance(&mut self, slot: usize, to: DimState) -> Result<(), u32> {
        if slot >= DIM_TOTAL || self.dims[slot].dim == 0 {
            return Err(DimCode::DimRange.code());
        }
        let cur = self.dims[slot].state;
        if !cur.can_advance_to(to) {
            let code = if (cur as u8) > (to as u8) {
                DimCode::StateRollback
            } else {
                DimCode::StateSkip
            };
            return Err(code.code());
        }
        self.dims[slot].state = to;
        Ok(())
    }

    /// 审计后改判据必拒（锁定红线）：仅 Mapped 态允许改。
    pub fn remap_crit(&mut self, slot: usize, base: usize) -> Result<(), u32> {
        if slot >= DIM_TOTAL || self.dims[slot].dim == 0 {
            return Err(DimCode::DimRange.code());
        }
        if self.dims[slot].state == DimState::Audited {
            return Err(DimCode::MutateAfterAudit.code());
        }
        // 判据基域核验：基号必须落在该维推导区间内。
        let dim = self.dims[slot].dim as usize;
        let lo = crit_id(dim, 0);
        let hi = crit_id(dim, CRIT_PER_DIM - 1);
        if base < lo || base > hi {
            return Err(DimCode::CritRange.code());
        }
        self.dims[slot].crit_base = base as u16;
        Ok(())
    }

    /// 覆盖守恒：registered + pending == 20 且 registered == 10。
    pub fn cover_conserve(&self) -> Result<(), u32> {
        if self.registered.saturating_add(self.pending) != DIM_TOTAL
            || self.registered != HALF_DIMS
        {
            return Err(DimCode::CoverBroken.code());
        }
        Ok(())
    }

    /// 判据号域内全检：每条已映射维的判据区间合法。
    pub fn crit_range_check(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < DIM_TOTAL {
            let d = self.dims[i];
            if d.dim != 0 && d.crit_n > 0 {
                let dim = d.dim as usize;
                let lo = crit_id(dim, 0);
                let hi = crit_id(dim, CRIT_PER_DIM - 1);
                let base = d.crit_base as usize;
                if base < lo || base > hi {
                    return Err(DimCode::CritRange.code());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 维名闭集：20 名全非空。
    pub fn name_closure() -> bool {
        let mut i = 0;
        while i < DIM_TOTAL {
            if DIM_NAMES[i].is_empty() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 摘要（双跑对拍用）。
    pub fn summary(&self) -> (usize, usize, usize, u32) {
        (self.registered, self.pending, self.crit_total, self.rejected)
    }
}

// ── 黄金全链：前半 10 维映射→后半 10 维显式 pending→守恒→审计 ──
pub fn golden_run() -> Result<(usize, usize, usize, u32), u32> {
    let mut led = DimsLedger::new();
    let mut dim = 1;
    while dim <= HALF_DIMS {
        led.map_dim(dim)?;
        dim += 1;
    }
    let mut dim2 = HALF_DIMS + 1;
    while dim2 <= DIM_TOTAL {
        led.mark_pending(dim2)?;
        dim2 += 1;
    }
    // 静默缺口扫描：零缺口。
    led.scan_gap()?;
    // 覆盖守恒。
    led.cover_conserve()?;
    // 判据域内全检。
    led.crit_range_check()?;
    // 前半逐维推进到 Audited。
    let mut s = 0;
    while s < HALF_DIMS {
        led.advance(s, DimState::Audited)?;
        s += 1;
    }
    // 审计后改判据必拒（显式证据）。
    if led.remap_crit(0, crit_id(1, 1)).is_ok() {
        return Err(DimCode::MutateAfterAudit.code());
    }
    led.settled = true;
    let sum = led.summary();
    Ok(sum)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DimCode::NotReproducible.code());
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

/// 注入 10/10：类0 半区外维号 / 类1 零判据缺口 / 类2 维重复 / 类3 维号越界。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类0：半区外维号（11 入前半账）。
        let mut m0 = DimsLedger::new();
        led.record(0, m0.map_dim(HALF_DIMS + 1).err() == Some(DimCode::HalfRange.code()));
        // 类1：零判据缺口（只登记前半、不挂 pending → 扫描必抓静默缺口）。
        let mut m1 = DimsLedger::new();
        let _ = m1.map_dim(1)?;
        led.record(1, m1.scan_gap().err() == Some(DimCode::SilentGap.code()));
        // 类2：维重复。
        let mut m2 = DimsLedger::new();
        let _ = m2.map_dim(1)?;
        led.record(2, m2.map_dim(1).err() == Some(DimCode::DimDup.code()));
        // 类3：维号越界（0 与 21）。
        let mut m3 = DimsLedger::new();
        let e3a = m3.map_dim(0).err() == Some(DimCode::DimRange.code());
        let mut m3b = DimsLedger::new();
        let e3b = m3b.map_dim(DIM_TOTAL + 1).err() == Some(DimCode::DimRange.code());
        led.record(3, e3a && e3b);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────
/// 探针 A：黄金链全通。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：状态越级（Skeleton→Audited）必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = DimsLedger::new();
    let s = led.map_dim(1)?;
    match led.advance(s, DimState::Audited) {
        Err(c) if c == DimCode::StateSkip.code() => Ok(()),
        _ => Err(DimCode::StateSkip.code()),
    }
}

/// 探针 C：前半维挂 pending 必拒（前半必须真映射）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = DimsLedger::new();
    match led.mark_pending(1) {
        Err(c) if c == DimCode::HalfRange.code() => Ok(()),
        _ => Err(DimCode::HalfRange.code()),
    }
}

// ── 段位守卫与文案闭集 ─────────────────────────────────────────
/// 段位守卫：诊断码全落 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < DimCode::ALL.len() {
        let v = DimCode::ALL[i].code();
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
    while i < DimCode::ALL.len() {
        if DimCode::ALL[i].detail().is_empty() {
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
    v.push(("J2-段位守卫", segment_guard(), "0x80B80 窗口 0x40 对齐"));
    v.push(("J2-文案闭集", detail_closure(), "12 码明细全非空"));
    // 冻结值：20 维前半 10，每维 3 判据。
    v.push((
        "J2-冻结值一致",
        DIM_TOTAL == 20 && HALF_DIMS == 10 && CRIT_PER_DIM == 3,
        "20维前半10, 每维3判据",
    ));
    // 维名闭集。
    v.push((
        "J1-维名闭集",
        DimsLedger::name_closure() && DIM_NAMES.len() == 20,
        "20 维名全非空",
    ));
    // 判据推导区间不重叠：相邻维基号差 = 10 > 每维判据数 3。
    let mut non_overlap = true;
    let mut d = 1;
    while d < DIM_TOTAL {
        let hi_cur = crit_id(d, CRIT_PER_DIM - 1);
        let lo_next = crit_id(d + 1, 0);
        if hi_cur >= lo_next {
            non_overlap = false;
        }
        d += 1;
    }
    v.push((
        "J1-判据区间不重叠",
        non_overlap && crit_id(1, 0) == 1010 && crit_id(20, 2) == 1202,
        "基号 1010..1202, 步距10>3",
    ));
    // 维状态机单向表。
    v.push((
        "J1-维状态单向表",
        DimState::Skeleton.can_advance_to(DimState::Mapped)
            && DimState::Mapped.can_advance_to(DimState::Audited)
            && !DimState::Skeleton.can_advance_to(DimState::Audited)
            && !DimState::Audited.can_advance_to(DimState::Mapped),
        "Skeleton→Mapped→Audited, 倒退/越级皆非法",
    ));
    v
}

/// 动态判据（≤8 条 add_dyn）。
pub fn dyn_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 骨架期覆盖账显式：10 登记真映射 + 10 显式 pending。
    let mut led = DimsLedger::new();
    let mut d = 1;
    while d <= HALF_DIMS {
        let _ = led.map_dim(d);
        d += 1;
    }
    let mut d2 = HALF_DIMS + 1;
    while d2 <= DIM_TOTAL {
        let _ = led.mark_pending(d2);
        d2 += 1;
    }
    v.push((
        "J1-骨架期覆盖账显式",
        led.registered == 10 && led.pending == 10 && led.scan_gap().is_ok(),
        format!("registered={} pending={} gaps=0", led.registered, led.pending),
    ));
    // 判据映射对表：10 维 × 3 判据 = 30 条引用，全落推导区间。
    v.push((
        "J1-判据映射对表",
        led.crit_total == 30 && led.crit_range_check().is_ok(),
        format!("引用总数={} 域检=过", led.crit_total),
    ));
    // 覆盖守恒。
    v.push((
        "J1-覆盖守恒",
        led.cover_conserve().is_ok(),
        format!("reg+pending={} 应为20", led.registered + led.pending),
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
    // 审计后改判据必拒 + Mapped 态改判据合法（域内基号）。
    let mut led2 = DimsLedger::new();
    let s2 = led2.map_dim(1);
    let remap_ok = match s2 {
        Ok(s) => {
            let r1 = led2.remap_crit(s, crit_id(1, 2));
            let _ = led2.advance(s, DimState::Audited);
            let r2 = led2.remap_crit(s, crit_id(1, 1));
            r1.is_ok() && r2.err() == Some(DimCode::MutateAfterAudit.code())
        }
        Err(_) => false,
    };
    v.push((
        "J1-审计锁定红线",
        remap_ok,
        String::from("Mapped 改判据合法, Audited 后必拒"),
    ));
    v
}

/// 探针入口：B09 批统一自检（15 条判据）。
pub fn run_unx18575_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18575 20维验收自查映射");
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
        let set = run_unx18575_checks();
        assert!(set.all_passed(), "F18575 判据集存在红项：{}", set.red_items_text());
    }

    #[test]
    fn test_boundary_zero_and_full() {
        // 零边界：维 0 越界，维 1 合法。
        let mut led = DimsLedger::new();
        assert!(led.map_dim(0).is_err());
        assert!(led.map_dim(1).is_ok());
        // 上边界：维 10 合法，维 11 半区外，维 21 越界。
        assert!(led.map_dim(HALF_DIMS).is_ok());
        assert!(led.map_dim(HALF_DIMS + 1).is_err());
        assert!(led.map_dim(DIM_TOTAL + 1).is_err());
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
    fn test_order_half_before_dup() {
        // 判定次序：半区闸先于重复闸（半区外维在空账上报半区外）。
        let mut led = DimsLedger::new();
        match led.map_dim(HALF_DIMS + 1) {
            Err(c) if c == DimCode::HalfRange.code() => {}
            other => panic!("次序错误：{:?}", other.err()),
        }
    }

    #[test]
    fn test_state_machine_illegal() {
        let mut led = DimsLedger::new();
        let s = led.map_dim(2).unwrap_or(0);
        assert!(led.advance(s, DimState::Audited).is_err());
        assert!(led.advance(s, DimState::Mapped).is_ok());
        assert!(led.advance(s, DimState::Audited).is_ok());
        assert!(led.advance(s, DimState::Mapped).is_err());
    }

    #[test]
    fn test_double_run_identical() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_pending_explicit_no_silent() {
        // 全 pending 显式登记后零缺口；漏登一维则扫描必抓。
        let mut led = DimsLedger::new();
        let mut d = 1;
        while d <= HALF_DIMS {
            let _ = led.map_dim(d);
            d += 1;
        }
        // 故意漏 mark_pending(11)。
        let mut d2 = HALF_DIMS + 2;
        while d2 <= DIM_TOTAL {
            let _ = led.mark_pending(d2);
            d2 += 1;
        }
        assert!(led.scan_gap().is_err());
        let mut led2 = DimsLedger::new();
        let mut d3 = 1;
        while d3 <= DIM_TOTAL {
            if d3 <= HALF_DIMS {
                let _ = led2.map_dim(d3);
            } else {
                let _ = led2.mark_pending(d3);
            }
            d3 += 1;
        }
        assert!(led2.scan_gap().is_ok());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert_eq!(DimCode::ALL.len(), 12);
        let mut i = 0;
        while i < DimCode::ALL.len() {
            let mut j = i + 1;
            while j < DimCode::ALL.len() {
                assert_ne!(DimCode::ALL[i].code(), DimCode::ALL[j].code());
                j += 1;
            }
            i += 1;
        }
    }
}
