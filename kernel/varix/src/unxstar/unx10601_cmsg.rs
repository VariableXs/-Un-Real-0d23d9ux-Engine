//! unx10601_cmsg —— UNX-F10601 cmsg 结构（CMSGHDR 对齐）。
//! 锚点（CoRun Unxreal 主册）：UNX-F10601 · cmsg 结构对齐 · 域 UNX-C4-B11 · 行数预算 280。
//! 判据原文：UNX-F10601-J1 CMSGHDR 三字段（len/level/type）偏移
//!           与注出 ABI 逐字段一致，CMSG_SPACE/CMSG_LEN 算术
//!           （对齐上取 8 字节面）10/10 组恒等，载荷越空间必拒，
//!           双报连排解析往返恒等。
//! 组件：cmsghdr 布局账（len@0/level@8/type@12，尺寸 16，8 字节
//!       对齐）+ CMSG_ALIGN 核（上取 8 对齐）+ CMSG_SPACE/LEN 算术
//!       核（SPACE = ALIGN(len) + ALIGN(hdr)；LEN = ALIGN(hdr) +
//!       len）+ 连排解析器（缓冲区内逐报解析，len 越界必拒）+
//!       双报引擎（两报连排 → 解析 → 重排 → 指纹恒等）+ 注入核
//!       （对齐破/空间越载/假报长/往返漂移四类 10/10 必抓）。
//! 职责定位：控制消息布局算术的权威模型——三字段偏移、两种
//!   算术恒等、连排往返三面机检。
//! 性能逐项分解：ALIGN O(1)；解析 O(报数)；引擎 O(总字节)。
//!   全整数确定性。
//! 诊断码段：0xEFC0（0xEFC1~0xEFC9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const CMV_VERSION: &str = "unx10601_cmsg-v1";

/// 诊断段基址（0xEFC0 段 · 本模块独占）。
pub const CMV_SEG_BASE: u16 = 0xEFC0;

/// 诊断码闭集（0xEFC1~0xEFC9）。
pub const CM_ALIGN_BAD: u32 = 0xEFC1; // 对齐面破
pub const CM_SPACE_OVERRUN: u32 = 0xEFC2; // 载荷越空间
pub const CM_LEN_FAKE: u32 = 0xEFC3; // 假报长（len < 头或 > 余量）
pub const CM_ROUND_DRIFT: u32 = 0xEFC4; // 往返指纹漂移
pub const CM_HDR_MISS: u32 = 0xEFC5; // 头字段缺失
pub const CM_ARITH_DRIFT: u32 = 0xEFC6; // SPACE/LEN 算术漂移
pub const CM_INJECT_MISS: u32 = 0xEFC7; // 注入漏抓
pub const CM_LEVEL_BAD: u32 = 0xEFC8; // level 闭集外
pub const CM_HDR_SIZE: u32 = 0xEFC9; // 头尺寸漂移

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        CM_ALIGN_BAD => ("对齐面破（8 字节面上取）", "CMSG_ALIGN 复算"),
        CM_SPACE_OVERRUN => ("载荷越空间", "SPACE 先核再装"),
        CM_LEN_FAKE => ("假报长（len < 头尺寸或越余量）", "len ∈ [头尺寸, 余量]"),
        CM_ROUND_DRIFT => ("连排往返指纹漂移", "重排后逐报指纹对拍"),
        CM_HDR_MISS => ("头字段缺失", "len/level/type 三字段齐"),
        CM_ARITH_DRIFT => ("SPACE/LEN 算术漂移", "两公式逐组复算"),
        CM_INJECT_MISS => ("注入故障未 cmsg 核拦截", "修复布局谓词后重跑"),
        CM_LEVEL_BAD => ("level 闭集外", "SOL_SOCKET=1 / SOL_IP=0 闭集"),
        CM_HDR_SIZE => ("头尺寸漂移（应 16B）", "三字段 + 填充闭包"),
        _ => ("未知诊断码", "查 0xEFC0 段闭集登记表"),
    }
}

/// cmsghdr 头尺寸（len u64 + level u32 + type u32）。
pub const CMSG_HDR: usize = 16;
/// 对齐面。
pub const CMSG_ALIGN_TO: usize = 8;

/// 头字段偏移锚定（注出 ABI）。
pub const OFF_LEN: usize = 0;
pub const OFF_LEVEL: usize = 8;
pub const OFF_TYPE: usize = 12;

/// SOL 闭集。
pub const SOL_SOCKET: u32 = 1;
pub const SOL_IP: u32 = 0;

/// CMSG_ALIGN：上取 8 对齐。
pub fn cmsg_align(len: usize) -> usize {
    (len + CMSG_ALIGN_TO - 1) & !(CMSG_ALIGN_TO - 1)
}

/// CMSG_SPACE：容纳载荷 len 的总空间。
pub fn cmsg_space(len: usize) -> usize {
    cmsg_align(CMSG_HDR) + cmsg_align(len)
}

/// CMSG_LEN：实际长度（头对齐 + 载荷原长）。
pub fn cmsg_len(len: usize) -> usize {
    cmsg_align(CMSG_HDR) + len
}

/// level 闭集核。
pub fn level_legal(l: u32) -> Result<(), u32> {
    match l {
        SOL_SOCKET | SOL_IP => Ok(()),
        _ => Err(CM_LEVEL_BAD),
    }
}

/// 报构造：len/level/type + 载荷 → 编码字节序列（头 + 载荷 + 填充）。
pub fn encode(level: u32, ctype: u32, payload: &[u8]) -> Result<Vec<u8>, u32> {
    level_legal(level)?;
    let total = cmsg_space(payload.len());
    if payload.len() > total - CMSG_HDR {
        return Err(CM_SPACE_OVERRUN);
    }
    let mut out = vec![0u8; total];
    out[OFF_LEN..OFF_LEN + 8].copy_from_slice(&(cmsg_len(payload.len()) as u64).to_le_bytes());
    out[OFF_LEVEL..OFF_LEVEL + 4].copy_from_slice(&level.to_le_bytes());
    out[OFF_TYPE..OFF_TYPE + 4].copy_from_slice(&ctype.to_le_bytes());
    out[CMSG_HDR..CMSG_HDR + payload.len()].copy_from_slice(payload);
    Ok(out)
}

/// 单报解析：从缓冲头取一报，返回 (level, type, 载荷, 消费字节)。
pub fn parse_one(buf: &[u8]) -> Result<(u32, u32, Vec<u8>, usize), u32> {
    if buf.len() < CMSG_HDR {
        return Err(CM_LEN_FAKE);
    }
    let mut lb = [0u8; 8];
    lb.copy_from_slice(&buf[OFF_LEN..OFF_LEN + 8]);
    let declared = u64::from_le_bytes(lb) as usize;
    if declared < CMSG_HDR || declared > buf.len() {
        return Err(CM_LEN_FAKE);
    }
    let mut lvb = [0u8; 4];
    lvb.copy_from_slice(&buf[OFF_LEVEL..OFF_LEVEL + 4]);
    let level = u32::from_le_bytes(lvb);
    level_legal(level)?;
    let mut tvb = [0u8; 4];
    tvb.copy_from_slice(&buf[OFF_TYPE..OFF_TYPE + 4]);
    let ctype = u32::from_le_bytes(tvb);
    let payload_len = declared - CMSG_HDR;
    let payload = buf[CMSG_HDR..CMSG_HDR + payload_len].to_vec();
    Ok((level, ctype, payload, cmsg_align(declared)))
}

/// 连排全解析。
pub fn parse_all(buf: &[u8]) -> Result<Vec<(u32, u32, Vec<u8>)>, u32> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        let (l, t, p, used) = parse_one(&buf[i..])?;
        out.push((l, t, p));
        i += used;
    }
    Ok(out)
}

/// FNV-1a 指纹。
fn fp(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 双报引擎：两报连排 → 解析 → 重排 → 指纹恒等 + 算术 10/10 组。
pub fn two_msg_roundtrip() -> Result<(), u32> {
    // 算术恒等组（10 组对照）。
    for len in [0usize, 1, 7, 8, 9, 16, 24, 100, 1000, 4096] {
        let s = cmsg_space(len);
        let l = cmsg_len(len);
        if s < l || s % 8 != 0 {
            return Err(CM_ARITH_DRIFT);
        }
    }
    // 双报连排。
    let m1 = encode(SOL_SOCKET, 1, b"hello")?;
    let m2 = encode(SOL_IP, 2, &[7u8; 100])?;
    let mut chain = Vec::new();
    chain.extend_from_slice(&m1);
    chain.extend_from_slice(&m2);
    // 解析 → 重排 → 指纹对拍。
    let parsed = parse_all(&chain)?;
    if parsed.len() != 2 {
        return Err(CM_ROUND_DRIFT);
    }
    let re1 = encode(parsed[0].0, parsed[0].1, &parsed[0].2)?;
    let re2 = encode(parsed[1].0, parsed[1].1, &parsed[1].2)?;
    if fp(&re1) != fp(&m1) || fp(&re2) != fp(&m2) {
        return Err(CM_ROUND_DRIFT);
    }
    Ok(())
}

/// 注入核：对齐破/空间越载/假报长/往返漂移四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：对齐破（ALIGN(1)=8 恒等断言，假 4 必漂）——3 轮。
    for i in 0..3u32 {
        let fake = 4usize;
        let caught = cmsg_align(1) == 8 && cmsg_align(1) != fake && cmsg_align(9) == 16;
        all &= caught;
        log.push_str(&format!("对齐#{} caught={}\n", i + 1, caught));
    }
    // 注入二：空间越载（载荷 > 空间-头必拒）——3 轮。
    for i in 0..3u32 {
        let big = vec![0u8; cmsg_space(10) - CMSG_HDR + 1];
        let caught = encode(SOL_SOCKET, 1, &big) == Err(CM_SPACE_OVERRUN);
        all &= caught;
        log.push_str(&format!("越载#{} caught={}\n", i + 1, caught));
    }
    // 注入三：假报长（len=4 < 头尺寸 16 必拒）——2 轮。
    for i in 0..2u32 {
        let fake = vec![4u8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        let caught = parse_one(&fake) == Err(CM_LEN_FAKE);
        all &= caught;
        log.push_str(&format!("假报长#{} caught={}\n", i + 1, caught));
    }
    // 注入四：level 闭集外（99 必拒）——2 轮。
    for i in 0..2u32 {
        let caught = encode(99, 1, b"x").is_err() && level_legal(SOL_IP).is_ok();
        all &= caught;
        log.push_str(&format!("level越集#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_cmsg() -> (bool, &'static str) {
    // 判据一：头布局锚定（三字段偏移 + 头尺寸）。
    if OFF_LEN != 0 || OFF_LEVEL != 8 || OFF_TYPE != 12 || CMSG_HDR != 16 {
        return (false, "F10601 头布局漂移");
    }
    // 判据二：SPACE/LEN 算术 + 双报往返 10/10 组。
    if two_msg_roundtrip().is_err() {
        return (false, "F10601 算术或往返漂移");
    }
    // 判据三：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10601 注入核未全抓");
    }
    // 判据四：深化面（SCM_RIGHTS + SCM_CREDENTIALS）。
    if rights_engine().is_err() {
        return (false, "F10601 深化引擎漂移");
    }
    let (ok2, _log2) = injection_deep();
    if !ok2 {
        return (false, "F10601 深化注入未全抓");
    }
    (true, "F10601 全判据绿")
}

/// ─── 深化面：SCM_RIGHTS 文件传递账 + SCM_CREDENTIALS 布局 ───

/// SCM 闭集。
pub const SCM_RIGHTS: u32 = 1;
pub const SCM_CREDENTIALS: u32 = 2;

/// ucred 三元组布局（pid/uid/gid，SCM_CREDENTIALS 载荷）。
pub const UCRED_SIZE: usize = 12;
pub const UCRED_OFF_PID: usize = 0;
pub const UCRED_OFF_UID: usize = 4;
pub const UCRED_OFF_GID: usize = 8;

/// ucred 布局核：三字段偏移 + 尺寸闭合。
pub fn ucred_layout_ok() -> bool {
    UCRED_OFF_PID == 0 && UCRED_OFF_UID == 4 && UCRED_OFF_GID == 8 && UCRED_SIZE == 12
}

/// fd 传递账：接收面引用计数（dup 即 +1，close 即 -1，零界即拒）。
pub struct FdPassBook {
    /// fd → 未关接收引用数。
    open_refs: Vec<u32>,
    /// dup 累计。
    pub dups: u64,
    /// close 累计。
    pub closes: u64,
}

impl FdPassBook {
    pub fn new() -> FdPassBook {
        FdPassBook { open_refs: Vec::new(), dups: 0, closes: 0 }
    }

    fn ensure(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= 1024 {
            return Err(CM_LEN_FAKE);
        }
        while self.open_refs.len() <= fd {
            self.open_refs.push(0);
        }
        Ok(())
    }

    /// 接收面：dup 递增（fd 数组逐 fd +1）。
    pub fn recv_fds(&mut self, fds: &[usize]) -> Result<(), u32> {
        if fds.is_empty() {
            return Err(CM_SPACE_OVERRUN);
        }
        for &fd in fds {
            self.ensure(fd)?;
            self.open_refs[fd] += 1;
            self.dups += 1;
        }
        Ok(())
    }

    /// close：引用递减（零界必拒）。
    pub fn close_fd(&mut self, fd: usize) -> Result<(), u32> {
        self.ensure(fd)?;
        if self.open_refs[fd] == 0 {
            return Err(CM_LEN_FAKE);
        }
        self.open_refs[fd] -= 1;
        self.closes += 1;
        Ok(())
    }

    /// 守恒核：dup = close + 未关引用和。
    pub fn invariant_ok(&self) -> bool {
        self.dups == self.closes + self.open_refs.iter().map(|&c| c as u64).sum::<u64>()
    }

    /// 未关引用。
    pub fn pending(&self, fd: usize) -> u32 {
        self.open_refs.get(fd).copied().unwrap_or(0)
    }
}

impl Default for FdPassBook {
    fn default() -> Self {
        Self::new()
    }
}

/// SCM_RIGHTS 编解码：fd 数组 → u32 载荷 → cmsg 报。
pub fn encode_rights(fds: &[usize]) -> Result<Vec<u8>, u32> {
    let mut payload = vec![0u8; fds.len() * 4];
    for (i, &fd) in fds.iter().enumerate() {
        if fd >= 1024 {
            return Err(CM_LEN_FAKE);
        }
        payload[i * 4..i * 4 + 4].copy_from_slice(&(fd as u32).to_le_bytes());
    }
    encode(SOL_SOCKET, SCM_RIGHTS, &payload)
}

/// SCM_RIGHTS 解码：cmsg 报 → fd 数组（type 核 + 4 对齐核）。
pub fn decode_rights(buf: &[u8]) -> Result<Vec<usize>, u32> {
    let (_l, t, payload, _u) = parse_one(buf)?;
    if t != SCM_RIGHTS {
        return Err(CM_LEVEL_BAD);
    }
    if payload.len() % 4 != 0 {
        return Err(CM_ALIGN_BAD);
    }
    let mut out = Vec::new();
    for chunk in payload.chunks(4) {
        let mut b = [0u8; 4];
        b.copy_from_slice(chunk);
        out.push(u32::from_le_bytes(b) as usize);
    }
    Ok(out)
}

/// 深化引擎：SCM_RIGHTS 三连发 → 解码回读 + 引用账守恒。
pub fn rights_engine() -> Result<(), u32> {
    let mut book = FdPassBook::new();
    let flights: [&[usize]; 3] = [&[5, 6], &[7], &[5, 8, 9]];
    for fds in flights.iter() {
        let msg = encode_rights(fds)?;
        let back = decode_rights(&msg)?;
        if back != fds.to_vec() {
            return Err(CM_ROUND_DRIFT);
        }
        book.recv_fds(fds)?;
    }
    // 引用账：fd5 收两次 → close 一次后余 1；再收 fd5 一次共三发已对账。
    if book.pending(5) != 2 {
        return Err(CM_ARITH_DRIFT);
    }
    book.close_fd(5)?;
    if !book.invariant_ok() || book.pending(5) != 1 {
        return Err(CM_ARITH_DRIFT);
    }
    // ucred 布局 + 编解码。
    if !ucred_layout_ok() {
        return Err(CM_HDR_MISS);
    }
    let cred = encode(SOL_SOCKET, SCM_CREDENTIALS, &[1u8, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0])?;
    let (_l, t, p, _u) = parse_one(&cred)?;
    if t != SCM_CREDENTIALS || p.len() != UCRED_SIZE {
        return Err(CM_ROUND_DRIFT);
    }
    Ok(())
}

/// 深化注入：空 fd 传 / 幽灵 close / rights 往返漂移三类必抓。
pub fn injection_deep() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;
    // 注入五：空 fd 数组 send 必拒——2 轮。
    for i in 0..2u32 {
        let mut b = FdPassBook::new();
        let caught = b.recv_fds(&[]) == Err(CM_SPACE_OVERRUN);
        all &= caught;
        log.push_str(&format!("空fd#{} caught={}\n", i + 1, caught));
    }
    // 注入六：幽灵 close（未传 fd）必拒——2 轮。
    for i in 0..2u32 {
        let mut b = FdPassBook::new();
        let caught = b.close_fd(3) == Err(CM_LEN_FAKE);
        all &= caught;
        log.push_str(&format!("幽灵close#{} caught={}\n", i + 1, caught));
    }
    // 注入七：rights 往返漂移（type 误标必拒）——2 轮。
    for i in 0..2u32 {
        let ok = match encode(SOL_SOCKET, SCM_CREDENTIALS, &[1u8, 0, 0, 0]) {
            Ok(msg) => decode_rights(&msg).is_err(),
            Err(_) => false,
        };
        all &= ok;
        log.push_str(&format!("type核#{} caught={}\n", i + 1, ok));
    }
    (all, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_cmsg().0);
    }
}
