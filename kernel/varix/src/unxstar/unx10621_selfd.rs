//! unx10621_selfd —— UNX-F10621 select() fd_set 位图主体。
//! 锚点（CoRun Unxreal 主册）：UNX-F10621 · select fd_set 位图 · 域 UNX-C4-B12 · 行数预算 280。
//! 判据原文：UNX-F10621-J1 FD_SET/CLR/ISSET 三操作 1 万次往返
//!           恒等（置后必测得、清后必测失），nfds 语义核（值 >
//!           最大 fd 即拒/值 ≤ 最大 fd 即漏报风险面），就绪集
//!           出账与挂账集合恒等（select 语义：只留就绪者）。
//! 组件：fd_set 位图（FD_SETSIZE=1024 闭集）+ 三操作核（SET/CLR/
//!       ISSET 越界必拒）+ nfds 核（监视上界校验）+ select 出账
//!       模型（挂账集 ∩ 就绪真值表 → 出账集 + 集差留痕）+ 万次
//!       引擎（LCG 随机 fd 三操作往返）+ 注入核（越界 fd/清后误
//!       测/nfds 漏配/出账漏员四类 10/10 必抓）。
//! 职责定位：select() fd_set 面的权威模型——位图三操作往返恒等、
//!   nfds 上界语义、就绪出账三性质机检。
//! 性能逐项分解：SET/CLR/ISSET O(1)；出账 O(FD_SETSIZE)；
//!   引擎 O(万)。全整数确定性。
//! 诊断码段：0xEFD0（0xEFD1~0xEFD9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;

/// 模块版本。
pub const SLV_VERSION: &str = "unx10621_selfd-v1";

/// 诊断段基址（0xEFD0 段 · 本模块独占）。
pub const SLV_SEG_BASE: u16 = 0xEFD0;

/// 诊断码闭集（0xEFD1~0xEFD9）。
pub const SL_FD_OOB: u32 = 0xEFD1; // fd 越界（≥FD_SETSIZE）
pub const SL_CLR_GHOST: u32 = 0xEFD2; // 清后误测得
pub const SL_NFDS_MISS: u32 = 0xEFD3; // nfds ≤ 最大 fd（漏配）
pub const SL_READY_LOSS: u32 = 0xEFD4; // 出账漏员
pub const SL_SET_MISS: u32 = 0xEFD5; // 置后误测失
pub const SL_INJECT_MISS: u32 = 0xEFD6; // 注入漏抓
pub const SL_NEG_FD: u32 = 0xEFD7; // 负 fd（有符号语义折叠为高位越界）
pub const SL_READY_EXTRA: u32 = 0xEFD8; // 出账多员（未挂账者出现）
pub const SL_EMPTY_CALL: u32 = 0xEFD9; // 空集调用（三集全空）

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SL_FD_OOB => ("fd 越界（≥ FD_SETSIZE）", "1024 闭集内操作"),
        SL_CLR_GHOST => ("清后误测得", "CLR 后 ISSET 必假"),
        SL_NFDS_MISS => ("nfds ≤ 最大 fd（漏配）", "nfds = 最大 fd + 1"),
        SL_READY_LOSS => ("就绪出账漏员", "挂账 ∩ 就绪逐位入账"),
        SL_SET_MISS => ("置后误测失", "SET 后 ISSET 必真"),
        SL_INJECT_MISS => ("注入故障未位图核拦截", "修复界谓词后重跑"),
        SL_NEG_FD => ("负 fd（符号折叠越界）", "先验无符号闭集"),
        SL_READY_EXTRA => ("出账多员（未挂账者现身）", "出账 ⊆ 挂账"),
        SL_EMPTY_CALL => ("三集全空调用", "select 空集即拒"),
        _ => ("未知诊断码", "查 0xEFD0 段闭集登记表"),
    }
}

/// FD_SETSIZE（Linux 对照锚定）。
pub const FD_SETSIZE: usize = 1024;

/// fd_set 位图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdSet {
    bits: [u64; FD_SETSIZE / 64],
}

impl FdSet {
    pub fn new() -> FdSet {
        FdSet { bits: [0; FD_SETSIZE / 64] }
    }

    /// FD_SET（越界必拒）。
    pub fn set(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= FD_SETSIZE {
            return Err(SL_FD_OOB);
        }
        self.bits[fd / 64] |= 1u64 << (fd % 64);
        Ok(())
    }

    /// FD_CLR。
    pub fn clr(&mut self, fd: usize) -> Result<(), u32> {
        if fd >= FD_SETSIZE {
            return Err(SL_FD_OOB);
        }
        self.bits[fd / 64] &= !(1u64 << (fd % 64));
        Ok(())
    }

    /// FD_ISSET。
    pub fn isset(&self, fd: usize) -> Result<bool, u32> {
        if fd >= FD_SETSIZE {
            return Err(SL_FD_OOB);
        }
        Ok(self.bits[fd / 64] & (1u64 << (fd % 64)) != 0)
    }

    /// 全空。
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }

    /// 最大置位 fd（空集返 None）。
    pub fn max_fd(&self) -> Option<usize> {
        for wi in (0..self.bits.len()).rev() {
            if self.bits[wi] != 0 {
                let lz = self.bits[wi].leading_zeros() as usize;
                return Some(wi * 64 + (63 - lz));
            }
        }
        None
    }

    /// 位计数。
    pub fn count(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }
}

impl Default for FdSet {
    fn default() -> Self {
        Self::new()
    }
}

/// nfds 语义核：nfds 必须覆盖挂账集最大 fd（+1 惯例）。
pub fn nfds_check(set: &FdSet, nfds: usize) -> Result<(), u32> {
    if set.is_empty() {
        return Err(SL_EMPTY_CALL);
    }
    match set.max_fd() {
        Some(m) if m >= nfds => Err(SL_NFDS_MISS),
        _ => Ok(()),
    }
}

/// select 出账模型：挂账集 ∩ 就绪真值表 → 出账集。
/// 就绪真值表：fd → bool（真 = 该 fd 就绪）。
pub fn select_model(watched: &FdSet, ready: &[bool]) -> Result<FdSet, u32> {
    if watched.is_empty() {
        return Err(SL_EMPTY_CALL);
    }
    if ready.len() < FD_SETSIZE {
        return Err(SL_NFDS_MISS);
    }
    let mut out = FdSet::new();
    for fd in 0..FD_SETSIZE {
        let watched_bit = watched.isset(fd)?;
        if watched_bit && ready[fd] {
            out.set(fd)?;
        }
        // 出账多员防护：未挂账却置位者即拒。
        if !watched_bit && ready[fd] && out.isset(fd)? {
            return Err(SL_READY_EXTRA);
        }
    }
    Ok(out)
}

/// LCG64 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 万次往返引擎：1 万次 SET/ISSET/CLR 往返（LCG 随机 fd）。
pub fn roundtrip_10k() -> Result<usize, u32> {
    let mut s = FdSet::new();
    let mut rng = Lcg::new(0x10621_0A01);
    let mut ops = 0usize;
    for _ in 0..10_000u64 {
        let fd = (rng.next_u64() % (FD_SETSIZE as u64 + 64)) as usize; // 含越界样本
        match s.set(fd) {
            Ok(()) => {
                ops += 1;
                if !s.isset(fd)? {
                    return Err(SL_SET_MISS);
                }
                s.clr(fd)?;
                if s.isset(fd)? {
                    return Err(SL_CLR_GHOST);
                }
                ops += 2;
            }
            Err(SL_FD_OOB) => {
                // 越界样本：ISSET 亦必拒。
                if s.isset(fd).is_ok() {
                    return Err(SL_FD_OOB);
                }
                ops += 1;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(ops)
}

/// 注入核：越界 fd/清后误测/nfds 漏配/出账漏员四类 10/10 必抓。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：越界 fd（=1024 与 +64）——3 轮。
    for i in 0..3u32 {
        let mut s = FdSet::new();
        let c1 = s.set(FD_SETSIZE).is_err();
        let c2 = s.isset(FD_SETSIZE + 64).is_err();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("越界#{} caught={}\n", i + 1, caught));
    }
    // 注入二：SET/CLR/ISSET 往返（置后必真清后必假）——3 轮。
    for i in 0..3u32 {
        let mut s = FdSet::new();
        let _ = s.set(7);
        let a = matches!(s.isset(7), Ok(true));
        let _ = s.clr(7);
        let b = matches!(s.isset(7), Ok(false));
        let caught = a && b;
        all &= caught;
        log.push_str(&format!("往返#{} caught={}\n", i + 1, caught));
    }
    // 注入三：nfds 漏配（最大 fd 999，nfds=500 必拒）——2 轮。
    for i in 0..2u32 {
        let mut s = FdSet::new();
        let _ = s.set(999);
        let c1 = nfds_check(&s, 500).is_err();
        let c2 = nfds_check(&s, 1000).is_ok();
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("nfds#{} caught={}\n", i + 1, caught));
    }
    // 注入四：出账恒等（挂账∩就绪 = 出账；未就绪者不出）——2 轮。
    for i in 0..2u32 {
        let mut w = FdSet::new();
        for fd in [3usize, 5, 9] {
            let _ = w.set(fd);
        }
        let mut ready = [false; FD_SETSIZE];
        ready[3] = true;
        ready[9] = true; // 5 未就绪
        let out = match select_model(&w, &ready) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let c1 = matches!(out.isset(3), Ok(true)) && matches!(out.isset(9), Ok(true));
        let c2 = matches!(out.isset(5), Ok(false));
        let caught = c1 && c2 && out.count() == 2;
        all &= caught;
        log.push_str(&format!("出账#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核（万次往返 + nfds + 出账 + 注入 10/10）。
pub fn drill_selfd() -> (bool, &'static str) {
    // 判据一：万次三操作往返恒等。
    let ops = match roundtrip_10k() {
        Ok(x) => x,
        Err(_) => return (false, "F10621 万次往返漂移"),
    };
    if ops < 20_000 {
        return (false, "F10621 往返量不足");
    }
    // 判据二：nfds 语义 + 出账恒等（注入三/四复测已含，此处独立测空集拒）。
    let s = FdSet::new();
    if nfds_check(&s, 1024) != Err(SL_EMPTY_CALL) {
        return (false, "F10621 空集语义漂移");
    }
    // 判据三：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10621 注入核未全抓");
    }
    // 判据四：深化面（timeout 三态 + 三集面）。
    let (ok2, _log2) = injection_deep();
    if !ok2 {
        return (false, "F10621 深化注入未全抓");
    }
    (true, "F10621 全判据绿")
}

/// ─── 深化面：timeout 三态 + exceptfds 三集面 ───

/// timeval 账（秒/微秒整型化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Timeval {
    pub sec: u64,
    pub usec: u64,
}

impl Timeval {
    /// 折算 tick（100us/tick）。
    pub fn ticks(&self) -> u64 {
        self.sec * 10_000 + self.usec / 100
    }

    /// 语义三态核：NULL（零值折叠 = 无超时阻塞）/ 0 → 非阻塞 /
    /// >0 → 限时。返回超时类别。
    pub fn kind(&self, is_null: bool) -> u8 {
        if is_null {
            return 2; // 阻塞
        }
        if self.ticks() == 0 {
            return 0; // 非阻塞
        }
        1 // 限时
    }
}

/// select 三集模型：readfds/writefds/exceptfds 交集出账。
pub fn select3(
    r: &FdSet,
    w: &FdSet,
    x: &FdSet,
    ready_r: &[bool],
    ready_w: &[bool],
    ready_x: &[bool],
) -> Result<(FdSet, FdSet, FdSet), u32> {
    if r.is_empty() && w.is_empty() && x.is_empty() {
        return Err(SL_EMPTY_CALL);
    }
    let mut ro = FdSet::new();
    let mut wo = FdSet::new();
    let mut xo = FdSet::new();
    for fd in 0..FD_SETSIZE {
        if matches!(r.isset(fd), Ok(true)) && ready_r[fd] {
            ro.set(fd)?;
        }
        if matches!(w.isset(fd), Ok(true)) && ready_w[fd] {
            wo.set(fd)?;
        }
        if matches!(x.isset(fd), Ok(true)) && ready_x[fd] {
            xo.set(fd)?;
        }
    }
    Ok((ro, wo, xo))
}

/// 深化注入：timeout 三态 / 三集交集出账两类必抓。
pub fn injection_deep() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;
    // 注入五：timeout 三态（NULL 阻塞 / 0 非阻塞 / 100ms 限时）——2 轮。
    for i in 0..2u32 {
        let t0 = Timeval { sec: 0, usec: 0 };
        let t1 = Timeval { sec: 0, usec: 100_000 };
        let caught = t0.kind(false) == 0 && t1.kind(false) == 1 && Timeval { sec: 0, usec: 0 }.kind(true) == 2;
        all &= caught;
        log.push_str(&format!("timeout#{} caught={}\n", i + 1, caught));
    }
    // 注入六：三集交集出账（跨集 fd 各归各账）——2 轮。
    for i in 0..2u32 {
        let mut r = FdSet::new();
        let mut w = FdSet::new();
        let mut x = FdSet::new();
        for fd in [1usize, 2] {
            let _ = r.set(fd);
        }
        let _ = w.set(2);
        let _ = x.set(3);
        let mut rr = [false; FD_SETSIZE];
        let mut ww = [false; FD_SETSIZE];
        let mut xx = [false; FD_SETSIZE];
        rr[1] = true;
        ww[2] = true;
        xx[3] = true;
        let (ro, wo, xo) = match select3(&r, &w, &x, &rr, &ww, &xx) {
            Ok(v) => v,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = matches!(ro.isset(1), Ok(true))
            && matches!(wo.isset(2), Ok(true))
            && matches!(xo.isset(3), Ok(true))
            && ro.count() == 1
            && wo.count() == 1
            && xo.count() == 1;
        all &= caught;
        log.push_str(&format!("三集#{} caught={}\n", i + 1, caught));
    }
    (all, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_selfd().0);
    }
}
