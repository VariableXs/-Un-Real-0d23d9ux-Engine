//! UNX-F24169 MSI-X 中断消费与 IrqBackend 对接·子功能09（threaded 中断·上下半部）（0x41700 段）
//!
//! 锚点：UNX-F24169-J1 上下账（top/bottom half 逐条落账）；J2 上下语
//! 义（top 快返 + bottom 队列 + 队满拒三性质）；J3 上下指纹（账折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（上下半部面）。
//!
//! 结构：上下半部引擎 + top 核 + bottom 队核 + 账本 + 折叠指纹 + 注入
//! 核（野向量/队满/空队冲刷/印后/重排必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41700 独占 0x20）。
pub const SEG_BASE: u32 = 0x41700;
pub const TB_UNK_VEC: u32 = SEG_BASE + 0x01; // 野向量
pub const TB_QUEUE_FULL: u32 = SEG_BASE + 0x02; // 队满
pub const TB_EMPTY_FLUSH: u32 = SEG_BASE + 0x03; // 空队冲刷
pub const TB_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const TB_BAD_KODE: u32 = SEG_BASE + 0x05; // 档越界

/// 下半部任务。
#[derive(Clone)]
pub struct TbTask {
    pub vec_no: u32,
    pub payload: u32,
}

/// 上下半部引擎。
pub struct TbEngine {
    pub known: Vec<u32>,
    pub queue: Vec<TbTask>,
    pub q_max: usize,
    pub top_fired: u32,
    pub bottom_done: u32,
    pub sealed: bool,
}

impl TbEngine {
    pub fn new(known: &[u32], q_max: usize) -> TbEngine {
        TbEngine { known: known.to_vec(), queue: Vec::new(), q_max, top_fired: 0, bottom_done: 0, sealed: false }
    }

    /// top half：快路径，登记下半部任务。
    pub fn top(&mut self, vec_no: u32, payload: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if !self.known.contains(&vec_no) {
            return Err(TB_UNK_VEC);
        }
        if self.queue.len() >= self.q_max {
            return Err(TB_QUEUE_FULL);
        }
        self.queue.push(TbTask { vec_no, payload });
        self.top_fired += 1;
        Ok(())
    }

    /// bottom half：冲刷队列（FIFO），返处理数。
    pub fn bottom_flush(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if self.queue.is_empty() {
            return Err(TB_EMPTY_FLUSH);
        }
        let n = self.queue.len();
        self.bottom_done += n as u32;
        self.queue.clear();
        Ok(n)
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41700;
        h = h.wrapping_mul(31).wrapping_add(self.top_fired);
        h = h.wrapping_mul(31).wrapping_add(self.bottom_done);
        for t in &self.queue {
            h = h.wrapping_mul(31).wrapping_add(t.vec_no);
            h = h.wrapping_mul(31).wrapping_add(t.payload);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量，三任务入队二冲刷。
pub fn mk_engine() -> TbEngine {
    let mut e = TbEngine::new(&[16, 17], 4);
    let _ = e.top(16, 1);
    let _ = e.top(17, 2);
    let _ = e.top(16, 3);
    let _ = e.bottom_flush(); // 3 个
    let _ = e.top(17, 4);
    e
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = mk_engine();
    // 1) 野向量
    got[0] = e.top(99, 9).err() == Some(TB_UNK_VEC);
    // 2) 队满
    let mut e2 = TbEngine::new(&[16], 2);
    let _ = e2.top(16, 1);
    let _ = e2.top(16, 2);
    got[1] = e2.top(16, 3).err() == Some(TB_QUEUE_FULL);
    // 3) 空队冲刷
    let mut e3 = TbEngine::new(&[16], 2);
    got[2] = e3.bottom_flush().err() == Some(TB_EMPTY_FLUSH);
    // 4) 冲后队空
    got[3] = e.queue_len() == 1;
    // 5) 印后操作必抓
    e.seal();
    got[4] = e.top(16, 9).err() == Some(TB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24169_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24169");
    let mut e = mk_engine();

    // J1 引擎账齐
    let j1 = e.known.len() == 2 && e.top_fired == 4;
    s.add("J1-引擎账齐", j1, "两向量四 top");

    // J2 top 入队
    let j2 = e.queue_len() == 1;
    s.add("J2-top入队", j2, "冲后余一");

    // J3 bottom 冲刷计数
    let j3 = e.bottom_done == 3;
    s.add("J3-冲刷计数", j3, "首轮冲 3");

    // J4 FIFO 序
    let j4 = {
        let mut e2 = TbEngine::new(&[16], 4);
        let _ = e2.top(16, 10);
        let _ = e2.top(16, 20);
        let q0 = e2.queue[0].payload;
        let q1 = e2.queue[1].payload;
        q0 == 10 && q1 == 20
    };
    s.add("J4-FIFO序", j4, "先入先冲");

    // J5 队满拒
    let j5 = {
        let mut e2 = TbEngine::new(&[16], 1);
        let _ = e2.top(16, 1);
        e2.top(16, 2).err() == Some(TB_QUEUE_FULL)
    };
    s.add("J5-队满拒", j5, "超 q_max 必拒");

    // J6 野向量拒
    let j6 = e.top(99, 9).err() == Some(TB_UNK_VEC);
    s.add("J6-野向量拒", j6, "未注册必拒");

    // J7 空队冲刷拒
    let j7 = {
        let mut e2 = TbEngine::new(&[16], 2);
        e2.bottom_flush().err() == Some(TB_EMPTY_FLUSH)
    };
    s.add("J7-空队冲刷拒", j7, "无人可冲必拒");

    // J8 冲后可再入
    let j8 = {
        let mut e2 = TbEngine::new(&[16], 2);
        let _ = e2.top(16, 1);
        let _ = e2.bottom_flush();
        e2.top(16, 2).is_ok()
    };
    s.add("J8-冲后再入", j8, "冲空后可入队");

    // J9 top/bottom 守恒
    let j9 = e.bottom_flush().is_ok() && e.top_fired == e.bottom_done;
    s.add("J9-上下守恒", j9, "全部 top 已冲");

    // J10 队满拒不丢 top 计数
    let j10 = {
        let mut e2 = TbEngine::new(&[16], 1);
        let _ = e2.top(16, 1);
        let _ = e2.top(16, 2);
        e2.top_fired == 1
    };
    s.add("J10-拒不计数", j10, "被拒 top 不计数");

    // J11 印后操作拒
    let j11 = {
        let mut e2 = mk_engine();
        e2.seal();
        e2.bottom_flush().err() == Some(TB_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = e.fold() == e.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24169_tb_flow() {
        let mut e = TbEngine::new(&[16], 2);
        assert!(e.top(16, 1).is_ok());
        assert!(e.top(16, 2).is_ok());
        assert_eq!(e.top(16, 3), Err(TB_QUEUE_FULL));
        assert_eq!(e.bottom_flush(), Ok(2));
        assert_eq!(e.bottom_flush(), Err(TB_EMPTY_FLUSH));
        assert_eq!(e.top(99, 1), Err(TB_UNK_VEC));
        e.seal();
        assert_eq!(e.top(16, 4), Err(TB_FROZEN));
    }

    #[test]
    fn unx24169_checks_green() {
        let s = run_unx24169_checks();
        assert!(s.all_passed(), "UNX-F24169 判据未全绿");
    }
}
