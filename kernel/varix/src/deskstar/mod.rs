//! Varix STAR I · 泳道三 C 桌面体验域·前段（AI-D1 · F076-F092）。
//!
//! 本目录是《Varix STAR I start · AI分工完成图》AI-D1 分队的施工落位：
//! 十七项功能（F076-F092）逐项一模块，共享底盘收在 [`dbase`]（缓动
//! 曲线族、浮层生命周期状态机、防抖合并器、滑动均速、焦点环、平面
//! 几何、预算口径）。与既有内核模块的关系纪律（对齐 star/ 域既约）：
//!
//! - 旧代 F 编号（AI-01..AI-10 划分的 F001-F500）与本目录的 STAR I
//!   新编号**同号不同义**——本目录所有模块的判据一律以
//!   《Varix STAR I start.md》主册为准（锚 G-C-06 ~ G-C-22）；
//! - 跨分队依赖（F072 相对时间、F073 缩略管道、F069 性能档、F093
//!   缩略引擎等）已落地者直调公开接口（一处一事实），未落地者以
//!   **显式参数/闭包注入口**承接，不反向制造编译依赖；
//! - 每个模块自带 `run_*_checks() -> CheckSet` 自检（登记进本域聚合
//!   器）与 `#[cfg(test)]` 单元测试（宿主侧 `cargo test` 直跑）。
//!
//! ## 域内模块地图
//!
//! | 模块 | 功能 | 主册判据锚 |
//! | --- | --- | --- |
//! | [`dbase`]       | D1 共享底盘        | 曲线/浮层/防抖/均速/焦点/几何 |
//! | [`quickset`]    | F076 快速设置面板  | 六开关生效；弹出 ≤100ms；全键盘 |
//! | [`notifctr`]    | F077 通知中心      | toast→历史→直达；风暴合并；免打扰 |
//! | [`calflyout`]   | F078 日历飞出      | 万年历 12 月全对；弹出 ≤100ms |
//! | [`sndfx`]       | F079 全局音效体系  | 六事件全链；静音总闸 100% |
//! | [`snapwin`]     | F080 窗口吸附动画  | 八落点；键盘鼠标一致；帧率不掉 |
//! | [`taskview`]    | F081 任务视图      | 四桌×三窗压测；跨桌焦点正确 |
//! | [`alttab`]      | F082 Alt+Tab 现代化 | 双窗 <100ms；十二窗不乱；假死标注 |
//! | [`deskrefresh`] | F083 桌面刷新语义  | 位移零；80ms 反馈；缓存重建 |
//! | [`icongrid`]    | F084 图标拖拽网格  | 波浪换位；框选不散架；双模式 |
//! | [`trashui`]     | F085 回收站体验化  | 100 轮零损失；80fps；容量环一致 |
//! | [`copydlg`]     | F086 复制/移动进度 | 40GB 不卡；暂停续传；冲突三选 |
//! | [`conflict`]    | F087 冲突智能提示  | 30 冲突单面板；后缀 100 例；undo |
//! | [`livesearch`]  | F088 搜索即输即显  | 首结果 <300ms；防抖零扫描；高亮 |
//! | [`tabexplorer`] | F089 标签页资源管理 | 10 标签流畅；重启全恢复；拖出成窗 |
//! | [`crumbsbar`]   | F090 面包屑增强    | 拖放 20 例；补全 <50ms；零残留 |
//! | [`detailpane`]  | F091 详情窗格      | EXIF 全对；150ms 开合；统计一致 |
//! | [`zipkit`]      | F092 压缩/解压内置 | 20 枚全对；round-trip 50 例哈希 |

use crate::checks::CheckSet;

pub mod alttab;
pub mod calflyout;
pub mod copydlg;
pub mod conflict;
pub mod crumbsbar;
pub mod dbase;
pub mod deskrefresh;
pub mod detailpane;
pub mod icongrid;
pub mod livesearch;
pub mod notifctr;
pub mod quickset;
pub mod snapwin;
pub mod sndfx;
pub mod tabexplorer;
pub mod taskview;
pub mod trashui;
pub mod zipkit;

/// 域标识（CheckSet 聚合用）。
pub const DESK_DOMAIN: &str = "deskstar-d1";

/// 本域自检聚合：逐模块 `run_*_checks` 汇总（施工期随模块落地扩列，
/// 全量 dbase + 十七项）。
///
/// CheckSet 容量上限 64 条（`crate::checks::MAX_CHECKS`），单模块超限
/// 由该模块自身裁剪——聚合器如实报告每份 Set 的截断态。
pub fn run_deskstar_checks() -> CheckSet {
    let mut set = CheckSet::new(DESK_DOMAIN);
    let blocks: [(&'static str, CheckSet); 18] = [
        ("dbase", dbase::run_dbase_checks()),
        ("F076", quickset::run_quickset_checks()),
        ("F077", notifctr::run_notifctr_checks()),
        ("F078", calflyout::run_calflyout_checks()),
        ("F079", sndfx::run_sndfx_checks()),
        ("F080", snapwin::run_snapwin_checks()),
        ("F081", taskview::run_taskview_checks()),
        ("F082", alttab::run_alttab_checks()),
        ("F083", deskrefresh::run_deskrefresh_checks()),
        ("F084", icongrid::run_icongrid_checks()),
        ("F085", trashui::run_trashui_checks()),
        ("F086", copydlg::run_copydlg_checks()),
        ("F087", conflict::run_conflict_checks()),
        ("F088", livesearch::run_livesearch_checks()),
        ("F089", tabexplorer::run_tabexplorer_checks()),
        ("F090", crumbsbar::run_crumbsbar_checks()),
        ("F091", detailpane::run_detailpane_checks()),
        ("F092", zipkit::run_zipkit_checks()),
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
    fn desk_domain_aggregate_all_green() {
        let set = run_deskstar_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "DESKSTAR-D1 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
