//! UNX-F10904 poll EFAULT/EINVAL 矩阵（0x1d080 段）
//!
//! 锚点：UNX-F10904-J1 正面：坏 fds 指针→EFAULT、nfds 负值→EINVAL
//! （各 10/10）；反面：合法数组零误报（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·poll 双错误面）。
//!
//! 结构：切片描述符（指针有效位 + 条数符号位）+ 裁断器（指针先于
//! 条数，EFAULT → EINVAL → 正常就绪计数）+ 注入核（坏指针/负条数/
//! 超容条数/坏条目混入四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d080 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d080;
pub const PF_EFAULT: u32 = SEG_BASE + 0x01; // fds 指针无效
pub const PF_EINVAL: u32 = SEG_BASE + 0x02; // nfds 负值或超容
pub const PF_EBADF: u32 = SEG_BASE + 0x03; // 条目内坏 fd
pub const PF_NOEVENT: u32 = SEG_BASE + 0x04; // 就绪数为零
pub const PF_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改表

/// poll 数组容量上限。
pub const NFDS_CAP: i32 = 32;
/// PF 域标（fold 种子）。
pub const PF_TAG: u64 = 0x1d08c04;

/// poll 条目：用户域 events / 内核域 revents（本文件只裁断错误面）。
#[derive(Clone, Debug)]
pub struct PollSlot {
    pub fd: i32,
    pub live: bool,
    pub events: u16,
    pub revents: u16,
}

impl PollSlot {
    /// 条目是否有效（fd 为正且存活）。
    pub fn valid(&self) -> bool {
        self.fd > 0 && self.live
    }

    /// 就绪位：有效且带监听掩码。
    pub fn ready(&self) -> bool {
        self.valid() && self.events != 0
    }
}

/// poll 双错误面裁断器。
pub struct PollGate {
    pub slots: Vec<PollSlot>,
    pub ptr_valid: bool,
    pub frozen: bool,
}

impl PollGate {
    pub fn new() -> PollGate {
        PollGate { slots: Vec::new(), ptr_valid: true, frozen: false }
    }

    /// 登记条目。
    pub fn push(&mut self, fd: i32, events: u16) -> Result<(), u32> {
        if self.frozen {
            return Err(PF_FROZEN);
        }
        if fd <= 0 {
            return Err(PF_EBADF);
        }
        self.slots.push(PollSlot { fd, live: true, events, revents: 0 });
        Ok(())
    }

    /// 杀条目（标死）。
    pub fn kill(&mut self, fd: i32) {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                self.slots[k].live = false;
            }
            k += 1;
        }
    }

    /// poll 主裁断：EFAULT（指针）→ EINVAL（nfds）→ 条目级 EBADF → 就绪计数。
    pub fn poll(&self, nfds: i32) -> Result<usize, u32> {
        if !self.ptr_valid {
            return Err(PF_EFAULT);
        }
        if nfds < 0 || nfds > NFDS_CAP {
            return Err(PF_EINVAL);
        }
        // nfds 允许小于实际条目数（只看前 nfds 条），但为 0 时零事件。
        if nfds == 0 {
            return Err(PF_NOEVENT);
        }
        let bound = (nfds as usize).min(self.slots.len());
        let mut n = 0usize;
        let mut bad = false;
        let mut k = 0usize;
        while k < bound {
            if !self.slots[k].valid() {
                bad = true;
            } else if self.slots[k].ready() {
                n += 1;
            }
            k += 1;
        }
        if bad {
            return Err(PF_EBADF);
        }
        if n == 0 {
            return Err(PF_NOEVENT);
        }
        Ok(n)
    }

    /// 链式 fold 指纹（序敏感，含指针位与条目掩码）。
    pub fn fold(&self) -> u64 {
        let mut h = PF_TAG;
        h = (h ^ self.ptr_valid as u64).wrapping_mul(0x100000001b3);
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
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
    // 1) 坏指针必报 EFAULT（优先于条数）
    let mut g = PollGate::new();
    let _ = g.push(3, 1);
    g.ptr_valid = false;
    got[0] = g.poll(-1).err() == Some(PF_EFAULT) && g.poll(1).err() == Some(PF_EFAULT);
    // 2) 负条数必报 EINVAL
    let mut n = PollGate::new();
    let _ = n.push(4, 1);
    got[1] = n.poll(-1).err() == Some(PF_EINVAL) && n.poll(-8).err() == Some(PF_EINVAL);
    // 3) 超容条数必报 EINVAL
    got[2] = n.poll(NFDS_CAP + 1).err() == Some(PF_EINVAL);
    // 4) 坏条目混入必报条目级 EBADF
    let mut b = PollGate::new();
    let _ = b.push(5, 1);
    b.kill(5);
    got[3] = b.poll(1).err() == Some(PF_EBADF);
    got
}

/// 条数确定性样本：第 r 轮的 nfds。
pub fn pf_sample_nfds(r: u32) -> i32 {
    let m = r.wrapping_mul(2654435761).wrapping_add(11) % 8;
    (m + 1) as i32
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10904_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10904");

    // J1 正面·EFAULT 10/10
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut g = PollGate::new();
        let _ = g.push(r as i32 + 1, 1);
        g.ptr_valid = false;
        if g.poll(pf_sample_nfds(r)).err() == Some(PF_EFAULT) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EFAULT十连", h1 == 10, "坏指针十轮全数 EFAULT");

    // J2 正面·EINVAL 10/10（负值）
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let g = PollGate::new();
        if g.poll(-(r2 as i32) - 1).err() == Some(PF_EINVAL) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EINVAL负值十连", h2 == 10, "负 nfds 十轮全数 EINVAL");

    // J3 超容 EINVAL
    let j3 = PollGate::new().poll(NFDS_CAP + 1).err() == Some(PF_EINVAL)
        && PollGate::new().poll(i32::MAX).err() == Some(PF_EINVAL);
    s.add("J3-超容EINVAL", j3, "nfds 超过 NFDS_CAP 必拒");

    // J4 反面：合法数组千次零误报
    let mut ok = 0u32;
    let mut r4 = 0u32;
    while r4 < 1000 {
        let mut g = PollGate::new();
        let n = pf_sample_nfds(r4);
        let mut f = 0i32;
        let mut legal = true;
        while f < n {
            if g.push(f + 1, 1).is_err() {
                legal = false;
            }
            f += 1;
        }
        if legal && g.poll(n).is_ok() {
            ok += 1;
        }
        r4 += 1;
    }
    s.add("J4-合法千次零误报", ok == 1000, "合法数组千轮零误报");

    // J5 优先序：EFAULT 先于 EINVAL
    let j5 = {
        let mut g = PollGate::new();
        g.ptr_valid = false;
        g.poll(-5).err() == Some(PF_EFAULT)
    };
    s.add("J5-优先序EFAULT先", j5, "坏指针优先于负条数裁断");

    // J6 nfds=0 显性零事件
    let j6 = {
        let mut g = PollGate::new();
        let _ = g.push(7, 1);
        g.poll(0).err() == Some(PF_NOEVENT)
    };
    s.add("J6-零条数显性", j6, "nfds=0 显性报零事件而非成功");

    // J7 条目级坏 fd 报 EBADF
    let j7 = {
        let mut g = PollGate::new();
        let _ = g.push(8, 1);
        let _ = g.push(9, 1);
        g.kill(9);
        g.poll(2).err() == Some(PF_EBADF)
    };
    s.add("J7-条目坏fd", j7, "混入死条目必报 EBADF");

    // J8 就绪计数与掩码一致
    let j8 = {
        let mut g = PollGate::new();
        let _ = g.push(10, 1);
        let _ = g.push(11, 0);
        let _ = g.push(12, 4);
        g.poll(3) == Ok(2)
    };
    s.add("J8-就绪计数一致", j8, "零掩码条目不计入就绪数");

    // J9 注入四路全抓
    let inj = inject_four();
    s.add("J9-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "坏指针/负条数/超容/坏条目全数命中");

    // J10 条目 fd 非正拒收
    let j10 = {
        let mut g = PollGate::new();
        g.push(0, 1).err() == Some(PF_EBADF) && g.push(-3, 1).err() == Some(PF_EBADF)
    };
    s.add("J10-非正fd拒收", j10, "fd 非正登记必拒");

    // J11 冻结后登记拒
    let j11 = {
        let mut g = PollGate::new();
        g.freeze();
        g.push(13, 1).err() == Some(PF_FROZEN)
    };
    s.add("J11-冻结拒登记", j11, "冻结后登记必拒");

    // J12 fold 稳定：同态一致、改指针位必变
    let j12 = {
        let mut g = PollGate::new();
        let _ = g.push(14, 1);
        let f1 = g.fold();
        let f2 = g.fold();
        g.ptr_valid = false;
        f1 == f2 && f1 != g.fold() && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "同态指纹稳定且异态指纹必变");

    // J13 动态明细：合法千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("efault={} einval={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10904_pollgate_flow() {
        let mut g = PollGate::new();
        assert!(g.push(3, 1).is_ok());
        assert!(g.push(-1, 1).err() == Some(PF_EBADF));
        assert_eq!(g.poll(1), Ok(1));
        assert!(g.poll(-1).err() == Some(PF_EINVAL));
        g.ptr_valid = false;
        assert!(g.poll(1).err() == Some(PF_EFAULT));
        g.ptr_valid = true;
        let _ = g.kill(3);
        assert!(g.poll(1).err() == Some(PF_EBADF));
        g.freeze();
        assert!(g.push(4, 1).err() == Some(PF_FROZEN));
    }

    #[test]
    fn unx10904_checks_green() {
        let s = run_unx10904_checks();
        assert!(s.all_passed(), "UNX-F10904 判据未全绿");
    }
}
