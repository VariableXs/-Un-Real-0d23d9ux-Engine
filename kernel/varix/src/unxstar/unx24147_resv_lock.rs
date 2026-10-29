//! UNX-F24147 reservation 锁与显存互斥纪律·子功能07（等待队列与唤醒序 FIFO）（0x41440 段）
//!
//! 锚点：UNX-F24147-J1 等待账（排队与唤醒逐条落账）；J2 队列语义（
//! FIFO 唤醒 + 插队拒 + 空队免唤三性质）；J3 队列指纹（账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（等待队列唤醒面）。
//!
//! 结构：等待队列 + 入队/唤醒核 + 账本 + 折叠指纹 + 注入核（重复排
//! 队/野队列/跳队唤醒/印后/空队唤醒必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41440 独占 0x20）。
pub const SEG_BASE: u32 = 0x41440;
pub const WQ_DUP_WAIT: u32 = SEG_BASE + 0x01; // 重复排队
pub const WQ_UNK_QUEUE: u32 = SEG_BASE + 0x02; // 野队列
pub const WQ_JUMP: u32 = SEG_BASE + 0x03; // 跳队唤醒
pub const WQ_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const WQ_EMPTY_WAKE: u32 = SEG_BASE + 0x05; // 空队唤醒

/// 等待队列。
#[derive(Clone)]
pub struct WaitQ {
    pub q_id: u32,
    pub waiters: Vec<String>,
    pub woken: Vec<String>,
}

/// 队簿。
pub struct WqBook {
    pub queues: Vec<WaitQ>,
    pub sealed: bool,
}

impl WqBook {
    pub fn new() -> WqBook {
        WqBook { queues: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, q_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(WQ_FROZEN);
        }
        if self.queues.iter().any(|q| q.q_id == q_id) {
            return Err(WQ_UNK_QUEUE);
        }
        self.queues.push(WaitQ { q_id, waiters: Vec::new(), woken: Vec::new() });
        Ok(())
    }

    pub fn wait(&mut self, q_id: u32, who: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(WQ_FROZEN);
        }
        let q = match self.queues.iter_mut().find(|q| q.q_id == q_id) {
            Some(q) => q,
            None => return Err(WQ_UNK_QUEUE),
        };
        if who.is_empty() || q.waiters.contains(&who.to_string()) {
            return Err(WQ_DUP_WAIT);
        }
        q.waiters.push(who.to_string());
        Ok(q.waiters.len() as u32)
    }

    /// 唤醒队首（FIFO）。
    pub fn wake_one(&mut self, q_id: u32) -> Result<String, u32> {
        if self.sealed {
            return Err(WQ_FROZEN);
        }
        let q = match self.queues.iter_mut().find(|q| q.q_id == q_id) {
            Some(q) => q,
            None => return Err(WQ_UNK_QUEUE),
        };
        if q.waiters.is_empty() {
            return Err(WQ_EMPTY_WAKE);
        }
        let head = q.waiters.remove(0);
        q.woken.push(head.clone());
        Ok(head)
    }

    /// 跳队唤醒：指定非队首者唤醒必拒。
    pub fn wake_nth(&mut self, q_id: u32, who: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(WQ_FROZEN);
        }
        let q = match self.queues.iter_mut().find(|q| q.q_id == q_id) {
            Some(q) => q,
            None => return Err(WQ_UNK_QUEUE),
        };
        match q.waiters.first() {
            Some(first) if first == who => self.wake_one(q_id),
            _ => Err(WQ_JUMP),
        }
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41440;
        for q in &self.queues {
            h = h.wrapping_mul(31).wrapping_add(q.q_id);
            for w in &q.waiters {
                for by in w.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(by as u32);
                }
            }
            for w in &q.woken {
                for by in w.bytes() {
                    h = h.wrapping_mul(31).wrapping_add(by.wrapping_mul(3) as u32);
                }
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：一队列三等待者，唤醒一人。
pub fn mk_book() -> WqBook {
    let mut b = WqBook::new();
    let _ = b.declare(1);
    let _ = b.wait(1, "a");
    let _ = b.wait(1, "b");
    let _ = b.wait(1, "c");
    let _ = b.wake_one(1); // 唤 a
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 重复排队
    got[0] = b.wait(1, "b").err() == Some(WQ_DUP_WAIT);
    // 2) 野队列
    got[1] = b.wait(9, "x").err() == Some(WQ_UNK_QUEUE);
    // 3) 跳队唤醒
    got[2] = b.wake_nth(1, "c").err() == Some(WQ_JUMP);
    // 4) 空队唤醒
    let mut b2 = WqBook::new();
    let _ = b2.declare(7);
    got[3] = b2.wake_one(7).err() == Some(WQ_EMPTY_WAKE);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.wake_one(1).err() == Some(WQ_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24147");
    let mut b = mk_book();

    // J1 队列账齐
    let j1 = b.queues.len() == 1 && b.queues[0].waiters.len() == 2;
    s.add("J1-队列账齐", j1, "三入队一唤醒余二");

    // J2 FIFO 唤醒队首
    let j2 = b.queues[0].woken == vec_str(&["a"]);
    s.add("J2-FIFO队首", j2, "先到先唤醒");

    // J3 跳队拒
    let j3 = b.wake_nth(1, "c").err() == Some(WQ_JUMP);
    s.add("J3-跳队拒", j3, "非队首指定唤醒必拒");

    // J4 队首指定唤醒放行
    let j4 = b.wake_nth(1, "b") == Ok("b".to_string());
    s.add("J4-队首指定放行", j4, "指定队首等价 wake_one");

    // J5 重复排队拒
    let j5 = b.wait(1, "c").err() == Some(WQ_DUP_WAIT);
    s.add("J5-重复排队拒", j5, "同者二队必拒");

    // J6 野队列拒
    let j6 = b.wait(9, "x").err() == Some(WQ_UNK_QUEUE);
    s.add("J6-野队列拒", j6, "未声明队列必拒");

    // J7 空队唤醒拒
    let j7 = {
        let mut b2 = WqBook::new();
        let _ = b2.declare(7);
        b2.wake_one(7).err() == Some(WQ_EMPTY_WAKE)
    };
    s.add("J7-空队唤醒拒", j7, "无人可唤必拒");

    // J8 唤空续唤拒
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.wake_one(1);
        let _ = b2.wake_one(1);
        let _ = b2.wake_one(1);
        b2.wake_one(1).err() == Some(WQ_EMPTY_WAKE)
    };
    s.add("J8-唤空续唤拒", j8, "三唤后队空必拒");

    // J9 唤醒序累积
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.wake_one(1);
        let _ = b2.wake_one(1);
        let w = b2.queues[0].woken.clone();
        w.len() == 3 && w[0] == "a" && w[1] == "b" && w[2] == "c"
    };
    s.add("J9-唤醒序累积", j9, "woken 序=入队序");

    // J10 重号队列拒
    let j10 = b.declare(1).err() == Some(WQ_UNK_QUEUE);
    s.add("J10-重号队列拒", j10, "队列重号必拒");

    // J11 排队返回位次
    let j11 = {
        let mut b2 = WqBook::new();
        let _ = b2.declare(3);
        b2.wait(3, "x") == Ok(1) && b2.wait(3, "y") == Ok(2)
    };
    s.add("J11-位次返回", j11, "入队返队列位次");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

/// 小工具：字符串切片转 Vec<String>。
fn vec_str(xs: &[&str]) -> Vec<String> {
    let mut v = Vec::new();
    for x in xs {
        v.push(x.to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24147_waitq_flow() {
        let mut b = mk_book();
        assert_eq!(b.wake_nth(1, "c"), Err(WQ_JUMP));
        assert!(b.wake_one(1).is_ok());
        assert!(b.wake_one(1).is_ok());
        assert_eq!(b.wake_one(1), Err(WQ_EMPTY_WAKE));
        assert_eq!(b.wait(1, "a"), Err(WQ_DUP_WAIT));
        b.seal();
        assert_eq!(b.wake_one(1), Err(WQ_FROZEN));
    }

    #[test]
    fn unx24147_checks_green() {
        let s = run_unx24147_checks();
        assert!(s.all_passed(), "UNX-F24147 判据未全绿");
    }
}
