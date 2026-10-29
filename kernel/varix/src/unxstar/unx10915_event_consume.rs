//! UNX-F10915 事件消费语义矩阵（wait 后事件归属）（0x1d1e0 段）
//!
//! 锚点：UNX-F10915-J1 正面：wait 返回事件即消费单点（10/10）、LT
//! 未消费条件仍在重报（10/10）；反面：双消费零发生（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·事件消费语义面）。
//!
//! 结构：事件账（ET 一次性消费位 / LT 条件保持位双模）+ 裁断器
//! （ET wait 后清位、LT wait 后保持直到条件消失）+ 注入核（双消费/
//! ET 复报/LT 过期重报/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d1e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d1e0;
pub const CS_DOUBLE: u32 = SEG_BASE + 0x01; // ET 双消费
pub const CS_BADFD: u32 = SEG_BASE + 0x02; // fd 未登记
pub const CS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改账
pub const CS_NOEVENT: u32 = SEG_BASE + 0x04; // 全无事件
pub const CS_DUP: u32 = SEG_BASE + 0x05; // 重号登记

/// 触发模式。
pub const MODE_ET: u8 = 1;
pub const MODE_LT: u8 = 2;

/// 事件账定容。
pub const CAP: usize = 24;
/// CS 域标（fold 种子）。
pub const CS_TAG: u64 = 0x1d1e20f;

/// 事件账条目。
#[derive(Clone, Debug)]
pub struct ConsumeSlot {
    pub fd: i32,
    pub mode: u8,
    /// ET：事件就绪位（wait 后清）；LT：条件保持位。
    pub ready: bool,
    pub wait_hits: u32,
}

impl ConsumeSlot {
    /// ET 语义：wait 后即消费。
    pub fn is_et(&self) -> bool {
        self.mode == MODE_ET
    }
}

/// 事件消费账。
pub struct ConsumeLedger {
    pub slots: Vec<ConsumeSlot>,
    pub frozen: bool,
}

impl ConsumeLedger {
    pub fn new() -> ConsumeLedger {
        ConsumeLedger { slots: Vec::new(), frozen: false }
    }

    /// 登记。
    pub fn enroll(&mut self, fd: i32, mode: u8, ready: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(CS_FROZEN);
        }
        if mode != MODE_ET && mode != MODE_LT {
            return Err(CS_BADFD);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(CS_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(CS_BADFD);
        }
        self.slots.push(ConsumeSlot { fd, mode, ready, wait_hits: 0 });
        Ok(())
    }

    /// 供给事件（置就绪/条件位）。
    pub fn supply(&mut self, fd: i32) -> Result<(), u32> {
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(CS_BADFD);
        }
        self.slots[idx].ready = true;
        Ok(())
    }

    /// 撤条件（LT 语义下条件消失即停报）。
    pub fn drain(&mut self, fd: i32) -> Result<(), u32> {
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(CS_BADFD);
        }
        self.slots[idx].ready = false;
        Ok(())
    }

    /// wait 主裁断：ET 返回即消费（清位）；LT 条件在则重报。
    pub fn wait(&mut self, fd: i32) -> Result<u32, u32> {
        if self.frozen {
            return Err(CS_FROZEN);
        }
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(CS_BADFD);
        }
        let is_et = self.slots[idx].is_et();
        let ready = self.slots[idx].ready;
        if !ready {
            return Err(CS_NOEVENT);
        }
        self.slots[idx].wait_hits += 1;
        if is_et {
            self.slots[idx].ready = false;
        }
        Ok(self.slots[idx].wait_hits)
    }

    /// 双消费审计：ET 条目 wait 后再 wait 必报 NOEVENT。
    pub fn et_no_double(&self, fd: i32) -> bool {
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.fd == fd && e.is_et() && !e.ready {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 链式 fold 指纹（序敏感，含 wait 计数）。
    pub fn fold(&self) -> u64 {
        let mut h = CS_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.mode as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.ready as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.wait_hits as u64).wrapping_mul(0x100000001b3);
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
    // 1) ET 双消费必抓：第一次成功第二次 NOEVENT
    let mut l = ConsumeLedger::new();
    let _ = l.enroll(1, MODE_ET, true);
    let _ = l.wait(1);
    got[0] = l.wait(1).err() == Some(CS_NOEVENT);
    // 2) ET 供给后复报必抓：消费后未再供给不得再报
    // 复报被拒且审计位为“已消费”（et_no_double 真）方算双路命中。
    let mut e = ConsumeLedger::new();
    let _ = e.enroll(2, MODE_ET, true);
    let _ = e.wait(2);
    got[1] = e.wait(2).is_err() && e.et_no_double(2);
    // 3) LT 过期重报必抓：条件撤除后不得再报
    let mut t = ConsumeLedger::new();
    let _ = t.enroll(3, MODE_LT, true);
    let _ = t.wait(3);
    let _ = t.drain(3);
    got[2] = t.wait(3).err() == Some(CS_NOEVENT);
    // 4) 冻结后 wait 必抓
    let mut f = ConsumeLedger::new();
    f.freeze();
    got[3] = f.wait(9).err() == Some(CS_FROZEN);
    got
}

/// 模式确定性样本：第 r 轮的模式。
pub fn cs_sample_mode(r: u32) -> u8 {
    if r.wrapping_mul(2654435761) % 2 == 0 {
        MODE_ET
    } else {
        MODE_LT
    }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10915_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10915");

    // J1 正面·wait 即消费单点十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(r as i32 + 1, MODE_ET, true);
        let _ = l.wait(r as i32 + 1);
        if l.wait(r as i32 + 1).err() == Some(CS_NOEVENT) && l.et_no_double(r as i32 + 1) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-ET消费十连", h1 == 10, "ET wait 后即消费十轮全成立");

    // J2 正面·LT 未消费条件仍在重报十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(r2 as i32 + 1, MODE_LT, true);
        let _ = l.wait(r2 as i32 + 1);
        let _ = l.wait(r2 as i32 + 1);
        if l.wait(r2 as i32 + 1).is_ok() && l.slots[0].ready {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-LT重报十连", h2 == 10, "LT 条件在则连报十轮全成立");

    // J3 反面：双消费零发生千次
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut l = ConsumeLedger::new();
        let mode = cs_sample_mode(r3);
        let _ = l.enroll(r3 as i32 % 7 + 1, mode, true);
        let first = l.wait(r3 as i32 % 7 + 1).is_ok();
        let second = l.wait(r3 as i32 % 7 + 1);
        let legal = if mode == MODE_ET {
            first && second.is_err()
        } else {
            first && second.is_ok()
        };
        if legal {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-千次零双消费", ok == 1000, "千轮 ET/LT 消费语义全对");

    // J4 注入四路全抓
    let inj = inject_four();
    s.add("J4-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "双消费/复报/过期/冻结全数命中");

    // J5 ET 再供给后可再消费
    let j5 = {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(11, MODE_ET, true);
        let _ = l.wait(11);
        let _ = l.supply(11);
        l.wait(11).is_ok()
    };
    s.add("J5-再供再消费", j5, "ET 再供给后可再 wait");

    // J6 LT 撤条件后停报
    let j6 = {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(12, MODE_LT, true);
        let _ = l.drain(12);
        l.wait(12).err() == Some(CS_NOEVENT)
    };
    s.add("J6-LT撤条件停报", j6, "LT 条件撤除后必停报");

    // J7 wait 计数逐次累积
    let j7 = {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(13, MODE_LT, true);
        let _ = l.wait(13);
        let _ = l.wait(13);
        l.wait(13) == Ok(3)
    };
    s.add("J7-wait计数累积", j7, "LT 三次 wait 计数为三");

    // J8 坏 fd 拒
    let j8 = ConsumeLedger::new().wait(99).err() == Some(CS_BADFD);
    s.add("J8-坏fd拒", j8, "未登记 fd wait 必拒");

    // J9 重号登记拒
    let j9 = {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(14, MODE_ET, true);
        l.enroll(14, MODE_LT, true).err() == Some(CS_DUP)
    };
    s.add("J9-重号拒收", j9, "同 fd 重复登记必拒");

    // J10 非法模式拒
    let j10 = ConsumeLedger::new().enroll(15, 9, true).err() == Some(CS_BADFD);
    s.add("J10-模式非法拒", j10, "非法触发模式必拒");

    // J11 冻结后 wait/登记双拒
    let j11 = {
        let mut l = ConsumeLedger::new();
        l.freeze();
        l.enroll(16, MODE_ET, true).err() == Some(CS_FROZEN)
            && l.wait(16).err() == Some(CS_FROZEN)
    };
    s.add("J11-冻结双拒", j11, "冻结后登记与 wait 均必拒");

    // J12 fold 稳定：wait 前后指纹必变
    let j12 = {
        let mut l = ConsumeLedger::new();
        let _ = l.enroll(17, MODE_ET, true);
        let f0 = l.fold();
        let _ = l.wait(17);
        let f1 = l.fold();
        let f2 = l.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹随消费变", j12, "消费改变指纹且同态稳定");

    // J13 动态明细：千次合法计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("et={} lt={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10915_consumeledger_flow() {
        let mut l = ConsumeLedger::new();
        assert!(l.enroll(1, MODE_ET, true).is_ok());
        assert!(l.enroll(1, MODE_LT, true).err() == Some(CS_DUP));
        assert!(l.wait(1).is_ok());
        assert!(l.wait(1).err() == Some(CS_NOEVENT));
        let _ = l.supply(1);
        assert!(l.wait(1).is_ok());
        assert!(l.enroll(2, MODE_LT, true).is_ok());
        assert!(l.wait(2).is_ok());
        assert!(l.wait(2).is_ok());
        let _ = l.drain(2);
        assert!(l.wait(2).err() == Some(CS_NOEVENT));
        l.freeze();
        assert!(l.wait(1).err() == Some(CS_FROZEN));
    }

    #[test]
    fn unx10915_checks_green() {
        let s = run_unx10915_checks();
        assert!(s.all_passed(), "UNX-F10915 判据未全绿");
    }
}
