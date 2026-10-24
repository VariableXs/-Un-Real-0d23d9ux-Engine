//! ves286_s13mixline — VE-F4046 · T03 混排基线与间距
//! CJK×拉丁混排基线（复用 F2909 基线表单源复述）+ 中西文间距规则表 + 混排行高（取最大度量——行高塌陷 P0 红线）+ 混排×BiDi isolate 复用。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x79C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum MixCode {
    Ok = 0,
    BaselineWrong = SEG_BASE + 0x01,
    Collapse = SEG_BASE + 0x02,
    GapRuleMissing = SEG_BASE + 0x03,
    IsolateMissing = SEG_BASE + 0x04,
}

/// 基线表（复用 F2909 单源）：文种 → 基线偏移（1/1000 em）。
pub struct BaselineBook {
    pub entries: Vec<(&'static str, i32)>,
}

impl BaselineBook {
    pub fn canonical() -> Self {
        BaselineBook {
            entries: vec![
                ("latin", 0),
                ("cjk", 88),      // CJK 视觉中心下沉
                ("arabic", -40),  // 阿连写下沉
                ("indic", 20),
            ],
        }
    }
    pub fn offset_of(&self, script: &str) -> Option<i32> {
        for e in self.entries.iter() {
            if e.0 == script { return Some(e.1); }
        }
        None
    }
    /// 单源复述。
    pub fn retell_matches(&self, other: &[(&'static str, i32)]) -> bool {
        if other.len() != self.entries.len() { return false; }
        for e in self.entries.iter() {
            if !other.iter().any(|o| o.0 == e.0 && o.1 == e.1) { return false; }
        }
        true
    }
}

/// 中西文间距规则：CJK↔Latin 邻接自动加 1/8 em。
pub fn mixed_gap(prev_cjk: bool, next_cjk: bool) -> u32 {
    if prev_cjk != next_cjk { 125 } else { 0 } // 1/8 em = 125/1000
}

/// 行高断言：混排行高取最大度量，塌陷（行高 < 最大字高）= P0。
pub fn line_height_assert(cell_heights: &[u32], line_height: u32) -> Result<(), MixCode> {
    if cell_heights.is_empty() { return Ok(()); }
    let max_h = *cell_heights.iter().max().unwrap();
    if line_height < max_h { return Err(MixCode::Collapse); }
    Ok(())
}

/// isolate 复用断言：混排中 BiDi 段必须有 isolate 包裹（复用 F4023 单源）。
pub fn isolate_required(has_bidi_mixed: bool, isolate_on: bool) -> Result<(), MixCode> {
    if has_bidi_mixed && !isolate_on { return Err(MixCode::IsolateMissing); }
    Ok(())
}

pub fn run_ves286_checks() -> CheckSet {
    let mut set = CheckSet::new("ves286_s13mixline");
    let bb = BaselineBook::canonical();

    // 1) 基线查表。
    set.add("C4046-01", bb.offset_of("cjk") == Some(88) && bb.offset_of("latin") == Some(0), "基线查表");

    // 2) 基线错位检出：未知文种。
    set.add("C4046-02", bb.offset_of("xx").is_none(), "未知文种");

    // 3) 基线单源复述。
    let copy = bb.entries.clone();
    set.add("C4046-03", bb.retell_matches(&copy), "基线单源复述");

    // 4) 复述分叉检出。
    let mut bad = copy.clone();
    bad[1].1 = 100;
    set.add("C4046-04", !bb.retell_matches(&bad), "复述分叉检出");

    // 5) 中西文间距：邻接加 1/8 em。
    set.add("C4046-05", mixed_gap(true, false) == 125 && mixed_gap(false, true) == 125, "邻接间距");

    // 6) 同文种间距零。
    set.add("C4046-06", mixed_gap(true, true) == 0 && mixed_gap(false, false) == 0, "同文种零距");

    // 7) 行高取最大度量。
    set.add("C4046-07", line_height_assert(&[1000, 1200, 900], 1200).is_ok(), "行高最大");

    // 8) 行高塌陷 P0。
    set.add("C4046-08", line_height_assert(&[1000, 1200], 1100) == Err(MixCode::Collapse), "塌陷 P0");

    // 9) 空行通过。
    set.add("C4046-09", line_height_assert(&[], 0).is_ok(), "空行通过");

    // 10) isolate 缺检出。
    set.add("C4046-10", isolate_required(true, false) == Err(MixCode::IsolateMissing), "isolate 缺");

    // 11) isolate 复用通过。
    set.add("C4046-11", isolate_required(true, true).is_ok() && isolate_required(false, false).is_ok(), "isolate 复用");

    // 12) 间距规则表覆盖两向。
    set.add("C4046-12", mixed_gap(true, false) == mixed_gap(false, true), "间距两向对称");

    // 13) 基线表四文种齐。
    set.add("C4046-13", bb.entries.len() == 4, "基线四文种");

    // 14) 段位自查。
    set.add("C4046-14", SEG_BASE == 0x79C0, "段位 0x79C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_and_gap() {
        let bb = BaselineBook::canonical();
        assert_eq!(bb.offset_of("arabic"), Some(-40));
        assert_eq!(mixed_gap(true, false), 125);
        assert!(line_height_assert(&[1200], 1300).is_ok());
    }
}
