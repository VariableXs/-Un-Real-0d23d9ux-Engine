//! F023 深化批次三 · 缓冲收发、就绪集与地址族边界（compatstar2/deep2 · G-A-23）。
//!
//! 主层 winsock.rs 覆盖套接字表/getaddrinfo/异步消息/公平调度；本批补齐
//! 主册【功能定义】「全语义对齐」的执行/边界/注入面：WSABUF scatter/gather
//! 模型（定长 8 段、聚合写总长计账溢出检测、分散读顺序分摊）、select 三
//! 就绪集模型（read/write/except 三张 fd 位图、容量 256 位、遍历返回就绪
//! fd 列表定长 16、超列如实计数）、inet_pton/inet_ntop 双向（IPv4 点分
//! 十进制严格解析——前导零拒绝/每段 0-255；IPv6 八组十六进制含 :: 压缩
//! 一组模型）、半开连接探测账（探测/超时/回收三账）。
//! 判据对账：主册 G-A-23【设计细节】+ MS WSABUF/select/inet_pton（RFC 3493）
//! 语义对拍。零堆纪律：定长数组，无 Vec/String/Box/format!。

use crate::checks::CheckSet;
/// WSABUF 段数定长（MS WSABUF scatter/gather，域内口径 8）。
pub const WSA_BUF_SLOTS: usize = 8;
/// 聚合写总长上限（u16 载荷语义）。
pub const GATHER_CAP: u32 = 65535;
/// fd 位图容量 256 位（MS FD_SETSIZE 语义）。
pub const FD_CAP: usize = 256;
/// select 就绪 fd 列表定长（域内模型口径）。
pub const READY_OUT: usize = 16;

/// WSABUF 定长段组：聚合写计账 + 分散读分摊。
pub struct WsaBufSet {
    lens: [u16; WSA_BUF_SLOTS],
    pub n: usize,
    pub overflow_events: u32,
}

impl WsaBufSet {
    pub const fn new() -> Self {
        WsaBufSet { lens: [0; WSA_BUF_SLOTS], n: 0, overflow_events: 0 }
    }
    /// 装载第 i 段长度（i 必须连续，超槽/乱序拒绝）。
    pub fn set_seg(&mut self, i: usize, len: u16) -> Result<(), &'static str> {
        if i >= WSA_BUF_SLOTS || i != self.n { return Err("buf-slot-order"); }
        self.lens[i] = len;
        self.n += 1;
        Ok(())
    }
    /// 聚合写：Σ 段长超 GATHER_CAP 如实拒绝并记账（WSAENOBUFS 语义面）。
    pub fn gather(&mut self) -> Result<u32, &'static str> {
        let t: u32 = self.lens.iter().map(|&l| l as u32).sum();
        if t > GATHER_CAP { self.overflow_events += 1; return Err("gather-overflow"); }
        Ok(t)
    }
    /// 分散读：按段序分摊 want 字节，返回每段填充量与已消费总量。
    pub fn scatter(&self, want: u32) -> ([u16; WSA_BUF_SLOTS], u32) {
        let mut filled = [0u16; WSA_BUF_SLOTS];
        let mut left = want;
        for i in 0..self.n {
            filled[i] = (self.lens[i] as u32).min(left) as u16;
            left -= filled[i] as u32;
        }
        (filled, want - left)
    }
}

/// fd 位图（256 位 = 4 × u64）。
#[derive(Clone, Copy)]
pub struct FdSet {
    bits: [u64; FD_CAP / 64],
}

impl FdSet {
    pub const fn new() -> Self {
        FdSet { bits: [0; FD_CAP / 64] }
    }
    pub fn set(&mut self, fd: usize) -> bool {
        if fd < FD_CAP { self.bits[fd / 64] |= 1u64 << (fd % 64); true } else { false }
    }
    pub fn clear(&mut self, fd: usize) -> bool {
        if fd < FD_CAP { self.bits[fd / 64] &= !(1u64 << (fd % 64)); true } else { false }
    }
    pub fn is_set(&self, fd: usize) -> bool {
        fd < FD_CAP && self.bits[fd / 64] & (1u64 << (fd % 64)) != 0
    }
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// select 三就绪集模型：read/write/except 三张位图；read 就绪 fd 遍历
/// 升序输出（定长 16，超出部分如实例数——不静默丢）。返回（就绪 fd
/// 升序列表, 个数, read 溢出数, write 就绪数, except 就绪数）。
pub fn select(reads: &FdSet, writes: &FdSet, excepts: &FdSet) -> ([u16; READY_OUT], usize, u32, u32, u32) {
    let mut list = [0u16; READY_OUT];
    let mut n = 0usize;
    let mut overflow = 0u32;
    for fd in 0..FD_CAP {
        if reads.is_set(fd) {
            if n < READY_OUT {
                list[n] = fd as u16;
                n += 1;
            } else {
                overflow += 1;
            }
        }
    }
    (list, n, overflow, writes.count() as u32, excepts.count() as u32)
}

/// inet_pton IPv4：点分十进制严格解析——恰四段、每段 1~3 位数字、
/// 前导零拒绝、每段 0-255（RFC 3493 + MS 文档语义）。
pub fn pton4(s: &str) -> Result<[u8; 4], &'static str> {
    let mut ip = [0u8; 4];
    let mut i = 0usize;
    for part in s.split('.') {
        if i >= 4 { return Err("pton4-format"); }
        let b = part.as_bytes();
        if b.is_empty() || b.len() > 3 { return Err("pton4-format"); }
        if b.len() > 1 && b[0] == b'0' { return Err("pton4-leading-zero"); }
        let mut v: u16 = 0;
        for &c in b {
            if !c.is_ascii_digit() { return Err("pton4-format"); }
            v = v * 10 + (c - b'0') as u16;
        }
        if v > 255 { return Err("pton4-octet"); }
        ip[i] = v as u8;
        i += 1;
    }
    if i != 4 { return Err("pton4-format"); }
    Ok(ip)
}

/// inet_ntop IPv4：定长字节缓冲渲染（最长 "255.255.255.255" 15 字节）。
pub fn ntop4(ip: [u8; 4]) -> ([u8; 15], usize) {
    let mut buf = [0u8; 15];
    let mut n = 0usize;
    for (i, &o) in ip.iter().enumerate() {
        if i > 0 { buf[n] = b'.'; n += 1; }
        if o >= 100 { buf[n] = b'0' + o / 100; n += 1; }
        if o >= 10 { buf[n] = b'0' + (o / 10) % 10; n += 1; }
        buf[n] = b'0' + o % 10;
        n += 1;
    }
    (buf, n)
}

fn parse_hex_group(part: &str) -> Result<u16, &'static str> {
    if part.is_empty() || part.len() > 4 { return Err("pton6-group"); }
    let mut v: u16 = 0;
    for c in part.bytes() {
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u16, b'a'..=b'f' => (c - b'a') as u16 + 10,
            b'A'..=b'F' => (c - b'A') as u16 + 10, _ => return Err("pton6-group"),
        };
        v = v * 16 + d;
    }
    Ok(v)
}

/// 单侧装载（:: 压缩左侧顺序 / 右侧逆序）：空侧原位返回；非空侧含空段
/// 拒绝；逆序越 floor 拒绝。返回写指针。
fn fill_side(groups: &mut [u16; 8], side: &str, forward: bool, floor: usize) -> Result<usize, &'static str> {
    let mut k = if forward { 0usize } else { 8usize };
    for part in side.split(':') {
        if part.is_empty() {
            if !side.is_empty() { return Err("pton6-format"); }
            continue;
        }
        let v = parse_hex_group(part)?;
        if forward {
            if k >= 8 { return Err("pton6-count"); }
            groups[k] = v;
            k += 1;
        } else {
            if k <= floor { return Err("pton6-count"); }
            k -= 1;
            groups[k] = v;
        }
    }
    Ok(k)
}

/// inet_pton IPv6（八组模型）：恰八组十六进制，或含一处 `::` 压缩
/// （压缩后总组数 ≤ 7）；双压缩/空段/超组如实拒绝。
pub fn pton6(s: &str) -> Result<[u16; 8], &'static str> {
    let mut groups = [0u16; 8];
    if let Some(pos) = s.find("::") {
        if s[pos + 2..].contains("::") || s[..pos].contains("::") { return Err("pton6-multi-compress"); }
        let li = fill_side(&mut groups, &s[..pos], true, 0)?;
        let ri = fill_side(&mut groups, &s[pos + 2..], false, li)?;
        if li + (8 - ri) > 7 { return Err("pton6-count"); }
        Ok(groups)
    } else if s.is_empty() {
        Err("pton6-format")
    } else if fill_side(&mut groups, s, true, 0)? != 8 {
        Err("pton6-count")
    } else {
        Ok(groups)
    }
}

/// inet_ntop IPv6：八组十六进制定长渲染（:: 压缩面由 pton6 解析侧承载，
/// 渲染侧保持无歧义直排——RFC 5952 压缩渲染属可选优化，模型面不展开）。
pub fn ntop6(groups: [u16; 8]) -> ([u8; 39], usize) {
    fn hex(out: &mut [u8], n: &mut usize, v: u16) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut i = if v >= 0x1000 { 0 } else if v >= 0x100 { 1 } else if v >= 0x10 { 2 } else { 3 };
        while i < 4 {
            out[*n] = HEX[((v >> [12u16, 8, 4, 0][i]) & 0xF) as usize];
            *n += 1;
            i += 1;
        }
    }
    let mut buf = [0u8; 39];
    let mut n = 0usize;
    for (g, &v) in groups.iter().enumerate() {
        if g > 0 {
            buf[n] = b':';
            n += 1;
        }
        hex(&mut buf, &mut n, v);
    }
    (buf, n)
}

/// 半开连接探测账：探测/超时/回收三账，超账回收显性拒绝。
pub struct HalfOpenLedger {
    pub probed: u32,
    pub timed_out: u32,
    pub reclaimed: u32,
    pub violations: u32,
}

impl HalfOpenLedger {
    pub const fn new() -> Self {
        HalfOpenLedger { probed: 0, timed_out: 0, reclaimed: 0, violations: 0 }
    }
    pub fn probe(&mut self) {
        self.probed += 1;
    }
    pub fn on_timeout(&mut self) -> bool {
        if self.probed == self.timed_out { self.violations += 1; return false; }
        self.timed_out += 1;
        true
    }
    pub fn reclaim(&mut self) -> bool {
        if self.timed_out == self.reclaimed { self.violations += 1; return false; }
        self.reclaimed += 1;
        true
    }
}

/// 域自检（深化批次三）。
pub fn run_f023e_checks() -> crate::checks::CheckSet {
    let mut cs = CheckSet::new("F023-winsock-d3");
    // 1) 聚合写计账：8×8191 不超限；8×9000 超 65535 如实拒绝并记账。
    let mut b1 = WsaBufSet::new();
    let mut b2 = WsaBufSet::new();
    for i in 0..WSA_BUF_SLOTS {
        assert!(b1.set_seg(i, 8191).is_ok() && b2.set_seg(i, 9000).is_ok());
    }
    cs.add("gather_ledger", b1.gather() == Ok(65528) && b2.gather() == Err("gather-overflow") && b2.overflow_events == 1, "");
    // 2) 分散读顺序分摊：段 [10,0,5]，want=12 → 填 [10,0,2]，消费 12。
    let mut b3 = WsaBufSet::new();
    let _ = b3.set_seg(0, 10);
    let _ = b3.set_seg(1, 0);
    let _ = b3.set_seg(2, 5);
    let (filled, consumed) = b3.scatter(12);
    cs.add("scatter_order", filled[0] == 10 && filled[1] == 0 && filled[2] == 2 && consumed == 12, "");
    // 3) fd 位图容量：fd 255 可置、越界 256 拒绝、清除生效。
    let mut fs = FdSet::new();
    cs.add("fd_set_capacity", fs.set(255) && !fs.set(256) && fs.is_set(255) && fs.clear(255) && !fs.is_set(255), "");
    // 4) select 就绪列表：fd 1..=20 就绪 → 升序 16 个、溢出 4 计账。
    let mut rs = FdSet::new();
    for fd in 1..=20 {
        let _ = rs.set(fd);
    }
    let (list, n, overflow, wr, xr) = select(&rs, &FdSet::new(), &FdSet::new());
    let ordered = (0..n).all(|i| list[i] == (i + 1) as u16);
    cs.add("select_ready_list", n == READY_OUT && overflow == 4 && ordered && wr == 0 && xr == 0, "");
    // 5) inet_pton/ntop v4 往返 + 严格性（前导零/256/段数不足）。
    let ip = pton4("192.168.0.1").expect("合法 v4 必成");
    let (buf, nb) = ntop4(ip);
    cs.add(
        "pton4_ntop4_roundtrip",
        ip == [192, 168, 0, 1] && &buf[..nb] == b"192.168.0.1" && ntop4([255, 255, 255, 255]).1 == 15, "",
    );
    cs.add(
        "pton4_strict",
        pton4("01.2.3.4") == Err("pton4-leading-zero") && pton4("256.1.1.1") == Err("pton4-octet")
            && pton4("1.2.3") == Err("pton4-format"),
        "",
    );
    // 6) pton6 :: 压缩 + 严格性：双压缩/超组拒绝、全八组无压缩成立。
    cs.add(
        "pton6_compress_strict",
        pton6("::1") == Ok([0, 0, 0, 0, 0, 0, 0, 1]) && pton6("2001:db8::1") == Ok([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1])
            && pton6("1::2::3") == Err("pton6-multi-compress") && pton6("1:2:3:4:5:6:7:8:9") == Err("pton6-count")
            && pton6("1:2:3:4:5:6:7:8").is_ok(),
        "",
    );
    // 7) ntop6 八组直排渲染往返（:: 压缩解析面见上一条）。
    let (v6buf, v6n) = ntop6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
    cs.add("ntop6_render", &v6buf[..v6n] == b"2001:db8:0:0:0:0:0:1", "");
    // 8) 半开账：探测 3 → 超时 2 → 回收 2，outstanding 1；无超账回收违规。
    let mut h = HalfOpenLedger::new();
    h.probe();
    h.probe();
    h.probe();
    let t1 = h.on_timeout() && h.on_timeout();
    let drained = h.reclaim() && h.reclaim();
    let bad = !h.reclaim();
    cs.add(
        "halfopen_ledger",
        t1 && drained && bad && h.probed - h.timed_out == 1 && h.violations == 1
            && h.probed == 3 && h.timed_out == 2 && h.reclaimed == 2, "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_consumes_at_most_total() {
        let mut b = WsaBufSet::new();
        let _ = b.set_seg(0, 4);
        let _ = b.set_seg(1, 6);
        let (filled, consumed) = b.scatter(100);
        assert_eq!(consumed, 10, "段总量封顶，多余输入不虚构消费");
        assert_eq!((filled[0], filled[1]), (4, 6));
    }

    #[test]
    fn pton6_edges() {
        assert_eq!(pton6("::"), Ok([0u16; 8]));
        assert_eq!(pton6("fe80::"), Ok([0xfe80, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!((pton6(""), pton6("1:2:3")), (Err("pton6-format"), Err("pton6-count")));
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f023e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
