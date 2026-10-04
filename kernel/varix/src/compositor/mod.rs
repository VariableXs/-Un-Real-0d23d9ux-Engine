//! UNREAL-X-15000 · AI-07 内核窗口引擎域（族0061~0069 · X01501~X01725）。
//!
//! 九族合入本目录：帧调度 / 窗口树 / 输入路由 / 碰撞检测 / 功耗 /
//! IPC 协议 / 故障恢复 / 高刷自适应 / 快照序列化。
//! 族0070（合成器基准）落 code-analysis/core/src/spatial/。
//! 每族 25 项 CheckSet，共 225 项；单元测试随族内 `mod tests`。

pub mod collide;
pub mod compower;
pub mod framesched;
pub mod recover;
pub mod refresh;
pub mod routing;
pub mod wipc;
pub mod wmsnap;
pub mod wtree;

/// 九族自检汇总（顺序即族号序）。
pub fn run_all_family_checks() -> [crate::checks::CheckSet; 9] {
    [
        framesched::run_framesched_checks(),
        wtree::run_wtree_checks(),
        routing::run_routing_checks(),
        collide::run_collide_checks(),
        compower::run_compower_checks(),
        wipc::run_wipc_checks(),
        recover::run_recover_checks(),
        refresh::run_refresh_checks(),
        wmsnap::run_wmsnap_checks(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositor_nine_families_all_green() {
        let sets = run_all_family_checks();
        assert_eq!(sets.len(), 9);
        let mut total = 0;
        for s in sets.iter() {
            assert_eq!(s.len(), 25, "family {} must have 25 checks", s.domain);
            for i in 0..s.len() {
                let c = s.get(i).unwrap();
                assert!(c.passed, "family {} check {} failed: {}", s.domain, c.name, c.detail);
            }
            total += s.len();
        }
        assert_eq!(total, 225);
    }
}
