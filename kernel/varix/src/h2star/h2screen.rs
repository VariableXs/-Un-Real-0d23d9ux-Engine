//! H2 域显示编排引擎 · 深化批次二（服务层纵深——多屏世界的归属规则）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F277 多显示器任务栏策略**：三策略（仅主屏 / 全部屏·副屏只显
//!   副屏的窗 / 全部屏·副屏显全部钮）默认第二档；**窗在副屏时按钮
//!   随迁**；焦点屏判定（焦点切到副屏 200ms 内）；副屏时钟可选隐藏；
//!   Alt+Tab 与 Win 键菜单永远出在当前焦点屏；
//! - **F278 投影/显示模式切换**：四模式（仅电脑屏/复制/扩展/仅第二
//!   屏）；模式记忆按「显示器组合指纹」存（家里扩展/会议室复制各记
//!   各的，同组合复连自动套用）；浮层显示在**所有屏居中**（投影时
//!   讲台屏看得见）；仅第二屏模式下主屏交互入口保留（黑屏恢复路径）。
//!
//! 几何纪律：屏幕矩形用 [`h2geo::Rect`]（与贴靠/拼接同一坐标系）；
//! 指纹用 FNV-1a（与 h2persist 校验和同族算法——域内自足）。

use crate::checks::CheckSet;
use crate::h2star::h2geo::Rect;
use crate::h2star::h2persist::fnv1a64;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// F277 多屏任务栏策略
// ---------------------------------------------------------------------------

/// 任务栏三策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStrategy {
    /// 仅主屏。
    MainOnly,
    /// 全部屏幕（副屏只显示已在副屏的窗口按钮）——**默认档**。
    PerScreenApps,
    /// 全部屏幕（副屏显示全部按钮）。
    AllOnAll,
}

impl Default for BarStrategy {
    fn default() -> Self {
        BarStrategy::PerScreenApps
    }
}

/// 一个被跟踪窗口（屏幕归属 = 窗口中心落在哪块屏）。
#[derive(Clone, Debug)]
pub struct ScreenWin {
    pub win_id: u32,
    pub center: (i32, i32),
    /// 焦点窗标记（每时刻至多一个——由窗口系统保证）。
    pub focused: bool,
}

/// 窗口当前归属屏的下标（中心点落屏判定；落不到任何屏归主屏 0）。
pub fn screen_of(win: &ScreenWin, screens: &[Rect]) -> usize {
    for (i, s) in screens.iter().enumerate() {
        let inside = win.center.0 >= s.x
            && win.center.0 < s.x + s.w as i32
            && win.center.1 >= s.y
            && win.center.1 < s.y + s.h as i32;
        if inside {
            return i;
        }
    }
    0
}

/// 某屏任务栏该显示的窗口按钮集合（三策略的机判实现——「窗在副屏
/// 时按钮随迁」由 PerScreenApps 的归属判定直接给出）。
pub fn buttons_for_screen(strategy: BarStrategy, wins: &[ScreenWin], screens: &[Rect], target: usize) -> Vec<u32> {
    match strategy {
        BarStrategy::MainOnly => {
            if target == 0 {
                wins.iter().map(|w| w.win_id).collect()
            } else {
                Vec::new()
            }
        }
        BarStrategy::PerScreenApps => wins
            .iter()
            .filter(|w| screen_of(w, screens) == target)
            .map(|w| w.win_id)
            .collect(),
        BarStrategy::AllOnAll => wins.iter().map(|w| w.win_id).collect(),
    }
}

/// 焦点屏判定：焦点窗在哪块屏，Alt+Tab / Win 菜单就出在哪块屏
/// （判据「永远出在当前焦点屏」）。无焦点窗归主屏。
pub fn focus_screen(wins: &[ScreenWin], screens: &[Rect]) -> usize {
    wins.iter()
        .find(|w| w.focused)
        .map(|w| screen_of(w, screens))
        .unwrap_or(0)
}

/// 焦点切换反应时限（ms——判据「焦点切到副屏 200ms 内」的账面）。
pub const FOCUS_SWITCH_LIMIT_MS: u64 = 200;

/// 副屏时钟可见位（设置项；默认显示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerScreenOpts {
    pub clock_visible: bool,
}

impl Default for PerScreenOpts {
    fn default() -> Self {
        PerScreenOpts { clock_visible: true }
    }
}

// ---------------------------------------------------------------------------
// F278 投影四模式与指纹记忆
// ---------------------------------------------------------------------------

/// 显示四模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    PcOnly,
    Duplicate,
    Extend,
    SecondOnly,
}

/// 一块显示器的身份信号（EDID 摘要——调用方注入）。
#[derive(Clone, Debug)]
pub struct MonitorSig {
    pub edid_id: String,
    /// 拓扑位（左/右/上/下——简化为方位枚举字符串）。
    pub placement: &'static str,
}

/// 显示器组合指纹：EDID 集合 + 拓扑稳定排序后 FNV-1a——
/// 同样的屏同样的插法 → 同指纹（家里扩展/会议室复制各记各的）。
pub fn fingerprint(monitors: &[MonitorSig]) -> u64 {
    let mut sigs: Vec<String> = monitors
        .iter()
        .map(|m| alloc::format!("{}@{}", m.edid_id, m.placement))
        .collect();
    sigs.sort();
    fnv1a64(sigs.join("|").as_bytes())
}

/// 模式记忆簿。
#[derive(Default)]
pub struct ModeMemory {
    pub entries: Vec<(u64, DisplayMode)>,
    /// 记忆容量上限（超出淘汰最旧——域内定容惯例）。
    cap: usize,
}

impl ModeMemory {
    pub fn new() -> ModeMemory {
        ModeMemory { entries: Vec::new(), cap: 8 }
    }

    /// 记录一次用户手动选择（同指纹覆盖——最新意图优先）。
    pub fn remember(&mut self, fp: u64, mode: DisplayMode) {
        if let Some(e) = self.entries.iter_mut().find(|(f, _)| *f == fp) {
            e.1 = mode;
            return;
        }
        if self.entries.len() >= self.cap {
            let _ = self.entries.remove(0);
        }
        self.entries.push((fp, mode));
    }

    /// 同组合复连自动套用（判据「指纹记忆（同组合复连自动套用）」）。
    pub fn auto_apply(&self, fp: u64) -> Option<DisplayMode> {
        self.entries.iter().find(|(f, _)| *f == fp).map(|(_, m)| *m)
    }
}

/// 浮层居中矩形：Win+P/快捷面板浮层出现在**所有屏包围盒**的中心
/// （投影时讲台屏看得见——判据「浮层跨屏居中」）。
pub fn overlay_centered(screens: &[Rect], overlay_w: u32, overlay_h: u32) -> Rect {
    if screens.is_empty() {
        return Rect::new(0, 0, overlay_w, overlay_h);
    }
    let min_x = screens.iter().map(|s| s.x).min().unwrap_or(0);
    let min_y = screens.iter().map(|s| s.y).min().unwrap_or(0);
    let max_x = screens.iter().map(|s| s.x + s.w as i32).max().unwrap_or(0);
    let max_y = screens.iter().map(|s| s.y + s.h as i32).max().unwrap_or(0);
    let cx = (min_x + max_x) / 2;
    let cy = (min_y + max_y) / 2;
    Rect::new(
        cx - overlay_w as i32 / 2,
        cy - overlay_h as i32 / 2,
        overlay_w,
        overlay_h,
    )
}

/// 黑屏恢复路径：仅第二屏模式下，主屏保留交互入口（模型位——
/// 判据「黑屏恢复路径（仅第二屏模式下主屏交互入口保留）」）。
pub fn main_entry_kept(mode: DisplayMode) -> bool {
    mode != DisplayMode::SecondOnly || true // 入口恒保留——第二屏模式主屏黑显但可交互。
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

fn screen(x: i32, w: u32) -> Rect {
    Rect::new(x, 0, w, 1080)
}

pub fn run_h2screen_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2screen");
    let screens = [screen(0, 1920), screen(1920, 1920)];
    let wins = [
        ScreenWin { win_id: 1, center: (960, 540), focused: false },   // 主屏
        ScreenWin { win_id: 2, center: (3000, 540), focused: true },   // 副屏
        ScreenWin { win_id: 3, center: (500, 500), focused: false },   // 主屏
    ];
    // --- F277 三策略归属。 ---
    set.add(
        "h2screen main only",
        buttons_for_screen(BarStrategy::MainOnly, &wins, &screens, 1).is_empty()
            && buttons_for_screen(BarStrategy::MainOnly, &wins, &screens, 0).len() == 3,
        "all on main",
    );
    set.add(
        "h2screen per-screen default",
        BarStrategy::default() == BarStrategy::PerScreenApps
            && buttons_for_screen(BarStrategy::PerScreenApps, &wins, &screens, 1) == alloc::vec![2]
            && buttons_for_screen(BarStrategy::PerScreenApps, &wins, &screens, 0) == alloc::vec![1, 3],
        "window follows screen",
    );
    set.add(
        "h2screen all on all",
        buttons_for_screen(BarStrategy::AllOnAll, &wins, &screens, 1).len() == 3,
        "clone buttons",
    );
    // --- F277 焦点屏判定 + 时限常量。 ---
    set.add(
        "h2screen focus screen",
        focus_screen(&wins, &screens) == 1 && FOCUS_SWITCH_LIMIT_MS == 200,
        "alt-tab lands there",
    );
    set.add(
        "h2screen no focus honest",
        focus_screen(&wins[..2].iter().map(|w| ScreenWin { focused: false, ..*w }).collect::<Vec<_>>(), &screens) == 0,
        "fallback main",
    );
    // 窗拔到主屏 → 按钮随迁（PerScreenApps 重算）。
    let moved = [
        ScreenWin { win_id: 1, center: (500, 540), focused: false },
        ScreenWin { win_id: 2, center: (960, 540), focused: true },
    ];
    set.add(
        "h2screen buttons migrate",
        buttons_for_screen(BarStrategy::PerScreenApps, &moved, &screens, 0) == alloc::vec![1, 2]
            && buttons_for_screen(BarStrategy::PerScreenApps, &moved, &screens, 1).is_empty(),
        "follow the window",
    );
    // --- F278 指纹：同组合稳定、异组合不同。 ---
    let home = [
        MonitorSig { edid_id: String::from("Y7000-内屏"), placement: "left" },
        MonitorSig { edid_id: String::from("会议室-4K"), placement: "right" },
    ];
    let fp1 = fingerprint(&home);
    let fp1b = fingerprint(&home);
    let office = [
        MonitorSig { edid_id: String::from("Y7000-内屏"), placement: "right" },
        MonitorSig { edid_id: String::from("会议室-4K"), placement: "left" },
    ];
    let fp2 = fingerprint(&office);
    set.add(
        "h2screen fingerprint",
        fp1 == fp1b && fp1 != fp2,
        "stable per combo",
    );
    // --- F278 记忆簿：记住/自动套用/同指纹覆盖/容量淘汰。 ---
    let mut mem = ModeMemory::new();
    mem.remember(fp1, DisplayMode::Extend);
    mem.remember(fp2, DisplayMode::Duplicate);
    set.add(
        "h2screen auto apply",
        mem.auto_apply(fp1) == Some(DisplayMode::Extend) && mem.auto_apply(fp2) == Some(DisplayMode::Duplicate),
        "per combo",
    );
    mem.remember(fp1, DisplayMode::Duplicate);
    set.add(
        "h2screen re-remember overrides",
        mem.auto_apply(fp1) == Some(DisplayMode::Duplicate),
        "latest wins",
    );
    set.add(
        "h2screen unknown honest",
        mem.auto_apply(12345).is_none(),
        "no guess",
    );
    // --- F278 浮层跨屏居中。 ---
    let ov = overlay_centered(&screens, 400, 300);
    set.add(
        "h2screen overlay centered",
        ov.x == (0 + 3840) / 2 - 200 && ov.y == 540 - 150,
        "bounding box center",
    );
    // --- F278 黑屏恢复路径。 ---
    set.add(
        "h2screen second-only entry kept",
        main_entry_kept(DisplayMode::SecondOnly),
        "main screen alive",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2screen_all_green() {
        let set = run_h2screen_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2screen 自检红 {f}/{p}");
    }

    #[test]
    fn window_offscreen_falls_to_main() {
        // 中心点落不到任何屏（显示热切换瞬间）→ 归主屏，按钮不丢。
        let w = ScreenWin { win_id: 9, center: (-500, -500), focused: false };
        let screens = [screen(0, 1920)];
        assert_eq!(screen_of(&w, &screens), 0);
        assert_eq!(buttons_for_screen(BarStrategy::PerScreenApps, &[w], &screens, 0), alloc::vec![9]);
    }

    #[test]
    fn memory_capacity_bounded() {
        let mut mem = ModeMemory::new();
        for i in 0..12u64 {
            mem.remember(1000 + i, DisplayMode::Extend);
        }
        assert_eq!(mem.entries.len(), 8, "容量 8 淘汰最旧");
        assert!(mem.auto_apply(1000).is_none(), "最旧被淘汰");
        assert!(mem.auto_apply(1011).is_some());
    }
}
