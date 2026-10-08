//! F282 应用单例策略 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三策略声明读取；未声明默认单例判据；带参数转发
//! 用例（文件管理器双击文件唤醒已有窗）；任务栏与开始菜单两入口行为
//! 一致。
//!
//! **设计要点（主册）**：点击已运行应用的图标/磁贴时的行为按应用声明
//! 走三策略：单例（聚焦已有窗）、多实例（新开一窗）、单例带参数（把新
//! 请求转给已有窗，如「再打开一个文件」变成已有窗开新标签）；策略写在
//! vxapp 清单里，未声明默认单例。
//!
//! 实装：策略枚举（清单声明解析——缺省即 Single）；启动路由器（运行态
//! 查询 + 三策略分流：聚焦/新开/转发参数）；两入口同路由（任务栏与开始
//! 菜单汇入同一函数——行为一致的结构保证）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 单例策略三档（vxapp 清单声明制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchPolicy {
    /// 单例——聚焦已有窗。
    Single,
    /// 多实例——新开一窗。
    Multi,
    /// 单例带参数——新请求转发给已有窗。
    SingleWithArgs,
}

/// 清单声明解析：缺省/未知值 → Single（未声明默认单例判据）。
pub fn parse_policy(declared: Option<&str>) -> LaunchPolicy {
    match declared {
        Some("multi") => LaunchPolicy::Multi,
        Some("single-with-args") => LaunchPolicy::SingleWithArgs,
        _ => LaunchPolicy::Single,
    }
}

/// 路由结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteOutcome {
    /// 聚焦已有窗（返回 win_id）。
    FocusExisting(u32),
    /// 新开窗。
    NewWindow,
    /// 转发参数给已有窗（win_id + 参数——如「已有窗开新标签」）。
    ForwardArgs(u32, String),
}

/// 启动路由器。
pub struct LaunchRouter {
    /// 已运行窗口表：(应用, win_id)。
    running: Vec<(String, u32)>,
    next_win: u32,
}

impl LaunchRouter {
    pub fn new() -> LaunchRouter {
        LaunchRouter { running: Vec::new(), next_win: 1 }
    }

    /// 注册运行窗口（应用启动时登记）。
    pub fn register_running(&mut self, app: &str) -> u32 {
        let id = self.next_win;
        self.next_win += 1;
        self.running.push((String::from(app), id));
        id
    }

    /// 统一路由入口——任务栏与开始菜单**两入口都调这一个函数**
    /// （行为一致判据的结构保证：不存在第二条路由路径）。
    pub fn launch_click(
        &mut self,
        app: &str,
        policy: LaunchPolicy,
        args: Option<&str>,
    ) -> RouteOutcome {
        let existing = self.running.iter().find(|(a, _)| a == app).map(|(_, id)| *id);
        match (policy, existing) {
            (_, None) => {
                let id = self.register_running(app);
                RouteOutcome::FocusExisting(id) // 首启：开窗即聚焦。
            }
            (LaunchPolicy::Single, Some(id)) => RouteOutcome::FocusExisting(id),
            (LaunchPolicy::Multi, _) => RouteOutcome::NewWindow,
            (LaunchPolicy::SingleWithArgs, Some(id)) => match args {
                Some(a) => RouteOutcome::ForwardArgs(id, String::from(a)),
                None => RouteOutcome::FocusExisting(id),
            },
        }
    }

    pub fn is_running(&self, app: &str) -> bool {
        self.running.iter().any(|(a, _)| a == app)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_singleton_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F282");
    // 三策略声明读取。
    set.add(
        "F282 parse three",
        parse_policy(Some("single")) == LaunchPolicy::Single
            && parse_policy(Some("multi")) == LaunchPolicy::Multi
            && parse_policy(Some("single-with-args")) == LaunchPolicy::SingleWithArgs,
        "declared values",
    );
    // 未声明默认单例（None/乱值都归 Single）。
    set.add(
        "F282 default single",
        parse_policy(None) == LaunchPolicy::Single
            && parse_policy(Some("乱写")) == LaunchPolicy::Single,
        "default fallback",
    );
    // 三策略路由：设置中心类（单例）点两次仍一窗；记事本类（多实例）两窗。
    let mut r = LaunchRouter::new();
    let calc = r.register_running("计算器");
    let out1 = r.launch_click("计算器", LaunchPolicy::Single, None);
    let out2 = r.register_running("记事本");
    let out3 = r.launch_click("记事本", LaunchPolicy::Multi, None);
    set.add(
        "F282 single vs multi",
        out1 == RouteOutcome::FocusExisting(calc) && out3 == RouteOutcome::NewWindow,
        "focus vs new",
    );
    let _ = out2;
    // 带参数转发：文件管理器双击文件 → 已有窗开新标签。
    let fm = r.register_running("文件管理器");
    let fwd = r.launch_click("文件管理器", LaunchPolicy::SingleWithArgs, Some("vx:/报告.docx"));
    set.add(
        "F282 args forwarded",
        fwd == RouteOutcome::ForwardArgs(fm, String::from("vx:/报告.docx")),
        "wake existing window",
    );
    // 无参数的单例带参数应用 → 普通聚焦。
    let plain = r.launch_click("文件管理器", LaunchPolicy::SingleWithArgs, None);
    set.add(
        "F282 no-args focus",
        plain == RouteOutcome::FocusExisting(fm),
        "no args = focus",
    );
    // 两入口行为一致：同函数同参同结果。
    let via_taskbar = r.launch_click("计算器", LaunchPolicy::Single, None);
    let via_startmenu = r.launch_click("计算器", LaunchPolicy::Single, None);
    set.add(
        "F282 two lanes same",
        via_taskbar == via_startmenu && via_taskbar == RouteOutcome::FocusExisting(calc),
        "one router",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f282_policy_routes() {
        let set = run_singleton_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F282 自检红 {f}/{p}");
    }

    #[test]
    fn first_launch_always_focuses_new() {
        let mut r = LaunchRouter::new();
        let o = r.launch_click("新应用", LaunchPolicy::Single, None);
        assert!(matches!(o, RouteOutcome::FocusExisting(_)), "未运行时任何策略都先开窗");
    }
}
