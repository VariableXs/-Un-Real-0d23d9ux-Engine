//! UNX-F11228 set_tid_address 与退出清理联签（0x1f828 段）
//!
//! 锚点：UNX-F11228-J1 set_tid_address 登记→线程退出清 tid→futex 唤醒
//! joiner 链路判据绿，与 AI-14 联签零缺项。
//! 域：CoRun·UNX-C5-B02——登记时记住 clear_child_tid 地址；退出必清字、必
//! 唤醒等待者；清了不醒或醒了不清都算链路断裂。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f828;
pub const TA_REG: u32 = SEG_BASE + 0x01; // 登记地址为零
pub const TA_DUP: u32 = SEG_BASE + 0x02; // 线程号或地址重录
pub const TA_EXIT: u32 = SEG_BASE + 0x03; // 未退出即清算
pub const TA_JOIN: u32 = SEG_BASE + 0x04; // 目标未退即等
pub const TA_WAKE: u32 = SEG_BASE + 0x05; // 清字未唤醒
pub const TA_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const TA_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 线程数与字宽。
pub const WANT: usize = 3;
pub const CAP: usize = 8;
pub const TA_TAG: u64 = 0x1f828f1;
pub const FP_STEP: u64 = 0x100000001b3;

/// 一条待清算线程。
#[derive(Clone, Debug)]
pub struct Thread {
    pub id: u32,
    pub clear_tid: u64,
    pub exited: bool,
    pub tid_zeroed: bool,
    pub woken: u32,
}

impl Thread {
    /// 清字与唤醒两件事缺一即链路未闭合。
    pub fn closed(&self) -> bool {
        self.exited && self.tid_zeroed && self.woken > 0
    }
}

/// tid 登记与 joiner 链路账。
#[derive(Clone, Debug)]
pub struct TidLink {
    pub threads: Vec<Thread>,
    pub joins: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TidLink {
    pub fn new() -> TidLink {
        TidLink { threads: Vec::new(), joins: 0, frozen: false, root: None }
    }

    /// 登记：地址零、线程号重、地址重、超容四路拒。
    pub fn register(&mut self, id: u32, clear_tid: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(TA_FROZEN);
        }
        if id == 0 || clear_tid == 0 {
            return Err(TA_REG);
        }
        if self.threads.iter().any(|t| t.id == id || t.clear_tid == clear_tid) {
            return Err(TA_DUP);
        }
        if self.threads.len() >= CAP {
            return Err(TA_OVER);
        }
        self.threads.push(Thread { id, clear_tid, exited: false, tid_zeroed: false, woken: 0 });
        Ok(())
    }

    /// 退出：写零 clear_child_tid 并唤醒 joiner。
    pub fn exit_thread(&mut self, id: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(TA_FROZEN);
        }
        let mut k = 0usize;
        while k < self.threads.len() {
            let t = &mut self.threads[k];
            if t.id == id {
                if t.exited {
                    return Err(TA_EXIT);
                }
                t.exited = true;
                t.tid_zeroed = true;
                t.woken = t.woken.saturating_add(1);
                self.joins = self.joins.saturating_add(1);
                return Ok(t.woken);
            }
            k += 1;
        }
        Err(TA_EXIT)
    }

    /// joiner 等待：目标未退出则拒。
    pub fn join(&mut self, id: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(TA_FROZEN);
        }
        let mut k = 0usize;
        while k < self.threads.len() {
            if self.threads[k].id == id {
                if !self.threads[k].exited {
                    return Err(TA_JOIN);
                }
                self.threads[k].woken = self.threads[k].woken.saturating_add(1);
                self.joins = self.joins.saturating_add(1);
                return Ok(self.threads[k].woken);
            }
            k += 1;
        }
        Err(TA_JOIN)
    }

    pub fn all_closed(&self) -> bool {
        self.threads.len() == WANT && self.threads.iter().all(|t| t.closed())
    }

    pub fn zeroed(&self) -> usize {
        self.threads.iter().filter(|t| t.tid_zeroed).count()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(TA_FROZEN);
        }
        if self.threads.len() != WANT {
            return Err(TA_EXIT);
        }
        if self.zeroed() != WANT {
            return Err(TA_WAKE);
        }
        if !self.all_closed() {
            return Err(TA_JOIN);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("tid {}/{} joins {}", self.zeroed(), self.threads.len(), self.joins)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = TA_TAG;
        let mut k = 0usize;
        while k < self.threads.len() {
            h = (h ^ self.threads[k].id as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.threads[k].clear_tid).wrapping_mul(FP_STEP);
            h = (h ^ self.threads[k].woken as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.threads[k].tid_zeroed as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h = (h ^ self.joins as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 三个 clear_child_tid 地址（十六字节对齐）。
pub const ADDRS: [u64; 3] = [0x7f20_0000_0000, 0x7f20_0000_0010, 0x7f20_0000_0020];

/// 组一张登记→退出→join 全闭合的链路账。
pub fn build() -> TidLink {
    let mut l = TidLink::new();
    let mut k = 0usize;
    while k < WANT {
        let _ = l.register(k as u32 + 1, ADDRS[k]);
        k += 1;
    }
    k = 0;
    while k < WANT {
        let _ = l.exit_thread(k as u32 + 1);
        let _ = l.join(k as u32 + 1);
        k += 1;
    }
    l
}

/// 四路对抗：零地址、重号、未退先等、未清即冻存。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = TidLink::new();
    out[0] = a.register(1, 0) == Err(TA_REG);
    let _ = a.register(1, ADDRS[0]);
    out[1] = a.register(1, ADDRS[1]) == Err(TA_DUP);
    let mut b = TidLink::new();
    let _ = b.register(1, ADDRS[0]);
    out[2] = b.join(1) == Err(TA_JOIN);
    let mut c = TidLink::new();
    let mut k = 0usize;
    while k < WANT {
        let _ = c.register(k as u32 + 1, ADDRS[k]);
        k += 1;
    }
    out[3] = c.freeze() == Err(TA_WAKE);
    out
}

/// 判据入口。
pub fn run_unx11228_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11228");
    let mut l = build();
    let root = l.freeze().unwrap_or(0);

    s.add("J1-三线程登记", l.threads.len() == WANT, "三条 clear_child_tid 登记在册");
    s.add("J2-地址唯一", {
        let mut uniq: Vec<u64> = Vec::new();
        for t in l.threads.iter() {
            if !uniq.contains(&t.clear_tid) {
                uniq.push(t.clear_tid);
            }
        }
        uniq.len() == WANT
    }, "三地址两两不同");
    s.add("J3-退出清零", l.zeroed() == WANT, "三条线程退出后 tid 字全部写零");
    s.add("J4-唤醒 joiner", l.joins >= 3, "每次退出都唤醒至少一个等待者");
    s.add("J5-链路闭合", l.all_closed(), "清字与唤醒成对出现");
    s.add("J6-冻存根", root != 0 && l.root == Some(root), "联签账冻存产出非零根");

    s.add("J7-冻后退拒", l.exit_thread(1) == Err(TA_FROZEN), "冻存后再退出拒为 TA_FROZEN");
    s.add("J8-冻后等拒", l.join(1) == Err(TA_FROZEN), "冻存后再 join 拒为 TA_FROZEN");
    s.add("J9-空地址拒", {
        let mut x = TidLink::new();
        x.register(1, 0) == Err(TA_REG)
    }, "clear_child_tid 为零拒为 TA_REG");
    s.add("J10-重号拒", {
        let mut x = TidLink::new();
        let _ = x.register(1, ADDRS[0]);
        x.register(1, ADDRS[2]) == Err(TA_DUP)
    }, "线程号重录拒为 TA_DUP");
    s.add("J11-未退先等拒", {
        let mut x = TidLink::new();
        let _ = x.register(2, ADDRS[0]);
        x.join(2) == Err(TA_JOIN)
    }, "目标未退出即等拒为 TA_JOIN");

    let four = inject_four();
    s.add("J12-四路注入", four.iter().all(|x| *x), "零地址/重号/未退先等/未清冻存四路全拒");
    s.add_dyn(
        "J13-联签明细",
        l.zeroed() == 3 && l.joins >= 3,
        &format!("清字 {} 条，唤醒 {} 次，与 AI-14 零缺项", l.zeroed(), l.joins),
    );
    s.add_dyn(
        "J14-账根",
        root == l.fold(),
        &format!("根 0x{:x}，首址 0x{:x}", root, l.threads[0].clear_tid),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11228_tid_address_flow() {
        let mut l = build();
        assert!(l.all_closed());
        let r = l.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(l.join(1), Err(TA_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11228_checks_green() {
        let s = run_unx11228_checks();
        assert!(s.all_passed(), "UNX-F11228 判据未全绿");
    }
}
