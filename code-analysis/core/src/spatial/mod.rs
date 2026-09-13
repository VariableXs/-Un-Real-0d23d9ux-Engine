//! UNREAL-X-15000 · code-analysis `spatial` 域模块。
//! AI-07 族0070（合成器基准 X01726~X01750）+ AI-08 十族
//! （画像/热力/切换成本/混乱度/推荐/剖析/接力/脚本/审计/收官，
//!   X01751~X02000）。每族 25 项 CheckSet，共 275 项。

pub mod a11yaudit;
pub mod autoscript;
pub mod bench;
pub mod handoff;
pub mod heatmap;
pub mod layoutrec;
pub mod messscore;
pub mod perfprof;
pub mod profile;
pub mod switchcost;
pub mod wrapup;

use crate::checks::CheckSet;

/// AI-07 族0070（合成器基准）自检。
pub fn run_ux07_bench_checks() -> CheckSet {
    bench::run_bench_checks()
}

/// AI-08 十族自检汇总（X01751~X02000）。
pub fn run_ux08_checks() -> Vec<CheckSet> {
    vec![
        profile::run_profile_checks(),
        heatmap::run_heatmap_checks(),
        switchcost::run_switchcost_checks(),
        messscore::run_messscore_checks(),
        layoutrec::run_layoutrec_checks(),
        perfprof::run_perfprof_checks(),
        handoff::run_handoff_checks(),
        autoscript::run_autoscript_checks(),
        a11yaudit::run_a11yaudit_checks(),
        wrapup::run_wrapup_checks(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux07_bench_25_all_pass() {
        let cs = run_ux07_bench_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }

    #[test]
    fn ux08_ten_families_250_all_pass() {
        let sets = run_ux08_checks();
        assert_eq!(sets.len(), 10);
        let mut total = 0;
        for cs in &sets {
            assert_eq!(cs.total(), 25, "family {} must have 25", cs.domain);
            assert!(cs.all_pass(), "{}", cs.render());
            total += cs.total();
        }
        assert_eq!(total, 250);
    }
}
