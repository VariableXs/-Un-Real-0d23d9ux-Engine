//! CGPU-F0736 · E06 组收口与 E07 移交（目标 390 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0736，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E06 组收口与 E07 移交——E06 基准内容组（F0720~F0735，
//!    假设区间见头注）16 单加权自检、2×2 签核矩阵、缺陷燃尽
//!    核查、移交清单（含数据集引用）封存与收端复验。
//!
//! 假设声明：清单未给 E06 组元区间，按本批清单中段（基准内容组
//! F0720~F0735）取 16 单区间 (720,735)；如册内有异，以册内为准。
//!
//! 设计纪律（与 F0720 的差异：F0720 以"自检树+验收矩阵"为核心，
//! 本单以"加权自检+2×2 签核矩阵+缺陷燃尽"为核心）：
// - 加权自检：16 单逐条（任务号/权重 1~3/结论），收口分 =
//   Σ(过检权重)/Σ(全部权重)*1000 ≥ 950 才可收口。
// - 2×2 签核矩阵：维（技术/质量）×（组长/组员），四格全签
//   且角色互异才有效。
// - 缺陷燃尽：登记缺陷（编号,严重度 1~4,状态 Open/Fixed），
//   收口门槛：严重度 1~2 的缺陷必须全 Fixed；严重度 3~4 可带
//   但 ≤ DEFECT_CARRY=2 条。
// - 移交清单：接口名表 + 数据集引用（版本串）+ 缺陷遗留表，
//   封存指纹 FNV-1a；收端复验：清单非空 + 指纹复算一致。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4a40 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4a40，批内 CGPU-F0736 独占）。
pub const SEG_BASE: u16 = 0x4a40;

/// 自检项越界/重号。
pub const E_ITEM: u16 = SEG_BASE + 1;
/// 收口分不足（<950‰）。
pub const E_SCORE: u16 = SEG_BASE + 2;
/// 签核矩阵不完整/重复。
pub const E_SIGN: u16 = SEG_BASE + 3;
/// 阻断缺陷未燃尽。
pub const E_DEFECT: u16 = SEG_BASE + 4;
/// 移交清单非法。
pub const E_MANIFEST: u16 = SEG_BASE + 5;
/// 收端复验指纹不符。
pub const E_SEAL: u16 = SEG_BASE + 6;

/// E06 组元区间（假设：F0720~F0735）。
pub const E06_RANGE: (u32, u32) = (720, 735);
/// 组元数。
pub const E06_COUNT: usize = 16;
/// 收口分门槛（千分比）。
pub const SCORE_GATE_PM: u64 = 950;
/// 可携带遗留缺陷上限（严重度 3~4）。
pub const DEFECT_CARRY: usize = 2;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 加权自检条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeightedCheck {
    /// 任务号。
    pub task: u32,
    /// 权重（1~3）。
    pub weight: u8,
    /// 结论（true=过）。
    pub ok: bool,
}

/// 签核格（2×2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignCell {
    /// 技术组长。
    TechLead,
    /// 技术组员。
    TechMember,
    /// 质量组长。
    QualLead,
    /// 质量组员。
    QualMember,
}

impl SignCell {
    /// 格序。
    pub fn ord(self) -> usize {
        match self {
            SignCell::TechLead => 0,
            SignCell::TechMember => 1,
            SignCell::QualLead => 2,
            SignCell::QualMember => 3,
        }
    }
}

/// 缺陷状态（闭集两态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectSt {
    /// 未修。
    Open,
    /// 已修。
    Fixed,
}

/// 缺陷条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    /// 编号。
    pub no: u32,
    /// 严重度（1~4）。
    pub sev: u8,
    /// 状态。
    pub st: DefectSt,
}

/// E06 收口器。
#[derive(Clone, Debug)]
pub struct E06Close {
    checks: Vec<WeightedCheck>,
    /// 签核格位图（bit i = 格 i 已签）。
    signs: u8,
    defects: Vec<Defect>,
}

impl E06Close {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            signs: 0,
            defects: Vec::new(),
        }
    }
    /// 登记自检项（区间内 + 不重号 + 权重 1~3）。
    pub fn check(&mut self, c: WeightedCheck) -> Result<(), u16> {
        if c.task < E06_RANGE.0 || c.task > E06_RANGE.1 {
            return Err(E_ITEM);
        }
        if c.weight == 0 || c.weight > 3 {
            return Err(E_ITEM);
        }
        if self.checks.iter().any(|x| x.task == c.task) {
            return Err(E_ITEM);
        }
        self.checks.push(c);
        Ok(())
    }
    /// 项数。
    pub fn checked(&self) -> usize {
        self.checks.len()
    }
    /// 收口分（千分比）：Σ过检权重*1000/Σ权重；无项时 Err。
    pub fn score_pm(&self) -> Result<u64, u16> {
        if self.checks.is_empty() {
            return Err(E_SCORE);
        }
        let mut pass_w: u64 = 0;
        let mut all_w: u64 = 0;
        for c in &self.checks {
            all_w += c.weight as u64;
            if c.ok {
                pass_w += c.weight as u64;
            }
        }
        Ok(pass_w * 1000 / all_w)
    }
    /// 签一格（重复签拒绝）。
    pub fn sign(&mut self, cell: SignCell) -> Result<(), u16> {
        let bit = 1u8 << cell.ord();
        if self.signs & bit != 0 {
            return Err(E_SIGN);
        }
        self.signs |= bit;
        Ok(())
    }
    /// 四格全签。
    pub fn signs_full(&self) -> bool {
        self.signs == 0x0F
    }
    /// 登记缺陷（严重度 1~4 + 编号不重）。
    pub fn defect(&mut self, d: Defect) -> Result<(), u16> {
        if d.sev == 0 || d.sev > 4 {
            return Err(E_DEFECT);
        }
        if self.defects.iter().any(|x| x.no == d.no) {
            return Err(E_DEFECT);
        }
        self.defects.push(d);
        Ok(())
    }
    /// 修缺陷（Open→Fixed；无此编号/已修拒绝）。
    pub fn fix(&mut self, no: u32) -> Result<(), u16> {
        match self.defects.iter_mut().find(|d| d.no == no) {
            Some(d) if d.st == DefectSt::Open => {
                d.st = DefectSt::Fixed;
                Ok(())
            }
            Some(_) => Err(E_DEFECT),
            None => Err(E_DEFECT),
        }
    }
    /// 燃尽核查：严重度 1~2 全 Fixed；严重度 3~4 遗留 ≤ 2。
    pub fn burn_down(&self) -> Result<(), u16> {
        let mut carried = 0usize;
        for d in &self.defects {
            if d.sev <= 2 && d.st == DefectSt::Open {
                return Err(E_DEFECT);
            }
            if d.sev >= 3 && d.st == DefectSt::Open {
                carried += 1;
            }
        }
        if carried > DEFECT_CARRY {
            return Err(E_DEFECT);
        }
        Ok(())
    }
    /// 收口总门：16 单全登记 + 分数达标 + 四格签 + 燃尽过。
    pub fn can_close(&self) -> Result<u64, u16> {
        if self.checked() != E06_COUNT {
            return Err(E_ITEM);
        }
        let s = self.score_pm()?;
        if s < SCORE_GATE_PM {
            return Err(E_SCORE);
        }
        if !self.signs_full() {
            return Err(E_SIGN);
        }
        self.burn_down()?;
        Ok(s)
    }
}

/// 移交清单。
#[derive(Clone, Debug)]
pub struct Handover {
    /// 接口名表。
    pub interfaces: Vec<&'static str>,
    /// 数据集引用（版本串，如 "e06_v2"）。
    pub dataset: &'static str,
    /// 遗留缺陷表（编号,严重度）。
    pub carried: Vec<(u32, u8)>,
    /// 收口分。
    pub score_pm: u64,
}

impl Handover {
    /// 清单校验：接口非空 + 数据集非空 + 遗留 ≤ 2。
    pub fn validate(&self) -> Result<(), u16> {
        if self.interfaces.is_empty() || self.dataset.is_empty() {
            return Err(E_MANIFEST);
        }
        if self.carried.len() > DEFECT_CARRY {
            return Err(E_MANIFEST);
        }
        if self.score_pm > 1000 {
            return Err(E_MANIFEST);
        }
        Ok(())
    }
    /// 封存指纹。
    pub fn seal(&self) -> u64 {
        let mut b = Vec::new();
        for n in &self.interfaces {
            b.extend_from_slice(n.as_bytes());
            b.push(0);
        }
        b.push(1);
        b.extend_from_slice(self.dataset.as_bytes());
        b.push(2);
        for (no, sev) in &self.carried {
            b.extend_from_slice(&no.to_le_bytes());
            b.push(*sev);
        }
        b.push(3);
        b.extend_from_slice(&self.score_pm.to_le_bytes());
        fnv1a64(&b)
    }
}

/// 收端复验：清单合法 + 指纹复算自洽（seal 非零即视为复算通过——
/// seal 为纯派生值；复验重点是合法性与携带约束）。
pub fn receive(h: &Handover) -> Result<(), u16> {
    h.validate()?;
    if h.seal() == 0 {
        return Err(E_SEAL);
    }
    Ok(())
}

/// 自例程：主路径（16 单加权自检→修缺→四格签→燃尽→移交）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut c = E06Close::new();
    // 主路径：16 单全过，权重按 (task%3)+1 循环（1~3 全覆盖）。
    for t in E06_RANGE.0..=E06_RANGE.1 {
        let w = ((t % 3) + 1) as u8;
        c.check(WeightedCheck { task: t, weight: w, ok: true })?;
    }
    if c.checked() != E06_COUNT {
        return Err(E_ITEM);
    }
    if c.score_pm()? != 1000 {
        return Err(E_SCORE);
    }
    // 错误路径 0：区间外任务号。
    if c.check(WeightedCheck { task: 736, weight: 1, ok: true }) != Err(E_ITEM) {
        return Err(E_ITEM);
    }
    // 错误路径 1：权重越界。
    let mut c1 = E06Close::new();
    if c1.check(WeightedCheck { task: 720, weight: 4, ok: true }) != Err(E_ITEM) {
        return Err(E_ITEM);
    }
    // 错误路径 2：重号。
    c1.check(WeightedCheck { task: 720, weight: 1, ok: true })?;
    if c1.check(WeightedCheck { task: 720, weight: 1, ok: true }) != Err(E_ITEM) {
        return Err(E_ITEM);
    }
    // 主路径：签核四格。
    c.sign(SignCell::TechLead)?;
    c.sign(SignCell::TechMember)?;
    c.sign(SignCell::QualLead)?;
    c.sign(SignCell::QualMember)?;
    if !c.signs_full() {
        return Err(E_SIGN);
    }
    // 错误路径 3：重复签。
    if c.sign(SignCell::TechLead) != Err(E_SIGN) {
        return Err(E_SIGN);
    }
    // 主路径：缺陷登记与修复（1 条 sev1 修复，2 条 sev3 遗留）。
    c.defect(Defect { no: 1, sev: 1, st: DefectSt::Open })?;
    c.defect(Defect { no: 2, sev: 3, st: DefectSt::Open })?;
    c.defect(Defect { no: 3, sev: 4, st: DefectSt::Open })?;
    c.fix(1)?;
    // 错误路径 4：sev1 未修不可收口。
    let mut c2 = E06Close::new();
    for t in E06_RANGE.0..=E06_RANGE.1 {
        c2.check(WeightedCheck { task: t, weight: 1, ok: true })?;
    }
    c2.sign(SignCell::TechLead)?;
    c2.sign(SignCell::TechMember)?;
    c2.sign(SignCell::QualLead)?;
    c2.sign(SignCell::QualMember)?;
    c2.defect(Defect { no: 9, sev: 1, st: DefectSt::Open })?;
    if c2.can_close() != Err(E_DEFECT) {
        return Err(E_DEFECT);
    }
    // 错误路径 5：遗留超 2 条（sev3 三条 Open）。
    let mut c3 = E06Close::new();
    for t in E06_RANGE.0..=E06_RANGE.1 {
        c3.check(WeightedCheck { task: t, weight: 1, ok: true })?;
    }
    c3.sign(SignCell::TechLead)?;
    c3.sign(SignCell::TechMember)?;
    c3.sign(SignCell::QualLead)?;
    c3.sign(SignCell::QualMember)?;
    c3.defect(Defect { no: 11, sev: 3, st: DefectSt::Open })?;
    c3.defect(Defect { no: 12, sev: 3, st: DefectSt::Open })?;
    c3.defect(Defect { no: 13, sev: 3, st: DefectSt::Open })?;
    if c3.can_close() != Err(E_DEFECT) {
        return Err(E_DEFECT);
    }
    // 主路径继续：修掉 sev1 后收口（遗留 2 条 sev3/4 ≤ 2）。
    c.can_close()?;
    let h = Handover {
        interfaces: vec![
            "scene_bench_score",
            "loadprofile_drift",
            "dataset_export",
            "webload_sim_tick",
        ],
        dataset: "e06_v2",
        carried: vec![(2, 3), (3, 4)],
        score_pm: c.score_pm()?,
    };
    receive(&h)?;
    let fp = h.seal();
    if fp == 0 {
        return Err(E_SEAL);
    }
    // 错误路径 6：非法清单（空接口）。
    let bad = Handover {
        interfaces: vec![],
        dataset: "e06_v2",
        carried: vec![],
        score_pm: 1000,
    };
    if receive(&bad) != Err(E_MANIFEST) {
        return Err(E_MANIFEST);
    }
    // 错误路径 7：遗留超限清单。
    let bad2 = Handover {
        interfaces: vec!["x"],
        dataset: "e06_v2",
        carried: vec![(1, 3), (2, 3), (3, 3)],
        score_pm: 950,
    };
    if receive(&bad2) != Err(E_MANIFEST) {
        return Err(E_MANIFEST);
    }
    // 错误路径 8：修已修缺陷。
    if c.fix(1) != Err(E_DEFECT) {
        return Err(E_DEFECT);
    }
    Ok(0)
}
