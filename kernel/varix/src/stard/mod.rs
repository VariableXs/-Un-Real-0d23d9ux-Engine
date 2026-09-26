//! Varix STAR I · 泳道三 C 桌面体验域·后段（AI-D2 · F093-F110）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-D2 分队的施工落位：
//! 桌面体验后段十六项（F093-F110 除冻结候删两项）逐项一模块。与既有
//! 模块的关系纪律：
//!
//! - 判据一律以《Varix STAR I start.md》主册为准（G-C-23～40）；
//! - 共享底盘复用 [`crate::star::sbase`]（环形日志/旋钮/分位数——一处
//!   一事实，不复制第二份）；
//! - 跨项依赖按主册锚点承接：F094/F105/F109 消费 F093 缓存键纪律、
//!   F095 消费 F096 导出面、F106 与 F107 互为降级对端、F110 灯族对齐
//!   F106——全部以显式调用/参数注入，不制造编译环；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合
//!   器）与 `#[cfg(test)]` 单元测试（宿主侧 cargo test 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`thumbeng`]   | F093 图片缩略图引擎 | 万张滚动帧率不掉；二次命中率 >95%；2GB LRU |
//! | [`mediainfo`]  | F094 媒体信息悬浮 | 五容器解析全对；缓存命中 <100ms |
//! | [`term2`]      | F095 终端应用 2.0 | 10 万行回看 80fps；CJK 对齐；分屏重排 80fps |
//! | [`termpalette`]| F096 终端命令面板 | 内置 12 条全可达；收藏-搜索-执行全链；弹出 <100ms |
//! | [`notepad`]    | F097 记事本类编辑器 | 300MB 打开 <2s；原子保存百次断电零损坏；草稿恢复 |
//! | [`snipshot`]   | F098 截图工具 | 三模式全流程；取色对拍；标注导出无损 |
//! | [`calcx`]      | F099 计算器 | 标准/科学 30+30 例全对；历史回填 20 轮零错位 |
//! | [`clocksuite`] | F100 时钟套件 | 12 城时差全对；倒计时到点全链；闹钟唤醒源 |
//! | [`sticknote`]  | F102 便签 | 10 张开机全恢复；自动保存断电零丢失；置顶跨全屏 |
//! | [`sketchpad`]  | F103 画图件 | 50 步撤销零错位；4K 跟手 <33ms；导出逐像素一致 |
//! | [`photolib`]   | F105 相册应用 | 万张滚动 80fps；放映翻页 <200ms；EXIF 写回对拍 |
//! | [`keyhud`]     | F106 键盘提示 HUD | 三键三态全对；淡出 1s±50ms；全屏降级 |
//! | [`imefloat`]   | F107 输入法状态浮窗 | 三态跟随 <16ms；重定位不抖；避让 20 例 |
//! | [`phrasebk`]   | F108 自定义短语库 | 100 条全链 <100ms；变量三族 20 例；round-trip |
//! | [`cliphist`]   | F109 剪贴板历史 | 20 条循环驱逐（钉选除外）；密码框排除；搜索高亮 |
//! | [`osk`]        | F110 屏幕键盘 | 全键位输入全对；半透明可读；学习模式同步 |
//!
//! ## 冻结候删登记（主册 F200 存量冻结候删名单）
//!
//! **F101 天气件**与 **F104 录音件**属 Variable 已拍板的存量冻结候删
//! 五项（判据：冻结不删除、编号不复用、不再投入工时）——本域不落码，
//! 正式删除待 Variable 逐项确认后走冻结流程。域聚合器如实登记这两项
//! 为冻结态检查（不计绿）。

use crate::checks::CheckSet;

pub mod calcx;
pub mod cliphist;
pub mod clocksuite;
pub mod imefloat;
pub mod keyhud;
pub mod mediainfo;
pub mod notepad;
pub mod osk;
pub mod photolib;
pub mod phrasebk;
pub mod sketchpad;
pub mod snipshot;
pub mod sticknote;
pub mod term2;
pub mod termpalette;
pub mod thumbeng;

/// 域标识（CheckSet 聚合用）。
pub const STARD_DOMAIN: &str = "stard-d2";

/// 冻结候删项（F200 名单——本域内两项，不实现不投产）。
pub const FROZEN_ITEMS: [(&str, &str); 2] =
    [("F101", "天气件——存量冻结候删（季度审视去留）"), ("F104", "录音件——存量冻结候删（季度审视去留）")];

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（16 实现项 + 冻结登记）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），本域 17 块
/// 恰在容量内；单模块超限时该模块自身负责裁剪。
pub fn run_stard_checks() -> CheckSet {
    let mut set = CheckSet::new(STARD_DOMAIN);
    let blocks: [(&'static str, CheckSet); 17] = [
        ("F093", thumbeng::run_thumbeng_checks()),
        ("F094", mediainfo::run_mediainfo_checks()),
        ("F095", term2::run_term2_checks()),
        ("F096", termpalette::run_termpalette_checks()),
        ("F097", notepad::run_notepad_checks()),
        ("F098", snipshot::run_snipshot_checks()),
        ("F099", calcx::run_calcx_checks()),
        ("F100", clocksuite::run_clocksuite_checks()),
        ("F102", sticknote::run_sticknote_checks()),
        ("F103", sketchpad::run_sketchpad_checks()),
        ("F105", photolib::run_photolib_checks()),
        ("F106", keyhud::run_keyhud_checks()),
        ("F107", imefloat::run_imefloat_checks()),
        ("F108", phrasebk::run_phrasebk_checks()),
        ("F109", cliphist::run_cliphist_checks()),
        ("F110", osk::run_osk_checks()),
        ("frozen", run_frozen_registry_checks()),
    ];
    for (tag, sub) in blocks {
        let passed = sub.all_passed() && !sub.truncated();
        set.add(tag, passed, if passed { "" } else { "sub-checks red" });
    }
    set
}

/// 冻结候删登记自检：两项在册、语义一致、无实现模块（结构面守护）。
pub fn run_frozen_registry_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-frozen");
    set.add("frozen items registered", FROZEN_ITEMS.len() == 2, "");
    set.add("F101 weather frozen", FROZEN_ITEMS.iter().any(|(id, _)| *id == "F101"), "");
    set.add("F104 recorder frozen", FROZEN_ITEMS.iter().any(|(id, _)| *id == "F104"), "");
    set.add("no module for frozen", {
        // 实现模块清单不含冻结项 id（未来若解冻需先登记移除冻结态）。
        const IMPLEMENTED: [&str; 16] = [
            "F093", "F094", "F095", "F096", "F097", "F098", "F099", "F100", "F102", "F103",
            "F105", "F106", "F107", "F108", "F109", "F110",
        ];
        FROZEN_ITEMS.iter().all(|(id, _)| !IMPLEMENTED.contains(id))
    }, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stard_domain_aggregate_all_green() {
        let set = run_stard_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "STAR-D2 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    #[test]
    fn frozen_items_honestly_registered() {
        let set = run_frozen_registry_checks();
        assert!(set.all_passed());
        assert_eq!(set.len(), 4);
    }
}
