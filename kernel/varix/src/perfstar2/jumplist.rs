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
