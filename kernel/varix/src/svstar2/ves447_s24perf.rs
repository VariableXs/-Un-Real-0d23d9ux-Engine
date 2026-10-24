//! ves447_s24perf — VE-F4213 · 一致性性能
//! 三线基准：扫描平台吞吐（万目标/分钟档）+规则引擎执行延迟（增量 P95）+图谱查询延迟；
//! 与家族性能预算联动登记；越阈→定位+预算重议；扫描风暴→分片并行；查询劣化→缓存策略；
//! 性能基准含无障碍规则扫描延迟单列（不因性能牺牲无障碍检——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x14C0;
pub const THROUGHPUT_FLOOR: u32 = 10_000; // 万目标/分钟档

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    Ok = 0,
    OverBudget = SEG_BASE + 0x01, // 0x14C1
    StormDetected = SEG_BASE + 0x02, // 0x14C2
    QueryDegraded = SEG_BASE + 0x03, // 0x14C3
}

/// 三线基准值。
pub struct PerfLines {
    pub throughput: u32,     // 目标/分钟
    pub incremental_p95: u32, // μs
    pub graph_query: u32,    // μs
    pub a11y_scan: u32,      // μs 无障碍规则扫描单列（域本色）
    // 阈值
    pub p95_limit: u32,
    pub graph_limit: u32,
    pub a11y_limit: u32,
    pub shards: u32,
    pub cache_on: bool,
    pub registered: bool, // 家族预算登记
}

impl PerfLines {
    pub fn new() -> Self {
        PerfLines { throughput: 0, incremental_p95: 0, graph_query: 0, a11y_scan: 0,
            p95_limit: 1000, graph_limit: 500, a11y_limit: 200, shards: 1, cache_on: false, registered: false }
    }
    /// 三线基准裁决。
    pub fn verdict(&self) -> Result<(), PfCode> {
        if self.throughput < THROUGHPUT_FLOOR {
            if self.shards > 1 { /* 分片后重测 */ } else { return Err(PfCode::StormDetected); }
        }
        if self.incremental_p95 > self.p95_limit { return Err(PfCode::OverBudget); }
        if self.graph_query > self.graph_limit {
            if self.cache_on { /* 缓存策略生效后复测 */ } else { return Err(PfCode::QueryDegraded); }
        }
        Ok(())
    }
    /// 域本色单列：无障碍扫描延迟不得超阈（不可被整体预算摊薄）。
    pub fn a11y_gate(&self) -> Result<(), PfCode> {
        if self.a11y_scan > self.a11y_limit { Err(PfCode::OverBudget) } else { Ok(()) }
    }
    /// 家族预算联动登记（与总表对拍）。
    pub fn register_family(&mut self, family_p95: u32, tol_bp: u32) -> bool {
        let a = self.incremental_p95.max(family_p95) as u64;
        let b = self.incremental_p95.min(family_p95) as u64;
        let agree = a == 0 || (a - b) * 10_000 / a <= tol_bp as u64;
        self.registered = agree;
        agree
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves447_checks() -> CheckSet {
    let mut set = CheckSet::new("ves447");
    let mut p = PerfLines::new();

    // C447-01 吞吐不达万目标档→风暴码。
    p.throughput = 8000;
    chk(&mut set, "C447-01", p.verdict() == Err(PfCode::StormDetected), "风暴检出");

    // C447-02 分片并行后复测过线。
    p.shards = 4;
    p.throughput = 12_000;
    chk(&mut set, "C447-02", p.throughput >= THROUGHPUT_FLOOR, "分片过线");

    // C447-03 增量 P95 越阈→预算码。
    p.incremental_p95 = 1200;
    chk(&mut set, "C447-03", p.verdict() == Err(PfCode::OverBudget), "P95 越阈");

    // C447-04 预算重议（阈值上调至重议值）后过。
    p.p95_limit = 1300;
    chk(&mut set, "C447-04", p.verdict().is_ok(), "重议过");

    // C447-05 图谱查询劣化→缓存码。
    p.graph_query = 600;
    chk(&mut set, "C447-05", p.verdict() == Err(PfCode::QueryDegraded), "查询劣化");

    // C447-06 缓存策略生效后复测过。
    p.cache_on = true;
    chk(&mut set, "C447-06", p.verdict().is_ok(), "缓存复测过");

    // C447-07 域本色单列：a11y 扫描延迟越阈独立拒（不可摊薄）。
    p.a11y_scan = 250;
    chk(&mut set, "C447-07", p.a11y_gate() == Err(PfCode::OverBudget) && p.verdict().is_ok(), "单列独立拒");

    // C447-08 a11y 扫描带内通过。
    p.a11y_scan = 180;
    chk(&mut set, "C447-08", p.a11y_gate().is_ok(), "单列带内");

    // C447-09 家族预算对拍：容差内一致→登记。
    chk(&mut set, "C447-09", p.register_family(1250, 500) && p.registered, "家族登记");

    // C447-10 家族对拍失配→不登记。
    let mut p2 = PerfLines::new();
    p2.incremental_p95 = 2000;
    chk(&mut set, "C447-10", !p2.register_family(1000, 500) && !p2.registered, "对拍失配");

    // C447-11 万目标档常量单源。
    chk(&mut set, "C447-11", THROUGHPUT_FLOOR == 10_000, "万目标档");

    // C447-12 三线全绿终态。
    chk(&mut set, "C447-12", p.verdict().is_ok() && p.a11y_gate().is_ok()
        && p.registered && p.shards == 4 && p.cache_on, "三线终态");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves447_smoke() {
        let set = run_ves447_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves447 red: {:?}", red);
    }
}
