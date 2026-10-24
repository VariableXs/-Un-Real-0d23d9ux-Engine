//! ves274_s12fuzz — VE-F4034 · RTL fuzz
//! 混合方向 fuzz（BiDi 控制符风暴/镜像规则 fuzz/RTL 语料变异）+ 变异三级（结构/参数/时序）+ 四不变量分级复用 + 最小化闭环复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x6F40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum FuzzCode {
    Ok = 0,
    Misread = SEG_BASE + 0x01,
    Incomplete = SEG_BASE + 0x02,
    Hang = SEG_BASE + 0x03,
    LoopMissing = SEG_BASE + 0x04,
    QuotaOver = SEG_BASE + 0x05,
    CorpusEmpty = SEG_BASE + 0x06,
    MinimizedEmpty = SEG_BASE + 0x07,
}

/// BiDi 控制符集合（风暴源）。
pub const CTRL_CHARS: [u32; 6] = [0x202A, 0x202B, 0x202C, 0x2066, 0x2067, 0x2069];

/// 结构变异：控制符风暴（随机插入控制符）与嵌套方向畸形（超深嵌套）。
pub struct StructuralMutator {
    pub storm_count: u32,
    pub nest_depth: u32,
}

impl StructuralMutator {
    pub fn new(storm: u32, nest: u32) -> Self {
        StructuralMutator { storm_count: storm, nest_depth: nest }
    }
    /// 控制符风暴：在文本各位置循环插入控制符（确定性位置=种子偏移）。
    pub fn storm(&self, text: &[u32], seed: u64) -> Vec<u32> {
        let mut out = Vec::new();
        let n = text.len();
        for i in 0..n {
            out.push(text[i]);
            if (i as u64 + seed) % 3 == 0 {
                out.push(CTRL_CHARS[((i as u64 + seed) as usize) % CTRL_CHARS.len()]);
            }
        }
        for _ in 0..self.storm_count {
            out.push(CTRL_CHARS[0]);
        }
        out
    }
    /// 嵌套方向畸形：超深嵌套（深度超限即守恒拒绝——fuzz 输入而非崩溃）。
    pub fn nested(&self, base: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for _ in 0..self.nest_depth {
            out.push(0x2066);
        }
        out.push(base);
        for _ in 0..self.nest_depth {
            out.push(0x2069);
        }
        out
    }
}

/// 参数变异：镜像表越界索引、方向值极端。
pub fn param_mutate_mirror_idx(idx: usize, table_len: usize) -> Result<usize, FuzzCode> {
    if table_len == 0 { return Err(FuzzCode::CorpusEmpty); }
    if idx >= table_len { return Err(FuzzCode::Incomplete); }
    Ok(idx % table_len)
}

pub fn param_mutate_dir_value(v: i32) -> i32 {
    if v > 1 { 1 } else if v < -1 { -1 } else { v }
}

/// 时序变异：切换风暴×输入×排版竞态——用配额拦截。
pub struct TimingStorm {
    pub quota: u32,
    pub fired: u32,
}

impl TimingStorm {
    pub fn new(quota: u32) -> Self { TimingStorm { quota, fired: 0 } }
    pub fn fire(&mut self, n: u32) -> Result<u32, FuzzCode> {
        if self.fired + n > self.quota { return Err(FuzzCode::QuotaOver); }
        self.fired += n;
        Ok(self.fired)
    }
}

/// 四不变量。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Invariant { AlgorithmComplete = 0, MirrorComplete = 1, SemanticsPreserve = 2, CursorCorrect = 3 }

impl Invariant {
    pub const ALL: [Invariant; 4] = [
        Invariant::AlgorithmComplete, Invariant::MirrorComplete,
        Invariant::SemanticsPreserve, Invariant::CursorCorrect];
    pub fn name(self) -> &'static str {
        match self {
            Invariant::AlgorithmComplete => "算法完整",
            Invariant::MirrorComplete => "镜像完整",
            Invariant::SemanticsPreserve => "语义保持",
            Invariant::CursorCorrect => "光标正确",
        }
    }
}

/// 分级：错读=P0；其余不变量破=P1。
pub fn severity_of(inv: Invariant) -> u8 {
    match inv {
        Invariant::SemanticsPreserve => 0, // 错读 = P0
        _ => 1,
    }
}

/// 最小化：从崩溃输入中剥出最小触发子串（保留控制符骨架）。
pub fn minimize(mut text: Vec<u32>) -> Vec<u32> {
    // 简化确定性最小化：剥离成对控制符内的填充字符。
    loop {
        let mut changed = false;
        let mut i = 0;
        while i + 1 < text.len() {
            let is_ctrl = CTRL_CHARS.contains(&text[i]);
            if is_ctrl && CTRL_CHARS.contains(&text[i + 1]) && text[i] != text[i + 1] {
                text.remove(i + 1);
                changed = true;
            } else {
                i += 1;
            }
        }
        if !changed { break; }
    }
    text
}

/// 闭环：fuzz 发现 → 复述闭环单。
#[derive(Clone)]
pub struct Finding {
    pub invariant: Invariant,
    pub minimized: Vec<u32>,
    pub looped: bool,
}

pub struct FindingBook {
    pub items: Vec<Finding>,
}

impl FindingBook {
    pub fn new() -> Self { FindingBook { items: Vec::new() } }
    pub fn record(&mut self, inv: Invariant, text: Vec<u32>) -> Result<(), FuzzCode> {
        let m = minimize(text);
        if m.is_empty() { return Err(FuzzCode::MinimizedEmpty); }
        self.items.push(Finding { invariant: inv, minimized: m, looped: false });
        Ok(())
    }
    pub fn close(&mut self, i: usize) -> Result<(), FuzzCode> {
        if i >= self.items.len() { return Err(FuzzCode::LoopMissing); }
        self.items[i].looped = true;
        Ok(())
    }
    pub fn all_closed(&self) -> bool {
        self.items.iter().all(|f| f.looped)
    }
}

pub fn run_ves274_checks() -> CheckSet {
    let mut set = CheckSet::new("ves274_s12fuzz");

    // 1) 控制符风暴产出含风暴尾。
    let m1 = StructuralMutator::new(3, 0);
    let out = m1.storm(&[0x41, 0x42], 1);
    set.add("C4034-01", out.len() >= 3 && out.last().map_or(false, |c| CTRL_CHARS.contains(c)), "风暴产出");

    // 2) 确定性：同种子同产出。
    let out2 = m1.storm(&[0x41, 0x42], 1);
    set.add("C4034-02", out == out2, "风暴确定性");

    // 3) 嵌套畸形：深度对称。
    let m3 = StructuralMutator::new(0, 5);
    let nest = m3.nested(0x41);
    set.add("C4034-03", nest.len() == 11 && nest[0] == 0x2066 && nest[10] == 0x2069, "嵌套对称");

    // 4) 参数变异：越界索引归一。
    set.add("C4034-04", param_mutate_mirror_idx(7, 4) == Ok(3), "越界归一");

    // 5) 参数变异：空表拒。
    set.add("C4034-05", param_mutate_mirror_idx(0, 0) == Err(FuzzCode::CorpusEmpty), "空表拒");

    // 6) 方向值极端钳制。
    set.add("C4034-06", param_mutate_dir_value(99) == 1 && param_mutate_dir_value(-99) == -1
        && param_mutate_dir_value(0) == 0, "方向值钳制");

    // 7) 时序配额：超配额拒。
    let mut ts = TimingStorm::new(5);
    set.add("C4034-07", ts.fire(3) == Ok(3) && ts.fire(3) == Err(FuzzCode::QuotaOver), "时序配额");

    // 8) 四不变量齐。
    set.add("C4034-08", Invariant::ALL.len() == 4, "四不变量");

    // 9) 分级：语义保持（错读）= P0。
    set.add("C4034-09", severity_of(Invariant::SemanticsPreserve) == 0
        && severity_of(Invariant::MirrorComplete) == 1, "错读 P0 分级");

    // 10) 最小化：剥离相邻异对控制符。
    let mini = minimize(vec![0x202A, 0x202B, 0x41, 0x202C]);
    set.add("C4034-10", mini.len() <= 3, "最小化剥离");

    // 11) 最小化结果非空守恒。
    set.add("C4034-11", !minimize(vec![0x41, 0x42]).is_empty(), "最小化守恒");

    // 12) 发现入库带最小化。
    let mut fb = FindingBook::new();
    set.add("C4034-12", fb.record(Invariant::SemanticsPreserve, vec![0x202A, 0x41, 0x202B]).is_ok()
        && !fb.items[0].minimized.is_empty(), "发现入库");

    // 13) 最小化空拒。
    set.add("C4034-13", fb.record(Invariant::MirrorComplete, vec![]) == Err(FuzzCode::MinimizedEmpty), "空最小化拒");

    // 14) 闭环复述：关闭后 all_closed。
    let _ = fb.close(0);
    set.add("C4034-14", fb.all_closed(), "闭环复述");

    // 15) 闭环缺检出。
    let mut fb2 = FindingBook::new();
    let _ = fb2.record(Invariant::CursorCorrect, vec![0x41]);
    set.add("C4034-15", !fb2.all_closed() && fb2.close(5) == Err(FuzzCode::LoopMissing), "闭环缺检出");

    // 16) 控制符集合非空且唯一。
    let mut cdup = false;
    for i in 0..CTRL_CHARS.len() {
        for j in (i + 1)..CTRL_CHARS.len() {
            if CTRL_CHARS[i] == CTRL_CHARS[j] { cdup = true; }
        }
    }
    set.add("C4034-16", !cdup && CTRL_CHARS.len() == 6, "控制符集唯一");

    // 17) 不变量名双射。
    let mut names: Vec<&str> = Vec::new();
    for v in Invariant::ALL.iter() { names.push(v.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4034-17", !ndup, "不变量名双射");

    // 18) 段位自查。
    set.add("C4034-18", SEG_BASE == 0x6F40, "段位 0x6F40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_and_quota() {
        let m = StructuralMutator::new(2, 0);
        let a = m.storm(&[1, 2, 3], 7);
        let b = m.storm(&[1, 2, 3], 7);
        assert_eq!(a, b);
        let mut ts = TimingStorm::new(2);
        assert!(ts.fire(2).is_ok());
        assert_eq!(ts.fire(1), Err(FuzzCode::QuotaOver));
    }
}
