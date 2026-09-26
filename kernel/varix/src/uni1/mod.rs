//! Varix STAR I · 泳道四 I 通用域·一分队（AI-U1 · F401-F450 全域收官）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-U1 分队的施工落位：
//! **F401-F450 五十项全部落地**（批次一 F401-F425 前段 + 批次二
//! F426-F450 后段，本文件随批次二收官合并登记）。共享底盘收在
//! [`ubase`]（组合键注册表、浮层层级栈、事件环、旋钮——F244 键位
//! 注册唯一落位）。与既有模块的关系纪律：
//!
//! - 判据一律以《Varix STAR I start.md》主册 I-1 深化设计报告为准，
//!   引用格式 `F4xx` 均指 STAR I 语义（与旧代 F 编号同号不同义）；
//! - 跨队依赖一律以显式参数/闭包注入口承接（对本仓库其它分队目录
//!   **零引用**——跨泳道借力 = 0）；
//! - 每模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧直跑）；
//! - 聚合器 51 块（ubase + F401-F450），容量 64 内——随收官冻结。
//!
//! ## 全域模块地图（50 项 + 底盘）
//!
//! | 模块 | 功能 | 模块 | 功能 |
//! | --- | --- | --- | --- |
//! | [`autoarrange`]| F401 桌面自动排列 | [`docskeys`] | F426 文档键位保存族 |
//! | [`taskmhot`]   | F402 任务管理器快捷 | [`clipkeys`] | F427 Ctrl+X/C/V 语义表 |
//! | [`lockhot`]    | F403 Win+L 锁屏快捷 | [`printkey`] | F428 Ctrl+P 打印链路 |
//! | [`explorehot`] | F404 Win+E 资源管理器 | [`fullscreen`] | F429 F11 全屏模式 |
//! | [`altf4`]      | F405 Alt+F4 关机菜单 | [`zoomwheel`] | F430 Ctrl+滚轮缩放 |
//! | [`secscr`]     | F406 安全屏（简版） | [`newfolder`] | F431 Ctrl+Shift+N 新建 |
//! | [`sethot`]     | F407 Win+I 设置快捷 | [`listnav`] | F432 列表翻页与定位键 |
//! | [`winxmenu`]   | F408 Win+X 快捷菜单 | [`kbdmenu`] | F433 Shift+F10 键盘右键 |
//! | [`ctxhelp`]    | F409 F1 上下文帮助 | [`dlgkeys`] | F434 对话框控件键位 |
//! | [`enterkey`]   | F410 Enter 打开/新窗 | [`dropdown`] | F435 下拉框键盘操作 |
//! | [`backnav`]    | F411 Backspace 上级 | [`sliderkeys`] | F436 滑杆键盘操作 |
//! | [`altrprop`]   | F412 Alt+Enter 属性 | [`fmtdisk`] | F437 磁盘格式化工具 |
//! | [`prtsrc`]     | F413 PrintScreen 接入 | [`diskmnt`] | F438 盘符与挂载管理 |
//! | [`dragtrash`]  | F414 拖拽进回收站 | [`drvcrypt`] | F439 驱动器加密 |
//! | [`trashicon`]  | F415 回收站满空两态 | [`isomount`] | F440 ISO 镜像挂载 |
//! | [`winkey`]     | F416 Win 键开合开始菜单 | [`schedtask`] | F441 计划任务创建 |
//! | [`tilegrid`]   | F417 开始菜单磁贴交互 | [`restpoint`] | F442 手动创建还原点 |
//! | [`pinbar`]     | F418 任务栏固定应用 | [`btpair`] | F443 蓝牙配对流程 |
//! | [`barctx`]     | F419 任务栏右键菜单 | [`ptrsetup`] | F444 打印机安装向导 |
//! | [`clockctx`]   | F420 时钟右键快捷 | [`disparrange`] | F445 显示器排列拖拽 |
//! | [`imeind`]     | F421 输入法指示器点击 | [`ressel`] | F446 分辨率与刷新率 |
//! | [`volfly`]     | F422 音量图标浮层 | [`sndaudition`] | F447 事件声音试听 |
//! | [`batfly`]     | F423 电池图标浮层 | [`mictest`] | F448 麦克风测试向导 |
//! | [`escstack`]   | F424 Esc 通用关闭语义 | [`camtest`] | F449 摄像头预览测试 |
//! | [`shake`]      | F425 Aero Shake 晃动 | [`stickkeys`] | F450 粘滞键与筛选键 |
//! | [`ubase`]      | 共享底盘：Chord/HotkeyTable（F244 唯一落位）、LayerStack、RingLog、Knob | | |

use crate::checks::CheckSet;

pub mod altrprop;
pub mod altf4;
pub mod autoarrange;
pub mod backnav;
pub mod barctx;
pub mod batfly;
pub mod btpair;
pub mod camtest;
pub mod clipkeys;
pub mod clockctx;
pub mod ctxhelp;
pub mod dlgkeys;
pub mod docskeys;
pub mod dragtrash;
pub mod diskmnt;
pub mod disparrange;
pub mod dropdown;
pub mod drvcrypt;
pub mod enterkey;
pub mod escstack;
pub mod explorehot;
pub mod fmtdisk;
pub mod fullscreen;
pub mod imeind;
pub mod isomount;
pub mod kbdmenu;
pub mod listnav;
pub mod mictest;
pub mod lockhot;
pub mod newfolder;
pub mod prtsrc;
pub mod pinbar;
pub mod printkey;
pub mod restpoint;
pub mod ressel;
pub mod schedtask;
pub mod secscr;
pub mod sethot;
pub mod shake;
pub mod sliderkeys;
pub mod sndaudition;
pub mod stickkeys;
pub mod taskmhot;
pub mod tilegrid;
pub mod trashicon;
pub mod ptrsetup;
pub mod ubase;
pub mod volfly;
pub mod winkey;
pub mod winxmenu;
pub mod zoomwheel;

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
    let blocks: [(&'static str, CheckSet); 51] = [
        ("ubase", ubase::run_ubase_checks()),
        ("F401", autoarrange::run_autoarrange_checks()),
        ("F402", taskmhot::run_taskmhot_checks()),
        ("F403", lockhot::run_lockhot_checks()),
        ("F404", explorehot::run_explorehot_checks()),
        ("F405", altf4::run_altf4_checks()),
        ("F406", secscr::run_secscr_checks()),
        ("F407", sethot::run_sethot_checks()),
        ("F408", winxmenu::run_winxmenu_checks()),
        ("F409", ctxhelp::run_ctxhelp_checks()),
        ("F410", enterkey::run_enterkey_checks()),
        ("F411", backnav::run_backnav_checks()),
        ("F412", altrprop::run_altrprop_checks()),
        ("F413", prtsrc::run_prtsrc_checks()),
        ("F414", dragtrash::run_dragtrash_checks()),
        ("F415", trashicon::run_trashicon_checks()),
        ("F416", winkey::run_winkey_checks()),
        ("F417", tilegrid::run_tilegrid_checks()),
        ("F418", pinbar::run_pinbar_checks()),
        ("F419", barctx::run_barctx_checks()),
        ("F420", clockctx::run_clockctx_checks()),
        ("F421", imeind::run_imeind_checks()),
        ("F422", volfly::run_volfly_checks()),
        ("F423", batfly::run_batfly_checks()),
        ("F424", escstack::run_escstack_checks()),
        ("F425", shake::run_shake_checks()),
        ("F426", docskeys::run_docskeys_checks()),
        ("F427", clipkeys::run_clipkeys_checks()),
        ("F428", printkey::run_printkey_checks()),
        ("F429", fullscreen::run_fullscreen_checks()),
        ("F430", zoomwheel::run_zoomwheel_checks()),
        ("F431", newfolder::run_newfolder_checks()),
        ("F432", listnav::run_listnav_checks()),
        ("F433", kbdmenu::run_kbdmenu_checks()),
        ("F434", dlgkeys::run_dlgkeys_checks()),
        ("F435", dropdown::run_dropdown_checks()),
        ("F436", sliderkeys::run_sliderkeys_checks()),
        ("F437", fmtdisk::run_fmtdisk_checks()),
        ("F438", diskmnt::run_diskmnt_checks()),
        ("F439", drvcrypt::run_drvcrypt_checks()),
        ("F440", isomount::run_isomount_checks()),
        ("F441", schedtask::run_schedtask_checks()),
        ("F442", restpoint::run_restpoint_checks()),
        ("F443", btpair::run_btpair_checks()),
        ("F444", ptrsetup::run_ptrsetup_checks()),
        ("F445", disparrange::run_disparrange_checks()),
        ("F446", ressel::run_ressel_checks()),
        ("F447", sndaudition::run_sndaudition_checks()),
        ("F448", mictest::run_mictest_checks()),
        ("F449", camtest::run_camtest_checks()),
        ("F450", stickkeys::run_stickkeys_checks()),
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
