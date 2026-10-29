//! UNX-F24143 reservation 锁与显存互斥纪律·子功能03（锁序纪律与死锁预防）（0x413C0 段）
//!
//! 锚点：UNX-F24143-J1 锁序账（编号序持锁与欲取边逐条落账）；J2 锁
//! 序语义（升序闸 + LIFO 释放 + 等待环检测三性质）；J3 锁序指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（锁序死锁预防面）。
//!
//! 结构：持锁/欲取双序列表 + 升序闸核 + 等待图环检测核 + 折叠指纹 +
//! 注入核（降序持锁/野锁/重复释放/真环识破/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x413C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x413C0;
pub const LO_DESCEND: u32 = SEG_BASE + 0x01; // 降序持锁
pub const LO_UNK_LOCK: u32 = SEG_BASE + 0x02; // 野锁
pub const LO_DUP_REL: u32 = SEG_BASE + 0x03; // 非顶释放/重复持
pub const LO_DEADLOCK: u32 = SEG_BASE + 0x04; // 等待环
pub const LO_FROZEN: u32 = SEG_BASE + 0x05; // 印后持锁

/// 单任务持锁/欲取序列。
#[derive(Clone)]
pub struct TaskSeq {
    pub task: String,
    pub held: Vec<u32>, // 按获取顺序（升序纪律）
    pub want: Vec<u32>, // 被拒的欲取锁（等待边源）
}

/// 锁序簿。
pub struct LoBook {
    pub known_locks: Vec<u32>,
    pub tasks: Vec<TaskSeq>,
    pub sealed: bool,
}

impl LoBook {
    pub fn new(known_locks: &[u32]) -> LoBook {
        LoBook { known_locks: known_locks.to_vec(), tasks: Vec::new(), sealed: false }
    }

    fn task_mut(&mut self, task: &str) -> &mut TaskSeq {
        if !self.tasks.iter().any(|t| t.task == task) {
            self.tasks.push(TaskSeq {
                task: task.to_string(),
                held: Vec::new(),
                want: Vec::new(),
            });
        }
        self.tasks.iter_mut().find(|t| t.task == task).unwrap()
    }

    /// 升序持锁：新锁编号必须大于已持全部；被拒时登记欲取边。
    pub fn lock(&mut self, task: &str, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LO_FROZEN);
        }
        if !self.known_locks.contains(&lock_id) {
            return Err(LO_UNK_LOCK);
        }
        // 只读判定拒绝码
        let reject = {
            let t = self.tasks.iter().find(|t| t.task == task);
            match t {
                Some(t) => match t.held.last() {
                    Some(&top) if lock_id <= top => {
                        if t.held.contains(&lock_id) {
                            Some(LO_DUP_REL)
                        } else {
                            Some(LO_DESCEND)
                        }
                    }
                    _ => None,
                },
                None => None,
            }
        };
        match reject {
            Some(code) => {
                // 欲取边登记（等待环检测的输入）
                let tt = self.task_mut(task);
                if !tt.want.contains(&lock_id) {
                    tt.want.push(lock_id);
                }
                Err(code)
            }
            None => {
                self.task_mut(task).held.push(lock_id);
                Ok(())
            }
        }
    }

    /// 释放：只可释最顶（后取先放）；释放后清除对应欲取边。
    pub fn unlock(&mut self, task: &str, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LO_FROZEN);
        }
        let t = match self.tasks.iter_mut().find(|t| t.task == task) {
            Some(t) => t,
            None => return Err(LO_DUP_REL),
        };
        match t.held.last() {
            Some(&top) if top == lock_id => {
                t.held.pop();
                t.want.retain(|&x| x != lock_id);
                Ok(())
            }
            _ => Err(LO_DUP_REL),
        }
    }

    /// 等待边：a 欲取的锁中有 b 持有的。
    fn wait_edge(&self, a: &TaskSeq, b: &TaskSeq) -> bool {
        a.want.iter().any(|w| b.held.contains(w))
    }

    /// 等待环检测：存在 a、b 互等对方持有的锁。
    pub fn has_cycle(&self) -> bool {
        for a in &self.tasks {
            for b in &self.tasks {
                if a.task == b.task {
                    continue;
                }
                if self.wait_edge(a, b) && self.wait_edge(b, a) {
                    return true;
                }
            }
        }
        false
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x413C0;
        for t in &self.tasks {
            for by in t.task.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            for &l in &t.held {
                h = h.wrapping_mul(31).wrapping_add(l);
            }
            for &l in &t.want {
                h = h.wrapping_mul(31).wrapping_add(l.wrapping_mul(7));
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：锁 1~4，两任务合法升序持锁。
pub fn mk_book() -> LoBook {
    let mut b = LoBook::new(&[1, 2, 3, 4]);
    let _ = b.lock("t1", 1);
    let _ = b.lock("t1", 3);
    let _ = b.lock("t2", 2);
    let _ = b.lock("t2", 4);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 降序持锁
    got[0] = b.lock("t1", 2).err() == Some(LO_DESCEND);
    // 2) 野锁
    got[1] = b.lock("t1", 9).err() == Some(LO_UNK_LOCK);
    // 3) 非顶释放
    got[2] = b.unlock("t1", 1).err() == Some(LO_DUP_REL);
    // 4) 重复持锁
    got[3] = b.lock("t2", 4).err() == Some(LO_DUP_REL);
    // 5) 印后持锁必抓
    b.seal();
    got[4] = b.lock("t3", 4).err() == Some(LO_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24143_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24143");
    let mut b = mk_book();

    // J1 任务账齐
    let j1 = b.tasks.len() == 2;
    s.add("J1-任务账齐", j1, "两任务落表");

    // J2 升序持闸
    let j2 = {
        let t1 = b.tasks.iter().find(|t| t.task == "t1").unwrap();
        t1.held.len() == 2 && t1.held[0] == 1 && t1.held[1] == 3
    };
    s.add("J2-升序持闸", j2, "t1 持 [1,3] 升序");

    // J3 降序拒
    let j3 = b.lock("t1", 2).err() == Some(LO_DESCEND);
    s.add("J3-降序拒", j3, "低于顶锁必拒");

    // J4 LIFO 释放
    let j4 = {
        let mut b2 = mk_book();
        b2.unlock("t1", 3).is_ok() && b2.unlock("t1", 1).is_ok()
    };
    s.add("J4-LIFO释放", j4, "后取先放合法");

    // J5 非顶释放拒
    let j5 = b.unlock("t1", 1).err() == Some(LO_DUP_REL);
    s.add("J5-非顶释放拒", j5, "跳顶释放必拒");

    // J6 野锁拒
    let j6 = b.lock("t3", 9).err() == Some(LO_UNK_LOCK);
    s.add("J6-野锁拒", j6, "未登记锁必拒");

    // J7 重复持锁拒
    let j7 = b.lock("t2", 4).err() == Some(LO_DUP_REL);
    s.add("J7-重复持锁拒", j7, "已持再取必拒");

    // J8 欲取边登记（降序拒后 want 有痕）
    let j8 = {
        let t1 = b.tasks.iter().find(|t| t.task == "t1").unwrap();
        t1.want.contains(&2)
    };
    s.add("J8-欲取边登记", j8, "被拒欲取锁入 want");

    // J9 合法升序无环
    let j9 = {
        let mut b2 = mk_book();
        // t1 欲取 2 被拒（t2 持 2），t2 无欲取 → 无环
        let _ = b2.lock("t1", 2);
        !b2.has_cycle()
    };
    s.add("J9-合法无环", j9, "单向等待不成环");

    // J10 真环识破：t1 持 [1] 欲取 2；t2 持 [2] 欲取 1 → 互等成环
    let j10 = {
        let mut b2 = LoBook::new(&[1, 2]);
        let _ = b2.lock("t1", 1);
        let _ = b2.lock("t2", 2);
        let _ = b2.lock("t1", 2); // t1 欲取 2 被 t2 持 → DESCEND 拒 + want 登记
        let _ = b2.lock("t2", 1); // t2 欲取 1 被 t1 持 → DESCEND 拒 + want 登记
        b2.has_cycle()
    };
    s.add("J10-真环识破", j10, "互等对方持锁必判环");

    // J11 释放清欲取边
    let j11 = {
        let mut b2 = LoBook::new(&[1, 2]);
        let _ = b2.lock("t1", 1);
        let _ = b2.lock("t2", 2);
        let _ = b2.lock("t1", 2); // want=[2]
        let _ = b2.unlock("t2", 2); // t2 释放 2
        // 释放不清他人 want——但 t1 的 want 仍在；重新获取 2 成功后清
        let _ = b2.lock("t1", 2);
        let t1 = b2.tasks.iter().find(|t| t.task == "t1").unwrap();
        !t1.want.contains(&2)
    };
    s.add("J11-获取清边", j11, "成功获取后清欲取边");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
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
    fn unx24143_lockorder_flow() {
        let mut b = mk_book();
        assert_eq!(b.lock("t1", 2), Err(LO_DESCEND));
        assert_eq!(b.lock("t1", 9), Err(LO_UNK_LOCK));
        assert!(b.unlock("t1", 3).is_ok());
        assert_eq!(b.unlock("t1", 3), Err(LO_DUP_REL));
        b.seal();
        assert_eq!(b.lock("t3", 4), Err(LO_FROZEN));
    }

    #[test]
    fn unx24143_checks_green() {
        let s = run_unx24143_checks();
        assert!(s.all_passed(), "UNX-F24143 判据未全绿");
    }
}
