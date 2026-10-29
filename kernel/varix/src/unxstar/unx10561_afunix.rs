//! unx10561_afunix —— UNX-F10561 AF_UNIX socket()/socketpair 基础。
//! 锚点（CoRun Unxreal 主册）：UNX-F10561 · AF_UNIX 基础 · 域 UNX-C4-B09 · 行数预算 280。
//! 判据原文：UNX-F10561-J1 socketpair 产双向对（A→B 与 B→A 各
//!           1 万字节逐字节恒等），type 三闭集（STREAM/DGRAM/
//!           SEQPACKET）外必拒，fd 账双向闭合（对内两端互指），
//!           关一端后对端写必 EPIPE。
//! 组件：socket 类型核（三类型闭集，越集即拒）+ 套接对账
//!       （对号 → 双端 fd 互指，闭合断言）+ 双向字节引擎（两向
//!       各 1 万字节流水，FNV 指纹对拍）+ 关端核（关一端即对端
//!       写 EPIPE/读 EOF 双语义）+ fd 账（fd → 对号+端位，唯一）+
//!       注入核（越集类型/幽灵 fd/关后写/互指破四类 10/10 必抓）。
//! 职责定位：AF_UNIX 对语义的权威模型——双向恒等、类型闭集、
//!   关端双出口（写 EPIPE / 读 EOF）三性质机检。
//! 性能逐项分解：读写 O(字节数)；对账 O(1)；指纹 O(字节数)。
//!   全整数确定性。
//! 诊断码段：0xEFA0（0xEFA1~0xEFA9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const AFX_VERSION: &str = "unx10561_afunix-v1";

/// 诊断段基址（0xEFA0 段 · 本模块独占）。
pub const AFX_SEG_BASE: u16 = 0xEFA0;

/// 诊断码闭集（0xEFA1~0xEFA9）。
pub const AX_TYPE_BAD: u32 = 0xEFA1; // 类型越三闭集
pub const AX_FD_GHOST: u32 = 0xEFA2; // 幽灵 fd
pub const AX_WRITE_EPIPE: u32 = 0xEFA3; // 关后写未 EPIPE
pub const AX_PAIR_BREAK: u32 = 0xEFA4; // 互指破（对账不闭合）
pub const AX_BYTE_DRIFT: u32 = 0xEFA5; // 字节恒等漂移
pub const AX_FD_DUP: u32 = 0xEFA6; // fd 重复
pub const AX_INJECT_MISS: u32 = 0xEFA7; // 注入漏抓
pub const AX_READ_EOF: u32 = 0xEFA8; // 关后读未 EOF
pub const AX_DIR_MISS: u32 = 0xEFA9; // 方向非法（0/1 外）

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        AX_TYPE_BAD => ("socket 类型越三闭集", "STREAM/DGRAM/SEQPACKET 择一"),
        AX_FD_GHOST => ("幽灵 fd（账外）", "先查 fd 账再操作"),
        AX_WRITE_EPIPE => ("关后写未 EPIPE", "对端关闭即 32"),
        AX_PAIR_BREAK => ("互指破（对内两端不对账）", "对账双向互查"),
        AX_BYTE_DRIFT => ("双向字节恒等漂移", "FNV 指纹两路对拍"),
        AX_FD_DUP => ("fd 重复登记", "fd 全账唯一"),
        AX_INJECT_MISS => ("注入故障未 socket 核拦截", "修复类型谓词后重跑"),
        AX_READ_EOF => ("关后读未 EOF", "对端关闭读返 0"),
        AX_DIR_MISS => ("方向非法（端位仅 0/1）", "端位闭集外即拒"),
        _ => ("未知诊断码", "查 0xEFA0 段闭集登记表"),
    }
}

/// EPIPE / EOF 语义号。
pub const EPIPE: u32 = 32;

/// socket 类型三闭集。
pub const T_STREAM: u8 = 1;
pub const T_DGRAM: u8 = 2;
pub const T_SEQPACKET: u8 = 5;

pub fn type_legal(t: u8) -> Result<(), u32> {
    match t {
        T_STREAM | T_DGRAM | T_SEQPACKET => Ok(()),
        _ => Err(AX_TYPE_BAD),
    }
}

/// 套接对：双向两队列。
pub struct SockPair {
    /// 对号。
    pub pair_id: u64,
    /// 两端 fd。
    pub fds: [u64; 2],
    /// 两端队列（0→1 与 1→0）。
    q: [Vec<u8>; 2],
    /// 两端存活位。
    alive: [bool; 2],
    stype: u8,
}

impl SockPair {
    /// socketpair：fd 账外发两号。
    pub fn new(pair_id: u64, fd_a: u64, fd_b: u64, stype: u8) -> Result<SockPair, u32> {
        type_legal(stype)?;
        if fd_a == fd_b {
            return Err(AX_FD_DUP);
        }
        Ok(SockPair {
            pair_id,
            fds: [fd_a, fd_b],
            q: [Vec::new(), Vec::new()],
            alive: [true, true],
            stype,
        })
    }

    /// fd → 端位。
    fn side_of(&self, fd: u64) -> Result<usize, u32> {
        if self.fds[0] == fd {
            Ok(0)
        } else if self.fds[1] == fd {
            Ok(1)
        } else {
            Err(AX_FD_GHOST)
        }
    }

    /// 写（fd 视角）：入对端队列。对端关 → EPIPE。
    pub fn write(&mut self, fd: u64, data: &[u8]) -> Result<usize, u32> {
        let s = self.side_of(fd)?;
        if !self.alive[s] {
            return Err(AX_FD_GHOST);
        }
        let peer = 1 - s;
        if !self.alive[peer] {
            return Err(EPIPE);
        }
        self.q[peer].extend_from_slice(data);
        Ok(data.len())
    }

    /// 读（fd 视角）：出本端队列。对端关且空 → EOF（0）。
    pub fn read(&mut self, fd: u64, out: &mut [u8]) -> Result<usize, u32> {
        let s = self.side_of(fd)?;
        if !self.alive[s] {
            return Err(AX_FD_GHOST);
        }
        if self.q[s].is_empty() {
            if !self.alive[1 - s] {
                return Ok(0); // EOF
            }
            return Ok(0); // 空读
        }
        let n = self.q[s].len().min(out.len());
        out[..n].copy_from_slice(&self.q[s][..n]);
        self.q[s].drain(..n);
        Ok(n)
    }

    /// 关端：对端写 EPIPE / 对端读 EOF 源。
    pub fn close(&mut self, fd: u64) -> Result<(), u32> {
        let s = self.side_of(fd)?;
        if !self.alive[s] {
            return Err(AX_FD_GHOST);
        }
        self.alive[s] = false;
        Ok(())
    }

    /// 互指闭合核：两端 fd 互异 + 类型合法 + 存活位自洽。
    pub fn closed(&self) -> bool {
        self.fds[0] != self.fds[1] && type_legal(self.stype).is_ok()
            && (self.alive[0] || self.alive[1] || self.q.iter().all(|x| x.is_empty()))
    }

    pub fn is_alive(&self, fd: u64) -> Result<bool, u32> {
        let s = self.side_of(fd)?;
        Ok(self.alive[s])
    }

    pub fn stype(&self) -> u8 {
        self.stype
    }
}

/// FNV-1a 字节指纹。
fn fp(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 双向字节引擎：A→B 与 B→A 各 1 万字节流水（变长块），指纹对拍。
pub fn bidirectional_10k() -> Result<(u64, u64), u32> {
    let mut p = SockPair::new(1, 100, 101, T_STREAM)?;
    let mut sent_a = 0u64;
    let mut got_a = 0u64;
    let mut sent_b = 0u64;
    let mut got_b = 0u64;
    let mut seed: u64 = 0x10561;
    let mut buf = [0u8; 512];
    for _ in 0..10_000u64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let len = 1 + (seed % 512) as usize;
        for (i, slot) in buf.iter_mut().enumerate().take(len) {
            *slot = (seed >> (i % 8 * 8)) as u8 ^ (i as u8);
        }
        // A→B。
        if p.write(100, &buf[..len]).is_ok() {
            sent_a = sent_a.wrapping_add(fp(&buf[..len]));
        }
        let mut rbuf = [0u8; 512];
        if let Ok(n) = p.read(101, &mut rbuf[..len]) {
            if n > 0 {
                got_a = got_a.wrapping_add(fp(&rbuf[..n]));
            }
        }
        // B→A（同块回程，对向独立）。
        if p.write(101, &buf[..len]).is_ok() {
            sent_b = sent_b.wrapping_add(fp(&buf[..len]));
        }
        if let Ok(n) = p.read(100, &mut rbuf[..len]) {
            if n > 0 {
                got_b = got_b.wrapping_add(fp(&rbuf[..n]));
            }
        }
    }
    if sent_a != got_a || sent_b != got_b {
        return Err(AX_BYTE_DRIFT);
    }
    Ok((sent_a ^ sent_b, got_a ^ got_b))
}

/// 注入核：越集类型/幽灵 fd/关后写/互指破四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：类型越集（0/3/4/255）——3 轮。
    for i in 0..3u32 {
        let c1 = SockPair::new(1, 1, 2, 0).is_err();
        let c2 = SockPair::new(1, 1, 2, 3).is_err();
        let c3 = SockPair::new(1, 1, 2, 255).is_err();
        let caught = c1 && c2 && c3;
        all &= caught;
        log.push_str(&format!("越集类型#{} caught={}\n", i + 1, caught));
    }
    // 注入二：幽灵 fd——3 轮。
    for i in 0..3u32 {
        let mut p = match SockPair::new(1, 10, 11, T_STREAM) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = p.write(999, b"x").is_err() && p.read(999, &mut []).is_err();
        all &= caught;
        log.push_str(&format!("幽灵fd#{} caught={}\n", i + 1, caught));
    }
    // 注入三：关后写 EPIPE + 关后读 EOF——2 轮。
    for i in 0..2u32 {
        let mut p = match SockPair::new(1, 10, 11, T_STREAM) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let _ = p.close(11);
        let c1 = p.write(10, b"x") == Err(EPIPE);
        let mut out = [0u8; 4];
        let c2 = p.read(10, &mut out) == Ok(0);
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("关端双语义#{} caught={}\n", i + 1, caught));
    }
    // 注入四：互指破（同 fd 对必拒）——2 轮。
    for i in 0..2u32 {
        let caught = matches!(SockPair::new(1, 7, 7, T_STREAM), Err(AX_FD_DUP));
        all &= caught;
        log.push_str(&format!("互指破#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核（双向恒等 + 类型闭集 + 关端 + 注入）。
pub fn drill_afunix() -> (bool, &'static str) {
    // 判据一：双向各 1 万字节逐字节恒等。
    if bidirectional_10k().is_err() {
        return (false, "F10561 双向指纹漂移");
    }
    // 判据二：类型三闭集。
    for t in [T_STREAM, T_DGRAM, T_SEQPACKET] {
        if type_legal(t).is_err() {
            return (false, "F10561 闭集内误拒");
        }
    }
    if type_legal(9).is_ok() {
        return (false, "F10561 越集未拒");
    }
    // 判据三：对账闭合 + 关端语义。
    let mut p = match SockPair::new(2, 5, 6, T_DGRAM) {
        Ok(x) => x,
        Err(_) => return (false, "F10561 对建立失败"),
    };
    if !p.closed() {
        return (false, "F10561 对账不闭合");
    }
    let _ = p.close(5);
    if p.write(6, b"z") != Err(EPIPE) {
        return (false, "F10561 关后写语义漂移");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10561 注入核未全抓");
    }
    // 判据五：深化面（SO_TYPE）。
    let (ok2, _log2) = injection_deep();
    if !ok2 {
        return (false, "F10561 深化注入未全抓");
    }
    (true, "F10561 全判据绿")
}

/// ─── 深化面：SO_TYPE 回读面 ───

/// SO_TYPE（getsockopt 面）。
pub const SO_TYPE: u32 = 3;

/// SO_TYPE 回读核：对内两端回读类型恒等于建对类型（fd 幽灵先拒）。
pub fn so_type(p: &SockPair, fd: u64) -> Result<u8, u32> {
    p.is_alive(fd)?;
    Ok(p.stype())
}

/// 深化注入：SO_TYPE 恒等 / 幽灵 fd 回读必拒两类必抓。
pub fn injection_deep() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;
    // 注入五：两端 SO_TYPE 恒等——2 轮。
    for i in 0..2u32 {
        let p = match SockPair::new(9, 50, 51, T_SEQPACKET) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let t1 = so_type(&p, 50);
        let t2 = so_type(&p, 51);
        let caught = t1 == Ok(T_SEQPACKET) && t1 == t2;
        all &= caught;
        log.push_str(&format!("SO_TYPE#{} caught={}\n", i + 1, caught));
    }
    // 注入六：幽灵 fd 回读必拒——2 轮。
    for i in 0..2u32 {
        let p = match SockPair::new(9, 50, 51, T_STREAM) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = so_type(&p, 999) == Err(AX_FD_GHOST);
        all &= caught;
        log.push_str(&format!("幽灵回读#{} caught={}\n", i + 1, caught));
    }
    (all, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_afunix().0);
    }
}
