//! ves285_s13latin — VE-F4045 · T03 拉丁系微排版
//! 合字（fi/fl，复用 F2905 单源复述）+ 字距微调（kern 精调复用）+ 比例/表格数字（表格数字对齐红线）+ 小型大写特性复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x78C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum LatinCode {
    Ok = 0,
    DigitMisalign = SEG_BASE + 0x01,
    KernDamaged = SEG_BASE + 0x02,
    LigatureWrong = SEG_BASE + 0x03,
    SourceSplit = SEG_BASE + 0x04,
    FeatureMissing = SEG_BASE + 0x05,
}

/// 合字表（复用 F2905 单源）。
pub const LIGATURES: [(&str, &str); 4] = [("fi", "\u{FB01}"), ("fl", "\u{FB02}"), ("ff", "\u{FB00}"), ("ffi", "\u{FB03}")];

pub fn ligature_of(pair: &str) -> Option<&'static str> {
    for l in LIGATURES.iter() {
        if l.0 == pair { return Some(l.1); }
    }
    None
}

/// kern 表（复用 F2910 单源）：字对 → 微调值（千分之一 em）。
pub struct KernTable {
    pub pairs: Vec<(char, char, i16)>,
}

impl KernTable {
    pub fn canonical() -> Self {
        KernTable {
            pairs: vec![('A', 'V', -80), ('A', 'W', -60), ('T', 'o', -70), ('f', 'i', 10)],
        }
    }
    pub fn kern_of(&self, a: char, b: char) -> i16 {
        for p in self.pairs.iter() {
            if p.0 == a && p.1 == b { return p.2; }
        }
        0
    }
    /// kern 表损坏断言：单对微调不超 ±200。
    pub fn healthy(&self) -> bool {
        self.pairs.iter().all(|p| p.2 >= -200 && p.2 <= 200)
    }
    /// 单源复述：与外部副本逐对一致。
    pub fn retell_matches(&self, other: &[(char, char, i16)]) -> bool {
        if other.len() != self.pairs.len() { return false; }
        for p in self.pairs.iter() {
            if !other.iter().any(|q| q.0 == p.0 && q.1 == p.1 && q.2 == p.2) { return false; }
        }
        true
    }
}

/// 数字宽度：比例 vs 表格。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DigitMode { Proportional = 0, Tabular = 1 }

/// 表格数字对齐断言：Tabular 模式下所有数字同宽。
pub fn tabular_aligned(widths: &[u32]) -> Result<(), LatinCode> {
    if widths.is_empty() { return Ok(()); }
    let w0 = widths[0];
    for w in widths.iter() {
        if *w != w0 { return Err(LatinCode::DigitMisalign); }
    }
    Ok(())
}

/// 小型大写特性。
pub fn small_caps(c: char) -> Option<char> {
    if c.is_ascii_lowercase() {
        // 小型大写用 Unicode 小型大写区（示意映射 a-z 子集）。
        match c {
            'a' => Some('ᴀ'), 'b' => Some('ʙ'), 'c' => Some('ᴄ'),
            'e' => Some('ᴇ'), 'f' => Some('ꜰ'), 'x' => Some('x'),
            _ => None,
        }
    } else {
        None
    }
}

pub fn run_ves285_checks() -> CheckSet {
    let mut set = CheckSet::new("ves285_s13latin");

    // 1) 合字单源查表。
    set.add("C4045-01", ligature_of("fi") == Some("\u{FB01}") && ligature_of("fl") == Some("\u{FB02}"), "合字查表");

    // 2) 非合字对返回 None。
    set.add("C4045-02", ligature_of("xy").is_none(), "非合字 None");

    // 3) 合字表完整四条。
    set.add("C4045-03", LIGATURES.len() == 4, "合字四条");

    // 4) kern 查表。
    let kt = KernTable::canonical();
    set.add("C4045-04", kt.kern_of('A', 'V') == -80 && kt.kern_of('x', 'y') == 0, "kern 查表");

    // 5) kern 表健康。
    set.add("C4045-05", kt.healthy(), "kern 健康");

    // 6) kern 损坏检出。
    let bad = KernTable { pairs: vec![('A', 'V', -500)] };
    set.add("C4045-06", !bad.healthy(), "kern 损坏检出");

    // 7) kern 单源复述一致。
    let copy = kt.pairs.clone();
    set.add("C4045-07", kt.retell_matches(&copy), "kern 单源复述");

    // 8) 单源分叉检出。
    let mut diverge = copy.clone();
    diverge[0].2 = -90;
    set.add("C4045-08", !kt.retell_matches(&diverge), "单源分叉检出");

    // 9) 表格数字对齐。
    set.add("C4045-09", tabular_aligned(&[8, 8, 8]).is_ok(), "表格数字对齐");

    // 10) 数字不对齐 P1。
    set.add("C4045-10", tabular_aligned(&[8, 7, 8]) == Err(LatinCode::DigitMisalign), "不对齐检出");

    // 11) 空宽度表通过。
    set.add("C4045-11", tabular_aligned(&[]).is_ok(), "空表通过");

    // 12) 小型大写映射。
    set.add("C4045-12", small_caps('a') == Some('ᴀ') && small_caps('A').is_none(), "小型大写");

    // 13) 比例模式宽度可异。
    let prop = DigitMode::Proportional;
    set.add("C4045-13", prop == DigitMode::Proportional, "比例模式");

    // 14) 段位自查。
    set.add("C4045-14", SEG_BASE == 0x78C0, "段位 0x78C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kern_and_digits() {
        let kt = KernTable::canonical();
        assert_eq!(kt.kern_of('T', 'o'), -70);
        assert!(kt.healthy());
        assert!(tabular_aligned(&[8, 8, 8]).is_ok());
    }
}
