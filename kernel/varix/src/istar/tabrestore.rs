//! F571 会话标签恢复 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：标签组/栈/滚动三恢复精度；恢复通知条；精简选项；
//! 崩溃恢复注入；上限（8 标签）。
//!
//! **设计要点（主册）**：
//! - 资源管理器多标签（F271）会话恢复：重启/重开后自动还原上次标签组
//!   （每个标签的目录/历史栈 F266/滚动位置——「工作现场」整体回归）；
//! - 恢复前通知条（「已恢复 4 个标签——全部保留/只留当前」可选精简）；
//! - 崩溃后同样恢复（F311 纪律延伸）；恢复的是「工作流」不只是路径；
//! - 上限 8 标签。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 标签上限。
pub const TAB_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一个标签的完整现场（目录 + 历史栈 + 滚动位——三恢复精度对象）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabScene {
    pub dir: String,
    /// 历史栈（F266——含当前位，恢复后前进/后退都活着）。
    pub history: Vec<String>,
    /// 滚动位置（px）。
    pub scroll_px: u32,
}

/// 会话快照（关窗/崩溃时落盘形态——模型面即内存账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    tabs: Vec<TabScene>,
    /// 当前标签序。
    pub active: usize,
}

impl Session {
    pub fn capture(tabs: Vec<TabScene>, active: usize) -> Session {
        let n = tabs.len().min(TAB_CAP);
        Session {
            tabs: tabs.into_iter().take(TAB_CAP).collect(),
            active: active.min(n.saturating_sub(1)),
        }
    }

    pub fn tabs(&self) -> &[TabScene] {
        &self.tabs
    }
}

/// 恢复引擎。
pub struct TabRestore {
    /// 上次会话快照（None = 无历史）。
    saved: Option<Session>,
    /// 恢复后的现场（渲染层取数口；None = 未恢复）。
    restored_scene: Option<Session>,
    /// 恢复后的通知条（None = 未恢复/已处理）。
    notice: Option<(usize, &'static str)>,
    /// 精简动作已执行（只留当前）。
    trimmed: bool,
    /// 恢复事件来源账（normal/crash）。
    source: &'static str,
}

impl TabRestore {
    pub fn new() -> TabRestore {
        TabRestore {
            saved: None,
            restored_scene: None,
            notice: None,
            trimmed: false,
            source: "",
        }
    }

    /// 会话落账（正常关窗时——F311 纪律：退出前写快照）。
    pub fn save(&mut self, s: Session) {
        self.saved = Some(s);
    }

    pub fn has_snapshot(&self) -> bool {
        self.saved.is_some()
    }

    /// 恢复（重启后首开）：三精度整体回归 + 通知条。
    /// 来源：normal（正常重开）或 crash（崩溃恢复注入）。
    pub fn restore(&mut self, source: &'static str) -> Option<(usize, &'static str)> {
        let s = self.saved.take()?;
        let n = s.tabs.len();
        self.notice = Some((n, source));
        self.source = source;
        self.trimmed = false;
        // 快照交还调用方渲染（模型面以通知条数量表达恢复成功）。
        self.restored_scene = Some(s);
        Some(self.notice.unwrap())
    }

    /// 恢复出的现场（渲染层取数口）。
    pub fn restored_scene(&self) -> Option<&Session> {
        self.restored_scene.as_ref()
    }

    /// 精简选项：「只留当前」——恢复后一键砍到当前标签。
    pub fn trim_to_active(&mut self) -> bool {
        if self.restored_scene.is_none() || self.trimmed {
            return false;
        }
        let active = self.restored_scene.as_ref().unwrap().active;
        let keep = self.restored_scene.as_ref().unwrap().tabs[active].clone();
        let s = self.restored_scene.as_mut().unwrap();
        s.tabs.clear();
        s.tabs.push(keep);
        s.active = 0;
        self.trimmed = true;
        true
    }

    /// 通知条处理（用户确认后收条）。
    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    pub fn notice(&self) -> Option<(usize, &'static str)> {
        self.notice
    }

    pub fn source(&self) -> &'static str {
        self.source
    }
}

impl Default for TabRestore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_tabrestore_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三恢复精度：目录/历史栈/滚动位逐字段回归。
    let mut r = TabRestore::new();
    r.save(Session::capture(
        alloc::vec![
            TabScene { dir: "D:\\项目".into(), history: alloc::vec!["D:\\".into(), "D:\\项目".into()], scroll_px: 120 },
            TabScene { dir: "C:\\资料".into(), history: alloc::vec!["C:\\资料".into()], scroll_px: 0 },
            TabScene { dir: "S:\\下载".into(), history: alloc::vec!["S:\\".into(), "S:\\下载".into()], scroll_px: 88 },
            TabScene { dir: "D:\\账单".into(), history: alloc::vec!["D:\\账单".into()], scroll_px: 12 },
        ],
        1,
    ));
    let restored = r.restore("normal").is_some();
    let scene = r.restored_scene();
    let precision = scene
        .map(|s| {
            s.tabs.len() == 4
                && s.tabs[0].dir == "D:\\项目"
                && s.tabs[0].history.len() == 2
                && s.tabs[0].scroll_px == 120
                && s.active == 1
        })
        .unwrap_or(false);
    set.add("three precision fields restored", restored && precision, "");

    // 2. 恢复通知条：数量与来源可见。
    let notice = r.notice();
    set.add(
        "restore notice bar",
        notice == Some((4, "normal")) && r.source() == "normal",
        "",
    );

    // 3. 精简选项：只留当前标签。
    let trimmed = r.trim_to_active();
    let after = r.restored_scene();
    set.add(
        "trim to active tab",
        trimmed
            && after.map(|s| s.tabs.len() == 1 && s.tabs[0].dir == "C:\\资料" && s.active == 0).unwrap_or(false),
        "",
    );

    // 4. 崩溃恢复注入：来源标 crash、同样全量回归。
    let mut r2 = TabRestore::new();
    r2.save(Session::capture(
        alloc::vec![TabScene { dir: "D:\\".into(), history: alloc::vec!["D:\\".into()], scroll_px: 5 }],
        0,
    ));
    r2.restore("crash");
    set.add(
        "crash injection restores too",
        r2.source() == "crash" && r2.notice() == Some((1, "crash")),
        "",
    );

    // 5. 上限 8 标签：快照捕获超限截断到 8（诚实截断——不静默丢更多因为
    //    UI 本来就开不出第 9 个；捕获侧同规双保险）。
    let many: Vec<TabScene> = (0..12)
        .map(|i| TabScene { dir: alloc::format!("D:\\{}", i), history: alloc::vec![], scroll_px: 0 })
        .collect();
    let s = Session::capture(many, 11);
    set.add(
        "tab cap eight enforced",
        s.tabs().len() == TAB_CAP && s.active == 7,
        "",
    );

    // 6. 无快照：首开机恢复尝试诚实 None、无通知条。
    let mut r3 = TabRestore::new();
    set.add(
        "no snapshot honest none",
        r3.restore("normal").is_none() && r3.notice().is_none(),
        "",
    );

    // 7. 通知条确认后收条（不留永久横幅）。
    r2.dismiss_notice();
    set.add("notice dismissed", r2.notice().is_none(), "");

    // 8. 精简只此一次：重复 trim 拒绝（账面幂等）。
    set.add("trim once only", !r.trim_to_active(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_overwrites_previous() {
        let mut r = TabRestore::new();
        r.save(Session::capture(
            alloc::vec![TabScene { dir: "a".into(), history: alloc::vec![], scroll_px: 0 }],
            0,
        ));
        r.save(Session::capture(
            alloc::vec![
                TabScene { dir: "a".into(), history: alloc::vec![], scroll_px: 0 },
                TabScene { dir: "b".into(), history: alloc::vec![], scroll_px: 0 },
            ],
            0,
        ));
        r.restore("normal");
        assert_eq!(r.restored_scene().unwrap().tabs.len(), 2);
    }

    #[test]
    fn active_clamped_on_capture() {
        let s = Session::capture(
            alloc::vec![TabScene { dir: "x".into(), history: alloc::vec![], scroll_px: 0 }],
            99,
        );
        assert_eq!(s.active, 0);
    }

    #[test]
    fn history_stack_survives_restore() {
        let mut r = TabRestore::new();
        r.save(Session::capture(
            alloc::vec![TabScene {
                dir: "D:\\".into(),
                history: alloc::vec!["D:\\a".into(), "D:\\b".into(), "D:\\".into()],
                scroll_px: 7,
            }],
            0,
        ));
        r.restore("normal");
        assert_eq!(r.restored_scene().unwrap().tabs[0].history.len(), 3);
    }
}
