//! ves301_s15t04arch — VE-F4061 · T04 开工与翻译流程总纲
//! 翻译生产线四段流水线（抽取→翻译→验证→发布）+ 五段冻结交接签名 + 十八项映射 + 分工复述单源 + 语料脱敏隐私红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8740;

pub const MAPPING_TOTAL: u8 = 18;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum T4Code {
    Ok = 0,
    HandoffMismatch = SEG_BASE + 0x01,
    RangeShort = SEG_BASE + 0x02,
    Unsigned = SEG_BASE + 0x03,
    PrivacyBreach = SEG_BASE + 0x04,
}

/// 流水线段。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage { Extract = 0, Translate = 1, Review = 2, Merge = 3, Regression = 4 }

impl Stage {
    pub const ALL: [Stage; 5] = [Stage::Extract, Stage::Translate, Stage::Review, Stage::Merge, Stage::Regression];
    pub fn name(self) -> &'static str {
        match self {
            Stage::Extract => "抽取", Stage::Translate => "翻译", Stage::Review => "评审",
            Stage::Merge => "合入", Stage::Regression => "回归",
        }
    }
}

/// 交接物：段 → 签名。
pub struct HandoffBook {
    pub signed: Vec<(Stage, String)>,
}

impl HandoffBook {
    pub fn new() -> Self { HandoffBook { signed: Vec::new() } }
    pub fn sign(&mut self, s: Stage, who: &str) -> Result<(), T4Code> {
        if who.trim().is_empty() { return Err(T4Code::Unsigned); }
        for e in self.signed.iter() {
            if e.0 == s { return Err(T4Code::HandoffMismatch); }
        }
        self.signed.push((s, String::from(who)));
        Ok(())
    }
    /// 段间交接：下游段必须有上游段签名才能开工。
    pub fn can_start(&self, s: Stage) -> bool {
        match s {
            Stage::Extract => true,
            Stage::Translate => self.signed.iter().any(|e| e.0 == Stage::Extract),
            Stage::Review => self.signed.iter().any(|e| e.0 == Stage::Translate),
            Stage::Merge => self.signed.iter().any(|e| e.0 == Stage::Review),
            Stage::Regression => self.signed.iter().any(|e| e.0 == Stage::Merge),
        }
    }
    pub fn all_signed(&self) -> bool {
        for s in Stage::ALL.iter() {
            if !self.signed.iter().any(|e| e.0 == *s) { return false; }
        }
        true
    }
    /// 交接物缺签阻断。
    pub fn blocked(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for s in Stage::ALL.iter() {
            if !self.signed.iter().any(|e| e.0 == *s) { v.push(s.name()); }
        }
        v
    }
}

/// 十八项范围映射。
pub const RANGE_ITEMS: [&str; MAPPING_TOTAL as usize] = [
    "文案抽取", "翻译记忆", "机翻协同", "译后编辑", "LQA", "风格指南",
    "上下文注入", "进度", "质量评分", "无障碍翻译", "安全", "性能",
    "调试", "测试", "示例", "文档", "fuzz", "联调",
];

/// 分工单源：T01 定机制、T04 定流程。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner { T01Mechanism = 0, T04Process = 1 }

pub fn division_retell(n: &[(Owner, Owner)]) -> bool {
    n.len() == 1 && n[0] == (Owner::T01Mechanism, Owner::T04Process)
}

/// 语料脱敏红线：入库前必须脱敏标记。
pub fn sanitize_required(sanitized: bool) -> Result<(), T4Code> {
    if sanitized { Ok(()) } else { Err(T4Code::PrivacyBreach) }
}

pub fn run_ves301_checks() -> CheckSet {
    let mut set = CheckSet::new("ves301_s15t04arch");
    let mut hb = HandoffBook::new();

    // 1) 五段冻结。
    set.add("C4061-01", Stage::ALL.len() == 5 && Stage::ALL[0] == Stage::Extract && Stage::ALL[4] == Stage::Regression, "五段冻结");

    // 2) 首段可直接开工。
    set.add("C4061-02", hb.can_start(Stage::Extract) && !hb.can_start(Stage::Translate), "首段开工");

    // 3) 段间交接链。
    let _ = hb.sign(Stage::Extract, "e");
    set.add("C4061-03", hb.can_start(Stage::Translate) && !hb.can_start(Stage::Review), "交接链");

    // 4) 重复签拒。
    set.add("C4061-04", hb.sign(Stage::Extract, "x") == Err(T4Code::HandoffMismatch), "重复签拒");

    // 5) 空签拒。
    set.add("C4061-05", hb.sign(Stage::Translate, " ") == Err(T4Code::Unsigned), "空签拒");

    // 6) 全签齐。
    let _ = hb.sign(Stage::Translate, "t");
    let _ = hb.sign(Stage::Review, "r");
    let _ = hb.sign(Stage::Merge, "m");
    let _ = hb.sign(Stage::Regression, "g");
    set.add("C4061-06", hb.all_signed() && hb.can_start(Stage::Regression), "全签齐");

    // 7) 缺签阻断清单。
    let hb2 = HandoffBook::new();
    let blocked = hb2.blocked();
    set.add("C4061-07", blocked.len() == 5 && blocked[0] == "抽取", "缺签阻断");

    // 8) 十八项映射齐。
    set.add("C4061-08", RANGE_ITEMS.len() == 18 && RANGE_ITEMS[0] == "文案抽取" && RANGE_ITEMS[17] == "联调", "十八项映射");

    // 9) 分工复述单源。
    set.add("C4061-09", division_retell(&[(Owner::T01Mechanism, Owner::T04Process)]), "分工单源");

    // 10) 语料脱敏红线。
    set.add("C4061-10", sanitize_required(true).is_ok() && sanitize_required(false) == Err(T4Code::PrivacyBreach), "脱敏红线");

    // 11) 段名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in Stage::ALL.iter() { names.push(s.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4061-11", !ndup, "段名双射");

    // 12) 段位自查。
    set.add("C4061-12", SEG_BASE == 0x8740, "段位 0x8740");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_chain() {
        let mut hb = HandoffBook::new();
        assert!(hb.can_start(Stage::Extract));
        assert!(!hb.can_start(Stage::Merge));
        let _ = hb.sign(Stage::Extract, "a");
        let _ = hb.sign(Stage::Translate, "b");
        let _ = hb.sign(Stage::Review, "c");
        assert!(hb.can_start(Stage::Merge));
    }
}
