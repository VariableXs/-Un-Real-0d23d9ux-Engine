//! UNX-F12205 嵌套 try/finally ≥5 层展开判据（SEG 0x640a0..0x640c0 段窗）
//!
//! 卡面：嵌套 try/finally 深链展开——finally 链自内向外逆序收口、
//! 部分收口、重复层拒止；≥5 层深链判据逐条过闸。
//! 域：CoRun·UNX-D1-B11。
//!
//! 判据原文：
//! - J1 深链压栈：层 id 顺序压入，重复 id 必拒 FC_DUP。
//! - J2 深度上限：层深超 64 必拒 FC_DEPTH。
//! - J3 全收口逆序：unwind_all 自内向外（栈顶先跑）留痕。
//! - J4 深链五层：5 层链全收口得 5 条逆序留痕。
//! - J5 部分收口：unwind_to(n) 只收口栈顶 n 层。
//! - J6 收口越界拒止：n 超过当前层深必拒 FC_PARTIAL。
//! - J7 空链拒止：空链收口必拒 FC_EMPTY。
//! - J8 残链记账：部分收口后剩余层数可查。
//! - J9 印账冻结：seal 后压栈/收口必拒 FC_FROZEN。
//! - J10 收口幂等：链清空后再收口必拒 FC_EMPTY。
//! - J11 指纹幂等：同态同指纹。
//! - J12 注入五况全抓。
//!
//! 结构：FinallyChain 层链（层 id 栈/收口账/操作账）+ 收口核
//! （unwind_all/unwind_to）+ 折叠指纹 + 注入核（空链/超深/重复层/
//! 越界收口/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x640a0 独占 0x20 窗）。
pub const SEG_BASE: u32 = 0x640a0;
pub const FC_EMPTY: u32 = SEG_BASE + 0x01; // 空链收口
pub const FC_DEPTH: u32 = SEG_BASE + 0x02; // 层深超限
pub const FC_DUP: u32 = SEG_BASE + 0x03; // 重复层 id
pub const FC_PARTIAL: u32 = SEG_BASE + 0x04; // 收口越界
pub const FC_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 层深上限。
pub const MAX_LAYERS: usize = 64;

/// finally 层链。
pub struct FinallyChain {
    pub layers: Vec<u32>,
    pub run_log: Vec<(u8, u32)>, // (来源 1=全收口 2=部分收口, 层 id)
    pub ops: Vec<String>,
    pub sealed: bool,
}

impl FinallyChain {
    pub fn new() -> FinallyChain {
        FinallyChain { layers: Vec::new(), run_log: Vec::new(), ops: Vec::new(), sealed: false }
    }

    /// 层压栈：重复 id 必拒、深度受限。
    pub fn push(&mut self, layer: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if self.layers.contains(&layer) {
            return Err(FC_DUP);
        }
        if self.layers.len() >= MAX_LAYERS {
            return Err(FC_DEPTH);
        }
        self.layers.push(layer);
        self.ops.push(format!("push{}", layer));
        Ok(())
    }

    /// 全收口：自内向外（栈顶先跑）逐层留痕并清链。
    pub fn unwind_all(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if self.layers.is_empty() {
            return Err(FC_EMPTY);
        }
        let mut n = 0usize;
        while let Some(id) = self.layers.pop() {
            self.run_log.push((1, id));
            self.ops.push(format!("fin{}", id));
            n += 1;
        }
        Ok(n)
    }

    /// 部分收口：只收口栈顶 n 层；n 超过层深必拒。
    pub fn unwind_to(&mut self, n: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if self.layers.is_empty() {
            return Err(FC_EMPTY);
        }
        if n > self.layers.len() {
            return Err(FC_PARTIAL);
        }
        let mut k = 0usize;
        while k < n {
            match self.layers.pop() {
                Some(id) => {
                    self.run_log.push((2, id));
                    self.ops.push(format!("fin{}", id));
                    k += 1;
                }
                None => return Err(FC_EMPTY),
            }
        }
        Ok(k)
    }

    /// 剩余层深。
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    /// 收口账条数。
    pub fn run_count(&self) -> usize {
        self.run_log.len()
    }

    /// 栈顶第 i 个待收口层 id（0=最内层）。
    pub fn top_of(&self, i: usize) -> Option<u32> {
        let n = self.layers.len();
        if i < n {
            Some(self.layers[n - 1 - i])
        } else {
            None
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for l in &self.layers {
            h = h.wrapping_mul(211).wrapping_add(*l);
        }
        for r in &self.run_log {
            h = h.wrapping_mul(211).wrapping_add(r.0 as u32);
            h = h.wrapping_mul(211).wrapping_add(r.1);
        }
        for o in &self.ops {
            for by in o.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("dep={} run={}", self.depth(), self.run_count())
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 空链收口必拒。
    got[0] = FinallyChain::new().unwind_all().err() == Some(FC_EMPTY);
    // 2) 重复层 id 必拒。
    let mut c = FinallyChain::new();
    let _ = c.push(7);
    got[1] = c.push(7).err() == Some(FC_DUP);
    // 3) 收口越界必拒（3 层收 4）。
    let mut c = FinallyChain::new();
    let _ = c.push(1);
    let _ = c.push(2);
    let _ = c.push(3);
    got[2] = c.unwind_to(4).err() == Some(FC_PARTIAL);
    // 4) 超深必拒（65 层）。
    let mut d = FinallyChain::new();
    let mut i = 0u32;
    while i < 64 {
        let _ = d.push(0x1000 + i);
        i += 1;
    }
    got[3] = d.push(0x9999).err() == Some(FC_DEPTH);
    // 5) 印账后操作必抓。
    let mut f = FinallyChain::new();
    f.seal();
    got[4] = f.push(1).err() == Some(FC_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12205_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12205");

    // J1 深链压栈与重复拒止。
    let j1 = {
        let mut c = FinallyChain::new();
        c.push(1).is_ok() && c.push(1).err() == Some(FC_DUP)
    };
    s.add("J1-深链压栈", j1, "重复 id 必拒");

    // J2 深度上限。
    let j2 = {
        let mut c = FinallyChain::new();
        let mut ok = true;
        let mut i = 0u32;
        while i < 64 {
            if c.push(i).is_err() {
                ok = false;
            }
            i += 1;
        }
        ok && c.push(0xFFFF).err() == Some(FC_DEPTH)
    };
    s.add("J2-深度上限", j2, "64 层满后再压必拒");

    // J3 全收口逆序。
    let j3 = {
        let mut c = FinallyChain::new();
        let _ = c.push(1);
        let _ = c.push(2);
        let _ = c.push(3);
        let n = c.unwind_all().unwrap_or(0);
        n == 3 && c.run_log[0].1 == 3 && c.run_log[1].1 == 2 && c.run_log[2].1 == 1
    };
    s.add("J3-全收口逆序", j3, "栈顶 finally 先跑");

    // J4 深链五层全收口。
    let j4 = {
        let mut c = FinallyChain::new();
        let _ = c.push(10);
        let _ = c.push(11);
        let _ = c.push(12);
        let _ = c.push(13);
        let _ = c.push(14);
        let n = c.unwind_all().unwrap_or(0);
        n == 5 && c.run_log.iter().map(|r| r.1).collect::<Vec<u32>>() == vec![14, 13, 12, 11, 10]
    };
    s.add("J4-五层深链", j4, "≥5 层逆序全收口");

    // J5 部分收口。
    let j5 = {
        let mut c = FinallyChain::new();
        let _ = c.push(1);
        let _ = c.push(2);
        let _ = c.push(3);
        let k = c.unwind_to(2).unwrap_or(0);
        k == 2 && c.depth() == 1 && c.top_of(0) == Some(1)
    };
    s.add("J5-部分收口", j5, "只收栈顶 n 层");

    // J6 收口越界拒止。
    let j6 = {
        let mut c = FinallyChain::new();
        let _ = c.push(1);
        c.unwind_to(2).err() == Some(FC_PARTIAL) && c.depth() == 1
    };
    s.add("J6-收口越界拒止", j6, "n 超层深必拒且链不动");

    // J7 空链拒止。
    let j7 = FinallyChain::new().unwind_to(1).err() == Some(FC_EMPTY);
    s.add("J7-空链拒止", j7, "空链部分收口亦拒");

    // J8 残链记账。
    let j8 = {
        let mut c = FinallyChain::new();
        let _ = c.push(5);
        let _ = c.push(6);
        let _ = c.unwind_to(1);
        c.summary() == "dep=1 run=1"
    };
    s.add("J8-残链记账", j8, "余层与收口账可查");

    // J9 印账冻结。
    let j9 = {
        let mut c = FinallyChain::new();
        let _ = c.push(1);
        c.seal();
        c.push(2).err() == Some(FC_FROZEN)
            && c.unwind_all().err() == Some(FC_FROZEN)
            && c.unwind_to(1).err() == Some(FC_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后压栈/收口皆拒");

    // J10 收口幂等。
    let j10 = {
        let mut c = FinallyChain::new();
        let _ = c.push(1);
        let _ = c.unwind_all();
        c.unwind_all().err() == Some(FC_EMPTY)
    };
    s.add("J10-收口幂等", j10, "清链后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut c = FinallyChain::new();
        let _ = c.push(3);
        c.fold() == c.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 栈顶探针。
    let j12 = {
        let mut c = FinallyChain::new();
        let _ = c.push(4);
        let _ = c.push(5);
        c.top_of(0) == Some(5) && c.top_of(1) == Some(4) && c.top_of(2).is_none()
    };
    s.add("J12-栈顶探针", j12, "i=0 最内层、越界 None");

    let got = inject_five();
    s.add_dyn("J13-注入全抓", got.iter().all(|x| *x), &format!("inj5={:?}", got));
    s.add("J14-收口账次序", {
        let mut c = FinallyChain::new();
        let _ = c.push(2);
        let _ = c.push(1);
        let _ = c.unwind_all();
        c.run_log.iter().map(|r| r.1).collect::<Vec<u32>>() == vec![1, 2]
    }, "全收口账严格逆序");
    s.add("J15-指纹非零", {
        let mut c = FinallyChain::new();
        let _ = c.push(9);
        c.fold() != 0
    }, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12205_finally_flow() {
        let mut c = FinallyChain::new();
        assert!(c.push(1).is_ok());
        assert!(c.push(2).is_ok());
        assert_eq!(c.unwind_to(4), Err(FC_PARTIAL));
        assert_eq!(c.unwind_all(), Ok(2));
        assert_eq!(c.unwind_all(), Err(FC_EMPTY));
    }

    #[test]
    fn unx12205_checks_green() {
        let s = run_unx12205_checks();
        assert!(s.all_passed(), "UNX-F12205 判据未全绿");
    }

    #[test]
    fn unx12205_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
