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
pub mod bootmenu_b3;
pub mod bootmenu_b4;
pub mod bootmenu_b5;
pub mod bootmenu_b7;
pub mod capenforce;
pub mod capenforce_b3;
pub mod capenforce_b4;
pub mod capenforce_b5;
pub mod capenforce_b6;
pub mod capenforce_b7;
pub mod crashiso;
pub mod crashiso_b3;
pub mod crashiso_b4;
pub mod crashiso_b5;
pub mod crashiso_b6;
pub mod crashiso_b7;
pub mod diagsnap;
pub mod diagsnap_b3;
pub mod diagsnap_b4;
pub mod diagsnap_b5;
pub mod diagsnap_b7;
pub mod diskhealth;
pub mod diskhealth_b3;
pub mod diskhealth_b4;
pub mod diskhealth_b5;
pub mod diskhealth_b6;
pub mod diskhealth_b7;
pub mod diskhealth_b8;
pub mod duoclock;
pub mod duoclock_b3;
pub mod duoclock_b4;
pub mod duoclock_b5;
pub mod duoclock_b6;
pub mod duoclock_b7;
pub mod duoclock_b8;
pub mod handoffchk;
pub mod handoffchk_b3;
pub mod handoffchk_b4;
pub mod handoffchk_b5;
pub mod handoffchk_b6;
pub mod handoffchk_b7;
pub mod handoffchk_b8;
pub mod hotplug;
pub mod hotplug_b3;
pub mod hotplug_b4;
pub mod hotplug_b5;
pub mod hotplug_b6;
pub mod hotplug_b7;
pub mod memguard;
pub mod memguard_b3;
pub mod memguard_b4;
pub mod memguard_b5;
pub mod memguard_b6;
pub mod memguard_b7;
pub mod memguard_b8;
pub mod panicscreen;
pub mod panicscreen_b3;
pub mod panicscreen_b4;
pub mod panicscreen_b5;
pub mod panicscreen_b6;
pub mod panicscreen_b7;
pub mod permaudit;
pub mod permaudit_b3;
pub mod permaudit_b4;
pub mod permaudit_b5;
pub mod permaudit_b6;
pub mod permaudit_b7;
pub mod pwrdrill;
pub mod pwrdrill_b3;
pub mod pwrdrill_b4;
pub mod pwrdrill_b5;
pub mod pwrdrill_b6;
pub mod pwrdrill_b7;
pub mod pwrdrill_b8;
pub mod romount;
pub mod romount_b3;
pub mod romount_b4;
pub mod romount_b5;
pub mod romount_b6;
pub mod romount_b7;
pub mod selftestviz;
pub mod selftestviz_b3;
pub mod selftestviz_b4;
pub mod selftestviz_b5;
pub mod selftestviz_b6;
pub mod selftestviz_b7;
pub mod selftestviz_b8;
pub mod signbadge;
pub mod signbadge_b3;
pub mod signbadge_b4;
pub mod signbadge_b5;
pub mod signbadge_b6;
pub mod signbadge_b7;
pub mod signbadge_b8;

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

/// 域深化自检聚合（检查项对账层——各模块 run_*_deep_checks 与批次三
/// run_*_b3_checks、批次四 run_*_b4_checks 三层合并汇总；深化自检对账
/// 主册【设计细节】子句，与主判据层互补不重叠）。
/// 空检查集（F171/F174 无 b6 批次——占位行不影响全绿判定）。
fn no_checks_placeholder() -> CheckSet {
    CheckSet::new("b6-none")
}

pub fn run_secstar_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("secstar-s1-deep");
    // 函数指针表（8B/项）而非物化 CheckSet——多批次 × 15 模块的 CheckSet
    // 若在栈上同时物化会撑爆测试线程栈。指针表 + 循环内逐层判定把峰值
    // 压到单模块一层临时集合。
    type CheckFn = fn() -> CheckSet;
    let blocks: [(&'static str, CheckFn, CheckFn, CheckFn, CheckFn, CheckFn, CheckFn, CheckFn); 15] = [
        ("F171d", bootmenu::run_bootmenu_deep_checks, bootmenu_b3::run_bootmenu_b3_checks, bootmenu_b4::run_bootmenu_b4_checks, bootmenu_b5::run_bootmenu_b5_checks, no_checks_placeholder, bootmenu_b7::run_bootmenu_b7_checks, no_checks_placeholder),
        ("F172d", selftestviz::run_selftestviz_deep_checks, selftestviz_b3::run_selftestviz_b3_checks, selftestviz_b4::run_selftestviz_b4_checks, selftestviz_b5::run_selftestviz_b5_checks, selftestviz_b6::run_selftestviz_b6_checks, selftestviz_b7::run_selftestviz_b7_checks, selftestviz_b8::run_selftestviz_b8_checks),
        ("F173d", panicscreen::run_panicscreen_deep_checks, panicscreen_b3::run_panicscreen_b3_checks, panicscreen_b4::run_panicscreen_b4_checks, panicscreen_b5::run_panicscreen_b5_checks, panicscreen_b6::run_panicscreen_b6_checks, panicscreen_b7::run_panicscreen_b7_checks, no_checks_placeholder),
        ("F174d", diagsnap::run_diagsnap_deep_checks, diagsnap_b3::run_diagsnap_b3_checks, diagsnap_b4::run_diagsnap_b4_checks, diagsnap_b5::run_diagsnap_b5_checks, no_checks_placeholder, diagsnap_b7::run_diagsnap_b7_checks, no_checks_placeholder),
        ("F175d", crashiso::run_crashiso_deep_checks, crashiso_b3::run_crashiso_b3_checks, crashiso_b4::run_crashiso_b4_checks, crashiso_b5::run_crashiso_b5_checks, crashiso_b6::run_crashiso_b6_checks, crashiso_b7::run_crashiso_b7_checks, no_checks_placeholder),
        ("F176d", memguard::run_memguard_deep_checks, memguard_b3::run_memguard_b3_checks, memguard_b4::run_memguard_b4_checks, memguard_b5::run_memguard_b5_checks, memguard_b6::run_memguard_b6_checks, memguard_b7::run_memguard_b7_checks, memguard_b8::run_memguard_b8_checks),
        ("F177d", capenforce::run_capenforce_deep_checks, capenforce_b3::run_capenforce_b3_checks, capenforce_b4::run_capenforce_b4_checks, capenforce_b5::run_capenforce_b5_checks, capenforce_b6::run_capenforce_b6_checks, capenforce_b7::run_capenforce_b7_checks, no_checks_placeholder),
        ("F178d", signbadge::run_signbadge_deep_checks, signbadge_b3::run_signbadge_b3_checks, signbadge_b4::run_signbadge_b4_checks, signbadge_b5::run_signbadge_b5_checks, signbadge_b6::run_signbadge_b6_checks, signbadge_b7::run_signbadge_b7_checks, signbadge_b8::run_signbadge_b8_checks),
        ("F179d", permaudit::run_permaudit_deep_checks, permaudit_b3::run_permaudit_b3_checks, permaudit_b4::run_permaudit_b4_checks, permaudit_b5::run_permaudit_b5_checks, permaudit_b6::run_permaudit_b6_checks, permaudit_b7::run_permaudit_b7_checks, no_checks_placeholder),
        ("F180d", pwrdrill::run_pwrdrill_deep_checks, pwrdrill_b3::run_pwrdrill_b3_checks, pwrdrill_b4::run_pwrdrill_b4_checks, pwrdrill_b5::run_pwrdrill_b5_checks, pwrdrill_b6::run_pwrdrill_b6_checks, pwrdrill_b7::run_pwrdrill_b7_checks, pwrdrill_b8::run_pwrdrill_b8_checks),
        ("F181d", handoffchk::run_handoffchk_deep_checks, handoffchk_b3::run_handoffchk_b3_checks, handoffchk_b4::run_handoffchk_b4_checks, handoffchk_b5::run_handoffchk_b5_checks, handoffchk_b6::run_handoffchk_b6_checks, handoffchk_b7::run_handoffchk_b7_checks, handoffchk_b8::run_handoffchk_b8_checks),
        ("F182d", duoclock::run_duoclock_deep_checks, duoclock_b3::run_duoclock_b3_checks, duoclock_b4::run_duoclock_b4_checks, duoclock_b5::run_duoclock_b5_checks, duoclock_b6::run_duoclock_b6_checks, duoclock_b7::run_duoclock_b7_checks, duoclock_b8::run_duoclock_b8_checks),
        ("F183d", diskhealth::run_diskhealth_deep_checks, diskhealth_b3::run_diskhealth_b3_checks, diskhealth_b4::run_diskhealth_b4_checks, diskhealth_b5::run_diskhealth_b5_checks, diskhealth_b6::run_diskhealth_b6_checks, diskhealth_b7::run_diskhealth_b7_checks, diskhealth_b8::run_diskhealth_b8_checks),
        ("F184d", hotplug::run_hotplug_deep_checks, hotplug_b3::run_hotplug_b3_checks, hotplug_b4::run_hotplug_b4_checks, hotplug_b5::run_hotplug_b5_checks, hotplug_b6::run_hotplug_b6_checks, hotplug_b7::run_hotplug_b7_checks, no_checks_placeholder),
        ("F185d", romount::run_romount_deep_checks, romount_b3::run_romount_b3_checks, romount_b4::run_romount_b4_checks, romount_b5::run_romount_b5_checks, romount_b6::run_romount_b6_checks, romount_b7::run_romount_b7_checks, no_checks_placeholder),
    ];
    for (tag, f0, f1, f2, f3, f4, f5, f6) in blocks {
        // 逐层判定再 AND——七层合并后的集合会超 CheckSet 64 条上限
        // （truncated 误红）；各层独立 ≤64，行语义 = 全层全绿。
        let sets = [f0(), f1(), f2(), f3(), f4(), f5(), f6()];
        let passed = sets.iter().all(|s| s.all_passed() && !s.truncated());
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

    #[test]
    fn secstar_domain_deep_aggregate_all_green() {
        let set = run_secstar_deep_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "SECSTAR-S1 深化自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
