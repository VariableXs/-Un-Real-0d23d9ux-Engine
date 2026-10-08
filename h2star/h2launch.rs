//! H2 域启动编排引擎 · 深化批次二（服务层纵深——点击图标之后的全链）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F282 应用单例策略**：三策略（单例聚焦/多实例/单例带参数转发）
//!   按 vxapp 清单声明走；**未声明默认单例**；带参数转发=「文件管理
//!   器双击文件唤醒已有窗开新标签」；任务栏与开始菜单两入口行为
//!   一致（同一路由函数——单一定义点的结构保证）；
//! - **F283 应用启动骨架与首窗就绪**：三拍子（图标点击即反馈
//!   <100ms / 200ms 内窗框+骨架 / 内容就绪原位替换无跳变），超 2s
//!   显示进度原因；首窗先于次要窗；冷启动与热启动分别计账。
//!
//! 路由纪律：路由结果是纯枚举（Focus/New/Forward），真正的聚焦与
//! 转发由窗口系统执行——本引擎只管「该做什么」的判定，不碰窗口。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F282 单例策略路由
// ---------------------------------------------------------------------------

/// vxapp 清单声明的启动策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchPolicy {
    /// 单例：聚焦已有窗。
    Singleton,
    /// 多实例：每次新开。
    Multi,
    /// 单例带参数：新请求转给已有窗。
    SingletonArgs,
}

/// 路由结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 聚焦已有窗（单例）。
    FocusExisting,
    /// 新开一窗。
    NewWindow,
    /// 参数转发给已有窗（单例带参数）。
    ForwardArgs,
}

/// 已运行实例的状态注入（None=进程未在）。
#[derive(Clone, Copy, Debug)]
pub struct Existing {
    pub has_window: bool,
}

/// 策略路由（**未声明默认单例**由调用方传 `None` → Singleton——
/// 判据「未声明默认单例」的机判点）。
pub fn route(policy: Option<LaunchPolicy>, existing: &Existing) -> Route {
    match policy.unwrap_or(LaunchPolicy::Singleton) {
        LaunchPolicy::Singleton => {
            if existing.has_window {
                Route::FocusExisting
            } else {
                Route::NewWindow
            }
        }
        LaunchPolicy::Multi => Route::NewWindow,
        LaunchPolicy::SingletonArgs => {
            if existing.has_window {
                Route::ForwardArgs
            } else {
                Route::NewWindow
            }
        }
    }
}

/// 两入口一致性：任务栏与开始菜单走**同一个**路由函数——
/// 本函数是给对账自检用的显式别名（调用同一 [`route`]），
/// 「两入口行为一致」由此成为结构事实而非口头承诺。
pub fn route_from_taskbar(policy: Option<LaunchPolicy>, existing: &Existing) -> Route {
    route(policy, existing)
}

pub fn route_from_start_menu(policy: Option<LaunchPolicy>, existing: &Existing) -> Route {
    route(policy, existing)
}

// ---------------------------------------------------------------------------
// F283 三拍子节拍表
// ---------------------------------------------------------------------------

/// 三拍子阈值（ms——判据定值：<100 反馈 / 200 窗框 / 2000 门槛）。
pub const BEAT_FEEDBACK_MS: u64 = 100;
pub const BEAT_FRAME_MS: u64 = 200;
pub const BEAT_STALL_MS: u64 = 2_000;

/// 启动阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchPhase {
    /// 拍一：图标反馈（任务栏跳动）。
    Feedback,
    /// 拍二：窗框+骨架屏。
    Frame,
    /// 拍三：内容就绪（原位替换）。
    Content,
    /// 超 2s 未就绪——显示进度原因（诚实等待）。
    Stalled,
}

/// 从点击起经 `elapsed_ms` 后应处的拍位（冷启动口径）。
pub fn beat_at(elapsed_ms: u64) -> LaunchPhase {
    if elapsed_ms < BEAT_FEEDBACK_MS {
        LaunchPhase::Feedback
    } else if elapsed_ms < BEAT_FRAME_MS {
        LaunchPhase::Frame
    } else if elapsed_ms < BEAT_STALL_MS {
        LaunchPhase::Content
    } else {
        LaunchPhase::Stalled
    }
}

/// 超门槛提示（Stalled 态的人话——「正在加载插件」而非干等；
/// 原因文案由应用注入，引擎只管门槛判定与兜底文案）。
pub fn stall_hint(elapsed_ms: u64, app_reason: Option<&str>) -> Option<String> {
    if elapsed_ms < BEAT_STALL_MS {
        return None;
    }
    Some(String::from(app_reason.unwrap_or("正在启动——应用响应慢于预期")))
}

/// 热启动（进程已在，F282 路由聚焦/转发）：不走三拍子——
/// 直接就位（判据「冷启动与热启动分别计时入账」的分流点）。
pub fn is_hot_start(route: Route) -> bool {
    matches!(route, Route::FocusExisting | Route::ForwardArgs)
}

/// 首窗先于次要窗：窗口就绪序（窗口按声明的主/次排序——主窗
/// 永远 index 0 先出）。
pub fn ready_order<'a>(wins: &'a [(&'a str, bool)]) -> Vec<&'a str> {
    let mut primary: Vec<&str> = Vec::new();
    let mut secondary: Vec<&str> = Vec::new();
    for (name, is_primary) in wins {
        if *is_primary {
            primary.push(name);
        } else {
            secondary.push(name);
        }
    }
    primary.extend(secondary);
    primary
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2launch_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2launch");
    let alive = Existing { has_window: true };
    let dead = Existing { has_window: false };
    // --- F282 三策略 + 未声明默认单例。 ---
    set.add(
        "h2launch singleton",
        route(Some(LaunchPolicy::Singleton), &alive) == Route::FocusExisting
            && route(Some(LaunchPolicy::Singleton), &dead) == Route::NewWindow,
        "focus or new",
    );
    set.add(
        "h2launch multi",
        route(Some(LaunchPolicy::Multi), &alive) == Route::NewWindow,
        "always new",
    );
    set.add(
        "h2launch singleton-args",
        route(Some(LaunchPolicy::SingletonArgs), &alive) == Route::ForwardArgs,
        "args forward",
    );
    set.add(
        "h2launch undeclared default",
        route(None, &alive) == Route::FocusExisting && route(None, &dead) == Route::NewWindow,
        "default singleton",
    );
    // --- F282 两入口同路由（结构一致——同一函数）。 ---
    let same = (0..3u8)
        .all(|i| {
            let p = match i {
                0 => Some(LaunchPolicy::Singleton),
                1 => Some(LaunchPolicy::Multi),
                _ => None,
            };
            route_from_taskbar(p, &alive) == route_from_start_menu(p, &alive)
        });
    set.add("h2launch two entries", same, "taskbar==start");
    // --- F283 三拍子节拍表。 ---
    set.add(
        "h2launch beats",
        beat_at(50) == LaunchPhase::Feedback
            && beat_at(150) == LaunchPhase::Frame
            && beat_at(1_000) == LaunchPhase::Content
            && beat_at(2_100) == LaunchPhase::Stalled,
        "100/200/2000",
    );
    // 2s 门槛提示：无原因给兜底人话、有原因用应用文案。
    set.add(
        "h2launch stall hint",
        stall_hint(2_100, None).unwrap() == "正在启动——应用响应慢于预期"
            && stall_hint(2_100, Some("正在加载插件")).unwrap() == "正在加载插件"
            && stall_hint(1_999, Some("x")).is_none(),
        "honest wait",
    );
    // 热启动分流：聚焦/转发不走三拍子。
    set.add(
        "h2launch hot start",
        is_hot_start(Route::FocusExisting) && is_hot_start(Route::ForwardArgs) && !is_hot_start(Route::NewWindow),
        "no beats on hot",
    );
    // 首窗先于次要窗。
    let order = ready_order(&[("主窗", true), ("次要A", false), ("次要B", false), ("主窗B", true)]);
    set.add(
        "h2launch first window first",
        order[0] == "主窗" && order[1] == "主窗B" && order[3] == "次要B",
        "primary before secondary",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2launch_all_green() {
        let set = run_h2launch_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2launch 自检红 {f}/{p}");
    }

    #[test]
    fn calculator_vs_notepad_canonical() {
        // 判据正例钉死：计算器（单例）点两次=回到它；记事本（多实例）
        // 点两次=再开一个。
        let calc = Some(LaunchPolicy::Singleton);
        let notepad = Some(LaunchPolicy::Multi);
        let alive = Existing { has_window: true };
        assert_eq!(route(calc, &alive), Route::FocusExisting);
        assert_eq!(route(notepad, &alive), Route::NewWindow);
    }

    #[test]
    fn beats_never_skip_frames() {
        // 拍位序列连续：0→1→2→3 逐ms扫描不出现回跳。
        let mut prev = 0u8;
        for ms in 0..3_000u64 {
            let b = match beat_at(ms) {
                LaunchPhase::Feedback => 0,
                LaunchPhase::Frame => 1,
                LaunchPhase::Content => 2,
                LaunchPhase::Stalled => 3,
            };
            assert!(b >= prev, "拍位回跳 @{}ms", ms);
            prev = b;
        }
    }
}
