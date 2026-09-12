//! m700ipc — VARIX-M700 AI-05 IPC 与消息域 (F101~F125)
//!
//! 端点登记所/零拷贝通道/消息信封规范/请求响应配对律/广播树/
//! 消息优先级带/背压协议/消息时间戳链/端点死亡通知/消息熔断器/
//! 消息取证箱/异步信箱谱/消息重放墙/跨域桥/消息压测台/
//! 消息丢包考古/信封版本协商/消息亲和投递/批量合并投递/端点健康分/
//! 消息血缘标签/死信归档/消息配额官/通道巡检机器人/IPC 域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F101 — 端点登记所：端点注册、去重与死亡标记
// ===========================================================================

pub const IPC_MAX_ENDPOINTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcEndpointKind {
    Server,
    Client,
    Bridge,
}

#[derive(Clone, Copy, Debug)]
pub struct IpcEndpoint {
    pub id: u32,
    pub core: u8,
    pub kind: IpcEndpointKind,
}

/// 端点登记所：id 去重，容量 16 拒绝溢出。
pub struct IpcRegistry {
    ids: [Option<u32>; IPC_MAX_ENDPOINTS],
    cores: [u8; IPC_MAX_ENDPOINTS],
    alive: [bool; IPC_MAX_ENDPOINTS],
    count: usize,
}

impl IpcRegistry {
    pub const fn new() -> IpcRegistry {
        IpcRegistry {
            ids: [const { None }; IPC_MAX_ENDPOINTS],
            cores: [0; IPC_MAX_ENDPOINTS],
            alive: [false; IPC_MAX_ENDPOINTS],
            count: 0,
        }
    }

    /// 登记端点；id 重复或满员返回 false。
    pub fn register(&mut self, id: u32, core: u8) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                return false;
            }
            i += 1;
        }
        if self.count >= IPC_MAX_ENDPOINTS {
            return false;
        }
        self.ids[self.count] = Some(id);
        self.cores[self.count] = core;
        self.alive[self.count] = true;
        self.count += 1;
        true
    }

    pub fn mark_dead(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                self.alive[i] = false;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn is_alive(&self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                return self.alive[i];
            }
            i += 1;
        }
        false
    }

    pub fn core_of(&self, id: u32) -> Option<u8> {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                return Some(self.cores[i]);
            }
            i += 1;
        }
        None
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F102 — 零拷贝通道：页句柄移交而非复制
// ===========================================================================

/// 零拷贝通道：发送方把页句柄移交给接收方，接收方用完归还。
/// 同一时刻只有一方持有，防止双读双写。
#[derive(Clone, Copy, Debug)]
pub struct IpcZeroCopyChannel {
    pub page: u64,
    pub owned_by_sender: bool,
    pub handoffs: u32,
}

impl IpcZeroCopyChannel {
    pub const fn new(page: u64) -> IpcZeroCopyChannel {
        IpcZeroCopyChannel { page, owned_by_sender: true, handoffs: 0 }
    }

    /// 移交：仅发送方持有且未移交时可移交。
    pub fn handoff(&mut self) -> bool {
        if self.owned_by_sender {
            self.owned_by_sender = false;
            self.handoffs += 1;
            true
        } else {
            false
        }
    }

    /// 归还：仅接收方持有时可归还。
    pub fn release_back(&mut self) -> bool {
        if !self.owned_by_sender {
            self.owned_by_sender = true;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F103 — 消息信封规范：magic/版本/载荷上限
// ===========================================================================

pub const IPC_ENVELOPE_MAGIC: u32 = 0x4950_4331; // "IPC1"
pub const IPC_ENVELOPE_VERSION: u8 = 3;
pub const IPC_ENVELOPE_MIN_VERSION: u8 = 2;
pub const IPC_ENVELOPE_MAX_VERSION: u8 = 8;
pub const IPC_MAX_PAYLOAD: u32 = 4096;
pub const IPC_HEADER_BYTES: u32 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcEnvelope {
    pub magic: u32,
    pub version: u8,
    pub payload_len: u32,
    pub tag: u8,
}

pub fn envelope_valid(e: &IpcEnvelope) -> bool {
    e.magic == IPC_ENVELOPE_MAGIC
        && e.version >= IPC_ENVELOPE_MIN_VERSION
        && e.version <= IPC_ENVELOPE_MAX_VERSION
        && e.payload_len <= IPC_MAX_PAYLOAD
}

// ===========================================================================
// F104 — 请求响应配对律：一条请求恰好兑换一次响应
// ===========================================================================

pub const IPC_PAIR_SLOTS: usize = 8;

/// 请求-响应配对表：open 登记请求（去重），take 兑换后立即失效。
pub struct IpcPairTable {
    req_ids: [Option<u64>; IPC_PAIR_SLOTS],
    open: usize,
}

impl IpcPairTable {
    pub const fn new() -> IpcPairTable {
        IpcPairTable {
            req_ids: [const { None }; IPC_PAIR_SLOTS],
            open: 0,
        }
    }

    pub fn open_request(&mut self, id: u64) -> bool {
        let mut i = 0usize;
        while i < IPC_PAIR_SLOTS {
            if self.req_ids[i] == Some(id) {
                return false;
            }
            i += 1;
        }
        let mut slot = IPC_PAIR_SLOTS;
        i = 0;
        while i < IPC_PAIR_SLOTS {
            if self.req_ids[i].is_none() {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot == IPC_PAIR_SLOTS {
            return false;
        }
        self.req_ids[slot] = Some(id);
        self.open += 1;
        true
    }

    /// 响应方兑换：取走即失效，杜绝二次响应。
    pub fn take_request(&mut self, id: u64) -> bool {
        let mut i = 0usize;
        while i < IPC_PAIR_SLOTS {
            if self.req_ids[i] == Some(id) {
                self.req_ids[i] = None;
                self.open -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn open(&self) -> usize {
        self.open
    }
}

// ===========================================================================
// F105 — 广播树：扇出掩码与层级规模
// ===========================================================================

pub const IPC_FANOUT_MAX: u32 = 4;

/// 满扇出树的节点总数：1 + f + f² + … + f^depth。
pub fn broadcast_total(fanout: u32, depth: u32) -> u32 {
    if fanout == 0 {
        return 1;
    }
    let mut total = 1u32;
    let mut level = 1u32;
    let mut d = 0u32;
    while d < depth {
        level *= fanout;
        total += level;
        d += 1;
    }
    total
}

/// 直接子节点数 = 掩码置位数。
pub fn child_count(mask: u16) -> u32 {
    mask.count_ones()
}

// ===========================================================================
// F106 — 消息优先级带：4 级，带号越大越急
// ===========================================================================

pub const IPC_BANDS: usize = 4;
pub const IPC_BAND_CAP: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct IpcBandQueue {
    pub pending: [u32; IPC_BANDS],
}

impl IpcBandQueue {
    pub const fn new() -> IpcBandQueue {
        IpcBandQueue { pending: [0; IPC_BANDS] }
    }

    /// 入带；带号越界或超带容量拒绝。
    pub fn push_band(&mut self, band: usize) -> bool {
        if band >= IPC_BANDS || self.pending[band] >= IPC_BAND_CAP {
            return false;
        }
        self.pending[band] += 1;
        true
    }

    /// 最高优先级的非空带；全空返回 None。
    pub fn next_band(&self) -> Option<usize> {
        let mut band = IPC_BANDS;
        while band > 0 {
            band -= 1;
            if self.pending[band] > 0 {
                return Some(band);
            }
        }
        None
    }

    pub fn pop_band(&mut self, band: usize) -> bool {
        if band < IPC_BANDS && self.pending[band] > 0 {
            self.pending[band] -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F107 — 背压协议：低水位放行，中段限速，高水位丢弃
// ===========================================================================

pub const IPC_BP_LOW_PERMILLE: u32 = 300;
pub const IPC_BP_HIGH_PERMILLE: u32 = 800;
pub const IPC_BP_MAX_DELAY_US: u32 = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcBpAction {
    Pass,
    Slow(u32),
    Drop,
}

pub fn depth_permille(used: u32, cap: u32) -> u32 {
    if cap == 0 {
        0
    } else {
        used * 1000 / cap
    }
}

pub fn backpressure(depth: u32) -> IpcBpAction {
    if depth >= IPC_BP_HIGH_PERMILLE {
        IpcBpAction::Drop
    } else if depth > IPC_BP_LOW_PERMILLE {
        IpcBpAction::Slow(
            (depth - IPC_BP_LOW_PERMILLE) * IPC_BP_MAX_DELAY_US
                / (IPC_BP_HIGH_PERMILLE - IPC_BP_LOW_PERMILLE),
        )
    } else {
        IpcBpAction::Pass
    }
}

// ===========================================================================
// F108 — 消息时间戳链：严格单调递增
// ===========================================================================

pub fn chain_ok(stamps: &[u64]) -> bool {
    let mut i = 1usize;
    while i < stamps.len() {
        if stamps[i] <= stamps[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

/// 首尾跨度（不足两条计 0）。
pub fn chain_span(stamps: &[u64]) -> u64 {
    if stamps.len() < 2 {
        0
    } else {
        stamps[stamps.len() - 1] - stamps[0]
    }
}

// ===========================================================================
// F109 — 端点死亡通知：订阅去重，死者本人不收讣告
// ===========================================================================

pub const IPC_DEATH_SUBS: usize = 8;

pub struct IpcDeathNotifier {
    subs: [Option<u32>; IPC_DEATH_SUBS],
    sub_count: usize,
    pub notified: u32,
}

impl IpcDeathNotifier {
    pub const fn new() -> IpcDeathNotifier {
        IpcDeathNotifier {
            subs: [const { None }; IPC_DEATH_SUBS],
            sub_count: 0,
            notified: 0,
        }
    }

    pub fn subscribe(&mut self, ep: u32) -> bool {
        let mut i = 0usize;
        while i < self.sub_count {
            if self.subs[i] == Some(ep) {
                return false;
            }
            i += 1;
        }
        if self.sub_count >= IPC_DEATH_SUBS {
            return false;
        }
        self.subs[self.sub_count] = Some(ep);
        self.sub_count += 1;
        true
    }

    pub fn subscribers(&self) -> usize {
        self.sub_count
    }

    /// 广播讣告；死者本身除外。返回本次通知数。
    pub fn notify_death(&mut self, dead_ep: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.sub_count {
            if self.subs[i] != Some(dead_ep) {
                self.notified += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// F110 — 消息熔断器：连续 5 败熔断，3 连成功恢复
// ===========================================================================

pub const IPC_CB_FAIL_THRESHOLD: u32 = 5;
pub const IPC_CB_PROBE_SUCCESSES: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcCircuitState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug)]
pub struct IpcCircuitBreaker {
    pub consecutive_fails: u32,
    pub probe_successes: u32,
    pub open: bool,
}

impl IpcCircuitBreaker {
    pub const fn new() -> IpcCircuitBreaker {
        IpcCircuitBreaker { consecutive_fails: 0, probe_successes: 0, open: false }
    }

    pub fn record(&mut self, ok: bool) -> IpcCircuitState {
        if ok {
            if self.open {
                self.probe_successes += 1;
                if self.probe_successes >= IPC_CB_PROBE_SUCCESSES {
                    self.open = false;
                    self.consecutive_fails = 0;
                    self.probe_successes = 0;
                }
            } else {
                self.consecutive_fails = 0;
            }
        } else {
            self.probe_successes = 0;
            if !self.open {
                self.consecutive_fails += 1;
                if self.consecutive_fails >= IPC_CB_FAIL_THRESHOLD {
                    self.open = true;
                }
            }
        }
        if self.open {
            IpcCircuitState::Open
        } else {
            IpcCircuitState::Closed
        }
    }
}

// ===========================================================================
// F111 — 消息取证箱：摘要 + 防篡改链，定容 8 槽
// ===========================================================================

pub const IPC_FORENSIC_SLOTS: usize = 8;

/// FNV-1a 32 位摘要。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 取证箱：逐条摘要入槽，链条 = 上一链吸收新摘要，历史不可抹改。
pub struct IpcForensicsBox {
    digests: [Option<u32>; IPC_FORENSIC_SLOTS],
    count: usize,
    pub chain: u32,
}

impl IpcForensicsBox {
    pub const fn new() -> IpcForensicsBox {
        IpcForensicsBox {
            digests: [const { None }; IPC_FORENSIC_SLOTS],
            count: 0,
            chain: 0xCAFE_D00D,
        }
    }

    pub fn record(&mut self, payload: &[u8]) -> bool {
        if self.count >= IPC_FORENSIC_SLOTS {
            return false;
        }
        let d = fnv1a(payload);
        self.digests[self.count] = Some(d);
        self.chain = self.chain ^ d;
        self.chain = self.chain.wrapping_mul(0x0100_0193);
        self.count += 1;
        true
    }

    pub fn digest_at(&self, i: usize) -> Option<u32> {
        if i < self.count {
            self.digests[i]
        } else {
            None
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F112 — 异步信箱谱：定容 FIFO，满拒发空回无
// ===========================================================================

pub const IPC_MAILBOX_CAP: usize = 8;

pub struct IpcMailbox {
    slots: [Option<u64>; IPC_MAILBOX_CAP],
    head: usize,
    len: usize,
}

impl IpcMailbox {
    pub const fn new() -> IpcMailbox {
        IpcMailbox {
            slots: [const { None }; IPC_MAILBOX_CAP],
            head: 0,
            len: 0,
        }
    }

    pub fn send(&mut self, msg: u64) -> bool {
        if self.len >= IPC_MAILBOX_CAP {
            return false;
        }
        let tail = (self.head + self.len) % IPC_MAILBOX_CAP;
        self.slots[tail] = Some(msg);
        self.len += 1;
        true
    }

    pub fn recv(&mut self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        let m = self.slots[self.head].take();
        self.head = (self.head + 1) % IPC_MAILBOX_CAP;
        self.len -= 1;
        m
    }

    pub fn depth(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F113 — 消息重放墙：64 位滑窗位图拒绝重放
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct IpcReplayWall {
    pub watermark: u64,
    window: u64,
}

impl IpcReplayWall {
    pub const fn new() -> IpcReplayWall {
        IpcReplayWall { watermark: 0, window: 0 }
    }

    /// 收下返回 true；重放或过老（水印前 64 之外）拒绝。
    pub fn admit(&mut self, seq: u64) -> bool {
        if seq > self.watermark {
            let advance = seq - self.watermark;
            if advance >= 64 {
                self.window = 1;
            } else {
                self.window = (self.window << advance) | 1;
            }
            self.watermark = seq;
            return true;
        }
        let offset = self.watermark - seq;
        if offset >= 64 {
            return false;
        }
        if (self.window >> offset) & 1 == 1 {
            return false;
        }
        self.window |= 1u64 << offset;
        true
    }
}

// ===========================================================================
// F114 — 跨域桥：域 A 消息号 → 域 B 消息号，一路由一映射
// ===========================================================================

pub const IPC_BRIDGE_ROUTES: usize = 8;

pub struct IpcDomainBridge {
    a: [Option<u8>; IPC_BRIDGE_ROUTES],
    b: [u8; IPC_BRIDGE_ROUTES],
    count: usize,
}

impl IpcDomainBridge {
    pub const fn new() -> IpcDomainBridge {
        IpcDomainBridge {
            a: [const { None }; IPC_BRIDGE_ROUTES],
            b: [0; IPC_BRIDGE_ROUTES],
            count: 0,
        }
    }

    /// 添加路由；同一 A 侧消息号只允许一条路由。
    pub fn add_route(&mut self, a: u8, b: u8) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.a[i] == Some(a) {
                return false;
            }
            i += 1;
        }
        if self.count >= IPC_BRIDGE_ROUTES {
            return false;
        }
        self.a[self.count] = Some(a);
        self.b[self.count] = b;
        self.count += 1;
        true
    }

    pub fn translate(&self, a: u8) -> Option<u8> {
        let mut i = 0usize;
        while i < self.count {
            if self.a[i] == Some(a) {
                return Some(self.b[i]);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F115 — 消息压测台：确定性丢包率的投递统计
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct IpcStressBench {
    pub sent: u64,
    pub delivered: u64,
    pub dropped: u64,
}

impl IpcStressBench {
    /// 跑一轮：sends 次发送，丢包率 drop_rate_permille（确定性取整）。
    pub fn run_round(&mut self, sends: u64, drop_rate_permille: u64) {
        let dropped = sends * drop_rate_permille / 1000;
        self.sent += sends;
        self.dropped += dropped;
        self.delivered += sends - dropped;
    }

    pub fn delivery_permille(&self) -> u32 {
        if self.sent == 0 {
            0
        } else {
            (self.delivered * 1000 / self.sent) as u32
        }
    }
}

// ===========================================================================
// F116 — 消息丢包考古：序列号缺口统计
// ===========================================================================

/// 缺失序列号总数：(last-first+1) - 条数（不足两条计 0）。
pub fn missing_count(seqs: &[u64]) -> u64 {
    if seqs.len() < 2 {
        0
    } else {
        (seqs[seqs.len() - 1] - seqs[0] + 1) - seqs.len() as u64
    }
}

/// 最早的缺口号；无缺口返回 None。
pub fn first_gap(seqs: &[u64]) -> Option<u64> {
    let mut i = 1usize;
    while i < seqs.len() {
        if seqs[i] != seqs[i - 1] + 1 {
            return Some(seqs[i - 1] + 1);
        }
        i += 1;
    }
    None
}

// ===========================================================================
// F117 — 信封版本协商：共同最高版本 = 双方最小值
// ===========================================================================

pub fn negotiate(a: u8, b: u8) -> u8 {
    if a < b {
        a
    } else {
        b
    }
}

pub fn version_compatible(a: u8, b: u8) -> bool {
    let m = if a < b { a } else { b };
    let x = if a > b { a } else { b };
    m >= IPC_ENVELOPE_MIN_VERSION && x <= IPC_ENVELOPE_MAX_VERSION
}

// ===========================================================================
// F118 — 消息亲和投递：同核优先，否则取首个端点
// ===========================================================================

pub const IPC_AFFINITY_LOCAL_PERMILLE: u32 = 1000;
pub const IPC_AFFINITY_REMOTE_PERMILLE: u32 = 250;

pub fn affinity_score(ep_core: u8, msg_core: u8) -> u32 {
    if ep_core == msg_core {
        IPC_AFFINITY_LOCAL_PERMILLE
    } else {
        IPC_AFFINITY_REMOTE_PERMILLE
    }
}

pub fn pick_endpoint<'a>(eps: &'a [IpcEndpoint], msg_core: u8) -> Option<&'a IpcEndpoint> {
    let mut i = 0usize;
    while i < eps.len() {
        if eps[i].core == msg_core {
            return Some(&eps[i]);
        }
        i += 1;
    }
    if eps.is_empty() {
        None
    } else {
        Some(&eps[0])
    }
}

// ===========================================================================
// F119 — 批量合并投递：同标签 >=4 条值得合并
// ===========================================================================

pub const IPC_COALESCE_MIN: usize = 4;

/// 不同标签数（手动去重计数）。
pub fn coalesce_tags(tags: &[u8]) -> u32 {
    let mut distinct = 0u32;
    let mut i = 0usize;
    while i < tags.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < i {
            if tags[j] == tags[i] {
                seen = true;
                break;
            }
            j += 1;
        }
        if !seen {
            distinct += 1;
        }
        i += 1;
    }
    distinct
}

pub fn coalesce_worth(n_same_tag: usize) -> bool {
    n_same_tag >= IPC_COALESCE_MIN
}

// ===========================================================================
// F120 — 端点健康分：成功率 permille，低于 600‰ 判劣化
// ===========================================================================

pub const IPC_HEALTH_DEGRADE_PERMILLE: u32 = 600;

#[derive(Clone, Copy, Debug, Default)]
pub struct IpcHealth {
    pub ok: u64,
    pub fail: u64,
}

pub fn health_permille(h: &IpcHealth) -> u32 {
    let total = h.ok + h.fail;
    if total == 0 {
        0
    } else {
        (h.ok * 1000 / total) as u32
    }
}

pub fn health_degraded(h: &IpcHealth) -> bool {
    let total = h.ok + h.fail;
    total > 0 && health_permille(h) < IPC_HEALTH_DEGRADE_PERMILLE
}

// ===========================================================================
// F121 — 消息血缘标签：起源域 + 追踪号逐跳保留
// ===========================================================================

pub const IPC_HOP_MAX: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcLineage {
    pub origin_domain: u16,
    pub trace_id: u32,
    pub hops: u8,
}

pub fn lineage_valid(l: &IpcLineage) -> bool {
    l.origin_domain != 0
}

/// 逐跳传播：起源域与追踪号不变，跳数 +1。
pub fn lineage_propagate(l: &IpcLineage) -> IpcLineage {
    IpcLineage { origin_domain: l.origin_domain, trace_id: l.trace_id, hops: l.hops + 1 }
}

pub fn hop_exceeded(l: &IpcLineage) -> bool {
    l.hops > IPC_HOP_MAX
}

// ===========================================================================
// F122 — 死信归档：失败消息按 seq 去重入档，定容 8
// ===========================================================================

pub const IPC_DEADLETTER_CAP: usize = 8;

pub struct IpcDeadLetterBox {
    seqs: [Option<u64>; IPC_DEADLETTER_CAP],
    count: usize,
}

impl IpcDeadLetterBox {
    pub const fn new() -> IpcDeadLetterBox {
        IpcDeadLetterBox {
            seqs: [const { None }; IPC_DEADLETTER_CAP],
            count: 0,
        }
    }

    pub fn archive(&mut self, seq: u64) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.seqs[i] == Some(seq) {
                return false;
            }
            i += 1;
        }
        if self.count >= IPC_DEADLETTER_CAP {
            return false;
        }
        self.seqs[self.count] = Some(seq);
        self.count += 1;
        true
    }

    pub fn clear(&mut self) {
        let mut i = 0usize;
        while i < IPC_DEADLETTER_CAP {
            self.seqs[i] = None;
            i += 1;
        }
        self.count = 0;
    }

    pub fn archived(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F123 — 消息配额官：限额发放，永不透支，补充封顶
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct IpcQuota {
    pub granted: u32,
    pub used: u32,
}

impl IpcQuota {
    pub const fn new(granted: u32) -> IpcQuota {
        IpcQuota { granted, used: 0 }
    }

    pub fn remaining(&self) -> u32 {
        if self.used > self.granted {
            0
        } else {
            self.granted - self.used
        }
    }

    /// 申请额度，返回实际批准数（不超过余额）。
    pub fn take(&mut self, want: u32) -> u32 {
        let remaining = self.remaining();
        let grant = if want > remaining { remaining } else { want };
        self.used += grant;
        grant
    }

    /// 补充额度，余额封顶不超过 granted。
    pub fn refill(&mut self, n: u32) {
        self.used = self.used.saturating_sub(n);
    }
}

// ===========================================================================
// F124 — 通道巡检机器人：水位三级判定 + 陈旧即失败
// ===========================================================================

pub const IPC_PATROL_WARN_PERMILLE: u32 = 700;
pub const IPC_PATROL_FAIL_PERMILLE: u32 = 950;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcPatrolVerdict {
    Ok,
    Warn,
    Fail,
}

pub fn inspect_channel(depth: u32, stale: bool) -> IpcPatrolVerdict {
    if stale || depth >= IPC_PATROL_FAIL_PERMILLE {
        IpcPatrolVerdict::Fail
    } else if depth >= IPC_PATROL_WARN_PERMILLE {
        IpcPatrolVerdict::Warn
    } else {
        IpcPatrolVerdict::Ok
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IpcPatrolLog {
    pub ok: u32,
    pub warn: u32,
    pub fail: u32,
}

impl IpcPatrolLog {
    pub fn record(&mut self, v: IpcPatrolVerdict) {
        match v {
            IpcPatrolVerdict::Ok => self.ok += 1,
            IpcPatrolVerdict::Warn => self.warn += 1,
            IpcPatrolVerdict::Fail => self.fail += 1,
        }
    }
}

// ===========================================================================
// F125 — IPC 域年报：章节完备性 + 黄金往返剧本
// ===========================================================================

pub const IPC_REPORT_SECTIONS: [&str; 5] =
    ["registry", "channels", "envelopes", "backpressure", "deadletter"];

pub fn ipc_report_complete(filled: u32) -> bool {
    filled >= IPC_REPORT_SECTIONS.len() as u32
}

/// 黄金剧本：3 条消息入箱，依次取出，末态空箱。
pub fn ipc_golden_roundtrip() -> (u32, usize) {
    let mut mb = IpcMailbox::new();
    mb.send(1);
    mb.send(2);
    mb.send(3);
    let mut drained = 0u32;
    while mb.recv().is_some() {
        drained += 1;
    }
    (drained, mb.depth())
}

pub fn ipc_golden_matches(drained: u32, depth: usize) -> bool {
    drained == 3 && depth == 0
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700ipc_checks() -> CheckSet {
    let mut set = CheckSet::new("m700ipc");

    // F101 端点登记所
    let mut reg = IpcRegistry::new();
    let r1 = reg.register(1, 0);
    let r2 = reg.register(2, 1);
    let r3 = reg.register(1, 2);
    let reg_count = reg.count();
    set.add("F101 registry register", r1 && r2 && reg_count == 2, "two endpoints");
    set.add("F101 registry dedup", !r3 && reg.count() == 2, "id dedup");
    let mut full = IpcRegistry::new();
    let mut id = 1u32;
    while full.count() < IPC_MAX_ENDPOINTS {
        full.register(id, (id % 4) as u8);
        id += 1;
    }
    let full_count = full.count();
    set.add(
        "F101 registry capacity",
        full_count == IPC_MAX_ENDPOINTS && !full.register(999, 0),
        "cap 16, overflow rejected",
    );
    let mut dreg = IpcRegistry::new();
    dreg.register(7, 2);
    let was_alive = dreg.is_alive(7);
    dreg.mark_dead(7);
    let alive_after = dreg.is_alive(7);
    set.add("F101 death notice", was_alive && !alive_after, "mark dead");

    // F102 零拷贝通道
    let mut ch = IpcZeroCopyChannel::new(42);
    let h1 = ch.handoff();
    let sender_owns_after = ch.owned_by_sender;
    let h2 = ch.handoff();
    let r1 = ch.release_back();
    set.add("F102 zero-copy handoff", h1 && !sender_owns_after, "page handed over");
    set.add("F102 no double handoff", !h2 && r1 && ch.handoffs == 1, "one handoff, returned");

    // F103 消息信封规范
    let ok_env = IpcEnvelope {
        magic: IPC_ENVELOPE_MAGIC,
        version: IPC_ENVELOPE_VERSION,
        payload_len: 512,
        tag: 1,
    };
    let bad_magic = IpcEnvelope { magic: 0, version: IPC_ENVELOPE_VERSION, payload_len: 512, tag: 1 };
    let too_big = IpcEnvelope {
        magic: IPC_ENVELOPE_MAGIC,
        version: IPC_ENVELOPE_VERSION,
        payload_len: IPC_MAX_PAYLOAD + 1,
        tag: 1,
    };
    set.add("F103 envelope valid", envelope_valid(&ok_env), "magic+version+len");
    set.add(
        "F103 envelope reject",
        !envelope_valid(&bad_magic) && !envelope_valid(&too_big),
        "malformed rejected",
    );

    // F104 请求响应配对律
    let mut pt = IpcPairTable::new();
    let o1 = pt.open_request(100);
    let o2 = pt.open_request(100);
    let o3 = pt.open_request(200);
    let open_before = pt.open();
    set.add("F104 pair open", o1 && o3 && !o2 && open_before == 2, "dedup open");
    let t1 = pt.take_request(100);
    let t2 = pt.take_request(100);
    let open_after = pt.open();
    set.add("F104 pair take", t1 && !t2 && open_after == 1, "one-shot reply token");

    // F105 广播树
    set.add(
        "F105 broadcast fanout",
        broadcast_total(4, 2) == 21 && broadcast_total(2, 3) == 15,
        "geometric sum",
    );
    set.add("F105 broadcast child mask", child_count(0b1011) == 3 && child_count(0) == 0, "bitmask count");

    // F106 消息优先级带
    let mut q = IpcBandQueue::new();
    let p1 = q.push_band(0);
    let p3 = q.push_band(3);
    let p3b = q.push_band(3);
    let p9 = q.push_band(9);
    set.add("F106 band push", p1 && p3 && p3b && !p9, "push validated");
    let top = q.next_band();
    let top_count = q.pending[3];
    q.pop_band(3);
    let after_pop = q.pending[3];
    set.add(
        "F106 band priority",
        top == Some(3) && top_count == 2 && after_pop == 1,
        "urgent first, drained",
    );

    // F107 背压协议
    set.add("F107 backpressure pass", backpressure(100) == IpcBpAction::Pass, "low load");
    set.add(
        "F107 backpressure slow",
        backpressure(500) == IpcBpAction::Slow(800),
        "200/500 of max delay",
    );
    set.add("F107 backpressure drop", backpressure(900) == IpcBpAction::Drop, "high load");
    set.add(
        "F107 depth permille",
        depth_permille(6, 8) == 750 && depth_permille(0, 8) == 0,
        "queue load",
    );

    // F108 消息时间戳链
    let good = [10u64, 12, 13, 20];
    let bad = [10u64, 12, 12, 20];
    set.add("F108 chain ordered", chain_ok(&good) && !chain_ok(&bad), "strictly monotonic");
    set.add("F108 chain span", chain_span(&good) == 10 && chain_span(&[]) == 0, "span = last-first");

    // F109 端点死亡通知
    let mut dn = IpcDeathNotifier::new();
    let s1 = dn.subscribe(3);
    let s2 = dn.subscribe(3);
    let s3 = dn.subscribe(5);
    let s4 = dn.subscribe(9);
    let sub_count = dn.subscribers();
    set.add("F109 subscribe dedup", s1 && !s2 && s3 && s4 && sub_count == 3, "dedup subs");
    let n = dn.notify_death(5);
    let notified_total = dn.notified;
    set.add("F109 notify fan-out", n == 2 && notified_total == 2, "dead excluded");

    // F110 消息熔断器
    let mut cb = IpcCircuitBreaker::new();
    let mut i = 0u32;
    let mut state = IpcCircuitState::Closed;
    while i < IPC_CB_FAIL_THRESHOLD {
        state = cb.record(false);
        i += 1;
    }
    let open_now = cb.open;
    set.add("F110 breaker opens", state == IpcCircuitState::Open && open_now, "5 fails trip");
    let s1 = cb.record(true);
    let s2 = cb.record(true);
    let still_open = cb.open;
    let s3 = cb.record(true);
    set.add(
        "F110 breaker probe holds",
        s1 == IpcCircuitState::Open && s2 == IpcCircuitState::Open && still_open,
        "needs 3 successes",
    );
    set.add("F110 breaker closes", s3 == IpcCircuitState::Closed && !cb.open, "recovered");

    // F111 消息取证箱
    let mut fb = IpcForensicsBox::new();
    let d1 = fb.record(&[1, 2, 3]);
    let d2 = fb.record(&[1, 2, 3]);
    let d3 = fb.record(&[1, 2, 4]);
    set.add(
        "F111 digest distinct",
        d1 && d2 && d3
            && fb.digest_at(0) == fb.digest_at(1)
            && fb.digest_at(2) != fb.digest_at(1),
        "same input same digest",
    );
    let chain_before = fb.chain;
    fb.record(&[7]);
    let chain_after = fb.chain;
    set.add("F111 chain evolves", chain_before != chain_after, "tamper-evident chain");
    let mut ffull = IpcForensicsBox::new();
    let mut j = 0usize;
    let mut fill_ok = true;
    while j < IPC_FORENSIC_SLOTS {
        fill_ok &= ffull.record(&[9]);
        j += 1;
    }
    let fcount = ffull.count();
    set.add(
        "F111 forensics cap",
        fill_ok && fcount == IPC_FORENSIC_SLOTS && !ffull.record(&[9]),
        "8 slots",
    );

    // F112 异步信箱谱
    let mut mb = IpcMailbox::new();
    let ms1 = mb.send(11);
    let ms2 = mb.send(22);
    let ms3 = mb.send(33);
    let mr1 = mb.recv();
    let mr2 = mb.recv();
    set.add(
        "F112 mailbox fifo",
        ms1 && ms2 && ms3 && mr1 == Some(11) && mr2 == Some(22),
        "in order",
    );
    let mut fmb = IpcMailbox::new();
    let mut k = 0u64;
    while k < IPC_MAILBOX_CAP as u64 {
        fmb.send(k);
        k += 1;
    }
    let full_depth = fmb.depth();
    set.add("F112 mailbox full", full_depth == IPC_MAILBOX_CAP && !fmb.send(99), "cap 8");
    set.add("F112 mailbox empty", IpcMailbox::new().recv().is_none(), "empty none");

    // F113 消息重放墙
    let mut w = IpcReplayWall::new();
    let a1 = w.admit(10);
    let a2 = w.admit(10);
    let a3 = w.admit(11);
    let a4 = w.admit(9);
    let a5 = w.admit(9);
    set.add("F113 replay wall", a1 && !a2 && a3 && a4 && !a5, "replays blocked");
    let mut seq = 12u64;
    let mut seq_ok = true;
    while seq < 80 {
        seq_ok &= w.admit(seq);
        seq += 1;
    }
    set.add("F113 too old rejected", seq_ok && !w.admit(10), "aged out of window");

    // F114 跨域桥
    let mut br = IpcDomainBridge::new();
    let rt1 = br.add_route(2, 7);
    let rt2 = br.add_route(2, 9);
    let rt3 = br.add_route(3, 8);
    set.add("F114 bridge route", rt1 && !rt2 && rt3, "one route per source");
    let t1 = br.translate(2);
    let t2 = br.translate(3);
    let t3 = br.translate(4);
    set.add("F114 bridge translate", t1 == Some(7) && t2 == Some(8) && t3.is_none(), "mapped or none");

    // F115 消息压测台
    let mut sb = IpcStressBench::default();
    sb.run_round(100, 100);
    let delivered1 = sb.delivered;
    let dropped1 = sb.dropped;
    set.add("F115 stress round", delivered1 == 90 && dropped1 == 10, "10% drop deterministic");
    sb.run_round(50, 0);
    set.add("F115 stress rate", sb.delivery_permille() == 933, "140/150 delivered");

    // F116 消息丢包考古
    let seqs = [1u64, 2, 3, 6, 7];
    set.add("F116 gap count", missing_count(&seqs) == 2, "4 and 5 lost");
    set.add(
        "F116 first gap",
        first_gap(&seqs) == Some(4) && first_gap(&[5, 6, 7]).is_none() && missing_count(&[5]) == 0,
        "earliest hole / clean",
    );

    // F117 信封版本协商
    set.add("F117 negotiate", negotiate(5, 3) == 3 && negotiate(4, 4) == 4, "common = min");
    set.add("F117 negotiate compat", version_compatible(3, 5) && !version_compatible(1, 5), "floor enforced");

    // F118 消息亲和投递
    let eps = [
        IpcEndpoint { id: 1, core: 0, kind: IpcEndpointKind::Client },
        IpcEndpoint { id: 2, core: 2, kind: IpcEndpointKind::Server },
        IpcEndpoint { id: 3, core: 1, kind: IpcEndpointKind::Server },
    ];
    let picked_id = match pick_endpoint(&eps, 1) {
        Some(e) => e.id,
        None => 0,
    };
    let far_id = match pick_endpoint(&eps, 5) {
        Some(e) => e.id,
        None => 0,
    };
    set.add("F118 affinity pick", picked_id == 3 && far_id == 1, "same-core first, else first");
    set.add(
        "F118 affinity score",
        affinity_score(0, 0) == IPC_AFFINITY_LOCAL_PERMILLE
            && affinity_score(0, 3) == IPC_AFFINITY_REMOTE_PERMILLE,
        "local vs remote",
    );

    // F119 批量合并投递
    let tags = [7u8, 7, 7, 9, 9, 3, 7, 1, 1, 1, 1];
    set.add("F119 coalesce distinct", coalesce_tags(&tags) == 4, "7,9,3,1");
    set.add("F119 coalesce worth", coalesce_worth(4) && !coalesce_worth(3), "batch at 4+");

    // F120 端点健康分
    let healthy = IpcHealth { ok: 7, fail: 3 };
    let degraded = IpcHealth { ok: 5, fail: 5 };
    set.add(
        "F120 health permille",
        health_permille(&healthy) == 700 && health_permille(&degraded) == 500,
        "ratio",
    );
    set.add(
        "F120 health degrade",
        !health_degraded(&healthy) && health_degraded(&degraded),
        "600 permille line",
    );

    // F121 消息血缘标签
    let l0 = IpcLineage { origin_domain: 12, trace_id: 77, hops: 0 };
    let l1 = lineage_propagate(&l0);
    let l2 = lineage_propagate(&l1);
    set.add(
        "F121 lineage keep",
        l1.origin_domain == 12 && l1.trace_id == 77 && l1.hops == 1 && l2.hops == 2,
        "origin preserved",
    );
    set.add(
        "F121 lineage rules",
        !hop_exceeded(&l2)
            && hop_exceeded(&IpcLineage { origin_domain: 1, trace_id: 1, hops: 17 })
            && !lineage_valid(&IpcLineage { origin_domain: 0, trace_id: 1, hops: 0 }),
        "hop cap, origin nonzero",
    );

    // F122 死信归档
    let mut dl = IpcDeadLetterBox::new();
    let da1 = dl.archive(11);
    let da2 = dl.archive(11);
    let da3 = dl.archive(12);
    let dl_count = dl.archived();
    set.add("F122 deadletter dedup", da1 && !da2 && da3 && dl_count == 2, "dedup by seq");
    let mut fdl = IpcDeadLetterBox::new();
    let mut m = 0u64;
    let mut m_ok = true;
    while m < IPC_DEADLETTER_CAP as u64 {
        m_ok &= fdl.archive(m);
        m += 1;
    }
    set.add(
        "F122 deadletter cap",
        m_ok && fdl.archived() == IPC_DEADLETTER_CAP && !fdl.archive(99),
        "cap 8",
    );
    dl.clear();
    let dl_after_clear = dl.archived();
    let dl_re = dl.archive(11);
    set.add("F122 deadletter clear", dl_after_clear == 0 && dl_re, "drained, re-archive ok");

    // F123 消息配额官
    let mut quota = IpcQuota::new(10);
    let g1 = quota.take(6);
    let left1 = quota.remaining();
    set.add("F123 quota grant", g1 == 6 && left1 == 4, "within quota");
    let g2 = quota.take(6);
    let left2 = quota.remaining();
    set.add("F123 quota clamp", g2 == 4 && left2 == 0, "no overdraft");
    let mut rq = IpcQuota::new(4);
    rq.used = 4;
    rq.refill(2);
    let left3 = rq.remaining();
    rq.refill(9);
    let left4 = rq.remaining();
    set.add("F123 quota refill", left3 == 2 && left4 == 4, "refill capped at granted");

    // F124 通道巡检机器人
    let mut log = IpcPatrolLog::default();
    log.record(inspect_channel(500, false));
    log.record(inspect_channel(750, false));
    log.record(inspect_channel(960, false));
    log.record(inspect_channel(100, true));
    set.add(
        "F124 patrol verdicts",
        log.ok == 1 && log.warn == 1 && log.fail == 2,
        "tiers counted",
    );
    set.add(
        "F124 patrol stale fails",
        inspect_channel(0, true) == IpcPatrolVerdict::Fail,
        "stale = fail",
    );

    // F125 IPC 域年报
    set.add("F125 report sections", IPC_REPORT_SECTIONS.len() == 5, "five sections");
    set.add("F125 report complete", ipc_report_complete(5) && !ipc_report_complete(4), "completeness");
    let (drained, depth) = ipc_golden_roundtrip();
    set.add("F125 ipc golden roundtrip", ipc_golden_matches(drained, depth), "scripted end state");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f101_registry_dedup_and_death() {
        let mut reg = IpcRegistry::new();
        assert!(reg.register(1, 0));
        assert!(reg.register(2, 1));
        assert!(!reg.register(1, 2));
        assert_eq!(reg.count(), 2);
        assert!(reg.mark_dead(2));
        assert!(!reg.is_alive(2));
        assert!(reg.is_alive(1));
        assert_eq!(reg.core_of(1), Some(0));
    }

    #[test]
    fn f107_backpressure_curve() {
        assert_eq!(backpressure(0), IpcBpAction::Pass);
        assert_eq!(backpressure(IPC_BP_LOW_PERMILLE), IpcBpAction::Pass);
        assert_eq!(backpressure(400), IpcBpAction::Slow(400));
        assert_eq!(backpressure(IPC_BP_HIGH_PERMILLE), IpcBpAction::Drop);
        assert_eq!(backpressure(1000), IpcBpAction::Drop);
    }

    #[test]
    fn f110_circuit_breaker_cycle() {
        let mut cb = IpcCircuitBreaker::new();
        for _ in 0..4 {
            assert_eq!(cb.record(false), IpcCircuitState::Closed);
        }
        assert_eq!(cb.record(false), IpcCircuitState::Open);
        assert_eq!(cb.record(true), IpcCircuitState::Open);
        assert_eq!(cb.record(false), IpcCircuitState::Open); // 探测期内失败重置
        assert_eq!(cb.record(true), IpcCircuitState::Open);
        assert_eq!(cb.record(true), IpcCircuitState::Open);
        assert_eq!(cb.record(true), IpcCircuitState::Closed);
    }

    #[test]
    fn f112_mailbox_fifo_and_wrap() {
        let mut mb = IpcMailbox::new();
        for v in 0..IPC_MAILBOX_CAP as u64 {
            assert!(mb.send(v));
        }
        assert!(!mb.send(99));
        assert_eq!(mb.recv(), Some(0));
        assert!(mb.send(100));
        let mut expect = 1u64;
        while expect < IPC_MAILBOX_CAP as u64 {
            assert_eq!(mb.recv(), Some(expect));
            expect += 1;
        }
        assert_eq!(mb.recv(), Some(100));
        assert!(mb.recv().is_none());
    }

    #[test]
    fn f113_replay_wall_window() {
        let mut w = IpcReplayWall::new();
        assert!(w.admit(5));
        assert!(!w.admit(5));
        assert!(w.admit(6));
        assert!(w.admit(4));
        assert!(!w.admit(4));
        // 推进 80 个新序号后，5 已滑出窗口
        let mut s = 7u64;
        while s < 90 {
            assert!(w.admit(s));
            s += 1;
        }
        assert!(!w.admit(5));
    }

    #[test]
    fn f123_quota_never_overdraft() {
        let mut q = IpcQuota::new(5);
        assert_eq!(q.take(9), 5);
        assert_eq!(q.take(1), 0);
        assert_eq!(q.remaining(), 0);
        q.refill(3);
        assert_eq!(q.take(4), 3);
    }

    #[test]
    fn f125_ipc_selfcheck_all_pass() {
        let set = run_m700ipc_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
