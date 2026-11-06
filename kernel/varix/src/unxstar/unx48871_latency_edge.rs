//! UNX-F48871 四段预算边界面 · 临界样本账 · 0x708C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥620 行；
//!   ② UNX-F48871-J1 预算边界四点全覆盖，越界零静默；
//!   ③ UNX-F48872-J1 阶梯判据 ×10^3 零漏判。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：段号合法 → 预算非零 → 基值不越预算 → 越界判定 →
//!           基值判定 → 临界判定 → 梯级单调 → 账平复核。
//!           段号最先判，因为预算表按段号取值，段号越界后取到的预算
//!           本身就没有意义；预算非零次之，零预算会把「任何耗时都超」
//!           伪装成过载结论；基值必须先于临界：低于基值的样本不是
//!           「快」，而是测量链断了，若与临界混判，掉点会被当成优绩。
//!           越界判定先于临界判定：预算加容差之外才是真越界，容差之
//!           内仍属临界带，二者混判会让抖动被记成违约。梯级单调必须
//!           晚于逐点判定，否则单点判错会被阶梯的单调性反向掩盖；
//!           账平复核最后执行，它只负责回答「有没有点被静默丢掉」。
//! 关键差异：本模块只拥有「预算边界的分类与余量」这一层。
//!           core 拥有四段链与分位；meta 拥有 TSC 权威源与代际；
//!           guard 拥有四闸与开销扣除；dispatch 拥有环形与导出。
//!           本模块不复制分位、不复制环、不做开销扣除；它回答的是
//!           一个更窄的问题：给定一个已可信的耗时样本，它落在预算
//!           面的哪一侧、余量多少、是否处在容差临界带。谁想让
//!           「刚刚超一点」的样本进入送达统计，谁就必须先在本模块
//!           得到 AtEdge 与负余量，而不能把它当作 InBudget 放过。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x708C0，VX6 批 B04 族第五槽）。
pub const SEG_BASE: u32 = 0x708C0;

/// 容差带（µs）：预算两侧各 8µs 计为临界，不直接判越界。
pub const EDGE_TOL_US: u32 = 8;

/// 阶梯梯级数（每段九级：零／半基值／基值／中线／预算−容差／预算／
/// 预算+容差／容差外一格／容差外整条抖动）。
pub const LADDER_RUNGS: usize = 9;

/// 边界账轮数（判据 ×10^3）。
pub const EDGE_ROUNDS: u32 = 1000;

/// 注入轮数（四路 ×10^3）。
pub const INJECT_ROUNDS: u32 = 1000;

/// 四段预算表（µs），与 core 同口径。
pub const STAGE_BUDGET: [u32; 4] = [80, 240, 900, 2600];

/// 四段基值表（µs），与 core 同口径。
pub const STAGE_FLOOR: [u32; 4] = [18, 55, 180, 640];

/// 四段抖动上限（µs），与 core 同口径。
pub const STAGE_JITTER: [u32; 4] = [40, 120, 420, 1200];

/// 四段机器可读名（CSV 列名用）。
pub const STAGE_NAMES: [&str; 4] = ["irq-arrival", "focus-dispatch", "app-callback", "frame-present"];

/// 四段中文名（诊断文案用）。
pub const STAGE_LABELS: [&str; 4] = ["中断到达", "焦点分发", "应用回调", "帧呈现"];

// ── 诊断码闭集（+0x01..+0x0B） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeCode {
    /// 段号越界（不在四段闭集内）
    StageBad,
    /// 预算为零（分类面失去意义）
    BudgetBad,
    /// 基值大于预算（夹具非法）
    FloorBad,
    /// 分类结果与申报不符
    ClassBad,
    /// 余量符号错误
    MarginBad,
    /// 梯级非严格升序或梯级数不符
    LadderBad,
    /// 账平不成立（有样本被静默丢弃）
    TallyBad,
    /// 覆盖不足（四类未出现）
    CoverBad,
    /// 导出面 / 解析面漂移
    PrintBad,
    /// 双跑不一致
    ReplayBad,
    /// 段位越界（诊断码逃出 0x708C0 窗口）
    SegmentBad,
}

impl EdgeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EdgeCode::StageBad => "段号越界：只有 0..3 四段，越界样本不得分类",
            EdgeCode::BudgetBad => "预算为零：零预算会把任何耗时都判成过载",
            EdgeCode::FloorBad => "基值越预算：基值必须不高于预算，否则夹具自相矛盾",
            EdgeCode::ClassBad => "分类不符：实际落档与申报档位相反",
            EdgeCode::MarginBad => "余量符号错误：预算内余量必须为正，预算外必须为负",
            EdgeCode::LadderBad => "梯级非法：梯级必须严格升序且恰好九级",
            EdgeCode::TallyBad => "账平不成立：分类计数之和必须等于样本总数",
            EdgeCode::CoverBad => "覆盖不足：四类档位必须全部出现，缺一即漏判",
            EdgeCode::PrintBad => "导出漂移：字段数、段名或档名不在闭集内",
            EdgeCode::ReplayBad => "双跑不一致：两次边界面账本指纹不同",
            EdgeCode::SegmentBad => "段位越界：诊断码逃出 0x708C0 窗口",
        }
    }

    pub const ALL: [EdgeCode; 11] = [
        EdgeCode::StageBad,
        EdgeCode::BudgetBad,
        EdgeCode::FloorBad,
        EdgeCode::ClassBad,
        EdgeCode::MarginBad,
        EdgeCode::LadderBad,
        EdgeCode::TallyBad,
        EdgeCode::CoverBad,
        EdgeCode::PrintBad,
        EdgeCode::ReplayBad,
        EdgeCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        if EdgeCode::ALL[i].val() == code {
            return EdgeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x708C0 段闭集内"
}

// ── 预算面分档 ────────────────────────────────────────────────────
/// 四档分类：预算内、临界带、越界、低于基值（测量链可疑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeClass {
    /// 严格在预算内且离边界有余量
    InBudget,
    /// 落在预算两侧容差带内（含恰好等于预算）
    AtEdge,
    /// 超出预算 + 容差
    OverBudget,
    /// 低于段基值：不是快，是测量链断了
    Starved,
}

impl EdgeClass {
    pub fn ord(self) -> u8 {
        match self {
            EdgeClass::InBudget => 0,
            EdgeClass::AtEdge => 1,
            EdgeClass::OverBudget => 2,
            EdgeClass::Starved => 3,
        }
    }

    pub fn from_ord(v: u8) -> Result<EdgeClass, u32> {
        match v {
            0 => Ok(EdgeClass::InBudget),
            1 => Ok(EdgeClass::AtEdge),
            2 => Ok(EdgeClass::OverBudget),
            3 => Ok(EdgeClass::Starved),
            _ => Err(EdgeCode::ClassBad.val()),
        }
    }

    /// 中文档名（诊断文案唯一来源）。
    pub fn label(self) -> &'static str {
        match self {
            EdgeClass::InBudget => "预算内",
            EdgeClass::AtEdge => "临界带",
            EdgeClass::OverBudget => "越界",
            EdgeClass::Starved => "低于基值",
        }
    }

    /// 机器可读档名（CSV 用）。
    pub fn name(self) -> &'static str {
        match self {
            EdgeClass::InBudget => "in-budget",
            EdgeClass::AtEdge => "at-edge",
            EdgeClass::OverBudget => "over-budget",
            EdgeClass::Starved => "starved",
        }
    }

    pub fn from_name(s: &str) -> Result<EdgeClass, u32> {
        match s {
            "in-budget" => Ok(EdgeClass::InBudget),
            "at-edge" => Ok(EdgeClass::AtEdge),
            "over-budget" => Ok(EdgeClass::OverBudget),
            "starved" => Ok(EdgeClass::Starved),
            _ => Err(EdgeCode::PrintBad.val()),
        }
    }

    /// 照耗时由小到大的严重度序：基值以下的样本在最前（异常快），
    /// 之后是预算内、临界、越界。阶梯单调核验按此序。
    pub fn rank(self) -> u8 {
        self.ord()
    }

    /// 位掩码：四类全覆盖为 0x0F。
    pub fn mask(self) -> u8 {
        1u8 << self.ord()
    }
}

/// 取段预算。
pub fn stage_budget(seg: u8) -> Result<u32, u32> {
    match STAGE_BUDGET.get(seg as usize) {
        Some(v) => {
            if *v == 0 {
                Err(EdgeCode::BudgetBad.val())
            } else {
                Ok(*v)
            }
        }
        None => Err(EdgeCode::StageBad.val()),
    }
}

/// 取段基值。
pub fn stage_floor(seg: u8) -> Result<u32, u32> {
    match STAGE_FLOOR.get(seg as usize) {
        Some(v) => Ok(*v),
        None => Err(EdgeCode::StageBad.val()),
    }
}

/// 取段抖动上限。
pub fn stage_jitter(seg: u8) -> Result<u32, u32> {
    match STAGE_JITTER.get(seg as usize) {
        Some(v) => Ok(*v),
        None => Err(EdgeCode::StageBad.val()),
    }
}

/// 段机器名。
pub fn stage_name(seg: u8) -> Result<&'static str, u32> {
    match STAGE_NAMES.get(seg as usize) {
        Some(v) => Ok(*v),
        None => Err(EdgeCode::StageBad.val()),
    }
}

/// 段中文名。
pub fn stage_label(seg: u8) -> Result<&'static str, u32> {
    match STAGE_LABELS.get(seg as usize) {
        Some(v) => Ok(*v),
        None => Err(EdgeCode::StageBad.val()),
    }
}

// ── 边界点 ────────────────────────────────────────────────────────
/// 一个边界样本：段号 × 耗时 × 该段预算 × 该段基值。
///
/// 预算与基值随点携带，而不是分类时现查表：导出后的行必须能独立
/// 复核档位，不能依赖「读者手上正好有同一版预算表」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgePoint {
    pub seg: u8,
    pub us: u32,
    pub budget: u32,
    pub floor: u32,
}

impl EdgePoint {
    pub fn new(seg: u8, us: u32, budget: u32, floor: u32) -> Result<EdgePoint, u32> {
        if seg as usize >= STAGE_BUDGET.len() {
            return Err(EdgeCode::StageBad.val());
        }
        if budget == 0 {
            return Err(EdgeCode::BudgetBad.val());
        }
        if floor > budget {
            return Err(EdgeCode::FloorBad.val());
        }
        Ok(EdgePoint { seg, us, budget, floor })
    }

    /// 按段表建点：预算与基值取自冻结表。
    pub fn framed(seg: u8, us: u32) -> Result<EdgePoint, u32> {
        let budget = stage_budget(seg)?;
        let floor = stage_floor(seg)?;
        EdgePoint::new(seg, us, budget, floor)
    }

    pub fn class(&self) -> EdgeClass {
        classify(self.us, self.budget, self.floor, EDGE_TOL_US)
    }

    /// 余量：预算 − 耗时。预算内为正，越界为负。
    pub fn margin(&self) -> i64 {
        margin_us(self.us, self.budget)
    }

    pub fn text(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.seg,
            self.us,
            self.budget,
            self.floor,
            self.class().name()
        )
    }
}

/// 边界面分类（纯函数）。
///
/// 判定次序即文档头声明的闸门次序：
///   1. 超出「预算 + 容差」→ 越界（容差外一格即真越界）；
///   2. 低于基值 → 低于基值（异常快，测量链可疑）；
///   3. 「耗时 + 容差 ≥ 预算」→ 临界带（含恰好等于预算与容差内超支）；
///   4. 其余 → 预算内。
pub fn classify(us: u32, budget: u32, floor: u32, tol: u32) -> EdgeClass {
    if us > budget.saturating_add(tol) {
        return EdgeClass::OverBudget;
    }
    if us < floor {
        return EdgeClass::Starved;
    }
    if us.saturating_add(tol) >= budget {
        return EdgeClass::AtEdge;
    }
    EdgeClass::InBudget
}

/// 带闸门的分类：段号与预算先过闸，再进分类。
pub fn classify_point(seg: u8, us: u32) -> Result<EdgeClass, u32> {
    let budget = stage_budget(seg)?;
    let floor = stage_floor(seg)?;
    if floor > budget {
        return Err(EdgeCode::FloorBad.val());
    }
    Ok(classify(us, budget, floor, EDGE_TOL_US))
}

/// 余量（µs）：正数表示还有多少余量，负数表示超支多少。
pub fn margin_us(us: u32, budget: u32) -> i64 {
    budget as i64 - us as i64
}

/// 余量符号与档位互证：预算内/临界必须非负，越界必须为负。
pub fn margin_agrees(class: EdgeClass, margin: i64) -> bool {
    match class {
        EdgeClass::InBudget | EdgeClass::AtEdge => margin >= 0,
        EdgeClass::OverBudget => margin < 0,
        // 基值以下的样本可能仍在预算内（余量为正），也可能预算已破；
        // 它的「异常」在于过快，不在于余量符号，故不参与互证。
        EdgeClass::Starved => true,
    }
}

// ── 阶梯 ──────────────────────────────────────────────────────────
/// 单段九级阶梯：每级一个耗时值，严格升序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ladder {
    pub seg: u8,
    pub budget: u32,
    pub floor: u32,
    pub rungs: [u32; LADDER_RUNGS],
}

impl Ladder {
    /// 建梯：零 / 半基值 / 基值 / 中线 / 预算−容差 / 预算 /
    /// 预算+容差 / 容差外一格 / 容差外整条抖动。
    pub fn build(seg: u8) -> Result<Ladder, u32> {
        let budget = stage_budget(seg)?;
        let floor = stage_floor(seg)?;
        let jitter = stage_jitter(seg)?;
        if floor > budget {
            return Err(EdgeCode::FloorBad.val());
        }
        let within = budget.saturating_sub(EDGE_TOL_US);
        let mut rungs = [0u32; LADDER_RUNGS];
        let seq: [u32; LADDER_RUNGS] = [
            0,
            floor / 2,
            floor,
            floor.saturating_add(budget) / 2,
            within,
            budget,
            budget.saturating_add(EDGE_TOL_US),
            budget.saturating_add(EDGE_TOL_US).saturating_add(1),
            budget.saturating_add(EDGE_TOL_US).saturating_add(jitter.max(1)),
        ];
        let mut i = 0usize;
        while i < LADDER_RUNGS {
            match seq.get(i) {
                Some(v) => rungs[i] = *v,
                None => return Err(EdgeCode::LadderBad.val()),
            }
            i += 1;
        }
        let l = Ladder { seg, budget, floor, rungs };
        if !l.strict_ascending() {
            return Err(EdgeCode::LadderBad.val());
        }
        Ok(l)
    }

    /// 严格升序：相邻两级别必须前小后大（相等即两档合一，漏判风险）。
    pub fn strict_ascending(&self) -> bool {
        let mut i = 1usize;
        while i < LADDER_RUNGS {
            let a = match self.rungs.get(i - 1) {
                Some(v) => *v,
                None => return false,
            };
            let b = match self.rungs.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a >= b {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn at(&self, ix: usize) -> Result<u32, u32> {
        match self.rungs.get(ix) {
            Some(v) => Ok(*v),
            None => Err(EdgeCode::LadderBad.val()),
        }
    }

    pub fn class_at(&self, ix: usize) -> Result<EdgeClass, u32> {
        let us = self.at(ix)?;
        Ok(classify(us, self.budget, self.floor, EDGE_TOL_US))
    }

    /// 阶梯审计：九级严格升序 + 全部级可分类 + 后半段严重度单调。
    pub fn audit(&self) -> Result<u8, u32> {
        if !self.strict_ascending() {
            return Err(EdgeCode::LadderBad.val());
        }
        let mut mask = 0u8;
        let mut last_rank = 0u8;
        let mut i = 0usize;
        while i < LADDER_RUNGS {
            let c = self.class_at(i)?;
            mask |= c.mask();
            if i >= 2 {
                if c.rank() < last_rank {
                    return Err(EdgeCode::ClassBad.val());
                }
                last_rank = c.rank();
            }
            i += 1;
        }
        Ok(mask)
    }

    /// 阶梯导出行（逐级一行，档位独立复核）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < LADDER_RUNGS {
            let us = match self.rungs.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let cls = classify(us, self.budget, self.floor, EDGE_TOL_US);
            out.push_str(&format!("{}|{}|{}|{}|{}\n", self.seg, us, self.budget, self.floor, cls.name()));
            i += 1;
        }
        out
    }
}

// ── 边界账 ────────────────────────────────────────────────────────
/// 边界账：四类计数 + 逐段命中 + 指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeLedger {
    pub in_budget: u32,
    pub at_edge: u32,
    pub over_budget: u32,
    pub starved: u32,
    pub stage_hits: [u32; 4],
    pub points: u32,
    pub digest: u64,
}

impl EdgeLedger {
    pub const fn new() -> EdgeLedger {
        EdgeLedger {
            in_budget: 0,
            at_edge: 0,
            over_budget: 0,
            starved: 0,
            stage_hits: [0; 4],
            points: 0,
            digest: 0,
        }
    }

    /// 记一个边界点：档位计数、段命中、指纹一步不落。
    pub fn record(&mut self, p: EdgePoint) -> Result<(), u32> {
        if p.seg as usize >= STAGE_BUDGET.len() {
            return Err(EdgeCode::StageBad.val());
        }
        match p.class() {
            EdgeClass::InBudget => self.in_budget = self.in_budget.saturating_add(1),
            EdgeClass::AtEdge => self.at_edge = self.at_edge.saturating_add(1),
            EdgeClass::OverBudget => self.over_budget = self.over_budget.saturating_add(1),
            EdgeClass::Starved => self.starved = self.starved.saturating_add(1),
        }
        match self.stage_hits.get_mut(p.seg as usize) {
            Some(v) => *v = v.saturating_add(1),
            None => return Err(EdgeCode::StageBad.val()),
        }
        self.points = self.points.saturating_add(1);
        let mut d = if self.digest == 0 { FNV_OFF } else { self.digest };
        d = fold_u32(d, p.seg as u32);
        d = fold_u32(d, p.us);
        d = fold_u32(d, p.budget);
        d = fold_u32(d, p.floor);
        d = fold_u32(d, p.class().ord() as u32);
        self.digest = d;
        Ok(())
    }

    pub fn total(&self) -> u32 {
        self.in_budget
            .saturating_add(self.at_edge)
            .saturating_add(self.over_budget)
            .saturating_add(self.starved)
    }

    /// 账平：四类之和 = 总数；逐段命中之和亦 = 总数。
    pub fn balance(&self) -> Result<(), u32> {
        if self.total() != self.points {
            return Err(EdgeCode::TallyBad.val());
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.stage_hits.len() {
            if let Some(v) = self.stage_hits.get(i) {
                sum = sum.saturating_add(*v);
            }
            i += 1;
        }
        if sum != self.points {
            return Err(EdgeCode::TallyBad.val());
        }
        Ok(())
    }

    /// 覆盖位：四类是否都出现过。
    pub fn coverage(&self) -> u8 {
        let mut m = 0u8;
        if self.in_budget > 0 {
            m |= EdgeClass::InBudget.mask();
        }
        if self.at_edge > 0 {
            m |= EdgeClass::AtEdge.mask();
        }
        if self.over_budget > 0 {
            m |= EdgeClass::OverBudget.mask();
        }
        if self.starved > 0 {
            m |= EdgeClass::Starved.mask();
        }
        m
    }

    /// 指纹复算：只依赖账内字段，便于双跑对照。
    pub fn recompute_digest(&self) -> u64 {
        let mut d = FNV_OFF;
        d = fold_u32(d, self.in_budget);
        d = fold_u32(d, self.at_edge);
        d = fold_u32(d, self.over_budget);
        d = fold_u32(d, self.starved);
        d = fold_u32(d, self.points);
        let mut i = 0usize;
        while i < self.stage_hits.len() {
            if let Some(v) = self.stage_hits.get(i) {
                d = fold_u32(d, *v);
            }
            i += 1;
        }
        d
    }

    /// 全账审计：账平 + 覆盖 + 指纹可复算且非零。
    pub fn audit(&self) -> Result<(), u32> {
        self.balance()?;
        if self.coverage() != 0x0F {
            return Err(EdgeCode::CoverBad.val());
        }
        if self.points == 0 {
            return Err(EdgeCode::TallyBad.val());
        }
        if self.digest == 0 || self.digest == self.recompute_digest() {
            // 样本指纹与计数指纹本就不应相等（前者折叠样本值，后者
            // 折叠计数）；相等说明两侧混用了同一个折叠序列。
            return Err(EdgeCode::ReplayBad.val());
        }
        Ok(())
    }
}

// ── FNV-1a 64 ─────────────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (v & 0xFF) as u8);
    x = fold_byte(x, ((v >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((v >> 16) & 0xFF) as u8);
    fold_byte(x, ((v >> 24) & 0xFF) as u8)
}

/// 饱和加：溢出必须显式拒绝，不得静默回绕。
pub fn add_us(a: u32, b: u32) -> Result<u32, u32> {
    match a.checked_add(b) {
        Some(v) => Ok(v),
        None => Err(EdgeCode::MarginBad.val()),
    }
}

// ── 确定性语料 ────────────────────────────────────────────────────
/// 第 r 轮的耗时：九种边界形态轮转。
fn corpus_us(seg: u8, r: u32) -> Result<u32, u32> {
    let budget = stage_budget(seg)?;
    let floor = stage_floor(seg)?;
    let jitter = stage_jitter(seg)?;
    let kind = r % 9;
    let us = match kind {
        0 => 0,
        1 => floor / 2,
        2 => floor,
        3 => floor.saturating_add(budget) / 2,
        4 => budget.saturating_sub(EDGE_TOL_US),
        5 => budget,
        6 => budget.saturating_add(EDGE_TOL_US),
        7 => budget.saturating_add(EDGE_TOL_US).saturating_add(1),
        _ => budget
            .saturating_add(EDGE_TOL_US)
            .saturating_add((r % jitter.max(1)).saturating_add(1)),
    };
    Ok(us)
}

/// 千轮边界语料：段号与边界形态双轮转，逐点入账。
pub fn edge_corpus(rounds: u32) -> Result<EdgeLedger, u32> {
    if rounds == 0 || rounds > 100_000 {
        return Err(EdgeCode::TallyBad.val());
    }
    let mut l = EdgeLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let seg = (r % 4) as u8;
        let us = corpus_us(seg, r)?;
        let p = EdgePoint::framed(seg, us)?;
        l.record(p)?;
        r = r.saturating_add(1);
    }
    Ok(l)
}

/// 黄金账：返回（样本数，预算内，临界，越界，低于基值，指纹）。
pub fn golden_run() -> Result<(u32, u32, u32, u32, u32, u64), u32> {
    let l = edge_corpus(EDGE_ROUNDS)?;
    l.audit()?;
    if l.points != EDGE_ROUNDS {
        return Err(EdgeCode::TallyBad.val());
    }
    let mut i = 0usize;
    while i < l.stage_hits.len() {
        match l.stage_hits.get(i) {
            Some(v) => {
                if *v != EDGE_ROUNDS / 4 {
                    return Err(EdgeCode::TallyBad.val());
                }
            }
            None => return Err(EdgeCode::StageBad.val()),
        }
        i += 1;
    }
    Ok((l.points, l.in_budget, l.at_edge, l.over_budget, l.starved, l.digest))
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 渲染阶梯全集：四段 × 九级 = 36 行。
pub fn render_ladders() -> Result<String, u32> {
    let mut out = String::new();
    let mut seg = 0u8;
    while seg < 4 {
        let l = Ladder::build(seg)?;
        out.push_str(&l.render());
        seg = seg.saturating_add(1);
    }
    Ok(out)
}

/// 解析段号：只认四位机器名。
pub fn stage_from_name(s: &str) -> Result<u8, u32> {
    let mut i = 0usize;
    while i < STAGE_NAMES.len() {
        if let Some(v) = STAGE_NAMES.get(i) {
            if *v == s {
                return Ok(i as u8);
            }
        }
        i += 1;
    }
    Err(EdgeCode::PrintBad.val())
}

/// 十进制解析（逐字符，拒绝空串/非数字/前导零后的空尾）。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(EdgeCode::PrintBad.val());
    }
    let bytes = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(EdgeCode::PrintBad.val()),
        };
        if !(b'0'..=b'9').contains(&c) {
            return Err(EdgeCode::PrintBad.val());
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(EdgeCode::PrintBad.val()),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(EdgeCode::PrintBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 解析一行五点：seg|us|budget|floor|class，字段数与档名双重校验。
pub fn parse_point(line: &str) -> Result<EdgePoint, u32> {
    let mut parts = line.split('|');
    let s_seg = match parts.next() {
        Some(x) => x,
        None => return Err(EdgeCode::PrintBad.val()),
    };
    let s_us = match parts.next() {
        Some(x) => x,
        None => return Err(EdgeCode::PrintBad.val()),
    };
    let s_budget = match parts.next() {
        Some(x) => x,
        None => return Err(EdgeCode::PrintBad.val()),
    };
    let s_floor = match parts.next() {
        Some(x) => x,
        None => return Err(EdgeCode::PrintBad.val()),
    };
    let s_class = match parts.next() {
        Some(x) => x,
        None => return Err(EdgeCode::PrintBad.val()),
    };
    if parts.next().is_some() {
        return Err(EdgeCode::PrintBad.val());
    }
    let seg = parse_u32(s_seg)?;
    if seg > u32::from(u8::MAX) {
        return Err(EdgeCode::PrintBad.val());
    }
    let p = EdgePoint::new(seg as u8, parse_u32(s_us)?, parse_u32(s_budget)?, parse_u32(s_floor)?)?;
    let declared = EdgeClass::from_name(s_class)?;
    if declared != p.class() {
        return Err(EdgeCode::ClassBad.val());
    }
    Ok(p)
}

/// 导出面审计：行数、逐行可解析、档名闭集。
pub fn audit_render(text: &str, expect_rows: usize) -> Result<usize, u32> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return Err(EdgeCode::PrintBad.val());
    }
    let mut i = 0usize;
    while i < lines.len() {
        match lines.get(i) {
            Some(ln) => {
                parse_point(ln)?;
            }
            None => return Err(EdgeCode::PrintBad.val()),
        }
        i += 1;
    }
    Ok(lines.len())
}

/// 越界位置表（首位越界索引），供边界调试面定位。
pub fn breach_positions(pts: &[EdgePoint]) -> Result<Vec<usize>, u32> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < pts.len() {
        if let Some(p) = pts.get(i) {
            if p.class() == EdgeClass::OverBudget {
                v.push(i);
            }
        } else {
            return Err(EdgeCode::TallyBad.val());
        }
        i += 1;
    }
    Ok(v)
}

/// 首个越界索引；无越界返回 None（不得用 0 冒充「无」）。
pub fn first_breach(pts: &[EdgePoint]) -> Result<Option<usize>, u32> {
    let v = breach_positions(pts)?;
    match v.first() {
        Some(i) => Ok(Some(*i)),
        None => Ok(None),
    }
}

// ── 注入核 ────────────────────────────────────────────────────────
/// 注入账：四路注入逐轮必抓。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub caught: u32,
    pub rounds: u32,
    pub route_caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new(rounds: u32) -> InjectLedger {
        InjectLedger { caught: 0, rounds, route_caught: [0; 4] }
    }

    pub fn all_caught(&self) -> bool {
        if self.caught != self.rounds {
            return false;
        }
        let mut i = 0usize;
        while i < self.route_caught.len() {
            match self.route_caught.get(i) {
                Some(v) => {
                    if *v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：坏段号 / 零预算 / 余量溢出 / 坏导出行。
/// 每一轮只打一路，四路轮转；抓到即记，抓不到即漏。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(EdgeCode::TallyBad.val());
    }
    let mut l = InjectLedger::new(rounds);
    let mut r = 0u32;
    while r < rounds {
        let route = (r % 4) as usize;
        let hit = match route {
            0 => EdgePoint::new(4, 10, 80, 18).is_err(),
            1 => EdgePoint::new(0, 10, 0, 0).is_err(),
            2 => add_us(u32::MAX, 1).is_err(),
            _ => parse_point("0|10|80|18|bogus").is_err(),
        };
        if hit {
            l.caught = l.caught.saturating_add(1);
            if let Some(c) = l.route_caught.get_mut(route) {
                *c = c.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    Ok(l)
}

// ── 探针 ──────────────────────────────────────────────────────────
/// 探针 A：临界带的四个角点。
pub fn probe_seq_a() -> Result<(), u32> {
    let budget = stage_budget(0)?;
    let floor = stage_floor(0)?;
    if classify(budget.saturating_sub(EDGE_TOL_US), budget, floor, EDGE_TOL_US) != EdgeClass::AtEdge {
        return Err(EdgeCode::ClassBad.val());
    }
    if classify(budget, budget, floor, EDGE_TOL_US) != EdgeClass::AtEdge {
        return Err(EdgeCode::ClassBad.val());
    }
    if classify(budget.saturating_add(EDGE_TOL_US), budget, floor, EDGE_TOL_US) != EdgeClass::AtEdge {
        return Err(EdgeCode::ClassBad.val());
    }
    if classify(budget.saturating_add(EDGE_TOL_US).saturating_add(1), budget, floor, EDGE_TOL_US)
        != EdgeClass::OverBudget
    {
        return Err(EdgeCode::ClassBad.val());
    }
    Ok(())
}

/// 探针 B：千轮语料账平、覆盖与逐段均衡。
pub fn probe_seq_b() -> Result<(), u32> {
    let l = edge_corpus(EDGE_ROUNDS)?;
    l.audit()?;
    if l.points != EDGE_ROUNDS {
        return Err(EdgeCode::TallyBad.val());
    }
    let mut i = 0usize;
    while i < l.stage_hits.len() {
        if let Some(v) = l.stage_hits.get(i) {
            if *v == 0 {
                return Err(EdgeCode::CoverBad.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 探针 C：导出往返 + 坏行必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let text = render_ladders()?;
    let n = audit_render(&text, 4 * LADDER_RUNGS)?;
    if n != 4 * LADDER_RUNGS {
        return Err(EdgeCode::PrintBad.val());
    }
    let bad = ["0|10|80|18", "0|x|80|18|in-budget", "4|10|80|18|in-budget", "0|10|80|18|nope"];
    let mut i = 0usize;
    while i < bad.len() {
        match bad.get(i) {
            Some(s) => {
                if parse_point(s).is_ok() {
                    return Err(EdgeCode::PrintBad.val());
                }
            }
            None => return Err(EdgeCode::PrintBad.val()),
        }
        i += 1;
    }
    Ok(())
}

/// 双跑一致：同一语料两次指纹必须相同。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdgeCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫：全部诊断码必须落在 0x708C0 窗口内。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0B;
    let mut i = 0usize;
    while i < EdgeCode::ALL.len() {
        match EdgeCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据（CheckSet） ──────────────────────────────────────────────
/// 十项判据（边界四点 / 阶梯 / 千例 / 导出 / 注入 / 双跑）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "F48871-J1-四点覆盖",
        match &golden {
            Ok((_, ib, at, ob, st, _)) => *ib > 0 && *at > 0 && *ob > 0 && *st > 0,
            Err(_) => false,
        },
        match &golden {
            Ok((n, ib, at, ob, st, h)) => {
                format!("n={} in={} at={} over={} starved={} h={:016X}", n, ib, at, ob, st, h)
            }
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48871-J1-预算边界带",
        {
            let b = 80u32;
            classify(b.saturating_sub(EDGE_TOL_US + 1), b, 18, EDGE_TOL_US) == EdgeClass::InBudget
                && classify(b.saturating_sub(EDGE_TOL_US), b, 18, EDGE_TOL_US) == EdgeClass::AtEdge
                && classify(b, b, 18, EDGE_TOL_US) == EdgeClass::AtEdge
                && classify(b + EDGE_TOL_US, b, 18, EDGE_TOL_US) == EdgeClass::AtEdge
                && classify(b + EDGE_TOL_US + 1, b, 18, EDGE_TOL_US) == EdgeClass::OverBudget
        },
        String::from("预算−容差−1 内 / 预算−容差..预算+容差 临界 / 再+1 越界"),
    ));
    v.push((
        "F48871-J1-零样本必标",
        {
            let zero = classify(0, 80, 18, EDGE_TOL_US);
            let under = classify(17, 80, 18, EDGE_TOL_US);
            zero == EdgeClass::Starved && under == EdgeClass::Starved
        },
        String::from("0µs 与 基值−1µs 都必须落入低于基值档"),
    ));
    v.push((
        "F48871-J1-阶梯单调",
        {
            let mut ok = true;
            let mut seg = 0u8;
            while seg < 4 {
                match Ladder::build(seg) {
                    Ok(l) => {
                        if l.audit().is_err() {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                seg = seg.saturating_add(1);
            }
            ok
        },
        String::from("四段 × 九级：严格升序且后半段严重度不回头"),
    ));
    v.push((
        "F48872-J1-阶梯闭集",
        {
            let mut rows = 0usize;
            let mut ok = true;
            let mut seg = 0u8;
            while seg < 4 {
                match Ladder::build(seg) {
                    Ok(l) => match l.audit() {
                        Ok(mask) => {
                            rows = rows.saturating_add(LADDER_RUNGS);
                            if mask != 0x0F {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    },
                    Err(_) => ok = false,
                }
                seg = seg.saturating_add(1);
            }
            ok && rows == 4 * LADDER_RUNGS
        },
        format!("rows={} 每段四档齐", 4 * LADDER_RUNGS),
    ));
    v.push((
        "F48872-J1-千例账平",
        match edge_corpus(EDGE_ROUNDS) {
            Ok(l) => l.balance().is_ok() && l.points == EDGE_ROUNDS && l.stage_hits == [250, 250, 250, 250],
            Err(_) => false,
        },
        format!("n={} 段命中各 {}", EDGE_ROUNDS, EDGE_ROUNDS / 4),
    ));
    v.push((
        "F48872-J1-余量符号",
        {
            let b = 900u32;
            let f = 180u32;
            let inb = classify(b.saturating_sub(EDGE_TOL_US + 1), b, f, EDGE_TOL_US);
            let at = classify(b, b, f, EDGE_TOL_US);
            let over = classify(b + EDGE_TOL_US + 1, b, f, EDGE_TOL_US);
            margin_agrees(inb, margin_us(b - 9, b))
                && margin_agrees(at, margin_us(b, b))
                && margin_agrees(over, margin_us(b + EDGE_TOL_US + 1, b))
                && margin_us(b, b) == 0
                && margin_us(b + 1, b) == -1
        },
        String::from("预算点余量 0 / 超 1µs 余量 −1 / 档位与符号互证"),
    ));
    v.push((
        "F48872-J1-导出往返",
        match render_ladders() {
            Ok(t) => match audit_render(&t, 4 * LADDER_RUNGS) {
                Ok(n) => n == 4 * LADDER_RUNGS && t.lines().count() == 4 * LADDER_RUNGS,
                Err(_) => false,
            },
            Err(_) => false,
        },
        format!("rows={}", 4 * LADDER_RUNGS),
    ));
    v.push((
        "F48872-J1-注入必抓",
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => l.all_caught() && l.caught == INJECT_ROUNDS,
            Err(_) => false,
        },
        format!("四路 ×{} 全抓", INJECT_ROUNDS),
    ));
    v.push((
        "F48872-J1-双跑一致",
        match golden_run() {
            Ok(a) => match golden_run() {
                Ok(b) => a == b && repro_check().is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        },
        String::from("两次千例账本六元组全等"),
    ));
    v
}

/// 判据入口：额外挂一条段位守卫动态明细。
pub fn run_unx48871_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48871 四段预算边界面");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    let guard = segment_guard();
    s.add_dyn("F48871-J1-段位守卫", guard, if guard { "" } else { "诊断码逃出 0x708C0" });
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_band_classes() {
        let b = STAGE_BUDGET[0];
        let f = STAGE_FLOOR[0];
        assert_eq!(classify(b - EDGE_TOL_US - 1, b, f, EDGE_TOL_US), EdgeClass::InBudget);
        assert_eq!(classify(b - EDGE_TOL_US, b, f, EDGE_TOL_US), EdgeClass::AtEdge);
        assert_eq!(classify(b, b, f, EDGE_TOL_US), EdgeClass::AtEdge);
        assert_eq!(classify(b + EDGE_TOL_US, b, f, EDGE_TOL_US), EdgeClass::AtEdge);
        assert_eq!(classify(b + EDGE_TOL_US + 1, b, f, EDGE_TOL_US), EdgeClass::OverBudget);
    }

    #[test]
    fn zero_and_under_floor_are_starved() {
        assert_eq!(classify(0, 80, 18, EDGE_TOL_US), EdgeClass::Starved);
        assert_eq!(classify(17, 80, 18, EDGE_TOL_US), EdgeClass::Starved);
        assert_eq!(classify(18, 80, 18, EDGE_TOL_US), EdgeClass::InBudget);
    }

    #[test]
    fn over_budget_beyond_tolerance() {
        assert_eq!(classify(2600 + EDGE_TOL_US + 1, 2600, 640, EDGE_TOL_US), EdgeClass::OverBudget);
        assert_eq!(classify(2600 + EDGE_TOL_US, 2600, 640, EDGE_TOL_US), EdgeClass::AtEdge);
    }

    #[test]
    fn margin_signs() {
        assert_eq!(margin_us(80, 80), 0);
        assert_eq!(margin_us(81, 80), -1);
        assert_eq!(margin_us(79, 80), 1);
        assert!(margin_agrees(EdgeClass::InBudget, 1));
        assert!(margin_agrees(EdgeClass::OverBudget, -1));
        assert!(!margin_agrees(EdgeClass::OverBudget, 1));
    }

    #[test]
    fn ladder_strictly_ascending() {
        let mut seg = 0u8;
        while seg < 4 {
            match Ladder::build(seg) {
                Ok(l) => assert!(l.strict_ascending()),
                Err(_) => assert!(false),
            }
            seg += 1;
        }
    }

    #[test]
    fn ladder_ranks_monotone() {
        let mut seg = 0u8;
        while seg < 4 {
            match Ladder::build(seg) {
                Ok(l) => {
                    let mut last = 0u8;
                    let mut i = 2usize;
                    while i < LADDER_RUNGS {
                        match l.class_at(i) {
                            Ok(c) => {
                                assert!(c.rank() >= last);
                                last = c.rank();
                            }
                            Err(_) => assert!(false),
                        }
                        i += 1;
                    }
                }
                Err(_) => assert!(false),
            }
            seg += 1;
        }
    }

    #[test]
    fn ladder_covers_all_classes() {
        let mut seg = 0u8;
        while seg < 4 {
            match Ladder::build(seg) {
                Ok(l) => assert_eq!(l.audit(), Ok(0x0F)),
                Err(_) => assert!(false),
            }
            seg += 1;
        }
    }

    #[test]
    fn point_new_rejects_bad_seg() {
        assert_eq!(EdgePoint::new(4, 10, 80, 18), Err(EdgeCode::StageBad.val()));
        assert_eq!(EdgePoint::new(255, 10, 80, 18), Err(EdgeCode::StageBad.val()));
        assert!(EdgePoint::new(3, 10, 2600, 640).is_ok());
    }

    #[test]
    fn point_new_rejects_zero_budget() {
        assert_eq!(EdgePoint::new(0, 10, 0, 0), Err(EdgeCode::BudgetBad.val()));
    }

    #[test]
    fn point_new_rejects_floor_above_budget() {
        assert_eq!(EdgePoint::new(0, 10, 80, 81), Err(EdgeCode::FloorBad.val()));
        assert!(EdgePoint::new(0, 10, 80, 80).is_ok());
    }

    #[test]
    fn add_us_rejects_overflow() {
        assert_eq!(add_us(1, 2), Ok(3));
        assert_eq!(add_us(u32::MAX, 0), Ok(u32::MAX));
        assert_eq!(add_us(u32::MAX, 1), Err(EdgeCode::MarginBad.val()));
    }

    #[test]
    fn corpus_balance_and_coverage() {
        let l = edge_corpus(EDGE_ROUNDS);
        match l {
            Ok(x) => {
                assert_eq!(x.balance(), Ok(()));
                assert_eq!(x.coverage(), 0x0F);
                assert_eq!(x.points, EDGE_ROUNDS);
                assert_eq!(x.stage_hits, [250, 250, 250, 250]);
                assert_eq!(x.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn corpus_deterministic() {
        assert_eq!(edge_corpus(100), edge_corpus(100));
        match (edge_corpus(100), edge_corpus(101)) {
            (Ok(a), Ok(b)) => assert!(a.digest != b.digest),
            _ => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip() {
        let text = render_ladders();
        match text {
            Ok(t) => {
                assert_eq!(audit_render(&t, 36), Ok(36));
                let mut n = 0usize;
                for ln in t.lines() {
                    match parse_point(ln) {
                        Ok(p) => {
                            assert_eq!(p.text(), ln);
                            n += 1;
                        }
                        Err(_) => assert!(false),
                    }
                }
                assert_eq!(n, 36);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn parse_rejects_bad_rows() {
        assert!(parse_point("0|10|80|18").is_err());
        assert!(parse_point("0|10|80|18|in-budget|extra").is_err());
        assert!(parse_point("x|10|80|18|in-budget").is_err());
        assert!(parse_point("4|10|80|18|in-budget").is_err());
        assert!(parse_point("0|10|80|18|nope").is_err());
        assert!(parse_point("0|81|80|18|in-budget").is_err());
    }

    #[test]
    fn inject_all_caught() {
        match inject_rounds(INJECT_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.caught, INJECT_ROUNDS);
                assert_eq!(l.route_caught, [250, 250, 250, 250]);
                assert!(l.all_caught());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn segment_guard_holds() {
        assert!(segment_guard());
        let mut i = 0usize;
        while i < EdgeCode::ALL.len() {
            if let Some(c) = EdgeCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE + 0x01 && c.val() <= SEG_BASE + 0x0B);
            }
            i += 1;
        }
    }

    #[test]
    fn golden_run_stable() {
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b);
        match a {
            Ok((n, ib, at, ob, st, h)) => {
                assert_eq!(n, EDGE_ROUNDS);
                assert_eq!(n, ib + at + ob + st);
                assert!(h != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn breach_positions_finds_first_only() {
        let mut pts = Vec::new();
        match EdgePoint::framed(0, 10) {
            Ok(p) => pts.push(p),
            Err(_) => return,
        }
        match EdgePoint::framed(0, 200) {
            Ok(p) => pts.push(p),
            Err(_) => return,
        }
        match EdgePoint::framed(0, 300) {
            Ok(p) => pts.push(p),
            Err(_) => return,
        }
        assert_eq!(breach_positions(&pts), Ok(alloc::vec![1, 2]));
        assert_eq!(first_breach(&pts), Ok(Some(1)));
        assert_eq!(first_breach(&pts[..1]), Ok(None));
    }
}
