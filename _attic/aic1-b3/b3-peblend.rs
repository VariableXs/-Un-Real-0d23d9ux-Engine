
// ---------------------------------------------------------------------------
// F002 · 深化批次三：HIGHLOW 快路径占比观测（95% 判据）+ CUI 双击终端路由编排
//
// 主册依据（G-A-02【设计细节】）：「重定位 HIGHLOW 占 95% 场景优先快路径」
// （占比观测面——快路径资格判定）；【交互设计】「CUI 子系统自动挂接终端应用
// （F012）：若从资源管理器双击控制台程序，自动新开一个终端标签页运行（关闭
// 窗口即退出进程），GUI 子系统按 F001 流程」。RelocReport/Subsystem 为既有面
// （一处一事实），本段只补编排与观测，不重复实现。
// ---------------------------------------------------------------------------

/// HIGHLOW 快路径资格线（permille 950——主册「占 95% 场景」判据）。
pub const FAST_PATH_SHARE_PERMILLE: u32 = 950;

/// HIGHLOW 占比（HIGHLOW 条目 / (HIGHLOW+DIR64)，permille）。
/// 无类型化条目 → None（无从谈占比，不猜）。
pub fn highlow_share_permille(report: &RelocReport) -> Option<u32> {
    let total = report.highlow as u64 + report.dir64 as u64;
    if total == 0 {
        return None;
    }
    Some((report.highlow as u64 * 1000 / total) as u32)
}

/// 快路径资格：HIGHLOW 占比 ≥95% 的样本走快路径（批处理合并应用，
/// 不逐条二次查节表——批处理本身由既有 apply_relocations 承载）。
pub fn fast_path_eligible(report: &RelocReport) -> bool {
    matches!(highlow_share_permille(report), Some(p) if p >= FAST_PATH_SHARE_PERMILLE)
}

/// 装载路由（F002 编排层）：GUI/CE 走 F001 桌面管线；CUI 从资源管理器双击
/// → 新开终端标签页（关闭窗口即退出进程）；CUI 从已有终端 → 附着父终端；
/// NATIVE 不进桌面流程（主册边界句之外的底层通道）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaunchRoute {
    /// F001 无感双击管线（GUI/CE 窗口）。
    GuiPipeline,
    /// 双击控制台程序 → 自动新开终端标签页（F012）。
    ConsoleNewTab,
    /// 从终端启动的控制台程序 → 附着父终端。
    ConsoleAttachedParent,
    /// NATIVE 子系统：不承诺桌面动线（诚实边界，不冒充 GUI/CUI）。
    NotPromised,
}

pub fn launch_route(subsystem: Subsystem, from_desktop: bool) -> LaunchRoute {
    if subsystem.attaches_console() {
        if from_desktop {
            LaunchRoute::ConsoleNewTab
        } else {
            LaunchRoute::ConsoleAttachedParent
        }
    } else if subsystem.desktop_launch() {
        LaunchRoute::GuiPipeline
    } else {
        LaunchRoute::NotPromised
    }
}

/// F002 深化批次三自检。
pub fn run_peblend_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F002-peblend-deep2");
    // 1) 快路径占比：19:1 = 950‰（恰好达线 eligible）；18:2 = 900‰ 不 eligible；
    //    零类型化条目 → None（不猜）。
    let mut r95 = RelocReport::default();
    r95.highlow = 19;
    r95.dir64 = 1;
    let mut r90 = RelocReport::default();
    r90.highlow = 18;
    r90.dir64 = 2;
    let r_empty = RelocReport::default();
    cs.add(
        "highlow_share_fast_path_950",
        highlow_share_permille(&r95) == Some(950)
            && fast_path_eligible(&r95)
            && highlow_share_permille(&r90) == Some(900)
            && !fast_path_eligible(&r90)
            && highlow_share_permille(&r_empty).is_none(),
        "",
    );
    // 2) 路由编排四分支：CUI 双击 → 新标签页；CUI 终端内 → 附着；GUI → F001
    //    管线；NATIVE → 不承诺（诚实边界）。
    cs.add(
        "launch_route_four_branches",
        launch_route(Subsystem::Cui, true) == LaunchRoute::ConsoleNewTab
            && launch_route(Subsystem::Cui, false) == LaunchRoute::ConsoleAttachedParent
            && launch_route(Subsystem::Gui, true) == LaunchRoute::GuiPipeline
            && launch_route(Subsystem::Native, true) == LaunchRoute::NotPromised,
        "",
    );
    // 3) FAST_PATH 线钉值 950（一处一事实锚点）。
    cs.add(
        "fast_path_threshold_pinned",
        FAST_PATH_SHARE_PERMILLE == 950,
        "",
    );
    cs
}
