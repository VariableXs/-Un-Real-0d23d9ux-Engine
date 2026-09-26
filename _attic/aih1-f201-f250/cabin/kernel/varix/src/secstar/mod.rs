//! Varix STAR I · 泳道一 G 安全加固域·前段（AI-S1 · F171-F185）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-S1 分队的施工落位：
//! 十五项功能（F171-F185）逐项一模块。与既有内核模块的关系纪律：
//!
//! - 旧代 F 编号（AI-01..AI-10 等历史计划的 F001-F700）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以《Varix STAR I
//!   start.md》主册为准，引用格式 `F1xx` 均指 STAR I 语义（主册 G-G-01~
//!   G-G-15 段）；
//! - 与 AI-S2（F186-F200，`secstar2/` 目录）同泳道不共目录：分队落位互相
//!   独立，不依赖对方未注册的代码；跨分队接缝一律以**显式参数注入口**
//!   承接（如 F179 的三源账本由调用方注入，本层不复制账本本体）；
//! - 与 K 分队共享底盘（`crate::star::sbase` 的旋钮/环账/分位工具）按需
//!   复用——零冗余纪律，一处一事实；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`bootmenu`]    | F171 图形化引导选单   | 等价性 3×20 轮；资产 <200KB；降级实测 |
//! | [`selftestviz`] | F172 引导自检可视化   | 项数对拍 9/9·11/11·10/10；红闪；日志 100% |
//! | [`panicscreen`] | F173 panic 画面设计   | 百次演练 100%；倒计时；二维码三机型 |
//! | [`diagsnap`]    | F174 诊断快照键       | 落盘 <2s；帧率无感；脱敏三查 |
//! | [`crashiso`]    | F175 崩溃隔离强化     | 隔离 10/10；帧率不跌；遮罩 ≤500ms |
//! | [`memguard`]    | F176 内存守卫         | 6 类越界全捕获；开销 <3% |
//! | [`capenforce`]  | F177 能力执法可视化   | 四执法点注入准确；聚合正确；总闸全效 |
//! | [`signbadge`]   | F178 签名状态角标     | 三态角标 3 应用；tooltip 帮助链；截图保留 |
//! | [`permaudit`]   | F179 权限审计页       | 三源对拍一致；收回即时；导出脱敏 |
//! | [`pwrdrill`]    | F180 断电演练自动化   | 4 周 400 轮零漏跑；双盲 100%；三态覆盖 |
//! | [`handoffchk`]  | F181 交接预检器       | 三查三态面板；通行 <1s；拦停留痕 |
//! | [`duoclock`]    | F182 双域时钟同步     | 10 轮零漂移；>5s 提示；回拨保护 |
//! | [`diskhealth`]  | F183 存储健康监测     | 对拍 ±2%；掉电续计零丢失；三段阈值 |
//! | [`hotplug`]     | F184 热插拔体验       | 全链录屏；冲刷与 F046 一致；未弹出修复 |
//! | [`romount`]     | F185 只读卷保护提示   | 三类徽标分型；拖放受阻全链；帮助链通 |

use crate::checks::CheckSet;

pub mod bootmenu;
pub mod capenforce;
pub mod crashiso;
pub mod diagsnap;
pub mod diskhealth;
pub mod duoclock;
pub mod handoffchk;
pub mod hotplug;
pub mod memguard;
pub mod panicscreen;
pub mod permaudit;
pub mod pwrdrill;
pub mod romount;
pub mod selftestviz;
pub mod signbadge;

/// 域标识（CheckSet 聚合用）。
pub const SECSTAR_DOMAIN: &str = "secstar-s1";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（十五项全量）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`）——本聚合器按
/// 「每模块一行」登记（15 行），永不超容；单模块自身超限时由该模块负责
/// 裁剪（与 secstar2 同纪律）。
pub fn run_secstar_checks() -> CheckSet {
    let mut set = CheckSet::new(SECSTAR_DOMAIN);
    let blocks: [(&'static str, CheckSet); 15] = [
        ("F171", bootmenu::run_bootmenu_checks()),
        ("F172", selftestviz::run_selftestviz_checks()),
        ("F173", panicscreen::run_panicscreen_checks()),
        ("F174", diagsnap::run_diagsnap_checks()),
        ("F175", crashiso::run_crashiso_checks()),
        ("F176", memguard::run_memguard_checks()),
        ("F177", capenforce::run_capenforce_checks()),
        ("F178", signbadge::run_signbadge_checks()),
        ("F179", permaudit::run_permaudit_checks()),
        ("F180", pwrdrill::run_pwrdrill_checks()),
        ("F181", handoffchk::run_handoffchk_checks()),
        ("F182", duoclock::run_duoclock_checks()),
        ("F183", diskhealth::run_diskhealth_checks()),
        ("F184", hotplug::run_hotplug_checks()),
        ("F185", romount::run_romount_checks()),
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
    fn secstar_domain_aggregate_all_green() {
        let set = run_secstar_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "SECSTAR-S1 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
