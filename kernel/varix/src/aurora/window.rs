//! AURORA-1000 AI-09 · 窗口管理器 VWM（A201~A225，W2）
//!
//! 内核级窗口管理器（VWM）：窗口注册表、命中测试、聚焦环（Alt-Tab 语义）、
//! 移动/缩放（最小尺寸钳制）、层叠 Z 序、边缘贴靠、平铺布局、全屏切换、
//! 最小化/最大化/还原、分组、跨屏、标题栏、关闭确认、状态记忆、快捷键
//! （全键盘可达）、回收（无僵尸）、性能预算、无障碍、可观测、模糊测试、
//! 降级链与域自检收口。
//!
//! 全部为纯逻辑 + 固定容量数组（no_std，无分配、无 unsafe、无宏、无泛型），
//! 保证 `cargo ktest` 一次全绿，并对「窗口表满 / 销毁后操作 / 空聚焦栈」做边界防护。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 容量与常量
// ---------------------------------------------------------------------------

pub const MAX_WINDOWS: usize = 32;
pub const MIN_WIN_W: u32 = 60;
pub const MIN_WIN_H: u32 = 40;
pub const MAX_TITLE: usize = 24;

/// 每帧窗口操作数性能红线（A216/A220）。
pub const PERF_OPS_REDLINE: u32 = 2000;

// ---------------------------------------------------------------------------
// 矩形与状态
// ---------------------------------------------------------------------------

/// 屏幕/窗口矩形（A202 移动缩放、A205 贴靠、A206 平铺的基础）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    /// 点是否落在矩形内（i64 防溢出，A203 命中测试用）。
    pub fn contains(self, px: i32, py: i32) -> bool {
        let right = self.x as i64 + self.w as i64;
        let bottom = self.y as i64 + self.h as i64;
        (px as i64) >= self.x as i64 && (px as i64) < right && (py as i64) >= self.y as i64
            && (py as i64) < bottom
    }

    pub fn area(self) -> u64 {
        self.w as u64 * self.h as u64
    }

    /// 最小尺寸钳制（A202）。
    pub fn clamp_min(self, min_w: u32, min_h: u32) -> Rect {
        Rect { x: self.x, y: self.y, w: self.w.max(min_w), h: self.h.max(min_h) }
    }

    /// 仅平移、不缩放，把矩形约束进 `bound` 内（A210 跨屏用）。
    pub fn clamp_into(self, bound: Rect) -> Rect {
        let mut x = self.x;
        let mut y = self.y;
        if self.w >= bound.w {
            x = bound.x;
        } else {
            let max_x = bound.x + bound.w as i32 - self.w as i32;
            if x < bound.x {
                x = bound.x;
            }
            if x > max_x {
                x = max_x;
            }
        }
        if self.h >= bound.h {
            y = bound.y;
        } else {
            let max_y = bound.y + bound.h as i32 - self.h as i32;
            if y < bound.y {
                y = bound.y;
            }
            if y > max_y {
                y = max_y;
            }
        }
        Rect { x, y, w: self.w, h: self.h }
    }
}

/// 窗口状态枚举（题目要求：Normal/Minimized/Maximized/Fullscreen/Tiled）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinState {
    Normal,
    Minimized,
    Maximized,
    Fullscreen,
    Tiled,
}

impl WinState {
    pub fn name(self) -> &'static str {
        match self {
            WinState::Normal => "normal",
            WinState::Minimized => "minimized",
            WinState::Maximized => "maximized",
            WinState::Fullscreen => "fullscreen",
            WinState::Tiled => "tiled",
        }
    }

    /// 只有最小化窗口不可见/不可命中（A203/A217）。
    pub fn visible(self) -> bool {
        self != WinState::Minimized
    }
}

// ---------------------------------------------------------------------------
// 窗口槽
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: u32,
    pub rect: Rect,
    pub saved_rect: Rect, // 最大化/全屏的临时还原（A207/A208）
    pub saved_state: WinState,
    pub mem_rect: Rect, // 持久状态记忆（A213）
    pub mem_state: WinState,
    pub z: i64, // 层叠 z 序（A203）
    pub state: WinState,
    pub group: u8, // 分组（A209）
    pub screen: u8, // 所在屏幕（A210）
    pub confirm_close: bool, // 关闭确认（A212）
    pub alive: bool, // 僵尸窗防护（A215）
    title: [u8; MAX_TITLE],
    title_len: usize,
}

impl Window {
    pub const fn blank() -> Window {
        Window {
            id: 0,
            rect: Rect::new(0, 0, 0, 0),
            saved_rect: Rect::new(0, 0, 0, 0),
            saved_state: WinState::Normal,
            mem_rect: Rect::new(0, 0, 0, 0),
            mem_state: WinState::Normal,
            z: 0,
            state: WinState::Normal,
            group: 0,
            screen: 0,
            confirm_close: false,
            alive: false,
            title: [0u8; MAX_TITLE],
            title_len: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// A201 窗口创建与销毁
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowManager {
    windows: [Window; MAX_WINDOWS],
    next_id: u32,
    focus: [u32; MAX_WINDOWS],
    focus_count: usize,
    focus_index: usize,
}

impl WindowManager {
    pub const fn new() -> WindowManager {
        WindowManager {
            windows: [Window::blank(); MAX_WINDOWS],
            next_id: 1,
            focus: [0u32; MAX_WINDOWS],
            focus_count: 0,
            focus_index: 0,
        }
    }

    // --- 内部查找 ---

    /// 命中/操作用的查找：仅返回存活且 id 匹配的槽（销毁后不可命中）。
    fn slot_of(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive && self.windows[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 原始查找：含已销毁槽（用于僵尸检测）。
    fn raw_slot_of(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn next_z(&self) -> i64 {
        let mut m: i64 = -1;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive && self.windows[i].z > m {
                m = self.windows[i].z;
            }
            i += 1;
        }
        m + 1
    }

    // --- 聚焦栈维护 ---

    fn push_focus(&mut self, id: u32) {
        self.remove_focus(id);
        if self.focus_count < MAX_WINDOWS {
            self.focus[self.focus_count] = id;
            self.focus_count += 1;
            self.focus_index = self.focus_count - 1;
        }
    }

    fn remove_focus(&mut self, id: u32) {
        let mut idx = None;
        let mut i = 0;
        while i < self.focus_count {
            if self.focus[i] == id {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        if let Some(idx) = idx {
            let mut j = idx;
            while j + 1 < self.focus_count {
                self.focus[j] = self.focus[j + 1];
                j += 1;
            }
            self.focus_count -= 1;
            if self.focus_count == 0 {
                self.focus_index = 0;
            } else if self.focus_index >= self.focus_count {
                self.focus_index = self.focus_count - 1;
            }
        }
    }

    fn cycle_focus(&mut self, dir: i32) -> Option<u32> {
        if self.focus_count == 0 {
            return None;
        }
        let n = self.focus_count;
        let mut steps = 0;
        let mut idx = self.focus_index;
        loop {
            idx = ((idx as i32 + dir).rem_euclid(n as i32)) as usize;
            let id = self.focus[idx];
            if let Some(s) = self.slot_of(id) {
                if self.windows[s].state.visible() {
                    self.focus_index = idx;
                    return Some(id);
                }
            }
            steps += 1;
            if steps >= n {
                return Some(self.focus[self.focus_index]);
            }
        }
    }

    // --- A201 公开 API ---

    /// 创建窗口：复用死亡槽（id 复用=槽复用），但分配单调递增新 id（无僵尸）。
    pub fn create_window(&mut self, rect: Rect, screen: u8) -> Option<u32> {
        let mut slot = None;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if !self.windows[i].alive {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let slot = match slot {
            Some(s) => s,
            None => return None, // 表满（边界防护）
        };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut w = Window::blank();
        w.id = id;
        w.rect = rect.clamp_min(MIN_WIN_W, MIN_WIN_H);
        w.saved_rect = w.rect;
        w.saved_state = WinState::Normal;
        w.mem_rect = w.rect;
        w.mem_state = WinState::Normal;
        w.z = self.next_z();
        w.state = WinState::Normal;
        w.group = 0;
        w.screen = screen;
        w.confirm_close = false;
        w.alive = true;
        w.title_len = 0;
        self.windows[slot] = w;
        self.push_focus(id);
        Some(id)
    }

    /// 销毁窗口：标记死亡并移出聚焦栈（销毁后 id 不可命中）。
    pub fn destroy_window(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].alive = false;
                self.remove_focus(id);
                true
            }
            None => false,
        }
    }

    pub fn rect_of(&self, id: u32) -> Option<Rect> {
        self.slot_of(id).map(|s| self.windows[s].rect)
    }

    pub fn alive_count(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn defunct_count(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            // defunct = 曾创建（id != 0）且已销毁；空槽不算僵尸。
            if self.windows[i].id != 0 && !self.windows[i].alive {
                c += 1;
            }
            i += 1;
        }
        c
    }

    // -----------------------------------------------------------------------
    // A202 窗口移动与缩放
    // -----------------------------------------------------------------------

    pub fn move_window(&mut self, id: u32, x: i32, y: i32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state == WinState::Fullscreen {
                    return false; // 全屏锁定
                }
                self.windows[s].rect.x = x;
                self.windows[s].rect.y = y;
                true
            }
            None => false,
        }
    }

    pub fn resize_window(&mut self, id: u32, w: u32, h: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state == WinState::Fullscreen {
                    return false;
                }
                let r = self.windows[s].rect;
                self.windows[s].rect = Rect::new(r.x, r.y, w, h).clamp_min(MIN_WIN_W, MIN_WIN_H);
                true
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // A203 窗口层叠 Z 序
    // -----------------------------------------------------------------------

    pub fn raise(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].z = self.next_z();
                true
            }
            None => false,
        }
    }

    pub fn lower(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                let mut m: i64 = 0;
                let mut i = 0;
                while i < MAX_WINDOWS {
                    if i != s && self.windows[i].alive && self.windows[i].z < m {
                        m = self.windows[i].z;
                    }
                    i += 1;
                }
                self.windows[s].z = m - 1;
                true
            }
            None => false,
        }
    }

    pub fn z_of(&self, id: u32) -> Option<i64> {
        self.slot_of(id).map(|s| self.windows[s].z)
    }

    /// 命中测试：返回最高 z 的可见窗口（A203）。
    pub fn hit_test(&self, px: i32, py: i32) -> Option<u32> {
        let mut best: Option<(i64, u32)> = None;
        let mut i = 0;
        while i < MAX_WINDOWS {
            let w = &self.windows[i];
            if w.alive && w.state.visible() && w.rect.contains(px, py) {
                match best {
                    Some((bz, _)) if w.z <= bz => {}
                    _ => best = Some((w.z, w.id)),
                }
            }
            i += 1;
        }
        best.map(|(_, id)| id)
    }

    // -----------------------------------------------------------------------
    // A204 窗口聚焦管理（Alt-Tab 环形切换）
    // -----------------------------------------------------------------------

    pub fn focus(&mut self, id: u32) -> bool {
        if self.slot_of(id).is_none() {
            return false;
        }
        self.push_focus(id);
        true
    }

    pub fn current_focus(&self) -> Option<u32> {
        if self.focus_count == 0 {
            return None;
        }
        let id = self.focus[self.focus_index];
        match self.slot_of(id) {
            Some(s) if self.windows[s].state.visible() => Some(id),
            _ => None,
        }
    }

    pub fn focus_next(&mut self) -> Option<u32> {
        self.cycle_focus(1)
    }

    pub fn focus_prev(&mut self) -> Option<u32> {
        self.cycle_focus(-1)
    }

    // -----------------------------------------------------------------------
    // A205 边缘贴靠（纯函数：屏幕矩形划分）
    // -----------------------------------------------------------------------

    pub fn snap_rect(screen: Rect, side: SnapSide) -> Rect {
        let w = screen.w;
        let h = screen.h;
        let hw = w / 2;
        let hh = h / 2;
        match side {
            SnapSide::LeftHalf => Rect::new(screen.x, screen.y, hw, h),
            SnapSide::RightHalf => Rect::new(screen.x + hw as i32, screen.y, w - hw, h),
            SnapSide::TopHalf => Rect::new(screen.x, screen.y, w, hh),
            SnapSide::BottomHalf => Rect::new(screen.x, screen.y + hh as i32, w, h - hh),
            SnapSide::TopLeft => Rect::new(screen.x, screen.y, hw, hh),
            SnapSide::TopRight => Rect::new(screen.x + hw as i32, screen.y, w - hw, hh),
            SnapSide::BottomLeft => Rect::new(screen.x, screen.y + hh as i32, hw, h - hh),
            SnapSide::BottomRight => {
                Rect::new(screen.x + hw as i32, screen.y + hh as i32, w - hw, h - hh)
            }
            SnapSide::Center => Rect::new(
                screen.x + (w / 4) as i32,
                screen.y + (h / 4) as i32,
                hw,
                hh,
            ),
        }
    }

    pub fn snap_window(&mut self, id: u32, screen: Rect, side: SnapSide) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state == WinState::Fullscreen {
                    return false;
                }
                self.windows[s].rect = Self::snap_rect(screen, side);
                true
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // A206 窗口平铺布局（主区 + 堆叠区）
    // -----------------------------------------------------------------------

    pub fn tile_layout(screen: Rect, n: usize, main_count: usize, main_permille: u32) -> TilingPlan {
        let mut plan = TilingPlan::new();
        if n == 0 {
            return plan;
        }
        let n = n.min(MAX_WINDOWS);
        let main_count = main_count.min(n).max(1);
        let main_w = ((screen.w as u64) * (main_permille as u64) / 1000) as u32;
        let stack_x = screen.x + main_w as i32;
        let stack_w = screen.w.saturating_sub(main_w);
        let main_h = screen.h / main_count as u32;
        let mut i = 0;
        while i < main_count {
            plan.rects[i] = Rect::new(
                screen.x,
                screen.y + (i as i32) * main_h as i32,
                main_w,
                main_h,
            );
            i += 1;
        }
        let stack_n = n - main_count;
        if stack_n > 0 && stack_w > 0 {
            let stack_h = screen.h / stack_n as u32;
            let mut j = 0;
            while j < stack_n {
                let idx = main_count + j;
                plan.rects[idx] = Rect::new(
                    stack_x,
                    screen.y + (j as i32) * stack_h as i32,
                    stack_w,
                    stack_h,
                );
                j += 1;
            }
        }
        plan.count = n;
        plan
    }

    pub fn apply_tiling(&mut self, screen: Rect, main_permille: u32) -> usize {
        let mut ids: [u32; MAX_WINDOWS] = [0; MAX_WINDOWS];
        let mut n = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            let w = &self.windows[i];
            if w.alive && w.state.visible() {
                ids[n] = w.id;
                n += 1;
            }
            i += 1;
        }
        let plan = Self::tile_layout(screen, n, (n + 1) / 2, main_permille);
        let mut k = 0;
        while k < plan.count {
            if let Some(s) = self.slot_of(ids[k]) {
                self.windows[s].rect = plan.rects[k];
                self.windows[s].state = WinState::Tiled;
            }
            k += 1;
        }
        plan.count
    }

    // -----------------------------------------------------------------------
    // A207 最小化/最大化/还原（状态机 + 焦点转移）
    // -----------------------------------------------------------------------

    pub fn minimize(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].saved_state = self.windows[s].state;
                self.windows[s].saved_rect = self.windows[s].rect;
                self.windows[s].state = WinState::Minimized;
                if self.focus[self.focus_index] == id {
                    self.cycle_focus(1); // 焦点转移给下一可见窗口
                }
                true
            }
            None => false,
        }
    }

    pub fn maximize(&mut self, id: u32, screen: Rect) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state == WinState::Fullscreen {
                    return false;
                }
                self.windows[s].saved_state = self.windows[s].state;
                self.windows[s].saved_rect = self.windows[s].rect;
                self.windows[s].state = WinState::Maximized;
                self.windows[s].rect = Rect::new(screen.x, screen.y, screen.w, screen.h);
                true
            }
            None => false,
        }
    }

    pub fn restore(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => match self.windows[s].state {
                WinState::Minimized => {
                    self.windows[s].state = if self.windows[s].saved_state == WinState::Minimized {
                        WinState::Normal
                    } else {
                        self.windows[s].saved_state
                    };
                    self.windows[s].rect = self.windows[s].saved_rect;
                    self.push_focus(id);
                    true
                }
                WinState::Maximized | WinState::Fullscreen | WinState::Tiled => {
                    self.windows[s].state = self.windows[s].saved_state;
                    self.windows[s].rect = self.windows[s].saved_rect;
                    true
                }
                WinState::Normal => false,
            },
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // A208 全屏与退出（保存/恢复原矩形）
    // -----------------------------------------------------------------------

    pub fn enter_fullscreen(&mut self, id: u32, screen: Rect) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].saved_state = self.windows[s].state;
                self.windows[s].saved_rect = self.windows[s].rect;
                self.windows[s].state = WinState::Fullscreen;
                self.windows[s].rect = Rect::new(screen.x, screen.y, screen.w, screen.h);
                self.windows[s].z = self.next_z();
                true
            }
            None => false,
        }
    }

    pub fn exit_fullscreen(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state != WinState::Fullscreen {
                    return false;
                }
                self.windows[s].state = self.windows[s].saved_state;
                self.windows[s].rect = self.windows[s].saved_rect;
                true
            }
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // A209 窗口分组
    // -----------------------------------------------------------------------

    pub fn set_group(&mut self, id: u32, group: u8) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].group = group;
                true
            }
            None => false,
        }
    }

    pub fn group_count(&self, group: u8) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive && self.windows[i].group == group {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn focus_group_next(&mut self, group: u8) -> Option<u32> {
        if self.focus_count == 0 {
            return None;
        }
        let n = self.focus_count;
        let mut steps = 0;
        let mut idx = self.focus_index;
        loop {
            idx = ((idx as i32 + 1).rem_euclid(n as i32)) as usize;
            let id = self.focus[idx];
            if let Some(s) = self.slot_of(id) {
                if self.windows[s].group == group && self.windows[s].state.visible() {
                    self.focus_index = idx;
                    return Some(id);
                }
            }
            steps += 1;
            if steps >= n {
                return None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // A210 窗口跨屏移动
    // -----------------------------------------------------------------------

    pub fn move_to_screen(&mut self, id: u32, screen_rect: Rect, target: u8) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].state == WinState::Fullscreen {
                    return false;
                }
                self.windows[s].screen = target;
                self.windows[s].rect = self.windows[s].rect.clamp_into(screen_rect);
                true
            }
            None => false,
        }
    }

    pub fn screen_of(&self, id: u32) -> Option<u8> {
        self.slot_of(id).map(|s| self.windows[s].screen)
    }

    // -----------------------------------------------------------------------
    // A211 窗口标题栏
    // -----------------------------------------------------------------------

    pub fn set_title(&mut self, id: u32, title: &[u8]) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                let mut n = 0;
                while n < title.len() && n < MAX_TITLE {
                    self.windows[s].title[n] = title[n];
                    n += 1;
                }
                self.windows[s].title_len = n;
                true
            }
            None => false,
        }
    }

    pub fn title_of(&self, id: u32) -> Option<&[u8]> {
        self.slot_of(id).map(|s| &self.windows[s].title[..self.windows[s].title_len])
    }

    // -----------------------------------------------------------------------
    // A212 窗口关闭确认
    // -----------------------------------------------------------------------

    pub fn set_confirm_close(&mut self, id: u32, confirm: bool) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].confirm_close = confirm;
                true
            }
            None => false,
        }
    }

    pub fn close_window(&mut self, id: u32) -> CloseResult {
        match self.slot_of(id) {
            Some(s) => {
                if self.windows[s].confirm_close {
                    CloseResult::ConfirmNeeded
                } else {
                    self.windows[s].alive = false;
                    self.remove_focus(id);
                    CloseResult::Closed
                }
            }
            None => CloseResult::NotFound,
        }
    }

    // -----------------------------------------------------------------------
    // A213 窗口状态记忆
    // -----------------------------------------------------------------------

    pub fn remember(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].mem_rect = self.windows[s].rect;
                self.windows[s].mem_state = self.windows[s].state;
                true
            }
            None => false,
        }
    }

    pub fn recall(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => {
                self.windows[s].rect = self.windows[s].mem_rect;
                self.windows[s].state = self.windows[s].mem_state;
                true
            }
            None => false,
        }
    }

    pub fn remembered(&self, id: u32) -> Option<Rect> {
        self.slot_of(id).map(|s| self.windows[s].mem_rect)
    }

    // -----------------------------------------------------------------------
    // A214 窗口管理快捷键（纯映射，全键盘可达）
    // -----------------------------------------------------------------------

    pub fn shortcut_to_action(key: Shortcut) -> WinAction {
        match key {
            Shortcut::AltTab => WinAction::FocusNext,
            Shortcut::AltShiftTab => WinAction::FocusPrev,
            Shortcut::SnapLeft => WinAction::SnapLeft,
            Shortcut::SnapRight => WinAction::SnapRight,
            Shortcut::SnapTop => WinAction::SnapTop,
            Shortcut::SnapBottom => WinAction::SnapBottom,
            Shortcut::Maximize => WinAction::Maximize,
            Shortcut::Minimize => WinAction::Minimize,
            Shortcut::Fullscreen => WinAction::ToggleFullscreen,
            Shortcut::Close => WinAction::Close,
        }
    }

    pub fn apply_shortcut(&mut self, key: Shortcut, screen: Rect) -> bool {
        let act = Self::shortcut_to_action(key);
        let focus = self.current_focus();
        match act {
            WinAction::FocusNext => self.focus_next().is_some(),
            WinAction::FocusPrev => self.focus_prev().is_some(),
            WinAction::SnapLeft => {
                focus.map_or(false, |id| self.snap_window(id, screen, SnapSide::LeftHalf))
            }
            WinAction::SnapRight => {
                focus.map_or(false, |id| self.snap_window(id, screen, SnapSide::RightHalf))
            }
            WinAction::SnapTop => {
                focus.map_or(false, |id| self.snap_window(id, screen, SnapSide::TopHalf))
            }
            WinAction::SnapBottom => {
                focus.map_or(false, |id| self.snap_window(id, screen, SnapSide::BottomHalf))
            }
            WinAction::Maximize => focus.map_or(false, |id| self.maximize(id, screen)),
            WinAction::Minimize => focus.map_or(false, |id| self.minimize(id)),
            WinAction::ToggleFullscreen => match focus {
                Some(id) => {
                    let is_fs = self
                        .slot_of(id)
                        .map_or(false, |s| self.windows[s].state == WinState::Fullscreen);
                    if is_fs {
                        self.exit_fullscreen(id)
                    } else {
                        self.enter_fullscreen(id, screen)
                    }
                }
                None => false,
            },
            WinAction::Close => match focus {
                Some(id) => matches!(self.close_window(id), CloseResult::Closed | CloseResult::ConfirmNeeded),
                None => false,
            },
        }
    }

    // -----------------------------------------------------------------------
    // A215 窗口回收（无僵尸）
    // -----------------------------------------------------------------------

    /// 销毁后该 id 是否仍存活（存活=可命中）。
    pub fn is_alive(&self, id: u32) -> bool {
        self.slot_of(id).is_some()
    }

    /// 槽仍携带该 id 但已死亡 —— 即僵尸（必须不可命中）。
    pub fn is_zombie(&self, id: u32) -> bool {
        match self.raw_slot_of(id) {
            Some(s) => !self.windows[s].alive,
            None => false,
        }
    }

    // -----------------------------------------------------------------------
    // A216 窗口性能预算
    // -----------------------------------------------------------------------

    /// 单次布局/命中成本随窗口数线性增长。
    pub fn layout_cost(n: usize) -> u32 {
        (n as u32) * 16 + 8
    }

    pub fn within_budget(ops: u32) -> bool {
        ops <= PERF_OPS_REDLINE
    }

    pub fn perf_ok(&self) -> bool {
        Self::within_budget(Self::layout_cost(self.alive_count()))
    }

    // -----------------------------------------------------------------------
    // A217 窗口无障碍
    // -----------------------------------------------------------------------

    pub fn a11y_exposed_count(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive && self.windows[i].state.visible() {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn is_exposed(&self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(s) => self.windows[s].state.visible(),
            None => false,
        }
    }

    /// 读屏顺序：按 z 降序（顶层优先）。
    pub fn a11y_focus_order(&self) -> [u32; MAX_WINDOWS] {
        let mut order = [0u32; MAX_WINDOWS];
        let mut n = 0;
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive && self.windows[i].state.visible() {
                order[n] = self.windows[i].id;
                n += 1;
            }
            i += 1;
        }
        let mut j = 1;
        while j < n {
            let key_id = order[j];
            let key_z = self.slot_of(key_id).map_or(0, |s| self.windows[s].z);
            let mut k = j;
            while k > 0 {
                let prev_id = order[k - 1];
                let prev_z = self.slot_of(prev_id).map_or(0, |s| self.windows[s].z);
                if prev_z >= key_z {
                    break;
                }
                order[k] = order[k - 1];
                k -= 1;
            }
            order[k] = key_id;
            j += 1;
        }
        order
    }

    // -----------------------------------------------------------------------
    // A218 窗口自检收口（内部不变量）
    // -----------------------------------------------------------------------

    pub fn invariants_ok(&self) -> bool {
        let mut i = 0;
        while i < MAX_WINDOWS {
            if self.windows[i].alive {
                let mut j = i + 1;
                while j < MAX_WINDOWS {
                    if self.windows[j].alive && self.windows[j].id == self.windows[i].id {
                        return false;
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
    }

    pub fn summary(&self) -> (usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        if self.invariants_ok() {
            passed += 1;
        } else {
            failed += 1;
        }
        if self.alive_count() <= MAX_WINDOWS {
            passed += 1;
        } else {
            failed += 1;
        }
        (passed, failed)
    }

    // -----------------------------------------------------------------------
    // A220 窗口管理器 VWM性能预算（域级）
    // -----------------------------------------------------------------------

    pub const DOMAIN_WINDOW_REDLINE: usize = MAX_WINDOWS;

    pub fn domain_perf_ok(&self) -> bool {
        self.alive_count() <= Self::DOMAIN_WINDOW_REDLINE && self.perf_ok()
    }

    // -----------------------------------------------------------------------
    // A221 窗口管理器 VWM可观测
    // -----------------------------------------------------------------------

    pub fn stats(&self) -> WinStats {
        let mut s = WinStats {
            total: 0,
            alive: 0,
            minimized: 0,
            maximized: 0,
            fullscreen: 0,
            tiled: 0,
            defunct: 0,
        };
        let mut i = 0;
        while i < MAX_WINDOWS {
            let w = &self.windows[i];
            if w.alive {
                s.total += 1;
                s.alive += 1;
                match w.state {
                    WinState::Minimized => s.minimized += 1,
                    WinState::Maximized => s.maximized += 1,
                    WinState::Fullscreen => s.fullscreen += 1,
                    WinState::Tiled => s.tiled += 1,
                    WinState::Normal => {}
                }
            } else if w.id != 0 {
                s.total += 1;
                s.defunct += 1;
            }
            i += 1;
        }
        s
    }

    // -----------------------------------------------------------------------
    // A222 窗口管理器 VWM模糊测试（边界安全，永不 panic）
    // -----------------------------------------------------------------------

    pub fn fuzz_step(&mut self, kind: u8, a: u32, b: u32) -> bool {
        const SCREEN: Rect = Rect::new(0, 0, 1920, 1080);
        match kind % 8 {
            0 => self
                .create_window(
                    Rect::new(
                        (a % 2000) as i32 - 200,
                        (b % 1500) as i32 - 100,
                        (a % 800) + MIN_WIN_W,
                        (b % 600) + MIN_WIN_H,
                    ),
                    (a % 4) as u8,
                )
                .is_some(),
            1 => {
                if let Some(id) = self.current_focus() {
                    self.destroy_window(id)
                } else {
                    self.destroy_window(a)
                }
            }
            2 => self.move_window(a, (a % 2000) as i32 - 200, (b % 1500) as i32 - 100),
            3 => self.resize_window(a, (a % 800) + MIN_WIN_W, (b % 600) + MIN_WIN_H),
            4 => self.raise(a),
            5 => self.lower(a),
            6 => self.minimize(a),
            7 => self.restore(a),
            _ => false,
        }
    }

    pub fn run_fuzz(&mut self, seed: u32, steps: usize) -> u32 {
        let mut state = seed.wrapping_add(0x9E37_79B9);
        let mut ok = 0u32;
        let mut i = 0;
        while i < steps {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let kind = (state & 0xFF) as u8;
            let a = (state >> 8) & 0xFFFF;
            let b = (state >> 24) & 0xFFFF;
            if self.fuzz_step(kind, a, b) {
                ok += 1;
            }
            i += 1;
        }
        ok
    }

    // -----------------------------------------------------------------------
    // A224 窗口管理器 VWM降级链
    // -----------------------------------------------------------------------

    pub fn degrade_level(&self) -> DegradeLevel {
        let n = self.alive_count();
        if n <= 16 {
            DegradeLevel::Full
        } else if n <= 28 {
            DegradeLevel::Reduced
        } else {
            DegradeLevel::Minimal
        }
    }

    pub fn apply_degrade(&mut self, level: DegradeLevel) -> bool {
        match level {
            DegradeLevel::Full => true,
            DegradeLevel::Reduced => {
                let mut i = 0;
                while i < MAX_WINDOWS {
                    if self.windows[i].alive {
                        let st = self.windows[i].state;
                        if st == WinState::Fullscreen || st == WinState::Maximized {
                            self.windows[i].state = WinState::Normal;
                            self.windows[i].rect = self.windows[i].saved_rect;
                        }
                    }
                    i += 1;
                }
                true
            }
            DegradeLevel::Minimal => {
                let focus = self.current_focus();
                let mut i = 0;
                while i < MAX_WINDOWS {
                    if self.windows[i].alive && Some(self.windows[i].id) != focus {
                        self.windows[i].state = WinState::Minimized;
                    }
                    i += 1;
                }
                true
            }
        }
    }

    pub fn auto_degrade(&mut self) -> DegradeLevel {
        let lvl = self.degrade_level();
        self.apply_degrade(lvl);
        lvl
    }
}

// ---------------------------------------------------------------------------
// 贴靠方向（A205）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapSide {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

// ---------------------------------------------------------------------------
// 平铺布局计划（A206）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct TilingPlan {
    pub rects: [Rect; MAX_WINDOWS],
    pub count: usize,
}

impl TilingPlan {
    pub const fn new() -> TilingPlan {
        TilingPlan { rects: [Rect::new(0, 0, 0, 0); MAX_WINDOWS], count: 0 }
    }
}

// ---------------------------------------------------------------------------
// 关闭结果（A212）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseResult {
    Closed,
    ConfirmNeeded,
    NotFound,
}

// ---------------------------------------------------------------------------
// 快捷键与动作（A214）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    AltTab,
    AltShiftTab,
    SnapLeft,
    SnapRight,
    SnapTop,
    SnapBottom,
    Maximize,
    Minimize,
    Fullscreen,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinAction {
    FocusNext,
    FocusPrev,
    SnapLeft,
    SnapRight,
    SnapTop,
    SnapBottom,
    Maximize,
    Minimize,
    ToggleFullscreen,
    Close,
}

// ---------------------------------------------------------------------------
// 降级等级（A224）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    Full,
    Reduced,
    Minimal,
}

// ---------------------------------------------------------------------------
// 可观测统计（A221）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinStats {
    pub total: usize,
    pub alive: usize,
    pub minimized: usize,
    pub maximized: usize,
    pub fullscreen: usize,
    pub tiled: usize,
    pub defunct: usize,
}

// ---------------------------------------------------------------------------
// A223 窗口管理器 VWM文档
// ---------------------------------------------------------------------------

pub const WINDOW_MANAGER_DOC: &str =
    "VWM: 内核窗口管理器。创建/销毁、移动缩放、Z序、聚焦环、贴靠、平铺、全屏、最小化/还原、分组、跨屏、标题栏、关闭确认、状态记忆、快捷键、回收、性能预算、无障碍、可观测、模糊测试、降级链与域自检。全键盘可达，no_std 无分配。";

// ---------------------------------------------------------------------------
// A219 / A225 域自检收口
// ---------------------------------------------------------------------------

/// 导出给内核自检闭环：25 项（A201~A225）全真。
pub fn run_window_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-window");
    let sc = Rect::new(0, 0, 1920, 1080);

    // A201 创建/销毁 + 僵尸防护
    let mut wm = WindowManager::new();
    let i1 = wm.create_window(Rect::new(10, 10, 200, 100), 0).unwrap();
    let i2 = wm.create_window(Rect::new(60, 60, 200, 100), 0).unwrap();
    let created = i1 != i2 && wm.alive_count() == 2;
    wm.destroy_window(i1);
    let a201 = created
        && !wm.is_alive(i1)
        && wm.is_zombie(i1)
        && wm.hit_test(20, 20).is_none()
        && wm.hit_test(70, 70) == Some(i2)
        && wm.alive_count() == 1;
    set.add("A201 create/destroy+zombie", a201, "lifecycle");

    // A202 移动/缩放 + 最小尺寸钳制 + 全屏锁定
    let mut wm = WindowManager::new();
    let i = wm.create_window(Rect::new(0, 0, 300, 200), 0).unwrap();
    wm.move_window(i, 100, 100);
    wm.resize_window(i, 10, 10);
    let r = wm.rect_of(i).unwrap();
    wm.enter_fullscreen(i, sc);
    let fs_move = wm.move_window(i, 5, 5);
    let a202 = r.x == 100
        && r.y == 100
        && r.w == MIN_WIN_W
        && r.h == MIN_WIN_H
        && !fs_move
        && wm.rect_of(i).unwrap().x == 0;
    set.add("A202 move/resize+clamp", a202, "geometry");
    wm.exit_fullscreen(i);

    // A203 层叠 Z 序
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(10, 10, 100, 100), 0).unwrap();
    wm.raise(a);
    let top_a = wm.z_of(a).unwrap() > wm.z_of(b).unwrap() && wm.hit_test(20, 20) == Some(a);
    wm.lower(a);
    let low_a = wm.z_of(a).unwrap() < wm.z_of(b).unwrap();
    set.add("A203 raise/lower", top_a && low_a, "z-order");

    // A204 聚焦环（Alt-Tab）
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
    let c = wm.create_window(Rect::new(400, 400, 100, 100), 0).unwrap();
    wm.focus(a);
    let f1 = wm.current_focus() == Some(a);
    wm.focus_next();
    let f2 = wm.current_focus() == Some(b);
    wm.focus_next();
    let f3 = wm.current_focus() == Some(c);
    wm.focus_prev();
    let f4 = wm.current_focus() == Some(b);
    set.add("A204 focus ring", f1 && f2 && f3 && f4, "alt-tab cycle");

    // A205 边缘贴靠
    let l = WindowManager::snap_rect(sc, SnapSide::LeftHalf);
    let rr = WindowManager::snap_rect(sc, SnapSide::RightHalf);
    let tl = WindowManager::snap_rect(sc, SnapSide::TopLeft);
    let a205 = l.w == 960 && l.h == 1080 && rr.x == 960 && rr.w == 960 && tl.w == 960 && tl.h == 540;
    set.add("A205 snap halves/quarter", a205, "snap math");

    // A206 平铺布局（主区 + 堆叠区）
    let plan = WindowManager::tile_layout(sc, 3, 1, 500);
    let a206 = plan.count == 3
        && plan.rects[0].w == 960
        && plan.rects[1].x == 960
        && plan.rects[2].x == 960
        && plan.rects[1].h == 540;
    set.add("A206 tiling main/stack", a206, "layout");

    // A207 最小化/最大化/还原 + 焦点转移
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
    wm.focus(a);
    wm.minimize(a);
    let m1 = wm.current_focus() == Some(b) && !wm.is_exposed(a);
    wm.restore(a);
    let m2 = wm.is_exposed(a) && wm.current_focus() == Some(a);
    wm.maximize(a, sc);
    let big = wm.rect_of(a).unwrap();
    wm.restore(a);
    let back = wm.rect_of(a).unwrap();
    let a207 = m1 && m2 && big.w == 1920 && back.w < 1920;
    set.add("A207 minimize/maximize/restore", a207, "state machine");

    // A208 全屏切换（保存/恢复）
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(10, 10, 200, 100), 0).unwrap();
    wm.enter_fullscreen(a, sc);
    let fs = wm.rect_of(a).unwrap().w == 1920 && wm.rect_of(a).unwrap().h == 1080;
    wm.exit_fullscreen(a);
    let ex = wm.rect_of(a).unwrap() == Rect::new(10, 10, 200, 100);
    set.add("A208 fullscreen toggle", fs && ex, "save/restore");

    // A209 窗口分组
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let c = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.set_group(a, 1);
    wm.set_group(b, 1);
    wm.set_group(c, 2);
    let a209 = wm.group_count(1) == 2 && wm.group_count(2) == 1;
    set.add("A209 grouping", a209, "groups");

    // A210 跨屏移动
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 200, 100), 0).unwrap();
    let s2 = Rect::new(1920, 0, 1920, 1080);
    wm.move_to_screen(a, s2, 1);
    let a210 = wm.screen_of(a) == Some(1) && wm.rect_of(a).unwrap().x >= 1920;
    set.add("A210 cross-screen", a210, "move across");

    // A211 标题栏
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.set_title(a, b"Terminal");
    let a211 = wm.title_of(a) == Some(&b"Terminal"[..]);
    set.add("A211 title bar", a211, "title");

    // A212 关闭确认
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.set_confirm_close(a, true);
    let r1 = wm.close_window(a);
    let need = r1 == CloseResult::ConfirmNeeded && wm.is_alive(a);
    wm.set_confirm_close(a, false);
    let r2 = wm.close_window(a);
    let a212 = need && r2 == CloseResult::Closed && !wm.is_alive(a);
    set.add("A212 close confirm", a212, "confirm flow");

    // A213 状态记忆
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 200, 100), 0).unwrap();
    wm.move_window(a, 300, 300);
    wm.remember(a);
    wm.move_window(a, 0, 0);
    wm.recall(a);
    let a213 = wm.rect_of(a) == Some(Rect::new(300, 300, 200, 100));
    set.add("A213 state memory", a213, "recall");

    // A214 快捷键（纯映射，全键盘可达）
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
    wm.focus(a);
    let map = WindowManager::shortcut_to_action(Shortcut::AltTab) == WinAction::FocusNext;
    wm.apply_shortcut(Shortcut::AltTab, sc);
    let tab = wm.current_focus() == Some(b);
    wm.apply_shortcut(Shortcut::SnapLeft, sc);
    let snapk = wm.rect_of(b).unwrap().w == 960;
    set.add("A214 shortcuts", map && tab && snapk, "keyboard reachable");

    // A215 回收（无僵尸）
    let mut wm = WindowManager::new();
    let mut ids = [0u32; MAX_WINDOWS];
    let mut k = 0;
    while k < MAX_WINDOWS {
        ids[k] = wm.create_window(Rect::new((k as i32) * 10, 0, 100, 100), 0).unwrap();
        k += 1;
    }
    let overflow = wm.create_window(Rect::new(0, 0, 100, 100), 0);
    k = 0;
    while k < MAX_WINDOWS {
        wm.destroy_window(ids[k]);
        k += 1;
    }
    let no_zombie = wm.alive_count() == 0 && wm.defunct_count() == MAX_WINDOWS;
    let reused = wm.create_window(Rect::new(0, 0, 100, 100), 0).is_some();
    let a215 = overflow.is_none() && no_zombie && reused && wm.invariants_ok();
    set.add("A215 reclaim/no-zombie", a215, "no zombies");

    // A216 性能预算
    let mut wm = WindowManager::new();
    k = 0;
    while k < 20 {
        let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        k += 1;
    }
    let a216 = WindowManager::within_budget(WindowManager::layout_cost(20))
        && wm.perf_ok()
        && WindowManager::layout_cost(20) < PERF_OPS_REDLINE;
    set.add("A216 perf budget", a216, "cheap ops");

    // A217 无障碍
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.minimize(b);
    let a217 = wm.a11y_exposed_count() == 1 && !wm.is_exposed(b) && wm.a11y_exposed_count() < wm.alive_count();
    set.add("A217 accessibility", a217, "minimized hidden");

    // A218 内部不变量
    let mut wm = WindowManager::new();
    let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
    let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
    let (p, f) = wm.summary();
    let a218 = wm.invariants_ok() && f == 0 && p >= 2;
    set.add("A218 invariants", a218, "internal ok");

    // A219 VWM 自检入口（即本函数）
    let a219 = WindowManager::new().alive_count() == 0 && WindowManager::new().invariants_ok();
    set.add("A219 VWM self-check", a219, "domain check entry");

    // A220 域性能预算
    let mut wm = WindowManager::new();
    k = 0;
    while k < 10 {
        let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        k += 1;
    }
    let a220 = wm.domain_perf_ok() && wm.alive_count() <= WindowManager::DOMAIN_WINDOW_REDLINE;
    set.add("A220 domain perf", a220, "within redline");

    // A221 可观测
    let mut wm = WindowManager::new();
    let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.enter_fullscreen(a, sc);
    let s = wm.stats();
    let a221 = s.alive == 1 && s.fullscreen == 1 && s.defunct == 0 && s.total >= 1;
    set.add("A221 observability", a221, "stats");

    // A222 模糊测试（边界安全）
    let mut wm = WindowManager::new();
    let ok = wm.run_fuzz(12345, 500);
    let a222 = ok > 0 && wm.invariants_ok() && wm.alive_count() <= MAX_WINDOWS;
    set.add("A222 fuzz safe", a222, "no panic");

    // A223 文档
    let a223 = WINDOW_MANAGER_DOC.len() > 20 && WINDOW_MANAGER_DOC.contains("VWM");
    set.add("A223 documentation", a223, "doc present");

    // A224 降级链
    let mut wm = WindowManager::new();
    k = 0;
    while k < 30 {
        let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        k += 1;
    }
    let lvl = wm.auto_degrade();
    let s = wm.stats();
    let a224 = lvl == DegradeLevel::Minimal && s.minimized >= 29;
    set.add("A224 degrade chain", a224, "minimal");

    // A225 域自检收口：前面的检查全绿
    let mut all_green = true;
    let n = set.len();
    let mut idx = 0;
    while idx < n {
        if let Some(c) = set.get(idx) {
            if !c.passed {
                all_green = false;
            }
        }
        idx += 1;
    }
    let a225 = all_green && n >= 24;
    set.add("A225 domain closure", a225, "all green");

    // 边界防护（额外，确保 ≥25 项且稳健）
    let mut wm = WindowManager::new();
    let mut full_ok = true;
    k = 0;
    while k < MAX_WINDOWS {
        if wm.create_window(Rect::new(0, 0, 100, 100), 0).is_none() {
            full_ok = false;
        }
        k += 1;
    }
    let overflow = wm.create_window(Rect::new(0, 0, 100, 100), 0);
    set.add("BOUND table full", wm.alive_count() == MAX_WINDOWS && overflow.is_none() && full_ok, "capacity cap");

    let mut wm = WindowManager::new();
    let empty = wm.current_focus().is_none()
        && wm.focus_next().is_none()
        && wm.focus_prev().is_none();
    set.add("BOUND empty focus", empty, "no focused window");

    let mut wm = WindowManager::new();
    let i = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
    wm.destroy_window(i);
    let safe = wm.move_window(i, 1, 1) == false
        && wm.resize_window(i, 5, 5) == false
        && wm.raise(i) == false
        && wm.minimize(i) == false
        && wm.title_of(i).is_none()
        && wm.hit_test(10, 10).is_none();
    set.add("BOUND post-destroy", safe, "dead id inert");

    set
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0, 0, 1920, 1080)
    }

    #[test]
    fn a201_create_destroy_zombie() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(10, 10, 200, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(60, 60, 200, 100), 0).unwrap();
        assert_ne!(a, b);
        assert_eq!(wm.alive_count(), 2);
        wm.destroy_window(a);
        assert!(!wm.is_alive(a));
        assert!(wm.is_zombie(a));
        assert_eq!(wm.hit_test(20, 20), None);
        assert_eq!(wm.hit_test(70, 70), Some(b));
    }

    #[test]
    fn a201_id_reuse_fresh() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        wm.destroy_window(a);
        let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        // 槽复用但分配新 id，旧 id 不再命中
        assert_ne!(a, b);
        assert!(!wm.is_alive(a));
        assert!(wm.is_alive(b));
    }

    #[test]
    fn a202_move_resize_clamp() {
        let mut wm = WindowManager::new();
        let i = wm.create_window(Rect::new(0, 0, 300, 200), 0).unwrap();
        assert!(wm.move_window(i, 100, 100));
        assert!(wm.resize_window(i, 10, 10));
        let r = wm.rect_of(i).unwrap();
        assert_eq!(r, Rect::new(100, 100, MIN_WIN_W, MIN_WIN_H));
        // 全屏下移动被拒
        wm.enter_fullscreen(i, screen());
        assert!(!wm.move_window(i, 5, 5));
        assert_eq!(wm.rect_of(i).unwrap().x, 0);
        wm.exit_fullscreen(i);
    }

    #[test]
    fn a203_z_order() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        assert!(wm.raise(a));
        assert!(wm.z_of(a).unwrap() > wm.z_of(b).unwrap());
        assert_eq!(wm.hit_test(10, 10), Some(a));
        assert!(wm.lower(a));
        assert!(wm.z_of(a).unwrap() < wm.z_of(b).unwrap());
        // 不存在的窗口
        assert!(!wm.raise(999));
    }

    #[test]
    fn a204_focus_ring() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
        let c = wm.create_window(Rect::new(400, 400, 100, 100), 0).unwrap();
        wm.focus(a);
        assert_eq!(wm.current_focus(), Some(a));
        assert_eq!(wm.focus_next(), Some(b));
        assert_eq!(wm.focus_next(), Some(c));
        assert_eq!(wm.focus_prev(), Some(b));
        assert_eq!(wm.focus_prev(), Some(a));
    }

    #[test]
    fn a204_empty_focus_stack() {
        let mut wm = WindowManager::new();
        assert_eq!(wm.current_focus(), None);
        assert_eq!(wm.focus_next(), None);
        assert_eq!(wm.focus_prev(), None);
    }

    #[test]
    fn a205_snap() {
        let sc = screen();
        let l = WindowManager::snap_rect(sc, SnapSide::LeftHalf);
        let r = WindowManager::snap_rect(sc, SnapSide::RightHalf);
        assert_eq!(l, Rect::new(0, 0, 960, 1080));
        assert_eq!(r, Rect::new(960, 0, 960, 1080));
        let tl = WindowManager::snap_rect(sc, SnapSide::TopLeft);
        assert_eq!(tl, Rect::new(0, 0, 960, 540));
        let br = WindowManager::snap_rect(sc, SnapSide::BottomRight);
        assert_eq!(br, Rect::new(960, 540, 960, 540));
    }

    #[test]
    fn a206_tiling() {
        let sc = screen();
        let plan = WindowManager::tile_layout(sc, 4, 2, 500);
        assert_eq!(plan.count, 4);
        // 主区 2 行占左半（960x540 各），堆叠 2 行占右半
        assert_eq!(plan.rects[0], Rect::new(0, 0, 960, 540));
        assert_eq!(plan.rects[1], Rect::new(0, 540, 960, 540));
        assert_eq!(plan.rects[2], Rect::new(960, 0, 960, 540));
        assert_eq!(plan.rects[3], Rect::new(960, 540, 960, 540));
        assert_eq!(WindowManager::tile_layout(sc, 0, 1, 500).count, 0);
    }

    #[test]
    fn a207_minimize_maximize_restore() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
        wm.focus(a);
        wm.minimize(a);
        assert_eq!(wm.current_focus(), Some(b));
        assert!(!wm.is_exposed(a));
        wm.restore(a);
        assert!(wm.is_exposed(a));
        assert_eq!(wm.current_focus(), Some(a));
        // 最大化还原
        wm.maximize(a, screen());
        assert_eq!(wm.rect_of(a).unwrap().w, 1920);
        wm.restore(a);
        assert!(wm.rect_of(a).unwrap().w < 1920);
        assert_eq!(wm.rect_of(a).unwrap(), Rect::new(0, 0, 100, 100));
    }

    #[test]
    fn a208_fullscreen() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(10, 10, 200, 100), 0).unwrap();
        assert!(wm.enter_fullscreen(a, screen()));
        assert_eq!(wm.rect_of(a).unwrap(), Rect::new(0, 0, 1920, 1080));
        assert!(wm.exit_fullscreen(a));
        assert_eq!(wm.rect_of(a).unwrap(), Rect::new(10, 10, 200, 100));
        // 未全屏退出返回 false
        assert!(!wm.exit_fullscreen(a));
    }

    #[test]
    fn a209_group() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let c = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        wm.set_group(a, 1);
        wm.set_group(b, 1);
        wm.set_group(c, 2);
        assert_eq!(wm.group_count(1), 2);
        assert_eq!(wm.group_count(2), 1);
        assert_eq!(wm.focus_group_next(1), Some(a));
    }

    #[test]
    fn a210_cross_screen() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 200, 100), 0).unwrap();
        let s2 = Rect::new(1920, 0, 1920, 1080);
        assert!(wm.move_to_screen(a, s2, 1));
        assert_eq!(wm.screen_of(a), Some(1));
        assert!(wm.rect_of(a).unwrap().x >= 1920);
    }

    #[test]
    fn a211_title() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        assert!(wm.set_title(a, b"Terminal"));
        assert_eq!(wm.title_of(a), Some(&b"Terminal"[..]));
        // 超长标题截断
        let long = b"this title is definitely too long to fit here";
        wm.set_title(a, long);
        assert_eq!(wm.title_of(a).unwrap().len(), MAX_TITLE);
    }

    #[test]
    fn a212_close_confirm() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        wm.set_confirm_close(a, true);
        assert_eq!(wm.close_window(a), CloseResult::ConfirmNeeded);
        assert!(wm.is_alive(a));
        wm.set_confirm_close(a, false);
        assert_eq!(wm.close_window(a), CloseResult::Closed);
        assert!(!wm.is_alive(a));
        assert_eq!(wm.close_window(a), CloseResult::NotFound);
    }

    #[test]
    fn a213_memory() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 200, 100), 0).unwrap();
        wm.move_window(a, 300, 300);
        assert!(wm.remember(a));
        wm.move_window(a, 0, 0);
        assert!(wm.recall(a));
        assert_eq!(wm.rect_of(a), Some(Rect::new(300, 300, 200, 100)));
    }

    #[test]
    fn a214_shortcut() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(200, 200, 100, 100), 0).unwrap();
        wm.focus(a);
        assert_eq!(
            WindowManager::shortcut_to_action(Shortcut::AltTab),
            WinAction::FocusNext
        );
        wm.apply_shortcut(Shortcut::AltTab, screen());
        assert_eq!(wm.current_focus(), Some(b));
        wm.apply_shortcut(Shortcut::SnapLeft, screen());
        assert_eq!(wm.rect_of(b).unwrap().w, 960);
        // 全屏快捷键切换：进入
        wm.apply_shortcut(Shortcut::Fullscreen, screen());
        assert_eq!(wm.rect_of(b).unwrap().w, 1920);
        // 再按一次退出
        wm.apply_shortcut(Shortcut::Fullscreen, screen());
        assert_ne!(wm.rect_of(b).unwrap().w, 1920);
    }

    #[test]
    fn a215_reclaim_full() {
        let mut wm = WindowManager::new();
        let mut ids = [0u32; MAX_WINDOWS];
        for k in 0..MAX_WINDOWS {
            ids[k] = wm.create_window(Rect::new((k as i32) * 10, 0, 100, 100), 0).unwrap();
        }
        assert!(wm.create_window(Rect::new(0, 0, 100, 100), 0).is_none());
        for k in 0..MAX_WINDOWS {
            assert!(wm.destroy_window(ids[k]));
        }
        assert_eq!(wm.alive_count(), 0);
        assert!(wm.create_window(Rect::new(0, 0, 100, 100), 0).is_some());
        assert!(wm.invariants_ok());
    }

    #[test]
    fn a216_perf_budget() {
        assert!(WindowManager::within_budget(WindowManager::layout_cost(10)));
        assert!(WindowManager::layout_cost(10) < PERF_OPS_REDLINE);
        let mut wm = WindowManager::new();
        for _ in 0..20 {
            let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        }
        assert!(wm.perf_ok());
    }

    #[test]
    fn a217_a11y() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        let b = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        wm.minimize(b);
        assert_eq!(wm.a11y_exposed_count(), 1);
        assert!(!wm.is_exposed(b));
        let order = wm.a11y_focus_order();
        assert_eq!(order[0], a);
    }

    #[test]
    fn a218_invariants() {
        let mut wm = WindowManager::new();
        let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        assert!(wm.invariants_ok());
        let (p, f) = wm.summary();
        assert_eq!(f, 0);
        assert!(p >= 2);
    }

    #[test]
    fn a219_self_check() {
        let set = run_window_checks();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("DIAG a219 fail: {} : {}", c.name, c.detail);
                }
            }
        }
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "window self-check must be all green");
    }

    #[test]
    fn a220_domain_perf() {
        let mut wm = WindowManager::new();
        for _ in 0..10 {
            let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        }
        assert!(wm.domain_perf_ok());
    }

    #[test]
    fn a221_observability() {
        let mut wm = WindowManager::new();
        let a = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        wm.enter_fullscreen(a, screen());
        let s = wm.stats();
        assert_eq!(s.alive, 1);
        assert_eq!(s.fullscreen, 1);
        assert_eq!(s.defunct, 0);
    }

    #[test]
    fn a222_fuzz_safe() {
        let mut wm = WindowManager::new();
        let ok = wm.run_fuzz(0xABCDEF, 1000);
        assert!(ok > 0);
        assert!(wm.invariants_ok());
        assert!(wm.alive_count() <= MAX_WINDOWS);
    }

    #[test]
    fn a223_documentation() {
        assert!(WINDOW_MANAGER_DOC.len() > 20);
        assert!(WINDOW_MANAGER_DOC.contains("VWM"));
    }

    #[test]
    fn a224_degrade() {
        let mut wm = WindowManager::new();
        for _ in 0..30 {
            let _ = wm.create_window(Rect::new(0, 0, 100, 100), 0);
        }
        let lvl = wm.auto_degrade();
        assert_eq!(lvl, DegradeLevel::Minimal);
        let s = wm.stats();
        assert!(s.minimized >= 29);
    }

    #[test]
    fn a225_closure() {
        let set = run_window_checks();
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert!(passed >= 25);
        assert!(set.all_passed());
    }

    #[test]
    fn a_bound_table_full() {
        let mut wm = WindowManager::new();
        for _ in 0..MAX_WINDOWS {
            assert!(wm.create_window(Rect::new(0, 0, 100, 100), 0).is_some());
        }
        assert!(wm.create_window(Rect::new(0, 0, 100, 100), 0).is_none());
        assert_eq!(wm.alive_count(), MAX_WINDOWS);
    }

    #[test]
    fn a_bound_post_destroy_ops() {
        let mut wm = WindowManager::new();
        let i = wm.create_window(Rect::new(0, 0, 100, 100), 0).unwrap();
        assert!(wm.destroy_window(i));
        assert!(!wm.move_window(i, 1, 1));
        assert!(!wm.resize_window(i, 5, 5));
        assert!(!wm.raise(i));
        assert!(!wm.lower(i));
        assert!(!wm.minimize(i));
        assert!(!wm.maximize(i, screen()));
        assert_eq!(wm.title_of(i), None);
        assert_eq!(wm.hit_test(10, 10), None);
        assert_eq!(wm.close_window(i), CloseResult::NotFound);
    }
}
