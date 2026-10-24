//! ves288_s13n02 — VE-F4048 · T03 排版引擎与 N02 深化
//! N02 终版协同：排版断言双方一致对拍 + T03 策略×N02 执行×参考引擎三方对拍 + 零分歧破→P0 + 度量单源复用。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7AC0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum N02Code {
    Ok = 0,
    ZeroSplit = SEG_BASE + 0x01,
    PairDisagree = SEG_BASE + 0x02,
    ExecMissing = SEG_BASE + 0x03,
    PolicyMissing = SEG_BASE + 0x04,
    ArbitrationTie = SEG_BASE + 0x05,
}

/// 度量单源：字形宽度表唯一真源（复用 F2908）。
pub struct MetricSource {
    pub widths: Vec<(u32, u32)>, // (码位, 宽度 1/1000 em)
}

impl MetricSource {
    pub fn canonical() -> Self {
        MetricSource {
            widths: vec![(0x0041, 700), (0x0061, 550), (0x4E00, 1000)],
        }
    }
    pub fn width_of(&self, cp: u32) -> Option<u32> {
        for e in self.widths.iter() {
            if e.0 == cp { return Some(e.1); }
        }
        None
    }
    /// 度量零分歧：N02 侧度量与单源逐值一致。
    pub fn zero_divergence(&self, n02: &[(u32, u32)]) -> bool {
        n02.len() == self.widths.len()
            && self.widths.iter().all(|e| n02.iter().any(|n| n.0 == e.0 && n.1 == e.1))
    }
}

/// 排版断言双方对拍：T 策略期望 vs N 执行实测。
#[derive(Clone)]
pub struct PairClaim {
    pub line_id: u32,
    pub t_expect: u32,
    pub n_actual: u32,
}

pub struct PairBook {
    pub claims: Vec<PairClaim>,
}

impl PairBook {
    pub fn new() -> Self { PairBook { claims: Vec::new() } }
    pub fn add(&mut self, c: PairClaim) { self.claims.push(c); }
    pub fn judge(&self) -> (bool, Vec<u32>) {
        let mut bad = Vec::new();
        for c in self.claims.iter() {
            if c.t_expect != c.n_actual { bad.push(c.line_id); }
        }
        (bad.is_empty(), bad)
    }
}

/// 三方对拍：T 策略 × N 执行 × 参考引擎。
#[derive(Clone)]
pub struct TripleClaim {
    pub line_id: u32,
    pub t_policy: u32,
    pub n_exec: u32,
    pub reference: u32,
}

impl TripleClaim {
    pub fn agree(&self) -> bool {
        self.t_policy == self.n_exec && self.n_exec == self.reference
    }
}

pub struct TripleBook {
    pub claims: Vec<TripleClaim>,
}

impl TripleBook {
    pub fn new() -> Self { TripleBook { claims: Vec::new() } }
    pub fn add(&mut self, c: TripleClaim) { self.claims.push(c); }
    pub fn judge(&self) -> (bool, Vec<u32>) {
        let mut bad = Vec::new();
        for c in self.claims.iter() {
            if !c.agree() { bad.push(c.line_id); }
        }
        (bad.is_empty(), bad)
    }
    /// 仲裁：三方分歧按多数归责（两方一致者胜；三方各异 = 平局拒绝仲裁）。
    pub fn arbitrate(&self, id: u32) -> Result<u32, N02Code> {
        for c in self.claims.iter() {
            if c.line_id == id {
                if c.t_policy == c.n_exec { return Ok(c.t_policy); }
                if c.n_exec == c.reference { return Ok(c.n_exec); }
                if c.t_policy == c.reference { return Ok(c.t_policy); }
                return Err(N02Code::ArbitrationTie);
            }
        }
        Err(N02Code::PolicyMissing)
    }
}

pub fn run_ves288_checks() -> CheckSet {
    let mut set = CheckSet::new("ves288_s13n02");
    let ms = MetricSource::canonical();

    // 1) 度量查表。
    set.add("C4048-01", ms.width_of(0x0041) == Some(700) && ms.width_of(0x4E00) == Some(1000), "度量查表");

    // 2) 度量零分歧。
    let n02 = ms.widths.clone();
    set.add("C4048-02", ms.zero_divergence(&n02), "度量零分歧");

    // 3) 零分歧破 = P0 红线。
    let mut bad_n02 = n02.clone();
    bad_n02[0].1 = 800;
    set.add("C4048-03", !ms.zero_divergence(&bad_n02), "零分歧破红线");

    // 4) 双方对拍全绿。
    let mut pb = PairBook::new();
    pb.add(PairClaim { line_id: 1, t_expect: 700, n_actual: 700 });
    pb.add(PairClaim { line_id: 2, t_expect: 550, n_actual: 550 });
    set.add("C4048-04", pb.judge() == (true, Vec::new()), "双方对拍绿");

    // 5) 对拍分歧检出。
    pb.add(PairClaim { line_id: 3, t_expect: 1000, n_actual: 900 });
    let (ok, bad) = pb.judge();
    set.add("C4048-05", !ok && bad == [3], "对拍分歧检出");

    // 6) 三方对拍全绿。
    let mut tb = TripleBook::new();
    tb.add(TripleClaim { line_id: 1, t_policy: 700, n_exec: 700, reference: 700 });
    set.add("C4048-06", tb.judge() == (true, Vec::new()), "三方对拍绿");

    // 7) 三方分歧检出。
    tb.add(TripleClaim { line_id: 2, t_policy: 550, n_exec: 560, reference: 550 });
    let (ok2, bad2) = tb.judge();
    set.add("C4048-07", !ok2 && bad2 == [2], "三方分歧检出");

    // 8) 仲裁：两方一致者胜。
    set.add("C4048-08", tb.arbitrate(2) == Ok(550), "仲裁多数");

    // 9) 仲裁：三方各异平局拒。
    tb.add(TripleClaim { line_id: 3, t_policy: 1, n_exec: 2, reference: 3 });
    set.add("C4048-09", tb.arbitrate(3) == Err(N02Code::ArbitrationTie), "平局拒");

    // 10) 仲裁：查无行拒。
    set.add("C4048-10", tb.arbitrate(99) == Err(N02Code::PolicyMissing), "查无行拒");

    // 11) 度量表条目唯一。
    let mut dup = false;
    for i in 0..ms.widths.len() {
        for j in (i + 1)..ms.widths.len() {
            if ms.widths[i].0 == ms.widths[j].0 { dup = true; }
        }
    }
    set.add("C4048-11", !dup, "度量键唯一");

    // 12) 段位自查。
    set.add("C4048-12", SEG_BASE == 0x7AC0, "段位 0x7AC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_divergence() {
        let ms = MetricSource::canonical();
        assert!(ms.zero_divergence(&ms.widths.clone()));
        let mut bad = ms.widths.clone();
        bad[1].1 = 1;
        assert!(!ms.zero_divergence(&bad));
    }
}
