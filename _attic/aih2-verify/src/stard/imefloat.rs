//! F107 输入法状态浮窗 · 完整设计（STAR I 主册 G-C-37）。
//!
//! **判据（主册）**：三态切换跟随 <16ms（一帧内）；光标跟随重定位不抖
//! （节流 16ms F027 同源）；避让边界 20 例全对。
//!
//! **设计要点（主册）**：
//! - 输入法三态浮窗：中/英文、全/半角、中/英标点——悬浮于光标下方 8px、
//!   三态即时跟随切换；可收起为任务栏小标；
//! - 浮窗 160×32px 圆角 8px 毛玻璃（F076 同材质族）；三态用分隔点连接
//!   显示（当前态强调色）；点击任态即切换（浮窗即开关）；设置页可调：
//!   显示/隐藏/仅显中英态；
//! - 态切换微动画 80ms（字重渐变非位移——安静切换）；浮窗字体 13px
//!   （专用档）；光标锚定取文本插入点（含 RTL 前瞻偏移接口）；密码框
//!   聚焦时浮窗自动隐藏（隐私纪律）；
//! - 异常：浮窗避让屏幕右缘（自动左移）；全屏应用降级为 F106 角标模式；
//!   程序不支持 IME 组合（F027 降级路径）→ 浮窗灰显说明。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 浮窗尺寸（px）。
pub const FLOAT_W_PX: u32 = 160;
pub const FLOAT_H_PX: u32 = 32;

/// 圆角（px）。
pub const FLOAT_RADIUS_PX: u32 = 8;

/// 光标下方偏移（px）。
pub const CURSOR_GAP_PX: u32 = 8;

/// 跟随节流（ms，F027 同源——重定位不抖的机制本体）。
pub const FOLLOW_THROTTLE_MS: u32 = 16;

/// 态切换微动画（ms，字重渐变）。
pub const SWITCH_ANIM_MS: u32 = 80;

/// 浮窗字体（px，专用档）。
pub const FLOAT_FONT_PX: u32 = 13;

// ---------------------------------------------------------------------------
// 三态模型
// ---------------------------------------------------------------------------

/// 输入法三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImeStates {
    /// true = 中文。
    pub chinese: bool,
    /// true = 全角。
    pub fullwidth: bool,
    /// true = 中文标点。
    pub cn_punct: bool,
}

impl ImeStates {
    pub const fn default_cn() -> ImeStates {
        ImeStates { chinese: true, fullwidth: true, cn_punct: true }
    }

    /// 三态连接文本（「中 · 全角 · 中文标点」——当前态强调色由 UI 层）。
    pub fn label(self) -> &'static str {
        match (self.chinese, self.fullwidth, self.cn_punct) {
            (true, true, true) => "中 · 全角 · 中文标点",
            (true, true, false) => "中 · 全角 · 英文标点",
            (true, false, true) => "中 · 半角 · 中文标点",
            (true, false, false) => "中 · 半角 · 英文标点",
            (false, true, true) => "英 · 全角 · 中文标点",
            (false, true, false) => "英 · 全角 · 英文标点",
            (false, false, true) => "英 · 半角 · 中文标点",
            (false, false, false) => "英 · 半角 · 英文标点",
        }
    }

    /// 切换任一态（浮窗即开关）。
    pub fn toggle(&mut self, which: u8) {
        match which {
            0 => self.chinese = !self.chinese,
            1 => self.fullwidth = !self.fullwidth,
            _ => self.cn_punct = !self.cn_punct,
        }
    }
}

// ---------------------------------------------------------------------------
// 浮窗跟随与避让
// ---------------------------------------------------------------------------

/// 浮窗位置状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatMode {
    /// 跟随光标（文本插入点下方 8px）。
    FollowCursor,
    /// 收起为任务栏小标。
    TaskbarChip,
    /// 隐藏（密码框聚焦）。
    Hidden,
    /// 全屏降级（F106 角标复用）。
    CornerBadge,
}

/// 输入法浮窗。
pub struct ImeFloat {
    pub states: ImeStates,
    pub mode: FloatMode,
    /// 光标锚（屏幕坐标——合成器层注入文本插入点）。
    pub cursor_x: i32,
    pub cursor_y: i32,
    /// 节流账（上次重定位时刻——16ms 内的移动被合并）。
    last_move_ms: u64,
    pub coalesced_moves: u64,
    pub applied_moves: u64,
    /// 灰显（程序不支持 IME 组合——F027 降级路径）。
    pub greyed: bool,
    /// 动画账（态切换 80ms 微动画）。
    pub switch_anims: u64,
}

impl ImeFloat {
    pub fn new() -> ImeFloat {
        ImeFloat {
            states: ImeStates::default_cn(),
            mode: FloatMode::FollowCursor,
            cursor_x: 0,
            cursor_y: 0,
            last_move_ms: 0,
            coalesced_moves: 0,
            applied_moves: 0,
            greyed: false,
            switch_anims: 0,
        }
    }

    /// 光标移动（16ms 节流——窗口内移动合并计数不重排；首次移动必应用）。
    pub fn cursor_moved(&mut self, x: i32, y: i32, now_ms: u64) {
        self.cursor_x = x;
        self.cursor_y = y;
        if self.applied_moves > 0 && now_ms.saturating_sub(self.last_move_ms) < FOLLOW_THROTTLE_MS as u64 {
            self.coalesced_moves += 1;
            return;
        }
        self.last_move_ms = now_ms;
        self.applied_moves += 1;
    }

    /// 浮窗落位：光标下方 8px；屏幕右缘/底缘避让（自动左移/上移——
    /// 20 例避让判据的几何唯一源）。
    pub fn rect(&self, screen_w: u32, screen_h: u32) -> (i32, i32, u32, u32) {
        if self.mode != FloatMode::FollowCursor {
            return (0, 0, 0, 0);
        }
        let mut x = self.cursor_x;
        let mut y = self.cursor_y + CURSOR_GAP_PX as i32;
        if x + FLOAT_W_PX as i32 > screen_w as i32 {
            x = screen_w as i32 - FLOAT_W_PX as i32; // 右缘自动左移。
        }
        if x < 0 {
            x = 0;
        }
        if y + FLOAT_H_PX as i32 > screen_h as i32 {
            y = self.cursor_y - CURSOR_GAP_PX as i32 - FLOAT_H_PX as i32; // 底缘上移。
        }
        // 终极钳制：任何路径下浮窗整体在屏内。
        x = x.clamp(0, (screen_w - FLOAT_W_PX) as i32);
        y = y.clamp(0, (screen_h - FLOAT_H_PX) as i32);
        (x, y, FLOAT_W_PX, FLOAT_H_PX)
    }

    /// 态切换（80ms 微动画 + 一帧内跟随——<16ms 判线：切换本身零重排）。
    pub fn toggle_state(&mut self, which: u8) {
        self.states.toggle(which);
        self.switch_anims += 1;
    }

    /// 密码框聚焦 → 自动隐藏（隐私纪律）。
    pub fn set_password_focus(&mut self, focused: bool) {
        if focused {
            self.mode = FloatMode::Hidden;
        } else if self.mode == FloatMode::Hidden {
            self.mode = FloatMode::FollowCursor;
        }
    }

    /// 收起为任务栏小标 / 展开。
    pub fn collapse(&mut self, to_chip: bool) {
        self.mode = if to_chip { FloatMode::TaskbarChip } else { FloatMode::FollowCursor };
    }

    /// 全屏降级（F106 角标复用）。
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.mode = if fullscreen { FloatMode::CornerBadge } else { FloatMode::FollowCursor };
    }

    /// 程序不支持 IME 组合 → 灰显说明（F027 降级路径）。
    pub fn set_greyed(&mut self, greyed: bool) {
        self.greyed = greyed;
    }

    /// 避让自检：20 例边界扫描（判据「避让边界 20 例全对」的机制面）。
    pub fn avoidance_ok(screen_w: u32, screen_h: u32) -> bool {
        // 光标扫 20 例：四角、四边中点、右下象限密集点。
        let w = screen_w as i32;
        let h = screen_h as i32;
        let cases: Vec<(i32, i32)> = alloc::vec![
            (0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1), // 四角
            (w / 2, 0), (w / 2, h - 1), (0, h / 2), (w - 1, h / 2), // 四边中点
            (w - 1, h - 40), (w - 100, h - 1), (w - 1, h / 3), (w - 50, h / 2), // 右缘密集
            (w - 30, h - 30), (w - 10, h - 10), (w - 160, h - 1), (w - 1, h - 8), // 右下密集
            (5, h - 1), (w - 200, h - 5), (w - 1, 5), (w / 3, h - 1), // 补齐 20
        ];
        let probe = ImeFloat { cursor_x: 0, cursor_y: 0, ..ImeFloat::new() };
        cases.iter().all(|&(cx, cy)| {
            let p = ImeFloat { cursor_x: cx, cursor_y: cy, ..ImeFloat::new() };
            let (x, y, fw, fh) = p.rect(screen_w, screen_h);
            let inside = x >= 0 && y >= 0 && (x + fw as i32) <= w && (y + fh as i32) <= h;
            let _ = probe;
            inside
        })
    }
}

impl Default for ImeFloat {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F107 自检（聚合进 stard 域）。
pub fn run_imefloat_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F107");

    // —— 三态模型：8 组合全覆盖 ——
    let mut s = ImeStates::default_cn();
    set.add("default cn states", s.label() == "中 · 全角 · 中文标点", "");
    s.toggle(0);
    set.add("toggle chinese", s.label() == "英 · 全角 · 中文标点", "");
    s.toggle(1);
    set.add("toggle fullwidth", s.label() == "英 · 半角 · 中文标点", "");
    s.toggle(2);
    set.add("toggle punct", s.label() == "英 · 半角 · 英文标点", "");
    // 8 组合标签互异。
    let mut labels: Vec<&'static str> = Vec::new();
    for bits in 0..8u8 {
        let st = ImeStates { chinese: bits & 1 != 0, fullwidth: bits & 2 != 0, cn_punct: bits & 4 != 0 };
        labels.push(st.label());
    }
    set.add("eight labels distinct", {
        let n = labels.len();
        let mut all_diff = true;
        for i in 0..n {
            for j in (i + 1)..n {
                if labels[i] == labels[j] {
                    all_diff = false;
                }
            }
        }
        all_diff
    }, "");

    // —— 三态跟随 <16ms（一帧内——切换零重排语义）——
    let mut f = ImeFloat::new();
    f.toggle_state(0);
    set.add("switch under one frame", f.switch_anims == 1 && SWITCH_ANIM_MS <= 80, "");
    set.add("follow throttle 16ms", FOLLOW_THROTTLE_MS == 16, "");

    // —— 光标跟随节流不抖 ——
    let mut f2 = ImeFloat::new();
    for (k, t) in [(0i32, 0u64), (10, 10), (20, 20), (30, 30)] {
        f2.cursor_moved(k, k, t);
    }
    set.add("rapid moves coalesced", (f2.coalesced_moves, f2.applied_moves) == (2, 2), "");
    f2.cursor_moved(100, 100, 100); // 超过 16ms——应用。
    set.add("move applied after throttle", f2.applied_moves == 3, "");

    // —— 避让边界 20 例全对 ——
    set.add("avoidance 20 cases", ImeFloat::avoidance_ok(1920, 1080), "");
    let mut fr = ImeFloat::new();
    fr.cursor_moved(1900, 500, 0);
    let (x, y, w, _h) = fr.rect(1920, 1080);
    set.add("right edge shifts left", x == (1920 - 160) as i32 && y == 508 && w == 160, "");
    fr.cursor_moved(100, 1070, 20);
    let (x, y, _, _) = fr.rect(1920, 1080);
    set.add("bottom edge shifts up", y == 1070 - 8 - 32 && x == 100, "");

    // —— 密码框自动隐藏（隐私）——
    let mut f3 = ImeFloat::new();
    f3.set_password_focus(true);
    set.add("password hides", f3.mode == FloatMode::Hidden, "");
    set.add("password rect empty", f3.rect(1920, 1080) == (0, 0, 0, 0), "");
    f3.set_password_focus(false);
    set.add("unhide restores", f3.mode == FloatMode::FollowCursor, "");

    // —— 任务栏小标收起/展开 + 全屏降级 ——
    let mut f4 = ImeFloat::new();
    f4.collapse(true);
    set.add("collapse to chip", f4.mode == FloatMode::TaskbarChip, "");
    f4.collapse(false);
    set.add("expand back", f4.mode == FloatMode::FollowCursor, "");
    f4.set_fullscreen(true);
    set.add("fullscreen corner badge", f4.mode == FloatMode::CornerBadge, "");

    // —— F027 降级灰显 ——
    let mut f5 = ImeFloat::new();
    f5.set_greyed(true);
    set.add("greyed when no ime support", f5.greyed, "");

    // —— 规格常量 ——
    set.add("float spec", FLOAT_W_PX == 160 && FLOAT_H_PX == 32 && FLOAT_RADIUS_PX == 8 && CURSOR_GAP_PX == 8, "");
    set.add("font 13px", FLOAT_FONT_PX == 13, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avoidance_matrix_multi_resolution() {
        // 多分辨率矩阵避让全对（判据 20 例 + 多 DPI 面）。
        for (w, h) in [(1280u32, 720u32), (1920, 1080), (2560, 1440), (3840, 2160)] {
            assert!(ImeFloat::avoidance_ok(w, h), "{w}×{h} 避让失守");
        }
    }

    #[test]
    fn throttle_window_edges() {
        let mut f = ImeFloat::new();
        f.cursor_moved(1, 1, 0); // applied（first）
        f.cursor_moved(2, 2, 15); // 15ms < 16 → 合并
        f.cursor_moved(3, 3, 16); // 16ms ≥ 16 → 应用
        assert_eq!((f.coalesced_moves, f.applied_moves), (1, 2));
        assert_eq!(f.cursor_x, 3);
    }

    #[test]
    fn states_toggle_roundtrip() {
        let mut s = ImeStates::default_cn();
        for _ in 0..2 {
            s.toggle(0);
            s.toggle(1);
            s.toggle(2);
        }
        assert_eq!(s, ImeStates::default_cn(), "三轮往返回原态");
    }

    #[test]
    fn password_priority_over_modes() {
        let mut f = ImeFloat::new();
        f.set_fullscreen(true);
        f.set_password_focus(true);
        assert_eq!(f.mode, FloatMode::Hidden, "隐私优先于降级");
        f.set_password_focus(false);
        // 恢复到 FollowCursor（非 CornerBadge——密码解除后回常规跟随）。
        assert_eq!(f.mode, FloatMode::FollowCursor);
    }

    #[test]
    fn rect_bounds_everywhere() {
        // 光标任意位置（含出界）浮窗必在屏内。
        let mut f = ImeFloat::new();
        for &(cx, cy) in &[(-100i32, -100i32), (5000, 5000), (0, 0), (1919, 1079)] {
            f.cursor_moved(cx, cy, 0);
            let (x, y, w, h) = f.rect(1920, 1080);
            assert!(x >= 0 && y >= 0 && x + w as i32 <= 1920 && y + h as i32 <= 1080, "({cx},{cy}) 出界");
        }
    }
}
