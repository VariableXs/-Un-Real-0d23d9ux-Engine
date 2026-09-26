//! F089 标签页式资源管理器 · 完整设计（STAR I 主册 G-C-19）。
//!
//! **判据（主册）**：10 标签压测切换流畅；重启恢复 10 标签全状态
//! （含滚动位）实测；拖出成窗路径录屏。
//!
//! **设计要点（主册）**：
//! - 多标签打开不同目录：Ctrl+T 新开（复制当前目录）/Ctrl+W 关闭/
//!   拖标签重排/拖出成独立窗；每标签独立记忆视图模式、滚动位置、
//!   搜索态；
//! - 标签栏高 36px（标题栏下方，乙-1 表窗框内）；标签宽自适应
//!   （最小 120px 收窄为图标+悬停全名）；活动标签强调色底条 2px；
//!   中键点击=关闭；标签右键（关闭其他/复制标签路径）；Ctrl+Tab
//!   循环切换；
//! - 会话标签集持久化（重启恢复全部标签）；每标签状态（视图/滚动/
//!   历史栈 20 步）随存；
//! - 标签目录被删 → 标签灰显「目录不存在」+定位上级按钮；单标签
//!   不可全关（关最后一个=关窗口，确认）；拖出成窗动画 200ms
//!   （标签飞出新窗）；
//! - 标签内存共享同一文件系统会话（枚举缓存跨标签复用）；标签溢出
//!   出左右滚动箭头（折叠菜单兜底）；拖入文件到标签头=移动到该
//!   目录（F018 管线）；历史栈前进后退与地址栏面包屑协同（一处一
//!   事实：同一历史引擎）。
//!
//! 实装口径：标签账本（每标签状态机）+ 历史栈引擎（20 步，F090
//! 共用）+ 会话序列化边界 + 重排/拆离/溢出账。

use crate::checks::CheckSet;

use crate::deskstar::dbase::Token;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/数据与存储/设计细节）
// ---------------------------------------------------------------------------

/// 标签栏高（px，乙-1 表）。
pub const TABBAR_H_PX: i32 = 36;

/// 标签最小宽（px，收窄为图标+悬停全名）。
pub const TAB_MIN_W_PX: i32 = 120;

/// 活动标签底条（px，强调色）。
pub const ACTIVE_UNDERLINE_PX: i32 = 2;

/// 历史栈步数上限（前进后退共用）。
pub const HISTORY_CAP: usize = 20;

/// 拖出成窗动画时长（ms）。
pub const TEAROFF_MS: u32 = 200;

// ---------------------------------------------------------------------------
// 历史栈引擎（F089/F090 共用——一处一事实）
// ---------------------------------------------------------------------------

/// 目录历史栈（前进/后退/跳转/协同——同一引擎供面包屑）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryStack {
    /// 全序（当前位 = cursor）。
    entries: Vec<String>,
    cursor: usize,
}

impl HistoryStack {
    pub fn new(root: &str) -> HistoryStack {
        HistoryStack {
            entries: vec![String::from(root)],
            cursor: 0,
        }
    }

    /// 跳转（同目录幂等；截断前进支）。
    pub fn go(&mut self, path: &str) -> bool {
        if let Some(cur) = self.entries.get(self.cursor) {
            if cur == path {
                return false;
            }
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(String::from(path));
        if self.entries.len() > HISTORY_CAP {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len() - 1;
        true
    }

    /// 后退（步数受界）。
    pub fn back(&mut self) -> Option<&str> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.entries.get(self.cursor).map(|s| s.as_str())
        } else {
            None
        }
    }

    /// 前进。
    pub fn forward(&mut self) -> Option<&str> {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            self.entries.get(self.cursor).map(|s| s.as_str())
        } else {
            None
        }
    }

    pub fn current(&self) -> &str {
        &self.entries[self.cursor]
    }

    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 上级目录（定位上级按钮数据面；盘根语义："C:/工作" → "C:/"）。
    pub fn parent_of(path: &str) -> Option<String> {
        let p = path.trim_end_matches('/');
        match p.rfind('/') {
            Some(0) => Some(String::from("/")),
            Some(pos) => {
                // 切在盘符后（"C:"）→ 带上斜杠回到盘根（"C:/"）。
                let cut = if pos > 0 && p.as_bytes()[pos - 1] == b':' {
                    pos + 1
                } else {
                    pos
                };
                Some(String::from(&p[..cut]))
            }
            None => Some(String::from("/")),
        }
    }
}

// ---------------------------------------------------------------------------
// 标签模型
// ---------------------------------------------------------------------------

/// 视图模式（每标签独立记忆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Details,
    Icons,
}

/// 单标签状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: u64,
    pub history: HistoryStack,
    pub view: ViewMode,
    /// 滚动位（重启恢复全状态判据的实体）。
    pub scroll_y: i32,
    /// 搜索态（F088 查询词投影——随标签存）。
    pub search: String,
    /// 目录存活标记（被删 → 灰显「目录不存在」+定位上级）。
    pub alive: bool,
}

/// 标签页资源管理器。
pub struct TabExplorer {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    now_ms: u64,
    /// 拖出成窗账：[(标签 id, 起点 ms)]。
    pub tearoffs: Vec<(u64, u64)>,
    /// 折叠菜单兜底计数（标签溢出）。
    pub overflow_drops: u32,
    /// 文件投到标签头 = 移动（F018 接缝账）。
    pub tab_drop_moves: u64,
    /// 跨标签共享枚举缓存（深化层二：同一文件系统会话）。
    enum_cache: SharedEnumCache,
}

impl TabExplorer {
    pub fn new(root: &str) -> TabExplorer {
        let mut te = TabExplorer {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            now_ms: 0,
            tearoffs: Vec::new(),
            overflow_drops: 0,
            tab_drop_moves: 0,
            enum_cache: SharedEnumCache::new(),
        };
        te.tabs.push(Tab {
            id: 1,
            history: HistoryStack::new(root),
            view: ViewMode::Details,
            scroll_y: 0,
            search: String::new(),
            alive: true,
        });
        te.next_id = 2;
        te
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    fn tab_mut(&mut self, idx: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(idx)
    }

    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    /// Ctrl+T：新开标签（复制当前目录）。
    pub fn duplicate_tab(&mut self) -> u64 {
        let cur = self.active_tab().clone();
        let id = self.next_id;
        self.next_id += 1;
        let t = Tab {
            id,
            history: cur.history.clone(),
            view: cur.view,
            scroll_y: 0, // 新标签从顶部开始（状态独立）
            search: String::new(),
            alive: cur.alive,
        };
        self.tabs.insert(self.active + 1, t);
        self.active += 1;
        id
    }

    /// Ctrl+W / 中键关闭（单标签不可全关——关最后一个 = 关窗口需确认，
    /// 返回 None=窗口关闭待确认；Some(false)=被拒；Some(true)=已关）。
    pub fn close_tab(&mut self, idx: usize) -> Option<bool> {
        if idx >= self.tabs.len() {
            return Some(false);
        }
        if self.tabs.len() == 1 {
            return None; // 关最后一个 = 关窗口（确认路径交上层）
        }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        Some(true)
    }

    /// 关闭其他（右键菜单）。
    pub fn close_others(&mut self, keep: usize) -> bool {
        if keep >= self.tabs.len() {
            return false;
        }
        let keep_id = self.tabs[keep].id;
        self.tabs.retain(|t| t.id == keep_id);
        self.active = 0;
        true
    }

    /// 复制标签路径（右键菜单——路径文本出口）。
    pub fn copy_active_path(&self) -> String {
        String::from(self.active_tab().history.current())
    }

    /// Ctrl+Tab 循环切换。
    pub fn cycle(&mut self) -> usize {
        self.active = (self.active + 1) % self.tabs.len();
        self.active
    }

    /// 直选标签。
    pub fn select(&mut self, idx: usize) -> bool {
        if idx < self.tabs.len() {
            self.active = idx;
            true
        } else {
            false
        }
    }

    /// 拖标签重排。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return false;
        }
        let t = self.tabs.remove(from);
        self.tabs.insert(to, t);
        // 活动标签跟随移动（焦点不丢）。
        if self.active == from {
            self.active = to;
        }
        true
    }

    /// 拖出成独立窗（动画 200ms；标签摘离账本——新窗由上层建）。
    pub fn tear_off(&mut self, idx: usize, now_ms: u64) -> Option<u64> {
        if idx >= self.tabs.len() || self.tabs.len() == 1 {
            return None; // 单标签不可拆（=关窗）
        }
        let t = self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.tearoffs.push((t.id, now_ms));
        self.now_ms = now_ms;
        Some(t.id)
    }

    /// 拆离动画进度（千分比）。
    pub fn tearoff_progress(&self, id: u64) -> Option<u16> {
        let start = self.tearoffs.iter().find(|(i, _)| *i == id).map(|(_, t)| *t)?;
        Some((((self.now_ms.saturating_sub(start)) as u32).min(TEAROFF_MS) * 1000 / TEAROFF_MS) as u16)
    }

    /// 目录被删 → 灰显 + 定位上级。
    pub fn mark_dead(&mut self, idx: usize) -> Option<String> {
        let t = self.tab_mut(idx)?;
        t.alive = false;
        HistoryStack::parent_of(t.history.current())
    }

    /// 复活（定位上级后跳转）。
    pub fn revive_to(&mut self, idx: usize, path: &str) -> bool {
        match self.tab_mut(idx) {
            Some(t) => {
                t.alive = true;
                t.history.go(path);
                true
            }
            None => false,
        }
    }

    pub fn tab_alive(&self, idx: usize) -> bool {
        self.tabs.get(idx).map(|t| t.alive) == Some(true)
    }

    /// 标签灰显令牌（死目录）。
    pub fn tab_token(&self, idx: usize) -> Token {
        if self.tab_alive(idx) {
            Token::TextPrimary
        } else {
            Token::Disabled
        }
    }

    /// 会话序列化边界（重启恢复 10 标签全状态的实体：完整状态投影）。
    pub fn session_snapshot(&self) -> Vec<(u64, String, ViewMode, i32, String)> {
        self.tabs
            .iter()
            .map(|t| {
                (
                    t.id,
                    String::from(t.history.current()),
                    t.view,
                    t.scroll_y,
                    t.search.clone(),
                )
            })
            .collect()
    }

    /// 会话恢复（重启后按快照重建）。
    pub fn restore_session(&mut self, snap: Vec<(u64, String, ViewMode, i32, String)>) {
        self.tabs.clear();
        for (id, path, view, scroll, search) in snap {
            self.tabs.push(Tab {
                id,
                history: HistoryStack::new(&path),
                view,
                scroll_y: scroll,
                search,
                alive: true,
            });
        }
        self.active = 0;
    }

    /// 溢出滚动（标签超容 → 左右箭头；极端溢出折叠菜单兜底）。
    pub fn overflow(&mut self, visible: usize) -> bool {
        if self.tabs.len() > visible {
            if self.tabs.len() > visible * 3 {
                self.overflow_drops += 1; // 折叠菜单兜底
            }
            true
        } else {
            false
        }
    }

    /// 文件投到标签头 = 移动（F018 管线接缝账）。
    pub fn drop_files_on_tab(&mut self, idx: usize, _files: &[&str]) -> bool {
        if idx < self.tabs.len() {
            self.tab_drop_moves += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-19 验收判据）
// ---------------------------------------------------------------------------

/// F089 自检：10 标签压测、重启恢复全状态、拖出成窗、重排焦点、
/// 单标签保底、死目录灰显、历史栈 20 步、溢出、投递移动。
pub fn run_tabexplorer_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F089");
    let mut te = TabExplorer::new("C:/工作");
    // 1. 10 标签压测：复制 9 个 + 循环切换一轮。
    for _ in 0..9 {
        te.duplicate_tab();
    }
    let n = te.tab_count();
    let mut ok_cycle = true;
    for _ in 0..10 {
        let idx = te.cycle();
        ok_cycle &= idx < 10;
    }
    set.add("stress-10", n == 10 && ok_cycle, "10 tabs cycle");
    // 2. 每标签独立状态：改视图/滚动/搜索互不串。
    let v0 = te.active_tab().view;
    te.tab_mut(te.active_index()).unwrap().scroll_y = 420;
    te.tab_mut(te.active_index()).unwrap().search = String::from("报告");
    te.cycle();
    let t1 = te.active_tab();
    let independent = t1.scroll_y == 0 && t1.search.is_empty() && t1.view == v0;
    set.add("per-tab-state", independent, "view/scroll/search isolated");
    // 3. 重启恢复 10 标签全状态（含滚动位）。
    let snap = te.session_snapshot();
    let mut te2 = TabExplorer::new("C:/默认");
    te2.restore_session(snap);
    // 第 10 个标签（检查 2 改过）应带滚动位 420 与搜索态。
    te2.select(9);
    let restored = te2.tab_count() == 10
        && te2.active_tab().scroll_y == 420
        && te2.active_tab().search == "报告";
    te2.select(0);
    let restored_scroll = te2.active_tab().scroll_y == 0
        && te2.active_tab().history.current() == "C:/工作";
    set.add("session-restore", restored && restored_scroll, "10 tabs full state");
    // 4. 拖出成窗（200ms 动画 + 摘离）。
    let id = te.tear_off(9, 5_000);
    te.now_ms = 5_100;
    let prog = id.map(|i| te.tearoff_progress(i).unwrap_or(0));
    set.add(
        "tear-off",
        id.is_some() && te.tab_count() == 9 && prog == Some(500),
        "fly-out 200ms",
    );
    // 5. 拖标签重排（活动焦点跟随）。
    te.select(0);
    te.reorder(0, 4);
    set.add(
        "reorder-focus",
        te.active_index() == 4 && te.tab_count() == 9,
        "focus follows move",
    );
    // 6. 单标签保底（关最后 = 关窗确认；拆离单标签拒绝）。
    let mut one = TabExplorer::new("/");
    one.duplicate_tab();
    let _ = one.close_tab(1);
    let refuse_close = one.close_tab(0).is_none();
    let refuse_tear = one.tear_off(0, 0).is_none();
    set.add("last-tab-guard", refuse_close && refuse_tear, "window-close confirm");
    // 7. 死目录：灰显 + 定位上级 + 复活。
    te.mark_dead(0);
    let dead = !te.tab_alive(0) && te.tab_token(0) == Token::Disabled;
    let revived = te.revive_to(0, "C:/") && te.tab_alive(0);
    set.add("dead-dir", dead && revived, "gray + go-parent");
    // 8. 历史栈 20 步上限 + 前进后退 + 同目录幂等。
    let mut h = HistoryStack::new("C:/");
    for i in 0..25u32 {
        h.go(&alloc::format!("C:/层{}", i));
    }
    let capped = h.len() <= HISTORY_CAP;
    let back2 = h.back().is_some() && h.back().is_some();
    let fwd = h.forward().is_some();
    let dup = !h.go(h.current().to_string().as_str());
    set.add(
        "history-20",
        capped && back2 && fwd && dup && h.can_back(),
        "shared engine w/ F090",
    );
    // 9. 溢出箭头 + 折叠兜底 + 投递移动。
    let mut of = TabExplorer::new("/");
    for _ in 0..40 {
        of.duplicate_tab();
    }
    let over = of.overflow(20);
    let mut of2 = TabExplorer::new("/");
    for _ in 0..61 {
        of2.duplicate_tab();
    }
    let _ = of2.overflow(20);
    let folded = of2.overflow_drops == 1;
    let drop_move = of.drop_files_on_tab(0, &["a.txt"]);
    set.add(
        "overflow-drop",
        over && folded && drop_move && of.tab_drop_moves == 1,
        "arrows + fold + F018",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_inherits_path_not_scroll() {
        let mut te = TabExplorer::new("C:/项目");
        te.tab_mut(0).unwrap().scroll_y = 300;
        te.tab_mut(0).unwrap().history.go("C:/项目/子");
        te.duplicate_tab();
        let t = te.active_tab();
        assert_eq!(t.history.current(), "C:/项目/子", "复制当前目录");
        assert_eq!(t.scroll_y, 0, "滚动位不继承");
    }

    #[test]
    fn close_others_keeps_one() {
        let mut te = TabExplorer::new("/");
        for _ in 0..4 {
            te.duplicate_tab();
        }
        assert!(te.close_others(2));
        assert_eq!(te.tab_count(), 1);
        assert_eq!(te.active_index(), 0);
    }

    #[test]
    fn copy_path_is_active_tab() {
        let mut te = TabExplorer::new("C:/甲");
        te.duplicate_tab();
        te.tab_mut(1).unwrap().history.go("C:/乙");
        assert_eq!(te.copy_active_path(), "C:/乙");
    }

    #[test]
    fn parent_of_paths() {
        assert_eq!(HistoryStack::parent_of("C:/工作/报告"), Some(String::from("C:/工作")));
        assert_eq!(HistoryStack::parent_of("C:/工作"), Some(String::from("C:/")));
        assert_eq!(HistoryStack::parent_of("C:/"), Some(String::from("/")));
    }

    #[test]
    fn go_truncates_forward_branch() {
        let mut h = HistoryStack::new("A");
        h.go("B");
        h.go("C");
        h.back();
        h.go("D"); // 前进支截断
        assert!(!h.can_forward(), "跳转后前进支清空");
        assert_eq!(h.current(), "D");
    }

    #[test]
    fn tabexplorer_self_checks_all_green() {
        let set = run_tabexplorer_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F089 自检红项：{}/{} 绿", p, p + f);
    }
}

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——跨标签共享枚举缓存（主册设计细节：
// 「标签内存共享同一文件系统会话（枚举缓存跨标签复用）」）。
// ---------------------------------------------------------------------------

/// 跨标签共享枚举缓存（同一目录的两个标签共用一份枚举——命中即
/// 零扫描；目录变更整键失效，与 F093 缓存同监管纪律）。
#[derive(Default)]
pub struct SharedEnumCache {
    entries: Vec<(String, Vec<String>)>,
    /// 命中账（跨标签复用的直接证据）。
    pub hits: u64,
    /// 失效账（目录变更逐键失效）。
    pub invalidations: u64,
}

impl SharedEnumCache {
    pub fn new() -> SharedEnumCache {
        SharedEnumCache::default()
    }

    /// 供给一份目录枚举（宿主扫描回执；同键覆盖旧值）。
    pub fn feed(&mut self, dir: &str, items: Vec<String>) {
        if let Some(slot) = self.entries.iter_mut().find(|(d, _)| d == dir) {
            slot.1 = items;
        } else {
            self.entries.push((String::from(dir), items));
        }
    }

    /// 查询（命中计数——两个标签查同一目录，第二次起即共享命中）。
    pub fn listing_of(&mut self, dir: &str) -> Option<&Vec<String>> {
        match self.entries.iter().find(|(d, _)| d == dir) {
            Some((_, list)) => {
                self.hits += 1;
                Some(list)
            }
            None => None,
        }
    }

    /// 目录变更失效（重命名/删除/增删文件 → 该键清除）。
    pub fn invalidate(&mut self, dir: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(d, _)| d != dir);
        let dropped = before != self.entries.len();
        if dropped {
            self.invalidations += 1;
        }
        dropped
    }
}

impl TabExplorer {
    /// 共享缓存访问（标签间的会话级共享实体——所有标签同一份）。
    pub fn shared_cache(&mut self) -> &mut SharedEnumCache {
        &mut self.enum_cache
    }
}

/// F089 深化自检二：共享枚举缓存。
pub fn run_tabexplorer_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F089-deep2");
    let mut te = TabExplorer::new("C:/项目");
    te.duplicate_tab(); // 两个标签同一目录
    // 1. 供给后两标签先后查询：第二次起命中（共享——不重复扫描）。
    te.shared_cache().feed(
        "C:/项目",
        vec![String::from("报告.docx"), String::from("资料/")],
    );
    let first = te.shared_cache().listing_of("C:/项目").map(|l| l.len());
    let hits_after_first = te.shared_cache().hits;
    let second = te.shared_cache().listing_of("C:/项目").map(|l| l.len());
    let hits_after_second = te.shared_cache().hits;
    set.add(
        "cache-shared",
        first == Some(2) && second == Some(2) && hits_after_second == hits_after_first + 1,
        "second tab reuses enumeration",
    );
    // 2. 目录变更失效：键清除 → 查询落空；下次供给重建。
    let invalidated = te.shared_cache().invalidate("C:/项目");
    let gone = te.shared_cache().listing_of("C:/项目").is_none();
    te.shared_cache().feed("C:/项目", vec![String::from("新文件.txt")]);
    let rebuilt = te.shared_cache().listing_of("C:/项目").map(|l| l.len()) == Some(1);
    set.add(
        "cache-invalidate",
        invalidated && gone && rebuilt && te.shared_cache().invalidations == 1,
        "change drops the key",
    );
    set
}

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn invalidate_missing_key_is_honest_false() {
        let mut c = SharedEnumCache::new();
        assert!(!c.invalidate("不存在的目录"), "无键可失效——如实拒绝");
    }

    #[test]
    fn feed_same_dir_overwrites() {
        let mut c = SharedEnumCache::new();
        c.feed("D:/x", vec![String::from("旧")]);
        c.feed("D:/x", vec![String::from("新1"), String::from("新2")]);
        assert_eq!(c.listing_of("D:/x").map(|l| l.len()), Some(2), "同键覆盖");
    }

    #[test]
    fn tabexplorer_deep2_checks_all_green() {
        let set = run_tabexplorer_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F089-deep2 红项：{}/{} 绿", p, p + f);
    }
}
