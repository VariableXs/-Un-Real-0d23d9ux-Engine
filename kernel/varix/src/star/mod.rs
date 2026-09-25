//! Varix STAR I · 泳道一 B 内核底盘域·后段（AI-K2 · F058-F075）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-K2 分队的施工落位：
//! 十八项功能（F058-F075）逐项一模块，共享底盘收在 [`sbase`]（分钟账本、
//! 旋钮注册表、环形日志、分位数工具）。与既有内核模块的关系纪律：
//!
//! - 旧代 F 编号（AI-01..AI-10 划分的 F001-F500）与本目录的 STAR I 新编号
//!   **同号不同义**——本目录所有模块的判据一律以《Varix STAR I start.md》
//!   主册为准，引用格式 `F0xx` 均指 STAR I 语义；
//! - 依赖 AI-K1（F041-F057）与其它分队的接缝（帧率账本、写合并窗口、
//!   CPU 频率联动、图像解码面等）一律以**显式参数/闭包注入口**承接，
//!   不反向制造对未落地模块的编译依赖；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`memcomp`]   | F058 内存压缩前瞻 | 工作集报告先行；解压延迟/压缩比双达标 |
//! | [`netbatch`]  | F059 网络小包优化 | SSH 打字 P99 ≤30ms；10k pps 劣化 <20% |
//! | [`battery`]   | F060 电量账本     | 折算-实测相关性 >0.8；排行前三人工一致 |
//! | [`reggate`]   | F061 基准回归门   | 注入 5 处回退全检出；误报 <5% |
//! | [`selfcheck`] | F062 性能自检报告 | 阈值实机标定零误报；3 类慢故障全捕获 |
//! | [`touchpad`]  | F063 触控板手势前瞻 | 评估报告三节即交付 |
//! | [`audiolow`]  | F064 音频低延迟链 | 单流 ≤20ms；双流增量 <2ms |
//! | [`wakegov`]   | F065 唤醒源治理   | 合盖 2h 唤醒 ≤2 次、掉电 ≤2% |
//! | [`fsjournal`] | F066 文件系统日志策略 | 断电百次零 fsck；ordered 损失 <10% |
//! | [`coldhot`]   | F067 启动 IO 冷热分离 | 只读区随机读 P99 <2ms；耗时占比 <25% |
//! | [`assetldr`]  | F068 渲染资产按需装载 | 单主题 ≤80MB；切换无白屏 |
//! | [`perfmodes`] | F069 性能模式三档 | 三档差异可辨；切换无爆音无卡顿 |
//! | [`chainjudge`]| F070 性能域总判据 | P95 ≤12.5ms 且 P99 ≤16.6ms；证据两件 |
//! | [`startsearch`]| F071 开始菜单搜索直达 | 三类查询首结果 10/10；≤50ms |
//! | [`recenteng`] | F072 最近使用引擎 | 三消费面一致；衰减下沉 |
//! | [`thumbprev`] | F073 任务栏预览缩略图 | 一致性 10/10；出图 ≤400ms |
//! | [`jumplist`]  | F074 跳转清单     | 与 F072 一致；键盘可达 |
//! | [`traysys`]   | F075 托盘系统     | 滑杆 80fps；幽灵清除；点击 <100ms |

use crate::checks::CheckSet;

pub mod assetldr;
pub mod audiolow;
pub mod battery;
pub mod chainjudge;
pub mod coldhot;
pub mod fsjournal;
pub mod jumplist;
pub mod memcomp;
pub mod netbatch;
pub mod perfmodes;
pub mod recenteng;
pub mod reggate;
pub mod sbase;
pub mod selfcheck;
pub mod startsearch;
pub mod thumbprev;
pub mod touchpad;
pub mod traysys;
pub mod wakegov;

/// 域标识（CheckSet 聚合用）。
pub const STAR_DOMAIN: &str = "star-k2";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（施工期随模块落地扩列，
/// 全量十八项 + sbase）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限时
/// 该模块自身负责裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_star_checks() -> CheckSet {
    let mut set = CheckSet::new(STAR_DOMAIN);
    let blocks: [(&'static str, CheckSet); 19] = [
        ("sbase", sbase::run_sbase_checks()),
        ("F058", memcomp::run_memcomp_checks()),
        ("F059", netbatch::run_netbatch_checks()),
        ("F060", battery::run_battery_checks()),
        ("F061", reggate::run_reggate_checks()),
        ("F062", selfcheck::run_selfcheck_checks()),
        ("F063", touchpad::run_touchpad_checks()),
        ("F064", audiolow::run_audiolow_checks()),
        ("F065", wakegov::run_wakegov_checks()),
        ("F066", fsjournal::run_fsjournal_checks()),
        ("F067", coldhot::run_coldhot_checks()),
        ("F068", assetldr::run_assetldr_checks()),
        ("F069", perfmodes::run_perfmodes_checks()),
        ("F070", chainjudge::run_chainjudge_checks()),
        ("F071", startsearch::run_startsearch_checks()),
        ("F072", recenteng::run_recenteng_checks()),
        ("F073", thumbprev::run_thumbprev_checks()),
        ("F074", jumplist::run_jumplist_checks()),
        ("F075", traysys::run_traysys_checks()),
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
    fn star_domain_aggregate_all_green() {
        let set = run_star_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "STAR-K2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
