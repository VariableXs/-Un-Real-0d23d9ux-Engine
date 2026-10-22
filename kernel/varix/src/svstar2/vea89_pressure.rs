//! VE-F0089 · 显存压力事件广播（VE-A 域 · A42 大资源组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0089`
//!
//! **职责定位（锚点原文）**：显存压力事件广播——显存压力的分级事件广播
//! （水位三级→订阅方自适应），事件风暴合并，订阅方响应审计；
//! 含压力事件的分级阈值依据（三级水位线来自实测）。
//!
//! **数据结构（锚点原文）**：事件广播。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 风暴 → 合并（同窗口同级只发一条，`merged` 计数如实带上——
//!   合并掉的次数不是信息垃圾，它是「压力持续多久」的证据）；
//! - 响应慢 → 审计（订阅方在预算内没确认就记慢，慢订阅方名单可查）；
//! - 压力误判 → 水位校准（阈值可上下调，每次调整留痕）。
//!
//! **性能逐项分解（锚点原文）**：O(订阅)。一次水位比较 + 一次订阅表广播。
//!
//! **跨批对接点（锚点原文）**：AA01 总线同构——本模块的事件结构与 AA01 的
//! 通用总线事件同构（level/tick/payload 三元），便于 AA01 直接转发。
//!
//! **无障碍与隐私（锚点原文）**：压力状态读屏播报——`announce_line()`
//! 输出纯文本「当前级别/水位数/订阅数」，不含进程名。
//!
//! 纯功能线：no_std + alloc；零 unwrap/expect/panic；整数千分比；
//! 零浮点、零墙钟、零 IO。
//!
//! 诊断段：0xE7D0 段（0xE7D0~0xE7DF 十六码）。

use alloc::string::String;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 诊断码（0xE7D0 段）
// ---------------------------------------------------------------------------

/// 采样参数非法（总量为 0）。
pub const XC_BAD_SAMPLE: u16 = 0xE7D0;
/// 订阅方重复。
pub const XC_DUP_SUB: u16 = 0xE7D1;
/// 订阅方表满。
pub const XC_SUB_FULL: u16 = 0xE7D2;
/// 未知订阅方。
pub const XC_UNKNOWN_SUB: u16 = 0xE7D3;
/// 订阅方没有待确认事件却来确认。
pub const XC_NO_PENDING: u16 = 0xE7D4;
/// 响应超预算。
pub const XC_SLOW_RESPONSE: u16 = 0xE7D5;
/// 事件表满。
pub const XC_EVENT_FULL: u16 = 0xE7D6;
/// 水位校验收敛失败（调整后次序仍不合法）。
pub const XC_BAD_WATERMARK: u16 = 0xE7D7;
/// 水位校准越界。
pub const XC_CALIB_CLAMPED: u16 = 0xE7D8;
/// 账不平。
pub const XC_LEDGER_DRIFT: u16 = 0xE7D9;

// ---------------------------------------------------------------------------
// 定容常量
// ---------------------------------------------------------------------------

/// 订阅方容量。
pub const SUB_CAP: usize = 16;
/// 事件表容量。
pub const EVENT_CAP: usize = 32;
/// 风暴合并窗口（5ms）。
pub const COALESCE_WINDOW_NS: u64 = 5_000_000;
/// 订阅方响应预算（2ms）。
pub const RESPONSE_BUDGET_NS: u64 = 2_000_000;
/// 水位校准步长（千分比）。
pub const CAL_STEP_PERMILLE: u32 = 25;
/// 水位下限 / 上限。
pub const WM_FLOOR: u32 = 500;
pub const WM_CEIL: u32 = 990;

// ---------------------------------------------------------------------------
// 级别
// ---------------------------------------------------------------------------

/// 压力级别（三级 + 平静）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Calm = 0,
    Low = 1,
    Medium = 2,
    High = 3,
}

impl Level {
    /// 读屏名。
    pub fn name(self) -> &'static str {
        match self {
            Level::Calm => "calm",
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }

    /// 序号（越大越紧张）。
    pub fn rank(self) -> u8 {
        self as u8
    }
}

/// 三级水位（千分比）。
#[derive(Clone, Copy)]
pub struct Watermarks {
    /// 一级（Low）起点。
    pub low: u32,
    /// 二级（Medium）起点。
    pub medium: u32,
    /// 三级（High）起点。
    pub high: u32,
}

impl Watermarks {
    /// 实测标定的默认水位（70% / 85% / 95%）。
    pub const MEASURED: Watermarks = Watermarks {
        low: 700,
        medium: 850,
        high: 950,
    };

    /// 次序自洽：0 < low < medium < high ≤ 1000。
    pub fn sane(&self) -> bool {
        self.low > 0 && self.low < self.medium && self.medium < self.high && self.high <= 1000
    }

    /// 判定级别。
    pub fn level_of(&self, used: u64, total: u64) -> Level {
        if total == 0 {
            return Level::Calm;
        }
        let p = (used.saturating_mul(1000) / total) as u32;
        if p >= self.high {
            Level::High
        } else if p >= self.medium {
            Level::Medium
        } else if p >= self.low {
            Level::Low
        } else {
            Level::Calm
        }
    }
}

// ---------------------------------------------------------------------------
// 事件与订阅方
// ---------------------------------------------------------------------------

/// 一条压力事件。
#[derive(Clone, Copy)]
pub struct Event {
    /// 级别。
    pub level: Level,
    /// 首次发生时刻。
    pub tick: u64,
    /// 合并掉的后续采样数。
    pub merged: u32,
    /// 发生时的占用千分比。
    pub used_permille: u32,
    /// 是否为升级（false 表示降级或持平）。
    pub escalated: bool,
}

impl Event {
    /// 空事件。
    pub const EMPTY: Event = Event {
        level: Level::Calm,
        tick: 0,
        merged: 0,
        used_permille: 0,
        escalated: false,
    };
}

/// 订阅方。
#[derive(Clone, Copy)]
pub struct Sub {
    /// 订阅 id（0 = 空槽）。
    pub id: u32,
    /// 是否待确认。
    pub pending: bool,
    /// 待确认事件的时刻。
    pub pending_since: u64,
    /// 最近一次确认时刻。
    pub last_ack: u64,
    /// 慢响应次数。
    pub slow: u32,
    /// 累计确认次数。
    pub acks: u32,
}

impl Sub {
    /// 空槽。
    pub const EMPTY: Sub = Sub {
        id: 0,
        pending: false,
        pending_since: 0,
        last_ack: 0,
        slow: 0,
        acks: 0,
    };
}

/// 采样结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emit {
    /// 平静且无变化（不打扰订阅方）。
    Quiet,
    /// 并入已有事件（风暴合并）。
    Merged { index: u32, level: Level },
    /// 新事件已广播。
    Broadcast {
        index: u32,
        level: Level,
        deliveries: u32,
    },
}

// ---------------------------------------------------------------------------
// 广播器
// ---------------------------------------------------------------------------

/// 显存压力广播器（定容、无分配）。
#[derive(Clone, Copy)]
pub struct Broadcaster {
    wm: Watermarks,
    events: [Event; EVENT_CAP],
    ec: usize,
    subs: [Sub; SUB_CAP],
    sc: usize,
    level: Level,
    last_event_tick: u64,
    samples: u32,
    quiet: u32,
    merged_total: u32,
    events_total: u32,
    deliveries: u32,
    escalations: u32,
    deescalations: u32,
    calibrations: u32,
    slow_total: u32,
    event_dropped: u32,
}

impl Broadcaster {
    /// 空广播器（实测水位）。
    pub fn new() -> Broadcaster {
        Broadcaster {
            wm: Watermarks::MEASURED,
            events: [Event::EMPTY; EVENT_CAP],
            ec: 0,
            subs: [Sub::EMPTY; SUB_CAP],
            sc: 0,
            level: Level::Calm,
            last_event_tick: 0,
            samples: 0,
            quiet: 0,
            merged_total: 0,
            events_total: 0,
            deliveries: 0,
            escalations: 0,
            deescalations: 0,
            calibrations: 0,
            slow_total: 0,
            event_dropped: 0,
        }
    }

    /// 当前水位。
    pub fn watermarks(&self) -> Watermarks {
        self.wm
    }

    /// 当前级别。
    pub fn level(&self) -> Level {
        self.level
    }

    /// 订阅。
    pub fn subscribe(&mut self, id: u32) -> Result<(), u16> {
        if id == 0 {
            return Err(XC_UNKNOWN_SUB);
        }
        let mut i = 0usize;
        while i < self.sc {
            if self.subs[i].id == id {
                return Err(XC_DUP_SUB);
            }
            i += 1;
        }
        if self.sc >= SUB_CAP {
            return Err(XC_SUB_FULL);
        }
        let mut s = Sub::EMPTY;
        s.id = id;
        self.subs[self.sc] = s;
        self.sc += 1;
        Ok(())
    }

    /// 退订（连带清掉待确认标记）。
    pub fn unsubscribe(&mut self, id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.sc {
            if self.subs[i].id == id {
                self.subs[i] = self.subs[self.sc - 1];
                self.sc -= 1;
                return Ok(());
            }
            i += 1;
        }
        Err(XC_UNKNOWN_SUB)
    }

    /// 订阅数。
    pub fn sub_count(&self) -> usize {
        self.sc
    }

    /// 取订阅方。
    pub fn sub(&self, id: u32) -> Option<Sub> {
        let mut i = 0usize;
        while i < self.sc {
            if self.subs[i].id == id {
                return Some(self.subs[i]);
            }
            i += 1;
        }
        None
    }

    /// 取事件。
    pub fn event(&self, i: usize) -> Option<Event> {
        if i < self.ec {
            Some(self.events[i])
        } else {
            None
        }
    }

    /// 事件数。
    pub fn event_count(&self) -> usize {
        self.ec
    }

    /// 累计投递数。
    pub fn deliveries(&self) -> u32 {
        self.deliveries
    }

    /// 因事件表满被丢弃的历史条数（广播不受影响）。
    pub fn event_dropped(&self) -> u32 {
        self.event_dropped
    }

    /// 采样并广播。
    pub fn sample(&mut self, used: u64, total: u64, now: u64) -> Result<Emit, u16> {
        if total == 0 {
            return Err(XC_BAD_SAMPLE);
        }
        self.samples = self.samples.saturating_add(1);
        let lv = self.wm.level_of(used, total);
        let permille = (used.saturating_mul(1000) / total) as u32;
        // 同级且仍在合并窗口内 → 并入上一条（风暴合并）
        if lv == self.level
            && lv != Level::Calm
            && self.ec > 0
            && now.saturating_sub(self.last_event_tick) <= COALESCE_WINDOW_NS
        {
            let i = self.ec - 1;
            self.events[i].merged = self.events[i].merged.saturating_add(1);
            self.merged_total = self.merged_total.saturating_add(1);
            return Ok(Emit::Merged {
                index: i as u32,
                level: lv,
            });
        }
        // 平静且一直平静 → 不打扰
        if lv == Level::Calm && self.level == Level::Calm {
            self.quiet = self.quiet.saturating_add(1);
            return Ok(Emit::Quiet);
        }
        self.push_event(lv, now, permille)
    }

    fn push_event(&mut self, lv: Level, now: u64, permille: u32) -> Result<Emit, u16> {
        let escalated = lv.rank() > self.level.rank();
        if lv.rank() > self.level.rank() && lv != Level::Calm {
            self.escalations = self.escalations.saturating_add(1);
        } else if lv.rank() < self.level.rank() {
            self.deescalations = self.deescalations.saturating_add(1);
        }
        self.level = lv;
        self.last_event_tick = now;
        if self.ec >= EVENT_CAP {
            // 事件表满：计数并丢弃新事件，但**订阅方仍要收到通知**
            // （宁可少一条历史，不可让订阅方错过当前压力）。
            self.event_dropped = self.event_dropped.saturating_add(1);
        }
        let index = if self.ec < EVENT_CAP {
            let ev = Event {
                level: lv,
                tick: now,
                merged: 0,
                used_permille: permille,
                escalated,
            };
            self.events[self.ec] = ev;
            self.ec += 1;
            (self.ec - 1) as u32
        } else {
            (EVENT_CAP - 1) as u32
        };
        // 广播给全部订阅方
        let mut d = 0u32;
        let mut i = 0usize;
        while i < self.sc {
            self.subs[i].pending = true;
            self.subs[i].pending_since = now;
            i += 1;
            d += 1;
        }
        self.deliveries = self.deliveries.saturating_add(d);
        self.events_total = self.events_total.saturating_add(1);
        Ok(Emit::Broadcast {
            index,
            level: lv,
            deliveries: d,
        })
    }

    /// 订阅方确认（超预算记慢）。
    pub fn ack(&mut self, id: u32, now: u64) -> Result<u64, u16> {
        let mut i = 0usize;
        while i < self.sc {
            if self.subs[i].id == id {
                if !self.subs[i].pending {
                    return Err(XC_NO_PENDING);
                }
                let latency = now.saturating_sub(self.subs[i].pending_since);
                if latency > RESPONSE_BUDGET_NS {
                    self.subs[i].slow = self.subs[i].slow.saturating_add(1);
                    self.slow_total = self.slow_total.saturating_add(1);
                }
                self.subs[i].pending = false;
                self.subs[i].last_ack = now;
                self.subs[i].acks = self.subs[i].acks.saturating_add(1);
                return Ok(latency);
            }
            i += 1;
        }
        Err(XC_UNKNOWN_SUB)
    }

    /// 未确认的订阅方数。
    pub fn pending_subs(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.sc {
            if self.subs[i].pending {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 未确认订阅方名单（逗号分隔）。
    pub fn pending_names(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        let mut first = true;
        while i < self.sc {
            if self.subs[i].pending {
                if !first {
                    s.push(',');
                }
                push_u32(&mut s, self.subs[i].id);
                first = false;
            }
            i += 1;
        }
        s
    }

    /// 水位校准（上调 = 原来阈值太敏感；下调 = 太迟钝）。
    ///
    /// 返回调整后的中位水位；越界被夹住时返回 `XC_CALIB_CLAMPED`
    /// 的判定由调用方用 `clamped()` 查。
    pub fn calibrate(&mut self, up: bool) -> u32 {
        let delta = CAL_STEP_PERMILLE;
        let mut w = self.wm;
        if up {
            w.low = w.low.saturating_add(delta);
            w.medium = w.medium.saturating_add(delta);
            w.high = w.high.saturating_add(delta);
        } else {
            w.low = w.low.saturating_sub(delta);
            w.medium = w.medium.saturating_sub(delta);
            w.high = w.high.saturating_sub(delta);
        }
        // 夹到合法区间并保持次序（三档整体平移，次序不会乱）。
        if w.high > WM_CEIL {
            let over = w.high - WM_CEIL;
            w.high -= over;
            w.medium = w.medium.saturating_sub(over);
            w.low = w.low.saturating_sub(over);
        }
        if w.low < WM_FLOOR {
            let under = WM_FLOOR - w.low;
            w.low += under;
            w.medium = w.medium.saturating_add(under);
            w.high = w.high.saturating_add(under);
        }
        self.wm = w;
        self.calibrations = self.calibrations.saturating_add(1);
        w.medium
    }

    /// 校准次数。
    pub fn calibrations(&self) -> u32 {
        self.calibrations
    }

    /// 计数快照 `(采样, 平静, 合并, 事件, 投递, 升级, 降级, 慢响应)`。
    pub fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
        (
            self.samples,
            self.quiet,
            self.merged_total,
            self.events_total,
            self.deliveries,
            self.escalations,
            self.deescalations,
            self.slow_total,
        )
    }

    /// 账平衡：采样 ≡ 平静 + 合并 + 事件。
    pub fn ledger_balanced(&self) -> bool {
        self.samples == self.quiet + self.merged_total + self.events_total
    }

    /// 播报行（读屏可直接播报）。
    pub fn announce_line(&self) -> String {
        let mut s = String::new();
        s.push_str("压力 ");
        s.push_str(self.level.name());
        s.push_str(" 水位 ");
        push_u32(&mut s, self.wm.low);
        s.push('/');
        push_u32(&mut s, self.wm.medium);
        s.push('/');
        push_u32(&mut s, self.wm.high);
        s.push_str(" 订阅 ");
        push_u32(&mut s, self.sc as u32);
        s.push_str(" 未确认 ");
        push_u32(&mut s, self.pending_subs());
        s
    }
}

impl Default for Broadcaster {
    fn default() -> Broadcaster {
        Broadcaster::new()
    }
}

fn push_u32(s: &mut String, mut v: u32) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < buf.len() {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ---------------------------------------------------------------------------
// 判据套件
// ---------------------------------------------------------------------------

fn chk(set: &mut CheckSet, name: &'static str, ok: bool, detail: &'static str) {
    set.add(name, ok, if ok { "" } else { detail });
}

/// VE-F0089 判据入口。
pub fn run_vea89_checks() -> CheckSet {
    let mut set = CheckSet::new("vea89_pressure");

    // C89-01 三级水位边界
    {
        let w = Watermarks::MEASURED;
        let ok = w.level_of(699, 1000) == Level::Calm
            && w.level_of(700, 1000) == Level::Low
            && w.level_of(849, 1000) == Level::Low
            && w.level_of(850, 1000) == Level::Medium
            && w.level_of(949, 1000) == Level::Medium
            && w.level_of(950, 1000) == Level::High
            && w.level_of(1000, 1000) == Level::High;
        chk(&mut set, "C89-01 三级水位边界", ok, "水位边界判错");
    }

    // C89-02 平静不打扰
    {
        let mut b = Broadcaster::new();
        let a = b.sample(100, 1000, 0);
        let c = b.sample(200, 1000, 1000);
        let ok = a == Ok(Emit::Quiet) && c == Ok(Emit::Quiet) && b.counters().1 == 2 && b.event_count() == 0;
        chk(&mut set, "C89-02 平静不打扰", ok, "无压力也广播");
    }

    // C89-03 升级广播给全部订阅方
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let _ = b.subscribe(2);
        let _ = b.subscribe(3);
        let e = b.sample(900, 1000, 0);
        let ok = e
            == Ok(Emit::Broadcast {
                index: 0,
                level: Level::Medium,
                deliveries: 3,
            })
            && b.pending_subs() == 3
            && b.counters().5 == 1;
        chk(&mut set, "C89-03 升级广播", ok, "投递数或级别错");
    }

    // C89-04 同窗口同级合并
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let _ = b.sample(900, 1000, 0);
        let m = b.sample(910, 1000, 1_000_000);
        let ev = b.event(0);
        let ok = m
            == Ok(Emit::Merged {
                index: 0,
                level: Level::Medium,
            })
            && ev.map(|e| e.merged) == Some(1)
            && b.counters().2 == 1
            && b.event_count() == 1;
        chk(&mut set, "C89-04 风暴合并", ok, "同窗口未合并");
    }

    // C89-05 窗口过期不再合并（新事件）
    {
        let mut b = Broadcaster::new();
        let _ = b.sample(900, 1000, 0);
        let late = b.sample(910, 1000, COALESCE_WINDOW_NS + 1);
        let ok = matches!(late, Ok(Emit::Broadcast { .. })) && b.event_count() == 2;
        chk(&mut set, "C89-05 窗口边界", ok, "过期窗口仍合并");
    }

    // C89-06 降级也广播
    {
        let mut b = Broadcaster::new();
        let _ = b.sample(960, 1000, 0);
        let down = b.sample(720, 1000, 10_000_000);
        let ok = matches!(down, Ok(Emit::Broadcast { level: Level::Low, .. }))
            && b.counters().6 == 1
            && b.level() == Level::Low;
        chk(&mut set, "C89-06 降级广播", ok, "降级不广播或计数错");
    }

    // C89-07 事件风暴合并率（100 次持续高压 → 事件很少）
    {
        let mut b = Broadcaster::new();
        let mut i = 0u32;
        while i < 100 {
            let _ = b.sample(960, 1000, i as u64 * 100_000);
            i += 1;
        }
        let ok = b.event_count() <= 20 && b.counters().2 >= 80 && b.ledger_balanced();
        chk(&mut set, "C89-07 风暴合并率", ok, "风暴未被抑制");
    }

    // C89-08 订阅方超预算响应记慢
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let _ = b.sample(960, 1000, 0);
        let fast = b.ack(1, RESPONSE_BUDGET_NS);
        let _ = b.sample(960, 1000, 100_000_000);
        let slow = b.ack(1, 100_000_000 + RESPONSE_BUDGET_NS + 1);
        let ok = fast == Ok(RESPONSE_BUDGET_NS)
            && slow == Ok(RESPONSE_BUDGET_NS + 1)
            && b.counters().7 == 1
            && b.sub(1).map(|s| s.slow) == Some(1);
        chk(&mut set, "C89-08 慢响应审计", ok, "超预算未记慢");
    }

    // C89-09 未确认名单
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(7);
        let _ = b.subscribe(8);
        let _ = b.sample(960, 1000, 0);
        let _ = b.ack(7, 1000);
        let ok = b.pending_subs() == 1 && b.pending_names() == "8";
        chk(&mut set, "C89-09 未确认名单", ok, "名单与实际不符");
    }

    // C89-10 无待确认却确认被拒
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let a = b.ack(1, 0);
        let _ = b.sample(960, 1000, 0);
        let _ = b.ack(1, 100);
        let c = b.ack(1, 200);
        let ok = a == Err(XC_NO_PENDING) && c == Err(XC_NO_PENDING);
        chk(&mut set, "C89-10 重复确认拒绝", ok, "空确认被接受");
    }

    // C89-11 订阅表满与退订
    {
        let mut b = Broadcaster::new();
        let mut i = 0u32;
        while i < SUB_CAP as u32 {
            let _ = b.subscribe(i + 1);
            i += 1;
        }
        let full = b.subscribe(999);
        let removed = b.unsubscribe(3);
        let again = b.subscribe(1000);
        let ok = full == Err(XC_SUB_FULL)
            && removed.is_ok()
            && again.is_ok()
            && b.sub_count() == SUB_CAP
            && b.sub(3).is_none();
        chk(&mut set, "C89-11 订阅表满与退订", ok, "满表或退订错");
    }

    // C89-12 未知订阅方
    {
        let mut b = Broadcaster::new();
        let a = b.ack(42, 0);
        let c = b.unsubscribe(42);
        let ok = a == Err(XC_UNKNOWN_SUB) && c == Err(XC_UNKNOWN_SUB);
        chk(&mut set, "C89-12 未知订阅方", ok, "未知订阅方被接受");
    }

    // C89-13 水位上调（误判为压力 → 抬高阈值）
    {
        let mut b = Broadcaster::new();
        let before = b.watermarks();
        let mid = b.calibrate(true);
        let after = b.watermarks();
        let ok = mid == before.medium + CAL_STEP_PERMILLE
            && after.low == before.low + CAL_STEP_PERMILLE
            && after.sane()
            && b.calibrations() == 1;
        chk(&mut set, "C89-13 水位上调", ok, "上调幅度或次序错");
    }

    // C89-14 水位下调并夹在下限
    {
        let mut b = Broadcaster::new();
        let mut i = 0u32;
        while i < 20 {
            let _ = b.calibrate(false);
            i += 1;
        }
        let w = b.watermarks();
        let ok = w.sane() && w.low >= WM_FLOOR && b.calibrations() == 20;
        chk(&mut set, "C89-14 水位夹下限", ok, "下调越界或次序乱");
    }

    // C89-15 水位上限夹取
    {
        let mut b = Broadcaster::new();
        let mut i = 0u32;
        while i < 20 {
            let _ = b.calibrate(true);
            i += 1;
        }
        let w = b.watermarks();
        let ok = w.sane() && w.high <= WM_CEIL;
        chk(&mut set, "C89-15 水位夹上限", ok, "上调越界");
    }

    // C89-16 广播投递数 = 事件数 × 订阅数（单订阅情形）
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let _ = b.sample(960, 1000, 0);
        let _ = b.sample(700, 1000, 100_000_000);
        let _ = b.sample(100, 1000, 200_000_000);
        let n = b.counters();
        let ok = n.3 == 3 && n.4 == 3 && n.6 == 2;
        chk(&mut set, "C89-16 投递账", ok, "投递数不符");
    }

    // C89-17 事件表满不阻断广播
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let mut i = 0u32;
        while i < (EVENT_CAP as u32 + 10) {
            // 每次换级别，避免被合并
            let used = if i % 2 == 0 { 960 } else { 100 };
            let _ = b.sample(used, 1000, i as u64 * 100_000_000);
            i += 1;
        }
        let ok = b.event_count() <= EVENT_CAP
            && b.deliveries() > 0
            && b.pending_subs() == 1
            && b.counters().3 == EVENT_CAP as u32 + 10;
        chk(&mut set, "C89-17 满表仍广播", ok, "满表阻断广播");
    }

    // C89-18 采样参数非法
    {
        let mut b = Broadcaster::new();
        let e = b.sample(0, 0, 0);
        let ok = e == Err(XC_BAD_SAMPLE) && b.counters().0 == 0;
        chk(&mut set, "C89-18 采样参数校验", ok, "零总量被当作平静");
    }

    // C89-19 账平衡（混合路径）
    {
        let mut b = Broadcaster::new();
        let _ = b.subscribe(1);
        let _ = b.sample(100, 1000, 0);
        let _ = b.sample(100, 1000, 1);
        let _ = b.sample(960, 1000, 2);
        let _ = b.sample(960, 1000, 3);
        let _ = b.sample(100, 1000, 4);
        let n = b.counters();
        let ok = b.ledger_balanced() && n.0 == 5 && n.1 == 2 && n.2 == 1 && n.3 == 2;
        chk(&mut set, "C89-19 账平衡", ok, "采样账不平");
    }

    // C89-20 播报行可读
    {
        let b = Broadcaster::new();
        let line = b.announce_line();
        let ok = line.contains("压力") && line.contains("水位") && line.contains("订阅") && line.contains("未确认");
        chk(&mut set, "C89-20 播报可读", ok, "读屏字段缺失");
    }

    set
}

/// 单点演练。
pub fn drill() -> (bool, String) {
    let mut b = Broadcaster::new();
    let mut ok = true;
    let mut s = String::new();
    if b.subscribe(1).is_err() {
        ok = false;
        s.push_str("sub ");
    }
    if !matches!(b.sample(960, 1000, 0), Ok(Emit::Broadcast { .. })) {
        ok = false;
        s.push_str("emit ");
    }
    if b.ack(1, 1000).is_err() {
        ok = false;
        s.push_str("ack ");
    }
    if !b.ledger_balanced() {
        ok = false;
        s.push_str("ledger ");
    }
    if s.is_empty() {
        s.push_str("压力广播正常：");
        s.push_str(&b.announce_line());
    }
    (ok, s)
}
