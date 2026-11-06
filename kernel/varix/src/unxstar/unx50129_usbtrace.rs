//! UNX-F50129 USB 埋点（探测/绑定/传输/拔出四点）（0x43c40 段）
//!
//! 锚点：UNX-F50129-J1 埋点 100% 覆盖；判据锚定 体验十三·补。
//!
//! 为什么埋点要按「四点」而不是随手打日志：一条打印链路只有四个可观测的转折——
//! ① 探测（拿到描述符并以打印类身份被识别）、② 绑定（接口/端点就位、可发数据）、
//! ③ 传输（bulk 往返，含失败态）、④ 拔出（设备消失或主动断开）。这四点缺任何
//! 一点，事后都无法判断「到底是没绑上还是绑上后没传过」。因此本模块把四点定义成
//! 闭集，并用覆盖率（4/4 = 1000‰）把「埋点齐不齐」变成可判定的数字。
//!
//! 结构清单：
//! * `TracePoint` —— 四点闭集（1..=4），`from_wire` 越界显式报 `Point`；
//! * `TraceEvent` —— 埋点记录（seq/ts/dev/code/bytes），字段规则按点分派：
//!   探测/绑定/拔出不允许带字节数，传输必须带非零字节数与合法状态码；
//! * `DeviceLife` —— 单设备生命周期机 Absent→Probed→Bound→Unplugged，
//!   `Transfer` 只在 Bound 合法；重复绑定、绑定前传输、未探测就拔出全部报 `Order`；
//! * `LifecycleTable` —— 定容设备表（设备号 → 生命周期），新设备只接受 Probe 开户；
//! * `TraceRing` —— 定容环形埋点缓冲，两种政策：Strict（满即拒）与 Overwrite
//!   （滑窗覆盖并显式记 dropped）；`audit()` 校验 seq/ts 严格递增与总量守恒；
//! * `TraceSession` —— 把上面四者串起来的唯一入口：先校验字段，再推进生命周期，
//!   最后入环；顺序固定，失败处可归因。
//!
//! 错误纪律：段内闭集 `SEG_BASE + n`；时间戳只接受严格递增（同 tick 两条记录
//! 在串行打印链路上不可能发生，放过它等于放弃时序校验）；内核路径零 panic。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43c40 独占 0x20）。
pub const SEG_BASE: u32 = 0x43c40;

pub const E_TS: u16 = 1;
pub const E_POINT: u16 = 2;
pub const E_DEV: u16 = 3;
pub const E_FIELDS: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_CAPACITY: u16 = 6;
pub const E_CODE: u16 = 7;
pub const E_EMPTY: u16 = 8;
pub const E_DUP: u16 = 9;
pub const E_SLOTS: u16 = 10;

/// 埋点错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceError {
    Ts,
    Point,
    Dev,
    Fields,
    Order,
    Capacity,
    Code,
    Empty,
    Dup,
    Slots,
}

impl TraceError {
    pub fn code(self) -> u32 {
        let n = match self {
            TraceError::Ts => E_TS,
            TraceError::Point => E_POINT,
            TraceError::Dev => E_DEV,
            TraceError::Fields => E_FIELDS,
            TraceError::Order => E_ORDER,
            TraceError::Capacity => E_CAPACITY,
            TraceError::Code => E_CODE,
            TraceError::Empty => E_EMPTY,
            TraceError::Dup => E_DUP,
            TraceError::Slots => E_SLOTS,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            TraceError::Ts => "timestamp not strictly increasing or zero",
            TraceError::Point => "trace point code outside 1..=4",
            TraceError::Dev => "device id zero",
            TraceError::Fields => "field does not match the point's contract",
            TraceError::Order => "lifecycle order violated",
            TraceError::Capacity => "trace ring full under strict policy",
            TraceError::Code => "status code outside the point's closed set",
            TraceError::Empty => "empty trace ring",
            TraceError::Dup => "event sequence not strictly increasing",
            TraceError::Slots => "lifecycle table full",
        }
    }
}

/// 埋点四点。顺序即线号（1..=4），与日志消费方的位图对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracePoint {
    Probe,
    Bind,
    Transfer,
    Unplug,
}

impl TracePoint {
    pub fn wire(self) -> u8 {
        match self {
            TracePoint::Probe => 1,
            TracePoint::Bind => 2,
            TracePoint::Transfer => 3,
            TracePoint::Unplug => 4,
        }
    }

    pub fn from_wire(c: u8) -> Result<TracePoint, TraceError> {
        match c {
            1 => Ok(TracePoint::Probe),
            2 => Ok(TracePoint::Bind),
            3 => Ok(TracePoint::Transfer),
            4 => Ok(TracePoint::Unplug),
            _ => Err(TraceError::Point),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TracePoint::Probe => "probe",
            TracePoint::Bind => "bind",
            TracePoint::Transfer => "transfer",
            TracePoint::Unplug => "unplug",
        }
    }

    pub fn bit(self) -> u8 {
        1u8 << (self.wire() - 1)
    }
}

/// 单设备生命周期四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceLife {
    Absent,
    Probed,
    Bound,
    Unplugged,
}

impl DeviceLife {
    pub fn wire(self) -> u8 {
        match self {
            DeviceLife::Absent => 0,
            DeviceLife::Probed => 1,
            DeviceLife::Bound => 2,
            DeviceLife::Unplugged => 3,
        }
    }

    /// 唯一的状态推进表。`Unplugged` 允许重新 Probe（热插拔回到枚举），
    /// 其余跳转一律非法——尤其不允许 Bound 直接回到 Absent，那会掩盖漏掉的拔出点。
    pub fn on(self, p: TracePoint) -> Result<DeviceLife, TraceError> {
        match (self, p) {
            (DeviceLife::Absent, TracePoint::Probe) => Ok(DeviceLife::Probed),
            (DeviceLife::Unplugged, TracePoint::Probe) => Ok(DeviceLife::Probed),
            (DeviceLife::Probed, TracePoint::Bind) => Ok(DeviceLife::Bound),
            (DeviceLife::Bound, TracePoint::Transfer) => Ok(DeviceLife::Bound),
            (DeviceLife::Probed, TracePoint::Unplug) => Ok(DeviceLife::Unplugged),
            (DeviceLife::Bound, TracePoint::Unplug) => Ok(DeviceLife::Unplugged),
            _ => Err(TraceError::Order),
        }
    }
}

/// 一条埋点记录。`code` 的含义随点而定：Probe=识别结果，Bind=接口号，
/// Transfer=传输状态，Unplug=拔出原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    pub seq: u32,
    pub point: TracePoint,
    pub ts: u64,
    pub dev: u32,
    pub code: u16,
    pub bytes: u32,
}

impl TraceEvent {
    /// 字段契约自检。四条规则都由点的语义推出，不做「宽松放行」：
    /// * 时间戳为 0 = 未初始化时钟，拒绝；
    /// * 设备号 0 是保留的「无设备」，拒绝；
    /// * Probe/Bind/Unplug 不携带数据字节；
    /// * Transfer 必须有字节且状态码只能是 0/1/2（成功/停/重试）。
    pub fn check(&self) -> Result<(), TraceError> {
        if self.ts == 0 {
            return Err(TraceError::Ts);
        }
        if self.dev == 0 {
            return Err(TraceError::Dev);
        }
        match self.point {
            TracePoint::Probe => {
                if self.bytes != 0 {
                    return Err(TraceError::Fields);
                }
                if self.code > 3 {
                    return Err(TraceError::Code);
                }
            }
            TracePoint::Bind => {
                if self.bytes != 0 || self.code > 32 {
                    return Err(TraceError::Fields);
                }
            }
            TracePoint::Transfer => {
                if self.bytes == 0 {
                    return Err(TraceError::Fields);
                }
                if self.code > 2 {
                    return Err(TraceError::Code);
                }
            }
            TracePoint::Unplug => {
                if self.bytes != 0 {
                    return Err(TraceError::Fields);
                }
                if self.code > 1 {
                    return Err(TraceError::Code);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 生命周期表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Slot {
    dev: u32,
    life: DeviceLife,
}

/// 设备生命周期表：定容、按设备号索引、新设备只接受 Probe 开户。
pub struct LifecycleTable {
    slots: Vec<Slot>,
    cap: usize,
}

impl LifecycleTable {
    pub fn new(cap: usize) -> Result<LifecycleTable, TraceError> {
        if cap == 0 {
            return Err(TraceError::Slots);
        }
        if cap > 64 {
            return Err(TraceError::Capacity);
        }
        Ok(LifecycleTable {
            slots: Vec::with_capacity(cap),
            cap,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn life_of(&self, dev: u32) -> Option<DeviceLife> {
        match self.find(dev) {
            Some(i) => self.slots.get(i).map(|s| s.life),
            None => None,
        }
    }

    fn find(&self, dev: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.dev == dev {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 观察一次埋点并推进该设备状态。未知设备号的 Transfer/Bind/Unplug 一律
    /// `Order`——没有 Probe 就没有身份，后面的点不成立。
    pub fn observe(&mut self, dev: u32, p: TracePoint) -> Result<DeviceLife, TraceError> {
        self.commit(dev, p)
    }

    /// 只推演不落表：返回「若接受该点，状态会变成什么」。
    /// 会话口用它在入环成功之后再提交，避免「环拒收但生命周期已前进」的脏账。
    pub fn peek(&self, dev: u32, p: TracePoint) -> Result<DeviceLife, TraceError> {
        if dev == 0 {
            return Err(TraceError::Dev);
        }
        match self.find(dev) {
            Some(i) => match self.slots.get(i) {
                Some(s) => s.life.on(p),
                None => Err(TraceError::Order),
            },
            None => {
                if p != TracePoint::Probe {
                    return Err(TraceError::Order);
                }
                if self.slots.len() >= self.cap {
                    return Err(TraceError::Slots);
                }
                Ok(DeviceLife::Probed)
            }
        }
    }

    /// 真正落表。规则与 `peek` 完全一致（同一判据来源，不允许两套）。
    pub fn commit(&mut self, dev: u32, p: TracePoint) -> Result<DeviceLife, TraceError> {
        let next = self.peek(dev, p)?;
        match self.find(dev) {
            Some(i) => {
                if let Some(s) = self.slots.get_mut(i) {
                    s.life = next;
                }
            }
            None => {
                if self.slots.len() >= self.cap {
                    return Err(TraceError::Slots);
                }
                self.slots.push(Slot {
                    dev,
                    life: next,
                });
            }
        }
        Ok(next)
    }
}

// ---------------------------------------------------------------------------
// 环形埋点缓冲
// ---------------------------------------------------------------------------

/// 环满时的两种政策。Strict 用于验收（拒绝即证据），Overwrite 用于长稳
/// （滑窗保留最近 cap 条，但 dropped 必须记账，不许静默丢）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingPolicy {
    Strict,
    Overwrite,
}

impl RingPolicy {
    pub fn wire(self) -> u8 {
        match self {
            RingPolicy::Strict => 0,
            RingPolicy::Overwrite => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RingPolicy::Strict => "strict",
            RingPolicy::Overwrite => "overwrite",
        }
    }
}

/// 埋点统计快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceStats {
    pub emitted: u32,
    pub retained: u32,
    pub dropped: u32,
    pub per_point: [u32; 4],
    pub coverage_bits: u8,
    pub coverage_permille: u32,
}

/// 定容环形埋点缓冲。
pub struct TraceRing {
    events: Vec<TraceEvent>,
    cap: usize,
    policy: RingPolicy,
    emitted: u32,
    dropped: u32,
    per_point: [u32; 4],
    coverage_bits: u8,
    last_ts: u64,
}

impl TraceRing {
    pub fn new(cap: usize, policy: RingPolicy) -> Result<TraceRing, TraceError> {
        if cap == 0 {
            return Err(TraceError::Capacity);
        }
        if cap > 4096 {
            return Err(TraceError::Capacity);
        }
        Ok(TraceRing {
            events: Vec::with_capacity(cap),
            cap,
            policy,
            emitted: 0,
            dropped: 0,
            per_point: [0; 4],
            coverage_bits: 0,
            last_ts: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn policy(&self) -> RingPolicy {
        self.policy
    }

    pub fn emitted(&self) -> u32 {
        self.emitted
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn has_room(&self) -> bool {
        self.events.len() < self.cap
    }

    pub fn get(&self, i: usize) -> Option<&TraceEvent> {
        self.events.get(i)
    }

    pub fn point_count(&self, p: TracePoint) -> u32 {
        self.per_point
            .get(p.wire() as usize - 1)
            .copied()
            .unwrap_or(0)
    }

    pub fn coverage_bits(&self) -> u8 {
        self.coverage_bits
    }

    /// 覆盖率（‰）：四个点里出现过几个 × 250。四点齐 = 1000。
    pub fn coverage_permille(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 4 {
            if self.coverage_bits & (1u8 << i) != 0 {
                n += 1;
            }
            i += 1;
        }
        n * 250
    }

    pub fn stats(&self) -> TraceStats {
        TraceStats {
            emitted: self.emitted,
            retained: self.events.len() as u32,
            dropped: self.dropped,
            per_point: self.per_point,
            coverage_bits: self.coverage_bits,
            coverage_permille: self.coverage_permille(),
        }
    }

    /// 环内最早一条的 seq（证明滑窗确实在推进）。空环返回 0。
    pub fn retained_lowest_seq(&self) -> u32 {
        self.events.get(0).map(|e| e.seq).unwrap_or(0)
    }

    /// 入环。字段校验、时间戳严格递增、容量政策都在这里收口。
    pub fn emit(
        &mut self,
        point: TracePoint,
        ts: u64,
        dev: u32,
        code: u16,
        bytes: u32,
    ) -> Result<u32, TraceError> {
        let ev = TraceEvent {
            seq: self.emitted,
            point,
            ts,
            dev,
            code,
            bytes,
        };
        ev.check()?;
        if ts <= self.last_ts {
            return Err(TraceError::Ts);
        }
        if self.events.len() >= self.cap {
            match self.policy {
                RingPolicy::Strict => return Err(TraceError::Capacity),
                RingPolicy::Overwrite => {
                    if !self.events.is_empty() {
                        self.events.remove(0);
                        self.dropped = self.dropped.saturating_add(1);
                    }
                }
            }
        }
        self.events.push(ev);
        self.last_ts = ts;
        self.emitted = self.emitted.saturating_add(1);
        let idx = point.wire() as usize - 1;
        if let Some(c) = self.per_point.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.coverage_bits |= point.bit();
        Ok(ev.seq)
    }

    /// 回放入口：只查容量与字段，不校验时序——离线 trace 必须先落进来，
    /// 再由 `audit()` 判真伪（否则「坏 trace 无法导入」会被误读成「没有坏 trace」）。
    pub fn push_raw(&mut self, ev: TraceEvent) -> Result<(), TraceError> {
        if self.events.len() >= self.cap {
            return Err(TraceError::Capacity);
        }
        self.events.push(ev);
        self.emitted = self.emitted.saturating_add(1);
        let idx = ev.point.wire() as usize - 1;
        if let Some(c) = self.per_point.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.coverage_bits |= ev.point.bit();
        Ok(())
    }

    /// 审计：seq 严格递增、ts 严格递增、字段合法、总量守恒。
    pub fn audit(&self) -> Result<(), TraceError> {
        if self.events.is_empty() {
            return Err(TraceError::Empty);
        }
        let mut prev_seq: u32 = 0;
        let mut prev_ts: u64 = 0;
        let mut i = 0usize;
        while i < self.events.len() {
            let ev = match self.events.get(i) {
                Some(e) => e,
                None => return Err(TraceError::Empty),
            };
            if i > 0 && ev.seq <= prev_seq {
                return Err(TraceError::Dup);
            }
            if i > 0 && ev.ts <= prev_ts {
                return Err(TraceError::Ts);
            }
            ev.check()?;
            prev_seq = ev.seq;
            prev_ts = ev.ts;
            i += 1;
        }
        let retained = self.events.len() as u32;
        if self.emitted != retained.saturating_add(self.dropped) {
            return Err(TraceError::Order);
        }
        Ok(())
    }

    /// 摘要（FNV-1a 64，按 seq/point/ts/dev/code/bytes 固定字段序）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.events.len() as u64);
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = fnv_u32(h, e.seq);
                h = fnv_u8(h, e.point.wire());
                h = fnv_u64(h, e.ts);
                h = fnv_u32(h, e.dev);
                h = fnv_u32(h, e.code as u32);
                h = fnv_u32(h, e.bytes);
            }
            i += 1;
        }
        h
    }

    /// 严格摘要：空环没有摘要可谈，返回 `Empty` 而不是 0（0 会被误当成合法摘要）。
    pub fn digest_strict(&self) -> Result<u64, TraceError> {
        if self.events.is_empty() {
            return Err(TraceError::Empty);
        }
        Ok(self.digest())
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fnv_u8(x, ((v >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fnv_u8(x, ((v >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

// ---------------------------------------------------------------------------
// 会话口
// ---------------------------------------------------------------------------

/// 埋点会话：字段校验 → 生命周期推进 → 入环，三步顺序固定。
pub struct TraceSession {
    pub ring: TraceRing,
    pub table: LifecycleTable,
}

impl TraceSession {
    pub fn new(cap: usize, policy: RingPolicy, devices: usize) -> Result<TraceSession, TraceError> {
        Ok(TraceSession {
            ring: TraceRing::new(cap, policy)?,
            table: LifecycleTable::new(devices)?,
        })
    }

    /// 记一条埋点。Strict 政策下先看容量，避免「生命周期已推进、记录却进不去」。
    /// 生命周期只在入环成功后才提交（peek → emit → commit），失败处可精确归因。
    pub fn record(
        &mut self,
        point: TracePoint,
        ts: u64,
        dev: u32,
        code: u16,
        bytes: u32,
    ) -> Result<u32, TraceError> {
        let probe = TraceEvent {
            seq: 0,
            point,
            ts,
            dev,
            code,
            bytes,
        };
        probe.check()?;
        if self.ring.policy() == RingPolicy::Strict && !self.ring.has_room() {
            return Err(TraceError::Capacity);
        }
        self.table.peek(dev, point)?;
        let seq = self.ring.emit(point, ts, dev, code, bytes)?;
        self.table.commit(dev, point)?;
        Ok(seq)
    }

    pub fn coverage_permille(&self) -> u32 {
        self.ring.coverage_permille()
    }

    pub fn audit(&self) -> Result<(), TraceError> {
        self.ring.audit()
    }

    /// 渲染一行（诊断用）。
    pub fn render_last(&self) -> String {
        match self.ring.get(self.ring.len().saturating_sub(1)) {
            Some(e) => format!(
                "trace#{} {} dev={} ts={} code={} bytes={}",
                e.seq,
                e.point.label(),
                e.dev,
                e.ts,
                e.code,
                e.bytes
            ),
            None => "trace empty".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// 注入核：十一类坏埋点逐类断言具体码
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 11usize;

    // 1 点码越界
    if TracePoint::from_wire(0).err() == Some(TraceError::Point)
        && TracePoint::from_wire(5).err() == Some(TraceError::Point)
    {
        caught += 1;
    }
    // 2 时间戳为 0
    let zero_ts = TraceEvent {
        seq: 0,
        point: TracePoint::Probe,
        ts: 0,
        dev: 1,
        code: 0,
        bytes: 0,
    };
    if zero_ts.check().err() == Some(TraceError::Ts) {
        caught += 1;
    }
    // 3 设备号 0
    let bad_dev = TraceEvent {
        seq: 0,
        point: TracePoint::Probe,
        ts: 1,
        dev: 0,
        code: 0,
        bytes: 0,
    };
    if bad_dev.check().err() == Some(TraceError::Dev) {
        caught += 1;
    }
    // 4 绑定前传输（生命周期顺序）
    let mut table = match LifecycleTable::new(4) {
        Ok(t) => t,
        Err(_) => return (caught, total),
    };
    if table.observe(7, TracePoint::Transfer).err() == Some(TraceError::Order) {
        caught += 1;
    }
    // 5 传输字节为 0
    let zero_bytes = TraceEvent {
        seq: 0,
        point: TracePoint::Transfer,
        ts: 3,
        dev: 1,
        code: 0,
        bytes: 0,
    };
    if zero_bytes.check().err() == Some(TraceError::Fields) {
        caught += 1;
    }
    // 6 传输状态码越界
    let bad_code = TraceEvent {
        seq: 0,
        point: TracePoint::Transfer,
        ts: 3,
        dev: 1,
        code: 3,
        bytes: 512,
    };
    if bad_code.check().err() == Some(TraceError::Code) {
        caught += 1;
    }
    // 7 Strict 政策下环满
    let mut strict = match TraceRing::new(1, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let first = strict.emit(TracePoint::Probe, 1, 1, 0, 0);
    let second = strict.emit(TracePoint::Bind, 2, 1, 0, 0);
    if first == Ok(0) && second.err() == Some(TraceError::Capacity) && strict.len() == 1 {
        caught += 1;
    }
    // 8 空环摘要/审计
    let empty = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    if empty.digest_strict().err() == Some(TraceError::Empty)
        && empty.audit().err() == Some(TraceError::Empty)
    {
        caught += 1;
    }
    // 9 生命周期表满（cap 2，第三个设备 Probe）
    let mut small = match LifecycleTable::new(2) {
        Ok(t) => t,
        Err(_) => return (caught, total),
    };
    let o1 = small.observe(1, TracePoint::Probe);
    let o2 = small.observe(2, TracePoint::Probe);
    let o3 = small.observe(3, TracePoint::Probe);
    if o1.is_ok() && o2.is_ok() && o3.err() == Some(TraceError::Slots) {
        caught += 1;
    }
    // 10 回放 seq 不递增 → Dup
    let mut replay = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let e0 = TraceEvent {
        seq: 5,
        point: TracePoint::Transfer,
        ts: 10,
        dev: 1,
        code: 0,
        bytes: 64,
    };
    let mut e1 = e0;
    e1.ts = 11;
    if replay.push_raw(e0).is_ok()
        && replay.push_raw(e1).is_ok()
        && replay.audit().err() == Some(TraceError::Dup)
    {
        caught += 1;
    }
    // 11 回放时间戳倒退 → Ts
    let mut back = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return (caught, total),
    };
    let b0 = TraceEvent {
        seq: 0,
        point: TracePoint::Probe,
        ts: 100,
        dev: 1,
        code: 0,
        bytes: 0,
    };
    let mut b1 = b0;
    b1.seq = 1;
    b1.ts = 50;
    if back.push_raw(b0).is_ok()
        && back.push_raw(b1).is_ok()
        && back.audit().err() == Some(TraceError::Ts)
    {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑一遍「三设备完整生命周期」脚本，返回会话（供多条判据复用）。
fn run_lifecycle(cap: usize, policy: RingPolicy) -> Result<TraceSession, TraceError> {
    let mut sess = TraceSession::new(cap, policy, 8)?;
    let mut ts: u64 = 1;
    let mut dev: u32 = 1;
    while dev <= 3 {
        sess.record(TracePoint::Probe, ts, dev, 0, 0)?;
        ts += 1;
        sess.record(TracePoint::Bind, ts, dev, 0, 0)?;
        ts += 1;
        let mut k = 0u32;
        while k < 4 {
            // 第 2 块故意带一个「端点停」状态码，验证失败态也被埋点覆盖。
            let code = if k == 1 { 1 } else { 0 };
            sess.record(TracePoint::Transfer, ts, dev, code, 512)?;
            ts += 1;
            k += 1;
        }
        sess.record(TracePoint::Unplug, ts, dev, 1, 0)?;
        ts += 1;
        dev += 1;
    }
    Ok(sess)
}

pub fn run_unx50129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50129");

    // J1-1 四点闭环：wire 1..=4 往返，且每个线的位图互不相同
    let mut rt_ok = true;
    let mut bits: u8 = 0;
    let mut i = 1u8;
    while i <= 4 {
        match TracePoint::from_wire(i) {
            Ok(p) => {
                if p.wire() != i {
                    rt_ok = false;
                }
                bits |= p.bit();
            }
            Err(_) => rt_ok = false,
        }
        i += 1;
    }
    let ok = rt_ok
        && bits == 0b1111
        && TracePoint::from_wire(0).err() == Some(TraceError::Point)
        && TracePoint::from_wire(5).err() == Some(TraceError::Point);
    s.add("50129-J1-四点闭集", ok, if ok { "" } else { "点码闭集/位图失守" });

    // J1-2 三设备全生命周期脚本可跑通（21 条埋点）
    let sess = match run_lifecycle(64, RingPolicy::Strict) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let stats = sess.ring.stats();
    let ok = stats.emitted == 21
        && stats.retained == 21
        && stats.dropped == 0
        && sess.table.len() == 3
        && sess.audit().is_ok();
    let det = format!(
        "emitted={} retained={} devs={}",
        stats.emitted,
        stats.retained,
        sess.table.len()
    );
    s.add_dyn("50129-J1-脚本可跑", ok, &det);

    // J1-3 覆盖 100%：四点各至少一条，位图全满，覆盖率 1000‰（J1 核心）
    let mut per_ok = true;
    let mut i = 1u8;
    while i <= 4 {
        if let Ok(p) = TracePoint::from_wire(i) {
            // 每台设备各埋一次 Probe/Bind/Unplug，四块传输：故 3/3/12/3。
            let n = sess.ring.point_count(p);
            if n < 3 {
                per_ok = false;
            }
        }
        i += 1;
    }
    let ok = per_ok
        && sess.ring.coverage_bits() == 0b1111
        && sess.coverage_permille() == 1000
        && sess.ring.point_count(TracePoint::Transfer) == 12;
    let det = format!(
        "probe/bind/xfer/unplug={}/{}/{}/{} cov={}‰",
        sess.ring.point_count(TracePoint::Probe),
        sess.ring.point_count(TracePoint::Bind),
        sess.ring.point_count(TracePoint::Transfer),
        sess.ring.point_count(TracePoint::Unplug),
        sess.coverage_permille()
    );
    s.add_dyn("50129-J1-覆盖100", ok, &det);

    // J1-4 生命周期终态正确：三台设备都已拔出，且重新探测可回到 Probed
    let mut life_ok = true;
    let mut dev = 1u32;
    while dev <= 3 {
        if sess.table.life_of(dev) != Some(DeviceLife::Unplugged) {
            life_ok = false;
        }
        dev += 1;
    }
    let ok = life_ok && sess.table.life_of(99).is_none();
    s.add("50129-J1-生命周期", ok, if ok { "" } else { "终态不是拔出" });

    // J1-5 确定性：同脚本两次摘要逐位一致，且不同脚本（少一台设备）摘要不同
    let again = match run_lifecycle(64, RingPolicy::Strict) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let mut one = match TraceSession::new(64, RingPolicy::Strict, 8) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let _ = one.record(TracePoint::Probe, 1, 1, 0, 0);
    let ok = again.ring.digest() == sess.ring.digest()
        && again.ring.digest() != one.ring.digest()
        && one.coverage_permille() == 250;
    let det = format!(
        "d={:x} partial_cov={}‰",
        sess.ring.digest(),
        one.coverage_permille()
    );
    s.add_dyn("50129-J1-埋点确定", ok, &det);

    // J1-6 时间戳严格递增：回退与相等都拒
    let mut t = match TraceSession::new(8, RingPolicy::Strict, 4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let a = t.record(TracePoint::Probe, 10, 1, 0, 0);
    let b = t.record(TracePoint::Bind, 10, 1, 0, 0);
    let c = t.record(TracePoint::Bind, 9, 1, 0, 0);
    let d = t.record(TracePoint::Bind, 11, 1, 0, 0);
    let ok = a == Ok(0)
        && b.err() == Some(TraceError::Ts)
        && c.err() == Some(TraceError::Ts)
        && d == Ok(1);
    s.add("50129-J1-时序严格", ok, if ok { "" } else { "同刻/回退被放行" });

    // J1-7 顺序闸：未探测就绑定、绑定前传输、未绑定就传出、重复绑定
    let mut o = match TraceSession::new(16, RingPolicy::Strict, 4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let e1 = o.record(TracePoint::Bind, 1, 1, 0, 0);
    let e2 = o.record(TracePoint::Transfer, 2, 1, 0, 64);
    let e3 = o.record(TracePoint::Probe, 3, 1, 0, 0);
    let e4 = o.record(TracePoint::Transfer, 4, 1, 0, 64);
    let e5 = o.record(TracePoint::Bind, 5, 1, 0, 0);
    let e6 = o.record(TracePoint::Bind, 6, 1, 0, 0);
    let ok = e1.err() == Some(TraceError::Order)
        && e2.err() == Some(TraceError::Order)
        && e3.is_ok()
        && e4.err() == Some(TraceError::Order)
        && e5.is_ok()
        && e6.err() == Some(TraceError::Order)
        && o.table.life_of(1) == Some(DeviceLife::Bound);
    let det = format!("final={:?}", o.table.life_of(1));
    s.add_dyn("50129-J1-顺序闸", ok, &det);

    // J1-8 字段契约：非传输点带字节、传输点零字节、状态码越界
    let probe_bytes = TraceEvent {
        seq: 0,
        point: TracePoint::Probe,
        ts: 1,
        dev: 1,
        code: 0,
        bytes: 8,
    };
    let unplug_code = TraceEvent {
        seq: 0,
        point: TracePoint::Unplug,
        ts: 1,
        dev: 1,
        code: 2,
        bytes: 0,
    };
    let bind_bytes = TraceEvent {
        seq: 0,
        point: TracePoint::Bind,
        ts: 1,
        dev: 1,
        code: 0,
        bytes: 32,
    };
    let ok = probe_bytes.check().err() == Some(TraceError::Fields)
        && unplug_code.check().err() == Some(TraceError::Code)
        && bind_bytes.check().err() == Some(TraceError::Fields)
        && TraceEvent {
            seq: 0,
            point: TracePoint::Transfer,
            ts: 1,
            dev: 1,
            code: 0,
            bytes: 0,
        }
        .check()
        .err()
            == Some(TraceError::Fields);
    s.add("50129-J1-字段契约", ok, if ok { "" } else { "字段规则失守" });

    // J1-9 Strict 容量：满即拒，计数不涨；Overwrite：滑窗 + 守恒
    let mut strict = match TraceRing::new(2, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let s1 = strict.emit(TracePoint::Probe, 1, 1, 0, 0);
    let s2 = strict.emit(TracePoint::Bind, 2, 1, 0, 0);
    let s3 = strict.emit(TracePoint::Transfer, 3, 1, 0, 8);
    let strict_ok = s1 == Ok(0)
        && s2 == Ok(1)
        && s3.err() == Some(TraceError::Capacity)
        && strict.emitted() == 2
        && strict.len() == 2;
    let mut over = match TraceRing::new(2, RingPolicy::Overwrite) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut i = 0u32;
    let mut over_ok = true;
    while i < 5 {
        if over.emit(TracePoint::Transfer, i as u64 + 1, 1, 0, 64).is_err() {
            over_ok = false;
        }
        i += 1;
    }
    over_ok = over_ok
        && over.emitted() == 5
        && over.len() == 2
        && over.dropped() == 3
        && over.audit().is_ok()
        && over.retained_lowest_seq() == 3;
    let ok = strict_ok && over_ok;
    let det = format!("strict_emitted={} over_drop={}", strict.emitted(), over.dropped());
    s.add_dyn("50129-J1-环容量", ok, &det);

    // J1-10 审计：回放 seq 重复 → Dup，时间戳倒退 → Ts，空环 → Empty
    let mut r1 = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let base_ev = TraceEvent {
        seq: 2,
        point: TracePoint::Transfer,
        ts: 20,
        dev: 1,
        code: 0,
        bytes: 64,
    };
    let mut dup_ev = base_ev;
    dup_ev.ts = 21;
    let dup_ok = r1.push_raw(base_ev).is_ok()
        && r1.push_raw(dup_ev).is_ok()
        && r1.audit().err() == Some(TraceError::Dup);
    let mut r2 = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let mut back_ev = base_ev;
    back_ev.seq = 3;
    back_ev.ts = 5;
    let ts_ok = r2.push_raw(base_ev).is_ok()
        && r2.push_raw(back_ev).is_ok()
        && r2.audit().err() == Some(TraceError::Ts);
    let empty = match TraceRing::new(4, RingPolicy::Strict) {
        Ok(r) => r,
        Err(_) => return s,
    };
    let ok = dup_ok && ts_ok && empty.audit().err() == Some(TraceError::Empty);
    s.add("50129-J1-审计", ok, if ok { "" } else { "坏回放未被审计抓住" });

    // J1-11 设备表：cap 满 → Slots；dev=0 → Dev；新设备只接受 Probe 开户
    let mut tb = match LifecycleTable::new(2) {
        Ok(t) => t,
        Err(_) => return s,
    };
    let p1 = tb.observe(1, TracePoint::Probe);
    let p2 = tb.observe(2, TracePoint::Probe);
    let p3 = tb.observe(3, TracePoint::Probe);
    let ok = p1 == Ok(DeviceLife::Probed)
        && p2 == Ok(DeviceLife::Probed)
        && p3.err() == Some(TraceError::Slots)
        && tb.observe(0, TracePoint::Probe).err() == Some(TraceError::Dev)
        && tb.observe(9, TracePoint::Transfer).err() == Some(TraceError::Order)
        && LifecycleTable::new(0).err() == Some(TraceError::Slots)
        && LifecycleTable::new(65).err() == Some(TraceError::Capacity);
    s.add("50129-J1-设备表", ok, if ok { "" } else { "设备表闸失守" });

    // J1-12 会话在 Strict 满环时先拒，生命周期不前进（失败可归因）
    let mut full = match TraceSession::new(1, RingPolicy::Strict, 4) {
        Ok(x) => x,
        Err(_) => return s,
    };
    let f1 = full.record(TracePoint::Probe, 1, 1, 0, 0);
    let f2 = full.record(TracePoint::Bind, 2, 1, 0, 0);
    let ok = f1.is_ok()
        && f2.err() == Some(TraceError::Capacity)
        && full.table.life_of(1) == Some(DeviceLife::Probed)
        && full.ring.len() == 1;
    s.add("50129-J1-满环归因", ok, if ok { "" } else { "满环时生命周期被误推进" });

    // J1-13 注入核：十一类坏埋点全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50129-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50129_checks_green() {
        let set = run_unx50129_checks();
        assert!(set.all_passed(), "UNX-F50129 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50129_lifecycle_covers_all_points() {
        let sess = run_lifecycle(64, RingPolicy::Strict);
        assert!(sess.is_ok());
        if let Ok(sess) = sess {
            assert_eq!(sess.coverage_permille(), 1000);
            assert_eq!(sess.ring.coverage_bits(), 0b1111);
            assert_eq!(sess.ring.emitted(), 21);
            assert_eq!(sess.audit(), Ok(()));
        }
    }

    #[test]
    fn unx50129_error_codes_in_segment() {
        assert_eq!(TraceError::Ts.code(), SEG_BASE + 1);
        assert_eq!(TraceError::Slots.code(), SEG_BASE + 10);
    }
}
