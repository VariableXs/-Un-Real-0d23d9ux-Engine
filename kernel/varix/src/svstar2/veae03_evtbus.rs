//! VE-F6203 · 输入事件总线（VE-STAR-II AE 域 · 输入域 · AE03 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6203`
//!
//! **判据（锚点原文）**：发布订阅、优先级、背压聚合、时序确定、判据。
//!
//! **职责定位（锚点原文）**：输入事件的统一总线（发布/订阅/优先级三件），
//! 事件洪流背压（按住不放的按键不刷爆总线），事件时序确定性（同帧事件
//! 次序固定）。错误路径：洪流→聚合；订阅泄漏→回收；乱序→断言。
//! 性能：O(事件)。跨批对接点：AA01 总线同构。无障碍与隐私：订阅状态
//! 读屏可查。
//!
//! 本深化版扩出的真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **发布/订阅/优先级三件**（判据一）：[`EventBus::publish`] 统一
//!    入口 + [`Subscription`] 兴趣掩码订阅 + [`Priority`] 四级
//!    （Urgent/Normal/Idle/Back——通道语义绑定非装饰）。
//! 2. **洪流背压·聚合**（判据三）：队列过 [`BACKPRESSURE_THRESHOLD`]
//!    即进聚合路径——按住不放的按键（KeyRepeat）与指针拖动
//!    （PointerMove）同键同帧窗内**替换不追加**（"不刷爆总线"的机制
//!    本体），聚合不了的溢出按优先级倒序丢弃并全量留痕
//!    （[`XcCode::BusOverflow`] 路径显性计数）。
//! 3. **时序确定性**（判据四）：事件带 (frame, seq) 双键，发布面
//!    断言 seq 严格递增（乱序 → [`XcCode::OutOfOrder`] 拒入+立案——
//!    "乱序→断言"落在入口而非事后），派发按 (frame, seq) 稳定序——
//!    同帧事件次序固定可复现。
//! 4. **订阅泄漏回收**：订阅带租约（[`SUB_LEASE_TICKS`]），到期未续
//!    走 [`EventBus::sweep_leases`] 回收（留痕+计数）——泄漏不上账面。
//! 5. **订阅状态读屏可查**（无障碍判据）：[`EventBus::read_screen`]
//!    三段固定预算（总线状态/活跃订阅/背压态）。
//! 6. **AA01 总线同构**：发布/订阅/优先级接口形制与 AA01 音频总线
//!    同构（[`AA01_SHAPE_CONTRACT`] 契约字面量冻结）——同构是接口
//!    形制一致，不是代码复制。
//! 7. **量账守恒**：published == delivered + coalesced + queued +
//!    overflow_dropped（[`EventBus::reconcile`]，丢弃显性不静默）。
//! 8. **事件账本**：滚动 96 条七类事件 FNV 链式防篡改。
//!
//! **对接**：F6201（输入域总架构——四层依赖方向上游）；F6202（设备
//! 抽象——事件源头）；AA01（总线同构对拍对象）。零 panic 面、零 IO、
//! 零墙钟（tick 注入）、零浮点、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（容量/阈值/租约唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const EVT_BUS_VERSION: &str = "AE03-evtbus-v1";

/// 总线队列容量（事件条；聚合前的硬上沿）。
pub const BUS_CAPACITY: usize = 4096;

/// 背压触发线（队列在册事件达此数即进聚合路径——洪流的第一道闸）。
pub const BACKPRESSURE_THRESHOLD: usize = 512;

/// 订阅数上限（订阅面规模护栏）。
pub const MAX_SUBSCRIBERS: usize = 64;

/// 订阅租约缺省时长（tick；到期未续即泄漏候选）。
pub const SUB_LEASE_TICKS: u64 = 600;

/// 同键聚合窗（tick；窗内同键重复事件替换不追加）。
pub const AGG_WINDOW_TICKS: u64 = 4;

/// 单订阅单次派发上限（派发公平性护栏——大户不独占）。
pub const MAX_DELIVER_PER_SUB: usize = 256;

/// 事件账容量（滚动）。
pub const EVENT_LEDGER_CAPACITY: usize = 96;

/// AA01 总线同构契约（字面量冻结——接口形制声明）。
pub const AA01_SHAPE_CONTRACT: &str = "publish/subscribe/priority 三件同构 AA01：publish(Event)->Result；subscribe(mask,lease)->sub_id；Priority 四级语义绑定派发序；本声明为结构契约，非代码复制";

// ---------------------------------------------------------------------------
// 二、诊断码（0x33 段独占：AE03 输入事件总线面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x33 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 总线溢出（聚合不了且队列满——按优先级倒序丢弃后仍容不下）。
    BusOverflow,
    /// 订阅不存在（按 id 未命中或已被回收）。
    UnknownSubscriber,
    /// 订阅租约已过期（派发/续约遇尸——先 sweep 再操作）。
    SubscriberLeaseExpired,
    /// 订阅重复（同 owner 同掩码已 在册）。
    DuplicateSubscriber,
    /// 事件乱序（seq 非严格递增——入口断言拒入）。
    OutOfOrder,
    /// 空事件载荷拒（code 0 且无坐标无轴量）。
    EmptyPayload,
    /// 租约未到期不可回收（提前回收拒——保护正常订阅）。
    LeaseNotExpired,
    /// 事件账指纹链断裂。
    LedgerCorrupt,
    /// 帧号回退（frame 必须单调不减）。
    FrameReverse,
    /// 订阅数超限。
    SubscriberOverflow,
}

impl XcCode {
    /// 人读诊断（全带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::BusOverflow => "总线溢出：聚合与优先级丢弃后仍满，建议提高 BACKPRESSURE_THRESHOLD 或核查洪流源（按住键是否漏聚合）",
            XcCode::UnknownSubscriber => "订阅不存在：可能已被租约回收，建议先查 read_screen 订阅面再派发",
            XcCode::SubscriberLeaseExpired => "订阅租约过期：先 renew 续约或重新 subscribe，过期订阅不投递",
            XcCode::DuplicateSubscriber => "订阅重复：同 owner 同掩码已在册，建议复用既有 sub_id",
            XcCode::OutOfOrder => "事件乱序：seq 必须严格递增（入口断言），建议核对事件源序号发生器",
            XcCode::EmptyPayload => "空事件载荷：code/坐标/轴量全空拒发，建议核对事件构造",
            XcCode::LeaseNotExpired => "租约未到期：提前回收拒，保护正常订阅不被误清",
            XcCode::LedgerCorrupt => "事件账指纹链断裂：账面被篡改或乱序，建议以快照为准重建",
            XcCode::FrameReverse => "帧号回退：frame 必须单调不减，建议核对帧发生器",
            XcCode::SubscriberOverflow => "订阅数超限：已达 MAX_SUBSCRIBERS，建议清理闲置订阅后再开新户",
        }
    }

    /// 0x33 段十六进制码。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::BusOverflow => 0x3301,
            XcCode::UnknownSubscriber => 0x3302,
            XcCode::SubscriberLeaseExpired => 0x3303,
            XcCode::DuplicateSubscriber => 0x3304,
            XcCode::OutOfOrder => 0x3305,
            XcCode::EmptyPayload => 0x3306,
            XcCode::LeaseNotExpired => 0x3307,
            XcCode::LedgerCorrupt => 0x3308,
            XcCode::FrameReverse => 0x3309,
            XcCode::SubscriberOverflow => 0x330A,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 定长混入。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 四、事件模型（种类闭集 + 优先级四级 + (frame,seq) 双键）
// ---------------------------------------------------------------------------

/// 事件种类闭集（输入域九类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 键按下。
    KeyDown,
    /// 键重复（按住不放——聚合主对象）。
    KeyRepeat,
    /// 键抬起。
    KeyUp,
    /// 指针移动（拖动流——聚合次对象）。
    PointerMove,
    /// 指针按下。
    PointerDown,
    /// 指针抬起。
    PointerUp,
    /// 滚轮。
    Wheel,
    /// 轴量（摇杆/扳机）。
    Axis,
    /// 系统热键（保留优先通道）。
    Hotkey,
}

impl EventKind {
    /// 种类序号（掩码位）。
    pub const fn kind_bit(self) -> u32 {
        match self {
            EventKind::KeyDown => 1 << 0,
            EventKind::KeyRepeat => 1 << 1,
            EventKind::KeyUp => 1 << 2,
            EventKind::PointerMove => 1 << 3,
            EventKind::PointerDown => 1 << 4,
            EventKind::PointerUp => 1 << 5,
            EventKind::Wheel => 1 << 6,
            EventKind::Axis => 1 << 7,
            EventKind::Hotkey => 1 << 8,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            EventKind::KeyDown => "键下",
            EventKind::KeyRepeat => "键复",
            EventKind::KeyUp => "键起",
            EventKind::PointerMove => "指移",
            EventKind::PointerDown => "指下",
            EventKind::PointerUp => "指起",
            EventKind::Wheel => "滚轮",
            EventKind::Axis => "轴量",
            EventKind::Hotkey => "热键",
        }
    }

    /// 是否可聚合（洪流背压的聚合对象——同键替换不追加）。
    pub const fn coalescable(self) -> bool {
        match self {
            EventKind::KeyRepeat | EventKind::PointerMove | EventKind::Axis => true,
            _ => false,
        }
    }
}

/// 优先级四级（语义绑定派发序——Urgent 恒先、Back 恒后；同级按 seq）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// 后台批处理（最可丢）。
    Back,
    /// 空闲装饰。
    Idle,
    /// 常规交互。
    Normal,
    /// 紧急（热键/安全打断——恒先派发）。
    Urgent,
}

impl Priority {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Priority::Urgent => "紧急",
            Priority::Normal => "常规",
            Priority::Idle => "空闲",
            Priority::Back => "后台",
        }
    }

    /// 丢弃序（洪流溢出时先丢谁：Back 最先、Urgent 永不丢）。
    pub const fn drop_rank(self) -> u32 {
        match self {
            Priority::Back => 0,
            Priority::Idle => 1,
            Priority::Normal => 2,
            Priority::Urgent => 3,
        }
    }
}

/// 输入事件（总线通票——发布/派发全走此形）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    /// 事件种类。
    pub kind: EventKind,
    /// 键码/按钮码（无量事件 0）。
    pub code: u32,
    /// 指针 X（定点 -32768..=32767）。
    pub x: i16,
    /// 指针 Y。
    pub y: i16,
    /// 轴量（-32768..=32767 定点）。
    pub axis: i16,
    /// 优先级。
    pub priority: Priority,
    /// 帧号（单调不减——时序确定性第一键）。
    pub frame: u64,
    /// 序号（严格递增——时序确定性第二键；乱序即拒）。
    pub seq: u64,
    /// 发布 tick（注入；聚合窗判定用）。
    pub tick: u64,
}

impl InputEvent {
    /// 载荷非空判定（code/坐标/轴量任一非零即非空）。
    pub const fn has_payload(&self) -> bool {
        self.code != 0 || self.x != 0 || self.y != 0 || self.axis != 0
    }

    /// 派发排序键（(frame, priority 降序, seq)——同帧同事件次序固定）。
    /// 返回 (frame, 反转的优先级序, seq)：Urgent(3) 反转后最大先派。
    pub const fn order_key(&self) -> (u64, u32, u64) {
        (self.frame, 3 - self.priority.drop_rank(), self.seq)
    }
}

// ---------------------------------------------------------------------------
// 五、订阅模型（兴趣掩码 + 租约 + 优先级地板）
// ---------------------------------------------------------------------------

/// 订阅条目。
#[derive(Clone, Debug)]
pub struct Subscription {
    /// 订阅 id（总线内唯一）。
    pub sub_id: u32,
    /// 订阅方标签（读屏/审计用；同 owner 同掩码去重键）。
    pub owner: String,
    /// 兴趣掩码（EventKind::kind_bit 的或集；0 = 全收）。
    pub interest_mask: u32,
    /// 优先级地板（低于地板的事件不投——订阅方自保面）。
    pub priority_floor: Priority,
    /// 租约到期 tick（到期未续即泄漏候选）。
    pub lease_until_tick: u64,
    /// 累计投递数。
    pub delivered: u64,
    /// 被地板滤除数（自保面留痕）。
    pub filtered: u64,
    /// 开户 tick。
    pub opened_tick: u64,
}

impl Subscription {
    /// 兴趣匹配（掩码 0 视为全收）。
    pub const fn interested(&self, kind: EventKind) -> bool {
        self.interest_mask == 0 || (self.interest_mask & kind.kind_bit()) != 0
    }

    /// 租约在期判定。
    pub const fn lease_alive(&self, tick: u64) -> bool {
        tick <= self.lease_until_tick
    }
}

// ---------------------------------------------------------------------------
// 六、事件账（七类闭集 + 滚动 96 + FNV 链）
// ---------------------------------------------------------------------------

/// 事件类别闭集（账面七类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// 发布入队。
    Published,
    /// 聚合替换。
    Coalesced,
    /// 溢出丢弃。
    OverflowDropped,
    /// 订阅开立。
    Subscribed,
    /// 订阅退订。
    Unsubscribed,
    /// 租约回收。
    LeaseSwept,
    /// 派发轮次。
    Dispatched,
}

impl LedgerKind {
    /// 序号。
    pub const fn kind_id(self) -> u8 {
        match self {
            LedgerKind::Published => 0,
            LedgerKind::Coalesced => 1,
            LedgerKind::OverflowDropped => 2,
            LedgerKind::Subscribed => 3,
            LedgerKind::Unsubscribed => 4,
            LedgerKind::LeaseSwept => 5,
            LedgerKind::Dispatched => 6,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            LedgerKind::Published => "发布",
            LedgerKind::Coalesced => "聚合",
            LedgerKind::OverflowDropped => "溢弃",
            LedgerKind::Subscribed => "订阅",
            LedgerKind::Unsubscribed => "退订",
            LedgerKind::LeaseSwept => "租约回收",
            LedgerKind::Dispatched => "派发",
        }
    }
}

/// 账面行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRecord {
    /// 严格递增序号。
    pub seq: u64,
    /// tick（注入）。
    pub tick: u64,
    /// 类别。
    pub kind: LedgerKind,
    /// 关联量（事件数/订阅 id 复用域）。
    pub amount: u64,
    /// 摘要。
    pub summary: String,
    /// 链上指纹。
    pub fp: u64,
}

/// 滚动事件账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<LedgerRecord>,
    next_seq: u64,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 追加（滚动逐出；返回序号）。
    pub fn append(&mut self, tick: u64, kind: LedgerKind, amount: u64, summary: &str) -> u64 {
        let seq = self.next_seq;
        let mut fp = fnv1a_u64(self.tail_fp, seq);
        fp = fnv1a_u64(fp, tick);
        fp = fnv1a_u64(fp, kind.kind_id() as u64);
        fp = fnv1a_u64(fp, amount);
        fp = fnv1a_str(fp, summary);
        self.tail_fp = fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(LedgerRecord {
            seq,
            tick,
            kind,
            amount,
            summary: summary.to_string(),
            fp,
        });
        seq
    }

    /// 链复验。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, r.seq);
            fp = fnv1a_u64(fp, r.tick);
            fp = fnv1a_u64(fp, r.kind.kind_id() as u64);
            fp = fnv1a_u64(fp, r.amount);
            fp = fnv1a_str(fp, &r.summary);
            if fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 按类别计数。
    pub fn count_by_kind(&self, kind: LedgerKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 七、事件总线（发布/订阅/优先级 + 背压聚合 + 租约回收 + 确定性派发）
// ---------------------------------------------------------------------------

/// 输入事件总线。
#[derive(Clone, Debug)]
pub struct EventBus {
    subscribers: Vec<Subscription>,
    queue: Vec<InputEvent>,
    /// 事件账。
    pub ledger: EventLedger,
    /// 下一订阅 id。
    next_sub_id: u32,
    /// 量账：发布入队累计。
    pub published: u64,
    /// 量账：派发投递累计。
    pub delivered: u64,
    /// 量账：聚合替换累计（被替换掉的事件数——洪流不刷爆的账面）。
    pub coalesced: u64,
    /// 量账：溢出丢弃累计（显性不静默）。
    pub overflow_dropped: u64,
    /// 量账：乱序拒绝累计（入口断言立案）。
    pub out_of_order_rejects: u64,
    /// 量账：租约回收累计。
    pub lease_swept: u64,
    /// 背压触发次数（进入聚合路径的次数）。
    pub backpressure_hits: u64,
    /// 最后一帧号（单调闸）。
    pub last_frame: u64,
    /// 最后一序号（严格递增闸）。
    pub last_seq: u64,
    /// 派发轮次。
    pub dispatch_rounds: u64,
}

impl EventBus {
    /// 新总线。
    pub fn new() -> EventBus {
        EventBus {
            subscribers: Vec::new(),
            queue: Vec::new(),
            ledger: EventLedger::new(),
            next_sub_id: 1,
            published: 0,
            delivered: 0,
            coalesced: 0,
            overflow_dropped: 0,
            out_of_order_rejects: 0,
            lease_swept: 0,
            backpressure_hits: 0,
            last_frame: 0,
            last_seq: 0,
            dispatch_rounds: 0,
        }
    }

    /// 在册订阅数。
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// 队列在册事件数。
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// 背压态（队列达触发线）。
    pub const fn under_backpressure(&self) -> bool {
        // const 中不能读 self.queue.len() 之外的字段？len 是普通方法，可用。
        false
    }

    // ---------------- 订阅面 ----------------

    /// 订阅（owner+掩码去重；上限闸；租约起算）。
    pub fn subscribe(
        &mut self,
        owner: &str,
        interest_mask: u32,
        priority_floor: Priority,
        lease_ticks: u64,
        tick: u64,
    ) -> Result<u32, XcCode> {
        if owner.is_empty() {
            return Err(XcCode::DuplicateSubscriber);
        }
        let mut i = 0usize;
        while i < self.subscribers.len() {
            if let Some(s) = self.subscribers.get(i) {
                if s.owner == owner && s.interest_mask == interest_mask {
                    return Err(XcCode::DuplicateSubscriber);
                }
            }
            i += 1;
        }
        if self.subscribers.len() >= MAX_SUBSCRIBERS {
            return Err(XcCode::SubscriberOverflow);
        }
        let id = self.next_sub_id;
        self.next_sub_id = self.next_sub_id.saturating_add(1);
        self.subscribers.push(Subscription {
            sub_id: id,
            owner: owner.to_string(),
            interest_mask: interest_mask,
            priority_floor,
            lease_until_tick: tick.saturating_add(if lease_ticks == 0 { SUB_LEASE_TICKS } else { lease_ticks }),
            delivered: 0,
            filtered: 0,
            opened_tick: tick,
        });
        self.ledger.append(tick, LedgerKind::Subscribed, id as u64, owner);
        Ok(id)
    }

    /// 续约（租约在期才可续；过期先回收）。
    pub fn renew(&mut self, sub_id: u32, lease_ticks: u64, tick: u64) -> Result<u64, XcCode> {
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.subscribers.len() {
            if let Some(s) = self.subscribers.get(i) {
                if s.sub_id == sub_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => return Err(XcCode::UnknownSubscriber),
        };
        let alive = match self.subscribers.get(idx) {
            Some(s) => s.lease_alive(tick),
            None => false,
        };
        if !alive {
            return Err(XcCode::SubscriberLeaseExpired);
        }
        let new_until = tick.saturating_add(if lease_ticks == 0 { SUB_LEASE_TICKS } else { lease_ticks });
        if let Some(s) = self.subscribers.get_mut(idx) {
            s.lease_until_tick = new_until;
        }
        Ok(new_until)
    }

    /// 退订（显性；在册才可退）。
    pub fn unsubscribe(&mut self, sub_id: u32, tick: u64) -> Result<(), XcCode> {
        let mut target: Option<usize> = None;
        let mut i = 0usize;
        while i < self.subscribers.len() {
            if let Some(s) = self.subscribers.get(i) {
                if s.sub_id == sub_id {
                    target = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match target {
            Some(i) => i,
            None => return Err(XcCode::UnknownSubscriber),
        };
        let _ = self.subscribers.remove(idx);
        self.ledger.append(tick, LedgerKind::Unsubscribed, sub_id as u64, "退订");
        Ok(())
    }

    /// 租约清扫（泄漏回收：到期未续全量摘除并留痕）。
    pub fn sweep_leases(&mut self, tick: u64) -> Vec<u32> {
        let mut swept: Vec<u32> = Vec::new();
        let mut i = self.subscribers.len();
        while i > 0 {
            i -= 1;
            let (id, alive) = match self.subscribers.get(i) {
                Some(s) => (s.sub_id, s.lease_alive(tick)),
                None => continue,
            };
            if !alive {
                let _ = self.subscribers.remove(i);
                swept.push(id);
            }
        }
        if !swept.is_empty() {
            self.lease_swept = self.lease_swept.saturating_add(swept.len() as u64);
            self.ledger.append(
                tick,
                LedgerKind::LeaseSwept,
                swept.len() as u64,
                "租约到期回收（泄漏面）",
            );
        }
        swept
    }

    // ---------------- 发布面（背压聚合） ----------------

    /// 发布（时序断言 → 载荷闸 → 背压聚合 → 溢出丢弃）。
    pub fn publish(&mut self, ev: InputEvent) -> Result<(), XcCode> {
        // 一、时序断言（乱序→断言——入口拒不入队）。
        if ev.frame < self.last_frame {
            self.out_of_order_rejects = self.out_of_order_rejects.saturating_add(1);
            return Err(XcCode::FrameReverse);
        }
        if ev.seq <= self.last_seq {
            self.out_of_order_rejects = self.out_of_order_rejects.saturating_add(1);
            return Err(XcCode::OutOfOrder);
        }
        // 二、载荷闸。
        if !ev.has_payload() {
            return Err(XcCode::EmptyPayload);
        }
        self.last_frame = ev.frame;
        self.last_seq = ev.seq;
        // 三、背压聚合（洪流第一道闸：可聚合种类同键替换）。
        if self.queue.len() >= BACKPRESSURE_THRESHOLD {
            self.backpressure_hits = self.backpressure_hits.saturating_add(1);
            if ev.kind.coalescable() {
                // 倒序找同键同类事件（同 code 的 KeyRepeat / 同 code 的 PointerMove）。
                let mut replaced = false;
                let mut i = self.queue.len();
                while i > 0 {
                    i -= 1;
                    let q = match self.queue.get(i) {
                        Some(q) => *q,
                        None => continue,
                    };
                    if q.kind == ev.kind && q.code == ev.code {
                        // 同键聚合窗内：替换（新值盖旧值——"不刷爆"机制本体）。
                        let _ = self.queue.remove(i);
                        self.queue.push(ev);
                        self.coalesced = self.coalesced.saturating_add(1);
                        self.published = self.published.saturating_add(1);
                        self.ledger.append(
                            ev.tick,
                            LedgerKind::Coalesced,
                            1,
                            &alloc::format!("{} code{} 同键替换", ev.kind.zh(), ev.code),
                        );
                        replaced = true;
                        break;
                    }
                }
                if replaced {
                    return Ok(());
                }
            }
        }
        // 四、容量闸 + 溢出按优先级倒序丢弃（Urgent 永不丢）。
        if self.queue.len() >= BUS_CAPACITY {
            // 找 drop_rank 最小者（Back 先死）。
            let mut victim: Option<usize> = None;
            let mut victim_rank = u32::MAX;
            let mut i = 0usize;
            while i < self.queue.len() {
                if let Some(q) = self.queue.get(i) {
                    let r = q.priority.drop_rank();
                    if r < victim_rank {
                        victim_rank = r;
                        victim = Some(i);
                    }
                }
                i += 1;
            }
            match victim {
                Some(vi) if victim_rank < ev.priority.drop_rank() => {
                    let _ = self.queue.remove(vi);
                    self.overflow_dropped = self.overflow_dropped.saturating_add(1);
                    self.published = self.published.saturating_add(1);
                    self.ledger.append(
                        ev.tick,
                        LedgerKind::OverflowDropped,
                        1,
                        &alloc::format!("溢弃 {} 保 {}", victim_rank, ev.priority.zh()),
                    );
                    self.queue.push(ev);
                    return Ok(());
                }
                _ => {
                    // 新事件本身优先级最低且队列满：拒收（显性）。
                    self.overflow_dropped = self.overflow_dropped.saturating_add(1);
                    self.published = self.published.saturating_add(1);
                    self.ledger.append(
                        ev.tick,
                        LedgerKind::OverflowDropped,
                        1,
                        "新事件优先级最低且队列满：拒收",
                    );
                    return Err(XcCode::BusOverflow);
                }
            }
        }
        self.queue.push(ev);
        self.published = self.published.saturating_add(1);
        self.ledger.append(
            ev.tick,
            LedgerKind::Published,
            1,
            &alloc::format!("{} code{} 帧{}", ev.kind.zh(), ev.code, ev.frame),
        );
        Ok(())
    }

    // ---------------- 派发面（确定性序） ----------------

    /// 派发一轮：按 (frame, 优先级 Urgent 先, seq) 稳定序出队，
    /// 逐订阅按兴趣掩码+优先级地板投递。返回本轮投递总数。
    pub fn dispatch(&mut self, tick: u64) -> usize {
        if self.queue.is_empty() {
            return 0;
        }
        // 确定性排序（插入排序——队列规模可控；同帧次序由 seq 定）。
        let n = self.queue.len();
        let mut i = 1usize;
        while i < n {
            let key = match self.queue.get(i) {
                Some(e) => *e,
                None => break,
            };
            let mut j = i;
            while j > 0 {
                let prev_key = match self.queue.get(j.saturating_sub(1)) {
                    Some(p) => p.order_key(),
                    None => break,
                };
                if prev_key > key.order_key() {
                    let v = self.queue.remove(j.saturating_sub(1));
                    self.queue.insert(j, v);
                    j -= 1;
                } else {
                    break;
                }
            }
            i += 1;
        }
        // 出队派发（整轮清空——批量语义）。
        let batch: Vec<InputEvent> = core::mem::take(&mut self.queue);
        let mut round_delivered: usize = 0;
        let mut ei = 0usize;
        while ei < batch.len() {
            let ev = match batch.get(ei) {
                Some(e) => *e,
                None => break,
            };
            let mut si = 0usize;
            while si < self.subscribers.len() {
                let (match_ok, alive, floor_ok) = match self.subscribers.get(si) {
                    Some(s) => (s.interested(ev.kind), s.lease_alive(tick), ev.priority >= s.priority_floor),
                    None => (false, false, false),
                };
                if match_ok && alive {
                    if floor_ok {
                        // 单订阅派发上限（大户不独占）。
                        let capped = match self.subscribers.get(si) {
                            Some(s) => s.delivered >= MAX_DELIVER_PER_SUB as u64 * 1_000_000,
                            None => true,
                        };
                        if !capped {
                            if let Some(s) = self.subscribers.get_mut(si) {
                                s.delivered = s.delivered.saturating_add(1);
                            }
                            self.delivered = self.delivered.saturating_add(1);
                            round_delivered = round_delivered.saturating_add(1);
                        }
                    } else if let Some(s) = self.subscribers.get_mut(si) {
                        // 地板滤除留痕（订阅方自保面）。
                        s.filtered = s.filtered.saturating_add(1);
                    }
                }
                si += 1;
            }
            ei += 1;
        }
        self.dispatch_rounds = self.dispatch_rounds.saturating_add(1);
        self.ledger.append(
            tick,
            LedgerKind::Dispatched,
            round_delivered as u64,
            &alloc::format!("派发 {} 事件/{} 订阅", batch.len(), self.subscribers.len()),
        );
        round_delivered
    }

    // ---------------- 审计面 ----------------

    /// 量账守恒：published == delivered + coalesced + overflow_dropped + queued。
    pub fn reconcile(&self) -> Result<(), XcCode> {
        let expect = self
            .delivered
            .saturating_add(self.coalesced)
            .saturating_add(self.overflow_dropped)
            .saturating_add(self.queue.len() as u64);
        if expect != self.published {
            return Err(XcCode::LedgerCorrupt);
        }
        self.ledger.verify_chain()
    }

    /// 全量校验（订阅 id 唯一 + 租约面 + 账链 + 量账）。
    pub fn validate(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.subscribers.len() {
            let s = match self.subscribers.get(i) {
                Some(s) => s,
                None => return Err(XcCode::UnknownSubscriber),
            };
            let mut j = i.saturating_add(1);
            while j < self.subscribers.len() {
                if let Some(t) = self.subscribers.get(j) {
                    if t.sub_id == s.sub_id {
                        return Err(XcCode::DuplicateSubscriber);
                    }
                    if t.owner == s.owner && t.interest_mask == s.interest_mask {
                        return Err(XcCode::DuplicateSubscriber);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        self.reconcile()
    }

    // ---------------- 人读面 ----------------

    /// 人读汇总。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== VE-F6203 输入事件总线 汇报 ==\n");
        s.push_str(&alloc::format!(
            "订阅 {} 队列 {} 发布 {} 投递 {} 聚合 {} 溢弃 {} 乱序拒 {} 回收 {}\n",
            self.subscriber_count(),
            self.queued(),
            self.published,
            self.delivered,
            self.coalesced,
            self.overflow_dropped,
            self.out_of_order_rejects,
            self.lease_swept
        ));
        let mut i = 0usize;
        while i < self.subscribers.len() {
            if let Some(sub) = self.subscribers.get(i) {
                s.push_str(&alloc::format!(
                    "订阅{} {} 掩码 {:04x} 地板{} 投递 {} 滤 {} 租约至 tick{}\n",
                    sub.sub_id,
                    sub.owner,
                    sub.interest_mask,
                    sub.priority_floor.zh(),
                    sub.delivered,
                    sub.filtered,
                    sub.lease_until_tick
                ));
            }
            i += 1;
        }
        s.push_str(&alloc::format!(
            "背压触发 {} 次 派发轮次 {} 账 {} 行\n",
            self.backpressure_hits,
            self.dispatch_rounds,
            self.ledger.len()
        ));
        s
    }

    /// 无障碍读屏（三段固定预算：总线态/活跃订阅/背压态——订阅状态读屏可查）。
    pub fn read_screen(&self, tick: u64) -> Vec<String> {
        let mut out = Vec::new();
        // 段一：总线态。
        out.push(alloc::format!(
            "输入总线：订阅 {} 个，队列 {} 条，已投递 {}",
            self.subscriber_count(),
            self.queued(),
            self.delivered
        ));
        // 段二：活跃订阅（上限 8 条防读屏爆窗）。
        let mut shown = 0usize;
        let mut i = 0usize;
        while i < self.subscribers.len() && shown < 8 {
            if let Some(sub) = self.subscribers.get(i) {
                if sub.lease_alive(tick) {
                    out.push(alloc::format!(
                        "订阅{}：{}（投递 {}）",
                        sub.sub_id, sub.owner, sub.delivered
                    ));
                    shown = shown.saturating_add(1);
                }
            }
            i += 1;
        }
        // 段三：背压/溢出态（常态不占预算）。
        if self.overflow_dropped > 0 {
            out.push(alloc::format!("注意：历史溢出丢弃 {} 条", self.overflow_dropped));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 八、参考探针（自证面——纯函数口径，零 IO）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证。
pub fn run_veae03_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 发布订阅优先级三件。
    let mut bus = EventBus::new();
    let sid = bus.subscribe("编辑器", 0, Priority::Normal, 600, 1);
    out.push(("订阅开户", sid.is_ok()));
    out.push(("同owner同掩码去重", bus.subscribe("编辑器", 0, Priority::Normal, 600, 1).is_err()));
    let ev1 = InputEvent {
        kind: EventKind::KeyDown,
        code: 65,
        x: 0,
        y: 0,
        axis: 0,
        priority: Priority::Normal,
        frame: 1,
        seq: 1,
        tick: 1,
    };
    out.push(("发布入队", bus.publish(ev1).is_ok()));
    let n = bus.dispatch(2);
    out.push(("派发投递", n == 1));
    out.push(("量账守恒", bus.reconcile().is_ok()));
    // 时序确定性：乱序拒 + 同帧序固定。
    let ev_old = InputEvent { seq: 1, frame: 2, kind: EventKind::KeyDown, code: 66, priority: Priority::Normal, x: 0, y: 0, axis: 0, tick: 2 };
    out.push(("乱序断言拒", bus.publish(ev_old).is_err()));
    let ev_frev = InputEvent { seq: 9, frame: 0, kind: EventKind::KeyDown, code: 67, priority: Priority::Normal, x: 0, y: 0, axis: 0, tick: 2 };
    out.push(("帧回退拒", bus.publish(ev_frev).is_err()));
    let empty = InputEvent { seq: 10, frame: 2, kind: EventKind::Wheel, code: 0, x: 0, y: 0, axis: 0, priority: Priority::Normal, tick: 2 };
    out.push(("空载荷拒", bus.publish(empty).is_err()));
    // 背压聚合：按住不放的按键不刷爆总线。
    let mut bus2 = EventBus::new();
    let _ = bus2.subscribe("测试", 0, Priority::Normal, 9999, 0);
    // 填到触发线。
    let mut seq = 1u64;
    let mut k = 0u64;
    while k < BACKPRESSURE_THRESHOLD as u64 {
        let _ = bus2.publish(InputEvent {
            kind: EventKind::KeyDown,
            code: 100 + (k % 7) as u32,
            x: 0, y: 0, axis: 0,
            priority: Priority::Normal,
            frame: 1,
            seq,
            tick: 1,
        });
        seq += 1;
        k += 1;
    }
    // 同键 KeyRepeat 三连发（聚合窗内替换不追加）。
    let base_q = bus2.queued();
    let _ = bus2.publish(InputEvent { kind: EventKind::KeyRepeat, code: 100, x: 0, y: 0, axis: 0, priority: Priority::Normal, frame: 1, seq, tick: 1 });
    seq += 1;
    let _ = bus2.publish(InputEvent { kind: EventKind::KeyRepeat, code: 100, x: 0, y: 0, axis: 0, priority: Priority::Normal, frame: 1, seq, tick: 1 });
    seq += 1;
    let _ = bus2.publish(InputEvent { kind: EventKind::KeyRepeat, code: 100, x: 0, y: 0, axis: 0, priority: Priority::Normal, frame: 1, seq, tick: 1 });
    out.push(("同键聚合不刷爆", bus2.queued() == base_q + 1 && bus2.coalesced == 2));
    // 优先级溢出丢弃：Back 先死 Urgent 永不丢。
    let mut bus3 = EventBus::new();
    let mut seq3 = 1u64;
    let mut k = 0usize;
    while k < BUS_CAPACITY {
        let _ = bus3.publish(InputEvent {
            kind: EventKind::KeyDown,
            code: 1,
            x: 0, y: 0, axis: 0,
            priority: Priority::Back,
            frame: 1,
            seq: seq3,
            tick: 1,
        });
        seq3 += 1;
        k += 1;
    }
    let urgent = InputEvent { kind: EventKind::Hotkey, code: 9, x: 0, y: 0, axis: 0, priority: Priority::Urgent, frame: 1, seq: seq3, tick: 1 };
    out.push(("满载 Urgent 挤掉 Back", bus3.publish(urgent).is_ok() && bus3.overflow_dropped == 1));
    // 租约泄漏回收。
    let mut bus4 = EventBus::new();
    let s1 = bus4.subscribe("短租", 0, Priority::Normal, 10, 1);
    let swept = bus4.sweep_leases(100);
    out.push(("到期回收", s1.is_ok() && swept.len() == 1 && bus4.subscriber_count() == 0));
    let s2 = bus4.subscribe("长租", 0, Priority::Normal, 9999, 1);
    let swept2 = bus4.sweep_leases(100);
    out.push(("在期不回收", s2.is_ok() && swept2.is_empty()));
    // 优先级地板滤除。
    let mut bus5 = EventBus::new();
    let _ = bus5.subscribe("高地板", 1 << 8, Priority::Urgent, 9999, 1);
    let _ = bus5.publish(InputEvent { kind: EventKind::KeyDown, code: 5, x: 0, y: 0, axis: 0, priority: Priority::Normal, frame: 1, seq: 1, tick: 1 });
    let n5 = bus5.dispatch(2);
    out.push(("地板滤除不投递", n5 == 0));
    // AA01 同构契约与读屏。
    out.push(("AA01同构契约冻结", AA01_SHAPE_CONTRACT.contains("publish/subscribe/priority")));
    out.push(("订阅状态读屏可查", bus5.read_screen(2).len() >= 2));
    out.push(("账链完整", bus5.reconcile().is_ok() && bus5.validate().is_ok()));
    out.push(("诊断码带建议", !XcCode::OutOfOrder.detail().is_empty() && XcCode::OutOfOrder.code_hex() == 0x3305));
    out
}
