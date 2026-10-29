//! UNX-F12096 DuplicateObject 语义预告（D3 联签）（0x19e20 段）
//!
//! 卡面：对象复制语义——源句柄须在源表、目标访问不得越源授权
//! （needed ⊆ granted）、DUQ_SAME_ACCESS 旗一致约束、目标表重句柄拒。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 源句柄存在性：源句柄不在源表必拒。
//! - J2 访问不越源：目标访问超出源授权位必拒。
//! - J3 SAME_ACCESS 一致性：旗置而访问不等于源授权必拒。
//! - J4 目标唯一：目标表重句柄必拒（含零句柄）。
//! - J5 复制正路径：合法复制生成新句柄且引用计数递增。
//! - J6 印账冻结：seal 后复制必拒。
//! - J7 注入五况全抓。
//!
//! 结构：DupReq 复制请求 + DupBook 双表账（源表 + 目标表 + 引用计数）+
//! 折叠指纹（211 进制卷积）+ 注入核（无源/越权/旗失配/重目标/印后复制）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19e20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19e20;
pub const DP_NO_SRC: u32 = SEG_BASE + 0x01; // 源不存在
pub const DP_ACCESS_EXCEED: u32 = SEG_BASE + 0x02; // 越源授权
pub const DP_FLAG_MISMATCH: u32 = SEG_BASE + 0x03; // SAME_ACCESS 失配
pub const DP_DUP_TARGET: u32 = SEG_BASE + 0x04; // 重目标
pub const DP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后复制

/// SAME_ACCESS 旗。
pub const DUQ_SAME_ACCESS: u32 = 0x0000_0002;

/// 源表行。
#[derive(Clone, Copy)]
pub struct SrcRow {
    pub handle: u32,
    pub granted: u32,
}

/// 复制请求。
#[derive(Clone, Copy)]
pub struct DupReq {
    pub src: u32,
    pub target: u32,
    pub access: u32,
    pub options: u32,
}

/// 复制账。
pub struct DupBook {
    pub srcs: Vec<SrcRow>,
    pub targets: Vec<u32>,
    pub refs: Vec<(u32, u32)>, // (handle, refcount)
    pub sealed: bool,
}

impl DupBook {
    pub fn new() -> DupBook {
        DupBook { srcs: Vec::new(), targets: Vec::new(), refs: Vec::new(), sealed: false }
    }

    /// 源表登记。
    pub fn add_src(&mut self, handle: u32, granted: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DP_FROZEN);
        }
        if handle == 0 {
            return Err(DP_NO_SRC); // 零源句柄视作无源
        }
        if self.srcs.iter().any(|r| r.handle == handle) {
            return Err(DP_NO_SRC);
        }
        self.srcs.push(SrcRow { handle, granted });
        self.refs.push((handle, 1));
        Ok(())
    }

    fn granted_of(&self, handle: u32) -> Option<u32> {
        self.srcs.iter().find(|r| r.handle == handle).map(|r| r.granted)
    }

    /// 复制核：无源 → SAME_ACCESS 一致 → 越权 → 重目标。
    pub fn dup(&mut self, req: DupReq) -> Result<(), u32> {
        if self.sealed {
            return Err(DP_FROZEN);
        }
        let granted = match self.granted_of(req.src) {
            Some(g) => g,
            None => return Err(DP_NO_SRC),
        };
        if req.options & DUQ_SAME_ACCESS != 0 && req.access != granted {
            return Err(DP_FLAG_MISMATCH);
        }
        if req.access & !granted != 0 {
            return Err(DP_ACCESS_EXCEED);
        }
        if req.target == 0 || self.targets.contains(&req.target) {
            return Err(DP_DUP_TARGET);
        }
        self.targets.push(req.target);
        // 目标引用计数挂源。
        if let Some(i) = self.refs.iter().position(|r| r.0 == req.src) {
            self.refs[i].1 += 1;
        }
        Ok(())
    }

    /// 引用计数回读。
    pub fn refcount_of(&self, handle: u32) -> Option<u32> {
        self.refs.iter().find(|r| r.0 == handle).map(|r| r.1)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.srcs {
            h = h.wrapping_mul(211).wrapping_add(r.handle);
            h = h.wrapping_mul(211).wrapping_add(r.granted);
        }
        for t in &self.targets {
            h = h.wrapping_mul(211).wrapping_add(*t);
        }
        for r in &self.refs {
            h = h.wrapping_mul(211).wrapping_add(r.0);
            h = h.wrapping_mul(211).wrapping_add(r.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("srcs={} targets={}", self.srcs.len(), self.targets.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, handle: u32) -> String {
        match self.refcount_of(handle) {
            Some(c) => format!("R{}|{}", handle, c),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = DupBook::new();
    let _ = b.add_src(4, 0x0011_0000);
    // 1) 无源必拒。
    got[0] = b.dup(DupReq { src: 9, target: 0x100, access: 0x0010_0000, options: 0 })
        .err() == Some(DP_NO_SRC);
    // 2) 越源授权必拒。
    got[1] = b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000 | 0x1000_0000, options: 0 })
        .err() == Some(DP_ACCESS_EXCEED);
    // 3) SAME_ACCESS 旗失配必拒。
    got[2] = b.dup(DupReq { src: 4, target: 0x100, access: 0x0001_0000, options: DUQ_SAME_ACCESS })
        .err() == Some(DP_FLAG_MISMATCH);
    // 4) 重目标必拒（合法复制一次后同目标再复制）。
    let _ = b.dup(DupReq { src: 4, target: 0x100, access: 0x0011_0000, options: 0 });
    got[3] = b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000, options: 0 })
        .err() == Some(DP_DUP_TARGET);
    // 5) 印账后复制必抓。
    b.seal();
    got[4] = b.dup(DupReq { src: 4, target: 0x200, access: 0x0010_0000, options: 0 })
        .err() == Some(DP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12096_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12096");
    const G: u32 = 0x0011_0000;

    // J1 无源拒。
    let j1 = {
        let mut b = DupBook::new();
        b.dup(DupReq { src: 4, target: 0x100, access: G, options: 0 })
            .err() == Some(DP_NO_SRC)
    };
    s.add("J1-无源拒", j1, "源不在表必拒");

    // J2 越权拒。
    let j2 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.dup(DupReq { src: 4, target: 0x100, access: G | 0x8000_0000, options: 0 })
            .err() == Some(DP_ACCESS_EXCEED)
    };
    s.add("J2-越权拒", j2, "needed ⊄ granted 必拒");

    // J3 SAME_ACCESS 失配拒。
    let j3 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000, options: DUQ_SAME_ACCESS })
            .err() == Some(DP_FLAG_MISMATCH)
    };
    s.add("J3-旗失配拒", j3, "旗置访问须等源");

    // J4 SAME_ACCESS 等值放行。
    let j4 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.dup(DupReq { src: 4, target: 0x100, access: G, options: DUQ_SAME_ACCESS }).is_ok()
    };
    s.add("J4-旗等值放行", j4, "访问等源即过");

    // J5 子集访问放行。
    let j5 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000, options: 0 }).is_ok()
    };
    s.add("J5-子集放行", j5, "授权子集复制合法");

    // J6 重目标拒。
    let j6 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        let _ = b.dup(DupReq { src: 4, target: 0x100, access: G, options: 0 });
        b.dup(DupReq { src: 4, target: 0x100, access: G, options: 0 })
            .err() == Some(DP_DUP_TARGET)
    };
    s.add("J6-重目标拒", j6, "同目标二次必拒");

    // J7 零目标拒。
    let j7 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.dup(DupReq { src: 4, target: 0, access: G, options: 0 })
            .err() == Some(DP_DUP_TARGET)
    };
    s.add("J7-零目标拒", j7, "零句柄必拒");

    // J8 引用计数递增。
    let j8 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        let _ = b.dup(DupReq { src: 4, target: 0x100, access: G, options: 0 });
        let _ = b.dup(DupReq { src: 4, target: 0x200, access: G, options: 0 });
        b.refcount_of(4) == Some(3)
    };
    s.add("J8-引用计数", j8, "1+2=3");

    // J9 印账冻结。
    let j9 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.seal();
        b.dup(DupReq { src: 4, target: 0x300, access: G, options: 0 })
            .err() == Some(DP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = DupBook::new();
        let _ = b.add_src(4, G);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = DupBook::new();
    let _ = b.add_src(4, G);
    let _ = b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000, options: 0 });

    s.add("摘要回读", b.summary().contains("targets=1"), "一目标在账");
    s.add("计数回读", b.refcount_of(4) == Some(2), "源计数 2");
    s.add("缺席行", b.line_of(9) == "missing", "缺行显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12096_dup_flow() {
        let mut b = DupBook::new();
        let _ = b.add_src(4, 0x0011_0000);
        assert!(b.dup(DupReq { src: 4, target: 0x100, access: 0x0010_0000, options: 0 }).is_ok());
        assert_eq!(
            b.dup(DupReq { src: 9, target: 0x200, access: 0x0010_0000, options: 0 }),
            Err(DP_NO_SRC)
        );
        assert_eq!(b.refcount_of(4), Some(2));
    }

    #[test]
    fn unx12096_checks_green() {
        let s = run_unx12096_checks();
        assert!(s.all_passed(), "UNX-F12096 判据未全绿");
    }

    #[test]
    fn same_access_flag() {
        let mut b = DupBook::new();
        let _ = b.add_src(4, 0x20);
        assert_eq!(
            b.dup(DupReq { src: 4, target: 0x10, access: 0x10, options: DUQ_SAME_ACCESS }),
            Err(DP_FLAG_MISMATCH)
        );
    }
}
