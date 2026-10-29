//! UNX-F10221 归因口径成文落账（三分类定义与判别规则齐备，类缺检出）（0x10ec0 段）
//!
//! 锚点：UNX-F10221-J1 归因口径成文落账（三分类定义与判别规则
//! 齐备），类缺 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：口径册（内核缺陷/环境受限/期望差异三分类）+ 判别规则表
//! （逐条触发条件 + 处置动作）+ 齐备核（三类定义与规则全在）+
//! 类缺检出 + 折叠指纹 + 注入核（类缺/规缺/空定义/重类/封册改写）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10ec0;
/// 诊断码：类缺
pub const ERR_CLASS_MISSING: u32 = SEG_BASE + 0x01;
/// 诊断码：规则缺
pub const ERR_RULE_MISSING: u32 = SEG_BASE + 0x02;
/// 诊断码：定义空
pub const ERR_EMPTY_DEF: u32 = SEG_BASE + 0x03;
/// 诊断码：重类
pub const ERR_DUP_CLASS: u32 = SEG_BASE + 0x04;
/// 诊断码：封册改写
pub const ERR_FROZEN: u32 = SEG_BASE + 0x05;

/// 归因三分类
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CauseClass {
    /// 内核缺陷
    KernelBug,
    /// 环境受限
    EnvLimited,
    /// 期望差异
    ExpectGap,
}

impl CauseClass {
    /// 类名
    pub fn name(&self) -> &'static str {
        match self {
            CauseClass::KernelBug => "KERNEL_BUG",
            CauseClass::EnvLimited => "ENV_LIMITED",
            CauseClass::ExpectGap => "EXPECT_GAP",
        }
    }
    /// 类序
    pub fn ord(&self) -> u8 {
        match self {
            CauseClass::KernelBug => 1,
            CauseClass::EnvLimited => 2,
            CauseClass::ExpectGap => 3,
        }
    }
}

/// 判别规则条目
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CauseRule {
    /// 适用类
    pub class: CauseClass,
    /// 触发条件关键词（非空）
    pub trigger: String,
    /// 处置动作（非空）
    pub action: String,
}

impl CauseRule {
    /// 条目自洽
    pub fn ok(&self) -> bool {
        !self.trigger.is_empty() && !self.action.is_empty()
    }
}

/// 口径册
pub struct CauseCodex {
    /// 定义表：类名→定义文本
    pub defs: Vec<(CauseClass, String)>,
    /// 规则表
    pub rules: Vec<CauseRule>,
    /// 封册标记
    pub frozen: bool,
}

impl CauseCodex {
    /// 空册
    pub fn new() -> Self {
        CauseCodex { defs: Vec::new(), rules: Vec::new(), frozen: false }
    }

    /// 立类定义
    pub fn define(&mut self, class: CauseClass, definition: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if definition.is_empty() {
            return Err(ERR_EMPTY_DEF);
        }
        if self.defs.iter().any(|(c, _)| *c == class) {
            return Err(ERR_DUP_CLASS);
        }
        self.defs.push((class, definition.to_string()));
        Ok(())
    }

    /// 立判别规则
    pub fn add_rule(&mut self, class: CauseClass, trigger: &str, action: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if trigger.is_empty() || action.is_empty() {
            return Err(ERR_RULE_MISSING);
        }
        self.rules.push(CauseRule {
            class,
            trigger: trigger.to_string(),
            action: action.to_string(),
        });
        Ok(())
    }

    /// 齐备核：三类定义全在
    pub fn classes_complete(&self) -> bool {
        for c in [CauseClass::KernelBug, CauseClass::EnvLimited, CauseClass::ExpectGap] {
            if !self.defs.iter().any(|(d, _)| *d == c) {
                return false;
            }
        }
        true
    }

    /// 类缺清单
    pub fn missing_classes(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for c in [CauseClass::KernelBug, CauseClass::EnvLimited, CauseClass::ExpectGap] {
            if !self.defs.iter().any(|(d, _)| *d == c) {
                out.push(c.name());
            }
        }
        out
    }

    /// 规则覆盖核：每个已立类至少一条规则
    pub fn rules_cover_defs(&self) -> bool {
        for (c, _) in &self.defs {
            if !self.rules.iter().any(|r| r.class == *c) {
                return false;
            }
        }
        true
    }

    /// 按触发词匹配规则
    pub fn match_rule(&self, symptom: &str) -> Option<&CauseRule> {
        self.rules.iter().find(|r| symptom.contains(&r.trigger))
    }

    /// 诊断面：定义回读
    pub fn definition_of(&self, class: CauseClass) -> Option<&String> {
        self.defs.iter().find(|(c, _)| *c == class).map(|(_, d)| d)
    }

    /// 诊断面：类规则计数
    pub fn rule_count_of(&self, class: CauseClass) -> usize {
        self.rules.iter().filter(|r| r.class == class).count()
    }

    /// 诊断面：处置动作清单
    pub fn action_list(&self) -> Vec<String> {
        self.rules.iter().map(|r| r.action.clone()).collect()
    }

    /// 诊断面：定义平均长度
    pub fn avg_def_len(&self) -> usize {
        if self.defs.is_empty() {
            return 0;
        }
        let sum: usize = self.defs.iter().map(|(_, d)| d.len()).sum();
        sum / self.defs.len()
    }

    /// 诊断面：口径册摘要
    pub fn codex_summary(&self) -> String {
        format!(
            "defs={} rules={} complete={}",
            self.defs.len(),
            self.rules.len(),
            self.classes_complete()
        )
    }

    /// 诊断面：全规则自洽复核
    pub fn all_rules_ok(&self) -> bool {
        self.rules.iter().all(|r| r.ok())
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, (c, d)) in self.defs.iter().enumerate() {
            acc = acc.wrapping_mul(6967).wrapping_add((c.ord() as u32) << (i % 3));
            acc = acc.wrapping_add(d.len() as u32);
        }
        for (i, r) in self.rules.iter().enumerate() {
            acc = acc.wrapping_mul(3571).wrapping_add((r.class.ord() as u32) + i as u32);
            acc = acc.wrapping_add(r.action.len() as u32);
        }
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 类缺检出
    let mut c1 = CauseCodex::new();
    let _ = c1.define(CauseClass::KernelBug, "def");
    out[0] = !c1.classes_complete() && c1.missing_classes().contains(&"ENV_LIMITED");
    // 2: 空定义拒绝
    let mut c2 = CauseCodex::new();
    out[1] = c2.define(CauseClass::KernelBug, "") == Err(ERR_EMPTY_DEF);
    // 3: 重类拒绝
    let mut c3 = CauseCodex::new();
    let _ = c3.define(CauseClass::KernelBug, "d1");
    out[2] = c3.define(CauseClass::KernelBug, "d2") == Err(ERR_DUP_CLASS);
    // 4: 规则缺触发词
    let mut c4 = CauseCodex::new();
    out[3] = c4.add_rule(CauseClass::KernelBug, "", "act") == Err(ERR_RULE_MISSING)
        && c4.add_rule(CauseClass::EnvLimited, "trg", "") == Err(ERR_RULE_MISSING);
    // 5: 封册后改写拒绝
    let mut c5 = CauseCodex::new();
    let _ = c5.define(CauseClass::KernelBug, "d");
    let _ = c5.seal();
    out[4] = c5.define(CauseClass::EnvLimited, "d2") == Err(ERR_FROZEN);
    out
}

/// 判据函数
pub fn run_unx10221_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10221");
    let mut c = CauseCodex::new();
    let d1 = c.define(CauseClass::KernelBug, "复跑三次同指纹失败，且注入类用例受控");
    let d2 = c.define(CauseClass::EnvLimited, "依赖外设/实机/网络未接入，replica 不可复现");
    let d3 = c.define(CauseClass::ExpectGap, "行为与规格文本冲突，需仲裁条款引用");
    s.add("三类定义齐", d1.is_ok() && d2.is_ok() && d3.is_ok() && c.classes_complete(), "三分类定义全部立册");
    let r1 = c.add_rule(CauseClass::KernelBug, "同指纹三跑", "开 BUG-UNX 工单");
    let r2 = c.add_rule(CauseClass::EnvLimited, "外设未接入", "挂补测标记转环境账");
    let r3 = c.add_rule(CauseClass::ExpectGap, "规格冲突", "仲裁留痕后改期望");
    s.add("三规则齐", r1.is_ok() && r2.is_ok() && r3.is_ok(), "三类判别规则全部立册");
    s.add("规则覆盖定义", c.rules_cover_defs(), "每类至少一条规则");
    s.add("触发词匹配", c.match_rule("同指纹三跑复现失败样例").map(|r| r.class) == Some(CauseClass::KernelBug), "症状词命中内核缺陷规则");
    s.add("环境词命中", c.match_rule("外设未接入导致").map(|r| r.class) == Some(CauseClass::EnvLimited), "环境词命中受限规则");
    s.add("类名与序", CauseClass::KernelBug.name() == "KERNEL_BUG" && CauseClass::ExpectGap.ord() == 3, "类名类序正确");
    s.add("条目自洽", c.rules.iter().all(|r| r.ok()), "各规则条目自洽");
    s.add("空症状不误配", c.match_rule("毫无关键词").is_none(), "无触发词不误配");
    s.add("定义回读", c.definition_of(CauseClass::KernelBug).is_some() && c.definition_of(CauseClass::KernelBug).unwrap().contains("三次"), "内核缺陷定义可回读");
    s.add("类规则计数", c.rule_count_of(CauseClass::KernelBug) == 1 && c.rule_count_of(CauseClass::ExpectGap) == 1, "各类规则一条");
    s.add("动作清单", c.action_list().len() == 3, "三条处置动作");
    s.add("定义均长", c.avg_def_len() > 0, "定义均长非零");
    s.add("口径摘要", c.codex_summary().contains("defs=3") && c.codex_summary().contains("rules=3"), "摘要含三类三规则");
    s.add("全规则自洽", c.all_rules_ok(), "全规则字段自洽");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = c.seal();
    s.add("封册后拒绝", c.define(CauseClass::KernelBug, "late") == Err(ERR_FROZEN), "封册后立类被拒");
    s.add("封册指纹非零", fp != 0, "封册折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_complete_flow() {
        let mut c = CauseCodex::new();
        let _ = c.define(CauseClass::KernelBug, "d1");
        let _ = c.define(CauseClass::EnvLimited, "d2");
        let _ = c.define(CauseClass::ExpectGap, "d3");
        assert!(c.classes_complete());
        assert!(c.missing_classes().is_empty());
        let _ = c.add_rule(CauseClass::KernelBug, "t1", "a1");
        let _ = c.add_rule(CauseClass::EnvLimited, "t2", "a2");
        let _ = c.add_rule(CauseClass::ExpectGap, "t3", "a3");
        assert!(c.rules_cover_defs());
    }

    #[test]
    fn dup_and_empty_rejected() {
        let mut c = CauseCodex::new();
        assert_eq!(c.define(CauseClass::KernelBug, ""), Err(ERR_EMPTY_DEF));
        let _ = c.define(CauseClass::KernelBug, "d");
        assert_eq!(c.define(CauseClass::KernelBug, "x"), Err(ERR_DUP_CLASS));
        assert_eq!(c.match_rule("t1 symptom"), None);
    }

    #[test]
    fn missing_class_listing() {
        let mut c = CauseCodex::new();
        let _ = c.define(CauseClass::KernelBug, "d1");
        let _ = c.define(CauseClass::EnvLimited, "d2");
        let miss = c.missing_classes();
        assert_eq!(miss, vec!["EXPECT_GAP"]);
        assert!(!c.classes_complete());
        assert!(!c.rules_cover_defs());
    }

    #[test]
    fn definition_roundtrip_and_count() {
        let mut c = CauseCodex::new();
        let _ = c.define(CauseClass::ExpectGap, "arbitration needed");
        assert!(c.definition_of(CauseClass::ExpectGap).unwrap().contains("arbitration"));
        assert_eq!(c.rule_count_of(CauseClass::ExpectGap), 0);
        assert_eq!(c.avg_def_len(), 17);
    }

    #[test]
    fn rules_and_actions_listing() {
        let mut c = CauseCodex::new();
        for cl in [CauseClass::KernelBug, CauseClass::EnvLimited, CauseClass::ExpectGap] {
            let _ = c.define(cl, "def text");
            let _ = c.add_rule(cl, "trigger", "action");
        }
        assert_eq!(c.action_list().len(), 3);
        assert!(c.codex_summary().contains("complete=true"));
        assert!(c.all_rules_ok());
        assert!(c.avg_def_len() > 0);
    }
}
