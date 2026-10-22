//! CGPU-F0476 · 移交包（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 移交包：冻结接口清单（C 域总账）；预算侧契约正式化（帧计时
//!   反馈/预算修正/合同校验三对接点的 D 域侧义务说明）；
//!   成本模型与配方体系交接（F0004 扩展版+F0353 配方框架的运维
//!   移交）；经验教训记录；证据三件套索引。判据：清单、契约、
//!   体系交接、教训、归档、判据。
//!
//! 设计纪律：
//! - 移交包五要件：接口清单（F0465 总账引用）+ D 域契约（三对接点
//!   义务说明齐备）+ 体系交接（成本模型+配方框架双在场）+ 教训
//!   记录（条目化，每条带类别码）+ 证据三件套索引（自检树/演练
//!   报告/基线册）。缺任一要件 → 移包拒收。
//! - D 域义务说明：每对接点（义务文本+履行判据号）配对——只讲
//!   权利不讲义务的契约不成立。
//! - 教训条目化：类别码枚举（设计/实现/判据/协同），正文非空。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10F0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10F0，全仓独占）。
pub const SEG_BASE: u16 = 0x10F0;

pub const E_HO_LIST: u16 = SEG_BASE + 1; // 接口清单缺失
pub const E_HO_OBLIGATION: u16 = SEG_BASE + 2; // 契约义务缺失
pub const E_HO_SYSTEM: u16 = SEG_BASE + 3; // 体系交接缺失
pub const E_HO_LESSON: u16 = SEG_BASE + 4; // 教训记录不合格
pub const E_HO_EVID: u16 = SEG_BASE + 5; // 证据三件套缺失

/// D 域三对接点（与 F0465 同表）。
pub const D_FRAME_TIMING: u8 = 1;
pub const D_BUDGET_FIX: u8 = 2;
pub const D_CONTRACT_CHECK: u8 = 3;

/// 契约义务条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obligation {
    pub link: u8,
    /// D 域侧义务文本（非空）。
    pub duty: &'static str,
    /// 履行判据号。
    pub evidence: &'static str,
}

impl Obligation {
    pub fn valid(&self) -> bool {
        [D_FRAME_TIMING, D_BUDGET_FIX, D_CONTRACT_CHECK].contains(&self.link)
            && !self.duty.is_empty()
            && !self.evidence.is_empty()
    }
}

/// 体系交接（成本模型+配方框架）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemHandoff {
    /// F0004 扩展版成本模型在场。
    pub cost_model: bool,
    /// F0353 配方框架运维移交在场。
    pub recipe_framework: bool,
}

impl SystemHandoff {
    pub fn complete(&self) -> bool {
        self.cost_model && self.recipe_framework
    }
}

/// 教训类别码。
pub const L_DESIGN: u8 = 1;
pub const L_IMPL: u8 = 2;
pub const L_CRITERIA: u8 = 3;
pub const L_COLLAB: u8 = 4;

/// 教训条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub category: u8,
    pub text: &'static str,
}

impl Lesson {
    pub fn valid(&self) -> bool {
        [L_DESIGN, L_IMPL, L_CRITERIA, L_COLLAB].contains(&self.category) && !self.text.is_empty()
    }
}

/// 证据三件套索引。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceIndex {
    /// 自检树判据数。
    pub tree_items: u32,
    /// 演练报告场景数。
    pub drill_scenes: u32,
    /// 基线册指标数。
    pub baseline_metrics: u32,
}

impl EvidenceIndex {
    pub fn complete(&self) -> bool {
        self.tree_items >= 16 && self.drill_scenes == 5 && self.baseline_metrics == 6
    }
}

/// 移交包。
#[derive(Debug)]
pub struct HandoffPackage {
    /// 接口清单（C 域总账签名数）。
    pub api_entries: u32,
    /// D 域契约义务。
    pub obligations: Vec<Obligation>,
    pub system: SystemHandoff,
    pub lessons: Vec<Lesson>,
    pub evidence: EvidenceIndex,
}

impl HandoffPackage {
    /// 五要件验收。
    pub fn accept(&self) -> Result<(), u16> {
        if self.api_entries < 9 {
            return Err(E_HO_LIST);
        }
        // 三对接点义务齐备。
        for l in [D_FRAME_TIMING, D_BUDGET_FIX, D_CONTRACT_CHECK].iter() {
            match self.obligations.iter().find(|o| o.link == *l) {
                Some(o) if o.valid() => {}
                _ => return Err(E_HO_OBLIGATION),
            }
        }
        if !self.system.complete() {
            return Err(E_HO_SYSTEM);
        }
        if self.lessons.is_empty() || self.lessons.iter().any(|l| !l.valid()) {
            return Err(E_HO_LESSON);
        }
        if !self.evidence.complete() {
            return Err(E_HO_EVID);
        }
        Ok(())
    }
}

/// 默认移交包（本域收官形态）。
pub fn package_v1() -> HandoffPackage {
    HandoffPackage {
        api_entries: 9,
        obligations: vec![
            Obligation { link: D_FRAME_TIMING, duty: "D 域每帧上报帧计时反馈（tick 级）", evidence: "F0332" },
            Obligation { link: D_BUDGET_FIX, duty: "D 域受理预算修正并在下帧生效", evidence: "F0402" },
            Obligation { link: D_CONTRACT_CHECK, duty: "D 域双向合同校验（口径与 F0471 一致）", evidence: "F0471" },
        ],
        system: SystemHandoff { cost_model: true, recipe_framework: true },
        lessons: vec![
            Lesson { category: L_DESIGN, text: "配额语义必须区分单次上限与周期累计水位" },
            Lesson { category: L_IMPL, text: "no_std 三件套缺一编译即炸，模块进链前自查" },
            Lesson { category: L_CRITERIA, text: "判据写死数字会漂移，改用性质断言" },
            Lesson { category: L_COLLAB, text: "板卡写回后必须 grep 特征串复核竞态" },
        ],
        evidence: EvidenceIndex { tree_items: 16, drill_scenes: 5, baseline_metrics: 6 },
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、默认包五要件通过。
    {
        let p = package_v1();
        v.push(("F0476-移交包-五要件齐备", p.accept().is_ok()));
        v.push(("F0476-移交包-接口清单九签名", p.api_entries == 9));
    }
    // 二、契约义务：缺义务/缺判据拒。
    {
        let mut p = package_v1();
        p.obligations[0].duty = "";
        v.push(("F0476-契约-义务缺失拒", p.accept() == Err(E_HO_OBLIGATION)));
        let mut p2 = package_v1();
        p2.obligations[1].evidence = "";
        v.push(("F0476-契约-履行判据缺失拒", p2.accept() == Err(E_HO_OBLIGATION)));
        let mut p3 = package_v1();
        p3.obligations.remove(2);
        v.push(("F0476-契约-对接点缺失拒", p3.accept() == Err(E_HO_OBLIGATION)));
    }
    // 三、体系交接：双在场。
    {
        let mut p = package_v1();
        p.system.cost_model = false;
        v.push(("F0476-体系-成本模型缺失拒", p.accept() == Err(E_HO_SYSTEM)));
        let mut p2 = package_v1();
        p2.system.recipe_framework = false;
        v.push(("F0476-体系-配方框架缺失拒", p2.accept() == Err(E_HO_SYSTEM)));
    }
    // 四、教训记录。
    {
        let mut p = package_v1();
        p.lessons.clear();
        v.push(("F0476-教训-空记录拒", p.accept() == Err(E_HO_LESSON)));
        let mut p2 = package_v1();
        p2.lessons[0].category = 9;
        v.push(("F0476-教训-类别码非法拒", p2.accept() == Err(E_HO_LESSON)));
        let mut p3 = package_v1();
        p3.lessons[1].text = "";
        v.push(("F0476-教训-空正文拒", p3.accept() == Err(E_HO_LESSON)));
    }
    // 五、证据三件套。
    {
        let mut p = package_v1();
        p.evidence.tree_items = 15;
        v.push(("F0476-证据-自检树不足拒", p.accept() == Err(E_HO_EVID)));
        let mut p2 = package_v1();
        p2.evidence.drill_scenes = 4;
        v.push(("F0476-证据-演练缺场景拒", p2.accept() == Err(E_HO_EVID)));
        let mut p3 = package_v1();
        p3.evidence.baseline_metrics = 5;
        v.push(("F0476-证据-基线缺指标拒", p3.accept() == Err(E_HO_EVID)));
    }
    // 六、判据自检。
    {
        let p = package_v1();
        v.push((
            "F0476-判据-三对接点枚举",
            p.obligations.iter().map(|o| o.link).collect::<Vec<u8>>() == vec![D_FRAME_TIMING, D_BUDGET_FIX, D_CONTRACT_CHECK],
        ));
        v.push(("F0476-判据-教训四类齐备", p.lessons.len() == 4 && p.lessons.iter().all(|l| l.valid())));
        v.push(("F0476-判据-证据三件套", p.evidence.complete()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0476_handoff_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
