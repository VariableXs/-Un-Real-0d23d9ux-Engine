//! AURORA-1000 AI-14 · 剪贴板与拖放（A326~A350，W2）
//!
//! 纯逻辑域：剪贴板多格式槽（Text/Html/Image/Files 各一槽 + 格式枚举）、
//! 写入/读取（所有权序号递增，读旧序号得 Stale）、复制/粘贴语义（源应用 id
//! 记录，跨应用粘贴允许、同 id 亦可）、历史栈（[Entry;16] 环形，循环粘贴）、
//! 大小钳制（超长截断+标记）、拖放状态机（DragStart→DragMove 命中目标高亮→
//! Drop/Cancel）、拖放与剪贴板共用数据槽、安全（secret 位：密钥管理器置位，
//! 历史不入栈）、清空/覆盖语义。
//!
//! 仅依赖 core：固定容量数组 + usize 计数；内容用定长 [u8; 256] + len。无
//! unsafe、无宏、无泛型魔法。所有策略均为纯函数或固定容量结构，可被宿主测试
//! 套件在无需硬件的情况下完整执行。

use crate::checks::CheckSet;

// --------------------------------------------------------------------------
// 常量
// --------------------------------------------------------------------------

/// 单槽内容容量（字节）。
pub const CLIP_MAX_BYTES: usize = 256;
/// 历史栈容量（环形）。
pub const HISTORY_CAP: usize = 16;
/// 权限许可表容量。
pub const MAX_APPS: usize = 16;
/// 每个时间窗允许的最大复制次数（性能预算）。
pub const COPY_BUDGET: u32 = 100;
/// secret 槽混淆用的固定 XOR 密钥（可逆变换，非密码学强度加密）。
pub const SECRET_KEY: u8 = 0x5A;
/// 文件管理器放置目标的固定 id（拖放协作）。
pub const FILE_MANAGER_TARGET: u16 = 0xFA;

// --------------------------------------------------------------------------
// A326 剪贴板核心
// --------------------------------------------------------------------------

/// 剪贴板格式枚举（多格式并存：每格式一槽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipFormat {
    Text = 0,
    Html = 1,
    Image = 2,
    Files = 3,
}

impl ClipFormat {
    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn from_id(id: u8) -> Option<ClipFormat> {
        match id {
            0 => Some(ClipFormat::Text),
            1 => Some(ClipFormat::Html),
            2 => Some(ClipFormat::Image),
            3 => Some(ClipFormat::Files),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ClipFormat::Text => "text",
            ClipFormat::Html => "html",
            ClipFormat::Image => "image",
            ClipFormat::Files => "files",
        }
    }
}

/// 单个格式槽：定长内容 + 长度 + 截断/secret 标记 + 所有权序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipSlot {
    pub data: [u8; CLIP_MAX_BYTES],
    pub len: usize,
    pub truncated: bool,
    pub seq: u32,
    pub present: bool,
    pub secret: bool,
}

impl ClipSlot {
    pub const fn new() -> ClipSlot {
        ClipSlot {
            data: [0u8; CLIP_MAX_BYTES],
            len: 0,
            truncated: false,
            seq: 0,
            present: false,
            secret: false,
        }
    }
}

/// 历史记录项（带内容快照，循环粘贴直接重放，不受活槽覆盖影响）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub fmt: ClipFormat,
    pub seq: u32,
    pub app_id: u16,
    pub secret: bool,
    pub data: [u8; CLIP_MAX_BYTES],
    pub len: usize,
}

/// 读取结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Ok,
    Stale,
    Empty,
    Denied,
}

/// 剪贴板整体状态（Copy，固定容量）。
#[derive(Clone, Copy, Debug)]
pub struct Clipboard {
    slots: [ClipSlot; 4],
    owner_seq: u32,
    source_app: u16,
    last_paste_app: u16,
    last_fmt: ClipFormat,
    history: [Option<HistoryEntry>; HISTORY_CAP],
    hist_head: usize,
    hist_count: usize,
    allowed: [u16; MAX_APPS],
    allowed_count: usize,
    /// 白名单模式：一旦使用过 grant，空表即「全部拒绝」而非「全部放行」。
    restricted: bool,
    copy_budget: u32,
    over_budget: bool,
    writes: u32,
    pastes: u32,
    drops: u32,
    drops_cancel: u32,
    denials: u32,
    degraded_copies: u32,
    drag_active: bool,
}

/// A326 剪贴板核心：构造空剪贴板。
pub const fn new_clipboard() -> Clipboard {
    Clipboard {
        slots: [ClipSlot::new(); 4],
        owner_seq: 0,
        source_app: 0,
        last_paste_app: 0,
        last_fmt: ClipFormat::Text,
        history: [None; HISTORY_CAP],
        hist_head: 0,
        hist_count: 0,
        allowed: [0u16; MAX_APPS],
        allowed_count: 0,
        restricted: false,
        copy_budget: COPY_BUDGET,
        over_budget: false,
        writes: 0,
        pastes: 0,
        drops: 0,
        drops_cancel: 0,
        denials: 0,
        degraded_copies: 0,
        drag_active: false,
    }
}

/// A326 剪贴板核心：是否存在任意非空槽。
pub fn has_content(cb: &Clipboard) -> bool {
    let mut any = false;
    for i in 0..4 {
        if cb.slots[i].present {
            any = true;
        }
    }
    any
}

/// A326 剪贴板核心：清空（覆盖语义 = 全部置空，序号不回退）。
pub fn clear(cb: &mut Clipboard) {
    for i in 0..4 {
        cb.slots[i] = ClipSlot::new();
    }
}

// --------------------------------------------------------------------------
// A327 文本/图片/文件多格式
// --------------------------------------------------------------------------

fn slot_index(fmt: ClipFormat) -> usize {
    fmt.id() as usize
}

/// A327 多格式写入：写入一格式槽，超长截断并标记，secret 位做 XOR 混淆。
pub fn write_slot(cb: &mut Clipboard, fmt: ClipFormat, src: &[u8], app_id: u16, secret: bool) -> bool {
    let idx = slot_index(fmt);
    if idx >= 4 {
        return false;
    }
    let n = src.len().min(CLIP_MAX_BYTES);
    let mut s = ClipSlot::new();
    if n > 0 {
        s.data[..n].copy_from_slice(&src[..n]);
    }
    if secret {
        let mut k = 0usize;
        while k < n {
            s.data[k] ^= SECRET_KEY;
            k += 1;
        }
    }
    s.len = n;
    s.truncated = src.len() > CLIP_MAX_BYTES;
    s.secret = secret;
    s.present = true;
    cb.owner_seq = cb.owner_seq.wrapping_add(1);
    s.seq = cb.owner_seq;
    cb.slots[idx] = s;
    cb.source_app = app_id;
    cb.last_fmt = fmt;
    cb.writes = cb.writes.wrapping_add(1);
    true
}

/// A327 多格式：某格式槽是否存在内容。
pub fn format_present(cb: &Clipboard, fmt: ClipFormat) -> bool {
    cb.slots[slot_index(fmt)].present
}

// --------------------------------------------------------------------------
// A328 复制粘贴
// --------------------------------------------------------------------------

/// A328 复制：写入非 secret 槽并记录源应用，写入历史。
pub fn copy(cb: &mut Clipboard, fmt: ClipFormat, src: &[u8], app_id: u16) -> bool {
    // 降级链：预算耗尽（count 归零或置 over_budget）时仅允许 Text（图片/文件复制被降级拒绝）。
    if (fmt == ClipFormat::Image || fmt == ClipFormat::Files) && (cb.copy_budget == 0 || cb.over_budget) {
        cb.degraded_copies = cb.degraded_copies.wrapping_add(1);
        return false;
    }
    if !write_slot(cb, fmt, src, app_id, false) {
        return false;
    }
    let _ = budget_consume(cb, 1);
    let seq = cb.slots[slot_index(fmt)].seq;
    history_push(cb, fmt, seq, app_id);
    true
}

/// A328 粘贴：读取当前槽；跨应用允许（同 id 亦可）；secret 仅源应用可读；
/// 传入 seen_seq 与当前不符得 Stale。
pub fn paste(cb: &mut Clipboard, fmt: ClipFormat, seen_seq: u32, app_id: u16, out: &mut [u8]) -> ReadOutcome {
    if !can_read(cb, app_id) {
        cb.denials = cb.denials.wrapping_add(1);
        return ReadOutcome::Denied;
    }
    let idx = slot_index(fmt);
    if idx >= 4 {
        return ReadOutcome::Denied;
    }
    let s = cb.slots[idx];
    if !s.present {
        return ReadOutcome::Empty;
    }
    if s.secret && app_id != cb.source_app {
        cb.denials = cb.denials.wrapping_add(1);
        return ReadOutcome::Denied;
    }
    if seen_seq != 0 && seen_seq != s.seq {
        return ReadOutcome::Stale;
    }
    let n = s.len.min(out.len());
    if n > 0 {
        out[..n].copy_from_slice(&s.data[..n]);
        if s.secret {
            let mut k = 0usize;
            while k < n {
                out[k] ^= SECRET_KEY;
                k += 1;
            }
        }
    }
    cb.last_paste_app = app_id;
    cb.pastes = cb.pastes.wrapping_add(1);
    ReadOutcome::Ok
}

// --------------------------------------------------------------------------
// A329 拖放引擎
// --------------------------------------------------------------------------

/// 拖放阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragPhase {
    Idle,
    Dragging,
    OverTarget,
    Dropped,
    Cancelled,
}

/// 拖放状态（数据复用剪贴板槽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragState {
    pub phase: DragPhase,
    pub fmt: ClipFormat,
    pub app_id: u16,
    pub x: u16,
    pub y: u16,
    pub target_hit: bool,
    pub secret: bool,
    pub data: [u8; CLIP_MAX_BYTES],
    pub len: usize,
}

/// 放置结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropResult {
    Dropped,
    Cancelled,
    NotDragging,
}

/// A329 拖放引擎：开始拖拽，载荷暂存于 DragState。
pub fn drag_start(cb: &mut Clipboard, fmt: ClipFormat, src: &[u8], app_id: u16, secret: bool) -> DragState {
    let n = src.len().min(CLIP_MAX_BYTES);
    let mut data = [0u8; CLIP_MAX_BYTES];
    if n > 0 {
        data[..n].copy_from_slice(&src[..n]);
    }
    cb.drag_active = true;
    cb.source_app = app_id;
    DragState {
        phase: DragPhase::Dragging,
        fmt,
        app_id,
        x: 0,
        y: 0,
        target_hit: false,
        secret,
        data,
        len: n,
    }
}

/// A329 拖放引擎：移动并做命中目标高亮判定（over_target 时进入 OverTarget）。
pub fn drag_move(st: &mut DragState, x: u16, y: u16, over_target: bool) {
    if st.phase == DragPhase::Dragging || st.phase == DragPhase::OverTarget {
        st.x = x;
        st.y = y;
        st.target_hit = over_target;
        st.phase = if over_target { DragPhase::OverTarget } else { DragPhase::Dragging };
    }
}

/// A329 拖放引擎：放置，命中目标则写入剪贴板槽（与剪贴板共用数据槽）。
pub fn drag_drop(cb: &mut Clipboard, st: &mut DragState) -> DropResult {
    if st.phase == DragPhase::Idle {
        return DropResult::NotDragging;
    }
    if st.target_hit {
        let _ = write_slot(cb, st.fmt, &st.data[..st.len], st.app_id, st.secret);
        st.phase = DragPhase::Dropped;
        cb.drops = cb.drops.wrapping_add(1);
        cb.drag_active = false;
        DropResult::Dropped
    } else {
        st.phase = DragPhase::Cancelled;
        cb.drops_cancel = cb.drops_cancel.wrapping_add(1);
        cb.drag_active = false;
        DropResult::Cancelled
    }
}

/// A329 拖放引擎：取消拖拽（计数并结束活动标记）。
pub fn drag_cancel(cb: &mut Clipboard, st: &mut DragState) {
    if st.phase == DragPhase::Idle {
        return;
    }
    st.phase = DragPhase::Cancelled;
    cb.drops_cancel = cb.drops_cancel.wrapping_add(1);
    cb.drag_active = false;
}

// --------------------------------------------------------------------------
// A330 拖拽预览
// --------------------------------------------------------------------------

/// A330 拖拽预览：返回预览框尺寸（按格式固定）。
pub fn preview_dims(fmt: ClipFormat, _len: usize) -> (u16, u16) {
    match fmt {
        ClipFormat::Text => (320, 80),
        ClipFormat::Html => (360, 120),
        ClipFormat::Image => (200, 150),
        ClipFormat::Files => (240, 120),
    }
}

// --------------------------------------------------------------------------
// A331 剪贴板历史
// --------------------------------------------------------------------------

/// A331 历史：环形压入一条（新项在 head，带内容快照，不覆盖 secret）。
pub fn history_push(cb: &mut Clipboard, fmt: ClipFormat, seq: u32, app_id: u16) {
    let s = cb.slots[slot_index(fmt)];
    let mut data = [0u8; CLIP_MAX_BYTES];
    if s.len > 0 {
        data[..s.len].copy_from_slice(&s.data[..s.len]);
    }
    let e = HistoryEntry {
        fmt,
        seq,
        app_id,
        secret: false,
        data,
        len: s.len,
    };
    cb.history[cb.hist_head] = Some(e);
    cb.hist_head = (cb.hist_head + 1) % HISTORY_CAP;
    if cb.hist_count < HISTORY_CAP {
        cb.hist_count = cb.hist_count + 1;
    }
}

/// A331 历史：按 index 取（0 = 最新），环形最新在前。
pub fn history_get(cb: &Clipboard, index: usize) -> Option<HistoryEntry> {
    if index >= cb.hist_count {
        return None;
    }
    let pos = (cb.hist_head + HISTORY_CAP - 1 - index) % HISTORY_CAP;
    cb.history[pos]
}

/// A331 历史/循环粘贴：重放最近第 n 条（n % count 循环）的内容快照。
/// 历史项永不含 secret，故直接回放，不受活槽后续覆盖影响。
pub fn cycle_paste(cb: &mut Clipboard, n: usize, app_id: u16, out: &mut [u8]) -> ReadOutcome {
    if cb.hist_count == 0 {
        return ReadOutcome::Empty;
    }
    let idx = n % cb.hist_count;
    match history_get(cb, idx) {
        Some(e) => {
            let m = e.len.min(out.len());
            if m > 0 {
                out[..m].copy_from_slice(&e.data[..m]);
            }
            cb.last_paste_app = app_id;
            cb.pastes = cb.pastes.wrapping_add(1);
            ReadOutcome::Ok
        }
        None => ReadOutcome::Empty,
    }
}

// --------------------------------------------------------------------------
// A332 剪贴板同步
// --------------------------------------------------------------------------

/// A332 同步：将最新非 secret、非空的槽快照序列化进 out，返回写入字节数。
pub fn sync_snapshot(cb: &Clipboard, out: &mut [u8]) -> usize {
    let mut n;
    if out.len() < 4 {
        return 0;
    }
    out[0..4].copy_from_slice(&cb.owner_seq.to_le_bytes());
    n = 4;
    let mut i = 0usize;
    while i < 4 {
        if n + 2 > out.len() {
            break;
        }
        let s = cb.slots[i];
        let live = s.present && !s.secret;
        out[n] = if live { 1 } else { 0 };
        n += 1;
        let l = if live { s.len } else { 0 };
        out[n] = (l & 0xFF) as u8;
        n += 1;
        if n + l > out.len() {
            break;
        }
        if l > 0 {
            out[n..n + l].copy_from_slice(&s.data[..l]);
        }
        n += l;
        i += 1;
    }
    n
}

// --------------------------------------------------------------------------
// A333 跨应用粘贴
// --------------------------------------------------------------------------

/// A333 跨应用粘贴：是否允许该应用读取（默认全部允许，除非被权限撤销）。
pub fn cross_app_ok(cb: &Clipboard, app_id: u16) -> bool {
    can_read(cb, app_id)
}

/// A333 跨应用粘贴：源应用 id 记录查询。
pub fn source_app(cb: &Clipboard) -> u16 {
    cb.source_app
}

// --------------------------------------------------------------------------
// A334 剪贴板权限
// --------------------------------------------------------------------------

/// A334 权限：将 app 加入允许表（首次授权即进入白名单模式）。
pub fn grant(cb: &mut Clipboard, app_id: u16) -> bool {
    cb.restricted = true;
    if cb.allowed_count >= MAX_APPS {
        return false;
    }
    let mut i = 0usize;
    while i < cb.allowed_count {
        if cb.allowed[i] == app_id {
            return true;
        }
        i += 1;
    }
    cb.allowed[cb.allowed_count] = app_id;
    cb.allowed_count += 1;
    true
}

/// A334 权限：撤销 app 的读取权限。
pub fn revoke(cb: &mut Clipboard, app_id: u16) -> bool {
    let mut i = 0usize;
    while i < cb.allowed_count {
        if cb.allowed[i] == app_id {
            let mut j = i;
            while j + 1 < cb.allowed_count {
                cb.allowed[j] = cb.allowed[j + 1];
                j += 1;
            }
            cb.allowed_count -= 1;
            return true;
        }
        i += 1;
    }
    false
}

/// A334 权限：是否允许读取（未进入白名单模式时放行全部）。
pub fn can_read(cb: &Clipboard, app_id: u16) -> bool {
    if !cb.restricted && cb.allowed_count == 0 {
        return true;
    }
    let mut i = 0usize;
    while i < cb.allowed_count {
        if cb.allowed[i] == app_id {
            return true;
        }
        i += 1;
    }
    false
}

// --------------------------------------------------------------------------
// A335 剪贴板加密（secret 混淆）
// --------------------------------------------------------------------------

/// A335 加密：原地 XOR 混淆（可逆）。
pub fn encrypt_bytes(data: &mut [u8]) {
    let mut i = 0usize;
    while i < data.len() {
        data[i] ^= SECRET_KEY;
        i += 1;
    }
}

/// A335 加密：原地 XOR 还原（与 encrypt 互为逆）。
pub fn decrypt_bytes(data: &mut [u8]) {
    encrypt_bytes(data);
}

/// A335 安全：secret 槽标记查询。
pub fn is_secret(cb: &Clipboard, fmt: ClipFormat) -> bool {
    cb.slots[slot_index(fmt)].secret
}

// --------------------------------------------------------------------------
// A336 拖放与文件管理器协作
// --------------------------------------------------------------------------

/// A336 协作：目标 id 是否为文件管理器放置区。
pub fn is_file_target(target_id: u16) -> bool {
    target_id == FILE_MANAGER_TARGET
}

/// A336 协作：统计 Files 槽中以 NUL 分隔的文件条目数。
pub fn file_entry_count(cb: &Clipboard) -> usize {
    let s = cb.slots[slot_index(ClipFormat::Files)];
    if !s.present {
        return 0;
    }
    let mut count = 0usize;
    let mut in_entry = false;
    let mut k = 0usize;
    while k < s.len {
        if s.data[k] == 0 {
            if in_entry {
                count += 1;
                in_entry = false;
            }
        } else {
            in_entry = true;
        }
        k += 1;
    }
    if in_entry {
        count += 1;
    }
    count
}

// --------------------------------------------------------------------------
// A337 剪贴板性能预算
// --------------------------------------------------------------------------

/// A337 性能预算：消费 cost 次复制额度，不足则置 over_budget 并返回 false。
pub fn budget_consume(cb: &mut Clipboard, cost: u32) -> bool {
    if cb.copy_budget >= cost {
        cb.copy_budget -= cost;
        true
    } else {
        cb.over_budget = true;
        false
    }
}

/// A337 性能预算：剩余复制额度。
pub fn copy_budget_left(cb: &Clipboard) -> u32 {
    cb.copy_budget
}

// --------------------------------------------------------------------------
// A338 剪贴板模糊测试
// --------------------------------------------------------------------------

/// A338 模糊测试：对随机输入跑 64 步随机操作，断言不变量始终成立。
pub fn fuzz_clipboard(input: &[u8]) -> bool {
    let mut cb = new_clipboard();
    if input.is_empty() {
        return clip_invariants(&cb);
    }
    let mut i = 0usize;
    let mut app: u16 = 1;
    let mut step = 0usize;
    while step < 64 {
        let op = input[i % input.len()];
        i = i.wrapping_add(1);
        let fmt = match ClipFormat::from_id(op % 4) {
            Some(f) => f,
            None => ClipFormat::Text,
        };
        match op % 5 {
            0 => {
                let take = input.len().min(8);
                let _ = copy(&mut cb, fmt, &input[..take], app);
            }
            1 => {
                let mut out = [0u8; CLIP_MAX_BYTES];
                let _ = paste(&mut cb, fmt, 0, app, &mut out);
            }
            2 => {
                let take = input.len().min(4);
                let mut st = drag_start(&mut cb, fmt, &input[..take], app, false);
                drag_move(&mut st, op as u16, 0, true);
                let _ = drag_drop(&mut cb, &mut st);
            }
            3 => {
                let _ = grant(&mut cb, app);
            }
            _ => {
                let _ = history_get(&cb, (op as usize) % HISTORY_CAP);
            }
        }
        app = app.wrapping_add(1).max(1);
        if !clip_invariants(&cb) {
            return false;
        }
        step += 1;
    }
    clip_invariants(&cb)
}

// --------------------------------------------------------------------------
// A339 剪贴板自检收口
// --------------------------------------------------------------------------

/// A339 自检收口：核心不变量（序号单调、长度合法、历史无 secret）。
pub fn clip_invariants(cb: &Clipboard) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < 4 {
        let s = cb.slots[i];
        if s.present && s.seq > cb.owner_seq {
            ok = false;
        }
        if s.len > CLIP_MAX_BYTES {
            ok = false;
        }
        i += 1;
    }
    let mut h = 0usize;
    while h < cb.hist_count {
        if let Some(e) = history_get(cb, h) {
            if e.secret {
                ok = false;
            }
        }
        h += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// A340 拖放无障碍
// --------------------------------------------------------------------------

/// A340 无障碍：键盘驱动的放置（无需指针，按目标 id 直接放置）。
pub fn accessible_drop(cb: &mut Clipboard, st: &mut DragState, target_id: u16) -> DropResult {
    st.target_hit = is_file_target(target_id);
    drag_drop(cb, st)
}

// --------------------------------------------------------------------------
// A341 剪贴板可观测
// --------------------------------------------------------------------------

/// A341 可观测：计数器快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipStats {
    pub writes: u32,
    pub pastes: u32,
    pub drops: u32,
    pub denials: u32,
}

/// A341 可观测：返回计数。
pub fn clip_stats(cb: &Clipboard) -> ClipStats {
    ClipStats {
        writes: cb.writes,
        pastes: cb.pastes,
        drops: cb.drops,
        denials: cb.denials,
    }
}

// --------------------------------------------------------------------------
// A342 剪贴板文档
// --------------------------------------------------------------------------

/// A342 文档：返回域说明文本。
pub fn docs() -> &'static str {
    "AI-14 clipboard: multi-format slots, seq ownership, copy/paste, history ring, \
     size clamp, drag&drop state machine, secret exclusion from history, permissions."
}

// --------------------------------------------------------------------------
// A343 剪贴板域自检收口
// --------------------------------------------------------------------------

/// A343 域自检收口：不变量 + 计数自洽。
pub fn clip_self_closeout(cb: &Clipboard) -> bool {
    clip_invariants(cb) && cb.denials <= cb.writes.wrapping_add(cb.pastes)
}

// --------------------------------------------------------------------------
// A344 剪贴板与拖放自检（域自检入口）
// --------------------------------------------------------------------------

/// A344 域自检：导出覆盖 A326~A350 的全真检查（受 MAX_CHECKS=32 约束，
/// 每项合并为单条检查 + 若干边界 extras + 末尾规模哨兵，共 ≤32 条）。
pub fn run_clipboard_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-clipboard");
    let sample: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    // A326 剪贴板核心：空 + 枚举 + 清空
    let mut c = new_clipboard();
    let _ = write_slot(&mut c, ClipFormat::Text, b"x", 1, false);
    clear(&mut c);
    set.add(
        "A326 core+enum+clear",
        !has_content(&c)
            && !format_present(&c, ClipFormat::Text)
            && ClipFormat::from_id(0) == Some(ClipFormat::Text)
            && ClipFormat::from_id(3) == Some(ClipFormat::Files)
            && ClipFormat::from_id(9).is_none()
            && ClipFormat::Image.name() == "image",
        "core empty + enum",
    );

    // A327 多格式：钳制 + 并存
    let mut c = new_clipboard();
    let big = [7u8; CLIP_MAX_BYTES + 40];
    let _ = write_slot(&mut c, ClipFormat::Text, &big, 1, false);
    set.add(
        "A327 clamp+trunc",
        c.slots[0].len == CLIP_MAX_BYTES && c.slots[0].truncated && format_present(&c, ClipFormat::Text),
        "oversize truncated",
    );
    let mut c = new_clipboard();
    let _ = write_slot(&mut c, ClipFormat::Text, b"hi", 1, false);
    let _ = write_slot(&mut c, ClipFormat::Image, b"img", 2, false);
    set.add(
        "A327 coexist",
        format_present(&c, ClipFormat::Text) && format_present(&c, ClipFormat::Image),
        "multi-format slots",
    );

    // A328 复制粘贴：往返 + 源 + Stale + 空
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"hello", 5);
    let mut out = [0u8; CLIP_MAX_BYTES];
    let r1 = paste(&mut c, ClipFormat::Text, 0, 9, &mut out);
    let old = c.slots[0].seq;
    let _ = copy(&mut c, ClipFormat::Text, b"world", 5);
    let mut o3 = [0u8; CLIP_MAX_BYTES];
    let r3 = paste(&mut c, ClipFormat::Text, old, 9, &mut o3);
    let mut o4 = [0u8; CLIP_MAX_BYTES];
    let r4 = paste(&mut c, ClipFormat::Image, 0, 9, &mut o4);
    set.add(
        "A328 copy/paste/stale/empty",
        r1 == ReadOutcome::Ok && &out[..5] == b"hello" && source_app(&c) == 5 && r3 == ReadOutcome::Stale && r4 == ReadOutcome::Empty,
        "copy/paste semantics",
    );

    // A329 拖放引擎：移动高亮 + 放置 + 取消
    let mut c = new_clipboard();
    let mut st = drag_start(&mut c, ClipFormat::Text, b"d", 3, false);
    drag_move(&mut st, 12, 34, true);
    let ph = st.phase;
    let th = st.target_hit;
    let dr = drag_drop(&mut c, &mut st);
    let dp = format_present(&c, ClipFormat::Text);
    let mut st2 = drag_start(&mut c, ClipFormat::Text, b"x", 3, false);
    drag_move(&mut st2, 0, 0, false);
    let c2 = drag_drop(&mut c, &mut st2);
    let mut st3 = drag_start(&mut c, ClipFormat::Text, b"y", 3, false);
    drag_cancel(&mut c, &mut st3);
    set.add(
        "A329 drag move/drop/cancel",
        th && ph == DragPhase::OverTarget && dr == DropResult::Dropped && dp && c2 == DropResult::Cancelled && st3.phase == DragPhase::Cancelled,
        "dnd state machine",
    );

    // A330 拖拽预览
    set.add(
        "A330 preview",
        preview_dims(ClipFormat::Text, 0) == (320, 80)
            && preview_dims(ClipFormat::Image, 0) == (200, 150)
            && preview_dims(ClipFormat::Files, 0).0 > 0,
        "preview sizes",
    );

    // A331 历史 / 循环粘贴
    let mut c = new_clipboard();
    let mut n: u8 = 0;
    while n < 20 {
        let b = [n; 1];
        let _ = copy(&mut c, ClipFormat::Text, &b, 1);
        n += 1;
    }
    let mut o = [0u8; CLIP_MAX_BYTES];
    let rc = cycle_paste(&mut c, 0, 1, &mut o);
    let mut o2 = [0u8; CLIP_MAX_BYTES];
    let rc2 = cycle_paste(&mut c, 15, 1, &mut o2);
    set.add(
        "A331 ring+cycle",
        c.hist_count == HISTORY_CAP && history_get(&c, 16).is_none() && rc == ReadOutcome::Ok && o[0] == 19 && rc2 == ReadOutcome::Ok && o2[0] == 4,
        "history loop",
    );

    // A332 同步
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"sync", 1);
    let mut buf = [0u8; 1100];
    let w = sync_snapshot(&c, &mut buf);
    let seq = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    set.add("A332 sync", w > 4 && seq == c.owner_seq && buf[4] == 1, "snapshot serializes");

    // A333 跨应用
    let c = new_clipboard();
    set.add("A333 cross-app", cross_app_ok(&c, 42) && source_app(&c) == 0, "default allow");

    // A334 权限：授权 / 撤销 / 拒绝
    let mut c = new_clipboard();
    let _ = grant(&mut c, 7);
    let g1 = can_read(&c, 7) && !can_read(&c, 8);
    let _ = revoke(&mut c, 7);
    let g2 = !can_read(&c, 7);
    let _ = grant(&mut c, 9);
    let _ = write_slot(&mut c, ClipFormat::Text, b"z", 1, false);
    let mut o = [0u8; CLIP_MAX_BYTES];
    let rd = paste(&mut c, ClipFormat::Text, 0, 8, &mut o);
    set.add("A334 grant/revoke/deny", g1 && g2 && rd == ReadOutcome::Denied, "permissions");

    // A335 加密 / secret
    let mut c = new_clipboard();
    let _ = write_slot(&mut c, ClipFormat::Text, b"pw", 7, true);
    let sf = is_secret(&c, ClipFormat::Text) && c.hist_count == 0;
    let mut o = [0u8; CLIP_MAX_BYTES];
    let r = paste(&mut c, ClipFormat::Text, 0, 7, &mut o);
    let mut o2 = [0u8; CLIP_MAX_BYTES];
    let r2 = paste(&mut c, ClipFormat::Text, 0, 8, &mut o2);
    let mut blk = [0xABu8, 0xCDu8, 0xEFu8];
    encrypt_bytes(&mut blk);
    decrypt_bytes(&mut blk);
    set.add(
        "A335 secret+xor",
        sf && r == ReadOutcome::Ok && &o[..2] == b"pw" && r2 == ReadOutcome::Denied && blk == [0xAB, 0xCD, 0xEF],
        "secret safe",
    );

    // A336 文件管理器协作
    let mut c = new_clipboard();
    let paths: [u8; 5] = *b"a\0b\0c";
    let _ = write_slot(&mut c, ClipFormat::Files, &paths, 1, false);
    set.add(
        "A336 file target+entries",
        is_file_target(FILE_MANAGER_TARGET) && !is_file_target(0) && file_entry_count(&c) == 3,
        "fm collab",
    );

    // A337 性能预算
    let mut c = new_clipboard();
    let okb = budget_consume(&mut c, 10);
    let left = copy_budget_left(&c);
    let ex = budget_consume(&mut c, 9999);
    set.add(
        "A337 budget+exhaust",
        okb && left == COPY_BUDGET - 10 && !ex && c.over_budget,
        "perf budget",
    );

    // A338 模糊测试
    set.add("A338 fuzz clipboard", fuzz_clipboard(&sample), "random ops safe");

    // A339 自检收口
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"k", 1);
    set.add("A339 invariants", clip_invariants(&c), "invariants hold");

    // A340 无障碍
    let mut c = new_clipboard();
    let mut st = drag_start(&mut c, ClipFormat::Text, b"a11y", 2, false);
    let ar = accessible_drop(&mut c, &mut st, FILE_MANAGER_TARGET);
    set.add("A340 a11y drop", ar == DropResult::Dropped && format_present(&c, ClipFormat::Text), "kb drop");

    // A341 可观测
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"m", 1);
    let mut o = [0u8; CLIP_MAX_BYTES];
    let _ = paste(&mut c, ClipFormat::Text, 0, 1, &mut o);
    let s = clip_stats(&c);
    set.add("A341 stats", s.writes == 1 && s.pastes == 1, "counters");

    // A342 文档
    set.add("A342 docs", docs().len() > 10, "doc text");

    // A343 域自检收口
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"x", 1);
    set.add("A343 closeout", clip_self_closeout(&c), "closeout ok");

    // A345 性能预算聚合
    let mut c = new_clipboard();
    let _ = budget_consume(&mut c, 5);
    set.add("A345 perf budget", copy_budget_left(&c) == COPY_BUDGET - 5, "aggregate budget");

    // A346 拖放可观测
    let mut c = new_clipboard();
    let mut st = drag_start(&mut c, ClipFormat::Text, b"t", 1, false);
    drag_move(&mut st, 0, 0, true);
    let _ = drag_drop(&mut c, &mut st);
    let mut st2 = drag_start(&mut c, ClipFormat::Text, b"t", 1, false);
    drag_cancel(&mut c, &mut st2);
    set.add("A346 dnd telemetry", c.drops == 1 && c.drops_cancel == 1, "dnd counters");

    // A347 拖放模糊测试
    let mut cb2 = new_clipboard();
    set.add("A347 fuzz dnd", fuzz_dnd(&mut cb2, &sample), "random dnd safe");

    // A348 拖放文档
    set.add("A348 docs dnd", docs_dnd().len() > 10, "dnd doc");

    // A349 降级链
    let mut c = new_clipboard();
    let _ = budget_consume(&mut c, 9999);
    set.add(
        "A349 degrade",
        degrade_mode(&c) == DegradeMode::TextOnly && copy(&mut c, ClipFormat::Image, b"i", 1) == false,
        "text-only fallback",
    );

    // A350 域自检收口
    let mut c = new_clipboard();
    let _ = copy(&mut c, ClipFormat::Text, b"fin", 1);
    set.add("A350 final check", clipboard_final_check(&c), "final closeout ok");

    // —— 边界 extras（仍 ≤32 条）——
    let mut c = new_clipboard();
    let _ = write_slot(&mut c, ClipFormat::Text, b"aa", 1, false);
    let first = c.slots[0].seq;
    let _ = write_slot(&mut c, ClipFormat::Text, b"bbbb", 1, false);
    set.add("A326 overwrite", c.slots[0].len == 4 && c.slots[0].seq != first, "overwrite bumps seq");

    let mut c = new_clipboard();
    let mut o = [0u8; CLIP_MAX_BYTES];
    let e0 = paste(&mut c, ClipFormat::Text, 0, 1, &mut o);
    let e1 = paste(&mut c, ClipFormat::Html, 0, 1, &mut o);
    let e2 = paste(&mut c, ClipFormat::Image, 0, 1, &mut o);
    let e3 = paste(&mut c, ClipFormat::Files, 0, 1, &mut o);
    set.add("A328 empty all", e0 == ReadOutcome::Empty && e1 == ReadOutcome::Empty && e2 == ReadOutcome::Empty && e3 == ReadOutcome::Empty, "all-empty boundary");

    let mut c = new_clipboard();
    let _ = write_slot(&mut c, ClipFormat::Text, b"pw", 7, true);
    set.add("A335 no secret history", c.hist_count == 0 && is_secret(&c, ClipFormat::Text), "secret excluded");

    let mut c = new_clipboard();
    let mut n: u8 = 0;
    while n < 40 {
        let b = [n; 1];
        let _ = copy(&mut c, ClipFormat::Text, &b, 1);
        n += 1;
    }
    set.add("A331 cap boundary", c.hist_count == HISTORY_CAP && history_get(&c, 40).is_none(), "cap 16");

    let mut c = new_clipboard();
    let _ = budget_consume(&mut c, COPY_BUDGET);
    set.add("A337 zero budget", copy_budget_left(&c) == 0 && budget_consume(&mut c, 1) == false, "zero blocks");

    // A344 规模哨兵（必须最后：此时已加满全部检查）
    set.add("A344 self-check >=25", set.len() >= 25, "suite size");

    set
}

// --------------------------------------------------------------------------
// A345 剪贴板与拖放性能预算（聚合）
// --------------------------------------------------------------------------

// （聚合预算复用 A337 的 copy_budget_left；此处提供语义包装。）

// --------------------------------------------------------------------------
// A346 剪贴板与拖放可观测（遥测）
// --------------------------------------------------------------------------

/// A346 拖放遥测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DndTelemetry {
    pub drops: u32,
    pub cancels: u32,
    pub active: bool,
}

/// A346 拖放遥测：返回当前拖放计数与活动标记。
pub fn dnd_telemetry(cb: &Clipboard) -> DndTelemetry {
    DndTelemetry {
        drops: cb.drops,
        cancels: cb.drops_cancel,
        active: cb.drag_active,
    }
}

// --------------------------------------------------------------------------
// A347 剪贴板与拖放模糊测试
// --------------------------------------------------------------------------

/// A347 模糊测试：对随机输入跑随机拖放步骤，断言不变量成立。
pub fn fuzz_dnd(cb: &mut Clipboard, input: &[u8]) -> bool {
    if input.is_empty() {
        return clip_invariants(cb);
    }
    let mut i = 0usize;
    let mut app: u16 = 1;
    let mut step = 0usize;
    while step < 48 {
        let op = input[i % input.len()];
        i = i.wrapping_add(1);
        let fmt = match ClipFormat::from_id(op % 4) {
            Some(f) => f,
            None => ClipFormat::Text,
        };
        match op % 3 {
            0 => {
                let take = input.len().min(6);
                let mut st = drag_start(cb, fmt, &input[..take], app, false);
                drag_move(&mut st, op as u16, 0, op % 2 == 0);
                let _ = drag_drop(cb, &mut st);
            }
            1 => {
                let mut st = drag_start(cb, fmt, &input[..input.len().min(2)], app, false);
                drag_cancel(cb, &mut st);
            }
            _ => {
                let mut st = drag_start(cb, fmt, &input[..input.len().min(2)], app, false);
                let _ = accessible_drop(cb, &mut st, FILE_MANAGER_TARGET);
            }
        }
        app = app.wrapping_add(1).max(1);
        if !clip_invariants(cb) {
            return false;
        }
        step += 1;
    }
    clip_invariants(cb)
}

// --------------------------------------------------------------------------
// A348 剪贴板与拖放文档
// --------------------------------------------------------------------------

/// A348 文档：返回拖放说明文本。
pub fn docs_dnd() -> &'static str {
    "AI-14 drag&drop: DragStart->DragMove(hit highlight)->Drop/Cancel shares the \
     clipboard slots; keyboard-accessible drop targets the file manager id."
}

// --------------------------------------------------------------------------
// A349 剪贴板与拖放降级链
// --------------------------------------------------------------------------

/// 降级模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeMode {
    Full,
    TextOnly,
}

/// A349 降级链：预算耗尽或超预算时降级为仅文本。
pub fn degrade_mode(cb: &Clipboard) -> DegradeMode {
    if cb.over_budget || cb.copy_budget == 0 {
        DegradeMode::TextOnly
    } else {
        DegradeMode::Full
    }
}

// --------------------------------------------------------------------------
// A350 剪贴板与拖放域自检收口
// --------------------------------------------------------------------------

/// A350 域自检收口：最终聚合不变量检查。
pub fn clipboard_final_check(cb: &Clipboard) -> bool {
    clip_invariants(cb)
}

// --------------------------------------------------------------------------
// 单元测试
// --------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a326_core_is_empty_and_zero() {
        let c = new_clipboard();
        assert!(!has_content(&c));
        assert_eq!(c.owner_seq, 0);
        assert_eq!(c.hist_count, 0);
    }

    #[test]
    fn a326_format_enum_roundtrip() {
        assert_eq!(ClipFormat::from_id(0), Some(ClipFormat::Text));
        assert_eq!(ClipFormat::from_id(3), Some(ClipFormat::Files));
        assert_eq!(ClipFormat::from_id(4), None);
        assert_eq!(ClipFormat::Html.name(), "html");
        assert_eq!(ClipFormat::Image.id(), 2);
    }

    #[test]
    fn a326_clear_wipes_slots() {
        let mut c = new_clipboard();
        let _ = write_slot(&mut c, ClipFormat::Text, b"x", 1, false);
        clear(&mut c);
        assert!(!has_content(&c));
        assert!(!format_present(&c, ClipFormat::Text));
    }

    #[test]
    fn a327_write_clamps_oversize_and_marks() {
        let mut c = new_clipboard();
        let big = [1u8; CLIP_MAX_BYTES + 50];
        let ok = write_slot(&mut c, ClipFormat::Text, &big, 1, false);
        assert!(ok);
        assert_eq!(c.slots[0].len, CLIP_MAX_BYTES);
        assert!(c.slots[0].truncated);
        assert!(format_present(&c, ClipFormat::Text));
    }

    #[test]
    fn a327_formats_coexist() {
        let mut c = new_clipboard();
        let _ = write_slot(&mut c, ClipFormat::Text, b"t", 1, false);
        let _ = write_slot(&mut c, ClipFormat::Html, b"<b>", 1, false);
        let _ = write_slot(&mut c, ClipFormat::Image, b"im", 1, false);
        let _ = write_slot(&mut c, ClipFormat::Files, b"f", 1, false);
        assert!(format_present(&c, ClipFormat::Text));
        assert!(format_present(&c, ClipFormat::Html));
        assert!(format_present(&c, ClipFormat::Image));
        assert!(format_present(&c, ClipFormat::Files));
    }

    #[test]
    fn a327_overwrite_semantics() {
        let mut c = new_clipboard();
        let _ = write_slot(&mut c, ClipFormat::Text, b"aa", 1, false);
        let first = c.slots[0].seq;
        let _ = write_slot(&mut c, ClipFormat::Text, b"bbbb", 1, false);
        assert_eq!(c.slots[0].len, 4);
        assert_ne!(c.slots[0].seq, first);
    }

    #[test]
    fn a328_copy_paste_roundtrip_and_source() {
        let mut c = new_clipboard();
        assert!(copy(&mut c, ClipFormat::Text, b"hello", 5));
        let mut out = [0u8; CLIP_MAX_BYTES];
        let r = paste(&mut c, ClipFormat::Text, 0, 9, &mut out);
        assert_eq!(r, ReadOutcome::Ok);
        assert_eq!(&out[..5], b"hello");
        assert_eq!(source_app(&c), 5);
    }

    #[test]
    fn a328_cross_app_paste_allowed() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"shared", 1);
        let mut out = [0u8; CLIP_MAX_BYTES];
        let r = paste(&mut c, ClipFormat::Text, 0, 99, &mut out);
        assert_eq!(r, ReadOutcome::Ok);
        assert_eq!(&out[..6], b"shared");
    }

    #[test]
    fn a328_stale_on_old_seq() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"v1", 1);
        let old = c.slots[0].seq;
        let _ = copy(&mut c, ClipFormat::Text, b"v2", 1);
        let mut out = [0u8; CLIP_MAX_BYTES];
        assert_eq!(paste(&mut c, ClipFormat::Text, old, 1, &mut out), ReadOutcome::Stale);
    }

    #[test]
    fn a328_empty_read_missing_format() {
        let mut c = new_clipboard();
        let mut out = [0u8; CLIP_MAX_BYTES];
        assert_eq!(paste(&mut c, ClipFormat::Image, 0, 1, &mut out), ReadOutcome::Empty);
    }

    #[test]
    fn a329_drag_move_highlights_target() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Text, b"d", 3, false);
        drag_move(&mut st, 12, 34, true);
        assert!(st.target_hit);
        assert_eq!(st.phase, DragPhase::OverTarget);
        assert_eq!(st.x, 12);
        assert_eq!(st.y, 34);
    }

    #[test]
    fn a329_drop_transfers_to_slot() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Image, b"pic", 3, false);
        drag_move(&mut st, 1, 1, true);
        assert_eq!(drag_drop(&mut c, &mut st), DropResult::Dropped);
        assert!(format_present(&c, ClipFormat::Image));
    }

    #[test]
    fn a329_drop_without_target_cancels() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Text, b"x", 3, false);
        drag_move(&mut st, 0, 0, false);
        assert_eq!(drag_drop(&mut c, &mut st), DropResult::Cancelled);
        assert!(!format_present(&c, ClipFormat::Text));
    }

    #[test]
    fn a329_cancel_state() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Text, b"x", 3, false);
        drag_cancel(&mut c, &mut st);
        assert_eq!(st.phase, DragPhase::Cancelled);
    }

    #[test]
    fn a330_preview_dims_per_format() {
        assert_eq!(preview_dims(ClipFormat::Text, 0), (320, 80));
        assert_eq!(preview_dims(ClipFormat::Image, 0), (200, 150));
        assert_eq!(preview_dims(ClipFormat::Files, 0).1, 120);
    }

    #[test]
    fn a331_history_ring_capped_at_16() {
        let mut c = new_clipboard();
        let mut n: u8 = 0;
        while n < 20 {
            let b = [n; 1];
            let _ = copy(&mut c, ClipFormat::Text, &b, 1);
            n += 1;
        }
        assert_eq!(c.hist_count, HISTORY_CAP);
        assert!(history_get(&c, 0).is_some());
        assert!(history_get(&c, 16).is_none());
    }

    #[test]
    fn a331_cycle_paste_newest_and_loop() {
        let mut c = new_clipboard();
        let mut n: u8 = 0;
        while n < 16 {
            let b = [n; 1];
            let _ = copy(&mut c, ClipFormat::Text, &b, 1);
            n += 1;
        }
        let mut o = [0u8; CLIP_MAX_BYTES];
        assert_eq!(cycle_paste(&mut c, 0, 1, &mut o), ReadOutcome::Ok);
        assert_eq!(o[0], 15);
        let mut o2 = [0u8; CLIP_MAX_BYTES];
        assert_eq!(cycle_paste(&mut c, 15, 1, &mut o2), ReadOutcome::Ok);
        assert_eq!(o2[0], 0);
    }

    #[test]
    fn a332_sync_snapshot_serializes() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"sync", 1);
        let mut buf = [0u8; 1100];
        let w = sync_snapshot(&c, &mut buf);
        assert!(w > 4);
        let seq = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        assert_eq!(seq, c.owner_seq);
        assert_eq!(buf[4], 1);
    }

    #[test]
    fn a332_sync_excludes_secret() {
        let mut c = new_clipboard();
        let _ = write_slot(&mut c, ClipFormat::Text, b"pub", 1, false);
        let _ = write_slot(&mut c, ClipFormat::Html, b"sec", 1, true);
        let mut buf = [0u8; 1100];
        let w = sync_snapshot(&c, &mut buf);
        assert!(w > 0);
        // text live (1), html suppressed (0)
        assert_eq!(buf[4], 1);
        assert_eq!(buf[4 + 2 + 3], 0);
    }

    #[test]
    fn a333_cross_app_ok_default() {
        let c = new_clipboard();
        assert!(cross_app_ok(&c, 42));
    }

    #[test]
    fn a334_permission_grant_revoke_deny() {
        let mut c = new_clipboard();
        assert!(grant(&mut c, 7));
        assert!(can_read(&c, 7));
        assert!(!can_read(&c, 8));
        assert!(revoke(&mut c, 7));
        assert!(!can_read(&c, 7));
    }

    #[test]
    fn a334_denied_read_returns_denied() {
        let mut c = new_clipboard();
        let _ = grant(&mut c, 9);
        let _ = write_slot(&mut c, ClipFormat::Text, b"z", 1, false);
        let mut o = [0u8; CLIP_MAX_BYTES];
        assert_eq!(paste(&mut c, ClipFormat::Text, 0, 8, &mut o), ReadOutcome::Denied);
    }

    #[test]
    fn a335_secret_not_in_history_and_source_only() {
        let mut c = new_clipboard();
        let _ = write_slot(&mut c, ClipFormat::Text, b"pw", 7, true);
        assert!(is_secret(&c, ClipFormat::Text));
        assert_eq!(c.hist_count, 0);
        let mut o = [0u8; CLIP_MAX_BYTES];
        assert_eq!(paste(&mut c, ClipFormat::Text, 0, 7, &mut o), ReadOutcome::Ok);
        assert_eq!(&o[..2], b"pw");
        let mut o2 = [0u8; CLIP_MAX_BYTES];
        assert_eq!(paste(&mut c, ClipFormat::Text, 0, 8, &mut o2), ReadOutcome::Denied);
    }

    #[test]
    fn a335_xor_reversible() {
        let mut blk = [0xABu8, 0xCDu8, 0xEFu8];
        encrypt_bytes(&mut blk);
        assert_ne!(blk, [0xAB, 0xCD, 0xEF]);
        decrypt_bytes(&mut blk);
        assert_eq!(blk, [0xAB, 0xCD, 0xEF]);
    }

    #[test]
    fn a336_file_target_and_entries() {
        assert!(is_file_target(FILE_MANAGER_TARGET));
        assert!(!is_file_target(0));
        let mut c = new_clipboard();
        let paths: [u8; 5] = *b"a\0b\0c";
        let _ = write_slot(&mut c, ClipFormat::Files, &paths, 1, false);
        assert_eq!(file_entry_count(&c), 3);
    }

    #[test]
    fn a337_budget_consume_and_exhaust() {
        let mut c = new_clipboard();
        assert!(budget_consume(&mut c, 10));
        assert_eq!(copy_budget_left(&c), COPY_BUDGET - 10);
        assert!(!budget_consume(&mut c, 9999));
        assert!(c.over_budget);
    }

    #[test]
    fn a338_fuzz_clipboard_safe() {
        let sample: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        assert!(fuzz_clipboard(&sample));
    }

    #[test]
    fn a339_invariants_hold() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"k", 1);
        assert!(clip_invariants(&c));
    }

    #[test]
    fn a340_accessible_drop_works() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Text, b"a11y", 2, false);
        assert_eq!(accessible_drop(&mut c, &mut st, FILE_MANAGER_TARGET), DropResult::Dropped);
        assert!(format_present(&c, ClipFormat::Text));
    }

    #[test]
    fn a341_stats_counters() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"m", 1);
        let mut o = [0u8; CLIP_MAX_BYTES];
        let _ = paste(&mut c, ClipFormat::Text, 0, 1, &mut o);
        let s = clip_stats(&c);
        assert_eq!(s.writes, 1);
        assert_eq!(s.pastes, 1);
    }

    #[test]
    fn a342_docs_nonempty() {
        assert!(docs().len() > 10);
    }

    #[test]
    fn a343_self_closeout_ok() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"x", 1);
        assert!(clip_self_closeout(&c));
    }

    #[test]
    fn a346_dnd_telemetry_counts() {
        let mut c = new_clipboard();
        let mut st = drag_start(&mut c, ClipFormat::Text, b"t", 1, false);
        drag_move(&mut st, 0, 0, true);
        let _ = drag_drop(&mut c, &mut st);
        let mut st2 = drag_start(&mut c, ClipFormat::Text, b"t", 1, false);
        drag_cancel(&mut c, &mut st2);
        let t = dnd_telemetry(&c);
        assert_eq!(t.drops, 1);
        assert_eq!(t.cancels, 1);
    }

    #[test]
    fn a347_fuzz_dnd_safe() {
        let mut c = new_clipboard();
        let sample: [u8; 16] = [3, 1, 0, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        assert!(fuzz_dnd(&mut c, &sample));
    }

    #[test]
    fn a348_docs_dnd_nonempty() {
        assert!(docs_dnd().len() > 10);
    }

    #[test]
    fn a349_degrade_text_only() {
        let mut c = new_clipboard();
        let _ = budget_consume(&mut c, 9999);
        assert_eq!(degrade_mode(&c), DegradeMode::TextOnly);
        assert!(!copy(&mut c, ClipFormat::Image, b"i", 1));
        assert_eq!(c.degraded_copies, 1);
    }

    #[test]
    fn a350_final_check_ok() {
        let mut c = new_clipboard();
        let _ = copy(&mut c, ClipFormat::Text, b"fin", 1);
        assert!(clipboard_final_check(&c));
    }

    #[test]
    fn a344_self_check_suite_passes() {
        let set = run_clipboard_checks();
        assert!(set.all_passed(), "some clipboard checks failed");
        assert!(set.len() >= 25);
    }
}
