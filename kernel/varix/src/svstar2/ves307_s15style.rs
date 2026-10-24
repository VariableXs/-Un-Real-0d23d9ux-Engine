//! ves307_s15style — VE-F4067 · T04 文案风格指南
//! 语调/称谓/标点/数字/大小写五维规则×语言差异化定制 + 规则转审查器（与 F4143 单源对拍——不一致以实测为准修指南）+ 版本化 + 语种矩阵 + 读屏友好措辞专章。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8AC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum StCode {
    Ok = 0,
    RuleConflict = SEG_BASE + 0x01,
    LangMissing = SEG_BASE + 0x02,
    SingleSourceSplit = SEG_BASE + 0x03,
    VersionMissing = SEG_BASE + 0x04,
    A11yChapterMissing = SEG_BASE + 0x05,
}

/// 五维。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dim { Tone = 0, Address = 1, Punct = 2, Numeral = 3, Case = 4 }

impl Dim {
    pub const ALL: [Dim; 5] = [Dim::Tone, Dim::Address, Dim::Punct, Dim::Numeral, Dim::Case];
    pub fn name(self) -> &'static str {
        match self { Dim::Tone => "语调", Dim::Address => "称谓", Dim::Punct => "标点", Dim::Numeral => "数字", Dim::Case => "大小写" }
    }
}

/// 指南规则（版本化）。
#[derive(Clone)]
pub struct StyleRule {
    pub lang: &'static str,
    pub dim: Dim,
    pub content: String,
    pub version: u32,
    /// 转译出的审查器规则 id（单源对拍键）。
    pub checker_rule: &'static str,
}

pub struct StyleBook {
    pub rules: Vec<StyleRule>,
    pub a11y_chapter: bool,
    pub version: u32,
}

impl StyleBook {
    pub fn new() -> Self { StyleBook { rules: Vec::new(), a11y_chapter: false, version: 0 } }

    pub fn add(&mut self, r: StyleRule) -> Result<(), StCode> {
        if r.version == 0 { return Err(StCode::VersionMissing); }
        for e in self.rules.iter() {
            if e.lang == r.lang && e.dim == r.dim && e.content != r.content {
                return Err(StCode::RuleConflict);
            }
        }
        self.rules.push(r);
        Ok(())
    }

    /// 语种矩阵：要求语种五维全有规则。
    pub fn lang_gaps(&self, lang: &str) -> Vec<&'static str> {
        let mut g = Vec::new();
        for d in Dim::ALL.iter() {
            if !self.rules.iter().any(|r| r.lang == lang && r.dim == *d) {
                g.push(d.name());
            }
        }
        g
    }

    /// 单源对拍：指南规则 ↔ 审查器实测规则 id 一致。
    pub fn retell_matches(&self, checker_rules: &[(&'static str, bool)]) -> bool {
        self.rules.iter().all(|r| {
            checker_rules.iter().any(|(id, ok)| *id == r.checker_rule && *ok)
        })
    }

    /// 不一致以实测为准修指南（此处返回需修正的指南规则数）。
    pub fn diverged(&self, checker_rules: &[(&'static str, bool)]) -> usize {
        self.rules.iter().filter(|r| {
            !checker_rules.iter().any(|(id, ok)| *id == r.checker_rule && *ok)
        }).count()
    }
}

/// 读屏友好措辞检查：提示语不得以纯视觉词开头。
pub fn screen_reader_friendly(text: &str) -> bool {
    !(text.starts_with("见上图") || text.starts_with("如下所示") || text.starts_with("点击这里"))
}

pub fn run_ves307_checks() -> CheckSet {
    let mut set = CheckSet::new("ves307_s15style");
    let mut sb = StyleBook::new();

    // 1) 版本缺拒。
    set.add("C4067-01", sb.add(StyleRule { lang: "zh", dim: Dim::Tone, content: String::from("亲切"), version: 0, checker_rule: "CR1" }) == Err(StCode::VersionMissing), "版本缺拒");

    // 2) 规则入库。
    let _ = sb.add(StyleRule { lang: "zh", dim: Dim::Tone, content: String::from("亲切"), version: 1, checker_rule: "CR1" });
    let _ = sb.add(StyleRule { lang: "zh", dim: Dim::Punct, content: String::from("全角"), version: 1, checker_rule: "CR2" });
    set.add("C4067-02", sb.rules.len() == 2, "规则入库");

    // 3) 同维冲突拒。
    set.add("C4067-03", sb.add(StyleRule { lang: "zh", dim: Dim::Tone, content: String::from("严肃"), version: 1, checker_rule: "CR3" }) == Err(StCode::RuleConflict), "同维冲突拒");

    // 4) 语种矩阵缺口。
    set.add("C4067-04", sb.lang_gaps("zh").len() == 3, "五维缺口");

    // 5) 语种缺登记。
    set.add("C4067-05", sb.lang_gaps("ja").len() == 5, "整语种缺");

    // 6) 单源对拍一致。
    let cr = [("CR1", true), ("CR2", true)];
    set.add("C4067-06", sb.retell_matches(&cr), "单源对拍一致");

    // 7) 分叉检出与修正计数。
    let cr_bad = [("CR1", true), ("CR2", false)];
    set.add("C4067-07", !sb.retell_matches(&cr_bad) && sb.diverged(&cr_bad) == 1, "分叉检出");

    // 8) 读屏友好措辞。
    set.add("C4067-08", screen_reader_friendly("保存成功") && !screen_reader_friendly("见上图按钮"), "读屏措辞");

    // 9) 无障碍专章。
    sb.a11y_chapter = true;
    set.add("C4067-09", sb.a11y_chapter, "无障碍专章");

    // 10) 五维名双射。
    let mut names: Vec<&str> = Vec::new();
    for d in Dim::ALL.iter() { names.push(d.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4067-10", !ndup, "五维名双射");

    // 11) 段位自查。
    set.add("C4067-11", SEG_BASE == 0x8AC0, "段位 0x8AC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_flow() {
        let mut sb = StyleBook::new();
        let _ = sb.add(StyleRule { lang: "en", dim: Dim::Case, content: String::from("Title Case"), version: 1, checker_rule: "CR1" });
        assert_eq!(sb.lang_gaps("en").len(), 4);
    }
}
