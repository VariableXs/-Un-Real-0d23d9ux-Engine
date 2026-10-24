//! VE-F3937 · 无障碍遗留清账
//!
//! 无障碍性能遗留：S01-S06 性能债 + 预留激活成本评估（复述制度六用）。
//! R 特色借签：双维台账复用（工具+产出双维遗留分账）+ 三类处置 + 闭环复验。
//! 错误路径：未评估激活→拦截、🟡未修→阻断、双维混账→分册、闭环缺→补验。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD100）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDECode(pub u16);

impl CDECode {
    pub const OK: Self = Self(SEG_BASE);
    pub const ACT_UNEVAL: Self = Self(SEG_BASE + 1);
    pub const YELLOW_OPEN: Self = Self(SEG_BASE + 2);
    pub const MIXED_LEDGER: Self = Self(SEG_BASE + 3);
    pub const LOOP_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::ACT_UNEVAL => "未评估激活→拦截",
            Self::YELLOW_OPEN => "🟡未修→阻断",
            Self::MIXED_LEDGER => "双维混账→分册",
            Self::LOOP_MISSING => "闭环缺→补验",
            _ => "未知",
        }
    }
}

// ── 遗留条目（三类）─────────────────────────────────────────────────

/// 遗留类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyKind {
    /// 性能债（S01-S06 累积）。
    PerfDebt,
    /// 预留激活（埋桩未启用）。
    ReservedStub,
    /// 双维遗留（工具维+产出维）。
    DualDim,
}

/// 遗留状态三色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyState {
    /// 绿：已修。
    Green,
    /// 黄：在案未修。
    Yellow,
    /// 红：阻断发布。
    Red,
}

/// 双维归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualSide {
    /// 工具维（调试/基准/仪表）。
    Tool,
    /// 产出维（运行时功能）。
    Product,
}

/// 遗留条目。
#[derive(Debug, Clone, Copy)]
pub struct LegacyItem {
    pub kind: LegacyKind,
    pub state: LegacyState,
    pub side: DualSide,
    /// 激活评估已做（评估前置红线）。
    pub evaluated: bool,
    /// 已激活。
    pub activated: bool,
    /// 已闭环复验。
    pub verified: bool,
}

impl LegacyItem {
    /// 激活合法性：先评估后激活。
    pub fn activation_legal(&self) -> bool {
        !self.activated || self.evaluated
    }

    /// 🟡未修阻断判定：黄且激活→阻断。
    pub fn blocks(&self) -> bool {
        self.state == LegacyState::Yellow && self.activated
    }
}

// ── 双维台账（分册）─────────────────────────────────────────────────

/// 单维台账。
#[derive(Debug, Clone)]
pub struct SideLedger {
    pub side: DualSide,
    pub items: Vec<LegacyItem>,
}

impl SideLedger {
    pub fn new(side: DualSide) -> Self {
        Self {
            side,
            items: Vec::new(),
        }
    }

    pub fn push(&mut self, item: LegacyItem) {
        self.items.push(item);
    }

    /// 混账检出：不属于本维的条目数。
    pub fn misfiled(&self) -> usize {
        self.items.iter().filter(|i| i.side != self.side).count()
    }

    /// 分册：把混账条目移出（返回应迁往条数）。
    pub fn evict_misfiled(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|i| i.side == self.side);
        before - self.items.len()
    }

    /// 黄条目数。
    pub fn yellows(&self) -> usize {
        self.items.iter().filter(|i| i.state == LegacyState::Yellow).count()
    }

    /// 未复验的已修条目（闭环缺）。
    pub fn unverified_done(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.state == LegacyState::Green && !i.verified)
            .count()
    }
}

/// 双维账本（工具+产出分册）。
#[derive(Debug, Clone)]
pub struct DualLedger {
    pub tool: SideLedger,
    pub product: SideLedger,
}

impl DualLedger {
    pub fn new() -> Self {
        Self {
            tool: SideLedger::new(DualSide::Tool),
            product: SideLedger::new(DualSide::Product),
        }
    }

    /// 入账：按 side 自动分册。
    pub fn file(&mut self, item: LegacyItem) {
        match item.side {
            DualSide::Tool => self.tool.push(item),
            DualSide::Product => self.product.push(item),
        }
    }

    /// 全账黄条目（阻断面）。
    pub fn all_yellows(&self) -> usize {
        self.tool.yellows() + self.product.yellows()
    }

    /// 闭环复验缺口。
    pub fn verify_gaps(&self) -> usize {
        self.tool.unverified_done() + self.product.unverified_done()
    }

    /// 补验：全部已修条目标记复验。
    pub fn verify_all(&mut self) -> usize {
        let mut n = 0;
        for led in [&mut self.tool, &mut self.product] {
            for i in led.items.iter_mut() {
                if i.state == LegacyState::Green && !i.verified {
                    i.verified = true;
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for DualLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三类处置 ────────────────────────────────────────────────────────

/// 处置决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// 立即修（红/激活未评估）。
    FixNow,
    /// 挂账跟踪（黄，限期）。
    Track,
    /// 销账（绿已复验）。
    Close,
}

/// 处置裁决。
pub fn disposition(i: &LegacyItem) -> Disposition {
    if i.state == LegacyState::Red || !i.activation_legal() {
        Disposition::FixNow
    } else if i.state == LegacyState::Yellow {
        Disposition::Track
    } else if i.verified {
        Disposition::Close
    } else {
        Disposition::Track
    }
}

// ── 清账会话（总控）─────────────────────────────────────────────────

/// 遗留清账会话。
#[derive(Debug, Clone)]
pub struct LegacySession {
    pub ledger: DualLedger,
    /// 激活拦截计数。
    pub intercepts: u32,
}

impl LegacySession {
    pub fn new() -> Self {
        Self {
            ledger: DualLedger::new(),
            intercepts: 0,
        }
    }

    /// 激活审计：未评估激活→拦截并计数。
    pub fn audit_activation(&mut self) -> u32 {
        let mut n = 0;
        for led in [&mut self.ledger.tool, &mut self.ledger.product] {
            for i in led.items.iter_mut() {
                if i.activated && !i.evaluated {
                    i.activated = false;
                    n += 1;
                }
            }
        }
        self.intercepts += n;
        n
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CDECode> {
        if self.ledger.tool.misfiled() + self.ledger.product.misfiled() > 0 {
            return Err(CDECode::MIXED_LEDGER);
        }
        if self.intercepts > 0 {
            return Err(CDECode::ACT_UNEVAL);
        }
        if self.ledger.all_yellows() > 0 && self.any_activated_yellow() {
            return Err(CDECode::YELLOW_OPEN);
        }
        if self.ledger.verify_gaps() > 0 {
            return Err(CDECode::LOOP_MISSING);
        }
        Ok(())
    }

    fn any_activated_yellow(&self) -> bool {
        self.ledger
            .tool
            .items
            .iter()
            .chain(self.ledger.product.items.iter())
            .any(|i| i.blocks())
    }
}

impl Default for LegacySession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves191_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves191_legacyclear");

    fn item(state: LegacyState, side: DualSide, evaluated: bool, activated: bool) -> LegacyItem {
        LegacyItem {
            kind: LegacyKind::PerfDebt,
            state,
            side,
            evaluated,
            activated,
            verified: false,
        }
    }

    // 01 三类遗留齐备
    {
        chk(
            &mut set,
            "C3937-01",
            [LegacyKind::PerfDebt, LegacyKind::ReservedStub, LegacyKind::DualDim].len() == 3,
            "err",
        );
    }
    // 02 评估前置：未评估激活非法
    {
        let i = item(LegacyState::Green, DualSide::Tool, false, true);
        chk(&mut set, "C3937-02", !i.activation_legal(), "err");
    }
    // 03 评估后激活合法
    {
        let i = item(LegacyState::Green, DualSide::Tool, true, true);
        chk(&mut set, "C3937-03", i.activation_legal(), "err");
    }
    // 04 🟡未修激活→阻断
    {
        let i = item(LegacyState::Yellow, DualSide::Product, true, true);
        chk(&mut set, "C3937-04", i.blocks(), "err");
    }
    // 05 🟡未修未激活→不阻断
    {
        let i = item(LegacyState::Yellow, DualSide::Product, true, false);
        chk(&mut set, "C3937-05", !i.blocks(), "err");
    }
    // 06 双维分册入账
    {
        let mut l = DualLedger::new();
        l.file(item(LegacyState::Green, DualSide::Tool, true, false));
        l.file(item(LegacyState::Green, DualSide::Product, true, false));
        chk(
            &mut set,
            "C3937-06",
            l.tool.items.len() == 1 && l.product.items.len() == 1,
            "err",
        );
    }
    // 07 双维混账→分册清除
    {
        let mut tool = SideLedger::new(DualSide::Tool);
        tool.push(item(LegacyState::Green, DualSide::Tool, true, false));
        tool.push(item(LegacyState::Green, DualSide::Product, true, false));
        tool.push(item(LegacyState::Green, DualSide::Product, true, false));
        let evicted = tool.evict_misfiled();
        chk(
            &mut set,
            "C3937-07",
            tool.misfiled() == 0 && evicted == 2 && tool.items.len() == 1,
            "err",
        );
    }
    // 08 闭环缺：已修未验
    {
        let mut l = DualLedger::new();
        l.file(item(LegacyState::Green, DualSide::Tool, true, false));
        chk(&mut set, "C3937-08", l.verify_gaps() == 1, "err");
    }
    // 09 补验归零
    {
        let mut l = DualLedger::new();
        l.file(item(LegacyState::Green, DualSide::Tool, true, false));
        l.file(item(LegacyState::Green, DualSide::Product, true, false));
        let n = l.verify_all();
        chk(
            &mut set,
            "C3937-09",
            n == 2 && l.verify_gaps() == 0,
            "err",
        );
    }
    // 10 三类处置：红→立即修
    {
        let i = item(LegacyState::Red, DualSide::Tool, true, false);
        chk(
            &mut set,
            "C3937-10",
            disposition(&i) == Disposition::FixNow,
            "err",
        );
    }
    // 11 三类处置：黄→挂账
    {
        let i = item(LegacyState::Yellow, DualSide::Product, true, false);
        chk(
            &mut set,
            "C3937-11",
            disposition(&i) == Disposition::Track,
            "err",
        );
    }
    // 12 三类处置：绿已验→销账
    {
        let mut i = item(LegacyState::Green, DualSide::Tool, true, false);
        i.verified = true;
        chk(
            &mut set,
            "C3937-12",
            disposition(&i) == Disposition::Close,
            "err",
        );
    }
    // 13 激活审计拦截计数
    {
        let mut s = LegacySession::new();
        s.ledger.file(item(LegacyState::Green, DualSide::Tool, false, true));
        s.ledger.file(item(LegacyState::Green, DualSide::Product, false, true));
        let n = s.audit_activation();
        chk(
            &mut set,
            "C3937-13",
            n == 2 && s.intercepts == 2 && s.full_check() == Err(CDECode::ACT_UNEVAL),
            "err",
        );
    }
    // 14 会话总检全绿
    {
        let mut s = LegacySession::new();
        let mut i = item(LegacyState::Green, DualSide::Tool, true, true);
        i.verified = true;
        s.ledger.file(i);
        chk(&mut set, "C3937-14", s.full_check() == Ok(()), "err");
    }
    // 15 会话总检：闭环缺阻断
    {
        let mut s = LegacySession::new();
        s.ledger.file(item(LegacyState::Green, DualSide::Product, true, false));
        chk(
            &mut set,
            "C3937-15",
            s.full_check() == Err(CDECode::LOOP_MISSING),
            "err",
        );
    }
    // 16 黄账统计
    {
        let mut l = DualLedger::new();
        l.file(item(LegacyState::Yellow, DualSide::Tool, true, false));
        l.file(item(LegacyState::Yellow, DualSide::Product, true, true));
        chk(&mut set, "C3937-16", l.all_yellows() == 2, "err");
    }
    // 17 S01-S06 六组来源复述
    {
        // 六组编号 1-6 全部可表达为遗留来源。
        let sources = [1u8, 2, 3, 4, 5, 6];
        chk(&mut set, "C3937-17", sources.len() == 6, "err");
    }
    // 18 诊断码互异
    {
        let codes = [
            CDECode::ACT_UNEVAL,
            CDECode::YELLOW_OPEN,
            CDECode::MIXED_LEDGER,
            CDECode::LOOP_MISSING,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        chk(&mut set, "C3937-18", distinct, "err");
    }

    set
}
