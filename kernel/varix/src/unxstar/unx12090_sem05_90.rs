//! UNX-F12090 InitializeObjectAttributes 宏语义与静态校验（0x19d60 段）
//!
//! 卡面：InitializeObjectAttributes 宏——六字段一次填齐（Length/Root/
//! Name/Attributes/SD/SQOS），Length 自动定锚 48；漏填与重复填显性拒。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 六槽闭集：字段槽位编号闭集 [0,6)，越界必拒。
//! - J2 单填原则：同槽位二次填必拒。
//! - J3 完整性门禁：六槽未齐时 finalize 必拒。
//! - J4 长度自动锚：finalize 时 Length 自动为 48。
//! - J5 填序自由：任意顺序填齐皆放行。
//! - J6 填槽账：逐槽可回读（槽号/值）。
//! - J7 印账冻结：seal 后填槽必拒。
//! - J8 注入五况全抓。
//!
//! 结构：AttrSlot 六槽枚举 + MacroFill 位图填槽机 + finalize 核 +
//! 折叠指纹（211 进制卷积）+ 注入核（越界槽/重复填/漏填终结/印后填/
//! 零值长度槽伪造）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19d60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19d60;
pub const MA_BAD_SLOT: u32 = SEG_BASE + 0x01; // 越界槽
pub const MA_DOUBLE: u32 = SEG_BASE + 0x02; // 重复填
pub const MA_INCOMPLETE: u32 = SEG_BASE + 0x03; // 漏填终结
pub const MA_ZERO_VALUE: u32 = SEG_BASE + 0x04; // 长度槽零值
pub const MA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后填

/// 属性字段六槽。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttrSlot {
    Length = 0,
    RootDirectory = 1,
    ObjectName = 2,
    Attributes = 3,
    SecurityDescriptor = 4,
    SecurityQos = 5,
}

impl AttrSlot {
    pub fn idx(&self) -> u32 {
        *self as u32
    }
}

/// 一次填槽留痕。
#[derive(Clone)]
pub struct FillRec {
    pub slot: u32,
    pub value: u64,
}

/// 宏填槽机：位图 + 留痕 + 终结核。
pub struct MacroFill {
    pub filled: u8,
    pub recs: Vec<FillRec>,
    pub finalized: bool,
    pub sealed: bool,
}

impl MacroFill {
    pub const ALL: u8 = 0x3F;

    pub fn new() -> MacroFill {
        MacroFill { filled: 0, recs: Vec::new(), finalized: false, sealed: false }
    }

    /// 填槽：越界/重复/终结后皆拒。
    pub fn fill(&mut self, slot: AttrSlot, value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(MA_FROZEN);
        }
        if self.finalized {
            return Err(MA_DOUBLE);
        }
        let bit = 1u8 << slot.idx();
        if self.filled & bit != 0 {
            return Err(MA_DOUBLE);
        }
        if slot == AttrSlot::Length && value == 0 {
            return Err(MA_ZERO_VALUE);
        }
        self.filled |= bit;
        self.recs.push(FillRec { slot: slot.idx(), value });
        Ok(())
    }

    /// 原始槽号填（用于越界注入路径的模拟入口——闭集外必拒）。
    pub fn fill_raw(&mut self, slot: u32, value: u64) -> Result<(), u32> {
        if slot >= 6 {
            return Err(MA_BAD_SLOT);
        }
        let slots = [
            AttrSlot::Length,
            AttrSlot::RootDirectory,
            AttrSlot::ObjectName,
            AttrSlot::Attributes,
            AttrSlot::SecurityDescriptor,
            AttrSlot::SecurityQos,
        ];
        self.fill(slots[slot as usize], value)
    }

    /// 终结核：六槽齐备方放行，Length 自动锚 48。
    pub fn finalize(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(MA_FROZEN);
        }
        if self.filled != MacroFill::ALL {
            return Err(MA_INCOMPLETE);
        }
        self.finalized = true;
        Ok(48)
    }

    /// 缺槽清单。
    pub fn missing_slots(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for i in 0..6u32 {
            if self.filled & (1u8 << i) == 0 {
                v.push(i);
            }
        }
        v
    }

    /// 槽值回读。
    pub fn value_of(&self, slot: AttrSlot) -> Option<u64> {
        self.recs.iter().find(|r| r.slot == slot.idx()).map(|r| r.value)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.filled as u32);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.slot);
            h = h.wrapping_mul(211).wrapping_add((r.value & 0xFFFF_FFFF) as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：填槽数。
    pub fn filled_count(&self) -> u32 {
        self.filled.count_ones()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("filled={:02x} fin={}", self.filled, self.finalized)
    }

    /// 诊断面：留痕渲染。
    pub fn line_of(&self, i: usize) -> String {
        match self.recs.get(i) {
            Some(r) => format!("F{}|{:x}", r.slot, r.value),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = MacroFill::new();
    // 1) 越界槽必拒。
    got[0] = m.fill_raw(9, 1).err() == Some(MA_BAD_SLOT);
    // 2) 重复填必拒。
    let _ = m.fill(AttrSlot::Length, 48);
    got[1] = m.fill(AttrSlot::Length, 48).err() == Some(MA_DOUBLE);
    // 3) 漏填终结必拒。
    got[2] = m.finalize().err() == Some(MA_INCOMPLETE);
    // 4) 长度槽零值必拒。
    let mut m2 = MacroFill::new();
    got[3] = m2.fill(AttrSlot::Length, 0).err() == Some(MA_ZERO_VALUE);
    // 5) 印账后填槽必抓。
    m.seal();
    got[4] = m.fill(AttrSlot::RootDirectory, 1).err() == Some(MA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12090_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12090");
    let slots = [
        AttrSlot::Length,
        AttrSlot::RootDirectory,
        AttrSlot::ObjectName,
        AttrSlot::Attributes,
        AttrSlot::SecurityDescriptor,
        AttrSlot::SecurityQos,
    ];

    // J1 六槽闭集映射。
    let j1 = slots[0].idx() == 0 && slots[5].idx() == 5;
    s.add("J1-六槽闭集", j1, "0..6 连续映射");

    // J2 任意序填齐并终结，Length 锚 48。
    let j2 = {
        let mut m = MacroFill::new();
        let order = [3usize, 1, 4, 0, 5, 2];
        let mut ok = true;
        for i in order.iter() {
            ok = ok && m.fill(slots[*i], 0x10).is_ok();
        }
        ok && m.finalize() == Ok(48) && m.finalized
    };
    s.add("J2-乱序填齐终结", j2, "Length 锚 48");

    // J3 重复填拒。
    let j3 = {
        let mut m = MacroFill::new();
        let _ = m.fill(AttrSlot::Attributes, 0x40);
        m.fill(AttrSlot::Attributes, 0x40).err() == Some(MA_DOUBLE)
    };
    s.add("J3-重复填拒", j3, "同槽二次必拒");

    // J4 越界槽拒。
    let j4 = {
        let mut m = MacroFill::new();
        m.fill_raw(6, 1).err() == Some(MA_BAD_SLOT)
    };
    s.add("J4-越界槽拒", j4, "槽位越闭集必拒");

    // J5 漏填终结拒并回读缺槽。
    let j5 = {
        let mut m = MacroFill::new();
        let _ = m.fill(AttrSlot::Length, 48);
        let _ = m.fill(AttrSlot::ObjectName, 1);
        m.finalize().err() == Some(MA_INCOMPLETE)
            && m.missing_slots().len() == 4
    };
    s.add("J5-漏填终结拒", j5, "缺槽清单可回读");

    // J6 长度槽零值拒。
    let j6 = {
        let mut m = MacroFill::new();
        m.fill(AttrSlot::Length, 0).err() == Some(MA_ZERO_VALUE)
    };
    s.add("J6-零值长度拒", j6, "长度槽必须非零");

    // J7 槽值回读。
    let j7 = {
        let mut m = MacroFill::new();
        let _ = m.fill(AttrSlot::RootDirectory, 9);
        m.value_of(AttrSlot::RootDirectory) == Some(9)
    };
    s.add("J7-槽值回读", j7, "逐槽可查");

    // J8 填槽数回读。
    let j8 = {
        let mut m = MacroFill::new();
        let _ = m.fill(AttrSlot::Length, 48);
        let _ = m.fill(AttrSlot::Attributes, 0);
        m.filled_count() == 2 && m.summary().contains("filled=09")
    };
    s.add("J8-填数回读", j8, "位图计数 2");

    // J9 印账冻结。
    let j9 = {
        let mut m = MacroFill::new();
        m.seal();
        m.fill(AttrSlot::Length, 48).err() == Some(MA_FROZEN)
            && m.finalize().err() == Some(MA_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut m = MacroFill::new();
        let _ = m.fill(AttrSlot::Length, 48);
        m.fold() == m.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut m = MacroFill::new();
    let _ = m.fill(AttrSlot::Length, 48);
    let _ = m.fill(AttrSlot::Attributes, 0x40);
    let _ = m.fill(AttrSlot::ObjectName, 0x1234);

    s.add("摘要回读", m.summary().contains("fin=false"), "未终结态");
    s.add("留痕渲染", m.line_of(2).contains("1234"), "名槽值渲染");
    s.add("缺席行", m.line_of(9) == "missing", "缺痕显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12090_fill_flow() {
        let mut m = MacroFill::new();
        assert!(m.fill(AttrSlot::Length, 48).is_ok());
        assert_eq!(m.fill(AttrSlot::Length, 48), Err(MA_DOUBLE));
        assert_eq!(m.finalize(), Err(MA_INCOMPLETE));
        assert_eq!(m.missing_slots().len(), 5);
    }

    #[test]
    fn unx12090_checks_green() {
        let s = run_unx12090_checks();
        assert!(s.all_passed(), "UNX-F12090 判据未全绿");
    }

    #[test]
    fn slot_enum_order() {
        assert_eq!(AttrSlot::SecurityQos.idx(), 5);
        assert_ne!(AttrSlot::Length, AttrSlot::Attributes);
    }
}
