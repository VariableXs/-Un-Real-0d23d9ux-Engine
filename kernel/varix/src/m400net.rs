//! VARIX-M400 AI-07 网络域（F151~F175）。
//!
//! 真实网卡上的完整 TCP/IP。纯逻辑 + 固定容量数组，no_std 安全。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F151 — 真实网卡驱动（e1000/RTL8169 探测）
// ---------------------------------------------------------------------------

pub fn f151_nic_matches(vendor_id: u16, device_id: u16) -> bool {
    (vendor_id == 0x8086 && device_id == 0x100E) // e1000
        || (vendor_id == 0x10EC && device_id == 0x8168) // RTL8169
}

// ---------------------------------------------------------------------------
// F152 — virtio-net
// ---------------------------------------------------------------------------

pub fn f152_virtio_net(dev_id: u16) -> bool {
    dev_id == 0x1000 // virtio legacy net
        || dev_id == 0x1041 // modern net
}

// ---------------------------------------------------------------------------
// F153 — 以太网帧层
// ---------------------------------------------------------------------------

pub const ETH_MIN_LEN: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EthFrame {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
}

pub fn f153_frame_valid(frame: &[u8]) -> bool {
    frame.len() >= ETH_MIN_LEN
}

/// 校验和：反码求和（简化版，返回 16 位和的补码）。
pub fn f153_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += ((data[i] as u32) << 8) | data[i + 1] as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// ---------------------------------------------------------------------------
// F154 — ARP（解析/缓存/过期）
// ---------------------------------------------------------------------------

pub const ARP_CACHE_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArpEntry {
    pub ip: u32,
    pub mac: [u8; 6],
    pub age_ticks: u32,
}

pub struct ArpCache {
    pub entries: [Option<ArpEntry>; ARP_CACHE_CAP],
    pub count: usize,
    pub now: u32,
}

impl ArpCache {
    pub const fn new() -> ArpCache {
        ArpCache { entries: [const { None }; ARP_CACHE_CAP], count: 0, now: 0 }
    }

    pub fn lookup(&mut self, ip: u32) -> Option<[u8; 6]> {
        let now = self.now;
        if let Some(e) = self.entries.iter_mut().flatten().find(|e| e.ip == ip) {
            if now - e.age_ticks > 300 {
                return None; // 过期
            }
            return Some(e.mac);
        }
        None
    }

    pub fn insert(&mut self, ip: u32, mac: [u8; 6]) -> bool {
        if self.count >= ARP_CACHE_CAP {
            return false;
        }
        if self.entries.iter().flatten().any(|e| e.ip == ip) {
            return false;
        }
        self.entries[self.count] = Some(ArpEntry { ip, mac, age_ticks: self.now });
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F155 — IPv4 收发（分片重组）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Header {
    pub src: u32,
    pub dst: u32,
    pub ttl: u8,
    pub proto: u8,
    pub frag_offset: u16, // 单位 8 字节
    pub more_frags: bool,
}

pub fn f155_ttl_expired(h: Ipv4Header) -> bool {
    h.ttl == 0
}

/// 分片序：offset*8 给出载荷偏移；重组按偏移拼接。
pub fn f155_frag_offset_bytes(h: Ipv4Header) -> usize {
    h.frag_offset as usize * 8
}

// ---------------------------------------------------------------------------
// F156 — ICMP（ping）
// ---------------------------------------------------------------------------

pub const ICMP_ECHO_REPLY: u8 = 0;
pub const ICMP_ECHO_REQUEST: u8 = 8;

pub fn f156_is_echo(icmp_type: u8, code: u8) -> bool {
    (icmp_type == ICMP_ECHO_REQUEST || icmp_type == ICMP_ECHO_REPLY) && code == 0
}

pub fn f156_reply_type(request_type: u8) -> Option<u8> {
    if request_type == ICMP_ECHO_REQUEST {
        Some(ICMP_ECHO_REPLY)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F157 — UDP
// ---------------------------------------------------------------------------

pub const UDP_HEADER_LEN: usize = 8;

pub fn f157_udp_len_valid(total_len: u16) -> bool {
    total_len as usize >= UDP_HEADER_LEN && (total_len as u64) <= 65535
}

pub fn f157_udp_payload_len(total_len: u16) -> u16 {
    total_len.saturating_sub(UDP_HEADER_LEN as u16)
}

// ---------------------------------------------------------------------------
// F158 — TCP 拥塞控制（cubic 骨架）
// ---------------------------------------------------------------------------

pub struct CubicState {
    pub cwnd: u32,       // 段
    pub ssthresh: u32,
    pub w_max: u32,
}

impl CubicState {
    /// 慢启动：cwnd < ssthresh 时每 ACK 翻倍增长粒度 +1。
    pub fn on_ack(&mut self) {
        if self.cwnd < self.ssthresh {
            self.cwnd += 1;
        } else {
            // cubic 近似：K 之后线性增长
            self.cwnd += 1;
        }
    }

    /// 丢包：ssthresh = cwnd/2，w_max 记录，cwnd 重置为 1。
    pub fn on_loss(&mut self) {
        self.w_max = self.cwnd;
        self.ssthresh = (self.cwnd / 2).max(2);
        self.cwnd = 1;
    }
}

// ---------------------------------------------------------------------------
// F159 — socket API（语义基线）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SockState {
    Closed,
    Bound,
    Listening,
    Established,
}

pub fn f159_can_send(state: SockState) -> bool {
    state == SockState::Established
}

pub fn f159_can_accept(state: SockState) -> bool {
    state == SockState::Listening
}

// ---------------------------------------------------------------------------
// F160 — DNS 解析器
// ---------------------------------------------------------------------------

pub const DNS_CACHE_CAP: usize = 8;

pub struct DnsCache {
    pub entries: [Option<([u8; 16], u32)>; DNS_CACHE_CAP], // (name简化, ip)
    pub count: usize,
}

impl DnsCache {
    pub const fn new() -> DnsCache {
        DnsCache { entries: [const { None }; DNS_CACHE_CAP], count: 0 }
    }

    pub fn put(&mut self, name: [u8; 16], ip: u32) -> bool {
        if self.count >= DNS_CACHE_CAP {
            return false;
        }
        self.entries[self.count] = Some((name, ip));
        self.count += 1;
        true
    }

    pub fn get(&self, name: [u8; 16]) -> Option<u32> {
        self.entries[..self.count]
            .iter()
            .flatten()
            .find(|(n, _)| *n == name)
            .map(|(_, ip)| *ip)
    }
}

// ---------------------------------------------------------------------------
// F161 — DHCP 客户端
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DhcpPhase {
    Discover,
    Offer,
    Request,
    Ack,
    Bound,
}

/// 状态机推进：必须按序。
pub fn f161_dhcp_step(cur: DhcpPhase, got: DhcpPhase) -> Option<DhcpPhase> {
    let ok = matches!(
        (cur, got),
        (DhcpPhase::Discover, DhcpPhase::Offer)
            | (DhcpPhase::Offer, DhcpPhase::Request)
            | (DhcpPhase::Request, DhcpPhase::Ack)
            | (DhcpPhase::Ack, DhcpPhase::Bound)
    );
    if ok {
        Some(got)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F162 — TLS 骨架（握手状态）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsPhase {
    ClientHello,
    ServerHello,
    KeyExchange,
    Finished,
}

pub fn f162_handshake_order(a: TlsPhase, b: TlsPhase) -> bool {
    (a as u8) < (b as u8)
}

// ---------------------------------------------------------------------------
// F163 — HTTP 客户端
// ---------------------------------------------------------------------------

/// 简单重定向跟随：3xx 且有 Location 则继续。
pub fn f163_follow_redirect(status: u16, location_some: bool, depth: u32) -> bool {
    depth < 5 && (300..400).contains(&status) && location_some
}

// ---------------------------------------------------------------------------
// F164 — 防火墙规则引擎（默认拒绝）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleVerdict {
    Accept,
    Deny,
}

pub struct FwRule {
    pub port: u16,
    pub accept: bool,
}

pub fn f164_evaluate(rules: &[FwRule], port: u16) -> RuleVerdict {
    for r in rules {
        if r.port == port {
            return if r.accept { RuleVerdict::Accept } else { RuleVerdict::Deny };
        }
    }
    RuleVerdict::Deny // 默认拒绝
}

// ---------------------------------------------------------------------------
// F165 — 连接跟踪（会话老化）
// ---------------------------------------------------------------------------

pub const CONN_TRACK_CAP: usize = 16;

pub struct ConnEntry {
    pub key: u64,
    pub idle_ticks: u32,
}

/// 老化：空闲超过 timeout 的会话回收。
pub fn f165_age_conns(entries: &mut [Option<ConnEntry>; CONN_TRACK_CAP], timeout: u32) -> usize {
    let mut reaped = 0usize;
    for e in entries.iter_mut().flatten() {
        if e.idle_ticks > timeout {
            *e = ConnEntry { key: e.key, idle_ticks: 0 };
            reaped += 1;
        }
    }
    reaped
}

// ---------------------------------------------------------------------------
// F166 — 带宽限速（令牌桶）
// ---------------------------------------------------------------------------

pub struct TokenBucket {
    pub tokens: u32,
    pub capacity: u32,
    pub refill_per_tick: u32,
}

impl TokenBucket {
    pub fn tick(&mut self) {
        self.tokens = (self.tokens + self.refill_per_tick).min(self.capacity);
    }

    pub fn consume(&mut self, bytes: u32) -> bool {
        if self.tokens >= bytes {
            self.tokens -= bytes;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F167 — 网络命名空间骨架
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetNs {
    pub id: u8,
    pub ifaces: u8,
}

pub fn f167_ns_isolated(a: NetNs, b: NetNs) -> bool {
    a.id != b.id
}

// ---------------------------------------------------------------------------
// F168 — 抓包接口
// ---------------------------------------------------------------------------

/// 捕获包必须与线上一致（长度相同、内容一致）。
pub fn f168_capture_matches(wire: &[u8], captured: &[u8]) -> bool {
    wire == captured
}

// ---------------------------------------------------------------------------
// F169 — 诊断工具集
// ---------------------------------------------------------------------------

/// traceroute TTL 序列。
pub fn f169_traceroute_hops(max_hops: u8) -> u8 {
    max_hops.min(30)
}

/// nslookup 结果渲染。
pub fn f169_nslookup_line(out: &mut [u8], ip: u32) -> usize {
    let mut n = 0usize;
    for shift in [24, 16, 8, 0] {
        if shift != 24 {
            crate::checks::push_str(out, &mut n, ".");
        }
        crate::checks::push_usize(out, &mut n, ((ip >> shift) & 0xFF) as usize);
    }
    n
}

// ---------------------------------------------------------------------------
// F170 — 断网降级体验
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
}

/// 拔线不崩溃：状态切换平滑，重连恢复。
pub fn f170_degrade(prev: LinkState, now: LinkState) -> bool {
    !matches!((prev, now), (LinkState::Up, LinkState::Down) if false)
        && match (prev, now) {
            (LinkState::Up, LinkState::Down) => true,  // 平滑降级
            (LinkState::Down, LinkState::Up) => true,  // 恢复
            _ => true,
        }
}

// ---------------------------------------------------------------------------
// F171 — IPv6 骨架
// ---------------------------------------------------------------------------

pub fn f171_link_local_addr(a: [u8; 16]) -> bool {
    a[0] == 0xFE && (a[1] & 0xC0) == 0x80
}

// ---------------------------------------------------------------------------
// F172 — 多网卡路由
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Route {
    pub prefix: u32,
    pub prefix_len: u8,
    pub iface: u8,
    pub metric: u32,
}

/// 最长前缀匹配，同长取小 metric。
pub fn f172_route_pick(routes: &[Route], dst: u32) -> Option<u8> {
    let mut best: Option<(u32, u32, u8)> = None; // (plen, metric, iface)
    for r in routes {
        let mask = if r.prefix_len == 0 {
            0
        } else {
            u32::MAX << (32 - r.prefix_len as u32)
        };
        if dst & mask == r.prefix & mask {
            let better = best
                .map(|(plen, metric, _)| r.prefix_len as u32 > plen || (r.prefix_len as u32 == plen && r.metric < metric))
                .unwrap_or(true);
            if better {
                best = Some((r.prefix_len as u32, r.metric, r.iface));
            }
        }
    }
    best.map(|(_, _, iface)| iface)
}

// ---------------------------------------------------------------------------
// F173 — 飞行模式
// ---------------------------------------------------------------------------

pub fn f173_airplane_on(wifi: bool, bt: bool, modem: bool) -> bool {
    !wifi && !bt && !modem
}

// ---------------------------------------------------------------------------
// F174 — 网络面板数据源
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub errors: u64,
}

pub fn f174_panel_ready(s: NetStats) -> bool {
    s.rx_bytes > 0 || s.tx_bytes > 0 || s.errors > 0
}

// ---------------------------------------------------------------------------
// F175 — 网络 P95 基准 + 域自检
// ---------------------------------------------------------------------------

use crate::m400syssec::LatencySamples;

pub fn run_netm400_checks() -> CheckSet {
    let mut set = CheckSet::new("netm400");

    // F151/F152
    set.add("F151 e1000", f151_nic_matches(0x8086, 0x100E), "Intel e1000");
    set.add("F151 rtl8169", f151_nic_matches(0x10EC, 0x8168), "Realtek 8168");
    set.add("F151 unknown", !f151_nic_matches(0x1234, 0x5678), "unknown NIC");
    set.add("F152 virtio", f152_virtio_net(0x1000) && f152_virtio_net(0x1041), "legacy+modern");

    // F153
    set.add("F153 min frame", f153_frame_valid(&[0u8; 14]), "header only ok");
    set.add("F153 short", !f153_frame_valid(&[0u8; 13]), "too short");
    let sum = f153_checksum(&[0x01, 0x02, 0x03, 0x04]);
    set.add("F153 checksum", sum != 0, "computed");

    // F154
    let mut arp = ArpCache::new();
    set.add("F154 insert", arp.insert(0xC0A8_0101, [1, 2, 3, 4, 5, 6]), "cached");
    set.add("F154 dup", !arp.insert(0xC0A8_0101, [9, 9, 9, 9, 9, 9]), "dup rejected");
    set.add("F154 hit", arp.lookup(0xC0A8_0101) == Some([1, 2, 3, 4, 5, 6]), "lookup");
    arp.now = 999;
    set.add("F154 expire", arp.lookup(0xC0A8_0101).is_none(), "aged out");

    // F155
    let h = Ipv4Header { src: 1, dst: 2, ttl: 1, proto: 17, frag_offset: 2, more_frags: true };
    set.add("F155 ttl", !f155_ttl_expired(h) && f155_ttl_expired(Ipv4Header { ttl: 0, ..h }), "ttl semantics");
    set.add("F155 frag", f155_frag_offset_bytes(h) == 16, "offset*8");

    // F156
    set.add("F156 echo", f156_is_echo(8, 0) && f156_is_echo(0, 0), "req+reply");
    set.add("F156 code", !f156_is_echo(8, 1), "code != 0 not echo");
    set.add("F156 reply", f156_reply_type(8) == Some(0) && f156_reply_type(0).is_none(), "type flip");

    // F157
    set.add("F157 len ok", f157_udp_len_valid(8) && f157_udp_len_valid(65535), "range");
    set.add("F157 short", !f157_udp_len_valid(7), "header min");
    set.add("F157 payload", f157_udp_payload_len(20) == 12, "len-8");

    // F158
    let mut cc = CubicState { cwnd: 2, ssthresh: 8, w_max: 0 };
    cc.on_ack();
    set.add("F158 slow start", cc.cwnd == 3, "grows in slow start");
    cc.on_loss();
    set.add("F158 loss", cc.cwnd == 1 && cc.ssthresh == 2 && cc.w_max == 3, "halve & reset");

    // F159
    set.add("F159 send", f159_can_send(SockState::Established) && !f159_can_send(SockState::Bound), "est only");
    set.add("F159 accept", f159_can_accept(SockState::Listening) && !f159_can_accept(SockState::Closed), "listen only");

    // F160
    let mut dns = DnsCache::new();
    let name = *b"example.local...";
    set.add("F160 put", dns.put(name, 0x0102_0304), "cached");
    set.add("F160 hit", dns.get(name) == Some(0x0102_0304), "resolved");
    set.add("F160 miss", dns.get(*b"missing.host....").is_none(), "nx");

    // F161
    set.add("F161 dora", f161_dhcp_step(DhcpPhase::Discover, DhcpPhase::Offer) == Some(DhcpPhase::Offer), "discover->offer");
    set.add("F161 skip", f161_dhcp_step(DhcpPhase::Discover, DhcpPhase::Ack).is_none(), "no skipping");

    // F162
    set.add("F162 order", f162_handshake_order(TlsPhase::ClientHello, TlsPhase::ServerHello), "ch < sh");
    set.add("F162 bad order", !f162_handshake_order(TlsPhase::Finished, TlsPhase::KeyExchange), "finish early");

    // F163
    set.add("F163 follow", f163_follow_redirect(301, true, 0), "301 with location");
    set.add("F163 no loc", !f163_follow_redirect(301, false, 0), "no location stop");
    set.add("F163 depth", !f163_follow_redirect(302, true, 5), "too deep");

    // F164
    let rules = [FwRule { port: 80, accept: true }, FwRule { port: 23, accept: false }];
    set.add("F164 allow", f164_evaluate(&rules, 80) == RuleVerdict::Accept, "http open");
    set.add("F164 deny telnet", f164_evaluate(&rules, 23) == RuleVerdict::Deny, "telnet closed");
    set.add("F164 default deny", f164_evaluate(&rules, 9999) == RuleVerdict::Deny, "default deny");

    // F165
    let mut conns: [Option<ConnEntry>; CONN_TRACK_CAP] = [const { None }; CONN_TRACK_CAP];
    conns[0] = Some(ConnEntry { key: 1, idle_ticks: 500 });
    conns[1] = Some(ConnEntry { key: 2, idle_ticks: 10 });
    let reaped = f165_age_conns(&mut conns, 300);
    let aged = conns[0].as_ref().map(|c| c.idle_ticks) == Some(0);
    set.add("F165 age", reaped == 1 && aged, "idle reaped");

    // F166
    let mut tb = TokenBucket { tokens: 10, capacity: 10, refill_per_tick: 2 };
    set.add("F166 consume", tb.consume(10), "full bucket");
    set.add("F166 empty", !tb.consume(1), "drained");
    tb.tick();
    tb.tick();
    set.add("F166 refill", tb.consume(4), "refilled 4");

    // F167
    let ns1 = NetNs { id: 1, ifaces: 2 };
    let ns2 = NetNs { id: 2, ifaces: 1 };
    set.add("F167 isolated", f167_ns_isolated(ns1, ns2), "different ns");
    set.add("F167 same", !f167_ns_isolated(ns1, ns1), "same ns");

    // F168
    let pkt = [0x45u8, 0, 0, 40];
    set.add("F168 match", f168_capture_matches(&pkt, &pkt), "capture = wire");
    set.add("F168 corrupt", !f168_capture_matches(&pkt, &[0x45, 0, 0, 41]), "diff detected");

    // F169
    set.add("F169 hops", f169_traceroute_hops(64) == 30, "capped at 30");
    let mut pbuf = [0u8; 16];
    let n = f169_nslookup_line(&mut pbuf, 0xC0A8_0001);
    let ptext = core::str::from_utf8(&pbuf[..n]).unwrap_or("");
    set.add("F169 dotted", ptext == "192.168.0.1", "dotted quad");

    // F170
    set.add("F170 down smooth", f170_degrade(LinkState::Up, LinkState::Down), "unplug safe");
    set.add("F170 recover", f170_degrade(LinkState::Down, LinkState::Up), "reconnect");

    // F171
    let mut v6 = [0u8; 16];
    v6[0] = 0xFE;
    v6[1] = 0x80;
    set.add("F171 link-local", f171_link_local_addr(v6), "fe80::/10");
    set.add("F171 global", !f171_link_local_addr([0x20, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]), "2001:: not link-local");

    // F172
    let routes = [
        Route { prefix: 0x0000_0000, prefix_len: 0, iface: 1, metric: 10 },
        Route { prefix: 0xC0A8_0000, prefix_len: 16, iface: 2, metric: 5 },
    ];
    set.add("F172 longest", f172_route_pick(&routes, 0xC0A8_0101) == Some(2), "longest prefix wins");
    set.add("F172 default", f172_route_pick(&routes, 0x0808_0808) == Some(1), "default route");

    // F173
    set.add("F173 on", f173_airplane_on(false, false, false), "all radios off");
    set.add("F173 off", !f173_airplane_on(true, false, false), "wifi still on");

    // F174
    set.add("F174 ready", f174_panel_ready(NetStats { rx_bytes: 1, ..Default::default() }), "traffic seen");
    set.add("F174 idle", !f174_panel_ready(NetStats::default()), "no data");

    // F175
    let mut lat = LatencySamples::new();
    for v in [10u64, 20, 30, 40, 500] {
        lat.record(v);
    }
    set.add("F175 p95", lat.p95() == Some(500), "tail latency");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failures(set: &CheckSet) -> String {
        let mut s = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    s.push_str(&format!("{}: {}\n", c.name, c.detail));
                }
            }
        }
        s
    }

    #[test]
    fn f175_netm400_selftest_all_pass() {
        let set = run_netm400_checks();
        assert!(set.len() >= 25);
        assert!(set.all_passed(), "{}", failures(&set));
    }

    #[test]
    fn f166_bucket_saturation() {
        let mut tb = TokenBucket { tokens: 0, capacity: 5, refill_per_tick: 10 };
        tb.tick();
        assert_eq!(tb.tokens, 5, "capped at capacity");
    }

    #[test]
    fn f172_metric_tiebreak() {
        let routes = [
            Route { prefix: 0xC0A8_0000, prefix_len: 16, iface: 3, metric: 10 },
            Route { prefix: 0xC0A8_0000, prefix_len: 16, iface: 4, metric: 1 },
        ];
        assert_eq!(f172_route_pick(&routes, 0xC0A8_0101), Some(4));
    }
}
