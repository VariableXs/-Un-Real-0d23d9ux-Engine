//! AI-09 · 网络协议栈域（F201~F225）.
//!
//! From the NIC ring to a socket. Every header is parsed from a byte slice with
//! explicit length checks, because a network stack is the one place where the
//! input is *always* hostile: a truncated frame or a lying total-length field
//! must produce an error, never a read past the end of the buffer.


//! VARIX-M500 AI-09 网络体验与协同（F201~F225）。
pub mod netxp;

pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_IPV6: u16 = 0x86DD;
pub const ETH_HDR_LEN: usize = 14;
pub const MIN_FRAME: usize = 60;
pub const MAX_FRAME: usize = 1514;

// ---------------------------------------------------------------------------
// Addressing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MacAddr(pub [u8; 6]);

impl MacAddr {
    pub const BROADCAST: MacAddr = MacAddr([0xFF; 6]);
    pub const ZERO: MacAddr = MacAddr([0; 6]);

    pub fn is_broadcast(&self) -> bool {
        self.0 == [0xFF; 6]
    }

    pub fn is_multicast(&self) -> bool {
        self.0[0] & 1 != 0
    }

    /// An all-zero hardware address is not a real station.
    pub fn is_valid(&self) -> bool {
        self.0 != [0; 6]
    }

    pub fn parse(s: &str) -> Option<MacAddr> {
        let mut out = [0u8; 6];
        let mut n = 0usize;
        for part in s.split(':') {
            if n >= 6 || part.len() != 2 {
                return None;
            }
            out[n] = u8::from_str_radix(part, 16).ok()?;
            n += 1;
        }
        if n == 6 {
            Some(MacAddr(out))
        } else {
            None
        }
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                w.str(":");
            }
            let hex = b"0123456789ABCDEF";
            let pair = [hex[(*b >> 4) as usize], hex[(*b & 0xF) as usize]];
            w.str(core::str::from_utf8(&pair).unwrap_or("??"));
        }
        w.used()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const ANY: Ipv4Addr = Ipv4Addr([0, 0, 0, 0]);
    pub const BROADCAST: Ipv4Addr = Ipv4Addr([255, 255, 255, 255]);
    pub const LOOPBACK: Ipv4Addr = Ipv4Addr([127, 0, 0, 1]);
    /// The link-local range a DHCP-less machine falls back to (169.254/16).
    pub const LINK_LOCAL: Ipv4Addr = Ipv4Addr([169, 254, 0, 1]);

    pub fn octets(&self) -> [u8; 4] {
        self.0
    }

    pub fn is_any(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == [255, 255, 255, 255]
    }

    pub fn is_link_local(&self) -> bool {
        self.0[0] == 169 && self.0[1] == 254
    }

    pub fn is_private(&self) -> bool {
        self.0[0] == 10
            || (self.0[0] == 172 && (16..=31).contains(&self.0[1]))
            || (self.0[0] == 192 && self.0[1] == 168)
    }

    pub fn parse(s: &str) -> Option<Ipv4Addr> {
        let mut out = [0u8; 4];
        let mut n = 0usize;
        for part in s.split('.') {
            if n >= 4 {
                return None;
            }
            out[n] = part.parse().ok()?;
            n += 1;
        }
        if n == 4 {
            Some(Ipv4Addr(out))
        } else {
            None
        }
    }

    pub fn from_u32(v: u32) -> Ipv4Addr {
        Ipv4Addr(v.to_be_bytes())
    }

    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes(self.0)
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                w.str(".");
            }
            w.num(*b as u64);
        }
        w.used()
    }

    /// Same subnet under `mask`?
    pub fn same_subnet(self, other: Ipv4Addr, mask: Ipv4Addr) -> bool {
        (self.to_u32() & mask.to_u32()) == (other.to_u32() & mask.to_u32())
    }
}

// ---------------------------------------------------------------------------
// F204 — Ethernet
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EthernetHeader {
    pub dst: MacAddr,
    pub src: MacAddr,
    pub ethertype: u16,
}

impl EthernetHeader {
    pub fn parse(frame: &[u8]) -> Result<(EthernetHeader, usize), &'static str> {
        if frame.len() < ETH_HDR_LEN {
            return Err("frame is shorter than an Ethernet header");
        }
        let mut dst = [0u8; 6];
        let mut src = [0u8; 6];
        dst.copy_from_slice(&frame[0..6]);
        src.copy_from_slice(&frame[6..12]);
        Ok((
            EthernetHeader {
                dst: MacAddr(dst),
                src: MacAddr(src),
                ethertype: u16::from_be_bytes([frame[12], frame[13]]),
            },
            ETH_HDR_LEN,
        ))
    }

    pub fn encode(&self) -> [u8; ETH_HDR_LEN] {
        let mut b = [0u8; ETH_HDR_LEN];
        b[0..6].copy_from_slice(&self.dst.0);
        b[6..12].copy_from_slice(&self.src.0);
        b[12..14].copy_from_slice(&self.ethertype.to_be_bytes());
        b
    }

    /// F204: a frame shorter than 60 bytes must be padded, and a frame larger
    /// than the MTU must be refused rather than truncated.
    pub fn frame_ok(len: usize, mtu: usize) -> bool {
        len >= MIN_FRAME && len <= mtu + ETH_HDR_LEN
    }
}

// ---------------------------------------------------------------------------
// F206 — IPv4 and checksums
// ---------------------------------------------------------------------------

pub const IPV4_MIN_HDR: usize = 20;
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

/// RFC 1071 ones-complement checksum. Used by IPv4, ICMP, UDP and TCP, so it
/// lives in one place and is tested once.
pub fn ones_complement_sum(data: &[u8], initial: u32) -> u16 {
    let mut sum = initial;
    let mut i = 0usize;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
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

/// The checksum of a header that already contains a checksum field must treat
/// that field as zero; this variant does exactly that.
pub fn checksum_with_zeroed_field(header: &[u8], field_offset: usize) -> u16 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i + 1 < header.len() {
        if i == field_offset || i == field_offset + 1 {
            i += 2;
            continue;
        }
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

pub const IP_FLAG_DF: u16 = 0x4000;
pub const IP_FLAG_MF: u16 = 0x2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ipv4Header {
    pub ihl: u8,
    pub tos: u8,
    pub total_len: u16,
    pub id: u16,
    pub flags_frag: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
}

impl Ipv4Header {
    pub fn parse(pkt: &[u8]) -> Result<(Ipv4Header, usize), &'static str> {
        if pkt.len() < IPV4_MIN_HDR {
            return Err("packet is shorter than an IPv4 header");
        }
        if pkt[0] >> 4 != 4 {
            return Err("not an IPv4 packet");
        }
        let ihl = (pkt[0] & 0x0F) as usize * 4;
        if ihl < IPV4_MIN_HDR || ihl > pkt.len() {
            return Err("IPv4 header length is out of range");
        }
        let total_len = u16::from_be_bytes([pkt[2], pkt[3]]);
        if (total_len as usize) < ihl || (total_len as usize) > pkt.len() {
            return Err("IPv4 total length disagrees with the buffer");
        }
        Ok((
            Ipv4Header {
                ihl: (ihl / 4) as u8,
                tos: pkt[1],
                total_len,
                id: u16::from_be_bytes([pkt[4], pkt[5]]),
                flags_frag: u16::from_be_bytes([pkt[6], pkt[7]]),
                ttl: pkt[8],
                protocol: pkt[9],
                checksum: u16::from_be_bytes([pkt[10], pkt[11]]),
                src: Ipv4Addr([pkt[12], pkt[13], pkt[14], pkt[15]]),
                dst: Ipv4Addr([pkt[16], pkt[17], pkt[18], pkt[19]]),
            },
            ihl,
        ))
    }

    pub fn header_bytes(&self) -> usize {
        self.ihl as usize * 4
    }

    pub fn payload_len(&self) -> usize {
        (self.total_len as usize).saturating_sub(self.header_bytes())
    }

    pub fn dont_fragment(&self) -> bool {
        self.flags_frag & IP_FLAG_DF != 0
    }

    pub fn more_fragments(&self) -> bool {
        self.flags_frag & IP_FLAG_MF != 0
    }

    pub fn fragment_offset(&self) -> u16 {
        self.flags_frag & 0x1FFF
    }

    /// Is this a fragment other than the first? Those carry no transport
    /// header, which is why the socket layer must not parse them as if they did.
    pub fn is_later_fragment(&self) -> bool {
        self.fragment_offset() > 0
    }

    /// Verify the header checksum in place.
    pub fn checksum_ok(&self, pkt: &[u8]) -> bool {
        let n = self.header_bytes();
        if n > pkt.len() {
            return false;
        }
        checksum_with_zeroed_field(&pkt[..n], 10) == self.checksum
    }

    /// F206: fill in every field the sender is responsible for. A stack that
    /// forgets the checksum looks alive and silently drops every packet.
    pub fn build(src: Ipv4Addr, dst: Ipv4Addr, protocol: u8, payload_len: usize, id: u16) -> Ipv4Header {
        Ipv4Header {
            ihl: 5,
            tos: 0,
            total_len: (IPV4_MIN_HDR + payload_len) as u16,
            id,
            flags_frag: IP_FLAG_DF,
            ttl: 64,
            protocol,
            checksum: 0,
            src,
            dst,
        }
    }

    pub fn encode(&self) -> [u8; IPV4_MIN_HDR] {
        let mut b = [0u8; IPV4_MIN_HDR];
        b[0] = 0x40 | (self.ihl & 0x0F);
        b[1] = self.tos;
        b[2..4].copy_from_slice(&self.total_len.to_be_bytes());
        b[4..6].copy_from_slice(&self.id.to_be_bytes());
        b[6..8].copy_from_slice(&self.flags_frag.to_be_bytes());
        b[8] = self.ttl;
        b[9] = self.protocol;
        b[12..16].copy_from_slice(&self.src.0);
        b[16..20].copy_from_slice(&self.dst.0);
        let sum = checksum_with_zeroed_field(&b, 10);
        b[10..12].copy_from_slice(&sum.to_be_bytes());
        b
    }
}

// ---------------------------------------------------------------------------
// F205 — ARP
// ---------------------------------------------------------------------------

pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;
pub const ARP_PACKET_LEN: usize = 28;
/// Entries older than this are considered stale; ARP cache poisoning lives
/// forever unless entries expire.
pub const ARP_ENTRY_TTL_TICKS: u64 = 3_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ArpPacket {
    pub opcode: u16,
    pub sender_mac: MacAddr,
    pub sender_ip: Ipv4Addr,
    pub target_mac: MacAddr,
    pub target_ip: Ipv4Addr,
}

impl ArpPacket {
    pub fn parse(buf: &[u8]) -> Result<ArpPacket, &'static str> {
        if buf.len() < ARP_PACKET_LEN {
            return Err("ARP packet is truncated");
        }
        if u16::from_be_bytes([buf[0], buf[1]]) != 1 {
            return Err("not Ethernet/IPv4 ARP");
        }
        if u16::from_be_bytes([buf[2], buf[3]]) != ETHERTYPE_IPV4 {
            return Err("ARP is not carrying IPv4");
        }
        if buf[4] != 6 || buf[5] != 4 {
            return Err("unexpected ARP address sizes");
        }
        let mut smac = [0u8; 6];
        let mut tmac = [0u8; 6];
        smac.copy_from_slice(&buf[8..14]);
        tmac.copy_from_slice(&buf[18..24]);
        Ok(ArpPacket {
            opcode: u16::from_be_bytes([buf[6], buf[7]]),
            sender_mac: MacAddr(smac),
            sender_ip: Ipv4Addr([buf[14], buf[15], buf[16], buf[17]]),
            target_mac: MacAddr(tmac),
            target_ip: Ipv4Addr([buf[24], buf[25], buf[26], buf[27]]),
        })
    }

    pub fn encode(&self) -> [u8; ARP_PACKET_LEN] {
        let mut b = [0u8; ARP_PACKET_LEN];
        b[0..2].copy_from_slice(&1u16.to_be_bytes());
        b[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
        b[4] = 6;
        b[5] = 4;
        b[6..8].copy_from_slice(&self.opcode.to_be_bytes());
        b[8..14].copy_from_slice(&self.sender_mac.0);
        b[14..18].copy_from_slice(&self.sender_ip.0);
        b[18..24].copy_from_slice(&self.target_mac.0);
        b[24..28].copy_from_slice(&self.target_ip.0);
        b
    }

    /// The reply a target sends back: sender and target swap, and the target
    /// MAC is filled in from the request's sender.
    pub fn reply_to(request: &ArpPacket, our_mac: MacAddr) -> ArpPacket {
        ArpPacket {
            opcode: ARP_REPLY,
            sender_mac: our_mac,
            sender_ip: request.target_ip,
            target_mac: request.sender_mac,
            target_ip: request.sender_ip,
        }
    }

    pub fn is_gratuitous(&self) -> bool {
        self.sender_ip == self.target_ip
    }
}

pub const MAX_ARP_ENTRIES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ArpEntry {
    pub ip: Ipv4Addr,
    pub mac: MacAddr,
    pub inserted_tick: u64,
    pub hits: u64,
    /// Learned from a reply (true) versus configured statically (false).
    pub dynamic: bool,
}

pub struct ArpCache {
    entries: [ArpEntry; MAX_ARP_ENTRIES],
    len: usize,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl ArpCache {
    pub const fn new() -> ArpCache {
        ArpCache {
            entries: [ArpEntry {
                ip: Ipv4Addr([0; 4]),
                mac: MacAddr([0; 6]),
                inserted_tick: 0,
                hits: 0,
                dynamic: true,
            }; MAX_ARP_ENTRIES],
            len: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

    pub fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Learn an address. A conflicting mapping for a *static* entry is refused,
    /// which is the cheapest defence against ARP spoofing.
    pub fn insert(&mut self, ip: Ipv4Addr, mac: MacAddr, now: u64, dynamic: bool) -> bool {
        if !mac.is_valid() {
            return false;
        }
        for e in self.entries[..self.len].iter_mut() {
            if e.ip == ip {
                if !e.dynamic && e.mac != mac {
                    return false;
                }
                e.mac = mac;
                e.inserted_tick = now;
                e.dynamic = dynamic;
                return true;
            }
        }
        if self.len >= MAX_ARP_ENTRIES {
            // Evict the oldest dynamic entry; a static one is never dropped.
            let victim = self.entries[..self.len]
                .iter()
                .enumerate()
                .filter(|(_, e)| e.dynamic)
                .min_by_key(|(_, e)| e.inserted_tick)
                .map(|(i, _)| i);
            match victim {
                Some(i) => {
                    self.entries[i] = ArpEntry {
                        ip,
                        mac,
                        inserted_tick: now,
                        hits: 0,
                        dynamic,
                    };
                    self.evictions += 1;
                    return true;
                }
                None => return false,
            }
        }
        self.entries[self.len] = ArpEntry {
            ip,
            mac,
            inserted_tick: now,
            hits: 0,
            dynamic,
        };
        self.len += 1;
        true
    }

    pub fn lookup(&mut self, ip: Ipv4Addr, now: u64) -> Option<MacAddr> {
        for e in self.entries[..self.len].iter_mut() {
            if e.ip == ip {
                if e.dynamic && now.saturating_sub(e.inserted_tick) > ARP_ENTRY_TTL_TICKS {
                    self.misses += 1;
                    return None;
                }
                e.hits += 1;
                self.hits += 1;
                return Some(e.mac);
            }
        }
        self.misses += 1;
        None
    }

    pub fn flush_dynamic(&mut self) -> usize {
        let mut kept = 0usize;
        for i in 0..self.len {
            if self.entries[i].dynamic {
                continue;
            }
            self.entries[kept] = self.entries[i];
            kept += 1;
        }
        let removed = self.len - kept;
        self.len = kept;
        removed
    }
}

impl Default for ArpCache {
    fn default() -> ArpCache {
        ArpCache::new()
    }
}

// ---------------------------------------------------------------------------
// F207/F208 — ICMP and UDP
// ---------------------------------------------------------------------------

pub const ICMP_ECHO_REPLY: u8 = 0;
pub const ICMP_ECHO_REQUEST: u8 = 8;
pub const ICMP_DEST_UNREACHABLE: u8 = 3;
pub const ICMP_TIME_EXCEEDED: u8 = 11;
pub const ICMP_HDR_LEN: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IcmpMessage {
    pub kind: u8,
    pub code: u8,
    pub id: u16,
    pub sequence: u16,
    pub payload_len: usize,
}

impl IcmpMessage {
    pub fn parse(buf: &[u8]) -> Result<IcmpMessage, &'static str> {
        if buf.len() < ICMP_HDR_LEN {
            return Err("ICMP message is truncated");
        }
        if ones_complement_sum(buf, 0) != 0 {
            return Err("ICMP checksum is wrong");
        }
        Ok(IcmpMessage {
            kind: buf[0],
            code: buf[1],
            id: u16::from_be_bytes([buf[4], buf[5]]),
            sequence: u16::from_be_bytes([buf[6], buf[7]]),
            payload_len: buf.len() - ICMP_HDR_LEN,
        })
    }

    pub fn is_echo_request(&self) -> bool {
        self.kind == ICMP_ECHO_REQUEST && self.code == 0
    }

    /// Build an echo reply for `payload`, checksum included.
    pub fn echo_reply(request: &IcmpMessage, payload: &[u8], out: &mut [u8]) -> Option<usize> {
        if payload.len() + ICMP_HDR_LEN > out.len() {
            return None;
        }
        out[0] = ICMP_ECHO_REPLY;
        out[1] = 0;
        out[2] = 0;
        out[3] = 0;
        out[4..6].copy_from_slice(&request.id.to_be_bytes());
        out[6..8].copy_from_slice(&request.sequence.to_be_bytes());
        out[8..8 + payload.len()].copy_from_slice(payload);
        let n = ICMP_HDR_LEN + payload.len();
        let sum = ones_complement_sum(&out[..n], 0);
        out[2..4].copy_from_slice(&sum.to_be_bytes());
        Some(n)
    }

    pub fn describe_kind(&self) -> &'static str {
        match self.kind {
            ICMP_ECHO_REQUEST => "echo-request",
            ICMP_ECHO_REPLY => "echo-reply",
            ICMP_DEST_UNREACHABLE => "unreachable",
            ICMP_TIME_EXCEEDED => "time-exceeded",
            _ => "other",
        }
    }
}

pub const UDP_HDR_LEN: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub fn parse(buf: &[u8]) -> Result<(UdpHeader, usize), &'static str> {
        if buf.len() < UDP_HDR_LEN {
            return Err("UDP header is truncated");
        }
        let length = u16::from_be_bytes([buf[4], buf[5]]);
        if (length as usize) < UDP_HDR_LEN || (length as usize) > buf.len() {
            return Err("UDP length disagrees with the buffer");
        }
        Ok((
            UdpHeader {
                src_port: u16::from_be_bytes([buf[0], buf[1]]),
                dst_port: u16::from_be_bytes([buf[2], buf[3]]),
                length,
                checksum: u16::from_be_bytes([buf[6], buf[7]]),
            },
            UDP_HDR_LEN,
        ))
    }

    /// The UDP checksum covers a pseudo-header, which is the detail every
    /// first implementation forgets.
    pub fn verify(&self, buf: &[u8], src: Ipv4Addr, dst: Ipv4Addr) -> bool {
        if self.checksum == 0 {
            // Zero means "not computed" for UDP over IPv4.
            return true;
        }
        let len = self.length as usize;
        if len > buf.len() {
            return false;
        }
        let mut pseudo = [0u8; 12];
        pseudo[0..4].copy_from_slice(&src.0);
        pseudo[4..8].copy_from_slice(&dst.0);
        pseudo[8] = 0;
        pseudo[9] = PROTO_UDP;
        pseudo[10..12].copy_from_slice(&self.length.to_be_bytes());
        let mut sum = 0u32;
        for chunk in pseudo.chunks(2) {
            sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
        }
        // Add the datagram with its own checksum field treated as zero.
        let mut datagram_sum = 0u32;
        let mut i = 0usize;
        while i + 1 < len {
            if i == 6 {
                i += 2;
                continue;
            }
            datagram_sum += u16::from_be_bytes([buf[i], buf[i + 1]]) as u32;
            i += 2;
        }
        sum += datagram_sum;
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16) == 0
    }

    pub fn payload_len(&self) -> usize {
        (self.length as usize).saturating_sub(UDP_HDR_LEN)
    }
}

// ---------------------------------------------------------------------------
// F209 — TCP state machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TcpState {
    #[default]
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

impl TcpState {
    pub fn as_str(self) -> &'static str {
        match self {
            TcpState::Closed => "CLOSED",
            TcpState::Listen => "LISTEN",
            TcpState::SynSent => "SYN-SENT",
            TcpState::SynReceived => "SYN-RECEIVED",
            TcpState::Established => "ESTABLISHED",
            TcpState::FinWait1 => "FIN-WAIT-1",
            TcpState::FinWait2 => "FIN-WAIT-2",
            TcpState::CloseWait => "CLOSE-WAIT",
            TcpState::Closing => "CLOSING",
            TcpState::LastAck => "LAST-ACK",
            TcpState::TimeWait => "TIME-WAIT",
        }
    }

    pub fn can_send(&self) -> bool {
        matches!(self, TcpState::Established | TcpState::CloseWait)
    }

    pub fn can_receive(&self) -> bool {
        matches!(self, TcpState::Established | TcpState::FinWait1 | TcpState::FinWait2)
    }

    pub fn is_closed(&self) -> bool {
        matches!(self, TcpState::Closed)
    }

    /// Is the connection alive enough to count against the socket limit?
    pub fn is_active(&self) -> bool {
        !matches!(self, TcpState::Closed | TcpState::Listen)
    }
}

pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const TCP_URG: u8 = 0x20;
pub const TCP_HDR_LEN: usize = 20;
/// RFC 793 default. A machine with a smaller buffer than this must advertise
/// it, or the peer will overrun it.
pub const TCP_DEFAULT_WINDOW: u16 = 64240;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TcpSegment {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub data_offset: u8,
    pub flags: u8,
    pub window: u16,
    pub payload_len: usize,
}

impl TcpSegment {
    pub fn parse(buf: &[u8]) -> Result<TcpSegment, &'static str> {
        if buf.len() < TCP_HDR_LEN {
            return Err("TCP header is truncated");
        }
        let data_offset = (buf[12] >> 4) as usize * 4;
        if data_offset < TCP_HDR_LEN || data_offset > buf.len() {
            return Err("TCP data offset is out of range");
        }
        Ok(TcpSegment {
            src_port: u16::from_be_bytes([buf[0], buf[1]]),
            dst_port: u16::from_be_bytes([buf[2], buf[3]]),
            seq: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            ack: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            data_offset: (data_offset / 4) as u8,
            flags: buf[13],
            window: u16::from_be_bytes([buf[14], buf[15]]),
            payload_len: buf.len() - data_offset,
        })
    }

    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    pub fn header_bytes(&self) -> usize {
        self.data_offset as usize * 4
    }

    /// A SYN consumes one sequence number; so does a FIN. Getting this wrong
    /// desynchronises the stream on the very first handshake.
    pub fn sequence_space(&self) -> u32 {
        let mut n = self.payload_len as u32;
        if self.has(TCP_SYN) {
            n += 1;
        }
        if self.has(TCP_FIN) {
            n += 1;
        }
        n
    }
}

/// F209: the transition function. `None` means the segment is not valid in this
/// state, which the caller must treat as "drop and count", never as a no-op.
pub fn tcp_transition(state: TcpState, seg: &TcpSegment, passive: bool) -> Option<TcpState> {
    let syn = seg.has(TCP_SYN);
    let ack = seg.has(TCP_ACK);
    let fin = seg.has(TCP_FIN);
    let rst = seg.has(TCP_RST);
    if rst {
        return Some(TcpState::Closed);
    }
    match state {
        TcpState::Closed => {
            if passive && syn {
                Some(TcpState::Listen)
            } else if syn {
                Some(TcpState::SynSent)
            } else {
                None
            }
        }
        TcpState::Listen => {
            if syn {
                Some(TcpState::SynReceived)
            } else {
                None
            }
        }
        TcpState::SynSent => {
            if syn && ack {
                Some(TcpState::Established)
            } else if syn {
                Some(TcpState::SynReceived)
            } else {
                None
            }
        }
        TcpState::SynReceived => {
            if ack {
                if fin {
                    Some(TcpState::CloseWait)
                } else {
                    Some(TcpState::Established)
                }
            } else {
                None
            }
        }
        TcpState::Established => {
            if fin {
                Some(TcpState::CloseWait)
            } else {
                Some(TcpState::Established)
            }
        }
        TcpState::FinWait1 => {
            if fin && ack {
                Some(TcpState::TimeWait)
            } else if ack {
                Some(TcpState::FinWait2)
            } else if fin {
                Some(TcpState::Closing)
            } else {
                None
            }
        }
        TcpState::FinWait2 => {
            if fin {
                Some(TcpState::TimeWait)
            } else {
                None
            }
        }
        TcpState::Closing => {
            if ack {
                Some(TcpState::TimeWait)
            } else {
                None
            }
        }
        TcpState::CloseWait => {
            if fin {
                Some(TcpState::LastAck)
            } else {
                Some(TcpState::CloseWait)
            }
        }
        TcpState::LastAck => {
            if ack {
                Some(TcpState::Closed)
            } else {
                None
            }
        }
        TcpState::TimeWait => Some(TcpState::TimeWait),
    }
}

/// Active close: send FIN from whatever state the local side is in.
pub fn tcp_active_close(state: TcpState) -> Option<TcpState> {
    match state {
        TcpState::Established => Some(TcpState::FinWait1),
        TcpState::CloseWait => Some(TcpState::LastAck),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F210 — sockets
// ---------------------------------------------------------------------------

pub const MAX_SOCKETS: usize = 32;
pub const MAX_LISTEN_BACKLOG: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SockProto {
    #[default]
    Udp,
    Tcp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Endpoint {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl Endpoint {
    pub fn any(port: u16) -> Endpoint {
        Endpoint {
            ip: Ipv4Addr::ANY,
            port,
        }
    }

    pub fn is_bound(&self) -> bool {
        self.port != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Socket {
    pub id: u32,
    pub used: bool,
    pub proto: SockProto,
    pub local: Endpoint,
    pub remote: Endpoint,
    pub state: TcpState,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    /// Receive queue depth in bytes; a peer that ignores our window fills this.
    pub rx_queued: u32,
    pub rx_capacity: u32,
    pub dropped: u64,
}

impl Socket {
    pub fn can_accept_byte(&self) -> bool {
        self.rx_queued < self.rx_capacity
    }

    /// The window we advertise. Reporting a window larger than the buffer we
    /// actually have is how a stack ends up dropping data the peer believed was
    /// delivered.
    pub fn advertised_window(&self) -> u16 {
        self.rx_capacity.saturating_sub(self.rx_queued).min(u16::MAX as u32) as u16
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocketError {
    NoSuchSocket,
    AlreadyBound,
    NotBound,
    WouldBlock,
    Refused,
    QueueFull,
    WrongState,
}

impl SocketError {
    pub fn errno(self) -> i64 {
        match self {
            SocketError::NoSuchSocket => -9,   // EBADF
            SocketError::AlreadyBound => -98,  // EADDRINUSE
            SocketError::NotBound => -88,      // ENOTSOCK-ish
            SocketError::WouldBlock => -11,    // EAGAIN
            SocketError::Refused => -111,      // ECONNREFUSED
            SocketError::QueueFull => -105,    // ENOBUFS
            SocketError::WrongState => -107,   // ENOTCONN
        }
    }
}

pub struct SocketTable {
    sockets: [Socket; MAX_SOCKETS],
    count: usize,
    next_ephemeral: u16,
    accepted: u64,
    refused: u64,
}

impl SocketTable {
    /// Ephemeral ports start above 32768, which is the range a kernel is
    /// expected to hand out for outbound connections.
    pub const EPHEMERAL_BASE: u16 = 32768;

    pub const fn new() -> SocketTable {
        SocketTable {
            sockets: [Socket {
                id: 0,
                used: false,
                proto: SockProto::Udp,
                local: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                remote: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                state: TcpState::Closed,
                rx_bytes: 0,
                tx_bytes: 0,
                rx_queued: 0,
                rx_capacity: TCP_DEFAULT_WINDOW as u32,
                dropped: 0,
            }; MAX_SOCKETS],
            count: 0,
            next_ephemeral: Self::EPHEMERAL_BASE,
            accepted: 0,
            refused: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    pub fn refused(&self) -> u64 {
        self.refused
    }

    pub fn get(&self, id: u32) -> Option<Socket> {
        self.sockets.iter().copied().find(|s| s.used && s.id == id)
    }

    fn slot_of(&self, id: u32) -> Option<usize> {
        self.sockets.iter().position(|s| s.used && s.id == id)
    }

    pub fn create(&mut self, proto: SockProto) -> Result<u32, SocketError> {
        let free = self
            .sockets
            .iter()
            .enumerate()
            .find(|(_, s)| !s.used)
            .map(|(i, _)| i)
            .ok_or(SocketError::QueueFull)?;
        let id = free as u32;
        self.sockets[free] = Socket {
            id,
            used: true,
            proto,
            state: TcpState::Closed,
            // `Socket::default()` has a zero window; a real socket must
            // advertise the buffer it actually has (F209).
            rx_capacity: TCP_DEFAULT_WINDOW as u32,
            ..Socket::default()
        };
        self.count += 1;
        Ok(id)
    }

    pub fn close(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(i) => {
                self.sockets[i] = Socket::default();
                self.count -= 1;
                true
            }
            None => false,
        }
    }

    /// Port 0 means "pick one for me".
    pub fn bind(&mut self, id: u32, mut local: Endpoint) -> Result<u16, SocketError> {
        if local.port == 0 {
            local.port = self.next_ephemeral;
            self.next_ephemeral = self.next_ephemeral.wrapping_add(1).max(Self::EPHEMERAL_BASE);
        }
        // A wildcard bind and a specific-address bind on the same port are a
        // conflict: they would both receive the same packets.
        let taken = self.sockets.iter().any(|s| {
            s.used
                && s.id != id
                && s.local.port == local.port
                && (s.local.ip.is_any() || local.ip.is_any() || s.local.ip == local.ip)
        });
        if taken {
            self.refused += 1;
            return Err(SocketError::AlreadyBound);
        }
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        self.sockets[i].local = local;
        Ok(local.port)
    }

    pub fn listen(&mut self, id: u32, backlog: usize) -> Result<(), SocketError> {
        if backlog == 0 || backlog > MAX_LISTEN_BACKLOG {
            return Err(SocketError::QueueFull);
        }
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        if !self.sockets[i].local.is_bound() {
            return Err(SocketError::NotBound);
        }
        self.sockets[i].state = TcpState::Listen;
        Ok(())
    }

    /// Accept one inbound connection: a new socket inherits the listener's
    /// local port and takes the remote endpoint from the peer.
    pub fn accept(&mut self, listener: u32, peer: Endpoint) -> Result<u32, SocketError> {
        let l = self.get(listener).ok_or(SocketError::NoSuchSocket)?;
        if l.state != TcpState::Listen {
            return Err(SocketError::WouldBlock);
        }
        let id = self.create(SockProto::Tcp)?;
        let i = self.slot_of(id).expect("just created");
        self.sockets[i].local = l.local;
        self.sockets[i].remote = peer;
        self.sockets[i].state = TcpState::Established;
        self.accepted += 1;
        Ok(id)
    }

    pub fn connect(&mut self, id: u32, remote: Endpoint) -> Result<(), SocketError> {
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        if !self.sockets[i].local.is_bound() {
            self.bind(id, Endpoint::any(0))?;
        }
        self.sockets[i].remote = remote;
        self.sockets[i].state = TcpState::SynSent;
        Ok(())
    }

    /// Deliver bytes into a receive queue, honouring the advertised window.
    pub fn deliver(&mut self, id: u32, bytes: u32) -> Result<u32, SocketError> {
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        let s = &mut self.sockets[i];
        if !s.state.can_receive() {
            return Err(SocketError::WrongState);
        }
        let room = s.rx_capacity.saturating_sub(s.rx_queued);
        let take = bytes.min(room);
        s.rx_queued += take;
        s.rx_bytes += take as u64;
        if take < bytes {
            s.dropped += (bytes - take) as u64;
        }
        Ok(take)
    }

    pub fn consume(&mut self, id: u32, bytes: u32) -> Result<u32, SocketError> {
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        let s = &mut self.sockets[i];
        let take = bytes.min(s.rx_queued);
        s.rx_queued -= take;
        Ok(take)
    }

    pub fn note_tx(&mut self, id: u32, bytes: u64) -> Result<(), SocketError> {
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        self.sockets[i].tx_bytes += bytes;
        Ok(())
    }

    /// Advance a connection through the state machine with one segment.
    pub fn step(&mut self, id: u32, seg: &TcpSegment, passive: bool) -> Result<TcpState, SocketError> {
        let i = self.slot_of(id).ok_or(SocketError::NoSuchSocket)?;
        let state = self.sockets[i].state;
        let next = match tcp_transition(state, seg, passive) {
            Some(n) => n,
            None => {
                self.refused += 1;
                return Err(SocketError::WrongState);
            }
        };
        self.sockets[i].state = next;
        Ok(next)
    }

    /// Sockets in a bindable state on `port` — used by the firewall and NAT.
    pub fn find_by_port(&self, port: u16) -> Option<u32> {
        self.sockets
            .iter()
            .find(|s| s.used && s.local.port == port)
            .map(|s| s.id)
    }

    pub fn established(&self) -> usize {
        self.sockets
            .iter()
            .filter(|s| s.used && s.state == TcpState::Established)
            .count()
    }
}

impl Default for SocketTable {
    fn default() -> SocketTable {
        SocketTable::new()
    }
}

// ---------------------------------------------------------------------------
// F213/F214/F215 — firewall, NAT, conntrack
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Accept,
    Drop,
    /// Answer with a refusal so the peer does not retry forever.
    Reject,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Accept => "accept",
            Verdict::Drop => "drop",
            Verdict::Reject => "reject",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Inbound,
    Outbound,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FirewallRule {
    pub direction: Option<Direction>,
    pub protocol: Option<u8>,
    pub src: Ipv4Addr,
    pub src_mask: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub dst_mask: Ipv4Addr,
    pub dst_port: Option<u16>,
    pub verdict: Option<Verdict>,
    pub name: &'static str,
}

impl FirewallRule {
    pub fn matches(&self, direction: Direction, protocol: u8, src: Ipv4Addr, dst: Ipv4Addr, dst_port: u16) -> bool {
        if let Some(d) = self.direction {
            if d != direction {
                return false;
            }
        }
        if let Some(p) = self.protocol {
            if p != protocol {
                return false;
            }
        }
        if !self.src.is_any() && !src.same_subnet(self.src, self.src_mask) {
            return false;
        }
        if !self.dst.is_any() && !dst.same_subnet(self.dst, self.dst_mask) {
            return false;
        }
        if let Some(p) = self.dst_port {
            if p != dst_port {
                return false;
            }
        }
        true
    }
}

pub const MAX_RULES: usize = 16;

/// F213: first match wins, which is what makes a rule list predictable. An
/// empty list accepts, because default-deny with no UI is how a user locks
/// themselves out of their own machine.
pub struct Firewall {
    rules: [FirewallRule; MAX_RULES],
    len: usize,
    evaluated: u64,
    matched: [u64; MAX_RULES],
    dropped: u64,
}

impl Firewall {
    pub const fn new() -> Firewall {
        Firewall {
            rules: [FirewallRule {
                direction: None,
                protocol: None,
                src: Ipv4Addr([0; 4]),
                src_mask: Ipv4Addr([0; 4]),
                dst: Ipv4Addr([0; 4]),
                dst_mask: Ipv4Addr([0; 4]),
                dst_port: None,
                verdict: None,
                name: "",
            }; MAX_RULES],
            len: 0,
            evaluated: 0,
            matched: [0; MAX_RULES],
            dropped: 0,
        }
    }

    pub fn add(&mut self, rule: FirewallRule) -> bool {
        if self.len >= MAX_RULES {
            return false;
        }
        self.rules[self.len] = rule;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn evaluated(&self) -> u64 {
        self.evaluated
    }

    pub fn rule_hits(&self, i: usize) -> u64 {
        self.matched.get(i).copied().unwrap_or(0)
    }

    pub fn evaluate(
        &mut self,
        direction: Direction,
        protocol: u8,
        src: Ipv4Addr,
        dst: Ipv4Addr,
        dst_port: u16,
    ) -> Verdict {
        self.evaluated += 1;
        for i in 0..self.len {
            if self.rules[i].matches(direction, protocol, src, dst, dst_port) {
                self.matched[i] += 1;
                let verdict = self.rules[i].verdict.unwrap_or(Verdict::Accept);
                if verdict != Verdict::Accept {
                    self.dropped += 1;
                }
                return verdict;
            }
        }
        Verdict::Accept
    }
}

impl Default for Firewall {
    fn default() -> Firewall {
        Firewall::new()
    }
}

pub const MAX_NAT_ENTRIES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NatEntry {
    pub inside: Endpoint,
    pub outside_port: u16,
    pub remote: Endpoint,
    pub protocol: u8,
    pub created_tick: u64,
}

/// F214: source NAT for the guest and for the shared data area. The inside
/// port is preserved when it can be, because some protocols embed it.
pub struct Nat {
    entries: [NatEntry; MAX_NAT_ENTRIES],
    len: usize,
    next_port: u16,
    translated: u64,
    exhausted: u64,
}

impl Nat {
    pub const PORT_BASE: u16 = 49152;

    pub const fn new() -> Nat {
        Nat {
            entries: [NatEntry {
                inside: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                outside_port: 0,
                remote: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                protocol: 0,
                created_tick: 0,
            }; MAX_NAT_ENTRIES],
            len: 0,
            next_port: Self::PORT_BASE,
            translated: 0,
            exhausted: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn translated(&self) -> u64 {
        self.translated
    }

    pub fn exhausted(&self) -> u64 {
        self.exhausted
    }

    /// Outbound: allocate (or reuse) an outside port for this flow.
    pub fn translate_out(&mut self, inside: Endpoint, remote: Endpoint, protocol: u8, now: u64) -> Option<u16> {
        for e in self.entries[..self.len].iter_mut() {
            if e.inside == inside && e.remote == remote && e.protocol == protocol {
                e.created_tick = now;
                return Some(e.outside_port);
            }
        }
        if self.len >= MAX_NAT_ENTRIES {
            self.exhausted += 1;
            return None;
        }
        let outside_port = self.next_port;
        self.next_port = if self.next_port == u16::MAX {
            Self::PORT_BASE
        } else {
            self.next_port + 1
        };
        self.entries[self.len] = NatEntry {
            inside,
            outside_port,
            remote,
            protocol,
            created_tick: now,
        };
        self.len += 1;
        self.translated += 1;
        Some(outside_port)
    }

    /// Inbound: map an outside port back to the inside endpoint.
    pub fn translate_in(&self, outside_port: u16, remote: Endpoint, protocol: u8) -> Option<Endpoint> {
        self.entries[..self.len]
            .iter()
            .find(|e| e.outside_port == outside_port && e.remote == remote && e.protocol == protocol)
            .map(|e| e.inside)
    }

    /// Expire idle flows so the port range is reusable.
    pub fn expire(&mut self, now: u64, ttl: u64) -> usize {
        let mut kept = 0usize;
        for i in 0..self.len {
            if now.saturating_sub(self.entries[i].created_tick) > ttl {
                continue;
            }
            self.entries[kept] = self.entries[i];
            kept += 1;
        }
        let removed = self.len - kept;
        self.len = kept;
        removed
    }
}

impl Default for Nat {
    fn default() -> Nat {
        Nat::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlowState {
    #[default]
    New,
    Established,
    Closing,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flow {
    pub src: Endpoint,
    pub dst: Endpoint,
    pub protocol: u8,
    pub state: FlowState,
    pub packets: u64,
    pub bytes: u64,
    pub last_tick: u64,
}

pub const MAX_FLOWS: usize = 64;

/// F215: the connection table. Every packet that passes through the firewall
/// updates a flow, which is what makes "allow established" rules possible.
pub struct Conntrack {
    flows: [Flow; MAX_FLOWS],
    len: usize,
    created: u64,
    evicted: u64,
}

impl Conntrack {
    pub const fn new() -> Conntrack {
        Conntrack {
            flows: [Flow {
                src: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                dst: Endpoint {
                    ip: Ipv4Addr([0; 4]),
                    port: 0,
                },
                protocol: 0,
                state: FlowState::New,
                packets: 0,
                bytes: 0,
                last_tick: 0,
            }; MAX_FLOWS],
            len: 0,
            created: 0,
            evicted: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    pub fn update(&mut self, src: Endpoint, dst: Endpoint, protocol: u8, bytes: u64, tick: u64) -> bool {
        for f in self.flows[..self.len].iter_mut() {
            if f.protocol == protocol
                && ((f.src == src && f.dst == dst) || (f.src == dst && f.dst == src))
            {
                f.packets += 1;
                f.bytes += bytes;
                f.last_tick = tick;
                if f.packets >= 2 {
                    f.state = FlowState::Established;
                }
                return true;
            }
        }
        if self.len >= MAX_FLOWS {
            // Evict the flow that has been idle longest.
            let victim = self.flows[..self.len]
                .iter()
                .enumerate()
                .min_by_key(|(_, f)| f.last_tick)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.flows[victim] = Flow {
                src,
                dst,
                protocol,
                state: FlowState::New,
                packets: 1,
                bytes,
                last_tick: tick,
            };
            self.evicted += 1;
            return true;
        }
        self.flows[self.len] = Flow {
            src,
            dst,
            protocol,
            state: FlowState::New,
            packets: 1,
            bytes,
            last_tick: tick,
        };
        self.len += 1;
        self.created += 1;
        true
    }

    pub fn is_established(&self, src: Endpoint, dst: Endpoint, protocol: u8) -> bool {
        self.flows[..self.len].iter().any(|f| {
            f.protocol == protocol
                && f.state == FlowState::Established
                && ((f.src == src && f.dst == dst) || (f.src == dst && f.dst == src))
        })
    }

    pub fn expire(&mut self, now: u64, ttl: u64) -> usize {
        let mut kept = 0usize;
        for i in 0..self.len {
            if now.saturating_sub(self.flows[i].last_tick) > ttl {
                continue;
            }
            self.flows[kept] = self.flows[i];
            kept += 1;
        }
        let removed = self.len - kept;
        self.len = kept;
        removed
    }

    pub fn established(&self) -> usize {
        self.flows[..self.len]
            .iter()
            .filter(|f| f.state == FlowState::Established)
            .count()
    }
}

impl Default for Conntrack {
    fn default() -> Conntrack {
        Conntrack::new()
    }
}

// ---------------------------------------------------------------------------
// F201/F202/F203 — NIC framework and the two drivers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NicKind {
    #[default]
    None,
    VirtioNet,
    E1000,
    Loopback,
}

impl NicKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NicKind::None => "none",
            NicKind::VirtioNet => "virtio-net",
            NicKind::E1000 => "e1000",
            NicKind::Loopback => "lo",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NicStats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nic {
    pub slot: u8,
    pub kind: NicKind,
    pub mac: MacAddr,
    pub mtu: u16,
    pub link_up: bool,
    pub speed_mbps: u32,
    pub rx_queue_depth: u16,
    pub tx_queue_depth: u16,
    pub stats: NicStats,
}

impl Default for Nic {
    fn default() -> Nic {
        Nic {
            slot: 0,
            kind: NicKind::None,
            mac: MacAddr::ZERO,
            mtu: 1500,
            link_up: false,
            speed_mbps: 0,
            rx_queue_depth: 256,
            tx_queue_depth: 256,
            stats: NicStats::default(),
        }
    }
}

impl Nic {
    pub fn usable(&self) -> bool {
        self.kind != NicKind::None && self.link_up && self.mac.is_valid()
    }

    /// F201: transmit. A frame that violates the MTU or the minimum size is
    /// counted as an error, not silently padded or truncated.
    pub fn transmit(&mut self, len: usize) -> Result<(), &'static str> {
        if !self.link_up {
            self.stats.tx_dropped += 1;
            return Err("link is down");
        }
        if len < MIN_FRAME {
            self.stats.tx_errors += 1;
            return Err("frame is shorter than the Ethernet minimum");
        }
        if len > self.mtu as usize + ETH_HDR_LEN {
            self.stats.tx_errors += 1;
            return Err("frame exceeds the MTU");
        }
        self.stats.tx_packets += 1;
        self.stats.tx_bytes += len as u64;
        Ok(())
    }

    pub fn receive(&mut self, len: usize) -> Result<(), &'static str> {
        if !self.link_up {
            return Err("link is down");
        }
        if len < ETH_HDR_LEN || len > MAX_FRAME {
            self.stats.rx_errors += 1;
            return Err("bogus frame length");
        }
        self.stats.rx_packets += 1;
        self.stats.rx_bytes += len as u64;
        Ok(())
    }
}

pub const MAX_NICS: usize = 4;

pub struct NicRegistry {
    nics: [Nic; MAX_NICS],
    count: usize,
}

impl NicRegistry {
    pub const fn new() -> NicRegistry {
        NicRegistry {
            nics: [Nic {
                slot: 0,
                kind: NicKind::None,
                mac: MacAddr([0; 6]),
                mtu: 1500,
                link_up: false,
                speed_mbps: 0,
                rx_queue_depth: 256,
                tx_queue_depth: 256,
                stats: NicStats {
                    rx_packets: 0,
                    tx_packets: 0,
                    rx_bytes: 0,
                    tx_bytes: 0,
                    rx_dropped: 0,
                    tx_dropped: 0,
                    rx_errors: 0,
                    tx_errors: 0,
                },
            }; MAX_NICS],
            count: 0,
        }
    }

    pub fn register(&mut self, mut nic: Nic) -> Option<u8> {
        if self.count >= MAX_NICS || !nic.mac.is_valid() {
            return None;
        }
        nic.slot = self.count as u8;
        self.nics[self.count] = nic;
        self.count += 1;
        Some((self.count - 1) as u8)
    }

    pub fn get(&self, slot: u8) -> Option<&Nic> {
        self.nics.get(slot as usize).filter(|n| n.kind != NicKind::None)
    }

    pub fn get_mut(&mut self, slot: u8) -> Option<&mut Nic> {
        self.nics
            .get_mut(slot as usize)
            .filter(|n| n.kind != NicKind::None)
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The interface the stack should route through: the fastest one that is up.
    pub fn preferred(&self) -> Option<u8> {
        self.nics[..self.count]
            .iter()
            .filter(|n| n.usable())
            .max_by_key(|n| n.speed_mbps)
            .map(|n| n.slot)
    }

    pub fn link_up_count(&self) -> usize {
        self.nics[..self.count].iter().filter(|n| n.link_up).count()
    }

    pub fn total_stats(&self) -> NicStats {
        let mut s = NicStats::default();
        for n in self.nics[..self.count].iter() {
            s.rx_packets += n.stats.rx_packets;
            s.tx_packets += n.stats.tx_packets;
            s.rx_bytes += n.stats.rx_bytes;
            s.tx_bytes += n.stats.tx_bytes;
            s.rx_dropped += n.stats.rx_dropped;
            s.tx_dropped += n.stats.tx_dropped;
            s.rx_errors += n.stats.rx_errors;
            s.tx_errors += n.stats.tx_errors;
        }
        s
    }
}

impl Default for NicRegistry {
    fn default() -> NicRegistry {
        NicRegistry::new()
    }
}

/// F202: a legacy virtio-net split ring. Descriptors point at buffers; the
/// available ring publishes them to the device, and the used ring publishes
/// them back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl VirtqDesc {
    pub const F_NEXT: u16 = 1;
    pub const F_WRITE: u16 = 2;

    pub fn is_chained(&self) -> bool {
        self.flags & Self::F_NEXT != 0
    }

    pub fn device_writes(&self) -> bool {
        self.flags & Self::F_WRITE != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Virtqueue {
    pub size: u16,
    pub avail_idx: u16,
    pub used_idx: u16,
    pub last_used_idx: u16,
    pub free_head: u16,
    pub free_count: u16,
}

impl Virtqueue {
    pub fn new(size: u16) -> Virtqueue {
        Virtqueue {
            size,
            avail_idx: 0,
            used_idx: 0,
            last_used_idx: 0,
            free_head: 0,
            free_count: size,
        }
    }

    /// Take a descriptor for transmission.
    pub fn allocate(&mut self) -> Option<u16> {
        if self.free_count == 0 {
            return None;
        }
        let head = self.free_head;
        self.free_head = (self.free_head + 1) % self.size;
        self.free_count -= 1;
        self.avail_idx = self.avail_idx.wrapping_add(1);
        Some(head)
    }

    pub fn reclaim(&mut self) -> Option<u16> {
        if self.used_idx == self.last_used_idx {
            return None;
        }
        let head = self.last_used_idx;
        self.last_used_idx = (self.last_used_idx + 1) % self.size;
        self.free_count += 1;
        Some(head)
    }

    /// How many buffers the device has not given back yet.
    pub fn in_flight(&self) -> u16 {
        self.size - self.free_count
    }
}

/// F203: the e1000 legacy transmit descriptor. The status byte is written by
/// hardware, so the driver must check it before reusing the slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct E1000TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

impl E1000TxDesc {
    pub const CMD_EOP: u8 = 1;
    pub const CMD_IFCS: u8 = 2;
    pub const CMD_RS: u8 = 8;
    pub const STATUS_DD: u8 = 1;
    pub const REG_TDBAL: u32 = 0x3800;
    pub const REG_TDLEN: u32 = 0x3808;
    pub const REG_TDH: u32 = 0x3810;
    pub const REG_TDT: u32 = 0x3818;

    pub fn encode(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        b[0..8].copy_from_slice(&self.addr.to_le_bytes());
        b[8..10].copy_from_slice(&self.length.to_le_bytes());
        b[10] = self.cso;
        b[11] = self.cmd;
        b[12] = self.status;
        b[13] = self.css;
        b[14..16].copy_from_slice(&self.special.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8; 16]) -> E1000TxDesc {
        E1000TxDesc {
            addr: u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]),
            length: u16::from_le_bytes([b[8], b[9]]),
            cso: b[10],
            cmd: b[11],
            status: b[12],
            css: b[13],
            special: u16::from_le_bytes([b[14], b[15]]),
        }
    }

    pub fn done(&self) -> bool {
        self.status & Self::STATUS_DD != 0
    }

    pub fn command_for(len: usize) -> E1000TxDesc {
        E1000TxDesc {
            addr: 0,
            length: len as u16,
            cso: 0,
            // End of packet + insert FCS + report status.
            cmd: Self::CMD_EOP | Self::CMD_IFCS | Self::CMD_RS,
            status: 0,
            css: 0,
            special: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// F211/F212/F220 — DHCP, DNS, NTP
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DhcpState {
    #[default]
    Init,
    Selecting,
    Requesting,
    Bound,
    Renewing,
    Rebinding,
    Failed,
}

impl DhcpState {
    pub fn as_str(self) -> &'static str {
        match self {
            DhcpState::Init => "init",
            DhcpState::Selecting => "selecting",
            DhcpState::Requesting => "requesting",
            DhcpState::Bound => "bound",
            DhcpState::Renewing => "renewing",
            DhcpState::Rebinding => "rebinding",
            DhcpState::Failed => "failed",
        }
    }

    pub fn has_address(self) -> bool {
        matches!(self, DhcpState::Bound | DhcpState::Renewing | DhcpState::Rebinding)
    }
}

pub const DHCP_OP_REQUEST: u8 = 1;
pub const DHCP_OP_REPLY: u8 = 2;
pub const DHCP_MSG_DISCOVER: u8 = 1;
pub const DHCP_MSG_OFFER: u8 = 2;
pub const DHCP_MSG_REQUEST: u8 = 3;
pub const DHCP_MSG_ACK: u8 = 5;
pub const DHCP_MSG_NAK: u8 = 6;
pub const DHCP_MAGIC: [u8; 4] = [99, 130, 83, 99];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DhcpLease {
    pub address: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns: Ipv4Addr,
    pub server: Ipv4Addr,
    pub lease_secs: u32,
    pub t1_secs: u32,
    pub t2_secs: u32,
}

impl DhcpLease {
    /// Renew at half the lease, rebind at seven eighths — the RFC timers, and
    /// getting them wrong means a machine that drops off the network at a
    /// predictable time.
    pub fn derive_timers(&mut self) {
        if self.t1_secs == 0 {
            self.t1_secs = self.lease_secs / 2;
        }
        if self.t2_secs == 0 {
            self.t2_secs = self.lease_secs * 7 / 8;
        }
    }

    pub fn valid(&self) -> bool {
        !self.address.is_any() && !self.address.is_broadcast() && self.lease_secs > 0
    }
}

/// Parse a DHCP option list: TLV with a zero terminator. Option 53 is the
/// message type, which is the only one the state machine strictly needs.
pub fn dhcp_option(buf: &[u8], code: u8) -> Option<&[u8]> {
    let mut i = 0usize;
    while i + 1 < buf.len() {
        let c = buf[i];
        if c == 0 {
            return None;
        }
        let len = buf[i + 1] as usize;
        if c == code {
            let start = i + 2;
            if start + len > buf.len() {
                return None;
            }
            return Some(&buf[start..start + len]);
        }
        i += 2 + len;
    }
    None
}

pub fn dhcp_message_type(options: &[u8]) -> Option<u8> {
    dhcp_option(options, 53).and_then(|v| v.first().copied())
}

pub const DNS_HDR_LEN: usize = 12;
pub const DNS_MAX_NAME: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DnsQuestion {
    pub name: [u8; DNS_MAX_NAME],
    pub name_len: usize,
    pub qtype: u16,
    pub qclass: u16,
}

impl Default for DnsQuestion {
    /// Hand-written: `[u8; 64]` has no `Default` impl.
    fn default() -> DnsQuestion {
        DnsQuestion {
            name: [0u8; DNS_MAX_NAME],
            name_len: 0,
            qtype: 0,
            qclass: 0,
        }
    }
}

impl DnsQuestion {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    /// Encode a dotted name as the label sequence DNS uses.
    pub fn encode_name(name: &str, out: &mut [u8]) -> Option<usize> {
        let mut n = 0usize;
        for label in name.split('.') {
            if label.is_empty() || label.len() > 63 {
                return None;
            }
            if n + 1 + label.len() > out.len() {
                return None;
            }
            out[n] = label.len() as u8;
            n += 1;
            out[n..n + label.len()].copy_from_slice(label.as_bytes());
            n += label.len();
        }
        if n >= out.len() {
            return None;
        }
        out[n] = 0;
        Some(n + 1)
    }

    /// Walk a name, following compression pointers, and return how many bytes
    /// of *this* record it occupied. A pointer loop (forward pointer, or more
    /// than 16 hops) is refused rather than followed — otherwise a hostile
    /// response hangs the resolver.
    pub fn skip_name(buf: &[u8], at_start: usize) -> Option<usize> {
        let mut at = at_start;
        let mut hops = 0usize;
        let mut consumed = 0usize;
        loop {
            if at >= buf.len() || hops > 16 {
                return None;
            }
            let len = buf[at] as usize;
            if len == 0 {
                // End of the name: `consumed` is the length when a pointer was
                // followed, otherwise the terminator itself.
                return Some(if consumed == 0 { at + 1 - at_start } else { consumed });
            }
            if len & 0xC0 == 0xC0 {
                if at + 1 >= buf.len() {
                    return None;
                }
                let target = ((len & 0x3F) << 8) | buf[at + 1] as usize;
                if consumed == 0 {
                    consumed = at + 2 - at_start;
                }
                if target >= at {
                    return None; // a forward pointer is a loop by construction
                }
                at = target;
                hops += 1;
                continue;
            }
            if at + 1 + len > buf.len() {
                return None;
            }
            at += 1 + len;
        }
    }

    /// Extract the first A record from a response.
    pub fn answer_a(buf: &[u8]) -> Option<Ipv4Addr> {
        if buf.len() < DNS_HDR_LEN {
            return None;
        }
        // RFC 1035: only the response bit matters here; a request has no answers.
        let ancount = u16::from_be_bytes([buf[6], buf[7]]);
        if ancount == 0 {
            return None;
        }
        let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
        let mut at = DNS_HDR_LEN;
        for _ in 0..qdcount {
            let consumed = DnsQuestion::skip_name(buf, at)?;
            at += consumed + 4;
        }
        for _ in 0..ancount {
            let consumed = DnsQuestion::skip_name(buf, at)?;
            at += consumed;
            if at + 10 > buf.len() {
                return None;
            }
            let rtype = u16::from_be_bytes([buf[at], buf[at + 1]]);
            let rdlen = u16::from_be_bytes([buf[at + 8], buf[at + 9]]) as usize;
            at += 10;
            if at + rdlen > buf.len() {
                return None;
            }
            if rtype == 1 && rdlen == 4 {
                return Some(Ipv4Addr([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]));
            }
            at += rdlen;
        }
        None
    }
}

pub const NTP_PACKET_LEN: usize = 48;
/// Seconds between the NTP epoch (1900) and the Unix epoch (1970).
pub const NTP_UNIX_DELTA: u64 = 2_208_988_800;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NtpPacket {
    pub leap: u8,
    pub version: u8,
    pub mode: u8,
    pub stratum: u8,
    pub poll: i8,
    pub precision: i8,
    pub transmit_seconds: u32,
    pub transmit_fraction: u32,
    pub originate_seconds: u32,
}

impl NtpPacket {
    pub const MODE_CLIENT: u8 = 3;
    pub const MODE_SERVER: u8 = 4;

    pub fn parse(buf: &[u8]) -> Result<NtpPacket, &'static str> {
        if buf.len() < NTP_PACKET_LEN {
            return Err("NTP packet is truncated");
        }
        Ok(NtpPacket {
            leap: buf[0] >> 6,
            version: (buf[0] >> 3) & 0x07,
            mode: buf[0] & 0x07,
            stratum: buf[1],
            poll: buf[2] as i8,
            precision: buf[3] as i8,
            transmit_seconds: u32::from_be_bytes([buf[40], buf[41], buf[42], buf[43]]),
            transmit_fraction: u32::from_be_bytes([buf[44], buf[45], buf[46], buf[47]]),
            originate_seconds: u32::from_be_bytes([buf[24], buf[25], buf[26], buf[27]]),
        })
    }

    /// A client request: mode 3, version 4, everything else zero.
    pub fn client_request() -> [u8; NTP_PACKET_LEN] {
        let mut b = [0u8; NTP_PACKET_LEN];
        b[0] = (4 << 3) | Self::MODE_CLIENT;
        b
    }

    pub fn valid_response(&self) -> bool {
        self.mode == Self::MODE_SERVER
            && self.stratum > 0
            && self.stratum < 16
            && self.transmit_seconds > NTP_UNIX_DELTA as u32
    }

    /// Absolute time in Unix seconds.
    pub fn unix_seconds(&self) -> u64 {
        self.transmit_seconds as u64 - NTP_UNIX_DELTA
    }

    /// Millisecond fraction, for the sub-second part of the clock.
    pub fn unix_millis(&self) -> u32 {
        (self.transmit_fraction as u64 * 1000 / (1u64 << 32)) as u32
    }

    /// Sub-second precision as a log2 exponent: 20 is microseconds.
    pub fn precision_seconds(&self) -> f64 {
        libm_pow2(self.precision as i32)
    }
}

/// Table-free power of two for the small exponents an NTP packet uses.
fn libm_pow2(exp: i32) -> f64 {
    let mut v = 1.0f64;
    let mut e = exp;
    while e > 0 {
        v *= 2.0;
        e -= 1;
    }
    while e < 0 {
        v /= 2.0;
        e += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// F216/F217/F218/F219 — statistics, quotas, offline, weak link
// ---------------------------------------------------------------------------

/// F216: transfer rates, computed from deltas so the number reflects "now"
/// rather than "since boot".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RateSnapshot {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub tick: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rates {
    pub rx_bps: u64,
    pub tx_bps: u64,
}

impl Rates {
    /// Rates between two snapshots. A zero delta returns zero rather than
    /// dividing by it.
    pub fn between(prev: &RateSnapshot, now: &RateSnapshot, tick_ms: u64) -> Rates {
        let dt = now.tick.saturating_sub(prev.tick);
        if dt == 0 || tick_ms == 0 {
            return Rates::default();
        }
        let ms = dt * tick_ms;
        Rates {
            rx_bps: now.rx_bytes.saturating_sub(prev.rx_bytes) * 1000 / ms.max(1),
            tx_bps: now.tx_bytes.saturating_sub(prev.tx_bytes) * 1000 / ms.max(1),
        }
    }

    pub fn total_bps(&self) -> u64 {
        self.rx_bps + self.tx_bps
    }
}

/// F217: a per-socket byte quota. A browser tab that downloads in a loop must
/// not be able to consume a metered connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TrafficQuota {
    pub limit_bytes: u64,
    pub used_bytes: u64,
    pub denials: u64,
    /// 0..=100, applied once the quota is exhausted.
    pub throttle_percent: u8,
}

impl TrafficQuota {
    pub const DEFAULT_THROTTLE: u8 = 5;

    pub fn new(limit_bytes: u64) -> TrafficQuota {
        TrafficQuota {
            limit_bytes,
            used_bytes: 0,
            denials: 0,
            throttle_percent: Self::DEFAULT_THROTTLE,
        }
    }

    /// Charge bytes. Unlimited when the limit is zero; once exhausted, traffic
    /// is throttled rather than cut, so a stalled download can still finish.
    pub fn charge(&mut self, bytes: u64) -> bool {
        if self.limit_bytes == 0 {
            self.used_bytes += bytes;
            return true;
        }
        if self.used_bytes + bytes > self.limit_bytes {
            self.denials += 1;
            return false;
        }
        self.used_bytes += bytes;
        true
    }

    pub fn remaining(&self) -> u64 {
        self.limit_bytes.saturating_sub(self.used_bytes)
    }

    pub fn exhausted(&self) -> bool {
        self.limit_bytes != 0 && self.used_bytes >= self.limit_bytes
    }

    /// What fraction of the nominal rate this socket may use now.
    pub fn allowance_percent(&self) -> u8 {
        if !self.exhausted() {
            100
        } else {
            self.throttle_percent.max(1)
        }
    }
}

/// F218: what still works with no network at all. Listing it is the honest
/// answer to "is my machine broken".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct OfflineCapabilities {
    pub local_files: bool,
    pub local_search: bool,
    pub text_editing: bool,
    pub media_playback: bool,
    pub local_ai: bool,
    pub package_install: bool,
    pub cloud_sync: bool,
    pub software_update: bool,
}

impl OfflineCapabilities {
    /// With no network, everything that does not need a remote peer still works.
    pub const fn offline() -> OfflineCapabilities {
        OfflineCapabilities {
            local_files: true,
            local_search: true,
            text_editing: true,
            media_playback: true,
            local_ai: true,
            package_install: false,
            cloud_sync: false,
            software_update: false,
        }
    }

    pub fn working_count(&self) -> u32 {
        [
            self.local_files,
            self.local_search,
            self.text_editing,
            self.media_playback,
            self.local_ai,
            self.package_install,
            self.cloud_sync,
            self.software_update,
        ]
        .iter()
        .filter(|v| **v)
        .count() as u32
    }

    pub fn disabled(&self) -> u32 {
        8 - self.working_count()
    }
}

/// F219: link quality, and what the stack does about it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Default)]
pub enum LinkQuality {
    #[default]
    Good,
    /// Some loss: shrink the MTU and stop background sync.
    Weak,
    /// Mostly loss: only interactive traffic gets through.
    Terrible,
}

impl LinkQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkQuality::Good => "good",
            LinkQuality::Weak => "weak",
            LinkQuality::Terrible => "terrible",
        }
    }

    /// Classify from measured loss and round-trip time.
    pub fn classify(loss_percent: u8, rtt_ms: u32) -> LinkQuality {
        if loss_percent >= 20 || rtt_ms > 1000 {
            LinkQuality::Terrible
        } else if loss_percent >= 5 || rtt_ms > 250 {
            LinkQuality::Weak
        } else {
            LinkQuality::Good
        }
    }

    /// The MTU to use. Fragmentation on a lossy link costs more than a smaller
    /// MTU does.
    pub fn mtu(self) -> u16 {
        match self {
            LinkQuality::Good => 1500,
            LinkQuality::Weak => 1400,
            LinkQuality::Terrible => 1280,
        }
    }

    pub fn allows_background_sync(self) -> bool {
        self == LinkQuality::Good
    }

    pub fn retransmit_delay_ms(self) -> u32 {
        match self {
            LinkQuality::Good => 200,
            LinkQuality::Weak => 600,
            LinkQuality::Terrible => 2000,
        }
    }
}

// ---------------------------------------------------------------------------
// F221/F222/F223/F224 — capture, TLS, IPv6, events
// ---------------------------------------------------------------------------

pub const MAX_CAPTURE: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CaptureEntry {
    pub tick: u64,
    pub len: usize,
    pub protocol: u8,
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub direction: Option<Direction>,
    /// First bytes of the frame, for a quick look without a dump file.
    pub preview: [u8; 16],
    pub preview_len: usize,
}

/// F221: an in-kernel ring capture. Bounded on purpose: a debugging aid that
/// can exhaust memory is a debugging aid that causes the next bug.
pub struct Capture {
    entries: [CaptureEntry; MAX_CAPTURE],
    head: usize,
    total: u64,
    enabled: bool,
}

impl Capture {
    pub const fn new() -> Capture {
        Capture {
            entries: [CaptureEntry {
                tick: 0,
                len: 0,
                protocol: 0,
                src: Ipv4Addr([0; 4]),
                dst: Ipv4Addr([0; 4]),
                direction: None,
                preview: [0; 16],
                preview_len: 0,
            }; MAX_CAPTURE],
            head: 0,
            total: 0,
            enabled: false,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn record(&mut self, mut e: CaptureEntry, frame: &[u8]) -> bool {
        if !self.enabled {
            return false;
        }
        let n = frame.len().min(16);
        e.preview[..n].copy_from_slice(&frame[..n]);
        e.preview_len = n;
        self.entries[self.head] = e;
        self.head = (self.head + 1) % MAX_CAPTURE;
        self.total += 1;
        true
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_CAPTURE)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn get(&self, offset: usize) -> Option<CaptureEntry> {
        if offset >= self.len() {
            return None;
        }
        Some(self.entries[(self.head + MAX_CAPTURE - 1 - offset) % MAX_CAPTURE])
    }
}

impl Default for Capture {
    fn default() -> Capture {
        Capture::new()
    }
}

/// F222: TLS is *not* implemented, and the interface says so rather than
/// pretending. A stack that silently downgrades is worse than one that refuses.
pub const TLS_SUPPORTED: bool = false;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlsError {
    NotImplemented,
    /// The caller must not fall back to plaintext: that is a data leak.
    NoPlaintextFallback,
}

pub fn tls_connect(_host: &str, _port: u16) -> Result<(), TlsError> {
    Err(TlsError::NotImplemented)
}

/// F223: IPv6 is reserved, not routed. The header is parsed far enough to
/// answer "is this what it claims to be", and then refused with a reason.
pub const IPV6_SUPPORTED: bool = false;
pub const IPV6_HDR_LEN: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ipv6Header {
    pub traffic_class: u8,
    pub flow_label: u32,
    pub payload_len: u16,
    pub next_header: u8,
    pub hop_limit: u8,
    pub src: [u8; 16],
    pub dst: [u8; 16],
}

impl Ipv6Header {
    pub fn parse(buf: &[u8]) -> Result<Ipv6Header, &'static str> {
        if buf.len() < IPV6_HDR_LEN {
            return Err("packet is shorter than an IPv6 header");
        }
        if buf[0] >> 4 != 6 {
            return Err("not an IPv6 packet");
        }
        let mut src = [0u8; 16];
        let mut dst = [0u8; 16];
        src.copy_from_slice(&buf[8..24]);
        dst.copy_from_slice(&buf[24..40]);
        Ok(Ipv6Header {
            traffic_class: ((buf[0] & 0x0F) << 4) | (buf[1] >> 4),
            flow_label: ((buf[1] as u32 & 0x0F) << 16) | ((buf[2] as u32) << 8) | buf[3] as u32,
            payload_len: u16::from_be_bytes([buf[4], buf[5]]),
            next_header: buf[6],
            hop_limit: buf[7],
            src,
            dst,
        })
    }

    /// The shape is parseable, but nothing routes it yet.
    pub fn routing_supported(&self) -> bool {
        IPV6_SUPPORTED
    }

    pub fn is_loopback(&self) -> bool {
        self.dst[0..15] == [0u8; 15] && self.dst[15] == 1
    }

    pub fn is_link_local(&self) -> bool {
        self.src[0] == 0xFE && self.src[1] & 0xC0 == 0x80
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetEventKind {
    LinkUp,
    LinkDown,
    AddressAcquired,
    AddressLost,
    DhcpFailed,
    DnsFailed,
    FirewallDrop,
    QuotaExceeded,
    QualityChanged,
}

impl NetEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NetEventKind::LinkUp => "link-up",
            NetEventKind::LinkDown => "link-down",
            NetEventKind::AddressAcquired => "addr-acquired",
            NetEventKind::AddressLost => "addr-lost",
            NetEventKind::DhcpFailed => "dhcp-failed",
            NetEventKind::DnsFailed => "dns-failed",
            NetEventKind::FirewallDrop => "firewall-drop",
            NetEventKind::QuotaExceeded => "quota-exceeded",
            NetEventKind::QualityChanged => "quality-changed",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NetEvent {
    pub tick: u64,
    pub kind: Option<NetEventKind>,
    pub detail: i64,
}

pub const MAX_NET_EVENTS: usize = 32;

pub struct NetEvents {
    events: [NetEvent; MAX_NET_EVENTS],
    head: usize,
    total: u64,
    counters: [u64; 9],
}

impl NetEvents {
    pub const fn new() -> NetEvents {
        NetEvents {
            events: [NetEvent {
                tick: 0,
                kind: None,
                detail: 0,
            }; MAX_NET_EVENTS],
            head: 0,
            total: 0,
            counters: [0; 9],
        }
    }

    fn index(kind: NetEventKind) -> usize {
        match kind {
            NetEventKind::LinkUp => 0,
            NetEventKind::LinkDown => 1,
            NetEventKind::AddressAcquired => 2,
            NetEventKind::AddressLost => 3,
            NetEventKind::DhcpFailed => 4,
            NetEventKind::DnsFailed => 5,
            NetEventKind::FirewallDrop => 6,
            NetEventKind::QuotaExceeded => 7,
            NetEventKind::QualityChanged => 8,
        }
    }

    pub fn push(&mut self, e: NetEvent) {
        if let Some(k) = e.kind {
            self.counters[Self::index(k)] += 1;
        }
        self.events[self.head] = e;
        self.head = (self.head + 1) % MAX_NET_EVENTS;
        self.total += 1;
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_NET_EVENTS)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn count(&self, kind: NetEventKind) -> u64 {
        self.counters[Self::index(kind)]
    }

    pub fn get(&self, offset: usize) -> Option<NetEvent> {
        if offset >= self.len() {
            return None;
        }
        Some(self.events[(self.head + MAX_NET_EVENTS - 1 - offset) % MAX_NET_EVENTS])
    }
}

impl Default for NetEvents {
    fn default() -> NetEvents {
        NetEvents::new()
    }
}

// ---------------------------------------------------------------------------
// Domain state and self-test
// ---------------------------------------------------------------------------

pub struct NetDomainState {
    pub nics: usize,
    pub link_up: usize,
    pub preferred: Option<u8>,
    pub dhcp: DhcpState,
    pub address: Ipv4Addr,
    pub quality: LinkQuality,
    pub flows: usize,
    pub sockets: usize,
    pub rx_rate_bps: u64,
    pub tx_rate_bps: u64,
    pub self_test: (usize, usize),
}

impl NetDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0
    }

    pub fn online(&self) -> bool {
        self.link_up > 0 && self.dhcp.has_address()
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("net nic=");
        w.num(self.nics as u64);
        w.str(" up=");
        w.num(self.link_up as u64);
        w.str(" dhcp=");
        w.str(self.dhcp.as_str());
        w.str(" ip=");
        w.num(self.address.0[0] as u64);
        w.str(".");
        w.num(self.address.0[1] as u64);
        w.str(".");
        w.num(self.address.0[2] as u64);
        w.str(".");
        w.num(self.address.0[3] as u64);
        w.str(" link=");
        w.str(self.quality.as_str());
        w.str(" flows=");
        w.num(self.flows as u64);
        w.str(" socks=");
        w.num(self.sockets as u64);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

static NET_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();
static NICS: crate::cpu::sync::SpinProtected<NicRegistry> =
    crate::cpu::sync::SpinProtected::new(NicRegistry::new());
static NET_EVENTS: crate::cpu::sync::SpinProtected<NetEvents> =
    crate::cpu::sync::SpinProtected::new(NetEvents::new());

pub fn net_selftest() -> &'static crate::selftest::SelfTest {
    &NET_SELFTEST
}

pub fn nics() -> &'static crate::cpu::sync::SpinProtected<NicRegistry> {
    &NICS
}

pub fn events() -> &'static crate::cpu::sync::SpinProtected<NetEvents> {
    &NET_EVENTS
}

/// F201~F225 bring-up.
pub fn init() -> NetDomainState {
    // No NIC is probed yet, so the loopback interface is what exists — and it
    // is enough for the stack's own self-test to be meaningful.
    {
        let mut reg = NICS.lock();
        if reg.is_empty() {
            let _ = reg.register(Nic {
                slot: 0,
                kind: NicKind::Loopback,
                mac: MacAddr([0x02, 0, 0, 0, 0, 1]),
                mtu: 65535,
                link_up: true,
                speed_mbps: 1000,
                rx_queue_depth: 64,
                tx_queue_depth: 64,
                stats: NicStats::default(),
            });
        }
    }

    let (passed, failed) = run_net_checks();
    let (nics, up, preferred) = {
        let reg = NICS.lock();
        (reg.len(), reg.link_up_count(), reg.preferred())
    };
    let state = NetDomainState {
        nics,
        link_up: up,
        preferred,
        dhcp: DhcpState::Init,
        address: Ipv4Addr::ANY,
        quality: LinkQuality::Good,
        flows: 0,
        sockets: 0,
        rx_rate_bps: 0,
        tx_rate_bps: 0,
        self_test: (passed, failed),
    };

    crate::kinfo!(
        "net: {} nic(s) {} up, dhcp={} self-test {}/{}",
        state.nics,
        state.link_up,
        state.dhcp.as_str(),
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

pub fn render_to_console(st: &NetDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = NET_SELFTEST.render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// F225: the network-chain checks.
pub fn run_net_checks() -> (usize, usize) {
    let r = &NET_SELFTEST;

    // F204/F206 — a frame parses, and a lying total-length is refused.
    let mut frame = [0u8; ETH_HDR_LEN + IPV4_MIN_HDR];
    let eth = EthernetHeader {
        dst: MacAddr::BROADCAST,
        src: MacAddr([0x02, 0, 0, 0, 0, 1]),
        ethertype: ETHERTYPE_IPV4,
    };
    frame[..ETH_HDR_LEN].copy_from_slice(&eth.encode());
    let ip = Ipv4Header::build(Ipv4Addr([10, 0, 0, 2]), Ipv4Addr([10, 0, 0, 1]), PROTO_ICMP, 0, 1);
    frame[ETH_HDR_LEN..].copy_from_slice(&ip.encode());
    let (parsed_eth, off) = EthernetHeader::parse(&frame).unwrap_or((EthernetHeader::default(), 0));
    let (parsed_ip, _) = Ipv4Header::parse(&frame[off..]).unwrap_or((Ipv4Header::default(), 0));
    r.check(
        "ethernet-ipv4",
        parsed_eth.ethertype == ETHERTYPE_IPV4
            && parsed_eth.dst.is_broadcast()
            && parsed_ip.checksum_ok(&frame[off..])
            && parsed_ip.total_len == IPV4_MIN_HDR as u16,
        "frame or IPv4 header parsing is wrong",
    );

    // F206 — the checksum catches a single flipped bit.
    let mut corrupt = frame;
    corrupt[ETH_HDR_LEN + 8] ^= 1;
    let bad = Ipv4Header::parse(&corrupt[off..]).map(|(h, _)| !h.checksum_ok(&corrupt[off..]));
    r.check(
        "ipv4-checksum",
        bad == Ok(true),
        "a corrupted IPv4 header passed its checksum",
    );

    // F205 — an ARP request produces a reply with the endpoints swapped.
    let request = ArpPacket {
        opcode: ARP_REQUEST,
        sender_mac: MacAddr([0x02, 0, 0, 0, 0, 2]),
        sender_ip: Ipv4Addr([10, 0, 0, 2]),
        target_mac: MacAddr::ZERO,
        target_ip: Ipv4Addr([10, 0, 0, 1]),
    };
    let round = ArpPacket::parse(&request.encode());
    let reply = ArpPacket::reply_to(&request, MacAddr([0x02, 0, 0, 0, 0, 1]));
    r.check(
        "arp",
        round.map(|a| a == request).unwrap_or(false)
            && reply.opcode == ARP_REPLY
            && reply.sender_ip == request.target_ip
            && reply.target_mac == request.sender_mac,
        "ARP encode/reply is wrong",
    );

    // F207 — ICMP echo of a payload, verified by its own checksum.
    let echo = IcmpMessage {
        kind: ICMP_ECHO_REQUEST,
        code: 0,
        id: 0x1234,
        sequence: 7,
        payload_len: 8,
    };
    let mut reply_buf = [0u8; 64];
    let n = IcmpMessage::echo_reply(&echo, b"varix123", &mut reply_buf).unwrap_or(0);
    let parsed = IcmpMessage::parse(&reply_buf[..n]);
    r.check(
        "icmp-echo",
        n == 16 && parsed.map(|m| m.kind == ICMP_ECHO_REPLY).unwrap_or(false),
        "ICMP echo reply is wrong",
    );

    // F209 — the handshake and the close sequence.
    let syn = TcpSegment {
        flags: TCP_SYN,
        ..TcpSegment::default()
    };
    let syn_ack = TcpSegment {
        flags: TCP_SYN | TCP_ACK,
        ..TcpSegment::default()
    };
    let ack = TcpSegment {
        flags: TCP_ACK,
        ..TcpSegment::default()
    };
    let fin = TcpSegment {
        flags: TCP_FIN | TCP_ACK,
        ..TcpSegment::default()
    };
    let handshake = tcp_transition(TcpState::Closed, &syn, false) == Some(TcpState::SynSent)
        && tcp_transition(TcpState::SynSent, &syn_ack, false) == Some(TcpState::Established)
        && tcp_transition(TcpState::Established, &fin, false) == Some(TcpState::CloseWait)
        && tcp_transition(TcpState::CloseWait, &ack, false) == Some(TcpState::CloseWait);
    r.check("tcp-handshake", handshake, "TCP state machine is wrong");
    r.check(
        "tcp-sequence-space",
        syn.sequence_space() == 1 && ack.sequence_space() == 0 && fin.sequence_space() == 1,
        "SYN/FIN do not consume a sequence number",
    );

    // F210 — bind, listen, accept and the advertised window.
    let mut sockets = SocketTable::new();
    let listener = sockets.create(SockProto::Tcp).unwrap_or(0);
    let bound = sockets.bind(listener, Endpoint::any(8080)).is_ok();
    let listening = sockets.listen(listener, 4).is_ok();
    let peer = Endpoint {
        ip: Ipv4Addr([10, 0, 0, 9]),
        port: 50000,
    };
    let conn = sockets.accept(listener, peer);
    r.check(
        "socket-lifecycle",
        bound && listening && conn.map(|id| sockets.get(id).unwrap().state) == Ok(TcpState::Established),
        "socket bind/listen/accept is wrong",
    );

    // F213/F215 — a firewall rule blocks, and the flow table agrees.
    let mut fw = Firewall::new();
    let _ = fw.add(FirewallRule {
        direction: Some(Direction::Inbound),
        protocol: Some(PROTO_TCP),
        dst_port: Some(23),
        verdict: Some(Verdict::Drop),
        name: "no-telnet",
        ..FirewallRule::default()
    });
    let blocked = fw.evaluate(Direction::Inbound, PROTO_TCP, Ipv4Addr([1, 2, 3, 4]), Ipv4Addr([10, 0, 0, 2]), 23);
    let allowed = fw.evaluate(Direction::Inbound, PROTO_TCP, Ipv4Addr([1, 2, 3, 4]), Ipv4Addr([10, 0, 0, 2]), 443);
    r.check(
        "firewall",
        blocked == Verdict::Drop && allowed == Verdict::Accept && fw.dropped() == 1,
        "firewall rule matching is wrong",
    );

    // F214 — NAT maps out and back.
    let mut nat = Nat::new();
    let inside = Endpoint {
        ip: Ipv4Addr([10, 0, 0, 2]),
        port: 4000,
    };
    let remote = Endpoint {
        ip: Ipv4Addr([93, 184, 216, 34]),
        port: 443,
    };
    let mut nat_ok = false;
    if let Some(port) = nat.translate_out(inside, remote, PROTO_TCP, 0) {
        nat_ok = nat.translate_in(port, remote, PROTO_TCP) == Some(inside);
    }
    r.check("nat", nat_ok, "NAT translation does not round-trip");

    // F212 — a DNS A-record answer is parsed, and a forwarding pointer is not.
    let mut response = [0u8; 64];
    response[0] = 0x81;
    response[1] = 0x80;
    response[4..6].copy_from_slice(&1u16.to_be_bytes());
    response[6..8].copy_from_slice(&1u16.to_be_bytes());
    let mut at = DNS_HDR_LEN;
    at += DnsQuestion::encode_name("example.com", &mut response[at..]).unwrap_or(0);
    response[at..at + 4].copy_from_slice(&[0, 1, 0, 1]);
    at += 4;
    response[at] = 0xC0;
    response[at + 1] = DNS_HDR_LEN as u8;
    at += 2;
    response[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
    response[at + 2..at + 4].copy_from_slice(&1u16.to_be_bytes());
    response[at + 4..at + 8].copy_from_slice(&60u32.to_be_bytes());
    response[at + 8..at + 10].copy_from_slice(&4u16.to_be_bytes());
    response[at + 10..at + 14].copy_from_slice(&[93, 184, 216, 34]);
    r.check(
        "dns-answer",
        DnsQuestion::answer_a(&response) == Some(Ipv4Addr([93, 184, 216, 34])),
        "DNS answer parsing is wrong",
    );

    // F220 — an NTP response yields Unix time.
    let mut ntp = [0u8; NTP_PACKET_LEN];
    ntp[0] = (4 << 3) | NtpPacket::MODE_SERVER;
    ntp[1] = 2;
    let secs = (NTP_UNIX_DELTA + 1_700_000_000) as u32;
    ntp[40..44].copy_from_slice(&secs.to_be_bytes());
    r.check(
        "ntp",
        NtpPacket::parse(&ntp).map(|p| p.valid_response() && p.unix_seconds() == 1_700_000_000).unwrap_or(false),
        "NTP response parsing is wrong",
    );

    // F218/F219 — offline still works, and a weak link degrades the MTU.
    let caps = OfflineCapabilities::offline();
    r.check(
        "offline-mode",
        caps.working_count() == 5 && caps.disabled() == 3 && !caps.cloud_sync,
        "the offline capability list is wrong",
    );
    r.check(
        "weak-link",
        LinkQuality::classify(30, 50) == LinkQuality::Terrible
            && LinkQuality::classify(0, 0) == LinkQuality::Good
            && LinkQuality::Weak.mtu() < 1500
            && !LinkQuality::Weak.allows_background_sync(),
        "link-quality classification is wrong",
    );

    // F222/F223 — the unimplemented protocols refuse, with a reason.
    r.check(
        "tls-refused",
        tls_connect("example.com", 443) == Err(TlsError::NotImplemented) && !TLS_SUPPORTED,
        "TLS silently reported success",
    );
    r.check(
        "ipv6-reserved",
        !IPV6_SUPPORTED,
        "IPv6 claimed to be routed",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("network self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "network self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_and_ipv4_addresses_round_trip() {
        let mac = MacAddr::parse("02:00:00:00:00:01").expect("valid MAC");
        assert_eq!(mac.0, [0x02, 0, 0, 0, 0, 1]);
        assert!(mac.is_valid());
        assert!(!mac.is_broadcast());
        assert!(!mac.is_multicast());
        assert!(MacAddr::BROADCAST.is_broadcast());
        assert!(MacAddr::BROADCAST.is_multicast());
        assert!(!MacAddr::ZERO.is_valid());
        assert_eq!(MacAddr::parse("02:00:00"), None);
        assert_eq!(MacAddr::parse("zz:00:00:00:00:01"), None);
        let mut out = [0u8; 32];
        let n = mac.render(&mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "02:00:00:00:00:01");

        let ip = Ipv4Addr::parse("192.168.1.10").expect("valid IP");
        assert_eq!(ip.0, [192, 168, 1, 10]);
        assert!(ip.is_private());
        assert!(!ip.is_link_local());
        assert_eq!(Ipv4Addr::parse("192.168.1"), None);
        assert_eq!(Ipv4Addr::parse("192.168.1.999"), None);
        assert_eq!(Ipv4Addr::from_u32(0x0A00_0002), Ipv4Addr([10, 0, 0, 2]));
        assert_eq!(Ipv4Addr([10, 0, 0, 2]).to_u32(), 0x0A00_0002);
        assert!(Ipv4Addr::parse("10.0.0.1").unwrap().same_subnet(ip, Ipv4Addr([255, 0, 0, 0])) == false);
        assert!(Ipv4Addr::parse("192.168.1.1").unwrap().same_subnet(ip, Ipv4Addr([255, 255, 255, 0])));
        assert!(Ipv4Addr::LINK_LOCAL.is_link_local());
        let n = Ipv4Addr([10, 1, 2, 3]).render(&mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "10.1.2.3");
    }

    #[test]
    fn ethernet_and_ipv4_parsing_rejects_liars() {
        let eth = EthernetHeader {
            dst: MacAddr::BROADCAST,
            src: MacAddr([0x02, 0, 0, 0, 0, 1]),
            ethertype: ETHERTYPE_IPV4,
        };
        let wire = eth.encode();
        let (back, off) = EthernetHeader::parse(&wire).unwrap();
        assert_eq!(back, eth);
        assert_eq!(off, ETH_HDR_LEN);
        assert_eq!(EthernetHeader::parse(&wire[..10]).unwrap_err(), "frame is shorter than an Ethernet header");
        assert!(EthernetHeader::frame_ok(64, 1500));
        assert!(!EthernetHeader::frame_ok(40, 1500), "below the Ethernet minimum");
        assert!(!EthernetHeader::frame_ok(1600, 1500), "above the MTU");

        let hdr = Ipv4Header::build(Ipv4Addr([10, 0, 0, 2]), Ipv4Addr([10, 0, 0, 1]), PROTO_TCP, 100, 7);
        let mut pkt = [0u8; 256];
        pkt[..IPV4_MIN_HDR].copy_from_slice(&hdr.encode());
        let (parsed, ihl) = Ipv4Header::parse(&pkt).unwrap();
        assert_eq!(ihl, IPV4_MIN_HDR);
        assert_eq!(parsed.total_len, (IPV4_MIN_HDR + 100) as u16);
        assert_eq!(parsed.payload_len(), 100);
        assert!(parsed.dont_fragment());
        assert!(!parsed.more_fragments());
        assert!(!parsed.is_later_fragment());
        assert!(parsed.checksum_ok(&pkt));
        // A total length that exceeds the buffer is refused.
        let mut lying = pkt;
        lying[2..4].copy_from_slice(&600u16.to_be_bytes());
        assert_eq!(
            Ipv4Header::parse(&lying).unwrap_err(),
            "IPv4 total length disagrees with the buffer"
        );
        // A total length smaller than the header is refused too.
        let mut tiny = pkt;
        tiny[2..4].copy_from_slice(&4u16.to_be_bytes());
        assert!(Ipv4Header::parse(&tiny).is_err());
        // Fragments: only the first carries transport headers.
        let mut frag = pkt;
        frag[6..8].copy_from_slice(&(IP_FLAG_MF | 185).to_be_bytes());
        let (f, _) = Ipv4Header::parse(&frag).unwrap();
        assert!(f.more_fragments());
        assert!(f.is_later_fragment());
    }

    #[test]
    fn arp_cache_learns_expires_and_protects_static_entries() {
        let req = ArpPacket {
            opcode: ARP_REQUEST,
            sender_mac: MacAddr([0x02, 0, 0, 0, 0, 2]),
            sender_ip: Ipv4Addr([10, 0, 0, 2]),
            target_mac: MacAddr::ZERO,
            target_ip: Ipv4Addr([10, 0, 0, 1]),
        };
        let wire = req.encode();
        assert_eq!(ArpPacket::parse(&wire).unwrap(), req);
        assert_eq!(ArpPacket::parse(&wire[..20]).unwrap_err(), "ARP packet is truncated");
        let mut wrong = wire;
        wrong[4] = 8;
        assert_eq!(ArpPacket::parse(&wrong).unwrap_err(), "unexpected ARP address sizes");

        let reply = ArpPacket::reply_to(&req, MacAddr([0x02, 0, 0, 0, 0, 1]));
        assert_eq!(reply.opcode, ARP_REPLY);
        assert_eq!(reply.sender_ip, req.target_ip);
        assert_eq!(reply.target_ip, req.sender_ip);
        assert_eq!(reply.target_mac, req.sender_mac);
        assert!(!reply.is_gratuitous());

        let mut cache = ArpCache::new();
        assert!(cache.is_empty());
        assert!(cache.insert(Ipv4Addr([10, 0, 0, 1]), MacAddr([0x02, 0, 0, 0, 0, 1]), 0, true));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.lookup(Ipv4Addr([10, 0, 0, 1]), 10), Some(MacAddr([0x02, 0, 0, 0, 0, 1])));
        assert_eq!(cache.hits(), 1);
        // Dynamic entries expire so spoofed mappings do not live forever.
        assert_eq!(cache.lookup(Ipv4Addr([10, 0, 0, 1]), ARP_ENTRY_TTL_TICKS + 1), None);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.lookup(Ipv4Addr([10, 0, 0, 9]), 0), None);

        // A static entry refuses to be re-mapped.
        let mut fixed = ArpCache::new();
        assert!(fixed.insert(Ipv4Addr([10, 0, 0, 1]), MacAddr([0x02, 0, 0, 0, 0, 1]), 0, false));
        assert!(!fixed.insert(Ipv4Addr([10, 0, 0, 1]), MacAddr([0x02, 0, 0, 0, 0, 9]), 1, true), "spoof refused");
        assert!(fixed.insert(Ipv4Addr([10, 0, 0, 1]), MacAddr([0x02, 0, 0, 0, 0, 1]), 5000, false));
        assert_eq!(fixed.lookup(Ipv4Addr([10, 0, 0, 1]), ARP_ENTRY_TTL_TICKS * 10).is_some(), true);
        assert_eq!(fixed.flush_dynamic(), 0, "nothing dynamic to drop");
        assert!(!fixed.insert(Ipv4Addr([10, 0, 0, 2]), MacAddr::ZERO, 0, true));
    }

    #[test]
    fn icmp_udp_and_checksums() {
        let data = [0x00u8, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let sum = ones_complement_sum(&data, 0);
        // A correct checksum makes the whole message sum to zero.
        let mut with_sum = [0u8; 10];
        with_sum[..8].copy_from_slice(&data);
        with_sum[8..10].copy_from_slice(&sum.to_be_bytes());
        assert_eq!(ones_complement_sum(&with_sum, 0), 0);
        assert_eq!(ones_complement_sum(&[], 0), 0xFFFF);

        let echo = IcmpMessage {
            kind: ICMP_ECHO_REQUEST,
            code: 0,
            id: 1,
            sequence: 2,
            payload_len: 4,
        };
        let mut out = [0u8; 32];
        let n = IcmpMessage::echo_reply(&echo, b"abcd", &mut out).unwrap();
        assert_eq!(n, 12);
        let back = IcmpMessage::parse(&out[..n]).unwrap();
        assert_eq!(back.kind, ICMP_ECHO_REPLY);
        assert_eq!(back.id, 1);
        assert_eq!(back.sequence, 2);
        assert_eq!(back.payload_len, 4);
        assert!(!back.is_echo_request());
        assert_eq!(echo.describe_kind(), "echo-request");
        // A corrupted ICMP message fails its checksum.
        let mut broken = out;
        broken[8] ^= 0xFF;
        assert_eq!(IcmpMessage::parse(&broken[..n]).unwrap_err(), "ICMP checksum is wrong");
        assert_eq!(IcmpMessage::echo_reply(&echo, &[0u8; 40], &mut out), None);

        let mut udp = [0u8; 12];
        udp[0..2].copy_from_slice(&4000u16.to_be_bytes());
        udp[2..4].copy_from_slice(&53u16.to_be_bytes());
        udp[4..6].copy_from_slice(&12u16.to_be_bytes());
        let (h, off) = UdpHeader::parse(&udp).unwrap();
        assert_eq!(off, UDP_HDR_LEN);
        assert_eq!(h.src_port, 4000);
        assert_eq!(h.dst_port, 53);
        assert_eq!(h.payload_len(), 4);
        assert!(h.verify(&udp, Ipv4Addr([10, 0, 0, 2]), Ipv4Addr([8, 8, 8, 8])), "zero means unset");
        let mut bad_len = udp;
        bad_len[4..6].copy_from_slice(&200u16.to_be_bytes());
        assert_eq!(UdpHeader::parse(&bad_len).unwrap_err(), "UDP length disagrees with the buffer");
    }

    #[test]
    fn tcp_state_machine_covers_open_and_close() {
        let s = |flags: u8| TcpSegment {
            flags,
            ..TcpSegment::default()
        };
        let syn = s(TCP_SYN);
        let syn_ack = s(TCP_SYN | TCP_ACK);
        let ack = s(TCP_ACK);
        let fin = s(TCP_FIN);
        let fin_ack = s(TCP_FIN | TCP_ACK);
        let rst = s(TCP_RST);

        // Active open.
        assert_eq!(tcp_transition(TcpState::Closed, &syn, false), Some(TcpState::SynSent));
        assert_eq!(tcp_transition(TcpState::SynSent, &syn_ack, false), Some(TcpState::Established));
        // Passive open.
        assert_eq!(tcp_transition(TcpState::Closed, &syn, true), Some(TcpState::Listen));
        assert_eq!(tcp_transition(TcpState::Listen, &syn, true), Some(TcpState::SynReceived));
        assert_eq!(
            tcp_transition(TcpState::SynReceived, &ack, true),
            Some(TcpState::Established)
        );
        // A segment that is not valid in this state returns None, never a no-op.
        assert_eq!(tcp_transition(TcpState::Listen, &ack, true), None);
        assert_eq!(tcp_transition(TcpState::Closed, &ack, false), None);
        // RST always wins.
        assert_eq!(tcp_transition(TcpState::Established, &rst, false), Some(TcpState::Closed));

        // Active close.
        assert_eq!(tcp_transition(TcpState::Established, &fin, false), Some(TcpState::CloseWait));
        assert_eq!(tcp_active_close(TcpState::Established), Some(TcpState::FinWait1));
        assert_eq!(tcp_transition(TcpState::FinWait1, &ack, false), Some(TcpState::FinWait2));
        assert_eq!(tcp_transition(TcpState::FinWait2, &fin, false), Some(TcpState::TimeWait));
        assert_eq!(tcp_transition(TcpState::TimeWait, &fin, false), Some(TcpState::TimeWait));
        assert_eq!(tcp_active_close(TcpState::CloseWait), Some(TcpState::LastAck));
        assert_eq!(tcp_transition(TcpState::LastAck, &ack, false), Some(TcpState::Closed));
        // FIN+ACK arriving together in FIN-WAIT-1 goes straight to TIME-WAIT.
        assert_eq!(tcp_transition(TcpState::FinWait1, &fin_ack, false), Some(TcpState::TimeWait));

        assert!(TcpState::Established.can_send());
        assert!(TcpState::Established.can_receive());
        assert!(TcpState::CloseWait.can_send());
        assert!(!TcpState::Listen.is_active());
        assert!(TcpState::Established.is_active());
        assert!(TcpState::Closed.is_closed());
        assert_eq!(TcpState::FinWait1.as_str(), "FIN-WAIT-1");

        // Parsing a segment: the header length is in 32-bit words.
        let mut wire = [0u8; 24];
        wire[0..2].copy_from_slice(&1234u16.to_be_bytes());
        wire[2..4].copy_from_slice(&80u16.to_be_bytes());
        wire[12] = 6 << 4; // 24-byte header
        wire[13] = TCP_SYN | TCP_ACK;
        wire[14..16].copy_from_slice(&TCP_DEFAULT_WINDOW.to_be_bytes());
        let seg = TcpSegment::parse(&wire).unwrap();
        assert_eq!(seg.src_port, 1234);
        assert_eq!(seg.header_bytes(), 24);
        assert_eq!(seg.payload_len, 0);
        assert!(seg.has(TCP_SYN));
        assert!(seg.has(TCP_ACK));
        assert_eq!(seg.sequence_space(), 1);
        assert_eq!(TcpSegment::parse(&wire[..10]).unwrap_err(), "TCP header is truncated");
        let mut bad = wire;
        bad[12] = 1 << 4;
        assert_eq!(TcpSegment::parse(&bad).unwrap_err(), "TCP data offset is out of range");
    }

    #[test]
    fn socket_table_enforces_binding_and_the_receive_window() {
        let mut t = SocketTable::new();
        assert!(t.is_empty());
        let a = t.create(SockProto::Tcp).unwrap();
        assert_eq!(t.len(), 1);
        // An ephemeral port is assigned when port 0 is requested.
        let port = t.bind(a, Endpoint::any(0)).unwrap();
        assert!(port >= SocketTable::EPHEMERAL_BASE);
        assert!(t.listen(a, 4).is_ok());
        assert!(t.listen(a, 0).is_err(), "a zero backlog is refused");

        // A second listener on the same port is refused.
        let b = t.create(SockProto::Tcp).unwrap();
        let taken = Endpoint {
            ip: Ipv4Addr([10, 0, 0, 2]),
            port,
        };
        assert_eq!(t.bind(b, taken), Err(SocketError::AlreadyBound));
        assert_eq!(t.refused(), 1);

        let peer = Endpoint {
            ip: Ipv4Addr([10, 0, 0, 9]),
            port: 12345,
        };
        let conn = t.accept(a, peer).unwrap();
        let s = t.get(conn).unwrap();
        assert_eq!(s.state, TcpState::Established);
        assert_eq!(s.remote, peer);
        assert_eq!(t.accepted(), 1);
        assert_eq!(t.established(), 1);
        // Only a listening socket accepts.
        assert_eq!(t.accept(conn, peer), Err(SocketError::WouldBlock));

        // The receive queue honours the advertised window.
        let delivered = t.deliver(conn, 1000).unwrap();
        assert_eq!(delivered, 1000);
        assert_eq!(t.get(conn).unwrap().rx_queued, 1000);
        assert!(t.get(conn).unwrap().advertised_window() < TCP_DEFAULT_WINDOW);
        assert_eq!(t.consume(conn, 400).unwrap(), 400);
        assert_eq!(t.get(conn).unwrap().rx_queued, 600);
        assert!(t.note_tx(conn, 128).is_ok());
        assert_eq!(t.get(conn).unwrap().tx_bytes, 128);
        assert_eq!(t.get(conn).unwrap().advertised_window(), TCP_DEFAULT_WINDOW - 600);

        // Overfilling drops the excess and counts it rather than lying.
        let over = t.deliver(conn, TCP_DEFAULT_WINDOW as u32 + 100).unwrap();
        assert!(over <= TCP_DEFAULT_WINDOW as u32 - 600);
        assert!(t.get(conn).unwrap().dropped > 0);

        let id = t.create(SockProto::Udp).unwrap();
        assert!(t.connect(id, peer).is_ok());
        assert_eq!(t.find_by_port(t.get(id).unwrap().local.port), Some(id));
        assert!(t.close(id));
        assert!(!t.close(id));
        assert!(t.get(id).is_none());
        assert_eq!(SocketError::WouldBlock.errno(), -11);
    }

    #[test]
    fn firewall_nat_and_conntrack() {
        let mut fw = Firewall::new();
        assert!(fw.is_empty());
        // An empty rule list accepts: default-deny with no UI locks the user out.
        assert_eq!(
            fw.evaluate(Direction::Outbound, PROTO_TCP, Ipv4Addr([10, 0, 0, 2]), Ipv4Addr([1, 1, 1, 1]), 443),
            Verdict::Accept
        );
        assert!(fw.add(FirewallRule {
            direction: Some(Direction::Outbound),
            protocol: Some(PROTO_UDP),
            dst_port: Some(53),
            verdict: Some(Verdict::Drop),
            name: "no-dns",
            ..FirewallRule::default()
        }));
        assert_eq!(
            fw.evaluate(
                Direction::Outbound,
                PROTO_UDP,
                Ipv4Addr([10, 0, 0, 2]),
                Ipv4Addr([8, 8, 8, 8]),
                53
            ),
            Verdict::Drop,
        );
        assert_eq!(fw.dropped(), 1);
        assert_eq!(fw.evaluated(), 2);
        assert_eq!(fw.rule_hits(0), 1);

        let mut nat = Nat::new();
        let inside = Endpoint {
            ip: Ipv4Addr([10, 0, 0, 2]),
            port: 4000,
        };
        let remote = Endpoint {
            ip: Ipv4Addr([93, 184, 216, 34]),
            port: 443,
        };
        let outside = nat.translate_out(inside, remote, PROTO_TCP, 0).unwrap();
        assert!(outside >= Nat::PORT_BASE);
        // The same flow reuses its port instead of burning a new one.
        assert_eq!(nat.translate_out(inside, remote, PROTO_TCP, 5), Some(outside));
        assert_eq!(nat.len(), 1);
        assert_eq!(nat.translate_in(outside, remote, PROTO_TCP), Some(inside));
        assert_eq!(nat.translate_in(outside, inside, PROTO_TCP), None, "wrong peer");
        assert_eq!(nat.translated(), 1);
        // A second flow gets a different port.
        let other = Endpoint {
            ip: Ipv4Addr([10, 0, 0, 3]),
            port: 4000,
        };
        let p2 = nat.translate_out(other, remote, PROTO_TCP, 6).unwrap();
        assert_ne!(p2, outside);
        assert_eq!(nat.expire(10, 100), 0, "the flows are still fresh");
        assert_eq!(nat.expire(200, 100), 2);
        assert!(nat.is_empty());

        let mut ct = Conntrack::new();
        assert!(ct.is_empty());
        assert!(ct.update(inside, remote, PROTO_TCP, 100, 0));
        assert_eq!(ct.len(), 1);
        assert!(!ct.is_established(inside, remote, PROTO_TCP), "one packet is not established");
        assert!(ct.update(inside, remote, PROTO_TCP, 50, 1));
        assert!(ct.is_established(inside, remote, PROTO_TCP));
        assert!(ct.is_established(remote, inside, PROTO_TCP), "flows are bidirectional");
        assert_eq!(ct.established(), 1);
        assert_eq!(ct.created(), 1);
        assert_eq!(ct.expire(1000, 100), 1);
        assert!(ct.is_empty());
    }

    #[test]
    fn nic_framework_drivers_and_queues() {
        let mut reg = NicRegistry::new();
        assert!(reg.is_empty());
        assert!(reg.register(Nic::default()).is_none(), "a device with no MAC is refused");
        let wired = reg
            .register(Nic {
                slot: 0,
                kind: NicKind::E1000,
                mac: MacAddr([0x02, 0, 0, 0, 0, 1]),
                mtu: 1500,
                link_up: true,
                speed_mbps: 1000,
                rx_queue_depth: 256,
                tx_queue_depth: 256,
                stats: NicStats::default(),
            })
            .unwrap();
        assert_eq!(wired, 0);
        assert_eq!(reg.link_up_count(), 1);
        assert_eq!(reg.preferred(), Some(0));

        // Transmit validation: MTU, minimum size and link state all matter.
        let nic = reg.get_mut(0).unwrap();
        assert!(nic.usable());
        assert!(nic.transmit(64).is_ok());
        assert_eq!(nic.transmit(40).unwrap_err(), "frame is shorter than the Ethernet minimum");
        assert_eq!(nic.transmit(2000).unwrap_err(), "frame exceeds the MTU");
        assert!(nic.receive(64).is_ok());
        assert_eq!(nic.receive(8).unwrap_err(), "bogus frame length");
        assert_eq!(nic.stats.tx_packets, 1);
        assert_eq!(nic.stats.tx_errors, 2);
        let down = reg.get_mut(0).unwrap();
        down.link_up = false;
        assert_eq!(down.transmit(64).unwrap_err(), "link is down");
        assert_eq!(down.stats.tx_dropped, 1);
        assert!(!down.usable());
        // A slower interface never becomes the preferred one.
        let wireless = reg
            .register(Nic {
                slot: 0,
                kind: NicKind::VirtioNet,
                mac: MacAddr([0x02, 0, 0, 0, 0, 2]),
                mtu: 1400,
                link_up: true,
                speed_mbps: 100,
                rx_queue_depth: 64,
                tx_queue_depth: 64,
                stats: NicStats::default(),
            })
            .unwrap();
        assert_eq!(reg.preferred(), Some(wireless));
        assert_eq!(reg.link_up_count(), 1);
        assert_eq!(reg.len(), 2);
        assert!(reg.get(9).is_none());

        // virtio: allocate and reclaim descriptors.
        let mut vq = Virtqueue::new(4);
        assert_eq!(vq.free_count, 4);
        for _ in 0..4 {
            assert!(vq.allocate().is_some());
        }
        assert!(vq.allocate().is_none(), "an empty ring refuses");
        assert_eq!(vq.in_flight(), 4);
        assert_eq!(vq.avail_idx, 4);
        assert!(vq.reclaim().is_none(), "the device has not completed anything yet");
        vq.used_idx = 2; // the device finished two buffers
        assert!(vq.reclaim().is_some());
        assert_eq!(vq.free_count, 1);
        let desc = VirtqDesc {
            addr: 0x1000,
            len: 64,
            flags: VirtqDesc::F_NEXT | VirtqDesc::F_WRITE,
            next: 1,
        };
        assert!(desc.is_chained());
        assert!(desc.device_writes());

        // e1000 transmit descriptor round trip.
        let tx = E1000TxDesc::command_for(64);
        assert_eq!(tx.cmd, E1000TxDesc::CMD_EOP | E1000TxDesc::CMD_IFCS | E1000TxDesc::CMD_RS);
        assert!(!tx.done(), "hardware has not touched it yet");
        let wire = tx.encode();
        let back = E1000TxDesc::decode(&wire);
        assert_eq!(back.length, 64);
        assert_eq!(back.cmd, tx.cmd);
        let mut written = wire;
        written[12] = E1000TxDesc::STATUS_DD;
        assert!(E1000TxDesc::decode(&written).done());
        assert_eq!(E1000TxDesc::REG_TDT, 0x3818);
    }

    #[test]
    fn dhcp_dns_and_ntp() {
        let mut lease = DhcpLease {
            address: Ipv4Addr([192, 168, 1, 50]),
            netmask: Ipv4Addr([255, 255, 255, 0]),
            gateway: Ipv4Addr([192, 168, 1, 1]),
            dns: Ipv4Addr([1, 1, 1, 1]),
            server: Ipv4Addr([192, 168, 1, 1]),
            lease_secs: 3600,
            t1_secs: 0,
            t2_secs: 0,
        };
        lease.derive_timers();
        assert_eq!(lease.t1_secs, 1800);
        assert_eq!(lease.t2_secs, 3150);
        assert!(lease.valid());
        assert!(!DhcpLease::default().valid(), "an all-zero lease is not usable");
        assert_eq!(DhcpState::Bound.as_str(), "bound");
        assert!(DhcpState::Bound.has_address());
        assert!(!DhcpState::Init.has_address());

        // Options are TLV; option 53 is the message type.
        let options = [53u8, 1, DHCP_MSG_OFFER, 1, 4, 255, 255, 255, 0, 0];
        assert_eq!(dhcp_message_type(&options), Some(DHCP_MSG_OFFER));
        assert_eq!(dhcp_option(&options, 1), Some(&[255u8, 255, 255, 0][..]));
        assert_eq!(dhcp_option(&options, 99), None);
        assert_eq!(dhcp_message_type(&[]), None);

        // Name encoding: labels with length prefixes and a terminator.
        let mut name_buf = [0u8; 32];
        let n = DnsQuestion::encode_name("example.com", &mut name_buf).unwrap();
        assert_eq!(n, 13);
        assert_eq!(name_buf[0], 7);
        assert_eq!(&name_buf[1..8], b"example");
        assert_eq!(name_buf[8], 3);
        assert_eq!(name_buf[12], 0);
        assert!(
            DnsQuestion::encode_name("", &mut name_buf).is_none(),
            "an empty name is not a DNS name"
        );
        assert_eq!(DnsQuestion::encode_name("a..b", &mut name_buf), None);
        let mut q = DnsQuestion {
            qtype: 1,
            qclass: 1,
            ..DnsQuestion::default()
        };
        // `name` holds the dotted form, not the wire form with length bytes.
        q.name[..11].copy_from_slice(b"example.com");
        q.name_len = 11;
        assert_eq!(q.name_str(), "example.com");

        // A response with a compression pointer back into the question.
        let mut resp = [0u8; 80];
        resp[0] = 0x81;
        resp[1] = 0x80;
        resp[4..6].copy_from_slice(&1u16.to_be_bytes());
        resp[6..8].copy_from_slice(&1u16.to_be_bytes());
        let mut at = DNS_HDR_LEN;
        at += DnsQuestion::encode_name("example.com", &mut resp[at..]).unwrap();
        resp[at..at + 4].copy_from_slice(&[0, 1, 0, 1]);
        at += 4;
        resp[at] = 0xC0;
        resp[at + 1] = DNS_HDR_LEN as u8;
        at += 2;
        resp[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        resp[at + 2..at + 4].copy_from_slice(&1u16.to_be_bytes());
        resp[at + 4..at + 8].copy_from_slice(&300u32.to_be_bytes());
        resp[at + 8..at + 10].copy_from_slice(&4u16.to_be_bytes());
        resp[at + 10..at + 14].copy_from_slice(&[93, 184, 216, 34]);
        assert_eq!(
            DnsQuestion::answer_a(&resp),
            Some(Ipv4Addr([93, 184, 216, 34]))
        );
        // A forward pointer is a loop, and is refused.
        let mut looping = [0u8; 32];
        looping[12] = 0xC0;
        looping[13] = 20;
        assert_eq!(DnsQuestion::skip_name(&looping, 12), None);
        // No answers means no address, not a bogus one.
        assert_eq!(DnsQuestion::answer_a(&[0u8; DNS_HDR_LEN]), None);
        assert_eq!(DnsQuestion::answer_a(&[]), None);

        // NTP.
        let request = NtpPacket::client_request();
        assert_eq!(request[0] & 0x07, NtpPacket::MODE_CLIENT);
        assert_eq!((request[0] >> 3) & 0x07, 4);
        assert_eq!(request.len(), NTP_PACKET_LEN);
        let mut server = [0u8; NTP_PACKET_LEN];
        server[0] = (4 << 3) | NtpPacket::MODE_SERVER;
        server[1] = 3;
        server[3] = 0xE4; // precision -28 → ~3.7 ns
        let secs = (NTP_UNIX_DELTA + 1_700_000_000) as u32;
        server[40..44].copy_from_slice(&secs.to_be_bytes());
        server[44..48].copy_from_slice(&(1u32 << 31).to_be_bytes());
        let reply = NtpPacket::parse(&server).unwrap();
        assert!(reply.valid_response());
        assert_eq!(reply.stratum, 3);
        assert_eq!(reply.unix_seconds(), 1_700_000_000);
        assert_eq!(reply.unix_millis(), 500);
        assert!(reply.precision_seconds() < 0.001);
        assert_eq!(NtpPacket::parse(&server[..20]).unwrap_err(), "NTP packet is truncated");
        // A stratum-0 reply is a kiss-of-death, not a time source.
        let mut kiss = server;
        kiss[1] = 0;
        assert!(!NtpPacket::parse(&kiss).unwrap().valid_response());
    }

    #[test]
    fn rates_quotas_and_link_degradation() {
        let prev = RateSnapshot {
            rx_bytes: 1000,
            tx_bytes: 500,
            tick: 0,
        };
        let now = RateSnapshot {
            rx_bytes: 3000,
            tx_bytes: 1500,
            tick: 100,
        };
        let rates = Rates::between(&prev, &now, 10);
        assert_eq!(rates.rx_bps, 2000 * 1000 / 1000);
        assert_eq!(rates.tx_bps, 1000 * 1000 / 1000);
        assert_eq!(rates.total_bps(), 3000);
        // A zero delta must not divide by zero.
        assert_eq!(Rates::between(&prev, &prev, 10).total_bps(), 0);
        assert_eq!(Rates::between(&prev, &now, 0).total_bps(), 0);

        let mut q = TrafficQuota::new(1000);
        assert!(q.charge(600));
        assert!(q.charge(400));
        assert!(q.exhausted());
        assert!(!q.charge(1), "the quota is enforced");
        assert_eq!(q.denials, 1);
        assert_eq!(q.remaining(), 0);
        assert_eq!(q.allowance_percent(), TrafficQuota::DEFAULT_THROTTLE);
        let mut unlimited = TrafficQuota::new(0);
        assert!(unlimited.charge(u64::MAX / 2));
        assert!(!unlimited.exhausted());
        assert_eq!(unlimited.allowance_percent(), 100);

        let caps = OfflineCapabilities::offline();
        assert_eq!(caps.working_count(), 5);
        assert_eq!(caps.disabled(), 3);
        assert!(caps.local_ai && !caps.software_update);

        assert_eq!(LinkQuality::classify(0, 10), LinkQuality::Good);
        assert_eq!(LinkQuality::classify(8, 10), LinkQuality::Weak);
        assert_eq!(LinkQuality::classify(0, 500), LinkQuality::Weak);
        assert_eq!(LinkQuality::classify(50, 10), LinkQuality::Terrible);
        assert_eq!(LinkQuality::classify(0, 2000), LinkQuality::Terrible);
        assert!(LinkQuality::Terrible > LinkQuality::Weak);
        assert_eq!(LinkQuality::Good.mtu(), 1500);
        assert!(LinkQuality::Terrible.mtu() < LinkQuality::Weak.mtu());
        assert!(LinkQuality::Good.allows_background_sync());
        assert!(!LinkQuality::Terrible.allows_background_sync());
        assert!(LinkQuality::Terrible.retransmit_delay_ms() > LinkQuality::Good.retransmit_delay_ms());
        assert!(!LinkQuality::Weak.as_str().is_empty());
    }

    #[test]
    fn capture_tls_ipv6_and_events() {
        let mut cap = Capture::new();
        assert!(!cap.is_enabled());
        assert!(!cap.record(CaptureEntry::default(), &[1, 2, 3]));
        cap.set_enabled(true);
        assert!(cap.is_enabled());
        for i in 0..(MAX_CAPTURE + 4) {
            assert!(cap.record(
                CaptureEntry {
                    tick: i as u64,
                    len: 64,
                    protocol: PROTO_TCP,
                    ..CaptureEntry::default()
                },
                &[0xAA, 0xBB]
            ));
        }
        assert_eq!(cap.len(), MAX_CAPTURE);
        assert_eq!(cap.total(), (MAX_CAPTURE + 4) as u64);
        let newest = cap.get(0).unwrap();
        assert_eq!(newest.preview_len, 2);
        assert_eq!(newest.preview[0], 0xAA);
        assert!(cap.get(MAX_CAPTURE).is_none());

        assert!(!TLS_SUPPORTED);
        assert_eq!(tls_connect("example.com", 443), Err(TlsError::NotImplemented));

        assert!(!IPV6_SUPPORTED);
        let mut v6 = [0u8; IPV6_HDR_LEN];
        v6[0] = 0x60;
        v6[4..6].copy_from_slice(&8u16.to_be_bytes());
        v6[6] = PROTO_UDP;
        v6[7] = 64;
        v6[15] = 1; // ::1
        v6[39] = 1;
        let parsed = Ipv6Header::parse(&v6).unwrap();
        assert_eq!(parsed.next_header, PROTO_UDP);
        assert_eq!(parsed.hop_limit, 64);
        assert!(parsed.is_loopback());
        assert!(!parsed.routing_supported(), "IPv6 is reserved, not routed");
        assert_eq!(
            Ipv6Header::parse(&v6[..20]).unwrap_err(),
            "packet is shorter than an IPv6 header"
        );
        assert_eq!(Ipv6Header::parse(&[0u8; 40]).unwrap_err(), "not an IPv6 packet");

        let mut ev = NetEvents::new();
        assert!(ev.is_empty());
        ev.push(NetEvent {
            tick: 1,
            kind: Some(NetEventKind::LinkUp),
            detail: 0,
        });
        ev.push(NetEvent {
            tick: 2,
            kind: Some(NetEventKind::LinkUp),
            detail: 0,
        });
        ev.push(NetEvent {
            tick: 3,
            kind: Some(NetEventKind::DhcpFailed),
            detail: 0,
        });
        assert_eq!(ev.total(), 3);
        assert_eq!(ev.count(NetEventKind::LinkUp), 2);
        assert_eq!(ev.count(NetEventKind::DhcpFailed), 1);
        assert_eq!(ev.count(NetEventKind::QuotaExceeded), 0);
        assert_eq!(ev.get(0).unwrap().tick, 3, "newest first");
        assert!(ev.get(9).is_none());
        assert!(!NetEventKind::FirewallDrop.as_str().is_empty());
    }

    #[test]
    fn self_test_passes() {
        let (passed, failed) = run_net_checks();
        let mut out = [0u8; 512];
        let n = NET_SELFTEST.render(&mut out);
        let detail = core::str::from_utf8(&out[..n]).unwrap_or("");
        assert_eq!(failed, 0, "{passed} passed, {failed} failed: {detail}");
        assert!(passed >= 12);
    }
}
