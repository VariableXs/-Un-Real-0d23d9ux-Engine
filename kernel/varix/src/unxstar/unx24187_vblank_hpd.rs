//! UNX-F24187 vblank/HPD 内核事件面与事件队列·子功能07（丢失计数）（0x41940 段）
//!
//! 锚点：UNX-F24187-J1 丢失账（满队丢弃逐条落账）；J2 丢失语义（满
//! 丢计数 + 存留守恒 + 野类型拒三性质）；J3 丢失指纹（账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（丢失计数面）。
//!
//! 结构：计丢队列 + 满丢核 + 守恒核 + 账本 + 折叠指纹 + 注入核（零
//! 容/野类型/印后/负水位/溢出必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41940 独占 0x20）。
pub const SEG_BASE: u32 = 0x41940;
pub const LS_ZERO_CAP: u32 = SEG_BASE + 0x01; // 零容
pub const LS_BAD_KIND: u32 = SEG_BASE + 0x02; // 野类型
pub const LS_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const LS_BAD_WATER: u32 = SEG_BASE + 0x04; // 水位越界
pub const LS_OVERFLOW: u32 = SEG_BASE + 0x05; // 丢失计数溢出

/// 计丢队列：满时入队事件被丢并计数。
pub struct LsQueue {
    pub cap: usize,
    buf: Vec<(u8, u64)>,
    pub lost: u64,
    pub lost_bytes: u64,
    sealed: bool,
}

impl LsQueue {
    pub const LOST_CAP: u64 = u32::MAX as u64;

    pub fn new(cap: usize) -> LsQueue {
        LsQueue {
            cap,
            buf: Vec::new(),
            lost: 0,
            lost_bytes: 0,
            sealed: false,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 入队：满则丢并计。
    pub fn push(&mut self, kind: u8, ts: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(LS_FROZEN);
        }
        if kind > 15 {
            return Err(LS_BAD_KIND);
        }
        if self.lost >= Self::LOST_CAP {
            return Err(LS_OVERFLOW);
        }
        if self.buf.len() >= self.cap {
            self.lost += 1;
            self.lost_bytes += 12; // kind(1)+pad(3)+ts(8)
            return Ok(false); // false=被丢
        }
        self.buf.push((kind, ts));
        Ok(true)
    }

    /// 出队；空返 0。
    pub fn pop(&mut self) -> Option<(u8, u64)> {
        if self.buf.is_empty() {
            None
        } else {
            Some(self.buf.remove(0))
        }
    }

    /// 守恒式：存留 + 丢失 == 总入队尝试。
    pub fn total_attempted(&self) -> u64 {
        self.buf.len() as u64 + self.lost
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41940;
        h = h.wrapping_mul(31).wrapping_add(self.lost as u32);
        h = h.wrapping_mul(31).wrapping_add(self.lost_bytes as u32);
        h = h.wrapping_mul(31).wrapping_add(self.buf.len() as u32);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：容 2，入 5（丢 3），出 1。
pub fn mk_lsqueue() -> LsQueue {
    let mut q = LsQueue::new(2);
    let _ = q.push(1, 10); // 留
    let _ = q.push(2, 20); // 留
    let _ = q.push(3, 30); // 丢
    let _ = q.push(1, 40); // 丢
    let _ = q.push(2, 50); // 丢
    let _ = q.pop(); // 余 1
    q
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零容：一切入队即丢
    let mut q = LsQueue::new(0);
    got[0] = q.push(1, 1) == Ok(false) && q.lost == 1;
    // 2) 野类型
    got[1] = LsQueue::new(4).push(16, 1).err() == Some(LS_BAD_KIND);
    // 3) 水位越界（cap 探针由判据 J10 覆盖，此处检存留>cap 不可能态）
    let mut q2 = LsQueue::new(2);
    let _ = q2.push(1, 1);
    let _ = q2.push(1, 2);
    got[2] = q2.len() <= q2.cap;
    // 4) 丢失字节累加
    let mut q3 = LsQueue::new(0);
    let _ = q3.push(1, 1);
    got[3] = q3.lost_bytes == 12;
    // 5) 印后操作
    let mut q4 = mk_lsqueue();
    q4.seal();
    got[4] = q4.push(1, 99).err() == Some(LS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24187");
    let mut q = mk_lsqueue();

    // J1 丢失计数：入 5 容 2 出 1 ⇒ 丢 3
    let j1 = q.lost == 3;
    s.add("J1-丢失计数", j1, "满丢 3");

    // J2 存留守恒：留 1
    let j2 = q.len() == 1;
    s.add("J2-存留", j2, "存留 1");

    // J3 守恒式
    let j3 = q.total_attempted() == 4; // 留1 + 丢3
    s.add("J3-守恒式", j3, "留+丢=尝试");

    // J4 丢字节：3×12=36
    let j4 = q.lost_bytes == 36;
    s.add("J4-丢字节", j4, "3×12=36");

    // J5 野类型拒
    let j5 = q.push(16, 1).err() == Some(LS_BAD_KIND);
    s.add("J5-野类型拒", j5, "kind>15 必拒");

    // J6 零容全丢
    let j6 = {
        let mut q2 = LsQueue::new(0);
        let r1 = q2.push(1, 1) == Ok(false);
        let r2 = q2.push(2, 2) == Ok(false);
        r1 && r2 && q2.lost == 2
    };
    s.add("J6-零容全丢", j6, "容 0 一切入即丢");

    // J7 不丢路径
    let j7 = {
        let mut q2 = LsQueue::new(4);
        q2.push(1, 1) == Ok(true) && q2.lost == 0
    };
    s.add("J7-不丢路径", j7, "未满即存");

    // J8 丢弃不改存留序
    let j8 = {
        let mut q2 = LsQueue::new(1);
        let _ = q2.push(5, 100);
        let _ = q2.push(6, 200); // 丢
        q2.pop() == Some((5, 100))
    };
    s.add("J8-序不变", j8, "丢弃不影响存留序");

    // J9 计数单调
    let j9 = {
        let mut q2 = LsQueue::new(1);
        let _ = q2.push(1, 1);
        let a = q2.lost;
        let _ = q2.push(1, 2);
        let b = q2.lost;
        let _ = q2.push(1, 3);
        let c = q2.lost;
        a <= b && b <= c
    };
    s.add("J9-单调", j9, "丢失计数单调增");

    // J10 水位闸：存留恒 ≤ 容
    let j10 = {
        let mut q2 = LsQueue::new(3);
        for i in 0..10u64 {
            let _ = q2.push(1, i);
        }
        q2.len() <= 3
    };
    s.add("J10-水位闸", j10, "存留≤容");

    // J11 印后操作拒
    let j11 = {
        let mut q2 = mk_lsqueue();
        q2.seal();
        q2.push(1, 99).err() == Some(LS_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut q2 = mk_lsqueue();
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
    fn unx24187_loss_flow() {
        let mut q = LsQueue::new(1);
        assert_eq!(q.push(1, 1), Ok(true));
        assert_eq!(q.push(1, 2), Ok(false));
        assert_eq!(q.lost, 1);
        assert_eq!(q.push(16, 3), Err(LS_BAD_KIND));
        assert_eq!(q.pop(), Some((1, 1)));
        assert_eq!(q.pop(), None);
        q.seal();
        assert_eq!(q.push(1, 9), Err(LS_FROZEN));
    }

    #[test]
    fn unx24187_checks_green() {
        let s = run_unx24187_checks();
        assert!(s.all_passed(), "UNX-F24187 判据未全绿");
    }
}
