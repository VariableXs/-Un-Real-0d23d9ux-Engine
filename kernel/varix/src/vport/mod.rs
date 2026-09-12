//! VARIABLE-200 AI-07 · AURORA-1000 移植与兼容域（F151~F175，W3）。
//!
//! 把桌面搬进内核：Tauri API 映射层（含显式降级表）、渲染后端双实现与
//! 像素级走查、窗口系统语义平移、任务栏/开始菜单进程拆分、应用生命周期、
//! 资产管线、状态持久化、剪贴板/拖放/通知/内总线、主题与动效复用、
//! 应用沙箱与崩溃不扩散。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate；
//! **不删 Windows 后端**——移植以"新增 Varix 后端"方式进行（回滚纪律）。

use crate::checks::CheckSet;
use crate::gfxsrv::{argb, color_b, color_g, color_r, rgb, Canvas};

pub mod render;
pub mod verify;

// ---------------------------------------------------------------------------
// F151 Tauri API 映射清单 — 每个调用 → Varix syscall/服务 的映射总表
// ---------------------------------------------------------------------------

pub const MAP_MAX: usize = 24;
pub const API_NAME_MAX: usize = 24;
pub const API_TARGET_MAX: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapKind {
    /// 一对一直接映射到 Varix 原语。
    Direct,
    /// 需要兼容层薄壳（语义拼接）。
    Shim,
    /// 无对应实现，显式降级（禁止静默缺失）。
    Degraded,
}

#[derive(Clone, Copy)]
pub struct MappingEntry {
    pub api: [u8; API_NAME_MAX],
    pub api_len: usize,
    pub target: [u8; API_TARGET_MAX],
    pub target_len: usize,
    pub kind: MapKind,
    /// 降级原因码（`MapKind::Degraded` 时必须非 0）。
    pub degrade_reason: u8,
}

impl MappingEntry {
    pub const fn empty() -> MappingEntry {
        MappingEntry {
            api: [0u8; API_NAME_MAX],
            api_len: 0,
            target: [0u8; API_TARGET_MAX],
            target_len: 0,
            kind: MapKind::Direct,
            degrade_reason: 0,
        }
    }

    pub fn api_eq(&self, want: &[u8]) -> bool {
        &self.api[..self.api_len] == want
    }

    pub fn target_bytes(&self) -> &[u8] {
        &self.target[..self.target_len]
    }

    /// 完整性：要么有目标，要么显式降级带原因。
    pub fn covered(&self) -> bool {
        match self.kind {
            MapKind::Degraded => self.degrade_reason != 0,
            _ => self.target_len > 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ApiMap {
    entries: [MappingEntry; MAP_MAX],
    count: usize,
    /// 映射清单版本（与 ABI 版本协商联动）。
    pub version: u16,
}

impl ApiMap {
    pub const fn new(version: u16) -> ApiMap {
        ApiMap { entries: [MappingEntry::empty(); MAP_MAX], count: 0, version }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn entry(&self, i: usize) -> Option<MappingEntry> {
        if i < self.count {
            Some(self.entries[i])
        } else {
            None
        }
    }

    pub fn add(
        &mut self,
        api: &[u8],
        target: &[u8],
        kind: MapKind,
        degrade_reason: u8,
    ) -> Option<usize> {
        if self.count >= MAP_MAX || api.is_empty() || api.len() >= API_NAME_MAX {
            return None;
        }
        if target.len() >= API_TARGET_MAX {
            return None;
        }
        if self.find(api).is_some() {
            return None;
        }
        let mut e = MappingEntry::empty();
        e.api_len = api.len();
        e.target_len = target.len();
        e.kind = kind;
        e.degrade_reason = degrade_reason;
        let mut i = 0usize;
        while i < api.len() {
            e.api[i] = api[i];
            i += 1;
        }
        let mut k = 0usize;
        while k < target.len() {
            e.target[k] = target[k];
            k += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, api: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].api_eq(api) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn count_kind(&self, kind: MapKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].kind == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 映射清单完整性（F151 判定）：无静默缺失。
    pub fn fully_covered(&self) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if !self.entries[i].covered() {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 降级原因码。
pub const DEGRADE_NONE: u8 = 0;
pub const DEGRADE_NO_GPU: u8 = 1;
pub const DEGRADE_NO_NET: u8 = 2;
pub const DEGRADE_NO_USB: u8 = 3;
pub const DEGRADE_UNSUPPORTED: u8 = 4;
pub const DEGRADE_TEXT_ONLY: u8 = 5;

/// AURORA-1000 的 Tauri API 映射清单（一页纸总表的可执行形态）。
pub fn tauri_api_map() -> ApiMap {
    let mut m = ApiMap::new(1);
    let _ = m.add(b"window.create", b"vwm.window_create", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"window.set_title", b"vwm.set_title", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"window.show", b"vwm.show", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"window.close", b"vwm.close", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"event.listen", b"ipc.port_subscribe", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"event.emit", b"ipc.port_publish", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"invoke", b"ipc.shim_invoke", MapKind::Shim, DEGRADE_NONE);
    let _ = m.add(b"fs.read_text", b"vfs.read", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"fs.write_text", b"vfs.write", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"path.resolve", b"vfs.resolve", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"clipboard.write", b"srv.clipboard_put", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"clipboard.read", b"srv.clipboard_get", MapKind::Direct, DEGRADE_NONE);
    let _ = m.add(b"notification.send", b"srv.notify_push", MapKind::Shim, DEGRADE_NONE);
    let _ = m.add(b"dialog.open", b"vfs.pick_path", MapKind::Shim, DEGRADE_NONE);
    let _ = m.add(b"webview.eval", b"aurora.webview", MapKind::Degraded, DEGRADE_UNSUPPORTED);
    let _ = m.add(b"updater.check", b"deploy.update_probe", MapKind::Degraded, DEGRADE_NO_NET);
    m
}

// ---------------------------------------------------------------------------
// F152 映射层实现 — Tauri 语义薄壳
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShimCode {
    Ok,
    Unsupported,
    BadArg,
    NotFound,
    Denied,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShimResult {
    pub code: ShimCode,
    /// 结果载荷（fd/句柄/值）。
    pub value: u32,
}

impl ShimResult {
    pub const fn ok(value: u32) -> ShimResult {
        ShimResult { code: ShimCode::Ok, value }
    }

    pub const fn err(code: ShimCode) -> ShimResult {
        ShimResult { code, value: 0 }
    }
}

#[derive(Clone, Copy)]
pub struct ShimLayer {
    pub map: ApiMap,
    /// 转发总次数。
    pub calls: u64,
    pub degraded: u64,
    pub errors: u64,
    /// 句柄分配计数（window.create 等）。
    next_handle: u32,
}

impl ShimLayer {
    pub const fn new(map: ApiMap) -> ShimLayer {
        ShimLayer { map, calls: 0, degraded: 0, errors: 0, next_handle: 1 }
    }

    /// 薄壳派发：语义翻译 + 降级显式化。
    pub fn dispatch(&mut self, api: &[u8], arg: u32) -> ShimResult {
        self.calls += 1;
        let idx = match self.map.find(api) {
            Some(i) => i,
            None => {
                self.errors += 1;
                return ShimResult::err(ShimCode::NotFound);
            }
        };
        let e = match self.map.entry(idx) {
            Some(e) => e,
            None => {
                self.errors += 1;
                return ShimResult::err(ShimCode::NotFound);
            }
        };
        match e.kind {
            MapKind::Degraded => {
                self.degraded += 1;
                ShimResult::err(ShimCode::Unsupported)
            }
            MapKind::Direct => {
                if arg == u32::MAX {
                    self.errors += 1;
                    return ShimResult::err(ShimCode::BadArg);
                }
                ShimResult::ok(arg)
            }
            MapKind::Shim => {
                // 薄壳：分配/复用句柄。
                let h = self.next_handle;
                self.next_handle += 1;
                ShimResult::ok(h)
            }
        }
    }

    pub fn handles_issued(&self) -> u32 {
        self.next_handle - 1
    }
}

// ---------------------------------------------------------------------------
// F153 显式降级表 — 无对应实现显式返回降级，禁止静默缺失
// ---------------------------------------------------------------------------

pub const DEGRADE_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct DegradeItem {
    pub feature: [u8; 24],
    pub feature_len: usize,
    pub reason: u8,
    /// 降级后的替代行为描述码（0 = 无替代 = 功能不可用）。
    pub fallback: u8,
    /// 用户可见提示（通知中心展示）。
    pub user_visible: bool,
}

impl DegradeItem {
    pub const fn empty() -> DegradeItem {
        DegradeItem { feature: [0u8; 24], feature_len: 0, reason: 0, fallback: 0, user_visible: false }
    }

    pub fn feature_eq(&self, want: &[u8]) -> bool {
        &self.feature[..self.feature_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct DegradeTable {
    items: [DegradeItem; DEGRADE_MAX],
    count: usize,
    /// 静默缺失计数（必须恒为 0）。
    pub silent_misses: u64,
}

impl DegradeTable {
    pub const fn new() -> DegradeTable {
        DegradeTable { items: [DegradeItem::empty(); DEGRADE_MAX], count: 0, silent_misses: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, feature: &[u8], reason: u8, fallback: u8, user_visible: bool) -> Option<usize> {
        if self.count >= DEGRADE_MAX || feature.is_empty() || feature.len() >= 24 || reason == 0 {
            return None;
        }
        let mut it = DegradeItem::empty();
        it.feature_len = feature.len();
        it.reason = reason;
        it.fallback = fallback;
        it.user_visible = user_visible;
        let mut i = 0usize;
        while i < feature.len() {
            it.feature[i] = feature[i];
            i += 1;
        }
        self.items[self.count] = it;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 查询降级：未登记即记一次静默缺失（构建期门禁会因此变红）。
    pub fn lookup(&mut self, feature: &[u8]) -> Option<DegradeItem> {
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].feature_eq(feature) {
                return Some(self.items[i]);
            }
            i += 1;
        }
        self.silent_misses += 1;
        None
    }

    pub fn by_reason(&self, reason: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].reason == reason {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 默认降级表（与映射清单的 Degraded 项一一对应）。
pub fn default_degrade_table() -> DegradeTable {
    let mut t = DegradeTable::new();
    let _ = t.add(b"webview.eval", DEGRADE_UNSUPPORTED, 0, true);
    let _ = t.add(b"updater.check", DEGRADE_NO_NET, 1, true);
    let _ = t.add(b"gpu.accel", DEGRADE_NO_GPU, 2, false);
    let _ = t.add(b"usb.hotplug", DEGRADE_NO_USB, 3, true);
    t
}

// ---------------------------------------------------------------------------
// F154/F155/F156 渲染后端抽象 + Varix 后端 + 像素级走查
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendKind {
    /// AURORA Windows 版后端（语义基准，保持可运行、不删）。
    Windows,
    /// 新增的 Varix 后端（光栅化到帧缓冲服务画布）。
    Varix,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// 实心矩形。
    Rect,
    /// 圆角矩形（用四角覆盖近似）。
    Rounded,
    /// 文本块（点阵方框代表字形覆盖）。
    Text,
}

#[derive(Clone, Copy)]
pub struct UiNode {
    pub id: u32,
    pub kind: NodeKind,
    pub z: i32,
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub fill: u32,
    /// 文本长度（Text 节点用）。
    pub text_len: usize,
    pub radius: usize,
}

impl UiNode {
    pub const fn rect(id: u32, z: i32, x: usize, y: usize, w: usize, h: usize, fill: u32) -> UiNode {
        UiNode { id, kind: NodeKind::Rect, z, x, y, w, h, fill, text_len: 0, radius: 0 }
    }

    pub const fn rounded(id: u32, z: i32, x: usize, y: usize, w: usize, h: usize, fill: u32, r: usize) -> UiNode {
        UiNode { id, kind: NodeKind::Rounded, z, x, y, w, h, fill, text_len: 0, radius: r }
    }

    pub const fn text(id: u32, z: i32, x: usize, y: usize, w: usize, h: usize, fill: u32, n: usize) -> UiNode {
        UiNode { id, kind: NodeKind::Text, z, x, y, w, h, fill, text_len: n, radius: 0 }
    }
}

pub const TREE_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct UiTree {
    nodes: [UiNode; TREE_MAX],
    count: usize,
}

impl UiTree {
    pub const fn new() -> UiTree {
        UiTree { nodes: [UiNode::rect(0, 0, 0, 0, 0, 0, 0); TREE_MAX], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn node(&self, i: usize) -> Option<UiNode> {
        if i < self.count {
            Some(self.nodes[i])
        } else {
            None
        }
    }

    pub fn add(&mut self, n: UiNode) -> Option<usize> {
        if self.count >= TREE_MAX {
            return None;
        }
        self.nodes[self.count] = n;
        self.count += 1;
        Some(self.count - 1)
    }

    /// Z 序（升序 = 底 → 顶），相等 Z 保持插入序（稳定）。
    pub fn z_order(&self) -> [usize; TREE_MAX] {
        let mut idx = [0usize; TREE_MAX];
        let mut i = 0usize;
        while i < TREE_MAX {
            idx[i] = i;
            i += 1;
        }
        let mut a = 1usize;
        while a < self.count {
            let key = idx[a];
            let mut b = a;
            while b > 0 && self.nodes[idx[b - 1]].z > self.nodes[key].z {
                idx[b] = idx[b - 1];
                b -= 1;
            }
            idx[b] = key;
            a += 1;
        }
        idx
    }
}

/// 语义基准后端（等价于 AURORA Windows 版后端的输出契约）。
pub fn rasterize_reference(tree: &UiTree, out: &mut Canvas) -> usize {
    out.fill(0);
    let order = tree.z_order();
    let mut painted = 0usize;
    let mut i = 0usize;
    while i < tree.count {
        let n = tree.nodes[order[i]];
        painted += paint_node(&n, out);
        i += 1;
    }
    painted
}

/// F155 Varix 渲染后端：组件树光栅化到帧缓冲服务画布。
/// 与基准后端实现路径不同（先按节点类型分桶再逐层提交），像素必须一致。
pub fn rasterize_varix(tree: &UiTree, out: &mut Canvas) -> usize {
    out.fill(0);
    let order = tree.z_order();
    let mut painted = 0usize;
    // 分桶：Rect → Rounded → Text（桶内保持 Z 序）。
    const BUCKETS: [NodeKind; 3] = [NodeKind::Rect, NodeKind::Rounded, NodeKind::Text];
    let mut bi = 0usize;
    while bi < BUCKETS.len() {
        let mut i = 0usize;
        while i < tree.count {
            let n = tree.nodes[order[i]];
            if n.kind == BUCKETS[bi] {
                painted += paint_node(&n, out);
            }
            i += 1;
        }
        bi += 1;
    }
    painted
}

fn paint_node(n: &UiNode, out: &mut Canvas) -> usize {
    if n.w == 0 || n.h == 0 {
        return 0;
    }
    let mut painted = 0usize;
    let mut y = 0usize;
    while y < n.h {
        let mut x = 0usize;
        while x < n.w {
            let inside = match n.kind {
                NodeKind::Rounded => {
                    let r = core::cmp::min(n.radius, core::cmp::min(n.w, n.h) / 2);
                    corner_inside(x, y, n.w, n.h, r)
                }
                NodeKind::Text => {
                    // 字形覆盖：仅在"笔画"位置落色，其余透明不上色。
                    let stem = 1usize;
                    let band = n.text_len;
                    x < band && (x < stem || y < stem || y + stem >= n.h)
                }
                NodeKind::Rect => true,
            };
            if inside {
                let c = match n.kind {
                    NodeKind::Text => argb(255, color_r(n.fill), color_g(n.fill), color_b(n.fill)),
                    _ => argb(255, color_r(n.fill), color_g(n.fill), color_b(n.fill)),
                };
                if out.put(n.x + x, n.y + y, c) {
                    painted += 1;
                }
            }
            x += 1;
        }
        y += 1;
    }
    painted
}

fn corner_inside(x: usize, y: usize, w: usize, h: usize, r: usize) -> bool {
    if r == 0 {
        return true;
    }
    let inside_x = x >= r && x + r < w;
    let inside_y = y >= r && y + r < h;
    if inside_x || inside_y {
        return true;
    }
    // 落在角区：按圆判定。
    let cx = if x < r { r - 1 - x } else { x - (w - r) };
    let cy = if y < r { r - 1 - y } else { y - (h - r) };
    let d2 = cx * cx + cy * cy;
    d2 <= r * r
}

/// F156 像素级走查：两后端逐像素对比，返回差异像素数。
pub fn pixel_walk(tree: &UiTree) -> (usize, usize) {
    let mut a = Canvas::new();
    let mut b = Canvas::new();
    let pa = rasterize_reference(tree, &mut a);
    let pb = rasterize_varix(tree, &mut b);
    let mut diff = 0usize;
    let mut i = 0usize;
    while i < a.as_slice().len() {
        if a.as_slice()[i] != b.as_slice()[i] {
            diff += 1;
        }
        i += 1;
    }
    let _ = (pa, pb);
    (diff, pa)
}

/// 标准走查用例组件树（覆盖三种节点类型与 Z 序交叠）。
pub fn walk_fixture() -> UiTree {
    let mut t = UiTree::new();
    let _ = t.add(UiNode::rect(1, 0, 0, 0, 24, 18, rgb(24, 24, 27)));
    let _ = t.add(UiNode::rounded(2, 5, 2, 2, 16, 12, rgb(56, 189, 248), 3));
    let _ = t.add(UiNode::text(3, 9, 4, 4, 12, 6, rgb(244, 244, 245), 6));
    let _ = t.add(UiNode::rect(4, 2, 10, 10, 18, 12, rgb(39, 39, 42)));
    t
}

// ---------------------------------------------------------------------------
// F157 窗口系统语义平移 — AURORA W2 窗口核心（层级/Z 序/拖拽）
// ---------------------------------------------------------------------------

pub const WIN_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinState {
    Normal,
    Minimized,
    Maximized,
    /// 快照贴边（左/右半屏）。
    SnappedLeft,
    SnappedRight,
}

#[derive(Clone, Copy)]
pub struct Win {
    pub id: u32,
    pub app: u32,
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub z: i32,
    pub state: WinState,
    /// 最大化/贴边前的几何（恢复用）。
    pub restore: (i32, i32, usize, usize),
    pub visible: bool,
}

impl Win {
    pub const fn empty() -> Win {
        Win {
            id: 0,
            app: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            z: 0,
            state: WinState::Normal,
            restore: (0, 0, 0, 0),
            visible: false,
        }
    }
}

/// 桌面工作区尺寸（语义平移的边界）。
pub const DESKTOP_W: i32 = 32;
pub const DESKTOP_H: i32 = 24;

#[derive(Clone, Copy)]
pub struct WindowCore {
    wins: [Win; WIN_MAX],
    count: usize,
    z_next: i32,
    pub focused: u32,
    pub drags: u64,
    pub raises: u64,
}

impl WindowCore {
    pub const fn new() -> WindowCore {
        WindowCore { wins: [Win::empty(); WIN_MAX], count: 0, z_next: 1, focused: 0, drags: 0, raises: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn win(&self, i: usize) -> Option<Win> {
        if i < self.count {
            Some(self.wins[i])
        } else {
            None
        }
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.wins[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn create(&mut self, id: u32, app: u32, x: i32, y: i32, w: usize, h: usize) -> Option<usize> {
        if self.count >= WIN_MAX || id == 0 || w == 0 || h == 0 || self.find(id).is_some() {
            return None;
        }
        let mut win = Win::empty();
        win.id = id;
        win.app = app;
        win.x = x;
        win.y = y;
        win.w = w;
        win.h = h;
        win.z = self.z_next;
        win.state = WinState::Normal;
        win.restore = (x, y, w, h);
        win.visible = true;
        self.z_next += 1;
        self.wins[self.count] = win;
        self.count += 1;
        self.focused = id;
        Some(self.count - 1)
    }

    /// 置顶：Z 序单调递增，焦点跟随。
    pub fn raise(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        self.wins[i].z = self.z_next;
        self.z_next += 1;
        self.focused = id;
        self.raises += 1;
        true
    }

    /// Z 序数组（升序 = 底 → 顶）。
    pub fn z_order(&self) -> [u32; WIN_MAX] {
        let mut ids = [0u32; WIN_MAX];
        let mut order = [0usize; WIN_MAX];
        let mut i = 0usize;
        while i < WIN_MAX {
            order[i] = i;
            i += 1;
        }
        let mut a = 1usize;
        while a < self.count {
            let key = order[a];
            let mut b = a;
            while b > 0 && self.wins[order[b - 1]].z > self.wins[key].z {
                order[b] = order[b - 1];
                b -= 1;
            }
            order[b] = key;
            a += 1;
        }
        let mut n = 0usize;
        while n < self.count {
            ids[n] = self.wins[order[n]].id;
            n += 1;
        }
        ids
    }

    /// 拖拽：位移后钳制在桌面内（窗口不出屏）。
    pub fn drag_by(&mut self, id: u32, dx: i32, dy: i32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if self.wins[i].state == WinState::Maximized {
            return false;
        }
        if self.wins[i].state == WinState::SnappedLeft || self.wins[i].state == WinState::SnappedRight {
            // 拖拽即脱离贴边（语义平移：与 Windows 一致）。
            self.wins[i].state = WinState::Normal;
            let (x, y, w, h) = self.wins[i].restore;
            self.wins[i].x = x;
            self.wins[i].y = y;
            self.wins[i].w = w;
            self.wins[i].h = h;
        }
        let max_x = DESKTOP_W - self.wins[i].w as i32;
        let max_y = DESKTOP_H - self.wins[i].h as i32;
        self.wins[i].x = clamp_i32(self.wins[i].x + dx, 0, max_x.max(0));
        self.wins[i].y = clamp_i32(self.wins[i].y + dy, 0, max_y.max(0));
        self.drags += 1;
        true
    }

    pub fn minimize(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        self.wins[i].state = WinState::Minimized;
        self.wins[i].visible = false;
        if self.focused == id {
            self.focused = 0;
        }
        true
    }

    pub fn maximize(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if self.wins[i].state != WinState::Maximized {
            self.wins[i].restore = (self.wins[i].x, self.wins[i].y, self.wins[i].w, self.wins[i].h);
        }
        self.wins[i].x = 0;
        self.wins[i].y = 0;
        self.wins[i].w = DESKTOP_W as usize;
        self.wins[i].h = DESKTOP_H as usize;
        self.wins[i].state = WinState::Maximized;
        self.wins[i].visible = true;
        true
    }

    pub fn snap(&mut self, id: u32, left: bool) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        if self.wins[i].state != WinState::Maximized {
            self.wins[i].restore = (self.wins[i].x, self.wins[i].y, self.wins[i].w, self.wins[i].h);
        }
        self.wins[i].w = (DESKTOP_W / 2) as usize;
        self.wins[i].h = DESKTOP_H as usize;
        self.wins[i].x = if left { 0 } else { DESKTOP_W / 2 };
        self.wins[i].y = 0;
        self.wins[i].state = if left { WinState::SnappedLeft } else { WinState::SnappedRight };
        true
    }

    pub fn restore_state(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        let (x, y, w, h) = self.wins[i].restore;
        self.wins[i].x = x;
        self.wins[i].y = y;
        self.wins[i].w = w;
        self.wins[i].h = h;
        self.wins[i].state = WinState::Normal;
        self.wins[i].visible = true;
        true
    }

    pub fn close(&mut self, id: u32) -> bool {
        let i = match self.find(id) {
            Some(i) => i,
            None => return false,
        };
        self.wins[i] = self.wins[self.count - 1];
        self.count -= 1;
        if self.focused == id {
            self.focused = 0;
        }
        true
    }

    /// 命中测试：取 Z 最高且可见的窗口。
    pub fn hit_test(&self, x: i32, y: i32) -> Option<u32> {
        let order = self.z_order();
        let mut i = self.count;
        while i > 0 {
            i -= 1;
            let k = match self.find(order[i]) {
                Some(k) => k,
                None => continue,
            };
            let w = self.wins[k];
            if w.visible
                && x >= w.x
                && x < w.x + w.w as i32
                && y >= w.y
                && y < w.y + w.h as i32
            {
                return Some(w.id);
            }
        }
        None
    }
}

pub fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// F158 任务栏/开始菜单进程拆分
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellPart {
    Desktop,
    Taskbar,
    StartMenu,
    Notification,
}

#[derive(Clone, Copy)]
pub struct ShellProcess {
    pub part: ShellPart,
    pub pid: u32,
    /// 命名服务端口。
    pub port: u32,
    pub alive: bool,
    /// 依赖的其他部分（位掩码）。
    pub deps: u32,
}

impl ShellProcess {
    pub const fn empty() -> ShellProcess {
        ShellProcess { part: ShellPart::Desktop, pid: 0, port: 0, alive: false, deps: 0 }
    }
}

/// 进程槽位上限：4 个标准部分 + 重启用余量（崩溃后重建自身进程项）。
pub const SHELL_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct ShellSplit {
    parts: [ShellProcess; SHELL_MAX],
    count: usize,
    /// 跨进程通知（bump 计数）。
    pub notify_and_bump: u64,
    pub notification_hits: u64,
}

impl ShellSplit {
    pub const fn new() -> ShellSplit {
        ShellSplit { parts: [ShellProcess::empty(); SHELL_MAX], count: 0, notify_and_bump: 0, notification_hits: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn register(&mut self, part: ShellPart, pid: u32, port: u32, deps: u32) -> Option<usize> {
        if self.count >= SHELL_MAX || pid == 0 || port == 0 {
            return None;
        }
        let mut p = ShellProcess::empty();
        p.part = part;
        p.pid = pid;
        p.port = port;
        p.alive = true;
        p.deps = deps;
        self.parts[self.count] = p;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn index_of(&self, part: ShellPart) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].part == part {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, part: ShellPart) -> Option<ShellProcess> {
        self.index_of(part).map(|i| self.parts[i])
    }

    /// 强杀某部分：只影响它自己（拆分纪律）。
    pub fn kill(&mut self, part: ShellPart) -> bool {
        match self.index_of(part) {
            Some(i) => {
                if !self.parts[i].alive {
                    return false;
                }
                self.parts[i].alive = false;
                true
            }
            None => false,
        }
    }

    pub fn alive_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.parts[i].alive {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 通知中心投递（跨进程消息）。
    pub fn notify(&mut self, part: ShellPart) -> bool {
        match self.index_of(part) {
            Some(i) if self.parts[i].alive => {
                self.notify_and_bump += 1;
                if part == ShellPart::Notification {
                    self.notification_hits += 1;
                }
                true
            }
            _ => false,
        }
    }
}

/// 标准拆分布局：桌面 / 任务栏 / 开始菜单 / 通知中心 各自进程。
pub fn standard_shell_split() -> ShellSplit {
    let mut s = ShellSplit::new();
    let _ = s.register(ShellPart::Desktop, 10, 7001, 0);
    let _ = s.register(ShellPart::Taskbar, 11, 7002, 1 << 0);
    let _ = s.register(ShellPart::StartMenu, 12, 7003, (1 << 0) | (1 << 1));
    let _ = s.register(ShellPart::Notification, 13, 7004, 1 << 0);
    s
}

// ---------------------------------------------------------------------------
// F159 应用生命周期
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppState {
    Created,
    Running,
    Suspended,
    Stopped,
    Crashed,
}

#[derive(Clone, Copy)]
pub struct AppLifecycle {
    pub app: u32,
    pub state: AppState,
    pub win_id: u32,
    /// 窗口可见性必须跟随应用状态（语义同步）。
    pub win_visible: bool,
    pub launches: u32,
    pub suspends: u32,
    pub resumes: u32,
    pub exits: u32,
}

impl AppLifecycle {
    pub const fn empty() -> AppLifecycle {
        AppLifecycle {
            app: 0,
            state: AppState::Created,
            win_id: 0,
            win_visible: false,
            launches: 0,
            suspends: 0,
            resumes: 0,
            exits: 0,
        }
    }

    pub fn new(app: u32) -> AppLifecycle {
        let mut a = AppLifecycle::empty();
        a.app = app;
        a
    }

    /// 启动：状态 → Running，窗口可见。
    pub fn launch(&mut self, win_id: u32) -> bool {
        if self.state == AppState::Running {
            return false;
        }
        self.state = AppState::Running;
        self.win_id = win_id;
        self.win_visible = true;
        self.launches += 1;
        true
    }

    /// 挂起：状态 → Suspended，窗口仍占位但不再绘制。
    pub fn suspend(&mut self) -> bool {
        if self.state != AppState::Running {
            return false;
        }
        self.state = AppState::Suspended;
        self.suspends += 1;
        true
    }

    pub fn resume(&mut self) -> bool {
        if self.state != AppState::Suspended {
            return false;
        }
        self.state = AppState::Running;
        self.win_visible = true;
        self.resumes += 1;
        true
    }

    pub fn exit(&mut self) -> bool {
        if self.state == AppState::Stopped {
            return false;
        }
        self.state = AppState::Stopped;
        self.win_visible = false;
        self.win_id = 0;
        self.exits += 1;
        true
    }

    /// 崩溃：进入 Crashed，窗口立即隐藏（通知中心随后明示）。
    pub fn crash(&mut self) -> bool {
        if self.state == AppState::Stopped || self.state == AppState::Crashed {
            return false;
        }
        self.state = AppState::Crashed;
        self.win_visible = false;
        true
    }
}

// ---------------------------------------------------------------------------
// F160 资产管线 — 图标/主题/字体/文案进 initfs，路径约定统一
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    Icon,
    Theme,
    Font,
    String,
    Layout,
}

pub const ASSET_MAX: usize = 16;
pub const ASSET_PATH_MAX: usize = 40;

#[derive(Clone, Copy)]
pub struct Asset {
    pub path: [u8; ASSET_PATH_MAX],
    pub path_len: usize,
    pub kind: AssetKind,
    pub refs: u32,
    pub present: bool,
}

impl Asset {
    pub const fn empty() -> Asset {
        Asset { path: [0u8; ASSET_PATH_MAX], path_len: 0, kind: AssetKind::Icon, refs: 0, present: false }
    }

    pub fn path_eq(&self, want: &[u8]) -> bool {
        &self.path[..self.path_len] == want
    }
}

/// 资产路径约定：`/assets/<category>/<name>`。
pub fn asset_path(kind: AssetKind, name: &[u8], out: &mut [u8]) -> usize {
    let cat: &[u8] = match kind {
        AssetKind::Icon => b"icons",
        AssetKind::Theme => b"themes",
        AssetKind::Font => b"fonts",
        AssetKind::String => b"i18n",
        AssetKind::Layout => b"layouts",
    };
    let mut n = 0usize;
    push(out, &mut n, b"/assets/");
    push(out, &mut n, cat);
    push(out, &mut n, b"/");
    push(out, &mut n, name);
    n
}

fn push(out: &mut [u8], n: &mut usize, s: &[u8]) {
    let mut i = 0usize;
    while i < s.len() {
        if *n < out.len() {
            out[*n] = s[i];
            *n += 1;
        }
        i += 1;
    }
}

#[derive(Clone, Copy)]
pub struct AssetIndex {
    assets: [Asset; ASSET_MAX],
    count: usize,
    /// 引用但缺失的资产数（必须为 0）。
    pub missing_refs: u64,
}

impl AssetIndex {
    pub const fn new() -> AssetIndex {
        AssetIndex { assets: [Asset::empty(); ASSET_MAX], count: 0, missing_refs: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn add(&mut self, path: &[u8], kind: AssetKind) -> Option<usize> {
        if self.count >= ASSET_MAX || path.is_empty() || path.len() >= ASSET_PATH_MAX {
            return None;
        }
        if self.find(path).is_some() {
            return None;
        }
        let mut a = Asset::empty();
        a.path_len = path.len();
        a.kind = kind;
        a.present = true;
        let mut i = 0usize;
        while i < path.len() {
            a.path[i] = path[i];
            i += 1;
        }
        self.assets[self.count] = a;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.assets[i].path_eq(path) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 引用资产：不存在即记一次缺失（构建期门禁）。
    pub fn reference(&mut self, path: &[u8]) -> bool {
        match self.find(path) {
            Some(i) => {
                self.assets[i].refs += 1;
                true
            }
            None => {
                self.missing_refs += 1;
                false
            }
        }
    }

    pub fn count_kind(&self, kind: AssetKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.assets[i].kind == kind && self.assets[i].present {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 标准资产索引：主题/图标/字体/文案/布局各就位。
pub fn standard_assets() -> AssetIndex {
    let mut idx = AssetIndex::new();
    let mut buf = [0u8; ASSET_PATH_MAX];
    let n = asset_path(AssetKind::Theme, b"dark.toml", &mut buf);
    let _ = idx.add(&buf[..n], AssetKind::Theme);
    let n = asset_path(AssetKind::Icon, b"app-128.ico", &mut buf);
    let _ = idx.add(&buf[..n], AssetKind::Icon);
    let n = asset_path(AssetKind::Font, b"aurora-sans.ttf", &mut buf);
    let _ = idx.add(&buf[..n], AssetKind::Font);
    let n = asset_path(AssetKind::String, b"zh-CN.txt", &mut buf);
    let _ = idx.add(&buf[..n], AssetKind::String);
    let n = asset_path(AssetKind::Layout, b"us.kbl", &mut buf);
    let _ = idx.add(&buf[..n], AssetKind::Layout);
    idx
}

// ---------------------------------------------------------------------------
// 域自检：F151~F175（本文件 F151~F160，子模块 render/verify）
// ---------------------------------------------------------------------------

pub fn run_vport_checks() -> CheckSet {
    let mut set = CheckSet::new("vport");

    // F151 Tauri API 映射清单
    let map = tauri_api_map();
    set.add(
        "F151 api map",
        map.len() == 16
            && map.version == 1
            && map.count_kind(MapKind::Direct) == 11
            && map.count_kind(MapKind::Shim) == 3
            && map.count_kind(MapKind::Degraded) == 2
            && map.fully_covered()
            && map.find(b"window.create").map(|i| map.entry(i).map(|e| e.target_bytes() == b"vwm.window_create").unwrap_or(false)).unwrap_or(false)
            && map.find(b"nope").is_none()
            && !ApiMap::new(1).fully_covered(),
        "映射总表/类别计数/零静默缺失",
    );

    // F152 映射层实现
    let mut shim = ShimLayer::new(tauri_api_map());
    let ok = shim.dispatch(b"window.create", 7);
    let deg = shim.dispatch(b"webview.eval", 1);
    let miss = shim.dispatch(b"not.an.api", 0);
    let bad = shim.dispatch(b"fs.read_text", u32::MAX);
    let shimmy = shim.dispatch(b"invoke", 0);
    set.add(
        "F152 mapping layer",
        ok == ShimResult::ok(7)
            && deg.code == ShimCode::Unsupported
            && miss.code == ShimCode::NotFound
            && bad.code == ShimCode::BadArg
            && shimmy.code == ShimCode::Ok
            && shimmy.value == 1
            && shim.handles_issued() == 1
            && shim.degraded == 1
            && shim.errors == 2
            && shim.calls == 5,
        "薄壳派发/降级显式/错误码",
    );

    // F153 显式降级表
    let mut dt = default_degrade_table();
    let wv = dt.lookup(b"webview.eval");
    let up = dt.lookup(b"updater.check");
    let absent = dt.lookup(b"ghost.feature");
    let silent = dt.silent_misses;
    set.add(
        "F153 degrade table",
        dt.len() == 4
            && wv.map(|i| i.reason == DEGRADE_UNSUPPORTED && i.user_visible).unwrap_or(false)
            && up.map(|i| i.reason == DEGRADE_NO_NET && i.fallback == 1).unwrap_or(false)
            && absent.is_none()
            && silent == 1
            && dt.by_reason(DEGRADE_NO_GPU) == 1
            && !DegradeTable::new().add(b"x", 0, 0, false).is_some(),
        "降级表查询/静默缺失计数",
    );

    // F154/F155/F156 渲染后端抽象 + 像素级走查
    let tree = walk_fixture();
    let mut a = Canvas::new();
    let mut b = Canvas::new();
    let pa = rasterize_reference(&tree, &mut a);
    let pb = rasterize_varix(&tree, &mut b);
    let (diff, _) = pixel_walk(&tree);
    set.add(
        "F154 backend abstraction",
        tree.len() == 4
            && BackendKind::Windows != BackendKind::Varix
            && pa > 0
            && pb > 0
            && tree.z_order()[0] == 0
            && tree.z_order()[3] == 2,
        "双后端枚举/Z 序稳定",
    );
    set.add(
        "F155 varix backend",
        pa == pb && b.get(0, 0) != 0 && a.get(0, 0) == b.get(0, 0),
        "组件树光栅化到画布",
    );
    set.add(
        "F156 pixel walk",
        diff == 0 && pixel_walk(&walk_fixture()).0 == 0,
        "同组件双后端差异清零",
    );

    // F157 窗口系统语义平移
    let mut wc = WindowCore::new();
    let w1 = wc.create(1, 100, 2, 2, 16, 12);
    let w2 = wc.create(2, 100, 6, 6, 16, 12);
    let z_before = wc.z_order();
    let raised = wc.raise(1);
    let z_after = wc.z_order();
    let drag = wc.drag_by(1, 200, 200);
    let clamped = wc.win(w1.unwrap()).map(|w| w.x == DESKTOP_W - 16 && w.y == DESKTOP_H - 12).unwrap_or(false);
    let hit = wc.hit_test(18, 14);
    let maxed = wc.maximize(1);
    let drag_denied = !wc.drag_by(1, 1, 1);
    let snapped = wc.snap(2, true);
    let unsnapped = wc.drag_by(2, 1, 0);
    set.add(
        "F157 window semantics",
        w1 == Some(0)
            && w2 == Some(1)
            && z_before[1] == 2
            && raised
            && z_after[1] == 1
            && drag
            && clamped
            && hit == Some(1)
            && maxed
            && drag_denied
            && snapped
            && unsnapped
            && wc.win(w1.unwrap()).map(|w| w.state == WinState::Maximized).unwrap_or(false)
            && wc.win(1).map(|w| w.state == WinState::Normal && w.x == 7 && w.w == 16).unwrap_or(false),
        "层级/拖拽钳制/最大化/贴边脱离",
    );
    let mut wc2 = WindowCore::new();
    let a2 = wc2.create(7, 1, 0, 0, 8, 8);
    let b2 = wc2.create(8, 2, 4, 4, 8, 8);
    let closed = wc2.close(7);
    set.add(
        "F157 window lifecycle",
        a2 == Some(0) && b2 == Some(1) && closed && wc2.len() == 1
            && wc2.win(0).map(|w| w.id == 8).unwrap_or(false)
            && wc2.focused == 8
            && !wc2.close(99),
        "关闭即回收/焦点清空",
    );

    // F158 任务栏/开始菜单进程拆分
    let mut split = standard_shell_split();
    let taskbar = split.get(ShellPart::Taskbar);
    let killed = split.kill(ShellPart::StartMenu);
    let killed_again = !split.kill(ShellPart::StartMenu);
    let desktop_alive = split.get(ShellPart::Desktop).map(|p| p.alive).unwrap_or(false);
    let notify = split.notify(ShellPart::Notification);
    let notify_dead = !split.notify(ShellPart::StartMenu);
    let restart = split.register(ShellPart::StartMenu, 22, 7013, (1 << 0) | (1 << 1));
    set.add(
        "F158 shell split",
        split.len() == 5
            && taskbar.map(|p| p.pid == 11 && p.port == 7002).unwrap_or(false)
            && killed
            && killed_again
            && desktop_alive
            && split.alive_count() == 4
            && notify
            && notify_dead
            && split.notification_hits == 1
            && restart.is_some()
            && ShellPart::Taskbar != ShellPart::Desktop,
        "四部分独立进程/强杀不扩散/可重启",
    );

    // F159 应用生命周期
    let mut life = AppLifecycle::new(500);
    let pre = life.state;
    let launch = life.launch(9);
    let win_id_after_launch = life.win_id;
    let visible_after_launch = life.win_visible;
    let dup = !life.launch(10);
    let sus = life.suspend();
    let state_suspended = life.state;
    let res = life.resume();
    let bad_resume = {
        let mut l2 = AppLifecycle::new(501);
        let _ = l2.launch(1);
        let _ = l2.exit();
        l2.resume()
    };
    let crash = life.crash();
    let state_crashed = life.state;
    let visible_after_crash = life.win_visible;
    let exit_after_crash = life.exit();
    let win_id_after_exit = life.win_id;
    set.add(
        "F159 app lifecycle",
        pre == AppState::Created
            && launch
            && win_id_after_launch == 9
            && visible_after_launch
            && dup
            && sus
            && state_suspended == AppState::Suspended
            && res
            && !bad_resume
            && crash
            && state_crashed == AppState::Crashed
            && !visible_after_crash
            && exit_after_crash
            && win_id_after_exit == 0
            && life.launches == 1
            && life.exits == 1,
        "启动/挂起/恢复/崩溃/退出同步窗口",
    );

    // F160 资产管线
    let mut assets = standard_assets();
    let mut buf = [0u8; ASSET_PATH_MAX];
    let n = asset_path(AssetKind::Icon, b"app-128.ico", &mut buf);
    let path_ok = n == 25 && &buf[..n] == b"/assets/icons/app-128.ico";
    let hit = assets.reference(&buf[..n]);
    let miss = assets.reference(b"/assets/icons/ghost.ico");
    set.add(
        "F160 asset pipeline",
        assets.len() == 5
            && path_ok
            && hit
            && !miss
            && assets.missing_refs == 1
            && assets.count_kind(AssetKind::Theme) == 1
            && assets.count_kind(AssetKind::Font) == 1
            && !assets.add(b"/assets/icons/app-128.ico", AssetKind::Icon).is_some(),
        "路径约定/引用完整性/去重",
    );

    render::extend_checks(&mut set);
    verify::extend_checks(&mut set);

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f151_map_has_no_silent_gap() {
        let m = tauri_api_map();
        assert!(m.fully_covered());
        assert_eq!(m.count_kind(MapKind::Degraded), 2);
        for i in 0..m.len() {
            let e = m.entry(i).unwrap();
            if e.kind == MapKind::Degraded {
                assert_ne!(e.degrade_reason, 0, "degraded entries need a reason");
            } else {
                assert!(e.target_len > 0);
            }
        }
    }

    #[test]
    fn f156_pixel_walk_is_clean() {
        let (diff, painted) = pixel_walk(&walk_fixture());
        assert_eq!(diff, 0);
        assert!(painted > 0);
    }

    #[test]
    fn f157_drag_clamps_inside_desktop() {
        let mut w = WindowCore::new();
        let _ = w.create(1, 1, 0, 0, 20, 16);
        assert!(w.drag_by(1, 100, 100));
        let win = w.win(0).unwrap();
        assert_eq!(win.x, DESKTOP_W - 20);
        assert_eq!(win.y, DESKTOP_H - 16);
        assert!(win.x + win.w as i32 <= DESKTOP_W);
    }

    #[test]
    fn f158_kill_is_contained() {
        let mut s = standard_shell_split();
        assert!(s.kill(ShellPart::Taskbar));
        assert!(s.get(ShellPart::Desktop).unwrap().alive);
        assert!(s.get(ShellPart::StartMenu).unwrap().alive);
        assert_eq!(s.alive_count(), 3);
    }

    #[test]
    fn f159_window_follows_state() {
        let mut a = AppLifecycle::new(1);
        assert!(a.launch(3));
        assert!(a.win_visible);
        assert!(a.exit());
        assert!(!a.win_visible);
        assert_eq!(a.win_id, 0);
    }

    #[test]
    fn f_run_vport_checks_pass() {
        let set = run_vport_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("vport self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() >= 25, "vport domain must expose >= 25 checks");
    }
}
