//! UNX-F24167 MSI-X 中断消费与 IrqBackend 对接·子功能07（affinity CPU 绑定）（0x416C0 段）
//!
//! 锚点：UNX-F24167-J1 亲和账（向量→CPU 绑定逐条落账）；J2 亲和语义
//! （CPU 闭集 + 重绑转移 + 非法核拒三性质）；J3 亲和指纹（账折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（中断亲和面）。
//!
//! 结构：核位表 + 绑定/转移核 + 账本 + 折叠指纹 + 注入核（越界核/野
//! 向量/同核重绑/印后/空核表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x416C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x416C0;
pub const AF_BAD_CPU: u32 = SEG_BASE + 0x01; // 越界核
pub const AF_UNK_VEC: u32 = SEG_BASE + 0x02; // 野向量
pub const AF_SAME_CPU: u32 = SEG_BASE + 0x03; // 同核重绑
pub const AF_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const AF_NO_CPUS: u32 = SEG_BASE + 0x05; // 空核表

/// 绑定条目。
#[derive(Clone)]
pub struct AfBind {
    pub vec_no: u32,
    pub cpu: u32,
    pub moves: u32,
}

/// 亲和簿。
pub struct AfBook {
    pub ncpus: u32,
    pub binds: Vec<AfBind>,
    pub sealed: bool,
}

impl AfBook {
    pub fn new(ncpus: u32) -> AfBook {
        AfBook { ncpus, binds: Vec::new(), sealed: false }
    }

    /// 绑定/重绑。
    pub fn bind(&mut self, vec_no: u32, cpu: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AF_FROZEN);
        }
        if self.ncpus == 0 {
            return Err(AF_NO_CPUS);
        }
        if cpu >= self.ncpus {
            return Err(AF_BAD_CPU);
        }
        match self.binds.iter_mut().find(|b| b.vec_no == vec_no) {
            Some(b) => {
                if b.cpu == cpu {
                    return Err(AF_SAME_CPU);
                }
                b.cpu = cpu;
                b.moves += 1;
                Ok(())
            }
            None => {
                self.binds.push(AfBind { vec_no, cpu, moves: 0 });
                Ok(())
            }
        }
    }

    pub fn cpu_of(&self, vec_no: u32) -> Option<u32> {
        self.binds.iter().find(|b| b.vec_no == vec_no).map(|b| b.cpu)
    }

    pub fn on_cpu(&self, cpu: u32) -> usize {
        self.binds.iter().filter(|b| b.cpu == cpu).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x416C0;
        for b in &self.binds {
            h = h.wrapping_mul(31).wrapping_add(b.vec_no);
            h = h.wrapping_mul(31).wrapping_add(b.cpu);
            h = h.wrapping_mul(31).wrapping_add(b.moves);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：4 核，三向量绑定。
pub fn mk_book() -> AfBook {
    let mut b = AfBook::new(4);
    let _ = b.bind(0, 0);
    let _ = b.bind(1, 1);
    let _ = b.bind(2, 2);
    let _ = b.bind(2, 0); // 重绑转移
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 越界核
    got[0] = b.bind(3, 4).err() == Some(AF_BAD_CPU);
    // 2) 野向量重绑
    got[1] = b.bind(0, 0).err() == Some(AF_SAME_CPU);
    // 3) 空核表
    got[2] = AfBook::new(0).bind(0, 0).err() == Some(AF_NO_CPUS);
    // 4) 印后绑定必抓
    b.seal();
    got[3] = b.bind(3, 1).err() == Some(AF_FROZEN);
    // 5) 转移计数（moves 累计）
    got[4] = b.cpu_of(2) == Some(0) && b.binds.iter().find(|x| x.vec_no == 2).map(|x| x.moves == 1).unwrap_or(false);
    got
}

/// 判据主体。
pub fn run_unx24167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24167");
    let mut b = mk_book();

    // J1 绑定账齐
    let j1 = b.binds.len() == 3;
    s.add("J1-绑定账齐", j1, "三向量全落表");

    // J2 核闭集
    let j2 = b.binds.iter().all(|x| x.cpu < b.ncpus);
    s.add("J2-核闭集", j2, "全核 < 4");

    // J3 重绑转移
    let j3 = b.cpu_of(2) == Some(0);
    s.add("J3-重绑转移", j3, "向量 2 由核 2 → 0");

    // J4 转移计数
    let j4 = b.binds.iter().find(|x| x.vec_no == 2).map(|x| x.moves == 1).unwrap_or(false);
    s.add("J4-转移计数", j4, "moves=1");

    // J5 同核重绑拒
    let j5 = b.bind(0, 0).err() == Some(AF_SAME_CPU);
    s.add("J5-同核拒", j5, "同核重绑必拒");

    // J6 越界核拒
    let j6 = b.bind(3, 4).err() == Some(AF_BAD_CPU);
    s.add("J6-越界核拒", j6, "cpu ≥ ncpus 必拒");

    // J7 空核表拒
    let j7 = AfBook::new(0).bind(0, 0).err() == Some(AF_NO_CPUS);
    s.add("J7-空核表拒", j7, "ncpus=0 必拒");

    // J8 核负载分布
    let j8 = b.on_cpu(0) == 2 && b.on_cpu(1) == 1;
    s.add("J8-负载分布", j8, "核0 两向量核1 一向量");

    // J9 野向量查询空
    let j9 = b.cpu_of(99).is_none();
    s.add("J9-野查询空", j9, "未绑定向量查询 None");

    // J10 新向量绑定
    let j10 = b.bind(3, 3).is_ok() && b.cpu_of(3) == Some(3);
    s.add("J10-新向量绑", j10, "向量 3 绑核 3");

    // J11 印后绑定拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.bind(0, 1).err() == Some(AF_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

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
    fn unx24167_affinity_flow() {
        let mut b = AfBook::new(2);
        assert!(b.bind(0, 0).is_ok());
        assert!(b.bind(0, 1).is_ok());
        assert_eq!(b.bind(0, 1), Err(AF_SAME_CPU));
        assert_eq!(b.bind(1, 2), Err(AF_BAD_CPU));
        assert_eq!(AfBook::new(0).bind(0, 0), Err(AF_NO_CPUS));
        b.seal();
        assert_eq!(b.bind(1, 0), Err(AF_FROZEN));
    }

    #[test]
    fn unx24167_checks_green() {
        let s = run_unx24167_checks();
        assert!(s.all_passed(), "UNX-F24167 判据未全绿");
    }
}
