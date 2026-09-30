//! F236 窗口排列命令 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F236（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：右键任务栏空白处三命令——层叠窗口
//! （斜向偏移 24px 叠放、标题栏全可见）、横向平铺、纵向平铺，一键整理
//! 所有普通窗口（浮层与最小化窗不参与）。
//!
//! **设计要点**：
//! - 三命令统一求解器：层叠=24px 斜偏移链 + 标题栏全可见钳制（工作区
//!   边界内 y ≤ bottom-32）；横/纵平铺=工作区均分 + F214 最小尺寸约束；
//! - 放不下的窗口进降级清单：部分最小化 + 菜单项标注事件
//!   （「（窗口过多，部分最小化）」），降级只发生在最小尺寸约束下；
//! - 撤销快照：执行前对参与窗全量几何快照，Ctrl+Z 精确恢复
//!   （整数像素域恢复精度 0 < 1px，验收「<1px」）；
//! - 参与资格过滤：浮层（always-on-top）与最小化窗一律不参与、
//!   几何不动——与 Windows 任务栏右键行为对齐；
//! - 多显示器：各屏独立整理（只排列与目标屏相交的窗，其余屏的窗
//!   几何不动），菜单文案与加速键与 Windows 任务栏右键对齐；
//! - 操作账本：分钟聚合 [层叠/横铺/纵铺/降级] 四计数器，保留 30 天
//!   （与全域统计通路一致，复用 sbase MinuteBook）。
//!
//! **依赖锚点**：几何用 [`crate::h1star::h1base::Rect`]（clamped_into
//! 是层叠钳制的唯一落点）；账本复用 [`crate::star::sbase::MinuteBook`]；
//! 最小尺寸锚 F214（本模块自带取值，接线时以 F214 实装常量为准——
//! 一处一事实的接缝约定）；时间由调用方注入。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;
use crate::star::sbase::MinuteBook;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 层叠斜向偏移——主册 F236「斜向偏移 24px 叠放」。
pub const CASCADE_OFFSET_PX: i32 = 24;

/// 标题栏高度——主册 F236「标题栏全可见」的钳制带宽（与 F213 标题栏
/// 几何一致）。
pub const TITLEBAR_H: i32 = 32;

/// F214 法定最小宽度锚点（本模块取值；接线时以 F214 实装常量为准）。
pub const MIN_WIN_W: i32 = 160;

/// F214 法定最小高度锚点（本模块取值；接线时以 F214 实装常量为准）。
pub const MIN_WIN_H: i32 = 120;

/// 菜单项降级标注——主册 F236「在菜单项上注明（窗口过多，部分最小化）」。
pub const DEGRADE_NOTE: &str = "（窗口过多，部分最小化）";

/// 撤销历史深度（执行记录环，Ctrl+Z 可连续回溯）。
pub const HISTORY_CAP: usize = 16;

/// 显示器槽上限（多屏独立整理的登记面）。
pub const MONITOR_CAP: usize = 4;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 参与排列的窗口几何快照（值语义，整像素域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapWin {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 最小化窗不参与（主册 F236）。
    pub minimized: bool,
    /// 浮层（always-on-top）不参与（主册 F236）。
    pub floater: bool,
}

impl SnapWin {
    fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn with_rect(&self, r: Rect) -> SnapWin {
        SnapWin { x: r.x, y: r.y, w: r.w, h: r.h, ..*self }
    }
}

/// 三命令（与 Windows 任务栏右键对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapCmd {
    /// 层叠窗口：24px 斜偏移链，标题栏全可见。
    Cascade,
    /// 横向平铺：每窗全宽、等高分行。
    TileRows,
    /// 纵向平铺：每窗全高、等宽分列。
    TileCols,
}

impl SnapCmd {
    /// 命令名（诊断面/账本口径）。
    pub fn name(&self) -> &'static str {
        match self {
            SnapCmd::Cascade => "cascade",
            SnapCmd::TileRows => "tile-rows",
            SnapCmd::TileCols => "tile-cols",
        }
    }

    /// 命令在账本计数器列中的下标。
    fn ledger_col(&self) -> usize {
        match self {
            SnapCmd::Cascade => 0,
            SnapCmd::TileRows => 1,
            SnapCmd::TileCols => 2,
        }
    }
}

/// 排列结果：放下的几何 + 降级清单 + 菜单标注事件。
#[derive(Clone, Debug)]
pub struct SnapResult {
    /// 放下的窗口（已按命令重排几何）。
    pub placed: Vec<SnapWin>,
    /// 降级最小化的窗口 id（保序）。
    pub degraded: Vec<u32>,
    /// 菜单项标注事件（与 degraded 非空等价）。
    pub annotated: bool,
    pub cmd: SnapCmd,
}

/// 任务栏右键菜单项（三命令 + 可选降级标注，与 Windows 文案对齐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    /// 菜单文案（含助记符）。
    pub label: &'static str,
    /// 加速键。
    pub accel: &'static str,
    /// 该项是否带降级标注（「（窗口过多，部分最小化）」）。
    pub note: bool,
}

/// 生成任务栏右键菜单：三命令固定序，最近一次执行的降级标注落到
/// 对应命令项上（层叠永不降级，标注恒 false）。
pub fn menu_entries(last_cmd: Option<SnapCmd>, annotated: bool) -> [MenuEntry; 3] {
    [
        MenuEntry {
            label: "层叠窗口(&W)",
            accel: "Shift+Ctrl+W",
            note: annotated && last_cmd == Some(SnapCmd::Cascade),
        },
        MenuEntry {
            label: "横向平铺(&H)",
            accel: "Shift+Ctrl+H",
            note: annotated && last_cmd == Some(SnapCmd::TileRows),
        },
        MenuEntry {
            label: "纵向平铺(&V)",
            accel: "Shift+Ctrl+V",
            note: annotated && last_cmd == Some(SnapCmd::TileCols),
        },
    ]
}

// ---------------------------------------------------------------------------
// 纯求解器（无状态，宿主可直测）
// ---------------------------------------------------------------------------

/// 参与资格：浮层与最小化窗排除（主册 F236 原文）。
pub fn eligible(w: &SnapWin) -> bool {
    !w.minimized && !w.floater
}

/// 显示器归属：与目标屏正面积相交即归属（跨屏窗跟随其主显屏整理）。
pub fn on_monitor(w: &SnapWin, m: &Rect) -> bool {
    w.rect().intersect_area(m) > 0
}

/// 恢复精度：两几何逐项最大差（整像素域，恢复精确=0 < 1px 达标）。
pub fn geom_diff_px(a: &SnapWin, b: &SnapWin) -> i32 {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.w - b.w).abs())
        .max((a.h - b.h).abs())
}

/// 层叠求解：第 i 窗偏移 (i·24, i·24)，钳进工作区保证标题栏全可见。
///
/// 钳制语义：clamped_into 保证 x ≤ right-w、y ≤ bottom-h；h ≥ 标题栏
/// 32px 时 y ≤ bottom-32 自动成立（标题栏全可见的唯一前提）。
/// 层叠永不降级（只钳制不挤占），降级清单恒空。
pub fn cascade(wins: &[SnapWin], work: &Rect) -> Vec<SnapWin> {
    let mut out = Vec::with_capacity(wins.len());
    for (i, w) in wins.iter().enumerate() {
        let off = (i as i32) * CASCADE_OFFSET_PX;
        let target = Rect::new(work.x + off, work.y + off, w.w.max(1), w.h.max(1));
        let r = target.clamped_into(work);
        out.push(w.with_rect(r));
    }
    out
}

/// 平铺求解（rows=横向平铺/等高分行；cols=纵向平铺/等宽分列）。
///
/// 降级规则：可用容量 = work 边长 / F214 最小边（向下取整，至少 1）；
/// 超出容量的窗（从队尾）进降级清单并最小化，菜单标注事件同步置位。
/// 放下的窗均分工作区（最后一窗收边长余数，保证铺满不留缝）。
pub fn tile(wins: &[SnapWin], work: &Rect, cols: bool) -> SnapResult {
    let n = wins.len();
    let mut placed = Vec::with_capacity(n);
    let mut degraded = Vec::new();
    let cmd = if cols { SnapCmd::TileCols } else { SnapCmd::TileRows };
    if n == 0 {
        return SnapResult { placed, degraded, annotated: false, cmd };
    }
    let span = if cols { work.w } else { work.h };
    let min_span = if cols { MIN_WIN_W } else { MIN_WIN_H };
    // 容量：整份 ≥ 最小尺寸；工作区本身不足一份最小尺寸时保 1 份
    // （钳进工作区，尺寸不回绕——F214 钳制纪律）。
    let fit_max = (span / min_span).max(1) as usize;
    let fit = n.min(fit_max);
    let share = span / fit as i32;
    let mut x = work.x;
    let mut y = work.y;
    for (i, w) in wins.iter().take(fit).enumerate() {
        let last = i + 1 == fit;
        let s = if last { span - share * (fit as i32 - 1) } else { share };
        let r = if cols {
            Rect::new(x, work.y, s, work.h)
        } else {
            Rect::new(work.x, y, work.w, s)
        };
        placed.push(w.with_rect(r));
        if cols {
            x += s;
        } else {
            y += s;
        }
    }
    for w in wins.iter().skip(fit) {
        degraded.push(w.id);
    }
    let annotated = !degraded.is_empty();
    SnapResult { placed, degraded, annotated, cmd }
}

/// 放置结果是否完全在工作区内（fuzz 不变量）。
fn inside(work: &Rect, r: &Rect) -> bool {
    r.x >= work.x && r.y >= work.y && r.right() <= work.right() && r.bottom() <= work.bottom()
}

// ---------------------------------------------------------------------------
// 排列引擎（快照撤销 + 多显示器）
// ---------------------------------------------------------------------------

/// 一次执行的撤销记录（定容环，执行历史可回溯）。
#[derive(Clone, Debug)]
struct ArrangeRecord {
    cmd: SnapCmd,
    /// 执行前参与窗的原始几何（撤销恢复源）。
    before: Vec<SnapWin>,
    /// 执行后的几何（对照面）。
    after: Vec<SnapWin>,
    degraded: Vec<u32>,
    annotated: bool,
    /// 执行所在显示器（多屏诊断面；单屏场景恒 0）。
    monitor: usize,
}

/// 排列引擎：执行三命令 + Ctrl+Z 撤销（精确恢复 <1px）+ 多屏独立整理。
pub struct ArrangeEngine {
    history: Vec<ArrangeRecord>,
    monitors: Vec<Rect>,
    undo_used: u32,
}

impl ArrangeEngine {
    pub fn new() -> ArrangeEngine {
        ArrangeEngine { history: Vec::new(), monitors: Vec::new(), undo_used: 0 }
    }

    /// 登记工作区/显示器（返回槽位号；上限 4，非法几何拒绝）。
    pub fn attach_monitor(&mut self, rect: Rect) -> Option<usize> {
        if rect.w <= 0 || rect.h <= 0 || self.monitors.len() >= MONITOR_CAP {
            return None;
        }
        self.monitors.push(rect);
        Some(self.monitors.len() - 1)
    }

    pub fn monitor(&self, idx: usize) -> Option<Rect> {
        self.monitors.get(idx).copied()
    }

    pub fn monitor_count(&self) -> usize {
        self.monitors.len()
    }

    /// 统一执行核：对已过滤的参与窗求解（execute 两条入口共用）。
    fn apply(cmd: SnapCmd, participating: &[SnapWin], work: &Rect) -> SnapResult {
        match cmd {
            SnapCmd::Cascade => {
                let placed = cascade(participating, work);
                SnapResult { placed, degraded: Vec::new(), annotated: false, cmd }
            }
            SnapCmd::TileRows => tile(participating, work, false),
            SnapCmd::TileCols => tile(participating, work, true),
        }
    }

    fn push_history(&mut self, cmd: SnapCmd, before: Vec<SnapWin>, result: &SnapResult, monitor: usize) {
        self.history.push(ArrangeRecord {
            cmd,
            before,
            after: result.placed.clone(),
            degraded: result.degraded.clone(),
            annotated: result.annotated,
            monitor,
        });
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
    }

    /// 执行命令（主显示器场景）：先快照参与窗原几何，再求解。
    pub fn execute(&mut self, cmd: SnapCmd, wins: &[SnapWin], work: &Rect) -> SnapResult {
        let participating: Vec<SnapWin> =
            wins.iter().filter(|w| eligible(w)).copied().collect();
        let result = Self::apply(cmd, &participating, work);
        self.push_history(cmd, participating, &result, 0);
        result
    }

    /// 按显示器执行（多屏纪律）：只排列与该屏相交且资格合格的窗，
    /// 其他屏的窗几何不动；无可登记屏返回 None。
    pub fn execute_on_monitor(
        &mut self,
        cmd: SnapCmd,
        wins: &[SnapWin],
        idx: usize,
    ) -> Option<SnapResult> {
        let work = self.monitors.get(idx).copied()?;
        let participating: Vec<SnapWin> = wins
            .iter()
            .filter(|w| eligible(w) && on_monitor(w, &work))
            .copied()
            .collect();
        let result = Self::apply(cmd, &participating, &work);
        self.push_history(cmd, participating, &result, idx);
        Some(result)
    }

    /// Ctrl+Z 撤销：恢复最近一次执行前几何（整像素精确，差 0 < 1px）。
    /// 返回 (命令, 恢复几何)；无可撤销时 None。
    pub fn undo(&mut self) -> Option<(SnapCmd, Vec<SnapWin>)> {
        let rec = self.history.pop()?;
        self.undo_used += 1;
        Some((rec.cmd, rec.before))
    }

    /// 最近一次执行的命令（菜单标注定位用）。
    pub fn last_cmd(&self) -> Option<SnapCmd> {
        self.history.last().map(|r| r.cmd)
    }

    /// 最近一次记录的降级标注（菜单渲染面读这里）。
    pub fn last_annotated(&self) -> Option<bool> {
        self.history.last().map(|r| r.annotated)
    }

    /// 撤销次数（诊断面）。
    pub fn undo_count(&self) -> u32 {
        self.undo_used
    }

    /// 历史深度（诊断面）。
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

impl Default for ArrangeEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 操作账本（分钟聚合，30 天保留）
// ---------------------------------------------------------------------------

/// 账本计数器列：0 层叠 / 1 横铺 / 2 纵铺 / 3 降级事件。
pub const LEDGER_COLS: usize = 4;

/// 排列操作账本：分钟聚合四计数器，保留 30 天（43200 分钟）。
pub struct SnapLedger {
    book: MinuteBook,
}

impl SnapLedger {
    pub fn new() -> SnapLedger {
        SnapLedger { book: MinuteBook::new(LEDGER_COLS, 30 * 1440) }
    }

    /// 记一笔操作（含降级窗口数——降级事件独立计数）。
    pub fn record(&mut self, cmd: SnapCmd, degraded: usize, minute: u64) {
        let mut vals = [0u64; LEDGER_COLS];
        vals[cmd.ledger_col()] = 1;
        vals[3] = degraded as u64;
        self.book.record_minute(minute, &vals);
    }

    /// 区间聚合 [cascade, rows, cols, degraded]。
    pub fn range_sum(&self, from_min: u64, to_min: u64) -> [u64; LEDGER_COLS] {
        let v = self.book.range_sum(from_min, to_min);
        [v[0], v[1], v[2], v[3]]
    }

    /// 按绝对时间驱逐窗口外槽（保留窗纪律）。
    pub fn evict_by_now(&mut self, now_min: u64) -> usize {
        self.book.evict_by_now(now_min)
    }

    /// 现存分钟槽数（诊断面）。
    pub fn slot_count(&self) -> usize {
        self.book.slot_count()
    }
}

impl Default for SnapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// 构造 n 个普通窗（带 id 与初始几何）。
fn mk_wins(n: u32, seed: u32) -> Vec<SnapWin> {
    let mut x = seed | 1;
    let mut v = Vec::new();
    for i in 0..n {
        let r = xors32(&mut x);
        v.push(SnapWin {
            id: 100 + i,
            x: (r % 400) as i32,
            y: ((r >> 8) % 300) as i32,
            w: 400 + ((r >> 16) % 200) as i32,
            h: 300 + ((r >> 24) % 150) as i32,
            minimized: false,
            floater: false,
        });
    }
    v
}

/// F236 自检（判据：三命令几何 3/5/8 三档 + 降级标注 + 撤销 <1px
/// + 资格过滤 + 多屏独立 + 菜单文案 + 账本）。
pub fn run_winsnap_checks() -> CheckSet {
    let mut set = CheckSet::new("F236-winsnap");
    let work = Rect::new(0, 0, 1600, 900);

    // 1. 层叠 3 窗：相邻窗斜偏移恰为 24px。
    let wins3 = mk_wins(3, 0x1234);
    let c3 = cascade(&wins3, &work);
    let stair = c3[1].x == c3[0].x + CASCADE_OFFSET_PX
        && c3[1].y == c3[0].y + CASCADE_OFFSET_PX
        && c3[2].x == c3[1].x + CASCADE_OFFSET_PX
        && c3[2].y == c3[1].y + CASCADE_OFFSET_PX;
    set.add("cascade 3 windows 24px stair", stair, "");

    // 2. 层叠钳制：8 窗小工作区也不出界，且 y ≤ bottom-32（标题栏全可见）。
    let small = Rect::new(0, 0, 800, 500);
    let wins8 = mk_wins(8, 0x8765);
    let c8 = cascade(&wins8, &small);
    let all_in = c8.iter().all(|w| inside(&small, &w.rect()));
    let tb_visible = c8.iter().all(|w| w.y + TITLEBAR_H <= small.bottom());
    set.add("cascade clamped, titlebar visible", all_in && tb_visible, "");

    // 3. 横向平铺 5 窗：等高分行、全宽、铺满工作区（无竖缝）。
    let t5 = tile(&mk_wins(5, 0xAAAA), &work, false);
    let rows_ok = t5.placed.len() == 5
        && t5.placed.iter().all(|w| w.x == work.x && w.w == work.w)
        && t5.placed[0].h == 900 / 5
        && (t5.placed.iter().map(|w| w.h).sum::<i32>()) == work.h;
    set.add("tile-rows 5 windows equal bands", rows_ok, "");

    // 4. 纵向平铺 8 窗：等宽分列、全高、铺满工作区（无横缝）。
    let t8 = tile(&mk_wins(8, 0xBBBB), &work, true);
    let cols_ok = t8.placed.len() == 8
        && t8.placed.iter().all(|w| w.y == work.y && w.h == work.h)
        && (t8.placed.iter().map(|w| w.w).sum::<i32>()) == work.w
        && !t8.annotated;
    set.add("tile-cols 8 windows equal columns", cols_ok, "");

    // 5. 三档走查：3/5/8 窗在 1600×900 工作区下纵向平铺均不降级
    //    （8×160=1280 ≤ 1600），放下部分满足最小尺寸。
    let tiers = [3usize, 5, 8];
    let mut tiers_ok = true;
    for n in tiers {
        let r = tile(&mk_wins(n as u32, 0xC000 + n as u32), &work, true);
        if !r.degraded.is_empty()
            || r.annotated
            || r.placed.len() != n
            || r.placed.iter().any(|w| w.w < MIN_WIN_W || w.h < MIN_WIN_H)
        {
            tiers_ok = false;
        }
    }
    set.add("tiers 3/5/8 no degrade on 1600x900", tiers_ok, "");

    // 6. 降级触发：400px 宽工作区放 8 窗纵向平铺 → 只放 2 列（400/160），
    //    其余 6 窗进降级清单 + 菜单标注；放下部分满足最小尺寸。
    let tiny = Rect::new(0, 0, 400, 900);
    let td = tile(&mk_wins(8, 0xDDDD), &tiny, true);
    set.add(
        "degrade on overflow + annotated + min size",
        td.placed.len() == 2
            && td.degraded.len() == 6
            && td.annotated
            && td.placed.iter().all(|w| w.w >= MIN_WIN_W && w.h >= MIN_WIN_H),
        "",
    );

    // 7. 资格过滤：浮层与最小化窗不参与、几何不动。
    //    [缺陷账本] 现象：本检查项红。根因：检查项直调纯求解器 `tile`
    //    ——而主册「浮层与最小化窗不参与」的资格过滤在执行核
    //    `ArrangeEngine::execute`（先 eligible 过滤再求解，见模块
    //    设计要点），绕过执行核自然全数放下，属检查项调用面错误。
    //    修法：改检查项走 execute 通路，断言不变。
    let mut mixed = mk_wins(4, 0xEEEE);
    mixed[1].floater = true;
    mixed[2].minimized = true;
    let mut eng7 = ArrangeEngine::new();
    let r = eng7.execute(SnapCmd::TileCols, &mixed, &work);
    set.add(
        "floater & minimized excluded",
        r.placed.len() == 2
            && r.placed.iter().all(|w| w.id != mixed[1].id && w.id != mixed[2].id),
        "",
    );

    // 8. 撤销：执行前快照 → 恢复逐窗差 0（<1px 验收）。
    let mut engine = ArrangeEngine::new();
    let before = mk_wins(4, 0x2222);
    let _ = engine.execute(SnapCmd::TileCols, &before, &work);
    let undo_ok = match engine.undo() {
        Some((cmd, wins)) => {
            cmd == SnapCmd::TileCols
                && wins.len() == 4
                && wins.iter().zip(before.iter()).all(|(a, b)| geom_diff_px(a, b) < 1)
        }
        None => false,
    };
    set.add("undo restores geometry <1px", undo_ok, "");

    // 9. 撤销只回一步：二次撤销 None；历史深度随之递减。
    set.add(
        "second undo none, history shrinks",
        engine.undo().is_none() && engine.history_len() == 0 && engine.undo_count() == 1,
        "",
    );

    // 10. 菜单标注事件与降级清单等价（三命令 × 两工作区）。
    let mut equiv = true;
    for cmd in [SnapCmd::Cascade, SnapCmd::TileRows, SnapCmd::TileCols] {
        let mut eng = ArrangeEngine::new();
        let r_big = eng.execute(cmd, &mk_wins(6, 0x3333), &work);
        if r_big.annotated != !r_big.degraded.is_empty()
            || eng.last_annotated() != Some(r_big.annotated)
        {
            equiv = false;
        }
        let r_small = eng.execute(cmd, &mk_wins(6, 0x4444), &tiny);
        if cmd == SnapCmd::Cascade && (r_small.annotated || !r_small.degraded.is_empty()) {
            equiv = false; // 层叠永不降级（只钳制）。
        }
        if cmd != SnapCmd::Cascade && r_small.annotated != !r_small.degraded.is_empty() {
            equiv = false;
        }
    }
    set.add("annotation iff degrade (3 cmds)", equiv, "");

    // 11. 菜单文案三命令 + 降级标注只落在降级的命令项上。
    let menu = menu_entries(Some(SnapCmd::TileCols), true);
    let menu_ok = menu.len() == 3
        && menu[0].label == "层叠窗口(&W)"
        && !menu[0].note
        && !menu[1].note
        && menu[2].note
        && menu[2].label.ends_with("(&V)");
    let menu_clean = menu_entries(Some(SnapCmd::Cascade), false).iter().all(|e| !e.note);
    set.add("taskbar menu labels & note placement", menu_ok && menu_clean, "");

    // 12. 多屏独立整理：左屏放窗、右屏放窗，对右屏执行只动右屏的窗。
    let mut eng2 = ArrangeEngine::new();
    let left = Rect::new(0, 0, 1600, 900);
    let right = Rect::new(1600, 0, 1600, 900);
    assert!(eng2.attach_monitor(left).is_some() && eng2.attach_monitor(right).is_some());
    let mut two_sides = mk_wins(4, 0x5555);
    for (i, w) in two_sides.iter_mut().enumerate() {
        if i < 2 {
            w.x = 100 + i as i32 * 20; // 左屏
        } else {
            w.x = 1700 + i as i32 * 20; // 右屏
        }
    }
    let orig_left0 = two_sides[0];
    let r_mon = eng2.execute_on_monitor(SnapCmd::TileCols, &two_sides, 1).unwrap();
    set.add(
        "per-monitor arrange isolates screens",
        r_mon.placed.len() == 2
            && r_mon.placed.iter().all(|w| w.x >= right.x && w.x + w.w <= right.right())
            && geom_diff_px(&two_sides[0], &orig_left0) == 0,
        "",
    );
    set.add("monitor attach rejects overflow", eng2.monitor_count() == 2, "");

    // 13. 操作账本：分钟聚合 + 区间求和 + 保留窗驱逐。
    let mut ledger = SnapLedger::new();
    ledger.record(SnapCmd::Cascade, 0, 10);
    ledger.record(SnapCmd::TileRows, 2, 10);
    ledger.record(SnapCmd::TileCols, 0, 11);
    ledger.record(SnapCmd::TileCols, 4, 12);
    let s = ledger.range_sum(10, 12);
    set.add(
        "ledger aggregates ops & degrades",
        s[0] == 1 && s[1] == 1 && s[2] == 2 && s[3] == 6 && ledger.slot_count() == 3,
        "",
    );
    // [缺陷账本] 现象：ledger evicts by now 红。根因：检查项在分钟
    // 10..12 上调 evict_by_now(12)，而 MinuteBook 契约是「now 之前
    // cap_minutes 外的槽全丢」（保留窗 30 天 = 43200 分钟）——12 分钟
    // 处没有任何槽越出 30 天保留窗，驱逐 0 个是契约的正确行为，属
    // 检查项把驱逐语义误当「now 之前全清」。修法：改检查项，now 取
    // 保留窗界外（43200+12），断言 slot_count == 1 不变（恰留 minute 12）。
    set.add("ledger evicts by now", { ledger.evict_by_now(30 * 1440 + 12); ledger.slot_count() == 1 }, "");

    // 14. xors32 fuzz：随机窗集 × 随机工作区 × 三命令，不变量=
    //     所有放下几何完全在工作区内、放下+降级=参与窗总数、不 panic。
    let mut x: u32 = 0x9E37_79B9;
    let mut ok = true;
    for _ in 0..1000u32 {
        let n = (xors32(&mut x) % 12) + 1;
        let wins = mk_wins(n, xors32(&mut x));
        let wx = (xors32(&mut x) % 500) as i32;
        let wy = (xors32(&mut x) % 300) as i32;
        let ww = 300 + (xors32(&mut x) % 1700) as i32;
        let wh = 200 + (xors32(&mut x) % 900) as i32;
        let wrect = Rect::new(wx, wy, ww, wh);
        let cmd = match xors32(&mut x) % 3 {
            0 => SnapCmd::Cascade,
            1 => SnapCmd::TileRows,
            _ => SnapCmd::TileCols,
        };
        let mut eng = ArrangeEngine::new();
        let r = eng.execute(cmd, &wins, &wrect);
        let part = wins.iter().filter(|w| eligible(w)).count() as u32;
        let in_bounds = r.placed.iter().all(|w| inside(&wrect, &w.rect()));
        let conserved = (r.placed.len() as u32) + (r.degraded.len() as u32) == part;
        if !in_bounds || !conserved || r.placed.iter().any(|w| w.w <= 0 || w.h <= 0) {
            ok = false;
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_offsets_and_clamp() {
        let work = Rect::new(0, 0, 1200, 800);
        let mut wins = Vec::new();
        for i in 0..40u32 {
            wins.push(SnapWin { id: i, x: 0, y: 0, w: 600, h: 400, minimized: false, floater: false });
        }
        let out = cascade(&wins, &work);
        // 前 10 窗按 24px 递进。
        for i in 1..10 {
            assert_eq!(out[i].x, out[i - 1].x + 24);
            assert_eq!(out[i].y, out[i - 1].y + 24);
        }
        // 越界窗口被钳回：全部在工作区内且标题栏全可见。
        for w in &out {
            assert!(inside(&work, &w.rect()));
            assert!(w.y + TITLEBAR_H <= work.bottom());
        }
        // 最后几窗 x 贴右缘（600 宽窗在 1200 宽工作区 x ≤ 600）。
        assert!(out[39].x <= work.right() - out[39].w);
    }

    #[test]
    fn tile_rows_remainder_fills() {
        let work = Rect::new(0, 0, 1000, 901); // 901/3 = 300 余 1 → 末行 301
        let wins = mk_wins(3, 0x7777);
        let r = tile(&wins, &work, false);
        assert_eq!(r.placed.len(), 3);
        assert_eq!(r.placed[0].h, 300);
        assert_eq!(r.placed[2].h, 301, "末行收余数，铺满不留缝");
        assert_eq!(r.placed.iter().map(|w| w.h).sum::<i32>(), 901);
        assert!(r.placed.windows(2).all(|p| p[1].y == p[0].y + p[0].h), "行间无缝");
    }

    #[test]
    fn tile_single_window_fullscreen() {
        let work = Rect::new(10, 20, 1900, 1000);
        let r = tile(&mk_wins(1, 0x9), &work, true);
        assert_eq!(r.placed[0].rect(), work);
        assert!(!r.annotated);
    }

    #[test]
    fn empty_input_noop() {
        let work = Rect::new(0, 0, 800, 600);
        let r = tile(&[], &work, true);
        assert!(r.placed.is_empty() && !r.annotated && r.cmd == SnapCmd::TileCols);
        assert!(cascade(&[], &work).is_empty());
        // 无可登记显示器时按屏执行显性失败。
        let mut eng = ArrangeEngine::new();
        assert!(eng.execute_on_monitor(SnapCmd::Cascade, &mk_wins(1, 1), 0).is_none());
    }

    #[test]
    fn per_monitor_arrange_uses_only_local_windows() {
        let mut eng = ArrangeEngine::new();
        let left = Rect::new(0, 0, 1000, 800);
        let right = Rect::new(1000, 0, 1000, 800);
        assert!(eng.attach_monitor(left).is_some());
        assert!(eng.attach_monitor(right).is_some());
        // 非法几何（零尺寸）登记显性拒绝。
        assert!(eng.attach_monitor(Rect::new(0, 0, 0, 100)).is_none());
        assert_eq!(eng.monitor_count(), 2);
        let mut wins = Vec::new();
        for i in 0..6u32 {
            wins.push(SnapWin {
                id: i,
                x: if i < 3 { 50 + i as i32 * 30 } else { 1100 + i as i32 * 30 },
                y: 100,
                w: 300,
                h: 200,
                minimized: false,
                floater: false,
            });
        }
        let r = eng.execute_on_monitor(SnapCmd::TileCols, &wins, 0).unwrap();
        assert_eq!(r.placed.len(), 3, "只整理左屏 3 窗");
        assert!(r.placed.iter().all(|w| w.x >= 0 && w.x + w.w <= 1000));
        // 右屏 3 窗几何未被触碰。
        for w in wins.iter().skip(3) {
            assert!(w.x >= 1100);
        }
    }

    #[test]
    fn undo_after_multiple_executes_pops_last() {
        let work = Rect::new(0, 0, 1600, 900);
        let mut eng = ArrangeEngine::new();
        let a = mk_wins(3, 0x11);
        let b = mk_wins(4, 0x22);
        let _ = eng.execute(SnapCmd::Cascade, &a, &work);
        let _ = eng.execute(SnapCmd::TileRows, &b, &work);
        assert_eq!(eng.history_len(), 2);
        assert_eq!(eng.last_cmd(), Some(SnapCmd::TileRows));
        let (cmd, wins) = eng.undo().unwrap();
        assert_eq!(cmd, SnapCmd::TileRows);
        assert_eq!(wins.len(), 4);
        for (got, want) in wins.iter().zip(b.iter()) {
            assert_eq!(geom_diff_px(got, want), 0, "整像素恢复差必须为 0");
        }
        let (cmd2, _) = eng.undo().unwrap();
        assert_eq!(cmd2, SnapCmd::Cascade);
        assert!(eng.undo().is_none());
    }

    #[test]
    fn ledger_ring_and_eviction() {
        let mut led = SnapLedger::new();
        for m in 0..40u64 {
            led.record(SnapCmd::TileRows, (m % 3) as usize, m);
        }
        let s = led.range_sum(0, 39);
        assert_eq!(s[1], 40);
        assert_eq!(s[3], 39, "m%3 求和：13×1 + 13×2 = 39");
        // 分钟 0..29 的槽在 30 天保留窗外（now = 43200+30）——全驱逐。
        assert_eq!(led.evict_by_now(30 * 1440 + 30), 30, "保留窗外 30 槽全驱逐");
        assert_eq!(led.slot_count(), 10);
    }

    #[test]
    fn winsnap_selfcheck_all_green() {
        let set = run_winsnap_checks();
        assert!(set.all_passed(), "F236 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F236。持久化面 = 撤销快照（前态矩形组）framed 记录；
// 壳接线面 = 降级触发判定（窗数×最小尺寸矩阵）+ DPI 四档换算；
// 判定面 = run_winsnap_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 单快照窗口数上限（容量上限在册：撤销快照按参与窗全量入册，
/// 超出 16 窗的快照显性 dropped 记账——不静默）。
pub const V2_SNAP_WIN_CAP: usize = 16;

/// 记录容量上限在册：cmd u8 + count u8 + 16 窗 × (id u32 + xywh i32
/// ×4 + flags u8) = 338 字节 payload（变长，按实际窗数写）。
pub const V2_PAYLOAD_MAX: usize = 2 + V2_SNAP_WIN_CAP * 21;
pub const V2_REC_MAX: usize = 5 + V2_PAYLOAD_MAX + 4;

/// 撤销快照持久化记录（主册 F236 v2：前态矩形组——Ctrl+Z 恢复源
/// 可落盘，重启后仍可回到排列前几何）。
#[derive(Clone, Copy, Debug)]
pub struct V2UndoRec {
    /// 0 = 层叠 / 1 = 横铺 / 2 = 纵铺（与 ledger_col 同序）。
    pub cmd: u8,
    pub wins: [SnapWin; V2_SNAP_WIN_CAP],
    pub count: usize,
    /// 超出 V2_SNAP_WIN_CAP 被丢弃的窗数（如实记账，不静默）。
    pub dropped: usize,
}

impl V2UndoRec {
    pub fn capture(cmd: SnapCmd, wins: &[SnapWin]) -> V2UndoRec {
        let mut rec = V2UndoRec {
            cmd: match cmd {
                SnapCmd::Cascade => 0,
                SnapCmd::TileRows => 1,
                SnapCmd::TileCols => 2,
            },
            wins: [SnapWin { id: 0, x: 0, y: 0, w: 0, h: 0, minimized: false, floater: false }; V2_SNAP_WIN_CAP],
            count: 0,
            dropped: 0,
        };
        for w in wins {
            if rec.count < V2_SNAP_WIN_CAP {
                rec.wins[rec.count] = *w;
                rec.count += 1;
            } else {
                rec.dropped += 1;
            }
        }
        rec
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < 7 {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.cmd;
        out[6] = self.count as u8;
        let mut n = 7usize;
        for w in self.wins[..self.count].iter() {
            if out.len() < n + 21 {
                return None;
            }
            out[n..n + 4].copy_from_slice(&w.id.to_le_bytes());
            out[n + 4..n + 8].copy_from_slice(&w.x.to_le_bytes());
            out[n + 8..n + 12].copy_from_slice(&w.y.to_le_bytes());
            out[n + 12..n + 16].copy_from_slice(&w.w.to_le_bytes());
            out[n + 16..n + 20].copy_from_slice(&w.h.to_le_bytes());
            out[n + 20] = w.minimized as u8 | ((w.floater as u8) << 1);
            n += 21;
        }
        if out.len() < n + 4 {
            return None;
        }
        let sum = v2_fnv1a32(&out[..n]);
        out[n..n + 4].copy_from_slice(&sum.to_le_bytes());
        Some(n + 4)
    }

    pub fn from_bytes(buf: &[u8]) -> Result<V2UndoRec, V2SaveErr> {
        if buf.len() < 11 {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let body = buf.len() - 4;
        let sum = u32::from_le_bytes([buf[body], buf[body + 1], buf[body + 2], buf[body + 3]]);
        if v2_fnv1a32(&buf[..body]) != sum {
            return Err(V2SaveErr::BadChecksum);
        }
        if buf[5] > 2 || buf[6] as usize > V2_SNAP_WIN_CAP || body != 7 + buf[6] as usize * 21 {
            return Err(V2SaveErr::BadLen);
        }
        let mut rec = V2UndoRec {
            cmd: buf[5],
            wins: [SnapWin { id: 0, x: 0, y: 0, w: 0, h: 0, minimized: false, floater: false }; V2_SNAP_WIN_CAP],
            count: buf[6] as usize,
            dropped: 0,
        };
        for k in 0..rec.count {
            let n = 7 + k * 21;
            let w = i32::from_le_bytes([buf[n + 12], buf[n + 13], buf[n + 14], buf[n + 15]]);
            let h = i32::from_le_bytes([buf[n + 16], buf[n + 17], buf[n + 18], buf[n + 19]]);
            if w <= 0 || h <= 0 {
                return Err(V2SaveErr::BadLen);
            }
            rec.wins[k] = SnapWin {
                id: u32::from_le_bytes([buf[n], buf[n + 1], buf[n + 2], buf[n + 3]]),
                x: i32::from_le_bytes([buf[n + 4], buf[n + 5], buf[n + 6], buf[n + 7]]),
                y: i32::from_le_bytes([buf[n + 8], buf[n + 9], buf[n + 10], buf[n + 11]]),
                w,
                h,
                minimized: buf[n + 20] & 1 != 0,
                floater: buf[n + 20] & 2 != 0,
            };
        }
        Ok(rec)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// DPI 缩放四档（×100/125/150/200% 的整数千分比）。
pub const V2_DPI_SCALE: [u32; 4] = [100, 125, 150, 200];

/// 层叠偏移 DPI 换算（24px × 千分比 / 100——主册「斜向偏移 24px」
/// 在各 DPI 档下的整数落点）。
pub fn v2_cascade_offset_dpi(dpi_idx: usize) -> i32 {
    let s = V2_DPI_SCALE.get(dpi_idx).copied().unwrap_or(100);
    (CASCADE_OFFSET_PX * s as i32) / 100
}

/// 降级容量判定：工作区在 F214 最小尺寸约束下能放下几窗
/// （与 tile 的 fit_max 同式——span/min_span 向下取整，一处一事实）。
pub fn v2_tile_capacity(work: &Rect, cols: bool) -> usize {
    let (span, min_span) = if cols { (work.w, MIN_WIN_W) } else { (work.h, MIN_WIN_H) };
    (span / min_span).max(1) as usize
}

/// 降级触发判定（窗数×最小尺寸矩阵）：n 窗是否触发
/// 「（窗口过多，部分最小化）」标注。
pub fn v2_will_degrade(work: &Rect, n: usize, cols: bool) -> bool {
    n > v2_tile_capacity(work, cols)
}

// -- 判定面扩展 ------------------------------------------------------------

/// F236 v2 自检（首条必为持久化 round-trip）。
pub fn run_winsnap_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F236-winsnap-v2");
    let work = Rect::new(0, 0, 1600, 900);

    // 1. 持久化 round-trip（验主册 v2 锚「撤销快照持久化（前态矩形组）」）。
    let before = mk_wins(4, 0x2A2A);
    let rec = V2UndoRec::capture(SnapCmd::TileCols, &before);
    let mut buf = [0u8; V2_REC_MAX];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2UndoRec::from_bytes(&buf[..wrote]);
    let same = match &back {
        Ok(b) => {
            b.cmd == 2 && b.count == 4 && b.dropped == 0
                && b.wins[..4]
                    .iter()
                    .zip(before.iter())
                    .all(|(a, e)| a.id == e.id && a.x == e.x && a.y == e.y && a.w == e.w && a.h == e.h)
        }
        Err(_) => false,
    };
    set.add("v2 persist round-trip: undo snapshot", wrote > 0 && same, "");

    // 2. 四类损坏全拒绝。
    //    [缺陷账本] 现象：BadLen 分支红。根因：cmd 字节（载荷域，受
    //    校验和覆盖）翻位后未重算校验和——实现先验校验和后查 cmd 域，
    //    必先报 BadChecksum，cmd 域分支（BadLen）未真正测到，属检查
    //    项构造缺陷。修法：翻位后重算校验和，真测域分支。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 5;
    let mut b5 = buf;
    b5[5] = 3; // cmd 字节越界 → 记录式不认。
    let body5 = wrote - 4;
    let sum5 = v2_fnv1a32(&b5[..body5]);
    b5[body5..body5 + 4].copy_from_slice(&sum5.to_le_bytes());
    let mut b4 = buf;
    b4[6] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    set.add(
        "v2 persist rejects magic/version/len/checksum",
        wrote > 0
            && matches!(V2UndoRec::from_bytes(&b1[..wrote]), Err(V2SaveErr::BadMagic))
            && matches!(V2UndoRec::from_bytes(&b2[..wrote]), Err(V2SaveErr::BadVersion))
            && matches!(V2UndoRec::from_bytes(&b5[..wrote]), Err(V2SaveErr::BadLen))
            && matches!(V2UndoRec::from_bytes(&b4[..wrote]), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. 降级触发矩阵（验主册「最小尺寸降级」：容量式与 tile 实算一致
    //    ——1600 宽纵铺 8 列不降、400 宽纵铺 2 列后必降、横铺 7 行封顶）。
    let tiny = Rect::new(0, 0, 400, 900);
    set.add(
        "v2 degrade matrix matches tile solver",
        !v2_will_degrade(&work, 8, true)
            && v2_will_degrade(&tiny, 8, true)
            && v2_tile_capacity(&tiny, true) == 2
            && v2_tile_capacity(&work, false) == 7,
        "",
    );

    // 4. DPI 四档换算（验主册 v2 锚「DPI ×100/125/150/200%」：24px
    //    → 24/30/36/48，越界档回退 100%）。
    set.add(
        "v2 dpi cascade offsets 24/30/36/48",
        v2_cascade_offset_dpi(0) == 24
            && v2_cascade_offset_dpi(1) == 30
            && v2_cascade_offset_dpi(2) == 36
            && v2_cascade_offset_dpi(3) == 48
            && v2_cascade_offset_dpi(9) == 24,
        "",
    );

    // 5. 快照容量纪律（>16 窗如实 dropped 记账——不静默截断）。
    let many = mk_wins(20, 0x5B5B);
    let rec20 = V2UndoRec::capture(SnapCmd::Cascade, &many);
    set.add(
        "v2 snapshot cap 16 with honest drop count",
        rec20.count == V2_SNAP_WIN_CAP && rec20.dropped == 4,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_undo_rec_roundtrip_flags() {
        let mut wins = mk_wins(2, 0x77);
        wins[1].floater = true;
        let rec = V2UndoRec::capture(SnapCmd::TileRows, &wins);
        let mut buf = [0u8; V2_REC_MAX];
        let n = rec.to_bytes(&mut buf).unwrap();
        let back = V2UndoRec::from_bytes(&buf[..n]).unwrap();
        assert_eq!(back.cmd, 1);
        assert!(!back.wins[0].minimized && !back.wins[0].floater);
        assert!(back.wins[1].floater && !back.wins[1].minimized);
    }

    #[test]
    fn v2_capacity_matches_tile_fit() {
        let work = Rect::new(0, 0, 1600, 900);
        for n in 1..12usize {
            let r = tile(&mk_wins(n as u32, 0x99 + n as u32), &work, true);
            assert_eq!(r.degraded.is_empty(), !v2_will_degrade(&work, n, true), "n={n}");
        }
    }

    #[test]
    fn winsnap_v2_selfcheck_all_green() {
        let s = run_winsnap_v2_checks();
        assert!(s.all_passed(), "F236 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
