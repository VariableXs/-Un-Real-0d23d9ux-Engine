//! ves290_s13perf — VE-F4050 · T03 排版性能
//! 分项预算（分词/整形/折行/定位四分项复述制度）+ 整形缓存复用（热表单源）+ 批量排版（万段合批复述）+ 规模阶梯（1k/10k/100k 三阶）+ 基准注册 + 近线性断言。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7C40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum PCode {
    Ok = 0,
    SegmentOver = SEG_BASE + 0x01,
    CacheBroken = SEG_BASE + 0x02,
    BatchFail = SEG_BASE + 0x03,
    Superlinear = SEG_BASE + 0x04,
    NotRegistered = SEG_BASE + 0x05,
}

/// 四分项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Segment { Tokenize = 0, Shape = 1, LineBreak = 2, Position = 3 }

impl Segment {
    pub const ALL: [Segment; 4] = [Segment::Tokenize, Segment::Shape, Segment::LineBreak, Segment::Position];
    pub fn name(self) -> &'static str {
        match self { Segment::Tokenize => "分词", Segment::Shape => "整形", Segment::LineBreak => "折行", Segment::Position => "定位" }
    }
}

pub struct BudgetTable {
    pub limits: [(Segment, u32); 4],
}

impl BudgetTable {
    pub fn canonical() -> Self {
        BudgetTable { limits: [(Segment::Tokenize, 300), (Segment::Shape, 600), (Segment::LineBreak, 500), (Segment::Position, 400)] }
    }
    pub fn limit_of(&self, s: Segment) -> u32 {
        for l in self.limits.iter() {
            if l.0 == s { return l.1; }
        }
        0
    }
    pub fn retell(&self, s: Segment, measured: u32) -> (u32, u32, bool) {
        let lim = self.limit_of(s);
        (measured, lim, measured <= lim)
    }
}

/// 整形缓存热表（复用单源）。
pub struct ShapeHot {
    pub entries: Vec<(u64, u32)>,
}

impl ShapeHot {
    pub fn new() -> Self { ShapeHot { entries: Vec::new() } }
    pub fn put(&mut self, fp: u64, cost: u32) -> Result<(), PCode> {
        for e in self.entries.iter() {
            if e.0 == fp { return Err(PCode::CacheBroken); }
        }
        self.entries.push((fp, cost));
        Ok(())
    }
    pub fn hit(&self, fp: u64) -> bool {
        self.entries.iter().any(|e| e.0 == fp)
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

/// 批量合批：万段排版合批执行（一帧一批）。
pub struct BatchRunner {
    pub batch_size: u32,
    pub frames: u32,
}

impl BatchRunner {
    pub fn new(batch: u32) -> Self { BatchRunner { batch_size: batch, frames: 0 } }
    /// 合批：n 段 → ceil(n/batch) 帧。
    pub fn run(&mut self, paragraphs: u32) -> Result<u32, PCode> {
        if self.batch_size == 0 { return Err(PCode::BatchFail); }
        self.frames = (paragraphs + self.batch_size - 1) / self.batch_size;
        Ok(self.frames)
    }
}

/// 规模阶梯：1k/10k/100k 三阶基线注册。
pub struct Ladder {
    pub registered: Vec<(u32, u32)>, // (段数, 实测 ms)
}

impl Ladder {
    pub const STEPS: [u32; 3] = [1_000, 10_000, 100_000];
    pub fn new() -> Self { Ladder { registered: Vec::new() } }
    pub fn register(&mut self, paras: u32, ms: u32) -> Result<(), PCode> {
        if !Self::STEPS.contains(&paras) { return Err(PCode::NotRegistered); }
        if self.registered.iter().any(|r| r.0 == paras) { return Err(PCode::NotRegistered); }
        self.registered.push((paras, ms));
        Ok(())
    }
    /// 近线性：相邻阶梯斜率比不超 20 倍（100k 相对 1k 时限放大 ≤ 150 倍）。
    pub fn near_linear(&self) -> bool {
        if self.registered.len() < 2 { return false; }
        let mut prev = (0u32, 0u32);
        let mut first = true;
        for r in self.registered.iter() {
            if first {
                prev = *r;
                first = false;
                continue;
            }
            let slope_now = r.1 as u64 * 1000 / r.0.max(1) as u64;
            let slope_prev = prev.1 as u64 * 1000 / prev.0.max(1) as u64;
            if slope_prev == 0 { return false; }
            if slope_now > slope_prev * 20 { return false; }
            prev = *r;
        }
        true
    }
}

pub fn run_ves290_checks() -> CheckSet {
    let mut set = CheckSet::new("ves290_s13perf");
    let bt = BudgetTable::canonical();

    // 1) 四分项预算齐。
    set.add("C4050-01", bt.limit_of(Segment::Tokenize) == 300 && bt.limit_of(Segment::Shape) == 600
        && bt.limit_of(Segment::LineBreak) == 500 && bt.limit_of(Segment::Position) == 400, "四分项预算");

    // 2) 分项复述对。
    let (m, lim, ok) = bt.retell(Segment::Shape, 550);
    set.add("C4050-02", m == 550 && lim == 600 && ok, "分项复述");

    // 3) 分项超检出。
    set.add("C4050-03", bt.retell(Segment::Shape, 700).2 == false, "分项超检出");

    // 4) 热表单源。
    let mut sh = ShapeHot::new();
    let _ = sh.put(0xAA, 100);
    set.add("C4050-04", sh.hit(0xAA) && sh.put(0xAA, 90) == Err(PCode::CacheBroken) && sh.single_source(), "热表单源");

    // 5) 缓存击穿直算降级：未命中。
    set.add("C4050-05", !sh.hit(0xBB), "未命中直算");

    // 6) 万段合批。
    let mut br = BatchRunner::new(2500);
    set.add("C4050-06", br.run(10_000) == Ok(4), "万段四帧");

    // 7) 零段一帧都不跑。
    set.add("C4050-07", br.run(0) == Ok(0), "零段零帧");

    // 8) 零批拒。
    let mut br2 = BatchRunner::new(0);
    set.add("C4050-08", br2.run(100) == Err(PCode::BatchFail), "零批拒");

    // 9) 阶梯注册。
    let mut ld = Ladder::new();
    let _ = ld.register(1_000, 40);
    let _ = ld.register(10_000, 420);
    let _ = ld.register(100_000, 4600);
    set.add("C4050-09", ld.registered.len() == 3, "三阶注册");

    // 10) 非阶梯段数拒。
    set.add("C4050-10", ld.register(5_000, 100) == Err(PCode::NotRegistered), "非阶梯拒");

    // 11) 重复阶梯拒。
    set.add("C4050-11", ld.register(1_000, 50) == Err(PCode::NotRegistered), "重复阶梯拒");

    // 12) 近线性通过。
    set.add("C4050-12", ld.near_linear(), "近线性");

    // 13) 超线性检出。
    let mut ld2 = Ladder::new();
    let _ = ld2.register(1_000, 40);
    let _ = ld2.register(10_000, 40_000);
    set.add("C4050-13", !ld2.near_linear(), "超线性检出");

    // 14) 单阶不成判。
    let mut ld3 = Ladder::new();
    let _ = ld3.register(1_000, 40);
    set.add("C4050-14", !ld3.near_linear(), "单阶不成判");

    // 15) 分项名双射。
    let mut names: Vec<&str> = Vec::new();
    for s in Segment::ALL.iter() { names.push(s.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4050-15", !ndup && names.len() == 4, "分项名双射");

    // 16) 段位自查。
    set.add("C4050-16", SEG_BASE == 0x7C40, "段位 0x7C40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_and_ladder() {
        let mut br = BatchRunner::new(100);
        assert_eq!(br.run(250), Ok(3));
        let mut ld = Ladder::new();
        let _ = ld.register(1_000, 40);
        let _ = ld.register(10_000, 400);
        assert!(ld.near_linear());
    }
}
