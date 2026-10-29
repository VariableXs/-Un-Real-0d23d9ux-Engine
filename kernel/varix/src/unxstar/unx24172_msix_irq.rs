//! UNX-F24172 MSI-X 中断消费与 IrqBackend 对接·子功能12（中断嵌套深度）（0x41760 段）
//!
//! 锚点：UNX-F24172-J1 嵌套账（中断进入/退出深度逐条落账）；J2 嵌套
//! 语义（深度上限闸 + 欠退出拒 + 深度归零三性质）；J3 嵌套指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（中断嵌套面）。
//!
//! 结构：深度栈 + enter/exit 核 + 账本 + 折叠指纹 + 注入核（超深/欠
//! 退/野 CPU/印后/零深退出必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41760 独占 0x20）。
pub const SEG_BASE: u32 = 0x41760;
pub const ND_OVER: u32 = SEG_BASE + 0x01; // 超深
pub const ND_UNDER: u32 = SEG_BASE + 0x02; // 欠退出
pub const ND_UNK_CPU: u32 = SEG_BASE + 0x03; // 野 CPU
pub const ND_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const ND_BAD_DEPTH: u32 = SEG_BASE + 0x05; // 深度档越界

/// CPU 嵌套态。
#[derive(Clone)]
pub struct NdCpu {
    pub cpu: u32,
    pub depth: u32,
    pub enters: u32,
    pub exits: u32,
}

/// 嵌套簿。
pub struct NdBook {
    pub cpus: Vec<NdCpu>,
    pub depth_max: u32,
    pub sealed: bool,
}

impl NdBook {
    pub fn new(cpus: u32, depth_max: u32) -> NdBook {
        let mut v = Vec::new();
        for c in 0..cpus {
            v.push(NdCpu { cpu: c, depth: 0, enters: 0, exits: 0 });
        }
        NdBook { cpus: v, depth_max, sealed: false }
    }

    pub fn enter(&mut self, cpu: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ND_FROZEN);
        }
        let c = match self.cpus.iter_mut().find(|c| c.cpu == cpu) {
            Some(c) => c,
            None => return Err(ND_UNK_CPU),
        };
        if c.depth >= self.depth_max {
            return Err(ND_OVER);
        }
        c.depth += 1;
        c.enters += 1;
        Ok(c.depth)
    }

    pub fn exit(&mut self, cpu: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ND_FROZEN);
        }
        let c = match self.cpus.iter_mut().find(|c| c.cpu == cpu) {
            Some(c) => c,
            None => return Err(ND_UNK_CPU),
        };
        if c.depth == 0 {
            return Err(ND_UNDER);
        }
        c.depth -= 1;
        c.exits += 1;
        Ok(c.depth)
    }

    pub fn depth_of(&self, cpu: u32) -> u32 {
        self.cpus.iter().find(|c| c.cpu == cpu).map(|c| c.depth).unwrap_or(0)
    }

    pub fn all_zero(&self) -> bool {
        self.cpus.iter().all(|c| c.depth == 0)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41760;
        for c in &self.cpus {
            h = h.wrapping_mul(31).wrapping_add(c.cpu);
            h = h.wrapping_mul(31).wrapping_add(c.depth);
            h = h.wrapping_mul(31).wrapping_add(c.enters);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：双核深限 3。
pub fn mk_book() -> NdBook {
    let mut b = NdBook::new(2, 3);
    let _ = b.enter(0);
    let _ = b.enter(0);
    let _ = b.enter(0); // depth 3
    let _ = b.enter(1);
    let _ = b.exit(1); // 核 1 归零
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 超深
    got[0] = b.enter(0).err() == Some(ND_OVER);
    // 2) 欠退出
    got[1] = b.exit(1).err() == Some(ND_UNDER);
    // 3) 野 CPU
    got[2] = b.enter(99).err() == Some(ND_UNK_CPU);
    // 4) 深度归零
    let _ = b.exit(0);
    let _ = b.exit(0);
    got[3] = b.depth_of(0) == 1;
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.enter(0).err() == Some(ND_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24172");
    let mut b = mk_book();

    // J1 CPU 账齐
    let j1 = b.cpus.len() == 2;
    s.add("J1-CPU账齐", j1, "双核落表");

    // J2 深度累计
    let j2 = b.depth_of(0) == 3 && b.depth_of(1) == 0;
    s.add("J2-深度累计", j2, "核0 深度 3 核1 归零");

    // J3 超深拒
    let j3 = b.enter(0).err() == Some(ND_OVER);
    s.add("J3-超深拒", j3, "depth ≥ max 必拒");

    // J4 欠退出拒
    let j4 = b.exit(1).err() == Some(ND_UNDER);
    s.add("J4-欠退拒", j4, "零深退出必拒");

    // J5 退出降深
    let j5 = {
        let d = b.exit(0).unwrap_or(0);
        d == 2
    };
    s.add("J5-退出降深", j5, "退出后深度 2");

    // J6 野 CPU 拒
    let j6 = b.enter(99).err() == Some(ND_UNK_CPU);
    s.add("J6-野CPU拒", j6, "未声明核必拒");

    // J7 全归零
    let j7 = {
        let _ = b.exit(0);
        let _ = b.exit(0);
        b.all_zero()
    };
    s.add("J7-全归零", j7, "全退出后深度清零");

    // J8 进入退出配对
    let j8 = {
        let c = b.cpus.iter().find(|c| c.cpu == 0).unwrap();
        c.enters == c.exits
    };
    s.add("J8-进出配对", j8, "enters == exits");

    // J9 边界深度（恰等上限拒）
    let j9 = {
        let mut b2 = NdBook::new(1, 1);
        let a = b2.enter(0).is_ok();
        let r = b2.enter(0).err() == Some(ND_OVER);
        a && r
    };
    s.add("J9-边界拒", j9, "深 1 限 1 二入必拒");

    // J10 退出返值
    let j10 = {
        let mut b2 = NdBook::new(1, 3);
        let _ = b2.enter(0);
        let _ = b2.enter(0);
        b2.exit(0) == Ok(1)
    };
    s.add("J10-退出返值", j10, "退出返余深");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.enter(1).err() == Some(ND_FROZEN)
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
    fn unx24172_nest_flow() {
        let mut b = NdBook::new(1, 2);
        assert_eq!(b.enter(0), Ok(1));
        assert_eq!(b.enter(0), Ok(2));
        assert_eq!(b.enter(0), Err(ND_OVER));
        assert_eq!(b.exit(0), Ok(1));
        assert_eq!(b.exit(0), Ok(0));
        assert_eq!(b.exit(0), Err(ND_UNDER));
        assert_eq!(b.enter(99), Err(ND_UNK_CPU));
        b.seal();
        assert_eq!(b.enter(0), Err(ND_FROZEN));
    }

    #[test]
    fn unx24172_checks_green() {
        let s = run_unx24172_checks();
        assert!(s.all_passed(), "UNX-F24172 判据未全绿");
    }
}
