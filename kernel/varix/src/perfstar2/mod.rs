//! perfstar2 — Varix STAR I start · B 性能域深化·后段 + 壳层五件（F058~F075 · AI-K2 分工包）。
//!
//! 本目录是《Varix STAR I start.md》主册 B-3 深化设计报告（G-B-18 ~ G-B-30）
//! 与 C-1 提案表报告（G-C-01 ~ G-C-05）的判据实装层——AI-K2 内核底盘·后段
//! 十八项，一项一事实：
//!
//! | 项 | 判据锚 | 子模块 |
//! | --- | --- | --- |
//! | F058 内存压缩前瞻 | G-B-18 | [`memcomp`] |
//! | F059 网络小包优化 | G-B-19 | [`smallpkt`] |
//! | F060 电量账本 | G-B-20 | [`powerledger`] |
//! | F061 基准回归门 | G-B-21 | [`benchgate`] |
//! | F062 性能自检报告 | G-B-22 | [`perfselfchk`] |
//! | F063 触控板手势前瞻 | G-B-23 | [`touchpad`] |
//! | F064 音频低延迟链 | G-B-24 | [`audiolat`] |
//! | F065 唤醒源治理 | G-B-25 | [`wakegov`] |
//! | F066 文件系统日志策略 | G-B-26 | [`fsjournal`] |
//! | F067 启动 IO 冷热分离 | G-B-27 | [`bootio`] |
//! | F068 渲染资产按需装载 | G-B-28 | [`assetload`] |
//! | F069 性能模式三档 | G-B-29 | [`perfmodes`] |
//! | F070 性能域总判据 | G-B-30 | [`perfgate`] |
//! | F071 开始菜单搜索直达 | G-C-01 | [`menusearch`] |
//! | F072 最近使用引擎 | G-C-02 | [`frecency`] |
//! | F073 任务栏预览缩略图 | G-C-03 | [`thumbcard`] |
//! | F074 跳转清单 | G-C-04 | [`jumplist`] |
//! | F075 托盘系统 | G-C-05 | [`trayhub`] |
//!
//! 共同纪律（与主册铁律对齐，同 perfstar/AI-K1 先例）：
//! - **零堆热路径**：所有内核路径定长结构，无 Vec/String/Box/format!；
//!   分位数一律 128 元素插入排序取位次（无 64K 直方图上栈——内核栈预算红线）。
//! - **一处一事实**：每条常量在注释里写明主册依据与推导；跨批次联动直接
//!   引用既有 API（F069 用 K1 `cpufreq::ManualMode`/`wcoalesce::Tier`、
//!   F066 用 `wcoalesce::Tier::window_ms`、F074 用本包 `frecency` 引擎），
//!   不复制常量。
//! - **判据唯一源**：每个域模块头注释逐条摘录主册判据，`run_*_checks`
//!   逐条实摆；十二查叠加执行。
//! - **诚实降级**：电池计不可读/索引未就绪/表满等一律显式呈现，不静默吞。

use crate::checks::CheckSet;

pub mod assetload;
pub mod audiolat;
pub mod benchgate;
pub mod bootio;
pub mod frecency;
pub mod fsjournal;
pub mod jumplist;
pub mod memcomp;
pub mod menusearch;
pub mod perfgate;
pub mod perfmodes;
pub mod perfselfchk;
pub mod powerledger;
pub mod smallpkt;
pub mod thumbcard;
pub mod touchpad;
pub mod trayhub;
pub mod wakegov;

/// 全域自检登记名（与 [`DOMAIN_NAMES`] 一一对账）。
pub const PERFSTAR2_DOMAIN: &str = "perf-k2";

/// 全域自检登记名（robust.rs KernelCheckup 用，每项一个独立域集）。
pub const DOMAIN_NAMES: [&str; 18] = [
    "F058-memcomp",
    "F059-smallpkt",
    "F060-powerledger",
    "F061-benchgate",
    "F062-perfselfchk",
    "F063-touchpad",
    "F064-audiolat",
    "F065-wakegov",
    "F066-fsjournal",
    "F067-bootio",
    "F068-assetload",
    "F069-perfmodes",
    "F070-perfgate",
    "F071-menusearch",
    "F072-frecency",
    "F073-thumbcard",
    "F074-jumplist",
    "F075-trayhub",
];

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（十八项全量）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——本聚合器按
/// 「每模块一行」登记（18 行 ≤ 64），永不超容；单模块自身超限时由该模块
/// 负责裁剪。域内逐判据红绿在各模块 `run_*_checks` 的子行展开。
pub fn run_perfstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(PERFSTAR2_DOMAIN);
    let blocks: [(&'static str, CheckSet); 18] = [
        ("F058", memcomp::run_memcomp_checks()),
        ("F059", smallpkt::run_smallpkt_checks()),
        ("F060", powerledger::run_powerledger_checks()),
        ("F061", benchgate::run_benchgate_checks()),
        ("F062", perfselfchk::run_perfselfchk_checks()),
        ("F063", touchpad::run_touchpad_checks()),
        ("F064", audiolat::run_audiolat_checks()),
        ("F065", wakegov::run_wakegov_checks()),
        ("F066", fsjournal::run_fsjournal_checks()),
        ("F067", bootio::run_bootio_checks()),
        ("F068", assetload::run_assetload_checks()),
        ("F069", perfmodes::run_perfmodes_checks()),
        ("F070", perfgate::run_perfgate_checks()),
        ("F071", menusearch::run_menusearch_checks()),
        ("F072", frecency::run_frecency_checks()),
        ("F073", thumbcard::run_thumbcard_checks()),
        ("F074", jumplist::run_jumplist_checks()),
        ("F075", trayhub::run_trayhub_checks()),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfstar2_domain_aggregate_all_green() {
        let set = run_perfstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "PERF-K2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert_eq!(passed, 18, "十八项逐行在账");
        assert!(!set.truncated(), "聚合器未超容");
    }

    #[test]
    fn domain_names_match_block_tags() {
        // DOMAIN_NAMES 与聚合 blocks 一一对账（一处一事实的登记面）。
        assert_eq!(DOMAIN_NAMES.len(), 18);
        assert!(DOMAIN_NAMES[0].starts_with("F058"));
        assert!(DOMAIN_NAMES[17].starts_with("F075"));
    }
}
