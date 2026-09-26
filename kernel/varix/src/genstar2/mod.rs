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
        ("F451", { let a = lnkwizard::run_lnkwizard_checks(); let b = lnkwizard::run_lnkwizard_deep_checks(); CheckSet::merge(a, b) }),
        ("F452", { let a = foldicon::run_foldicon_checks(); let b = foldicon::run_foldicon_deep_checks(); CheckSet::merge(a, b) }),
        ("F453", { let a = thumbbadge::run_thumbbadge_checks(); let b = thumbbadge::run_thumbbadge_deep_checks(); CheckSet::merge(a, b) }),
        ("F454", { let a = foldtmpl::run_foldtmpl_checks(); let b = foldtmpl::run_foldtmpl_deep_checks(); CheckSet::merge(a, b) }),
        ("F455", { let a = deskicons::run_deskicons_checks(); let b = deskicons::run_deskicons_deep_checks(); CheckSet::merge(a, b) }),
        ("F456", { let a = pcpage::run_pcpage_checks(); let b = pcpage::run_pcpage_deep_checks(); CheckSet::merge(a, b) }),
        ("F457", { let a = searchmath::run_searchmath_checks(); let c = searchmath::run_searchmath_v3_checks(); CheckSet::merge(a, c) }),
        ("F458", { let a = unitconv::run_unitconv_checks(); let b = unitconv::run_unitconv_deep_checks(); let c = unitconv::run_unitconv_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F459", { let a = webfallback::run_webfallback_checks(); let b = webfallback::run_webfallback_deep_checks(); let c = webfallback::run_webfallback_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F460", { let a = vxdict::run_vxdict_checks(); let b = vxdict::run_vxdict_deep_checks(); let c = vxdict::run_vxdict_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F461", { let a = fgmute::run_fgmute_checks(); let b = fgmute::run_fgmute_deep_checks(); let c = fgmute::run_fgmute_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F462", { let a = setwall::run_setwall_checks(); let b = setwall::run_setwall_deep_checks(); let c = setwall::run_setwall_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F463", { let a = imgops::run_imgops_checks(); let b = imgops::run_imgops_deep_checks(); let c = imgops::run_imgops_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F464", { let a = trashdrag::run_trashdrag_checks(); let b = trashdrag::run_trashdrag_deep_checks(); let c = trashdrag::run_trashdrag_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F465", { let a = termalias::run_termalias_checks(); let b = termalias::run_termalias_deep_checks(); CheckSet::merge(a, b) }),
        ("F466", { let a = termclip::run_termclip_checks(); let b = termclip::run_termclip_deep_checks(); let c = termclip::run_termclip_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F467", { let a = termfont::run_termfont_checks(); let b = termfont::run_termfont_deep_checks(); let c = termfont::run_termfont_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F468", { let a = cmdhist::run_cmdhist_checks(); let b = cmdhist::run_cmdhist_deep_checks(); let c = cmdhist::run_cmdhist_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F469", { let a = termcolor::run_termcolor_checks(); let d = termcolor::run_termcolor_v4_checks(); CheckSet::merge(a, d) }),
        ("F470", { let a = termdir::run_termdir_checks(); let b = termdir::run_termdir_deep_checks(); let c = termdir::run_termdir_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F471", { let a = scrollback::run_scrollback_checks(); let b = scrollback::run_scrollback_deep_checks(); let c = scrollback::run_scrollback_v4_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F472", { let a = termtheme::run_termtheme_checks(); let b = termtheme::run_termtheme_deep_checks(); CheckSet::merge(a, b) }),
        ("F473", { let a = setkbd::run_setkbd_checks(); let b = setkbd::run_setkbd_deep_checks(); CheckSet::merge(a, b) }),
        ("F474", { let a = setdesc::run_setdesc_checks(); let b = setdesc::run_setdesc_deep_checks(); CheckSet::merge(a, b) }),
        ("F475", { let a = idiffreg::run_idiffreg_checks(); let b = idiffreg::run_idiffreg_deep_checks(); CheckSet::merge(a, b) }),
        ("F476", { let a = envedit::run_envedit_checks(); let b = envedit::run_envedit_deep_checks(); let c = envedit::run_envedit_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F477", { let a = bootrepair::run_bootrepair_checks(); let c = bootrepair::run_bootrepair_v3_checks(); CheckSet::merge(a, c) }),
        ("F478", { let a = avatar::run_avatar_checks(); let b = avatar::run_avatar_deep_checks(); CheckSet::merge(a, b) }),
        ("F479", { let a = hostname::run_hostname_checks(); let b = hostname::run_hostname_deep_checks(); CheckSet::merge(a, b) }),
        ("F480", { let a = chantest::run_chantest_checks(); let b = chantest::run_chantest_deep_checks(); let c = chantest::run_chantest_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F481", { let a = touchpad::run_touchpad_checks(); let b = touchpad::run_touchpad_deep_checks(); CheckSet::merge(a, b) }),
        ("F482", { let a = swapbtn::run_swapbtn_checks(); let b = swapbtn::run_swapbtn_deep_checks(); CheckSet::merge(a, b) }),
        ("F483", { let a = wheeldir::run_wheeldir_checks(); let b = wheeldir::run_wheeldir_deep_checks(); CheckSet::merge(a, b) }),
        ("F484", { let a = vpnlite::run_vpnlite_checks(); let c = vpnlite::run_vpnlite_v3_checks(); CheckSet::merge(a, c) }),
        ("F485", { let a = sysproxy::run_sysproxy_checks(); let c = sysproxy::run_sysproxy_v3_checks(); CheckSet::merge(a, c) }),
        ("F486", { let a = recentmgr::run_recentmgr_checks(); let b = recentmgr::run_recentmgr_deep_checks(); CheckSet::merge(a, b) }),
        ("F487", { let a = privacysweep::run_privacysweep_checks(); let b = privacysweep::run_privacysweep_deep_checks(); CheckSet::merge(a, b) }),
        ("F488", { let a = appdata::run_appdata_checks(); let b = appdata::run_appdata_deep_checks(); CheckSet::merge(a, b) }),
        ("F489", { let a = shutbadge::run_shutbadge_checks(); let b = shutbadge::run_shutbadge_deep_checks(); CheckSet::merge(a, b) }),
        ("F490", { let a = shutblock::run_shutblock_checks(); let b = shutblock::run_shutblock_deep_checks(); CheckSet::merge(a, b) }),
        ("F491", { let a = autolum::run_autolum_checks(); let b = autolum::run_autolum_deep_checks(); let c = autolum::run_autolum_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F492", { let a = powplan::run_powplan_checks(); let c = powplan::run_powplan_v3_checks(); CheckSet::merge(a, c) }),
        ("F493", { let a = coolgov::run_coolgov_checks(); let b = coolgov::run_coolgov_deep_checks(); CheckSet::merge(a, b) }),
        ("F494", { let a = taskautohide::run_taskautohide_checks(); let b = taskautohide::run_taskautohide_deep_checks(); CheckSet::merge(a, b) }),
        ("F495", { let a = taskoverflow::run_taskoverflow_checks(); let b = taskoverflow::run_taskoverflow_deep_checks(); CheckSet::merge(a, b) }),
        ("F496", { let a = tilelongpress::run_tilelongpress_checks(); let b = tilelongpress::run_tilelongpress_deep_checks(); CheckSet::merge(a, b) }),
        ("F497", { let a = ncactrow::run_ncactrow_checks(); let b = ncactrow::run_ncactrow_deep_checks(); CheckSet::merge(a, b) }),
        ("F498", { let a = dpifix::run_dpifix_checks(); let b = dpifix::run_dpifix_deep_checks(); let c = dpifix::run_dpifix_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F499", { let a = lockwall::run_lockwall_checks(); let b = lockwall::run_lockwall_deep_checks(); let c = lockwall::run_lockwall_v3_checks(); CheckSet::merge(CheckSet::merge(a, b), c) }),
        ("F500", { let a = xmovkey::run_xmovkey_checks(); let b = xmovkey::run_xmovkey_deep_checks(); CheckSet::merge(a, b) }),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        if !passed {
            #[cfg(test)]
            {
                let (items, n) = sub.red_items();
                for i in 0..n {
                    if let Some(c) = items[i] {
                        if !c.passed {
                            eprintln!("AGG RED {}::{}", tag, c.name);
                        }
                    }
                }
            }
        }
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
