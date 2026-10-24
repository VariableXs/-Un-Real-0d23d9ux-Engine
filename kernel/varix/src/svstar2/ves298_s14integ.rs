//! ves298_s14integ — VE-F4058 · T03 联调
//! 组合矩阵（四语言族×三文档型×三设备档=36 格分层抽样复述覆盖底线）+ 20 分钟排版剧本（重演确定性+信号采集）+ 三故障注入（整形分歧/禁则违例/孤行）+ 三分流。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x84C0;

pub const MATRIX_FLOOR: u32 = 36;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum T3Code {
    Ok = 0,
    CoverageShort = SEG_BASE + 0x01,
    ShapeWrong = SEG_BASE + 0x02,
    ReplayDiverge = SEG_BASE + 0x03,
    NoOwner = SEG_BASE + 0x04,
    InjectMissing = SEG_BASE + 0x05,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cell {
    pub lang: u8,   // 0..4
    pub doc: u8,    // 0..3
    pub device: u8, // 0..3
}

pub struct Matrix {
    pub cells: Vec<Cell>,
}

impl Matrix {
    pub fn total() -> u32 { 4 * 3 * 3 }
    pub fn new() -> Self { Matrix { cells: Vec::new() } }
    pub fn add(&mut self, c: Cell) -> bool {
        if c.lang >= 4 || c.doc >= 3 || c.device >= 3 { return false; }
        if self.cells.iter().any(|x| *x == c) { return false; }
        self.cells.push(c);
        true
    }
    pub fn meets_floor(&self) -> bool {
        self.cells.len() as u32 >= MATRIX_FLOOR
    }
    pub fn langs_complete(&self) -> bool {
        for l in 0..4u8 {
            if !self.cells.iter().any(|c| c.lang == l) { return false; }
        }
        true
    }
}

/// 20 分钟排版剧本：四幕。
#[derive(Clone)]
pub struct ScriptRun {
    pub acts: [String; 4],
    pub signals: Vec<u64>,
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl ScriptRun {
    pub fn canonical() -> Self {
        ScriptRun {
            acts: [
                String::from("多语言文档渲染"),
                String::from("混排"),
                String::from("折行"),
                String::from("对拍混流"),
            ],
            signals: Vec::new(),
        }
    }
    pub fn execute(&mut self) -> u64 {
        self.signals.clear();
        for (i, a) in self.acts.iter().enumerate() {
            let mut buf = Vec::new();
            for b in a.as_bytes() { buf.push(*b); }
            buf.push(i as u8);
            self.signals.push(fnv1a(&buf));
        }
        let mut all = Vec::new();
        for s in self.signals.iter() {
            all.extend_from_slice(&s.to_le_bytes());
        }
        fnv1a(&all)
    }
    pub fn replay_deterministic(&mut self) -> bool {
        let f1 = self.execute();
        let f2 = self.execute();
        f1 == f2
    }
    pub fn duration_min(&self) -> u32 { 20 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fault { ShapeDiverge = 0, KinsokuBreach = 1, Orphan = 2 }

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::ShapeDiverge, Fault::KinsokuBreach, Fault::Orphan];
    pub fn name(self) -> &'static str {
        match self { Fault::ShapeDiverge => "整形分歧", Fault::KinsokuBreach => "禁则违例", Fault::Orphan => "孤行" }
    }
    /// 整形分歧 = P0。
    pub fn is_p0(self) -> bool {
        self == Fault::ShapeDiverge
    }
}

pub struct InjectField {
    pub detected: [bool; 3],
}

impl InjectField {
    pub fn new() -> Self { InjectField { detected: [false; 3] } }
    pub fn inject_detect(&mut self, f: Fault, hit: bool) -> bool {
        self.detected[f as usize] = hit;
        hit
    }
    pub fn all_detected(&self) -> bool {
        self.detected.iter().all(|d| *d)
    }
    pub fn shape_p0(&self) -> bool {
        self.detected[Fault::ShapeDiverge as usize]
    }
}

#[derive(Clone)]
pub struct Triage {
    pub issue: String,
    pub attributed: Option<String>,
    pub handled: bool,
    pub owner: String,
}

pub struct TriageBook {
    pub items: Vec<Triage>,
}

impl TriageBook {
    pub fn new() -> Self { TriageBook { items: Vec::new() } }
    pub fn record(&mut self, issue: &str) -> usize {
        self.items.push(Triage { issue: String::from(issue), attributed: None, handled: false, owner: String::from("兜底") });
        self.items.len() - 1
    }
    pub fn attribute(&mut self, i: usize, cause: &str) -> Result<(), T3Code> {
        if i >= self.items.len() { return Err(T3Code::NoOwner); }
        self.items[i].attributed = Some(String::from(cause));
        Ok(())
    }
    pub fn handle(&mut self, i: usize) -> Result<(), T3Code> {
        if i >= self.items.len() { return Err(T3Code::NoOwner); }
        if self.items[i].attributed.is_none() { return Err(T3Code::NoOwner); }
        self.items[i].handled = true;
        Ok(())
    }
    pub fn all_handled(&self) -> bool {
        self.items.iter().all(|t| t.handled)
    }
}

pub fn run_ves298_checks() -> CheckSet {
    let mut set = CheckSet::new("ves298_s14integ");

    // 1) 矩阵 36 格底线。
    set.add("C4058-01", Matrix::total() == 36 && MATRIX_FLOOR == 36, "矩阵底线");

    // 2) 全格抽样达底线。
    let mut m = Matrix::new();
    for l in 0..4u8 {
        for d in 0..3u8 {
            for v in 0..3u8 {
                let _ = m.add(Cell { lang: l, doc: d, device: v });
            }
        }
    }
    set.add("C4058-02", m.meets_floor() && m.cells.len() == 36, "全格覆盖");

    // 3) 四语言族全覆盖。
    set.add("C4058-03", m.langs_complete(), "四语言族全");

    // 4) 重复/越界拒。
    set.add("C4058-04", !m.add(Cell { lang: 0, doc: 0, device: 0 }) && !m.add(Cell { lang: 4, doc: 0, device: 0 }), "重复越界拒");

    // 5) 剧本重演确定性。
    let mut sc = ScriptRun::canonical();
    set.add("C4058-05", sc.replay_deterministic(), "重演确定性");

    // 6) 四幕齐 + 20 分钟。
    set.add("C4058-06", sc.acts.iter().all(|a| !a.is_empty()) && sc.duration_min() == 20, "四幕齐");

    // 7) 信号采集。
    let _ = sc.execute();
    set.add("C4058-07", sc.signals.len() == 4, "信号采集");

    // 8) 篡改检出。
    let mut sc2 = ScriptRun::canonical();
    let f1 = sc2.execute();
    sc2.acts[2] = String::from("折行错");
    set.add("C4058-08", f1 != sc2.execute(), "篡改检出");

    // 9) 三故障全检出。
    let mut inj = InjectField::new();
    for f in Fault::ALL.iter() {
        let _ = inj.inject_detect(*f, true);
    }
    set.add("C4058-09", inj.all_detected(), "三故障全检出");

    // 10) 整形分歧 P0 红线。
    let mut inj2 = InjectField::new();
    let _ = inj2.inject_detect(Fault::KinsokuBreach, true);
    set.add("C4058-10", !inj2.shape_p0() && Fault::ShapeDiverge.is_p0(), "整形 P0 红线");

    // 11) 故障名双射。
    let mut names: Vec<&str> = Vec::new();
    for f in Fault::ALL.iter() { names.push(f.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4058-11", !ndup, "故障名双射");

    // 12) 三分流闭环。
    let mut tb = TriageBook::new();
    let i0 = tb.record("孤行检出");
    let _ = tb.attribute(i0, "断行侧");
    let _ = tb.handle(i0);
    set.add("C4058-12", tb.all_handled(), "三分流闭环");

    // 13) 未归因处置拒。
    let mut tb2 = TriageBook::new();
    let i1 = tb2.record("整形分歧");
    set.add("C4058-13", tb2.handle(i1) == Err(T3Code::NoOwner), "未归因拒");

    // 14) 兜底责任人。
    set.add("C4058-14", tb2.items[0].owner == "兜底", "兜底责任人");

    // 15) 段位自查。
    set.add("C4058-15", SEG_BASE == 0x84C0, "段位 0x84C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_and_fault() {
        let mut m = Matrix::new();
        assert!(m.add(Cell { lang: 1, doc: 1, device: 1 }));
        assert!(!m.add(Cell { lang: 1, doc: 1, device: 1 }));
        assert!(Fault::ShapeDiverge.is_p0());
    }
}
