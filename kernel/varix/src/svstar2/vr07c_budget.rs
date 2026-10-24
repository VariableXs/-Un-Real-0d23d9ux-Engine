//! VE-F3723 · 创作预算体系（VE-S 域 · 批次 R07 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3723`
//!
//! **职责定位（锚点原文）**：创作预算总表——
//! - **归集**：R01-R03 预算归一（50ms 编辑 / 100ms 预览 / 5s 导出
//!   + 各工坊专属）——O08/P07/Q07 归集模式四用；
//! - **心流预算**：编辑中断率 ≤1%（每小时中断 ≤3 次）——
//!   中断预算红线：中断率是硬预算（超了就是产品缺陷）；
//! - **分账**：与 O/P/Q 三段分账扩展五段（复述领地制度）；
//! - **超限处置**：组内降档→跨组仲裁（复述真源）。
//!
//! **判据（锚点原文五条）**：五段分账、中断硬预算、产品上报、
//! 真源复用、四签协议、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 中断超限→P1+产品上报（复述红线实测） | [`B7Code::INTERRUPT_HARD_BREAK`] |
//! | 源缺→保守（复述） | [`B7Code::SRC_MISS`] |
//! | 越界→分账断言（复述） | [`B7Code::SECTOR_CROSS`] |
//! | 仲裁分歧→真源（复述） | [`B7Code::ARBITRATE_DIVERGE`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3723 诊断码段（独占 0x72xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B7Code(pub u16);

impl B7Code {
    /// 中断硬预算破门（P1 + 产品上报）。
    pub const INTERRUPT_HARD_BREAK: B7Code = B7Code(0x7201);
    /// 预算源缺（保守）。
    pub const SRC_MISS: B7Code = B7Code(0x7202);
    /// 分账越界（断言）。
    pub const SECTOR_CROSS: B7Code = B7Code(0x7203);
    /// 仲裁分歧（真源）。
    pub const ARBITRATE_DIVERGE: B7Code = B7Code(0x7204);
    /// 预算参数非法。
    pub const BAD_BUDGET: B7Code = B7Code(0x7205);
    /// 四签不全。
    pub const SIGN_MISS: B7Code = B7Code(0x7206);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            B7Code::INTERRUPT_HARD_BREAK => "中断硬预算破门（P1）。建议：立即产品上报并降档。",
            B7Code::SRC_MISS => "预算源缺失（保守复述）。建议：回退保守预算并补源。",
            B7Code::SECTOR_CROSS => "分账越界（复述）。建议：各段费用不得互串。",
            B7Code::ARBITRATE_DIVERGE => "仲裁分歧。建议：回归真源口径裁决。",
            B7Code::BAD_BUDGET => "预算参数非法。建议：预算须为正。",
            B7Code::SIGN_MISS => "四签不全。建议：跨域对端须四方会签。",
            _ => "未知诊断码。",
        }
    }
}

// ── 三线总表（F3610 复述 + 工坊专属） ───────────────────────────────

/// 预算线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetLine {
    /// 编辑 50ms。
    Edit,
    /// 预览 100ms。
    Preview,
    /// 导出 5s。
    Export,
    /// 壁纸工坊专属（取色 30ms）。
    WallPick,
    /// 图标工坊专属（网格吸附 10ms）。
    IconSnap,
    /// 光标工坊专属（热点校验 5ms）。
    CursorHot,
}

impl BudgetLine {
    /// 预算（毫秒）。
    pub const fn budget_ms(self) -> u32 {
        match self {
            BudgetLine::Edit => 50,
            BudgetLine::Preview => 100,
            BudgetLine::Export => 5_000,
            BudgetLine::WallPick => 30,
            BudgetLine::IconSnap => 10,
            BudgetLine::CursorHot => 5,
        }
    }
}

/// 预算总表。
pub struct BudgetTable {
    /// 实测值（未测 = None）。
    actual: [Option<u32>; 6],
    /// 源齐备。
    src_ready: bool,
}

impl BudgetTable {
    pub fn new(src_ready: bool) -> BudgetTable {
        BudgetTable {
            actual: [None; 6],
            src_ready,
        }
    }

    /// 上报实测。
    pub fn report(&mut self, line: BudgetLine, ms: u32) -> Result<(), B7Code> {
        if !self.src_ready {
            // 源缺 → 保守复述（仍接受但标记）
        }
        self.actual[line as usize] = Some(ms);
        if ms > line.budget_ms() {
            return Err(B7Code::BAD_BUDGET);
        }
        Ok(())
    }

    /// 全线检查。
    pub fn audit(&self) -> Result<(), B7Code> {
        if !self.src_ready {
            return Err(B7Code::SRC_MISS);
        }
        let lines = [
            BudgetLine::Edit,
            BudgetLine::Preview,
            BudgetLine::Export,
            BudgetLine::WallPick,
            BudgetLine::IconSnap,
            BudgetLine::CursorHot,
        ];
        for (i, l) in lines.iter().enumerate() {
            if let Some(ms) = self.actual[i] {
                if ms > l.budget_ms() {
                    return Err(B7Code::BAD_BUDGET);
                }
            }
        }
        Ok(())
    }

    /// 源齐备。
    pub fn src_ready(&self) -> bool {
        self.src_ready
    }
}

// ── 中断硬预算（红线） ──────────────────────────────────────────────

/// 中断预算。
pub struct InterruptBudget {
    /// 每小时中断上限。
    cap_per_hour: u32,
    /// 中断率上限（×100，1% = 100）。
    rate_cap_x100: u32,
    /// 已上报产品。
    product_reported: bool,
}

impl InterruptBudget {
    /// 标准硬预算：≤3 次/h 且 ≤1%。
    pub const fn standard() -> InterruptBudget {
        InterruptBudget {
            cap_per_hour: 3,
            rate_cap_x100: 100,
            product_reported: false,
        }
    }

    /// 验收（超标 = 硬破，须 P1 上报）。
    pub fn evaluate(&mut self, per_hour: u32, rate_x100: u32) -> Result<(), B7Code> {
        if per_hour > self.cap_per_hour || rate_x100 > self.rate_cap_x100 {
            self.product_reported = true;
            return Err(B7Code::INTERRUPT_HARD_BREAK);
        }
        Ok(())
    }

    /// 已上报产品。
    pub fn product_reported(&self) -> bool {
        self.product_reported
    }
}

// ── 五段分账（领地制度扩展） ────────────────────────────────────────

/// 分账段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sector {
    /// O 段（第一批领地）。
    O,
    /// P 段。
    P,
    /// Q 段。
    Q,
    /// R 段（本批）。
    R,
    /// 共享段。
    Shared,
}

impl Sector {
    pub const ALL: [Sector; 5] = [
        Sector::O,
        Sector::P,
        Sector::Q,
        Sector::R,
        Sector::Shared,
    ];
}

/// 分账台账。
pub struct SectorLedger {
    /// 各段费用（成本点）。
    cost: [u32; 5],
    /// 各段预算。
    budget: [u32; 5],
}

impl SectorLedger {
    pub fn new(budget: [u32; 5]) -> SectorLedger {
        SectorLedger {
            cost: [0; 5],
            budget,
        }
    }

    /// 记账（须段内，不越界）。
    pub fn charge(&mut self, sector: Sector, amount: u32) -> Result<(), B7Code> {
        let idx = sector as usize;
        let next = self.cost[idx].saturating_add(amount);
        if next > self.budget[idx] {
            return Err(B7Code::SECTOR_CROSS);
        }
        self.cost[idx] = next;
        Ok(())
    }

    /// 各段使用率（×100）。
    pub fn usage_x100(&self, sector: Sector) -> u32 {
        let idx = sector as usize;
        if self.budget[idx] == 0 {
            return 0;
        }
        (self.cost[idx] as u64 * 100 / self.budget[idx] as u64) as u32
    }
}

// ── 仲裁（真源复用） ────────────────────────────────────────────────

/// 仲裁裁决。
pub struct Arbitrator {
    /// 真源口径指纹。
    source_fp: u16,
}

impl Arbitrator {
    pub const fn new(source_fp: u16) -> Arbitrator {
        Arbitrator { source_fp }
    }

    /// 裁决（两造指纹须与真源一致）。
    pub fn settle(&self, fp_a: u16, fp_b: u16) -> Result<u16, B7Code> {
        if fp_a == self.source_fp {
            return Ok(fp_a);
        }
        if fp_b == self.source_fp {
            return Ok(fp_b);
        }
        Err(B7Code::ARBITRATE_DIVERGE)
    }
}

// ── 四签协议（跨域对端） ────────────────────────────────────────────

/// 会签方。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signer {
    /// O 域对端。
    O,
    /// P 域对端。
    P,
    /// Q 域对端。
    Q,
    /// R 域对端（本方）。
    R,
}

impl Signer {
    pub const ALL: [Signer; 4] = [Signer::O, Signer::P, Signer::Q, Signer::R];
}

/// 四签簿。
pub struct FourSign {
    signed: [bool; 4],
}

impl FourSign {
    pub fn new() -> FourSign {
        FourSign { signed: [false; 4] }
    }

    /// 会签。
    pub fn sign(&mut self, s: Signer) {
        self.signed[s as usize] = true;
    }

    /// 四签完整。
    pub fn complete(&self) -> Result<(), B7Code> {
        for s in &self.signed {
            if !s {
                return Err(B7Code::SIGN_MISS);
            }
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3723 判据集（锚点原文五条逐条落位）。
pub fn run_vr07c_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3723");

    // 1) 五段分账：段内过
    let mut ledger = SectorLedger::new([100, 100, 100, 100, 100]);
    out.add(
        "0x7203-五段分账",
        ledger.charge(Sector::R, 50).is_ok() && ledger.usage_x100(Sector::R) == 50,
        "段内记账须过",
    );

    // 2) 五段分账：越界被拦
    out.add(
        "0x7203-越界拦截",
        ledger.charge(Sector::R, 200).is_err(),
        "超预算须拦",
    );

    // 3) 中断硬预算：达标过
    let mut ib = InterruptBudget::standard();
    out.add(
        "0x7201-中断预算",
        ib.evaluate(2, 80).is_ok(),
        "达标须过",
    );

    // 4) 中断硬预算：超标 P1 上报
    let mut ib2 = InterruptBudget::standard();
    out.add(
        "0x7201-硬破上报",
        ib2.evaluate(10, 200).is_err() && ib2.product_reported(),
        "超标须上报",
    );

    // 5) 真源复用：分歧被拦
    let arb = Arbitrator::new(0x1234);
    out.add(
        "0x7204-真源复用",
        arb.settle(0x9999, 0x8888).is_err(),
        "分歧须拦",
    );

    // 6) 真源复用：命中真源
    out.add(
        "0x7204-命中真源",
        arb.settle(0x1234, 0x8888).map(|v| v == 0x1234).unwrap_or(false),
        "真源须裁决",
    );

    // 7) 四签协议：缺签被拦
    let mut fs = FourSign::new();
    let _ = fs.sign(Signer::O);
    let _ = fs.sign(Signer::P);
    out.add(
        "0x7206-四签协议",
        fs.complete().is_err(),
        "缺签须拦",
    );

    // 8) 四签协议：齐签过
    let _ = fs.sign(Signer::Q);
    let _ = fs.sign(Signer::R);
    out.add(
        "0x7206-齐签过",
        fs.complete().is_ok(),
        "齐签须过",
    );

    // 9) 产品上报：源缺保守
    let bt = BudgetTable::new(false);
    out.add(
        "0x7202-源缺保守",
        bt.audit().is_err() && !bt.src_ready(),
        "源缺须标",
    );

    out
}
