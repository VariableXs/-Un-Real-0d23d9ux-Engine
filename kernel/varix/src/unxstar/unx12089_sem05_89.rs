//! UNX-F12089 SecurityQualityOfService 字段语义（0x19d40 段）
//!
//! 卡面：OBJECT_ATTRIBUTES→SecurityQualityOfService——模拟级别闭集
//! （0..=3）、上下文跟踪模式闭集（动态/静态）、静态跟踪与委派级互斥、
//! 结构长度锚 12。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 长度锚：结构长度必为 12，违者必拒。
//! - J2 模拟级别闭集：级别 > 3 必拒。
//! - J3 跟踪模式闭集：模式 > 1 必拒。
//! - J4 静态×委派互斥：静态跟踪配委派级（3）必拒。
//! - J5 动态生效语义：动态跟踪时生效标志为真，静态时为假。
//! - J6 登记唯一：实例编号非零唯一。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：ImpLevel/TrackMode 双闭集 + SqosRec 档 + SqosBook 登记册 +
//! 折叠指纹（211 进制卷积）+ 注入核（坏长度/坏级别/坏模式/互斥/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19d40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19d40;
pub const SQ_BAD_LEN: u32 = SEG_BASE + 0x01; // 长度锚失配
pub const SQ_BAD_LEVEL: u32 = SEG_BASE + 0x02; // 级别越闭集
pub const SQ_BAD_MODE: u32 = SEG_BASE + 0x03; // 模式越闭集
pub const SQ_MODE_LEVEL: u32 = SEG_BASE + 0x04; // 静态×委派互斥
pub const SQ_DUP: u32 = SEG_BASE + 0x05; // 重号（含零号）
pub const SQ_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 结构长度锚。
pub const SQOS_LEN: u32 = 12;

/// 模拟级别闭集。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpLevel {
    Anonymous = 0,
    Identification = 1,
    Impersonation = 2,
    Delegation = 3,
}

/// 跟踪模式闭集。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackMode {
    Static = 0,
    Dynamic = 1,
}

/// SQOS 档。
#[derive(Clone, Copy)]
pub struct SqosRec {
    pub id: u32,
    pub length: u32,
    pub level: u32,
    pub mode: u32,
}

impl SqosRec {
    /// 校验核。
    pub fn validate(&self) -> Result<(), u32> {
        if self.length != SQOS_LEN {
            return Err(SQ_BAD_LEN);
        }
        if self.level > 3 {
            return Err(SQ_BAD_LEVEL);
        }
        if self.mode > 1 {
            return Err(SQ_BAD_MODE);
        }
        if self.mode == TrackMode::Static as u32
            && self.level == ImpLevel::Delegation as u32
        {
            return Err(SQ_MODE_LEVEL);
        }
        Ok(())
    }
    /// 动态跟踪生效判定。
    pub fn effective(&self) -> bool {
        self.mode == TrackMode::Dynamic as u32
    }
    pub fn render(&self) -> String {
        format!("Q{}|L{}|M{}", self.id, self.level, self.mode)
    }
}

/// SQOS 登记册。
pub struct SqosBook {
    pub recs: Vec<SqosRec>,
    pub sealed: bool,
}

impl SqosBook {
    pub fn new() -> SqosBook {
        SqosBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 校验核。
    pub fn put(&mut self, r: SqosRec) -> Result<(), u32> {
        if self.sealed {
            return Err(SQ_FROZEN);
        }
        if r.id == 0 || self.recs.iter().any(|x| x.id == r.id) {
            return Err(SQ_DUP);
        }
        r.validate()?;
        self.recs.push(r);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&SqosRec> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.length);
            h = h.wrapping_mul(211).wrapping_add(r.level);
            h = h.wrapping_mul(211).wrapping_add(r.mode);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.by_id(id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SqosBook::new();
    // 1) 坏长度必拒。
    got[0] = b.put(SqosRec { id: 1, length: 8, level: 1, mode: 1 })
        .err() == Some(SQ_BAD_LEN);
    // 2) 级别越闭集必拒。
    got[1] = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 4, mode: 1 })
        .err() == Some(SQ_BAD_LEVEL);
    // 3) 模式越闭集必拒。
    got[2] = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 1, mode: 2 })
        .err() == Some(SQ_BAD_MODE);
    // 4) 静态×委派互斥必拒。
    got[3] = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 3, mode: 0 })
        .err() == Some(SQ_MODE_LEVEL);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 1, mode: 1 })
        .err() == Some(SQ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12089_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12089");

    // J1 长度锚。
    let j1 = {
        let r = SqosRec { id: 1, length: 16, level: 1, mode: 1 };
        r.validate().err() == Some(SQ_BAD_LEN)
    };
    s.add("J1-长度锚", j1, "非 12 必拒");

    // J2 级别闭集。
    let j2 = {
        let r = SqosRec { id: 1, length: SQOS_LEN, level: 5, mode: 1 };
        r.validate().err() == Some(SQ_BAD_LEVEL)
    };
    s.add("J2-级别闭集", j2, "> 3 必拒");

    // J3 模式闭集。
    let j3 = {
        let r = SqosRec { id: 1, length: SQOS_LEN, level: 1, mode: 3 };
        r.validate().err() == Some(SQ_BAD_MODE)
    };
    s.add("J3-模式闭集", j3, "> 1 必拒");

    // J4 静态×委派互斥。
    let j4 = {
        let r = SqosRec { id: 1, length: SQOS_LEN, level: 3, mode: 0 };
        r.validate().err() == Some(SQ_MODE_LEVEL)
    };
    s.add("J4-静态委派互斥", j4, "同置必拒");

    // J5 动态生效语义。
    let j5 = {
        let dyn_r = SqosRec { id: 1, length: SQOS_LEN, level: 2, mode: 1 };
        let sta_r = SqosRec { id: 1, length: SQOS_LEN, level: 2, mode: 0 };
        dyn_r.effective() && !sta_r.effective()
    };
    s.add("J5-动态生效", j5, "动态真静态假");

    // J6 闭集边界放行。
    let j6 = {
        let r0 = SqosRec { id: 1, length: SQOS_LEN, level: 0, mode: 0 };
        let r3 = SqosRec { id: 1, length: SQOS_LEN, level: 3, mode: 1 };
        r0.validate().is_ok() && r3.validate().is_ok()
    };
    s.add("J6-边界放行", j6, "0 级静态与 3 级动态皆过");

    // J7 编号唯一。
    let j7 = {
        let mut b = SqosBook::new();
        let r = SqosRec { id: 2, length: SQOS_LEN, level: 1, mode: 1 };
        let _ = b.put(r);
        b.put(r).err() == Some(SQ_DUP)
    };
    s.add("J7-编号唯一", j7, "重号必拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = SqosBook::new();
        let _ = b.put(SqosRec { id: 4, length: SQOS_LEN, level: 2, mode: 1 });
        b.line_of(4).contains("L2") && b.line_of(5) == "missing"
    };
    s.add("J8-回读渲染", j8, "级别模式可回读");

    // J9 印账冻结。
    let j9 = {
        let mut b = SqosBook::new();
        b.seal();
        b.put(SqosRec { id: 1, length: SQOS_LEN, level: 1, mode: 1 })
            .err() == Some(SQ_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = SqosBook::new();
        let _ = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 1, mode: 1 });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = SqosBook::new();
    let _ = b.put(SqosRec { id: 1, length: SQOS_LEN, level: 2, mode: 1 });
    let _ = b.put(SqosRec { id: 2, length: SQOS_LEN, level: 0, mode: 0 });

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("生效分账", b.by_id(1).map(|r| r.effective()) == Some(true)
        && b.by_id(2).map(|r| r.effective()) == Some(false), "一动一静");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12089_validate_flow() {
        let good = SqosRec { id: 1, length: 12, level: 2, mode: 1 };
        assert!(good.validate().is_ok());
        let bad = SqosRec { level: 4, ..good };
        assert_eq!(bad.validate(), Err(SQ_BAD_LEVEL));
    }

    #[test]
    fn unx12089_checks_green() {
        let s = run_unx12089_checks();
        assert!(s.all_passed(), "UNX-F12089 判据未全绿");
    }

    #[test]
    fn level_mode_enums() {
        assert_eq!(ImpLevel::Delegation as u8, 3);
        assert_eq!(TrackMode::Dynamic as u8, 1);
    }
}
