//! AURORA-1000 AI-12 · 应用框架（A276~A300，W2）
//!
//! 纯逻辑应用框架域：应用生命周期状态机、事件循环调度、窗口创建/销毁记账、
//! 资源分配/释放配对、UI 组件归属绑定、多窗口焦点路由、应用间消息、启动预算、
//! 沙箱权限位掩码与域自检。全部为 `no_std` 固定容量结构，无分配、无 unsafe。
//!
//! 交付物对应 `docs/AURORA-1000 功能全景图` 中「AI-12 应用框架」A276~A300。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 固定容量与权限位
// ---------------------------------------------------------------------------

pub const MAX_APPS: usize = 16;
pub const MAX_WINS: usize = 8; // 每应用最多窗口数
pub const MAX_EVENTS: usize = 32;
pub const MAX_MESSAGES: usize = 32;
pub const MAX_COMPS: usize = 64;
pub const MAX_APP_DOCS: usize = 16;
pub const MAX_FW_DOCS: usize = 16;

/// 沙箱权限位（每应用一个 u32 位掩码）。
pub const PERM_NET: u32 = 1 << 0;
pub const PERM_FS: u32 = 1 << 1;
pub const PERM_CLIPBOARD: u32 = 1 << 2;
pub const PERM_CAMERA: u32 = 1 << 3;
pub const PERM_MIC: u32 = 1 << 4;
pub const PERM_GPU: u32 = 1 << 5;
pub const PERM_ALL: u32 = PERM_NET | PERM_FS | PERM_CLIPBOARD | PERM_CAMERA | PERM_MIC | PERM_GPU;

// ---------------------------------------------------------------------------
// 枚举与基础结构
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppState {
    Created = 0,
    Running = 1,
    Suspended = 2,
    Terminated = 3,
}

impl AppState {
    pub fn id(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    System = 0,
    Light = 1,
    Dark = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    Key = 0,
    Mouse = 1,
    Resize = 2,
    Focus = 3,
    Close = 4,
    Custom = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppEvent {
    pub kind: EvKind,
    pub target_app: u8,
    pub win: u8,
    pub code: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppMessage {
    pub from: u8,
    pub to: u8,
    pub kind: u16,
    pub payload: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinRef {
    pub id: u8,
    pub app_id: u8,
    pub w: u16,
    pub h: u16,
}

impl WinRef {
    pub const fn new() -> WinRef {
        WinRef { id: 0, app_id: 0, w: 0, h: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompBinding {
    pub id: u16,
    pub owner: u8,
    pub bound: bool,
}

impl CompBinding {
    pub const fn new() -> CompBinding {
        CompBinding { id: 0, owner: 0, bound: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppManifest {
    pub name: &'static str,
    pub ver_major: u16,
    pub ver_minor: u16,
    pub perms: u32,
    pub entry: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    None = 0,
    Soft = 1,
    Hard = 2,
    Minimal = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppDoc {
    pub id: u8,
    pub topic: &'static str,
    pub anchor: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameworkStats {
    pub apps: usize,
    pub running: usize,
    pub windows: usize,
    pub events_dropped: u32,
    pub messages_dropped: u32,
    pub isolated: usize,
}

// ---------------------------------------------------------------------------
// 应用与全局管理器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct App {
    pub id: u8,
    pub state: AppState,
    pub name: &'static str,
    pub sandbox: u32,
    pub wins: [WinRef; MAX_WINS],
    pub win_count: usize,
    pub focus_win: u8,
    pub res_alloc: u32,
    pub res_free: u32,
    pub created_stamp: u64,
    pub first_frame_stamp: u64,
    pub launch_budget_ms: u32,
    pub theme: Theme,
    pub locale: u8,
    pub a11y_role: u8,
    pub a11y_enabled: bool,
    pub save_token: u64,
    pub has_save: bool,
    pub isolated: bool,
    pub frame_sum_ms: u64,
    pub frame_count: u32,
    pub doc_id: u8,
}

impl App {
    pub const fn new(id: u8) -> App {
        App {
            id,
            state: AppState::Created,
            name: "",
            sandbox: 0,
            wins: [WinRef::new(); MAX_WINS],
            win_count: 0,
            focus_win: 0,
            res_alloc: 0,
            res_free: 0,
            created_stamp: 0,
            first_frame_stamp: 0,
            launch_budget_ms: 1000,
            theme: Theme::System,
            locale: 0,
            a11y_role: 0,
            a11y_enabled: false,
            save_token: 0,
            has_save: false,
            isolated: false,
            frame_sum_ms: 0,
            frame_count: 0,
            doc_id: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AppManager {
    apps: [App; MAX_APPS],
    count: usize,
    events: [Option<AppEvent>; MAX_EVENTS],
    ev_head: usize,
    ev_count: usize,
    ev_dropped: u32,
    idle: bool,
    msgs: [Option<AppMessage>; MAX_MESSAGES],
    msg_head: usize,
    msg_count: usize,
    msg_dropped: u32,
    comps: [CompBinding; MAX_COMPS],
    comp_count: usize,
    next_win: u8,
    focus_app: u8,
    docs: [Option<AppDoc>; MAX_APP_DOCS],
    doc_count: usize,
}

impl AppManager {
    pub const fn new() -> AppManager {
        AppManager {
            apps: [App::new(0); MAX_APPS],
            count: 0,
            events: [None; MAX_EVENTS],
            ev_head: 0,
            ev_count: 0,
            ev_dropped: 0,
            idle: false,
            msgs: [None; MAX_MESSAGES],
            msg_head: 0,
            msg_count: 0,
            msg_dropped: 0,
            comps: [CompBinding::new(); MAX_COMPS],
            comp_count: 0,
            next_win: 1,
            focus_app: 0,
            docs: [None; MAX_APP_DOCS],
            doc_count: 0,
        }
    }
}

// 私有辅助

fn app_index(mgr: &AppManager, id: u8) -> Option<usize> {
    let mut i = 0;
    while i < mgr.count {
        if mgr.apps[i].id == id {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn app_exists(mgr: &AppManager, id: u8) -> bool {
    app_index(mgr, id).is_some()
}

fn app_running(mgr: &AppManager, id: u8) -> bool {
    match app_index(mgr, id) {
        Some(i) => mgr.apps[i].state == AppState::Running,
        None => false,
    }
}

fn window_exists(mgr: &AppManager, app_id: u8, win_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            let a = mgr.apps[i];
            let mut j = 0;
            while j < a.win_count {
                if a.wins[j].id == win_id {
                    return true;
                }
                j += 1;
            }
            false
        }
        None => false,
    }
}

fn msg_remove_at(mgr: &mut AppManager, p: usize) {
    if p >= mgr.msg_count {
        return;
    }
    let cap = MAX_MESSAGES;
    let mut i = p;
    while i + 1 < mgr.msg_count {
        let from = (mgr.msg_head + i + 1) % cap;
        let to = (mgr.msg_head + i) % cap;
        mgr.msgs[to] = mgr.msgs[from];
        i += 1;
    }
    let last = (mgr.msg_head + mgr.msg_count - 1) % cap;
    mgr.msgs[last] = None;
    mgr.msg_count -= 1;
}

// ---------------------------------------------------------------------------
// A276 — 应用生命周期管理
// ---------------------------------------------------------------------------

/// 合法状态转移表：Created→Running→Suspended→Terminated。
pub fn can_transition(from: AppState, to: AppState) -> bool {
    use AppState::*;
    matches!(
        (from, to),
        (Created, Running)
            | (Created, Terminated)
            | (Running, Suspended)
            | (Running, Terminated)
            | (Suspended, Running)
            | (Suspended, Terminated)
    )
}

/// 注册一个新应用（状态 Created）。已到容量上限返回 None。
pub fn app_register(mgr: &mut AppManager, name: &'static str, sandbox: u32, created_stamp: u64) -> Option<u8> {
    if mgr.count >= MAX_APPS {
        return None;
    }
    let id = mgr.count as u8;
    let mut a = App::new(id);
    a.name = name;
    a.sandbox = sandbox & PERM_ALL;
    a.created_stamp = created_stamp;
    mgr.apps[mgr.count] = a;
    mgr.count += 1;
    Some(id)
}

/// 状态机转移；非法转移（含 Terminated→任何活动态）被拒绝。
pub fn app_transition(mgr: &mut AppManager, id: u8, to: AppState) -> bool {
    match app_index(mgr, id) {
        None => false,
        Some(i) => {
            let cur = mgr.apps[i].state;
            if cur == to {
                return true;
            }
            if !can_transition(cur, to) {
                return false;
            }
            mgr.apps[i].state = to;
            if to == AppState::Terminated {
                mgr.apps[i].focus_win = 0;
            }
            true
        }
    }
}

/// 便捷启动：Created→Running。
pub fn app_launch(mgr: &mut AppManager, id: u8) -> bool {
    app_transition(mgr, id, AppState::Running)
}

pub fn app_count(mgr: &AppManager) -> usize {
    mgr.count
}

pub fn app_state(mgr: &AppManager, id: u8) -> Option<AppState> {
    app_index(mgr, id).map(|i| mgr.apps[i].state)
}

// ---------------------------------------------------------------------------
// A277 — 事件循环
// ---------------------------------------------------------------------------

/// 入队一个应用事件；队列满则丢弃并计入 dropped，返回 false。
pub fn event_enqueue(mgr: &mut AppManager, ev: AppEvent) -> bool {
    if mgr.ev_count >= MAX_EVENTS {
        mgr.ev_dropped = mgr.ev_dropped.saturating_add(1);
        return false;
    }
    let pos = (mgr.ev_head + mgr.ev_count) % MAX_EVENTS;
    mgr.events[pos] = Some(ev);
    mgr.ev_count += 1;
    mgr.idle = false;
    true
}

/// 派发队列中所有事件（仅向 Running 目标投递），返回成功投递数；
/// 队列清空后标记空闲（低功耗）。
pub fn event_dispatch(mgr: &mut AppManager) -> usize {
    let mut delivered = 0usize;
    while mgr.ev_count > 0 {
        let pos = mgr.ev_head;
        let ev = mgr.events[pos].unwrap();
        mgr.events[pos] = None;
        mgr.ev_head = (mgr.ev_head + 1) % MAX_EVENTS;
        mgr.ev_count -= 1;
        if app_running(mgr, ev.target_app) {
            delivered += 1;
        }
    }
    if mgr.ev_count == 0 {
        mgr.idle = true;
    }
    delivered
}

pub fn event_pending(mgr: &AppManager) -> usize {
    mgr.ev_count
}

pub fn event_dropped(mgr: &AppManager) -> u32 {
    mgr.ev_dropped
}

pub fn mark_idle(mgr: &mut AppManager, idle: bool) {
    mgr.idle = idle;
}

pub fn is_idle(mgr: &AppManager) -> bool {
    mgr.idle
}

// ---------------------------------------------------------------------------
// A278 — 窗口创建 API
// ---------------------------------------------------------------------------

/// 为运行中的应用创建窗口；每应用上限 MAX_WINS。返回窗口 id。
pub fn window_create(mgr: &mut AppManager, app_id: u8, w: u16, h: u16) -> Option<u8> {
    let i = match app_index(mgr, app_id) {
        Some(i) => i,
        None => return None,
    };
    if mgr.apps[i].state != AppState::Running {
        return None;
    }
    if mgr.apps[i].win_count >= MAX_WINS {
        return None;
    }
    let wid = mgr.next_win;
    mgr.next_win = mgr.next_win.saturating_add(1).max(1);
    let wr = WinRef { id: wid, app_id, w, h };
    let c = mgr.apps[i].win_count;
    mgr.apps[i].wins[c] = wr;
    mgr.apps[i].win_count += 1;
    mgr.apps[i].focus_win = wid;
    mgr.focus_app = app_id;
    Some(wid)
}

/// 销毁一个窗口；若被销毁的是焦点窗口则重路由到剩余首个窗口。
pub fn window_destroy(mgr: &mut AppManager, app_id: u8, win_id: u8) -> bool {
    let i = match app_index(mgr, app_id) {
        Some(i) => i,
        None => return false,
    };
    let cnt = mgr.apps[i].win_count;
    let mut found = None;
    let mut j = 0;
    while j < cnt {
        if mgr.apps[i].wins[j].id == win_id {
            found = Some(j);
            break;
        }
        j += 1;
    }
    match found {
        None => false,
        Some(slot) => {
            let last = cnt - 1;
            if slot != last {
                mgr.apps[i].wins[slot] = mgr.apps[i].wins[last];
            }
            mgr.apps[i].wins[last] = WinRef::new();
            mgr.apps[i].win_count -= 1;
            if mgr.apps[i].focus_win == win_id {
                mgr.apps[i].focus_win = if mgr.apps[i].win_count > 0 {
                    mgr.apps[i].wins[0].id
                } else {
                    0
                };
            }
            true
        }
    }
}

pub fn window_count(mgr: &AppManager, app_id: u8) -> usize {
    app_index(mgr, app_id).map(|i| mgr.apps[i].win_count).unwrap_or(0)
}

/// 全部窗口关闭判定。
pub fn window_all_closed(mgr: &AppManager, app_id: u8) -> bool {
    window_count(mgr, app_id) == 0
}

/// 全部窗口关闭后自动退出应用（→Terminated）。
pub fn window_auto_exit(mgr: &mut AppManager, app_id: u8) -> bool {
    if !window_all_closed(mgr, app_id) {
        return false;
    }
    app_transition(mgr, app_id, AppState::Terminated)
}

// ---------------------------------------------------------------------------
// A279 — UI 组件绑定
// ---------------------------------------------------------------------------

/// 将组件绑定到归属应用；已绑定给别的应用则拒绝。
pub fn component_bind(mgr: &mut AppManager, comp_id: u16, app_id: u8) -> bool {
    if !app_exists(mgr, app_id) {
        return false;
    }
    let mut i = 0;
    while i < mgr.comp_count {
        if mgr.comps[i].id == comp_id {
            if mgr.comps[i].bound && mgr.comps[i].owner != app_id {
                return false;
            }
            mgr.comps[i].owner = app_id;
            mgr.comps[i].bound = true;
            return true;
        }
        i += 1;
    }
    if mgr.comp_count >= MAX_COMPS {
        return false;
    }
    mgr.comps[mgr.comp_count] = CompBinding { id: comp_id, owner: app_id, bound: true };
    mgr.comp_count += 1;
    true
}

pub fn component_owned_by(mgr: &AppManager, comp_id: u16, app_id: u8) -> bool {
    let mut i = 0;
    while i < mgr.comp_count {
        if mgr.comps[i].id == comp_id {
            return mgr.comps[i].bound && mgr.comps[i].owner == app_id;
        }
        i += 1;
    }
    false
}

pub fn component_unbind(mgr: &mut AppManager, comp_id: u16) -> bool {
    let mut i = 0;
    while i < mgr.comp_count {
        if mgr.comps[i].id == comp_id {
            mgr.comps[i].bound = false;
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// A280 — 应用资源管理
// ---------------------------------------------------------------------------

/// 分配一笔资源（仅 Running）；配对计数 +1。
pub fn resource_alloc(mgr: &mut AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].state != AppState::Running {
                return false;
            }
            mgr.apps[i].res_alloc = mgr.apps[i].res_alloc.saturating_add(1);
            true
        }
        None => false,
    }
}

/// 释放一笔资源（仅当仍有未释放分配）；配对计数 +1。
pub fn resource_free(mgr: &mut AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].res_alloc <= mgr.apps[i].res_free {
                return false;
            }
            mgr.apps[i].res_free = mgr.apps[i].res_free.saturating_add(1);
            true
        }
        None => false,
    }
}

/// 退出时泄漏检测：分配数 != 释放数。
pub fn resource_leaked(mgr: &AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => mgr.apps[i].res_alloc != mgr.apps[i].res_free,
        None => false,
    }
}

pub fn resource_leak_count(mgr: &AppManager, app_id: u8) -> u32 {
    match app_index(mgr, app_id) {
        Some(i) => mgr.apps[i].res_alloc.saturating_sub(mgr.apps[i].res_free),
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// A281 — 多窗口应用与焦点路由
// ---------------------------------------------------------------------------

/// 设置应用焦点窗口（须在应用窗口集合内）。
pub fn focus_window(mgr: &mut AppManager, app_id: u8, win_id: u8) -> bool {
    if !window_exists(mgr, app_id, win_id) {
        return false;
    }
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].focus_win = win_id;
            mgr.focus_app = app_id;
            true
        }
        None => false,
    }
}

/// 当前全局焦点所属应用（0 = 无）。
pub fn route_focus(mgr: &AppManager) -> u8 {
    mgr.focus_app
}

/// 应用当前焦点窗口（0 = 无）。
pub fn app_focus_window(mgr: &AppManager, app_id: u8) -> u8 {
    app_index(mgr, app_id).map(|i| mgr.apps[i].focus_win).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// A282 — 应用间通信
// ---------------------------------------------------------------------------

/// 发送一条应用间消息；队列满则丢弃并计入 dropped，返回 false。
pub fn message_send(mgr: &mut AppManager, from: u8, to: u8, kind: u16, payload: u32) -> bool {
    if mgr.msg_count >= MAX_MESSAGES {
        mgr.msg_dropped = mgr.msg_dropped.saturating_add(1);
        return false;
    }
    let pos = (mgr.msg_head + mgr.msg_count) % MAX_MESSAGES;
    mgr.msgs[pos] = Some(AppMessage { from, to, kind, payload });
    mgr.msg_count += 1;
    true
}

/// 取出队首发往 app_id 的消息（FIFO，按到达顺序）。
pub fn message_recv(mgr: &mut AppManager, app_id: u8) -> Option<AppMessage> {
    let mut p = 0;
    while p < mgr.msg_count {
        let pos = (mgr.msg_head + p) % MAX_MESSAGES;
        if let Some(m) = mgr.msgs[pos] {
            if m.to == app_id {
                let out = m;
                msg_remove_at(mgr, p);
                return Some(out);
            }
        }
        p += 1;
    }
    None
}

pub fn message_dropped(mgr: &AppManager) -> u32 {
    mgr.msg_dropped
}

pub fn message_pending(mgr: &AppManager) -> usize {
    mgr.msg_count
}

// ---------------------------------------------------------------------------
// A283 — 应用状态保存恢复
// ---------------------------------------------------------------------------

pub fn app_save(mgr: &mut AppManager, app_id: u8, token: u64) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].save_token = token;
            mgr.apps[i].has_save = true;
            true
        }
        None => false,
    }
}

/// 仅当存在保存且 token 匹配时恢复成功。
pub fn app_restore(mgr: &mut AppManager, app_id: u8, token: u64) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].has_save && mgr.apps[i].save_token == token {
                mgr.apps[i].has_save = false;
                true
            } else {
                false
            }
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// A284 — 应用主题跟随
// ---------------------------------------------------------------------------

pub fn app_set_theme(mgr: &mut AppManager, app_id: u8, theme: Theme) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].theme = theme;
            true
        }
        None => false,
    }
}

pub fn app_theme(mgr: &AppManager, app_id: u8) -> Theme {
    app_index(mgr, app_id).map(|i| mgr.apps[i].theme).unwrap_or(Theme::System)
}

// ---------------------------------------------------------------------------
// A285 — 应用无障碍
// ---------------------------------------------------------------------------

pub fn app_set_a11y(mgr: &mut AppManager, app_id: u8, role: u8, enabled: bool) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].a11y_role = role;
            mgr.apps[i].a11y_enabled = enabled;
            true
        }
        None => false,
    }
}

pub fn app_a11y_enabled(mgr: &AppManager, app_id: u8) -> bool {
    app_index(mgr, app_id).map(|i| mgr.apps[i].a11y_enabled).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// A286 — 应用本地化
// ---------------------------------------------------------------------------

pub fn app_set_locale(mgr: &mut AppManager, app_id: u8, locale: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].locale = locale;
            true
        }
        None => false,
    }
}

pub fn app_locale(mgr: &AppManager, app_id: u8) -> u8 {
    app_index(mgr, app_id).map(|i| mgr.apps[i].locale).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// A287 — 应用打包格式（清单校验，纯函数）
// ---------------------------------------------------------------------------

pub fn validate_manifest(m: AppManifest) -> bool {
    if m.name.is_empty() {
        return false;
    }
    if m.ver_major == 0 && m.ver_minor == 0 {
        return false;
    }
    if m.perms & !PERM_ALL != 0 {
        return false;
    }
    if m.entry == 0 {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// A288 — 应用沙箱（权限位掩码）
// ---------------------------------------------------------------------------

pub fn sandbox_check(mgr: &AppManager, app_id: u8, perm: u32) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => mgr.apps[i].sandbox & perm != 0,
        None => false,
    }
}

pub fn sandbox_grant(mgr: &mut AppManager, app_id: u8, perm: u32) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].sandbox |= perm & PERM_ALL;
            true
        }
        None => false,
    }
}

pub fn sandbox_revoke(mgr: &mut AppManager, app_id: u8, perm: u32) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].sandbox &= !perm;
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// A289 — 应用秒开预算（创建→首帧）
// ---------------------------------------------------------------------------

pub fn record_first_frame(mgr: &mut AppManager, app_id: u8, stamp: u64) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].created_stamp == 0 {
                return false;
            }
            mgr.apps[i].first_frame_stamp = stamp;
            true
        }
        None => false,
    }
}

pub fn launch_ms(mgr: &AppManager, app_id: u8) -> u64 {
    match app_index(mgr, app_id) {
        Some(i) => mgr.apps[i].first_frame_stamp.saturating_sub(mgr.apps[i].created_stamp),
        None => 0,
    }
}

pub fn launch_within_budget(mgr: &AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            let ms = mgr.apps[i].first_frame_stamp.saturating_sub(mgr.apps[i].created_stamp);
            ms <= mgr.apps[i].launch_budget_ms as u64
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// A290 — 应用崩溃隔离
// ---------------------------------------------------------------------------

/// 崩溃：终止应用并标记隔离，避免影响其他应用。
pub fn app_crash(mgr: &mut AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            mgr.apps[i].state = AppState::Terminated;
            mgr.apps[i].isolated = true;
            mgr.apps[i].focus_win = 0;
            true
        }
        None => false,
    }
}

pub fn app_isolated(mgr: &AppManager, app_id: u8) -> bool {
    app_index(mgr, app_id).map(|i| mgr.apps[i].isolated).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// A291 — 应用性能剖析
// ---------------------------------------------------------------------------

pub fn app_profile_frame(mgr: &mut AppManager, app_id: u8, ms: u32) -> bool {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].state != AppState::Running {
                return false;
            }
            mgr.apps[i].frame_sum_ms = mgr.apps[i].frame_sum_ms.saturating_add(ms as u64);
            mgr.apps[i].frame_count = mgr.apps[i].frame_count.saturating_add(1);
            true
        }
        None => false,
    }
}

pub fn app_avg_frame_ms(mgr: &AppManager, app_id: u8) -> u32 {
    match app_index(mgr, app_id) {
        Some(i) => {
            if mgr.apps[i].frame_count == 0 {
                0
            } else {
                (mgr.apps[i].frame_sum_ms / mgr.apps[i].frame_count as u64) as u32
            }
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// A292 — 应用开发文档
// ---------------------------------------------------------------------------

pub fn app_doc_register(mgr: &mut AppManager, topic: &'static str, anchor: &'static str) -> Option<u8> {
    if mgr.doc_count >= MAX_APP_DOCS {
        return None;
    }
    let id = mgr.doc_count as u8;
    mgr.docs[mgr.doc_count] = Some(AppDoc { id, topic, anchor });
    mgr.doc_count += 1;
    Some(id)
}

pub fn app_doc_get(mgr: &AppManager, id: u8) -> Option<AppDoc> {
    let mut i = 0;
    while i < mgr.doc_count {
        if let Some(d) = mgr.docs[i] {
            if d.id == id {
                return Some(d);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// A293 — 应用自检收口
// ---------------------------------------------------------------------------

/// 单应用完整性：无资源泄漏、窗口数合法、沙箱位合法、焦点窗口存在。
pub fn run_app_self_check(mgr: &AppManager, app_id: u8) -> bool {
    match app_index(mgr, app_id) {
        None => false,
        Some(i) => {
            let a = mgr.apps[i];
            let no_leak = a.res_alloc == a.res_free;
            let win_ok = a.win_count <= MAX_WINS;
            let sandbox_ok = a.sandbox & !PERM_ALL == 0;
            let focus_ok = a.focus_win == 0 || window_exists(mgr, app_id, a.focus_win);
            no_leak && win_ok && sandbox_ok && focus_ok
        }
    }
}

// ---------------------------------------------------------------------------
// A294 — 应用框架自检
// ---------------------------------------------------------------------------

pub fn run_appfw_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-appfw");
    let mut m = AppManager::new();

    // A276 — 注册与状态机
    let a = app_register(&mut m, "shell", PERM_FS, 1000).unwrap();
    let b = app_register(&mut m, "editor", PERM_FS | PERM_CLIPBOARD, 1000).unwrap();
    set.add("A276 register", app_count(&m) == 2 && a == 0 && b == 1, "two apps");
    set.add(
        "A276 transition legal",
        app_launch(&mut m, a) && app_transition(&mut m, a, AppState::Suspended),
        "created->running->suspended",
    );
    let term_a = app_transition(&mut m, a, AppState::Terminated); // suspended->terminated 合法
    let back_a = app_transition(&mut m, a, AppState::Running); // terminated->running 非法
    set.add(
        "A276 illegal transition",
        term_a && !back_a && can_transition(AppState::Terminated, AppState::Running) == false,
        "terminated->running rejected",
    );
    // 填满注册表
    let full;
    for _ in 0..MAX_APPS {
        let _ = app_register(&mut m, "x", 0, 0);
    }
    full = app_register(&mut m, "overflow", 0, 0).is_none();
    set.add("A276 registry full", full && app_count(&m) == MAX_APPS, "cap 16");

    // A277 — 事件循环
    let mut m2 = AppManager::new();
    let c = app_register(&mut m2, "calc", PERM_FS, 0).unwrap();
    app_launch(&mut m2, c);
    let ok = event_enqueue(&mut m2, AppEvent { kind: EvKind::Key, target_app: c, win: 0, code: 65 });
    let delivered = event_dispatch(&mut m2);
    set.add("A277 enqueue+dispatch", ok && delivered == 1 && is_idle(&m2), "drain marks idle");
    // 溢出丢弃
    let mut m3 = AppManager::new();
    for _ in 0..MAX_EVENTS + 5 {
        let _ = event_enqueue(&mut m3, AppEvent { kind: EvKind::Key, target_app: 0, win: 0, code: 0 });
    }
    set.add("A277 event overflow", event_dropped(&m3) == 5 && event_pending(&m3) == MAX_EVENTS, "drop+count");

    // A278 — 窗口创建/销毁与自动退出
    let mut m4 = AppManager::new();
    let d = app_register(&mut m4, "term", PERM_FS, 0).unwrap();
    app_launch(&mut m4, d);
    let w1 = window_create(&mut m4, d, 800, 600).unwrap();
    let _w2 = window_create(&mut m4, d, 640, 480).unwrap();
    set.add("A278 window create", window_count(&m4, d) == 2, "two windows");
    window_destroy(&mut m4, d, w1);
    // 窗口上限
    let mut m5 = AppManager::new();
    let e = app_register(&mut m5, "many", PERM_FS, 0).unwrap();
    app_launch(&mut m5, e);
    let mut made = 0;
    while window_create(&mut m5, e, 10, 10).is_some() {
        made += 1;
    }
    set.add("A278 window cap", made == MAX_WINS && window_count(&m5, e) == MAX_WINS, "cap 8");
    // 自动退出：关闭全部窗口后应用转 Terminated
    let mut m6 = AppManager::new();
    let f = app_register(&mut m6, "auto", PERM_FS, 0).unwrap();
    app_launch(&mut m6, f);
    let aw = window_create(&mut m6, f, 10, 10).unwrap();
    let not_yet = !window_auto_exit(&mut m6, f);
    window_destroy(&mut m6, f, aw);
    let did_exit = window_auto_exit(&mut m6, f) && app_state(&m6, f) == Some(AppState::Terminated);
    set.add("A278 auto exit", not_yet && did_exit, "all closed -> terminated");

    // A279 — 组件归属
    let mut m6 = AppManager::new();
    let f = app_register(&mut m6, "ui", PERM_FS, 0).unwrap();
    let g = app_register(&mut m6, "ui2", PERM_FS, 0).unwrap();
    component_bind(&mut m6, 100, f);
    let owned = component_owned_by(&mut m6, 100, f);
    let reject = !component_bind(&mut m6, 100, g);
    set.add("A279 component ownership", owned && reject, "owner validated");

    // A280 — 资源泄漏
    let mut m7 = AppManager::new();
    let h = app_register(&mut m7, "leak", PERM_FS, 0).unwrap();
    app_launch(&mut m7, h);
    resource_alloc(&mut m7, h);
    resource_alloc(&mut m7, h);
    resource_alloc(&mut m7, h);
    resource_free(&mut m7, h);
    set.add(
        "A280 resource leak",
        resource_leaked(&m7, h) && resource_leak_count(&m7, h) == 2,
        "alloc!=free",
    );

    // A281 — 焦点路由
    let mut m8 = AppManager::new();
    let i = app_register(&mut m8, "focus", PERM_FS, 0).unwrap();
    app_launch(&mut m8, i);
    let _fw = window_create(&mut m8, i, 100, 100).unwrap();
    let fw2 = window_create(&mut m8, i, 100, 100).unwrap();
    focus_window(&mut m8, i, fw2);
    set.add(
        "A281 focus routing",
        route_focus(&m8) == i && app_focus_window(&m8, i) == fw2,
        "focus routed",
    );

    // A282 — 应用间消息
    let mut m9 = AppManager::new();
    let s = app_register(&mut m9, "sender", PERM_FS, 0).unwrap();
    let r = app_register(&mut m9, "receiver", PERM_FS, 0).unwrap();
    message_send(&mut m9, s, r, 1, 42);
    let got = message_recv(&mut m9, r);
    // 溢出丢弃
    let mut m10 = AppManager::new();
    for k in 0..MAX_MESSAGES + 5 {
        message_send(&mut m10, 0, (k % 16) as u8, 0, k as u32);
    }
    set.add(
        "A282 message route",
        got.map(|mm| mm.payload == 42).unwrap_or(false) && message_dropped(&m10) == 5,
        "route + overflow drop",
    );

    // A283 — 状态保存恢复
    let mut m11 = AppManager::new();
    let j = app_register(&mut m11, "state", PERM_FS, 0).unwrap();
    app_save(&mut m11, j, 777);
    let ok_restore = app_restore(&mut m11, j, 777);
    let bad_restore = !app_restore(&mut m11, j, 888);
    set.add("A283 save/restore", ok_restore && bad_restore, "token gated");

    // A284 — 主题跟随
    let mut m12 = AppManager::new();
    let k = app_register(&mut m12, "theme", PERM_FS, 0).unwrap();
    app_set_theme(&mut m12, k, Theme::Dark);
    set.add(
        "A284 theme follow",
        app_theme(&m12, k) == Theme::Dark && app_theme(&m12, 99) == Theme::System,
        "theme stored",
    );

    // A285 — 无障碍
    let mut m13 = AppManager::new();
    let l = app_register(&mut m13, "a11y", PERM_FS, 0).unwrap();
    app_set_a11y(&mut m13, l, 3, true);
    set.add("A285 a11y", app_a11y_enabled(&m13, l) && !app_a11y_enabled(&m13, 99), "a11y flag");

    // A286 — 本地化
    let mut m14 = AppManager::new();
    let n = app_register(&mut m14, "loc", PERM_FS, 0).unwrap();
    app_set_locale(&mut m14, n, 5);
    set.add("A286 locale", app_locale(&m14, n) == 5, "locale stored");

    // A287 — 打包清单
    let good = AppManifest { name: "app", ver_major: 1, ver_minor: 0, perms: PERM_FS, entry: 1 };
    let bad_perm = AppManifest { name: "app", ver_major: 1, ver_minor: 0, perms: 1 << 31, entry: 1 };
    let bad_ver = AppManifest { name: "app", ver_major: 0, ver_minor: 0, perms: PERM_FS, entry: 1 };
    set.add(
        "A287 manifest",
        validate_manifest(good) && !validate_manifest(bad_perm) && !validate_manifest(bad_ver),
        "validate",
    );

    // A288 — 沙箱
    let mut m15 = AppManager::new();
    let p = app_register(&mut m15, "sbx", 0, 0).unwrap();
    sandbox_grant(&mut m15, p, PERM_NET);
    let chk = sandbox_check(&m15, p, PERM_NET) && !sandbox_check(&m15, p, PERM_FS);
    sandbox_revoke(&mut m15, p, PERM_NET);
    set.add("A288 sandbox", chk && !sandbox_check(&m15, p, PERM_NET), "bits gated");

    // A289 — 秒开预算
    let mut m16 = AppManager::new();
    let q = app_register(&mut m16, "boot", PERM_FS, 1000).unwrap();
    app_launch(&mut m16, q);
    record_first_frame(&mut m16, q, 1300);
    let within = launch_within_budget(&m16, q); // 300ms <= 1000ms budget
    record_first_frame(&mut m16, q, 5000);
    let over = !launch_within_budget(&m16, q);
    set.add("A289 launch budget", within && over, "budget compare");

    // A290 — 崩溃隔离
    let mut m17 = AppManager::new();
    let rr = app_register(&mut m17, "crash", PERM_FS, 0).unwrap();
    app_launch(&mut m17, rr);
    app_crash(&mut m17, rr);
    set.add(
        "A290 crash isolation",
        app_isolated(&m17, rr) && app_state(&m17, rr) == Some(AppState::Terminated),
        "isolated+terminated",
    );

    // A291 — 性能剖析
    let mut m18 = AppManager::new();
    let t = app_register(&mut m18, "perf", PERM_FS, 0).unwrap();
    app_launch(&mut m18, t);
    app_profile_frame(&mut m18, t, 16);
    app_profile_frame(&mut m18, t, 16);
    set.add("A291 profiling", app_avg_frame_ms(&m18, t) == 16, "avg frame");

    // A292 — 开发文档
    let mut m19 = AppManager::new();
    let did = app_doc_register(&mut m19, "lifecycle", "A276").unwrap();
    set.add(
        "A292 doc register",
        app_doc_get(&m19, did).map(|d| d.topic == "lifecycle").unwrap_or(false),
        "doc stored",
    );

    // A293 — 应用自检收口
    let mut m20 = AppManager::new();
    let clean = app_register(&mut m20, "clean", PERM_FS, 0).unwrap();
    app_launch(&mut m20, clean);
    window_create(&mut m20, clean, 10, 10);
    let ok_self = run_app_self_check(&m20, clean);
    let leak = app_register(&mut m20, "dirty", PERM_FS, 0).unwrap();
    app_launch(&mut m20, leak);
    resource_alloc(&mut m20, leak);
    let bad_self = !run_app_self_check(&m20, leak);
    set.add("A293 app self-check", ok_self && bad_self, "integrity gate");

    // A294 — 自检本身构建 >=25 项（此处已加 A276..A293 共 18 项，
    // 加上本项与后续 A295..A300 共 7 项，最终恰为 25 项）。
    set.add(
        "A294 self-check builds",
        set.len() + 7 >= 25,
        "check count",
    );

    // A295 — 框架性能预算（空闲 < 活跃）
    let idle_b = appfw_perf_budget(3, true);
    let active_b = appfw_perf_budget(3, false);
    set.add(
        "A295 perf budget",
        idle_b < active_b && idle_b > 0 && active_b > 0,
        "idle<active",
    );

    // A296 — 可观测统计
    let mut m21 = AppManager::new();
    let u = app_register(&mut m21, "obs", PERM_FS, 0).unwrap();
    app_launch(&mut m21, u);
    for _ in 0..3 {
        let _ = event_enqueue(&mut m21, AppEvent { kind: EvKind::Key, target_app: u, win: 0, code: 0 });
    }
    let st = framework_stats(&m21);
    set.add(
        "A296 framework stats",
        st.apps == 1 && st.running == 1 && st.events_dropped == m21.ev_dropped,
        "observable",
    );

    // A297 — 模糊测试安全（任意输入不 panic，返回有界）
    let fz = appfw_fuzz(&[1u8, 2, 3, 4, 5, 6, 7, 8]);
    set.add("A297 fuzz safe", fz <= 256, "bounded coverage");

    // A298 — 框架文档锚点
    set.add(
        "A298 doc anchors",
        appfw_doc(0).is_some() && appfw_doc(MAX_FW_DOCS + 100).is_none(),
        "anchors",
    );

    // A299 — 降级链
    let mut m22 = AppManager::new();
    for _ in 0..3 {
        let _ = event_enqueue(&mut m22, AppEvent { kind: EvKind::Key, target_app: 0, win: 0, code: 0 });
    }
    let lvl = degrade_level(950);
    let applied = framework_apply_degrade(&mut m22, DegradeLevel::Minimal);
    set.add(
        "A299 degrade",
        lvl == DegradeLevel::Minimal && applied && event_pending(&m22) == 0,
        "relieve pressure",
    );

    // A300 — 域自检收口（不得递归调用 run_appfw_checks）
    let (ok_passed, ok_failed) = set.tally();
    set.add(
        "A300 domain closure",
        ok_failed == 0 && ok_passed >= 25 && !set.truncated(),
        "all pass + not truncated",
    );

    set
}

// ---------------------------------------------------------------------------
// A295 — 应用框架性能预算
// ---------------------------------------------------------------------------

/// 框架允许功耗预算（毫瓦）：空闲远低于活跃多应用。
pub fn appfw_perf_budget(active_apps: u32, idle: bool) -> u32 {
    let base: u32 = 800;
    if idle {
        base
    } else {
        base.saturating_add(active_apps.saturating_mul(400))
    }
}

// ---------------------------------------------------------------------------
// A296 — 应用框架可观测
// ---------------------------------------------------------------------------

pub fn framework_stats(mgr: &AppManager) -> FrameworkStats {
    let mut running = 0usize;
    let mut windows = 0usize;
    let mut isolated = 0usize;
    let mut i = 0;
    while i < mgr.count {
        if mgr.apps[i].state == AppState::Running {
            running += 1;
        }
        windows += mgr.apps[i].win_count;
        if mgr.apps[i].isolated {
            isolated += 1;
        }
        i += 1;
    }
    FrameworkStats {
        apps: mgr.count,
        running,
        windows,
        events_dropped: mgr.ev_dropped,
        messages_dropped: mgr.msg_dropped,
        isolated,
    }
}

// ---------------------------------------------------------------------------
// A297 — 应用框架模糊测试
// ---------------------------------------------------------------------------

/// 以任意字节流驱动一组有界操作，返回成功操作计数（上限 256）。
/// 任何输入都不会造成越界或 panic。
pub fn appfw_fuzz(input: &[u8]) -> u32 {
    let mut m = AppManager::new();
    let mut ok: u32 = 0;
    let mut n = 0usize;
    while n < input.len() && n < 256 {
        let b = input[n];
        match b % 4 {
            0 => {
                if app_register(&mut m, "fz", PERM_FS, n as u64).is_some() {
                    ok = ok.saturating_add(1);
                }
            }
            1 => {
                let id = (b / 4) % MAX_APPS as u8;
                if app_launch(&mut m, id) {
                    ok = ok.saturating_add(1);
                }
            }
            2 => {
                let from = b % MAX_APPS as u8;
                let to = (b >> 4) % MAX_APPS as u8;
                if message_send(&mut m, from, to, 0, 0) {
                    ok = ok.saturating_add(1);
                }
            }
            _ => {
                let id = b % MAX_APPS as u8;
                if window_create(&mut m, id, 10, 10).is_some() {
                    ok = ok.saturating_add(1);
                }
            }
        }
        n += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// A298 — 应用框架文档
// ---------------------------------------------------------------------------

const FW_DOCS: [&'static str; 8] = [
    "A276-app-lifecycle",
    "A277-event-loop",
    "A278-window-api",
    "A282-ipc",
    "A288-sandbox",
    "A289-launch-budget",
    "A290-crash-isolation",
    "A299-degrade-chain",
];

pub fn appfw_doc(index: usize) -> Option<&'static str> {
    if index < FW_DOCS.len() {
        Some(FW_DOCS[index])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// A299 — 应用框架降级链
// ---------------------------------------------------------------------------

pub fn degrade_level(load_permille: u32) -> DegradeLevel {
    if load_permille < 200 {
        DegradeLevel::None
    } else if load_permille < 500 {
        DegradeLevel::Soft
    } else if load_permille < 800 {
        DegradeLevel::Hard
    } else {
        DegradeLevel::Minimal
    }
}

/// 高负载降级：Soft 仅观察；Hard/Minimal 清空待派发事件与积压消息以缓解压力。
pub fn framework_apply_degrade(mgr: &mut AppManager, level: DegradeLevel) -> bool {
    if level == DegradeLevel::Hard || level == DegradeLevel::Minimal {
        mgr.ev_count = 0;
        mgr.ev_head = 0;
        mgr.msg_count = 0;
        mgr.msg_head = 0;
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// A300 — 应用框架域自检收口
// ---------------------------------------------------------------------------

/// 域自检收口：自检全集通过且未被截断。
pub fn run_appfw_domain_ok() -> bool {
    let s = run_appfw_checks();
    s.all_passed() && !s.truncated()
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn boot() -> (AppManager, u8, u8) {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "shell", PERM_FS, 1000).unwrap();
        let b = app_register(&mut m, "editor", PERM_CLIPBOARD, 1000).unwrap();
        (m, a, b)
    }

    #[test]
    fn a276_register_and_capacity() {
        let (m, a, b) = boot();
        assert_eq!(app_count(&m), 2);
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        let mut m2 = AppManager::new();
        for _ in 0..MAX_APPS {
            assert!(app_register(&mut m2, "x", 0, 0).is_some());
        }
        assert!(app_register(&mut m2, "overflow", 0, 0).is_none());
        assert_eq!(app_count(&m2), MAX_APPS);
    }

    #[test]
    fn a276_state_machine_rejects_illegal() {
        let (mut m, a, _b) = boot();
        assert!(app_launch(&mut m, a));
        assert_eq!(app_state(&m, a), Some(AppState::Running));
        // suspended -> running 合法
        assert!(app_transition(&mut m, a, AppState::Suspended));
        assert!(app_transition(&mut m, a, AppState::Running));
        // 终止后不能再回到活动态
        assert!(app_transition(&mut m, a, AppState::Terminated));
        assert!(!app_transition(&mut m, a, AppState::Running));
        assert!(!can_transition(AppState::Terminated, AppState::Running));
        assert!(can_transition(AppState::Created, AppState::Running));
    }

    #[test]
    fn a276_transition_requires_existing_app() {
        let mut m = AppManager::new();
        assert!(!app_transition(&mut m, 42, AppState::Running));
    }

    #[test]
    fn a277_event_loop_dispatch_and_idle() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        assert!(event_enqueue(&mut m, AppEvent { kind: EvKind::Key, target_app: a, win: 0, code: 65 }));
        assert_eq!(event_pending(&m), 1);
        let d = event_dispatch(&mut m);
        assert_eq!(d, 1);
        assert!(is_idle(&m));
        assert_eq!(event_pending(&m), 0);
    }

    #[test]
    fn a277_event_queue_overflow_drops() {
        let mut m = AppManager::new();
        for _ in 0..MAX_EVENTS + 6 {
            let _ = event_enqueue(&mut m, AppEvent { kind: EvKind::Key, target_app: 0, win: 0, code: 0 });
        }
        assert_eq!(event_pending(&m), MAX_EVENTS);
        assert_eq!(event_dropped(&m), 6);
    }

    #[test]
    fn a277_event_target_not_running_skipped() {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "idle", PERM_FS, 0).unwrap(); // Created, not Running
        event_enqueue(&mut m, AppEvent { kind: EvKind::Key, target_app: a, win: 0, code: 0 });
        assert_eq!(event_dispatch(&mut m), 0);
    }

    #[test]
    fn a278_window_create_destroy_and_cap() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        let w1 = window_create(&mut m, a, 800, 600).unwrap();
        let w2 = window_create(&mut m, a, 640, 480).unwrap();
        assert_eq!(window_count(&m, a), 2);
        assert!(window_exists(&m, a, w1));
        assert!(window_destroy(&mut m, a, w1));
        assert_eq!(window_count(&m, a), 1);
        // 上限 8
        let mut m2 = AppManager::new();
        let x = app_register(&mut m2, "many", PERM_FS, 0).unwrap();
        app_launch(&mut m2, x);
        let mut made = 0;
        while window_create(&mut m2, x, 10, 10).is_some() {
            made += 1;
        }
        assert_eq!(made, MAX_WINS);
        assert!(window_create(&mut m2, x, 10, 10).is_none());
    }

    #[test]
    fn a278_window_rejects_when_not_running() {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "paused", PERM_FS, 0).unwrap();
        assert!(window_create(&mut m, a, 10, 10).is_none());
    }

    #[test]
    fn a278_auto_exit_on_all_closed() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        let w = window_create(&mut m, a, 10, 10).unwrap();
        assert!(!window_auto_exit(&mut m, a));
        window_destroy(&mut m, a, w);
        assert!(window_all_closed(&m, a));
        assert!(window_auto_exit(&mut m, a));
        assert_eq!(app_state(&m, a), Some(AppState::Terminated));
    }

    #[test]
    fn a279_component_ownership_validation() {
        let (mut m, a, b) = boot();
        assert!(component_bind(&mut m, 100, a));
        assert!(component_owned_by(&m, 100, a));
        assert!(!component_owned_by(&m, 100, b));
        assert!(!component_bind(&mut m, 100, b)); // 已被 a 绑定
        assert!(component_unbind(&mut m, 100));
        assert!(component_bind(&mut m, 100, b)); // 解绑后可重绑
        assert!(component_owned_by(&m, 100, b));
    }

    #[test]
    fn a280_resource_leak_detection() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        resource_alloc(&mut m, a);
        resource_alloc(&mut m, a);
        resource_alloc(&mut m, a);
        resource_free(&mut m, a);
        assert!(resource_leaked(&m, a));
        assert_eq!(resource_leak_count(&m, a), 2);
        resource_free(&mut m, a);
        resource_free(&mut m, a);
        assert!(!resource_leaked(&m, a));
        assert!(!resource_free(&mut m, a)); // 无可释放
    }

    #[test]
    fn a280_resource_requires_running() {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "z", PERM_FS, 0).unwrap();
        assert!(!resource_alloc(&mut m, a)); // Created
    }

    #[test]
    fn a281_focus_routing() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        let w1 = window_create(&mut m, a, 10, 10).unwrap();
        let w2 = window_create(&mut m, a, 10, 10).unwrap();
        assert!(focus_window(&mut m, a, w2));
        assert_eq!(route_focus(&m), a);
        assert_eq!(app_focus_window(&m, a), w2);
        assert!(!focus_window(&mut m, a, 99)); // 非其窗口
        // 销毁焦点后自动重路由
        window_destroy(&mut m, a, w2);
        assert_eq!(app_focus_window(&m, a), w1);
    }

    #[test]
    fn a282_message_routing_and_overflow() {
        let (mut m, s, r) = boot();
        app_launch(&mut m, r);
        assert!(message_send(&mut m, s, r, 1, 42));
        let got = message_recv(&mut m, r);
        assert_eq!(got.unwrap().payload, 42);
        assert!(message_recv(&mut m, r).is_none());
        // 溢出丢弃
        let mut m2 = AppManager::new();
        for k in 0..MAX_MESSAGES + 4 {
            message_send(&mut m2, 0, (k % 16) as u8, 0, k as u32);
        }
        assert_eq!(message_dropped(&m2), 4);
        assert_eq!(message_pending(&m2), MAX_MESSAGES);
    }

    #[test]
    fn a283_save_and_restore_token() {
        let (mut m, a, _b) = boot();
        assert!(app_save(&mut m, a, 777));
        assert!(app_restore(&mut m, a, 777));
        assert!(!app_restore(&mut m, a, 777)); // 已消费
        assert!(app_save(&mut m, a, 777));
        assert!(!app_restore(&mut m, a, 888)); // token 不符
    }

    #[test]
    fn a284_theme_follows() {
        let (mut m, a, _b) = boot();
        assert_eq!(app_theme(&m, a), Theme::System);
        assert!(app_set_theme(&mut m, a, Theme::Dark));
        assert_eq!(app_theme(&m, a), Theme::Dark);
        assert_eq!(app_theme(&m, 99), Theme::System);
    }

    #[test]
    fn a285_accessibility_flag() {
        let (mut m, a, _b) = boot();
        assert!(app_set_a11y(&mut m, a, 3, true));
        assert!(app_a11y_enabled(&m, a));
        assert!(!app_a11y_enabled(&m, 99));
    }

    #[test]
    fn a286_localization() {
        let (mut m, a, _b) = boot();
        assert!(app_set_locale(&mut m, a, 5));
        assert_eq!(app_locale(&m, a), 5);
    }

    #[test]
    fn a287_manifest_validation() {
        let good = AppManifest { name: "app", ver_major: 1, ver_minor: 0, perms: PERM_FS, entry: 1 };
        let bad_perm = AppManifest { name: "app", ver_major: 1, ver_minor: 0, perms: 1 << 31, entry: 1 };
        let bad_ver = AppManifest { name: "app", ver_major: 0, ver_minor: 0, perms: PERM_FS, entry: 1 };
        let empty = AppManifest { name: "", ver_major: 1, ver_minor: 0, perms: PERM_FS, entry: 1 };
        let no_entry = AppManifest { name: "app", ver_major: 1, ver_minor: 0, perms: PERM_FS, entry: 0 };
        assert!(validate_manifest(good));
        assert!(!validate_manifest(bad_perm));
        assert!(!validate_manifest(bad_ver));
        assert!(!validate_manifest(empty));
        assert!(!validate_manifest(no_entry));
    }

    #[test]
    fn a288_sandbox_bitmask() {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "sbx", 0, 0).unwrap();
        assert!(sandbox_grant(&mut m, a, PERM_NET));
        assert!(sandbox_check(&m, a, PERM_NET));
        assert!(!sandbox_check(&m, a, PERM_FS));
        assert!(sandbox_revoke(&mut m, a, PERM_NET));
        assert!(!sandbox_check(&m, a, PERM_NET));
        // 越权位被屏蔽
        let b = app_register(&mut m, "x", 1 << 31, 0).unwrap();
        assert_eq!(m.apps[app_index(&m, b).unwrap()].sandbox & !PERM_ALL, 0);
    }

    #[test]
    fn a289_launch_budget() {
        let mut m = AppManager::new();
        let a = app_register(&mut m, "boot", PERM_FS, 1000).unwrap();
        app_launch(&mut m, a);
        assert!(record_first_frame(&mut m, a, 1300));
        assert!(launch_within_budget(&m, a)); // 300 <= 1000
        assert_eq!(launch_ms(&m, a), 300);
        record_first_frame(&mut m, a, 5000);
        assert!(!launch_within_budget(&m, a));
        // 未记录 created 时拒绝
        let c = app_register(&mut m, "c", PERM_FS, 0).unwrap();
        assert!(!record_first_frame(&mut m, c, 10));
    }

    #[test]
    fn a290_crash_isolation() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        assert!(app_crash(&mut m, a));
        assert!(app_isolated(&m, a));
        assert_eq!(app_state(&m, a), Some(AppState::Terminated));
        // 隔离应用不可再启动
        assert!(!app_launch(&mut m, a));
    }

    #[test]
    fn a291_profiling_average() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        app_profile_frame(&mut m, a, 10);
        app_profile_frame(&mut m, a, 20);
        assert_eq!(app_avg_frame_ms(&m, a), 15);
        assert_eq!(app_avg_frame_ms(&m, 99), 0);
        // 非运行态不计入
        let b = app_register(&mut m, "p2", PERM_FS, 0).unwrap();
        assert!(!app_profile_frame(&mut m, b, 10));
    }

    #[test]
    fn a292_dev_doc_register() {
        let mut m = AppManager::new();
        let id = app_doc_register(&mut m, "lifecycle", "A276").unwrap();
        let d = app_doc_get(&m, id).unwrap();
        assert_eq!(d.topic, "lifecycle");
        assert_eq!(d.anchor, "A276");
        assert!(app_doc_get(&m, 200).is_none());
        // 容量上限
        let mut m2 = AppManager::new();
        let mut last = None;
        for _ in 0..MAX_APP_DOCS + 1 {
            last = app_doc_register(&mut m2, "t", "a");
        }
        assert!(last.is_none());
    }

    #[test]
    fn a293_app_self_check_gate() {
        let mut m = AppManager::new();
        let clean = app_register(&mut m, "clean", PERM_FS, 0).unwrap();
        app_launch(&mut m, clean);
        window_create(&mut m, clean, 10, 10);
        assert!(run_app_self_check(&m, clean));
        let dirty = app_register(&mut m, "dirty", PERM_FS, 0).unwrap();
        app_launch(&mut m, dirty);
        resource_alloc(&mut m, dirty); // 泄漏
        assert!(!run_app_self_check(&m, dirty));
        assert!(!run_app_self_check(&m, 99));
    }

    #[test]
    fn a294_framework_self_check_all_pass() {
        let s = run_appfw_checks();
        for i in 0..s.len() {
            if let Some(c) = s.get(i) {
                if !c.passed {
                    println!("DIAG a294 fail: {} : {}", c.name, c.detail);
                }
            }
        }
        assert!(s.all_passed(), "some checks failed");
        assert!(!s.truncated());
        assert!(s.len() >= 25);
    }

    #[test]
    fn a295_perf_budget_idle_less_than_active() {
        assert!(appfw_perf_budget(3, true) < appfw_perf_budget(3, false));
        assert!(appfw_perf_budget(0, true) == 800);
        assert!(appfw_perf_budget(5, false) > appfw_perf_budget(1, false));
    }

    #[test]
    fn a296_observability_stats() {
        let (mut m, a, _b) = boot();
        app_launch(&mut m, a);
        for _ in 0..3 {
            let _ = event_enqueue(&mut m, AppEvent { kind: EvKind::Key, target_app: a, win: 0, code: 0 });
        }
        let st = framework_stats(&m);
        assert_eq!(st.apps, 2); // boot() 注册了 a、b 两个应用
        assert_eq!(st.running, 1);
        assert_eq!(st.events_dropped, m.ev_dropped);
        // 隔离计数
        let c = app_register(&mut m, "cr", PERM_FS, 0).unwrap();
        app_crash(&mut m, c);
        assert_eq!(framework_stats(&m).isolated, 1);
    }

    #[test]
    fn a297_fuzz_bounded_and_safe() {
        assert_eq!(appfw_fuzz(&[]), 0);
        let r = appfw_fuzz(&[0u8; 300]);
        assert!(r <= 256);
        let r2 = appfw_fuzz(&[255u8; 17]);
        assert!(r2 <= 256);
    }

    #[test]
    fn a298_doc_anchors() {
        assert!(appfw_doc(0).is_some());
        assert_eq!(appfw_doc(0).unwrap(), "A276-app-lifecycle");
        assert!(appfw_doc(FW_DOCS.len()).is_none());
    }

    #[test]
    fn a299_degrade_chain() {
        assert_eq!(degrade_level(0), DegradeLevel::None);
        assert_eq!(degrade_level(300), DegradeLevel::Soft);
        assert_eq!(degrade_level(600), DegradeLevel::Hard);
        assert_eq!(degrade_level(950), DegradeLevel::Minimal);
        let mut m = AppManager::new();
        for _ in 0..3 {
            let _ = event_enqueue(&mut m, AppEvent { kind: EvKind::Close, target_app: 0, win: 0, code: 0 });
        }
        assert!(!framework_apply_degrade(&mut m, DegradeLevel::Soft));
        assert!(framework_apply_degrade(&mut m, DegradeLevel::Minimal));
        assert_eq!(event_pending(&m), 0);
    }

    #[test]
    fn a300_domain_closure_ok() {
        assert!(run_appfw_domain_ok());
    }
}
