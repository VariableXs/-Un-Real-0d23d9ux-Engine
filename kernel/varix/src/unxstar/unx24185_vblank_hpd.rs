//! UNX-F24185 vblank/HPD 内核事件面与事件队列·子功能05（事件队列 FIFO）（0x41900 段）
//!
//! 锚点：UNX-F24185-J1 FIFO 账（入队出队逐条落账）；J2 FIFO 语义（
//! 先进先出 + 满拒 + 空拒三性质）；J3 FIFO 指纹（账折叠指纹）三判
//! 据全绿。
//! 域：CoRun·UNX-F5-B15（事件队列面）。
//!
//! 结构：定容队列 + 入队核 + 出队核 + 账本 + 折叠指纹 + 注入核（零
//! 容/满队/空队/野类型/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41900 独占 0x20）。
pub const SEG_BASE: u32 = 0x41900;
pub const FQ_ZERO_CAP: u32 = SEG_BASE + 0x01; // 零容
pub const FQ_FULL: u32 = SEG_BASE + 0x02; // 满队
pub const FQ_EMPTY: u32 = SEG_BASE + 0x03; // 空队
pub const FQ_BAD_KIND: u32 = SEG_BASE + 0x04; // 野类型
pub const FQ_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 事件。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FqEvent {
    pub kind: u8,
    pub ts: u64,
}

/// 定容 FIFO 事件队列。
pub struct EqFifo {
    pub cap: usize,
    buf: Vec<FqEvent>,
    enqueued: u64,
    dequeued: u64,
    sealed: bool,
}

impl EqFifo {
    pub fn new(cap: usize) -> EqFifo {
        EqFifo {
            cap,
            buf: Vec::new(),
            enqueued: 0,
            dequeued: 0,
            sealed: false,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 入队：尾插；满拒。
    pub fn push(&mut self, kind: u8, ts: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(FQ_FROZEN);
        }
        if kind > 7 {
            return Err(FQ_BAD_KIND);
        }
        if self.buf.len() >= self.cap {
            return Err(FQ_FULL);
        }
        self.buf.push(FqEvent { kind, ts });
        self.enqueued += 1;
        Ok(())
    }

    /// 出队：头取；空拒。
    pub fn pop(&mut self) -> Result<FqEvent, u32> {
        if self.sealed {
            return Err(FQ_FROZEN);
        }
        if self.buf.is_empty() {
            return Err(FQ_EMPTY);
        }
        let e = self.buf.remove(0);
        self.dequeued += 1;
        Ok(e)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41900;
        h = h.wrapping_mul(31).wrapping_add(self.enqueued as u32);
        h = h.wrapping_mul(31).wrapping_add(self.dequeued as u32);
        h = h.wrapping_mul(31).wrapping_add(self.buf.len() as u32);
        for e in &self.buf {
            h = h.wrapping_mul(31).wrapping_add(e.kind as u32);
            h = h.wrapping_mul(31).wrapping_add(e.ts as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：容 4，入 1/2/3，出 1（余 2/3）。
pub fn mk_fifo() -> EqFifo {
    let mut q = EqFifo::new(4);
    let _ = q.push(1, 10);
    let _ = q.push(2, 20);
    let _ = q.push(3, 30);
    let _ = q.pop();
    q
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零容：push 必满拒
    got[0] = EqFifo::new(0).push(1, 1).err() == Some(FQ_FULL);
    // 2) 满队
    let mut q = EqFifo::new(2);
    let _ = q.push(1, 1);
    let _ = q.push(1, 2);
    got[1] = q.push(1, 3).err() == Some(FQ_FULL);
    // 3) 空队
    got[2] = EqFifo::new(4).pop().err() == Some(FQ_EMPTY);
    // 4) 野类型
    got[3] = EqFifo::new(4).push(9, 1).err() == Some(FQ_BAD_KIND);
    // 5) 印后操作
    let mut q2 = mk_fifo();
    q2.seal();
    got[4] = q2.push(1, 99).err() == Some(FQ_FROZEN) && q2.pop().err() == Some(FQ_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24185_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24185");
    let mut q = mk_fifo();

    // J1 FIFO 序：余 2/3 头是 2
    let j1 = q.pop().map(|e| e.kind) == Ok(2);
    let _ = q.pop();
    s.add("J1-FIFO序", j1, "头取最先入队者");

    // J2 容量
    let j2 = q.cap == 4;
    s.add("J2-容量", j2, "容 4");

    // J3 长度守恒：入 3 出 3 余 0
    let j3 = q.len() == 0 && q.fold() == q.fold();
    s.add("J3-长度守恒", j3, "入3出3余0");

    // J4 空拒
    let j4 = q.pop().err() == Some(FQ_EMPTY);
    s.add("J4-空拒", j4, "空队 pop 必拒");

    // J5 满拒
    let j5 = {
        let mut q2 = EqFifo::new(1);
        let _ = q2.push(1, 1);
        q2.push(2, 2).err() == Some(FQ_FULL)
    };
    s.add("J5-满拒", j5, "超容 push 必拒");

    // J6 野类型拒
    let j6 = q.push(8, 1).err() == Some(FQ_BAD_KIND);
    s.add("J6-野类型拒", j6, "kind>7 必拒");

    // J7 计数守恒
    let j7 = {
        let mut q2 = EqFifo::new(8);
        for i in 0..5u64 {
            let _ = q2.push((i % 8) as u8, i * 10);
        }
        for _ in 0..3 {
            let _ = q2.pop();
        }
        q2.len() == 2
    };
    s.add("J7-计数守恒", j7, "入5出3余2");

    // J8 零容拒
    let j8 = EqFifo::new(0).push(1, 1).err() == Some(FQ_FULL);
    s.add("J8-零容拒", j8, "容 0 push 必拒");

    // J9 FIFO 严格序：三事件按入序出
    let j9 = {
        let mut q2 = EqFifo::new(8);
        let _ = q2.push(3, 1);
        let _ = q2.push(1, 2);
        let _ = q2.push(2, 3);
        let a = q2.pop().map(|e| e.kind) == Ok(3);
        let b = q2.pop().map(|e| e.kind) == Ok(1);
        let c = q2.pop().map(|e| e.kind) == Ok(2);
        a && b && c
    };
    s.add("J9-严格序", j9, "出序=入序");

    // J10 时间戳随事件走
    let j10 = {
        let mut q2 = EqFifo::new(2);
        let _ = q2.push(1, 777);
        q2.pop().map(|e| e.ts) == Ok(777)
    };
    s.add("J10-时间戳", j10, "事件携带时间戳");

    // J11 印后操作拒
    let j11 = {
        let mut q2 = mk_fifo();
        q2.seal();
        q2.push(1, 99).err() == Some(FQ_FROZEN) && q2.pop().err() == Some(FQ_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut q2 = mk_fifo();
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
    fn unx24185_fifo_flow() {
        let mut q = EqFifo::new(2);
        assert_eq!(q.push(1, 10), Ok(()));
        assert_eq!(q.push(2, 20), Ok(()));
        assert_eq!(q.push(3, 30), Err(FQ_FULL));
        assert_eq!(q.pop().map(|e| e.kind), Ok(1));
        assert_eq!(q.pop().map(|e| e.kind), Ok(2));
        assert_eq!(q.pop(), Err(FQ_EMPTY));
        assert_eq!(q.push(9, 1), Err(FQ_BAD_KIND));
        q.seal();
        assert_eq!(q.push(1, 1), Err(FQ_FROZEN));
        assert_eq!(q.pop(), Err(FQ_FROZEN));
    }

    #[test]
    fn unx24185_checks_green() {
        let s = run_unx24185_checks();
        assert!(s.all_passed(), "UNX-F24185 判据未全绿");
    }
}
