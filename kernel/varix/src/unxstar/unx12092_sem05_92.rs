//! UNX-F12092 HANDLE_TABLE_ENTRY 权限掩码位（0x19da0 段）
//!
//! 卡面：HANDLE_TABLE_ENTRY 语义预告——低二位保留标签（bit0 锁定位/
//! bit1 继承位）、GENERIC 位族映射到具体权、零权限与保留位侵占显性拒。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 保留位净空：granted 侵占比特 0/1 必拒。
//! - J2 零权限拒：granted 为零必拒。
//! - J3 通用位映射：GENERIC_READ/WRITE/EXECUTE/ALL 映射具体权位集。
//! - J4 句柄唯一：同句柄二次登记必拒（含零句柄）。
//! - J5 掩码提取：granted & mask 提取与回读一致。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：HtEntry 表项 + HtEntryBook 表册 + 通用位映射 + 折叠指纹
//! （211 进制卷积）+ 注入核（零句柄/零权限/保留位侵占/重登/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19da0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19da0;
pub const TE_ZERO: u32 = SEG_BASE + 0x01; // 零句柄
pub const TE_NO_ACCESS: u32 = SEG_BASE + 0x02; // 零权限
pub const TE_RESERVED: u32 = SEG_BASE + 0x03; // 保留位侵占
pub const TE_DUP: u32 = SEG_BASE + 0x04; // 重登
pub const TE_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 保留标签位（bit0 锁定、bit1 继承）。
pub const TAG_MASK: u32 = 0x0000_0003;
/// 通用位。
pub const G_READ: u32 = 0x8000_0000;
pub const G_WRITE: u32 = 0x4000_0000;
pub const G_EXEC: u32 = 0x2000_0000;
pub const G_ALL: u32 = 0x1000_0000;
/// 具体权位（语义模型内自洽闭集）。
pub const R_QUERY: u32 = 0x0001_0000;
pub const R_SETINFO: u32 = 0x0002_0000;
pub const R_EXECUTE: u32 = 0x0020_0000;
pub const R_WRITE: u32 = 0x0010_0000;

/// 通用位 → 具体权映射。
pub fn generic_map(granted: u32) -> u32 {
    let mut spec = granted & !(G_READ | G_WRITE | G_EXEC | G_ALL);
    if granted & G_READ != 0 {
        spec |= R_QUERY;
    }
    if granted & G_WRITE != 0 {
        spec |= R_WRITE | R_SETINFO;
    }
    if granted & G_EXEC != 0 {
        spec |= R_EXECUTE;
    }
    if granted & G_ALL != 0 {
        spec |= R_QUERY | R_WRITE | R_SETINFO | R_EXECUTE;
    }
    spec
}

/// 表项。
#[derive(Clone, Copy)]
pub struct HtEntry {
    pub handle: u32,
    pub granted: u32,
}

impl HtEntry {
    /// 表项校验：零句柄 → 零权限 → 保留位净空。
    pub fn validate(&self) -> Result<(), u32> {
        if self.handle == 0 {
            return Err(TE_ZERO);
        }
        if self.granted == 0 {
            return Err(TE_NO_ACCESS);
        }
        if self.granted & TAG_MASK != 0 {
            return Err(TE_RESERVED);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("E{:x}|{:08x}", self.handle, self.granted)
    }
}

/// 表册。
pub struct HtEntryBook {
    pub recs: Vec<HtEntry>,
    pub sealed: bool,
}

impl HtEntryBook {
    pub fn new() -> HtEntryBook {
        HtEntryBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：句柄唯一 → 表项校验。
    pub fn put(&mut self, e: HtEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(TE_FROZEN);
        }
        if self.recs.iter().any(|r| r.handle == e.handle) {
            return Err(TE_DUP);
        }
        e.validate()?;
        self.recs.push(e);
        Ok(())
    }

    /// 权限掩码提取：通用位折算为具体权。
    pub fn access_of(&self, handle: u32) -> Option<u32> {
        self.recs.iter().find(|r| r.handle == handle).map(|r| generic_map(r.granted))
    }

    /// 权限足量判定：needed ⊆ mapped。
    pub fn grants(&self, handle: u32, needed: u32) -> Option<bool> {
        self.access_of(handle).map(|m| m & needed == needed)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.handle);
            h = h.wrapping_mul(211).wrapping_add(r.granted);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("entries={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, handle: u32) -> String {
        match self.recs.iter().find(|r| r.handle == handle) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = HtEntryBook::new();
    // 1) 零句柄必拒。
    got[0] = b.put(HtEntry { handle: 0, granted: R_QUERY }).err() == Some(TE_ZERO);
    // 2) 零权限必拒。
    got[1] = b.put(HtEntry { handle: 4, granted: 0 }).err() == Some(TE_NO_ACCESS);
    // 3) 保留位侵占必拒。
    got[2] = b.put(HtEntry { handle: 4, granted: R_QUERY | 0x2 })
        .err() == Some(TE_RESERVED);
    // 4) 重登必拒。
    let _ = b.put(HtEntry { handle: 8, granted: R_QUERY });
    got[3] = b.put(HtEntry { handle: 8, granted: R_WRITE }).err() == Some(TE_DUP);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(HtEntry { handle: 0x10, granted: R_QUERY }).err() == Some(TE_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12092_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12092");

    // J1 保留位净空。
    let j1 = {
        let e = HtEntry { handle: 4, granted: R_QUERY | 0x1 };
        e.validate().err() == Some(TE_RESERVED)
    };
    s.add("J1-保留位净空", j1, "bit0/1 侵占必拒");

    // J2 零权限拒。
    let j2 = {
        let e = HtEntry { handle: 4, granted: 0 };
        e.validate().err() == Some(TE_NO_ACCESS)
    };
    s.add("J2-零权限拒", j2, "granted 为零必拒");

    // J3 通用位映射。
    let j3 = {
        let m = generic_map(G_READ | G_WRITE);
        m & R_QUERY != 0 && m & R_WRITE != 0 && m & R_SETINFO != 0
            && m & R_EXECUTE == 0
    };
    s.add("J3-通用位映射", j3, "READ/WRITE 双向折算");

    // J4 G_ALL 全权映射。
    let j4 = {
        let m = generic_map(G_ALL);
        m == R_QUERY | R_WRITE | R_SETINFO | R_EXECUTE
    };
    s.add("J4-G_ALL 全权", j4, "四权齐发");

    // J5 掩码提取与足量判定。
    let j5 = {
        let mut b = HtEntryBook::new();
        let _ = b.put(HtEntry { handle: 0x10, granted: G_READ });
        b.grants(0x10, R_QUERY) == Some(true)
            && b.grants(0x10, R_WRITE) == Some(false)
    };
    s.add("J5-足量判定", j5, "needed ⊆ mapped");

    // J6 零句柄拒。
    let j6 = {
        let e = HtEntry { handle: 0, granted: R_QUERY };
        e.validate().err() == Some(TE_ZERO)
    };
    s.add("J6-零句柄拒", j6, "零号必拒");

    // J7 重登拒。
    let j7 = {
        let mut b = HtEntryBook::new();
        let e = HtEntry { handle: 8, granted: R_QUERY };
        let _ = b.put(e);
        b.put(e).err() == Some(TE_DUP)
    };
    s.add("J7-重登拒", j7, "同句柄二次必拒");

    // J8 渲染行。
    let j8 = {
        let mut b = HtEntryBook::new();
        let _ = b.put(HtEntry { handle: 0x1c, granted: R_QUERY });
        b.line_of(0x1c).contains("00010000") && b.line_of(0x20) == "missing"
    };
    s.add("J8-渲染行", j8, "权位十六进制回读");

    // J9 印账冻结。
    let j9 = {
        let mut b = HtEntryBook::new();
        b.seal();
        b.put(HtEntry { handle: 4, granted: R_QUERY }).err() == Some(TE_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = HtEntryBook::new();
        let _ = b.put(HtEntry { handle: 4, granted: R_QUERY });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = HtEntryBook::new();
    let _ = b.put(HtEntry { handle: 4, granted: G_READ | R_WRITE });
    let _ = b.put(HtEntry { handle: 8, granted: G_ALL });

    s.add("摘要回读", b.summary().contains("entries=2"), "双项在册");
    s.add("映射回读", b.access_of(4).map(|m| m & R_QUERY != 0) == Some(true), "GENERIC_READ 折算");
    s.add("全权足量", b.grants(8, R_EXECUTE) == Some(true), "G_ALL 足量");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12092_entry_flow() {
        let e = HtEntry { handle: 4, granted: R_QUERY };
        assert!(e.validate().is_ok());
        assert!(e.render().contains("0004"));
        let bad = HtEntry { granted: R_QUERY | 0x3, ..e };
        assert_eq!(bad.validate(), Err(TE_RESERVED));
    }

    #[test]
    fn unx12092_checks_green() {
        let s = run_unx12092_checks();
        assert!(s.all_passed(), "UNX-F12092 判据未全绿");
    }

    #[test]
    fn generic_map_helpers() {
        assert_eq!(generic_map(G_EXEC), R_EXECUTE);
        assert_eq!(generic_map(R_QUERY), R_QUERY);
    }
}
