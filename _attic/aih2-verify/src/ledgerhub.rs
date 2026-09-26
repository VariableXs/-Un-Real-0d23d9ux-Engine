//! 账本订阅面（WP-206 · B-2001）：一套账本多处消费，数字永远同源。
//!
//! MD2 篇 20.1：系统监视器是内核账本的订阅者。订阅模型按主题分频道
//! （内存、进程、CPU、块 IO、网络、温度风扇预留），每频道节流推送
//! （一秒粒度，实时曲线用窗口聚合——六十点滚动窗口）。订阅接口进
//! vx-SDK：监视器之外，更新器与诊断中心复用同一接口——一份打点三个
//! 消费者，不许各插各的桩（篇 14.1）。数据语义纪律：每个数字带单位
//! 与口径（内存分"应用、缓存、内核"三段，口径在界面固定注释——
//! 不解释口径的数字是误导源）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 频道模型：六频道（篇 20.1 枚举齐，温度风扇为预留占位频道）
// ---------------------------------------------------------------------------

pub const CHANNEL_COUNT: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Mem,
    Proc,
    Cpu,
    BlockIo,
    Net,
    ThermalFan, // 预留：阶段 3 接入（EC 通道），先占位明示
}

pub const ALL_CHANNELS: [Channel; CHANNEL_COUNT] = [
    Channel::Mem,
    Channel::Proc,
    Channel::Cpu,
    Channel::BlockIo,
    Channel::Net,
    Channel::ThermalFan,
];

// ---------------------------------------------------------------------------
// 读数：每个数字带单位与口径（篇 20.1 数据语义纪律）
// ---------------------------------------------------------------------------

pub const WINDOW_SLOTS: usize = 60; // 六十点滚动窗口
pub const THROTTLE_MS: u64 = 1000; // 一秒粒度节流
pub const SUBSCRIBER_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    KB,
    Pct,
    Mbps,
    Iops,
    Celsius,
    Count,
}

/// 单个读数：值 + 单位（口径随读数走，呈现面不许自行解释）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub value: u64,
    pub unit: Unit,
}

impl Reading {
    pub const fn new(value: u64, unit: Unit) -> Self {
        Reading { value, unit }
    }
}

/// 内存读数三段口径（应用/缓存/内核——口径在界面固定注释）。
pub const MEM_SEGMENTS: usize = 3;
/// 固定口径注释（呈现面原样展示，不是可选文案）。
pub const MEM_NOTE: &str = "内存口径：应用=进程私有页；缓存=页缓存与回写队列；内核=内核堆与元数据。三段相加为总占用。";

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemReading {
    pub app_kb: u64,
    pub cache_kb: u64,
    pub kernel_kb: u64,
}

impl MemReading {
    pub fn total_kb(&self) -> u64 {
        self.app_kb + self.cache_kb + self.kernel_kb
    }
}

/// 一帧账本快照：六频道当前读数 + 内存三段 + 单调序号。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub seq: u64,
    pub mem: MemReading,
    pub proc_count: u64,
    pub cpu_permille: u64, // 千分比（0..=1000，整数口径）
    pub blk_iops: u64,
    pub net_mbps: u64,
    pub thermal_c: i32, // 温度：阶段 3 前恒 PLACEHOLDER_TEMP
}

/// 阶段 3 占位温度：呈现面看到此值必须显示"阶段 3"占位而非编造数字。
pub const PLACEHOLDER_TEMP: i32 = 0;
/// 阶段 3 占位文案（诚实呈现——做不到与呈现假数据之间，永远选前者）。
pub const THERMAL_PLACEHOLDER_NOTE: &str = "温度与风扇：阶段 3 接入（EC 通道）";

impl Frame {
    pub fn channel_reading(&self, ch: Channel) -> Reading {
        match ch {
            Channel::Mem => Reading::new(self.mem.total_kb(), Unit::KB),
            Channel::Proc => Reading::new(self.proc_count, Unit::Count),
            Channel::Cpu => Reading::new(self.cpu_permille, Unit::Pct),
            Channel::BlockIo => Reading::new(self.blk_iops, Unit::Iops),
            Channel::Net => Reading::new(self.net_mbps, Unit::Mbps),
            Channel::ThermalFan => Reading::new(self.thermal_c.max(0) as u64, Unit::Celsius),
        }
    }
}

// ---------------------------------------------------------------------------
// 账本：读接口开放，写只在记账路径（篇 14.1：账本接口只有读，
// 写只在内核分配路径——本层建模为 record() 唯一写入口，常数时间）。
// ---------------------------------------------------------------------------

pub struct Ledger {
    pub now_ms: u64,
    seq: u64,
    cur: Frame,
}

impl Ledger {
    pub fn new() -> Self {
        Ledger {
            now_ms: 0,
            seq: 0,
            cur: Frame {
                seq: 0,
                mem: MemReading { app_kb: 0, cache_kb: 0, kernel_kb: 0 },
                proc_count: 0,
                cpu_permille: 0,
                blk_iops: 0,
                net_mbps: 0,
                thermal_c: PLACEHOLDER_TEMP,
            },
        }
    }

    /// 记账入口（常数时间：无循环无分配，逐字段赋值）。
    pub fn record(&mut self, mem: MemReading, proc_count: u64, cpu_permille: u64, blk_iops: u64, net_mbps: u64) {
        self.seq += 1;
        self.cur = Frame {
            seq: self.seq,
            mem,
            proc_count,
            cpu_permille,
            blk_iops,
            net_mbps,
            thermal_c: PLACEHOLDER_TEMP,
        };
    }

    /// 读接口：只有读（订阅面唯一数据源——同源的前提是只有一个源）。
    pub fn read(&self) -> Frame {
        self.cur
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.now_ms += ms;
    }
}

// ---------------------------------------------------------------------------
// 订阅面：多消费者 + 一秒节流 + 六十点滚动窗口
// ---------------------------------------------------------------------------

/// 消费者身份（监视器/更新器/诊断中心——vx-SDK 订阅接口的三类合法消费方）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consumer {
    Monitor,
    Updater,
    DiagCenter,
}

pub const CONSUMER_COUNT: usize = 3;

#[derive(Clone, Copy)]
struct SubSlot {
    used: bool,
    who: Consumer,
    last_push_ms: u64,
    /// 该消费者最近收到的帧（对表演练与节流缓存面）。
    held: Option<Frame>,
    delivered: u64,
}

impl SubSlot {
    const fn empty() -> Self {
        SubSlot { used: false, who: Consumer::Monitor, last_push_ms: 0, held: None, delivered: 0 }
    }
}

/// 订阅枢纽：所有消费者从同一账本取数，节流与窗口在此统一——
/// 不许各插各的桩（篇 14.1：一份打点三个消费者）。
pub struct SubHub {
    subs: [SubSlot; SUBSCRIBER_CAP],
    window: Window,
}

impl SubHub {
    pub fn new() -> Self {
        SubHub { subs: [SubSlot::empty(); SUBSCRIBER_CAP], window: Window::new() }
    }

    /// 订阅：返回槽位号；同一消费者重复订阅返回既有槽位（幂等）。
    pub fn subscribe(&mut self, who: Consumer) -> Option<usize> {
        for i in 0..SUBSCRIBER_CAP {
            if self.subs[i].used && self.subs[i].who == who {
                return Some(i);
            }
        }
        for i in 0..SUBSCRIBER_CAP {
            if !self.subs[i].used {
                self.subs[i] = SubSlot { used: true, who, last_push_ms: 0, held: None, delivered: 0 };
                return Some(i);
            }
        }
        None
    }

    /// 注销：槽位回收（可再订阅——资源面守恒）。
    pub fn unsubscribe(&mut self, who: Consumer) -> bool {
        for i in 0..SUBSCRIBER_CAP {
            if self.subs[i].used && self.subs[i].who == who {
                self.subs[i] = SubSlot::empty();
                return true;
            }
        }
        false
    }

    /// 节流广播：账本推进后调用。同一订阅者两次推送间隔 >= THROTTLE_MS；
    /// 节流窗口内的调用不推送（返回 false），不产生半帧。
    pub fn broadcast(&mut self, ledger: &Ledger) -> usize {
        let frame = ledger.read();
        let now = ledger.now_ms;
        self.window.push(frame.channel_reading(Channel::Cpu).value);
        let mut pushed = 0;
        for i in 0..SUBSCRIBER_CAP {
            if !self.subs[i].used {
                continue;
            }
            // 首帧（last_push_ms==0 且从未投递）直接推，之后按一秒节流。
            if self.subs[i].delivered > 0 && now.saturating_sub(self.subs[i].last_push_ms) < THROTTLE_MS {
                continue;
            }
            self.subs[i].held = Some(frame);
            self.subs[i].last_push_ms = now;
            self.subs[i].delivered += 1;
            pushed += 1;
        }
        pushed
    }

    /// 消费者读数：只读自己槽位持有的帧（对表演练的取数面）。
    pub fn read_by(&self, who: Consumer) -> Option<Frame> {
        for i in 0..SUBSCRIBER_CAP {
            if self.subs[i].used && self.subs[i].who == who {
                return self.subs[i].held;
            }
        }
        None
    }

    pub fn delivered_by(&self, who: Consumer) -> u64 {
        for i in 0..SUBSCRIBER_CAP {
            if self.subs[i].used && self.subs[i].who == who {
                return self.subs[i].delivered;
            }
        }
        0
    }

    pub fn window_agg(&self) -> WindowAgg {
        self.window.agg()
    }
}

// ---------------------------------------------------------------------------
// 六十点滚动窗口（实时曲线聚合——篇 20.1）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Window {
    slots: [u64; WINDOW_SLOTS],
    head: usize,
    len: usize,
    pushed_total: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WindowAgg {
    pub len: usize,
    pub sum: u64,
    pub min: u64,
    pub max: u64,
}

impl WindowAgg {
    /// 整数平均（sum/len，len>0 时合法）。
    pub fn mean(&self) -> u64 {
        if self.len == 0 {
            0
        } else {
            self.sum / self.len as u64
        }
    }
}

impl Window {
    pub const fn new() -> Self {
        Window { slots: [0; WINDOW_SLOTS], head: 0, len: 0, pushed_total: 0 }
    }

    pub fn push(&mut self, v: u64) {
        self.slots[self.head] = v;
        self.head = (self.head + 1) % WINDOW_SLOTS;
        if self.len < WINDOW_SLOTS {
            self.len += 1;
        }
        self.pushed_total += 1;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn pushed_total(&self) -> u64 {
        self.pushed_total
    }

    /// 被覆盖次数 = 推送总数 - 窗口内容量（恒 >= 0，环形语义）。
    pub fn evicted_total(&self) -> u64 {
        self.pushed_total - self.len as u64
    }

    pub fn agg(&self) -> WindowAgg {
        if self.len == 0 {
            return WindowAgg { len: 0, sum: 0, min: 0, max: 0 };
        }
        let mut sum = 0u64;
        let mut min = u64::MAX;
        let mut max = 0u64;
        for i in 0..self.len {
            let v = self.slots[i];
            sum += v;
            if v < min {
                min = v;
            }
            if v > max {
                max = v;
            }
        }
        WindowAgg { len: self.len, sum, min, max }
    }
}

// ---------------------------------------------------------------------------
// 对表演练：三消费者同帧逐字段一致（B-2001 达标线）
// ---------------------------------------------------------------------------

/// 对表演练：一次 record + 一次 broadcast 后，三类消费者各自取到的帧
/// 必须与账本当前帧逐字段一致——两套数字等于没有数字。
pub fn run_recon_drill() -> bool {
    let mut led = Ledger::new();
    let mut hub = SubHub::new();
    if hub.subscribe(Consumer::Monitor).is_none()
        || hub.subscribe(Consumer::Updater).is_none()
        || hub.subscribe(Consumer::DiagCenter).is_none()
    {
        return false;
    }
    led.advance_ms(10_000);
    led.record(
        MemReading { app_kb: 812_640, cache_kb: 262_144, kernel_kb: 96_432 },
        137,
        640,
        82,
        12,
    );
    hub.broadcast(&led);
    let expect = led.read();
    for k in 0..CONSUMER_COUNT {
        let who = match k {
            0 => Consumer::Monitor,
            1 => Consumer::Updater,
            _ => Consumer::DiagCenter,
        };
        let got = match hub.read_by(who) {
            Some(f) => f,
            None => return false,
        };
        if got != expect {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-2001 · 9 项）
// ---------------------------------------------------------------------------

pub fn run_ledgerhub_checks() -> CheckSet {
    let mut set = CheckSet::new("B-2001 账本订阅面");
    // 1. 六频道枚举齐（含温度风扇预留占位）。
    set.add(
        "B-2001 六频道枚举齐",
        ALL_CHANNELS.len() == CHANNEL_COUNT
            && ALL_CHANNELS[0] == Channel::Mem
            && ALL_CHANNELS[5] == Channel::ThermalFan,
        "订阅频道六类齐备，温度风扇为预留位",
    );
    // 2. 账本读接口面：record 唯一写入口，read 只读，seq 单调。
    let mut led = Ledger::new();
    led.record(MemReading { app_kb: 1, cache_kb: 2, kernel_kb: 3 }, 4, 5, 6, 7);
    let f1 = led.read();
    led.record(MemReading { app_kb: 10, cache_kb: 20, kernel_kb: 30 }, 40, 500, 60, 70);
    let f2 = led.read();
    set.add(
        "B-2001 账本读接口与单调序",
        f1.seq == 1 && f2.seq == 2 && f2.proc_count == 40 && f2.cpu_permille == 500,
        "record 唯一写入口，read 只读，序号严格单调",
    );
    // 3. 节流一秒粒度：一秒内多次广播只推一帧。
    let mut led3 = Ledger::new();
    let mut hub3 = SubHub::new();
    let s3 = hub3.subscribe(Consumer::Monitor).unwrap_or(0);
    led3.advance_ms(5_000);
    led3.record(MemReading { app_kb: 1, cache_kb: 0, kernel_kb: 0 }, 1, 1, 1, 1);
    let p1 = hub3.broadcast(&led3);
    led3.advance_ms(500);
    led3.record(MemReading { app_kb: 2, cache_kb: 0, kernel_kb: 0 }, 2, 2, 2, 2);
    let p2 = hub3.broadcast(&led3);
    led3.advance_ms(400);
    led3.record(MemReading { app_kb: 3, cache_kb: 0, kernel_kb: 0 }, 3, 3, 3, 3);
    let p3 = hub3.broadcast(&led3);
    set.add(
        "B-2001 节流一秒粒度",
        s3 == 0 && p1 == 1 && p2 == 0 && p3 == 0,
        "一秒窗口内重复广播不产生第二帧（节流不破不立）",
    );
    // 4. 节流不是永久锁：过一秒后新帧可推。
    led3.advance_ms(200);
    led3.record(MemReading { app_kb: 9, cache_kb: 0, kernel_kb: 0 }, 9, 9, 9, 9);
    let p4 = hub3.broadcast(&led3);
    let got4 = hub3.read_by(Consumer::Monitor);
    set.add(
        "B-2001 节流窗口后新帧可推",
        p4 == 1 && matches!(got4, Some(f) if f.proc_count == 9),
        "累计满一秒后广播恢复推送，且推的是最新帧",
    );
    // 5. 六十点窗口恒容量：push 61 次后 len==60 且最旧被覆盖。
    let mut w = Window::new();
    let mut i = 0u64;
    while i < 61 {
        w.push(i + 1);
        i += 1;
    }
    // 第 61 次推送覆盖第 1 个值（值 1），窗口内最小值应为 2。
    let agg5 = w.agg();
    set.add(
        "B-2001 滚动窗口恒容量",
        w.len() == WINDOW_SLOTS && w.pushed_total() == 61 && agg5.min == 2 && agg5.max == 61,
        "六十点环形窗口，第 61 点覆盖最旧点，容量恒不涨",
    );
    // 6. 窗口聚合整数面：sum/min/max/mean 自洽。
    set.add(
        "B-2001 窗口聚合自洽",
        agg5.sum == (2u64 + 61) * 60 / 2 && agg5.mean() == agg5.sum / 60 && w.evicted_total() == 1,
        "聚合 sum/min/max/mean 整数自洽，覆盖计数=推送-容量",
    );
    // 7. 内存三段口径：恒等式 + 口径注释非空（不解释口径的数字是误导源）。
    let mem7 = MemReading { app_kb: 812_640, cache_kb: 262_144, kernel_kb: 96_432 };
    let mut led7 = Ledger::new();
    led7.record(mem7, 1, 1, 1, 1);
    let fr7 = led7.read();
    set.add(
        "B-2001 内存三段口径",
        fr7.mem.total_kb() == mem7.app_kb + mem7.cache_kb + mem7.kernel_kb
            && !MEM_NOTE.is_empty()
            && MEM_SEGMENTS == 3,
        "三段相加==总占用恒等，口径注释随界面固定展示",
    );
    // 8. 多消费者对表演练：三消费者同帧逐字段一致。
    set.add(
        "B-2001 多消费者对表演练",
        run_recon_drill(),
        "监视器/更新器/诊断中心同帧逐字段一致——一套账本多处消费",
    );
    // 9. 订阅者槽位守恒：注销后槽位回收可再订阅，幂等订阅不占新槽。
    let mut hub9 = SubHub::new();
    let a9 = hub9.subscribe(Consumer::Monitor);
    let a9b = hub9.subscribe(Consumer::Monitor);
    let u9 = hub9.unsubscribe(Consumer::Monitor);
    let c9 = hub9.subscribe(Consumer::DiagCenter);
    set.add(
        "B-2001 订阅槽位守恒",
        a9.is_some() && a9b.is_some() && a9 == a9b && u9 && c9.is_some(),
        "幂等订阅不占新槽，注销回收后可再订阅",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fa01 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fa01_throttle_window() {
        // 节流与窗口联合：1 秒内 5 次广播只推 1 帧；窗口聚合覆盖。
        let mut led = Ledger::new();
        let mut hub = SubHub::new();
        assert!(hub.subscribe(Consumer::Monitor).is_some());
        led.advance_ms(2_000);
        let mut pushed = 0;
        let mut i = 0;
        while i < 5 {
            led.record(MemReading { app_kb: i, cache_kb: 0, kernel_kb: 0 }, i, i, i, i);
            if hub.broadcast(&led) == 1 {
                pushed += 1;
            }
            led.advance_ms(100); // 每次间隔 100ms，全程 500ms < 1s
            i += 1;
        }
        assert_eq!(pushed, 1, "一秒内五次广播只推首帧");
        assert_eq!(hub.window_agg().len, 5, "窗口记录全部推送尝试点的值");
    }

    #[test]
    fn fa01_recon_drill() {
        assert!(run_recon_drill(), "三消费者对表演练必须逐字段一致");
    }

    #[test]
    fn fa01_mem_segments() {
        let mem = MemReading { app_kb: 100, cache_kb: 30, kernel_kb: 20 };
        assert_eq!(mem.total_kb(), 150);
        assert!(!MEM_NOTE.is_empty());
        assert_eq!(MEM_SEGMENTS, 3);
    }

    #[test]
    fn fa01_sub_reuse() {
        let mut hub = SubHub::new();
        let s1 = hub.subscribe(Consumer::Monitor);
        assert!(s1.is_some());
        assert!(hub.unsubscribe(Consumer::Monitor));
        assert!(!hub.unsubscribe(Consumer::Monitor), "重复注销返回 false 而非误伤他人槽位");
        let s2 = hub.subscribe(Consumer::Monitor);
        assert!(s2.is_some(), "注销后槽位回收可再订阅");
    }
}
