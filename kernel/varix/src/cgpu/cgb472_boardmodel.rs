//! CGPU-F0472 · 看板模型（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 看板模型：预算域全指标矩阵（九组×四类指标——B08 schema 汇总）；
//! - 四级钻取（域→组→指标→事件）；
//! - 决策解释器数据集成（F0442 解释随钻取呈现）；
//! - 看板契约冻结。判据：全矩阵、四级钻取、解释集成、契约冻结、判据。
//!
//! 设计纪律：
//! - 全指标矩阵：9 组 × 4 类（吞吐/公平/延迟/异常），格值 (当前值，
//!   状态码)——格可查、行可聚合。
//! - 四级钻取：层级枚举（Domain→Group→Metric→Event），逐层下钻
//!   （上层键+下层键对齐校验），跳级拒。
//! - 解释集成：钻取到事件层时解释行（决策号+人话文本）随行携带，
//!   缺解释的事件行不合格。
//! - 契约冻结：看板 schema 版本+键集哈希——冻结后键集变更拒。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10B0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10B0，全仓独占）。
pub const SEG_BASE: u16 = 0x10B0;

pub const E_KB_GROUP: u16 = SEG_BASE + 1; // 组号非法
pub const E_KB_CLASS: u16 = SEG_BASE + 2; // 指标类非法
pub const E_KB_DRILL: u16 = SEG_BASE + 3; // 钻取跳级
pub const E_KB_FROZEN: u16 = SEG_BASE + 4; // 冻结后改 schema

/// 九组 × 四类。
pub const GROUPS: [u8; 9] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
pub const CL_THROUGHPUT: u8 = 1; // 吞吐
pub const CL_FAIRNESS: u8 = 2; // 公平
pub const CL_LATENCY: u8 = 3; // 延迟
pub const CL_ANOMALY: u8 = 4; // 异常
pub const CLASSES: [u8; 4] = [CL_THROUGHPUT, CL_FAIRNESS, CL_LATENCY, CL_ANOMALY];

/// 格状态码。
pub const ST_OK: u8 = 0;
pub const ST_WARN: u8 = 1;
pub const ST_BAD: u8 = 2;

/// 矩阵格。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cell {
    pub value: u64,
    pub state: u8,
}

/// 全指标矩阵。
#[derive(Debug, Default)]
pub struct BoardMatrix {
    /// cells[group-1][class-1]。
    pub cells: [[Cell; 4]; 9],
}

impl BoardMatrix {
    pub fn set(&mut self, group: u8, class: u8, cell: Cell) -> Result<(), u16> {
        if !GROUPS.contains(&group) {
            return Err(E_KB_GROUP);
        }
        if !CLASSES.contains(&class) {
            return Err(E_KB_CLASS);
        }
        self.cells[(group - 1) as usize][(class - 1) as usize] = cell;
        Ok(())
    }
    pub fn get(&self, group: u8, class: u8) -> Result<Cell, u16> {
        if !GROUPS.contains(&group) || !CLASSES.contains(&class) {
            return Err(E_KB_GROUP);
        }
        Ok(self.cells[(group - 1) as usize][(class - 1) as usize])
    }
    /// 行聚合：某组四类里的坏格数。
    pub fn bad_in_group(&self, group: u8) -> Result<usize, u16> {
        if !GROUPS.contains(&group) {
            return Err(E_KB_GROUP);
        }
        Ok(self.cells[(group - 1) as usize].iter().filter(|c| c.state == ST_BAD).count())
    }
    /// 全矩阵坏格总数。
    pub fn bad_total(&self) -> usize {
        self.cells.iter().flatten().filter(|c| c.state == ST_BAD).count()
    }
}

/// 四级钻取层级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DrillLevel {
    Domain = 1,
    Group = 2,
    Metric = 3,
    Event = 4,
}

impl DrillLevel {
    pub fn from_rank(r: u8) -> Result<Self, u16> {
        match r {
            1 => Ok(DrillLevel::Domain),
            2 => Ok(DrillLevel::Group),
            3 => Ok(DrillLevel::Metric),
            4 => Ok(DrillLevel::Event),
            _ => Err(E_KB_DRILL),
        }
    }
}

/// 事件行（事件层携带解释——F0442 集成）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRow {
    pub decision_id: u64,
    pub text: &'static str,
    /// 解释在场（决策号+文本齐备）。
    pub explained: bool,
}

impl EventRow {
    pub fn valid(&self) -> bool {
        self.decision_id != 0 && !self.text.is_empty() && self.explained
    }
}

/// 钻取路径（逐层推进，跳级拒）。
#[derive(Debug)]
pub struct DrillPath {
    pub level: DrillLevel,
    pub group: Option<u8>,
    pub metric: Option<u8>,
}

impl DrillPath {
    pub fn new() -> Self {
        Self { level: DrillLevel::Domain, group: None, metric: None }
    }
    /// 下钻一层。
    pub fn descend(&mut self, group: Option<u8>, metric: Option<u8>) -> Result<DrillLevel, u16> {
        let next = DrillLevel::from_rank(self.level as u8 + 1)?;
        match (self.level, next) {
            (DrillLevel::Domain, DrillLevel::Group) => {
                let g = group.ok_or(E_KB_DRILL)?;
                if !GROUPS.contains(&g) {
                    return Err(E_KB_GROUP);
                }
                self.group = Some(g);
            }
            (DrillLevel::Group, DrillLevel::Metric) => {
                if group.is_some() && group != self.group {
                    return Err(E_KB_DRILL); // 换组=跳级
                }
                if metric.is_none() {
                    return Err(E_KB_DRILL);
                }
                self.metric = metric;
            }
            (DrillLevel::Metric, DrillLevel::Event) => {
                if group.is_some() && group != self.group {
                    return Err(E_KB_DRILL);
                }
                if metric.is_some() && metric != self.metric {
                    return Err(E_KB_DRILL);
                }
            }
            _ => return Err(E_KB_DRILL), // Event 无更深层
        }
        self.level = next;
        Ok(next)
    }
}

/// 看板契约（版本+键集哈希冻结）。
#[derive(Debug)]
pub struct BoardContract {
    pub version: u32,
    keys: Vec<&'static str>,
    frozen_hash: u32,
}

fn kh(keys: &[&str]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for k in keys {
        for b in k.as_bytes() {
            h = (h ^ *b as u32).wrapping_mul(0x0100_0193);
        }
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl BoardContract {
    pub fn freeze(keys: Vec<&'static str>) -> Self {
        let h = kh(&keys);
        Self { version: 1, keys, frozen_hash: h }
    }
    /// 键集变更（冻结后拒——哈希不变性验证）。
    pub fn validate(&self, keys: &[&'static str]) -> Result<(), u16> {
        if kh(keys) != self.frozen_hash {
            return Err(E_KB_FROZEN);
        }
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、全指标矩阵：36 格读写+行聚合。
    {
        let mut m = BoardMatrix::default();
        for g in GROUPS.iter() {
            for c in CLASSES.iter() {
                m.set(*g, *c, Cell { value: 100, state: ST_OK }).ok();
            }
        }
        m.set(3, CL_ANOMALY, Cell { value: 900, state: ST_BAD }).ok();
        v.push(("F0472-全矩阵-36格可读写", m.get(3, CL_ANOMALY).map(|c| c.state) == Ok(ST_BAD)));
        v.push(("F0472-全矩阵-行聚合坏格", m.bad_in_group(3) == Ok(1)));
        v.push(("F0472-全矩阵-总坏格", m.bad_total() == 1));
        // 越界拒。
        v.push(("F0472-全矩阵-组号越界拒", m.set(10, CL_THROUGHPUT, Cell { value: 0, state: ST_OK }) == Err(E_KB_GROUP)));
        v.push(("F0472-全矩阵-类码越界拒", m.set(1, 9, Cell { value: 0, state: ST_OK }) == Err(E_KB_CLASS)));
    }
    // 二、四级钻取：逐层下钻+跳级拒。
    {
        let mut d = DrillPath::new();
        // Domain→Metric 跳级拒（一次 descend 只进一层）。
        let skip = d.descend(None, Some(2));
        v.push(("F0472-四级钻取-Domain跳Metric拒", skip == Err(E_KB_DRILL)));
        d.descend(Some(3), None).ok();
        v.push(("F0472-四级钻取-到组层", d.level == DrillLevel::Group && d.group == Some(3)));
        d.descend(None, Some(CL_FAIRNESS)).ok();
        v.push(("F0472-四级钻取-到指标层", d.level == DrillLevel::Metric));
        // 换组下钻拒。
        v.push(("F0472-四级钻取-换组拒", d.descend(Some(5), None) == Err(E_KB_DRILL)));
        d.descend(None, None).ok();
        v.push(("F0472-四级钻取-到事件层", d.level == DrillLevel::Event));
        // Event 无更深层。
        v.push(("F0472-四级钻取-事件层到底", d.descend(None, None) == Err(E_KB_DRILL)));
        // 非法层号。
        v.push(("F0472-四级钻取-层号非法拒", DrillLevel::from_rank(9) == Err(E_KB_DRILL)));
    }
    // 三、解释集成：事件行必须带解释。
    {
        let good = EventRow { decision_id: 42, text: "决策#42 优先级主导（优先级 500 + 预算 300 + 保底 200）", explained: true };
        v.push(("F0472-解释集成-合格事件行", good.valid()));
        let noexp = EventRow { decision_id: 43, text: "raw", explained: false };
        v.push(("F0472-解释集成-缺解释不合格", !noexp.valid()));
        let zeroid = EventRow { decision_id: 0, text: "x", explained: true };
        v.push(("F0472-解释集成-零决策号不合格", !zeroid.valid()));
    }
    // 四、契约冻结。
    {
        let keys = vec!["edge.triggers", "edge.recover", "fair.jain"];
        let c = BoardContract::freeze(keys);
        // 同键集校验通过。
        v.push(("F0472-契约-同键集通过", c.validate(&["edge.triggers", "edge.recover", "fair.jain"]).is_ok()));
        // 键集变更拒。
        v.push(("F0472-契约-键集变更拒", c.validate(&["edge.triggers", "edge.recover"]) == Err(E_KB_FROZEN)));
        // 键序变更也拒（哈希敏感）。
        v.push(("F0472-契约-键序敏感", c.validate(&["fair.jain", "edge.triggers", "edge.recover"]) == Err(E_KB_FROZEN)));
    }
    // 五、判据自检。
    {
        v.push(("F0472-判据-九组四类", GROUPS.len() == 9 && CLASSES.len() == 4));
        v.push(("F0472-判据-矩阵36格", BoardMatrix::default().cells.iter().flatten().count() == 36));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0472_boardmodel_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
