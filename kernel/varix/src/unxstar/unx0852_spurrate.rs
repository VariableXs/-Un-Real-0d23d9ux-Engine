//! UNX-F0852 · spurious 率统计账与健康阈值告警 · 诊断段 0x6760（独占 0x6760~0x676F）
//!
//! **判据原文摘录**：率统计精确（10 万事件误差 0），阈值告警触发/恢复各
//! 10/10 次正确。
//!
//! **设计纪律**：
//! 1. 率统计（判据「误差 0」）：全量精确计数——总事件、伪中断数、真中断
//!    数三个独立计数器，率 = spurious × 10⁶ / total（ppm 整数），任何窗口
//!    重算都必须与事件账完全一致（误差 0 = 计数器无丢失无重复）。
//! 2. 滑窗精确性：窗口重算采用「减出加进」增量法，每窗校验
//!    `window_sum == Σ窗内元素`，失配即红。
//! 3. 滞回告警（触发/恢复各 10/10）：升阈 1000ppm 触发、降阈 400ppm 恢复
//!    （1:2.5 滞回带），带内保持——防抖动。状态机三态：Normal / Alarm /
//!    Recovered（Recovered 即 Normal 的账面印记，恢复计数专用）。
//! 4. 守恒恒等式：`total = spurious + genuine`；`alarms = triggers -
//!    recovers + alarm_active`。
//! 5. 零 unwrap/expect/panic/浮点/std；确定性 LCG 注入；无墙钟。
//!
//! **诊断段声明**：0x6760~0x676F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6760~0x676F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6760;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x676F;

/// 窗口配置非法（窗长 0）。
pub const X_SR2_WINDOW: u16 = 0x6761;
/// 阈值配置非法（升阈 ≤ 降阈——滞回带倒挂）。
pub const X_SR2_THRESHOLD: u16 = 0x6762;
/// 率分母为零（无事件求率——统计器失灵）。
pub const X_SR2_ZERO_DENOM: u16 = 0x6763;
/// 守恒破缺（total ≠ spurious + genuine）。
pub const X_SR2_UNBALANCED: u16 = 0x6764;
/// 窗口增量失配（减出加进法与全量重算不一致）。
pub const X_SR2_WINDOW_MISMATCH: u16 = 0x6765;
/// 告警状态机越界（恢复态再次恢复）。
pub const X_SR2_ALARM_STATE: u16 = 0x6766;
/// 注入演练不达标（触发/恢复未满 10/10）。
pub const X_SR2_DRILL_MISS: u16 = 0x6767;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_SR2_WINDOW => "窗口配置非法",
        X_SR2_THRESHOLD => "阈值滞回倒挂",
        X_SR2_ZERO_DENOM => "率分母为零",
        X_SR2_UNBALANCED => "计数守恒破缺",
        X_SR2_WINDOW_MISMATCH => "窗口增量失配",
        X_SR2_ALARM_STATE => "告警状态越界",
        X_SR2_DRILL_MISS => "演练未达标",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、精确率统计账
// ===========================================================================

/// 事件种类（二分闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 伪中断。
    Spurious,
    /// 真中断。
    Genuine,
}

/// 精确统计账：三计数器 + 率（ppm 整数）。
#[derive(Clone, Copy, Default)]
pub struct RateLedger {
    pub total: u64,
    pub spurious: u64,
    pub genuine: u64,
}

impl RateLedger {
    pub const fn new() -> Self {
        Self {
            total: 0,
            spurious: 0,
            genuine: 0,
        }
    }

    /// 记一笔（守恒即时校验）。
    pub fn record(&mut self, k: EvKind) -> Result<(), u16> {
        self.total += 1;
        match k {
            EvKind::Spurious => self.spurious += 1,
            EvKind::Genuine => self.genuine += 1,
        }
        if self.total != self.spurious + self.genuine {
            return Err(X_SR2_UNBALANCED);
        }
        Ok(())
    }

    /// 伪中断率（ppm 整数；分母零拒绝）。
    pub fn spurious_ppm(&self) -> Result<u64, u16> {
        if self.total == 0 {
            return Err(X_SR2_ZERO_DENOM);
        }
        Ok(self.spurious * 1_000_000 / self.total)
    }
}

// ===========================================================================
// 三、滑窗（精确增量法）
// ===========================================================================

/// 定长滑窗：每窗元素 EvKind 计数，「减出加进」增量维护。
pub struct SlideWindow {
    buf: Vec<bool>, // true = spurious
    head: usize,
    len: usize,
    /// 窗内伪中断数（增量真源）。
    pub win_spur: u64,
    /// 窗内真中断数。
    pub win_gen: u64,
    /// 增量法校验失配数（判据要求 0）。
    pub mismatches: u64,
}

impl SlideWindow {
    pub fn new(cap: usize) -> Result<Self, u16> {
        if cap == 0 {
            return Err(X_SR2_WINDOW);
        }
        Ok(Self {
            buf: Vec::with_capacity(cap),
            head: 0,
            len: cap,
            win_spur: 0,
            win_gen: 0,
            mismatches: 0,
        })
    }

    /// 推入一事件：满则减出最旧。
    pub fn push(&mut self, k: EvKind) -> Result<(), u16> {
        let is_spur = k == EvKind::Spurious;
        if self.buf.len() < self.len {
            self.buf.push(is_spur);
            if is_spur {
                self.win_spur += 1;
            } else {
                self.win_gen += 1;
            }
        } else {
            // 减出最旧。
            let old = self.buf[self.head];
            if old {
                self.win_spur -= 1;
            } else {
                self.win_gen -= 1;
            }
            // 加进最新。
            self.buf[self.head] = is_spur;
            if is_spur {
                self.win_spur += 1;
            } else {
                self.win_gen += 1;
            }
            self.head = (self.head + 1) % self.len;
        }
        // 增量法 vs 全量重算（窗口小，重算廉价——精确性对拍）。
        let mut s = 0u64;
        let mut i = 0;
        while i < self.buf.len() {
            if self.buf[i] {
                s += 1;
            }
            i += 1;
        }
        if s != self.win_spur {
            self.mismatches += 1;
            return Err(X_SR2_WINDOW_MISMATCH);
        }
        Ok(())
    }

    /// 窗口率（ppm）。
    pub fn ppm(&self) -> Result<u64, u16> {
        let n = self.win_spur + self.win_gen;
        if n == 0 {
            return Err(X_SR2_ZERO_DENOM);
        }
        Ok(self.win_spur * 1_000_000 / n)
    }
}

// ===========================================================================
// 四、滞回告警状态机
// ===========================================================================

/// 告警态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlarmState {
    Normal,
    Alarm,
    /// 刚恢复（恢复计数印记；语义同 Normal）。
    Recovered,
}

/// 滞回告警器：升阈触发、降阈恢复、带内保持。
pub struct HysteresisAlarm {
    pub up_ppm: u64,
    pub down_ppm: u64,
    pub state: AlarmState,
    pub triggers: u64,
    pub recovers: u64,
}

impl HysteresisAlarm {
    pub fn new(up_ppm: u64, down_ppm: u64) -> Result<Self, u16> {
        if down_ppm >= up_ppm {
            return Err(X_SR2_THRESHOLD);
        }
        Ok(Self {
            up_ppm,
            down_ppm,
            state: AlarmState::Normal,
            triggers: 0,
            recovers: 0,
        })
    }

    /// 窗率驱动状态机。
    pub fn feed(&mut self, ppm: u64) -> Result<AlarmState, u16> {
        match self.state {
            AlarmState::Normal | AlarmState::Recovered => {
                if ppm >= self.up_ppm {
                    self.state = AlarmState::Alarm;
                    self.triggers += 1;
                }
            }
            AlarmState::Alarm => {
                if ppm <= self.down_ppm {
                    self.state = AlarmState::Recovered;
                    self.recovers += 1;
                }
            }
        }
        Ok(self.state)
    }

    /// 告警账守恒：triggers = recovers + (当前 Alarm?1:0)。
    pub fn balanced(&self) -> bool {
        let active = if self.state == AlarmState::Alarm { 1 } else { 0 };
        self.triggers == self.recovers + active
    }
}

// ===========================================================================
// 五、注入演练（触发/恢复各 10/10 + 10 万事件误差 0）
// ===========================================================================

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 演练结论。
pub struct AlarmVerdict {
    pub events: u64,
    pub triggers: u64,
    pub recovers: u64,
    pub window_mismatches: u64,
    pub exact_ok: bool,
}

/// 注入演练：10 万事件分 20 段，段内伪中断密度高低交替（高段率 ≥20%、
/// 低段率 ≤0.05%），滑窗 256 驱动滞回告警（升 100000ppm/降 40000ppm）。
/// 高低段各 10 次交替 ⇒ 触发/恢复各 10 次。
pub fn run_alarm_drill(events: u64, seed: u64) -> Result<AlarmVerdict, u16> {
    let mut rng = Lcg::new(seed);
    let mut led = RateLedger::new();
    let mut win = SlideWindow::new(256)?;
    let mut alarm = HysteresisAlarm::new(100_000, 40_000)?;
    // 段规划：每段 events/20，交替高/低密度。
    let seg = events / 20;
    let mut e = 0u64;
    while e < events {
        let s = e / seg.max(1);
        let high = s % 2 == 0;
        // 高段 30% 伪中断（>10% 升阈），低段 0% 伪中断（<4% 降阈）。
        let is_spur = if high { rng.next() % 100 < 30 } else { false };
        let kind = if is_spur { EvKind::Spurious } else { EvKind::Genuine };
        led.record(kind)?;
        win.push(kind)?;
        alarm.feed(win.ppm()?)?;
        e += 1;
    }
    // 精确性：三计数器与事件流一致（误差 0）。
    let exact_ok = led.total == events && led.total == led.spurious + led.genuine;
    if !exact_ok {
        return Err(X_SR2_UNBALANCED);
    }
    if win.mismatches != 0 {
        return Err(X_SR2_WINDOW_MISMATCH);
    }
    if alarm.triggers != 10 || alarm.recovers != 10 {
        return Err(X_SR2_DRILL_MISS);
    }
    if !alarm.balanced() {
        return Err(X_SR2_ALARM_STATE);
    }
    Ok(AlarmVerdict {
        events,
        triggers: alarm.triggers,
        recovers: alarm.recovers,
        window_mismatches: win.mismatches,
        exact_ok,
    })
}

// ===========================================================================
// 六、drill 自例程
// ===========================================================================

/// 自例程：10 万事件误差 0 + 触发/恢复 10/10 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 窗长 0 / 滞回倒挂拒绝。
    if SlideWindow::new(0).is_ok() {
        return Ok(1);
    }
    if HysteresisAlarm::new(400, 400).is_ok() || HysteresisAlarm::new(100, 200).is_ok() {
        return Ok(2);
    }
    // 2) 零分母拒绝。
    let led0 = RateLedger::new();
    if led0.spurious_ppm().is_ok() {
        return Ok(3);
    }
    // 3) 精确率：3 伪 / 7 真 = 300000ppm 整除无误差。
    let mut led = RateLedger::new();
    let mut i = 0;
    while i < 3 {
        led.record(EvKind::Spurious)?;
        i += 1;
    }
    let mut i = 0;
    while i < 7 {
        led.record(EvKind::Genuine)?;
        i += 1;
    }
    if led.spurious_ppm()? != 300_000 {
        return Ok(4);
    }
    // 4) 滑窗精确性：256 容窗灌 1000 混合事件，增量法零失配。
    let mut rng = Lcg::new(0x6760_6760);
    let mut win = SlideWindow::new(256)?;
    let mut i = 0;
    while i < 1000 {
        let k = if rng.next() % 4 == 0 {
            EvKind::Spurious
        } else {
            EvKind::Genuine
        };
        win.push(k)?;
        i += 1;
    }
    if win.mismatches != 0 {
        return Err(X_SR2_WINDOW_MISMATCH);
    }
    // 5) 滞回单步：0% → 触发不了；100% → 触发；降到 0% → 恢复。
    let mut al = HysteresisAlarm::new(100_000, 40_000)?;
    if al.feed(50_000)? != AlarmState::Normal {
        return Ok(5); // 带内保持。
    }
    if al.feed(100_000)? != AlarmState::Alarm {
        return Ok(6);
    }
    if al.feed(50_000)? != AlarmState::Alarm {
        return Ok(7); // 带内保持（滞回）。
    }
    if al.feed(0)? != AlarmState::Recovered {
        return Ok(8);
    }
    if !al.balanced() {
        return Err(X_SR2_ALARM_STATE);
    }
    // 6) 主路径：10 万事件演练。
    let v = run_alarm_drill(100_000, 0x6760_6761)?;
    if v.window_mismatches != 0 || !v.exact_ok {
        return Err(X_SR2_WINDOW_MISMATCH);
    }
    if v.triggers != 10 || v.recovers != 10 {
        return Err(X_SR2_DRILL_MISS);
    }
    // 7) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_SR2_WINDOW, X_SR2_THRESHOLD, X_SR2_ZERO_DENOM, X_SR2_UNBALANCED,
        X_SR2_WINDOW_MISMATCH, X_SR2_ALARM_STATE, X_SR2_DRILL_MISS,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(9);
        }
    }
    Ok(0)
}
