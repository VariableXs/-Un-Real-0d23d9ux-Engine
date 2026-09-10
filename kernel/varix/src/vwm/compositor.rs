//! TRINITY-500 · AI-09 VWM 窗口合成器域（F201~F224，W3）
//!
//! 把 VARIX-500 AI-17 的 vsem 窗口树**画**出来：Z 序、遮挡、命中、拖拽、贴靠、
//! 分屏、Alt+Tab、快照、画中画、事件总线、性能预算与越界钳制。
//! 所有几何运算走整数 permille，保证 QEMU 与真机手感一致。

use crate::gfx::surface::Rect;

pub const MAX_WINDOWS: usize = 16;
pub const SNAP_THRESHOLD: i32 = 12;
pub const MIN_VISIBLE: i32 = 48;

// ---------------------------------------------------------------------------
// F201 窗口树渲染 — 窗口记录
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinState {
    Normal,
    Minimised,
    Maximised,
    Fullscreen,
    Snapped,
}

impl WinState {
    pub fn name(self) -> &'static str {
        match self {
            WinState::Normal => "normal",
            WinState::Minimised => "minimised",
            WinState::Maximised => "maximised",
            WinState::Fullscreen => "fullscreen",
            WinState::Snapped => "snapped",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMaterial {
    Opaque,
    Glass,
    Acrylic,
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub id: u16,
    pub rect: Rect,
    /// 进入 Maximised/Snapped 前的几何，用于恢复。
    pub saved: Rect,
    pub z: i16,
    pub state: WinState,
    pub visible: bool,
    /// 0..255
    pub alpha: u8,
    pub material: WindowMaterial,
    /// 窗口编组号（0 = 未编组）
    pub group: u8,
    pub app: &'static str,
    pub focused: bool,
    /// 画中画标记
    pub pip: bool,
}

impl Window {
    pub fn new(id: u16, rect: Rect, app: &'static str) -> Window {
        Window {
            id,
            rect,
            saved: rect,
            z: 0,
            state: WinState::Normal,
            visible: true,
            alpha: 255,
            material: WindowMaterial::Opaque,
            group: 0,
            app,
            focused: false,
            pip: false,
        }
    }

    /// 参与命中与渲染的窗口：可见且未最小化。
    pub fn live(&self) -> bool {
        self.visible && self.state != WinState::Minimised
    }
}

pub struct WindowTree {
    windows: [Option<Window>; MAX_WINDOWS],
    count: usize,
    top_z: i16,
    /// MRU 顺序（Alt+Tab 用），下标 0 为最近使用。
    mru: [u16; MAX_WINDOWS],
    mru_count: usize,
}

impl WindowTree {
    pub const fn new() -> WindowTree {
        WindowTree { windows: [None; MAX_WINDOWS], count: 0, top_z: 0, mru: [0; MAX_WINDOWS], mru_count: 0 }
    }

    pub fn add(&mut self, w: Window) -> bool {
        if self.count >= MAX_WINDOWS {
            return false;
        }
        if (0..self.count).any(|i| self.windows[i].map(|x| x.id) == Some(w.id)) {
            return false;
        }
        let mut w = w;
        self.top_z += 1;
        w.z = self.top_z;
        self.windows[self.count] = Some(w);
        self.count += 1;
        self.touch_mru(w.id);
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Window> {
        if i < self.count {
            self.windows[i]
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Window> {
        if i < self.count {
            self.windows[i].as_mut()
        } else {
            None
        }
    }

    pub fn by_id(&self, id: u16) -> Option<usize> {
        (0..self.count).find(|i| self.windows[*i].map(|w| w.id) == Some(id))
    }

    pub fn remove(&mut self, id: u16) -> Option<Window> {
        let idx = self.by_id(id)?;
        let w = self.windows[idx];
        self.windows[idx] = self.windows[self.count - 1];
        self.windows[self.count - 1] = None;
        self.count -= 1;
        self.remove_mru(id);
        w
    }

    // ---------------------------------------------------------------- F202
    /// 置顶：拿到最大 z。
    pub fn raise(&mut self, id: u16) -> bool {
        let idx = match self.by_id(id) {
            Some(i) => i,
            None => return false,
        };
        self.top_z += 1;
        let z = self.top_z;
        if let Some(w) = self.get_mut(idx) {
            w.z = z;
            w.focused = true;
        }
        for i in 0..self.count {
            if let Some(w) = self.get_mut(i) {
                if w.id != id {
                    w.focused = false;
                }
            }
        }
        self.touch_mru(id);
        true
    }

    /// 按 z 从小到大输出索引（0 = 最底）。
    pub fn z_order(&self, out: &mut [usize]) -> usize {
        let mut idx = [0usize; MAX_WINDOWS];
        for i in 0..self.count {
            idx[i] = i;
        }
        let n = self.count;
        for i in 0..n {
            let mut best = i;
            for j in (i + 1)..n {
                let (a, b) = (self.get(idx[j]).map(|w| w.z), self.get(idx[best]).map(|w| w.z));
                if a.unwrap_or(0) < b.unwrap_or(0) {
                    best = j;
                }
            }
            idx.swap(i, best);
        }
        let take = if n < out.len() { n } else { out.len() };
        out[..take].copy_from_slice(&idx[..take]);
        take
    }

    /// F202：遮挡率（permille）。用 16×16 网格采样求被上方窗口覆盖的比例——
    /// 精确矩形布尔运算太贵，采样足以驱动「是否整窗跳过绘制」。
    pub fn occluded_permille(&self, i: usize) -> u32 {
        let target = match self.get(i) {
            Some(w) if w.live() => w.rect,
            _ => return 1000,
        };
        if target.is_empty() {
            return 1000;
        }
        let z = self.get(i).map(|w| w.z).unwrap_or(0);
        let grid = 16i32;
        let mut covered = 0u32;
        for gy in 0..grid {
            for gx in 0..grid {
                let x = target.x + (target.w * gx) / grid;
                let y = target.y + (target.h * gy) / grid;
                let mut hit = false;
                for k in 0..self.count {
                    if k == i {
                        continue;
                    }
                    if let Some(o) = self.get(k) {
                        if !o.live() || o.z <= z {
                            continue;
                        }
                        if x >= o.rect.x && y >= o.rect.y && x < o.rect.right() && y < o.rect.bottom() {
                            hit = true;
                            break;
                        }
                    }
                }
                if hit {
                    covered += 1;
                }
            }
        }
        (covered * 1000) / (grid as u32 * grid as u32)
    }

    /// 完全被遮挡 → 可跳过绘制（省算力，不影响正确性）。
    pub fn fully_occluded(&self, i: usize) -> bool {
        self.occluded_permille(i) >= 1000
    }

    // ---------------------------------------------------------------- F203
    /// 命中：从最上层往下找。
    pub fn hit(&self, x: i32, y: i32) -> Option<u16> {
        let mut best: Option<(i16, u16)> = None;
        for i in 0..self.count {
            if let Some(w) = self.get(i) {
                if !w.live() {
                    continue;
                }
                if x >= w.rect.x && y >= w.rect.y && x < w.rect.right() && y < w.rect.bottom() {
                    match best {
                        Some((z, _)) if z >= w.z => {}
                        _ => best = Some((w.z, w.id)),
                    }
                }
            }
        }
        best.map(|(_, id)| id)
    }

    pub fn focus(&mut self, x: i32, y: i32) -> Option<u16> {
        let id = self.hit(x, y)?;
        self.raise(id);
        Some(id)
    }

    // ---------------------------------------------------------------- F209
    fn touch_mru(&mut self, id: u16) {
        self.remove_mru(id);
        // 最近使用的排在最前（下标 0）。
        if self.mru_count < MAX_WINDOWS {
            for i in (1..=self.mru_count).rev() {
                self.mru[i] = self.mru[i - 1];
            }
            self.mru[0] = id;
            self.mru_count += 1;
        } else {
            for i in (1..MAX_WINDOWS).rev() {
                self.mru[i] = self.mru[i - 1];
            }
            self.mru[0] = id;
        }
    }

    fn remove_mru(&mut self, id: u16) {
        let mut n = 0usize;
        for i in 0..self.mru_count {
            if self.mru[i] != id {
                self.mru[n] = self.mru[i];
                n += 1;
            }
        }
        self.mru_count = n;
    }

    /// Alt+Tab：在存活窗口里按 MRU 顺序取第 `steps` 个并置顶。
    pub fn alt_tab(&mut self, steps: usize) -> Option<u16> {
        let mut live = [0u16; MAX_WINDOWS];
        let mut n = 0usize;
        for i in 0..self.mru_count {
            let id = self.mru[i];
            if let Some(idx) = self.by_id(id) {
                if self.get(idx).map(|w| w.live()).unwrap_or(false) {
                    live[n] = id;
                    n += 1;
                }
            }
        }
        if n == 0 {
            return None;
        }
        let id = live[steps % n];
        self.raise(id);
        Some(id)
    }

    pub fn mru(&self) -> &[u16] {
        &self.mru[..self.mru_count]
    }

    // ---------------------------------------------------------------- F208
    /// 窗口编组：同组一起最小化/恢复/置顶。
    pub fn group_ids(&self, group: u8, out: &mut [u16]) -> usize {
        if group == 0 {
            return 0;
        }
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(w) = self.get(i) {
                if w.group == group && n < out.len() {
                    out[n] = w.id;
                    n += 1;
                }
            }
        }
        n
    }

    // ---------------------------------------------------------------- F215
    /// 多开：同一 app 允许的实例数上限（防止无限开窗口拖垮合成器）。
    pub fn instances_of(&self, app: &str) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|w| w.app) == Some(app)).count()
    }

    pub const MAX_INSTANCES: usize = MAX_WINDOWS;

    pub fn can_open(&self, app: &str, limit: usize) -> bool {
        self.instances_of(app) < limit && self.count < MAX_WINDOWS
    }

    // ---------------------------------------------------------------- F211
    /// 快照：保存当前所有窗口的几何与状态。
    pub fn snapshot(&self, out: &mut [WindowSnapshot]) -> usize {
        let mut n = 0usize;
        for i in 0..self.count {
            if let Some(w) = self.get(i) {
                if n >= out.len() {
                    break;
                }
                out[n] = WindowSnapshot { id: w.id, rect: w.rect, state: w.state, z: w.z };
                n += 1;
            }
        }
        n
    }

    /// 恢复：按快照还原几何与状态（找不到的窗口保持原样）。
    pub fn restore(&mut self, snap: &[WindowSnapshot]) -> usize {
        let mut n = 0usize;
        for s in snap.iter() {
            if let Some(idx) = self.by_id(s.id) {
                if let Some(w) = self.get_mut(idx) {
                    w.rect = s.rect;
                    w.state = s.state;
                    w.z = s.z;
                    w.visible = true;
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for WindowTree {
    fn default() -> Self {
        WindowTree::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowSnapshot {
    pub id: u16,
    pub rect: Rect,
    pub state: WinState,
    pub z: i16,
}

// ---------------------------------------------------------------------------
// F204 拖拽移动/缩放
// F220 窗口越界钳制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    None,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub const MIN_W: i32 = 160;
pub const MIN_H: i32 = 96;
pub const EDGE_GRAB: i32 = 6;

impl ResizeEdge {
    /// 判断指针落在窗口的哪条边（用于启动缩放而非拖拽）。
    pub fn detect(r: &Rect, x: i32, y: i32) -> ResizeEdge {
        let near_left = (x - r.x).abs() <= EDGE_GRAB;
        let near_right = (r.right() - x).abs() <= EDGE_GRAB;
        let near_top = (y - r.y).abs() <= EDGE_GRAB;
        let near_bottom = (r.bottom() - y).abs() <= EDGE_GRAB;
        match (near_top, near_bottom, near_left, near_right) {
            (true, _, true, _) => ResizeEdge::TopLeft,
            (true, _, _, true) => ResizeEdge::TopRight,
            (_, true, true, _) => ResizeEdge::BottomLeft,
            (_, true, _, true) => ResizeEdge::BottomRight,
            (true, _, _, _) => ResizeEdge::Top,
            (_, true, _, _) => ResizeEdge::Bottom,
            (_, _, true, _) => ResizeEdge::Left,
            (_, _, _, true) => ResizeEdge::Right,
            _ => ResizeEdge::None,
        }
    }
}

/// 按边缩放；结果钳到最小尺寸。
pub fn resize_rect(r: &Rect, edge: ResizeEdge, dx: i32, dy: i32) -> Rect {
    let mut x = r.x;
    let mut y = r.y;
    let mut w = r.w;
    let mut h = r.h;
    match edge {
        ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft => {
            let nw = (w - dx).max(MIN_W);
            x += w - nw;
            w = nw;
        }
        ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight => {
            w = (w + dx).max(MIN_W);
        }
        _ => {}
    }
    match edge {
        ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight => {
            let nh = (h - dy).max(MIN_H);
            y += h - nh;
            h = nh;
        }
        ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight => {
            h = (h + dy).max(MIN_H);
        }
        _ => {}
    }
    Rect::new(x, y, w, h)
}

/// 越界钳制：窗口至少保留 `MIN_VISIBLE` 像素在屏幕内（否则用户抓不回来）。
pub fn clamp_to_screen(r: &Rect, screen: &Rect) -> Rect {
    let x = if r.x + MIN_VISIBLE > screen.right() {
        screen.right() - MIN_VISIBLE
    } else if r.right() - MIN_VISIBLE < screen.x {
        screen.x - r.w + MIN_VISIBLE
    } else {
        r.x
    };
    let y = if r.y + MIN_VISIBLE > screen.bottom() {
        screen.bottom() - MIN_VISIBLE
    } else if r.bottom() - MIN_VISIBLE < screen.y {
        screen.y - r.h + MIN_VISIBLE
    } else {
        r.y
    };
    Rect::new(x, y, r.w, r.h)
}

// ---------------------------------------------------------------------------
// F205 边缘贴靠/磁力
// F207 分屏布局
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapZone {
    None,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximise,
}

impl SnapZone {
    /// 依据四边与屏幕边的距离判定贴靠区（顶部触发最大化）。
    pub fn detect(r: &Rect, screen: &Rect) -> SnapZone {
        let l = (r.x - screen.x).abs();
        let t = (r.y - screen.y).abs();
        let rr = (screen.right() - r.right()).abs();
        let b = (screen.bottom() - r.bottom()).abs();
        let near_l = l <= SNAP_THRESHOLD;
        let near_t = t <= SNAP_THRESHOLD;
        let near_r = rr <= SNAP_THRESHOLD;
        let near_b = b <= SNAP_THRESHOLD;
        // 判据顺序很关键：占满一个方向的半屏优先于四角，
        // 否则「左半屏」会被误判成「左上角」（它同样贴着上边缘）。
        if near_l && near_r && (near_t || near_b) {
            SnapZone::Maximise
        } else if near_t && near_b && near_l {
            SnapZone::Left
        } else if near_t && near_b && near_r {
            SnapZone::Right
        } else if near_t && near_l {
            SnapZone::TopLeft
        } else if near_t && near_r {
            SnapZone::TopRight
        } else if near_b && near_l {
            SnapZone::BottomLeft
        } else if near_b && near_r {
            SnapZone::BottomRight
        } else if near_t {
            SnapZone::Maximise
        } else if near_l {
            SnapZone::Left
        } else if near_r {
            SnapZone::Right
        } else {
            SnapZone::None
        }
    }

    /// 贴靠后的几何（左右为半屏，四角为四分之一屏）。
    pub fn apply(self, screen: &Rect) -> Rect {
        let hw = screen.w / 2;
        let hh = screen.h / 2;
        match self {
            SnapZone::Left => Rect::new(screen.x, screen.y, hw, screen.h),
            SnapZone::Right => Rect::new(screen.x + hw, screen.y, screen.w - hw, screen.h),
            SnapZone::Top | SnapZone::Maximise => *screen,
            SnapZone::Bottom => *screen,
            SnapZone::TopLeft => Rect::new(screen.x, screen.y, hw, hh),
            SnapZone::TopRight => Rect::new(screen.x + hw, screen.y, screen.w - hw, hh),
            SnapZone::BottomLeft => Rect::new(screen.x, screen.y + hh, hw, screen.h - hh),
            SnapZone::BottomRight => Rect::new(screen.x + hw, screen.y + hh, screen.w - hw, screen.h - hh),
            SnapZone::None => *screen,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitKind {
    Halves,
    Thirds,
    Quarters,
    Main,
    Side,
}

/// 分屏布局：返回该布局下的槽位矩形。
pub fn split_layout(kind: SplitKind, screen: &Rect, out: &mut [Rect]) -> usize {
    let n = match kind {
        SplitKind::Halves => 2,
        SplitKind::Thirds => 3,
        SplitKind::Quarters => 4,
        SplitKind::Main | SplitKind::Side => 2,
    };
    if out.len() < n {
        return 0;
    }
    match kind {
        SplitKind::Halves => {
            out[0] = Rect::new(screen.x, screen.y, screen.w / 2, screen.h);
            out[1] = Rect::new(screen.x + screen.w / 2, screen.y, screen.w - screen.w / 2, screen.h);
        }
        SplitKind::Thirds => {
            let w = screen.w / 3;
            for i in 0..3 {
                out[i] = Rect::new(screen.x + w * i as i32, screen.y, w, screen.h);
            }
        }
        SplitKind::Quarters => {
            let hw = screen.w / 2;
            let hh = screen.h / 2;
            out[0] = Rect::new(screen.x, screen.y, hw, hh);
            out[1] = Rect::new(screen.x + hw, screen.y, screen.w - hw, hh);
            out[2] = Rect::new(screen.x, screen.y + hh, hw, screen.h - hh);
            out[3] = Rect::new(screen.x + hw, screen.y + hh, screen.w - hw, screen.h - hh);
        }
        SplitKind::Main | SplitKind::Side => {
            // 主侧布局：主区 62%（避免黄金分割的浮点，取 620‰）
            let main_w = (screen.w * 620) / 1000;
            out[0] = Rect::new(screen.x, screen.y, main_w, screen.h);
            out[1] = Rect::new(screen.x + main_w, screen.y, screen.w - main_w, screen.h);
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F206 最小化/最大化/恢复
// F221 窗口全屏/独占协议
// ---------------------------------------------------------------------------

/// 状态迁移：返回 (新状态, 是否改变了几何)。
pub fn apply_state(w: &mut Window, next: WinState, screen: &Rect) -> bool {
    match (w.state, next) {
        (WinState::Minimised, WinState::Normal) => {
            w.state = WinState::Normal;
            w.rect = w.saved;
            true
        }
        (_, WinState::Minimised) => {
            if w.state != WinState::Minimised {
                w.saved = w.rect;
            }
            w.state = WinState::Minimised;
            true
        }
        (_, WinState::Maximised) => {
            if w.state != WinState::Maximised {
                w.saved = w.rect;
            }
            w.state = WinState::Maximised;
            w.rect = *screen;
            true
        }
        (WinState::Maximised, WinState::Normal) => {
            w.state = WinState::Normal;
            w.rect = w.saved;
            true
        }
        (_, WinState::Fullscreen) => {
            if w.state != WinState::Fullscreen {
                w.saved = w.rect;
            }
            w.state = WinState::Fullscreen;
            w.rect = *screen;
            true
        }
        (WinState::Fullscreen, WinState::Normal) => {
            w.state = WinState::Normal;
            w.rect = w.saved;
            true
        }
        (_, WinState::Snapped) => {
            w.state = WinState::Snapped;
            true
        }
        (WinState::Snapped, WinState::Normal) => {
            w.state = WinState::Normal;
            w.rect = w.saved;
            true
        }
        (_, WinState::Normal) => {
            w.state = WinState::Normal;
            false
        }
    }
}

/// F221：独占全屏需要宿主空闲——否则拒绝（不抢占用户正在用的环境）。
pub fn fullscreen_allowed(host_load_permille: u16) -> bool {
    host_load_permille < 500
}

// ---------------------------------------------------------------------------
// F210 舞台编排
// F212 画中画
// F222 缩略图/停靠条
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageLayout {
    Focus,
    Cascade,
    Grid,
    Stack,
}

/// 舞台编排：给定屏幕与窗口数，为每个槽位产出矩形。
pub fn stage_layout(layout: StageLayout, screen: &Rect, count: usize, out: &mut [Rect]) -> usize {
    let n = count.min(out.len());
    if n == 0 {
        return 0;
    }
    match layout {
        StageLayout::Focus => {
            // 主窗口占 62%，其余叠在右侧成条
            let main_w = (screen.w * 620) / 1000;
            out[0] = Rect::new(screen.x, screen.y, main_w, screen.h);
            let rest_w = screen.w - main_w;
            let each = if n > 1 { screen.h / (n as i32 - 1) } else { 0 };
            for i in 1..n {
                out[i] = Rect::new(screen.x + main_w, screen.y + each * (i as i32 - 1), rest_w, each);
            }
        }
        StageLayout::Cascade => {
            let step = 32;
            for i in 0..n {
                let off = step * i as i32;
                out[i] = Rect::new(screen.x + off, screen.y + off, screen.w - off * 2, screen.h - off * 2);
            }
        }
        StageLayout::Grid => {
            let cols = if n <= 1 { 1 } else if n <= 4 { 2 } else { 3 };
            let rows = (n as i32 + cols - 1) / cols;
            let cw = screen.w / cols;
            let ch = screen.h / rows.max(1);
            for i in 0..n {
                let c = (i as i32) % cols;
                let r = (i as i32) / cols;
                out[i] = Rect::new(screen.x + c * cw, screen.y + r * ch, cw, ch);
            }
        }
        StageLayout::Stack => {
            for i in 0..n {
                out[i] = *screen;
            }
        }
    }
    n
}

/// F212：画中画固定在某个角，尺寸 = 屏幕短边 × permille。
pub fn pip_rect(screen: &Rect, corner: u8, scale_permille: u32, margin: i32) -> Rect {
    let size = (((if screen.w < screen.h { screen.w } else { screen.h }) as i64) * scale_permille as i64 / 1000) as i32;
    let (x, y) = match corner {
        0 => (screen.x + margin, screen.y + margin),
        1 => (screen.right() - size - margin, screen.y + margin),
        2 => (screen.x + margin, screen.bottom() - size - margin),
        _ => (screen.right() - size - margin, screen.bottom() - size - margin),
    };
    Rect::new(x, y, size, size)
}

/// F222：停靠条缩略图（宽 160，高 90，间距 8）。
pub fn thumb_rect(index: usize, dock_y: i32) -> Rect {
    Rect::new(8 + (index as i32) * (160 + 8), dock_y, 160, 90)
}

// ---------------------------------------------------------------------------
// F213 窗口透明度/材质
// F214 窗口弹性动画
// F223 窗口滚动同步
// ---------------------------------------------------------------------------

/// 材质决定不透明度下限：玻璃窗不能全不透明，否则看不出材质。
pub fn min_alpha(m: WindowMaterial) -> u8 {
    match m {
        WindowMaterial::Opaque => 255,
        WindowMaterial::Glass => 200,
        WindowMaterial::Acrylic => 220,
    }
}

pub fn effective_alpha(w: &Window) -> u8 {
    w.alpha.max(min_alpha(w.material)).min(255)
}

/// 弹性动画：拖拽越界后回弹，用统一弹簧令牌（AI-08 F177）。
pub fn elastic_step(pos: f32, vel: f32, target: f32, dt_ms: f32) -> (f32, f32) {
    let spec = crate::ui::motion::spring_for(crate::ui::motion::SpringKind::Bounce);
    crate::gfx::surface::Spring { stiffness: spec.stiffness, damping: spec.damping, mass: spec.mass }
        .step(pos, vel, target, dt_ms)
}

/// 滚动同步：从窗口按 permille 比例跟随主窗口。
pub fn scroll_sync(master_offset: i32, master_range: i32, follower_range: i32) -> i32 {
    if master_range <= 0 {
        return 0;
    }
    let t = ((master_offset as i64) * 1000) / master_range as i64;
    ((t * follower_range as i64) / 1000) as i32
}

// ---------------------------------------------------------------------------
// F216 窗口关闭/销毁取证
// F218 窗口事件总线
// F219 窗口性能预算
// F224 窗口层级调试器
// ---------------------------------------------------------------------------

pub const MAX_WIN_EVENTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinEventKind {
    Created,
    Closed,
    Raised,
    Moved,
    Resized,
    Snapped,
    Minimised,
    Restored,
}

#[derive(Clone, Copy, Debug)]
pub struct WinEvent {
    pub kind: WinEventKind,
    pub id: u16,
    pub stamp_ms: u32,
}

pub struct EventBus {
    events: [Option<WinEvent>; MAX_WIN_EVENTS],
    head: usize,
    count: usize,
    /// 已关闭窗口的计数（取证用：不因数组回收而丢失）。
    pub closed_total: u32,
}

impl EventBus {
    pub const fn new() -> EventBus {
        EventBus { events: [None; MAX_WIN_EVENTS], head: 0, count: 0, closed_total: 0 }
    }

    pub fn push(&mut self, e: WinEvent) {
        if matches!(e.kind, WinEventKind::Closed) {
            self.closed_total = self.closed_total.saturating_add(1);
        }
        self.events[self.head] = Some(e);
        self.head = (self.head + 1) % MAX_WIN_EVENTS;
        if self.count < MAX_WIN_EVENTS {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<WinEvent> {
        if i >= self.count {
            return None;
        }
        let start = (self.head + MAX_WIN_EVENTS - self.count) % MAX_WIN_EVENTS;
        self.events[(start + i) % MAX_WIN_EVENTS]
    }
}

impl Default for EventBus {
    fn default() -> Self {
        EventBus::new()
    }
}

pub const VWM_BUDGET_US: u32 = 3_000;

/// F219：窗口合成预算 3ms（渲染 4ms + 特效 6ms + 杂项）。
pub fn vwm_budget_ok(windows: usize, us_per_window: u32) -> bool {
    (windows as u64) * (us_per_window as u64) <= VWM_BUDGET_US as u64
}

/// F224：层级调试器输出 `id z state rect` 逐行文本。
pub fn render_tree(tree: &WindowTree, out: &mut [u8]) -> usize {
    let mut order = [0usize; MAX_WINDOWS];
    let n = tree.z_order(&mut order);
    let mut pos = 0usize;
    for i in 0..n {
        if let Some(w) = tree.get(order[i]) {
            push_num(out, &mut pos, w.id as usize);
            push_str(out, &mut pos, " z=");
            push_num(out, &mut pos, w.z as usize);
            push_str(out, &mut pos, " ");
            push_str(out, &mut pos, w.state.name());
            push_str(out, &mut pos, " ");
            push_num(out, &mut pos, w.rect.x as usize);
            push_str(out, &mut pos, ",");
            push_num(out, &mut pos, w.rect.y as usize);
            push_str(out, &mut pos, "\n");
        }
    }
    pos
}

fn push_str(out: &mut [u8], n: &mut usize, s: &str) {
    for &b in s.as_bytes() {
        if *n < out.len() {
            out[*n] = b;
            *n += 1;
        }
    }
}

fn push_num(out: &mut [u8], n: &mut usize, mut v: usize) {
    let neg = (v as isize) < 0;
    if v == 0 {
        push_str(out, n, "0");
        return;
    }
    if neg {
        push_str(out, n, "-");
    }
    let mut digits = [0u8; 12];
    let mut w = 0usize;
    while v > 0 && w < digits.len() {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        if *n < out.len() {
            out[*n] = digits[w];
            *n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0, 0, 1920, 1080)
    }

    #[test]
    fn f202_z_order_and_occlusion() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(0, 0, 400, 400), "a"));
        t.add(Window::new(2, Rect::new(0, 0, 200, 200), "b"));
        let mut order = [0usize; MAX_WINDOWS];
        let n = t.z_order(&mut order);
        assert_eq!(n, 2);
        assert_eq!(t.get(order[1]).unwrap().id, 2, "later window sits on top");
        let idx_of_1 = t.by_id(1).unwrap();
        assert!(t.occluded_permille(idx_of_1) > 0);
        assert!(!t.fully_occluded(idx_of_1));
        let idx2 = t.by_id(2).unwrap();
        assert_eq!(t.occluded_permille(idx2), 0);
    }

    #[test]
    fn f202_full_occlusion_skips_paint() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(0, 0, 100, 100), "a"));
        t.add(Window::new(2, Rect::new(0, 0, 100, 100), "b"));
        assert!(t.fully_occluded(t.by_id(1).unwrap()));
    }

    #[test]
    fn f203_hit_uses_topmost() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(0, 0, 100, 100), "a"));
        t.add(Window::new(2, Rect::new(50, 50, 100, 100), "b"));
        assert_eq!(t.hit(60, 60), Some(2));
        assert_eq!(t.hit(10, 10), Some(1));
        assert_eq!(t.hit(500, 500), None);
        assert_eq!(t.focus(10, 10), Some(1));
        assert!(t.get(t.by_id(1).unwrap()).unwrap().focused);
    }

    #[test]
    fn f204_resize_edges() {
        let r = Rect::new(10, 10, 400, 300);
        assert_eq!(ResizeEdge::detect(&r, 10, 50), ResizeEdge::Left);
        assert_eq!(ResizeEdge::detect(&r, 410, 50), ResizeEdge::Right);
        assert_eq!(ResizeEdge::detect(&r, 50, 50), ResizeEdge::None);
        let grown = resize_rect(&r, ResizeEdge::Right, 20, 0);
        assert_eq!(grown.w, 420);
        let shrunk = resize_rect(&r, ResizeEdge::Right, -500, 0);
        assert_eq!(shrunk.w, MIN_W, "never below the minimum");
    }

    #[test]
    fn f205_snap_zones() {
        let s = screen();
        assert_eq!(SnapZone::detect(&Rect::new(0, 0, 960, 1080), &s), SnapZone::Left);
        assert_eq!(SnapZone::detect(&Rect::new(960, 0, 960, 1080), &s), SnapZone::Right);
        assert_eq!(SnapZone::detect(&Rect::new(0, 0, 1920, 1080), &s), SnapZone::Maximise);
        assert_eq!(SnapZone::detect(&Rect::new(500, 500, 300, 200), &s), SnapZone::None);
        assert_eq!(SnapZone::Left.apply(&s).w, 960);
    }

    #[test]
    fn f206_state_transitions() {
        let mut w = Window::new(1, Rect::new(10, 10, 100, 100), "a");
        let s = screen();
        assert!(apply_state(&mut w, WinState::Maximised, &s));
        assert_eq!(w.rect, s);
        assert!(apply_state(&mut w, WinState::Normal, &s));
        assert_eq!(w.rect, Rect::new(10, 10, 100, 100));
        assert!(apply_state(&mut w, WinState::Minimised, &s));
        assert!(!w.live());
    }

    #[test]
    fn f207_split_layouts() {
        let s = screen();
        let mut out = [Rect::new(0, 0, 0, 0); 4];
        assert_eq!(split_layout(SplitKind::Halves, &s, &mut out), 2);
        assert_eq!(out[0].w + out[1].w, 1920);
        assert_eq!(split_layout(SplitKind::Quarters, &s, &mut out), 4);
        assert_eq!(split_layout(SplitKind::Thirds, &s, &mut [Rect::new(0, 0, 0, 0); 2]), 0);
    }

    #[test]
    fn f208_window_groups() {
        let mut t = WindowTree::new();
        let mut a = Window::new(1, Rect::new(0, 0, 10, 10), "a");
        a.group = 3;
        let mut b = Window::new(2, Rect::new(0, 0, 10, 10), "b");
        b.group = 3;
        t.add(a);
        t.add(b);
        let mut ids = [0u16; 8];
        assert_eq!(t.group_ids(3, &mut ids), 2);
        assert_eq!(t.group_ids(0, &mut ids), 0, "group 0 means ungrouped");
    }

    #[test]
    fn f209_alt_tab_cycles_mru() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(0, 0, 10, 10), "a"));
        t.add(Window::new(2, Rect::new(0, 0, 10, 10), "b"));
        t.add(Window::new(3, Rect::new(0, 0, 10, 10), "c"));
        // 当前 MRU 头是 3，切一次应回到 2
        assert_eq!(t.alt_tab(1), Some(2));
        assert_eq!(t.mru()[0], 2);
    }

    #[test]
    fn f210_stage_layouts() {
        let s = screen();
        let mut out = [Rect::new(0, 0, 0, 0); 4];
        assert_eq!(stage_layout(StageLayout::Grid, &s, 4, &mut out), 4);
        assert_eq!(out[0].x, 0);
        assert_eq!(out[1].x, 960);
        assert_eq!(stage_layout(StageLayout::Cascade, &s, 3, &mut out), 3);
        assert!(out[2].x > out[0].x);
    }

    #[test]
    fn f211_snapshot_roundtrip() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(5, 5, 100, 100), "a"));
        let mut snap = [WindowSnapshot { id: 0, rect: Rect::new(0, 0, 0, 0), state: WinState::Normal, z: 0 }; MAX_WINDOWS];
        let n = t.snapshot(&mut snap);
        assert_eq!(n, 1);
        if let Some(w) = t.get_mut(0) {
            w.rect = Rect::new(500, 500, 50, 50);
        }
        assert_eq!(t.restore(&snap[..n]), 1);
        assert_eq!(t.get(0).unwrap().rect, Rect::new(5, 5, 100, 100));
    }

    #[test]
    fn f212_pip_geometry() {
        let s = screen();
        let r = pip_rect(&s, 3, 200, 24);
        assert_eq!(r.w, r.h);
        assert_eq!(r.x + r.w, s.right() - 24);
        assert_eq!(r.y + r.h, s.bottom() - 24);
    }

    #[test]
    fn f213_material_alpha_floor() {
        let mut w = Window::new(1, Rect::new(0, 0, 10, 10), "a");
        w.material = WindowMaterial::Glass;
        w.alpha = 20;
        assert!(effective_alpha(&w) >= min_alpha(WindowMaterial::Glass));
        w.material = WindowMaterial::Opaque;
        assert_eq!(effective_alpha(&w), 255);
    }

    #[test]
    fn f214_elastic_settles() {
        let mut p = 100.0f32;
        let mut v = 0.0f32;
        for _ in 0..400 {
            let (np, nv) = elastic_step(p, v, 0.0, 16.0);
            p = np;
            v = nv;
        }
        assert!((p - 0.0).abs() < 1.0, "bounce spring returns to target");
    }

    #[test]
    fn f215_instance_limits() {
        let mut t = WindowTree::new();
        t.add(Window::new(1, Rect::new(0, 0, 10, 10), "notes"));
        assert!(t.can_open("notes", 2));
        t.add(Window::new(2, Rect::new(0, 0, 10, 10), "notes"));
        assert!(!t.can_open("notes", 2));
        assert_eq!(t.instances_of("notes"), 2);
    }

    #[test]
    fn f216_close_forensics_survives_ring() {
        let mut bus = EventBus::new();
        for i in 0..MAX_WIN_EVENTS + 5 {
            bus.push(WinEvent { kind: WinEventKind::Closed, id: i as u16, stamp_ms: i as u32 });
        }
        assert_eq!(bus.closed_total as usize, MAX_WIN_EVENTS + 5);
        assert_eq!(bus.len(), MAX_WIN_EVENTS);
    }

    #[test]
    fn f220_clamp_keeps_window_reachable() {
        let s = screen();
        let off = Rect::new(5000, 5000, 400, 300);
        let c = clamp_to_screen(&off, &s);
        assert!(c.x + MIN_VISIBLE <= s.right());
        assert!(c.y + MIN_VISIBLE <= s.bottom());
        let onscreen = Rect::new(100, 100, 400, 300);
        assert_eq!(clamp_to_screen(&onscreen, &s), onscreen, "no needless movement");
    }

    #[test]
    fn f221_fullscreen_gate() {
        assert!(fullscreen_allowed(100));
        assert!(!fullscreen_allowed(900));
    }

    #[test]
    fn f222_thumb_strip() {
        assert_eq!(thumb_rect(0, 900).x, 8);
        assert_eq!(thumb_rect(1, 900).x, 8 + 168);
        assert_eq!(thumb_rect(0, 900).h, 90);
    }

    #[test]
    fn f223_scroll_sync_proportional() {
        assert_eq!(scroll_sync(500, 1000, 2000), 1000);
        assert_eq!(scroll_sync(0, 0, 1000), 0);
    }

    #[test]
    fn f224_debugger_renders_tree() {
        let mut t = WindowTree::new();
        t.add(Window::new(7, Rect::new(1, 2, 3, 4), "a"));
        let mut out = [0u8; 256];
        let n = render_tree(&t, &mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("7 z=1 normal 1,2\n"));
    }

    #[test]
    fn f219_budget_gate() {
        assert!(vwm_budget_ok(10, 100));
        assert!(!vwm_budget_ok(40, 100));
    }
}
