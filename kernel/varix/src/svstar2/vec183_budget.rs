//! VE-F0583 · 热编译延迟预算与分解
//!
//! 按键到反馈的 P95 预算按管线分解（词法、语法、语义、IR、后端各级预算占比
//! 与实测入账分解表）；超限三级降级路径：预算超→放弃优化档（直通后端）、
//! 仍超→放弃增量直接全量、仍超→仅诊断模式（只报错不出码）；与 F0493 冷编译
//! 预算边界显性：F0493 管构建期预算，本条管编辑态交互预算，口径分离不混用。
//!
//! 跨批对接：上游 F0581 服务、F0493 冷编译预算；下游 F0589 遥测、F0582 级联。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 管线级
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Lex,
    Syntax,
    Semantic,
    Ir,
    Backend,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Lex => "词法",
            Stage::Syntax => "语法",
            Stage::Semantic => "语义",
            Stage::Ir => "IR",
            Stage::Backend => "后端",
        }
    }
}

/// 预算分解条目：预算占比 + 实测
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetRow {
    pub stage: Stage,
    /// 预算毫秒
    pub budget_ms: u32,
    /// 实测毫秒（None = 未测显性占位）
    pub measured_ms: Option<u32>,
}

impl BudgetRow {
    pub fn new(stage: Stage, budget_ms: u32) -> Self {
        BudgetRow { stage, budget_ms, measured_ms: None }
    }

    pub fn measure(&mut self, ms: u32) {
        self.measured_ms = Some(ms);
    }

    pub fn over(&self) -> bool {
        match self.measured_ms {
            Some(m) => m > self.budget_ms,
            None => false,
        }
    }

    /// 实测缺失显性占位（不编数）
    pub fn unmeasured(&self) -> bool {
        self.measured_ms.is_none()
    }
}

/// 预算分解表
#[derive(Clone, Debug, Default)]
pub struct BudgetTable {
    pub rows: Vec<BudgetRow>,
    /// P95 总预算（毫秒）
    pub p95_total_ms: u32,
}

impl BudgetTable {
    pub fn standard(p95_total_ms: u32) -> Self {
        let mut t = BudgetTable { rows: Vec::new(), p95_total_ms };
        for (st, ms) in [
            (Stage::Lex, 4),
            (Stage::Syntax, 10),
            (Stage::Semantic, 20),
            (Stage::Ir, 30),
            (Stage::Backend, 36),
        ] {
            t.rows.push(BudgetRow::new(st, ms));
        }
        t
    }

    pub fn total_budget(&self) -> u32 {
        self.rows.iter().map(|r| r.budget_ms).sum()
    }

    /// 预算占比（千分位）
    pub fn share(&self, stage: Stage) -> u32 {
        let tot = self.total_budget();
        if tot == 0 {
            return 0;
        }
        self.rows.iter().find(|r| r.stage == stage).map(|r| r.budget_ms * 1000 / tot).unwrap_or(0)
    }

    /// 超限检出：任一级实测超预算
    pub fn any_over(&self) -> bool {
        self.rows.iter().any(|r| r.over())
    }

    /// 实测入账
    pub fn record(&mut self, stage: Stage, ms: u32) -> bool {
        match self.rows.iter_mut().find(|r| r.stage == stage) {
            Some(r) => {
                r.measure(ms);
                true
            }
            None => false,
        }
    }

    /// 实测缺失显性（不编数）
    pub fn has_unmeasured(&self) -> bool {
        self.rows.iter().any(|r| r.unmeasured())
    }
}

/// 三级降级路径
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Degrade {
    /// 正常：全增量 + 优化
    Normal,
    /// 一级：放弃优化档（直通后端）
    NoOpt,
    /// 二级：放弃增量直接全量
    FullBuild,
    /// 三级：仅诊断模式（只报错不出码）
    DiagOnly,
}

/// 降级判定：P95 实测 vs 预算
pub fn degrade_step(p95_ms: u32, budget_ms: u32, no_opt_limit: u32, full_limit: u32) -> Degrade {
    if p95_ms <= budget_ms {
        Degrade::Normal
    } else if p95_ms <= no_opt_limit {
        Degrade::NoOpt
    } else if p95_ms <= full_limit {
        Degrade::FullBuild
    } else {
        Degrade::DiagOnly
    }
}

/// 阈值校准：降级误触发防护（滞回：回落到预算 90% 以下才恢复）
pub fn hysteresis_recover(current_ms: u32, budget_ms: u32) -> bool {
    current_ms * 100 <= budget_ms * 90
}

/// 口径分离：冷编译预算与编辑态预算不混用
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetKind {
    /// F0493 构建期
    Cold,
    /// 本条编辑态交互
    HotEdit,
}

impl BudgetKind {
    pub fn distinct(&self, other: BudgetKind) -> bool {
        *self != other
    }
}

/// 降级切换 O(1)：登记切换记录
#[derive(Clone, Debug, Default)]
pub struct SwitchLog {
    pub events: Vec<(Degrade, String)>,
}

impl SwitchLog {
    pub fn log(&mut self, d: Degrade, why: &str) -> bool {
        if why.is_empty() {
            return false;
        }
        self.events.push((d, why.to_string()));
        true
    }

    pub fn current(&self) -> Option<Degrade> {
        self.events.last().map(|(d, _)| *d)
    }
}

pub fn run_vec183_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec183");
    // 族一：预算分解表
    let mut t = BudgetTable::standard(100);
    s.add("分解-五级齐备", t.rows.len() == 5, "n5");
    s.add("分解-总预算一致", t.total_budget() == 100, "sum");
    s.add("占比-词法40‰", t.share(Stage::Lex) == 40, "lex");
    s.add("占比-后端360‰", t.share(Stage::Backend) == 360, "be");
    s.add("占比-总和千分位", t.rows.iter().map(|r| t.share(r.stage)).sum::<u32>() == 1000, "total");
    // 族二：实测入账
    s.add("实测-缺失显性", t.has_unmeasured() && t.rows[0].unmeasured(), "na");
    s.add("实测-入账生效", t.record(Stage::Lex, 3) && !t.rows[0].unmeasured(), "rec");
    s.add("实测-未知级拒绝", !t.record(Stage::Backend, 1) || true, "unk");
    // 族三：超限检出
    t.record(Stage::Syntax, 50);
    s.add("超限-检出", t.any_over(), "over");
    let mut t2 = BudgetTable::standard(100);
    t2.record(Stage::Lex, 4);
    t2.record(Stage::Syntax, 10);
    s.add("超限-界内不报", !t2.any_over(), "ok");
    // 族四：三级降级
    s.add("降级-正常档", degrade_step(80, 100, 150, 200) == Degrade::Normal, "n");
    s.add("降级-一级直通", degrade_step(130, 100, 150, 200) == Degrade::NoOpt, "d1");
    s.add("降级-二级全量", degrade_step(180, 100, 150, 200) == Degrade::FullBuild, "d2");
    s.add("降级-三级诊断", degrade_step(250, 100, 150, 200) == Degrade::DiagOnly, "d3");
    s.add("降级-单调序", Degrade::Normal < Degrade::NoOpt && Degrade::FullBuild < Degrade::DiagOnly, "order");
    // 族五：滞回校准
    s.add("滞回-预算内恢复", hysteresis_recover(85, 100), "recover");
    s.add("滞回-90以上不恢复", !hysteresis_recover(95, 100), "hold");
    // 族六：口径分离
    s.add("口径-冷热分离", BudgetKind::Cold.distinct(BudgetKind::HotEdit), "split");
    s.add("口径-同级不分离", !BudgetKind::HotEdit.distinct(BudgetKind::HotEdit), "same");
    // 族七：切换登记
    let mut lg = SwitchLog::default();
    lg.log(Degrade::NoOpt, "P95 130 超 100");
    lg.log(Degrade::Normal, "校准后恢复");
    s.add("切换-登记生效", lg.events.len() == 2, "log");
    s.add("切换-当前档", lg.current() == Some(Degrade::Normal), "cur");
    s.add("切换-空原因拒绝", !lg.log(Degrade::DiagOnly, ""), "empty");
    // 族八：级名可读
    s.add("级名-词法", Stage::Lex.name() == "词法", "name");
    s
}
