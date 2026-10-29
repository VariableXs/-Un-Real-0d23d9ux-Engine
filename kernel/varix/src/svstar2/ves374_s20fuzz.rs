//! ves374_s20fuzz — VE-F4134 · T07 i18n 性能 fuzz
//! fuzz 三面：成本模型输入（极端系数）/ 基准运行器（畸形参数）/ 调试器采样（高频注入）——三面语料库 + 三零线（崩溃 0/挂起 0/越权写 0）+ 线未达阻断 + 最小复现入库 + 补面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD940;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    Ok = 0,
    Crash = SEG_BASE + 0x01,
    Hang = SEG_BASE + 0x02,
    PrivWrite = SEG_BASE + 0x03,
}

/// 三面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzFace { CostInput = 0, BenchParam = 1, SamplerInject = 2 }

impl FuzzFace {
    pub const ALL: [FuzzFace; 3] = [FuzzFace::CostInput, FuzzFace::BenchParam, FuzzFace::SamplerInject];
    pub fn name(self) -> &'static str {
        match self { FuzzFace::CostInput => "cost-input", FuzzFace::BenchParam => "bench-param",
            FuzzFace::SamplerInject => "sampler-inject" }
    }
}

/// 被测面一：成本模型输入校验（系数越界钳制必报错不崩）。
pub fn cost_input_check(factor_bp: u32, units: u32) -> Result<u32, FzCode> {
    if factor_bp == 0 { return Err(FzCode::Crash); }
    if factor_bp > 100_000 { return Err(FzCode::PrivWrite); } // 越权写线
    // 防溢出：units 极大时钳制。
    let capped = units.min(1_000_000);
    Ok(factor_bp.wrapping_mul(capped) / 1_000)
}

/// 被测面二：基准运行器参数（组序号/语种序号越界必拒）。
pub fn bench_param_check(group: u8, lang: u8) -> Result<(u8, u8), FzCode> {
    if group >= 6 { return Err(FzCode::Crash); }
    if lang >= 4 { return Err(FzCode::Crash); }
    Ok((group, lang))
}

/// 被测面三：调试器高频注入（每 tick 采样 → 深度上限防挂起）。
pub fn sampler_depth(ticks: u32) -> Result<u32, FzCode> {
    if ticks == u32::MAX { return Err(FzCode::Hang); } // 无限挂起线
    let depth = ticks.min(1_000_000);
    if depth > 900_000 { return Err(FzCode::PrivWrite); } // 越权写线
    Ok(depth)
}

/// 语料样本。
#[derive(Clone)]
pub struct Sample { pub face: FuzzFace, pub payload: &'static str, pub malformed: bool }

pub struct Corpus { pub samples: Vec<Sample> }

impl Corpus {
    pub fn new() -> Self { Corpus { samples: Vec::new() } }
    pub fn add(&mut self, s: Sample) { self.samples.push(s); }
    pub fn covered(&self) -> bool {
        FuzzFace::ALL.iter().all(|f| self.samples.iter().filter(|s| s.face == *f).count() >= 2)
    }
    pub fn missing_faces(&self) -> Vec<&'static str> {
        FuzzFace::ALL.iter().filter(|f| self.samples.iter().filter(|s| s.face == **f).count() < 2)
            .map(|f| f.name()).collect()
    }
}

/// 三零线 + 执行器。
pub struct Runner { pub zero: (u32, u32, u32), pub minimals: Vec<&'static str>, pub blocked: bool }

impl Runner {
    pub fn new() -> Self { Runner { zero: (0, 0, 0), minimals: Vec::new(), blocked: false } }

    /// 畸形被接受或合法被拒 → 破线 + 复现入库。
    pub fn run_cost(&mut self, factor_bp: u32, units: u32, malformed: bool) -> Result<u32, FzCode> {
        let r = cost_input_check(factor_bp, units);
        let broke = (malformed && r.is_ok()) || (!malformed && r.is_err());
        if broke {
            self.zero.0 += 1;
            self.minimals.push("cost-input");
            self.blocked = true;
            return Err(FzCode::Crash);
        }
        r
    }

    pub fn clean(&self) -> bool { self.zero == (0, 0, 0) && !self.blocked }

    pub fn gate(&self) -> Result<(), FzCode> {
        if self.clean() { Ok(()) } else { Err(FzCode::Crash) }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves374_checks() -> CheckSet {
    let mut set = CheckSet::new("ves374");

    // C374-01 三面单源。
    chk(&mut set, "C374-01", FuzzFace::ALL.len() == 3 && FuzzFace::ALL[2].name() == "sampler-inject", "三面单源");

    // C374-02 语料三面齐备。
    let mut cp = Corpus::new();
    cp.add(Sample { face: FuzzFace::CostInput, payload: "1600", malformed: false });
    cp.add(Sample { face: FuzzFace::CostInput, payload: "0", malformed: true });
    cp.add(Sample { face: FuzzFace::BenchParam, payload: "g5l3", malformed: false });
    cp.add(Sample { face: FuzzFace::BenchParam, payload: "g9", malformed: true });
    cp.add(Sample { face: FuzzFace::SamplerInject, payload: "1000", malformed: false });
    cp.add(Sample { face: FuzzFace::SamplerInject, payload: "u32max", malformed: true });
    chk(&mut set, "C374-02", cp.covered() && cp.missing_faces().is_empty(), "三面各 2 齐");

    // C374-03 面缺检出。
    let mut cp2 = Corpus::new();
    cp2.add(Sample { face: FuzzFace::CostInput, payload: "x", malformed: false });
    chk(&mut set, "C374-03", cp2.missing_faces() == vec!["bench-param", "sampler-inject"], "缺两面");

    // C374-04 极端系数：0 系数拒。
    chk(&mut set, "C374-04", cost_input_check(0, 100) == Err(FzCode::Crash), "0 系数 => 拒");

    // C374-05 越权系数 → 写线。
    chk(&mut set, "C374-05", cost_input_check(200_000, 10) == Err(FzCode::PrivWrite), "20 倍系数 => PrivWrite");

    // C374-06 正常系数计算。
    chk(&mut set, "C374-06", cost_input_check(1_600, 1_000) == Ok(1_600), "1.6×1000=1600");

    // C374-07 极大 units 钳制不溢出。
    chk(&mut set, "C374-07", cost_input_check(1_600, u32::MAX).is_ok(), "极大 units 钳制安全");

    // C374-08 基准参数越界拒。
    chk(&mut set, "C374-08",
        bench_param_check(9, 0).is_err() && bench_param_check(0, 7).is_err()
        && bench_param_check(5, 3).is_ok(), "组 9/语种 7 拒 组 5 语种 3 过");

    // C374-09 高频注入挂起线。
    chk(&mut set, "C374-09", sampler_depth(u32::MAX) == Err(FzCode::Hang), "u32max tick => 挂起线");

    // C374-10 注入越权写线。
    chk(&mut set, "C374-10", sampler_depth(950_000) == Err(FzCode::PrivWrite), "95 万深度 => PrivWrite");

    // C374-11 注入带内。
    chk(&mut set, "C374-11", sampler_depth(1_000) == Ok(1_000), "1000 tick 正常");

    // C374-12 线破阻断 + 复现 + 干净过闸。
    let mut rn = Runner::new();
    let _ = rn.run_cost(0, 100, true);
    chk(&mut set, "C374-12", !rn.clean() && rn.gate().is_err() && rn.minimals.len() == 1
        && Runner::new().gate().is_ok(), "破线阻断 复现入库 干净过闸");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves374_smoke() {
        let set = run_ves374_checks();
        assert!(set.failed_count() == 0, "ves374 red: {}", set.red_items_text());
    }

    #[test]
    fn ves374_cost() {
        assert!(cost_input_check(1_000, 42).is_ok());
        assert_eq!(cost_input_check(1_000, 42), Ok(42));
    }
}
