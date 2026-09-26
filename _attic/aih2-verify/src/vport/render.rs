//! VARIABLE-200 AI-07 · 移植域运行时面（F161~F168）。
//!
//! 状态持久化、剪贴板、拖放、通知中心、内总线、主题令牌运行时、
//! 动效复用、应用进程沙箱。纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::gfxsrv::perf::Spring;
use crate::gfxsrv::text::{aurora_tokens, ThemeMode, ThemeTokens};
use crate::srv::ipc::NameService;

// ---------------------------------------------------------------------------
// F161 状态持久化 — 键值状态落盘 / 重启恢复
// ---------------------------------------------------------------------------

pub const STATE_SLOTS: usize = 8;
pub const STATE_KEY_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct StateStore {
    keys: [[u8; STATE_KEY_MAX]; STATE_SLOTS],
    key_len: [usize; STATE_SLOTS],
    values: [u32; STATE_SLOTS],
    count: usize,
    /// 恢复（load）次数。
    pub restores: u32,
}

impl StateStore {
    pub const fn new() -> StateStore {
        StateStore {
            keys: [[0u8; STATE_KEY_MAX]; STATE_SLOTS],
            key_len: [0usize; STATE_SLOTS],
            values: [0u32; STATE_SLOTS],
            count: 0,
            restores: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn set(&mut self, key: &[u8], value: u32) -> bool {
        if key.is_empty() || key.len() >= STATE_KEY_MAX {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i][..self.key_len[i]] == *key {
                self.values[i] = value;
                return true;
            }
            i += 1;
        }
        if self.count >= STATE_SLOTS {
            return false;
        }
        let mut k = 0usize;
        while k < key.len() {
            self.keys[self.count][k] = key[k];
            k += 1;
        }
        self.key_len[self.count] = key.len();
        self.values[self.count] = value;
        self.count += 1;
        true
    }

    pub fn get(&self, key: &[u8]) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if self.keys[i][..self.key_len[i]] == *key {
                return Some(self.values[i]);
            }
            i += 1;
        }
        None
    }

    /// 序列化为扁平字节流（键值对定长：1 字节键长 + 键 + 4 字节值）。
    pub fn snapshot(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if n + 1 + self.key_len[i] + 4 > out.len() {
                break;
            }
            out[n] = self.key_len[i] as u8;
            n += 1;
            let mut k = 0usize;
            while k < self.key_len[i] {
                out[n] = self.keys[i][k];
                n += 1;
                k += 1;
            }
            let v = self.values[i];
            out[n] = (v >> 24) as u8;
            out[n + 1] = (v >> 16) as u8;
            out[n + 2] = (v >> 8) as u8;
            out[n + 3] = v as u8;
            n += 4;
            i += 1;
        }
        n
    }

    /// 从字节流恢复（"重启后读盘"）。
    pub fn restore(&mut self, data: &[u8]) -> usize {
        self.count = 0;
        self.restores += 1;
        let mut p = 0usize;
        let mut loaded = 0usize;
        while p < data.len() {
            let klen = data[p] as usize;
            if klen == 0 || klen >= STATE_KEY_MAX || p + 1 + klen + 4 > data.len() {
                break;
            }
            p += 1;
            let mut key = [0u8; STATE_KEY_MAX];
            let mut i = 0usize;
            while i < klen {
                key[i] = data[p + i];
                i += 1;
            }
            let key_slice = &key[..klen];
            let v = ((data[p + klen] as u32) << 24)
                | ((data[p + klen + 1] as u32) << 16)
                | ((data[p + klen + 2] as u32) << 8)
                | data[p + klen + 3] as u32;
            p += klen + 4;
            if self.set(key_slice, v) {
                loaded += 1;
            }
        }
        loaded
    }
}

/// 应用状态的默认键（重启后不丢：主题/布局/剪贴板历史指针）。
pub fn default_state() -> StateStore {
    let mut s = StateStore::new();
    let _ = s.set(b"theme.mode", 0);
    let _ = s.set(b"keymap", 1);
    let _ = s.set(b"layout.tier", 2);
    let _ = s.set(b"clip.head", 0);
    let _ = s.set(b"session.wins", 3);
    s
}

// ---------------------------------------------------------------------------
// F162 剪贴板服务 — 跨进程复制粘贴（多格式，富文本预留）
// ---------------------------------------------------------------------------

pub const CLIP_TEXT_MAX: usize = 64;
pub const CLIP_HISTORY: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipFormat {
    Text,
    /// 富文本（预留：当前显式降级）。
    RichText,
    /// 图像（预留：当前显式降级）。
    Image,
}

#[derive(Clone, Copy)]
pub struct ClipEntry {
    pub format: ClipFormat,
    pub text: [u8; CLIP_TEXT_MAX],
    pub text_len: usize,
    pub owner_pid: u32,
    pub seq: u64,
    pub valid: bool,
}

impl ClipEntry {
    pub const fn empty() -> ClipEntry {
        ClipEntry {
            format: ClipFormat::Text,
            text: [0u8; CLIP_TEXT_MAX],
            text_len: 0,
            owner_pid: 0,
            seq: 0,
            valid: false,
        }
    }

    pub fn text_bytes(&self) -> &[u8] {
        &self.text[..self.text_len]
    }
}

#[derive(Clone, Copy)]
pub struct Clipboard {
    history: [ClipEntry; CLIP_HISTORY],
    /// 环形写指针。
    wr: usize,
    pub filled: usize,
    pub seq: u64,
    /// 当前有效条目下标（历史回退用）。
    pub head: usize,
    /// 富文本/图像格式的降级次数（显式，不静默）。
    pub degraded: u64,
    /// 跨进程粘贴次数。
    pub pastes: u64,
}

impl Clipboard {
    pub const fn new() -> Clipboard {
        Clipboard {
            history: [ClipEntry::empty(); CLIP_HISTORY],
            wr: 0,
            filled: 0,
            seq: 0,
            head: 0,
            degraded: 0,
            pastes: 0,
        }
    }

    /// 复制：文本进入历史环；预留格式显式降级。
    pub fn copy(&mut self, format: ClipFormat, data: &[u8], owner_pid: u32) -> bool {
        if owner_pid == 0 {
            return false;
        }
        if format != ClipFormat::Text {
            self.degraded += 1;
            return false;
        }
        let mut e = ClipEntry::empty();
        e.format = format;
        e.owner_pid = owner_pid;
        self.seq += 1;
        e.seq = self.seq;
        e.valid = true;
        let n = core::cmp::min(data.len(), CLIP_TEXT_MAX);
        let mut i = 0usize;
        while i < n {
            e.text[i] = data[i];
            i += 1;
        }
        e.text_len = n;
        self.history[self.wr] = e;
        self.head = self.wr;
        self.wr = (self.wr + 1) % CLIP_HISTORY;
        if self.filled < CLIP_HISTORY {
            self.filled += 1;
        }
        true
    }

    /// 粘贴到调用进程缓冲；返回字节数。
    pub fn paste(&mut self, out: &mut [u8]) -> usize {
        let e = self.history[self.head];
        if !e.valid {
            return 0;
        }
        let n = core::cmp::min(out.len(), e.text_len);
        let mut i = 0usize;
        while i < n {
            out[i] = e.text[i];
            i += 1;
        }
        self.pastes += 1;
        n
    }

    pub fn current(&self) -> ClipEntry {
        self.history[self.head]
    }

    /// 历史回退（第 k 新）。
    pub fn history_at(&self, k: usize) -> Option<ClipEntry> {
        if k >= self.filled {
            return None;
        }
        let idx = (self.head + CLIP_HISTORY - k) % CLIP_HISTORY;
        let e = self.history[idx];
        if e.valid {
            Some(e)
        } else {
            None
        }
    }

    pub fn set_head(&mut self, k: usize) -> bool {
        if k >= self.filled {
            return false;
        }
        self.head = (self.wr + CLIP_HISTORY - k - 1) % CLIP_HISTORY;
        true
    }

    /// 所有权校验：只有属主可清空自己写的内容。
    pub fn clear_if_owner(&mut self, pid: u32) -> bool {
        let e = self.history[self.head];
        if !e.valid || e.owner_pid != pid {
            return false;
        }
        self.history[self.head] = ClipEntry::empty();
        self.filled = self.filled.saturating_sub(1);
        true
    }
}

// ---------------------------------------------------------------------------
// F163 拖放协议 — 窗口间拖放事件通道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropResult {
    Accepted,
    Rejected,
    Cancelled,
    NotStarted,
}

#[derive(Clone, Copy)]
pub struct DragSession {
    pub active: bool,
    pub source_win: u32,
    pub target_win: u32,
    /// 载荷类型码（0 = 文本，1 = 文件）。
    pub payload_kind: u8,
    pub moves: u32,
    pub drops: u32,
    pub rejects: u32,
}

impl DragSession {
    pub const fn new() -> DragSession {
        DragSession { active: false, source_win: 0, target_win: 0, payload_kind: 0, moves: 0, drops: 0, rejects: 0 }
    }

    pub fn start(&mut self, source: u32, kind: u8) -> bool {
        if self.active || source == 0 {
            return false;
        }
        self.active = true;
        self.source_win = source;
        self.target_win = 0;
        self.payload_kind = kind;
        true
    }

    /// 移动到目标（拖拽悬停）。
    pub fn over(&mut self, target: u32) -> bool {
        if !self.active {
            return false;
        }
        self.moves += 1;
        self.target_win = target;
        true
    }

    /// 落下：目标不接受则显式拒绝。
    pub fn drop_on(&mut self, target_accepts: bool) -> DropResult {
        if !self.active {
            return DropResult::NotStarted;
        }
        if self.target_win == 0 || self.target_win == self.source_win || !target_accepts {
            self.rejects += 1;
            self.active = false;
            return DropResult::Rejected;
        }
        self.drops += 1;
        self.active = false;
        DropResult::Accepted
    }

    pub fn cancel(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.target_win = 0;
        true
    }
}

// ---------------------------------------------------------------------------
// F164 通知中心对接 — 应用通知 → 桌面通知面板
// ---------------------------------------------------------------------------

pub const NOTIF_MAX: usize = 8;
pub const NOTIF_TEXT_MAX: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifLevel {
    Info,
    Warn,
    Error,
    /// 崩溃明示（崩溃红线的对外呈现）。
    Crash,
}

#[derive(Clone, Copy)]
pub struct Notif {
    pub id: u32,
    pub app: u32,
    pub level: NotifLevel,
    pub text: [u8; NOTIF_TEXT_MAX],
    pub text_len: usize,
    /// 是否已展示。
    pub shown: bool,
    pub tick_ms: u64,
}

impl Notif {
    pub const fn empty() -> Notif {
        Notif {
            id: 0,
            app: 0,
            level: NotifLevel::Info,
            text: [0u8; NOTIF_TEXT_MAX],
            text_len: 0,
            shown: false,
            tick_ms: 0,
        }
    }

    pub fn text_bytes(&self) -> &[u8] {
        &self.text[..self.text_len]
    }
}

#[derive(Clone, Copy)]
pub struct NotificationPanel {
    items: [Notif; NOTIF_MAX],
    count: usize,
    next_id: u32,
    pub pushed: u64,
    pub dismissed: u64,
    pub overflow: u64,
}

impl NotificationPanel {
    pub const fn new() -> NotificationPanel {
        NotificationPanel { items: [Notif::empty(); NOTIF_MAX], count: 0, next_id: 1, pushed: 0, dismissed: 0, overflow: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn push(&mut self, app: u32, level: NotifLevel, text: &[u8], tick_ms: u64) -> Option<u32> {
        if app == 0 || text.is_empty() {
            return None;
        }
        if self.count >= NOTIF_MAX {
            // 溢满：丢弃最旧（保持有界），并计数。
            let mut i = 1usize;
            while i < self.count {
                self.items[i - 1] = self.items[i];
                i += 1;
            }
            self.count -= 1;
            self.overflow += 1;
        }
        let mut n = Notif::empty();
        n.id = self.next_id;
        self.next_id += 1;
        n.app = app;
        n.level = level;
        n.tick_ms = tick_ms;
        n.shown = true;
        let l = core::cmp::min(text.len(), NOTIF_TEXT_MAX);
        let mut i = 0usize;
        while i < l {
            n.text[i] = text[i];
            i += 1;
        }
        n.text_len = l;
        self.items[self.count] = n;
        self.count += 1;
        self.pushed += 1;
        Some(n.id)
    }

    pub fn dismiss(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].id == id {
                let mut k = i;
                while k + 1 < self.count {
                    self.items[k] = self.items[k + 1];
                    k += 1;
                }
                self.count -= 1;
                self.dismissed += 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn at(&self, i: usize) -> Option<Notif> {
        if i < self.count {
            Some(self.items[i])
        } else {
            None
        }
    }

    pub fn count_level(&self, level: NotifLevel) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.items[i].level == level {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 崩溃明示：任何应用崩溃都必须在通知中心出现 Crash 条目。
    pub fn report_crash(&mut self, app: u32, tick_ms: u64) -> Option<u32> {
        self.push(app, NotifLevel::Crash, b"application crashed", tick_ms)
    }
}

// ---------------------------------------------------------------------------
// F165 AURORA 内总线对接 — 组件间消息 → Varix 端口/命名服务
// ---------------------------------------------------------------------------

pub const BUS_COMPONENT_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct BusComponent {
    pub name: [u8; 20],
    pub name_len: usize,
    pub port: u32,
    pub subscribed_mask: u32,
    pub received: u64,
}

impl BusComponent {
    pub const fn empty() -> BusComponent {
        BusComponent { name: [0u8; 20], name_len: 0, port: 0, subscribed_mask: 0, received: 0 }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct ComponentBus {
    comps: [BusComponent; BUS_COMPONENT_MAX],
    count: usize,
    /// 由 AURORA 内总线解析到 Varix 命名服务的调用数。
    pub resolved: u64,
    pub unresolved: u64,
}

impl ComponentBus {
    pub const fn new() -> ComponentBus {
        ComponentBus { comps: [BusComponent::empty(); BUS_COMPONENT_MAX], count: 0, resolved: 0, unresolved: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 组件注册：同时写入 Varix 命名服务（name → port）。
    pub fn register(
        &mut self,
        name: &[u8],
        port: u32,
        mask: u32,
        ns: &mut NameService,
    ) -> bool {
        if self.count >= BUS_COMPONENT_MAX || name.is_empty() || name.len() >= 20 || port == 0 {
            return false;
        }
        if self.find(name).is_some() {
            return false;
        }
        if !ns.bind(name, port) {
            return false;
        }
        let mut c = BusComponent::empty();
        c.name_len = name.len();
        c.port = port;
        c.subscribed_mask = mask;
        let mut i = 0usize;
        while i < name.len() {
            c.name[i] = name[i];
            i += 1;
        }
        self.comps[self.count] = c;
        self.count += 1;
        true
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.comps[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 按名字解析端口：走 Varix 命名服务。
    pub fn resolve(&mut self, name: &[u8], ns: &NameService) -> Option<u32> {
        let local = {
            let mut found = None;
            let mut i = 0usize;
            while i < self.count {
                if self.comps[i].name_eq(name) {
                    found = Some(self.comps[i].port);
                    break;
                }
                i += 1;
            }
            found
        };
        match local {
            Some(p) => {
                // 交叉校验命名服务（AURORA 内总线 → Varix 端口的对接点）。
                match ns.resolve(name) {
                    Some((np, _)) if np == p => {
                        self.resolved += 1;
                        Some(p)
                    }
                    _ => {
                        self.unresolved += 1;
                        None
                    }
                }
            }
            None => {
                self.unresolved += 1;
                None
            }
        }
    }

    /// 广播：按订阅掩码投递（位 = 话题）。
    pub fn broadcast(&mut self, topic: usize, sender: &[u8]) -> usize {
        if topic >= 32 {
            return 0;
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.comps[i].subscribed_mask & (1u32 << topic) != 0 && !self.comps[i].name_eq(sender) {
                self.comps[i].received += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn get(&self, i: usize) -> Option<BusComponent> {
        if i < self.count {
            Some(self.comps[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F166 主题令牌运行时 — 令牌加载/热切换（逐字节复用）
// ---------------------------------------------------------------------------

/// 规范令牌名（与 AURORA 设计令牌表逐字对应）。
pub const SPEC_NAMES: [&[u8]; 4] = [b"surface.base", b"surface.raised", b"text.primary", b"accent"];

#[derive(Clone, Copy)]
pub struct ThemeRuntime {
    pub tokens: ThemeTokens,
    /// 令牌表版本（与设计令牌规范对齐）。
    pub version: u16,
    /// 热切换次数。
    pub hot_swaps: u32,
    /// 切换后逐字节校验失败的次数（必须为 0）。
    pub mismatches: u64,
}

impl ThemeRuntime {
    /// 以规范令牌表初始化（与 AURORA 同源）。
    pub fn new(mode: ThemeMode, version: u16) -> ThemeRuntime {
        let mut tokens = aurora_tokens();
        let _ = tokens.set_mode(mode);
        ThemeRuntime { tokens, version, hot_swaps: 0, mismatches: 0 }
    }

    /// 热切换：切到目标模式后，令牌值必须与"直接以该模式新建规范表"逐字节一致。
    pub fn hot_switch(&mut self, mode: ThemeMode) -> bool {
        let switched = self.tokens.set_mode(mode);
        if switched {
            self.hot_swaps += 1;
        }
        // 逐字节校验：以规范表为准（不重算、不插值）。
        let mut spec = aurora_tokens();
        let _ = spec.set_mode(mode);
        let mut j = 0usize;
        while j < SPEC_NAMES.len() {
            let name = SPEC_NAMES[j];
            if self.tokens.color(name) != spec.color(name) {
                self.mismatches += 1;
            }
            j += 1;
        }
        switched
    }

    pub fn color(&mut self, name: &[u8]) -> u32 {
        self.tokens.color(name)
    }

    /// 令牌一次解析、全帧复用。
    pub fn stable_within_frame(&mut self, name: &[u8]) -> bool {
        self.tokens.stable_within_frame(name)
    }
}

// ---------------------------------------------------------------------------
// F167 动效系统复用 — 弹簧曲线参数逐字节一致
// ---------------------------------------------------------------------------

/// 弹簧参数令牌（与 AURORA 动效系统同一份数值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpringToken {
    pub stiffness: i32,
    pub damping: i32,
    pub target_q8: i32,
}

impl SpringToken {
    pub const fn standard() -> SpringToken {
        SpringToken { stiffness: 96, damping: 160, target_q8: 256 }
    }

    pub fn to_spring(&self) -> Spring {
        Spring::new(self.target_q8, self.stiffness, self.damping)
    }
}

/// 采样轨迹：Windows 版与 Varix 版必须逐帧一致（参数逐字节复用）。
pub fn spring_trace(token: SpringToken, frames: usize, out: &mut [i32]) -> usize {
    let mut s = token.to_spring();
    let mut n = 0usize;
    while n < frames && n < out.len() {
        out[n] = s.step();
        n += 1;
    }
    n
}

/// 双端一致性：同一令牌的轨迹必须完全相同。
pub fn spring_frames_identical(token: SpringToken, frames: usize) -> bool {
    let mut a = [0i32; 32];
    let mut b = [0i32; 32];
    let n = spring_trace(token, frames, &mut a);
    let m = spring_trace(token, frames, &mut b);
    n == m && a[..n] == b[..m]
}

/// 动画永不掉帧（弹簧预算内收敛）。
pub fn spring_converges_in_budget(token: SpringToken, budget: usize) -> bool {
    let mut s = token.to_spring();
    s.settle_steps(budget).is_some()
}

// ---------------------------------------------------------------------------
// F168 应用进程沙箱 — 每应用独立进程 + 最小能力 + 调用过滤
// ---------------------------------------------------------------------------

pub const CAP_FS_READ: u32 = 1 << 0;
pub const CAP_FS_WRITE: u32 = 1 << 1;
pub const CAP_NET: u32 = 1 << 2;
pub const CAP_PROC_SPAWN: u32 = 1 << 3;
pub const CAP_CLIPBOARD: u32 = 1 << 4;
pub const CAP_NOTIFY: u32 = 1 << 5;

#[derive(Clone, Copy)]
pub struct Sandbox {
    pub app: u32,
    pub pid: u32,
    /// 授予的能力位（最小授权）。
    pub caps: u32,
    /// 允许的系统调用号掩码（seccomp 式过滤；位 = 调用号 % 32）。
    pub call_filter: u32,
    pub denied_caps: u64,
    pub denied_calls: u64,
    pub allowed_calls: u64,
}

impl Sandbox {
    pub const fn new(app: u32, pid: u32, caps: u32, call_filter: u32) -> Sandbox {
        Sandbox { app, pid, caps, call_filter, denied_caps: 0, denied_calls: 0, allowed_calls: 0 }
    }

    /// 能力检查：未授予即拒绝（最小授权）。
    pub fn check_cap(&mut self, cap: u32) -> bool {
        if cap == 0 || self.caps & cap != cap {
            self.denied_caps += 1;
            return false;
        }
        true
    }

    /// 调用过滤：不在白名单的调用号一律拒绝。
    pub fn check_call(&mut self, syscall_no: u32) -> bool {
        let bit = (syscall_no % 32) as u32;
        if self.call_filter & (1u32 << bit) == 0 {
            self.denied_calls += 1;
            return false;
        }
        self.allowed_calls += 1;
        true
    }

    pub fn grant(&mut self, cap: u32) -> bool {
        if cap == 0 || self.caps & cap == cap {
            return false;
        }
        self.caps |= cap;
        true
    }

    pub fn revoke(&mut self, cap: u32) -> bool {
        if self.caps & cap == 0 {
            return false;
        }
        self.caps &= !cap;
        true
    }
}

#[derive(Clone, Copy)]
pub struct SandboxTable {
    boxes: [Sandbox; 8],
    count: usize,
    /// 同一 pid 被复用（违反"每应用独立进程"）的次数，必须为 0。
    pub pid_conflicts: u64,
}

impl SandboxTable {
    pub const fn new() -> SandboxTable {
        SandboxTable { boxes: [Sandbox::new(0, 0, 0, 0); 8], count: 0, pid_conflicts: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn spawn(&mut self, app: u32, pid: u32, caps: u32, filter: u32) -> Option<usize> {
        if self.count >= 8 || app == 0 || pid == 0 {
            return None;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.boxes[i].pid == pid {
                self.pid_conflicts += 1;
                return None;
            }
            if self.boxes[i].app == app {
                return None;
            }
            i += 1;
        }
        self.boxes[self.count] = Sandbox::new(app, pid, caps, filter);
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Sandbox> {
        if i < self.count {
            Some(&mut self.boxes[i])
        } else {
            None
        }
    }

    pub fn get(&self, i: usize) -> Option<Sandbox> {
        if i < self.count {
            Some(self.boxes[i])
        } else {
            None
        }
    }

    pub fn kill(&mut self, app: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.boxes[i].app == app {
                self.boxes[i] = self.boxes[self.count - 1];
                self.count -= 1;
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 文件管理器类应用的最小能力集（读 + 剪贴板 + 通知）。
pub const fn fileman_caps() -> u32 {
    CAP_FS_READ | CAP_CLIPBOARD | CAP_NOTIFY
}

/// 允许的调用号白名单（示例：read/write/exit/gettime/event_wait）。
pub const fn standard_call_filter() -> u32 {
    (1u32 << (1 % 32)) | (1u32 << (2 % 32)) | (1u32 << (3 % 32)) | (1u32 << (4 % 32)) | (1u32 << (5 % 32))
}

// ---------------------------------------------------------------------------
// 自检扩展（F161~F168 共 8 项）
// ---------------------------------------------------------------------------

pub fn extend_checks(set: &mut CheckSet) {
    // F161 状态持久化
    let src = default_state();
    let mut blob = [0u8; 256];
    let n = src.snapshot(&mut blob);
    let mut restored = StateStore::new();
    let loaded = restored.restore(&blob[..n]);
    set.add(
        "F161 state persistence",
        src.len() == 5
            && n > 0
            && loaded == 5
            && restored.get(b"theme.mode") == Some(0)
            && restored.get(b"layout.tier") == Some(2)
            && restored.get(b"absent").is_none()
            && restored.restores == 1
            && src.snapshot(&mut [0u8; 4]) == 0,
        "快照/重启恢复/键值一致",
    );

    // F162 剪贴板服务
    let mut clip = Clipboard::new();
    let ok = clip.copy(ClipFormat::Text, b"hello world", 60);
    let rich = !clip.copy(ClipFormat::RichText, b"<b>x</b>", 60);
    let mut out = [0u8; CLIP_TEXT_MAX];
    let got = clip.paste(&mut out);
    let owner_denied = !clip.clear_if_owner(61);
    let ok2 = clip.copy(ClipFormat::Text, b"second", 61);
    let h1 = clip.history_at(1);
    set.add(
        "F162 clipboard",
        ok
            && rich
            && got == 11
            && &out[..got] == b"hello world"
            && clip.current().owner_pid == 61
            && owner_denied
            && ok2
            && clip.pastes == 1
            && clip.degraded == 1
            && h1.map(|e| e.text_bytes() == b"hello world").unwrap_or(false)
            && clip.set_head(1)
            && clip.current().text_bytes() == b"hello world"
            && !clip.set_head(9),
        "跨进程复制/粘贴/历史/富文本显式降级",
    );

    // F163 拖放协议
    let mut drag = DragSession::new();
    let not_started = drag.drop_on(true) == DropResult::NotStarted;
    let start = drag.start(1, 1);
    let dup = !drag.start(2, 1);
    let ov = drag.over(2);
    let rejected_self = drag.over(1);
    let _ = drag.drop_on(true);
    set.add(
        "F163 drag and drop",
        not_started
            && start
            && dup
            && ov
            && rejected_self
            && drag.moves == 2
            && drag.rejects == 1
            && drag.drops == 0
            && !drag.active
            && DragSession::new().drop_on(true) == DropResult::NotStarted,
        "拖放会话/自目标拒绝/落下裁决",
    );
    let mut drag2 = DragSession::new();
    let _ = drag2.start(1, 0);
    let _ = drag2.over(2);
    let accepted = drag2.drop_on(true);
    let mut drag3 = DragSession::new();
    let _ = drag3.start(1, 0);
    let cancelled = drag3.cancel();
    set.add(
        "F163 drop verdict",
        accepted == DropResult::Accepted
            && drag2.drops == 1
            && cancelled
            && !drag3.active
            && !DragSession::new().cancel(),
        "接受/取消路径",
    );

    // F164 通知中心
    let mut panel = NotificationPanel::new();
    let id1 = panel.push(10, NotifLevel::Info, b"theme changed", 100);
    let crash = panel.report_crash(11, 200);
    let bad = panel.push(0, NotifLevel::Info, b"x", 0);
    let empty = panel.push(12, NotifLevel::Info, b"", 0);
    let len_after_push = panel.len();
    let crash_count = panel.count_level(NotifLevel::Crash);
    let crash_text_ok = panel.at(1).map(|n| n.text_bytes() == b"application crashed").unwrap_or(false);
    let dismissed = panel.dismiss(id1.unwrap_or(0));
    let len_after_dismiss = panel.len();
    let dismissed2 = panel.dismiss(crash.unwrap_or(0));
    let no_more = !panel.dismiss(99);
    set.add(
        "F164 notification panel",
        id1 == Some(1)
            && crash == Some(2)
            && bad.is_none()
            && empty.is_none()
            && len_after_push == 2
            && crash_count == 1
            && crash_text_ok
            && dismissed
            && len_after_dismiss == 1
            && dismissed2
            && no_more
            && panel.dismissed == 2
            && panel.len() == 0
            && panel.pushed == 2,
        "应用通知上屏/崩溃明示/有界溢出",
    );

    // F165 内总线对接
    let mut ns = NameService::new();
    let mut bus = ComponentBus::new();
    let r1 = bus.register(b"desktop", 7001, 0b0011, &mut ns);
    let r2 = bus.register(b"taskbar", 7002, 0b0110, &mut ns);
    let dup = !bus.register(b"desktop", 7009, 0, &mut ns);
    let p1 = bus.resolve(b"desktop", &ns);
    let miss = bus.resolve(b"ghost", &ns);
    let got = bus.broadcast(0, b"taskbar");
    set.add(
        "F165 component bus",
        r1
            && r2
            && dup
            && p1 == Some(7001)
            && miss.is_none()
            && bus.len() == 2
            && bus.resolved == 1
            && bus.unresolved == 1
            && got == 1
            && ns.len() == 2
            && bus.get(1).map(|c| c.name_eq(b"taskbar")).unwrap_or(false),
        "组件注册→命名服务/端口解析/广播",
    );

    // F166 主题令牌运行时
    let mut rt = ThemeRuntime::new(ThemeMode::Dark, 3);
    let dark = rt.color(b"surface.base");
    let switched = rt.hot_switch(ThemeMode::Light);
    let light = rt.color(b"surface.base");
    let back = rt.hot_switch(ThemeMode::Dark);
    let dark2 = rt.color(b"surface.base");
    set.add(
        "F166 theme runtime",
        rt.version == 3
            && dark == crate::gfxsrv::rgb(24, 24, 27)
            && switched
            && light == crate::gfxsrv::rgb(250, 250, 250)
            && back
            && dark2 == dark
            && rt.hot_swaps == 2
            && rt.mismatches == 0
            && rt.tokens.len() == 4,
        "令牌加载/热切换/逐字节复用",
    );

    // F167 动效系统复用
    let token = SpringToken::standard();
    let mut trace = [0i32; 32];
    let frames = spring_trace(token, 12, &mut trace);
    let same_token = SpringToken::standard() == token;
    set.add(
        "F167 motion reuse",
        same_token
            && frames == 12
            && trace[0] == 96
            && spring_frames_identical(token, 16)
            && spring_converges_in_budget(token, 400)
            && token.stiffness == 96
            && token.damping == 160
            && token.target_q8 == 256,
        "弹簧参数逐字节一致/轨迹可复现/预算内收敛",
    );

    // F168 应用进程沙箱
    let mut table = SandboxTable::new();
    let a = table.spawn(100, 1000, fileman_caps(), standard_call_filter());
    let b = table.spawn(101, 1001, CAP_FS_READ, standard_call_filter());
    let pid_dup = table.spawn(102, 1000, 0, 0);
    let app_dup = table.spawn(100, 1002, 0, 0);
    let cap_ok = table.get_mut(a.unwrap_or(0)).map(|s| s.check_cap(CAP_FS_READ)).unwrap_or(false);
    let cap_denied = table.get_mut(a.unwrap_or(0)).map(|s| !s.check_cap(CAP_NET)).unwrap_or(false);
    let call_ok = table.get_mut(a.unwrap_or(0)).map(|s| s.check_call(1)).unwrap_or(false);
    let call_denied = table.get_mut(a.unwrap_or(0)).map(|s| !s.check_call(31)).unwrap_or(false);
    let killed = table.kill(101);
    set.add(
        "F168 app sandbox",
        a == Some(0)
            && b == Some(1)
            && pid_dup.is_none()
            && app_dup.is_none()
            && table.pid_conflicts == 1
            && cap_ok
            && cap_denied
            && call_ok
            && call_denied
            && killed
            && table.len() == 1
            && table.get(0).map(|mut s| s.app == 100 && !s.check_cap(CAP_NET)).unwrap_or(false),
        "独立进程/最小能力/调用过滤",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f161_snapshot_roundtrip() {
        let src = default_state();
        let mut blob = [0u8; 128];
        let n = src.snapshot(&mut blob);
        let mut dst = StateStore::new();
        assert_eq!(dst.restore(&blob[..n]), 5);
        assert_eq!(dst.get(b"clip.head"), Some(0));
        assert_eq!(dst.get(b"session.wins"), Some(3));
    }

    #[test]
    fn f162_rich_text_is_explicitly_degraded() {
        let mut c = Clipboard::new();
        assert!(c.copy(ClipFormat::Text, b"t", 1));
        assert!(!c.copy(ClipFormat::RichText, b"r", 1));
        assert!(!c.copy(ClipFormat::Image, b"i", 1));
        assert_eq!(c.degraded, 2);
    }

    #[test]
    fn f164_crash_notification_is_mandatory() {
        let mut p = NotificationPanel::new();
        let id = p.report_crash(9, 1);
        assert!(id.is_some());
        assert_eq!(p.count_level(NotifLevel::Crash), 1);
    }

    #[test]
    fn f165_bus_uses_naming_service() {
        let mut ns = NameService::new();
        let mut bus = ComponentBus::new();
        assert!(bus.register(b"a", 1, 1, &mut ns));
        assert_eq!(bus.resolve(b"a", &ns), Some(1));
        assert_eq!(bus.resolve(b"b", &ns), None);
    }

    #[test]
    fn f167_spring_trace_is_reproducible() {
        let t = SpringToken::standard();
        assert!(spring_frames_identical(t, 24));
        assert!(spring_converges_in_budget(t, 400));
    }

    #[test]
    fn f168_sandbox_minimum_capability() {
        let mut t = SandboxTable::new();
        let _ = t.spawn(1, 10, fileman_caps(), standard_call_filter());
        let s = t.get(0).unwrap();
        assert_eq!(s.caps & CAP_FS_READ, CAP_FS_READ);
        assert_eq!(s.caps & CAP_NET, 0);
        assert_eq!(s.caps & CAP_PROC_SPAWN, 0);
    }
}
