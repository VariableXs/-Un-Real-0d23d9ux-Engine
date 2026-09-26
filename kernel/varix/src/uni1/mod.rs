//! Varix STAR I · 泳道四 I 通用域·一分队（AI-U1 · F401-F450 批次一）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-U1 分队的施工落位。
//! 批次一落八项（F401/F403/F404/F405/F407/F408/F416/F424）——「系统
//! 快捷键与通用交互语义」簇；共享底盘收在 [`ubase`]（组合键注册表、
//! 浮层层级栈、事件环、旋钮）。与既有模块的关系纪律：
//!
//! - 判据一律以《Varix STAR I start.md》主册 I-1 深化设计报告为准，
//!   引用格式 `F4xx` 均指 STAR I 语义（与旧代 F 编号同号不同义）；
//! - 跨队依赖一律以显式参数/闭包注入口承接（对本仓库其它分队目录
//!   **零引用**——跨泳道借力 = 0）；
//! - 每模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧直跑）；
//! - 批次二/三（其余 42 项）续建本目录，聚合器随批次扩列。
//!
//! ## 批次一模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`autoarrange`]| F401 桌面自动排列 | 四序 20 项实测；插入/删除补位；互斥切换；弹回（F124 弹性档） |
//! | [`lockhot`]    | F403 Win+L 锁屏快捷 | 锁定 <300ms；媒体暂停续播；窗口保持；摘要只计数；F316 同入口 |
//! | [`explorehot`] | F404 Win+E 资源管理器 | 标签/窗模式；此机页清单；连按行为；首开 <1.5s 骨架先行 |
//! | [`altf4`]      | F405 Alt+F4 与关机菜单 | 焦点三场景；菜单三选项默认关机；三问联动；与 × 同语义 |
//! | [`sethot`]     | F407 Win+I 设置快捷 | 三场景；单例聚焦；搜索框光标就绪；注册表登记 |
//! | [`winxmenu`]   | F408 Win+X 快捷菜单 | 九项对照表；首字母快捷；子菜单二级；打开 <1s |
//! | [`winkey`]     | F416 Win 键开合开始菜单 | 开 <150ms；焦点落搜索框；打字零丢失；三出路；连按稳定 |
//! | [`escstack`]   | F424 Esc 通用关闭语义 | 四层语义表；逐层剥离；桌面态无副作用；响应 <100ms |
//! | [`ubase`]      | 共享底盘 | Chord/HotkeyTable（F244 唯一落位）、LayerStack、RingLog、Knob |

use crate::checks::CheckSet;

pub mod altf4;
pub mod autoarrange;
pub mod escstack;
pub mod explorehot;
pub mod lockhot;
pub mod sethot;
pub mod ubase;
pub mod winkey;
pub mod winxmenu;

/// 域标识（CheckSet 聚合用）。
pub const UNI1_DOMAIN: &str = "uni1-u1";

/// 批次一已落项数。
pub const BATCH1_ITEMS: usize = 8;

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（批次二/三随落地扩列）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限
/// 由该模块自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_uni1_checks() -> CheckSet {
    let mut set = CheckSet::new(UNI1_DOMAIN);
    let blocks: [(&'static str, CheckSet); 9] = [
        ("ubase", ubase::run_ubase_checks()),
        ("F401", autoarrange::run_autoarrange_checks()),
        ("F403", lockhot::run_lockhot_checks()),
        ("F404", explorehot::run_explorehot_checks()),
        ("F405", altf4::run_altf4_checks()),
        ("F407", sethot::run_sethot_checks()),
        ("F408", winxmenu::run_winxmenu_checks()),
        ("F416", winkey::run_winkey_checks()),
        ("F424", escstack::run_escstack_checks()),
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
    fn uni1_batch1_aggregate_all_green() {
        let set = run_uni1_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "uni1 批次一自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
        assert!(!set.truncated(), "聚合器截断——扩列时必须检查容量");
    }
}
