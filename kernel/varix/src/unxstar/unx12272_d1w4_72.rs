//! UNX-F12272 文件族与 B1 VFS 联签收口（0x64900 段）
//!
//! 卡面：NT 文件句柄面与 B1 VFS 节点面的双向绑定——每条联签记录含
//! VFS 节点号与 NT 句柄号，两侧均须在域内、互不重复；联签完成须双侧
//! 核对（节点查句柄、句柄查节点皆命中）方可 seal。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 双侧在域：VFS 节点号 0 或 NT 句柄号 0 必拒。
//! - J2 句柄唯一：同句柄重复联签必拒。
//! - J3 节点唯一：同节点重复联签必拒（一对多绑定须走 Dup 再签）。
//! - J4 双向核对：节点↔句柄正反查询皆命中。
//! - J5 未联签不可 seal：空账 seal 必拒。
//! - J6 印账冻结：seal 后联签必拒。
//! - J7 摘除一致：一侧摘除后反向查询落空。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64900 独占 0x20）。
pub const SEG_BASE: u32 = 0x64900;
pub const VS_BAD_ID: u32 = SEG_BASE + 0x01; // 双侧越域（0 号）
pub const VS_DUP: u32 = SEG_BASE + 0x02; // 句柄/节点重号
pub const VS_UNSIGNED: u32 = SEG_BASE + 0x03; // 空账印账
pub const VS_NOT_FOUND: u32 = SEG_BASE + 0x04; // 摘除目标不在册
pub const VS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后联签

/// 节点种类。
pub const NK_FILE: u8 = 1;
pub const NK_DIR: u8 = 2;
pub const NK_DEV: u8 = 3;

/// 一条联签记录。
#[derive(Clone)]
pub struct SignRec {
    pub vfs: u32,
    pub handle: u32,
    pub kind: u8,
}

impl SignRec {
    pub fn render(&self) -> String {
        format!("vfs{}|h{}|k{}", self.vfs, self.handle, self.kind)
    }
}

/// 联签账册。
pub struct SignBook {
    pub recs: Vec<SignRec>,
    pub sealed: bool,
}

impl SignBook {
    pub fn new() -> SignBook {
        SignBook { recs: Vec::new(), sealed: false }
    }

    fn by_vfs(&self, vfs: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.vfs == vfs)
    }

    fn by_handle(&self, handle: u32) -> Option<usize> {
        self.recs.iter().position(|r| r.handle == handle)
    }

    /// 联签：双侧非零、节点/句柄唯一。
    pub fn sign(&mut self, vfs: u32, handle: u32, kind: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(VS_FROZEN);
        }
        if vfs == 0 || handle == 0 || kind == 0 || kind > NK_DEV {
            return Err(VS_BAD_ID);
        }
        if self.by_handle(handle).is_some() || self.by_vfs(vfs).is_some() {
            return Err(VS_DUP);
        }
        self.recs.push(SignRec { vfs, handle, kind });
        Ok(())
    }

    /// 摘除（按句柄）。
    pub fn unsign(&mut self, handle: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(VS_FROZEN);
        }
        match self.by_handle(handle) {
            None => Err(VS_NOT_FOUND),
            Some(i) => {
                self.recs.remove(i);
                Ok(())
            }
        }
    }

    /// 双向核对。
    pub fn cross_check(&self, vfs: u32, handle: u32) -> bool {
        match (self.by_vfs(vfs), self.by_handle(handle)) {
            (Some(i), Some(j)) => i == j,
            _ => false,
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.vfs);
            h = h.wrapping_mul(211).wrapping_add(r.handle);
            h = h.wrapping_mul(211).wrapping_add(r.kind as u32);
        }
        h
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.recs.is_empty() {
            return Err(VS_UNSIGNED);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("signed={}", self.recs.len())
    }

    pub fn line_by_handle(&self, handle: u32) -> String {
        match self.by_handle(handle) {
            Some(i) => self.recs[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SignBook::new();
    // 1) 零节点号必拒。
    got[0] = b.sign(0, 1, NK_FILE).err() == Some(VS_BAD_ID);
    // 2) 零句柄号必拒。
    got[1] = b.sign(1, 0, NK_FILE).err() == Some(VS_BAD_ID);
    // 3) 句柄重号必拒。
    let _ = b.sign(1, 2, NK_FILE);
    got[2] = b.sign(3, 2, NK_FILE).err() == Some(VS_DUP);
    // 4) 空账 seal 必拒。
    got[3] = SignBook::new().seal().err() == Some(VS_UNSIGNED);
    // 5) 印账后联签必抓。
    b.seal();
    got[4] = b.sign(5, 5, NK_DIR).err() == Some(VS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12272_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12272");

    // J1 双侧在域。
    let j1 = {
        let mut b = SignBook::new();
        b.sign(0, 1, NK_FILE).err() == Some(VS_BAD_ID)
            && b.sign(1, 0, NK_FILE).err() == Some(VS_BAD_ID)
            && b.sign(1, 1, 0).err() == Some(VS_BAD_ID)
    };
    s.add("J1-双侧在域", j1, "零号/坏种类必拒");

    // J2 句柄唯一。
    let j2 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 10, NK_FILE);
        b.sign(2, 10, NK_FILE).err() == Some(VS_DUP)
    };
    s.add("J2-句柄唯一", j2, "同句柄拒");

    // J3 节点唯一。
    let j3 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 10, NK_FILE);
        b.sign(1, 11, NK_FILE).err() == Some(VS_DUP)
    };
    s.add("J3-节点唯一", j3, "同节点拒");

    // J4 双向核对。
    let j4 = {
        let mut b = SignBook::new();
        let _ = b.sign(7, 8, NK_DIR);
        b.cross_check(7, 8) && !b.cross_check(7, 9) && !b.cross_check(9, 8)
    };
    s.add("J4-双向核对", j4, "正反查询一致");

    // J5 未联签不可 seal。
    let j5 = SignBook::new().seal().err() == Some(VS_UNSIGNED);
    s.add("J5-空账seal拒", j5, "零记录必拒");

    // J6 印账冻结。
    let j6 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 1, NK_FILE);
        let _ = b.seal();
        b.sign(2, 2, NK_FILE).err() == Some(VS_FROZEN)
            && b.unsign(1).err() == Some(VS_FROZEN)
    };
    s.add("J6-印账冻结", j6, "seal 后签/摘皆拒");

    // J7 摘除一致。
    let j7 = {
        let mut b = SignBook::new();
        let _ = b.sign(4, 5, NK_FILE);
        let _ = b.unsign(5);
        b.cross_check(4, 5) == false
            && b.unsign(5).err() == Some(VS_NOT_FOUND)
    };
    s.add("J7-摘除一致", j7, "摘后反向落空");

    // J8 指纹幂等。
    let j8 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 2, NK_DEV);
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 回读渲染。
    let j9 = {
        let mut b = SignBook::new();
        let _ = b.sign(3, 4, NK_DEV);
        b.line_by_handle(4).contains("k3") && b.line_by_handle(9) == "missing"
    };
    s.add("J9-回读渲染", j9, "联签行可渲染");

    // J10 摘要回读。
    let j10 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 1, NK_FILE);
        let _ = b.sign(2, 2, NK_FILE);
        b.summary() == "signed=2"
    };
    s.add("J10-摘要回读", j10, "计数可回读");

    // J11 种类记账。
    let j11 = {
        let mut b = SignBook::new();
        let _ = b.sign(1, 1, NK_DIR);
        b.recs[0].kind == NK_DIR
    };
    s.add("J11-种类记账", j11, "种类入档");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = SignBook::new();
    let _ = b.sign(11, 21, NK_FILE);
    let _ = b.sign(12, 22, NK_DIR);
    let ok = b.cross_check(11, 21) && b.cross_check(12, 22);
    s.add("样例核对", ok, "双联签互查命中");
    s.add("摘要在册", b.summary().contains("signed=2"), "双行在册");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12272_sign_flow() {
        let mut b = SignBook::new();
        assert!(b.sign(1, 2, NK_FILE).is_ok());
        assert!(b.cross_check(1, 2));
        assert_eq!(b.sign(1, 3, NK_FILE), Err(VS_DUP));
        assert!(b.unsign(2).is_ok());
        assert!(!b.cross_check(1, 2));
        assert!(b.seal().is_ok());
        assert_eq!(b.sign(3, 4, NK_FILE), Err(VS_FROZEN));
    }

    #[test]
    fn unx12272_checks_green() {
        let s = run_unx12272_checks();
        assert!(s.all_passed(), "UNX-F12272 判据未全绿");
    }

    #[test]
    fn unx12272_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
