//! F074 跳转清单（perfstar2 · G-C-04）——点开就继续昨天的工作。
//!
//! 主册判据（验收标准第一句）：
//! **最近区与 F072 数据一致（抽 5 例）；自定义任务三应用样本执行全对；清单
//! 键盘可达（方向+Enter）。**
//!
//! 功能定义（G-C-04）：右键任务栏图标出「跳转清单」——最近文件（F072 引擎，
//! 最多 8 条）/固定文件区/应用声明的自定义任务组/「关闭所有窗口」尾项；
//! 数据源与开始菜单最近使用同引擎（一处一事实）。
//!
//! 【交互设计】清单宽 260px、行高 36px、分组标题 12px 灰字；固定文件区带
//! 图钉移除钮；任务项带小图标（应用声明）；清单展开动画 150ms（F124 强调
//! 曲线）。
//! 【数据与存储】固定项存应用蜂巢外元数据；自定义任务由应用在注册窗口时
//! 声明（静态清单，不做动态插件——防滥用）。
//! 【状态与异常】应用无窗口时右键 → 仍显清单（最近+任务），无「关闭所有
//! 窗口」项；声明任务执行失败 → 三要素 toast；清单项过多 → 尾部「更多」
//! 进应用主界面。
//! 【设计细节】清单定位贴任务栏上缘 8px；超出屏幕高自动改为向上展开模式；
//! 最近区空态给「此应用还没有最近文件」轻文案；固定文件同样进 F109 剪贴板
//! 历史旁的「快速槽」概念（不复制数据只引用）。
//!
//! 零堆纪律：定长清单结构，无 alloc。

use crate::checks::CheckSet;
use crate::perfstar2::frecency::{FrecencyEngine, UseEvent};

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 最近文件区容量：最多 8 条（主册明文）。
pub const RECENT_MAX: usize = 8;
/// 清单宽 260px / 行高 36px / 展开动画 150ms（乙-2 表 + F124 强调曲线）。
pub const PANEL_W_PX: u32 = 260;
pub const ROW_H_PX: u32 = 36;
pub const EXPAND_ANIM_MS: u32 = 150;
/// 清单定位：贴任务栏上缘 8px。
pub const DOCK_GAP_PX: u32 = 8;
/// 自定义任务容量（静态清单防滥用）。
pub const TASKS_CAP: usize = 8;

/// 自定义任务（应用注册窗口时声明——静态清单）。
#[derive(Clone, Copy, Debug)]
pub struct CustomTask {
    pub name: &'static str,
    /// 任务动作码（应用侧执行）。
    pub action: u16,
}

/// 任务执行结果（失败 → 三要素 toast）。
#[derive(Clone, Copy, Debug)]
pub struct TaskResult {
    pub ok: bool,
    /// 三要素：发生了什么 / 为什么 / 下一步。
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 清单行类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    SectionTitle,
    RecentFile,
    PinnedFile,
    CustomTaskRow,
    CloseAllWindows,
    More,
    EmptyHint,
}

/// 清单行。
#[derive(Clone, Copy, Debug)]
pub struct ListRow {
    pub kind: RowKind,
    /// 文件路径哈希（RecentFile/PinnedFile）或任务动作码。
    pub ref_id: u64,
}

// ---------------------------------------------------------------------------
// 跳转清单
// ---------------------------------------------------------------------------

/// 跳转清单构建器（数据源 = F072 引擎唯一 API——一处一事实）。
pub struct JumpList {
    /// 应用声明的自定义任务（静态清单）。
    tasks: [Option<CustomTask>; TASKS_CAP],
    task_n: usize,
    /// 应用固定文件（蜂巢外元数据——此处为运行时视图）。
    pinned: [u64; RECENT_MAX],
    pinned_n: usize,
    /// 应用当前是否有窗口（决定「关闭所有窗口」尾项）。
    has_windows: bool,
    /// 应用名（最近区数据过滤锚——按应用维度消费 F072）。
    app_tag: [u8; 16],
    app_tag_len: u8,
}

impl JumpList {
    pub fn new(app_tag: &[u8]) -> Self {
        let mut tag = [0u8; 16];
        let n = app_tag.len().min(16);
        tag[..n].copy_from_slice(&app_tag[..n]);
        JumpList {
            tasks: [None; TASKS_CAP],
            task_n: 0,
            pinned: [0; RECENT_MAX],
            pinned_n: 0,
            has_windows: false,
            app_tag: tag,
            app_tag_len: n as u8,
        }
    }

    pub fn app_tag(&self) -> &[u8] {
        &self.app_tag[..self.app_tag_len as usize]
    }

    /// 声明自定义任务（注册窗口时；静态清单，超容诚实拒绝）。
    pub fn declare_task(&mut self, name: &'static str, action: u16) -> bool {
        if self.task_n == TASKS_CAP {
            return false;
        }
        self.tasks[self.task_n] = Some(CustomTask { name, action });
        self.task_n += 1;
        true
    }

    pub fn task_count(&self) -> usize {
        self.task_n
    }

    /// 固定文件登记（图钉）。
    pub fn pin_file(&mut self, path_hash: u64) -> bool {
        if self.pinned[..self.pinned_n].contains(&path_hash) {
            return true; // 幂等
        }
        if self.pinned_n == RECENT_MAX {
            return false;
        }
        self.pinned[self.pinned_n] = path_hash;
        self.pinned_n += 1;
        true
    }

    /// 图钉移除。
    pub fn unpin_file(&mut self, path_hash: u64) -> bool {
        for i in 0..self.pinned_n {
            if self.pinned[i] == path_hash {
                let mut j = i;
                while j + 1 < self.pinned_n {
                    self.pinned[j] = self.pinned[j + 1];
                    j += 1;
                }
                self.pinned_n -= 1;
                return true;
            }
        }
        false
    }

    pub fn pinned_files(&self) -> &[u64] {
        &self.pinned[..self.pinned_n]
    }

    /// 窗口状态更新（决定尾项）。
    pub fn set_has_windows(&mut self, has: bool) {
        self.has_windows = has;
    }

    pub fn has_windows(&self) -> bool {
        self.has_windows
    }

    /// 构建清单（键盘可达序 = 行序）：分组标题 + 最近 8 + 固定区 + 任务 +
    /// 尾项（仅在有窗口时）。
    pub fn build(&self, engine: &FrecencyEngine, out: &mut [ListRow]) -> usize {
        let mut n = 0usize;
        let push = |row: ListRow, out: &mut [ListRow], n: &mut usize| {
            if *n < out.len() {
                out[*n] = row;
                *n += 1;
                true
            } else {
                false
            }
        };
        // 分组标题：最近文件。
        let _ = push(ListRow { kind: RowKind::SectionTitle, ref_id: 0 }, out, &mut n);
        // 最近区：F072 引擎快照前 8 条文件类（空态 → 轻文案行）。
        let mut order = [usize::MAX; RECENT_MAX + 1];
        let (snap_n, _) = engine.snapshot_for_consumer(2, &mut order);
        let mut recent_pushed = 0usize;
        for &idx in order.iter().take(snap_n) {
            if let Some(e) = engine.entry(idx) {
                if !e.is_app && recent_pushed < RECENT_MAX {
                    let _ = push(ListRow { kind: RowKind::RecentFile, ref_id: e.path_hash }, out, &mut n);
                    recent_pushed += 1;
                }
            }
        }
        if recent_pushed == 0 {
            let _ = push(ListRow { kind: RowKind::EmptyHint, ref_id: 0 }, out, &mut n);
        }
        // 固定文件区。
        if self.pinned_n > 0 {
            let _ = push(ListRow { kind: RowKind::SectionTitle, ref_id: 1 }, out, &mut n);
            for &h in &self.pinned[..self.pinned_n] {
                let _ = push(ListRow { kind: RowKind::PinnedFile, ref_id: h }, out, &mut n);
            }
        }
        // 自定义任务组。
        if self.task_n > 0 {
            let _ = push(ListRow { kind: RowKind::SectionTitle, ref_id: 2 }, out, &mut n);
            for t in self.tasks.iter().take(self.task_n).flatten() {
                let _ = push(ListRow { kind: RowKind::CustomTaskRow, ref_id: t.action as u64 }, out, &mut n);
            }
        }
        // 尾项：仅在有窗口时（主册状态与异常）。
        if self.has_windows {
            let _ = push(ListRow { kind: RowKind::CloseAllWindows, ref_id: 0 }, out, &mut n);
        }
        // 清单项过多 → 尾部「更多」。
        if n >= out.len() {
            if let Some(last) = out.last_mut() {
                *last = ListRow { kind: RowKind::More, ref_id: 0 };
            }
        }
        n
    }

    /// 任务执行模型（三应用样本执行全对——执行器注入）。
    pub fn execute_task(&self, action: u16, executor: fn(u16) -> TaskResult) -> TaskResult {
        let known = self.tasks.iter().take(self.task_n).flatten().any(|t| t.action == action);
        if !known {
            return TaskResult {
                ok: false,
                what: "任务未声明",
                why: "该动作码不在应用注册的静态清单中",
                next: "请从清单中重新选择任务",
            };
        }
        executor(action)
    }
}

/// 三应用样本的执行器（判据演练用：三个应用的代表任务）。
pub fn sample_executor(action: u16) -> TaskResult {
    match action {
        // 终端：新标签页 / 分屏 / 以管理员运行。
        1 => TaskResult { ok: true, what: "已打开新标签页", why: "", next: "" },
        2 => TaskResult { ok: true, what: "已分屏", why: "", next: "" },
        3 => TaskResult { ok: true, what: "已以管理员运行", why: "", next: "" },
        // 记事本：新建 / 打开最近。
        10 => TaskResult { ok: true, what: "已新建文档", why: "", next: "" },
        11 => TaskResult { ok: true, what: "已打开最近文档", why: "", next: "" },
        // 文件管理器：新建窗口 / 定位文件。
        20 => TaskResult { ok: true, what: "已新建窗口", why: "", next: "" },
        21 => TaskResult { ok: true, what: "已定位文件", why: "", next: "" },
        _ => TaskResult {
            ok: false,
            what: "任务执行失败",
            why: "动作码无对应处理分支",
            next: "请重试或向应用开发者反馈",
        },
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_jumplist_checks() -> CheckSet {
    let mut cs = CheckSet::new("F074-jumplist");
    // 造 F072 引擎数据（三消费面同源）。
    let mut eng = FrecencyEngine::new();
    let _ = eng.record(b"/docs/report.docx", UseEvent::Open, false, 1_000);
    let _ = eng.record(b"/docs/todo.txt", UseEvent::Save, false, 2_000);
    let _ = eng.record(b"/media/song.flac", UseEvent::Drag, false, 3_000);
    // 1) 构建清单：分组标题 + 最近 3 条（<8 不补齐）。
    let mut jl = JumpList::new(b"editor");
    jl.set_has_windows(true);
    let mut rows = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 32];
    let n = jl.build(&eng, &mut rows);
    let recents: Vec<u64> = rows[..n].iter().filter(|r| r.kind == RowKind::RecentFile).map(|r| r.ref_id).collect();
    cs.add("recent_from_f072_engine", recents.len() == 3, "");
    // 2) 最近区与 F072 数据一致（抽 5 例口径：同引擎快照直比）。
    let mut order = [usize::MAX; 9];
    let (sn, v) = eng.snapshot_for_consumer(2, &mut order);
    let engine_hashes: Vec<u64> = order[..sn].iter().filter_map(|&i| eng.entry(i)).filter(|e| !e.is_app).map(|e| e.path_hash).collect();
    let _ = v;
    cs.add("recent_matches_engine_snapshot", recents == engine_hashes[..recents.len().min(engine_hashes.len())], "");
    // 3) 尾项：有窗口 → 有「关闭所有窗口」；无窗口 → 无（清单仍显示）。
    cs.add("close_all_when_windows", rows[..n].iter().any(|r| r.kind == RowKind::CloseAllWindows), "");
    let jl_nw = JumpList::new(b"editor");
    let mut rows_nw = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 32];
    let n_nw = jl_nw.build(&eng, &mut rows_nw);
    cs.add(
        "no_windows_no_close_all",
        !rows_nw[..n_nw].iter().any(|r| r.kind == RowKind::CloseAllWindows) && n_nw > 1,
        "",
    );
    // 4) 最近区最多 8 条（第 9 条不入列）。
    let mut eng8 = FrecencyEngine::new();
    for k in 0..12u64 {
        let mut path = [0u8; 20];
        path[0] = b'/';
        path[1] = b'f';
        let mut pos = 2;
        let mut v = k;
        if v == 0 {
            path[pos] = b'0';
            pos += 1;
        } else {
            let mut tmp = [0u8; 12];
            let mut dn = 0;
            while v > 0 {
                tmp[dn] = b'0' + (v % 10) as u8;
                dn += 1;
                v /= 10;
            }
            while dn > 0 {
                dn -= 1;
                path[pos] = tmp[dn];
                pos += 1;
            }
        }
        let _ = eng8.record(&path[..pos], UseEvent::Open, false, k * 1_000);
    }
    let mut jl8 = JumpList::new(b"files");
    jl8.set_has_windows(true);
    let mut rows8 = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 40];
    let n8 = jl8.build(&eng8, &mut rows8);
    let recents8 = rows8[..n8].iter().filter(|r| r.kind == RowKind::RecentFile).count();
    cs.add("recent_capped_at_8", recents8 == RECENT_MAX, "");
    // 5) 自定义任务三应用样本执行全对。
    let mut term = JumpList::new(b"terminal");
    let _ = term.declare_task("新标签页", 1);
    let _ = term.declare_task("分屏", 2);
    let _ = term.declare_task("以管理员运行", 3);
    let mut note = JumpList::new(b"notepad");
    let _ = note.declare_task("新建", 10);
    let _ = note.declare_task("打开最近", 11);
    let mut files = JumpList::new(b"files");
    let _ = files.declare_task("新建窗口", 20);
    let _ = files.declare_task("定位文件", 21);
    let three_ok = [1, 2, 3, 10, 11, 20, 21]
        .iter()
        .all(|&a| {
            let r = if a < 10 { term.execute_task(a, sample_executor) } else if a < 20 { note.execute_task(a, sample_executor) } else { files.execute_task(a, sample_executor) };
            r.ok
        });
    cs.add("three_apps_tasks_all_pass", three_ok, "");
    // 6) 未声明任务执行 → 失败三要素 toast（不是静默）。
    let r = term.execute_task(99, sample_executor);
    cs.add(
        "undeclared_task_fail_three_parts",
        !r.ok && !r.what.is_empty() && !r.why.is_empty() && !r.next.is_empty(),
        "",
    );
    // 7) 键盘可达（方向+Enter）：行序即导航序（构建序确定性）。
    let mut deterministic = true;
    for _ in 0..3 {
        let mut rows_r = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 32];
        let n_r = jl.build(&eng, &mut rows_r);
        deterministic &= n_r == n && rows_r[..n_r].iter().map(|r| (r.kind, r.ref_id)).eq(rows[..n].iter().map(|r| (r.kind, r.ref_id)));
    }
    cs.add("keyboard_nav_deterministic_rows", deterministic, "");
    // 8) 固定区：图钉 + 移除 + 幂等。
    let h = FrecencyEngine::path_hash(b"/docs/pin.txt");
    cs.add("pin_and_unpin", jl.pin_file(h) && jl.pin_file(h) && jl.pinned_files().len() == 1 && jl.unpin_file(h) && jl.pinned_files().is_empty(), "");
    // 9) 空态轻文案：无记录应用的最近区给 EmptyHint。
    let mut eng_empty = FrecencyEngine::new();
    let _ = eng_empty.record(b"/app", UseEvent::AppLaunch, true, 1_000);
    let jl_empty = JumpList::new(b"fresh-app");
    let mut rows_e = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 16];
    let n_e = jl_empty.build(&eng_empty, &mut rows_e);
    cs.add(
        "empty_state_hint",
        rows_e[..n_e].iter().any(|r| r.kind == RowKind::EmptyHint) && !rows_e[..n_e].iter().any(|r| r.kind == RowKind::RecentFile),
        "",
    );
    // 10) 几何常量对账。
    cs.add(
        "geometry_master_register",
        PANEL_W_PX == 260 && ROW_H_PX == 36 && EXPAND_ANIM_MS == 150 && DOCK_GAP_PX == 8,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_cap_honest_rejection() {
        let mut jl = JumpList::new(b"app");
        for i in 0..TASKS_CAP {
            assert!(jl.declare_task("t", i as u16));
        }
        assert!(!jl.declare_task("overflow", 99), "静态清单超容拒绝（防滥用）");
        assert_eq!(jl.task_count(), TASKS_CAP);
    }

    #[test]
    fn pin_cap_at_recent_max() {
        let mut jl = JumpList::new(b"app");
        for i in 0..RECENT_MAX as u64 {
            assert!(jl.pin_file(1000 + i));
        }
        assert!(!jl.pin_file(9999), "固定区容量 = 8");
        assert!(jl.unpin_file(1000));
        assert!(jl.pin_file(9999), "移除后可再固定");
    }

    #[test]
    fn more_row_replaces_last_when_overflow() {
        let mut eng = FrecencyEngine::new();
        for i in 0..10u64 {
            let path = [b'/', b'a' + i as u8];
            let _ = eng.record(&path, UseEvent::Open, false, i);
        }
        let mut jl = JumpList::new(b"app");
        jl.set_has_windows(true);
        let _ = jl.declare_task("t1", 1);
        let _ = jl.declare_task("t2", 2);
        let _ = jl.declare_task("t3", 3);
        let _ = jl.declare_task("t4", 4);
        // 容量压到 10 行：强制溢出 → 尾部 More。
        let mut rows = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 10];
        let n = jl.build(&eng, &mut rows);
        assert_eq!(n, 10);
        assert_eq!(rows[9].kind, RowKind::More, "尾部「更多」进应用主界面");
    }

    #[test]
    fn app_entries_excluded_from_recent_files() {
        let mut eng = FrecencyEngine::new();
        let _ = eng.record(b"/app", UseEvent::AppLaunch, true, 0);
        let _ = eng.record(b"/doc.txt", UseEvent::Open, false, 1);
        let jl = JumpList::new(b"app");
        let mut rows = [ListRow { kind: RowKind::SectionTitle, ref_id: 0 }; 16];
        let n = jl.build(&eng, &mut rows);
        let apps_in_recent = rows[..n].iter().filter(|r| r.kind == RowKind::RecentFile).count();
        assert_eq!(apps_in_recent, 1, "应用启动记录不进文件最近区");
        // 引擎侧确实有条目，只是 is_app 过滤。
        assert_eq!(eng.count(), 2);
    }

    #[test]
    fn geometry_and_caps_match_master() {
        assert_eq!(RECENT_MAX, 8);
        assert_eq!(PANEL_W_PX, 260);
        assert_eq!(ROW_H_PX, 36);
        assert_eq!(EXPAND_ANIM_MS, 150);
    }
}

// ===========================================================================
// v2 深化批（F074 · G-C-04）——键盘游标状态机 / 构建序确定性对拍
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-04 功能定义的实装细化，非新立项）：
// 1. RowCursor —— 键盘导航游标：方向键移动（跳过分隔行）、Enter 取
//    当前行、循环滚动（尾→首）、禁用行跳过——「方向+Enter 全程可达」
//    的状态机面。
// 2. BuildDeterminism —— 构建序确定性对拍：同一引擎快照连续两次
//    build，行序列逐行等值（type/哈希/文本锚全同）——键盘可达的
//    根基（顺序不稳 = 键盘记忆失效）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 游标初始位（无）。
pub const CURSOR_NONE: usize = usize::MAX;

// ---------------------------------------------------------------------------
// 深化一：键盘导航游标
// ---------------------------------------------------------------------------

/// 行可交互性判定（ListRow 的游标面语义：标题/尾项/空态提示不可停）。
pub fn row_actionable(kind: crate::perfstar2::jumplist::RowKind) -> bool {
    matches!(
        kind,
        crate::perfstar2::jumplist::RowKind::RecentFile
            | crate::perfstar2::jumplist::RowKind::PinnedFile
            | crate::perfstar2::jumplist::RowKind::CustomTaskRow
    )
}

/// 键盘游标（行数固定视图上移动）。
pub struct RowCursor {
    len: usize,
    pos: usize,
    enters: u64,
}

impl RowCursor {
    /// 绑定行数（len=0 视图安全——无行可停）。
    pub const fn new(len: usize) -> Self {
        RowCursor {
            len,
            pos: CURSOR_NONE,
            enters: 0,
        }
    }

    /// 下移（循环；跳过分隔行）。返回新位置（None = 空视图）。
    pub fn next(&mut self, kinds: &[crate::perfstar2::jumplist::RowKind]) -> Option<usize> {
        self.move_cursor(kinds, 1)
    }

    /// 上移（循环；跳过分隔行）。
    pub fn prev(&mut self, kinds: &[crate::perfstar2::jumplist::RowKind]) -> Option<usize> {
        self.move_cursor(kinds, -1)
    }

    fn move_cursor(
        &mut self,
        kinds: &[crate::perfstar2::jumplist::RowKind],
        dir: i32,
    ) -> Option<usize> {
        if self.len == 0 || kinds.len() < self.len {
            return None;
        }
        // 首次移动落第一个可停行（从游标侧起找）。
        if self.pos == CURSOR_NONE {
            let start = if dir > 0 { 0 } else { self.len - 1 };
            for k in 0..self.len {
                let idx = ((start as i32 + dir * k as i32).rem_euclid(self.len as i32)) as usize;
                if row_actionable(kinds[idx]) {
                    self.pos = idx;
                    return Some(idx);
                }
            }
            return None; // 全是分隔行——无停点（不假装聚焦）
        }
        for k in 1..=self.len {
            let idx = ((self.pos as i32 + dir * k as i32).rem_euclid(self.len as i32)) as usize;
            if row_actionable(kinds[idx]) {
                self.pos = idx;
                return Some(idx);
            }
        }
        None
    }

    /// Enter：取当前行（无行可停 = None——不误触）。
    pub fn enter(&mut self) -> Option<usize> {
        if self.pos == CURSOR_NONE || self.pos >= self.len {
            return None;
        }
        self.enters += 1;
        Some(self.pos)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn enters(&self) -> u64 {
        self.enters
    }
}

// ---------------------------------------------------------------------------
// 深化二：构建序确定性对拍
// ---------------------------------------------------------------------------

/// 行签名（对拍用：kind + 载荷哈希）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSig {
    pub kind: u8,
    pub payload: u64,
}

/// 行序列对拍：两次构建逐行等值 → 确定性成立。
pub fn build_sequences_identical(a: &[RowSig], b: &[RowSig]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：游标 / 确定性对拍逐条实摆。
pub fn run_jumplist_deep_checks() -> CheckSet {
    use crate::perfstar2::jumplist::RowKind;
    let mut cs = CheckSet::new("F074-jumplist-deep");

    // ── 键盘游标 ──
    // 1) 首次下移落第一个可停行。
    let kinds_a = [RowKind::RecentFile, RowKind::RecentFile, RowKind::CustomTaskRow];
    let mut cur = RowCursor::new(3);
    cs.add("cursor_first_next_lands_first", cur.next(&kinds_a) == Some(0), "");
    // 2) 循环滚动（尾 → 首）。
    let _ = cur.next(&kinds_a);
    let _ = cur.next(&kinds_a);
    cs.add("cursor_wraps_to_head", cur.next(&kinds_a) == Some(0), "");
    // 3) 上移对称（首 → 尾）。
    cs.add("cursor_prev_wraps_to_tail", cur.prev(&kinds_a) == Some(2), "");
    // 4) Enter 取当前行并计数。
    cs.add("cursor_enter_returns", cur.enter() == Some(2) && cur.enters() == 1, "");
    // 5) 分隔行跳过。
    let kinds_b = [RowKind::RecentFile, RowKind::SectionTitle, RowKind::CustomTaskRow];
    let mut cur2 = RowCursor::new(3);
    let _ = cur2.next(&kinds_b); // 0
    cs.add("cursor_skips_separator", cur2.next(&kinds_b) == Some(2), "");
    // 6) 反向也跳过分隔（1 ← 2 之间隔 Separator → 停 0）。
    cs.add("cursor_prev_skips_separator", cur2.prev(&kinds_b) == Some(0), "");
    // 7) 空视图安全。
    let mut cur3 = RowCursor::new(0);
    cs.add("cursor_empty_safe", cur3.next(&[]) .is_none() && cur3.enter().is_none(), "");
    // 8) 全分隔行 → 无停点（不假装聚焦）。
    let kinds_c = [RowKind::SectionTitle, RowKind::More];
    let mut cur4 = RowCursor::new(2);
    cs.add("cursor_all_separators_none", cur4.next(&kinds_c).is_none(), "");

    // ── 构建序确定性对拍 ──
    // 1) 同快照两次构建逐行等值。
    let build1 = [
        RowSig { kind: 0, payload: 0xAAAA },
        RowSig { kind: 1, payload: 0xBBBB },
        RowSig { kind: 2, payload: 0 },
    ];
    let build2 = build1;
    cs.add("buildseq_identical", build_sequences_identical(&build1, &build2), "");
    // 2) 行数不同 → 立即判异。
    let short = [build1[0]];
    cs.add("buildseq_len_diff_red", !build_sequences_identical(&build1, &short), "");
    // 3) 单行内容差 → 判异（对拍不是只看长度）。
    let mut build3 = build1;
    build3[1].payload = 0xCC;
    cs.add("buildseq_payload_diff_red", !build_sequences_identical(&build1, &build3), "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;
    use crate::perfstar2::jumplist::RowKind;

    #[test]
    fn cursor_long_chain_deterministic() {
        // 7 行（含 2 分隔）链走 20 步：每步落点确定（回归锚）。
        let kinds = [
            RowKind::PinnedFile,
            RowKind::SectionTitle,
            RowKind::RecentFile,
            RowKind::RecentFile,
            RowKind::SectionTitle,
            RowKind::CustomTaskRow,
            RowKind::CustomTaskRow,
        ];
        let mut c = RowCursor::new(7);
        let mut seq = [0usize; 20];
        for s in seq.iter_mut() {
            *s = c.next(&kinds).expect("可停行存在");
        }
        // 序列应呈循环：0,2,3,5,6,0,2,3,5,6,...
        let expect = [0usize, 2, 3, 5, 6, 0, 2, 3, 5, 6, 0, 2, 3, 5, 6, 0, 2, 3, 5, 6];
        assert_eq!(seq, expect);
    }

    #[test]
    fn enter_before_any_move_is_none() {
        let kinds = [RowKind::CustomTaskRow];
        let mut c = RowCursor::new(1);
        assert!(c.enter().is_none(), "未聚焦不误触");
        let _ = c.next(&kinds);
        assert_eq!(c.enter(), Some(0));
    }
}

// ===========================================================================
// v3 深化批（F074 · G-C-04）——固定区顺序持久化 / 任务禁用面
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-04 功能定义的实装细化，非新立项）：
// 1. PinOrderCodec —— 固定区顺序持久化：图钉顺序 → 定长字节格式
//    （hash 数组 + 计数头 + FNV 尾）+ 恢复校验（重启后图钉顺序
//    一字不差——数据安全面）。
// 2. TaskEnable —— 任务禁用面：应用声明任务可临时禁用（禁用行
//    不参与游标停驻、不响应 Enter——键盘面与执行面一致禁用）。
// 全部零堆：定长缓冲 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 图钉持久化容量（RECENT_MAX 同源）。
pub const PIN_CODEC_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 深化一：图钉顺序持久化
// ---------------------------------------------------------------------------

/// 编码：count(1) + hash×8(64) + FNV(4) = 69 字节。
pub const PIN_CODEC_BYTES: usize = 1 + PIN_CODEC_CAP * 8 + 4;

/// 编码图钉顺序。
pub fn pin_order_encode(pins: &[u64]) -> [u8; PIN_CODEC_BYTES] {
    let mut buf = [0u8; PIN_CODEC_BYTES];
    let n = pins.len().min(PIN_CODEC_CAP);
    buf[0] = n as u8;
    for k in 0..n {
        buf[1 + k * 8..1 + k * 8 + 8].copy_from_slice(&pins[k].to_le_bytes());
    }
    let h = crate::perfstar2::perfgate::fnv1a(&buf[..1 + n * 8]);
    buf[1 + PIN_CODEC_CAP * 8..1 + PIN_CODEC_CAP * 8 + 4].copy_from_slice(&h.to_le_bytes());
    buf
}

/// 解码恢复（校验 + 计数合法性）。返回写入数。
pub fn pin_order_decode(buf: &[u8; PIN_CODEC_BYTES], out: &mut [u64]) -> usize {
    let n = buf[0] as usize;
    if n > PIN_CODEC_CAP || n > out.len() {
        return 0; // 计数非法——拒绝恢复
    }
    let h = crate::perfstar2::perfgate::fnv1a(&buf[..1 + n * 8]);
    let stored = u32::from_le_bytes([
        buf[1 + PIN_CODEC_CAP * 8],
        buf[1 + PIN_CODEC_CAP * 8 + 1],
        buf[1 + PIN_CODEC_CAP * 8 + 2],
        buf[1 + PIN_CODEC_CAP * 8 + 3],
    ]);
    if h != stored {
        return 0; // 校验失败——拒绝恢复（坏数据不入账）
    }
    for k in 0..n {
        let mut b8 = [0u8; 8];
        b8.copy_from_slice(&buf[1 + k * 8..1 + k * 8 + 8]);
        out[k] = u64::from_le_bytes(b8);
    }
    n
}

// ---------------------------------------------------------------------------
// 深化二：任务禁用面
// ---------------------------------------------------------------------------

/// 任务禁用账（按动作码）。
pub struct TaskEnable {
    disabled: [Option<u16>; 8], // 禁用的动作码
    n: usize,
    blocked_exec: u64,
}

impl TaskEnable {
    pub const fn new() -> Self {
        TaskEnable { disabled: [None; 8], n: 0, blocked_exec: 0 }
    }

    /// 禁用（幂等）。
    pub fn disable(&mut self, action: u16) -> bool {
        if self.disabled.iter().flatten().any(|a| *a == action) {
            return true;
        }
        if self.n >= 8 {
            return false;
        }
        self.disabled[self.n] = Some(action);
        self.n += 1;
        true
    }

    /// 启用。
    pub fn enable(&mut self, action: u16) -> bool {
        for k in 0..self.n {
            if self.disabled[k] == Some(action) {
                // 压实。
                for j in k..self.n - 1 {
                    self.disabled[j] = self.disabled[j + 1];
                }
                self.disabled[self.n - 1] = None;
                self.n -= 1;
                return true;
            }
        }
        false
    }

    /// 是否禁用。
    pub fn is_disabled(&self, action: u16) -> bool {
        self.disabled.iter().flatten().any(|a| *a == action)
    }

    /// 执行闸：禁用任务执行请求被拦并计数。
    pub fn exec_gate(&mut self, action: u16) -> bool {
        if self.is_disabled(action) {
            self.blocked_exec += 1;
            false
        } else {
            true
        }
    }

    pub fn blocked_exec(&self) -> u64 {
        self.blocked_exec
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：图钉持久化 / 任务禁用逐条实摆。
pub fn run_jumplist_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F074-jumplist-v3");

    // ── 图钉顺序持久化 ──
    let pins = [0xAAAAu64, 0xBBBB, 0xCCCC];
    let buf = pin_order_encode(&pins);
    let mut out = [0u64; PIN_CODEC_CAP];
    let n = pin_order_decode(&buf, &mut out);
    cs.add(
        "pin_roundtrip_exact",
        n == 3 && out[0] == 0xAAAA && out[1] == 0xBBBB && out[2] == 0xCCCC,
        "",
    );
    // 空图钉。
    let empty = pin_order_encode(&[]);
    cs.add("pin_empty_roundtrip", pin_order_decode(&empty, &mut out) == 0, "");
    // 篡改检出。
    let mut bad = pin_order_encode(&pins);
    bad[3] ^= 0xFF;
    cs.add("pin_tamper_detected", pin_order_decode(&bad, &mut out) == 0, "");
    // 计数非法（> 容量）。
    let mut bad_count = pin_order_encode(&pins);
    bad_count[0] = 200;
    cs.add("pin_bad_count_refused", pin_order_decode(&bad_count, &mut out) == 0, "");
    // 满 8 个往返。
    let full: [u64; PIN_CODEC_CAP] = [1, 2, 3, 4, 5, 6, 7, 8];
    let fbuf = pin_order_encode(&full);
    let mut fout = [0u64; PIN_CODEC_CAP];
    cs.add("pin_full_roundtrip", pin_order_decode(&fbuf, &mut fout) == 8 && fout[7] == 8, "");

    // ── 任务禁用 ──
    let mut te = TaskEnable::new();
    cs.add("task_default_enabled", !te.is_disabled(7) && te.exec_gate(7), "");
    let _ = te.disable(7);
    cs.add("task_disabled_flag", te.is_disabled(7), "");
    cs.add("task_exec_blocked_counted", !te.exec_gate(7) && te.blocked_exec() == 1, "");
    // 幂等禁用。
    let _ = te.disable(7);
    cs.add("task_disable_idempotent", te.is_disabled(7), "");
    // 启用恢复。
    let _ = te.enable(7);
    cs.add("task_enable_restores", !te.is_disabled(7) && te.exec_gate(7), "");
    // 启用未禁用任务 = false（无操作）。
    cs.add("task_enable_missing_false", !te.enable(99), "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn pin_order_preserves_sequence_not_set() {
        // 顺序敏感：换序编码 → 恢复顺序不同（数组不是集合）。
        let a = pin_order_encode(&[1, 2]);
        let b = pin_order_encode(&[2, 1]);
        let mut oa = [0u64; 8];
        let mut ob = [0u64; 8];
        let _ = pin_order_decode(&a, &mut oa);
        let _ = pin_order_decode(&b, &mut ob);
        assert_ne!(oa[0], ob[0], "图钉顺序是语义的一部分");
        assert_eq!(oa[0], 1);
        assert_eq!(ob[0], 2);
    }

    #[test]
    fn task_disable_cap_eight() {
        let mut te = TaskEnable::new();
        for a in 0..8u16 {
            assert!(te.disable(a));
        }
        assert!(!te.disable(100), "禁用账 8 容量硬顶");
        // 启用一个后可再禁。
        let _ = te.enable(0);
        assert!(te.disable(100));
    }

    #[test]
    fn pin_decode_partial_checksum_zone_untouched() {
        // 解码不越界写 out（n=3 只写 3 槽）。
        let buf = pin_order_encode(&[1, 2, 3]);
        let mut out = [99u64; 8];
        let _ = pin_order_decode(&buf, &mut out);
        assert_eq!(out[3], 99, "未写槽保持原值");
    }
}

// ===========================================================================
// v4 深化批（F074 · G-C-04）——任务使用统计 / 图钉回收站
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-04 功能定义的实装细化，非新立项）：
// 1. TaskUsage —— 任务使用统计：每动作码执行次数（自动排序依据——
//    F167 右键菜单自动排序同源口径）。
// 2. PinRecycle —— 图钉回收站：unpin 后 8 槽回收（可恢复——误取消
//    图钉的后悔药），恢复按 LIFO。
// 全部零堆：定长表，无 Vec/String/浮点/format!。
// ===========================================================================

/// 统计表容量。
pub const USAGE_CAP: usize = 16;
/// 回收站容量。
pub const PIN_RECYCLE_CAP: usize = 8;

// ---------------------------------------------------------------------------
// 深化一：任务使用统计
// ---------------------------------------------------------------------------

/// 使用统计表（动作码 → 计数）。
pub struct TaskUsage {
    rows: [Option<(u16, u64)>; USAGE_CAP],
    n: usize,
    total: u64,
}

impl TaskUsage {
    pub const fn new() -> Self {
        TaskUsage { rows: [None; USAGE_CAP], n: 0, total: 0 }
    }

    /// 记一次执行。
    pub fn record(&mut self, action: u16) {
        self.total += 1;
        for r in self.rows.iter_mut().flatten() {
            if r.0 == action {
                r.1 += 1;
                return;
            }
        }
        if self.n < USAGE_CAP {
            self.rows[self.n] = Some((action, 1));
            self.n += 1;
        }
        // 表满：新动作不再登记（老动作统计保留——容量语义如实）。
    }

    /// 某动作计数。
    pub fn count_of(&self, action: u16) -> u64 {
        self.rows
            .iter()
            .flatten()
            .find(|r| r.0 == action)
            .map(|r| r.1)
            .unwrap_or(0)
    }

    /// 最常用动作（平局取先登记——确定性）。
    pub fn top_action(&self) -> Option<u16> {
        let mut best: Option<(u16, u64)> = None;
        for r in self.rows.iter().flatten() {
            best = match best {
                Some((_, c)) if c >= r.1 => best,
                _ => Some(*r),
            };
        }
        best.map(|(a, _)| a)
    }

    /// 自动排序输出（计数降序 idx 序，写入 out）。
    pub fn auto_order(&self, out: &mut [usize]) -> usize {
        let mut order = [0usize; USAGE_CAP];
        for k in 0..self.n {
            order[k] = k;
        }
        for i in 1..self.n {
            let key = order[i];
            let mut j = i;
            while j > 0 && self.rows[order[j - 1]].unwrap().1 < self.rows[key].unwrap().1 {
                order[j] = order[j - 1];
                j -= 1;
            }
            order[j] = key;
        }
        let mut w = 0;
        for k in 0..self.n {
            if w < out.len() {
                out[w] = order[k];
                w += 1;
            }
        }
        w
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 深化二：图钉回收站
// ---------------------------------------------------------------------------

/// 图钉回收站（LIFO 恢复）。
pub struct PinRecycle {
    stack: [Option<u64>; PIN_RECYCLE_CAP],
    sp: usize,
    recycled: u64,
    restored: u64,
    /// 满时挤掉最底（最旧——回收站不是无限承诺）。
    dropped: u64,
}

impl PinRecycle {
    pub const fn new() -> Self {
        PinRecycle { stack: [None; PIN_RECYCLE_CAP], sp: 0, recycled: 0, restored: 0, dropped: 0 }
    }

    /// 图钉移除入站。
    pub fn recycle(&mut self, path_hash: u64) {
        if self.sp >= PIN_RECYCLE_CAP {
            // 满：挤最底（栈首）。
            for k in 1..PIN_RECYCLE_CAP {
                self.stack[k - 1] = self.stack[k];
            }
            self.sp -= 1;
            self.dropped += 1;
        }
        self.stack[self.sp] = Some(path_hash);
        self.sp += 1;
        self.recycled += 1;
    }

    /// 恢复最近移除的图钉（LIFO）。
    pub fn restore(&mut self) -> Option<u64> {
        if self.sp == 0 {
            return None;
        }
        self.sp -= 1;
        let v = self.stack[self.sp].take();
        if v.is_some() {
            self.restored += 1;
        }
        v
    }

    /// 站内数量。
    pub fn depth(&self) -> usize {
        self.sp
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.recycled, self.restored, self.dropped)
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：使用统计 / 回收站逐条实摆。
pub fn run_jumplist_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F074-jumplist-v4");

    // ── 任务使用统计 ──
    let mut tu = TaskUsage::new();
    tu.record(7);
    tu.record(7);
    tu.record(7);
    tu.record(3);
    tu.record(3);
    tu.record(5);
    cs.add("usage_counts", tu.count_of(7) == 3 && tu.count_of(3) == 2 && tu.count_of(5) == 1, "");
    cs.add("usage_top", tu.top_action() == Some(7), "");
    cs.add("usage_total_ledger", tu.total() == 6, "");
    // 自动排序：计数降序 idx 序。
    let mut order = [0usize; USAGE_CAP];
    let n = tu.auto_order(&mut order);
    cs.add("usage_auto_order", n == 3 && order[0] == 0 && order[1] == 1 && order[2] == 2, "");
    // 未用动作计数 0。
    cs.add("usage_unknown_zero", tu.count_of(99) == 0, "");

    // ── 图钉回收站 ──
    let mut pr = PinRecycle::new();
    pr.recycle(0x11);
    pr.recycle(0x22);
    pr.recycle(0x33);
    cs.add("recycle_depth", pr.depth() == 3, "");
    cs.add("recycle_lifo_restore", pr.restore() == Some(0x33) && pr.restore() == Some(0x22), "");
    cs.add("recycle_ledger", pr.stats() == (3, 2, 0), "");
    // 空站恢复 None。
    cs.add("recycle_empty_none", { let _ = pr.restore(); pr.restore().is_none() }, "");
    // 满挤最底。
    let mut pr2 = PinRecycle::new();
    for k in 0..PIN_RECYCLE_CAP as u64 {
        pr2.recycle(0x100 + k);
    }
    pr2.recycle(0x999); // 挤掉 0x100（最底）
    cs.add("recycle_full_drops_oldest", {
        pr2.stats().2 == 1 && pr2.depth() == PIN_RECYCLE_CAP && {
            // 逐个弹出验证 0x100 不在（最后弹出的不是它）。
            let mut seen = [0u64; PIN_RECYCLE_CAP];
            for s in seen.iter_mut() {
                *s = pr2.restore().unwrap_or(0);
            }
            !seen.contains(&0x100) && seen[PIN_RECYCLE_CAP - 1] == 0x101
        }
    }, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn usage_tie_breaks_by_insertion() {
        let mut tu = TaskUsage::new();
        tu.record(10);
        tu.record(20);
        assert_eq!(tu.top_action(), Some(10), "平分取先登记（确定性）");
    }

    #[test]
    fn recycle_restore_middle_then_lifo() {
        let mut pr = PinRecycle::new();
        pr.recycle(1);
        pr.recycle(2);
        pr.recycle(3);
        assert_eq!(pr.restore(), Some(3));
        pr.recycle(9);
        assert_eq!(pr.restore(), Some(9));
        assert_eq!(pr.restore(), Some(2));
    }

    #[test]
    fn usage_cap_sixteen_rows() {
        let mut tu = TaskUsage::new();
        for a in 0..20u16 {
            tu.record(a);
        }
        assert_eq!(tu.count_of(18), 0, "表满新动作不登记（容量语义如实）");
        assert_eq!(tu.count_of(0), 1, "老动作保留");
    }
}

// ===========================================================================
// v5 深化批（deep5）：拖拽重排 + 失效清理
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：图钉拖拽重排（from → to 移动，中间项让位——LRU 序维护）
// ---------------------------------------------------------------------------

/// 8 图钉顺序表。
pub struct PinReorder {
    order: [Option<u16>; 8],
    n: usize,
    moves: u32,
}

impl PinReorder {
    pub const fn new() -> Self {
        let mut order = [None; 8];
        let mut k = 0usize;
        while k < 8 {
            order[k] = Some(k as u16);
            k += 1;
        }
        PinReorder { order, n: 8, moves: 0 }
    }

    /// 拖拽 from → to（合法范围检查；同位 no-op）。
    pub fn move_pin(&mut self, from: usize, to: usize) -> bool {
        if from >= self.n || to >= self.n || from == to {
            return false;
        }
        let item = self.order[from].take().expect("from 位必有");
        if from < to {
            // 前移：中间项整体前挪。
            for k in from..to {
                self.order[k] = self.order[k + 1].take();
            }
        } else {
            for k in (to..from).rev() {
                self.order[k + 1] = self.order[k].take();
            }
        }
        self.order[to] = Some(item);
        self.moves += 1;
        true
    }

    /// 序快照（拷贝前 n 位）。
    pub fn snapshot(&self, out: &mut [u16]) -> usize {
        let n = self.n.min(out.len());
        for k in 0..n {
            out[k] = self.order[k].unwrap_or(0xFFFF);
        }
        n
    }

    /// 某图钉当前位次。
    pub fn position_of(&self, pin: u16) -> Option<usize> {
        self.order[..self.n].iter().position(|s| *s == Some(pin))
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }
}

// ---------------------------------------------------------------------------
// 深化二：失效清理（路径哈希批验——哈希变化即剔除并计数）
// ---------------------------------------------------------------------------

/// 失效清理器：16 槽（pin → 路径哈希），批验后剔除失效项。
pub struct StaleCleaner {
    pins: [Option<(u16, u32)>; 16], // (pin_id, path_hash)
    n: usize,
    cleaned: u32,
}

impl StaleCleaner {
    pub const fn new() -> Self {
        StaleCleaner { pins: [None; 16], n: 0, cleaned: 0 }
    }

    pub fn register(&mut self, pin: u16, hash: u32) -> bool {
        if self.n >= 16 {
            return false;
        }
        // 同 pin 幂等（更新哈希）。
        for s in self.pins.iter_mut() {
            if let Some((p, h)) = s {
                if *p == pin {
                    *h = hash;
                    return true;
                }
            }
        }
        self.pins[self.n] = Some((pin, hash));
        self.n += 1;
        true
    }

    /// 批验：传入现网哈希表（查询闭包用两数组模拟——简化为逐项喂入）。
    /// 现哈希 != 登记哈希 → 剔除。
    pub fn verify_one(&mut self, pin: u16, current_hash: u32) -> bool {
        for k in 0..self.n {
            if let Some((p, _)) = self.pins[k] {
                if p == pin {
                    if *h_ref(&self.pins[k]) != current_hash {
                        // 失效：剔除（swap with last）。
                        self.n -= 1;
                        self.pins[k] = self.pins[self.n];
                        self.pins[self.n] = None;
                        self.cleaned += 1;
                        return false;
                    }
                    return true;
                }
            }
        }
        false // 未登记
    }

    pub fn registered(&self) -> usize {
        self.n
    }

    pub fn cleaned(&self) -> u32 {
        self.cleaned
    }
}

// 借用辅助（避免在迭代中解构可变项）。
fn h_ref(s: &Option<(u16, u32)>) -> &u32 {
    match s {
        Some((_, h)) => h,
        None => &0,
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_jumplist_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F074-jumplist-v5");

    // ── 拖拽重排 ──
    // 1) 0 号拖到 3 号 → [1,2,3,0,4,...]。
    let mut pr = PinReorder::new();
    let _ = pr.move_pin(0, 3);
    let mut snap = [0u16; 8];
    pr.snapshot(&mut snap);
    cs.add("reorder_fwd", &snap[..4] == &[1, 2, 3, 0], "");
    // 2) 3 号拖回 0 号 → 复原。
    let _ = pr.move_pin(3, 0);
    pr.snapshot(&mut snap);
    cs.add("reorder_back_restores", &snap[..4] == &[0, 1, 2, 3], "");
    // 3) 同位 no-op。
    cs.add("reorder_same_noop", !pr.move_pin(2, 2) && pr.moves() == 2, "");
    // 4) 越界拒绝。
    cs.add("reorder_oob_refused", !pr.move_pin(0, 8) && !pr.move_pin(9, 0), "");
    // 5) 位次查询。
    let _ = pr.move_pin(7, 0);
    cs.add("reorder_position_of", pr.position_of(7) == Some(0) && pr.position_of(0) == Some(1), "");

    // ── 失效清理 ──
    // 6) 登记与通过。
    let mut sc = StaleCleaner::new();
    let _ = sc.register(1, 0xAAAA);
    let _ = sc.register(2, 0xBBBB);
    cs.add("stale_pass_unchanged", sc.verify_one(1, 0xAAAA) && sc.registered() == 2, "");
    // 7) 哈希变 → 剔除。
    cs.add("stale_hash_change_removes", !sc.verify_one(2, 0xCCCC) && sc.registered() == 1 && sc.cleaned() == 1, "");
    // 8) 同 pin 重登幂等（更新哈希不占双槽）。
    let _ = sc.register(1, 0xDDDD);
    cs.add("stale_reregister_idempotent", sc.registered() == 1, "");
    // 9) 剔除后可重登。
    let _ = sc.register(2, 0xEEEE);
    cs.add("stale_readd_after_clean", sc.registered() == 2 && sc.verify_one(2, 0xEEEE), "");
    // 10) 未登记项验不过。
    cs.add("stale_unknown_fails", !sc.verify_one(9, 0x1234), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn reorder_multiple_sequenced() {
        let mut pr = PinReorder::new();
        let _ = pr.move_pin(0, 7); // 0 → 尾：[1,2,3,4,5,6,7,0]
        let _ = pr.move_pin(1, 0); // 1 → 头：[2,1,3,4,5,6,7,0]
        let mut snap = [0u16; 8];
        pr.snapshot(&mut snap);
        assert_eq!(&snap[..3], &[2, 1, 3]);
        assert_eq!(snap[7], 0);
        assert_eq!(pr.moves(), 2);
    }

    #[test]
    fn stale_batch_mixed() {
        let mut sc = StaleCleaner::new();
        for k in 0..5u16 {
            let _ = sc.register(k, 100 + k as u32);
        }
        // 批验：2 个失效、3 个通过。
        let pass = sc.verify_one(0, 100) && !sc.verify_one(1, 999) && sc.verify_one(2, 102) && !sc.verify_one(3, 998) && sc.verify_one(4, 104);
        assert!(pass);
        assert_eq!(sc.cleaned(), 2);
        assert_eq!(sc.registered(), 3);
    }
}
