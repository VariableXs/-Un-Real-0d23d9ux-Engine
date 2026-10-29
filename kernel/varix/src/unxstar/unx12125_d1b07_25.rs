//! UNX-F12125 UNW_FLAG_NHANDLER/EHANDLER/UHANDLER/CHAININFO 标志族（0x1a1c0 段）
//!
//! 卡面：展开标志族——NHANDLER（0）无处理器、EHANDLER（1）异常处理、
//! UHANDLER（2）终止处理、CHAININFO（4）链式展开；E/U 互斥、
//! CHAIN 与 E/U 互斥、族外位拒。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 族闭集：仅 0/1/2/4 位合法，族外位必拒。
//! - J2 E/U 互斥：EHANDLER 与 UHANDLER 同置必拒。
//! - J3 CHAIN 互斥：CHAININFO 与 E/U 同置必拒。
//! - J4 NHANDLER 口径：flags 为 0 即无处理器（合法）。
//! - J5 处理器数据对账：E/U 置位时处理器数据必须在场，CHAIN 时不在场。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：FlagSet 标志族 + FlagBook 登记册 + 互斥核 + 折叠指纹
//! （211 进制卷积）+ 注入核（族外位/EU 同置/链E同置/重号/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a1c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a1c0;
pub const UF_UNKNOWN: u32 = SEG_BASE + 0x01; // 族外位
pub const UF_EU_CLASH: u32 = SEG_BASE + 0x02; // E/U 同置
pub const UF_CHAIN_CLASH: u32 = SEG_BASE + 0x03; // CHAIN 与处理器同置
pub const UF_HANDLER_MISS: u32 = SEG_BASE + 0x04; // 处理器数据缺位
pub const UF_DUP: u32 = SEG_BASE + 0x05; // 重号
pub const UF_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 标志位。
pub const F_EHANDLER: u8 = 0x01;
pub const F_UHANDLER: u8 = 0x02;
pub const F_CHAININFO: u8 = 0x04;
/// 已知族掩码（低三位）。
pub const F_KNOWN: u8 = F_EHANDLER | F_UHANDLER | F_CHAININFO;

/// 标志族档。
#[derive(Clone, Copy)]
pub struct FlagSet {
    pub id: u32,
    pub flags: u8,
    pub has_handler_data: bool,
}

impl FlagSet {
    /// 校验核：族外位 → E/U 互斥 → CHAIN 互斥 → 处理器数据对账。
    pub fn validate(&self) -> Result<(), u32> {
        if self.flags & !F_KNOWN != 0 {
            return Err(UF_UNKNOWN);
        }
        if self.flags & F_EHANDLER != 0 && self.flags & F_UHANDLER != 0 {
            return Err(UF_EU_CLASH);
        }
        if self.flags & F_CHAININFO != 0 && self.flags & (F_EHANDLER | F_UHANDLER) != 0 {
            return Err(UF_CHAIN_CLASH);
        }
        let needs_handler = self.flags & (F_EHANDLER | F_UHANDLER) != 0;
        if needs_handler && !self.has_handler_data {
            return Err(UF_HANDLER_MISS);
        }
        if self.flags & F_CHAININFO != 0 && self.has_handler_data {
            return Err(UF_CHAIN_CLASH);
        }
        Ok(())
    }
    pub fn nhandler(&self) -> bool {
        self.flags == 0
    }
    pub fn render(&self) -> String {
        format!("UF{}|f{:x}|d{}", self.id, self.flags, self.has_handler_data as u8)
    }
}

/// 登记册。
pub struct FlagBook {
    pub recs: Vec<FlagSet>,
    pub sealed: bool,
}

impl FlagBook {
    pub fn new() -> FlagBook {
        FlagBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 校验核。
    pub fn put(&mut self, f: FlagSet) -> Result<(), u32> {
        if self.sealed {
            return Err(UF_FROZEN);
        }
        if f.id == 0 || self.recs.iter().any(|r| r.id == f.id) {
            return Err(UF_DUP);
        }
        f.validate()?;
        self.recs.push(f);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&FlagSet> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.flags as u32);
            h = h.wrapping_mul(211).wrapping_add(r.has_handler_data as u32);
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
    let mut b = FlagBook::new();
    // 1) 族外位必拒。
    got[0] = b.put(FlagSet { id: 1, flags: 0x08, has_handler_data: false })
        .err() == Some(UF_UNKNOWN);
    // 2) E/U 同置必拒。
    got[1] = b.put(FlagSet {
        id: 1, flags: F_EHANDLER | F_UHANDLER, has_handler_data: true,
    }).err() == Some(UF_EU_CLASH);
    // 3) CHAIN 与 E 同置必拒。
    got[2] = b.put(FlagSet {
        id: 1, flags: F_CHAININFO | F_EHANDLER, has_handler_data: false,
    }).err() == Some(UF_CHAIN_CLASH);
    // 4) E 置位而无处理器数据必拒。
    got[3] = b.put(FlagSet { id: 1, flags: F_EHANDLER, has_handler_data: false })
        .err() == Some(UF_HANDLER_MISS);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(FlagSet { id: 1, flags: 0, has_handler_data: false })
        .err() == Some(UF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12125");

    // J1 族闭集。
    let j1 = {
        let bad = FlagSet { id: 1, flags: 0x10, has_handler_data: false };
        bad.validate().err() == Some(UF_UNKNOWN)
    };
    s.add("J1-族闭集", j1, "0x10 越族必拒");

    // J2 E/U 互斥。
    let j2 = {
        let f = FlagSet {
            id: 1, flags: F_EHANDLER | F_UHANDLER, has_handler_data: true,
        };
        f.validate().err() == Some(UF_EU_CLASH)
    };
    s.add("J2-EU互斥", j2, "同置必拒");

    // J3 CHAIN 互斥（双面）。
    let j3 = {
        let f1 = FlagSet {
            id: 1, flags: F_CHAININFO | F_UHANDLER, has_handler_data: false,
        };
        let f2 = FlagSet { id: 1, flags: F_CHAININFO, has_handler_data: true };
        f1.validate().err() == Some(UF_CHAIN_CLASH)
            && f2.validate().err() == Some(UF_CHAIN_CLASH)
    };
    s.add("J3-CHAIN互斥", j3, "与处理器双面拒");

    // J4 NHANDLER 口径。
    let j4 = {
        let f = FlagSet { id: 1, flags: 0, has_handler_data: false };
        f.validate().is_ok() && f.nhandler()
    };
    s.add("J4-NHANDLER", j4, "零标志即无处理器");

    // J5 E 置位配数据放行。
    let j5 = {
        let f = FlagSet { id: 1, flags: F_EHANDLER, has_handler_data: true };
        f.validate().is_ok()
    };
    s.add("J5-E配数据", j5, "处理器数据在场放行");

    // J6 U 置位配数据放行。
    let j6 = {
        let f = FlagSet { id: 1, flags: F_UHANDLER, has_handler_data: true };
        f.validate().is_ok()
    };
    s.add("J6-U配数据", j6, "终止处理同口径");

    // J7 CHAIN 纯置放行。
    let j7 = {
        let f = FlagSet { id: 1, flags: F_CHAININFO, has_handler_data: false };
        f.validate().is_ok()
    };
    s.add("J7-CHAIN纯置", j7, "链式无处理器数据");

    // J8 编号唯一。
    let j8 = {
        let mut b = FlagBook::new();
        let f = FlagSet { id: 3, flags: 0, has_handler_data: false };
        let _ = b.put(f);
        b.put(f).err() == Some(UF_DUP)
    };
    s.add("J8-编号唯一", j8, "重号必拒");

    // J9 回读渲染。
    let j9 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 5, flags: F_EHANDLER, has_handler_data: true });
        b.line_of(5).contains("f1") && b.line_of(6) == "missing"
    };
    s.add("J9-回读渲染", j9, "标志行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut b = FlagBook::new();
        b.seal();
        b.put(FlagSet { id: 1, flags: 0, has_handler_data: false })
            .err() == Some(UF_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 1, flags: 0, has_handler_data: false });
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = FlagBook::new();
    let _ = b.put(FlagSet { id: 1, flags: F_EHANDLER, has_handler_data: true });
    let _ = b.put(FlagSet { id: 2, flags: F_CHAININFO, has_handler_data: false });

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12125_flag_flow() {
        let ok = FlagSet { id: 1, flags: F_UHANDLER, has_handler_data: true };
        assert!(ok.validate().is_ok());
        let clash = FlagSet {
            id: 2, flags: F_EHANDLER | F_UHANDLER, has_handler_data: true,
        };
        assert_eq!(clash.validate(), Err(UF_EU_CLASH));
    }

    #[test]
    fn unx12125_checks_green() {
        let s = run_unx12125_checks();
        assert!(s.all_passed(), "UNX-F12125 判据未全绿");
    }

    #[test]
    fn flag_mask() {
        assert_eq!(F_KNOWN, 0x07);
        assert_eq!(F_CHAININFO, 0x04);
    }
}
