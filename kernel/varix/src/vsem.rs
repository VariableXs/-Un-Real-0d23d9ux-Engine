//! AI-17 Variable 语义移植域（F401~F425）。
//!
//! The Variable/VWM desktop semantics re-expressed inside the kernel: the
//! icon grid, the tiling window tree, the three-pane layout, the dual key
//! tables (host + guest planes), the compute-exclusivity weights, the
//! wallpaper/particle layer, the seven-layer boot light show, the 128px icon
//! pipeline, trash/tags/launcher/settings, layout snapshots, the privacy
//! dashboard and the zero-telemetry audit.
//!
//! 画质无损 and 手感无损: every ratio is computed in integer permille so the
//! desktop lays out identically on every resolution.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F401 — 桌面语义（图标网格）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopGrid {
    pub cols: u8,
    pub cell_w: u32,
    pub cell_h: u32,
    pub origin_x: u32,
    pub origin_y: u32,
}

impl DesktopGrid {
    pub fn cell_of(&self, index: u32) -> (u32, u32) {
        let cols = self.cols.max(1) as u32;
        let col = index % cols;
        let row = index / cols;
        (
            self.origin_x + col * self.cell_w,
            self.origin_y + row * self.cell_h,
        )
    }

    /// Snap a point to the nearest cell origin (icons never sit off-grid).
    pub fn snap(&self, x: u32, y: u32) -> (u32, u32) {
        let col = x.saturating_sub(self.origin_x) / self.cell_w.max(1);
        let row = y.saturating_sub(self.origin_y) / self.cell_h.max(1);
        (
            self.origin_x + col * self.cell_w,
            self.origin_y + row * self.cell_h,
        )
    }

    /// Which grid slot a point falls in.
    pub fn index_at(&self, x: u32, y: u32) -> Option<u32> {
        if x < self.origin_x || y < self.origin_y {
            return None;
        }
        let col = (x - self.origin_x) / self.cell_w.max(1);
        let row = (y - self.origin_y) / self.cell_h.max(1);
        if col >= self.cols.max(1) as u32 {
            return None;
        }
        Some(row * self.cols.max(1) as u32 + col)
    }
}

// ---------------------------------------------------------------------------
// F402 — VWM 窗口树平移
// ---------------------------------------------------------------------------

pub const MAX_VWM_NODES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VwmKind {
    Leaf,
    HSplit,
    VSplit,
}

#[derive(Clone, Copy, Debug)]
pub struct VwmNode {
    pub id: u16,
    pub kind: VwmKind,
    /// Split position in permille (HSplit = left share, VSplit = top share).
    pub ratio_permille: u16,
    pub child_a: u16,
    pub child_b: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VwmRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct VwmTree {
    nodes: [Option<VwmNode>; MAX_VWM_NODES],
    count: usize,
}

impl VwmTree {
    pub const fn new() -> VwmTree {
        VwmTree { nodes: [None; MAX_VWM_NODES], count: 0 }
    }

    pub fn add(&mut self, node: VwmNode) -> bool {
        if self.count >= MAX_VWM_NODES {
            return false;
        }
        self.nodes[self.count] = Some(node);
        self.count += 1;
        true
    }

    pub fn find(&self, id: u16) -> Option<VwmNode> {
        (0..self.count).find_map(|i| match self.nodes[i] {
            Some(n) if n.id == id => Some(n),
            _ => None,
        })
    }

    /// Tiling layout: recursively split `rect` for `root`.
    /// Returns the number of leaf rects written to `out` as (id, rect) pairs.
    pub fn layout(
        &self,
        root: u16,
        rect: VwmRect,
        out: &mut [(u16, VwmRect)],
        depth: usize,
    ) -> usize {
        if depth > MAX_VWM_NODES {
            return 0;
        }
        let node = match self.find(root) {
            Some(n) => n,
            None => return 0,
        };
        match node.kind {
            VwmKind::Leaf => {
                if out.is_empty() {
                    return 0;
                }
                out[0] = (node.id, rect);
                1
            }
            VwmKind::HSplit => {
                let ratio = node.ratio_permille.clamp(50, 950) as u32;
                let left = (rect.w * ratio / 1000).max(1);
                let a = VwmRect { x: rect.x, y: rect.y, w: left, h: rect.h };
                let b = VwmRect {
                    x: rect.x + left as i32,
                    y: rect.y,
                    w: rect.w.saturating_sub(left),
                    h: rect.h,
                };
                self.layout_pair(node.child_a, node.child_b, a, b, out, depth)
            }
            VwmKind::VSplit => {
                let ratio = node.ratio_permille.clamp(50, 950) as u32;
                let top = (rect.h * ratio / 1000).max(1);
                let a = VwmRect { x: rect.x, y: rect.y, w: rect.w, h: top };
                let b = VwmRect {
                    x: rect.x,
                    y: rect.y + top as i32,
                    w: rect.w,
                    h: rect.h.saturating_sub(top),
                };
                self.layout_pair(node.child_a, node.child_b, a, b, out, depth)
            }
        }
    }

    fn layout_pair(
        &self,
        a: u16,
        b: u16,
        rect_a: VwmRect,
        rect_b: VwmRect,
        out: &mut [(u16, VwmRect)],
        depth: usize,
    ) -> usize {
        let mut buffer_a = [(0u16, VwmRect { x: 0, y: 0, w: 0, h: 0 }); MAX_VWM_NODES];
        let mut buffer_b = [(0u16, VwmRect { x: 0, y: 0, w: 0, h: 0 }); MAX_VWM_NODES];
        let na = self.layout(a, rect_a, &mut buffer_a, depth + 1);
        let nb = self.layout(b, rect_b, &mut buffer_b, depth + 1);
        let mut n = 0usize;
        for i in 0..na {
            if n < out.len() {
                out[n] = buffer_a[i];
                n += 1;
            }
        }
        for i in 0..nb {
            if n < out.len() {
                out[n] = buffer_b[i];
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F403 — 三区布局
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreePane {
    /// Library / preview / inspector shares in permille (must total 1000).
    pub ratios: [u16; 3],
    pub min_widths: [u32; 3],
    pub gap: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneLayout {
    pub widths: [u32; 3],
    pub collapsed: [bool; 3],
}

impl ThreePane {
    pub fn compute(&self, total: u32) -> PaneLayout {
        let gaps = self.gap * 2;
        let usable = total.saturating_sub(gaps);
        let mut widths = [0u32; 3];
        let mut collapsed = [false; 3];
        let mut remaining = usable;
        for i in 0..3 {
            let share = remaining * self.ratios[i] as u32 / 1000;
            let w = share.max(self.min_widths[i].min(usable));
            widths[i] = w;
            remaining = remaining.saturating_sub(w);
            collapsed[i] = w <= self.min_widths[i] && usable < self.min_widths.iter().sum::<u32>();
            if collapsed[i] {
                widths[i] = 0;
            }
        }
        PaneLayout { widths, collapsed }
    }
}

// ---------------------------------------------------------------------------
// F404 — 任务栏语义（常驻栏行为）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskbarSemantics {
    pub auto_hide: bool,
    pub always_on_top: bool,
    pub height_px: u32,
    pub reveal_zone_px: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarVisibility {
    Shown,
    Hidden,
    Revealed,
}

impl TaskbarSemantics {
    /// Pointer at `y` from the bottom edge; `pinned_open` = a menu is open.
    pub fn visibility(&self, pointer_from_bottom: u32, pinned_open: bool) -> TaskbarVisibility {
        if !self.auto_hide {
            return TaskbarVisibility::Shown;
        }
        if pinned_open {
            return TaskbarVisibility::Revealed;
        }
        if pointer_from_bottom <= self.reveal_zone_px {
            TaskbarVisibility::Revealed
        } else {
            TaskbarVisibility::Hidden
        }
    }
}

// ---------------------------------------------------------------------------
// F405 — 双键位双表落地
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plane {
    Host,
    Guest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyTable {
    pub name: &'static str,
    /// Scancode → character (0 = unmapped).
    pub map: [u8; 64],
}

impl KeyTable {
    pub const fn new(name: &'static str) -> KeyTable {
        KeyTable { name, map: [0; 64] }
    }

    pub fn bind(&mut self, scancode: u8, ch: u8) -> bool {
        if scancode as usize >= self.map.len() || ch == 0 {
            return false;
        }
        self.map[scancode as usize] = ch;
        true
    }

    pub fn lookup(&self, scancode: u8) -> Option<u8> {
        match self.map.get(scancode as usize) {
            Some(0) | None => None,
            Some(c) => Some(*c),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DualKeymap {
    pub host: KeyTable,
    pub guest: KeyTable,
    pub active: Plane,
    /// Hotkey that flips planes (F418 学习模式 shows this).
    pub switch_scancode: u8,
}

impl DualKeymap {
    pub const fn new() -> DualKeymap {
        DualKeymap {
            host: KeyTable::new("varix"),
            guest: KeyTable::new("guest"),
            active: Plane::Host,
            switch_scancode: 0x38, // left Alt
        }
    }

    /// Focus decides the table — the hand-feel core of the dual plane.
    pub fn on_focus(&mut self, plane: Plane) {
        self.active = plane;
    }

    pub fn lookup(&self, scancode: u8) -> Option<u8> {
        if scancode == self.switch_scancode {
            return None; // the switch key never types a character
        }
        match self.active {
            Plane::Host => self.host.lookup(scancode),
            Plane::Guest => self.guest.lookup(scancode),
        }
    }

    pub fn toggle(&mut self) {
        self.active = match self.active {
            Plane::Host => Plane::Guest,
            Plane::Guest => Plane::Host,
        };
    }
}

// ---------------------------------------------------------------------------
// F406 — 算力独占策略落地
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExclusivePolicy {
    /// Foreground weight, permille of the background weight's counterpart.
    pub foreground_permille: u16,
    pub background_permille: u16,
    pub exclusive: bool,
}

/// Scheduling weight for a foreground/background task under the policy.
pub fn compute_weight(policy: ExclusivePolicy, foreground: bool) -> u16 {
    if policy.exclusive && foreground {
        return 1000;
    }
    if foreground {
        policy.foreground_permille
    } else {
        policy.background_permille
    }
}

/// Exclusive mode is refused when the host is already saturated (F340 link).
pub fn exclusive_allowed(host_load_permille: u16) -> bool {
    host_load_permille < 900
}

// ---------------------------------------------------------------------------
// F407/F408 — 壁纸引擎与动态壁纸层
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperKind {
    Static,
    KenBurns,
    Particles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallpaperLayer {
    pub kind: WallpaperKind,
    pub opacity_permille: u16,
    pub z: u8,
    /// Motion is dropped when reduce-motion is on (F423).
    pub motion: bool,
}

/// Cross-fade weight between the outgoing and incoming wallpaper.
pub fn crossfade_alpha(progress_permille: u16) -> (u16, u16) {
    let p = progress_permille.min(1000);
    (1000 - p, p)
}

/// Ken Burns crop rect: a slow zoom with no up-scaling (画质无损).
pub fn ken_burns_rect(width: u32, height: u32, progress_permille: u16) -> (u32, u32, u32, u32) {
    let p = progress_permille.min(1000) as u64;
    // Zoom 100% → 108% over the cycle.
    let zoom = 1000 + (80 * p / 1000) as u32;
    let w = (width as u64 * 1000 / zoom as u64) as u32;
    let h = (height as u64 * 1000 / zoom as u64) as u32;
    let x = (width - w) * p as u32 / 1000;
    let y = (height - h) * p as u32 / 2000;
    (x, y, w.max(1), h.max(1))
}

/// Deterministic particle field (LCG) — identical on every boot and machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleField {
    pub capacity: usize,
    pub seed: u64,
    pub drift_permille: u16,
}

impl ParticleField {
    pub const fn new(capacity: usize, seed: u64) -> ParticleField {
        ParticleField { capacity, seed, drift_permille: 100 }
    }

    /// Position (x, y) in 1/1000 of the screen for particle `index` at `tick`.
    pub fn position(&self, index: usize, tick: u64, width: u32, height: u32) -> (u32, u32) {
        let mut state = self
            .seed
            .wrapping_add((index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .wrapping_add(tick);
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let x = (state % width.max(1) as u64) as u32;
        let y = ((state >> 32) % height.max(1) as u64) as u32;
        (x, y)
    }

    /// Particles that fit inside the frame budget: fewer when the device is
    /// slow, more when it is idle.
    pub fn active_count(&self, budget_us: u32) -> usize {
        if budget_us < 2_000 {
            self.capacity / 4
        } else if budget_us < 8_000 {
            self.capacity / 2
        } else {
            self.capacity
        }
    }
}

// ---------------------------------------------------------------------------
// F409 — 启动动画七层光效
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightLayer {
    Aurora,
    Dust,
    Meteor,
    Crt,
    Bloom,
    Vignette,
    Logo,
}

pub const BOOT_LAYERS: [LightLayer; 7] = [
    LightLayer::Aurora,
    LightLayer::Dust,
    LightLayer::Meteor,
    LightLayer::Crt,
    LightLayer::Bloom,
    LightLayer::Vignette,
    LightLayer::Logo,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootAnimation {
    pub total_ms: u32,
    pub reduce_motion: bool,
}

impl BootAnimation {
    pub const fn new() -> BootAnimation {
        BootAnimation { total_ms: 1400, reduce_motion: false }
    }

    pub fn layer_count(&self) -> usize {
        if self.reduce_motion {
            1 // a single static emblem
        } else {
            BOOT_LAYERS.len()
        }
    }

    /// Layer opacity in permille at `t_ms`.
    pub fn layer_opacity(&self, layer: LightLayer, t_ms: u32) -> u16 {
        if self.reduce_motion {
            return 1000;
        }
        let idx = BOOT_LAYERS.iter().position(|l| *l == layer).unwrap_or(0) as u32;
        let stagger = self.total_ms / (BOOT_LAYERS.len() as u32 + 2);
        let local = t_ms.saturating_sub(idx * stagger);
        let fade = (self.total_ms * 2 / 3).max(1);
        if local >= fade {
            1000
        } else {
            (local * 1000 / fade) as u16
        }
    }

    pub fn progress_permille(&self, t_ms: u32) -> u16 {
        if self.total_ms == 0 {
            return 1000;
        }
        ((t_ms.min(self.total_ms) as u64 * 1000 / self.total_ms as u64) as u16).min(1000)
    }
}

// ---------------------------------------------------------------------------
// F410 — 图标 128px 管线平移
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconStage {
    Vector,
    Rasterize128,
    Downscale64,
    Downscale32,
    Downscale16,
}

/// Pipeline for a requested rung: always rasterize at 128 then downscale —
/// that keeps small sizes crisp instead of blurry.
pub fn icon_pipeline(requested: u32, out: &mut [IconStage]) -> usize {
    let stages = if requested >= 128 {
        [IconStage::Vector, IconStage::Rasterize128, IconStage::Rasterize128]
    } else if requested >= 64 {
        [IconStage::Vector, IconStage::Rasterize128, IconStage::Downscale64]
    } else if requested >= 32 {
        [IconStage::Vector, IconStage::Rasterize128, IconStage::Downscale32]
    } else {
        [IconStage::Vector, IconStage::Rasterize128, IconStage::Downscale16]
    };
    let n = stages.len().min(out.len());
    out[..n].copy_from_slice(&stages[..n]);
    n
}

/// The Variable icon contract: square, integer sizes, 128px master.
pub fn icon_contract_ok(size: u32, master: u32) -> bool {
    master == 128 && size >= 16 && size <= 256 && size % 8 == 0
}

// ---------------------------------------------------------------------------
// F411/F412/F413 — 回收站、文件标签、快捷启动器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrashItem {
    pub name: &'static str,
    pub original_dir: &'static str,
    pub deleted_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trash {
    pub count: u32,
    pub bytes: u64,
    pub retention_days: u32,
}

/// Restore target: collisions get " (restored)" — the same rule every time,
/// so the UI can preview the name before the user confirms.
pub fn restore_target(_name: &str, exists: bool) -> &'static str {
    if exists {
        " (restored)"
    } else {
        ""
    }
}

/// Items older than the retention window are purge candidates.
pub fn purge_candidates(items: &[TrashItem], now_ms: u64, retention_days: u32) -> usize {
    let window = retention_days as u64 * 86_400_000;
    items.iter().filter(|i| now_ms.saturating_sub(i.deleted_ms) > window).count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: &'static str,
    pub color: u32,
}

pub const MAX_TAGS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct TagIndex {
    tags: [Option<Tag>; MAX_TAGS],
    counts: [u32; MAX_TAGS],
    count: usize,
}

impl TagIndex {
    pub const fn new() -> TagIndex {
        TagIndex { tags: [None; MAX_TAGS], counts: [0; MAX_TAGS], count: 0 }
    }

    pub fn add_tag(&mut self, tag: Tag) -> bool {
        if self.count >= MAX_TAGS || self.find(tag.name).is_some() {
            return false;
        }
        self.tags[self.count] = Some(tag);
        self.count += 1;
        true
    }

    pub fn find(&self, name: &str) -> Option<Tag> {
        (0..self.count).find_map(|i| match self.tags[i] {
            Some(t) if t.name == name => Some(t),
            _ => None,
        })
    }

    pub fn assign(&mut self, name: &str) -> bool {
        for i in 0..self.count {
            if self.tags[i].map(|t| t.name == name).unwrap_or(false) {
                self.counts[i] += 1;
                return true;
            }
        }
        false
    }

    pub fn count_of(&self, name: &str) -> u32 {
        (0..self.count)
            .find(|i| self.tags[*i].map(|t| t.name == name).unwrap_or(false))
            .map(|i| self.counts[i])
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LauncherAlias {
    pub alias: &'static str,
    pub target: &'static str,
}

pub const MAX_ALIASES: usize = 8;

/// Alias lookup: exact alias first, then prefix match (typing "ch" finds
/// "chrome" without a query engine).
pub fn alias_resolve(aliases: &[LauncherAlias], query: &str) -> Option<&'static str> {
    if query.is_empty() {
        return None;
    }
    for a in aliases {
        if a.alias.eq_ignore_ascii_case(query) {
            return Some(a.target);
        }
    }
    for a in aliases {
        if a.alias.len() >= query.len()
            && a.alias[..query.len()].eq_ignore_ascii_case(query)
        {
            return Some(a.target);
        }
    }
    for a in aliases {
        if a.target.len() >= query.len()
            && a.target[..query.len()].eq_ignore_ascii_case(query)
        {
            return Some(a.target);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F414/F415 — 通知路由与设置中心
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyDestination {
    Banner,
    Center,
    Silent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotifyRule {
    pub app: &'static str,
    pub mute: bool,
    /// Apps allowed to banner (others only reach the centre).
    pub allow_banner: bool,
}

pub fn route_notification(
    app: &str,
    urgency: u8,
    rules: &[NotifyRule],
    dnd: bool,
) -> NotifyDestination {
    for r in rules {
        if r.app == app {
            if r.mute {
                return NotifyDestination::Silent;
            }
            if !r.allow_banner {
                return NotifyDestination::Center;
            }
        }
    }
    if dnd && urgency < 3 {
        return NotifyDestination::Center;
    }
    if urgency >= 2 {
        NotifyDestination::Banner
    } else {
        NotifyDestination::Center
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsCategory {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static str,
    pub depth: u8,
}

/// Keyword search across category titles and keyword lists.
pub fn settings_search(
    cats: &[SettingsCategory],
    query: &str,
    out: &mut [usize],
) -> usize {
    if query.is_empty() {
        return 0;
    }
    let mut n = 0usize;
    for (i, c) in cats.iter().enumerate() {
        if n >= out.len() {
            break;
        }
        if contains_ignore_case(c.title, query) || contains_ignore_case(c.keywords, query) {
            out[n] = i;
            n += 1;
        }
    }
    n
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let h: &[u8] = haystack.as_bytes();
    let n: &[u8] = needle.as_bytes();
    if n.len() > h.len() {
        return false;
    }
    for start in 0..=(h.len() - n.len()) {
        if h[start..start + n.len()]
            .iter()
            .zip(n.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// F416/F417 — 布局快照与窗口隐藏语义
// ---------------------------------------------------------------------------

pub const LAYOUT_SNAPSHOT_BYTES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutSnapshot {
    pub revision: u32,
    pub window_count: u8,
    pub grid_cols: u8,
    pub theme_hash: u32,
    pub scene: u16,
}

impl LayoutSnapshot {
    pub fn encode(self) -> [u8; LAYOUT_SNAPSHOT_BYTES] {
        let mut out = [0u8; LAYOUT_SNAPSHOT_BYTES];
        out[0..4].copy_from_slice(b"VLAY");
        out[4..8].copy_from_slice(&self.revision.to_le_bytes());
        out[8] = self.window_count;
        out[9] = self.grid_cols;
        out[10..14].copy_from_slice(&self.theme_hash.to_le_bytes());
        out[14..16].copy_from_slice(&self.scene.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<LayoutSnapshot> {
        if bytes.len() < LAYOUT_SNAPSHOT_BYTES || &bytes[0..4] != b"VLAY" {
            return None;
        }
        Some(LayoutSnapshot {
            revision: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            window_count: bytes[8],
            grid_cols: bytes[9],
            theme_hash: u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
            scene: u16::from_le_bytes([bytes[14], bytes[15]]),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HiddenWindow {
    pub id: u16,
    pub hidden: bool,
    pub still_running: bool,
}

/// Hidden windows keep running but leave the shell surfaces (F417).
pub fn in_shell_surfaces(hidden: bool) -> bool {
    !hidden
}

pub fn hidden_but_alive(win: HiddenWindow) -> bool {
    win.hidden && win.still_running
}

// ---------------------------------------------------------------------------
// F418 — 快捷键学习模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintLevel {
    Beginner,
    Intermediate,
    Expert,
}

#[derive(Clone, Copy, Debug)]
pub struct ShortcutCoach {
    pub level: HintLevel,
    /// Successful uses per shortcut, index = shortcut id.
    pub usage: [u16; 8],
    pub sessions: u16,
}

impl ShortcutCoach {
    pub const fn new() -> ShortcutCoach {
        ShortcutCoach { level: HintLevel::Beginner, usage: [0; 8], sessions: 0 }
    }

    pub fn note_use(&mut self, id: usize) {
        if id < self.usage.len() {
            self.usage[id] = self.usage[id].saturating_add(1);
        }
        self.recompute_level();
    }

    fn recompute_level(&mut self) {
        let mastered = self.usage.iter().filter(|u| **u >= 5).count();
        self.level = if mastered >= 6 {
            HintLevel::Expert
        } else if mastered >= 2 {
            HintLevel::Intermediate
        } else {
            HintLevel::Beginner
        };
    }

    /// How many tips to surface at the current level (fewer as you master it).
    pub fn tips_to_show(&self) -> usize {
        match self.level {
            HintLevel::Beginner => 3,
            HintLevel::Intermediate => 2,
            HintLevel::Expert => 1,
        }
    }

    /// The next shortcut the coach should teach (least used).
    pub fn next_lesson(&self) -> usize {
        let mut best = 0usize;
        for i in 1..self.usage.len() {
            if self.usage[i] < self.usage[best] {
                best = i;
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F419/F420 — 场景切换与主题跟随壁纸
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    pub name: &'static str,
    /// Bitmask of windows belonging to the scene.
    pub windows: u32,
    pub wallpaper: u16,
}

/// Windows that must be hidden/shown when moving between scenes.
pub fn scene_diff(from: Scene, to: Scene) -> (u32, u32) {
    (from.windows & !to.windows, to.windows & !from.windows)
}

/// Dominant-colour extraction for the "theme follows wallpaper" rule.
/// 4-bit-per-channel quantisation, then the most frequent bucket wins.
pub fn dominant_color(pixels: &[u32]) -> Option<u32> {
    if pixels.is_empty() {
        return None;
    }
    let mut buckets = [0u32; 16];
    for p in pixels {
        let idx = (((p >> 20) & 0xF) ^ ((p >> 12) & 0xF) ^ ((p >> 4) & 0xF)) as usize & 0xF;
        buckets[idx] += 1;
    }
    let mut best = 0usize;
    for i in 1..16 {
        if buckets[i] > buckets[best] {
            best = i;
        }
    }
    // Average the bucket's members to get a usable accent.
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut n = 0u64;
    for p in pixels {
        let idx = (((p >> 20) & 0xF) ^ ((p >> 12) & 0xF) ^ ((p >> 4) & 0xF)) as usize & 0xF;
        if idx == best {
            r += ((p >> 16) & 0xFF) as u64;
            g += ((p >> 8) & 0xFF) as u64;
            b += (p & 0xFF) as u64;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    Some((((r / n) as u32) << 16) | (((g / n) as u32) << 8) | ((b / n) as u32))
}

/// Readability guard: dark accents get the light foreground and vice versa.
pub fn foreground_for(bg: u32) -> u32 {
    let luma = ((bg >> 16) & 0xFF) * 299 + ((bg >> 8) & 0xFF) * 587 + (bg & 0xFF) * 114;
    if luma > 128 * 1000 {
        0x111111
    } else {
        0xFFFFFF
    }
}

// ---------------------------------------------------------------------------
// F421/F422/F423/F424 — 隐私仪表、零遥测、reduce-motion、性能预算
// ---------------------------------------------------------------------------

pub const MAX_PRIVACY_RECORDS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyRecord {
    pub app: &'static str,
    pub capability: &'static str,
    pub stamp_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct PrivacyDashboard {
    records: [Option<PrivacyRecord>; MAX_PRIVACY_RECORDS],
    count: usize,
}

impl PrivacyDashboard {
    pub const fn new() -> PrivacyDashboard {
        PrivacyDashboard { records: [None; MAX_PRIVACY_RECORDS], count: 0 }
    }

    pub fn record(&mut self, record: PrivacyRecord) -> bool {
        if self.count >= MAX_PRIVACY_RECORDS {
            return false;
        }
        self.records[self.count] = Some(record);
        self.count += 1;
        true
    }

    pub fn uses_of(&self, app: &str, capability: &str) -> usize {
        (0..self.count)
            .filter(|i| {
                self.records[*i]
                    .map(|r| r.app == app && r.capability == capability)
                    .unwrap_or(false)
            })
            .count()
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// Zero-telemetry audit: any outbound endpoint is a hard failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryAudit {
    pub outbound_endpoints: usize,
    pub local_log_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryVerdict {
    Clean,
    Violation(usize),
}

pub fn audit_telemetry(audit: TelemetryAudit) -> TelemetryVerdict {
    if audit.outbound_endpoints == 0 && audit.local_log_only {
        TelemetryVerdict::Clean
    } else {
        TelemetryVerdict::Violation(audit.outbound_endpoints)
    }
}

/// reduce-motion propagation: every animated layer must observe this.
pub fn apply_reduce_motion(reduce: bool, tokens: &mut crate::ui::MotionTokens, layers: &mut [WallpaperLayer]) {
    if !reduce {
        return;
    }
    tokens.reduce_motion = true;
    for layer in layers.iter_mut() {
        layer.motion = false;
        if layer.kind == WallpaperKind::Particles {
            layer.opacity_permille = layer.opacity_permille.min(600);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticBudget {
    pub per_frame_us: u32,
    pub spent_us: u32,
    pub animating_layers: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticVerdict {
    Within,
    Over,
    /// Over budget while animating: drop one animated layer (F423 fallback).
    DropLayer,
}

pub fn semantic_frame_verdict(budget: SemanticBudget, reduce_motion: bool) -> SemanticVerdict {
    let cap = if reduce_motion { budget.per_frame_us / 2 } else { budget.per_frame_us };
    if budget.spent_us <= cap {
        SemanticVerdict::Within
    } else if budget.animating_layers > 1 {
        SemanticVerdict::DropLayer
    } else {
        SemanticVerdict::Over
    }
}

// ---------------------------------------------------------------------------
// F425 — 语义移植自检
// ---------------------------------------------------------------------------

pub fn run_vsem_checks() -> CheckSet {
    let mut set = CheckSet::new("vsem");

    let grid = DesktopGrid { cols: 6, cell_w: 96, cell_h: 108, origin_x: 24, origin_y: 24 };
    set.add(
        "F401 desktop grid",
        grid.cell_of(0) == (24, 24)
            && grid.cell_of(6) == (24, 132)
            && grid.cell_of(7) == (120, 132)
            && grid.snap(100, 100) == (24, 24)
            && grid.index_at(25, 26) == Some(0)
            && grid.index_at(0, 0).is_none(),
        "icon grid",
    );

    let mut vwm = VwmTree::new();
    vwm.add(VwmNode { id: 1, kind: VwmKind::HSplit, ratio_permille: 500, child_a: 2, child_b: 3 });
    vwm.add(VwmNode { id: 2, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
    vwm.add(VwmNode { id: 3, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
    let mut rects = [(0u16, VwmRect { x: 0, y: 0, w: 0, h: 0 }); MAX_VWM_NODES];
    let n = vwm.layout(
        1,
        VwmRect { x: 0, y: 0, w: 1000, h: 600 },
        &mut rects,
        0,
    );
    set.add(
        "F402 vwm tiling",
        n == 2
            && rects[0] == (2, VwmRect { x: 0, y: 0, w: 500, h: 600 })
            && rects[1].1.x == 500
            && rects[1].1.w == 500,
        "tiling layout",
    );
    set.add("F402 leaf missing", vwm.layout(99, VwmRect { x: 0, y: 0, w: 10, h: 10 }, &mut rects, 0) == 0, "no node");

    let panes = ThreePane { ratios: [400, 400, 200], min_widths: [200, 300, 200], gap: 8 };
    let layout = panes.compute(1200);
    let collapsed = panes.compute(300);
    set.add(
        "F403 three pane",
        layout.widths[0] + layout.widths[1] + layout.widths[2] <= 1200 - 16
            && layout.widths[0] >= 200
            && collapsed.collapsed.iter().any(|c| *c),
        "ratios + collapse",
    );

    let bar = TaskbarSemantics { auto_hide: true, always_on_top: true, height_px: 44, reveal_zone_px: 3 };
    set.add(
        "F404 taskbar semantics",
        bar.visibility(10, false) == TaskbarVisibility::Hidden
            && bar.visibility(1, false) == TaskbarVisibility::Revealed
            && bar.visibility(10, true) == TaskbarVisibility::Revealed
            && TaskbarSemantics { auto_hide: false, ..bar }.visibility(50, false)
                == TaskbarVisibility::Shown,
        "auto hide",
    );

    let mut dual = DualKeymap::new();
    let mut host = KeyTable::new("varix");
    let mut guest = KeyTable::new("guest");
    host.bind(0x1E, b'a');
    guest.bind(0x1E, b'q');
    dual.host = host;
    dual.guest = guest;
    let host_hit = dual.lookup(0x1E);
    dual.on_focus(Plane::Guest);
    let guest_hit = dual.lookup(0x1E);
    dual.toggle();
    set.add(
        "F405 dual keymap",
        host_hit == Some(b'a')
            && guest_hit == Some(b'q')
            && dual.active == Plane::Host
            && dual.lookup(dup_switch(dual)) == None,
        "dual plane",
    );
    set.add(
        "F405 unmapped",
        KeyTable::new("x").lookup(5).is_none() && !KeyTable::new("x").bind(0xFF, b'a'),
        "bounds",
    );

    let policy = ExclusivePolicy { foreground_permille: 800, background_permille: 100, exclusive: true };
    set.add(
        "F406 compute exclusive",
        compute_weight(policy, true) == 1000
            && compute_weight(ExclusivePolicy { exclusive: false, ..policy }, true) == 800
            && compute_weight(policy, false) == 100
            && exclusive_allowed(100)
            && !exclusive_allowed(950),
        "weights",
    );

    let (out_alpha, in_alpha) = crossfade_alpha(250);
    let (x, y, w, h) = ken_burns_rect(1920, 1080, 1000);
    set.add(
        "F407/F408 wallpaper",
        out_alpha == 750
            && in_alpha == 250
            && w <= 1920
            && h <= 1080
            && x <= 1920 - w
            && y <= 1080 - h
            && {
                let field = ParticleField::new(64, 0x1234);
                field.position(0, 0, 1920, 1080) == field.position(0, 0, 1920, 1080)
                    && field.position(0, 1, 1920, 1080) != field.position(0, 0, 1920, 1080)
                    && field.active_count(1000) < field.active_count(9000)
            },
        "ken burns + particles",
    );

    let boot = BootAnimation::new();
    let reduced = BootAnimation { reduce_motion: true, ..boot };
    set.add(
        "F409 boot light layers",
        boot.layer_count() == 7
            && reduced.layer_count() == 1
            && boot.layer_opacity(LightLayer::Logo, 0) < boot.layer_opacity(LightLayer::Logo, 2000)
            && boot.progress_permille(700) == 500
            && boot.progress_permille(99_999) == 1000,
        "seven layers",
    );

    let mut stages = [IconStage::Vector; 3];
    let sn = icon_pipeline(32, &mut stages);
    set.add(
        "F410 icon pipeline",
        sn == 3
            && stages[1] == IconStage::Rasterize128
            && stages[2] == IconStage::Downscale32
            && icon_contract_ok(128, 128)
            && !icon_contract_ok(130, 128)
            && !icon_contract_ok(128, 64),
        "128px master",
    );

    let trash = Trash { count: 4, bytes: 4096, retention_days: 30 };
    let items = [
        TrashItem { name: "a", original_dir: "/home", deleted_ms: 0 },
        TrashItem { name: "b", original_dir: "/home", deleted_ms: 1_000 },
    ];
    set.add(
        "F411 trash semantics",
        trash.retention_days == 30
            && restore_target("a", true) == " (restored)"
            && restore_target("a", false).is_empty()
            && purge_candidates(&items, 31 * 86_400_000, 30) == 2
            && purge_candidates(&items, 0, 30) == 0,
        "trash",
    );

    let mut tags = TagIndex::new();
    tags.add_tag(Tag { name: "work", color: 0x3B82F6 });
    let assigned = tags.assign("work");
    set.add(
        "F412 file tags",
        assigned
            && tags.count_of("work") == 1
            && tags.find("work").map(|t| t.color) == Some(0x3B82F6)
            && !tags.add_tag(Tag { name: "work", color: 0 })
            && !tags.assign("missing"),
        "tag index",
    );

    let aliases = [
        LauncherAlias { alias: "term", target: "terminal" },
        LauncherAlias { alias: "chr", target: "chrome" },
    ];
    set.add(
        "F413 quick launcher",
        alias_resolve(&aliases, "term") == Some("terminal")
            && alias_resolve(&aliases, "chr") == Some("chrome")
            && alias_resolve(&aliases, "chrom") == Some("chrome")
            && alias_resolve(&aliases, "zzz").is_none()
            && alias_resolve(&aliases, "").is_none(),
        "alias resolve",
    );

    let rules = [
        NotifyRule { app: "focus", mute: true, allow_banner: true },
        NotifyRule { app: "mail", mute: false, allow_banner: false },
    ];
    set.add(
        "F414 notification routing",
        route_notification("focus", 3, &rules, false) == NotifyDestination::Silent
            && route_notification("mail", 3, &rules, false) == NotifyDestination::Center
            && route_notification("chat", 3, &rules, false) == NotifyDestination::Banner
            && route_notification("chat", 2, &rules, true) == NotifyDestination::Center
            && route_notification("chat", 1, &rules, false) == NotifyDestination::Center,
        "routing",
    );

    let cats = [
        SettingsCategory { id: "display", title: "Display", keywords: "brightness scale hidpi", depth: 1 },
        SettingsCategory { id: "power", title: "Power", keywords: "battery sleep", depth: 1 },
    ];
    let mut found = [0usize; 4];
    let hits = settings_search(&cats, "bright", &mut found);
    set.add(
        "F415 settings centre",
        hits == 1
            && found[0] == 0
            && settings_search(&cats, "POWER", &mut found) == 1
            && settings_search(&cats, "", &mut found) == 0
            && settings_search(&cats, "nope", &mut found) == 0,
        "search",
    );

    let snapshot = LayoutSnapshot { revision: 7, window_count: 4, grid_cols: 6, theme_hash: 0xAABB, scene: 2 };
    let encoded = snapshot.encode();
    let decoded = LayoutSnapshot::decode(&encoded).expect("snapshot");
    set.add(
        "F416 layout snapshot",
        decoded == snapshot
            && decoded.revision == 7
            && LayoutSnapshot::decode(b"XXXX").is_none()
            && encoded[0..4] == *b"VLAY",
        "round trip",
    );

    set.add(
        "F417 hidden windows",
        in_shell_surfaces(false)
            && !in_shell_surfaces(true)
            && hidden_but_alive(HiddenWindow { id: 1, hidden: true, still_running: true })
            && !hidden_but_alive(HiddenWindow { id: 1, hidden: true, still_running: false }),
        "hide semantics",
    );

    let mut coach = ShortcutCoach::new();
    coach.note_use(0);
    let beginner_tips = coach.tips_to_show();
    for id in 0..6 {
        for _ in 0..5 {
            coach.note_use(id);
        }
    }
    set.add(
        "F418 shortcut coach",
        beginner_tips == 3
            && coach.level == HintLevel::Expert
            && coach.tips_to_show() == 1
            && coach.next_lesson() == 6,
        "progressive hints",
    );

    let a = Scene { name: "work", windows: 0b0011, wallpaper: 1 };
    let b = Scene { name: "play", windows: 0b1100, wallpaper: 2 };
    let (hide, show) = scene_diff(a, b);
    set.add(
        "F419 scene switch",
        hide == 0b0011 && show == 0b1100 && scene_diff(a, a) == (0, 0),
        "scene diff",
    );

    let pixels = [0x0033_66AAu32; 4];
    let accent = dominant_color(&pixels).expect("colour");
    set.add(
        "F420 theme follows wallpaper",
        dominant_color(&[]) == None
            && accent == 0x3366AA
            && foreground_for(0x3366AA) == 0xFFFFFF
            && foreground_for(0xFFFFFF) == 0x111111,
        "palette",
    );

    let mut dash = PrivacyDashboard::new();
    dash.record(PrivacyRecord { app: "cam", capability: "camera", stamp_ms: 10 });
    dash.record(PrivacyRecord { app: "cam", capability: "microphone", stamp_ms: 20 });
    set.add(
        "F421 privacy dashboard",
        dash.len() == 2 && dash.uses_of("cam", "camera") == 1 && dash.uses_of("cam", "x") == 0,
        "records",
    );

    set.add(
        "F422 zero telemetry",
        audit_telemetry(TelemetryAudit { outbound_endpoints: 0, local_log_only: true })
            == TelemetryVerdict::Clean
            && audit_telemetry(TelemetryAudit { outbound_endpoints: 1, local_log_only: true })
                == TelemetryVerdict::Violation(1),
        "audit",
    );

    let mut tokens = crate::ui::MotionTokens::base();
    let mut layers = [
        WallpaperLayer { kind: WallpaperKind::Static, opacity_permille: 1000, z: 0, motion: false },
        WallpaperLayer { kind: WallpaperKind::Particles, opacity_permille: 1000, z: 1, motion: true },
    ];
    apply_reduce_motion(true, &mut tokens, &mut layers);
    set.add(
        "F423 reduce motion",
        tokens.reduce_motion
            && !layers[1].motion
            && layers[1].opacity_permille == 600
            && tokens.duration_for(crate::ui::MotionKind::Slow) == 0,
        "propagation",
    );

    set.add(
        "F424 semantic budget",
        semantic_frame_verdict(
            SemanticBudget { per_frame_us: 16_000, spent_us: 8_000, animating_layers: 3 },
            false,
        ) == SemanticVerdict::Within
            && semantic_frame_verdict(
                SemanticBudget { per_frame_us: 16_000, spent_us: 20_000, animating_layers: 3 },
                false,
            ) == SemanticVerdict::DropLayer
            && semantic_frame_verdict(
                SemanticBudget { per_frame_us: 16_000, spent_us: 20_000, animating_layers: 1 },
                false,
            ) == SemanticVerdict::Over
            && semantic_frame_verdict(
                SemanticBudget { per_frame_us: 16_000, spent_us: 9_000, animating_layers: 1 },
                true,
            ) == SemanticVerdict::Over,
        "per-frame budget",
    );

    set
}

/// Helper for the dual-plane self check: the switch scancode of a keymap.
fn dup_switch(dual: DualKeymap) -> u8 {
    dual.switch_scancode
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f401_grid_wraps_rows() {
        let g = DesktopGrid { cols: 4, cell_w: 100, cell_h: 100, origin_x: 0, origin_y: 0 };
        assert_eq!(g.cell_of(4), (0, 100));
        assert_eq!(g.index_at(350, 99), Some(3));
        assert_eq!(g.index_at(400, 0), None); // beyond the last column
    }

    #[test]
    fn f402_split_ratios_clamped() {
        let mut t = VwmTree::new();
        t.add(VwmNode { id: 1, kind: VwmKind::VSplit, ratio_permille: 0, child_a: 2, child_b: 3 });
        t.add(VwmNode { id: 2, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
        t.add(VwmNode { id: 3, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
        let mut out = [(0u16, VwmRect { x: 0, y: 0, w: 0, h: 0 }); 4];
        assert_eq!(t.layout(1, VwmRect { x: 0, y: 0, w: 100, h: 100 }, &mut out, 0), 2);
        // ratio clamps to 50 permille minimum => at least 5px of 100.
        assert!(out[0].1.h >= 5);
        assert_eq!(out[0].1.h + out[1].1.h, 100);
    }

    #[test]
    fn f402_zero_width_split_stays_positive() {
        let mut t = VwmTree::new();
        t.add(VwmNode { id: 1, kind: VwmKind::HSplit, ratio_permille: 500, child_a: 2, child_b: 3 });
        t.add(VwmNode { id: 2, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
        t.add(VwmNode { id: 3, kind: VwmKind::Leaf, ratio_permille: 0, child_a: 0, child_b: 0 });
        let mut out = [(0u16, VwmRect { x: 0, y: 0, w: 0, h: 0 }); 4];
        assert_eq!(t.layout(1, VwmRect { x: 0, y: 0, w: 1, h: 10 }, &mut out, 0), 2);
        assert_eq!(out[0].1.w, 1);
        assert_eq!(out[1].1.w, 0);
    }

    #[test]
    fn f403_narrow_collapses() {
        let panes = ThreePane { ratios: [340, 330, 330], min_widths: [200, 200, 200], gap: 4 };
        let l = panes.compute(500);
        assert_eq!(l.widths[2], 0);
        assert!(l.collapsed[2] && l.collapsed[0]);
        assert!(l.widths.iter().sum::<u32>() <= 500);
        let wide = panes.compute(2000);
        assert!(wide.widths.iter().all(|w| *w > 0));
        assert_eq!(wide.collapsed.iter().filter(|c| **c).count(), 0);
    }

    #[test]
    fn f405_dual_plane_switch() {
        let mut d = DualKeymap::new();
        let mut h = KeyTable::new("h");
        h.bind(1, b'x');
        d.host = h;
        assert_eq!(d.lookup(1), Some(b'x'));
        d.toggle();
        assert_eq!(d.active, Plane::Guest);
        assert_eq!(d.lookup(1), None);
        d.on_focus(Plane::Host);
        assert_eq!(d.lookup(1), Some(b'x'));
        assert!(!KeyTable::new("h").bind(1, 0));
    }

    #[test]
    fn f407_crossfade_clamps() {
        assert_eq!(crossfade_alpha(0), (1000, 0));
        assert_eq!(crossfade_alpha(1000), (0, 1000));
        assert_eq!(crossfade_alpha(5000), (0, 1000));
    }

    #[test]
    fn f408_particle_count_scales() {
        let f = ParticleField::new(100, 7);
        assert_eq!(f.active_count(500), 25);
        assert_eq!(f.active_count(5_000), 50);
        assert_eq!(f.active_count(20_000), 100);
        let (x, _y) = f.position(3, 9, 100, 100);
        assert!(x < 100);
    }

    #[test]
    fn f410_pipeline_bounds() {
        let mut small = [IconStage::Vector; 1];
        assert_eq!(icon_pipeline(16, &mut small), 1);
        assert_eq!(small[0], IconStage::Vector);
        assert!(!icon_contract_ok(0, 128));
        assert!(icon_contract_ok(256, 128));
    }

    #[test]
    fn f411_retention_edges() {
        let items = [TrashItem { name: "x", original_dir: "/", deleted_ms: 0 }];
        assert_eq!(purge_candidates(&items, 0, 0), 0);
        assert_eq!(purge_candidates(&[], 0, 30), 0);
        assert_eq!(purge_candidates(&items, 86_400_001, 1), 1);
    }

    #[test]
    fn f413_alias_prefix_rules() {
        let aliases = [LauncherAlias { alias: "term", target: "terminal" }];
        assert_eq!(alias_resolve(&aliases, "TERM"), Some("terminal"));
        assert_eq!(alias_resolve(&aliases, "termi"), Some("terminal"));
        assert_eq!(alias_resolve(&aliases, "terminal"), Some("terminal"));
        assert_eq!(alias_resolve(&aliases, "termx"), None);
    }

    #[test]
    fn f415_search_is_case_insensitive() {
        let cats = [SettingsCategory { id: "a", title: "Network", keywords: "wifi ethernet", depth: 1 }];
        let mut out = [0usize; 2];
        assert_eq!(settings_search(&cats, "WIFI", &mut out), 1);
        assert_eq!(settings_search(&cats, "net", &mut out), 1);
        assert_eq!(settings_search(&cats, "ethernet", &mut out), 1);
        assert_eq!(settings_search(&cats, "bluetooth", &mut out), 0);
    }

    #[test]
    fn f418_coach_levels() {
        let mut c = ShortcutCoach::new();
        assert_eq!(c.level, HintLevel::Beginner);
        c.note_use(0);
        c.note_use(0);
        assert_eq!(c.level, HintLevel::Beginner);
        for _ in 0..5 {
            c.note_use(0);
            c.note_use(1);
        }
        assert_eq!(c.level, HintLevel::Intermediate);
        for id in 2..6 {
            for _ in 0..5 {
                c.note_use(id);
            }
        }
        assert_eq!(c.level, HintLevel::Expert);
        assert_eq!(c.next_lesson(), 6);
    }

    #[test]
    fn f420_palette_and_contrast() {
        let pixels = [0xFFFFFFu32; 3];
        assert_eq!(dominant_color(&pixels), Some(0xFFFFFF));
        let dark = [0x101010u32; 2];
        assert_eq!(foreground_for(dominant_color(&dark).unwrap()), 0xFFFFFF);
    }

    #[test]
    fn f424_budget_matrix() {
        let b = SemanticBudget { per_frame_us: 10_000, spent_us: 3_000, animating_layers: 0 };
        assert_eq!(semantic_frame_verdict(b, false), SemanticVerdict::Within);
        assert_eq!(
            semantic_frame_verdict(SemanticBudget { spent_us: 6_000, ..b }, true),
            SemanticVerdict::Over
        );
        assert_eq!(
            semantic_frame_verdict(SemanticBudget { spent_us: 6_000, animating_layers: 2, ..b }, true),
            SemanticVerdict::DropLayer
        );
    }

    #[test]
    fn f425_self_test_passes() {
        let set = run_vsem_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("vsem self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
