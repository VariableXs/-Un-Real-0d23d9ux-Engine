//! VE-F0167 · 瓶颈自动分析（VE-A 域 · A07 · 目标 320 行）
//!
//! 铁律：瓶颈可定，归因可查，建议可给，优化可追。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8ED;
pub const XC_NO_BOTTLENECK: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bottleneck {
    Vertex,
    Fragment,
    Memory,
    Bandwidth,
    Unknown,
}

pub struct BottleneckAnalyzer {
    vertex_us: u32,
    fragment_us: u32,
    memory_us: u32,
    bandwidth_us: u32,
    analyses: u32,
    last_code: u16,
}

impl BottleneckAnalyzer {
    pub fn new() -> Self { Self { vertex_us: 0, fragment_us: 0, memory_us: 0, bandwidth_us: 0, analyses: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn analyze(&mut self, vertex_us: u32, fragment_us: u32, memory_us: u32, bandwidth_us: u32) -> Bottleneck {
        self.vertex_us = vertex_us;
        self.fragment_us = fragment_us;
        self.memory_us = memory_us;
        self.bandwidth_us = bandwidth_us;
        self.analyses += 1;
        self.last_code = 0;
        let max_us = vertex_us.max(fragment_us).max(memory_us).max(bandwidth_us);
        if max_us == 0 { self.last_code = XC_NO_BOTTLENECK; return Bottleneck::Unknown; }
        if max_us == vertex_us { Bottleneck::Vertex }
        else if max_us == fragment_us { Bottleneck::Fragment }
        else if max_us == memory_us { Bottleneck::Memory }
        else { Bottleneck::Bandwidth }
    }
    pub fn analyses(&self) -> u32 { self.analyses }
}

pub fn run_vea167_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea167_bottleneck");
    { let b = BottleneckAnalyzer::new(); chk(&mut set, "C167-01", b.analyses() == 0, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(100, 50, 30, 20);
        chk(&mut set, "C167-02", r == Bottleneck::Vertex, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(50, 100, 30, 20);
        chk(&mut set, "C167-03", r == Bottleneck::Fragment, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(50, 30, 100, 20);
        chk(&mut set, "C167-04", r == Bottleneck::Memory, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(50, 30, 20, 100);
        chk(&mut set, "C167-05", r == Bottleneck::Bandwidth, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(0, 0, 0, 0);
        chk(&mut set, "C167-06", r == Bottleneck::Unknown, "err"); }
    { let mut b = BottleneckAnalyzer::new(); b.analyze(1, 2, 3, 4); b.analyze(4, 3, 2, 1);
        chk(&mut set, "C167-07", b.analyses() == 2, "err"); }
    { let mut b = BottleneckAnalyzer::new(); b.analyze(100, 50, 30, 20);
        chk(&mut set, "C167-08", b.last_code() == 0, "err"); }
    { let mut b = BottleneckAnalyzer::new(); b.analyze(0, 0, 0, 0);
        chk(&mut set, "C167-09", b.last_code() == XC_NO_BOTTLENECK, "err"); }
    { let mut b = BottleneckAnalyzer::new(); let r = b.analyze(50, 50, 50, 50);
        chk(&mut set, "C167-10", r == Bottleneck::Vertex, "err"); }
    set
}
