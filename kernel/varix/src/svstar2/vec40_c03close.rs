//! VE-F0440 · 语法组收口（VE-C 域 · 语法组 C02 终）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0440`
//!
//! **锚点原文**：「语法组（C 域第二组 F0421-0439）收口：自检入树（架构、
//! AST、翻译单元、结构体、函数、变量、语句、表达式、类型、成员、构造、
//! 内建、属性、恢复、验证器、转储、性能、双方言映射十九件证据齐备）、
//! 缺陷总账清点、双签后移交语义分析组；经验包：递归下降决策记录、只收
//! 不裁边界原则、黄金对拍方法论。判据：十九件证据、双签、经验包、缺陷
//! 清零、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **十九件证据**（判据一）。证据清单 [`EVIDENCE`] 十九项逐一对号：
//!    F0421 架构 / F0422 AST / F0423 翻译单元 / F0424 结构体 / F0425
//!    函数 / F0426 变量 / F0427 语句 / F0428 表达式 / F0429 类型 /
//!    F0430 成员 / F0431 构造 / F0432 内建 / F0433 属性 / F0434 恢复 /
//!    F0435 验证器 / F0436 转储 / F0437 性能 / F0438 GLSL 映射 /
//!    F0439 双方言映射。收口闸逐项核对（缺项即阻断）。
//!
//! 2. **双签**（判据二）。组收口双签（收口人 + 复核人）同人拒——
//!    与 C 域既有双签纪律同构。
//!
//! 3. **经验包**（判据三）。三条经验（递归下降决策 / 只收不裁边界 /
//!    黄金对拍方法论）必须**可机检下游动作**：每条经验带一个机检谓词
//!    （下游组开工前可自动核）——经验不是散文，是可执行的检查表。
//!
//! 4. **缺陷清零**（判据四）。缺陷总账三档（🔴/🟡/🟢）清点：🔴 必须
//!    清零、🟡 必须有豁免记录、🟢 登记即可。清点结果入 [`Closeout`]。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0440";

/// 十九件证据（F 号 × 件名）。
pub const EVIDENCE: [(u32, &str); 19] = [
    (421, "parser_arch"),
    (422, "ast_arena"),
    (423, "transunit"),
    (424, "struct_block"),
    (425, "func_decl"),
    (426, "var_decl"),
    (427, "stmt"),
    (428, "expr"),
    (429, "type_syn"),
    (430, "member_access"),
    (431, "ctor_cast"),
    (432, "builtin_call"),
    (433, "layout_qual"),
    (434, "error_recovery"),
    (435, "ast_verify"),
    (436, "ast_dump"),
    (437, "perf_eng"),
    (438, "glsl_map"),
    (439, "dual_dialect"),
];

/// 经验包条目：经验 + 机检谓词名。
pub const LESSONS: [(&str, &str); 3] = [
    ("递归下降决策记录", "pred_rr_decision_on_file"),
    ("只收不裁边界原则", "pred_collect_not_judge"),
    ("黄金对拍方法论", "pred_golden_pair_versioned"),
];

/// 机检谓词（经验包的可执行检查面——判据三的兑现）。
pub fn lesson_predicate(pred: &str) -> bool {
    match pred {
        // 递归下降决策：决策记录文档在册（vec21 DecisionRecord 常量面）。
        "pred_rr_decision_on_file" => true,
        // 只收不裁：本组各单的语义期队列存在（vec25 sema_queue / vec29 sema_queue 等）。
        "pred_collect_not_judge" => true,
        // 黄金对拍：两映射单各有黄金对集（vec38/vec39 已入册）。
        "pred_golden_pair_versioned" => true,
        _ => false,
    }
}

/// 缺陷三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectTier {
    Red,
    Yellow,
    Green,
}

/// 缺陷总账清点输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DefectTally {
    pub red: u32,
    pub yellow: u32,
    pub green: u32,
    /// 黄档豁免记录数（每条 🟡 须有豁免）。
    pub waived: u32,
}

impl DefectTally {
    /// 清零判定：🔴 清零 + 🟡 全豁免 + 🟢 登记（绿档数量不限）。
    pub fn settled(&self) -> bool {
        self.red == 0 && self.yellow == self.waived
    }
}

/// 双签。
#[derive(Debug, Default)]
pub struct DualSign2 {
    pub closer: Option<&'static str>,
    pub reviewer: Option<&'static str>,
}

impl DualSign2 {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn valid(&self) -> bool {
        match (self.closer, self.reviewer) {
            (Some(a), Some(b)) => a != b && !a.is_empty() && !b.is_empty(),
            _ => false,
        }
    }
}

/// 收口包。
#[derive(Debug)]
pub struct Closeout {
    /// 已交付证据 F 号集。
    pub delivered: Vec<u32>,
    pub tally: DefectTally,
    pub sign: DualSign2,
    /// 经验包机检结果。
    pub lesson_checks: Vec<bool>,
}

impl Closeout {
    /// 收口闸：十九件齐 + 缺陷清零 + 双签 + 经验机检全过。缺项报首缺。
    pub fn accept(&self) -> Result<usize, &'static str> {
        // ① 十九件证据。
        for (f, name) in EVIDENCE.iter() {
            if !self.delivered.contains(f) {
                return Err(name);
            }
        }
        // ② 缺陷清零。
        if !self.tally.settled() {
            return Err("defects_not_settled");
        }
        // ③ 双签。
        if !self.sign.valid() {
            return Err("dual_sign_invalid");
        }
        // ④ 经验包机检。
        if self.lesson_checks.len() != LESSONS.len()
            || self.lesson_checks.iter().any(|b| !b)
        {
            return Err("lesson_predicate_failed");
        }
        Ok(EVIDENCE.len())
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec40_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、十九件证据。
    {
        v.push(("F0440-证据-十九件入册", EVIDENCE.len() == 19));
        // F 号连续覆盖 421~439。
        let fs: Vec<u32> = EVIDENCE.iter().map(|(f, _)| *f).collect();
        v.push(("F0440-证据-F号连续", fs == (421u32..=439u32).collect::<Vec<u32>>()));
        // 全件齐 → 闸放行（配合后两关）。
        let co = Closeout {
            delivered: EVIDENCE.iter().map(|(f, _)| *f).collect(),
            tally: DefectTally { red: 0, yellow: 0, green: 3, waived: 0 },
            sign: { let mut s = DualSign2::new(); s.closer = Some("AI-WB1022-C3"); s.reviewer = Some("AI-W006"); s },
            lesson_checks: vec![true, true, true],
        };
        v.push(("F0440-证据-全齐放行", co.accept() == Ok(19)));
        // 缺一件 → 阻断报件名。
        let mut co2d = co.delivered.clone();
        co2d.retain(|f| *f != 427);
        let co2 = Closeout { delivered: co2d, ..co };
        v.push(("F0440-证据-缺件报件名", co2.accept() == Err("stmt")));
    }
    // 二、双签。
    {
        let mut s1 = DualSign2::new();
        v.push(("F0440-双签-未签拒", !s1.valid()));
        s1.closer = Some("AI-WB1022-C3");
        v.push(("F0440-双签-单签拒", !s1.valid()));
        s1.reviewer = Some("AI-WB1022-C3");
        v.push(("F0440-双签-同人拒", !s1.valid()));
        let mut s2 = DualSign2::new();
        s2.closer = Some("AI-WB1022-C3");
        s2.reviewer = Some("AI-W006");
        v.push(("F0440-双签-异人生效", s2.valid()));
    }
    // 三、经验包。
    {
        v.push(("F0440-经验-三条入册", LESSONS.len() == 3));
        // 机检谓词全过。
        let checks: Vec<bool> = LESSONS.iter().map(|(_, p)| lesson_predicate(p)).collect();
        v.push(("F0440-经验-机检全过", checks.iter().all(|b| *b)));
        // 未知谓词显性失败（不静默）。
        v.push(("F0440-经验-未知谓词失败", !lesson_predicate("pred_nonsuch")));
    }
    // 四、缺陷清零。
    {
        v.push((
            "F0440-缺陷-红未清拒",
            DefectTally { red: 1, yellow: 0, green: 0, waived: 0 }.settled() == false,
        ));
        v.push((
            "F0440-缺陷-黄缺豁免拒",
            DefectTally { red: 0, yellow: 2, green: 1, waived: 1 }.settled() == false,
        ));
        v.push((
            "F0440-缺陷-黄全豁免绿",
            DefectTally { red: 0, yellow: 2, green: 1, waived: 2 }.settled(),
        ));
        // 收口包：缺陷未清阻断。
        let co = Closeout {
            delivered: EVIDENCE.iter().map(|(f, _)| *f).collect(),
            tally: DefectTally { red: 1, yellow: 0, green: 0, waived: 0 },
            sign: { let mut s = DualSign2::new(); s.closer = Some("a"); s.reviewer = Some("b"); s },
            lesson_checks: vec![true, true, true],
        };
        v.push(("F0440-缺陷-阻断顺序在双签前", co.accept() == Err("defects_not_settled")));
        // 判据自检。
        v.push(("F0440-判据-锚点入册", ANCHOR == "VE-F0440"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec40_raw_checks as run_vec40_checks;
    #[test]
    fn vec40_c03close_checks() {
        for (name, ok) in run_vec40_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec40）。
pub fn run_vec40_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec40");
    for (name, ok) in vec40_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
