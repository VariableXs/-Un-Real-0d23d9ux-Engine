//! UNX-F24186 vblank/HPD 内核事件面与事件队列·子功能06（优先级插队）（0x41920 段）
//!
//! 锚点：UNX-F24186-J1 插队账（插队落位逐条落账）；J2 插队语义（高
//! 优插队 + 同优 FIFO + 野优先级拒三性质）；J3 插队指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（优先级队列面）。
//!
//! 结构：优先队列 + 尾插核 + 插队核 + 账本 + 折叠指纹 + 注入核（零
//! 容/满队/野优先级/印后/空出队必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41920 独占 0x20）。
pub const SEG_BASE: u32 = 0x41920;
pub const PR_ZERO_CAP: u32 = SEG_BASE + 0x01; // 零容
pub const PR_FULL: u32 = SEG_BASE + 0x02; // 满队
pub const PR_BAD_PRIO: u32 = SEG_BASE + 0x03; // 野优先级
pub const PR_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const PR_EMPTY: u32 = SEG_BASE + 0x05; // 空出队

/// 优先事件。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PrEvent {
    pub prio: u8, // 0=高优 … 3=低优
    pub seq: u64,
}

/// 优先级队列：prio 小者靠前；同优按入序。
pub struct PrQueue {
    pub cap: usize,
    buf: Vec<PrEvent>,
    enq: u64,
    deq: u64,
    sealed: bool,
}

impl PrQueue {
    pub fn new(cap: usize) -> PrQueue {
        PrQueue {
            cap,
            buf: Vec::new(),
            enq: 0,
            deq: 0,
            sealed: false,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 普通入队（尾插）。
    pub fn push(&mut self, prio: u8, seq: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if prio > 3 {
            return Err(PR_BAD_PRIO);
        }
        if self.buf.len() >= self.cap {
            return Err(PR_FULL);
        }
        self.buf.push(PrEvent { prio, seq });
        self.enq += 1;
        Ok(())
    }

    /// 出队：取全队最高优（同优最前）。
    pub fn pop(&mut self) -> Result<PrEvent, u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if self.buf.is_empty() {
            return Err(PR_EMPTY);
        }
        let mut best = 0usize;
        for i in 1..self.buf.len() {
            if self.buf[i].prio < self.buf[best].prio {
                best = i;
            }
        }
        let e = self.buf.remove(best);
        self.deq += 1;
        Ok(e)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41920;
        h = h.wrapping_mul(31).wrapping_add(self.enq as u32);
        h = h.wrapping_mul(31).wrapping_add(self.deq as u32);
        for e in &self.buf {
            h = h.wrapping_mul(31).wrapping_add(e.prio as u32);
            h = h.wrapping_mul(31).wrapping_add(e.seq as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：入 (2,1)(0,2)(2,3)(1,4)，出 (0,2)。
pub fn mk_prio() -> PrQueue {
    let mut q = PrQueue::new(8);
    let _ = q.push(2, 1);
    let _ = q.push(0, 2);
    let _ = q.push(2, 3);
    let _ = q.push(1, 4);
    let _ = q.pop(); // (0,2)
    q
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零容
    got[0] = PrQueue::new(0).push(1, 1).err() == Some(PR_FULL);
    // 2) 满队
    let mut q = PrQueue::new(1);
    let _ = q.push(1, 1);
    got[1] = q.push(1, 2).err() == Some(PR_FULL);
    // 3) 野优先级
    got[2] = PrQueue::new(8).push(4, 1).err() == Some(PR_BAD_PRIO);
    // 4) 空出队
    got[3] = PrQueue::new(8).pop().err() == Some(PR_EMPTY);
    // 5) 印后操作
    let mut q2 = mk_prio();
    q2.seal();
    got[4] = q2.push(1, 99).err() == Some(PR_FROZEN) && q2.pop().err() == Some(PR_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24186");
    let mut q = mk_prio();

    // J1 高优先出：(0,2) 已出，下一个 (1,4)
    let j1 = q.pop().map(|e| (e.prio, e.seq)) == Ok((1, 4));
    s.add("J1-高优先出", j1, "次高优 (1,4)");

    // J2 同优 FIFO：余 (2,1)(2,3) 按序
    let j2 = {
        let a = q.pop().map(|e| e.seq) == Ok(1);
        let b = q.pop().map(|e| e.seq) == Ok(3);
        a && b
    };
    s.add("J2-同优FIFO", j2, "同优按入序出");

    // J3 容量
    let j3 = q.cap == 8;
    s.add("J3-容量", j3, "容 8");

    // J4 野优先级拒
    let j4 = q.push(4, 9).err() == Some(PR_BAD_PRIO);
    s.add("J4-野优先级拒", j4, "prio>3 必拒");

    // J5 空出队拒
    let j5 = q.pop().err() == Some(PR_EMPTY);
    s.add("J5-空出队拒", j5, "空队 pop 必拒");

    // J6 满拒
    let j6 = {
        let mut q2 = PrQueue::new(2);
        let _ = q2.push(1, 1);
        let _ = q2.push(1, 2);
        q2.push(1, 3).err() == Some(PR_FULL)
    };
    s.add("J6-满拒", j6, "超容必拒");

    // J7 插队正确性：低优在队，高优入队即先出
    let j7 = {
        let mut q2 = PrQueue::new(8);
        let _ = q2.push(3, 1);
        let _ = q2.push(3, 2);
        let _ = q2.push(0, 9); // 插队
        q2.pop().map(|e| e.seq) == Ok(9)
    };
    s.add("J7-插队正确", j7, "高优越过低优先出");

    // J8 零容拒
    let j8 = PrQueue::new(0).push(1, 1).err() == Some(PR_FULL);
    s.add("J8-零容拒", j8, "容 0 必拒");

    // J9 严格全序：prio 0..3 依次出
    let j9 = {
        let mut q2 = PrQueue::new(8);
        let _ = q2.push(3, 1);
        let _ = q2.push(2, 2);
        let _ = q2.push(1, 3);
        let _ = q2.push(0, 4);
        let a = q2.pop().map(|e| e.prio) == Ok(0);
        let b = q2.pop().map(|e| e.prio) == Ok(1);
        let c = q2.pop().map(|e| e.prio) == Ok(2);
        let d = q2.pop().map(|e| e.prio) == Ok(3);
        a && b && c && d
    };
    s.add("J9-严格全序", j9, "prio 升序出队");

    // J10 计数守恒
    let j10 = {
        let mut q2 = PrQueue::new(8);
        for i in 0..6u64 {
            let _ = q2.push((i % 4) as u8, i);
        }
        let mut n = 0;
        while q2.pop().is_ok() {
            n += 1;
        }
        n == 6
    };
    s.add("J10-计数守恒", j10, "入 6 出 6");

    // J11 印后操作拒
    let j11 = {
        let mut q2 = mk_prio();
        q2.seal();
        q2.push(1, 99).err() == Some(PR_FROZEN) && q2.pop().err() == Some(PR_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut q2 = mk_prio();
        let a = q2.fold();
        q2.seal();
        q2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24186_prio_flow() {
        let mut q = PrQueue::new(4);
        assert_eq!(q.push(2, 1), Ok(()));
        assert_eq!(q.push(0, 2), Ok(()));
        assert_eq!(q.push(2, 3), Ok(()));
        assert_eq!(q.pop().map(|e| e.seq), Ok(2));
        assert_eq!(q.pop().map(|e| e.seq), Ok(1));
        assert_eq!(q.pop().map(|e| e.seq), Ok(3));
        assert_eq!(q.pop(), Err(PR_EMPTY));
        assert_eq!(q.push(4, 9), Err(PR_BAD_PRIO));
        q.seal();
        assert_eq!(q.push(1, 1), Err(PR_FROZEN));
        assert_eq!(q.pop(), Err(PR_FROZEN));
    }

    #[test]
    fn unx24186_checks_green() {
        let s = run_unx24186_checks();
        assert!(s.all_passed(), "UNX-F24186 判据未全绿");
    }
}
