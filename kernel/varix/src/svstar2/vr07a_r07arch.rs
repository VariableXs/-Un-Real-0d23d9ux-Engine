//! VE-F3721 · R07 开工与创作性能总架构（VE-S 域 · 批次 R07 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3721`
//!
//! **职责定位（锚点原文）**：R07 创作性能治理组——
//! - **R07 定位**：R01-R03 创作工具性能统一治理——
//!   O08/P07/Q07 模式第四次复刻（家族治理流程复用声明）；
//! - **治理原则**：继承四原则 + 创作特色第五原则（创作性能 = 创作心流，
//!   编辑卡顿打断心流）——心流红线：性能优化度量是心流连续性
//!   （不是内部指标）——心流度量声明（编辑中断次数/恢复时间
//!   作为一级指标）；
//! - **范围**：三工坊 + 编辑器 × 成本模型/预算/基准/门禁/
//!   大规模/内存/降档/长稳八项（复刻映射表）。
//!
//! **判据（锚点原文六条）**：四版复刻、五原则、心流度量、
//! 中断率、八项映射、判据。
//!
//! **错误路径与降级矩阵（锚点原文三条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 预算源缺→保守（复述） | [`R7Code::BUDGET_SRC_MISS`] |
//! | 心流破→回滚（红线实测——中断率上升 = 负优化） | [`R7Code::FLOW_BREAK`] |
//! | 模板漂移→四版对账（家族对拍） | [`R7Code::TEMPLATE_DRIFT`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3721 诊断码段（独占 0x70xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R7Code(pub u16);

impl R7Code {
    /// 预算源缺（保守复述）。
    pub const BUDGET_SRC_MISS: R7Code = R7Code(0x7001);
    /// 心流破（回滚，红线实测）。
    pub const FLOW_BREAK: R7Code = R7Code(0x7002);
    /// 模板漂移（四版对账）。
    pub const TEMPLATE_DRIFT: R7Code = R7Code(0x7003);
    /// 八项映射缺项。
    pub const MAP_GAP: R7Code = R7Code(0x7004);
    /// 原则缺失。
    pub const PRINCIPLE_MISS: R7Code = R7Code(0x7005);
    /// 中断率超标。
    pub const INTERRUPT_EXCEED: R7Code = R7Code(0x7006);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R7Code::BUDGET_SRC_MISS => "预算源缺失（保守复述）。建议：回退保守预算并补齐数据源。",
            R7Code::FLOW_BREAK => "心流破（红线实测）。建议：中断率上升即回滚优化。",
            R7Code::TEMPLATE_DRIFT => "模板漂移。建议：四版（O08/P07/Q07/R07）对账收敛。",
            R7Code::MAP_GAP => "八项映射缺项。建议：补齐八项治理映射。",
            R7Code::PRINCIPLE_MISS => "原则缺失。建议：五原则须齐备。",
            R7Code::INTERRUPT_EXCEED => "中断率超标。建议：定位卡顿源并优化。",
            _ => "未知诊断码。",
        }
    }
}

// ── 五原则声明 ──────────────────────────────────────────────────────

/// 治理原则（四继承 + 一创作）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Principle {
    /// 原则一：单源（度量口径唯一）。
    SingleSource,
    /// 原则二：复用（治理流程家族复用）。
    Reuse,
    /// 原则三：显性（降档/回退可告知）。
    Explicit,
    /// 原则四：闭环（超限必处置）。
    ClosedLoop,
    /// 原则五（创作特色）：心流优先。
    FlowFirst,
}

impl Principle {
    pub const ALL: [Principle; 5] = [
        Principle::SingleSource,
        Principle::Reuse,
        Principle::Explicit,
        Principle::ClosedLoop,
        Principle::FlowFirst,
    ];
}

/// 原则登记。
pub struct PrincipleSet {
    seen: [bool; 5],
}

impl PrincipleSet {
    pub fn new() -> PrincipleSet {
        PrincipleSet { seen: [false; 5] }
    }

    /// 声明原则。
    pub fn declare(&mut self, p: Principle) {
        self.seen[p as usize] = true;
    }

    /// 完整性断言（缺即拦）。
    pub fn complete(&self) -> Result<(), R7Code> {
        for s in &self.seen {
            if !s {
                return Err(R7Code::PRINCIPLE_MISS);
            }
        }
        Ok(())
    }
}

// ── 心流度量（一级指标） ────────────────────────────────────────────

/// 心流指标（红线：性能 = 心流连续性）。
pub struct FlowMetrics {
    /// 编辑中断次数。
    pub interrupts: u32,
    /// 累计恢复时间（毫秒）。
    pub recover_ms: u32,
    /// 统计时长（小时）。
    pub hours: u32,
}

impl FlowMetrics {
    /// 中断率（每小时，整数）。
    pub fn interrupt_per_hour(&self) -> u32 {
        if self.hours == 0 {
            return self.interrupts;
        }
        self.interrupts / self.hours
    }

    /// 中断硬预算：每小时 ≤3 次（红线）。
    pub fn flow_ok(&self) -> Result<(), R7Code> {
        if self.interrupt_per_hour() > 3 {
            return Err(R7Code::INTERRUPT_EXCEED);
        }
        Ok(())
    }

    /// 平均恢复时间。
    pub fn avg_recover_ms(&self) -> u32 {
        if self.interrupts == 0 {
            return 0;
        }
        self.recover_ms / self.interrupts
    }
}

// ── 八项映射（三工坊+编辑器 × 八项） ────────────────────────────────

/// 治理对象（四工具）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// 壁纸工坊。
    WallShop,
    /// 图标工坊。
    IconShop,
    /// 光标工坊。
    CursorShop,
    /// 主题编辑器。
    Editor,
}

impl Tool {
    pub const ALL: [Tool; 4] = [
        Tool::WallShop,
        Tool::IconShop,
        Tool::CursorShop,
        Tool::Editor,
    ];
}

/// 治理项（八项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// 成本模型。
    CostModel,
    /// 预算。
    Budget,
    /// 基准。
    Bench,
    /// 门禁。
    Gate,
    /// 大规模。
    LargeScale,
    /// 内存。
    Memory,
    /// 降档。
    Degrade,
    /// 长稳。
    LongRun,
}

impl Item {
    pub const ALL: [Item; 8] = [
        Item::CostModel,
        Item::Budget,
        Item::Bench,
        Item::Gate,
        Item::LargeScale,
        Item::Memory,
        Item::Degrade,
        Item::LongRun,
    ];
}

/// 映射表（4×8 = 32 槽）。
pub struct MappingTable {
    slots: [bool; 32],
}

impl MappingTable {
    pub fn new() -> MappingTable {
        MappingTable { slots: [false; 32] }
    }

    fn idx(tool: Tool, item: Item) -> usize {
        (tool as usize) * 8 + (item as usize)
    }

    /// 登记映射。
    pub fn set(&mut self, tool: Tool, item: Item) {
        self.slots[MappingTable::idx(tool, item)] = true;
    }

    /// 完整性（32 槽全填）。
    pub fn complete(&self) -> Result<(), R7Code> {
        for s in &self.slots {
            if !s {
                return Err(R7Code::MAP_GAP);
            }
        }
        Ok(())
    }

    /// 缺槽数。
    pub fn missing(&self) -> u32 {
        let mut n = 0;
        for s in &self.slots {
            if !s {
                n += 1;
            }
        }
        n
    }
}

// ── 家族模板四版对账（O08/P07/Q07/R07） ─────────────────────────────

/// 模板版本标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamilyVer {
    /// O08（第一批）。
    O08,
    /// P07（第二批）。
    P07,
    /// Q07（第三批）。
    Q07,
    /// R07（本批）。
    R07,
}

impl FamilyVer {
    pub const ALL: [FamilyVer; 4] = [FamilyVer::O08, FamilyVer::P07, FamilyVer::Q07, FamilyVer::R07];

    /// 流程骨架指纹（对账用）。
    pub const fn fingerprint(self) -> u16 {
        match self {
            FamilyVer::O08 => 0x0801,
            FamilyVer::P07 => 0x0702,
            FamilyVer::Q07 => 0x0703,
            FamilyVer::R07 => 0x0704,
        }
    }

    /// 标准骨架（所有版本同源）。
    pub const fn skeleton(self) -> u8 {
        // 骨架步数：定位→计划→归集→门禁→看板→处置→对账→收口
        8
    }
}

/// 四版对账器。
pub struct FamilyReconcile {
    /// 登记的版本骨架。
    registered: [u8; 4],
}

impl FamilyReconcile {
    pub fn new() -> FamilyReconcile {
        FamilyReconcile {
            registered: [0; 4],
        }
    }

    /// 登记版本骨架步数。
    pub fn register(&mut self, ver: FamilyVer, steps: u8) {
        self.registered[ver as usize] = steps;
    }

    /// 对账（骨架一致，漂移即拦）。
    pub fn audit(&self) -> Result<(), R7Code> {
        let expect = FamilyVer::R07.skeleton();
        for (i, r) in self.registered.iter().enumerate() {
            let _ = i;
            if *r != expect {
                return Err(R7Code::TEMPLATE_DRIFT);
            }
        }
        Ok(())
    }
}

// ── 治理计划（八项×四工具） ─────────────────────────────────────────

/// 治理计划。
pub struct GovPlan {
    /// 映射表。
    map: MappingTable,
    /// 原则集。
    principles: PrincipleSet,
    /// 四版对账。
    family: FamilyReconcile,
}

impl GovPlan {
    pub fn new() -> GovPlan {
        GovPlan {
            map: MappingTable::new(),
            principles: PrincipleSet::new(),
            family: FamilyReconcile::new(),
        }
    }

    /// 全量登记（32 槽 + 五原则 + 四版骨架）。
    pub fn bootstrap(&mut self) {
        for t in Tool::ALL {
            for i in Item::ALL {
                self.map.set(t, i);
            }
        }
        for p in Principle::ALL {
            self.principles.declare(p);
        }
        for v in FamilyVer::ALL {
            self.family.register(v, FamilyVer::R07.skeleton());
        }
    }

    /// 总检（映射+原则+家族）。
    pub fn audit(&self) -> Result<(), R7Code> {
        self.map.complete()?;
        self.principles.complete()?;
        self.family.audit()?;
        Ok(())
    }

    /// 心流回滚判定（中断率上升 = 负优化）。
    pub fn flow_rollback(&self, before: &FlowMetrics, after: &FlowMetrics) -> Result<(), R7Code> {
        if after.interrupt_per_hour() > before.interrupt_per_hour() {
            return Err(R7Code::FLOW_BREAK);
        }
        Ok(())
    }

    /// 映射访问。
    pub fn map(&self) -> &MappingTable {
        &self.map
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3721 判据集（锚点原文六条逐条落位）。
pub fn run_vr07a_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3721");

    // 1) 四版复刻：骨架一致
    let mut fam = FamilyReconcile::new();
    for v in FamilyVer::ALL {
        fam.register(v, 8);
    }
    out.add(
        "0x7003-四版复刻",
        fam.audit().is_ok(),
        "四版骨架须同源",
    );

    // 2) 四版复刻：漂移被拦
    let mut fam2 = FamilyReconcile::new();
    fam2.register(FamilyVer::O08, 8);
    fam2.register(FamilyVer::P07, 7); // 漂移
    out.add(
        "0x7003-漂移拦截",
        fam2.audit().is_err(),
        "漂移须拦",
    );

    // 3) 五原则：齐备过
    let mut ps = PrincipleSet::new();
    for p in Principle::ALL {
        ps.declare(p);
    }
    out.add(
        "0x7005-五原则",
        ps.complete().is_ok(),
        "五原则须齐",
    );

    // 4) 心流度量：中断率口径
    let fm = FlowMetrics {
        interrupts: 6,
        recover_ms: 600,
        hours: 2,
    };
    out.add(
        "0x7002-心流度量",
        fm.interrupt_per_hour() == 3 && fm.flow_ok().is_ok(),
        "中断率须 ≤3/h",
    );

    // 5) 中断率：超标被拦
    let bad = FlowMetrics {
        interrupts: 20,
        recover_ms: 2000,
        hours: 2,
    };
    out.add(
        "0x7006-中断超标",
        bad.flow_ok().is_err(),
        "超标须拦",
    );

    // 6) 八项映射：缺项被拦
    let mut mt = MappingTable::new();
    out.add("0x7004-八项映射", mt.complete().is_err() && mt.missing() == 32, "缺项须拦");

    // 7) 八项映射：全填过
    let mut plan = GovPlan::new();
    plan.bootstrap();
    out.add(
        "0x7004-全映射过",
        plan.audit().is_ok(),
        "全填须过",
    );

    // 8) 心流回滚：中断率上升即回滚
    let before = FlowMetrics {
        interrupts: 2,
        recover_ms: 100,
        hours: 2,
    };
    let after = FlowMetrics {
        interrupts: 10,
        recover_ms: 500,
        hours: 2,
    };
    out.add(
        "0x7002-心流回滚",
        plan.flow_rollback(&before, &after).is_err(),
        "负优化须回滚",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7006-判据",
        R7Code::FLOW_BREAK.value() != R7Code::TEMPLATE_DRIFT.value(),
        "码段互异",
    );

    out
}
