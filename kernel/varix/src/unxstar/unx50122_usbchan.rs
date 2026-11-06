//! UNX-F50122 打印类两通道语义（全双工/单向绑定端点账）（0x43b60 段）
//!
//! 锚点：UNX-F50122-J1 通道绑定 ×2 档落账，端点方向断言；判据锚定 §AI-63 §2 专题八。
//!
//! 「两通道」不是「两个端点」的同义词。USB 打印类在传输层只有两种合法形态：
//!
//! * **全双工**（bInterfaceProtocol = 2 / 3）：bulk OUT 承载作业数据，bulk IN
//!   承载状态回程。两侧方向必须由端点地址 bit7 明确给出——OUT 地址 bit7=0、
//!   IN 地址 bit7=1。方向位错了，主机就会把状态包写进数据管道。
//! * **单向绑定**（bInterfaceProtocol = 1）：只有 bulk OUT，设备没有回程端点。
//!   此时状态由本机状态账**合成**（`Backchannel::Synthetic`），直接读回程必须
//!   显式失败（`NoBackchannel`），绝不能返回零长度假包——假包会让上层把「没有
//!   回程」误当成「设备说没事」，这正是半双工打印最常见的静默故障。
//!
//! 绑定本身是原子操作：`bind()` 先把声明校验成 `ChannelBinding`，校验失败时账本
//! 里不会留下半条占位记录；只有整体合法才落账并给出序号。
//!
//! 数据结构：
//! * `EndpointDecl` / `Direction` —— 端点声明与方向位派生（bit7）；
//! * `ChannelTier` / `Backchannel` / `ChannelBinding` —— 两档通道与绑定结果；
//! * `ChannelLedger` —— 定容、去重、两档计数的绑定账（关闭状态单独记账）；
//! * `ChannelSession` —— 会话状态机（Idle/Streaming/Closed）+ 字节/信用/拒绝计数。
//!
//! 错误闭集：`SEG_BASE + n`（0x43b60 段独占 0x20），所有码值在判据或注入核里
//! 都被真实构造过。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43b60 独占 0x20）。
pub const SEG_BASE: u32 = 0x43b60;

pub const E_EMPTY: u16 = 1;
pub const E_NO_OUT: u16 = 2;
pub const E_EP_ZERO: u16 = 3;
pub const E_EP_ADDR: u16 = 4;
pub const E_DIRECTION: u16 = 5;
pub const E_PACKET: u16 = 6;
pub const E_TIER: u16 = 7;
pub const E_NO_IN: u16 = 8;
pub const E_EXTRA_IN: u16 = 9;
pub const E_CAPACITY: u16 = 10;
pub const E_DUP_KEY: u16 = 11;
pub const E_NO_KEY: u16 = 12;
pub const E_CLOSED: u16 = 13;
pub const E_NO_BACKCHANNEL: u16 = 14;
pub const E_OVER_MAX: u16 = 15;
pub const E_NO_CREDIT: u16 = 16;
pub const E_BAD_STATE: u16 = 17;

/// 单次 bulk 提交上限：8 × 512B。
///
/// 推导：高速 bulk 端点最大包长 512B，主机侧一次 URB 的散列表最多挂 8 个
/// 页内块（本内核页 4KiB 的一半），超过它的提交必须在会话层拆包，否则
/// 单次写就会突破 DMA 映射的单段长度假设。
pub const MAX_CHUNK: u16 = 4096;

/// 信用窗口上限（16 个 in-flight 提交）。超过 16 的窗口在单队列传输层上
/// 没有意义——队列深度只有 16。
pub const MAX_CREDITS: u32 = 16;

/// 通道错误闭集。`code()` 统一映射到 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelError {
    Empty,
    NoOut,
    EpZero,
    EpAddr,
    Direction,
    Packet,
    Tier,
    NoIn,
    ExtraIn,
    Capacity,
    DupKey,
    NoKey,
    Closed,
    NoBackchannel,
    OverMax,
    NoCredit,
    BadState,
}

impl ChannelError {
    pub fn code(self) -> u32 {
        let n = match self {
            ChannelError::Empty => E_EMPTY,
            ChannelError::NoOut => E_NO_OUT,
            ChannelError::EpZero => E_EP_ZERO,
            ChannelError::EpAddr => E_EP_ADDR,
            ChannelError::Direction => E_DIRECTION,
            ChannelError::Packet => E_PACKET,
            ChannelError::Tier => E_TIER,
            ChannelError::NoIn => E_NO_IN,
            ChannelError::ExtraIn => E_EXTRA_IN,
            ChannelError::Capacity => E_CAPACITY,
            ChannelError::DupKey => E_DUP_KEY,
            ChannelError::NoKey => E_NO_KEY,
            ChannelError::Closed => E_CLOSED,
            ChannelError::NoBackchannel => E_NO_BACKCHANNEL,
            ChannelError::OverMax => E_OVER_MAX,
            ChannelError::NoCredit => E_NO_CREDIT,
            ChannelError::BadState => E_BAD_STATE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            ChannelError::Empty => "channel declaration carries no endpoint",
            ChannelError::NoOut => "channel declaration has no OUT endpoint",
            ChannelError::EpZero => "endpoint number 0 belongs to control pipe",
            ChannelError::EpAddr => "endpoint address uses reserved bits 4..6",
            ChannelError::Direction => "endpoint direction bit contradicts role",
            ChannelError::Packet => "max packet size not a power of two in 8..=1024",
            ChannelError::Tier => "protocol code outside 1..=3",
            ChannelError::NoIn => "full-duplex tier without bulk IN endpoint",
            ChannelError::ExtraIn => "unidirectional tier declares a backchannel endpoint",
            ChannelError::Capacity => "channel ledger full or window out of range",
            ChannelError::DupKey => "same device key bound twice",
            ChannelError::NoKey => "device key not bound",
            ChannelError::Closed => "channel already closed",
            ChannelError::NoBackchannel => "unidirectional channel has no real backchannel",
            ChannelError::OverMax => "chunk larger than MAX_CHUNK",
            ChannelError::NoCredit => "credit window exhausted",
            ChannelError::BadState => "operation illegal in current link state",
        }
    }
}

/// 端点方向。由地址 bit7 派生，不另存字段——派生值不会与地址失配。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
}

impl Direction {
    pub fn bit7(self) -> u8 {
        match self {
            Direction::Out => 0x00,
            Direction::In => 0x80,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::Out => "out",
            Direction::In => "in",
        }
    }
}

/// 端点声明（只保留通道绑定要用的两字段 + 地址本身）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointDecl {
    pub addr: u8,
    pub max_packet: u16,
}

impl EndpointDecl {
    pub const fn new(addr: u8, max_packet: u16) -> EndpointDecl {
        EndpointDecl { addr, max_packet }
    }

    /// 方向位（bit7）。0x81 → In，0x01 → Out。
    pub fn direction(&self) -> Direction {
        if self.addr & 0x80 != 0 {
            Direction::In
        } else {
            Direction::Out
        }
    }

    /// 端点号（bit0..3）。EP0 是控制管道，不参与数据通道绑定。
    pub fn number(&self) -> u8 {
        self.addr & 0x0f
    }

    /// 包长必须是 8..=1024 内的 2 的幂。先判下界再算 `v-1`，
    /// 避免 v=0 时无符号下溢（debug 构建会直接 panic）。
    pub fn valid_packet(&self) -> bool {
        let v = self.max_packet;
        v >= 8 && v <= 1024 && (v & (v - 1)) == 0
    }
}

/// 通道两档。两档是**互斥闭集**：不是「有无回程」的布尔，而是两种协议语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelTier {
    /// 全双工：数据 + 真实状态回程。
    FullDuplex,
    /// 单向绑定：只有数据方向，回程由本机账合成。
    UniBound,
}

impl ChannelTier {
    /// 协议码 → 档位。1 单向；2 双向；3 为 IEEE 1284.4，语义上必须双向。
    pub fn from_proto(code: u8) -> Result<ChannelTier, ChannelError> {
        match code {
            1 => Ok(ChannelTier::UniBound),
            2 | 3 => Ok(ChannelTier::FullDuplex),
            _ => Err(ChannelError::Tier),
        }
    }

    /// 档位码（落账用）：1 全双工 / 2 单向绑定。
    /// 与协议码分开编号，避免把「档」和「声明」混成同一个数。
    pub fn tier_code(self) -> u8 {
        match self {
            ChannelTier::FullDuplex => 1,
            ChannelTier::UniBound => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChannelTier::FullDuplex => "full-duplex",
            ChannelTier::UniBound => "uni-bound",
        }
    }

    /// 全双工必须有 IN 端点；单向绑定必须没有。
    pub fn needs_in(self) -> bool {
        matches!(self, ChannelTier::FullDuplex)
    }
}

/// 状态回程来源。`Synthetic` 不是「空回程」，而是「本机状态账代答」，
/// 调用方必须看 `ChannelStatus::synthetic` 区分真实设备回复与本地推断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backchannel {
    Real { ep: u8, max_packet: u16 },
    Synthetic,
}

impl Backchannel {
    pub fn is_real(&self) -> bool {
        matches!(self, Backchannel::Real { .. })
    }

    /// 真实回程端点地址；单向档没有端点，返回 None 而不是 0——0 是控制端点的
    /// 合法地址，用 0 表示「没有」会与 EP0 混淆。
    pub fn ep_addr(&self) -> Option<u8> {
        match self {
            Backchannel::Real { ep, .. } => Some(*ep),
            Backchannel::Synthetic => None,
        }
    }

    pub fn max_packet(&self) -> u16 {
        match self {
            Backchannel::Real { max_packet, .. } => *max_packet,
            Backchannel::Synthetic => 0,
        }
    }
}

/// 通道绑定结果：一经生成即不可变（字段全 pub 但由 `bind` 单点构造）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelBinding {
    pub tier: ChannelTier,
    pub proto: u8,
    pub out_ep: u8,
    pub out_packet: u16,
    pub backchannel: Backchannel,
}

impl ChannelBinding {
    /// 已绑定的方向掩码：bit0 = OUT，bit1 = IN。单向档恒为 0b01。
    pub fn direction_mask(&self) -> u8 {
        let mut m = 0x01u8;
        if self.backchannel.is_real() {
            m |= 0x02;
        }
        m
    }

    pub fn can_write(&self) -> bool {
        self.out_ep & 0x80 == 0 && self.out_packet >= 8
    }

    pub fn can_read(&self) -> bool {
        self.backchannel.is_real()
    }

    /// 摘要（FNV-1a 64，字段序固定）。同一声明两次绑定必须逐位一致。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u8(h, self.tier.tier_code());
        h = fnv_u8(h, self.proto);
        h = fnv_u8(h, self.out_ep);
        h = fnv_u16(h, self.out_packet);
        match self.backchannel {
            Backchannel::Real { ep, max_packet } => {
                h = fnv_u8(h, 1);
                h = fnv_u8(h, ep);
                h = fnv_u16(h, max_packet);
            }
            Backchannel::Synthetic => {
                h = fnv_u8(h, 2);
            }
        }
        h
    }

    pub fn render(&self) -> String {
        let back = match self.backchannel {
            Backchannel::Real { ep, max_packet } => format!("in={:02x}/{}", ep, max_packet),
            Backchannel::Synthetic => "in=synthetic".to_string(),
        };
        format!(
            "chan {} proto={} out={:02x}/{} {} mask={:02b}",
            self.tier.label(),
            self.proto,
            self.out_ep,
            self.out_packet,
            back,
            self.direction_mask()
        )
    }
}

/// 通道声明（绑定输入）。两档由 proto 决定，但**仍要求调用方显式给出端点**：
/// 档位不能凭空推出端口布局，二者必须互相印证。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelDecl {
    pub proto: u8,
    pub out: Option<EndpointDecl>,
    pub in_ep: Option<EndpointDecl>,
}

impl ChannelDecl {
    pub const fn new(
        proto: u8,
        out: Option<EndpointDecl>,
        in_ep: Option<EndpointDecl>,
    ) -> ChannelDecl {
        ChannelDecl { proto, out, in_ep }
    }

    pub const fn uni(proto: u8, out: EndpointDecl) -> ChannelDecl {
        ChannelDecl { proto, out: Some(out), in_ep: None }
    }

    pub const fn duplex(proto: u8, out: EndpointDecl, in_ep: EndpointDecl) -> ChannelDecl {
        ChannelDecl { proto, out: Some(out), in_ep: Some(in_ep) }
    }
}

/// 绑定入口：声明 → 绑定，逐项校验，失败不留副作用（纯函数）。
///
/// 顺序即判据顺序：先查「有没有端点」，再查端点本身，最后才查档位与端点组合。
/// 这样注入核可以按错误码精确定位是哪一层拒绝的。
pub fn bind(decl: &ChannelDecl) -> Result<ChannelBinding, ChannelError> {
    let out = match decl.out {
        Some(e) => e,
        None => {
            if decl.in_ep.is_none() {
                return Err(ChannelError::Empty);
            }
            return Err(ChannelError::NoOut);
        }
    };
    if out.number() == 0 {
        return Err(ChannelError::EpZero);
    }
    if out.addr & 0x70 != 0 {
        return Err(ChannelError::EpAddr);
    }
    if out.direction() != Direction::Out {
        return Err(ChannelError::Direction);
    }
    if !out.valid_packet() {
        return Err(ChannelError::Packet);
    }
    let tier = ChannelTier::from_proto(decl.proto)?;
    match decl.in_ep {
        None => {
            if tier.needs_in() {
                return Err(ChannelError::NoIn);
            }
        }
        Some(ep) => {
            if !tier.needs_in() {
                // 单向档声明了 IN：不是「多一个端点也无妨」，而是协议语义冲突。
                return Err(ChannelError::ExtraIn);
            }
            if ep.number() == 0 {
                return Err(ChannelError::EpZero);
            }
            if ep.addr & 0x70 != 0 {
                return Err(ChannelError::EpAddr);
            }
            if ep.direction() != Direction::In {
                return Err(ChannelError::Direction);
            }
            if !ep.valid_packet() {
                return Err(ChannelError::Packet);
            }
        }
    }
    let backchannel = match decl.in_ep {
        Some(ep) => Backchannel::Real { ep: ep.addr, max_packet: ep.max_packet },
        None => Backchannel::Synthetic,
    };
    Ok(ChannelBinding {
        tier,
        proto: decl.proto,
        out_ep: out.addr,
        out_packet: out.max_packet,
        backchannel,
    })
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_u8(fnv_u8(h, (v & 0xff) as u8), (v >> 8) as u8)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_u16(fnv_u16(h, (v & 0xffff) as u16), (v >> 16) as u16)
}

// ---------------------------------------------------------------------------
// 绑定账：两档计数 + 去重 + 原子落账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub key: u32,
    pub binding: ChannelBinding,
    pub closed: bool,
}

/// 绑定账本。定容（≤8，一张打印设备表的规模），按设备键去重。
pub struct ChannelLedger {
    entries: Vec<LedgerEntry>,
    cap: usize,
    dups: u16,
    recloses: u16,
}

impl ChannelLedger {
    /// 新建账本。容量 0 或超 8 显式拒绝：静默扩表会让「表满」这类真故障
    /// 变成无人看见的丢记录。
    pub fn new(cap: usize) -> Result<ChannelLedger, ChannelError> {
        if cap == 0 || cap > 8 {
            return Err(ChannelError::Capacity);
        }
        Ok(ChannelLedger { entries: Vec::with_capacity(cap), cap, dups: 0, recloses: 0 })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dups(&self) -> u16 {
        self.dups
    }

    pub fn recloses(&self) -> u16 {
        self.recloses
    }

    /// 绑定并落账，返回落账序号。先校验后查重再查容量，任何一步失败都不写入。
    pub fn bind(&mut self, key: u32, decl: &ChannelDecl) -> Result<u32, ChannelError> {
        if key == 0 {
            return Err(ChannelError::Empty);
        }
        let binding = bind(decl)?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    self.dups += 1;
                    return Err(ChannelError::DupKey);
                }
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(ChannelError::Capacity);
        }
        let seq = self.entries.len() as u32;
        self.entries.push(LedgerEntry { key, binding, closed: false });
        Ok(seq)
    }

    pub fn get(&self, key: u32) -> Option<&LedgerEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 关闭一条绑定。重复关闭显式拒绝（`Closed`）并单独计数——重复关闭是
    /// 上层生命周期管理出错的信号，不能当成幂等成功吞掉。
    pub fn close(&mut self, key: u32) -> Result<ChannelBinding, ChannelError> {
        let mut i = 0usize;
        let mut found: Option<usize> = None;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key {
                    found = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match found {
            Some(v) => v,
            None => return Err(ChannelError::NoKey),
        };
        let mut already = false;
        let mut binding: Option<ChannelBinding> = None;
        if let Some(e) = self.entries.get(idx) {
            already = e.closed;
            binding = Some(e.binding);
        }
        if already {
            self.recloses += 1;
            return Err(ChannelError::Closed);
        }
        if let Some(e) = self.entries.get_mut(idx) {
            e.closed = true;
        }
        match binding {
            Some(b) => Ok(b),
            None => Err(ChannelError::NoKey),
        }
    }

    /// 两档落账计数：(全双工条数, 单向绑定条数)。关闭的绑定仍然计入——
    /// 落账记的是「曾经这样绑过」，不是「当前还开着」。
    pub fn tier_counts(&self) -> (u32, u32) {
        let mut full = 0u32;
        let mut uni = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                match e.binding.tier {
                    ChannelTier::FullDuplex => full += 1,
                    ChannelTier::UniBound => uni += 1,
                }
            }
            i += 1;
        }
        (full, uni)
    }

    /// 仍打开的绑定条数。
    pub fn open_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if !e.closed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账本摘要（含序号、键、绑定摘要与两档计数）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.entries.len() as u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_u32(h, e.key);
                h ^= e.binding.digest();
                h = h.wrapping_mul(0x100_0000_01b3);
                h = fnv_u8(h, if e.closed { 1 } else { 0 });
            }
            i += 1;
        }
        let (f, u) = self.tier_counts();
        h = fnv_u32(h, f);
        fnv_u32(h, u)
    }
}

// ---------------------------------------------------------------------------
// 会话状态机：写/边带读/收尾/关闭
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 没有在途提交。
    Idle,
    /// 有提交未收尾（flush 之前）。
    Streaming,
    /// 已关闭，所有操作一律 `Closed`。
    Closed,
}

impl LinkState {
    pub fn label(self) -> &'static str {
        match self {
            LinkState::Idle => "idle",
            LinkState::Streaming => "streaming",
            LinkState::Closed => "closed",
        }
    }
}

/// 本地状态账对外呈现的状态快照。`synthetic` 为真表示这条状态不是设备回的，
/// 而是单向档由本机账代答——上层必须据此决定是否展示「设备状态」字样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelStatus {
    pub synthetic: bool,
    pub credits: u32,
    pub bytes_out: u32,
    pub bytes_in: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionTotals {
    pub bytes_out: u32,
    pub bytes_in: u32,
    pub chunks: u32,
    pub zlps: u32,
    pub flushes: u32,
    pub reads: u32,
    pub synthetic_reads: u32,
    pub rejected_reads: u32,
    pub starved: u32,
}

impl SessionTotals {
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u32(h, self.bytes_out);
        h = fnv_u32(h, self.bytes_in);
        h = fnv_u32(h, self.chunks);
        h = fnv_u32(h, self.zlps);
        h = fnv_u32(h, self.flushes);
        h = fnv_u32(h, self.reads);
        h = fnv_u32(h, self.synthetic_reads);
        h = fnv_u32(h, self.rejected_reads);
        fnv_u32(h, self.starved)
    }
}

/// 一条绑定的运行期会话。状态机刻意只有三态：打印作业没有「半连接」这种
/// 中间态，收尾靠 flush，失败靠关闭重建——状态越少，拔出/超时路径越不会
/// 停在一个谁也回收不了的状态上。
pub struct ChannelSession {
    binding: ChannelBinding,
    state: LinkState,
    credits: u32,
    bytes_out: u32,
    bytes_in: u32,
    chunks: u32,
    zlps: u32,
    flushes: u32,
    reads: u32,
    synthetic_reads: u32,
    rejected_reads: u32,
    starved: u32,
    job_bytes: u32,
}

impl ChannelSession {
    /// 打开会话。信用窗口 1..=16，越界显式拒绝（窗口为 0 等于永不发车）。
    pub fn open(binding: ChannelBinding, credits: u32) -> Result<ChannelSession, ChannelError> {
        if credits == 0 || credits > MAX_CREDITS {
            return Err(ChannelError::Capacity);
        }
        Ok(ChannelSession {
            binding,
            state: LinkState::Idle,
            credits,
            bytes_out: 0,
            bytes_in: 0,
            chunks: 0,
            zlps: 0,
            flushes: 0,
            reads: 0,
            synthetic_reads: 0,
            rejected_reads: 0,
            starved: 0,
            job_bytes: 0,
        })
    }

    pub fn binding(&self) -> &ChannelBinding {
        &self.binding
    }

    pub fn state(&self) -> LinkState {
        self.state
    }

    pub fn credits(&self) -> u32 {
        self.credits
    }

    pub fn totals(&self) -> SessionTotals {
        SessionTotals {
            bytes_out: self.bytes_out,
            bytes_in: self.bytes_in,
            chunks: self.chunks,
            zlps: self.zlps,
            flushes: self.flushes,
            reads: self.reads,
            synthetic_reads: self.synthetic_reads,
            rejected_reads: self.rejected_reads,
            starved: self.starved,
        }
    }

    /// 提交一段作业数据。零长度必须走 `write_zlp`（ZLP 是收尾语义，
    /// 不是「写了 0 字节」），因此这里的 len=0 是调用错误。
    pub fn write(&mut self, len: u16) -> Result<u32, ChannelError> {
        if self.state == LinkState::Closed {
            return Err(ChannelError::Closed);
        }
        if len == 0 {
            return Err(ChannelError::BadState);
        }
        if len > MAX_CHUNK {
            return Err(ChannelError::OverMax);
        }
        if self.credits == 0 {
            self.starved += 1;
            return Err(ChannelError::NoCredit);
        }
        self.credits -= 1;
        self.bytes_out += len as u32;
        self.job_bytes += len as u32;
        self.chunks += 1;
        self.state = LinkState::Streaming;
        Ok(self.credits)
    }

    /// 提交零长度包（短包收尾）。不消耗信用：ZLP 不是数据，不应挤占窗口。
    pub fn write_zlp(&mut self) -> Result<u32, ChannelError> {
        if self.state == LinkState::Closed {
            return Err(ChannelError::Closed);
        }
        self.zlps += 1;
        self.state = LinkState::Streaming;
        Ok(self.zlps)
    }

    /// 收尾：返回本次 flush 之后累计的作业字节数，并回到 Idle。
    /// Idle 状态下 flush 是空操作，但显式报 `BadState`——空 flush 会让
    /// 「已提交」与「已排出」两个概念悄悄混淆。
    pub fn flush(&mut self) -> Result<u32, ChannelError> {
        if self.state == LinkState::Closed {
            return Err(ChannelError::Closed);
        }
        if self.state != LinkState::Streaming {
            return Err(ChannelError::BadState);
        }
        let n = self.job_bytes;
        self.job_bytes = 0;
        self.flushes += 1;
        self.state = LinkState::Idle;
        Ok(n)
    }

    /// 读真实边带（bulk IN）。单向档没有真实回程，必须显式失败：
    /// 返回 0 会让上层把「无回程」当作「设备回了空状态」。
    /// 失败不改状态、不计字节，因此也不可能把会话卡在某个中间态。
    pub fn read_backchannel(&mut self) -> Result<u16, ChannelError> {
        if self.state == LinkState::Closed {
            return Err(ChannelError::Closed);
        }
        match self.binding.backchannel {
            Backchannel::Real { max_packet, .. } => {
                self.reads += 1;
                self.bytes_in += max_packet as u32;
                Ok(max_packet)
            }
            Backchannel::Synthetic => {
                self.rejected_reads += 1;
                Err(ChannelError::NoBackchannel)
            }
        }
    }

    /// 取状态快照。单向档由本机账代答（`synthetic = true`），这也是唯一
    /// 合法的「单向档读状态」入口。
    pub fn status(&mut self) -> Result<ChannelStatus, ChannelError> {
        if self.state == LinkState::Closed {
            return Err(ChannelError::Closed);
        }
        let synthetic = !self.binding.backchannel.is_real();
        if synthetic {
            self.synthetic_reads += 1;
        } else {
            self.reads += 1;
            self.bytes_in += self.binding.backchannel.max_packet() as u32;
        }
        Ok(ChannelStatus {
            synthetic,
            credits: self.credits,
            bytes_out: self.bytes_out,
            bytes_in: self.bytes_in,
        })
    }

    /// 关闭会话，返回最终计数。关闭后所有操作统一 `Closed`。
    pub fn close(&mut self) -> SessionTotals {
        self.state = LinkState::Closed;
        self.job_bytes = 0;
        self.totals()
    }
}

// ---------------------------------------------------------------------------
// 三档模型：两个档位的真实声明（也是判据与注入核的底座）
// ---------------------------------------------------------------------------

/// 全双工声明：高速 bulk OUT/IN 各 512B。
pub fn decl_full_duplex() -> ChannelDecl {
    ChannelDecl::duplex(
        2,
        EndpointDecl::new(0x01, 512),
        EndpointDecl::new(0x81, 512),
    )
}

/// 单向绑定声明：全速 bulk OUT 64B，proto=1，无 IN。
pub fn decl_uni_bound() -> ChannelDecl {
    ChannelDecl::uni(1, EndpointDecl::new(0x02, 64))
}

/// 1284.4 声明：协议 3，仍须双向（1284.4 的可靠通道本身就带回程）。
pub fn decl_ieee1284() -> ChannelDecl {
    ChannelDecl::duplex(
        3,
        EndpointDecl::new(0x03, 64),
        EndpointDecl::new(0x83, 64),
    )
}

// ---------------------------------------------------------------------------
// 注入核：十类坏声明必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 空声明
    if bind(&ChannelDecl::new(2, None, None)).err() == Some(ChannelError::Empty) {
        caught += 1;
    }
    // 2 只有 IN，没有 OUT
    if bind(&ChannelDecl::new(
        2,
        None,
        Some(EndpointDecl::new(0x81, 512)),
    ))
    .err()
        == Some(ChannelError::NoOut)
    {
        caught += 1;
    }
    // 3 端点号为 0（EP0 是控制管道）
    if bind(&ChannelDecl::uni(1, EndpointDecl::new(0x00, 64))).err()
        == Some(ChannelError::EpZero)
    {
        caught += 1;
    }
    // 4 地址保留位 4..6 非零
    if bind(&ChannelDecl::uni(1, EndpointDecl::new(0x11, 64))).err()
        == Some(ChannelError::EpAddr)
    {
        caught += 1;
    }
    // 5 OUT 端点写成 IN 方向
    if bind(&ChannelDecl::uni(1, EndpointDecl::new(0x81, 64))).err()
        == Some(ChannelError::Direction)
    {
        caught += 1;
    }
    // 6 包长非 2 的幂
    if bind(&ChannelDecl::uni(1, EndpointDecl::new(0x01, 100))).err()
        == Some(ChannelError::Packet)
    {
        caught += 1;
    }
    // 7 全双工缺 IN
    if bind(&ChannelDecl::uni(2, EndpointDecl::new(0x01, 512))).err()
        == Some(ChannelError::NoIn)
    {
        caught += 1;
    }
    // 8 单向档多声明了 IN
    if bind(&ChannelDecl::duplex(
        1,
        EndpointDecl::new(0x01, 64),
        EndpointDecl::new(0x81, 64),
    ))
    .err()
        == Some(ChannelError::ExtraIn)
    {
        caught += 1;
    }
    // 9 协议码越界
    if bind(&ChannelDecl::uni(0, EndpointDecl::new(0x01, 64))).err()
        == Some(ChannelError::Tier)
    {
        caught += 1;
    }
    // 10 IN 端点方向位写反（声明 IN 但地址 bit7=0）
    if bind(&ChannelDecl::duplex(
        2,
        EndpointDecl::new(0x01, 512),
        EndpointDecl::new(0x02, 512),
    ))
    .err()
        == Some(ChannelError::Direction)
    {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50122");

    // J1-1 两档声明都能绑定成功（全双工 / 单向绑定各一）
    let full = bind(&decl_full_duplex());
    let uni = bind(&decl_uni_bound());
    let d1284 = bind(&decl_ieee1284());
    let ok = full.is_ok() && uni.is_ok() && d1284.is_ok();
    s.add("50122-J1-两档可绑", ok, if ok { "" } else { "合法声明被拒" });

    let f = match full {
        Ok(v) => v,
        Err(_) => return s,
    };
    let u = match uni {
        Ok(v) => v,
        Err(_) => return s,
    };
    let d = match d1284 {
        Ok(v) => v,
        Err(_) => return s,
    };

    // J1-2 两档分派：proto 1 → 单向，proto 2/3 → 全双工
    let ok = f.tier == ChannelTier::FullDuplex
        && d.tier == ChannelTier::FullDuplex
        && u.tier == ChannelTier::UniBound
        && ChannelTier::from_proto(0).err() == Some(ChannelError::Tier)
        && ChannelTier::from_proto(4).err() == Some(ChannelError::Tier);
    s.add("50122-J1-档位分派", ok, if ok { "" } else { "proto→档映射错" });

    // J1-3 端点方向断言：OUT bit7=0、IN bit7=1，且两档方向掩码不同
    let real_ep = f.backchannel.ep_addr();
    let ok = f.out_ep & 0x80 == 0
        && f.out_packet == 512
        && match real_ep {
            Some(e) => e & 0x80 == 0x80,
            None => false,
        }
        && f.direction_mask() == 0b11
        && u.direction_mask() == 0b01
        && u.backchannel.ep_addr().is_none()
        && !u.can_read()
        && f.can_read()
        && f.can_write()
        && u.can_write();
    let det = format!("f={:02b} u={:02b} in={:?}", f.direction_mask(), u.direction_mask(), real_ep);
    s.add_dyn("50122-J1-方向断言", ok, &det);

    // J1-4 两档落账：账本各记一条、计数 2:1（含 1284.4 那条全双工）
    let mut ledger = match ChannelLedger::new(4) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let s0 = ledger.bind(101, &decl_full_duplex());
    let s1 = ledger.bind(102, &decl_uni_bound());
    let s2 = ledger.bind(103, &decl_ieee1284());
    let (nf, nu) = ledger.tier_counts();
    let ok = s0 == Ok(0) && s1 == Ok(1) && s2 == Ok(2) && ledger.len() == 3 && nf == 2 && nu == 1;
    let det = format!("full={} uni={} len={}", nf, nu, ledger.len());
    s.add_dyn("50122-J1-两档落账", ok, &det);

    // J1-5 绑定原子性：坏声明的序号不进账，账本长度与摘要都不变
    let before_len = ledger.len();
    let before_digest = ledger.digest();
    let bad = ledger.bind(104, &ChannelDecl::new(2, None, None));
    let ok = bad.err() == Some(ChannelError::Empty)
        && ledger.len() == before_len
        && ledger.digest() == before_digest;
    s.add("50122-J1-绑定原子", ok, if ok { "" } else { "坏声明污染账本" });

    // J1-6 单向档直接读回程必拒，且状态/字节/回程账不动（零死锁面）
    let mut uni_sess = match ChannelSession::open(u, 4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let before_state = uni_sess.state();
    let r = uni_sess.read_backchannel();
    let t = uni_sess.totals();
    let ok = r.err() == Some(ChannelError::NoBackchannel)
        && uni_sess.state() == before_state
        && t.bytes_in == 0
        && t.rejected_reads == 1
        && t.reads == 0;
    let det = format!("st={} in={} rej={}", uni_sess.state().label(), t.bytes_in, t.rejected_reads);
    s.add_dyn("50122-J1-单向读拒", ok, &det);

    // J1-7 单向档状态由本机账代答：synthetic 为真、且不计真实回程字节
    let st = uni_sess.status();
    let ok = match st {
        Ok(v) => v.synthetic && v.bytes_in == 0 && v.bytes_out == 0 && uni_sess.totals().synthetic_reads == 1,
        Err(_) => false,
    };
    s.add("50122-J1-合成状态", ok, if ok { "" } else { "单向档状态未标记合成" });

    // J1-8 全双工读回程：real 回程、字节按 IN 包长累积
    let mut full_sess = match ChannelSession::open(f, 4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let back = full_sess.read_backchannel();
    let ok = back == Ok(512) && full_sess.totals().bytes_in == 512 && full_sess.totals().reads == 1;
    s.add("50122-J1-全双工读", ok, if ok { "" } else { "真实回程读数错" });

    // J1-9 写路径守恒：字节总和 = 各次提交之和，块数 = 提交次数
    let mut sum = 0u32;
    let mut i = 0u16;
    let mut ok = true;
    while i < 3 {
        let len = 64 + i * 64;
        match full_sess.write(len) {
            Ok(_) => sum += len as u32,
            Err(_) => ok = false,
        }
        i += 1;
    }
    let t = full_sess.totals();
    ok = ok && t.bytes_out == sum && t.chunks == 3 && t.zlps == 0;
    let det = format!("out={} sum={} chunks={}", t.bytes_out, sum, t.chunks);
    s.add_dyn("50122-J1-写守恒", ok, &det);

    // J1-10 信用窗口：耗尽后显式 NoCredit，已写字节不回退
    let mut starve = match ChannelSession::open(f, 1) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let first = starve.write(64);
    let second = starve.write(64);
    let t = starve.totals();
    let ok = first == Ok(0)
        && second.err() == Some(ChannelError::NoCredit)
        && t.bytes_out == 64
        && t.starved == 1
        && t.chunks == 1;
    s.add("50122-J1-信用窗口", ok, if ok { "" } else { "信用耗尽路径错" });

    // J1-11 超长提交必拒（MAX_CHUNK 之上），且不消耗信用
    let mut over = match ChannelSession::open(f, 4) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let r = over.write(MAX_CHUNK + 1);
    let ok = r.err() == Some(ChannelError::OverMax) && over.credits() == 4 && over.totals().bytes_out == 0;
    s.add("50122-J1-超长拒", ok, if ok { "" } else { "超长提交被放行" });

    // J1-12 flush 语义：Idle 下 flush 是错误；Streaming 下返回本次作业字节并回 Idle
    let idle_flush = over.flush();
    let wz = over.write_zlp();
    let w = over.write(128);
    let done = over.flush();
    let ok = idle_flush.err() == Some(ChannelError::BadState)
        && wz == Ok(1)
        && w == Ok(3)
        && done == Ok(128)
        && over.state() == LinkState::Idle
        && over.totals().flushes == 1
        && over.totals().zlps == 1;
    s.add("50122-J1-flush语义", ok, if ok { "" } else { "flush 边界错" });

    // J1-13 零长度写必须被拒（ZLP 只走 write_zlp）
    let zero = over.write(0);
    let ok = zero.err() == Some(ChannelError::BadState);
    s.add("50122-J1-零长拒", ok, if ok { "" } else { "零长写被当成合法" });

    // J1-14 关闭语义：关闭后写/读一律 Closed，重复关闭计数显性
    let totals = over.close();
    let after = over.write(64);
    let after_read = over.read_backchannel();
    let ok = totals.bytes_out == 128
        && after.err() == Some(ChannelError::Closed)
        && after_read.err() == Some(ChannelError::Closed)
        && over.state() == LinkState::Closed;
    s.add("50122-J1-关闭封口", ok, if ok { "" } else { "关闭后仍可操作" });

    // J1-15 账本去重/容量/未知键
    let dup = ledger.bind(101, &decl_uni_bound());
    let unknown = ledger.close(9999);
    let ok = dup.err() == Some(ChannelError::DupKey)
        && ledger.dups() == 1
        && ledger.len() == 3
        && unknown.err() == Some(ChannelError::NoKey)
        && ChannelLedger::new(0).err() == Some(ChannelError::Capacity)
        && ChannelLedger::new(9).err() == Some(ChannelError::Capacity);
    s.add("50122-J1-账本边界", ok, if ok { "" } else { "去重/容量/未知键失守" });

    // J1-16 关闭记账：关闭后仍计两档、open_len 递减、重复关闭显性
    let c1 = ledger.close(101);
    let c2 = ledger.close(101);
    let (nf, nu) = ledger.tier_counts();
    let ok = c1.is_ok()
        && c2.err() == Some(ChannelError::Closed)
        && ledger.recloses() == 1
        && ledger.open_len() == 2
        && nf == 2
        && nu == 1;
    let det = format!("open={} recloses={} f/u={}/{}", ledger.open_len(), ledger.recloses(), nf, nu);
    s.add_dyn("50122-J1-关闭记账", ok, &det);

    // J1-17 确定性：同声明两次绑定摘要逐位一致，同脚本两次会话计数一致
    let b1 = bind(&decl_full_duplex());
    let b2 = bind(&decl_full_duplex());
    let sa = match (b1, b2) {
        (Ok(x), Ok(y)) => (x, y),
        _ => return s,
    };
    let mut ra = match ChannelSession::open(sa.0, 3) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut rb = match ChannelSession::open(sa.1, 3) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = ra.write(200);
    let _ = rb.write(200);
    let _ = ra.read_backchannel();
    let _ = rb.read_backchannel();
    let ta = ra.close();
    let tb = rb.close();
    let ok = sa.0 == sa.1 && sa.0.digest() == sa.1.digest() && ta == tb && ta.digest() == tb.digest();
    s.add("50122-J1-确定性", ok, if ok { "" } else { "同输入漂移" });

    // J1-18 注入核：十类坏声明全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50122-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50122_checks_green() {
        let set = run_unx50122_checks();
        assert!(set.all_passed(), "UNX-F50122 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50122_two_tiers_land_in_ledger() {
        let mut l = match ChannelLedger::new(4) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.bind(1, &decl_full_duplex()).is_ok());
        assert!(l.bind(2, &decl_uni_bound()).is_ok());
        let (f, u) = l.tier_counts();
        assert_eq!((f, u), (1, 1));
    }

    #[test]
    fn unx50122_uni_read_has_no_backchannel() {
        let b = bind(&decl_uni_bound());
        assert!(b.is_ok());
        if let Ok(b) = b {
            let mut sess = ChannelSession::open(b, 2);
            assert!(sess.is_ok());
            if let Ok(mut sess) = sess {
                assert_eq!(sess.read_backchannel().err(), Some(ChannelError::NoBackchannel));
                assert_eq!(sess.state(), LinkState::Idle);
            }
        }
    }

    #[test]
    fn unx50122_error_codes_in_segment() {
        assert_eq!(ChannelError::Empty.code(), SEG_BASE + 1);
        assert_eq!(ChannelError::BadState.code(), SEG_BASE + 17);
    }
}
