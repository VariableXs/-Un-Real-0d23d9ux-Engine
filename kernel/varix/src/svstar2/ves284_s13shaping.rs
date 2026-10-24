//! ves284_s13shaping — VE-F4044 · T03 阿拉伯整形深化
//! GSUB/GPOS 整形执行（连写变形/标记定位）+ 整形对拍（与参考引擎零分歧——整形分歧=文字不可读 P0）+ 整形缓存（复用热表单源）+ 整形回退降级显性。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7840;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ShapeCode {
    Ok = 0,
    GsubDiverge = SEG_BASE + 0x01,
    CacheBroken = SEG_BASE + 0x02,
    FallbackSilent = SEG_BASE + 0x03,
    TableDamaged = SEG_BASE + 0x04,
    GposWrong = SEG_BASE + 0x05,
}

/// 阿拉伯连写形态：isolated/final/initial/medial。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JoinForm { Isolated = 0, Final = 1, Initial = 2, Medial = 3 }

impl JoinForm {
    pub const ALL: [JoinForm; 4] = [JoinForm::Isolated, JoinForm::Final, JoinForm::Initial, JoinForm::Medial];
    pub fn name(self) -> &'static str {
        match self {
            JoinForm::Isolated => "isolated", JoinForm::Final => "final",
            JoinForm::Initial => "initial", JoinForm::Medial => "medial",
        }
    }
}

/// GSUB 连写变形执行：根据前后连接性推导形态。
pub fn join_form(prev_joins: bool, next_joins: bool) -> JoinForm {
    match (prev_joins, next_joins) {
        (false, false) => JoinForm::Isolated,
        (true, false) => JoinForm::Final,
        (false, true) => JoinForm::Initial,
        (true, true) => JoinForm::Medial,
    }
}

/// 字符连接性表（简化：阿文字母均可右连；双连接字母可左连）。
pub fn joins_left(cp: u32) -> bool {
    // 双连接字母（可向左连接）示意集合。
    matches!(cp, 0x0628 | 0x062A | 0x062B | 0x0633 | 0x0639 | 0x0641 | 0x0644 | 0x0646)
}

/// 整形引擎：对文本逐字推导 GSUB 形态序列。
pub fn shape_gsub(text: &[u32]) -> Vec<JoinForm> {
    let mut out = Vec::new();
    for i in 0..text.len() {
        let prev = i > 0 && joins_left(text[i - 1]);
        let next = i + 1 < text.len() && joins_left(text[i]);
        out.push(join_form(prev, next));
    }
    out
}

/// GPOS 标记定位：上方标记（fatha 等）锚定在基字上方。
pub fn gpos_above(cp: u32) -> Result<bool, ShapeCode> {
    match cp {
        0x064E | 0x064F | 0x0650 => Ok(true),   // fatha/damma/kasra 族
        0x0651 => Ok(true),                     // shadda
        0x064B..=0x0652 => Ok(true),
        _ => {
            if cp >= 0x0600 && cp <= 0x06FF { Ok(false) } // 基字
            else { Err(ShapeCode::GposWrong) }
        }
    }
}

/// 整形对拍：与参考引擎形态序列零分歧。
pub fn shape_agrees(mine: &[JoinForm], reference: &[JoinForm]) -> bool {
    mine.len() == reference.len() && mine.iter().zip(reference.iter()).all(|(a, b)| a == b)
}

/// 整形缓存：热表单源（key = 文本指纹）。
pub struct ShapeCache {
    pub entries: Vec<(u64, Vec<u8>)>,
}

impl ShapeCache {
    pub fn new() -> Self { ShapeCache { entries: Vec::new() } }
    pub fn put(&mut self, fp: u64, forms: &[u8]) -> Result<(), ShapeCode> {
        for e in self.entries.iter() {
            if e.0 == fp { return Err(ShapeCode::CacheBroken); }
        }
        self.entries.push((fp, forms.to_vec()));
        Ok(())
    }
    pub fn get(&self, fp: u64) -> Option<Vec<u8>> {
        for e in self.entries.iter() {
            if e.0 == fp { return Some(e.1.clone()); }
        }
        None
    }
    pub fn single_source(&self) -> bool {
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].0 == self.entries[j].0 { return false; }
            }
        }
        true
    }
}

/// 整形回退：失败 → 逐字符降级 + 显性标记（不得静默）。
pub fn shape_fallback(text: &[u32]) -> (Vec<u32>, bool) {
    let mut out = Vec::new();
    for cp in text.iter() {
        if *cp >= 0x0600 && cp <= &0x06FF { out.push(*cp); }
    }
    (out, true)
}

pub fn run_ves284_checks() -> CheckSet {
    let mut set = CheckSet::new("ves284_s13shaping");

    // 1) 连写形态推导四象限。
    set.add("C4044-01", join_form(false, false) == JoinForm::Isolated
        && join_form(true, false) == JoinForm::Final
        && join_form(false, true) == JoinForm::Initial
        && join_form(true, true) == JoinForm::Medial, "连写四象限");

    // 2) GSUB 序列：贝特（0x0628 双连）夹心。
    let seq = shape_gsub(&[0x0644, 0x0628, 0x0646]);
    set.add("C4044-02", seq[0] == JoinForm::Initial && seq[1] == JoinForm::Medial && seq[2] == JoinForm::Final, "GSUB 序列");

    // 3) 孤立字母。
    let seq2 = shape_gsub(&[0x0621]);
    set.add("C4044-03", seq2[0] == JoinForm::Isolated, "孤立形态");

    // 4) 整形对拍零分歧。
    let mine = shape_gsub(&[0x0644, 0x0628]);
    let reference = vec![JoinForm::Initial, JoinForm::Final];
    set.add("C4044-04", shape_agrees(&mine, &reference), "对拍零分歧");

    // 5) 对拍分歧 = P0 红线。
    let ref_bad = vec![JoinForm::Isolated, JoinForm::Final];
    set.add("C4044-05", !shape_agrees(&mine, &ref_bad), "分歧 P0 红线");

    // 6) GPOS 标记上方。
    set.add("C4044-06", gpos_above(0x064E) == Ok(true) && gpos_above(0x0651) == Ok(true), "GPOS 上方");

    // 7) 基字非标记。
    set.add("C4044-07", gpos_above(0x0628) == Ok(false), "基字非标记");

    // 8) 阿区外拒。
    set.add("C4044-08", gpos_above(0x0041) == Err(ShapeCode::GposWrong), "区外拒");

    // 9) 缓存写读。
    let mut sc = ShapeCache::new();
    let _ = sc.put(0xBEEF, &[2, 3]);
    set.add("C4044-09", sc.get(0xBEEF) == Some(vec![2, 3]), "缓存写读");

    // 10) 缓存单源：同 key 双份拒。
    set.add("C4044-10", sc.put(0xBEEF, &[1]) == Err(ShapeCode::CacheBroken) && sc.single_source(), "缓存单源");

    // 11) 回退显性。
    let (fb, exp) = shape_fallback(&[0x0628, 0x064E]);
    set.add("C4044-11", fb.len() == 2 && exp, "回退显性");

    // 12) 形态名双射。
    let mut names: Vec<&str> = Vec::new();
    for f in JoinForm::ALL.iter() { names.push(f.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4044-12", !ndup, "形态名双射");

    // 13) 空文本整形。
    set.add("C4044-13", shape_gsub(&[]).is_empty(), "空文本整形");

    // 14) 段位自查。
    set.add("C4044-14", SEG_BASE == 0x7840, "段位 0x7840");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaping_flow() {
        assert_eq!(join_form(true, true), JoinForm::Medial);
        let seq = shape_gsub(&[0x0644, 0x0628, 0x0646]);
        assert_eq!(seq, vec![JoinForm::Initial, JoinForm::Medial, JoinForm::Final]);
    }
}
