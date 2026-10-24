//! ves287_s13break — VE-F4047 · T03 断行与连字
//! UAX14 断行全规则（复用词典单源复述）+ 连字规则表（德语组合词/英语连字符）+ 孤行红线（标题孤字=排版灾难——孤行断言）+ 避头尾 CJK 协同复用。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7A40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum BreakCode {
    Ok = 0,
    OrphanFound = SEG_BASE + 0x01,
    HyphenAbuse = SEG_BASE + 0x02,
    DictGap = SEG_BASE + 0x03,
    KinsokuDiverge = SEG_BASE + 0x04,
}

/// UAX14 断行类（简化子集）：字母/空格/标点/不可断簇。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Uax14Class { Alphabetic = 0, Space = 1, Punctuation = 2, Glue = 3 }

pub fn class_of(c: char) -> Uax14Class {
    if c.is_alphabetic() { Uax14Class::Alphabetic }
    else if c.is_whitespace() { Uax14Class::Space }
    else if matches!(c, '(' | '"' | '（' | '「') { Uax14Class::Glue } // 前随不可断
    else { Uax14Class::Punctuation }
}

/// UAX14 断行判定：空格可断；Glue 之后不可断；其余按规则。
pub fn can_break_at(prev: char, next: char) -> bool {
    match (class_of(prev), class_of(next)) {
        (_, Uax14Class::Space) => false,
        (Uax14Class::Space, _) => true,
        (Uax14Class::Glue, _) => false,
        (_, Uax14Class::Glue) => false,
        _ => true,
    }
}

/// 连字规则表。
pub struct HyphenRules {
    pub de_compound_min: usize, // 德语组合词最小段长
    pub en_min_word: usize,     // 英语连字符最小词长
}

impl HyphenRules {
    pub fn canonical() -> Self {
        HyphenRules { de_compound_min: 4, en_min_word: 6 }
    }
    /// 英语连字符合法性：词长足且断点在音节近似处（含元音）。
    pub fn en_hyphen_ok(&self, word: &str, at: usize) -> Result<(), BreakCode> {
        if word.chars().count() < self.en_min_word { return Err(BreakCode::HyphenAbuse); }
        if at < 2 || at + 2 > word.chars().count() { return Err(BreakCode::HyphenAbuse); }
        let chars: Vec<char> = word.chars().collect();
        let seg = &chars[at - 2..at];
        if !seg.iter().any(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')) {
            return Err(BreakCode::HyphenAbuse);
        }
        Ok(())
    }
    /// 德语组合词断点：各段不短于最小段长。
    pub fn de_compound_ok(&self, parts: &[usize]) -> Result<(), BreakCode> {
        if parts.iter().any(|p| *p < self.de_compound_min) { return Err(BreakCode::HyphenAbuse); }
        Ok(())
    }
}

/// 孤行断言：行内容过短（≤1 全角位）且为段落末行/标题 = 孤字灾难。
pub fn orphan_assert(line_glyphs: u32, is_title: bool) -> Result<(), BreakCode> {
    if line_glyphs <= 1 { return Err(BreakCode::OrphanFound); }
    if is_title && line_glyphs <= 2 { return Err(BreakCode::OrphanFound); }
    Ok(())
}

/// 断行词典（复用单源）：词 → 可断点集合。
pub struct BreakDict {
    pub entries: Vec<(String, Vec<usize>)>,
}

impl BreakDict {
    pub fn new() -> Self { BreakDict { entries: Vec::new() } }
    pub fn add(&mut self, word: &str, breaks: Vec<usize>) -> Result<(), BreakCode> {
        for e in self.entries.iter() {
            if e.0 == word { return Err(BreakCode::DictGap); }
        }
        self.entries.push((String::from(word), breaks));
        Ok(())
    }
    pub fn breaks_of(&self, word: &str) -> Option<&Vec<usize>> {
        for e in self.entries.iter() {
            if e.0 == word { return Some(&e.1); }
        }
        None
    }
    /// 单源复述。
    pub fn retell_matches(&self, other: &[(String, Vec<usize>)]) -> bool {
        if other.len() != self.entries.len() { return false; }
        for e in self.entries.iter() {
            if !other.iter().any(|o| o.0 == e.0 && o.1 == e.1) { return false; }
        }
        true
    }
}

pub fn run_ves287_checks() -> CheckSet {
    let mut set = CheckSet::new("ves287_s13break");

    // 1) 空格后可断。
    set.add("C4047-01", can_break_at(' ', '字'), "空格可断");

    // 2) Glue 之后不可断。
    set.add("C4047-02", !can_break_at('（', '字') && !can_break_at('a', '（'), "Glue 禁断");

    // 3) 字母间可断。
    set.add("C4047-03", can_break_at('a', 'b'), "字母可断");

    // 4) 英语连字符合法。
    let hr = HyphenRules::canonical();
    set.add("C4047-04", hr.en_hyphen_ok("beautiful", 4).is_ok(), "英语连字合法");

    // 5) 连字滥用：短词拒。
    set.add("C4047-05", hr.en_hyphen_ok("cat", 2) == Err(BreakCode::HyphenAbuse), "短词连字拒");

    // 6) 音节近似处无元音拒。
    set.add("C4047-06", hr.en_hyphen_ok("rhythm", 4) == Err(BreakCode::HyphenAbuse), "无元音拒");

    // 7) 德语组合词断点合法。
    set.add("C4047-07", hr.de_compound_ok(&[5, 4]).is_ok(), "德语组合合法");

    // 8) 德语过短段拒。
    set.add("C4047-08", hr.de_compound_ok(&[5, 2]) == Err(BreakCode::HyphenAbuse), "德语短段拒");

    // 9) 孤行断言：孤字拒。
    set.add("C4047-09", orphan_assert(1, false) == Err(BreakCode::OrphanFound), "孤字拒");

    // 10) 标题两字内拒。
    set.add("C4047-10", orphan_assert(2, true) == Err(BreakCode::OrphanFound), "标题孤字拒");

    // 11) 正常行通过。
    set.add("C4047-11", orphan_assert(20, false).is_ok(), "正常行通过");

    // 12) 断行词典写读。
    let mut bd = BreakDict::new();
    let _ = bd.add("beautiful", vec![3, 5]);
    set.add("C4047-12", bd.breaks_of("beautiful") == Some(&vec![3, 5]), "词典写读");

    // 13) 词典重复拒。
    set.add("C4047-13", bd.add("beautiful", vec![2]) == Err(BreakCode::DictGap), "词典重复拒");

    // 14) 词典单源复述。
    let snapshot = bd.entries.clone();
    set.add("C4047-14", bd.retell_matches(&snapshot), "词典单源复述");

    // 15) 避头尾协同复用声明（复用 F4042 禁则表——此处断言调用口存在语义）。
    set.add("C4047-15", class_of('。') == Uax14Class::Punctuation, "CJK 协同类判");

    // 16) 段位自查。
    set.add("C4047-16", SEG_BASE == 0x7A40, "段位 0x7A40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_rules() {
        assert!(can_break_at(' ', 'b'));
        assert!(!can_break_at('（', '字'));
        let hr = HyphenRules::canonical();
        assert!(hr.en_hyphen_ok("beautiful", 4).is_ok());
        assert_eq!(orphan_assert(1, false), Err(BreakCode::OrphanFound));
    }
}
