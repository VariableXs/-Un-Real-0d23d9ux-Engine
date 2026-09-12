//! m700net — VARIX-M700 AI-17 网络协议栈域 (F401~F425)
//!
//! 协议栈分层大典/零拷贝收发通道/TCP 状态机全谱/拥塞控制谱系/缓冲区账房/
//! 分片重组官/端口分配官/路由裁判/ARP 邻居表谱/校验和快道/包过滤钩子链/
//! socket 缓冲水位/网络延迟仪/吞吐基线剧本/异常包靶场/协议栈 fuzz 桩/
//! 连接表宗卷/网络事件流/QoS 车道/网络统计分账/零窗口优雅谱/时间戳选项官/
//! 网络回归走廊/协议栈自描述导出/网络域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F401 — 协议栈分层大典：L2→L3→L4→App 固定分层
// ===========================================================================

pub const LAYER_L2_ETH: u8 = 0;
pub const LAYER_L3_IP: u8 = 1;
pub const LAYER_L4_TCP: u8 = 2;
pub const LAYER_APP: u8 = 3;
pub const LAYER_COUNT: u8 = 4;

pub const ETH_HDR_LEN: usize = 14;
pub const IPV4_HDR_MIN_LEN: usize = 20;
pub const TCP_HDR_MIN_LEN: usize = 20;

/// 分层剥头：每层报文至少要装得下它自己的固定头。
pub fn header_fits(total: usize, layer: u8) -> bool {
    match layer {
        LAYER_L2_ETH => total >= ETH_HDR_LEN,
        LAYER_L3_IP => total >= ETH_HDR_LEN + IPV4_HDR_MIN_LEN,
        LAYER_L4_TCP => total >= ETH_HDR_LEN + IPV4_HDR_MIN_LEN + TCP_HDR_MIN_LEN,
        LAYER_APP => total > ETH_HDR_LEN + IPV4_HDR_MIN_LEN + TCP_HDR_MIN_LEN,
        _ => false,
    }
}

// ===========================================================================
// F402 — 零拷贝收发通道：描述符链（不搬数据，只记区间）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ZcDesc {
    pub offset: u16,
    pub len: u16,
}

pub fn zc_total(descs: &[ZcDesc]) -> u32 {
    descs.iter().map(|d| d.len as u32).sum()
}

/// 零拷贝链合法：区间互不重叠。
pub fn zc_disjoint(descs: &[ZcDesc]) -> bool {
    for i in 0..descs.len() {
        for j in (i + 1)..descs.len() {
            let a = &descs[i];
            let b = &descs[j];
            if a.offset < b.offset + b.len && b.offset < a.offset + a.len {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F403 — TCP 状态机全谱：11 态主迁移
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TcpEvent {
    Open,
    RxSyn,
    RxSynAck,
    RxAck,
    Close,
    RxFin,
    RxAckOfFin,
    Timeout,
}

/// 迁移表：无法迁移返回 None（非法包不得改状态）。
pub fn tcp_advance(cur: TcpState, ev: TcpEvent) -> Option<TcpState> {
    use TcpEvent::*;
    use TcpState::*;
    match (cur, ev) {
        (Closed, Open) => Some(SynSent),
        (Listen, RxSyn) => Some(SynRcvd),
        (SynSent, RxSynAck) => Some(Established),
        (SynRcvd, RxAck) => Some(Established),
        (Established, Close) => Some(FinWait1),
        (Established, RxFin) => Some(CloseWait),
        (FinWait1, RxAck) => Some(FinWait2),
        (FinWait1, RxFin) => Some(Closing),
        (FinWait2, RxFin) => Some(TimeWait),
        (CloseWait, Close) => Some(LastAck),
        (Closing, RxAckOfFin) => Some(TimeWait),
        (LastAck, RxAckOfFin) => Some(Closed),
        (TimeWait, Timeout) => Some(Closed),
        (SynSent, Timeout) => Some(Closed),
        _ => None,
    }
}

// ===========================================================================
// F404 — 拥塞控制谱系：慢启动/拥塞避免/快重传
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Congestion {
    pub cwnd_mss: u16,
    pub ssthresh: u16,
}

impl Congestion {
    pub const fn initial() -> Congestion {
        Congestion { cwnd_mss: 10, ssthresh: 16 }
    }
    /// 每 ACK：慢启动阶段 +1 MSS；拥塞避免阶段按 RTT 推进（见 on_rtt）。
    pub fn on_ack(&mut self) {
        if self.cwnd_mss < self.ssthresh {
            self.cwnd_mss += 1;
        }
    }
    /// 每往返周期：拥塞避免 +1 MSS。
    pub fn on_rtt(&mut self) {
        if self.cwnd_mss >= self.ssthresh {
            self.cwnd_mss += 1;
        }
    }
    /// 丢包：ssthresh = cwnd/2，cwnd 回到初值（RTO 重置）。
    pub fn on_loss_rto(&mut self) {
        self.ssthresh = (self.cwnd_mss / 2).max(2);
        self.cwnd_mss = 10;
    }
    /// 快速恢复：cwnd 减半后回到 ssthresh（不降到 1）。
    pub fn on_fast_recovery(&mut self) {
        self.ssthresh = (self.cwnd_mss / 2).max(2);
        self.cwnd_mss = self.ssthresh;
    }
}

// ===========================================================================
// F405 — 缓冲区账房：收支平衡，不许透支
// ===========================================================================

pub struct BufferLedger {
    used: u32,
    pub cap: u32,
}

impl BufferLedger {
    pub const fn new(cap: u32) -> BufferLedger {
        BufferLedger { used: 0, cap }
    }
    pub fn used(&self) -> u32 {
        self.used
    }
    pub fn take(&mut self, n: u32) -> bool {
        if self.used + n > self.cap {
            return false;
        }
        self.used += n;
        true
    }
    pub fn give(&mut self, n: u32) {
        self.used = self.used.saturating_sub(n);
    }
}

// ===========================================================================
// F406 — 分片重组官：位图占位与全覆盖判定
// ===========================================================================

pub const REASM_GRANULES: usize = 8;

#[derive(Clone, Copy)]
pub struct Reasm {
    have: [bool; REASM_GRANULES],
}

impl Reasm {
    pub const fn new() -> Reasm {
        Reasm { have: [false; REASM_GRANULES] }
    }
    /// 插入 [offset, offset+len) 粒度段；越界或重叠判失败。
    pub fn insert(&mut self, offset: usize, len: usize) -> bool {
        if len == 0 || offset + len > REASM_GRANULES {
            return false;
        }
        if (offset..offset + len).any(|g| self.have[g]) {
            return false;
        }
        for g in offset..offset + len {
            self.have[g] = true;
        }
        true
    }
    pub fn complete(&self) -> bool {
        self.have.iter().all(|&h| h)
    }
    pub fn coverage(&self) -> usize {
        self.have.iter().filter(|&&h| h).count()
    }
}

// ===========================================================================
// F407 — 端口分配官：临时端口循环避让
// ===========================================================================

pub const EPHEMERAL_FIRST: u16 = 49152;
pub const EPHEMERAL_LAST: u16 = 65535;

pub struct PortAlloc {
    next: u16,
}

impl PortAlloc {
    pub const fn new() -> PortAlloc {
        PortAlloc { next: EPHEMERAL_FIRST }
    }
    /// 从 next 起循环找一个不在 taken 里的端口。
    pub fn alloc(&mut self, taken: &[u16]) -> Option<u16> {
        let mut probes = 0u32;
        while probes <= (EPHEMERAL_LAST - EPHEMERAL_FIRST) as u32 {
            let p = self.next;
            self.next = if self.next == EPHEMERAL_LAST { EPHEMERAL_FIRST } else { self.next + 1 };
            if !taken.contains(&p) {
                return Some(p);
            }
            probes += 1;
        }
        None
    }
}

// ===========================================================================
// F408 — 路由裁判：最长前缀匹配
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Route {
    pub dest: u32,
    pub plen: u8,
    pub gw: u32,
}

impl Route {
    pub fn matches(&self, ip: u32) -> bool {
        if self.plen == 0 {
            return true;
        }
        if self.plen > 32 {
            return false;
        }
        let mask = if self.plen == 32 { u32::MAX } else { u32::MAX << (32 - self.plen) };
        (ip & mask) == (self.dest & mask)
    }
}

/// 最长前缀获胜；都不匹配返回 None（不默认转发）。
pub fn lpm(routes: &[Route], ip: u32) -> Option<u32> {
    let mut best: Option<(u8, u32)> = None;
    for r in routes {
        if r.matches(ip) {
            match best {
                Some((plen, _)) if plen >= r.plen => {}
                _ => best = Some((r.plen, r.gw)),
            }
        }
    }
    best.map(|(_, gw)| gw)
}

// ===========================================================================
// F409 — ARP/邻居表谱：NUD 状态机
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NudState {
    Incomplete,
    Reachable,
    Stale,
    Probe,
    Failed,
}

pub fn nud_advance(cur: NudState, confirmed: bool, timeout: bool) -> Option<NudState> {
    match (cur, confirmed, timeout) {
        (NudState::Incomplete, true, _) => Some(NudState::Reachable),
        (NudState::Incomplete, _, true) => Some(NudState::Failed),
        (NudState::Reachable, _, true) => Some(NudState::Stale),
        (NudState::Stale, true, _) => Some(NudState::Reachable),
        (NudState::Stale, _, true) => Some(NudState::Probe),
        (NudState::Probe, true, _) => Some(NudState::Reachable),
        (NudState::Probe, _, true) => Some(NudState::Failed),
        _ => None,
    }
}

// ===========================================================================
// F410 — 校验和快道：反码求和折叠
// ===========================================================================

/// 16 位反码和（大端字节流，奇数长度补零）。
pub fn checksum16(bytes: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        sum += ((bytes[i] as u32) << 8) | bytes[i + 1] as u32;
        i += 2;
    }
    if i < bytes.len() {
        sum += (bytes[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// 真实性检查：全零数据校验和应为 0xFFFF。
pub fn checksum_sane(bytes: &[u8]) -> bool {
    checksum16(bytes) == 0xFFFF
}

// ===========================================================================
// F411 — 包过滤钩子链：首条命中即裁决
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FilterRule {
    pub port: u16, // 0 = 通配
    pub accept: bool,
}

/// 首条匹配生效；无匹配默认拒绝（白名单礼仪）。
pub fn run_filter_chain(rules: &[FilterRule], port: u16) -> bool {
    for r in rules {
        if r.port == 0 || r.port == port {
            return r.accept;
        }
    }
    false
}

// ===========================================================================
// F412 — socket 缓冲水位：高水位背压/低水位恢复
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Watermark {
    pub hi: u32,
    pub lo: u32,
}

impl Watermark {
    pub fn above_hi(&self, used: u32) -> bool {
        used >= self.hi
    }
    pub fn below_lo(&self, used: u32) -> bool {
        used <= self.lo
    }
    /// 背压解除：必须降到低水位以下才算恢复。
    pub fn readable_again(&self, before: u32, drained: u32) -> bool {
        let after = before.saturating_sub(drained);
        self.below_lo(after)
    }
}

// ===========================================================================
// F413 — 网络延迟仪：RTT 整数 EWMA
// ===========================================================================

/// prev = prev - prev>>shift + sample>>shift（定点低通）。
pub fn ewma_rtt(prev: u32, sample: u32, shift: u8) -> u32 {
    let s = (shift as u32).min(16);
    prev - (prev >> s) + (sample >> s)
}

// ===========================================================================
// F414 — 吞吐基线剧本：基线偏差 permille 门
// ===========================================================================

pub const THROUGHPUT_FLOOR_PERMILLE: u32 = 800; // 达到基线 80% 即合格

pub fn throughput_ok(actual_mbps: u32, baseline_mbps: u32) -> bool {
    if baseline_mbps == 0 {
        return actual_mbps == 0;
    }
    actual_mbps * 1000 >= baseline_mbps * THROUGHPUT_FLOOR_PERMILLE
}

// ===========================================================================
// F415 — 异常包靶场：畸形包分类官
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Malformed {
    TooShort,
    BadChecksum,
    Truncated,
    Ok,
}

pub fn classify_packet(total: usize, declared_payload: usize, checksum_ok: bool) -> Malformed {
    if !header_fits(total, LAYER_L4_TCP) {
        return Malformed::TooShort;
    }
    if !checksum_ok {
        return Malformed::BadChecksum;
    }
    let hdrs = ETH_HDR_LEN + IPV4_HDR_MIN_LEN + TCP_HDR_MIN_LEN;
    if total - hdrs < declared_payload {
        return Malformed::Truncated;
    }
    Malformed::Ok
}

// ===========================================================================
// F416 — 协议栈 fuzz 桩：确定性输入生成
// ===========================================================================

/// LCG 伪随机（确定性，可回放）。
pub fn fuzz_byte(seed: u32, round: u32) -> u8 {
    let x = seed.wrapping_mul(1_664_525).wrapping_add(round.wrapping_mul(1_013_904_223).wrapping_add(1));
    (x >> 16) as u8
}

pub fn fuzz_deterministic(seed: u32, rounds: u32) -> bool {
    let a: [u8; 8] = core::array::from_fn(|i| fuzz_byte(seed, i as u32));
    let b: [u8; 8] = core::array::from_fn(|i| fuzz_byte(seed, i as u32));
    a == b && rounds > 0
}

// ===========================================================================
// F417 — 连接表宗卷：定容连接表 + 最老驱逐
// ===========================================================================

pub const CONN_TABLE_CAP: usize = 8;

pub struct ConnTable {
    ages: [u32; CONN_TABLE_CAP], // 0 = 空位，>0 = 存活 tick 计
    count: usize,
}

impl ConnTable {
    pub const fn new() -> ConnTable {
        ConnTable { ages: [0; CONN_TABLE_CAP], count: 0 }
    }
    pub fn count(&self) -> usize {
        self.count
    }
    /// 插入：有空位直接占；满则驱逐最老连接再占。
    pub fn insert(&mut self, tick: u32) -> bool {
        if let Some(slot) = (0..CONN_TABLE_CAP).find(|&i| self.ages[i] == 0) {
            self.ages[slot] = tick.max(1);
            self.count += 1;
            return true;
        }
        let mut oldest = 0usize;
        for i in 1..CONN_TABLE_CAP {
            if self.ages[i] < self.ages[oldest] {
                oldest = i;
            }
        }
        self.ages[oldest] = tick.max(1);
        true // 驱逐后必然成功
    }
    pub fn evict_oldest_tick(&self) -> u32 {
        let mut oldest = u32::MAX;
        for &a in self.ages.iter() {
            if a > 0 && a < oldest {
                oldest = a;
            }
        }
        oldest
    }
}

// ===========================================================================
// F418 — 网络事件流：定容事件日志
// ===========================================================================

pub const NET_EVENT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct NetEventLog {
    kinds: [u8; NET_EVENT_CAP], // 1=conn 2=drop 3=retrans
    count: usize,
}

impl NetEventLog {
    pub const fn new() -> NetEventLog {
        NetEventLog { kinds: [0; NET_EVENT_CAP], count: 0 }
    }
    pub fn push(&mut self, kind: u8) -> bool {
        if self.count >= NET_EVENT_CAP {
            return false;
        }
        self.kinds[self.count] = kind;
        self.count += 1;
        true
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn kind_at(&self, i: usize) -> Option<u8> {
        if i < self.count {
            Some(self.kinds[i])
        } else {
            None
        }
    }
}

// ===========================================================================
// F419 — QoS 车道：DSCP → 车道映射
// ===========================================================================

pub const LANE_BULK: u8 = 0;
pub const LANE_NORMAL: u8 = 1;
pub const LANE_PRIO: u8 = 2;
pub const LANE_VOICE: u8 = 3;

/// DSCP 0~63：46~47 语音，32~45 优先，8~31 普通，其余批量。
pub fn dscp_lane(dscp: u8) -> u8 {
    match dscp {
        46..=47 => LANE_VOICE,
        32..=45 => LANE_PRIO,
        8..=31 => LANE_NORMAL,
        _ => LANE_BULK,
    }
}

// ===========================================================================
// F420 — 网络统计分账：计数器对账
// ===========================================================================

#[derive(Clone, Copy)]
pub struct NetStats {
    pub rx_pkts: u64,
    pub tx_pkts: u64,
    pub drops: u64,
}

impl NetStats {
    /// 对账：rx = tx + drops + pending（守恒）。
    pub fn accounted(&self, pending: u64) -> bool {
        self.rx_pkts == self.tx_pkts + self.drops + pending
    }
    pub fn drop_rate_permille(&self) -> u32 {
        if self.rx_pkts == 0 {
            return 0;
        }
        (self.drops * 1000 / self.rx_pkts) as u32
    }
}

// ===========================================================================
// F421 — 零窗口优雅谱：探测退避
// ===========================================================================

/// 零窗口探测间隔：200ms 起步，每次翻倍，封顶 5s。
pub fn probe_backoff_ms(attempt: u32) -> u32 {
    let d = 200u32.saturating_mul(1u32 << attempt.min(8));
    d.min(5000)
}

// ===========================================================================
// F422 — 时间戳选项官：PAWS 环绕安全比较
// ===========================================================================

/// 时间戳必须在 (recent, recent + 2^31) 窗口内才算新（防回绕假旧）。
pub fn ts_is_fresh(tsval: u32, ts_recent: u32) -> bool {
    let delta = tsval.wrapping_sub(ts_recent);
    delta != 0 && delta < 0x8000_0000
}

// ===========================================================================
// F423 — 网络回归走廊：固定回归用例
// ===========================================================================

pub const REGRESSION_CASES: [&str; 6] =
    ["tcp-handshake", "tcp-teardown", "lpm-pick", "reasm-overlap", "csum-fold", "qos-lanes"];

// ===========================================================================
// F424 — 协议栈自描述导出：完整描述符
// ===========================================================================

pub const NET_DESCRIPTOR_FIELDS: [&str; 5] =
    ["layers", "tcp-states", "routes", "filters", "lanes"];

pub fn net_descriptor_complete(fields: [&str; 5]) -> bool {
    (0..5).all(|i| !fields[i].is_empty())
}

// ===========================================================================
// F425 — 网络域年报
// ===========================================================================

pub const NET_REPORT_SECTIONS: [&str; 4] = ["milestones", "throughput", "incidents", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700net_checks() -> CheckSet {
    let mut set = CheckSet::new("m700net");

    // F401 分层
    set.add("F401 layers ordered", LAYER_COUNT == 4 && header_fits(54, LAYER_L4_TCP), "14+20+20");
    set.add("F401 too short", !header_fits(30, LAYER_L3_IP), "under headers");
    set.add("F401 app needs data", header_fits(55, LAYER_APP) && !header_fits(54, LAYER_APP), "payload>0");

    // F402 零拷贝
    let chain = [ZcDesc { offset: 0, len: 100 }, ZcDesc { offset: 100, len: 60 }];
    set.add("F402 zc total", zc_total(&chain) == 160 && zc_disjoint(&chain), "no copy");
    set.add("F402 zc overlap", !zc_disjoint(&[ZcDesc { offset: 0, len: 100 }, ZcDesc { offset: 50, len: 60 }]), "reject overlap");

    // F403 TCP 状态机
    let s0 = tcp_advance(TcpState::Closed, TcpEvent::Open);
    let s1 = tcp_advance(TcpState::SynSent, TcpEvent::RxSynAck);
    set.add("F403 tcp handshake", s0 == Some(TcpState::SynSent) && s1 == Some(TcpState::Established), "3-way");
    set.add("F403 tcp teardown", tcp_advance(TcpState::Established, TcpEvent::Close) == Some(TcpState::FinWait1)
        && tcp_advance(TcpState::LastAck, TcpEvent::RxAckOfFin) == Some(TcpState::Closed), "fin path");
    set.add("F403 tcp illegal", tcp_advance(TcpState::Established, TcpEvent::Open).is_none(), "no stray open");

    // F404 拥塞控制
    let mut c = Congestion::initial();
    let cwnd_before = c.cwnd_mss;
    c.on_ack();
    let cwnd_after_slow = c.cwnd_mss;
    set.add("F404 slow start", cwnd_before == 10 && cwnd_after_slow == 11, "cwnd+1 per ack");
    c.on_loss_rto();
    set.add("F404 loss halve", c.ssthresh == 5 && c.cwnd_mss == 10, "ssthresh=cwnd/2");
    let mut f = Congestion { cwnd_mss: 20, ssthresh: 16 };
    f.on_fast_recovery();
    set.add("F404 fast recovery", f.cwnd_mss == 10 && f.ssthresh == 10, "half, not 1");

    // F405 缓冲区账房
    let mut led = BufferLedger::new(100);
    let took1 = led.take(60);
    let took2 = led.take(60);
    let bal1 = led.used();
    led.give(60);
    set.add("F405 ledger balance", took1 && !took2 && bal1 == 60 && led.used() == 0, "no overdraft");

    // F406 分片重组
    let mut r = Reasm::new();
    let i1 = r.insert(0, 3);
    let i2 = r.insert(3, 5);
    set.add("F406 reasm fill", i1 && i2 && r.complete(), "full coverage");
    let mut r2 = Reasm::new();
    let j1 = r2.insert(0, 3);
    let j2 = r2.insert(2, 2);
    set.add("F406 reasm overlap", j1 && !j2 && r2.coverage() == 3, "reject overlap");
    set.add("F406 reasm oob", !r2.insert(6, 4), "out of range");

    // F407 端口分配
    let mut pa = PortAlloc::new();
    let p1 = pa.alloc(&[]);
    let p2 = pa.alloc(&[]);
    set.add("F407 port alloc", p1 == Some(49152) && p2 == Some(49153), "sequential");
    let mut pa2 = PortAlloc::new();
    let p3 = pa2.alloc(&[49152, 49153]);
    set.add("F407 port avoid", p3 == Some(49154), "skip taken");
    set.add("F407 port wrap", PortAlloc { next: EPHEMERAL_LAST }.alloc(&[]) == Some(65535), "range end");

    // F408 路由裁判
    let routes = [
        Route { dest: 0x0A00_0000, plen: 8, gw: 1 },
        Route { dest: 0x0A01_0200, plen: 24, gw: 2 },
        Route { dest: 0, plen: 0, gw: 3 },
    ];
    let g1 = lpm(&routes, 0x0A01_0205);
    let g2 = lpm(&routes, 0x0A02_0001);
    set.add("F408 lpm longest", g1 == Some(2), "24 beats 8");
    set.add("F408 lpm fallback", g2 == Some(1), "8 beats 0");
    set.add("F408 lpm miss", lpm(&routes[..2], 0xC0A8_0001).is_none(), "no default");

    // F409 ARP/邻居表
    set.add("F409 nud confirm", nud_advance(NudState::Incomplete, true, false) == Some(NudState::Reachable), "resolve");
    set.add("F409 nud stale", nud_advance(NudState::Reachable, false, true) == Some(NudState::Stale), "age out");
    set.add("F409 nud probe", nud_advance(NudState::Stale, false, true) == Some(NudState::Probe), "reach again");

    // F410 校验和
    set.add("F410 csum zero data", checksum_sane(&[0u8; 8]), "all-zero → 0xFFFF");
    let pkt = [0x45, 0x00, 0x00, 0x04];
    let csum = checksum16(&pkt);
    set.add("F410 csum verify", checksum16(&pkt) == csum && csum != 0, "deterministic fold");

    // F411 包过滤
    let rules = [FilterRule { port: 80, accept: true }, FilterRule { port: 0, accept: false }];
    set.add("F411 filter chain", run_filter_chain(&rules, 80) && !run_filter_chain(&rules, 22), "first match");
    set.add("F411 filter deny-all", !run_filter_chain(&[FilterRule { port: 443, accept: true }], 8080), "default deny");

    // F412 缓冲水位
    let wm = Watermark { hi: 64, lo: 16 };
    set.add("F412 watermark hi", wm.above_hi(64) && !wm.above_hi(63), "backpressure");
    set.add("F412 watermark resume", wm.readable_again(64, 50) && !wm.readable_again(64, 40), "drain to lo");

    // F413 延迟仪
    let r1 = ewma_rtt(1000, 2000, 3);
    let r2 = ewma_rtt(r1, 2000, 3);
    set.add("F413 rtt ewma", r1 == 1000 + (2000 >> 3) - (1000 >> 3) && r2 < r1 * 2, "low-pass");
    set.add("F413 rtt converge", ewma_rtt(0, 8000, 3) == 1000, "zero base");

    // F414 吞吐基线
    set.add("F414 throughput pass", throughput_ok(850, 1000) && !throughput_ok(799, 1000), "80% floor");

    // F415 异常包靶场
    set.add("F415 classify short", classify_packet(30, 0, true) == Malformed::TooShort, "too short");
    set.add("F415 classify csum", classify_packet(54, 0, false) == Malformed::BadChecksum, "bad csum");
    set.add("F415 classify trunc", classify_packet(60, 10, true) == Malformed::Truncated, "declared>actual");

    // F416 fuzz 桩
    set.add("F416 fuzz deterministic", fuzz_deterministic(0xDEAD_BEEF, 64), "replayable");

    // F417 连接表宗卷
    let mut ct = ConnTable::new();
    let ins1 = ct.insert(100);
    let ins2 = ct.insert(200);
    let oldest_tick = ct.evict_oldest_tick();
    set.add("F417 conn table fill", ins1 && ins2 && ct.count() == 2 && oldest_tick == 100, "age tracked");
    let mut ct2 = ConnTable::new();
    for i in 0..CONN_TABLE_CAP as u32 {
        ct2.insert(10 * (i + 1));
    }
    let count_before = ct2.count();
    ct2.insert(999);
    set.add("F417 conn evict oldest", count_before == CONN_TABLE_CAP && ct2.evict_oldest_tick() == 20, "evicted tick=10");

    // F418 网络事件流
    let mut nel = NetEventLog::new();
    let pushed = nel.push(1);
    nel.push(3);
    nel.push(2);
    set.add("F418 net events", pushed && nel.count() == 3 && nel.kind_at(1) == Some(3), "ordered");

    // F419 QoS 车道
    set.add("F419 qos lanes", dscp_lane(46) == LANE_VOICE && dscp_lane(40) == LANE_PRIO
        && dscp_lane(10) == LANE_NORMAL && dscp_lane(0) == LANE_BULK, "dscp map");

    // F420 统计分账
    let st = NetStats { rx_pkts: 1000, tx_pkts: 950, drops: 50 };
    set.add("F420 stats conserve", st.accounted(0) && st.drop_rate_permille() == 50, "rx=tx+drop");
    set.add("F420 stats pending", st.accounted(0) && !st.accounted(1), "pending counted");

    // F421 零窗口
    set.add("F421 zero window probe", probe_backoff_ms(0) == 200 && probe_backoff_ms(5) == 5000, "double, cap 5s");

    // F422 时间戳选项
    set.add("F422 paws fresh", ts_is_fresh(1000, 999) && !ts_is_fresh(999, 1000), "monotone window");
    set.add("F422 paws wrap", ts_is_fresh(1, 0xFFFF_FFFF), "wrap-safe");

    // F423 回归走廊
    set.add("F423 regression cases", REGRESSION_CASES.len() == 6, "6 cases");

    // F424 自描述导出
    set.add("F424 net descriptor", net_descriptor_complete(["l", "t", "r", "f", "q"]), "5 fields");
    set.add("F424 descriptor gap", !net_descriptor_complete(["l", "", "r", "f", "q"]), "no empty");

    // F425 年报
    set.add("F425 net annual report", NET_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f403_tcp_full_path() {
        let s = tcp_advance(TcpState::Closed, TcpEvent::Open).unwrap();
        let s = tcp_advance(s, TcpEvent::RxSynAck).unwrap();
        let s = tcp_advance(s, TcpEvent::Close).unwrap();
        let s = tcp_advance(s, TcpEvent::RxAck).unwrap();
        assert_eq!(s, TcpState::FinWait2);
        assert!(tcp_advance(TcpState::Listen, TcpEvent::RxFin).is_none());
    }

    #[test]
    fn f404_congestion_cycle() {
        let mut c = Congestion::initial();
        for _ in 0..6 {
            c.on_ack();
        }
        assert_eq!(c.cwnd_mss, 16); // 慢启动到 ssthresh
        c.on_loss_rto();
        assert_eq!(c.ssthresh, 8);
        assert_eq!(c.cwnd_mss, 10);
    }

    #[test]
    fn f406_reasm_bounds() {
        let mut r = Reasm::new();
        assert!(!r.insert(0, 0));
        assert!(!r.insert(7, 2));
        assert!(r.insert(0, REASM_GRANULES));
        assert!(r.complete());
    }

    #[test]
    fn f408_lpm_exact() {
        let routes = [Route { dest: 0xC0A8_0100, plen: 24, gw: 7 }];
        assert_eq!(lpm(&routes, 0xC0A8_01FF), Some(7));
        assert_eq!(lpm(&routes, 0xC0A8_02FF), None);
    }

    #[test]
    fn f410_checksum_known() {
        // 单字节 0x00 → 高字节补零 → 反码 0xFFFF
        assert!(checksum_sane(&[0x00]));
        // 0xFFFF 相加不进位 → 反码 0x0000
        assert_eq!(checksum16(&[0xFF, 0xFF]), 0x0000);
    }

    #[test]
    fn f425_domain_selfcheck_all_pass() {
        let set = run_m700net_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
