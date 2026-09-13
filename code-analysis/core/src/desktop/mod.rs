//! UNREAL-X：桌面域（族0109~0112 · X02701~X02800）落点。
//!
//! - AI-11：族0109 桌面基准 / 族0110 桌面遥测（代码分析侧 50 项）
//! - AI-12：族0111 桌面布局分析 / 族0112 图标语义聚类（代码分析侧 50 项）
//!
//! 每族 25 项 CheckSet，四族合计 100 项；零 AI：全部确定性算法。

pub mod base;
pub mod benchmark;
pub mod cluster;
pub mod layout;
pub mod telemetry;

use crate::checks::CheckSet;

/// 族0109 桌面基准（X02701~X02725）。
pub fn run_desktop_bench_checks() -> Vec<CheckSet> {
    vec![benchmark::run_bench_checks()]
}

/// 族0110 桌面遥测（X02726~X02750）。
pub fn run_desktop_telemetry_checks() -> Vec<CheckSet> {
    vec![telemetry::run_telemetry_checks()]
}

/// 族0111 桌面布局分析（X02751~X02775）。
pub fn run_desktop_layout_checks() -> Vec<CheckSet> {
    vec![layout::run_layout_checks()]
}

/// 族0112 图标语义聚类（X02776~X02800）。
pub fn run_desktop_cluster_checks() -> Vec<CheckSet> {
    vec![cluster::run_cluster_checks()]
}

/// 桌面域全量聚合（100 项）。
pub fn run_desktop_checks() -> Vec<CheckSet> {
    let mut v = run_desktop_bench_checks();
    v.extend(run_desktop_telemetry_checks());
    v.extend(run_desktop_layout_checks());
    v.extend(run_desktop_cluster_checks());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_100_checks_pass() {
        let sets = run_desktop_checks();
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 100);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }
}
