//! UNX-F10906 select EBADF/FD_SETSIZE 矩阵（0x1d0c0 段）
//!
//! 锚点：UNX-F10906-J1 正面：坏 fd→EBADF、fd≥FD_SETSIZE→EINVAL/越界
//! （各 10/10）；反面：域内零误报（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·select 双错误面）。
//!
//! 结构：fd 域守卫册（FD_SETSIZE 上限 + fd 存活账）+ 裁断器
//! （越界→EINVAL 优先、死 fd→EBADF 次之、合法→就绪计数）+ 注入核
//! （越界/死 fd/负 fd/冻结改表四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d0c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d0c0;
pub const SL_EBADF: u32 = SEG_BASE + 0x01; // fd 未打开或已死
pub const SL_EINVAL: u32 = SEG_BASE + 0x02; // fd≥FD_SETSIZE 或负值
pub const SL_DUP: u32 = SEG_BASE + 0x03; // 同 fd 重复入集
pub const SL_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改集
pub const SL_NOEVENT: u32 = SEG_BASE + 0x05; // 全无就绪
pub const SL_TOOBIG: u32 = SEG_BASE + 0x06; // fdmax 非法

/// FD_SETSIZE 上限。
pub const FD_SETSIZE: u32 = 1024;
/// 守卫册定容。
pub const CAP: usize = 48;
/// SL 域标（fold 种子）。
pub const SL_TAG: u64 = 0x1d0cc06;

/// 集合条目。
#[derive(Clone, Debug)]
pub struct SelSlot {
    pub fd: u32,
    pub live: bool,
    pub watch: bool,
}

impl SelSlot {
    /// 域内判定：fd < FD_SETSIZE。
    pub fn in_domain(&self) -> bool {
        self.fd < FD_SETSIZE
    }

    /// 就绪判定：域内且存活且监听。
    pub fn ready(&self) -> bool {
        self.in_domain() && self.live && self.watch
    }

    pub fn render(&self) -> String {
        let f = if self.ready() { "R" } else if !self.in_domain() { "X" } else { "I" };
        format!("S{}|{}", self.fd, f)
    }
}

/// select 双错误面守卫册。
pub struct SelGuard {
    pub slots: Vec<SelSlot>,
    pub frozen: bool,
}

impl SelGuard {
    pub fn new() -> SelGuard {
        SelGuard { slots: Vec::new(), frozen: false }
    }

    /// 入集：越界/负值（以超大 u32 表达）域守卫先裁。
    pub fn add(&mut self, fd: u32, watch: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(SL_FROZEN);
        }
        if fd >= FD_SETSIZE {
            return Err(SL_EINVAL);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(SL_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(SL_TOOBIG);
        }
        self.slots.push(SelSlot { fd, live: true, watch });
        Ok(())
    }

    /// 杀 fd。
    pub fn kill(&mut self, fd: u32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// select 主裁断：fdmax 域守卫 → 条目级越界 EBADF/EINVAL → 就绪计数。
    pub fn select(&self, fdmax: u32) -> Result<usize, u32> {
        if fdmax >= FD_SETSIZE {
            return Err(SL_EINVAL);
        }
        let mut n = 0usize;
        let mut bad = false;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.fd > fdmax {
                k += 1;
                continue;
            }
            if !e.in_domain() {
                bad = true;
            } else if !e.live {
                bad = true;
            } else if e.ready() {
                n += 1;
            }
            k += 1;
        }
        if bad {
            return Err(SL_EBADF);
        }
        if n == 0 {
            return Err(SL_NOEVENT);
        }
        Ok(n)
    }

    /// 链式 fold 指纹（序敏感，含 watch/live 位）。
    pub fn fold(&self) -> u64 {
        let mut h = SL_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.watch as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 越界 fd 入集必拒 EINVAL
    let mut g = SelGuard::new();
    got[0] = g.add(FD_SETSIZE, true).err() == Some(SL_EINVAL);
    // 2) 死 fd 必报 EBADF
    let mut d = SelGuard::new();
    let _ = d.add(5, true);
    d.kill(5);
    got[1] = d.select(10).err() == Some(SL_EBADF);
    // 3) fdmax 越界必报 EINVAL
    let mut m = SelGuard::new();
    let _ = m.add(6, true);
    got[2] = m.select(FD_SETSIZE).err() == Some(SL_EINVAL);
    // 4) 冻结后入集必抓
    let mut f = SelGuard::new();
    f.freeze();
    got[3] = f.add(7, true).err() == Some(SL_FROZEN);
    got
}

/// 监听位确定性样本。
pub fn sl_sample_watch(r: u32) -> bool {
    r.wrapping_mul(2654435761).wrapping_add(9) % 5 != 0
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10906_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10906");

    // J1 正面·EBADF 10/10
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut g = SelGuard::new();
        let _ = g.add(r + 1, true);
        g.kill(r + 1);
        if g.select(r + 10).err() == Some(SL_EBADF) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EBADF十连", h1 == 10, "死 fd 十轮全数 EBADF");

    // J2 正面·EINVAL（fd≥FD_SETSIZE）10/10
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut g = SelGuard::new();
        let big = FD_SETSIZE + r2;
        if g.add(big, true).err() == Some(SL_EINVAL) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EINVAL十连", h2 == 10, "越界 fd 十轮全数 EINVAL");

    // J3 反面：域内千次零误报
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut g = SelGuard::new();
        let fd = r3 % 16;
        let _ = g.add(fd, sl_sample_watch(r3));
        let v = g.select(31);
        let expect = if sl_sample_watch(r3) { Ok(1usize) } else { Err(SL_NOEVENT) };
        if v == expect {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-域内千次零误报", ok == 1000, "域内千轮结果与期望一致零误报");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "越界/死fd/fdmax越界/冻结全数命中");

    // J5 fdmax 越界优先裁断
    let j5 = {
        let mut g = SelGuard::new();
        let _ = g.add(9, true);
        g.select(FD_SETSIZE + 1).err() == Some(SL_EINVAL)
    };
    s.add("J5-fdmax越界先裁", j5, "fdmax 域守卫优先于条目级裁断");

    // J6 fdmax 边界语义：条目 fd 恰等于 fdmax 计入
    let j6 = {
        let mut g = SelGuard::new();
        let _ = g.add(12, true);
        g.select(12) == Ok(1)
    };
    s.add("J6-fdmax含端点", j6, "fd==fdmax 条目计入就绪");

    // J7 watch 假条目不就绪
    let j7 = {
        let mut g = SelGuard::new();
        let _ = g.add(13, false);
        g.select(20).err() == Some(SL_NOEVENT)
    };
    s.add("J7-不监听不就绪", j7, "watch 假条目不计就绪");

    // J8 重号入集拒
    let j8 = {
        let mut g = SelGuard::new();
        let _ = g.add(14, true);
        g.add(14, false).err() == Some(SL_DUP)
    };
    s.add("J8-重号拒收", j8, "同 fd 重复入集必拒");

    // J9 冻结后入集拒
    let j9 = {
        let mut g = SelGuard::new();
        g.freeze();
        g.add(15, true).err() == Some(SL_FROZEN)
    };
    s.add("J9-冻结拒入集", j9, "冻结后入集必拒");

    // J10 渲染面：就绪/越界/惰三态可区分
    let j10 = {
        let a = SelSlot { fd: 1, live: true, watch: true };
        let b = SelSlot { fd: FD_SETSIZE + 1, live: true, watch: true };
        let c = SelSlot { fd: 2, live: true, watch: false };
        a.render() != b.render() && a.render() != c.render() && a.render().contains("R") && b.render().contains("X")
    };
    s.add("J10-渲染分旗", j10, "R/X/I 三态渲染可区分");

    // J11 容量边界
    let j11 = {
        let mut g = SelGuard::new();
        let mut f = 0u32;
        let mut full = true;
        while f < CAP as u32 {
            if g.add(f, true).is_err() {
                full = false;
            }
            f += 1;
        }
        full && g.add(9999, true).err() == Some(SL_EINVAL)
    };
    s.add("J11-容量边界", j11, "域守卫先于容量裁断");

    // J12 fold 稳定：同态一致、杀 fd 必变
    let j12 = {
        let mut g = SelGuard::new();
        let _ = g.add(20, true);
        let f1 = g.fold();
        let f2 = g.fold();
        g.kill(20);
        f1 == f2 && f1 != g.fold() && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "同态指纹稳定且异态指纹必变");

    // J13 动态明细：域内千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("badf={} einval={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10906_selguard_flow() {
        let mut g = SelGuard::new();
        assert!(g.add(5, true).is_ok());
        assert!(g.add(5, false).err() == Some(SL_DUP));
        assert!(g.add(FD_SETSIZE, true).err() == Some(SL_EINVAL));
        assert_eq!(g.select(9), Ok(1));
        g.kill(5);
        assert!(g.select(9).err() == Some(SL_EBADF));
        assert!(g.select(FD_SETSIZE).err() == Some(SL_EINVAL));
        g.freeze();
        assert!(g.add(6, true).err() == Some(SL_FROZEN));
    }

    #[test]
    fn unx10906_checks_green() {
        let s = run_unx10906_checks();
        assert!(s.all_passed(), "UNX-F10906 判据未全绿");
    }
}
