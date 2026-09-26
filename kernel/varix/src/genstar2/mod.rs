//! genstar2 — Varix STAR I start · I 通用域·二分队（F451~F500 · AI-U2 分工包）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-U2 分队的施工落位：
//! 五十项功能（F451-F500）逐项一模块，一项一事实。与既有内核模块的关系
//! 纪律（与 secstar2 同源的分队落位纪律）：
//!
//! - 旧代 F 编号（AI-01..AI-20 等历史计划的 F001-F700）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有判据一律以《Varix STAR I start.md》
//!   主册为准，引用格式 `F4xx` 均指 STAR I 语义（主册第 7 部分 I 通用域）；
//! - 与 AI-U1（F401-F450）/AI-U3（F501-F550）/AI-U4（F551-F600）同泳道
//!   不共目录：分队落位互相独立，不依赖对方未注册的代码；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器，
//!   聚合器在 robust.rs 以单行注册——不占 domains 定长数组名额）与
//!   `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图（F451-F500 · 50 项）
//!
//! | 模块 | 功能 | 模块 | 功能 |
//! | --- | --- | --- | --- |
//! | [`lnkwizard`] | F451 快捷方式创建向导 | [`envedit`] | F476 环境变量编辑器 |
//! | [`foldicon`] | F452 文件夹自定义图标 | [`bootrepair`] | F477 启动修复引导 |
//! | [`thumbbadge`] | F453 缩略图角标覆盖 | [`avatar`] | F478 本机用户头像 |
//! | [`foldtmpl`] | F454 文件夹类型模板 | [`hostname`] | F479 主机名与设备名 |
//! | [`deskicons`] | F455 桌面系统图标开关 | [`chantest`] | F480 扬声器声道测试 |
//! | [`pcpage`] | F456 「此机」总览页 | [`touchpad`] | F481 触控板灵敏度与自然滚动 |
//! | [`searchmath`] | F457 搜索直出算式 | [`swapbtn`] | F482 鼠标主键交换 |
//! | [`unitconv`] | F458 搜索单位换算 | [`wheeldir`] | F483 滚轮方向独立设置 |
//! | [`webfallback`] | F459 搜索网络兜底 | [`vpnlite`] | F484 VPN 简版连接 |
//! | [`vxdict`] | F460 输入法词典导入导出 | [`sysproxy`] | F485 系统代理设置 |
//! | [`fgmute`] | F461 前台应用通知静默 | [`recentmgr`] | F486 最近文件管理 |
//! | [`setwall`] | F462 图片设为壁纸 | [`privacysweep`] | F487 隐私一键清除面板 |
//! | [`imgops`] | F463 图片轻操作 | [`appdata`] | F488 应用数据位置查看 |
//! | [`trashdrag`] | F464 回收站拖出还原 | [`shutbadge`] | F489 关机时长徽标 |
//! | [`termalias`] | F465 终端常用别名 | [`shutblock`] | F490 关机阻止管理器 |
//! | [`termclip`] | F466 终端复制粘贴 | [`autolum`] | F491 自动亮度（环境光） |
//! | [`termfont`] | F467 终端字号快捷 | [`powplan`] | F492 电源计划自定义 |
//! | [`cmdhist`] | F468 命令历史 | [`coolgov`] | F493 散热策略选择 |
//! | [`termcolor`] | F469 终端输出着色 | [`taskautohide`] | F494 任务栏自动隐藏 |
//! | [`termdir`] | F470 终端启动目录记忆 | [`taskoverflow`] | F495 任务栏溢出折叠 |
//! | [`scrollback`] | F471 终端回滚缓冲 | [`tilelongpress`] | F496 快速设置磁贴长按 |
//! | [`termtheme`] | F472 终端主题跟随 | [`ncactrow`] | F497 通知中心动作行 |
//! | [`setkbd`] | F473 设置中心键盘流 | [`dpifix`] | F498 高 DPI 模糊修复提示 |
//! | [`setdesc`] | F474 设置项说明文案 | [`lockwall`] | F499 锁屏壁纸独立 |
//! | [`idiffreg`] | F475 I 域行为差异登记册 | [`xmovkey`] | F500 窗口跨屏移动键 |

use crate::checks::CheckSet;

// --- 批次三（F451-F475） ---
pub mod cmdhist;
pub mod deskicons;
pub mod envedit;
pub mod foldicon;
pub mod foldtmpl;
pub mod fgmute;
pub mod idiffreg;
pub mod imgops;
pub mod lnkwizard;
pub mod pcpage;
pub mod scrollback;
pub mod searchmath;
pub mod setdesc;
pub mod setkbd;
pub mod setwall;
pub mod termalias;
pub mod termclip;
pub mod termcolor;
pub mod termdir;
pub mod termfont;
pub mod termtheme;
pub mod thumbbadge;
pub mod trashdrag;
pub mod unitconv;
pub mod vxdict;
pub mod webfallback;

// --- 批次四（F476-F500） ---
pub mod appdata;
pub mod autolum;
pub mod avatar;
pub mod bootrepair;
pub mod chantest;
pub mod coolgov;
pub mod dpifix;
pub mod hostname;
pub mod lockwall;
pub mod ncactrow;
pub mod powplan;
pub mod privacysweep;
pub mod recentmgr;
pub mod shutbadge;
pub mod shutblock;
pub mod swapbtn;
pub mod sysproxy;
pub mod taskautohide;
pub mod taskoverflow;
pub mod tilelongpress;
pub mod touchpad;
pub mod vpnlite;
pub mod wheeldir;
pub mod xmovkey;

/// 域标识（CheckSet 聚合用）。
pub const GENSTAR2_DOMAIN: &str = "igen-u2";

/// 本域自检聚合：逐模块一行登记（50 模块 ≤ CheckSet 容量 64，永不超容）；
/// 单模块自身超限时由该模块负责裁剪（checks.rs MAX_CHECKS 纪律）。
pub fn run_genstar2_checks() -> CheckSet {
    let mut set = CheckSet::new(GENSTAR2_DOMAIN);
    let blocks: [(&'static str, CheckSet); 50] = [
        ("F451", lnkwizard::run_lnkwizard_checks()),
        ("F452", foldicon::run_foldicon_checks()),
        ("F453", thumbbadge::run_thumbbadge_checks()),
        ("F454", foldtmpl::run_foldtmpl_checks()),
        ("F455", deskicons::run_deskicons_checks()),
        ("F456", pcpage::run_pcpage_checks()),
        ("F457", searchmath::run_searchmath_checks()),
        ("F458", unitconv::run_unitconv_checks()),
        ("F459", webfallback::run_webfallback_checks()),
        ("F460", vxdict::run_vxdict_checks()),
        ("F461", fgmute::run_fgmute_checks()),
        ("F462", setwall::run_setwall_checks()),
        ("F463", imgops::run_imgops_checks()),
        ("F464", trashdrag::run_trashdrag_checks()),
        ("F465", termalias::run_termalias_checks()),
        ("F466", termclip::run_termclip_checks()),
        ("F467", termfont::run_termfont_checks()),
        ("F468", cmdhist::run_cmdhist_checks()),
        ("F469", termcolor::run_termcolor_checks()),
        ("F470", termdir::run_termdir_checks()),
        ("F471", scrollback::run_scrollback_checks()),
        ("F472", termtheme::run_termtheme_checks()),
        ("F473", setkbd::run_setkbd_checks()),
        ("F474", setdesc::run_setdesc_checks()),
        ("F475", idiffreg::run_idiffreg_checks()),
        ("F476", envedit::run_envedit_checks()),
        ("F477", bootrepair::run_bootrepair_checks()),
        ("F478", avatar::run_avatar_checks()),
        ("F479", hostname::run_hostname_checks()),
        ("F480", chantest::run_chantest_checks()),
        ("F481", touchpad::run_touchpad_checks()),
        ("F482", swapbtn::run_swapbtn_checks()),
        ("F483", wheeldir::run_wheeldir_checks()),
        ("F484", vpnlite::run_vpnlite_checks()),
        ("F485", sysproxy::run_sysproxy_checks()),
        ("F486", recentmgr::run_recentmgr_checks()),
        ("F487", privacysweep::run_privacysweep_checks()),
        ("F488", appdata::run_appdata_checks()),
        ("F489", shutbadge::run_shutbadge_checks()),
        ("F490", shutblock::run_shutblock_checks()),
        ("F491", autolum::run_autolum_checks()),
        ("F492", powplan::run_powplan_checks()),
        ("F493", coolgov::run_coolgov_checks()),
        ("F494", taskautohide::run_taskautohide_checks()),
        ("F495", taskoverflow::run_taskoverflow_checks()),
        ("F496", tilelongpress::run_tilelongpress_checks()),
        ("F497", ncactrow::run_ncactrow_checks()),
        ("F498", dpifix::run_dpifix_checks()),
        ("F499", lockwall::run_lockwall_checks()),
        ("F500", xmovkey::run_xmovkey_checks()),
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
    fn genstar2_domain_aggregate_all_green() {
        let set = run_genstar2_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "IGEN-U2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    #[test]
    fn fifty_modules_registered() {
        let set = run_genstar2_checks();
        let (passed, failed) = set.tally();
        assert_eq!(passed + failed, 50, "五十项一行账");
    }
}
