//! VE-F3683 · 创作者经济（VE-S 域 · 批次 R05 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3683`
//!
//! **职责定位（锚点原文）**：创作者经济——
//! - **分成模型**：平台/创作者比例公开（比例暗改 = P0）；
//! - **结算系统**：结算周期 / 最小提现额（拖欠 = 信任崩——逾期红线）；
//! - **激励计划**：新人/优质激励（激励透明——承诺必须算过账——承诺背书红线）；
//! - **经济健康**：分成池可持续评估（不可持续激励 = 先甜后崩）。
//!
//! **判据（锚点原文）**：比例公开、逾期立案、激励背书、结算对账、可持续评估、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 逾期未结→P1（红线实测——结算计时器） | [`C83Code::SETTLE_OVERDUE`] |
//! | 比例暗改→P0（复述红线实测） | [`C83Code::RATIO_TAMPER`] |
//! | 激励无背书→拦截（红线实测） | [`C83Code::INCENTIVE_UNBACKED`] |
//! | 结算错误→P0（钱错=最高级） | [`C83Code::SETTLE_ERROR`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3683 诊断码段（独占 0x83xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C83Code(pub u16);

impl C83Code {
    /// 逾期未结（P1 红线实测）。
    pub const SETTLE_OVERDUE: C83Code = C83Code(0x8301);
    /// 比例暗改（P0）。
    pub const RATIO_TAMPER: C83Code = C83Code(0x8302);
    /// 激励无背书。
    pub const INCENTIVE_UNBACKED: C83Code = C83Code(0x8303);
    /// 结算错误（P0 钱错最高级）。
    pub const SETTLE_ERROR: C83Code = C83Code(0x8304);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C83Code::SETTLE_OVERDUE => "结算逾期未结（P1 红线实测）。建议：立案清欠。",
            C83Code::RATIO_TAMPER => "分成比例暗改（P0 红线）。建议：恢复并走 ADR。",
            C83Code::INCENTIVE_UNBACKED => "激励承诺无背书。建议：算账后上线。",
            C83Code::SETTLE_ERROR => "结算金额错误（P0 最高级）。建议：对账修正。",
            _ => "未知诊断码。",
        }
    }
}

// ── 分成比例（公开） ────────────────────────────────────────────────

/// 分成模型（比例公开）。
pub struct RevShare {
    /// 平台比例（千分比）。
    platform_bp: u16,
    /// 创作者比例（千分比）。
    creator_bp: u16,
    /// 变更日志（ADR 留痕）。
    changes: u32,
    tamper_hits: u32,
    last_code: C83Code,
    has_error: bool,
}

impl RevShare {
    /// 默认：平台 300 / 创作者 700（千分比）。
    pub fn default() -> Self {
        Self {
            platform_bp: 300,
            creator_bp: 700,
            changes: 0,
            tamper_hits: 0,
            last_code: C83Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C83Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 公开读取。
    pub fn platform_bp(&self) -> u16 {
        self.platform_bp
    }

    pub fn creator_bp(&self) -> u16 {
        self.creator_bp
    }

    /// 变更比例（必须留 ADR 痕）。
    pub fn change(&mut self, platform_bp: u16, creator_bp: u16, adr_recorded: bool) -> Result<bool, C83Code> {
        if !adr_recorded {
            self.tamper_hits += 1;
            self.last_code = C83Code::RATIO_TAMPER;
            self.has_error = true;
            return Err(C83Code::RATIO_TAMPER);
        }
        if platform_bp + creator_bp != 1000 {
            self.last_code = C83Code::SETTLE_ERROR;
            self.has_error = true;
            return Err(C83Code::SETTLE_ERROR);
        }
        self.platform_bp = platform_bp;
        self.creator_bp = creator_bp;
        self.changes += 1;
        self.last_code = C83Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 金额拆分：按比例分。
    pub fn split(&self, total: i64) -> (i64, i64) {
        let platform = total * self.platform_bp as i64 / 1000;
        (platform, total - platform)
    }

    pub fn changes(&self) -> u32 {
        self.changes
    }

    pub fn tamper_hits(&self) -> u32 {
        self.tamper_hits
    }
}

// ── 结算系统 ────────────────────────────────────────────────────────

/// 结算单。
#[derive(Clone, Copy)]
pub struct Settlement {
    pub id: u16,
    pub amount: i64,
    /// 应结时点（抽象 tick）。
    pub due_tick: u32,
    /// 是否已结。
    pub settled: bool,
}

/// 结算系统（逾期红线 + 对账）。
pub struct SettleSystem {
    bills: Vec<Settlement>,
    /// 当前 tick。
    now_tick: u32,
    overdue_hits: u32,
    error_hits: u32,
    last_code: C83Code,
    has_error: bool,
}

impl SettleSystem {
    pub fn new() -> Self {
        Self {
            bills: Vec::new(),
            now_tick: 0,
            overdue_hits: 0,
            error_hits: 0,
            last_code: C83Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C83Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn advance(&mut self, tick: u32) {
        self.now_tick = tick;
    }

    pub fn add_bill(&mut self, b: Settlement) -> usize {
        self.bills.push(b);
        self.bills.len()
    }

    /// 结算一笔。
    pub fn settle(&mut self, id: u16, paid: i64) -> Result<bool, C83Code> {
        match self.bills.iter_mut().find(|b| b.id == id) {
            Some(b) => {
                if paid != b.amount {
                    self.error_hits += 1;
                    self.last_code = C83Code::SETTLE_ERROR;
                    self.has_error = true;
                    return Err(C83Code::SETTLE_ERROR);
                }
                b.settled = true;
                self.last_code = C83Code(0);
                self.has_error = false;
                Ok(true)
            }
            None => {
                self.error_hits += 1;
                self.last_code = C83Code::SETTLE_ERROR;
                self.has_error = true;
                Err(C83Code::SETTLE_ERROR)
            }
        }
    }

    /// 逾期审计：未结且超期即红线。
    pub fn audit_overdue(&mut self) -> Result<bool, C83Code> {
        let mut ok = true;
        for b in &self.bills {
            if !b.settled && self.now_tick > b.due_tick {
                ok = false;
                break;
            }
        }
        if ok {
            self.last_code = C83Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.overdue_hits += 1;
            self.last_code = C83Code::SETTLE_OVERDUE;
            self.has_error = true;
            Err(C83Code::SETTLE_OVERDUE)
        }
    }

    /// 对账：已结金额总和。
    pub fn settled_sum(&self) -> i64 {
        self.bills.iter().filter(|b| b.settled).map(|b| b.amount).sum()
    }

    pub fn overdue_hits(&self) -> u32 {
        self.overdue_hits
    }

    pub fn error_hits(&self) -> u32 {
        self.error_hits
    }

    pub fn bill_count(&self) -> usize {
        self.bills.len()
    }
}

// ── 激励计划 ────────────────────────────────────────────────────────

/// 激励项。
#[derive(Clone, Copy)]
pub struct Incentive {
    pub id: u16,
    pub amount: i64,
    /// 是否已算账背书。
    pub backed: bool,
}

/// 激励计划（承诺背书红线）。
pub struct IncentivePlan {
    plans: Vec<Incentive>,
    unbacked_hits: u32,
    last_code: C83Code,
    has_error: bool,
}

impl IncentivePlan {
    pub fn new() -> Self {
        Self {
            plans: Vec::new(),
            unbacked_hits: 0,
            last_code: C83Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C83Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 上线激励：必须背书。
    pub fn launch(&mut self, inc: Incentive) -> Result<bool, C83Code> {
        if !inc.backed {
            self.unbacked_hits += 1;
            self.last_code = C83Code::INCENTIVE_UNBACKED;
            self.has_error = true;
            return Err(C83Code::INCENTIVE_UNBACKED);
        }
        self.plans.push(inc);
        self.last_code = C83Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 承诺总额。
    pub fn promised_sum(&self) -> i64 {
        self.plans.iter().map(|p| p.amount).sum()
    }

    pub fn unbacked_hits(&self) -> u32 {
        self.unbacked_hits
    }

    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }
}

// ── 可持续评估 ──────────────────────────────────────────────────────

/// 分成池可持续评估。
pub struct Sustainability {
    /// 池余额。
    pub pool: i64,
    /// 周期承诺支出。
    pub burn_per_cycle: i64,
}

impl Sustainability {
    pub fn new(pool: i64, burn_per_cycle: i64) -> Self {
        Self { pool, burn_per_cycle }
    }

    /// 可持续周期数（支撑几个结算周期）。
    pub fn cycles_left(&self) -> i64 {
        if self.burn_per_cycle <= 0 {
            return i64::MAX;
        }
        self.pool / self.burn_per_cycle
    }

    /// 可持续断言：至少支撑 6 个周期。
    pub fn sustainable(&self) -> bool {
        self.cycles_left() >= 6
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves83_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves83_creatorecon");
    {
        let r = RevShare::default();
        chk(&mut set, "C3683-01", r.platform_bp() == 300 && r.creator_bp() == 700, "err");
    }
    {
        let mut r = RevShare::default();
        let res = r.change(200, 800, true);
        chk(&mut set, "C3683-02", res == Ok(true) && r.changes() == 1, "err");
    }
    {
        let mut r = RevShare::default();
        let res = r.change(200, 800, false);
        chk(&mut set, "C3683-03", res.err() == Some(C83Code::RATIO_TAMPER), "err");
    }
    {
        let r = RevShare::default();
        let (p, c) = r.split(1000);
        chk(&mut set, "C3683-04", p == 300 && c == 700, "err");
    }
    {
        let mut s = SettleSystem::new();
        s.add_bill(Settlement { id: 1, amount: 100, due_tick: 50, settled: false });
        let res = s.settle(1, 100);
        chk(&mut set, "C3683-05", res == Ok(true) && s.settled_sum() == 100, "err");
    }
    {
        let mut s = SettleSystem::new();
        s.add_bill(Settlement { id: 1, amount: 100, due_tick: 50, settled: false });
        let res = s.settle(1, 90);
        chk(&mut set, "C3683-06", res.err() == Some(C83Code::SETTLE_ERROR), "err");
    }
    {
        let mut s = SettleSystem::new();
        s.add_bill(Settlement { id: 1, amount: 100, due_tick: 50, settled: false });
        s.advance(80);
        let res = s.audit_overdue();
        chk(&mut set, "C3683-07", res.err() == Some(C83Code::SETTLE_OVERDUE), "err");
    }
    {
        let mut s = SettleSystem::new();
        s.add_bill(Settlement { id: 1, amount: 100, due_tick: 50, settled: true });
        s.advance(80);
        chk(&mut set, "C3683-08", s.audit_overdue() == Ok(true), "err");
    }
    {
        let mut p = IncentivePlan::new();
        let res = p.launch(Incentive { id: 1, amount: 500, backed: true });
        chk(&mut set, "C3683-09", res == Ok(true) && p.promised_sum() == 500, "err");
    }
    {
        let mut p = IncentivePlan::new();
        let res = p.launch(Incentive { id: 1, amount: 500, backed: false });
        chk(&mut set, "C3683-10", res.err() == Some(C83Code::INCENTIVE_UNBACKED), "err");
    }
    {
        let s = Sustainability::new(600, 100);
        chk(&mut set, "C3683-11", s.sustainable() && s.cycles_left() == 6, "err");
    }
    {
        let s = Sustainability::new(300, 100);
        chk(&mut set, "C3683-12", !s.sustainable(), "err");
    }
    set
}
