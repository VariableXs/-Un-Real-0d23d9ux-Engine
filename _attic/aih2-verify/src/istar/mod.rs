//! Varix STAR I · 泳道四 I 通用域·四分队（AI-U4 · F551-F600）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-U4 分队的施工落位：
//! 五十项功能（F551-F600）逐项一模块，共享底盘收在 [`ibase`]（跨午夜时段、
//! 12 色标签板、重名递增命名——三项共用语义的唯一源）。纪律与 AI-K2 对齐：
//!
//! - 判据一律以《Varix STAR I start.md》主册 F551-F600 各节为准；
//! - 依赖接缝（F078 日历飞出 / F122 更新链 / F341 勿扰档 / F406 安全屏 /
//!   F361 录屏 / F235 虚拟桌面等）一律以**显式参数/闭包注入口**承接，
//!   不反向制造对未落地模块的编译依赖；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合器）
//!   与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图（批次七 F551-F575）
//!
//! | 模块 | 功能 | 批次 |
//! | --- | --- | --- |
//! | [`privconfirm`] | F551 特权操作确认窗 | 七 |
//! | [`adminrun`]    | F552 以管理员身份运行 | 七 |
//! | [`dragbadge`]   | F553 多选拖影计数 | 七 |
//! | [`dndtimer`]    | F554 定时勿扰 | 七 |
//! | [`micalm`]      | F555 麦克风降噪 | 七 |
//! | [`userredir`]   | F556 用户目录重定向 | 七 |
//! | [`migmate`]     | F557 换机迁移助手 | 七 |
//! | [`keybtest`]    | F558 键盘测试工具 | 七 |
//! | [`deadpixel`]   | F559 屏幕坏点检测 | 七 |
//! | [`holiday`]     | F560 节假日标注 | 七 |
//! | [`lnkparams`]   | F561 快捷方式参数编辑 | 七 |
//! | [`rescuedisk`]  | F562 系统恢复盘创建 | 七 |
//! | [`tvsearch`]    | F563 任务视图搜索 | 七 |
//! | [`lockclock`]   | F564 锁屏时钟样式 | 七 |
//! | [`greet`]       | F565 登录问候 | 七 |
//! | [`foldertint`]  | F566 文件夹颜色标记 | 七 |
//! | [`mutetimer`]   | F567 定时静音 | 七 |
//! | [`midclose`]    | F568 任务栏中键关闭 | 七 |
//! | [`notifyjump`]  | F569 通知点击直达 | 七 |
//! | [`showdesk`]    | F570 显示桌面按钮 | 七 |
//! | [`tabrestore`]  | F571 会话标签恢复 | 七 |
//! | [`crashbrief`]  | F572 应用崩溃简报 | 七 |
//! | [`candcount`]   | F573 候选词数量设置 | 七 |
//! | [`upsummary`]   | F574 系统更新摘要卡 | 七 |
//! | [`batch7gate`]  | F575 批次七验收锚点 | 七 |
//!
//! ## 域内模块地图（批次八 F576-F600）
//!
//! | 模块 | 功能 | 批次 |
//! | --- | --- | --- |
//! | [`tilegroup`]   | F576 磁贴分组文件夹 | 八 |
//! | [`searchchips`] | F577 搜索结果过滤片 | 八 |
//! | [`openloc`]     | F578 打开文件位置 | 八 |
//! | [`scrollhide`]  | F579 滚动条自动隐藏 | 八 |
//! | [`scrolledge`]  | F580 滚动条端点双击 | 八 |
//! | [`tabselect`]   | F581 Tab 进框全选 | 八 |
//! | [`btndebounce`] | F582 按钮防双击 | 八 |
//! | [`focusmem`]    | F583 窗口焦点记忆 | 八 |
//! | [`titletrunc`]  | F584 标题超长截断 | 八 |
//! | [`focusfollow`] | F585 焦点跟随鼠标（可选） | 八 |
//! | [`dirsize`]     | F586 文件夹大小列排序 | 八 |
//! | [`savedsearch`] | F587 保存的搜索 | 八 |
//! | [`pasteimg`]    | F588 图片粘贴为文件 | 八 |
//! | [`dropupload`]  | F589 拖拽上传 Edge | 八 |
//! | [`wallpair`]    | F590 深浅壁纸配对 | 八 |
//! | [`outdevkey`]   | F591 输出设备切换热键 | 八 |
//! | [`shotcursor`]  | F592 截图含光标开关 | 八 |
//! | [`delayshot`]   | F593 定时截图 | 八 |
//! | [`clickripple`] | F594 录屏点击高亮 | 八 |
//! | [`reshot`]      | F595 固定区域重截 | 八 |
//! | [`loginime`]    | F596 登录屏输入法 | 八 |
//! | [`desknum`]     | F597 虚拟桌面数字直达 | 八 |
//! | [`staggerboot`] | F598 自启动错峰 | 八 |
//! | [`setverify`]   | F599 升级后设置校验 | 八 |
//! | [`iregistry`]   | F600 I 域收官登记 | 八 |

use crate::checks::CheckSet;

pub mod adminrun;
pub mod batch7gate;
pub mod btndebounce;
pub mod candcount;
pub mod clickripple;
pub mod delayshot;
pub mod reshot;
pub mod focusmem;
pub mod scrolledge;
pub mod tabselect;
pub mod titletrunc;
pub mod scrollhide;
pub mod searchchips;
pub mod crashbrief;
pub mod deadpixel;
pub mod dragbadge;
pub mod greet;
pub mod dirsize;
pub mod dndtimer;
pub mod dropupload;
pub mod holiday;
pub mod ibase;
pub mod focusfollow;
pub mod foldertint;
pub mod desknum;
pub mod iregistry;
pub mod keybtest;
pub mod loginime;
pub mod setverify;
pub mod staggerboot;
pub mod lnkparams;
pub mod lockclock;
pub mod micalm;
pub mod midclose;
pub mod mutetimer;
pub mod notifyjump;
pub mod openloc;
pub mod outdevkey;
pub mod wallpair;
pub mod pasteimg;
pub mod rescuedisk;
pub mod savedsearch;
pub mod shotcursor;
pub mod tilegroup;
pub mod showdesk;
pub mod tabrestore;
pub mod migmate;
pub mod tvsearch;
pub mod upsummary;
pub mod privconfirm;
pub mod userredir;

/// 域标识（CheckSet 聚合用）。
pub const ISTAR_U4: &str = "istar-u4";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（施工期随模块落地扩列，
/// 全量 50 项 + ibase）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限时
/// 该模块自身负责裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_istar_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_U4);
    let blocks: [(&'static str, CheckSet); 51] = [
        ("ibase", ibase::run_ibase_checks()),
        ("F551", privconfirm::run_privconfirm_checks()),
        ("F552", adminrun::run_adminrun_checks()),
        ("F553", dragbadge::run_dragbadge_checks()),
        ("F554", dndtimer::run_dndtimer_checks()),
        ("F555", micalm::run_micalm_checks()),
        ("F556", userredir::run_userredir_checks()),
        ("F557", migmate::run_migmate_checks()),
        ("F558", keybtest::run_keybtest_checks()),
        ("F559", deadpixel::run_deadpixel_checks()),
        ("F560", holiday::run_holiday_checks()),
        ("F561", lnkparams::run_lnkparams_checks()),
        ("F562", rescuedisk::run_rescuedisk_checks()),
        ("F563", tvsearch::run_tvsearch_checks()),
        ("F564", lockclock::run_lockclock_checks()),
        ("F565", greet::run_greet_checks()),
        ("F566", foldertint::run_foldertint_checks()),
        ("F567", mutetimer::run_mutetimer_checks()),
        ("F568", midclose::run_midclose_checks()),
        ("F569", notifyjump::run_notifyjump_checks()),
        ("F570", showdesk::run_showdesk_checks()),
        ("F571", tabrestore::run_tabrestore_checks()),
        ("F572", crashbrief::run_crashbrief_checks()),
        ("F573", candcount::run_candcount_checks()),
        ("F574", upsummary::run_upsummary_checks()),
        ("F575", batch7gate::run_batch7gate_checks()),
        ("F576", tilegroup::run_tilegroup_checks()),
        ("F577", searchchips::run_searchchips_checks()),
        ("F578", openloc::run_openloc_checks()),
        ("F579", scrollhide::run_scrollhide_checks()),
        ("F580", scrolledge::run_scrolledge_checks()),
        ("F581", tabselect::run_tabselect_checks()),
        ("F582", btndebounce::run_btndebounce_checks()),
        ("F583", focusmem::run_focusmem_checks()),
        ("F584", titletrunc::run_titletrunc_checks()),
        ("F585", focusfollow::run_focusfollow_checks()),
        ("F586", dirsize::run_dirsize_checks()),
        ("F587", savedsearch::run_savedsearch_checks()),
        ("F588", pasteimg::run_pasteimg_checks()),
        ("F589", dropupload::run_dropupload_checks()),
        ("F590", wallpair::run_wallpair_checks()),
        ("F591", outdevkey::run_outdevkey_checks()),
        ("F592", shotcursor::run_shotcursor_checks()),
        ("F593", delayshot::run_delayshot_checks()),
        ("F594", clickripple::run_clickripple_checks()),
        ("F595", reshot::run_reshot_checks()),
        ("F596", loginime::run_loginime_checks()),
        ("F597", desknum::run_desknum_checks()),
        ("F598", staggerboot::run_staggerboot_checks()),
        ("F599", setverify::run_setverify_checks()),
        ("F600", iregistry::run_iregistry_checks()),
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
    fn istar_domain_aggregate_all_green() {
        let set = run_istar_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "ISTAR-U4 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
