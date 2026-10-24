//! VE-F3735 · 创作性能遗留清账（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3735`
//!
//! **职责定位（锚点原文）**：创作性能遗留清账——
//! - **清账**：创作性能遗留（R01-R03 性能债 + 预留激活成本评估，
//!   复述制度五用）；
//! - **Q 特色借签**：格式迁移台账模式复用（创作资产旧格式迁移
//!   deadline）——deadline 复用红线；
//! - **三类处置**复述；**闭环复验**复述。
//!
//! **判据（锚点原文五条）**：三类遗留、评估前置、deadline 复用、
//! 闭环复验、五用复刻、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 未评估激活→拦截（复述） | [`E7Code::UNASSESS_ACTIVATE`] |
//! | 未修→阻断（复述） | [`E7Code::UNFIXED_BLOCK`] |
//! | deadline 违约→P1（复用红线实测） | [`E7Code::DEADLINE_BREACH`] |
//! | 闭环缺→补验（复述） | [`E7Code::CLOSEDLOOP_MISS`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3735 诊断码段（独占 0x7Exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct E7Code(pub u16);

impl E7Code {
    /// 未评估激活（拦截）。
    pub const UNASSESS_ACTIVATE: E7Code = E7Code(0x7E01);
    /// 未修（阻断）。
    pub const UNFIXED_BLOCK: E7Code = E7Code(0x7E02);
    /// deadline 违约（P1）。
    pub const DEADLINE_BREACH: E7Code = E7Code(0x7E03);
    /// 闭环缺（补验）。
    pub const CLOSEDLOOP_MISS: E7Code = E7Code(0x7E04);
    /// 台账参数非法。
    pub const BAD_LEDGER: E7Code = E7Code(0x7E05);
    /// 遗留类别未知。
    pub const UNKNOWN_KIND: E7Code = E7Code(0x7E06);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            E7Code::UNASSESS_ACTIVATE => "未评估激活（复述）。建议：预留成本须先评估。",
            E7Code::UNFIXED_BLOCK => "未修阻断（复述）。建议：修复后才可放行。",
            E7Code::DEADLINE_BREACH => "deadline 违约（P1 复用红线实测）。建议：立即补迁移。",
            E7Code::CLOSEDLOOP_MISS => "闭环缺（复述）。建议：补复验闭环。",
            E7Code::BAD_LEDGER => "台账参数非法。建议：字段须齐。",
            E7Code::UNKNOWN_KIND => "遗留类别未知。建议：仅三类。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三类遗留 ────────────────────────────────────────────────────────

/// 遗留类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebtKind {
    /// 性能债（R01-R03）。
    PerfDebt,
    /// 预留激活成本。
    ReservedCost,
    /// 格式迁移（Q 特色借签）。
    FormatMig,
}

impl DebtKind {
    pub const ALL: [DebtKind; 3] = [
        DebtKind::PerfDebt,
        DebtKind::ReservedCost,
        DebtKind::FormatMig,
    ];
}

/// 遗留条目。
pub struct DebtEntry {
    /// 类别。
    pub kind: DebtKind,
    /// 已评估。
    pub assessed: bool,
    /// 已修。
    pub fixed: bool,
    /// 已闭环复验。
    pub closed: bool,
}

/// 遗留清账。
pub struct DebtLedger {
    entries: Vec<DebtEntry>,
}

impl DebtLedger {
    pub fn new() -> DebtLedger {
        DebtLedger {
            entries: Vec::new(),
        }
    }

    /// 入账。
    pub fn add(&mut self, kind: DebtKind) {
        self.entries.push(DebtEntry {
            kind,
            assessed: false,
            fixed: false,
            closed: false,
        });
    }

    /// 评估。
    pub fn assess(&mut self, idx: usize) -> Result<(), E7Code> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.assessed = true;
                Ok(())
            }
            None => Err(E7Code::BAD_LEDGER),
        }
    }

    /// 激活检查（未评估不得激活）。
    pub fn activation_check(&self, idx: usize) -> Result<(), E7Code> {
        match self.entries.get(idx) {
            Some(e) => {
                if e.assessed {
                    Ok(())
                } else {
                    Err(E7Code::UNASSESS_ACTIVATE)
                }
            }
            None => Err(E7Code::BAD_LEDGER),
        }
    }

    /// 放行检查（未修阻断）。
    pub fn release_check(&self, idx: usize) -> Result<(), E7Code> {
        match self.entries.get(idx) {
            Some(e) => {
                if e.fixed {
                    Ok(())
                } else {
                    Err(E7Code::UNFIXED_BLOCK)
                }
            }
            None => Err(E7Code::BAD_LEDGER),
        }
    }

    /// 闭环复验。
    pub fn close_loop(&mut self, idx: usize) -> Result<(), E7Code> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                if !e.fixed {
                    return Err(E7Code::UNFIXED_BLOCK);
                }
                e.closed = true;
                Ok(())
            }
            None => Err(E7Code::BAD_LEDGER),
        }
    }

    /// 闭环缺检出。
    pub fn closedloop_miss_count(&self) -> usize {
        self.entries.iter().filter(|e| e.fixed && !e.closed).count()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 格式迁移台账（Q 借签 deadline 红线） ────────────────────────────

/// 迁移条目。
pub struct MigrationEntry {
    /// deadline（纪元）。
    pub deadline: u32,
    /// 已迁移。
    pub migrated: bool,
}

/// 迁移台账。
pub struct MigrationBook {
    entries: Vec<MigrationEntry>,
}

impl MigrationBook {
    pub fn new() -> MigrationBook {
        MigrationBook {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, deadline: u32) {
        self.entries.push(MigrationEntry {
            deadline,
            migrated: false,
        });
    }

    /// 标记已迁。
    pub fn mark(&mut self, idx: usize) -> Result<(), E7Code> {
        match self.entries.get_mut(idx) {
            Some(e) => {
                e.migrated = true;
                Ok(())
            }
            None => Err(E7Code::BAD_LEDGER),
        }
    }

    /// deadline 巡检（超期未迁即 P1）。
    pub fn deadline_check(&self, now: u32) -> Result<(), E7Code> {
        for e in &self.entries {
            if !e.migrated && now > e.deadline {
                return Err(E7Code::DEADLINE_BREACH);
            }
        }
        Ok(())
    }
}

// ── 评估记录（前置红线） ────────────────────────────────────────────

/// 成本评估。
pub struct CostAssessment {
    /// 激活成本（点）。
    pub activate_cost: u32,
    /// 是否已评估。
    pub done: bool,
}

/// 评估前置检查。
pub struct AssessGate;

impl AssessGate {
    /// 前置断言（未评估不得激活）。
    pub fn precheck(a: &CostAssessment) -> Result<(), E7Code> {
        if a.done {
            Ok(())
        } else {
            Err(E7Code::UNASSESS_ACTIVATE)
        }
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3735 判据集（锚点原文五条逐条落位）。
pub fn run_vr07o_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3735");

    // 1) 三类遗留：入账
    let mut dl = DebtLedger::new();
    for k in DebtKind::ALL {
        dl.add(k);
    }
    out.add(
        "0x7E06-三类遗留",
        dl.len() == 3,
        "三类须齐",
    );

    // 2) 评估前置：未评估拦截
    out.add(
        "0x7E01-评估前置",
        dl.activation_check(0).is_err(),
        "未评估须拦",
    );

    // 3) 评估后放行激活
    let _ = dl.assess(0);
    out.add(
        "0x7E01-评估后过",
        dl.activation_check(0).is_ok(),
        "评估后须过",
    );

    // 4) 未修阻断
    out.add(
        "0x7E02-未修阻断",
        dl.release_check(0).is_err(),
        "未修须阻",
    );

    // 5) deadline 复用：超期 P1
    let mut mb = MigrationBook::new();
    mb.add(100);
    out.add(
        "0x7E03-deadline",
        mb.deadline_check(200).is_err(),
        "超期须 P1",
    );

    // 6) deadline：已迁过
    let _ = mb.mark(0);
    out.add(
        "0x7E03-已迁过",
        mb.deadline_check(200).is_ok(),
        "已迁须过",
    );

    // 7) 闭环复验：缺被计
    out.add(
        "0x7E04-闭环复验",
        dl.closedloop_miss_count() == 0,
        "未修不计缺",
    );

    // 8) 闭环：修后须闭
    let mut dl2 = DebtLedger::new();
    dl2.add(DebtKind::PerfDebt);
    // 模拟 fixed
    if let Some(e) = dl2.entries.get_mut(0) {
        e.assessed = true;
        e.fixed = true;
    }
    out.add(
        "0x7E04-闭环缺计",
        dl2.closedloop_miss_count() == 1,
        "修后未闭须计",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7E06-判据",
        E7Code::DEADLINE_BREACH.value() != E7Code::UNFIXED_BLOCK.value(),
        "码段互异",
    );

    out
}
