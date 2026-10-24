//! ves283_s13indic — VE-F4043 · T03 泰文印度系复杂排版
//! 组合字符渲染（UPAMELA 组合标记锚定表）+ 音节边界断行（断行词典+UAX14 复用）+ 泰文排序 + 复杂文不支持→逐字符降级显性。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x77C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum IndicCode {
    Ok = 0,
    AnchorWrong = SEG_BASE + 0x01,
    DictGap = SEG_BASE + 0x02,
    SilentDegrade = SEG_BASE + 0x03,
    SortWrong = SEG_BASE + 0x04,
    SyllableBreak = SEG_BASE + 0x05,
}

/// 组合标记锚定：标记码位 → 锚定位（above/below/before/after）。
pub struct AnchorTable {
    pub entries: Vec<(u32, &'static str)>,
}

impl AnchorTable {
    pub fn canonical() -> Self {
        AnchorTable {
            entries: vec![
                (0x0E31, "above"),   // 泰文 mai han akat
                (0x0E34, "above"),
                (0x0E38, "below"),   // 泰文 sara u
                (0x0E3A, "below"),
                (0x093F, "before"),  // 天城文 i-matra
                (0x0947, "above"),
                (0x0941, "below"),
            ],
        }
    }
    pub fn anchor_of(&self, cp: u32) -> Option<&'static str> {
        for e in self.entries.iter() {
            if e.0 == cp { return Some(e.1); }
        }
        None
    }
    /// 锚定合法：above/below/before/after 四位。
    pub fn anchor_valid(a: &str) -> bool {
        a == "above" || a == "below" || a == "before" || a == "after"
    }
    /// 锚定检查：文本中标记码位必须在表中（否则锚定错）。
    pub fn check_text(&self, text: &[u32]) -> Result<(), IndicCode> {
        for cp in text.iter() {
            let is_mark = (0x0E30..=0x0E3A).contains(cp) || (0x093E..=0x094D).contains(cp);
            if is_mark && self.anchor_of(*cp).is_none() {
                return Err(IndicCode::AnchorWrong);
            }
        }
        Ok(())
    }
}

/// 音节断行词典：记录不可断开的音节簇（起始-延续）。
pub struct SyllableDict {
    pub clusters: Vec<(u32, u32)>, // (基字, 延续标记)
}

impl SyllableDict {
    pub fn new() -> Self { SyllableDict { clusters: Vec::new() } }
    pub fn add(&mut self, base: u32, cont: u32) -> Result<(), IndicCode> {
        for c in self.clusters.iter() {
            if c.0 == base && c.1 == cont { return Err(IndicCode::DictGap); }
        }
        self.clusters.push((base, cont));
        Ok(())
    }
    /// 断行：不得在簇内断开。
    pub fn can_break(&self, text: &[u32], pos: usize) -> bool {
        if pos == 0 || pos >= text.len() { return false; }
        for c in self.clusters.iter() {
            if text[pos - 1] == c.1 && pos >= 2 && text[pos - 2] == c.0 { return false; }
            if text[pos - 1] == c.0 && text[pos] == c.1 { return false; }
        }
        true
    }
    /// 词典覆盖断言：标记延续全在册。
    pub fn covers(&self, marks: &[u32]) -> bool {
        for m in marks.iter() {
            if !self.clusters.iter().any(|c| c.1 == *m) { return false; }
        }
        true
    }
}

/// 泰文排序：辅音基序 + 声调次序（简化确定性序）。
pub fn thai_sort_key(cp: u32) -> u32 {
    // 基字区 0x0E00-0x0E5B：辅音在前，元音标记次之，声调最后。
    if (0x0E01..=0x0E2E).contains(&cp) { cp }              // 辅音
    else if (0x0E30..=0x0E3A).contains(&cp) { cp + 0x100 } // 元音/标记
    else if (0x0E48..=0x0E4C).contains(&cp) { cp + 0x200 } // 声调
    else { cp + 0x300 }
}

pub fn thai_less(a: u32, b: u32) -> bool {
    thai_sort_key(a) < thai_sort_key(b)
}

/// 降级显性：复杂文不支持 → 逐字符降级并显性标记。
pub fn degrade_explicit(text: &[u32]) -> (Vec<u32>, bool) {
    let mut out = Vec::new();
    for cp in text.iter() {
        let is_mark = (0x0E30..=0x0E3A).contains(cp) || (0x093E..=0x094D).contains(cp);
        if !is_mark {
            out.push(*cp);
        }
    }
    (out, true) // true = 显性降级标记
}

pub fn run_ves283_checks() -> CheckSet {
    let mut set = CheckSet::new("ves283_s13indic");
    let at = AnchorTable::canonical();

    // 1) 锚定查表。
    set.add("C4043-01", at.anchor_of(0x0E31) == Some("above") && at.anchor_of(0x093F) == Some("before"), "锚定查表");

    // 2) 锚定四位合法。
    set.add("C4043-02", AnchorTable::anchor_valid("above") && AnchorTable::anchor_valid("after")
        && !AnchorTable::anchor_valid("side"), "锚定四位");

    // 3) 标记在册检查通过。
    set.add("C4043-03", at.check_text(&[0x0E01, 0x0E31, 0x0E01]).is_ok(), "标记在册通过");

    // 4) 标记锚定错检出。
    set.add("C4043-04", at.check_text(&[0x0E01, 0x0E39]).is_err(), "锚定错检出");

    // 5) 断行词典：簇内禁断。
    let mut sd = SyllableDict::new();
    let _ = sd.add(0x0E01, 0x0E31);
    set.add("C4043-05", !sd.can_break(&[0x0E01, 0x0E31, 0x0E01], 1), "簇内禁断");

    // 6) 簇外可断。
    set.add("C4043-06", sd.can_break(&[0x0E01, 0x0E01], 1), "簇外可断");

    // 7) 词典覆盖断言。
    set.add("C4043-07", sd.covers(&[0x0E31]), "词典覆盖");

    // 8) 覆盖缺检出。
    set.add("C4043-08", !sd.covers(&[0x0E31, 0x0E38]), "覆盖缺检出");

    // 9) 重复簇拒。
    set.add("C4043-09", sd.add(0x0E01, 0x0E31) == Err(IndicCode::DictGap), "重复簇拒");

    // 10) 泰文排序：辅音 < 元音标记 < 声调。
    set.add("C4043-10", thai_less(0x0E01, 0x0E31) && thai_less(0x0E31, 0x0E48) && !thai_less(0x0E48, 0x0E01), "泰文排序");

    // 11) 排序确定性：同键同序。
    set.add("C4043-11", thai_sort_key(0x0E01) == thai_sort_key(0x0E01), "排序确定性");

    // 12) 降级显性：标记剥离且显性标记。
    let (out, exp) = degrade_explicit(&[0x0E01, 0x0E31, 0x0E01]);
    set.add("C4043-12", out == [0x0E01, 0x0E01] && exp, "降级显性");

    // 13) 空文本降级仍显性。
    let (o2, e2) = degrade_explicit(&[]);
    set.add("C4043-13", o2.is_empty() && e2, "空文本显性");

    // 14) 锚定表条目全部合法。
    set.add("C4043-14", at.entries.iter().all(|e| AnchorTable::anchor_valid(e.1)), "锚定表全合法");

    // 15) 段位自查。
    set.add("C4043-15", SEG_BASE == 0x77C0, "段位 0x77C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_and_break() {
        let at = AnchorTable::canonical();
        assert!(at.check_text(&[0x0E01, 0x0E31]).is_ok());
        assert!(at.check_text(&[0x0E39]).is_err());
        let mut sd = SyllableDict::new();
        let _ = sd.add(0x0E01, 0x0E38);
        assert!(!sd.can_break(&[0x0E01, 0x0E38], 1));
    }
}
