//! svstar2 — Varix STAR II · VE 图形引擎册 · 用户态服务落位。
//!
//! # 落位依据（★ 位置铁律，勿改 ★）
//!
//! 《VE Varix STAR II · 总纲与施工书》铁律 6：
//! > **内核红线**：全部功能针对 VARIX Rust 内核编写；内核态只留合成裁决与安全，
//! > VE 全部活在用户态服务；引擎崩溃零拖垮。
//!
//! 读法（勿误读为"能用前端语言写"）：
//! - 「用户态服务」指**不在内核态特权上下文运行**——崩溃不拖垮内核；
//! - 本仓内核树是 no_std Rust（`kernel/varix/Cargo.toml` 的 `[dependencies]` 为空，
//!   `[[bin]]` 带 `required-features = ["kernel-image"]`）；
//! - TypeScript 编译出 JS，**必须靠 V8/JSC 运行时才能执行**，内核态无堆、无 GC、
//!   无动态链接，装不下也不允许跑；且 GC 停顿与内核延迟硬要求直接冲突。
//! - 故 VE 全 6400 项落地为 **Rust**，本目录即其家。
//!
//! 目录名沿用仓库既有命名先例（`svstar` 服务守护域 → `svstar2` 承载 STAR II）。
//!
//! # 命名约定
//!
//! | 前缀 | 含义 |
//! | --- | --- |
//! | `vea01` … `vea10` | VE-A 域（内核图形抽象层，F0001-F0200）|
//! | `veb01` … `veb10` | VE-B 域（GPU 驱动矩阵，F0201-F0400）|
//! | 后续按域顺延 | 域字母与册内 `VE-x` 一致 |
//!
//! # 与既有模块的关系纪律（对齐 svstar/ 与 genstar2/ 的域约）
//!
//! - 本目录只放 **VE 册**功能；CoRun 册（操作系统本体/三通道兼容）走既有内核模块，
//!   CGPU 册（计算内核）另设落位——三册互不混装；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检并登记进本域聚合器，
//!   聚合器在 `robust.rs` 以单行注册（不占 domains 定长数组名额）；
//! - 每个模块带 `#[cfg(test)]` 单元测试，宿主侧 `cargo test` 直跑；
//! - 全部确定性算法、零 IO、可序列化：时间用逻辑 tick 注入，不用墙钟——
//!   保证回归可复现、对拍可重现（规格对拍红线）。
//!
//! # 域内模块地图（施工中，随进度增补）
//!
//! | 模块 | 功能 | 判据锚 |
//! | --- | --- | --- |
//! | [`vea01`] | F0001 虚拟显卡探测仲裁器 | VE 册 #VE-F0001 |
//! | [`vea02`] | F0002 图形上下文生命周期管理器 | VE 册 #VE-F0002 |
//! | [`vea03`] | F0003 围栏与同步原语集 | VE 册 #VE-F0003 |
//! | [`vea04`] | F0004 命令缓冲环形分配器 | VE 册 #VE-F0004 |

pub mod vea01_arbitrate;
pub mod vea01_engine;
pub mod vea01_index;
pub mod vea01_probe;
pub mod vea01_virtfeat;
pub mod vea02_ctx;
pub mod vea03_checks;
pub mod vea03_sync;
pub mod vea04_checks;
pub mod vea04_ring;

pub use vea01_index::{ProbeReport, effective_renderer, run_a01};

use crate::checks::CheckSet;

/// 域标识（CheckSet 聚合用）。
pub const VEA_DOMAIN: &str = "svstar2-ve";

/// 本域自检聚合：逐项 `run_*_checks` 汇总。
///
/// CheckSet 容量上限见 `crate::checks::MAX_CHECKS`；单模块超限由该模块
/// 自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_svstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(VEA_DOMAIN);
    // (标签, 子集) —— 逐项加行，施工一项加一项
    let blocks: [(&'static str, CheckSet); 4] = [
        ("VE-F0001", vea01_index::run_vea01_checks()),
        ("VE-F0002", vea02_ctx::run_vea02_checks()),
        ("VE-F0003", vea03_checks::run_vea03_checks()),
        ("VE-F0004", vea04_checks::run_vea04_checks()),
    ];
    for (tag, sub) in blocks.iter() {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(
            tag,
            passed,
            if passed { "" } else { "sub-checks red" },
        );
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svstar2_domain_aggregate_all_green() {
        let set = run_svstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "svstar2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 落位纪律自检：VE 册功能必须在 Rust 侧，不许混进前端 TS。
    /// 这条断言是"位置写错"的最后一道闸——防止后人又把它写回 `src/`。
    #[test]
    fn vea_lives_in_kernel_not_frontend() {
        // VE-F0001 的类型定义在本模块树内可见，且由内核 crate 编译
        let probe = vea01_probe::AdapterProbeInput::physical(
            "0000:01:00.0",
            "NVIDIA GeForce RTX 4060",
            "551.23",
        );
        let r = vea01_engine::probe_all(&[probe]);
        assert_eq!(r.len(), 1, "VE-F0001 由内核 Rust 侧实现并可执行");
    }
}
