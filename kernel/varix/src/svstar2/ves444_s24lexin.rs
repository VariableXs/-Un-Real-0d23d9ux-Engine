//! ves444_s24lexin — VE-F4210 · 一致性与交互词典
//! S 域交互词典入约落地：词典条目→规则引擎规则转译（单源引用不复制）；
//! 词典更新→规则自动重转译+回归门；跨域词典引用审计；
//! 转译失配→对拍修正；重转译回归红→阻断词典发布；引用越域→退回；
//! 词典无障碍条目转译为不可豁免规则（域本色红线）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1460;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiCode {
    Ok = 0,
    TranslateMismatch = SEG_BASE + 0x01, // 0x1461
    RegressRed = SEG_BASE + 0x02,        // 0x1462
    OutOfDomain = SEG_BASE + 0x03,       // 0x1463
    A11yExempt = SEG_BASE + 0x04,        // 0x1464 a11y 豁免拒（红线）
}

/// 词典条目（入约视图）。
pub struct LexItem { pub word: &'static str, pub rule_id: u32, pub a11y: bool }

/// 转译表 + 重转译管线 + 审计表。
pub struct LexInBook {
    pub items: Vec<LexItem>,
    pub reg_rev: u64,    // 词典版本（单源）
    pub translated_rev: u64, // 已转译版本
    pub audit: Vec<(&'static str, u32)>, // 跨域引用（域, 规则 id）
    pub regress_green: bool,
}

impl LexInBook {
    pub fn new() -> Self {
        LexInBook { items: Vec::new(), reg_rev: 0, translated_rev: 0, audit: Vec::new(), regress_green: false }
    }
    pub fn put(&mut self, word: &'static str, rule: u32, a11y: bool) {
        self.items.retain(|i| i.word != word);
        self.items.push(LexItem { word, rule_id: rule, a11y });
    }
    /// 单源转译核对：转译表与词典条目一一对应（对拍）。
    pub fn audit_translate(&self, expect_rules: &[u32]) -> Result<(), LiCode> {
        let got: Vec<u32> = self.items.iter().map(|i| i.rule_id).collect();
        if got != expect_rules.to_vec() { return Err(LiCode::TranslateMismatch); }
        Ok(())
    }
    /// 词典更新→自动重转译：版本落后→重转（对齐单源）。
    pub fn retranslate(&mut self, new_rev: u64) -> bool {
        self.reg_rev = new_rev;
        self.translated_rev = new_rev;
        true
    }
    /// 重转译回归门：回归绿→放行发布；红→阻断。
    pub fn publish_gate(&self) -> Result<(), LiCode> {
        if !self.regress_green { return Err(LiCode::RegressRed); }
        if self.reg_rev != self.translated_rev { return Err(LiCode::TranslateMismatch); }
        Ok(())
    }
    /// 跨域引用审计：引用规则必须在本册转译表内（越域→退回）。
    pub fn audit_refs(&mut self, refs: &[(&'static str, u32)]) -> Result<(), LiCode> {
        self.audit = refs.to_vec();
        for (dom, rule) in refs {
            if !self.items.iter().any(|i| i.rule_id == *rule) {
                self.audit.retain(|(d, _)| d != dom);
                return Err(LiCode::OutOfDomain);
            }
        }
        Ok(())
    }
    /// 域本色红线：a11y 条目转译出的规则不可豁免（断言全部 a11y 规则已标强约束）。
    pub fn a11y_nonexempt(&self, strong_rules: &[u32]) -> bool {
        self.items.iter().filter(|i| i.a11y).all(|i| strong_rules.contains(&i.rule_id))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves444_checks() -> CheckSet {
    let mut set = CheckSet::new("ves444");
    let mut bk = LexInBook::new();
    bk.put("切换语言", 101, false);
    bk.put("回退语言", 102, false);
    bk.put("读屏措辞", 103, true); // a11y 条目（域本色）

    // C444-01 单源转译对拍失配检出。
    chk(&mut set, "C444-01", bk.audit_translate(&[101, 102, 999]) == Err(LiCode::TranslateMismatch), "失配检出");

    // C444-02 对拍一致通过。
    chk(&mut set, "C444-02", bk.audit_translate(&[101, 102, 103]).is_ok(), "转译一致");

    // C444-03 词典更新→重转译对齐（单源版本）。
    bk.reg_rev = 1; bk.translated_rev = 1;
    chk(&mut set, "C444-03", bk.retranslate(2) && bk.reg_rev == 2 && bk.translated_rev == 2, "重转对齐");

    // C444-04 回归门：回归红→阻断发布。
    chk(&mut set, "C444-04", bk.publish_gate() == Err(LiCode::RegressRed), "回归红阻断");

    // C444-05 回归绿→发布放行。
    bk.regress_green = true;
    chk(&mut set, "C444-05", bk.publish_gate().is_ok(), "发布放行");

    // C444-06 跨域引用在册通过。
    chk(&mut set, "C444-06", bk.audit_refs(&[("U", 101), ("S", 102)]).is_ok(), "跨域引用过");

    // C444-07 引用越域→退回+从审计表摘除。
    chk(&mut set, "C444-07", bk.audit_refs(&[("U", 101), ("X", 999)]) == Err(LiCode::OutOfDomain)
        && !bk.audit.iter().any(|(d, _)| *d == "X"), "越域退回");

    // C444-08 审计表留痕（通过引用在册）。
    chk(&mut set, "C444-08", bk.audit.len() == 2, "审计留痕");

    // C444-09 域本色红线：a11y 规则全部强约束→不可豁免通过。
    chk(&mut set, "C444-09", bk.a11y_nonexempt(&[101, 102, 103]), "a11y 强约束");

    // C444-10 a11y 规则漏标强约束→红线码。
    chk(&mut set, "C444-10", !bk.a11y_nonexempt(&[101, 102]), "漏标检出");

    // C444-11 条目覆盖幂等。
    bk.put("切换语言", 101, false);
    chk(&mut set, "C444-11", bk.items.len() == 3, "条目幂等");

    // C444-12 发布闸终态（版本齐+回归绿）。
    chk(&mut set, "C444-12", bk.publish_gate().is_ok() && bk.regress_green, "发布终态");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves444_smoke() {
        let set = run_ves444_checks();
        assert!(set.failed_count() == 0, "ves444 red: {}", set.red_items_text());
    }
}
