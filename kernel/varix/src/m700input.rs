//! m700input — VARIX-M700 AI-14 输入设备内核域 (F326~F350)
//!
//! 输入事件信封/输入延迟仪/键盘映射中台/指针加速度谱/触控协议谱/
//! 输入事件风暴阀/输入设备档案/按键去抖官/事件序号墙/输入回放流/
//! 手势内核原语/输入功耗账/输入权限门/死键组合键谱/输入设备热插拔律/
//! 滚轮物理谱/输入延迟预算门/无障碍输入通道/输入事件考古/键盘灯效通道/
//! 输入统计分账/输入设备健康分/输入回归走廊/输入安全审计/输入域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F326 — 输入事件信封：统一事件包装
// ===========================================================================

pub const EV_KEY: u8 = 1;
pub const EV_ABS: u8 = 2;
pub const EV_REL: u8 = 3;
pub const EV_WHEEL: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub kind: u8,     // EV_*
    pub code: u16,    // 键码/轴号
    pub value: i32,   // 按下/位移/槽位值
    pub seq: u64,     // 全局单调序号
    pub stamp_us: u64, // 设备时间戳
}

impl InputEvent {
    pub fn envelope_valid(&self) -> bool {
        (EV_KEY..=EV_WHEEL).contains(&self.kind) && self.seq > 0
    }
}

// ===========================================================================
// F327 — 输入延迟仪：事件到派发的延迟统计
// ===========================================================================

pub const INPUT_LATENCY_BUDGET_US: u64 = 8000; // 8ms 预算

#[derive(Clone, Copy, Debug, Default)]
pub struct LatencyMeter {
    pub samples: u64,
    pub total_us: u64,
    pub worst_us: u64,
}

impl LatencyMeter {
    /// 采样一次：now - stamp。
    pub fn sample(&mut self, stamp_us: u64, dispatch_us: u64) {
        let d = dispatch_us.saturating_sub(stamp_us);
        self.samples += 1;
        self.total_us += d;
        if d > self.worst_us {
            self.worst_us = d;
        }
    }

    pub fn avg_us(&self) -> u64 {
        if self.samples == 0 {
            0
        } else {
            self.total_us / self.samples
        }
    }

    /// 最坏延迟占预算的 permille。
    pub fn worst_permille(&self) -> u32 {
        (self.worst_us * 1000 / INPUT_LATENCY_BUDGET_US) as u32
    }
}

// ===========================================================================
// F328 — 键盘映射中台：键码 → 字符，含 Shift 层
// ===========================================================================

/// 固定映射表：(base_code, base_char, shift_char)。
pub const KEYMAP: [(u16, u8, u8); 8] = [
    (30, b'a', b'A'),
    (48, b'b', b'B'),
    (31, b's', b'S'),
    (32, b'd', b'D'),
    (2, b'1', b'!'),
    (3, b'2', b'@'),
    (57, b' ', b' '),
    (28, b'\r', b'\r'),
];

/// 查表：shift 选择上层字符；未收录返回 None。
pub fn keymap_lookup(code: u16, shift: bool) -> Option<u8> {
    for &(c, base, upper) in KEYMAP.iter() {
        if c == code {
            return Some(if shift { upper } else { base });
        }
    }
    None
}

// ===========================================================================
// F329 — 指针加速度谱：定点增益曲线
// ===========================================================================

pub const ACCEL_LOW_PERMILLE: u32 = 1000; // 慢速 1.0x
pub const ACCEL_HIGH_PERMILLE: u32 = 1800; // 快速 1.8x
pub const ACCEL_THRESHOLD: i32 = 20;

/// 速度（|dx|）低于阈值用低增益，否则线性爬升到高增益；返回缩放后位移。
pub fn accel_apply(dx: i32, speed: i32) -> i32 {
    let gain_permille = if speed < ACCEL_THRESHOLD {
        ACCEL_LOW_PERMILLE
    } else {
        let excess = (speed - ACCEL_THRESHOLD).min(100);
        ACCEL_LOW_PERMILLE + (ACCEL_HIGH_PERMILLE - ACCEL_LOW_PERMILLE) * excess as u32 / 100
    };
    ((dx as i64 * gain_permille as i64) / 1000) as i32
}

// ===========================================================================
// F330 — 触控协议谱：MT 槽位状态机
// ===========================================================================

pub const TOUCH_SLOTS: usize = 5;
pub const TOUCH_IDLE: i32 = -1;

#[derive(Clone, Copy, Debug)]
pub struct TouchSlots {
    pub tracking: [i32; TOUCH_SLOTS], // 每槽 tracking id，-1 = 空
    pub active: u32,
}

impl TouchSlots {
    pub const fn new() -> TouchSlots {
        TouchSlots { tracking: [TOUCH_IDLE; TOUCH_SLOTS], active: 0 }
    }

    /// 槽位按下/抬起：按下分配 tracking id，抬起归还。
    pub fn touch(&mut self, slot: usize, tracking_id: i32) -> bool {
        if slot >= TOUCH_SLOTS || tracking_id < 0 {
            return false;
        }
        if self.tracking[slot] == TOUCH_IDLE {
            self.active += 1;
        }
        self.tracking[slot] = tracking_id;
        true
    }

    pub fn release(&mut self, slot: usize) -> bool {
        if slot >= TOUCH_SLOTS || self.tracking[slot] == TOUCH_IDLE {
            return false;
        }
        self.tracking[slot] = TOUCH_IDLE;
        self.active -= 1;
        true
    }
}

// ===========================================================================
// F331 — 输入事件风暴阀：窗口限流
// ===========================================================================

pub const STORM_WINDOW_EVENTS: u32 = 64;

#[derive(Clone, Copy, Debug)]
pub struct StormValve {
    pub passed: u32,
    pub dropped: u32,
}

impl StormValve {
    pub const fn new() -> StormValve {
        StormValve { passed: 0, dropped: 0 }
    }

    /// 每窗口最多放行 64 个事件。
    pub fn admit(&mut self) -> bool {
        if self.passed < STORM_WINDOW_EVENTS {
            self.passed += 1;
            true
        } else {
            self.dropped += 1;
            false
        }
    }
}

// ===========================================================================
// F332 — 输入设备档案：总线/厂商/产品/版本
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputProfile {
    pub bustype: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

impl InputProfile {
    pub fn identifiable(&self) -> bool {
        self.bustype != 0 && self.vendor != 0 && self.product != 0
    }

    pub fn identity(&self) -> u64 {
        (self.bustype as u64) << 48 | (self.vendor as u64) << 32 | (self.product as u64) << 16 | self.version as u64
    }
}

// ===========================================================================
// F333 — 按键去抖官：窗口期内重复按下忽略
// ===========================================================================

pub const DEBOUNCE_MS: u32 = 30;

#[derive(Clone, Copy, Debug)]
pub struct Debouncer {
    pub last_press_ms: u32,
    pub primed: bool,
    pub bounced: u32,
}

impl Debouncer {
    pub const fn new() -> Debouncer {
        Debouncer { last_press_ms: 0, primed: false, bounced: 0 }
    }

    /// 同键重复按下：距上次不足 DEBOUNCE_MS 视为抖动。
    pub fn press(&mut self, now_ms: u32) -> bool {
        if self.primed && now_ms.wrapping_sub(self.last_press_ms) < DEBOUNCE_MS {
            self.bounced += 1;
            return false;
        }
        self.last_press_ms = now_ms;
        self.primed = true;
        true
    }
}

// ===========================================================================
// F334 — 事件序号墙：单调序号 + 缺口检测
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct SeqWall {
    pub next_seq: u64,
    pub gaps: u32,
    pub stale_dropped: u32,
}

impl SeqWall {
    pub const fn new() -> SeqWall {
        SeqWall { next_seq: 1, gaps: 0, stale_dropped: 0 }
    }

    /// 验收入场：期望序号放行，超前记缺口，回退丢弃。
    pub fn admit(&mut self, seq: u64) -> bool {
        if seq == self.next_seq {
            self.next_seq += 1;
            true
        } else if seq > self.next_seq {
            self.gaps += 1;
            self.next_seq = seq + 1;
            true
        } else {
            self.stale_dropped += 1;
            false
        }
    }
}

// ===========================================================================
// F335 — 输入回放流：录制/回放一致
// ===========================================================================

pub const REPLAY_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct ReplayStream {
    buf: [InputEvent; REPLAY_CAP],
    len: usize,
    pub overflow: u32,
}

impl ReplayStream {
    pub const fn new() -> ReplayStream {
        ReplayStream {
            buf: [InputEvent { kind: 0, code: 0, value: 0, seq: 0, stamp_us: 0 }; REPLAY_CAP],
            len: 0,
            overflow: 0,
        }
    }

    pub fn record(&mut self, ev: InputEvent) {
        if self.len < REPLAY_CAP {
            self.buf[self.len] = ev;
            self.len += 1;
        } else {
            self.overflow += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn event_at(&self, i: usize) -> Option<InputEvent> {
        if i < self.len {
            Some(self.buf[i])
        } else {
            None
        }
    }
}

/// 回放一致：流内容与期望逐条相同。
pub fn replay_identical(stream: &ReplayStream, expect: &[InputEvent]) -> bool {
    stream.len() == expect.len()
        && (0..expect.len()).all(|i| stream.event_at(i) == Some(expect[i]))
}

// ===========================================================================
// F336 — 手势内核原语：tap/drag/swipe 判定
// ===========================================================================

pub const TAP_MAX_MOVE_PX: i32 = 8;
pub const TAP_MAX_MS: u32 = 250;
pub const SWIPE_MIN_MOVE_PX: i32 = 48;

#[derive(Clone, Copy, Debug)]
pub struct GestureTracker {
    pub start_x: i32,
    pub start_y: i32,
    pub start_ms: u32,
    pub last_x: i32,
    pub last_y: i32,
    pub end_ms: u32,
    pub primed: bool,
}

impl GestureTracker {
    pub const IDLE: GestureTracker = GestureTracker {
        start_x: 0, start_y: 0, start_ms: 0, last_x: 0, last_y: 0, end_ms: 0, primed: false,
    };

    pub fn begin(&mut self, x: i32, y: i32, now_ms: u32) {
        self.start_x = x;
        self.start_y = y;
        self.last_x = x;
        self.last_y = y;
        self.start_ms = now_ms;
        self.primed = true;
    }

    pub fn move_to(&mut self, x: i32, y: i32, now_ms: u32) {
        self.last_x = x;
        self.last_y = y;
        self.end_ms = now_ms;
    }

    fn dist(&self) -> i32 {
        let dx = self.last_x - self.start_x;
        let dy = self.last_y - self.start_y;
        // 定点近似欧氏距离：max + min/2
        let (ax, ay) = (dx.abs(), dy.abs());
        let (hi, lo) = if ax > ay { (ax, ay) } else { (ay, ax) };
        hi + lo / 2
    }

    pub fn verdict(&self) -> u8 {
        if !self.primed {
            return 0;
        }
        let d = self.dist();
        let dur = self.end_ms.saturating_sub(self.start_ms);
        if d < TAP_MAX_MOVE_PX && dur <= TAP_MAX_MS {
            1 // tap
        } else if d >= SWIPE_MIN_MOVE_PX {
            3 // swipe
        } else if d >= TAP_MAX_MOVE_PX {
            2 // drag
        } else {
            0 // 未定
        }
    }
}

// ===========================================================================
// F337 — 输入功耗账：唤醒记账
// ===========================================================================

pub const WAKEUP_BUDGET_PER_S: u32 = 50;

#[derive(Clone, Copy, Debug, Default)]
pub struct WakeupLedger {
    pub wakeups: u32,
}

impl WakeupLedger {
    /// 唤醒占预算的 permille。
    pub fn permille(&self) -> u32 {
        self.wakeups * 1000 / WAKEUP_BUDGET_PER_S
    }

    pub fn over_budget(&self) -> bool {
        self.wakeups > WAKEUP_BUDGET_PER_S
    }
}

// ===========================================================================
// F338 — 输入权限门：设备类别限定事件类型
// ===========================================================================

/// 类别 0=键盘 1=指针 2=触控。
pub fn input_event_allowed(device_class: u8, ev_kind: u8) -> bool {
    match device_class {
        0 => ev_kind == EV_KEY,
        1 => ev_kind == EV_REL || ev_kind == EV_KEY,
        _ => ev_kind == EV_ABS || ev_kind == EV_KEY,
    }
}

// ===========================================================================
// F339 — 死键/组合键谱：死键组合状态机
// ===========================================================================

/// 死键基础码。
pub const DEAD_GRAVE: u16 = 100;
pub const DEAD_ACUTE: u16 = 101;

/// 死键 + 基础字母 → 组合码点（u32）；无组合返回 None。
pub fn dead_key_compose(dead: u16, base: u8) -> Option<u32> {
    match (dead, base) {
        (DEAD_GRAVE, b'a') => Some(0xE0), // à
        (DEAD_GRAVE, b'e') => Some(0xE8), // è
        (DEAD_ACUTE, b'a') => Some(0xE1), // á
        (DEAD_ACUTE, b'e') => Some(0xE9), // é
        _ => None,
    }
}

/// 死键之后必须跟基础字母，否则死键悬空报错。
pub fn dead_key_pending(dead: u16) -> bool {
    dead == DEAD_GRAVE || dead == DEAD_ACUTE
}

// ===========================================================================
// F340 — 输入设备热插拔律：先断流再拔线
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPlugPhase {
    Announce,
    StreamsDrained,
    Removed,
}

/// 拔出必须经过 StreamsDrained 才能到 Removed。
pub fn unplug_phase_ok(from: InputPlugPhase, to: InputPlugPhase) -> bool {
    matches!(
        (from, to),
        (InputPlugPhase::Announce, InputPlugPhase::StreamsDrained)
            | (InputPlugPhase::StreamsDrained, InputPlugPhase::Removed)
    )
}

// ===========================================================================
// F341 — 滚轮物理谱：齿格累积与 hi-res 细分
// ===========================================================================

pub const WHEEL_NOTCH_DELTA: i32 = 120;

#[derive(Clone, Copy, Debug)]
pub struct WheelAccum {
    pub residual: i32,
    pub notches: i32,
}

impl WheelAccum {
    pub const fn new() -> WheelAccum {
        WheelAccum { residual: 0, notches: 0 }
    }

    /// hi-res 滚轮增量入账：满一个齿格（±120）才产出行事件。
    pub fn feed(&mut self, delta: i32) -> i32 {
        self.residual += delta;
        let notches = self.residual / WHEEL_NOTCH_DELTA;
        self.residual -= notches * WHEEL_NOTCH_DELTA;
        self.notches += notches;
        notches
    }
}

// ===========================================================================
// F342 — 输入延迟预算门：逐事件硬门
// ===========================================================================

/// 单事件延迟超预算即越门。
pub fn latency_gate(latency_us: u64) -> bool {
    latency_us <= INPUT_LATENCY_BUDGET_US
}

// ===========================================================================
// F343 — 无障碍输入通道：粘滞键/慢速键
// ===========================================================================

pub const SLOW_KEY_MS: u32 = 500;

#[derive(Clone, Copy, Debug)]
pub struct A11yChannel {
    pub sticky_latched: u32, // 粘滞修饰键位掩码
    pub slow_key_down_ms: u32,
    pub slow_pending: bool,
}

impl A11yChannel {
    pub const fn new() -> A11yChannel {
        A11yChannel { sticky_latched: 0, slow_key_down_ms: 0, slow_pending: false }
    }

    /// 粘滞键：修饰键单按锁存，再按同键释放。
    pub fn sticky_toggle(&mut self, mod_bit: u8) {
        self.sticky_latched ^= 1u32 << mod_bit;
    }

    /// 慢速键：按下后必须停留满 SLOW_KEY_MS 才生效。
    pub fn slow_key_down(&mut self, now_ms: u32) {
        self.slow_key_down_ms = now_ms;
        self.slow_pending = true;
    }

    pub fn slow_key_up(&mut self, now_ms: u32) -> bool {
        if !self.slow_pending {
            return false;
        }
        let held = now_ms.saturating_sub(self.slow_key_down_ms);
        self.slow_pending = false;
        held >= SLOW_KEY_MS
    }
}

// ===========================================================================
// F344 — 输入事件考古：定容历史环检索
// ===========================================================================

pub const ARCHAEOLOGY_RING: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct EventRing {
    kinds: [u8; ARCHAEOLOGY_RING],
    codes: [u16; ARCHAEOLOGY_RING],
    seqs: [u64; ARCHAEOLOGY_RING],
    head: usize,
}

impl EventRing {
    pub const fn new() -> EventRing {
        EventRing { kinds: [0; ARCHAEOLOGY_RING], codes: [0; ARCHAEOLOGY_RING], seqs: [0; ARCHAEOLOGY_RING], head: 0 }
    }

    pub fn push(&mut self, ev: InputEvent) {
        self.kinds[self.head] = ev.kind;
        self.codes[self.head] = ev.code;
        self.seqs[self.head] = ev.seq;
        self.head = (self.head + 1) % ARCHAEOLOGY_RING;
    }

    /// 检索最近一次指定类型事件（按 seq 比较，跨环绕）。
    pub fn find_latest(&self, kind: u8) -> Option<(u64, u16)> {
        let mut best: Option<(u64, u16)> = None;
        for i in 0..ARCHAEOLOGY_RING {
            if self.kinds[i] == kind && self.seqs[i] > 0 {
                match best {
                    Some((bs, _)) if bs >= self.seqs[i] => {}
                    _ => best = Some((self.seqs[i], self.codes[i])),
                }
            }
        }
        best
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % ARCHAEOLOGY_RING]
    }
}

// ===========================================================================
// F345 — 键盘灯效通道：三灯状态
// ===========================================================================

pub const LED_CAPS: u8 = 1;
pub const LED_NUM: u8 = 2;
pub const LED_SCROLL: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardLeds {
    pub state: u8,
}

impl KeyboardLeds {
    pub const OFF: KeyboardLeds = KeyboardLeds { state: 0 };

    pub fn toggle(&mut self, led: u8) {
        self.state ^= led;
    }

    pub fn is_on(&self, led: u8) -> bool {
        self.state & led != 0
    }
}

// ===========================================================================
// F346 — 输入统计分账：按事件类型统计
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct TypeStat {
    pub events: u64,
    pub dropped: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputStatsBook {
    pub by_kind: [TypeStat; 8],
}

impl InputStatsBook {
    pub fn account(&mut self, kind: u8, dropped: bool) {
        if (kind as usize) < 8 {
            let s = &mut self.by_kind[kind as usize];
            s.events += 1;
            if dropped {
                s.dropped += 1;
            }
        }
    }

    pub fn total_events(&self) -> u64 {
        self.by_kind.iter().map(|s| s.events).sum()
    }
}

// ===========================================================================
// F347 — 输入设备健康分：卡键/丢事件扣分
// ===========================================================================

/// 健康分 = 1000 - 卡键×20 - 丢事件率‰×2，下限 0。
pub fn input_health(events: u64, dropped: u64, stuck_keys: u32) -> u32 {
    let drop_pm = if events == 0 { 0 } else { (dropped * 1000 / events) as u32 };
    1000u32
        .saturating_sub(stuck_keys.saturating_mul(20))
        .saturating_sub(drop_pm.saturating_mul(2))
}

// ===========================================================================
// F348 — 输入回归走廊
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct InputScenario {
    pub name: &'static str,
    pub passed: bool,
}

pub fn input_regression_green(scenarios: &[InputScenario]) -> bool {
    !scenarios.is_empty() && scenarios.iter().all(|s| s.passed && !s.name.is_empty())
}

// ===========================================================================
// F349 — 输入安全审计：未授权设备的事件告警
// ===========================================================================

pub const AUDIT_ALARM_RING: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct SecurityAudit {
    alarms: [u16; AUDIT_ALARM_RING], // 违规设备 id
    head: usize,
    pub alarms_total: u64,
}

impl SecurityAudit {
    pub const fn new() -> SecurityAudit {
        SecurityAudit { alarms: [0; AUDIT_ALARM_RING], head: 0, alarms_total: 0 }
    }

    /// 事件来自未授权设备 → 告警入环。
    pub fn report_unauthorized(&mut self, device_id: u16) {
        self.alarms[self.head] = device_id;
        self.head = (self.head + 1) % AUDIT_ALARM_RING;
        self.alarms_total += 1;
    }

    pub fn alarm_at(&self, i: usize) -> u16 {
        self.alarms[i % AUDIT_ALARM_RING]
    }

    /// 设备是否在最近告警名单里（环形扫描）。
    pub fn device_flagged(&self, device_id: u16) -> bool {
        self.alarms.iter().any(|&a| a == device_id && a != 0)
    }
}

// ===========================================================================
// F350 — 输入域年报
// ===========================================================================

pub const INPUT_REPORT_SECTIONS: [&str; 5] = ["events", "latency", "keys", "touch", "a11y"];

pub fn input_report_complete(filled: u32) -> bool {
    filled >= INPUT_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700input_checks() -> CheckSet {
    let mut set = CheckSet::new("m700input");

    // F326 输入事件信封
    let ev = InputEvent { kind: EV_KEY, code: 30, value: 1, seq: 1, stamp_us: 1000 };
    let bad = InputEvent { kind: 9, code: 30, value: 1, seq: 2, stamp_us: 1000 };
    set.add(
        "F326 envelope valid",
        ev.envelope_valid() && !bad.envelope_valid(),
        "kind range + seq",
    );
    set.add(
        "F326 kind set",
        EV_KEY < EV_ABS && EV_ABS < EV_REL && EV_REL < EV_WHEEL,
        "ordered kinds",
    );

    // F327 输入延迟仪
    let mut lm = LatencyMeter::default();
    lm.sample(1000, 2000);
    lm.sample(2000, 6000);
    let avg_now = lm.avg_us();
    set.add(
        "F327 latency stats",
        lm.samples == 2 && avg_now == 2500 && lm.worst_us == 4000,
        "avg+worst",
    );
    set.add(
        "F327 worst permille",
        lm.worst_permille() == 500,
        "half of 8ms budget",
    );

    // F328 键盘映射中台
    set.add(
        "F328 keymap base",
        keymap_lookup(30, false) == Some(b'a') && keymap_lookup(2, false) == Some(b'1'),
        "base layer",
    );
    set.add(
        "F328 keymap shift",
        keymap_lookup(30, true) == Some(b'A') && keymap_lookup(3, true) == Some(b'@'),
        "shift layer",
    );
    set.add("F328 keymap miss", keymap_lookup(255, false).is_none(), "unknown code");

    // F329 指针加速度谱
    set.add(
        "F329 accel slow zone",
        accel_apply(10, 5) == 10,
        "1.0x below threshold",
    );
    set.add(
        "F329 accel fast zone",
        accel_apply(10, 70) == 14 && accel_apply(10, 200) == 18,
        "up to 1.8x",
    );

    // F330 触控协议谱
    let mut ts = TouchSlots::new();
    let t0 = ts.touch(0, 11);
    let t1 = ts.touch(1, 12);
    let active_two = ts.active;
    set.add(
        "F330 slots press",
        t0 && t1 && active_two == 2 && ts.tracking[0] == 11,
        "two fingers",
    );
    let r0 = ts.release(0);
    let active_one = ts.active;
    set.add("F330 slots release", r0 && active_one == 1 && ts.tracking[0] == TOUCH_IDLE, "slot freed");
    set.add("F330 double release", !ts.release(0), "no ghost finger");

    // F331 输入事件风暴阀
    let mut sv = StormValve::new();
    let mut i = 0;
    while sv.admit() {
        i += 1;
    }
    let dropped_now = sv.dropped;
    set.add(
        "F331 storm valve",
        i == STORM_WINDOW_EVENTS as usize && dropped_now == 1,
        "64 pass then choke",
    );
    sv.admit();
    set.add("F331 storm drop counted", sv.dropped == 2, "drop accounted");

    // F332 输入设备档案
    let prof = InputProfile { bustype: 0x0003, vendor: 0x046D, product: 0xC52B, version: 0x0111 };
    set.add(
        "F332 profile id",
        prof.identifiable()
            && !InputProfile { bustype: 0, vendor: 1, product: 1, version: 1 }.identifiable(),
        "bustype+vendor+product",
    );
    set.add("F332 identity pack", prof.identity() >> 48 == 0x0003, "bustype in high bits");

    // F333 按键去抖官
    let mut db = Debouncer::new();
    let p1 = db.press(100);
    let bounce = db.press(110);
    let bounced_now = db.bounced;
    let p2 = db.press(200);
    set.add(
        "F333 debounce bounce",
        p1 && !bounce && bounced_now == 1,
        "30ms window",
    );
    set.add("F333 debounce pass", p2 && db.bounced == 1, "after window ok");

    // F334 事件序号墙
    let mut wall = SeqWall::new();
    let a1 = wall.admit(1);
    let a2 = wall.admit(2);
    let after_two = wall.next_seq;
    let gap = wall.admit(5);
    let gaps_now = wall.gaps;
    let stale = wall.admit(3);
    set.add(
        "F334 seq wall in order",
        a1 && a2 && after_two == 3,
        "1,2 accepted",
    );
    set.add(
        "F334 seq wall gap+stale",
        gap && gaps_now == 1 && !stale && wall.stale_dropped == 1,
        "gap logged, stale dropped",
    );

    // F335 输入回放流
    let mut rs = ReplayStream::new();
    let evs = [
        InputEvent { kind: EV_KEY, code: 30, value: 1, seq: 1, stamp_us: 10 },
        InputEvent { kind: EV_KEY, code: 30, value: 0, seq: 2, stamp_us: 20 },
        InputEvent { kind: EV_REL, code: 0, value: 5, seq: 3, stamp_us: 30 },
    ];
    for &e in evs.iter() {
        rs.record(e);
    }
    set.add("F335 replay record", replay_identical(&rs, &evs), "byte-faithful");
    set.add(
        "F335 replay mismatch",
        !replay_identical(&rs, &evs[..2]),
        "length guard",
    );

    // F336 手势内核原语
    let mut g = GestureTracker::IDLE;
    g.begin(100, 100, 0);
    g.move_to(103, 102, 100);
    g.move_to(104, 103, 200);
    set.add("F336 tap verdict", g.verdict() == 1, "small+fast");
    let mut s = GestureTracker::IDLE;
    s.begin(0, 0, 0);
    s.move_to(0, 100, 400);
    set.add("F336 swipe verdict", s.verdict() == 3, "big move");
    let mut dr = GestureTracker::IDLE;
    dr.begin(0, 0, 0);
    dr.move_to(20, 10, 500);
    set.add("F336 drag verdict", dr.verdict() == 2, "mid move");

    // F337 输入功耗账
    let mut wl = WakeupLedger { wakeups: 25 };
    let pm_now = wl.permille();
    wl.wakeups += 25;
    set.add("F337 wakeup half", pm_now == 500 && wl.permille() == 1000, "50/s budget");
    let over = WakeupLedger { wakeups: 51 };
    set.add("F337 wakeup over", over.over_budget(), "51 > 50");

    // F338 输入权限门
    set.add(
        "F338 perms keyboard",
        input_event_allowed(0, EV_KEY) && !input_event_allowed(0, EV_ABS),
        "keys only",
    );
    set.add(
        "F338 perms touch",
        input_event_allowed(2, EV_ABS) && !input_event_allowed(2, EV_REL),
        "abs only",
    );

    // F339 死键/组合键谱
    set.add(
        "F339 compose grave",
        dead_key_compose(DEAD_GRAVE, b'a') == Some(0xE0)
            && dead_key_compose(DEAD_ACUTE, b'e') == Some(0xE9),
        "à & é",
    );
    set.add(
        "F339 compose dangling",
        dead_key_compose(DEAD_GRAVE, b'q').is_none() && dead_key_pending(DEAD_ACUTE),
        "unknown pair pending",
    );

    // F340 输入设备热插拔律
    set.add(
        "F340 unplug order",
        unplug_phase_ok(InputPlugPhase::Announce, InputPlugPhase::StreamsDrained)
            && unplug_phase_ok(InputPlugPhase::StreamsDrained, InputPlugPhase::Removed),
        "drain then remove",
    );
    set.add(
        "F340 unplug yank",
        !unplug_phase_ok(InputPlugPhase::Announce, InputPlugPhase::Removed),
        "no yanking",
    );

    // F341 滚轮物理谱
    let mut wh = WheelAccum::new();
    let n1 = wh.feed(100);
    let n2 = wh.feed(100);
    let n3 = wh.feed(-60);
    set.add(
        "F341 notch accumulate",
        n1 == 0 && n2 == 1 && n3 == 0 && wh.residual == 20,
        "residual carries",
    );
    let n4 = wh.feed(100);
    set.add("F341 notch flush", n4 == 1 && wh.notches == 2, "second notch");

    // F342 输入延迟预算门
    set.add(
        "F342 latency gate",
        latency_gate(8000) && !latency_gate(8001),
        "8ms hard line",
    );

    // F343 无障碍输入通道
    let mut a11y = A11yChannel::new();
    a11y.sticky_toggle(0);
    a11y.sticky_toggle(1);
    let latched = a11y.sticky_latched;
    a11y.sticky_toggle(0);
    set.add(
        "F343 sticky keys",
        latched == 0b11 && a11y.sticky_latched == 0b10,
        "toggle latch",
    );
    a11y.slow_key_down(1000);
    let short = a11y.slow_key_up(1200);
    a11y.slow_key_down(2000);
    let held = a11y.slow_key_up(2600);
    set.add(
        "F343 slow keys",
        !short && held,
        "500ms hold required",
    );

    // F344 输入事件考古
    let mut ring = EventRing::new();
    ring.push(InputEvent { kind: EV_KEY, code: 30, value: 1, seq: 1, stamp_us: 0 });
    ring.push(InputEvent { kind: EV_REL, code: 0, value: 3, seq: 2, stamp_us: 0 });
    ring.push(InputEvent { kind: EV_KEY, code: 31, value: 0, seq: 3, stamp_us: 0 });
    let found = ring.find_latest(EV_KEY);
    set.add(
        "F344 archaeology latest",
        found == Some((3, 31)),
        "newest key wins",
    );
    let found_rel = ring.find_latest(EV_REL);
    set.add("F344 archaeology kind", found_rel == Some((2, 0)), "kind filter");

    // F345 键盘灯效通道
    let mut leds = KeyboardLeds::OFF;
    leds.toggle(LED_CAPS | LED_NUM);
    let both_on = leds.is_on(LED_CAPS) && leds.is_on(LED_NUM) && !leds.is_on(LED_SCROLL);
    leds.toggle(LED_CAPS);
    set.add(
        "F345 led toggle",
        both_on && !leds.is_on(LED_CAPS) && leds.is_on(LED_NUM),
        "caps off num on",
    );

    // F346 输入统计分账
    let mut book = InputStatsBook::default();
    book.account(EV_KEY, false);
    book.account(EV_KEY, true);
    book.account(EV_REL, false);
    let total_now = book.total_events();
    set.add(
        "F346 stats per kind",
        total_now == 3 && book.by_kind[EV_KEY as usize].dropped == 1,
        "accounted by kind",
    );

    // F347 输入设备健康分
    set.add("F347 healthy device", input_health(1000, 0, 0) == 1000, "flawless");
    set.add(
        "F347 degraded device",
        input_health(1000, 50, 2) == 1000 - 40 - 100,
        "stuck+drop demerits",
    );

    // F348 输入回归走廊
    let green = [
        InputScenario { name: "key-storm", passed: true },
        InputScenario { name: "touch-slots", passed: true },
    ];
    let red = [InputScenario { name: "debounce", passed: false }];
    set.add("F348 regression green", input_regression_green(&green), "all pass");
    set.add(
        "F348 regression red",
        !input_regression_green(&red) && !input_regression_green(&[]),
        "fail & empty rejected",
    );

    // F349 输入安全审计
    let mut sec = SecurityAudit::new();
    sec.report_unauthorized(0xBEEF);
    sec.report_unauthorized(0xCAFE);
    let alarm1 = sec.alarm_at(0);
    let flagged = sec.device_flagged(0xBEEF);
    set.add(
        "F349 audit alarms",
        alarm1 == 0xBEEF && flagged && sec.alarms_total == 2,
        "unauthorized flagged",
    );
    set.add("F349 audit clean dev", !sec.device_flagged(0x1234), "clean device");

    // F350 输入域年报
    set.add(
        "F350 input report",
        INPUT_REPORT_SECTIONS.len() == 5 && input_report_complete(5) && !input_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f328_keymap_layers() {
        assert_eq!(keymap_lookup(57, false), Some(b' '));
        assert_eq!(keymap_lookup(57, true), Some(b' ')); // 空格两层一致
        assert_eq!(keymap_lookup(48, true), Some(b'B'));
        assert_eq!(keymap_lookup(0, false), None);
    }

    #[test]
    fn f329_accel_monotonic() {
        let lo = accel_apply(100, 0);
        let mid = accel_apply(100, 30);
        let hi = accel_apply(100, 120);
        assert_eq!(lo, 100);
        assert!(mid > lo && hi > mid);
        assert!(hi <= 180); // 上限 1.8x
    }

    #[test]
    fn f334_seq_wall_lifecycle() {
        let mut w = SeqWall::new();
        assert!(w.admit(1));
        assert!(w.admit(2));
        assert!(!w.admit(1)); // 回退丢弃
        assert!(w.admit(9)); // 跳跃放行
        assert_eq!(w.gaps, 1);
        assert_eq!(w.next_seq, 10);
    }

    #[test]
    fn f341_wheel_physics() {
        let mut w = WheelAccum::new();
        assert_eq!(w.feed(240), 2); // 一次满两格
        assert_eq!(w.feed(-130), -1); // 反向一格余 -10
        assert_eq!(w.residual, -10);
        assert_eq!(w.notches, 1);
    }

    #[test]
    fn f343_slow_key_boundary() {
        let mut a = A11yChannel::new();
        a.slow_key_down(0);
        assert!(!a.slow_key_up(SLOW_KEY_MS - 1));
        a.slow_key_down(0);
        assert!(a.slow_key_up(SLOW_KEY_MS));
    }

    #[test]
    fn f350_domain_selfcheck_all_pass() {
        let set = run_m700input_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
