//! VE-F0156 · 渲染管线可视化器（VE-A 域 · A07 调试与可视化组 · 目标 340 行）
//!
//! 铁律：阶段可视，时间线准确，开销声明，数据驱动。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E2;
pub const XC_STAGE_GAP: u16 = SEG_BASE | 0x01;
pub const XC_TIMELINE_CORRUPT: u16 = SEG_BASE | 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageTiming {
    pub stage: u8,
    pub duration_us: u32,
}

pub struct PipelineViz {
    stages: Vec<StageTiming>,
    capacity: usize,
    total_us: u64,
    last_code: u16,
}

impl PipelineViz {
    pub fn new(capacity: usize) -> Self { Self { stages: Vec::new(), capacity, total_us: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record_stage(&mut self, stage: u8, duration_us: u32) -> Result<(), u16> {
        if self.stages.len() >= self.capacity { self.last_code = XC_STAGE_GAP; return Err(XC_STAGE_GAP); }
        self.stages.push(StageTiming { stage, duration_us });
        self.total_us += duration_us as u64;
        self.last_code = 0;
        Ok(())
    }
    pub fn total_us(&self) -> u64 { self.total_us }
    pub fn stage_count(&self) -> usize { self.stages.len() }
    pub fn bottleneck(&self) -> Option<u8> { self.stages.iter().max_by_key(|s| s.duration_us).map(|s| s.stage) }
}

pub fn run_vea156_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea156_pipelineviz");
    { let v = PipelineViz::new(10); chk(&mut set, "C156-01", v.stage_count() == 0, "err"); }
    { let mut v = PipelineViz::new(10); let r = v.record_stage(1, 100);
        chk(&mut set, "C156-02", r.is_ok() && v.stage_count() == 1, "err"); }
    { let mut v = PipelineViz::new(1); v.record_stage(1, 100); let r = v.record_stage(2, 200);
        chk(&mut set, "C156-03", r.err() == Some(XC_STAGE_GAP), "err"); }
    { let mut v = PipelineViz::new(10); v.record_stage(1, 100); v.record_stage(2, 300);
        chk(&mut set, "C156-04", v.total_us() == 400, "err"); }
    { let mut v = PipelineViz::new(10); v.record_stage(1, 100); v.record_stage(2, 300);
        chk(&mut set, "C156-05", v.bottleneck() == Some(2), "err"); }
    { let v = PipelineViz::new(10); chk(&mut set, "C156-06", v.bottleneck().is_none(), "err"); }
    { let mut v = PipelineViz::new(10); for i in 0..5 { v.record_stage(i, 50); }
        chk(&mut set, "C156-07", v.stage_count() == 5, "err"); }
    { let v = PipelineViz::new(0); chk(&mut set, "C156-08", v.total_us() == 0, "err"); }
    { let mut v = PipelineViz::new(10); v.record_stage(1, 100);
        chk(&mut set, "C156-09", v.last_code() == 0, "err"); }
    { let mut v = PipelineViz::new(10); v.record_stage(1, 0);
        chk(&mut set, "C156-10", v.total_us() == 0, "err"); }
    set
}
