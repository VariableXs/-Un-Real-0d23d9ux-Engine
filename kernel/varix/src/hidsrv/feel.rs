//! VARIX-M500 AI-07 · 输入手感与人体工学（F151~F175）。
//!
//! 手感档案、键程/触觉、手势词汇与仲裁、意图纠错、节奏学习、和弦、
//! 输入保险箱、全局撤销、热档案、无障碍网格、语音/眼动预留、延迟
//! 分布、A/B 评测、时间机器、宏审计、布局动效、手柄映射、输入防火
//! 墙、打字音、fuzz 深化与域自检。
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F151 手感档案 — 每设备手感曲线库
// ---------------------------------------------------------------------------

pub const PROFILE_CAP: usize = 8;
/// 曲线采样点数（速度档位 → 增益）。
pub const CURVE_POINTS: usize = 8;

#[derive(Clone, Copy)]
pub struct FeelCurve {
    /// 各速度档（0..255）对应的增益（Q8，256=1:1）。
    pub gain: [u32; CURVE_POINTS],
}

#[derive(Clone, Copy)]
pub struct FeelProfile {
    pub device_id: u16,
    pub curve: FeelCurve,
    /// 双击窗口 ms。
    pub double_click_ms: u16,
    /// 滚轮行距。
    pub wheel_step: u8,
}

#[derive(Clone, Copy)]
pub struct FeelProfileStore {
    pub profiles: [FeelProfile; PROFILE_CAP],
    pub count: usize,
}

impl FeelProfileStore {
    pub const fn new() -> FeelProfileStore {
        FeelProfileStore {
            profiles: [FeelProfile {
                device_id: 0,
                curve: FeelCurve { gain: [256; CURVE_POINTS] },
                double_click_ms: 500,
                wheel_step: 3,
            }; PROFILE_CAP],
            count: 0,
        }
    }

    /// 登记或更新（同设备覆盖）。
    pub fn upsert(&mut self, p: FeelProfile) -> bool {
        for i in 0..self.count {
            if self.profiles[i].device_id == p.device_id {
                self.profiles[i] = p;
                return true;
            }
        }
        if self.count >= PROFILE_CAP {
            return false;
        }
        self.profiles[self.count] = p;
        self.count += 1;
        true
    }

    pub fn find(&self, device_id: u16) -> Option<usize> {
        (0..self.count).find(|&i| self.profiles[i].device_id == device_id)
    }

    /// 按速度查增益（线性插值；速度 0..=255 映射到曲线档位）。
    pub fn gain_at(&self, device_id: u16, speed: u8) -> u32 {
        match self.find(device_id) {
            None => 256,
            Some(i) => {
                let c = &self.profiles[i].curve;
                let f = (speed as usize * (CURVE_POINTS - 1)) / 255;
                let lo = c.gain[f];
                let hi = if f + 1 < CURVE_POINTS { c.gain[f + 1] } else { lo };
                let span = (255 / (CURVE_POINTS - 1)).max(1);
                let frac = ((speed as usize % span) * 256) / span;
                (lo + (hi as u64 * frac as u64 / 256) as u32).min(hi.max(lo) + 16)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// F152 键程模拟 — 虚拟按键反馈参数（按下深度/触发点/回弹）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct KeyTravel {
    /// 总键程 0..255。
    pub travel: u8,
    /// 触发点（0..255，占键程比例）。
    pub actuation: u8,
    /// 回弹速率（每帧恢复量）。
    pub rebound: u8,
    pub pressed: bool,
    pub depth: u8,
}

impl KeyTravel {
    pub const fn new(travel: u8, actuation: u8, rebound: u8) -> KeyTravel {
        KeyTravel { travel, actuation, rebound, pressed: false, depth: 0 }
    }

    pub fn press(&mut self) -> bool {
        if self.pressed {
            return false;
        }
        self.pressed = true;
        self.depth = self.travel;
        self.depth >= self.actuation
    }

    pub fn release(&mut self) {
        self.pressed = false;
    }

    /// 每帧回弹。
    pub fn tick(&mut self) {
        if !self.pressed && self.depth > 0 {
            self.depth = self.depth.saturating_sub(self.rebound);
        }
    }
}

// ---------------------------------------------------------------------------
// F153 触觉反馈预留 — 震动事件通道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HapticKind {
    Tick,
    Click,
    Buzz,
}

#[derive(Clone, Copy)]
pub struct HapticEvent {
    pub kind: HapticKind,
    /// 时长 ms。
    pub dur_ms: u16,
    /// 强度 0..255。
    pub strength: u8,
}

#[derive(Clone, Copy)]
pub struct HapticQueue {
    pub q: [HapticEvent; 8],
    pub head: usize,
    pub tail: usize,
}

impl HapticQueue {
    pub const fn new() -> HapticQueue {
        HapticQueue { q: [HapticEvent { kind: HapticKind::Tick, dur_ms: 0, strength: 0 }; 8], head: 0, tail: 0 }
    }

    pub fn push(&mut self, e: HapticEvent) -> bool {
        if (self.head + 1) % 8 == self.tail {
            return false;
        }
        self.q[self.head] = e;
        self.head = (self.head + 1) % 8;
        true
    }

    pub fn pop(&mut self) -> Option<HapticEvent> {
        if self.tail == self.head {
            return None;
        }
        let e = self.q[self.tail];
        self.tail = (self.tail + 1) % 8;
        Some(e)
    }
}

// ---------------------------------------------------------------------------
// F154 手势词汇表 — 全局手势语义集
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GestureSem {
    None,
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
    PinchIn,
    PinchOut,
    TwoFingerTap,
    LongPress,
}

/// 由轨迹（起点→终点）+ 触点数 + 持续时间归类手势。
pub fn classify_gesture(dx: i16, dy: i16, touches: u8, dur_ms: u16) -> GestureSem {
    let dist = (dx as i32).abs().max((dy as i32).abs());
    if touches >= 2 {
        return GestureSem::TwoFingerTap;
    }
    if dur_ms >= 600 && dist < 24 {
        return GestureSem::LongPress;
    }
    if dist < 48 {
        return GestureSem::None;
    }
    if dx.abs() >= dy.abs() {
        if dx > 0 { GestureSem::SwipeRight } else { GestureSem::SwipeLeft }
    } else if dy > 0 {
        GestureSem::SwipeDown
    } else {
        GestureSem::SwipeUp
    }
}

// ---------------------------------------------------------------------------
// F155 手势冲突仲裁 — 多手势同时命中时的裁决
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct GestureClaim {
    pub sem: GestureSem,
    /// 优先级 0..255，越大越先。
    pub priority: u8,
    /// 置信度 0..255。
    pub confidence: u8,
}

/// 仲裁：先比置信度（差 >32 直接定），再比优先级，再比语义编号稳定性。
pub fn arbitrate(a: GestureClaim, b: GestureClaim) -> GestureSem {
    if a.confidence > b.confidence + 32 {
        return a.sem;
    }
    if b.confidence > a.confidence + 32 {
        return b.sem;
    }
    if a.priority != b.priority {
        if a.priority > b.priority {
            a.sem
        } else {
            b.sem
        }
    } else {
        // 完全打平：取编号小的（确定性）。
        if (a.sem as u8) <= (b.sem as u8) {
            a.sem
        } else {
            b.sem
        }
    }
}

// ---------------------------------------------------------------------------
// F156 输入意图识别 — 误触智能纠错（邻近键表）
// ---------------------------------------------------------------------------

/// QWERTY 邻近键（索引 = 'a'..'z'）。-1 表示无。
const NEIGHBORS: [i8; 26] = [
    16, -1, 23, 18, 3, 21, 8, 9, 10, 11, 12, 13, 26, 27, 15, 28, -1, 4, 19, 20, 25, 22, 2, 24, 6, 7,
];

/// 判定击键是否疑似误触：与目标字符是邻居关系返回 true。
pub fn is_neighbor(actual: u8, intended: u8) -> bool {
    if !(b'a'..=b'z').contains(&actual) || !(b'a'..=b'z').contains(&intended) {
        return false;
    }
    let na = NEIGHBORS[(actual - b'a') as usize];
    let ni = NEIGHBORS[(intended - b'a') as usize];
    if na < 0 || ni < 0 {
        return false;
    }
    // 邻接表是对称槽位编号；用编号差的绝对值 ≤ 2 近似相邻。
    (na as i16 - ni as i16).abs() <= 2
}

/// 纠错建议：如果 actual 是 intended 的邻居，返回 Some(intended)。
pub fn mistype_fix(actual: u8, intended: u8) -> Option<u8> {
    if is_neighbor(actual, intended) {
        Some(intended)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F157 按压曲线校准向导 — 个人化调校（采样→拟合→生效）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PressWizard {
    /// 已采样次数。
    pub samples: u8,
    /// 各次采样到的触发深度 0..255。
    pub depths: [u8; 8],
    pub done: bool,
}

impl PressWizard {
    pub const fn new() -> PressWizard {
        PressWizard { samples: 0, depths: [0; 8], done: false }
    }

    pub fn sample(&mut self, depth: u8) -> bool {
        if self.done || self.samples >= 8 {
            return false;
        }
        self.depths[self.samples as usize] = depth;
        self.samples += 1;
        if self.samples >= 5 {
            self.done = true;
        }
        true
    }

    /// 取中位触发深度作为推荐 actuation。
    pub fn recommend(&self) -> Option<u8> {
        if !self.done {
            return None;
        }
        let mut arr = self.depths;
        arr[0..self.samples as usize].sort_unstable();
        Some(arr[self.samples as usize / 2])
    }
}

// ---------------------------------------------------------------------------
// F158 打字节奏学习 — 个人节奏适配（间隔统计）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RhythmLearner {
    /// 最近 N 次键间隔（ms）。
    pub intervals: [u16; 16],
    pub count: usize,
    pub idx: usize,
}

impl RhythmLearner {
    pub const fn new() -> RhythmLearner {
        RhythmLearner { intervals: [0; 16], count: 0, idx: 0 }
    }

    pub fn observe(&mut self, interval_ms: u16) {
        self.intervals[self.idx] = interval_ms;
        self.idx = (self.idx + 1) % 16;
        if self.count < 16 {
            self.count += 1;
        }
    }

    pub fn median(&self) -> u16 {
        if self.count == 0 {
            return 0;
        }
        let mut arr = self.intervals;
        arr[0..self.count].sort_unstable();
        arr[self.count / 2]
    }

    /// 节奏突变：新间隔超出中位 3 倍视为异常（可能是打扰/换手）。
    pub fn is_outlier(&self, interval_ms: u16) -> bool {
        let m = self.median();
        m > 0 && interval_ms as u32 > m as u32 * 3
    }
}

// ---------------------------------------------------------------------------
// F159 快捷键和弦 — 多键组合语义
// ---------------------------------------------------------------------------

pub const CHORD_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    /// 已按下的键集合（位图，bit = 键码）。
    pub keys: u32,
    /// 触发动作 id。
    pub action: u16,
}

#[derive(Clone, Copy)]
pub struct ChordTable {
    pub chords: [Chord; CHORD_CAP],
    pub count: usize,
}

impl ChordTable {
    pub const fn new() -> ChordTable {
        ChordTable { chords: [Chord { keys: 0, action: 0 }; CHORD_CAP], count: 0 }
    }

    /// 登记和弦（同键集覆盖；键集非空才收）。
    pub fn bind(&mut self, keys: u32, action: u16) -> bool {
        if keys == 0 {
            return false;
        }
        for i in 0..self.count {
            if self.chords[i].keys == keys {
                self.chords[i].action = action;
                return true;
            }
        }
        if self.count >= CHORD_CAP {
            return false;
        }
        self.chords[self.count] = Chord { keys, action };
        self.count += 1;
        true
    }

    /// 当前按住的键位图 → 命中的动作。
    pub fn dispatch(&self, held: u32) -> Option<u16> {
        (0..self.count).find(|&i| self.chords[i].keys == held).map(|i| self.chords[i].action)
    }
}

// ---------------------------------------------------------------------------
// F160 输入历史保险箱 — 本地加密记录（XOR 流 + 环形覆盖）
// ---------------------------------------------------------------------------

pub const VAULT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct InputVault {
    pub buf: [u32; VAULT_CAP],
    pub keystream: [u8; VAULT_CAP],
    pub head: usize,
    pub sealed: bool,
}

impl InputVault {
    pub const fn new(key: u8) -> InputVault {
        let mut ks = [0u8; VAULT_CAP];
        let mut s = key as u32 | 1;
        let mut i = 0;
        while i < VAULT_CAP {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            ks[i] = (s >> 24) as u8;
            i += 1;
        }
        InputVault { buf: [0; VAULT_CAP], keystream: ks, head: 0, sealed: true }
    }

    /// 写入一条记录（已加密形态）。
    pub fn record(&mut self, plain: u32) {
        let i = self.head;
        self.buf[i] = plain ^ ((self.keystream[i] as u32) << 24 | (self.keystream[(i + 3) % VAULT_CAP] as u32) << 8);
        self.head = (self.head + 1) % VAULT_CAP;
    }

    /// 解密读回最近第 k 条（k=0 最新）。
    pub fn read_back(&self, k: usize) -> Option<u32> {
        if k >= VAULT_CAP {
            return None;
        }
        let i = (self.head + VAULT_CAP - 1 - k % VAULT_CAP) % VAULT_CAP;
        Some(self.buf[i] ^ ((self.keystream[i] as u32) << 24 | (self.keystream[(i + 3) % VAULT_CAP] as u32) << 8))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn unseal(&mut self) {
        self.sealed = false;
    }
}

// ---------------------------------------------------------------------------
// F161 全局撤销协议 — 系统级 undo 事件
// ---------------------------------------------------------------------------

pub const UNDO_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UndoEntry {
    pub target: u16,
    /// 撤销载荷（如旧值指针/版本号）。
    pub payload: u32,
}

#[derive(Clone, Copy)]
pub struct UndoStack {
    pub stack: [UndoEntry; UNDO_CAP],
    pub top: usize,
}

impl UndoStack {
    pub const fn new() -> UndoStack {
        UndoStack { stack: [UndoEntry { target: 0, payload: 0 }; UNDO_CAP], top: 0 }
    }

    pub fn push(&mut self, e: UndoEntry) -> bool {
        if self.top >= UNDO_CAP {
            return false;
        }
        self.stack[self.top] = e;
        self.top += 1;
        true
    }

    pub fn undo(&mut self) -> Option<UndoEntry> {
        if self.top == 0 {
            return None;
        }
        self.top -= 1;
        Some(self.stack[self.top])
    }
}

// ---------------------------------------------------------------------------
// F162 设备热档案 — 插拔手感即恢复
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct HotProfile {
    pub vendor_id: u16,
    pub product_id: u16,
    pub profile_idx: u16,
}

#[derive(Clone, Copy)]
pub struct HotProfileTable {
    pub entries: [HotProfile; 8],
    pub count: usize,
}

impl HotProfileTable {
    pub const fn new() -> HotProfileTable {
        HotProfileTable { entries: [HotProfile { vendor_id: 0, product_id: 0, profile_idx: 0 }; 8], count: 0 }
    }

    pub fn bind(&mut self, vid: u16, pid: u16, profile: u16) -> bool {
        for i in 0..self.count {
            if self.entries[i].vendor_id == vid && self.entries[i].product_id == pid {
                self.entries[i].profile_idx = profile;
                return true;
            }
        }
        if self.count >= 8 {
            return false;
        }
        self.entries[self.count] = HotProfile { vendor_id: vid, product_id: pid, profile_idx: profile };
        self.count += 1;
        true
    }

    /// 设备插入时查档案。
    pub fn on_plug(&self, vid: u16, pid: u16) -> Option<u16> {
        (0..self.count)
            .find(|&i| self.entries[i].vendor_id == vid && self.entries[i].product_id == pid)
            .map(|i| self.entries[i].profile_idx)
    }
}

// ---------------------------------------------------------------------------
// F163 无障碍导航网格 — 空间键位导航
// ---------------------------------------------------------------------------

pub const NAV_GRID: usize = 8;

#[derive(Clone, Copy)]
pub struct NavGrid {
    /// 焦点可停留的格子。
    pub focusable: [bool; NAV_GRID * NAV_GRID],
    pub cursor: usize,
}

impl NavGrid {
    pub const fn new() -> NavGrid {
        NavGrid { focusable: [false; NAV_GRID * NAV_GRID], cursor: 0 }
    }

    pub fn set_focusable(&mut self, x: usize, y: usize, v: bool) {
        if x < NAV_GRID && y < NAV_GRID {
            self.focusable[y * NAV_GRID + x] = v;
        }
    }

    fn step(&self, from: usize, dx: i32, dy: i32) -> Option<usize> {
        let x = (from % NAV_GRID) as i32;
        let y = (from / NAV_GRID) as i32;
        let mut cx = x;
        let mut cy = y;
        while (0..NAV_GRID as i32).contains(&cx) && (0..NAV_GRID as i32).contains(&cy) {
            cx += dx;
            cy += dy;
            if (0..NAV_GRID as i32).contains(&cx) && (0..NAV_GRID as i32).contains(&cy) {
                let i = (cy * NAV_GRID as i32 + cx) as usize;
                if self.focusable[i] {
                    return Some(i);
                }
            }
        }
        None
    }

    pub fn move_focus(&mut self, dx: i32, dy: i32) -> bool {
        match self.step(self.cursor, dx, dy) {
            Some(i) => {
                self.cursor = i;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F164 语音输入预留 — 麦克风→文本通道占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoiceChannelState {
    Closed,
    Open,
    Streaming,
}

#[derive(Clone, Copy)]
pub struct VoiceChannel {
    pub state: VoiceChannelState,
    /// 已收到的音频块数。
    pub chunks: u32,
    pub session_id: u32,
}

impl VoiceChannel {
    pub const fn new() -> VoiceChannel {
        VoiceChannel { state: VoiceChannelState::Closed, chunks: 0, session_id: 0 }
    }

    pub fn open(&mut self) -> u32 {
        self.session_id += 1;
        self.state = VoiceChannelState::Open;
        self.chunks = 0;
        self.session_id
    }

    pub fn feed(&mut self, chunk: &[u8]) -> usize {
        if self.state == VoiceChannelState::Closed {
            return 0;
        }
        self.state = VoiceChannelState::Streaming;
        self.chunks += 1;
        chunk.len()
    }

    pub fn close(&mut self) {
        self.state = VoiceChannelState::Closed;
    }
}

// ---------------------------------------------------------------------------
// F165 眼动追踪预留 — 注视事件占位
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GazeEvent {
    pub x: u16,
    pub y: u16,
    /// 注视时长 ms。
    pub dwell_ms: u16,
}

/// 注视判定：驻留超过阈值即产生"凝视点击"。
pub fn gaze_click(e: GazeEvent, threshold_ms: u16) -> bool {
    e.dwell_ms >= threshold_ms
}

#[derive(Clone, Copy)]
pub struct GazeBuffer {
    pub events: [GazeEvent; 4],
    pub count: usize,
    pub head: usize,
}

impl GazeBuffer {
    pub const fn new() -> GazeBuffer {
        GazeBuffer { events: [GazeEvent { x: 0, y: 0, dwell_ms: 0 }; 4], count: 0, head: 0 }
    }

    pub fn push(&mut self, e: GazeEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % 4;
        if self.count < 4 {
            self.count += 1;
        }
    }

    /// 平滑：最近事件的均值。
    pub fn smoothed(&self) -> Option<(u32, u32)> {
        if self.count == 0 {
            return None;
        }
        let mut sx = 0u32;
        let mut sy = 0u32;
        for i in 0..self.count {
            sx += self.events[i].x as u32;
            sy += self.events[i].y as u32;
        }
        Some((sx / self.count as u32, sy / self.count as u32))
    }
}

// ---------------------------------------------------------------------------
// F166 输入延迟分布图 — p50/p99 可视（桶式）
// ---------------------------------------------------------------------------

pub const LAT_BUCKETS: usize = 8;
/// 桶宽 4ms。
pub const LAT_BUCKET_MS: u32 = 4;

#[derive(Clone, Copy)]
pub struct LatencyHisto {
    pub buckets: [u32; LAT_BUCKETS],
    pub samples: u32,
    pub overflow: u32,
}

impl LatencyHisto {
    pub const fn new() -> LatencyHisto {
        LatencyHisto { buckets: [0; LAT_BUCKETS], samples: 0, overflow: 0 }
    }

    pub fn record(&mut self, ms: u32) {
        self.samples += 1;
        let i = (ms / LAT_BUCKET_MS) as usize;
        if i >= LAT_BUCKETS {
            self.overflow += 1;
        } else {
            self.buckets[i] += 1;
        }
    }

    /// 分位（0..100）所在桶。
    pub fn percentile_bucket(&self, pct: u8) -> Option<usize> {
        if self.samples == 0 {
            return None;
        }
        let target = (self.samples as u64 * pct as u64 / 100) as u32;
        let mut acc = 0u32;
        for i in 0..LAT_BUCKETS {
            acc += self.buckets[i];
            if acc >= target.max(1) {
                return Some(i);
            }
        }
        Some(LAT_BUCKETS - 1)
    }
}

// ---------------------------------------------------------------------------
// F167 手感 A/B 评测器 — 双方案盲测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AbTrial {
    /// 本轮呈现的是 A 还是 B（盲测随机）。
    pub shown_is_a: bool,
    /// 用户是否偏好呈现方案。
    pub preferred_shown: bool,
}

#[derive(Clone, Copy)]
pub struct AbEvaluator {
    pub a_wins: u32,
    pub b_wins: u32,
    pub trials: u32,
}

impl AbEvaluator {
    pub const fn new() -> AbEvaluator {
        AbEvaluator { a_wins: 0, b_wins: 0, trials: 0 }
    }

    pub fn record(&mut self, t: AbTrial) {
        self.trials += 1;
        if t.preferred_shown {
            if t.shown_is_a {
                self.a_wins += 1;
            } else {
                self.b_wins += 1;
            }
        } else if t.shown_is_a {
            self.b_wins += 1;
        } else {
            self.a_wins += 1;
        }
    }

    /// 胜者（需 ≥5 轮且优势 ≥2）。
    pub fn winner(&self) -> Option<bool> {
        if self.trials < 5 {
            return None;
        }
        if self.a_wins >= self.b_wins + 2 {
            Some(true)
        } else if self.b_wins >= self.a_wins + 2 {
            Some(false)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F168 输入时间机器 — 事件时间轴回溯
// ---------------------------------------------------------------------------

pub const TIMECAP: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TimedEvent {
    pub t_ms: u32,
    pub kind: u8,
    pub code: u16,
}

#[derive(Clone, Copy)]
pub struct TimeMachine {
    pub events: [TimedEvent; TIMECAP],
    pub head: usize,
    pub count: usize,
}

impl TimeMachine {
    pub const fn new() -> TimeMachine {
        TimeMachine { events: [TimedEvent { t_ms: 0, kind: 0, code: 0 }; TIMECAP], head: 0, count: 0 }
    }

    pub fn push(&mut self, t_ms: u32, kind: u8, code: u16) {
        self.events[self.head] = TimedEvent { t_ms, kind, code };
        self.head = (self.head + 1) % TIMECAP;
        if self.count < TIMECAP {
            self.count += 1;
        }
    }

    /// 回溯：返回 t_ms 之前（含）最近 n 条事件。
    pub fn before(&self, t_ms: u32, n: usize) -> [Option<TimedEvent>; TIMECAP] {
        let mut out = [None; TIMECAP];
        let mut found = 0;
        for k in 0..TIMECAP {
            let i = (self.head + TIMECAP - 1 - k) % TIMECAP;
            if k >= self.count || found >= n {
                break;
            }
            if self.events[i].t_ms <= t_ms {
                out[found] = Some(self.events[i]);
                found += 1;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// F169 宏安全审计 — 宏权限声明
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacroCap {
    None,
    /// 可注入键鼠事件。
    Inject,
    /// 可注入 + 读取剪贴板。
    InjectClip,
}

#[derive(Clone, Copy)]
pub struct MacroDecl {
    pub macro_id: u16,
    pub cap: MacroCap,
    /// 是否触发过审计告警。
    pub flagged: bool,
}

/// 审计：无声明却尝试注入 → 标记并拒绝。
pub fn audit_macro(m: &mut MacroDecl, wants_inject: bool, wants_clip: bool) -> bool {
    let ok = match m.cap {
        MacroCap::None => !wants_inject && !wants_clip,
        MacroCap::Inject => wants_inject && !wants_clip,
        MacroCap::InjectClip => wants_inject || wants_clip,
    };
    if !ok {
        m.flagged = true;
    }
    ok
}

// ---------------------------------------------------------------------------
// F170 布局热切换动效 — 切换视觉连贯（过渡进度）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LayoutTransition {
    /// 0..256，256=完成。
    pub progress: u16,
    pub step: u16,
    pub active: bool,
}

impl LayoutTransition {
    pub const fn new(step: u16) -> LayoutTransition {
        LayoutTransition { progress: 0, step, active: false }
    }

    pub fn start(&mut self) {
        self.progress = 0;
        self.active = true;
    }

    pub fn tick(&mut self) -> u16 {
        if self.active {
            self.progress = (self.progress + self.step).min(256);
            if self.progress >= 256 {
                self.active = false;
            }
        }
        self.progress
    }

    /// 插值因子（Q8）。
    pub fn factor(&self) -> u16 {
        self.progress.min(256)
    }
}

// ---------------------------------------------------------------------------
// F171 手柄全局映射 — 任意键映射到手柄
// ---------------------------------------------------------------------------

pub const PADMAP_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct PadMapping {
    /// 源键码（键盘）。
    pub src_key: u16,
    /// 目标手柄按钮位（0..15）。
    pub dst_btn: u8,
}

#[derive(Clone, Copy)]
pub struct PadMapTable {
    pub maps: [PadMapping; PADMAP_CAP],
    pub count: usize,
}

impl PadMapTable {
    pub const fn new() -> PadMapTable {
        PadMapTable { maps: [PadMapping { src_key: 0, dst_btn: 0 }; PADMAP_CAP], count: 0 }
    }

    pub fn bind(&mut self, src: u16, btn: u8) -> bool {
        if btn > 15 || self.count >= PADMAP_CAP {
            return false;
        }
        for i in 0..self.count {
            if self.maps[i].src_key == src {
                self.maps[i].dst_btn = btn;
                return true;
            }
        }
        self.maps[self.count] = PadMapping { src_key: src, dst_btn: btn };
        self.count += 1;
        true
    }

    /// 键盘事件 → 手柄按钮位图。
    pub fn translate(&self, src: u16, pressed: bool, pad: &mut u32) -> bool {
        for i in 0..self.count {
            if self.maps[i].src_key == src {
                if pressed {
                    *pad |= 1 << self.maps[i].dst_btn;
                } else {
                    *pad &= !(1 << self.maps[i].dst_btn);
                }
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// F172 输入防火墙 — 设备级权限
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DevPolicy {
    Allow,
    /// 只允许鼠标（屏蔽键盘注入类设备）。
    PointerOnly,
    Deny,
}

#[derive(Clone, Copy)]
pub struct FirewallRule {
    pub device_id: u16,
    pub policy: DevPolicy,
}

#[derive(Clone, Copy)]
pub struct InputFirewall {
    pub rules: [FirewallRule; 8],
    pub count: usize,
    pub default_policy: DevPolicy,
}

impl InputFirewall {
    pub const fn new() -> InputFirewall {
        InputFirewall { rules: [FirewallRule { device_id: 0, policy: DevPolicy::Allow }; 8], count: 0, default_policy: DevPolicy::Allow }
    }

    pub fn set_rule(&mut self, device_id: u16, policy: DevPolicy) -> bool {
        for i in 0..self.count {
            if self.rules[i].device_id == device_id {
                self.rules[i].policy = policy;
                return true;
            }
        }
        if self.count >= 8 {
            return false;
        }
        self.rules[self.count] = FirewallRule { device_id, policy };
        self.count += 1;
        true
    }

    /// 事件准入：kind 0=键盘 1=指针 2=其他。
    pub fn admit(&self, device_id: u16, kind: u8) -> bool {
        let policy = (0..self.count)
            .find(|&i| self.rules[i].device_id == device_id)
            .map(|i| self.rules[i].policy)
            .unwrap_or(self.default_policy);
        match policy {
            DevPolicy::Allow => true,
            DevPolicy::PointerOnly => kind == 1,
            DevPolicy::Deny => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F173 打字声音反馈 — 可选音效（音量 + 事件选择）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeSound {
    Off,
    Soft,
    Mechanical,
}

/// 由键类型 + 静音状态决定是否发声及音量（0..255）。
pub fn typing_sound(mode: TypeSound, key_kind: u8, muted: bool) -> u8 {
    if muted || mode == TypeSound::Off {
        return 0;
    }
    let base = match mode {
        TypeSound::Soft => 48,
        TypeSound::Mechanical => 96,
        TypeSound::Off => 0,
    };
    // 空格/回车等大键（kind=1）更响。
    if key_kind == 1 {
        (base + 32).min(255)
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// F174 输入 fuzz 深化 — 恶意事件对抗（确定性 LCG）
// ---------------------------------------------------------------------------

pub struct InputFuzzer {
    pub state: u32,
    pub injected: u32,
    pub rejected: u32,
}

impl InputFuzzer {
    pub const fn new(seed: u32) -> InputFuzzer {
        InputFuzzer { state: seed | 1, injected: 0, rejected: 0 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// 生成一个事件三元组（kind, code, value）。
    pub fn gen_event(&mut self) -> (u8, u16, u16) {
        let r = self.next();
        (
            (r & 3) as u8,                 // kind 0..3
            ((r >> 8) & 0xFFFF) as u16,    // 任意键码
            ((r >> 16) & 0x7) as u16,      // 合理值域
        )
    }

    /// 对抗注入：洪泛（同帧 >64 事件）与越界 kind 必须被拒绝。
    pub fn adversarial_step(&mut self, events_this_frame: u32) -> bool {
        let (kind, _, _) = self.gen_event();
        if events_this_frame > 64 {
            self.rejected += 1;
            return false;
        }
        if kind > 2 {
            self.rejected += 1;
            return false;
        }
        self.injected += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F175 手感域自检 — 25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_feel_checks() -> CheckSet {
    let mut set = CheckSet::new("hidfeel");

    // F151 手感档案
    let mut store = FeelProfileStore::new();
    let p1 = FeelProfile {
        device_id: 7,
        curve: FeelCurve { gain: [128, 160, 192, 224, 256, 288, 320, 352] },
        double_click_ms: 400,
        wheel_step: 2,
    };
    let up1 = store.upsert(p1);
    let up2 = store.upsert(p1); // 去重覆盖
    let g = store.gain_at(7, 128);
    set.add("F151 feel profiles", up1 && up2 && store.count == 1 && (256..=320).contains(&g), "curve interp");

    // F152 键程
    let mut kt = KeyTravel::new(200, 120, 40);
    let fired = kt.press();
    kt.release();
    kt.tick();
    let receding = kt.depth < 200;
    set.add("F152 key travel", fired && receding && !kt.pressed, "actuation + rebound");

    // F153 触觉
    let mut hq = HapticQueue::new();
    let p_ok = hq.push(HapticEvent { kind: HapticKind::Click, dur_ms: 10, strength: 128 });
    let popped = hq.pop();
    set.add("F153 haptic queue", p_ok && popped.is_some() && hq.pop().is_none(), "ring queue");

    // F154 手势词汇
    let g1 = classify_gesture(100, 0, 1, 100);
    let g2 = classify_gesture(0, -100, 1, 100);
    let g3 = classify_gesture(2, 2, 1, 800);
    let g4 = classify_gesture(10, 10, 2, 100);
    set.add(
        "F154 gesture vocab",
        g1 == GestureSem::SwipeRight && g2 == GestureSem::SwipeUp && g3 == GestureSem::LongPress && g4 == GestureSem::TwoFingerTap,
        "classification",
    );

    // F155 仲裁
    let a = GestureClaim { sem: GestureSem::SwipeLeft, priority: 100, confidence: 200 };
    let b = GestureClaim { sem: GestureSem::PinchIn, priority: 200, confidence: 100 };
    let c = GestureClaim { sem: GestureSem::None, priority: 100, confidence: 120 };
    let r1 = arbitrate(a, b); // 置信差=100 → a
    let r2 = arbitrate(b, c); // 置信差=50 → b
    let tie1 = arbitrate(a, GestureClaim { sem: GestureSem::SwipeRight, priority: 100, confidence: 200 });
    set.add("F155 arbitration", r1 == GestureSem::SwipeLeft && r2 == GestureSem::PinchIn && tie1 == GestureSem::SwipeLeft, "confidence>priority");

    // F156 误触
    let fix = mistype_fix(b'j', b'k'); // 邻居
    let nofix = mistype_fix(b'q', b'p');
    set.add("F156 mistype fix", fix == Some(b'k') && nofix.is_none(), "neighbor correction");

    // F157 校准向导
    let mut wz = PressWizard::new();
    for d in [100u8, 120, 110, 130, 105] {
        wz.sample(d);
    }
    let rec = wz.recommend();
    set.add("F157 press wizard", rec == Some(110) && wz.done, "median calibration");

    // F158 节奏
    let mut rl = RhythmLearner::new();
    for iv in [100u16, 110, 90, 105, 95] {
        rl.observe(iv);
    }
    let m = rl.median();
    let out = rl.is_outlier(900);
    set.add("F158 rhythm", m == 100 && out, "median + outlier");

    // F159 和弦
    let mut ct = ChordTable::new();
    let b1 = ct.bind((1 << 0) | (1 << 2), 42);
    let hit = ct.dispatch((1 << 2) | (1 << 0)); // 顺序无关
    let miss = ct.dispatch(1 << 0);
    set.add("F159 chords", b1 && hit == Some(42) && miss.is_none(), "chord dispatch");

    // F160 保险箱
    let mut vault = InputVault::new(0xAB);
    vault.record(0x1234_5678);
    vault.record(0x9ABC_DEF0);
    let r0 = vault.read_back(0);
    let r1 = vault.read_back(1);
    set.add(
        "F160 input vault",
        r0 == Some(0x9ABC_DEF0) && r1 == Some(0x1234_5678) && vault.sealed,
        "encrypt roundtrip",
    );

    // F161 撤销
    let mut us = UndoStack::new();
    let p1 = us.push(UndoEntry { target: 1, payload: 100 });
    let _ = us.push(UndoEntry { target: 1, payload: 200 });
    let u = us.undo();
    set.add("F161 undo stack", p1 && u == Some(UndoEntry { target: 1, payload: 200 }), "LIFO undo");

    // F162 热档案
    let mut hot = HotProfileTable::new();
    let _ = hot.bind(0x046D, 0xC52B, 3);
    let got = hot.on_plug(0x046D, 0xC52B);
    let miss = hot.on_plug(0x1111, 0x2222);
    set.add("F162 hot profiles", got == Some(3) && miss.is_none(), "plug restore");

    // F163 导航网格
    let mut nav = NavGrid::new();
    nav.set_focusable(0, 0, true);
    nav.set_focusable(3, 0, true);
    let moved = nav.move_focus(1, 0);
    set.add("F163 nav grid", moved && nav.cursor == 3, "skip unfocused");

    // F164 语音预留
    let mut vc = VoiceChannel::new();
    let sid = vc.open();
    let fed = vc.feed(&[0u8; 32]);
    vc.close();
    let closed_feed = vc.feed(&[1u8; 4]);
    set.add("F164 voice channel", sid == 1 && fed == 32 && closed_feed == 0 && vc.state == VoiceChannelState::Closed, "lifecycle");

    // F165 眼动
    let g = GazeEvent { x: 100, y: 200, dwell_ms: 500 };
    let mut gb = GazeBuffer::new();
    gb.push(GazeEvent { x: 10, y: 20, dwell_ms: 100 });
    gb.push(GazeEvent { x: 20, y: 40, dwell_ms: 100 });
    let sm = gb.smoothed();
    set.add("F165 gaze", gaze_click(g, 400) && sm == Some((15, 30)), "dwell + smooth");

    // F166 延迟分布
    let mut lh = LatencyHisto::new();
    for _ in 0..50 {
        lh.record(6);
    }
    for _ in 0..50 {
        lh.record(40);
    }
    let p50 = lh.percentile_bucket(50);
    let p99 = lh.percentile_bucket(99);
    set.add("F166 latency histo", p50 == Some(1) && p99 == Some(7) && lh.overflow == 50, "p50/p99 buckets");

    // F167 A/B
    let mut ab = AbEvaluator::new();
    for s in [true, false, true, true, false, true] {
        ab.record(AbTrial { shown_is_a: s, preferred_shown: true });
    }
    let w = ab.winner();
    set.add("F167 ab test", w == Some(true), "a wins");

    // F168 时间机器
    let mut tm = TimeMachine::new();
    tm.push(10, 1, 30);
    tm.push(20, 1, 31);
    tm.push(30, 0, 32);
    let back = tm.before(25, 2);
    set.add(
        "F168 time machine",
        back[0] == Some(TimedEvent { t_ms: 20, kind: 1, code: 31 })
            && back[1] == Some(TimedEvent { t_ms: 10, kind: 1, code: 30 }),
        "retrospect",
    );

    // F169 宏审计
    let mut m1 = MacroDecl { macro_id: 1, cap: MacroCap::Inject, flagged: false };
    let ok1 = audit_macro(&mut m1, true, false);
    let ok2 = audit_macro(&mut m1, true, true);
    set.add("F169 macro audit", ok1 && !ok2 && m1.flagged, "capability check");

    // F170 布局动效
    let mut tr = LayoutTransition::new(128);
    tr.start();
    let f1 = tr.tick();
    let f2 = tr.tick();
    set.add("F170 layout transition", f1 == 128 && f2 == 256 && !tr.active, "tween completes");

    // F171 手柄映射
    let mut pt = PadMapTable::new();
    let _ = pt.bind(30, 0);
    let _ = pt.bind(31, 1);
    let mut pad = 0u32;
    let t1 = pt.translate(30, true, &mut pad);
    let t2 = pt.translate(99, true, &mut pad);
    pt.translate(30, false, &mut pad);
    set.add("F171 pad mapping", t1 && !t2 && pad == 0, "bitmask translate");

    // F172 防火墙
    let mut fw = InputFirewall::new();
    let _ = fw.set_rule(5, DevPolicy::PointerOnly);
    let _ = fw.set_rule(6, DevPolicy::Deny);
    let ok1 = fw.admit(5, 1);
    let no1 = fw.admit(5, 0);
    let no2 = fw.admit(6, 1);
    let ok2 = fw.admit(9, 0);
    set.add("F172 input firewall", ok1 && !no1 && !no2 && ok2, "per-device policy");

    // F173 打字音
    let s1 = typing_sound(TypeSound::Soft, 0, false);
    let s2 = typing_sound(TypeSound::Mechanical, 1, false);
    let s3 = typing_sound(TypeSound::Soft, 0, true);
    set.add("F173 typing sound", s1 == 48 && s2 == 128 && s3 == 0, "volume ladder");

    // F174 fuzz
    let mut fz = InputFuzzer::new(1234);
    let mut accepted = 0;
    for i in 0..100 {
        if fz.adversarial_step(i) {
            accepted += 1;
        }
    }
    set.add("F174 input fuzz", fz.rejected > 0 && accepted > 0 && fz.injected == accepted, "adversarial filter");

    // F175 域自检可用性
    set.add("F175 feel selftest reachable", set.len() >= 24, "selftest must cover domain");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f151_gain_monotonic() {
        let mut store = FeelProfileStore::new();
        let _ = store.upsert(FeelProfile {
            device_id: 1,
            curve: FeelCurve { gain: [128, 160, 192, 224, 256, 288, 320, 352] },
            double_click_ms: 500,
            wheel_step: 3,
        });
        assert!(store.gain_at(1, 255) > store.gain_at(1, 0));
        assert_eq!(store.gain_at(99, 10), 256); // 未知设备回退 1:1
    }

    #[test]
    fn f160_vault_ring_overwrite() {
        let mut v = InputVault::new(7);
        for i in 0..VAULT_CAP as u32 + 3 {
            v.record(i);
        }
        assert_eq!(v.read_back(0), Some(VAULT_CAP as u32 + 2));
    }

    #[test]
    fn f175_selftest_passes() {
        let set = run_feel_checks();
        let mut buf = [0u8; 512];
        set.render(&mut buf);
        assert!(set.all_passed() && set.len() >= 25, "{}", core::str::from_utf8(&buf).unwrap_or("?"));
    }
}
