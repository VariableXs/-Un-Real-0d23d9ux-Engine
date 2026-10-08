//! GALAXY-1800 AI-07 网络·RDMA·QUIC 域（G361~G420）。
//!
//! Three merged sub-domains: TCP/IP stack (G361~G380), RDMA (G381~G400)
//! and QUIC (G401~G420). Pure logic, fixed arrays, host-testable;
//! the "零出站默认态" policy is enforced as pure permission logic.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G361~G380 — TCP/IP 栈
// ---------------------------------------------------------------------------

/// G361 IPv4 头校验和（RFC 1071）。
pub fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut i = 0;
    while i + 1 < header.len() {
        sum += u16::from_be_bytes([header[i], header[i + 1]]) as u32;
        i += 2;
    }
    if i < header.len() {
        sum += (header[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn ipv4_verify(header: &[u8]) -> bool {
    let mut copy = [0u8; 20];
    copy[..header.len().min(20)].copy_from_slice(&header[..header.len().min(20)]);
    ipv4_checksum(&copy) == 0
}

/// G361 ARP 表：IP→MAC 缓存。
pub struct ArpTable {
    pub ip: [u32; 8],
    pub mac: [u64; 8], // 低 48 位
    pub len: usize,
}

impl ArpTable {
    pub const fn new() -> ArpTable {
        ArpTable { ip: [0; 8], mac: [0; 8], len: 0 }
    }
    pub fn learn(&mut self, ip: u32, mac: u64) -> bool {
        for i in 0..self.len {
            if self.ip[i] == ip {
                self.mac[i] = mac;
                return true;
            }
        }
        if self.len < 8 {
            self.ip[self.len] = ip;
            self.mac[self.len] = mac;
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn lookup(&self, ip: u32) -> Option<u64> {
        (0..self.len).find(|&i| self.ip[i] == ip).map(|i| self.mac[i])
    }
}

/// G362 IPv6 地址解析：8 组 hextet，支持 `::` 简写。
pub fn parse_ipv6(s: &[u8]) -> Option<[u16; 8]> {
    let mut out = [0u16; 8];
    let mut groups = [0u16; 8];
    let mut n = 0usize;
    let mut i = 0usize;
    let mut dcolon: Option<usize> = None;
    while i < s.len() {
        if s[i] == b':' {
            if i + 1 < s.len() && s[i + 1] == b':' {
                if dcolon.is_some() {
                    return None;
                }
                dcolon = Some(n);
                i += 2;
                if i >= s.len() {
                    break;
                }
                continue;
            }
            i += 1;
            continue;
        }
        let start = i;
        let mut v: u32 = 0;
        while i < s.len() && n < 8 && (s[i].is_ascii_hexdigit()) {
            v = v * 16 + (s[i] as char).to_digit(16)?;
            i += 1;
            if i - start > 4 {
                return None;
            }
        }
        if i == start {
            return None;
        }
        if n >= 8 {
            return None;
        }
        groups[n] = v as u16;
        n += 1;
    }
    match dcolon {
        None => {
            if n != 8 {
                return None;
            }
            out = groups;
        }
        Some(pos) => {
            let tail = n - pos;
            for k in 0..pos {
                out[k] = groups[k];
            }
            for k in 0..tail {
                out[8 - tail + k] = groups[pos + k];
            }
        }
    }
    Some(out)
}

pub fn is_link_local_v6(a: &[u16; 8]) -> bool {
    a[0] & 0xFFC0 == 0xFE80
}

/// 整数立方根（no_std 安全，输入截断防溢出）。
fn icbrt(x: u64) -> u64 {
    let x = x.min(1 << 30);
    let mut r = 0u64;
    while (r + 1) * (r + 1) * (r + 1) <= x {
        r += 1;
    }
    r
}

/// G363 CUBIC 拥塞窗口（简化）：竞争期三次函数增长。
pub fn cubic_cwnd(t_ticks: u64, w_max: u64) -> u64 {
    let k = icbrt(w_max / 2);
    let dt = t_ticks.saturating_sub(k).min(1_000_000);
    dt * dt * dt + w_max
}

/// G364 TCP 状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
}

pub fn tcp_step(st: TcpState, ev: u8) -> TcpState {
    // ev: 0=open/listen, 1=syn, 2=syn-ack, 3=ack, 4=close/f(fin), 5=fin-ack, 6=ack-of-fin
    match (st, ev) {
        (TcpState::Closed, 0) => TcpState::Listen,
        (TcpState::Closed, 1) => TcpState::SynSent,
        (TcpState::Listen, 1) => TcpState::SynReceived,
        (TcpState::SynSent, 2) => TcpState::Established,
        (TcpState::SynReceived, 3) => TcpState::Established,
        (TcpState::Established, 4) => TcpState::FinWait1,
        (TcpState::FinWait1, 5) => TcpState::TimeWait,
        (TcpState::FinWait1, 4) => TcpState::Closing2,
        (TcpState::Established, 6) => TcpState::CloseWait,
        (TcpState::CloseWait, 4) => TcpState::LastAck,
        (TcpState::LastAck, 5) => TcpState::Closed,
        _ => st,
    }
}

// 让 Closing2 参与枚举（半关路径）
impl TcpState {
    #[allow(non_upper_case_globals)]
    pub const Closing2: TcpState = TcpState::FinWait2;
}

/// G364 重传：RTO 指数退避。
pub fn rto_backoff(base_ms: u64, retries: u8) -> u64 {
    base_ms << retries.min(6)
}

/// G365 UDP 头解析。
pub fn udp_parse(h: &[u8]) -> Option<(u16, u16, u16)> {
    if h.len() < 8 {
        return None;
    }
    let sport = u16::from_be_bytes([h[0], h[1]]);
    let dport = u16::from_be_bytes([h[2], h[3]]);
    let len = u16::from_be_bytes([h[4], h[5]]);
    if len < 8 {
        return None;
    }
    Some((sport, dport, len))
}

/// G366 socket 表：bind/lookup。
pub struct SocketTable {
    pub port: [u16; 16],
    pub owner: [u8; 16],
    pub len: usize,
}

impl SocketTable {
    pub const fn new() -> SocketTable {
        SocketTable { port: [0; 16], owner: [0; 16], len: 0 }
    }
    pub fn bind(&mut self, port: u16, owner: u8) -> bool {
        if (0..self.len).any(|i| self.port[i] == port) || self.len >= 16 {
            return false;
        }
        self.port[self.len] = port;
        self.owner[self.len] = owner;
        self.len += 1;
        true
    }
    pub fn lookup(&self, port: u16) -> Option<u8> {
        (0..self.len).find(|&i| self.port[i] == port).map(|i| self.owner[i])
    }
}

/// G367 TLS 1.3 记录层：头部解析（type/version/length）。
pub fn tls_record_header(h: &[u8]) -> Option<(u8, u16, u16)> {
    if h.len() < 5 {
        return None;
    }
    let ver = u16::from_be_bytes([h[1], h[2]]);
    let len = u16::from_be_bytes([h[3], h[4]]);
    if h[0] > 0x18 || len > 0x4000 + 0x100 {
        return None;
    }
    Some((h[0], ver, len))
}

/// G368 HTTP/1.1 请求行与方法。
pub fn http_request(buf: &[u8]) -> Option<(&'static str, usize)> {
    let get = b"GET ";
    let post = b"POST ";
    if buf.starts_with(get) {
        let eol = buf.iter().position(|&c| c == b'\n')?;
        Some(("GET", eol))
    } else if buf.starts_with(post) {
        let eol = buf.iter().position(|&c| c == b'\n')?;
        Some(("POST", eol))
    } else {
        None
    }
}

/// G369 DNS：名字编码/解析 + A 记录。
pub fn dns_encode_name(name: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < name.len() {
        let start = i;
        while i < name.len() && name[i] != b'.' {
            i += 1;
        }
        let lbl = i - start;
        if n + 1 + lbl > out.len() || lbl == 0 || lbl > 63 {
            return 0;
        }
        out[n] = lbl as u8;
        n += 1;
        out[n..n + lbl].copy_from_slice(&name[start..i]);
        n += lbl;
        if i < name.len() {
            i += 1;
        }
    }
    if n + 1 <= out.len() {
        out[n] = 0;
        n += 1;
    }
    n
}

pub fn dns_a_record(resp: &[u8]) -> Option<u32> {
    // 简化应答：[header 12B][question ...][answer name 2B][type 2B][class 2B][ttl 4B][rdlen 2B][rdata 4B]
    if resp.len() < 30 {
        return None;
    }
    let rdlen = u16::from_be_bytes([resp[24], resp[25]]) as usize;
    if rdlen != 4 || resp[20] != 0 || resp[21] != 1 {
        return None; // type A
    }
    Some(u32::from_be_bytes([resp[26], resp[27], resp[28], resp[29]]))
}

/// G370 网卡描述符环。
pub struct NicRing {
    pub status: [u8; 8], // bit0 done
    pub head: usize,
}

impl NicRing {
    pub const fn new() -> NicRing {
        NicRing { status: [0; 8], head: 0 }
    }
    pub fn advance_if_done(&mut self) -> usize {
        let mut n = 0;
        while self.head < 8 && self.status[self.head] & 1 != 0 {
            self.status[self.head] = 0;
            self.head += 1;
            n += 1;
        }
        n
    }
}

/// G371 零出站默认态：无授权 → 拒绝一切出站。
pub fn egress_allowed(grant_mask: u32, dst_port_class: u32) -> bool {
    grant_mask & (1 << dst_port_class.min(31)) != 0
}

/// G373 防火墙规则：端口/方向匹配。
pub struct FwRule {
    pub deny: bool,
    pub port: u16,
}

pub fn firewall_decide(rules: &[FwRule], port: u16) -> bool {
    // true = allow；首条匹配生效
    for r in rules {
        if r.port == port {
            return !r.deny;
        }
    }
    true
}

/// G374 NAT 转换表。
pub struct NatTable {
    pub ext_port: [u16; 8],
    pub int_ip: [u32; 8],
    pub int_port: [u16; 8],
    pub len: usize,
}

impl NatTable {
    pub const fn new() -> NatTable {
        NatTable { ext_port: [0; 8], int_ip: [0; 8], int_port: [0; 8], len: 0 }
    }
    pub fn add(&mut self, ext: u16, ip: u32, port: u16) -> bool {
        if self.len >= 8 {
            return false;
        }
        self.ext_port[self.len] = ext;
        self.int_ip[self.len] = ip;
        self.int_port[self.len] = port;
        self.len += 1;
        true
    }
    pub fn translate_out(&self, int_ip: u32, int_port: u16) -> Option<u16> {
        (0..self.len)
            .find(|&i| self.int_ip[i] == int_ip && self.int_port[i] == int_port)
            .map(|i| self.ext_port[i])
    }
}

/// G375 DHCP 选项解析：magic cookie + option code/len/value。
pub fn dhcp_option(payload: &[u8], want: u8) -> Option<Option<&[u8]>> {
    // 返回 None=包坏；Some(None)=选项不存在
    if payload.len() < 4 || payload[0..4] != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut i = 4usize;
    while i < payload.len() {
        let code = payload[i];
        if code == 0xFF {
            return Some(None);
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *payload.get(i + 1)? as usize;
        if code == want {
            return Some(Some(payload.get(i + 2..i + 2 + len)?));
        }
        i += 2 + len;
    }
    Some(None)
}

// ---------------------------------------------------------------------------
// G381~G400 — RDMA
// ---------------------------------------------------------------------------

/// G381 QP 状态机：RESET→INIT→RTR→RTS→SQD→ERROR。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QpState {
    Reset,
    Init,
    Rtr,
    Rts,
    Error,
}

pub fn qp_transition(st: QpState, ev: u8) -> QpState {
    match (st, ev) {
        (QpState::Reset, 0) => QpState::Init,
        (QpState::Init, 1) => QpState::Rtr,
        (QpState::Rtr, 2) => QpState::Rts,
        (_, 3) => QpState::Error,
        (QpState::Error, 4) => QpState::Reset,
        _ => st,
    }
}

/// G383 内核旁路判定：QP 就绪 + MR 注册 + 直通权限 → 旁路。
pub fn bypass_path(qp: QpState, mr_registered: bool, capability: bool) -> bool {
    qp == QpState::Rts && mr_registered && capability
}

/// G384 内存注册：lkey/rkey 生成与校验。
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    pub lkey: u32,
    pub rkey: u32,
}

pub fn mr_register(base: u64, len: u64, pd: u32) -> MemRegion {
    let seed = (base as u32) ^ (len as u32) ^ pd.wrapping_mul(0x9E3779B9);
    MemRegion { base, len, lkey: seed ^ 0x1, rkey: seed ^ 0x2 }
}

pub fn mr_access_ok(mr: &MemRegion, addr: u64, len: u64, key: u32, remote: bool) -> bool {
    let expect = if remote { mr.rkey } else { mr.lkey };
    key == expect && addr >= mr.base && addr + len <= mr.base + mr.len
}

/// G385 完成队列：CQE 环。
pub struct RdmaCq {
    pub completions: [(u64, u32); 8], // (wr_id, status)
    pub head: usize,
    pub tail: usize,
}

impl RdmaCq {
    pub const fn new() -> RdmaCq {
        RdmaCq { completions: [(0, 0); 8], head: 0, tail: 0 }
    }
    pub fn push(&mut self, wr_id: u64, status: u32) -> bool {
        let next = (self.tail + 1) % 8;
        if next == self.head {
            return false;
        }
        self.completions[self.tail] = (wr_id, status);
        self.tail = next;
        true
    }
    pub fn poll(&mut self) -> Option<(u64, u32)> {
        if self.head == self.tail {
            return None;
        }
        let c = self.completions[self.head];
        self.head = (self.head + 1) % 8;
        Some(c)
    }
}

/// G391 降级链：RDMA 不可用回退 TCP。
pub fn rdma_degrade(hardware: bool, soft_roce: bool) -> &'static str {
    if hardware {
        "rdma-nic"
    } else if soft_roce {
        "soft-roce"
    } else {
        "tcp"
    }
}

/// G396 RDMA 安全：写操作必须 MR 带 remote-write 授权位。
pub fn rdma_write_allowed(grants: u32, remote_write_bit: u32) -> bool {
    grants & remote_write_bit != 0
}

// ---------------------------------------------------------------------------
// G401~G420 — QUIC
// ---------------------------------------------------------------------------

/// G401 QUIC 变长整数（RFC 9000 §16）。
pub fn quic_varint_encode(v: u64, out: &mut [u8]) -> usize {
    let n = if v < 64 {
        1
    } else if v < 16_384 {
        2
    } else if v < 1_073_741_824 {
        4
    } else {
        8
    };
    if out.len() < n {
        return 0;
    }
    match n {
        1 => out[0] = v as u8,
        2 => {
            out[0] = (v >> 8) as u8 | 0x40;
            out[1] = v as u8;
        }
        4 => {
            out[0] = (v >> 24) as u8 | 0x80;
            out[1] = (v >> 16) as u8;
            out[2] = (v >> 8) as u8;
            out[3] = v as u8;
        }
        _ => {
            out[0] = (v >> 56) as u8 | 0xC0;
            for k in 1..8 {
                out[k] = (v >> (56 - 8 * k)) as u8;
            }
        }
    }
    n
}

pub fn quic_varint_decode(buf: &[u8]) -> Option<(u64, usize)> {
    let first = *buf.first()?;
    let n = 1usize << (first >> 6);
    if buf.len() < n {
        return None;
    }
    let mut v = (first & 0x3F) as u64;
    for k in 1..n {
        v = v << 8 | buf[k] as u64;
    }
    Some((v, n))
}

/// G402 帧解析：STREAM/ACK/CRYPTO。
pub fn quic_frame(buf: &[u8]) -> Option<(&'static str, u64)> {
    match buf.first()? {
        0x08..=0x0F => {
            let (sid, _) = quic_varint_decode(&buf[1..])?;
            Some(("STREAM", sid))
        }
        0x02 => {
            let (largest, _) = quic_varint_decode(&buf[1..])?;
            Some(("ACK", largest))
        }
        0x06 => Some(("CRYPTO", 0)),
        0x1C => Some(("CONNECTION_CLOSE", 0)),
        _ => None,
    }
}

/// G402 多路复用流：流 ID → 状态表。
pub struct StreamTable {
    pub ids: [u64; 8],
    pub bytes_rx: [u64; 8],
    pub len: usize,
}

impl StreamTable {
    pub const fn new() -> StreamTable {
        StreamTable { ids: [0; 8], bytes_rx: [0; 8], len: 0 }
    }
    pub fn on_stream_data(&mut self, sid: u64, n: u64) -> bool {
        if let Some(i) = (0..self.len).find(|&i| self.ids[i] == sid) {
            self.bytes_rx[i] += n;
            true
        } else if self.len < 8 {
            self.ids[self.len] = sid;
            self.bytes_rx[self.len] = n;
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn rx_of(&self, sid: u64) -> Option<u64> {
        (0..self.len).find(|&i| self.ids[i] == sid).map(|i| self.bytes_rx[i])
    }
}

/// G403 0-RTT 会话恢复：ticket 有效 + 时间窗内。
pub fn zero_rtt_allowed(ticket_age_ms: u64, ticket_valid: bool, max_early_ms: u64) -> bool {
    ticket_valid && ticket_age_ms <= max_early_ms
}

/// G404 连接迁移：新地址 + 同 CID → 迁移。
pub fn connection_migration_ok(cid_match: bool, path_validated: bool) -> bool {
    cid_match && path_validated
}

/// G405 BBR 类带宽探测：按 RTT 采样更新速率。
pub fn bbr_rate_mbps(bw_samples: &[u32]) -> u32 {
    // 取最大带宽样本（BBR max-filter）
    bw_samples.iter().copied().max().unwrap_or(0)
}

/// G416 0-RTT 重放防护：单次票据使用。
pub struct AntiReplay {
    pub used: [u64; 8],
    pub len: usize,
}

impl AntiReplay {
    pub const fn new() -> AntiReplay {
        AntiReplay { used: [0; 8], len: 0 }
    }
    pub fn check_and_mark(&mut self, ticket_id: u64) -> bool {
        if (0..self.len).any(|i| self.used[i] == ticket_id) {
            return false;
        }
        if self.len < 8 {
            self.used[self.len] = ticket_id;
            self.len += 1;
        }
        true
    }
}

/// G412 降级链：QUIC 失败回退 TCP+TLS。
pub fn quic_degrade(quic_ok: bool) -> &'static str {
    if quic_ok {
        "quic"
    } else {
        "tcp-tls"
    }
}

// ---------------------------------------------------------------------------
// 自检与收口
// ---------------------------------------------------------------------------

/// AI-07 域自检：≤32 项覆盖三段。
pub fn run_net_checks() -> CheckSet {
    let mut s = CheckSet::new("gnet");
    // IPv4 头（校验和已填好）
    let mut hdr = [
        0x45u8, 0, 0, 20, 0, 1, 0, 0, 64, 17, 0, 0, 192, 168, 0, 1, 192, 168, 0, 2,
    ];
    let c = ipv4_checksum(&hdr);
    hdr[10] = (c >> 8) as u8;
    hdr[11] = c as u8;
    s.add("G361 ip csum", ipv4_verify(&hdr) && c != 0, "rfc1071");
    hdr[12] ^= 1;
    s.add("G361 bad csum", !ipv4_verify(&hdr), "detect");
    let mut arp = ArpTable::new();
    arp.learn(0xC0A8_0001, 0xAABB_CCDD_EEFF);
    s.add("G361 arp", arp.lookup(0xC0A8_0001) == Some(0xAABB_CCDD_EEFF) && arp.lookup(2).is_none(), "learn/lookup");
    let v6 = parse_ipv6(b"fe80::1");
    s.add("G362 ipv6", v6.is_some() && is_link_local_v6(&v6.unwrap()) && v6.unwrap()[7] == 1, "parse ::");
    s.add("G363 cubic", cubic_cwnd(0, 8) >= 8 && cubic_cwnd(10, 8) > 8, "growth");
    let st = tcp_step(tcp_step(TcpState::Closed, 1), 2);
    s.add("G364 tcp sm", st == TcpState::Established && tcp_step(TcpState::Established, 4) == TcpState::FinWait1, "syn/fin");
    s.add("G364 rto", rto_backoff(100, 2) == 400 && rto_backoff(100, 9) == 6400, "backoff cap");
    let u = udp_parse(&[0x13, 0x88, 0x00, 0x35, 0x00, 0x0C, 0x00, 0x00]);
    s.add("G365 udp", u == Some((5000, 53, 12)), "header");
    let mut tbl = SocketTable::new();
    tbl.bind(80, 1);
    tbl.bind(443, 2);
    s.add("G366 sockets", tbl.lookup(80) == Some(1) && !tbl.bind(80, 3), "bind/unique");
    let tls = tls_record_header(&[0x17, 0x03, 0x03, 0x01, 0x00]);
    s.add("G367 tls", tls == Some((0x17, 0x0303, 0x100)), "record");
    s.add("G368 http", http_request(b"GET /x HTTP/1.1\r\n") == Some(("GET", 16)) && http_request(b"DELETE / HTTP/1.1\r\n").is_none(), "request line");
    let mut enc = [0u8; 32];
    let en = dns_encode_name(b"www.example.com", &mut enc);
    s.add("G369 dns enc", en == 17 && enc[0] == 3 && enc[4] == 7 && enc[16] == 0, "labels");
    let mut a_resp = [0u8; 30];
    a_resp[20] = 0;
    a_resp[21] = 1; // type A
    a_resp[24] = 0;
    a_resp[25] = 4; // rdlen=4
    a_resp[26..30].copy_from_slice(&[8, 8, 8, 8]);
    let a = dns_a_record(&a_resp);
    s.add("G369 dns A", a == Some(0x0808_0808), "a record");
    let mut nic = NicRing::new();
    nic.status[0] = 1;
    nic.status[1] = 1;
    nic.status[2] = 0;
    s.add("G370 nic ring", nic.advance_if_done() == 2 && nic.head == 2, "done batch");
    s.add("G371 zero egress", !egress_allowed(0, 80) && egress_allowed(1 << 2, 2), "default deny");
    let rules = [FwRule { deny: true, port: 23 }, FwRule { deny: false, port: 80 }];
    s.add("G373 firewall", !firewall_decide(&rules, 23) && firewall_decide(&rules, 80) && firewall_decide(&rules, 999), "first match");
    let mut nat = NatTable::new();
    nat.add(40000, 0xC0A8_0005, 8080);
    s.add("G374 nat", nat.translate_out(0xC0A8_0005, 8080) == Some(40000), "outbound");
    s.add("G375 dhcp", dhcp_option(&[0x63, 0x82, 0x53, 0x63, 53, 1, 2], 53) == Some(Some(&[2u8][..])) && dhcp_option(&[1, 2, 3, 4], 53).is_none(), "option 53");
    // RDMA 段
    let qp = qp_transition(qp_transition(qp_transition(QpState::Reset, 0), 1), 2);
    s.add("G381 qp sm", qp == QpState::Rts && qp_transition(QpState::Rts, 3) == QpState::Error, "reset->rts");
    let mr = mr_register(0x1000, 0x100, 7);
    s.add("G383 bypass", bypass_path(QpState::Rts, true, true) && !bypass_path(QpState::Rtr, true, true), "path");
    s.add("G384 mr access", mr_access_ok(&mr, 0x1040, 0x10, mr.lkey, false) && !mr_access_ok(&mr, 0x2000, 0x10, mr.lkey, false) && mr_access_ok(&mr, 0x1040, 0x10, mr.rkey, true), "lkey/rkey");
    let mut cq = RdmaCq::new();
    cq.push(7, 0);
    s.add("G385 cq", cq.poll() == Some((7, 0)) && cq.poll().is_none(), "poll");
    s.add("G391 rdma degrade", rdma_degrade(false, false) == "tcp" && rdma_degrade(false, true) == "soft-roce", "fallback");
    s.add("G396 rdma write", rdma_write_allowed(0b100, 0b100) && !rdma_write_allowed(0b010, 0b100), "grant bit");
    // QUIC 段
    let mut vo = [0u8; 8];
    let vn = quic_varint_encode(300, &mut vo);
    s.add("G401 varint", vn == 2 && quic_varint_decode(&vo[..2]) == Some((300, 2)), "2B form");
    let mut vo8 = [0u8; 8];
    let vn8 = quic_varint_encode(1 << 40, &mut vo8);
    s.add("G401 varint 8B", vn8 == 8 && quic_varint_decode(&vo8) == Some((1 << 40, 8)), "8B form");
    s.add("G402 frames", quic_frame(&[0x08, 0x04]) == Some(("STREAM", 4)) && quic_frame(&[0x02, 0x0A]) == Some(("ACK", 10)) && quic_frame(&[0x99]).is_none(), "parse");
    let mut stm = StreamTable::new();
    stm.on_stream_data(4, 100);
    stm.on_stream_data(4, 50);
    s.add("G402 mux", stm.rx_of(4) == Some(150) && stm.len == 1, "stream bytes");
    s.add("G403 0-rtt", zero_rtt_allowed(5000, true, 60000) && !zero_rtt_allowed(70000, true, 60000) && !zero_rtt_allowed(100, false, 60000), "window");
    s.add("G404 migration", connection_migration_ok(true, true) && !connection_migration_ok(false, true), "cid match");
    s.add("G405 bbr", bbr_rate_mbps(&[100, 400, 250]) == 400 && bbr_rate_mbps(&[]) == 0, "max filter");
    let mut ar2 = AntiReplay::new();
    let first = ar2.check_and_mark(9);
    let second = ar2.check_and_mark(9);
    s.add("G416 anti-replay", first && !second, "ticket once");
    s.add("G412 quic degrade", quic_degrade(false) == "tcp-tls" && quic_degrade(true) == "quic", "fallback");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g361_checksum_known_vector() {
        // RFC 1071 示例式自反：构造头后填校验和必须自验证
        let mut h = [0x45u8, 0, 0x00, 0x1C, 0, 0, 0, 0, 0x40, 6, 0, 0, 10, 0, 0, 1, 10, 0, 0, 2];
        let c = ipv4_checksum(&h);
        h[10] = (c >> 8) as u8;
        h[11] = c as u8;
        assert!(ipv4_verify(&h));
    }

    #[test]
    fn g362_ipv6_parse_forms() {
        assert_eq!(parse_ipv6(b"1:2:3:4:5:6:7:8").unwrap()[7], 8);
        assert_eq!(parse_ipv6(b"::").unwrap(), [0u16; 8]);
        let v = parse_ipv6(b"fe80::abcd").unwrap();
        assert_eq!(v[0], 0xFE80);
        assert_eq!(v[7], 0xABCD);
        assert!(parse_ipv6(b"1:2:3").is_none());
        assert!(parse_ipv6(b"1::2::3").is_none());
    }

    #[test]
    fn g364_tcp_full_teardown() {
        // 服务端：listen → syn → est → 收 fin → close_wait → last_ack → closed
        let st = tcp_step(TcpState::Listen, 1);
        let st = tcp_step(st, 3);
        assert_eq!(st, TcpState::Established);
        let st = tcp_step(st, 6);
        assert_eq!(st, TcpState::CloseWait);
        let st = tcp_step(st, 4);
        assert_eq!(st, TcpState::LastAck);
        assert_eq!(tcp_step(st, 5), TcpState::Closed);
    }

    #[test]
    fn g369_dns_name_roundtrip() {
        let mut buf = [0u8; 64];
        let n = dns_encode_name(b"a.b.c", &mut buf);
        assert_eq!(n, 7);
        assert_eq!(&buf[..7], &[1, b'a', 1, b'b', 1, b'c', 0]);
        assert_eq!(dns_encode_name(b"toolonglabeliswaymorethansixtythreecharactersaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &mut buf), 0);
    }

    #[test]
    fn g401_varint_all_widths() {
        let mut buf = [0u8; 8];
        for (v, w) in [(0u64, 1usize), (63, 1), (64, 2), (16383, 2), (16384, 4), (1_073_741_824, 8)] {
            let n = quic_varint_encode(v, &mut buf);
            assert_eq!(n, w, "width of {v}");
            assert_eq!(quic_varint_decode(&buf[..n]), Some((v, n)));
        }
    }

    #[test]
    fn g420_domain_selftest_all_green() {
        let s = run_net_checks();
        if !s.all_passed() {
            let mut buf = [0u8; 2048];
            let n = s.render(&mut buf);
            panic!("domain self-test must pass://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(s.len() >= 25);
    }
}
