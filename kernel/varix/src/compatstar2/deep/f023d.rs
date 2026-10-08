//! F023 深化批次二 · fd_set/字节序/数据报面（compatstar2/deep · G-A-23）。
//!
//! 批次一深化覆盖 WSA 错误码全集/socket 选项/ioctlsocket/hostent；本批补齐：
//! select() 的 fd_set 定长位图模型、网络字节序四函数（htonl/htons/ntohl/ntohs
//! ——FTP 判据端口字段的线上格式）、inet_addr/inet_ntoa 定长换算、UDP 数据报
//! 截断语义（MSG_TRUNC）、shutdown 半关闭状态机。
//!
//! 零堆纪律：位图与定长缓冲，无 alloc。

use crate::checks::CheckSet;

/// fd_set 容量（FD_SETSIZE 语义；MS 默认 64）。
pub const FD_SETSIZE: usize = 64;
/// UDP 数据报上限：65535 - 20(IP) - 8(UDP) = 65507 字节。
pub const MAX_DATAGRAM: usize = 65_507;

/// fd_set：64 位位图（FD_ZERO/FD_SET/FD_CLR/FD_ISSET 语义承载）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FdSet(pub u64);

impl FdSet {
    pub const fn zero() -> Self {
        FdSet(0)
    }
    pub fn set(&mut self, fd: usize) -> bool {
        if fd >= FD_SETSIZE {
            return false;
        }
        self.0 |= 1u64 << fd;
        true
    }
    pub fn clear(&mut self, fd: usize) -> bool {
        if fd >= FD_SETSIZE {
            return false;
        }
        self.0 &= !(1u64 << fd);
        true
    }
    pub fn is_set(&self, fd: usize) -> bool {
        fd < FD_SETSIZE && self.0 >> fd & 1 == 1
    }
    /// 已置位计数（select 返回值的模型面）。
    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }
}

/// select 模型：返回就绪句柄数（就绪集由输入集与就绪位的交承载）；
/// timeout_ms < 0 = 阻塞语义、0 = 非阻塞轮询——模型面只裁结果计数。
pub fn select_count(read: &FdSet, ready: &FdSet) -> i32 {
    (read.0 & ready.0).count_ones() as i32
}

// ---------------------------------------------------------------------------
// 字节序四函数（线上格式 = 大端）
// ---------------------------------------------------------------------------

pub fn htons(host: u16) -> u16 {
    host.to_be()
}
pub fn htonl(host: u32) -> u32 {
    host.to_be()
}
pub fn ntohs(net: u16) -> u16 {
    u16::from_be(net)
}
pub fn ntohl(net: u32) -> u32 {
    u32::from_be(net)
}

/// inet_addr：点分十进制 → 网络序 u32（非法串如实拒绝）。
pub fn inet_addr(dotted: &str) -> Option<u32> {
    let mut out: u32 = 0;
    let mut filled = 0usize;
    for part in dotted.split('.') {
        if filled >= 4 {
            return None;
        }
        let v: u32 = part.parse().ok()?;
        if v > 255 {
            return None;
        }
        out = out << 8 | v;
        filled += 1;
    }
    if filled == 4 { Some(out) } else { None }
}

/// inet_ntoa：网络序 u32 → 定长点分缓冲（返回写入长度；零分配）。
pub fn inet_ntoa(net: u32, out: &mut [u8]) -> usize {
    let octets = [(net >> 24) & 0xFF, (net >> 16) & 0xFF, (net >> 8) & 0xFF, net & 0xFF];
    let mut n = 0usize;
    for (i, &o) in octets.iter().enumerate() {
        if i > 0 {
            out[n] = b'.';
            n += 1;
        }
        if o >= 100 {
            out[n] = b'0' + (o / 100) as u8;
            n += 1;
        }
        if o >= 10 {
            out[n] = b'0' + (o / 10 % 10) as u8;
            n += 1;
        }
        out[n] = b'0' + (o % 10) as u8;
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// UDP 数据报与 shutdown 半关闭
// ---------------------------------------------------------------------------

/// recvfrom 截断语义：缓冲小于报文 → 拷满缓冲并置 MSG_TRUNC 标记
/// （返回 (拷贝字节数, 是否截断)）。
pub fn recvfrom_truncate(payload_len: usize, buf_len: usize) -> (usize, bool) {
    if payload_len <= buf_len {
        (payload_len, false)
    } else {
        (buf_len, true)
    }
}

/// shutdown 半关闭状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShutdownHow {
    Read,
    Write,
    Both,
}

/// 半关闭后的收发裁决：写关 → send 拒 / recv 照常；读关 → recv 拒；双关全拒。
pub fn half_close_verdict(how: ShutdownHow, op: u8) -> bool {
    // op: 0 = send, 1 = recv
    match how {
        ShutdownHow::Read => op == 0,
        ShutdownHow::Write => op == 1,
        ShutdownHow::Both => false,
    }
}

/// 域自检（深化批次二）。
pub fn run_f023d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F023-winsock-d2");
    // 1) fd_set 四操作与计数。
    let mut s = FdSet::zero();
    let ops_ok = s.set(3) && s.set(60) && s.is_set(3) && !s.set(64) && s.count() == 2;
    s.clear(3);
    cs.add("fdset_ops", ops_ok && !s.is_set(3) && s.is_set(60) && s.count() == 1, "");
    // 2) select 交集计数（3 就绪 ∩ 2 监听 = 1）。
    let mut listen = FdSet::zero();
    listen.set(0);
    listen.set(5);
    let mut ready = FdSet::zero();
    ready.set(0);
    ready.set(7);
    ready.set(9);
    cs.add("select_intersection", select_count(&listen, &ready) == 1, "");
    // 3) 字节序 round-trip（端口 21 与地址 10.0.0.1；htonl 值交换 + 线上字节序）。
    cs.add(
        "byteorder_roundtrip",
        htons(21) == 21u16 << 8 && ntohs(htons(21)) == 21
            && htonl(0x0A00_0001) == 0x0100_000A
            && htonl(0x0A00_0001).to_le_bytes() == [10, 0, 0, 1]
            && ntohl(htonl(0xDEAD_BEEF)) == 0xDEAD_BEEF,
        "",
    );
    // 4) inet_addr/ntoa 双向（10.0.0.1）与非法串拒绝。
    let addr = inet_addr("10.0.0.1");
    let mut buf = [0u8; 16];
    let n = inet_ntoa(addr.unwrap(), &mut buf);
    cs.add(
        "inet_convert",
        addr == Some(0x0A00_0001) && &buf[..n] == b"10.0.0.1" && inet_addr("256.1.1.1").is_none() && inet_addr("1.2.3").is_none(),
        "",
    );
    // 5) UDP 截断：65507 上限在册 + 缓冲不足置 MSG_TRUNC。
    cs.add(
        "datagram_trunc",
        MAX_DATAGRAM == 65_507 && recvfrom_truncate(100, 64) == (64, true) && recvfrom_truncate(64, 128) == (64, false),
        "",
    );
    // 6) shutdown 半关闭裁决（写关后 send 拒 recv 照常）。
    cs.add(
        "half_close",
        !half_close_verdict(ShutdownHow::Write, 0)
            && half_close_verdict(ShutdownHow::Write, 1)
            && half_close_verdict(ShutdownHow::Read, 0)
            && !half_close_verdict(ShutdownHow::Both, 1),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inet_ntoa_boundaries() {
        let mut buf = [0u8; 16];
        let n = inet_ntoa(0xFFFF_FFFF, &mut buf);
        assert_eq!(&buf[..n], b"255.255.255.255");
        let n0 = inet_ntoa(0, &mut buf);
        assert_eq!(&buf[..n0], b"0.0.0.0");
    }

    #[test]
    fn loopback_address() {
        assert_eq!(inet_addr("127.0.0.1"), Some(0x7F00_0001));
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f023d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
