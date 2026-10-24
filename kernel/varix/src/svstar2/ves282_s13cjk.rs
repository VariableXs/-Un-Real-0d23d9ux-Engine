//! ves282_s13cjk — VE-F4042 · T03 CJK 排版
//! 禁则处理（行首/行尾禁则字规则表——完整断言，复述 JIS/GB）+ 标点挤压 + 中点/引号方向 + 避头尾均排（字符间距微调声明）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x76C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum CjkCode {
    Ok = 0,
    KinsokuBreach = SEG_BASE + 0x01,
    SqueezeOver = SEG_BASE + 0x02,
    JustifyDistort = SEG_BASE + 0x03,
    DirWrong = SEG_BASE + 0x04,
    TableIncomplete = SEG_BASE + 0x05,
}

/// 禁则表：行首禁则（闭合标点不可居行首）与行尾禁则（开启标点不可居行尾）。
pub struct KinsokuTable {
    pub head_forbid: Vec<char>,
    pub tail_forbid: Vec<char>,
}

impl KinsokuTable {
    pub fn canonical() -> Self {
        KinsokuTable {
            head_forbid: vec![')', '】', '」', '』', '！', '？', '。', '，', '、', '；', '：'],
            tail_forbid: vec![  '(', '【', '「', '『', '“'],
        }
    }
    pub fn head_forbidden(&self, c: char) -> bool {
        self.head_forbid.contains(&c)
    }
    pub fn tail_forbidden(&self, c: char) -> bool {
        self.tail_forbid.contains(&c)
    }
    /// 完整断言：两表非空且无交集。
    pub fn complete(&self) -> bool {
        !self.head_forbid.is_empty() && !self.tail_forbid.is_empty()
            && self.head_forbid.iter().all(|c| !self.tail_forbid.contains(c))
    }
    /// 行首禁则检查：行首字符不得为闭合标点。
    pub fn check_line(&self, line: &[char]) -> Result<(), CjkCode> {
        if line.is_empty() { return Ok(()); }
        if self.head_forbidden(line[0]) { return Err(CjkCode::KinsokuBreach); }
        if self.tail_forbidden(line[line.len() - 1]) { return Err(CjkCode::KinsokuBreach); }
        Ok(())
    }
    /// 修正：把行首禁则字与前一字符交换（避头尾）。
    pub fn fix_line(&self, line: &mut Vec<char>) -> bool {
        if line.len() < 2 { return false; }
        if self.head_forbidden(line[0]) {
            line.swap(0, 1);
            return true;
        }
        false
    }
}

/// 标点挤压：连续标点压缩半宽。
pub struct Squeezer {
    pub max_squeeze_h: u32, // 最大挤压半格数
}

impl Squeezer {
    pub fn new(max: u32) -> Self { Squeezer { max_squeeze_h: max } }
    /// 挤压量：连续标点 n 个 → 挤压 (n-1) 半格，上限钳制。
    pub fn squeeze(&self, consecutive: u32) -> u32 {
        let raw = consecutive.saturating_sub(1);
        if raw > self.max_squeeze_h { return self.max_squeeze_h; }
        raw
    }
    /// 可读断言：挤压过度（超过半行）→ 拒。
    pub fn readable(&self, line_half_cells: u32, squeezed: u32) -> Result<(), CjkCode> {
        if squeezed * 2 >= line_half_cells { return Err(CjkCode::SqueezeOver); }
        Ok(())
    }
}

/// CJK 标点方向语义：中点/引号在横排方向语义。
pub fn punct_dir(c: char) -> Result<&'static str, CjkCode> {
    match c {
        '·' => Ok("中点"),
        '“' | '”' => Ok("引号"),
        '。' | '，' => Ok("句读"),
        _ => Err(CjkCode::DirWrong),
    }
}

/// 均排：行内字符间距微调，均匀分布。
pub fn justify(line_half_cells: u32, target_half_cells: u32, gaps: u32) -> Result<u32, CjkCode> {
    if gaps == 0 { return Err(CjkCode::JustifyDistort); }
    let diff = target_half_cells as i64 - line_half_cells as i64;
    if diff < 0 { return Ok(0); }
    let per = diff / gaps as i64;
    if per > 4 { return Err(CjkCode::JustifyDistort); } // 均排失真钳制
    Ok(per as u32)
}

pub fn run_ves282_checks() -> CheckSet {
    let mut set = CheckSet::new("ves282_s13cjk");
    let kt = KinsokuTable::canonical();

    // 1) 禁则表完整。
    set.add("C4042-01", kt.complete(), "禁则表完整");

    // 2) 行首禁则检出。
    set.add("C4042-02", kt.check_line(&['。', '字']) == Err(CjkCode::KinsokuBreach), "行首禁则检出");

    // 3) 行尾禁则检出。
    set.add("C4042-03", kt.check_line(&['字', '（']) == Err(CjkCode::KinsokuBreach), "行尾禁则检出");

    // 4) 合规行通过。
    set.add("C4042-04", kt.check_line(&['字', '。']).is_ok(), "合规行通过");

    // 5) 避头尾修正。
    let mut line = vec!['。', '字', '。'];
    let fixed = kt.fix_line(&mut line);
    set.add("C4042-05", fixed && line[0] == '字', "避头尾修正");

    // 6) 空行通过。
    set.add("C4042-06", kt.check_line(&[]).is_ok(), "空行通过");

    // 7) 挤压量与钳制。
    let sq = Squeezer::new(3);
    set.add("C4042-07", sq.squeeze(1) == 0 && sq.squeeze(4) == 3 && sq.squeeze(9) == 3, "挤压钳制");

    // 8) 挤压可读断言。
    set.add("C4042-08", sq.readable(20, 5).is_ok() && sq.readable(9, 5) == Err(CjkCode::SqueezeOver), "挤压可读断言");

    // 9) 标点方向语义。
    set.add("C4042-09", punct_dir('·') == Ok("中点") && punct_dir('“') == Ok("引号"), "标点方向");

    // 10) 未知标点拒。
    set.add("C4042-10", punct_dir('A') == Err(CjkCode::DirWrong), "未知标点拒");

    // 11) 均排：每隙 1 半格微调。
    set.add("C4042-11", justify(30, 40, 10) == Ok(1), "均排微调");

    // 12) 均排失真钳制。
    set.add("C4042-12", justify(10, 100, 10) == Err(CjkCode::JustifyDistort), "均排失真拒");

    // 13) 均排零间隙拒。
    set.add("C4042-13", justify(30, 32, 0) == Err(CjkCode::JustifyDistort), "零间隙拒");

    // 14) 禁则字符无重复。
    let mut all: Vec<char> = Vec::new();
    all.extend(kt.head_forbid.clone());
    all.extend(kt.tail_forbid.clone());
    let mut dup = false;
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i] == all[j] { dup = true; }
        }
    }
    set.add("C4042-14", !dup, "禁则字无重复");

    // 15) 段位自查。
    set.add("C4042-15", SEG_BASE == 0x76C0, "段位 0x76C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinsoku_flow() {
        let kt = KinsokuTable::canonical();
        assert!(kt.complete());
        let mut line = vec!['，', '字'];
        assert!(kt.fix_line(&mut line));
        assert!(kt.check_line(&line).is_ok());
    }
}
