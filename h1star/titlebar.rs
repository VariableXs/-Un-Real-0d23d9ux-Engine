//! F213 双击标题栏最大化与拖离还原 · 判据实装（H 基础通用域 · AI-H1）。
//!
//! **判据锚**：主册 F213「双击标题栏最大化与拖离还原」。
//!
//! **验收标准（主册第一句）**：光标保持比例误差 <4px（10 次拖离采样）；
//! 双击判定窗口 500ms（双击间隔阈值）；顶缘/侧缘触发区 8px；动画与
//! 终态一致性录屏比对全绿。
//!
//! **设计要点**：
//! - 标题栏 32px 基线（卷首·乙-1）三条手势共用一个状态机：双击=
//!   最大化/还原（F080 吸附动画）、拖到屏幕顶=最大化（全屏预览虚框）、
//!   从最大化拖离标题栏=还原且**光标保持在标题栏原比例位置**；
//! - 光标比例保持是几何构造保证：还原窗按 `ratio = 光标在最大化标题
//!   栏的相对位置` 落位，还原瞬间 `|新比例 - 旧比例| × 新宽 < 4px`
//!   对任意窗口宽度恒成立（比例本身不变，误差只来自取整）；
//! - 双击判定窗口 500ms（与 F250 双击速度基线 500ms 默认档同源）；
//! - 顶缘/侧缘 8px 触发区与 F212 拖放预览同一条宽度（一处一事实）。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Rect/Clamp）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 标题栏高度——卷首·乙-1：「窗口标题栏高度 32px」。
pub const TITLEBAR_H_PX: i32 = 32;

/// 双击判定窗口——主册 F213：「双击判定窗口 500ms」。
pub const DOUBLE_CLICK_MS: u64 = 500;

/// 顶缘/侧缘触发区——主册 F213：「顶缘/侧缘触发区 8px」。
pub const EDGE_ZONE_PX: i32 = 8;

/// 光标保持比例误差上限——主册 F213：「光标保持比例误差 <4px」。
pub const CURSOR_RATIO_ERR_PX: i32 = 4;

/// 拖离采样数——主册 F213：「10 次拖离采样」。
pub const DRAG_OFF_SAMPLES: usize = 10;

// ---------------------------------------------------------------------------
// 标题栏手势状态机
// ---------------------------------------------------------------------------

/// 标题栏按压分类（复用 F201 多击语义的间隔阈值，独立实例不共享状态）。
#[derive(Clone, Copy)]
pub struct TitlebarClicks {
    last_press_ts: u64,
    streak: u8,
}

impl TitlebarClicks {
    pub fn new() -> TitlebarClicks {
        TitlebarClicks { last_press_ts: 0, streak: 0 }
    }

    /// 注入一次标题栏按压。500ms 内连续 → 双击（两击封顶：标题栏无三击语义）。
    pub fn press(&mut self, ts: u64) -> TitlePress {
        let in_win = ts >= self.last_press_ts && ts.saturating_sub(self.last_press_ts) <= DOUBLE_CLICK_MS;
        self.streak = if self.streak > 0 && in_win { 2 } else { 1 };
        self.last_press_ts = ts;
        match self.streak {
            2 => TitlePress::DoubleClick,
            _ => TitlePress::Single,
        }
    }
}

impl Default for TitlebarClicks {
    fn default() -> Self {
        Self::new()
    }
}

/// 标题栏按压分类结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitlePress {
    Single,
    DoubleClick,
}

/// 窗口几何状态。
#[derive(Clone, Copy, Debug)]
pub struct WindowGeo {
    pub rect: Rect,
    pub maximized: bool,
}

/// 拖动标题栏的落点动作（按拖动轨迹终点判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleDragOutcome {
    /// 拖到顶缘 8px → 最大化（全屏预览虚框出现）。
    MaximizePreview,
    /// 拖到左/右缘 8px → 半屏贴靠预览（F080 沿用）。
    SnapHalfPreview { left: bool },
    /// 从最大化状态拖离 → 还原为窗口（光标比例保持）。
    DragOffRestore,
    /// 普通移动。
    Move,
}

/// 拖动落点判定（同步函数——轨迹终点 + 屏幕几何 → 动作）。
///
/// `maximized` 为 true 时任何向下拖离都产生 [`TitleDragOutcome::DragOffRestore`]；
/// 否则按 8px 触发区判顶缘/侧缘。
pub fn classify_drag_end(cursor: (i32, i32), screen: &Rect, maximized: bool) -> TitleDragOutcome {
    let (x, y) = cursor;
    if maximized && y > screen.y + TITLEBAR_H_PX {
        return TitleDragOutcome::DragOffRestore;
    }
    if y < screen.y + EDGE_ZONE_PX {
        return TitleDragOutcome::MaximizePreview;
    }
    if x < screen.x + EDGE_ZONE_PX {
        return TitleDragOutcome::SnapHalfPreview { left: true };
    }
    if x > screen.right() - EDGE_ZONE_PX {
        return TitleDragOutcome::SnapHalfPreview { left: false };
    }
    TitleDragOutcome::Move
}

/// 从最大化拖离还原：按光标在最大化标题栏的水平比例落位还原窗。
///
/// - `max_rect`：最大化态窗口矩形（标题栏在其顶部 32px）；
/// - `cursor_x`：拖离瞬间光标 x；
/// - `restored_w/h`：还原目标尺寸（调用方按 F237 记忆或默认给）；
/// - 返回 `(还原窗矩形, 双击封顶后的比例误差 px)`。
///
/// 几何构造：`ratio = (cursor_x - max.x) / max.w`，新标题栏 x = cursor_x -
/// ratio × restored_w——光标相对标题栏的比例**逐字节不变**，误差只来自
/// 整数取整（≤1px，判据 4px 富余 4 倍）。
pub fn drag_off_restore(
    max_rect: &Rect,
    cursor_x: i32,
    restored_w: i32,
    restored_h: i32,
    screen: &Rect,
) -> (Rect, i32) {
    let ratio_nume = (cursor_x - max_rect.x).clamp(0, max_rect.w) as i64;
    let ratio_den = max_rect.w.max(1) as i64;
    // 新标题栏 x = cursor_x - ratio × restored_w（整数域，误差 ≤1px）。
    let new_x = cursor_x - ((ratio_nume * restored_w as i64) / ratio_den) as i32;
    let mut r = Rect::new(new_x, cursor_y_for_restore(max_rect, cursor_x), restored_w, restored_h);
    r = r.clamped_into(screen);
    // 误差 = |光标在新标题栏的实际比例 - 原比例| × 新宽（取整后 ≤1px）。
    let new_ratio_nume = (cursor_x - r.x).clamp(0, r.w) as i64;
    let err_px = ((new_ratio_nume * ratio_den - ratio_nume * r.w as i64).abs() * r.w.max(1) as i64
        / (ratio_den * r.w.max(1) as i64)) as i32;
    (r, err_px)
}

/// 还原窗的 y：光标保持在标题栏内 → 窗顶 = 光标 y - 按压点在标题栏内的
/// 偏移（此处以标题栏中线简化——按压点 y 由调用方带标题栏内偏移并入
/// cursor，主通路只构造几何）。
fn cursor_y_for_restore(max_rect: &Rect, _cursor_x: i32) -> i32 {
    max_rect.y
}

// ---------------------------------------------------------------------------
// 动画与终态一致性（录屏比对的判定面）
// ---------------------------------------------------------------------------

/// 开合动画帧与终态一致性：动画第 t 帧的插值矩形必须单调趋向终态，
/// 终点=终态（t=duration 时逐字节相等）——「动画只改变观感、不改变
/// 落点」的自动化面。
pub fn anim_reaches_final(start: &Rect, final_: &Rect, t_ms: u32, dur_ms: u32) -> (Rect, bool) {
    let t = (t_ms.min(dur_ms)) as i64;
    let d = dur_ms.max(1) as i64;
    let lerp = |a: i32, b: i32| (a as i64 + ((b - a) as i64 * t / d)) as i32;
    let frame = Rect::new(
        lerp(start.x, final_.x),
        lerp(start.y, final_.y),
        lerp(start.w, final_.w),
        lerp(start.h, final_.h),
    );
    let at_end = t_ms >= dur_ms
        && frame.x == final_.x
        && frame.y == final_.y
        && frame.w == final_.w
        && frame.h == final_.h;
    (frame, at_end)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F213 自检（判据面：比例误差 + 双击窗 + 触发区 + 动画终态一致）。
pub fn run_titlebar_checks() -> CheckSet {
    let mut set = CheckSet::new("F213-titlebar");

    // 1. 双击判定窗口 500ms：恰 500ms 同链、501ms 出窗。
    let mut c = TitlebarClicks::new();
    let first = c.press(1_000);
    let in_win = c.press(1_500);
    let mut c2 = TitlebarClicks::new();
    let _ = c2.press(0);
    let out_win = c2.press(501);
    set.add(
        "double click window 500ms",
        first == TitlePress::Single && in_win == TitlePress::DoubleClick && out_win == TitlePress::Single,
        "",
    );

    // 2. 光标保持比例 <4px：10 次拖离采样全过（比例构造保证，误差≤1px）。
    let screen = Rect::new(0, 0, 1920, 1048);
    let max_rect = Rect::new(0, 0, 1920, 1048);
    let mut worst = 0i32;
    for i in 0..DRAG_OFF_SAMPLES as i32 {
        let cursor_x = 40 + i * 200; // 40..1840 扫过全宽
        let (_, err) = drag_off_restore(&max_rect, cursor_x, 1000 + i * 37, 700, &screen);
        worst = worst.max(err);
    }
    set.add("10 drag-off samples ratio error <4px", worst < CURSOR_RATIO_ERR_PX, "");

    // 3. 顶缘触发区 8px：y<8 → 最大化预览。
    set.add(
        "top edge 8px maximize preview",
        classify_drag_end((960, 4), &screen, false) == TitleDragOutcome::MaximizePreview,
        "",
    );

    // 4. 侧缘触发区 8px：左/右 → 半屏贴靠预览（F080 沿用）。
    set.add(
        "side edges 8px snap previews",
        classify_drag_end((4, 500), &screen, false) == TitleDragOutcome::SnapHalfPreview { left: true }
            && classify_drag_end((1916, 500), &screen, false)
                == TitleDragOutcome::SnapHalfPreview { left: false },
        "",
    );

    // 5. 最大化态向下拖离 → 还原。
    set.add(
        "drag off from maximized restores",
        classify_drag_end((960, 200), &screen, true) == TitleDragOutcome::DragOffRestore,
        "",
    );

    // 6. 非最大化态拖到屏幕中部 → 普通移动（不误触发）。
    set.add(
        "mid screen drag is plain move",
        classify_drag_end((960, 500), &screen, false) == TitleDragOutcome::Move,
        "",
    );

    // 7. 动画与终态一致：终点逐字节=终态（200ms F080 吸附动画采样）。
    let start = Rect::new(100, 100, 800, 600);
    let final_ = Rect::new(0, 0, 1920, 1048);
    let (_, at_end) = anim_reaches_final(&start, &final_, 200, 200);
    let (mid, _) = anim_reaches_final(&start, &final_, 100, 200);
    // 缺陷账本：现象=「anim ends exactly at final geo」红；根因=中帧断言区间
    // 写反（`mid.x > start.x - 1` 要求中帧仍在起点侧，与主册判据「插值矩形
    // 单调趋向终态」矛盾——中帧应落在起点与终态之间）；修法=按判据改为
    // 中帧 ∈ [终态, 起点] 区间（此处 start 在 final 右侧），不改实现。
    set.add(
        "anim ends exactly at final geo",
        at_end && mid.x >= final_.x && mid.x <= start.x,
        "",
    );

    // 8. 标题栏 32px 基线常量（卷首·乙-1 一处一事实）。
    set.add("titlebar height 32px baseline", TITLEBAR_H_PX == 32, "");

    // 9. 还原窗被钳制进屏幕（拖离点靠右时窗口不丢在屏外）。
    let (r, _) = drag_off_restore(&max_rect, 1900, 1000, 700, &screen);
    set.add(
        "restored window clamped into screen",
        r.x >= screen.x && r.right() <= screen.right(),
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

    #[test]
    fn double_click_window_boundaries() {
        let mut c = TitlebarClicks::new();
        assert_eq!(c.press(0), TitlePress::Single);
        assert_eq!(c.press(500), TitlePress::DoubleClick); // 恰 500ms 同链
        let mut c2 = TitlebarClicks::new();
        assert_eq!(c2.press(0), TitlePress::Single);
        assert_eq!(c2.press(501), TitlePress::Single); // 501ms 出窗
        let mut c3 = TitlebarClicks::new();
        assert_eq!(c3.press(0), TitlePress::Single);
        assert_eq!(c3.press(100), TitlePress::DoubleClick);
        assert_eq!(c3.press(150), TitlePress::DoubleClick); // 两击封顶
    }

    #[test]
    fn ratio_preserved_on_restore() {
        let screen = Rect::new(0, 0, 1920, 1048);
        let max_rect = Rect::new(0, 0, 1920, 1048);
        // 光标在最大化标题栏 25% 处 → 还原后仍 25%。
        let (r, err) = drag_off_restore(&max_rect, 480, 1200, 800, &screen);
        let ratio_before = 480.0 / 1920.0;
        let ratio_after = (480 - r.x) as f64 / r.w as f64;
        assert!((ratio_before - ratio_after).abs() * 1200.0 < 4.0);
        assert!(err < 4);
        // 右端 75% 处同理。
        let (r2, err2) = drag_off_restore(&max_rect, 1440, 900, 700, &screen);
        let ratio_after2 = (1440 - r2.x) as f64 / r2.w as f64;
        assert!((0.75 - ratio_after2).abs() * 900.0 < 4.0);
        assert!(err2 < 4);
    }

    #[test]
    fn ten_samples_all_within_4px() {
        let screen = Rect::new(0, 0, 1920, 1048);
        let max_rect = Rect::new(0, 0, 1920, 1048);
        for i in 0..10u32 {
            let x = (i * 191 + 5) as i32;
            let (_, err) = drag_off_restore(&max_rect, x, 800 + (i as i32) * 53, 640, &screen);
            assert!(err < 4, "sample {} err {} >= 4px", i, err);
        }
    }

    #[test]
    fn edge_zones_and_move() {
        let screen = Rect::new(0, 0, 1920, 1048);
        assert_eq!(classify_drag_end((960, 7), &screen, false), TitleDragOutcome::MaximizePreview);
        assert_eq!(classify_drag_end((7, 500), &screen, false), TitleDragOutcome::SnapHalfPreview { left: true });
        assert_eq!(classify_drag_end((1913, 500), &screen, false), TitleDragOutcome::SnapHalfPreview { left: false });
        assert_eq!(classify_drag_end((960, 8), &screen, false), TitleDragOutcome::Move); // 恰 8px 出区
    }

    #[test]
    fn titlebar_selfcheck_all_green() {
        let set = run_titlebar_checks();
        assert!(set.all_passed(), "F213 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 7 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F213 验收定义的实装细化，非新立项）：持久化面 = 10 次
// 拖离采样（光标 x + 比例误差）的 VXH1 定长记录；壳接线面 = 标题栏 8px 触发
// 区命中测试 + 最大化动画帧清单生成；判定面 = run_titlebar_v2_checks。
// 零堆：编解码全走定长缓冲。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 记录定长：4 魔数 + 1 版本 + 30 载荷（10 × (u16 光标 x + u8 误差)）+ 4 校验。
pub const V2_RECORD_BYTES: usize = 39;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 拖离采样记录（持久化面）：判据「光标保持比例误差 <4px（10 次拖离
/// 采样）」的存档载体——采样点位与误差逐条入册。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragOffSampleRecord {
    pub cursor_x: [u16; DRAG_OFF_SAMPLES], // 扫过最大化标题栏全宽的采样 x
    pub err_px: [u8; DRAG_OFF_SAMPLES],    // 对应比例误差 px（取整误差 ≤1px）
}

impl DragOffSampleRecord {
    /// 采集：光标扫过全宽 10 点，逐点跑 drag_off_restore（同一判据源）。
    pub fn capture(screen: &Rect, max_rect: &Rect) -> DragOffSampleRecord {
        let mut rec = DragOffSampleRecord { cursor_x: [0; DRAG_OFF_SAMPLES], err_px: [0; DRAG_OFF_SAMPLES] };
        for i in 0..DRAG_OFF_SAMPLES {
            let x = 40 + i as i32 * 200;
            let (_, err) = drag_off_restore(max_rect, x, 1000 + i as i32 * 37, 700, screen);
            rec.cursor_x[i] = x as u16;
            rec.err_px[i] = err.min(u8::MAX as i32) as u8;
        }
        rec
    }

    /// 最坏比例误差（判据「<4px」的记录面读数）。
    pub fn worst_err(&self) -> u8 {
        let mut m = 0u8;
        for &e in self.err_px.iter() {
            if e > m {
                m = e;
            }
        }
        m
    }

    /// 编码：VXH1 + 版本 + 30 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        for i in 0..DRAG_OFF_SAMPLES {
            let x = self.cursor_x[i].to_le_bytes();
            out[5 + i * 3] = x[0];
            out[6 + i * 3] = x[1];
            out[7 + i * 3] = self.err_px[i];
        }
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<DragOffSampleRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[35], b[36], b[37], b[38]]);
        if v2_fnv1a(&b[..35]) != sum { return Err(V2PersistError::BadChecksum); }
        let mut rec = DragOffSampleRecord { cursor_x: [0; DRAG_OFF_SAMPLES], err_px: [0; DRAG_OFF_SAMPLES] };
        for i in 0..DRAG_OFF_SAMPLES {
            rec.cursor_x[i] = u16::from_le_bytes([b[5 + i * 3], b[6 + i * 3]]);
            rec.err_px[i] = b[7 + i * 3];
        }
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：标题栏命中分区 + 动画帧清单生成
// ---------------------------------------------------------------------------

/// 标题栏命中分区（点→语义动作的纯函数判定面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleZone { TopEdge, LeftEdge, RightEdge, Caption, Outside }

/// 命中测试：顶缘 8px → 最大化预览触发区；标题栏 32px 内左右 8px → 贴靠/
/// 缩放缘；其余标题栏 → 可拖拽区；窗外与标题栏以下 → 不命中。
pub fn hit_test_titlebar(x: i32, y: i32, win: &Rect) -> TitleZone {
    if !win.contains(x, y) {
        return TitleZone::Outside;
    }
    if y < win.y + EDGE_ZONE_PX {
        return TitleZone::TopEdge;
    }
    if y < win.y + TITLEBAR_H_PX {
        if x < win.x + EDGE_ZONE_PX {
            return TitleZone::LeftEdge;
        }
        if x >= win.right() - EDGE_ZONE_PX {
            return TitleZone::RightEdge;
        }
        return TitleZone::Caption;
    }
    TitleZone::Outside
}

/// 动画帧清单容量（200ms 吸附动画按 20ms 步进采样 11 帧，含终态）。
pub const ANIM_FRAME_CAP: usize = 11;

/// 最大化动画帧清单生成：线性采样 anim_reaches_final——末帧=终态
/// （「动画与终态一致性」的帧面，供合成器逐帧消费）。
pub fn anim_frame_list(start: &Rect, final_: &Rect, dur_ms: u32) -> ([Rect; ANIM_FRAME_CAP], usize) {
    let mut frames = [Rect::new(0, 0, 0, 0); ANIM_FRAME_CAP];
    let step = (dur_ms / (ANIM_FRAME_CAP as u32 - 1)).max(1);
    for (i, f) in frames.iter_mut().enumerate() {
        *f = anim_reaches_final(start, final_, step * i as u32, dur_ms).0;
    }
    (frames, ANIM_FRAME_CAP)
}

/// F213 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_titlebar_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F213-titlebar-v2");
    let screen = Rect::new(0, 0, 1920, 1048);
    let max_rect = Rect::new(0, 0, 1920, 1048);
    let rec = DragOffSampleRecord::capture(&screen, &max_rect);
    let blob = rec.to_bytes();
    // 1. round-trip：采样记录采集→编码→解码逐字段相等（v2 记录纪律）。
    set.add("v2 record round-trip 10 samples", DragOffSampleRecord::from_bytes(&blob) == Ok(rec), "");
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        DragOffSampleRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && DragOffSampleRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && DragOffSampleRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && DragOffSampleRecord::from_bytes(&blob[..38]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 验「光标保持比例误差 <4px（10 次拖离采样）」：存档最坏误差读数。
    set.add("10 archived samples worst err <4px", rec.worst_err() < CURSOR_RATIO_ERR_PX as u8, "");
    // 4. 验「顶缘/侧缘触发区 8px」命中面：五区边界逐一核对。
    let win = Rect::new(0, 0, 800, 600);
    set.add(
        "titlebar zone boundaries 8px",
        hit_test_titlebar(400, 7, &win) == TitleZone::TopEdge
            && hit_test_titlebar(4, 16, &win) == TitleZone::LeftEdge
            && hit_test_titlebar(796, 16, &win) == TitleZone::RightEdge
            && hit_test_titlebar(400, 16, &win) == TitleZone::Caption
            && hit_test_titlebar(400, 100, &win) == TitleZone::Outside
            && hit_test_titlebar(-1, 5, &win) == TitleZone::Outside,
        "",
    );
    // 5. 验「动画与终态一致性」帧面：首帧=起点、末帧=终态。
    let (frames, n) = anim_frame_list(&Rect::new(100, 100, 800, 600), &Rect::new(0, 0, 1920, 1048), 200);
    set.add(
        "anim frame list ends at final geo",
        n == ANIM_FRAME_CAP && frames[0] == Rect::new(100, 100, 800, 600) && frames[n - 1] == Rect::new(0, 0, 1920, 1048),
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn sample_record_round_trip_and_reject() {
        let screen = Rect::new(0, 0, 1920, 1048);
        let rec = DragOffSampleRecord::capture(&screen, &screen);
        let blob = rec.to_bytes();
        assert_eq!(DragOffSampleRecord::from_bytes(&blob), Ok(rec));
        assert!(DragOffSampleRecord::from_bytes(&vec![0u8; 20]).is_err());
    }

    #[test]
    fn zone_and_frame_list() {
        let win = Rect::new(0, 0, 800, 600);
        assert_eq!(hit_test_titlebar(792, 16, &win), TitleZone::RightEdge); // 恰 8px 内
        assert_eq!(hit_test_titlebar(400, 8, &win), TitleZone::Caption); // 恰 8px 出顶缘区
        let (frames, n) = anim_frame_list(&Rect::new(0, 0, 100, 100), &Rect::new(50, 50, 200, 200), 100);
        assert_eq!(n, ANIM_FRAME_CAP);
        assert_eq!(frames[n - 1], Rect::new(50, 50, 200, 200));
    }

    #[test]
    fn titlebar_v2_selfcheck_all_green() {
        let set = run_titlebar_v2_checks();
        assert!(set.all_passed(), "F213 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
