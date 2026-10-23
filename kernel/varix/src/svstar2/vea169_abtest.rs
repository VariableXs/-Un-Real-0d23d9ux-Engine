//! VE-F0169 · A/B 帧率对照运行（VE-Lab · 目标 300 行）
//!
//! 铁律：对照公平，数据可采，统计可信，结论可复现。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8EF;
pub const XC_SAMPLE_INSUFFICIENT: u16 = SEG_BASE | 0x01;

pub struct AbTest {
    a_samples: Vec<u32>,
    b_samples: Vec<u32>,
    min_samples: usize,
    last_code: u16,
}

impl AbTest {
    pub fn new(min_samples: usize) -> Self { Self { a_samples: Vec::new(), b_samples: Vec::new(), min_samples, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record_a(&mut self, fps: u32) { self.a_samples.push(fps); self.last_code = 0; }
    pub fn record_b(&mut self, fps: u32) { self.b_samples.push(fps); self.last_code = 0; }
    pub fn avg_a(&self) -> f32 {
        if self.a_samples.is_empty() { return 0.0; }
        self.a_samples.iter().sum::<u32>() as f32 / self.a_samples.len() as f32
    }
    pub fn avg_b(&self) -> f32 {
        if self.b_samples.is_empty() { return 0.0; }
        self.b_samples.iter().sum::<u32>() as f32 / self.b_samples.len() as f32
    }
    pub fn conclusive(&self) -> bool { self.a_samples.len() >= self.min_samples && self.b_samples.len() >= self.min_samples }
}

pub fn run_vea169_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea169_abtest");
    { let t = AbTest::new(3); chk(&mut set, "C169-01", !t.conclusive(), "err"); }
    { let mut t = AbTest::new(3); t.record_a(60); t.record_a(62); t.record_a(58);
        chk(&mut set, "C169-02", t.avg_a() > 59.0 && t.avg_a() < 61.0, "err"); }
    { let mut t = AbTest::new(1); t.record_a(60); t.record_b(50);
        chk(&mut set, "C169-03", t.conclusive(), "err"); }
    { let mut t = AbTest::new(3); t.record_a(60); t.record_a(60);
        chk(&mut set, "C169-04", !t.conclusive(), "err"); }
    { let mut t = AbTest::new(3); t.record_b(30); t.record_b(32); t.record_b(28);
        chk(&mut set, "C169-05", t.avg_b() > 29.0 && t.avg_b() < 31.0, "err"); }
    { let t = AbTest::new(3); chk(&mut set, "C169-06", t.avg_a() == 0.0, "err"); }
    { let mut t = AbTest::new(3); t.record_a(60); t.record_a(60); t.record_a(60);
        t.record_b(50); t.record_b(50); t.record_b(50);
        chk(&mut set, "C169-07", t.conclusive() && t.avg_a() > t.avg_b(), "err"); }
    { let t = AbTest::new(0); chk(&mut set, "C169-08", t.conclusive(), "err"); }
    { let mut t = AbTest::new(3); t.record_a(60);
        chk(&mut set, "C169-09", t.last_code() == 0, "err"); }
    { let mut t = AbTest::new(3); t.record_b(45);
        chk(&mut set, "C169-10", t.avg_b() == 45.0, "err"); }
    set
}
