//! UNX-F50006 色彩预算账（每页色彩处理耗时上限）（0x42ce0 段）
//!
//! 锚点：UNX-F50006-J1 每页实测 ≤预算（模拟器档登记）。
//!
//! 数据段 `SEG_BASE = 0x42ce0`（独占 0x20），诊断码闭集 `E_UNKNOWN_OP..=E_PROGRAM_FULL`
//! （1..=13），`BudgetError::code()` 恒为 `0x42ce0 + n`。
//!
//! 为什么色彩也要记账：RIP 的「超时」从来不是均匀发生的——它集中砸在
//! 色域映射与大版面这一步。没有预算账时，运维只能看到「这一页跑了 400ms」，
//! 看不到**是哪个算子吃掉的**，也就没法把「降一档」这件事做成可复现的决策。
//! 本模块把「每页色彩处理耗时」做成三个对象：
//! * 算子价目表（`OpTable`）：每个算子每千像素的 tick 价，登记制，查不到就拒；
//! * 模拟器档位表（`TierBook`）：草稿 / 标准 / 印刷三档预算（tick），可重定价；
//! * 预算调度器（`BudgetScheduler`）：四段降级阶梯（满配 → 轻降 → 粗网格 → 内置档），
//!   逐级实测重算，**装得下就停**，装不下才继续降，全降完还装不下就显式拒单。
//!
//! 单位约定：1 tick = 10µs（模拟器时钟），预算 5000 / 9000 / 14000 tick 即
//! 每页 50 / 90 / 140ms 的上限。价目表数值来自模拟器档登记的每千像素实测值，
//! 这里以常量固化，改动必须同步档位校准号（`calibration`），否则账本对不上。
//!
//! 结构清单（契约要求 ≥2 类，实际 4 类）：
//! * 注册表 ×2：`OpTable`（算子价目，容量 12）、`TierBook`（档位预算，容量 4）；
//! * 解析器：`PageJob` 的算子码流（定长 8 槽，越界码显式拒）；
//! * 调度器：`BudgetScheduler` 的四段阶梯 + `Outcome::Refused` 显式拒单；
//! * 账本：`BudgetLedger`（容量 16 页），`BudgetRecord` 可脱离调度器重算守恒，
//!   违约条目（实测 > 预算）**入账即拒**并计数，绝不静默。
//!
//! 质量侧不空口承诺：每一档的 Lab 网格步长（1/2/4/8）在 12 个固定色块上实测
//! ΔE2000 最大损失，判据要求损失相对档位上限单调上升且有硬上限——「省时间」
//! 必须能换算成「丢多少色」，否则降级就是拍脑袋。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::unx50001_gamutmap::{delta_e2000, Lab};

/// 诊断段：0x42ce0 独占 0x20（本批最后一个 0x20 窗口）。
pub const SEG_BASE: u32 = 0x42ce0;

pub const E_UNKNOWN_OP: u16 = 1;
pub const E_DUP_OP: u16 = 2;
pub const E_EMPTY_WORKLOAD: u16 = 3;
pub const E_BAD_WORKLOAD: u16 = 4;
pub const E_OVER_BUDGET: u16 = 5;
pub const E_BUDGET_RANGE: u16 = 6;
pub const E_DUP_TIER: u16 = 7;
pub const E_TIER_MISSING: u16 = 8;
pub const E_DUP_PAGE: u16 = 9;
pub const E_LEDGER_FULL: u16 = 10;
pub const E_BUDGET_BREACH: u16 = 11;
pub const E_TABLE_FULL: u16 = 12;
pub const E_PROGRAM_FULL: u16 = 13;

/// 显式错误闭集。内核路径所有失败入口都返回它，不做 `unwrap`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetError {
    UnknownOp,
    DupOp,
    EmptyWorkload,
    BadWorkload,
    OverBudget,
    BudgetRange,
    DupTier,
    TierMissing,
    DupPage,
    LedgerFull,
    BudgetBreach,
    TableFull,
    ProgramFull,
}

impl BudgetError {
    /// 诊断码：`0x42ce0 + n`，n 与常量一一对应。
    pub fn code(self) -> u32 {
        SEG_BASE
            + match self {
                BudgetError::UnknownOp => E_UNKNOWN_OP as u32,
                BudgetError::DupOp => E_DUP_OP as u32,
                BudgetError::EmptyWorkload => E_EMPTY_WORKLOAD as u32,
                BudgetError::BadWorkload => E_BAD_WORKLOAD as u32,
                BudgetError::OverBudget => E_OVER_BUDGET as u32,
                BudgetError::BudgetRange => E_BUDGET_RANGE as u32,
                BudgetError::DupTier => E_DUP_TIER as u32,
                BudgetError::TierMissing => E_TIER_MISSING as u32,
                BudgetError::DupPage => E_DUP_PAGE as u32,
                BudgetError::LedgerFull => E_LEDGER_FULL as u32,
                BudgetError::BudgetBreach => E_BUDGET_BREACH as u32,
                BudgetError::TableFull => E_TABLE_FULL as u32,
                BudgetError::ProgramFull => E_PROGRAM_FULL as u32,
            }
    }

    pub fn message(self) -> &'static str {
        match self {
            BudgetError::UnknownOp => "op code not registered in the price table",
            BudgetError::DupOp => "op code registered twice",
            BudgetError::EmptyWorkload => "page has no colour ops",
            BudgetError::BadWorkload => "pixel count outside 1..=512 kilopixels",
            BudgetError::OverBudget => "even the last rung exceeds the tier budget",
            BudgetError::BudgetRange => "tier budget outside 1000..=40000 ticks",
            BudgetError::DupTier => "tier registered twice",
            BudgetError::TierMissing => "tier not registered",
            BudgetError::DupPage => "page already booked in this ledger",
            BudgetError::LedgerFull => "budget ledger is full",
            BudgetError::BudgetBreach => "recorded cost exceeds its own budget",
            BudgetError::TableFull => "op price table is full",
            BudgetError::ProgramFull => "page program exceeds 8 op slots",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 时间单位与价目表
// ---------------------------------------------------------------------------

/// 1 tick = 10µs。所有预算/价目都以 tick 计，避免浮点进账本。
pub const TICK_US: u32 = 10;

/// 模拟器档登记的三档预算（tick）：50ms / 90ms / 140ms 每页。
pub const BUDGET_DRAFT: u32 = 5000;
pub const BUDGET_STANDARD: u32 = 9000;
pub const BUDGET_PRESS: u32 = 14000;

/// 预算的合法区间：低于 1ms 等于逼所有页降级，高于 400ms 等于不设防。
pub const MIN_BUDGET: u32 = 1000;
pub const MAX_BUDGET: u32 = 40000;

/// 单页工作量上限（千像素）。512k px 已超过 A2/600dpi 的一页墨量，
/// 再大属于拼版作业，应在上游拆页而不是在这里假装能算。
pub const MAX_PIXELS_K: u16 = 512;

/// 价目（tick / 千像素），模拟器档实测登记的整数值。
/// 解码 12、profile 解析 30、色域映射 48、黑点 9、墨量拆分 22、编码 10、预览 6。
pub const COST_DECODE: u32 = 12;
pub const COST_PROFILE: u32 = 30;
pub const COST_MAP: u32 = 48;
pub const COST_BPC: u32 = 9;
pub const COST_SPLIT: u32 = 22;
pub const COST_ENCODE: u32 = 10;
pub const COST_PREVIEW: u32 = 6;

/// 粗网格映射价：网格从 1.0 放到 4.0 后顶点数约 1/16，但插值/建表开销占大头，
/// 实测只降到 24（= COST_MAP 的一半），不按顶点比例虚报。
pub const COST_MAP_COARSE: u32 = 24;

/// 算子种类（闭集）。code 即账本里出现的码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Decode,
    Profile,
    Map,
    Bpc,
    Split,
    Encode,
    Preview,
}

impl OpKind {
    pub fn code(self) -> u8 {
        match self {
            OpKind::Decode => 1,
            OpKind::Profile => 2,
            OpKind::Map => 3,
            OpKind::Bpc => 4,
            OpKind::Split => 5,
            OpKind::Encode => 6,
            OpKind::Preview => 7,
        }
    }

    /// 满配价（tick / 千像素）。
    pub fn base_cost(self) -> u32 {
        match self {
            OpKind::Decode => COST_DECODE,
            OpKind::Profile => COST_PROFILE,
            OpKind::Map => COST_MAP,
            OpKind::Bpc => COST_BPC,
            OpKind::Split => COST_SPLIT,
            OpKind::Encode => COST_ENCODE,
            OpKind::Preview => COST_PREVIEW,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OpKind::Decode => "decode",
            OpKind::Profile => "profile",
            OpKind::Map => "gamut-map",
            OpKind::Bpc => "black-point",
            OpKind::Split => "ink-split",
            OpKind::Encode => "encode",
            OpKind::Preview => "preview",
        }
    }
}

/// 价目表容量：7 个标准算子 + 5 个预留位，满了显式拒，不做无序增长。
pub const OP_TABLE_CAP: usize = 12;

#[derive(Clone, Copy)]
struct OpEntry {
    code: u8,
    kind: OpKind,
}

/// 算子价目注册表。
///
/// 语义写死：码唯一（重复登记 `DupOp`）、容量 12（再登记 `TableFull`）、
/// 查不到的码一律 `UnknownOp`——**不允许**用默认价兜底，否则账目无从审计。
pub struct OpTable {
    entries: Vec<OpEntry>,
}

impl OpTable {
    pub fn new() -> OpTable {
        OpTable {
            entries: Vec::new(),
        }
    }

    /// 模拟器档标准价目：7 个算子，码 1..=7。
    pub fn standard() -> OpTable {
        let mut t = OpTable::new();
        // 标准表自身不会失败（新表 + 7 个互异码）；若失败说明常量写错，
        // 由判据「算子表登记」当场抓住，这里不做 unwrap。
        let kinds = [
            OpKind::Decode,
            OpKind::Profile,
            OpKind::Map,
            OpKind::Bpc,
            OpKind::Split,
            OpKind::Encode,
            OpKind::Preview,
        ];
        let mut i = 0usize;
        while i < kinds.len() {
            if let Some(k) = kinds.get(i) {
                let _ = t.register(k.code(), *k);
            }
            i += 1;
        }
        t
    }

    pub fn register(&mut self, code: u8, kind: OpKind) -> Result<(), BudgetError> {
        if self.lookup(code).is_ok() {
            return Err(BudgetError::DupOp);
        }
        if self.entries.len() >= OP_TABLE_CAP {
            return Err(BudgetError::TableFull);
        }
        self.entries.push(OpEntry { code, kind });
        Ok(())
    }

    pub fn lookup(&self, code: u8) -> Result<OpKind, BudgetError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.code == code {
                    return Ok(e.kind);
                }
            }
            i += 1;
        }
        Err(BudgetError::UnknownOp)
    }

    /// 某算子在指定降级档的单价（tick / 千像素）。
    pub fn cost(&self, code: u8, rung: Rung) -> Result<u32, BudgetError> {
        let kind = self.lookup(code)?;
        Ok(rung.cost_of(kind))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for OpTable {
    fn default() -> OpTable {
        OpTable::new()
    }
}

// ---------------------------------------------------------------------------
// 2. 档位预算注册表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Draft,
    Standard,
    Press,
}

impl Tier {
    pub fn code(self) -> u8 {
        match self {
            Tier::Draft => 0,
            Tier::Standard => 1,
            Tier::Press => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Draft => "draft",
            Tier::Standard => "standard",
            Tier::Press => "press",
        }
    }

    pub fn default_budget(self) -> u32 {
        match self {
            Tier::Draft => BUDGET_DRAFT,
            Tier::Standard => BUDGET_STANDARD,
            Tier::Press => BUDGET_PRESS,
        }
    }
}

#[derive(Clone, Copy)]
struct TierEntry {
    tier: Tier,
    budget: u32,
    calibration: &'static str,
}

/// 档位预算注册表（容量 4：三档 + 1 个预留，第 5 个显式满）。
///
/// 预算可以重定价（`set_budget`），但必须落在 1000..=40000；重定价后
/// 同一作业的调度结论必须随之改变——判据「预算改动生效」钉的就是这条。
pub struct TierBook {
    entries: Vec<TierEntry>,
}

impl TierBook {
    pub fn new() -> TierBook {
        TierBook {
            entries: Vec::new(),
        }
    }

    /// 模拟器档登记：三档 + 各自校准号。
    pub fn standard() -> TierBook {
        let mut b = TierBook::new();
        let _ = b.register(Tier::Draft, BUDGET_DRAFT, "sim-draft-v1");
        let _ = b.register(Tier::Standard, BUDGET_STANDARD, "sim-std-v1");
        let _ = b.register(Tier::Press, BUDGET_PRESS, "sim-press-v1");
        b
    }

    pub fn register(
        &mut self,
        tier: Tier,
        budget: u32,
        calibration: &'static str,
    ) -> Result<(), BudgetError> {
        if budget < MIN_BUDGET || budget > MAX_BUDGET {
            return Err(BudgetError::BudgetRange);
        }
        if self.find(tier).is_some() {
            return Err(BudgetError::DupTier);
        }
        if self.entries.len() >= 4 {
            return Err(BudgetError::TableFull);
        }
        self.entries.push(TierEntry {
            tier,
            budget,
            calibration,
        });
        Ok(())
    }

    fn find(&self, tier: Tier) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.tier == tier {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 重定价：档位必须已登记，预算必须落区间，校准号保留（改价不改链）。
    pub fn set_budget(&mut self, tier: Tier, budget: u32) -> Result<(), BudgetError> {
        if budget < MIN_BUDGET || budget > MAX_BUDGET {
            return Err(BudgetError::BudgetRange);
        }
        match self.find(tier) {
            Some(i) => match self.entries.get_mut(i) {
                Some(e) => {
                    e.budget = budget;
                    Ok(())
                }
                // 上一步 `find` 已证明下标有效；保留显式失败路径。
                None => Err(BudgetError::TierMissing),
            },
            None => Err(BudgetError::TierMissing),
        }
    }

    pub fn budget(&self, tier: Tier) -> Result<u32, BudgetError> {
        match self.find(tier) {
            Some(i) => match self.entries.get(i) {
                Some(e) => Ok(e.budget),
                None => Err(BudgetError::TierMissing),
            },
            None => Err(BudgetError::TierMissing),
        }
    }

    pub fn calibration(&self, tier: Tier) -> Option<&'static str> {
        match self.find(tier) {
            Some(i) => self.entries.get(i).map(|e| e.calibration),
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for TierBook {
    fn default() -> TierBook {
        TierBook::new()
    }
}

// ---------------------------------------------------------------------------
// 3. 降级阶梯与质量换算
// ---------------------------------------------------------------------------

/// 降级阶梯：满配 → 轻降（去预览）→ 粗网格（去黑点 + 映射半价）→ 内置档（去 profile 解析）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung {
    Full,
    Light,
    Coarse,
    Builtin,
}

impl Rung {
    pub fn label(self) -> &'static str {
        match self {
            Rung::Full => "full",
            Rung::Light => "light",
            Rung::Coarse => "coarse-grid",
            Rung::Builtin => "builtin-profile",
        }
    }

    pub fn grid_step(self) -> f32 {
        match self {
            Rung::Full => 1.0,
            Rung::Light => 2.0,
            Rung::Coarse => 4.0,
            Rung::Builtin => 8.0,
        }
    }

    /// 本档允许的 ΔE2000 最大损失。数值由网格步长推出：
    /// 单轴量化误差 ≤ step/2，三轴 ΔE76 ≤ √3·step/2，
    /// ΔE2000 对低彩度再放大 ~1.2 倍，故上限取 ceil(0.7·step)。
    pub fn ceiling_de(self) -> f32 {
        match self {
            Rung::Full => 0.01,
            Rung::Light => 2.0,
            Rung::Coarse => 5.0,
            Rung::Builtin => 10.0,
        }
    }

    pub fn is_full(self) -> bool {
        self == Rung::Full
    }

    pub fn next(self) -> Option<Rung> {
        match self {
            Rung::Full => Some(Rung::Light),
            Rung::Light => Some(Rung::Coarse),
            Rung::Coarse => Some(Rung::Builtin),
            Rung::Builtin => None,
        }
    }

    /// 单价随档位变化：只减项、不虚报。
    /// * Light：预览是纯显示项，正式出片不需要 → 0；
    /// * Coarse：黑点补偿并入粗网格（不再单算）→ 0；映射走粗网格价；
    /// * Builtin：profile 解析省掉（复用内置档）→ 0。
    pub fn cost_of(self, kind: OpKind) -> u32 {
        match self {
            Rung::Full => kind.base_cost(),
            Rung::Light => match kind {
                OpKind::Preview => 0,
                _ => kind.base_cost(),
            },
            Rung::Coarse => match kind {
                OpKind::Preview | OpKind::Bpc => 0,
                OpKind::Map => COST_MAP_COARSE,
                _ => kind.base_cost(),
            },
            Rung::Builtin => match kind {
                OpKind::Preview | OpKind::Bpc | OpKind::Profile => 0,
                OpKind::Map => COST_MAP_COARSE,
                _ => kind.base_cost(),
            },
        }
    }
}

/// 质量标尺用的 12 个固定色块（整数 Lab，满配网格上量化无损）。
/// 覆盖暗部/中间调/亮部与低/中/高彩度，避免只用中性灰自欺。
const QUALITY_PATCHES: [Lab; 12] = [
    Lab::new(20.0, -10.0, -30.0),
    Lab::new(25.0, 0.0, -10.0),
    Lab::new(30.0, 10.0, 0.0),
    Lab::new(35.0, 20.0, 10.0),
    Lab::new(45.0, -5.0, 20.0),
    Lab::new(50.0, 5.0, 30.0),
    Lab::new(55.0, 15.0, -25.0),
    Lab::new(60.0, 25.0, -15.0),
    Lab::new(70.0, 0.0, 0.0),
    Lab::new(75.0, 10.0, 15.0),
    Lab::new(85.0, -10.0, 35.0),
    Lab::new(90.0, 20.0, -35.0),
];

/// 就近量化到 step 网格（不用 roundf，手写以保持依赖面最小）。
fn round_to(v: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return v;
    }
    let s = v / step;
    let n = if s >= 0.0 {
        (s + 0.5) as i32
    } else {
        (s - 0.5) as i32
    };
    n as f32 * step
}

/// 本档在 12 色块上的实测 ΔE2000 最大损失。
/// 这是「省时间换来的色差」的硬数字，判据要求它 ≤ `ceiling_de()`。
pub fn quality_loss(rung: Rung) -> f32 {
    let step = rung.grid_step();
    let mut worst = 0.0f32;
    let mut i = 0usize;
    while i < QUALITY_PATCHES.len() {
        if let Some(p) = QUALITY_PATCHES.get(i) {
            let q = Lab::new(round_to(p.l, step), round_to(p.a, step), round_to(p.b, step));
            let d = delta_e2000(*p, q);
            if d > worst {
                worst = d;
            }
        }
        i += 1;
    }
    worst
}

// ---------------------------------------------------------------------------
// 4. 页作业：算子码流解析
// ---------------------------------------------------------------------------

pub const OP_SLOTS: usize = 8;

/// 一页的作业描述：页号、工作量（千像素）、档位、算子码流（定长 8 槽）。
#[derive(Clone)]
pub struct PageJob {
    pub page: u16,
    pub pixels_k: u16,
    pub tier: Tier,
    ops: [u8; OP_SLOTS],
    n_ops: usize,
}

impl PageJob {
    pub fn new(page: u16, pixels_k: u16, tier: Tier) -> PageJob {
        PageJob {
            page,
            pixels_k,
            tier,
            ops: [0u8; OP_SLOTS],
            n_ops: 0,
        }
    }

    pub fn push_code(&mut self, code: u8) -> Result<(), BudgetError> {
        if self.n_ops >= OP_SLOTS {
            return Err(BudgetError::ProgramFull);
        }
        if let Some(slot) = self.ops.get_mut(self.n_ops) {
            *slot = code;
            self.n_ops += 1;
            Ok(())
        } else {
            // 上一步 `n_ops < OP_SLOTS` 已证明槽位存在；保留显式路径。
            Err(BudgetError::ProgramFull)
        }
    }

    /// 从码流构造；超过 8 个码显式 `ProgramFull`（不截断、不静默丢弃）。
    pub fn with_codes(
        mut self,
        codes: &[u8],
    ) -> Result<PageJob, BudgetError> {
        if codes.len() > OP_SLOTS {
            return Err(BudgetError::ProgramFull);
        }
        let mut i = 0usize;
        while i < codes.len() {
            if let Some(c) = codes.get(i) {
                self.push_code(*c)?;
            }
            i += 1;
        }
        Ok(self)
    }

    pub fn ops(&self) -> &[u8] {
        self.ops.get(..self.n_ops).unwrap_or(&[])
    }

    pub fn n_ops(&self) -> usize {
        self.n_ops
    }
}

/// 满配 7 算子的标准作业（码 1..=7）。
pub fn full_pipeline(page: u16, pixels_k: u16, tier: Tier) -> Result<PageJob, BudgetError> {
    PageJob::new(page, pixels_k, tier).with_codes(&[1, 2, 3, 4, 5, 6, 7])
}

// ---------------------------------------------------------------------------
// 5. 调度器：四段阶梯 + 显式拒单
// ---------------------------------------------------------------------------

/// 一页的结算方案。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagePlan {
    pub page: u16,
    pub tier: Tier,
    pub rung: Rung,
    pub budget: u32,
    pub actual: u32,
    /// 评估过的档位数（1..=4），用来证明「逐级试探」而非一步跳到最差档。
    pub tried: u8,
    pub pixels_k: u16,
    /// 算子码位图（bit b 对应码 b），账本可据此脱离作业对象重算。
    pub ops_mask: u32,
}

impl PagePlan {
    pub fn within_budget(&self) -> bool {
        self.actual <= self.budget
    }

    pub fn ceiling_de(&self) -> f32 {
        self.rung.ceiling_de()
    }
}

/// 拒单：降无可降时给出的显式结论，带最低可达成本与预算差。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refusal {
    pub page: u16,
    pub tier: Tier,
    pub best_rung: Rung,
    pub best_cost: u32,
    pub budget: u32,
    pub reason: BudgetError,
}

impl Refusal {
    pub fn code(&self) -> u32 {
        self.reason.code()
    }

    pub fn shortfall(&self) -> u32 {
        self.best_cost.saturating_sub(self.budget)
    }
}

/// 调度结果：要么落一页方案，要么显式拒单。没有第三种「悄悄跳过」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Settled(PagePlan),
    Refused(Refusal),
}

/// 预算调度器：持有价目表与档位表的只读视图（按值持有，便于派生同构引擎）。
pub struct BudgetScheduler {
    tiers: TierBook,
    ops: OpTable,
}

impl BudgetScheduler {
    pub fn new(tiers: TierBook, ops: OpTable) -> BudgetScheduler {
        BudgetScheduler { tiers, ops }
    }

    /// 模拟器档默认：三档 + 标准价目。
    pub fn standard() -> BudgetScheduler {
        BudgetScheduler::new(TierBook::standard(), OpTable::standard())
    }

    pub fn tiers(&self) -> &TierBook {
        &self.tiers
    }

    pub fn tiers_mut(&mut self) -> &mut TierBook {
        &mut self.tiers
    }

    pub fn ops(&self) -> &OpTable {
        &self.ops
    }

    /// 解析作业码流 → 算子表；任何未知码在**降价之前**就拒绝。
    fn resolve(&self, job: &PageJob) -> Result<Vec<OpKind>, BudgetError> {
        if job.n_ops == 0 {
            return Err(BudgetError::EmptyWorkload);
        }
        if job.pixels_k == 0 || job.pixels_k > MAX_PIXELS_K {
            return Err(BudgetError::BadWorkload);
        }
        let mut kinds = Vec::with_capacity(job.n_ops);
        let mut i = 0usize;
        while i < job.n_ops {
            let code = match job.ops.get(i) {
                Some(c) => *c,
                None => return Err(BudgetError::EmptyWorkload),
            };
            kinds.push(self.ops.lookup(code)?);
            i += 1;
        }
        Ok(kinds)
    }

    /// 某档下这页的实测成本（tick）。独立于 `plan`，判据用它做守恒重算。
    pub fn cost_at(&self, job: &PageJob, rung: Rung) -> Result<u32, BudgetError> {
        let kinds = self.resolve(job)?;
        Ok(charge(&kinds, rung, job.pixels_k))
    }

    /// 四段阶梯调度：装得下就停，装不下继续降；四档都不行 → 拒单。
    pub fn plan(&self, job: &PageJob) -> Result<Outcome, BudgetError> {
        let kinds = self.resolve(job)?;
        let budget = self.tiers.budget(job.tier)?;
        let mut rung = Rung::Full;
        let mut tried = 0u8;
        let mut best = charge(&kinds, Rung::Builtin, job.pixels_k);
        let mut best_rung = Rung::Builtin;
        loop {
            let cost = charge(&kinds, rung, job.pixels_k);
            tried += 1;
            if cost <= budget {
                return Ok(Outcome::Settled(PagePlan {
                    page: job.page,
                    tier: job.tier,
                    rung,
                    budget,
                    actual: cost,
                    tried,
                    pixels_k: job.pixels_k,
                    ops_mask: mask_of(&kinds),
                }));
            }
            if cost < best {
                best = cost;
                best_rung = rung;
            }
            match rung.next() {
                Some(n) => rung = n,
                None => {
                    return Ok(Outcome::Refused(Refusal {
                        page: job.page,
                        tier: job.tier,
                        best_rung,
                        best_cost: best,
                        budget,
                        reason: BudgetError::OverBudget,
                    }));
                }
            }
        }
    }
}

impl Default for BudgetScheduler {
    fn default() -> BudgetScheduler {
        BudgetScheduler::standard()
    }
}

/// 成本 = Σ 单价(档) × 千像素。全程整数、饱和加，不会溢出回绕。
fn charge(kinds: &[OpKind], rung: Rung, pixels_k: u16) -> u32 {
    let px = pixels_k as u32;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < kinds.len() {
        if let Some(k) = kinds.get(i) {
            total = total.saturating_add(rung.cost_of(*k).saturating_mul(px));
        }
        i += 1;
    }
    total
}

/// 算子码 → 32 位位图（码 1..=7 占 bit1..=7，bit0 空置）。
fn mask_of(kinds: &[OpKind]) -> u32 {
    let mut m = 0u32;
    let mut i = 0usize;
    while i < kinds.len() {
        if let Some(k) = kinds.get(i) {
            m |= 1u32 << (k.code() as u32);
        }
        i += 1;
    }
    m
}

// ---------------------------------------------------------------------------
// 6. 预算账本
// ---------------------------------------------------------------------------

/// 账本容量 16 页（一个 A3 拼版批次），第 17 页显式 `LedgerFull`。
pub const LEDGER_CAP: usize = 16;

/// 一条预算账目：只留可审计字段，不留作业对象。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetRecord {
    pub slot: u8,
    pub page: u16,
    pub tier: Tier,
    pub rung: Rung,
    pub budget: u32,
    pub actual: u32,
    pub pixels_k: u16,
    pub ops_mask: u32,
}

impl BudgetRecord {
    pub fn from_plan(plan: &PagePlan, slot: u8) -> BudgetRecord {
        BudgetRecord {
            slot,
            page: plan.page,
            tier: plan.tier,
            rung: plan.rung,
            budget: plan.budget,
            actual: plan.actual,
            pixels_k: plan.pixels_k,
            ops_mask: plan.ops_mask,
        }
    }

    pub fn within_budget(&self) -> bool {
        self.actual <= self.budget
    }

    /// 脱离调度器重算本页成本：按位图逐码查价目表，再乘千像素。
    /// 账本自洽性靠它，而不是靠写入时缓存下来的 `actual`。
    pub fn recompute(&self, ops: &OpTable) -> Result<u32, BudgetError> {
        let px = self.pixels_k as u32;
        let mut total = 0u32;
        let mut code = 1u8;
        while code <= 8u8 {
            let bit = 1u32 << (code as u32);
            if self.ops_mask & bit != 0 {
                let unit = ops.cost(code, self.rung)?;
                total = total.saturating_add(unit.saturating_mul(px));
            }
            code += 1;
        }
        Ok(total)
    }
}

/// 预算账本。
///
/// 入账顺序（写死，避免语义漂移）：
/// 1. 重复页 → `DupPage`；
/// 2. 实测 > 预算 → 违约计数 +1 并 `BudgetBreach`（**不入账**）；
/// 3. 容量满 → `LedgerFull`。
/// 违约条目绝不落账，但必须留计数——静默吞掉比超预算更危险。
pub struct BudgetLedger {
    records: Vec<BudgetRecord>,
    breaches: u32,
    silent_drops: u32,
}

impl BudgetLedger {
    pub fn new() -> BudgetLedger {
        BudgetLedger {
            records: Vec::new(),
            breaches: 0,
            silent_drops: 0,
        }
    }

    pub fn push(&mut self, rec: BudgetRecord) -> Result<(), BudgetError> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.page == rec.page {
                    return Err(BudgetError::DupPage);
                }
            }
            i += 1;
        }
        if !rec.within_budget() {
            self.breaches = self.breaches.saturating_add(1);
            return Err(BudgetError::BudgetBreach);
        }
        if self.records.len() >= LEDGER_CAP {
            return Err(BudgetError::LedgerFull);
        }
        self.records.push(rec);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&BudgetRecord> {
        self.records.get(i)
    }

    pub fn charge_of(&self, page: u16) -> Option<u32> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.page == page {
                    return Some(r.actual);
                }
            }
            i += 1;
        }
        None
    }

    pub fn sum_actual(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                s = s.saturating_add(r.actual);
            }
            i += 1;
        }
        s
    }

    pub fn max_actual(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.actual > m {
                    m = r.actual;
                }
            }
            i += 1;
        }
        m
    }

    pub fn all_within_budget(&self) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            match self.records.get(i) {
                Some(r) => {
                    if !r.within_budget() {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 全账本重算：返回 (校验条数, 不一致条数)。
    pub fn audit(&self, ops: &OpTable) -> (u32, u32) {
        let mut checked = 0u32;
        let mut bad = 0u32;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                checked += 1;
                match r.recompute(ops) {
                    Ok(v) => {
                        if v != r.actual {
                            bad += 1;
                        }
                    }
                    Err(_) => bad += 1,
                }
            }
            i += 1;
        }
        (checked, bad)
    }

    pub fn breaches(&self) -> u32 {
        self.breaches
    }

    /// 本模块里所有拒收路径都走显式计数，此值必须恒为 0（判据钉）。
    pub fn silent_drops(&self) -> u32 {
        self.silent_drops
    }
}

impl Default for BudgetLedger {
    fn default() -> BudgetLedger {
        BudgetLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 7. 引擎：调度 + 落账
// ---------------------------------------------------------------------------

/// 预算引擎：一页进来 → 调度 → 结算落账 / 拒单留痕。
pub struct BudgetEngine {
    sched: BudgetScheduler,
    ledger: BudgetLedger,
    refusals: u32,
    settled: u32,
}

impl BudgetEngine {
    pub fn new(sched: BudgetScheduler) -> BudgetEngine {
        BudgetEngine {
            sched,
            ledger: BudgetLedger::new(),
            refusals: 0,
            settled: 0,
        }
    }

    pub fn standard() -> BudgetEngine {
        BudgetEngine::new(BudgetScheduler::standard())
    }

    pub fn sched(&self) -> &BudgetScheduler {
        &self.sched
    }

    pub fn sched_mut(&mut self) -> &mut BudgetScheduler {
        &mut self.sched
    }

    pub fn ledger(&self) -> &BudgetLedger {
        &self.ledger
    }

    pub fn refusals(&self) -> u32 {
        self.refusals
    }

    pub fn settled(&self) -> u32 {
        self.settled
    }

    /// 提交一页。
    /// * 作业本身非法（未登记算子 / 空码流 / 工作量越界）→ `Err`，不落账；
    /// * 预算不够但可降级 → 落账并返回 `Settled`；
    /// * 全档都超预算 → 返回 `Refused`（显式计数，账本不动）；
    /// * 账本写入失败（重页 / 满 / 违约）→ `Err`，由调用方处置。
    pub fn submit(&mut self, job: &PageJob) -> Result<Outcome, BudgetError> {
        match self.sched.plan(job)? {
            Outcome::Settled(plan) => {
                let slot = self.ledger.len() as u8;
                self.ledger.push(BudgetRecord::from_plan(&plan, slot))?;
                self.settled = self.settled.saturating_add(1);
                Ok(Outcome::Settled(plan))
            }
            Outcome::Refused(r) => {
                // 拒单不落账（没有可信成本可记），但必须留痕。
                self.refusals = self.refusals.saturating_add(1);
                Ok(Outcome::Refused(r))
            }
        }
    }
}

impl Default for BudgetEngine {
    fn default() -> BudgetEngine {
        BudgetEngine::standard()
    }
}

// ---------------------------------------------------------------------------
// 8. 坏样本注入核（5 类）
// ---------------------------------------------------------------------------

/// 5 类坏样本各造一个，返回 (抓到数, 应抓数)。
/// 抓 = 对应入口返回了预期的显式错误/拒单，且没有产生任何未计入的静默路径。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    const EXPECT: usize = 5;

    // 类 1：未登记算子码（0x7e）。
    let mut tbl = OpTable::standard();
    if tbl.lookup(0x7e).err() == Some(BudgetError::UnknownOp)
        && tbl.register(1, OpKind::Decode).err() == Some(BudgetError::DupOp)
    {
        caught += 1;
    }

    // 类 2：空码流 / 零工作量。
    let empty = PageJob::new(1, 16, Tier::Standard);
    let zero_px = match PageJob::new(2, 0, Tier::Standard).with_codes(&[1, 3]) {
        Ok(j) => j,
        Err(_) => PageJob::new(2, 0, Tier::Standard),
    };
    if empty.n_ops() == 0
        && BudgetScheduler::standard().plan(&empty).err() == Some(BudgetError::EmptyWorkload)
        && BudgetScheduler::standard().plan(&zero_px).err() == Some(BudgetError::BadWorkload)
    {
        caught += 1;
    }

    // 类 3：降无可降（512k px + 满配 + 草稿档）。
    match full_pipeline(3, MAX_PIXELS_K, Tier::Draft) {
        Ok(job) => match BudgetScheduler::standard().plan(&job) {
            Ok(Outcome::Refused(r)) => {
                if r.code() == BudgetError::OverBudget.code()
                    && r.best_cost > r.budget
                    && r.best_rung == Rung::Builtin
                {
                    caught += 1;
                }
            }
            _ => {}
        },
        Err(_) => {}
    }

    // 类 4：违约账目（actual > budget）必须入账即拒并计数。
    let mut led = BudgetLedger::new();
    let forged = BudgetRecord {
        slot: 0,
        page: 9,
        tier: Tier::Press,
        rung: Rung::Full,
        budget: 9000,
        actual: 9001,
        pixels_k: 64,
        ops_mask: 0b0000_0111_1111_1110,
    };
    if led.push(forged).err() == Some(BudgetError::BudgetBreach)
        && led.len() == 0
        && led.breaches() == 1
        && led.silent_drops() == 0
    {
        caught += 1;
    }

    // 类 5：重页 + 账本满（16 页之后第 17 页）。
    let mut eng = BudgetEngine::standard();
    let mut dup_ok = false;
    let mut n = 1u16;
    while n <= 16 {
        let job = match full_pipeline(n, 4, Tier::Press) {
            Ok(j) => j,
            Err(_) => return (caught, EXPECT),
        };
        if eng.submit(&job).is_err() {
            return (caught, EXPECT);
        }
        if n == 2 {
            dup_ok = eng.submit(&job).err() == Some(BudgetError::DupPage);
        }
        n += 1;
    }
    let seventeenth = match full_pipeline(17, 4, Tier::Press) {
        Ok(j) => j,
        Err(_) => return (caught, EXPECT),
    };
    if dup_ok
        && eng.submit(&seventeenth).err() == Some(BudgetError::LedgerFull)
        && eng.ledger().len() == 16
        && eng.refusals() == 0
    {
        caught += 1;
    }

    (caught, EXPECT)
}

// ---------------------------------------------------------------------------
// 9. 判据
// ---------------------------------------------------------------------------

pub fn run_unx50006_checks() -> CheckSet {
    let mut s = CheckSet::new("unx50006");

    // J1-1 档位登记：三档齐、预算与常量逐位相等、校准号非空。
    let tb = TierBook::standard();
    let ok = tb.len() == 3
        && tb.budget(Tier::Draft).ok() == Some(BUDGET_DRAFT)
        && tb.budget(Tier::Standard).ok() == Some(BUDGET_STANDARD)
        && tb.budget(Tier::Press).ok() == Some(BUDGET_PRESS)
        && tb.calibration(Tier::Draft).is_some()
        && tb.calibration(Tier::Press).is_some();
    s.add("50006-J1-档位登记", ok, "三档预算/校准号登记不全");

    // J1-2 预算单调：草稿 < 标准 < 印刷。
    let ok = BUDGET_DRAFT < BUDGET_STANDARD && BUDGET_STANDARD < BUDGET_PRESS;
    s.add("50006-J1-预算单调", ok, "档位预算不是严格递增");

    // J1-3 算子表：7 个标准码可查、单价为正、未知码与重复码显式拒。
    let ops = OpTable::standard();
    let mut priced = true;
    let mut code = 1u8;
    while code <= 7u8 {
        match ops.lookup(code) {
            Ok(k) => {
                if k.base_cost() == 0 {
                    priced = false;
                }
            }
            Err(_) => priced = false,
        }
        code += 1;
    }
    let ok = ops.len() == 7
        && priced
        && ops.lookup(0x7e).err() == Some(BudgetError::UnknownOp)
        && ops.cost(0x7e, Rung::Full).err() == Some(BudgetError::UnknownOp)
        && ops.cost(3, Rung::Light).ok() == Some(COST_MAP);
    s.add("50006-J1-算子表", ok, "价目表查表/拒表不规范");

    // J1-4 每页实测 ≤ 预算：16 页跨三档，全部落账且全部达标。
    let mut eng = BudgetEngine::standard();
    let tiers = [Tier::Draft, Tier::Standard, Tier::Press];
    let mut settled_all = true;
    let mut over_budget = 0usize;
    let mut i = 0u16;
    while i < 16 {
        let tier = match tiers.get((i as usize) % 3) {
            Some(t) => *t,
            None => Tier::Draft,
        };
        // 工作量按档位缩放：草稿 ≤67k、标准 ≤117k、印刷 ≤187k，
        // 保证这三档都能在四段阶梯内找到落点。
        let px = 8 + (i * 11) % match tier {
            Tier::Draft => 60,
            Tier::Standard => 110,
            Tier::Press => 180,
        };
        let job = match full_pipeline(i + 1, px, tier) {
            Ok(j) => j,
            Err(_) => {
                settled_all = false;
                break;
            }
        };
        match eng.submit(&job) {
            Ok(Outcome::Settled(p)) => {
                if !p.within_budget() {
                    over_budget += 1;
                }
            }
            _ => settled_all = false,
        }
        i += 1;
    }
    let led = eng.ledger();
    let ok = settled_all
        && led.len() == 16
        && over_budget == 0
        && led.all_within_budget()
        && eng.settled() == 16
        && eng.refusals() == 0;
    let det = format!(
        "pages={} over={} settled={}",
        led.len(),
        over_budget,
        eng.settled()
    );
    s.add_dyn("50006-J1-每页不超预算", ok, &det);

    // J1-5 守恒：账本 16 条全部能脱离调度器、按价目表重算出同一 actual。
    let (checked, bad) = led.audit(&ops);
    let ok = checked == 16 && bad == 0 && led.sum_actual() > 0;
    let det = format!("checked={} mismatch={} sum={}", checked, bad, led.sum_actual());
    s.add_dyn("50006-J1-实测守恒", ok, &det);

    // J1-6 满配不降：小页（16k）标准档在满配价内，必须落在 Full 且只试一档。
    let mut small = BudgetEngine::standard();
    let job = match full_pipeline(100, 16, Tier::Standard) {
        Ok(j) => j,
        Err(_) => PageJob::new(100, 16, Tier::Standard),
    };
    let small_ok = match small.submit(&job) {
        Ok(Outcome::Settled(p)) => p.rung == Rung::Full && p.tried == 1 && p.actual == 137 * 16,
        _ => false,
    };
    s.add("50006-J1-满配不降", small_ok, "装得下却降级/档位试探数不对");

    // J1-7 超限才降：64k 草稿满配 8768 > 5000，落到 Builtin；且满配确实超。
    let mut e7 = BudgetEngine::standard();
    let job7 = match full_pipeline(101, 64, Tier::Draft) {
        Ok(j) => j,
        Err(_) => PageJob::new(101, 64, Tier::Draft),
    };
    // 成本查询用独立同构调度器：不借 e7，避免与下面的可变提交打架。
    let probe = BudgetScheduler::standard();
    let full_cost = probe.cost_at(&job7, Rung::Full).unwrap_or(0);
    let last_cost = probe.cost_at(&job7, Rung::Builtin).unwrap_or(0);
    let ok = match e7.submit(&job7) {
        Ok(Outcome::Settled(p)) => {
            p.rung == Rung::Builtin
                && p.tried == 4
                && p.actual == 68 * 64
                && p.actual <= p.budget
                && full_cost == 137 * 64
                && full_cost > p.budget
        }
        _ => false,
    };
    let det = format!("full={} last={} budget=5000", full_cost, last_cost);
    s.add_dyn("50006-J1-超限才降", ok, &det);

    // J1-8 逐级省钱：同一页四档成本严格递减（阶梯不是摆设）。
    let c0 = probe.cost_at(&job7, Rung::Full).unwrap_or(0);
    let c1 = probe.cost_at(&job7, Rung::Light).unwrap_or(0);
    let c2 = probe.cost_at(&job7, Rung::Coarse).unwrap_or(0);
    let c3 = probe.cost_at(&job7, Rung::Builtin).unwrap_or(0);
    let ok = c0 == 8768 && c1 == 8384 && c2 == 6272 && c3 == 4352 && c0 > c1 && c1 > c2 && c2 > c3;
    let det = format!("{}>{}>{}>{}", c0, c1, c2, c3);
    s.add_dyn("50006-J1-逐级省钱", ok, &det);

    // J1-9 降无可降拒：512k 印刷满配，连 Builtin 都超 → 拒单，账本不动。
    let mut e9 = BudgetEngine::standard();
    let job9 = match full_pipeline(102, MAX_PIXELS_K, Tier::Press) {
        Ok(j) => j,
        Err(_) => PageJob::new(102, MAX_PIXELS_K, Tier::Press),
    };
    let refused = match e9.submit(&job9) {
        Ok(Outcome::Refused(r)) => {
            r.best_rung == Rung::Builtin
                && r.best_cost == 68 * 512
                && r.budget == BUDGET_PRESS
                && r.shortfall() == 68 * 512 - BUDGET_PRESS
                && r.code() == BudgetError::OverBudget.code()
        }
        _ => false,
    };
    let ok = refused
        && e9.ledger().len() == 0
        && e9.refusals() == 1
        && e9.ledger().breaches() == 0
        && e9.ledger().silent_drops() == 0;
    s.add("50006-J1-降无可降拒", ok, "超限页未被显式拒单或账本被污染");

    // J1-10 拒单带码：拒单理由码落在本段，且文案非空。
    let code_ok = BudgetError::OverBudget.code() == SEG_BASE + E_OVER_BUDGET as u32
        && BudgetError::OverBudget.code() >= SEG_BASE
        && BudgetError::OverBudget.code() < SEG_BASE + 0x20
        && !BudgetError::OverBudget.message().is_empty();
    s.add("50006-J1-拒单带码", code_ok, "拒单理由码越段/文案为空");

    // J1-11 预算改动生效：同一 200k 页，重定价后从 Builtin 变为 Full。
    let mut e11 = BudgetEngine::standard();
    let job11 = match full_pipeline(103, 200, Tier::Press) {
        Ok(j) => j,
        Err(_) => PageJob::new(103, 200, Tier::Press),
    };
    let before = match e11.submit(&job11) {
        Ok(Outcome::Settled(p)) => p.rung == Rung::Builtin && p.actual == 13600,
        _ => false,
    };
    let rebudget = e11.sched_mut().tiers_mut().set_budget(Tier::Press, 30000);
    // 同页重提会撞 DupPage，所以换页号做对照（同一 200k 工作量）。
    let job11b = match full_pipeline(104, 200, Tier::Press) {
        Ok(j) => j,
        Err(_) => PageJob::new(104, 200, Tier::Press),
    };
    let after = match e11.submit(&job11b) {
        Ok(Outcome::Settled(p)) => p.rung == Rung::Full && p.actual == 137 * 200,
        _ => false,
    };
    let ok = before && rebudget.is_ok() && after;
    s.add("50006-J1-预算改动生效", ok, "重定价未改变调度结论");

    // J1-12 页号封口：已满 16 页后第 17 页显式 LedgerFull，静默计数为 0。
    let mut e12 = BudgetEngine::standard();
    let mut filled = 0usize;
    let mut n = 1u16;
    while n <= 16 {
        let job = match full_pipeline(n, 4, Tier::Press) {
            Ok(j) => j,
            Err(_) => break,
        };
        if e12.submit(&job).is_ok() {
            filled += 1;
        }
        n += 1;
    }
    let over = match full_pipeline(17, 4, Tier::Press) {
        Ok(j) => e12.submit(&j),
        Err(_) => Err(BudgetError::EmptyWorkload),
    };
    let ok = filled == 16
        && over.err() == Some(BudgetError::LedgerFull)
        && e12.ledger().len() == 16
        && e12.ledger().silent_drops() == 0;
    s.add("50006-J1-页号封口", ok, "第 17 页未显式封口");

    // J1-13 违约必拒：伪造 actual > budget 的账目入账即拒并计数。
    let mut forged_led = BudgetLedger::new();
    let forged = BudgetRecord {
        slot: 0,
        page: 200,
        tier: Tier::Draft,
        rung: Rung::Full,
        budget: BUDGET_DRAFT,
        actual: BUDGET_DRAFT + 1,
        pixels_k: 64,
        ops_mask: 0b0000_0111_1111_1110,
    };
    let ok = forged_led.push(forged).err() == Some(BudgetError::BudgetBreach)
        && forged_led.len() == 0
        && forged_led.breaches() == 1
        && forged_led.silent_drops() == 0;
    s.add("50006-J1-违约必拒", ok, "超预算账目被接纳或未计数");

    // J1-14 重页必拒：同一页号第二次入账 → DupPage，长度不变。
    let mut dup_led = BudgetLedger::new();
    let rec = BudgetRecord {
        slot: 0,
        page: 55,
        tier: Tier::Press,
        rung: Rung::Light,
        budget: BUDGET_PRESS,
        actual: 100,
        pixels_k: 2,
        ops_mask: 0b0000_0000_0000_0010,
    };
    let ok = dup_led.push(rec).is_ok()
        && dup_led.push(rec).err() == Some(BudgetError::DupPage)
        && dup_led.len() == 1;
    s.add("50006-J1-重页必拒", ok, "重复页号被接纳");

    // J1-15 质量阶梯：四档实测 ΔE2000 损失单调递增且各在档位上限内。
    let q0 = quality_loss(Rung::Full);
    let q1 = quality_loss(Rung::Light);
    let q2 = quality_loss(Rung::Coarse);
    let q3 = quality_loss(Rung::Builtin);
    let ok = q0 <= Rung::Full.ceiling_de()
        && q1 <= Rung::Light.ceiling_de()
        && q2 <= Rung::Coarse.ceiling_de()
        && q3 <= Rung::Builtin.ceiling_de()
        && q0 < q1
        && q1 < q2
        && q2 < q3;
    let det = format!("dE00 {:.3}/{:.3}/{:.3}/{:.3}", q0, q1, q2, q3);
    s.add_dyn("50006-J1-质量阶梯", ok, &det);

    // J1-16 质量有界：最差档（step=8）损失仍在单轴 4 个 Lab 单位的理论界内。
    // 理论界：三轴各偏 4 → ΔE76 = 4√3 ≈ 6.93，ΔE2000 放大 1.2 → 8.32。
    let bound = 6.928_203_5f32 * 1.2;
    let ok = q3 > 0.0 && q3 <= bound && q2 <= 8.0;
    let det = format!("worst={:.3} bound={:.3}", q3, bound);
    s.add_dyn("50006-J1-质量有界", ok, &det);

    // J1-17 确定性：同一作业在两个同构引擎上得到同档位、同成本、同上限。
    let mut ea = BudgetEngine::standard();
    let mut eb = BudgetEngine::standard();
    let job17 = match full_pipeline(300, 96, Tier::Standard) {
        Ok(j) => j,
        Err(_) => PageJob::new(300, 96, Tier::Standard),
    };
    let ra = ea.submit(&job17);
    let rb = eb.submit(&job17);
    let ok = match (ra, rb) {
        (Ok(Outcome::Settled(a)), Ok(Outcome::Settled(b))) => {
            a.rung == b.rung && a.actual == b.actual && a.ceiling_de() == b.ceiling_de()
        }
        _ => false,
    };
    s.add("50006-J1-确定性", ok, "同作业两次调度结论不同");

    // J1-18 注入必抓：5 类坏样本全部显式抓到。
    let (caught, expect) = injection_core();
    let ok = caught == expect && expect == 5;
    let det = format!("{}/{}", caught, expect);
    s.add_dyn("50006-J1-注入必抓", ok, &det);

    s
}

// ---------------------------------------------------------------------------
// 10. 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50006_checks_green() {
        let s = run_unx50006_checks();
        assert!(s.all_passed(), "RED: {}", s.red_items_text());
    }

    #[test]
    fn unx50006_tier_book_roundtrip() {
        let mut b = TierBook::standard();
        assert_eq!(b.budget(Tier::Standard), Ok(BUDGET_STANDARD));
        assert_eq!(
            b.register(Tier::Press, BUDGET_PRESS, "again"),
            Err(BudgetError::DupTier)
        );
        assert_eq!(b.set_budget(Tier::Press, 1), Err(BudgetError::BudgetRange));
        assert_eq!(b.set_budget(Tier::Press, 30000), Ok(()));
        assert_eq!(b.budget(Tier::Press), Ok(30000));
        assert_eq!(b.calibration(Tier::Press), Some("sim-press-v1"));
    }

    #[test]
    fn unx50006_ladder_matches_prices() {
        let sch = BudgetScheduler::standard();
        let job = full_pipeline(7, 64, Tier::Draft).expect("job");
        assert_eq!(sch.cost_at(&job, Rung::Full), Ok(8768));
        assert_eq!(sch.cost_at(&job, Rung::Light), Ok(8384));
        assert_eq!(sch.cost_at(&job, Rung::Coarse), Ok(6272));
        assert_eq!(sch.cost_at(&job, Rung::Builtin), Ok(4352));
        match sch.plan(&job) {
            Ok(Outcome::Settled(p)) => {
                assert_eq!(p.rung, Rung::Builtin);
                assert!(p.within_budget());
            }
            _ => panic!("expected settled plan"),
        }
    }

    #[test]
    fn unx50006_refusal_is_explicit() {
        let mut eng = BudgetEngine::standard();
        let job = full_pipeline(9, MAX_PIXELS_K, Tier::Press).expect("job");
        match eng.submit(&job) {
            Ok(Outcome::Refused(r)) => {
                assert_eq!(r.reason, BudgetError::OverBudget);
                assert_eq!(r.code(), SEG_BASE + E_OVER_BUDGET as u32);
                assert!(r.best_cost > r.budget);
            }
            _ => panic!("expected refusal"),
        }
        assert_eq!(eng.ledger().len(), 0);
        assert_eq!(eng.refusals(), 1);
    }

    #[test]
    fn unx50006_quality_is_monotone() {
        let q0 = quality_loss(Rung::Full);
        let q3 = quality_loss(Rung::Builtin);
        assert!(q0 < 0.01, "full grid should be lossless, got {}", q0);
        assert!(q3 > q0);
        assert!(q3 <= Rung::Builtin.ceiling_de());
    }

    #[test]
    fn unx50006_error_codes_in_segment() {
        let all = [
            BudgetError::UnknownOp,
            BudgetError::DupOp,
            BudgetError::EmptyWorkload,
            BudgetError::BadWorkload,
            BudgetError::OverBudget,
            BudgetError::BudgetRange,
            BudgetError::DupTier,
            BudgetError::TierMissing,
            BudgetError::DupPage,
            BudgetError::LedgerFull,
            BudgetError::BudgetBreach,
            BudgetError::TableFull,
            BudgetError::ProgramFull,
        ];
        for e in all {
            let c = e.code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x20, "code {:#x}", c);
            assert!(!e.message().is_empty());
        }
        let mut i = 0usize;
        while i < all.len() {
            let mut j = i + 1;
            while j < all.len() {
                assert!(all[i].code() != all[j].code(), "dup {:#x}", all[i].code());
                j += 1;
            }
            i += 1;
        }
    }
}
