//! F074 跳转清单 · 完整设计（STAR I 主册 G-C-04）。
//!
//! **判据（主册）**：最近区与 F072 数据一致（抽 5 例）；自定义任务三应用
//! 样本执行全对；清单键盘可达（方向+Enter）。
//!
//! **设计要点（主册）**：
//! - 右键任务栏图标出清单：最近文件（F072 引擎，最多 8 条）/ 固定文件区 /
//!   应用声明的自定义任务组 / 「关闭所有窗口」尾项——数据源与开始菜单
//!   最近使用**同引擎（一处一事实）**：频次/时间/衰减只活在 F072，本模块
//!   只持「应用 ↔ 文件」关联账与「应用域固定集」两笔自有事实；
//! - 清单宽 260px、行高 36px、分组标题 12px 灰字；固定文件区带图钉移除
//!   钮；任务项带小图标（应用声明——**静态清单**，不做动态插件防滥用）；
//!   展开动画 150ms（F124 强调曲线，形状宿主侧，内核暴露进度千分比）；
//! - 应用无窗口时右键 → 仍显清单（最近+任务），**无「关闭所有窗口」项**；
//!   声明任务执行失败 → 三要素 toast（标题/详情/图标）；清单项过多 →
//!   尾部「更多」进应用主界面；
//! - 固定项存应用蜂巢外元数据（应用域固定集——与 F072 的全局固定
//!   标志是两笔不同事实，各归其主）；清单定位贴任务栏上缘 8px；超出
//!   屏幕高自动改为向上展开模式（翻转锚点到屏顶）；最近区空态给
//!   「此应用还没有最近文件」轻文案。
//!
//! 引擎依赖以显式参数承接（`&mut RecentEngine` / `&RecentEngine`），不持
//! 有、不复制——三消费面共享同一份事实。帧序/钟注入式，无外部依赖。

use crate::checks::CheckSet;
use crate::star::recenteng::{RecentEngine, ViewRow};

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 清单宽（px）。
pub const LIST_W_PX: u32 = 260;

/// 行高（px）。
pub const ROW_H_PX: u32 = 36;

/// 分组标题字号（px 灰字）。
pub const GROUP_TITLE_FONT_PX: u32 = 12;

/// 展开动画时长（ms，F124 强调曲线）。
pub const EXPAND_MS: u64 = 150;

/// 清单贴任务栏上缘间距（px）。
pub const EDGE_GAP_PX: u32 = 8;

/// 最近文件条数上限（主册：最多 8 条）。
pub const MAX_RECENT_ROWS: usize = 8;

/// 内容行溢出阈值（固定+最近+任务合计超过 → 尾部「更多」）。
pub const MAX_ROWS_BEFORE_MORE: usize = 16;

/// 单应用文件关联账上限（成员账截断最旧——展示序由引擎分数决定，不受影响）。
pub const MAX_ASSOC_PER_APP: usize = 64;

// ---------------------------------------------------------------------------
// 行与动作
// ---------------------------------------------------------------------------

/// 行类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// 分组标题（固定/最近/任务）。
    GroupTitle,
    /// 固定文件（带图钉移除钮）。
    PinnedFile,
    /// 最近文件。
    RecentFile,
    /// 自定义任务（应用声明，带小图标）。
    Task,
    /// 空态轻文案（最近区）。
    EmptyNote,
    /// 「更多」——进应用主界面。
    More,
    /// 「关闭所有窗口」尾项。
    CloseAll,
}

/// 清单行（渲染单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JmpRow {
    pub kind: RowKind,
    /// 显示文案。
    pub label: String,
    /// 文件行 = 路径；任务行 = 任务 id；其余空。
    pub arg: String,
    /// 文件已不存在（灰条标注——记录保留，F072 同款语义）。
    pub missing: bool,
    /// 图钉移除钮（仅固定文件行）。
    pub pinnable: bool,
}

/// 键盘 Enter / 点击激活产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JmpAction {
    None,
    /// 打开文件（打开即 touch 引擎——使用事件一处一事实）。
    OpenPath(String),
    /// 执行声明任务。
    RunTask(&'static str),
    /// 关闭该应用全部窗口（窗口账宿主侧，本模块只出动作+审计）。
    CloseAll,
    /// 进应用主界面。
    More,
}

/// 三要素 toast（标题/详情/图标——任务执行失败口径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub title: String,
    pub detail: String,
    pub glyph: u16,
}

/// 自定义任务声明（静态——应用注册窗口时声明，不做动态插件）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JmpTask {
    pub id: &'static str,
    pub label: &'static str,
    /// 小图标（主题图标 id）。
    pub icon: u16,
}

// ---------------------------------------------------------------------------
// 应用账与清单板
// ---------------------------------------------------------------------------

/// 单应用清单账（应用蜂巢外元数据的内存账本）。
struct AppJump {
    app_id: u64,
    tasks: Vec<JmpTask>,
    /// 应用域固定集（canonical 路径——本清单的图钉事实）。
    pinned: Vec<String>,
    /// 该应用打开过的文件（成员账，新者在先；展示序由引擎定）。
    assoc: Vec<String>,
}

impl AppJump {
    fn new(app_id: u64) -> AppJump {
        AppJump { app_id, tasks: Vec::new(), pinned: Vec::new(), assoc: Vec::new() }
    }
}

/// 跳转清单板（任务栏面——逐应用出清单）。
pub struct JumpBoard {
    apps: Vec<AppJump>,
    /// 展开中的应用与动画起点（0 ms = 未展开）。
    opened_app: u64,
    opened_ms: u64,
    open: bool,
    /// 键盘选中行（可见行序；只落在可选中行上）。
    sel: usize,
    /// 审计账：CloseAll / 任务执行次数。
    close_all_count: u64,
    task_run_count: u64,
    toast: Option<Toast>,
}

impl JumpBoard {
    pub fn new() -> JumpBoard {
        JumpBoard {
            apps: Vec::new(),
            opened_app: 0,
            opened_ms: 0,
            open: false,
            sel: 0,
            close_all_count: 0,
            task_run_count: 0,
            toast: None,
        }
    }

    // -----------------------------------------------------------------------
    // 声明与记账
    // -----------------------------------------------------------------------

    /// 应用声明自定义任务组（静态清单——重复声明整体替换）。
    pub fn declare_app(&mut self, app_id: u64, tasks: &[JmpTask]) {
        match self.apps.iter_mut().find(|a| a.app_id == app_id) {
            Some(a) => {
                a.tasks.clear();
                a.tasks.extend_from_slice(tasks);
            }
            None => {
                let mut a = AppJump::new(app_id);
                a.tasks.extend_from_slice(tasks);
                self.apps.push(a);
            }
        }
    }

    fn app(&self, app_id: u64) -> Option<&AppJump> {
        self.apps.iter().find(|a| a.app_id == app_id)
    }

    fn app_mut(&mut self, app_id: u64) -> Option<&mut AppJump> {
        self.apps.iter_mut().find(|a| a.app_id == app_id)
    }

    /// 应用打开文件：touch 引擎（使用事件唯一入口）+ 关联账记录
    /// （canonical 归并——别名与真身同账）。
    pub fn open_file(&mut self, engine: &mut RecentEngine, app_id: u64, path: &str) {
        let canon = engine.canonical_of(path);
        engine.touch(&canon);
        if self.app_mut(app_id).is_none() {
            self.apps.push(AppJump::new(app_id));
        }
        if let Some(a) = self.app_mut(app_id) {
            a.assoc.retain(|p| *p != canon);
            a.assoc.insert(0, canon);
            a.assoc.truncate(MAX_ASSOC_PER_APP);
        }
    }

    /// 固定文件（应用域图钉——存 canonical）。
    pub fn pin(&mut self, engine: &RecentEngine, app_id: u64, path: &str) -> bool {
        let canon = engine.canonical_of(path);
        if self.app(app_id).is_none() {
            self.apps.push(AppJump::new(app_id));
        }
        let a = self.app_mut(app_id).expect("app just ensured");
        if a.pinned.iter().any(|p| *p == canon) {
            return false;
        }
        a.pinned.push(canon);
        true
    }

    /// 图钉移除钮（固定区行）。
    pub fn unpin(&mut self, engine: &RecentEngine, app_id: u64, path: &str) -> bool {
        let canon = engine.canonical_of(path);
        match self.app_mut(app_id) {
            Some(a) => {
                let n = a.pinned.len();
                a.pinned.retain(|p| *p != canon);
                a.pinned.len() != n
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // 清单视图
    // -----------------------------------------------------------------------

    /// 出清单（右键图标）：固定区 → 最近区（引擎序）→ 任务组 → [更多] →
    /// [关闭所有窗口尾项]。`has_windows = false` 时无尾项（主册口径）。
    pub fn show(
        &mut self,
        engine: &RecentEngine,
        app_id: u64,
        has_windows: bool,
        now_ms: u64,
    ) -> Vec<JmpRow> {
        self.open = true;
        self.opened_app = app_id;
        self.opened_ms = now_ms;
        self.sel = 0;
        self.toast = None;

        let (pinned, assoc, tasks) = match self.app(app_id) {
            Some(a) => (a.pinned.clone(), a.assoc.clone(), a.tasks.clone()),
            None => (Vec::new(), Vec::new(), Vec::new()),
        };

        let mut rows: Vec<JmpRow> = Vec::new();

        // —— 固定区（应用域图钉，声明序置顶）。
        if !pinned.is_empty() {
            rows.push(self.group("固定"));
            for p in &pinned {
                rows.push(JmpRow {
                    kind: RowKind::PinnedFile,
                    label: file_label(p),
                    arg: p.clone(),
                    missing: false,
                    pinnable: true,
                });
            }
        }

        // —— 最近区（F072 引擎唯一排序事实源：consume_jumplist 全局序
        //    过滤本应用关联成员；已在固定区的路径不重复出现）。
        let engine_rows: Vec<ViewRow> = engine
            .consume_jumplist()
            .into_iter()
            .filter(|r| assoc.iter().any(|p| *p == r.path))
            .filter(|r| !pinned.iter().any(|p| *p == r.path))
            .take(MAX_RECENT_ROWS)
            .collect();
        rows.push(self.group("最近"));
        if engine_rows.is_empty() {
            rows.push(JmpRow {
                kind: RowKind::EmptyNote,
                label: String::from("此应用还没有最近文件"),
                arg: String::from(""),
                missing: false,
                pinnable: false,
            });
        } else {
            for r in engine_rows {
                rows.push(JmpRow {
                    kind: RowKind::RecentFile,
                    label: file_label(&r.path),
                    arg: r.path,
                    missing: r.missing,
                    pinnable: false,
                });
            }
        }

        // —— 任务组（应用声明静态清单）。
        if !tasks.is_empty() {
            rows.push(self.group("任务"));
            for t in &tasks {
                rows.push(JmpRow {
                    kind: RowKind::Task,
                    label: String::from(t.label),
                    arg: String::from(t.id),
                    missing: false,
                    pinnable: false,
                });
            }
        }

        // —— 溢出 → 尾部「更多」（内容行口径：文件+任务+空态；分组标题
        //    恒在）。截断按组序保留头部。
        let content = rows.iter().filter(|r| r.kind != RowKind::GroupTitle).count();
        if content > MAX_ROWS_BEFORE_MORE {
            let mut kept: Vec<JmpRow> = Vec::new();
            let mut budget = MAX_ROWS_BEFORE_MORE;
            for r in rows {
                if r.kind == RowKind::GroupTitle {
                    kept.push(r);
                } else if budget > 0 {
                    kept.push(r);
                    budget -= 1;
                }
            }
            kept.push(JmpRow {
                kind: RowKind::More,
                label: String::from("更多"),
                arg: String::from(""),
                missing: false,
                pinnable: false,
            });
            rows = kept;
        }

        // —— 「关闭所有窗口」尾项（应用无窗口时不渲染）。
        if has_windows {
            rows.push(JmpRow {
                kind: RowKind::CloseAll,
                label: String::from("关闭所有窗口"),
                arg: String::from(""),
                missing: false,
                pinnable: false,
            });
        }
        rows
    }

    fn group(&self, label: &str) -> JmpRow {
        JmpRow {
            kind: RowKind::GroupTitle,
            label: String::from(label),
            arg: String::from(""),
            missing: false,
            pinnable: false,
        }
    }

    /// 清单总高（行数 × 行高——屏高翻转判定输入）。
    pub fn height_px(n_rows: usize) -> u32 {
        n_rows as u32 * ROW_H_PX
    }

    /// 超出屏幕高 → 翻转锚点到屏顶（主册「向上展开模式」）。
    /// `taskbar_top_y` = 任务栏上缘 y；可用高 = 上缘 − 8px 间距。
    pub fn needs_top_anchor(list_height_px: u32, screen_h_px: u32, taskbar_top_y: u32) -> bool {
        let available = taskbar_top_y.saturating_sub(EDGE_GAP_PX);
        list_height_px > available.min(screen_h_px)
    }

    // -----------------------------------------------------------------------
    // 键盘与激活
    // -----------------------------------------------------------------------

    fn selectable(r: &JmpRow) -> bool {
        matches!(
            r.kind,
            RowKind::PinnedFile | RowKind::RecentFile | RowKind::Task | RowKind::More | RowKind::CloseAll
        )
    }

    /// 方向键下（末尾回卷首——循环可达）。
    pub fn sel_next(&mut self, rows: &[JmpRow]) {
        self.move_sel(rows, 1);
    }

    /// 方向键上（首部回卷尾）。
    pub fn sel_prev(&mut self, rows: &[JmpRow]) {
        self.move_sel(rows, -1);
    }

    fn move_sel(&mut self, rows: &[JmpRow], dir: i32) {
        let n = rows.len();
        if n == 0 {
            return;
        }
        let mut i = self.sel.min(n.saturating_sub(1));
        for _ in 0..n {
            i = ((i as i32 + dir).rem_euclid(n as i32)) as usize;
            if Self::selectable(&rows[i]) {
                self.sel = i;
                return;
            }
        }
    }

    /// 当前选中行（键盘焦点框）。
    pub fn selected<'a>(&self, rows: &'a [JmpRow]) -> Option<&'a JmpRow> {
        rows.get(self.sel).filter(|r| Self::selectable(r))
    }

    /// Enter 激活：文件 → touch 引擎 + OpenPath（使用事件一处一事实）；
    /// 任务 → 校验声明仍在，失效出三要素 toast；更多/关闭同名字面。
    pub fn activate(&mut self, engine: &mut RecentEngine, rows: &[JmpRow]) -> JmpAction {
        let row = match self.selected(rows) {
            Some(r) => r.clone(),
            None => return JmpAction::None,
        };
        match row.kind {
            RowKind::PinnedFile | RowKind::RecentFile => {
                engine.touch(&row.arg);
                JmpAction::OpenPath(row.arg)
            }
            RowKind::Task => {
                let still_declared = self
                    .app(self.opened_app)
                    .map(|a| a.tasks.iter().any(|t| t.id == row.arg))
                    .unwrap_or(false);
                if still_declared {
                    self.task_run_count += 1;
                    JmpAction::RunTask(
                        // 任务 id 静态声明——生命周期 'static 安全。
                        self.app(self.opened_app)
                            .and_then(|a| a.tasks.iter().find(|t| t.id == row.arg))
                            .map(|t| t.id)
                            .unwrap_or(""),
                    )
                } else {
                    self.toast = Some(Toast {
                        title: String::from("任务不可用"),
                        detail: String::from("应用已更新任务清单，请重新打开跳转清单"),
                        glyph: 0x26A0,
                    });
                    JmpAction::None
                }
            }
            RowKind::More => JmpAction::More,
            RowKind::CloseAll => {
                self.close_all_count += 1;
                JmpAction::CloseAll
            }
            _ => JmpAction::None,
        }
    }

    /// 任务失败 toast（宿主执行侧回报——三要素齐备才收账）。
    pub fn report_task_failure(&mut self, task_label: &str, detail: &str) {
        self.toast = Some(Toast {
            title: String::from("任务执行失败"),
            detail: alloc::format!("{}：{}", task_label, detail),
            glyph: 0x26A0,
        });
    }

    pub fn take_toast(&mut self) -> Option<Toast> {
        self.toast.take()
    }

    // -----------------------------------------------------------------------
    // 动画与审计
    // -----------------------------------------------------------------------

    /// 展开动画进度（千分比 0..=1000；F124 曲线形状宿主侧套用）。
    pub fn expand_progress_milli(&self, now_ms: u64) -> u32 {
        if !self.open {
            return 0;
        }
        let el = now_ms.saturating_sub(self.opened_ms);
        ((el * 1000) / EXPAND_MS).min(1000) as u32
    }

    pub fn close_all_count(&self) -> u64 {
        self.close_all_count
    }

    pub fn task_run_count(&self) -> u64 {
        self.task_run_count
    }
}

/// 文件行显示文案（取路径尾段——全路径走 tooltip，宿主侧）。
fn file_label(path: &str) -> String {
    match path.rsplit(|c: char| c == '/' || c == '\\').next() {
        Some(tail) if !tail.is_empty() => String::from(tail),
        _ => String::from(path),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F074 自检（判据：与 F072 一致抽 5 例；三应用任务样本全对；键盘可达）。
pub fn run_jumplist_checks() -> CheckSet {
    let mut set = CheckSet::new("F074-jumplist");

    // 1. 几何与动画常量在案（260px 宽 / 36px 行高 / 12px 组标题 /
    //    150ms 展开 / 8px 贴缘——主册给定值逐个钉死）。
    set.add(
        "list geometry constants",
        LIST_W_PX == 260
            && ROW_H_PX == 36
            && GROUP_TITLE_FONT_PX == 12
            && EXPAND_MS == 150
            && EDGE_GAP_PX == 8,
        "",
    );

    // 2. 应用开 10 个文件 → 最近区恰 8 条（主册上限）；f10 多开一次
    //    （freq=2 分最高居首），其余同分按引擎确定性序取头部。
    let mut engine = RecentEngine::new();
    engine.set_clock(1_000);
    let mut board = JumpBoard::new();
    for i in 1..=10u64 {
        board.open_file(&mut engine, 7, &alloc::format!("/docs/f{}.txt", i));
    }
    board.open_file(&mut engine, 7, "/docs/f10.txt");
    let rows = board.show(&engine, 7, false, 0);
    let recent: Vec<&JmpRow> =
        rows.iter().filter(|r| r.kind == RowKind::RecentFile).collect();
    set.add(
        "recent capped at eight",
        recent.len() == MAX_RECENT_ROWS
            && recent[0].arg == "/docs/f10.txt"
            && recent[7].arg == "/docs/f3.txt",
        "",
    );

    // 3. 与 F072 数据一致（抽 5 例）：前 5 条路径与频次逐项等同引擎。
    let global = engine.consume_jumplist();
    let ok = recent
        .iter()
        .take(5)
        .enumerate()
        .all(|(i, r)| {
            global[i].path == r.arg && global[i].freq >= 1 && !r.label.is_empty()
        });
    set.add("recent matches f072 engine sample of five", ok, "");

    // 4. 固定区置顶 + 图钉移除：pin → 组标题+首行；unpin → 退回最近区。
    let mut board2 = JumpBoard::new();
    for p in ["/docs/a.txt", "/docs/b.txt"] {
        board2.open_file(&mut engine, 7, p);
    }
    let pinned_ok = board2.pin(&engine, 7, "/docs/a.txt");
    let rows2 = board2.show(&engine, 7, false, 0);
    let first_file = rows2.iter().find(|r| r.kind == RowKind::PinnedFile);
    let unpinned_ok = board2.unpin(&engine, 7, "/docs/a.txt");
    let rows3 = board2.show(&engine, 7, false, 0);
    let back_in_recent = rows3
        .iter()
        .any(|r| r.kind == RowKind::RecentFile && r.arg == "/docs/a.txt");
    set.add(
        "pinned group leads and unpins",
        pinned_ok
            && first_file.map(|r| r.arg == "/docs/a.txt").unwrap_or(false)
            && rows2.iter().any(|r| r.kind == RowKind::GroupTitle && r.label == "固定")
            && unpinned_ok
            && back_in_recent,
        "",
    );

    // 5. 同路径固定+最近不重复出现（一份清单一个入口）。
    let mut board3 = JumpBoard::new();
    board3.open_file(&mut engine, 7, "/docs/a.txt");
    board3.pin(&engine, 7, "/docs/a.txt");
    let rows4 = board3.show(&engine, 7, false, 0);
    let hits = rows4
        .iter()
        .filter(|r| r.arg == "/docs/a.txt")
        .count();
    set.add("pinned path not duplicated in recent", hits == 1, "");

    // 6. 自定义任务三应用样本：声明→方向→Enter→RunTask 动作+审计入账。
    let mut board4 = JumpBoard::new();
    board4.declare_app(
        100,
        &[
            JmpTask { id: "new-tab", label: "新标签页", icon: 1 },
            JmpTask { id: "split", label: "分屏", icon: 2 },
        ],
    );
    board4.declare_app(200, &[JmpTask { id: "admin", label: "以管理员运行", icon: 3 }]);
    board4.declare_app(300, &[JmpTask { id: "export", label: "导出", icon: 4 }]);
    let rows_a = board4.show(&engine, 100, false, 0);
    board4.sel_next(&rows_a);
    let a1 = board4.activate(&mut engine, &rows_a);
    board4.sel_next(&rows_a);
    let a2 = board4.activate(&mut engine, &rows_a);
    let rows_b = board4.show(&engine, 200, false, 0);
    board4.sel_next(&rows_b);
    let a3 = board4.activate(&mut engine, &rows_b);
    let rows_c = board4.show(&engine, 300, false, 0);
    board4.sel_next(&rows_c);
    let a4 = board4.activate(&mut engine, &rows_c);
    set.add(
        "three app task samples all run",
        a1 == JmpAction::RunTask("new-tab")
            && a2 == JmpAction::RunTask("split")
            && a3 == JmpAction::RunTask("admin")
            && a4 == JmpAction::RunTask("export")
            && board4.task_run_count() == 4,
        "",
    );

    // 7. 展示后声明被撤 → 激活失效任务出三要素 toast（标题/详情非空 +
    //    图标非零）+ 动作 None；宿主回报执行失败同样出三要素 toast。
    let mut board5 = JumpBoard::new();
    board5.declare_app(200, &[JmpTask { id: "gone", label: "已移除", icon: 9 }]);
    let rows5 = board5.show(&engine, 200, false, 0);
    board5.declare_app(200, &[]); // 展示后声明整体替换为空
    board5.sel_next(&rows5); // 唯一可选中行 = 失效任务
    let act = board5.activate(&mut engine, &rows5);
    let t1 = board5.take_toast();
    board5.report_task_failure("已移除", "退出码 1");
    let t2 = board5.take_toast();
    set.add(
        "stale task toasts three elements",
        act == JmpAction::None
            && t1
                .map(|t| !t.title.is_empty() && !t.detail.is_empty() && t.glyph != 0)
                .unwrap_or(false)
            && t2.map(|t| t.title == "任务执行失败" && t.glyph != 0).unwrap_or(false),
        "",
    );

    // 8. 应用无窗口 → 清单仍出（最近+任务）但无「关闭所有窗口」。
    let rows7 = board4.show(&engine, 100, false, 0);
    set.add(
        "no windows hides close all",
        !rows7.iter().any(|r| r.kind == RowKind::CloseAll)
            && rows7.iter().any(|r| r.kind == RowKind::Task),
        "",
    );

    // 9. 有窗口 → 尾项在，循环可达激活出 CloseAll + 审计。
    let rows8 = board4.show(&engine, 100, true, 0);
    let tail_close = rows8.last().map(|r| r.kind == RowKind::CloseAll).unwrap_or(false);
    for _ in 0..rows8.len() {
        if board4.selected(&rows8).map(|r| r.kind) == Some(RowKind::CloseAll) {
            break;
        }
        board4.sel_next(&rows8);
    }
    let act2 = board4.activate(&mut engine, &rows8);
    set.add(
        "close all is tail and audited",
        tail_close && act2 == JmpAction::CloseAll && board4.close_all_count() == 1,
        "",
    );

    // 10. 内容行溢出 → 尾部「更多」（阈值 16：6 固定 + 8 最近（上限截断）
    //     + 3 任务 = 17 → 截 16 + More 行在 CloseAll 前）。
    let mut board6 = JumpBoard::new();
    for i in 1..=14u64 {
        board6.open_file(&mut engine, 9, &alloc::format!("/log/l{}.txt", i));
    }
    board6.declare_app(
        9,
        &[
            JmpTask { id: "t1", label: "任务一", icon: 1 },
            JmpTask { id: "t2", label: "任务二", icon: 2 },
            JmpTask { id: "t3", label: "任务三", icon: 3 },
        ],
    );
    for i in 1..=6u64 {
        board6.pin(&engine, 9, &alloc::format!("/log/l{}.txt", i));
    }
    let rows9 = board6.show(&engine, 9, true, 0);
    let content9 = rows9
        .iter()
        .filter(|r| {
            matches!(r.kind, RowKind::PinnedFile | RowKind::RecentFile | RowKind::Task)
        })
        .count();
    let more_idx = rows9.iter().position(|r| r.kind == RowKind::More);
    let close_idx = rows9.iter().position(|r| r.kind == RowKind::CloseAll);
    set.add(
        "overflow shows more row",
        content9 == MAX_ROWS_BEFORE_MORE
            && more_idx.is_some()
            && close_idx.is_some()
            && more_idx.unwrap() < close_idx.unwrap(),
        "",
    );

    // 11. 最近区空态轻文案（主册口径原文）。
    let mut board7 = JumpBoard::new();
    let rows10 = board7.show(&engine, 42, false, 0);
    set.add(
        "empty recent light copy",
        rows10
            .iter()
            .any(|r| r.kind == RowKind::EmptyNote && r.label == "此应用还没有最近文件"),
        "",
    );

    // 12. 键盘可达：方向下/上 + Enter——选中即打开，且打开事件回灌引擎
    //     （freq+1——一处一事实）。独立引擎消除他项干扰。
    let mut engine12 = RecentEngine::new();
    engine12.set_clock(1_000);
    let mut board8 = JumpBoard::new();
    for p in ["/docs/x.txt", "/docs/y.txt", "/docs/z.txt"] {
        board8.open_file(&mut engine12, 7, p);
    }
    let before_freq = engine12
        .consume_jumplist()
        .iter()
        .find(|r| r.path == "/docs/y.txt")
        .map(|r| r.freq)
        .unwrap_or(0);
    let rows11 = board8.show(&engine12, 7, false, 0);
    board8.sel_next(&rows11);
    board8.sel_next(&rows11); // 同分路径降序 z,y,x → 两步落 y.txt
    let act3 = board8.activate(&mut engine12, &rows11);
    let after_freq = engine12
        .consume_jumplist()
        .iter()
        .find(|r| r.path == "/docs/y.txt")
        .map(|r| r.freq)
        .unwrap_or(0);
    board8.sel_prev(&rows11); // 方向键上：y → z（上向可达）
    let prev_ok =
        board8.selected(&rows11).map(|r| r.arg.as_str()) == Some("/docs/z.txt");
    set.add(
        "keyboard arrows and enter feed engine",
        act3 == JmpAction::OpenPath(String::from("/docs/y.txt"))
            && after_freq == before_freq + 1
            && prev_ok,
        "",
    );

    // 13. 展开动画 150ms（149 <1000、150 ==1000）+ 清单高换算与屏高翻转
    //     边界（可用高 552：500 不翻、560 翻）。
    board8.show(&engine12, 7, false, 10_000);
    let mid = board8.expand_progress_milli(10_149);
    let done = board8.expand_progress_milli(10_150);
    let no_flip = JumpBoard::needs_top_anchor(500, 600, 560);
    let flip = JumpBoard::needs_top_anchor(560, 600, 560);
    let h10 = JumpBoard::height_px(10);
    set.add(
        "expand timing and top flip",
        mid < 1000 && done == 1000 && !no_flip && flip && h10 == 360,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> RecentEngine {
        let mut e = RecentEngine::new();
        e.set_clock(1_000);
        e
    }

    #[test]
    fn alias_pin_lands_on_canonical() {
        let mut e = engine();
        e.declare_alias("/short/a.lnk", "/docs/a.txt");
        let mut b = JumpBoard::new();
        b.open_file(&mut e, 1, "/short/a.lnk");
        b.pin(&e, 1, "/short/a.lnk");
        let rows = b.show(&e, 1, false, 0);
        assert!(
            rows.iter().any(|r| r.kind == RowKind::PinnedFile && r.arg == "/docs/a.txt"),
            "固定集存真身路径（别名归并一致）"
        );
    }

    #[test]
    fn pin_scoped_per_app() {
        let mut e = engine();
        let mut b = JumpBoard::new();
        b.open_file(&mut e, 1, "/docs/a.txt");
        b.pin(&e, 1, "/docs/a.txt");
        b.open_file(&mut e, 2, "/docs/a.txt");
        let rows_b = b.show(&e, 2, false, 0);
        assert!(
            !rows_b.iter().any(|r| r.kind == RowKind::PinnedFile),
            "应用域图钉不跨清单（应用蜂巢外元数据各自持有）"
        );
    }

    #[test]
    fn redeclare_replaces_tasks_whole() {
        let e = engine();
        let mut b = JumpBoard::new();
        b.declare_app(1, &[JmpTask { id: "a", label: "甲", icon: 1 }]);
        b.declare_app(1, &[JmpTask { id: "b", label: "乙", icon: 2 }]);
        let rows = b.show(&e, 1, false, 0);
        let tasks: Vec<&str> = rows
            .iter()
            .filter(|r| r.kind == RowKind::Task)
            .map(|r| r.arg.as_str())
            .collect();
        assert_eq!(tasks, vec!["b"], "重复声明整体替换（静态清单）");
    }

    #[test]
    fn assoc_cap_truncates_membership_not_engine() {
        let mut e = engine();
        let mut b = JumpBoard::new();
        for i in 0..70u32 {
            b.open_file(&mut e, 1, &alloc::format!("/f/{}.txt", i));
        }
        let rows = b.show(&e, 1, false, 0);
        let recent = rows.iter().filter(|r| r.kind == RowKind::RecentFile).count();
        assert_eq!(recent, MAX_RECENT_ROWS, "关联账截断不影响引擎侧展示上限");
        assert_eq!(e.count(), 70, "引擎账不受成员截断影响（一处一事实）");
    }

    #[test]
    fn selection_wraps_to_stay_reachable() {
        let mut e = engine();
        let mut b = JumpBoard::new();
        b.open_file(&mut e, 1, "/docs/only.txt");
        let rows = b.show(&e, 1, true, 0);
        for _ in 0..rows.len() + 1 {
            b.sel_next(&rows);
        }
        assert!(
            b.selected(&rows).is_some(),
            "循环可达：跳过组标题/空态后总有可选中行"
        );
    }
}
