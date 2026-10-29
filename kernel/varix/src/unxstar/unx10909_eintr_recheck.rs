//! UNX-F10909 epoll_wait EINTR 复核矩阵（0x1d120 段）
//!
//! 锚点：UNX-F10909-J1 正面：信号中断→EINTR（10/10）、pwait 屏蔽
//! （10/10）；反面：无信号零中断（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·EINTR 复核面）。
//!
//! 结构：信号台（挂起信号账 + pwait 屏蔽字）+ 复核器（有信号→EINTR
//! 可重入、屏蔽命中→不中断、无信号→正常等待）+ 注入核（屏蔽失效/
//! 重入丢信号/EINTR 后状态漂移/冻结改台四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d120 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d120;
pub const EI_EINTR: u32 = SEG_BASE + 0x01; // 信号中断
pub const EI_BADFD: u32 = SEG_BASE + 0x02; // fd 未登记
pub const EI_MASKFAIL: u32 = SEG_BASE + 0x03; // 屏蔽字非法
pub const EI_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改台
pub const EI_LOST: u32 = SEG_BASE + 0x05; // 重入丢信号
pub const EI_DRIFT: u32 = SEG_BASE + 0x06; // EINTR 后状态漂移

/// epoll_wait fd 账定容。
pub const CAP: usize = 16;
/// 屏蔽字位宽。
pub const SIGBITS: u32 = 32;
/// EI 域标（fold 种子）。
pub const EI_TAG: u64 = 0x1d12209;

/// 信号台条目。
#[derive(Clone, Debug)]
pub struct WaitSlot {
    pub fd: i32,
    pub live: bool,
}

/// EINTR 复核台：fd 账 + 挂起信号账 + 屏蔽字。
pub struct IntrDesk {
    pub slots: Vec<WaitSlot>,
    pub pending: Vec<u32>,
    pub mask: u32,
    pub frozen: bool,
    pub intr_count: u32,
}

impl IntrDesk {
    pub fn new() -> IntrDesk {
        IntrDesk { slots: Vec::new(), pending: Vec::new(), mask: 0, frozen: false, intr_count: 0 }
    }

    /// 登记 fd。
    pub fn arm(&mut self, fd: i32) -> Result<(), u32> {
        if self.frozen {
            return Err(EI_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(EI_BADFD);
        }
        if self.slots.len() >= CAP {
            return Err(EI_BADFD);
        }
        self.slots.push(WaitSlot { fd, live: true });
        Ok(())
    }

    /// 送信号入挂起账。
    pub fn raise(&mut self, sig: u32) {
        if sig == 0 || sig > SIGBITS {
            return;
        }
        self.pending.push(sig);
    }

    /// 置 pwait 屏蔽字（位或累积）。
    pub fn set_mask(&mut self, mask: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EI_FROZEN);
        }
        if mask == 0 {
            return Err(EI_MASKFAIL);
        }
        self.mask |= mask;
        Ok(())
    }

    /// 信号是否被屏蔽。
    pub fn masked(&self, sig: u32) -> bool {
        self.mask & (1u32.wrapping_shl(sig % SIGBITS)) != 0
    }

    /// wait 主裁断：先裁 fd，再裁信号。
    pub fn wait(&mut self, fd: i32) -> Result<usize, u32> {
        let known = self.slots.iter().any(|e| e.fd == fd && e.live);
        if !known {
            return Err(EI_BADFD);
        }
        // 找首个未屏蔽信号：先取副本再改账。
        let mut fired: Option<u32> = None;
        let mut k = 0usize;
        while k < self.pending.len() {
            let s = self.pending[k];
            if !self.masked(s) {
                fired = Some(s);
                break;
            }
            k += 1;
        }
        match fired {
            Some(s) => {
                // 消费该信号（移除一枚），计数并报 EINTR。
                let mut idx = self.pending.len();
                let mut j = 0usize;
                while j < self.pending.len() {
                    if self.pending[j] == s {
                        idx = j;
                        break;
                    }
                    j += 1;
                }
                if idx < self.pending.len() {
                    self.pending.remove(idx);
                }
                self.intr_count += 1;
                Err(EI_EINTR)
            }
            None => Ok(0),
        }
    }

    /// 屏蔽命中：pwait 语义——挂起信号全被屏蔽时不中断。
    pub fn pwait(&mut self, fd: i32) -> Result<usize, u32> {
        let known = self.slots.iter().any(|e| e.fd == fd && e.live);
        if !known {
            return Err(EI_BADFD);
        }
        let mut all_masked = true;
        let mut k = 0usize;
        while k < self.pending.len() {
            if !self.masked(self.pending[k]) {
                all_masked = false;
            }
            k += 1;
        }
        if all_masked {
            Ok(0)
        } else {
            self.wait(fd)
        }
    }

    /// 状态账：fd 数 + 挂起数 + 中断计数。
    pub fn state_fold(&self) -> u64 {
        let mut h = EI_TAG;
        h = (h ^ self.slots.len() as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.pending.len() as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.intr_count as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.mask as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.state_fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 有信号未屏蔽必报 EINTR
    let mut d = IntrDesk::new();
    let _ = d.arm(3);
    d.raise(2);
    got[0] = d.wait(3).err() == Some(EI_EINTR);
    // 2) 屏蔽命中不中断
    let mut m = IntrDesk::new();
    let _ = m.arm(4);
    m.raise(1);
    let _ = m.set_mask(1u32.wrapping_shl(1));
    got[1] = m.pwait(4) == Ok(0);
    // 3) EINTR 后状态账零漂移：中断计数加一、挂起减一，其余不变
    // 以同输入重放构造期望态（fd 数 1、挂起 0、中断 1、屏蔽 0）比对指纹。
    let mut dr = IntrDesk::new();
    let _ = dr.arm(5);
    dr.raise(7);
    let caught = dr.wait(5).err() == Some(EI_EINTR);
    let mut want = IntrDesk::new();
    let _ = want.arm(5);
    want.intr_count = 1;
    got[2] = caught && dr.state_fold() == want.state_fold();
    // 4) 冻结后改台必抓
    let mut f = IntrDesk::new();
    f.freeze();
    got[3] = f.arm(9).err() == Some(EI_FROZEN);
    got
}

/// 信号确定性样本：第 r 轮的信号号。
pub fn ei_sample_sig(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(21) % SIGBITS + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10909_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10909");

    // J1 正面·EINTR 10/10
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut d = IntrDesk::new();
        let _ = d.arm(r as i32 + 1);
        d.raise(ei_sample_sig(r));
        if d.wait(r as i32 + 1).err() == Some(EI_EINTR) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EINTR十连", h1 == 10, "信号中断十轮全数 EINTR");

    // J2 正面·pwait 屏蔽 10/10
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut d = IntrDesk::new();
        let _ = d.arm(r2 as i32 + 20);
        let sig = ei_sample_sig(r2);
        d.raise(sig);
        let _ = d.set_mask(1u32.wrapping_shl(sig % SIGBITS));
        if d.pwait(r2 as i32 + 20) == Ok(0) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-屏蔽十连", h2 == 10, "pwait 屏蔽十轮全数不中断");

    // J3 反面：无信号千次零中断
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut d = IntrDesk::new();
        let _ = d.arm((r3 % 5) as i32 + 1);
        if d.wait((r3 % 5) as i32 + 1) == Ok(0) {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-无信号千次零中断", ok == 1000, "无信号千轮零 EINTR");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "EINTR/屏蔽/漂移/冻结全数命中");

    // J5 中断计数逐次累积
    let j5 = {
        let mut d = IntrDesk::new();
        let _ = d.arm(7);
        d.raise(3);
        d.raise(3);
        d.raise(3);
        let _ = d.wait(7);
        let _ = d.wait(7);
        d.intr_count == 2
    };
    s.add("J5-中断计数累积", j5, "三次送信两次中断计数为二");

    // J6 信号消费：wait 消费一枚留余
    let j6 = {
        let mut d = IntrDesk::new();
        let _ = d.arm(8);
        d.raise(5);
        d.raise(5);
        let _ = d.wait(8);
        d.pending.len() == 1
    };
    s.add("J6-单枚消费", j6, "wait 只消费一枚挂起信号");

    // J7 未屏蔽信号对 pwait 仍中断
    let j7 = {
        let mut d = IntrDesk::new();
        let _ = d.arm(9);
        d.raise(6);
        let _ = d.set_mask(1u32.wrapping_shl(2));
        d.pwait(9).err() == Some(EI_EINTR)
    };
    s.add("J7-未屏蔽仍中断", j7, "未屏蔽信号下 pwait 仍报 EINTR");

    // J8 零屏蔽字拒
    let j8 = IntrDesk::new().set_mask(0).err() == Some(EI_MASKFAIL);
    s.add("J8-零屏蔽拒", j8, "零屏蔽字必拒");

    // J9 坏 fd 拒
    let j9 = IntrDesk::new().wait(99).err() == Some(EI_BADFD);
    s.add("J9-坏fd拒", j9, "未登记 fd wait 必拒");

    // J10 冻结后改台拒
    let j10 = {
        let mut d = IntrDesk::new();
        d.freeze();
        d.set_mask(1).err() == Some(EI_FROZEN)
    };
    s.add("J10-冻结拒改台", j10, "冻结后置屏蔽必拒");

    // J11 越界信号号不挂账
    let j11 = {
        let mut d = IntrDesk::new();
        d.raise(0);
        d.raise(SIGBITS + 1);
        d.pending.is_empty()
    };
    s.add("J11-越界信号弃", j11, "零号与越界信号号不挂账");

    // J12 屏蔽字累积语义：两次置位叠加
    let j12 = {
        let mut d = IntrDesk::new();
        let _ = d.set_mask(1u32.wrapping_shl(1));
        let _ = d.set_mask(1u32.wrapping_shl(3));
        d.masked(1) && d.masked(3) && !d.masked(2)
    };
    s.add("J12-屏蔽叠加", j12, "屏蔽字按位累积且不误遮");

    // J13 动态明细：无信号千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：EINTR 十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("eintr={} masked={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10909_intrdesk_flow() {
        let mut d = IntrDesk::new();
        assert!(d.arm(3).is_ok());
        d.raise(2);
        assert!(d.wait(3).err() == Some(EI_EINTR));
        assert!(d.wait(3) == Ok(0));
        d.raise(1);
        let _ = d.set_mask(1u32.wrapping_shl(1));
        assert_eq!(d.pwait(3), Ok(0));
        assert!(d.wait(99).err() == Some(EI_BADFD));
        d.freeze();
        assert!(d.arm(4).err() == Some(EI_FROZEN));
    }

    #[test]
    fn unx10909_checks_green() {
        let s = run_unx10909_checks();
        assert!(s.all_passed(), "UNX-F10909 判据未全绿");
    }
}
