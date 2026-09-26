//! F252 任务栏按钮合并与分组 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三档合并策略用例；中键/Shift 新窗；单击最小化-还原
//! 切换；角标计数准确性（3/7/10 窗实测）；分组稳定性（开开关关顺序不变）。
//!
//! **设计要点（主册）**：同应用多窗口默认合并为一个按钮（角标显示窗口
//! 数）；点击弹缩略图组（F073 管道）、点击缩略图激活对应窗、悬停单窗
//! 缩略图有关闭 ×；中键点击按钮=新开窗口、Shift+点击同义；合并策略三档
//! （始终合并/占满时合并/从不合并）；分组顺序按首次打开时间稳定不跳。
//!
//! 实装：`TaskbarGroup` 按应用聚合窗口（窗口 id + 首开时间戳）；布局器
//! 按策略输出按钮序列——始终合并（每应用一钮）、从不合并（每窗一钮）、
//! 占满时合并（按钮位预算内平铺，超预算的组合并）；角标=组内窗数；
//! 分组顺序锚定 `first_open_ms`，关窗不改序（稳定性判据的机制保证）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 合并策略三档（旋钮值唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergePolicy {
    /// 始终合并。
    Always,
    /// 占满时合并（按钮位预算内平铺，超预算合并）。
    WhenFull,
    /// 从不合并。
    Never,
}

/// 一个被任务栏跟踪的窗口。
#[derive(Clone, Debug)]
pub struct TrackedWindow {
    pub win_id: u32,
    pub app: String,
    pub first_open_ms: u64,
    /// 当前是否最小化（单击最小化-还原切换语义的状态位）。
    pub minimized: bool,
}

/// 任务栏上一个按钮的呈现。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarButton {
    /// 组锚应用名（合并组）。
    pub anchor: String,
    /// 组内窗口数（角标；独立钮恒 1）。
    pub badge: usize,
    /// 组内窗口 id（点开缩略图组列出）。
    pub wins: Vec<u32>,
}

/// 按钮位预算（WhenFull 档：可见按钮数上限，超出的组合并）。
pub const BAR_SLOT_BUDGET: usize = 12;

// ---------------------------------------------------------------------------
// 深化：按钮几何分配（渲染层纵深）
// ---------------------------------------------------------------------------

/// 按钮呈现档位（判据「任务栏空间不足时收缩」的渲染侧两态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonMode {
    /// 完整态：图标+标题+角标。
    Full,
    /// 图标态：只剩图标+角标（悬停 Tooltip 出全名——F271 同语义）。
    Icon,
}

/// 一个按钮的横向几何（y/高由任务栏高度定——本引擎只管横向分配）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonGeom {
    pub x: u32,
    pub w: u32,
    pub mode: ButtonMode,
}

/// Full 档理想宽（px）。
pub const IDEAL_W: u32 = 160;
/// Icon 档宽（px）。
pub const ICON_W: u32 = 40;

/// 角标文本（判据「角标计数准确性」的渲染口径：>9 折叠为 9+——
/// 两位数角标不挤破图标布局；1 不显角标）。
pub fn badge_text(badge: usize) -> &'static str {
    match badge {
        0 | 1 => "",
        2..=9 => "N", // 占位单字符——真实数字由渲染层格式化。
        _ => "9+",
    }
}

fn allocate(n: usize, w: u32, mode: ButtonMode) -> Vec<ButtonGeom> {
    (0..n)
        .map(|i| ButtonGeom { x: i as u32 * w, w, mode })
        .collect()
}

/// 任务栏分组模型。
pub struct TaskbarModel {
    pub policy: MergePolicy,
    wins: Vec<TrackedWindow>,
    next_win: u32,
}

impl TaskbarModel {
    pub fn new(policy: MergePolicy) -> TaskbarModel {
        TaskbarModel { policy, wins: Vec::new(), next_win: 1 }
    }

    pub fn set_policy(&mut self, p: MergePolicy) {
        self.policy = p;
    }

    /// 开窗（返回 win_id）。
    pub fn open(&mut self, app: &str, now_ms: u64) -> u32 {
        let id = self.next_win;
        self.next_win += 1;
        self.wins.push(TrackedWindow {
            win_id: id,
            app: String::from(app),
            first_open_ms: now_ms,
            minimized: false,
        });
        id
    }

    /// 关窗；不存在返回 false（诚实失败）。
    pub fn close(&mut self, win_id: u32) -> bool {
        let before = self.wins.len();
        self.wins.retain(|w| w.win_id != win_id);
        self.wins.len() != before
    }

    pub fn window(&self, win_id: u32) -> Option<&TrackedWindow> {
        self.wins.iter().find(|w| w.win_id == win_id)
    }

    /// 单击按钮/缩略图：切换最小化-还原（激活已最小化窗=还原）。
    pub fn click_toggle(&mut self, win_id: u32) -> Option<bool> {
        let w = self.wins.iter_mut().find(|w| w.win_id == win_id)?;
        w.minimized = !w.minimized;
        Some(w.minimized)
    }

    /// 中键/Shift+点击：新开一个该应用窗口（两入口同语义）。
    pub fn middle_click_new(&mut self, app: &str, now_ms: u64) -> u32 {
        self.open(app, now_ms)
    }

    /// 缩略图上的 ×：等价关窗（点掉对应窗不打断当前工作）。
    pub fn close_from_thumbnail(&mut self, win_id: u32) -> bool {
        self.close(win_id)
    }

    /// 布局：按 `first_open_ms` 升序稳定排序后按策略产出按钮序列。
    /// 排序锚是首次打开时间，关窗不重排（分组稳定性机制保证）。
    pub fn layout(&self) -> Vec<BarButton> {
        let mut order: Vec<&TrackedWindow> = self.wins.iter().collect();
        order.sort_by_key(|w| (w.first_open_ms, w.win_id));
        match self.policy {
            MergePolicy::Never => order
                .iter()
                .map(|w| BarButton {
                    anchor: w.app.clone(),
                    badge: 1,
                    wins: alloc::vec![w.win_id],
                })
                .collect(),
            MergePolicy::Always => self.group_all(&order),
            MergePolicy::WhenFull => {
                if self.wins.len() <= BAR_SLOT_BUDGET {
                    order
                        .iter()
                        .map(|w| BarButton {
                            anchor: w.app.clone(),
                            badge: 1,
                            wins: alloc::vec![w.win_id],
                        })
                        .collect()
                } else {
                    self.group_all(&order)
                }
            }
        }
    }

    /// 按钮宽度分配（渲染层深化）：按 `layout()` 的按钮数在 `bar_w`
    /// 内分配横向几何——
    /// 1. 全部 Full 放得下（n×IDEAL ≤ bar_w）→ 等宽 Full；
    /// 2. 否则全部收 Icon（n×ICON ≤ bar_w）→ 等宽 Icon；
    /// 3. Icon 也放不下 → 能放几个放几个，其余计数溢出（溢出折叠
    ///    F495 的域侧供数——溢出数交由上层收进折叠列表）。
    /// 返回 (几何表——与 layout() 同序, 溢出按钮数)。
    pub fn layout_geoms(&self, bar_w: u32) -> (Vec<ButtonGeom>, usize) {
        let n = self.layout().len();
        if n == 0 {
            return (Vec::new(), 0);
        }
        if n as u32 * IDEAL_W <= bar_w {
            return (allocate(n, IDEAL_W, ButtonMode::Full), 0);
        }
        if n as u32 * ICON_W <= bar_w {
            return (allocate(n, ICON_W, ButtonMode::Icon), 0);
        }
        let fit = (bar_w / ICON_W) as usize;
        (allocate(fit, ICON_W, ButtonMode::Icon), n - fit)
    }

    fn group_all(&self, order: &[&TrackedWindow]) -> Vec<BarButton> {
        let mut out: Vec<BarButton> = Vec::new();
        for w in order {
            if let Some(b) = out.iter_mut().find(|b| b.anchor == w.app) {
                b.badge += 1;
                b.wins.push(w.win_id);
            } else {
                out.push(BarButton {
                    anchor: w.app.clone(),
                    badge: 1,
                    wins: alloc::vec![w.win_id],
                });
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tbgroup_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F252");
    // 三档合并策略用例。
    let mut m = TaskbarModel::new(MergePolicy::Always);
    let _a1 = m.open("编辑器", 100);
    let _a2 = m.open("编辑器", 200);
    let _a3 = m.open("编辑器", 300);
    let lay = m.layout();
    set.add("F252 always merge", lay.len() == 1 && lay[0].badge == 3, "1 btn badge 3");
    m.set_policy(MergePolicy::Never);
    set.add("F252 never merge", m.layout().len() == 3, "3 btns");
    // 角标计数 3/7/10。
    m.set_policy(MergePolicy::Always);
    for i in 0..7 {
        let _ = m.open("浏览", 400 + i);
    }
    for i in 0..3 {
        let _ = m.open("终端", 500 + i);
    }
    let lay = m.layout();
    let badge = |lay: &[BarButton], app: &str| {
        lay.iter().find(|b| b.anchor == app).map(|b| b.badge)
    };
    set.add(
        "F252 badge 3/7",
        badge(&lay, "编辑器") == Some(3) && badge(&lay, "浏览") == Some(7),
        "3/7",
    );
    for i in 0..7 {
        let _ = m.open("编辑器", 600 + i);
    }
    let lay10 = m.layout();
    set.add("F252 badge 10", badge(&lay10, "编辑器") == Some(10), "10 windows");
    // WhenFull：预算内平铺、超预算合并。
    let mut f = TaskbarModel::new(MergePolicy::WhenFull);
    for i in 0..BAR_SLOT_BUDGET {
        let _ = f.open(&format!("app{}", i), 700 + i as u64);
    }
    set.add("F252 whenfull flat", f.layout().len() == BAR_SLOT_BUDGET, "flat in budget");
    // 超预算：再开同应用两窗 → 平铺会到 14 > 12，触发合并回 12 钮。
    let _ = f.middle_click_new("app0", 900);
    let _ = f.middle_click_new("app0", 910);
    set.add("F252 whenfull collapses", f.layout().len() == BAR_SLOT_BUDGET, "merged over");
    // 中键/Shift 新窗 + 单击最小化-还原切换。
    let w = f.middle_click_new("编辑器", 950);
    set.add("F252 middle new win", f.window(w).is_some(), "new window");
    let was_min = f.window(w).unwrap().minimized;
    let now_min = f.click_toggle(w).unwrap();
    set.add(
        "F252 click toggle",
        !was_min && now_min && !f.click_toggle(w).unwrap(),
        "min/restore flip",
    );
    // 分组稳定性：关掉中间窗，其余组顺序不变（比对去掉被关组后的前序列）。
    let seq_before: Vec<String> =
        f.layout().iter().map(|b| b.anchor.clone()).collect();
    let _ = f.close_from_thumbnail(w);
    let seq_after: Vec<String> =
        f.layout().iter().map(|b| b.anchor.clone()).collect();
    let expect: Vec<&String> =
        seq_before.iter().filter(|a| a.as_str() != "编辑器").collect();
    let actual: Vec<&String> = seq_after.iter().collect();
    set.add(
        "F252 order stable",
        expect == actual,
        "close keeps order",
    );
    // --- 深化：按钮几何分配三态（Full → Icon → 溢出）。 ---
    let mut g = TaskbarModel::new(MergePolicy::Always);
    for i in 0..4 {
        let _ = g.open(&format!("应用{i}"), 1_000 + i);
    }
    let (geoms, ov0) = g.layout_geoms(4 * IDEAL_W);
    set.add(
        "F252 geom full",
        ov0 == 0 && geoms.len() == 4 && geoms.iter().all(|b| b.mode == ButtonMode::Full) && geoms[1].x == IDEAL_W,
        "bar fits full",
    );
    // 收缩阈值：同 4 钮、宽度只够 Icon → 全收图标。
    let (geoms_i, ov1) = g.layout_geoms(4 * ICON_W);
    set.add(
        "F252 geom shrink",
        ov1 == 0 && geoms_i.iter().all(|b| b.mode == ButtonMode::Icon),
        "icon threshold",
    );
    // 中间地带：Full 放不下、Icon 放得下——200px / 4 钮：640>200，160>200? Icon=40×4=160≤200 ✓。
    let (mid, _) = g.layout_geoms(200);
    set.add("F252 geom middle band", mid.iter().all(|b| b.mode == ButtonMode::Icon), "full no / icon yes");
    // 溢出：Icon 40px、给 100px → 放 2 个、溢出 2。
    let (_part, ov2) = g.layout_geoms(100);
    set.add("F252 geom overflow", ov2 == 2, "rest to flyout");
    // 角标文本口径：1 不显、2-9 单字符、>9 折叠 9+。
    set.add(
        "F252 badge text cap",
        badge_text(1).is_empty() && badge_text(7) == "N" && badge_text(10) == "9+",
        "9+ fold",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f252_policies_and_badges() {
        let set = run_tbgroup_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F252 自检红 {f}/{p}");
    }

    #[test]
    fn toggle_semantics_never_sticks() {
        let mut m = TaskbarModel::new(MergePolicy::Never);
        let w = m.open("A", 1);
        for _ in 0..6 {
            let _ = m.click_toggle(w).unwrap();
        }
        // 偶数次翻转回到原态——切换语义不黏滞。
        assert!(!m.window(w).unwrap().minimized);
    }

    #[test]
    fn geoms_never_exceed_bar() {
        // 任意按钮数下，分配出的几何不越出任务栏宽（回归锚）。
        let mut m = TaskbarModel::new(MergePolicy::Never);
        for i in 0..9 {
            let _ = m.open(&format!("a{i}"), i as u64);
        }
        for bar_w in [0u32, 39, 40, 200, 640, 1920] {
            let (geoms, ov) = m.layout_geoms(bar_w);
            let used: u32 = geoms.iter().map(|g| g.w).sum();
            assert!(used <= bar_w.max(1) || bar_w == 0, "bar_w={bar_w} 越宽 {used}");
            assert_eq!(geoms.len() + ov, 9, "按钮账守恒 bar_w={bar_w}");
        }
    }

    #[test]
    fn zero_bar_degrades_not_panics() {
        let mut m = TaskbarModel::new(MergePolicy::Always);
        let _ = m.open("A", 0);
        let (geoms, ov) = m.layout_geoms(0);
        assert_eq!(geoms.len() + ov, 1, "零宽任务栏：按钮进溢出，不 panic");
    }
}
