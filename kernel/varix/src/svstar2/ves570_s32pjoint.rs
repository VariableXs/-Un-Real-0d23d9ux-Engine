//! VE-F4336 · 一致性性能联调（U07 性能与预算组）
//! 职责：性能组内与跨组联调——组内链十三段贯通（架构→成本→预算→基准→
//! 门→规模化→内存→功耗→CGPU→降级→低端→长稳→调试器）+ 跨组对接（上承
//! U06 场景性能、下启 U08 测试组基准复用）+ 十例全链性能画像 + 逐段探针
//! + 画像劣化阻断收口 + 读屏响应时延（域本色）。
//! 段位：0x37E0（续表 idx570）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x37E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PjCode {
    /// 越站（顺序违规）。
    StageSkip = SEG_BASE,
    /// 契约失配（跨组版本不符）。
    Mismatch = SEG_BASE + 0x01,
    /// 画像劣化（阻断收口）。
    ProfileBad = SEG_BASE + 0x02,
    /// 读屏时延超阈（域色硬线）。
    SrLatency = SEG_BASE + 0x03,
}

/// 十三段链。
pub const STAGES: usize = 13;

/// 联调台。
#[derive(Default)]
pub struct PerfJoint {
    pub stage_pass: [bool; STAGES],
    /// 逐段探针：(段, 命中)。
    pub probes: Vec<(usize, bool)>,
    /// 十例画像：(例号, 通过, 综合耗时 ms)。
    pub cases: [(bool, bool, u32); 10],
    pub up_ver: u32,
    pub down_ver: u32,
    /// 读屏响应时延（ms——域色硬线 ≤50）。
    pub sr_latency_ms: u32,
    /// 画像劣化阈（均值 ms）。
    pub profile_budget: u32,
    pub blocks: u32,
}

impl PerfJoint {
    pub fn new(up: u32, down: u32, sr_ms: u32, budget: u32) -> Self {
        PerfJoint { up_ver: up, down_ver: down, sr_latency_ms: sr_ms, profile_budget: budget, ..Default::default() }
    }

    /// 顺序过段。
    pub fn pass_stage(&mut self, stage: usize) -> Result<(), PjCode> {
        if stage >= STAGES {
            return Err(PjCode::StageSkip);
        }
        if stage > 0 && !self.stage_pass[stage - 1] {
            return Err(PjCode::StageSkip);
        }
        self.stage_pass[stage] = true;
        Ok(())
    }

    /// 逐段探针。
    pub fn probe(&mut self, stage: usize, hit: bool) {
        self.probes.push((stage, hit));
    }

    /// 断点归因：最后未命中探针段。
    pub fn fault_stage(&self) -> Option<usize> {
        self.probes.iter().rev().find(|p| !p.1).map(|p| p.0)
    }

    /// 跨组契约对拍。
    pub fn contract_ok(&self, up: u32, down: u32) -> Result<(), PjCode> {
        if self.up_ver != up || self.down_ver != down {
            return Err(PjCode::Mismatch);
        }
        Ok(())
    }

    /// 十例执行：画像耗时入账 + 读屏时延硬线核验。
    pub fn run_case(&mut self, idx: usize, ok: bool, cost_ms: u32) -> Result<(), PjCode> {
        if idx >= 10 {
            return Err(PjCode::StageSkip);
        }
        if self.sr_latency_ms > 50 {
            self.blocks += 1;
            return Err(PjCode::SrLatency);
        }
        self.cases[idx] = (true, ok, cost_ms);
        Ok(())
    }

    /// 十例画像均值。
    pub fn profile_mean(&self) -> u32 {
        let done: Vec<u32> = self.cases.iter().filter(|c| c.0 && c.1).map(|c| c.2).collect();
        if done.is_empty() {
            return 0;
        }
        let sum: u64 = done.iter().map(|v| *v as u64).sum();
        (sum / done.len() as u64) as u32
    }

    /// 画像劣化核验：均值超预算 = 阻断收口（计数）。
    pub fn profile_gate(&mut self) -> Result<u32, PjCode> {
        let m = self.profile_mean();
        if m > self.profile_budget {
            self.blocks += 1;
            return Err(PjCode::ProfileBad);
        }
        Ok(m)
    }

    /// 收口就绪：十三段 + 十例全过 + 契约 + 读屏时延达标 + 画像达标。
    pub fn ready(&self, up: u32, down: u32) -> bool {
        self.stage_pass.iter().all(|p| *p)
            && self.cases.iter().all(|c| c.0 && c.1)
            && self.contract_ok(up, down).is_ok()
            && self.sr_latency_ms <= 50
            && self.profile_mean() <= self.profile_budget
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves570_checks() -> CheckSet {
    let mut set = CheckSet::new("ves570");
    let mut pj = PerfJoint::new(20, 30, 40, 200);

    // C570-01 十三段顺序贯通。
    let mut ok = true;
    for s in 0..STAGES {
        if pj.pass_stage(s).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C570-01", ok && pj.stage_pass.iter().all(|p| *p), "十三段贯通");

    // C570-02 越站拦截。
    let mut pj2 = PerfJoint::new(20, 30, 40, 200);
    chk(&mut set, "C570-02", pj2.pass_stage(5) == Err(PjCode::StageSkip), "越站拦截");

    // C570-03 探针断点归因。
    let _ = pj2.pass_stage(0);
    pj2.probe(0, true);
    pj2.probe(1, false);
    chk(&mut set, "C570-03", pj2.fault_stage() == Some(1), "断点归因");

    // C570-04 契约对拍一致 / 失配检出。
    chk(&mut set, "C570-04", pj.contract_ok(20, 30).is_ok() && pj.contract_ok(21, 30).is_err(), "契约对拍");

    // C570-05 读屏时延达标 + 超阈硬线（域色）。
    chk(&mut set, "C570-05", pj.run_case(0, true, 150).is_ok(), "读屏达标记过");
    pj.sr_latency_ms = 80;
    chk(&mut set, "C570-05b", pj.run_case(1, true, 100) == Err(PjCode::SrLatency) && pj.blocks == 1, "读屏超阈阻断");

    // C570-06 恢复达标后十例全过。
    pj.sr_latency_ms = 30;
    let mut all = true;
    for i in 1..10 {
        if pj.run_case(i, true, 100).is_err() {
            all = false;
        }
    }
    chk(&mut set, "C570-06", all && pj.cases.iter().all(|c| c.0 && c.1), "十例全过");

    // C570-07 画像均值核算。
    chk(&mut set, "C570-07", pj.profile_mean() == 100, "画像均值 100");

    // C570-08 画像达标放行 / 劣化阻断。
    chk(&mut set, "C570-08", pj.profile_gate() == Ok(100), "画像达标");
    pj.profile_budget = 50;
    chk(&mut set, "C570-08b", pj.profile_gate() == Err(PjCode::ProfileBad) && pj.blocks == 2, "画像劣化阻断");

    // C570-09 例号越界拒。
    pj.profile_budget = 200;
    chk(&mut set, "C570-09", pj.run_case(10, true, 1).is_err(), "例号越界拒");

    // C570-10 空画像均值恒零（不臆测）。
    let pj3 = PerfJoint::new(1, 1, 10, 100);
    chk(&mut set, "C570-10", pj3.profile_mean() == 0, "空画像不臆测");

    // C570-11 收口就绪总闸。
    pj.profile_budget = 200;
    chk(&mut set, "C570-11", pj.ready(20, 30), "收口总闸放行");
    chk(&mut set, "C570-11b", !pj2.ready(20, 30), "未过链不放行");

    // C570-12 契约版本漂移阻断就绪。
    chk(&mut set, "C570-12", !pj.ready(21, 30), "契约漂移不放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_order() {
        let mut pj = PerfJoint::new(1, 1, 10, 100);
        assert_eq!(pj.pass_stage(3), Err(PjCode::StageSkip));
        for s in 0..STAGES {
            assert!(pj.pass_stage(s).is_ok());
        }
        assert_eq!(pj.pass_stage(STAGES), Err(PjCode::StageSkip));
    }

    #[test]
    fn sr_latency_hardline() {
        let mut pj = PerfJoint::new(1, 1, 51, 100);
        assert_eq!(pj.run_case(0, true, 10), Err(PjCode::SrLatency));
        pj.sr_latency_ms = 50; // 边界达标
        assert!(pj.run_case(0, true, 10).is_ok());
    }

    #[test]
    fn profile_gate() {
        let mut pj = PerfJoint::new(1, 1, 10, 100);
        for i in 0..10 {
            let _ = pj.run_case(i, true, 100);
        }
        assert_eq!(pj.profile_gate(), Ok(100));
        pj.profile_budget = 99;
        assert_eq!(pj.profile_gate(), Err(PjCode::ProfileBad));
    }
}
