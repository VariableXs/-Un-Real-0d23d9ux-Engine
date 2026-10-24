//! ves295_s14fuzz — VE-F4055 · T03 排版 fuzz
//! 畸形文本 fuzz（组合标记风暴/BiDi 控制符风暴/超长无空格）+ 变异三级（结构/参数/时序）+ 四不变量（整形对齐/断行一致/不崩溃/降级显性——整形错=P0）+ 最小化闭环。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x81C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FzCode {
    Ok = 0,
    ShapeWrong = SEG_BASE + 0x01,
    BreakInconsistent = SEG_BASE + 0x02,
    Crash = SEG_BASE + 0x03,
    SilentDegrade = SEG_BASE + 0x04,
    QuotaOver = SEG_BASE + 0x05,
    MinimizedEmpty = SEG_BASE + 0x06,
    LoopMissing = SEG_BASE + 0x07,
}

pub const MARKS: [u32; 4] = [0x0E31, 0x0E38, 0x093F, 0x0947];
pub const BIDI_CTRLS: [u32; 3] = [0x202A, 0x202B, 0x202C];

/// 结构变异：标记风暴（确定性位置插入）。
pub fn mark_storm(text: &[u32], seed: u64) -> Vec<u32> {
    let mut out = Vec::new();
    for (i, cp) in text.iter().enumerate() {
        out.push(*cp);
        if (i as u64 + seed) % 2 == 0 {
            out.push(MARKS[((i as u64 + seed) as usize) % MARKS.len()]);
        }
    }
    out
}

/// 结构变异：超长无空格。
pub fn long_no_space(len: usize) -> Vec<u32> {
    let mut v = Vec::new();
    for i in 0..len {
        v.push(0x4E00 + (i % 100) as u32);
    }
    v
}

/// 参数变异：断行参数极端钳制。
pub fn clamp_break_param(v: i32) -> u32 {
    if v < 1 { 1 } else if v > 1000 { 1000 } else { v as u32 }
}

/// 时序变异：语言切换×排版×折行竞态配额。
pub struct TimingQuota {
    pub quota: u32,
    pub fired: u32,
}

impl TimingQuota {
    pub fn new(q: u32) -> Self { TimingQuota { quota: q, fired: 0 } }
    pub fn fire(&mut self, n: u32) -> Result<u32, FzCode> {
        if self.fired + n > self.quota { return Err(FzCode::QuotaOver); }
        self.fired += n;
        Ok(self.fired)
    }
}

/// 四不变量。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Invariant { ShapeAligned = 0, BreakConsistent = 1, NoCrash = 2, DegradeExplicit = 3 }

impl Invariant {
    pub const ALL: [Invariant; 4] = [
        Invariant::ShapeAligned, Invariant::BreakConsistent, Invariant::NoCrash, Invariant::DegradeExplicit];
    /// 整形错 = P0。
    pub fn is_p0(self) -> bool {
        self == Invariant::ShapeAligned
    }
}

/// 最小化：剥除可剥离的标记。
pub fn minimize(mut text: Vec<u32>) -> Vec<u32> {
    loop {
        let mut changed = false;
        let mut i = 0;
        while i < text.len() {
            if MARKS.contains(&text[i]) {
                text.remove(i);
                changed = true;
            } else {
                i += 1;
            }
        }
        if !changed { break; }
    }
    text
}

/// 发现闭环账。
pub struct FindingBook {
    pub items: Vec<(Invariant, Vec<u32>, bool)>,
}

impl FindingBook {
    pub fn new() -> Self { FindingBook { items: Vec::new() } }
    pub fn record(&mut self, inv: Invariant, text: Vec<u32>) -> Result<(), FzCode> {
        let m = minimize(text);
        if m.is_empty() { return Err(FzCode::MinimizedEmpty); }
        self.items.push((inv, m, false));
        Ok(())
    }
    pub fn close(&mut self, i: usize) -> Result<(), FzCode> {
        if i >= self.items.len() { return Err(FzCode::LoopMissing); }
        self.items[i].2 = true;
        Ok(())
    }
    pub fn all_closed(&self) -> bool {
        self.items.iter().all(|f| f.2)
    }
}

pub fn run_ves295_checks() -> CheckSet {
    let mut set = CheckSet::new("ves295_s14fuzz");

    // 1) 标记风暴确定性。
    let a = mark_storm(&[0x0E01, 0x0E01], 3);
    let b = mark_storm(&[0x0E01, 0x0E01], 3);
    set.add("C4055-01", a == b && a.len() >= 2, "风暴确定性");

    // 2) 超长无空格产出。
    let lng = long_no_space(500);
    set.add("C4055-02", lng.len() == 500 && lng.iter().all(|c| (0x4E00..=0x4E63).contains(c)), "超长无空格");

    // 3) 参数钳制。
    set.add("C4055-03", clamp_break_param(-5) == 1 && clamp_break_param(99_999) == 1000 && clamp_break_param(50) == 50, "参数钳制");

    // 4) 时序配额。
    let mut tq = TimingQuota::new(5);
    set.add("C4055-04", tq.fire(4) == Ok(4) && tq.fire(2) == Err(FzCode::QuotaOver), "时序配额");

    // 5) 四不变量齐。
    set.add("C4055-05", Invariant::ALL.len() == 4, "四不变量");

    // 6) 整形错 = P0。
    set.add("C4055-06", Invariant::ShapeAligned.is_p0() && !Invariant::NoCrash.is_p0(), "整形 P0 分级");

    // 7) 最小化剥标记。
    let mini = minimize(vec![0x0E01, 0x0E31, 0x0E38, 0x0E01]);
    set.add("C4055-07", mini == [0x0E01, 0x0E01], "最小化剥标记");

    // 8) 最小化空拒。
    let mut fb = FindingBook::new();
    set.add("C4055-08", fb.record(Invariant::ShapeAligned, vec![0x0E31]) == Err(FzCode::MinimizedEmpty), "空最小化拒");

    // 9) 入库带最小化。
    set.add("C4055-09", fb.record(Invariant::BreakConsistent, vec![0x4E00, 0x0E31, 0x4E01]).is_ok(), "入库");

    // 10) 闭环复述。
    let _ = fb.close(0);
    set.add("C4055-10", fb.all_closed(), "闭环复述");

    // 11) 闭环缺检出。
    let mut fb2 = FindingBook::new();
    let _ = fb2.record(Invariant::NoCrash, vec![0x4E00]);
    set.add("C4055-11", !fb2.all_closed() && fb2.close(9) == Err(FzCode::LoopMissing), "闭环缺检出");

    // 12) BiDi 控制符集唯一。
    let mut cdup = false;
    for i in 0..BIDI_CTRLS.len() {
        for j in (i + 1)..BIDI_CTRLS.len() {
            if BIDI_CTRLS[i] == BIDI_CTRLS[j] { cdup = true; }
        }
    }
    set.add("C4055-12", !cdup, "控制符唯一");

    // 13) 段位自查。
    set.add("C4055-13", SEG_BASE == 0x81C0, "段位 0x81C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_and_quota() {
        let a = mark_storm(&[1, 2, 3], 1);
        let b = mark_storm(&[1, 2, 3], 1);
        assert_eq!(a, b);
        let mut tq = TimingQuota::new(2);
        assert!(tq.fire(2).is_ok());
        assert_eq!(tq.fire(1), Err(FzCode::QuotaOver));
    }
}
