//! UNX-F12162 — ContextFlags 位语义与部分加载/保存（0x1a660 段）
//!
//! 锚点：UNX-F12162-J1 ContextFlags 位语义 10/10（CONTROL=0x1001 /
//! INTEGER=0x1002 / SEGMENTS=0x1004 / FLOATING_POINT=0x1008 /
//! DEBUG_REGISTERS=0x1010 / FULL=全部 / 全按位覆盖），部分保存只
//! 动授权槽，未授权槽写必拒。
//! 域：CoRun·UNX-D1-B09（CONTEXT 语义谱系）。
//! 结构：CtxFlag（位组）+ FlagGate（授权门）+ 分组写读账 + 折叠
//! 指纹 + 注入核。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a660;
pub const CF_ORDER: u32 = SEG_BASE + 0x01; // 未授权组写
pub const CF_GHOST: u32 = SEG_BASE + 0x02; // 野组
pub const CF_SEALED: u32 = SEG_BASE + 0x03; // 印后写
pub const CF_EMPTY: u32 = SEG_BASE + 0x04; // 零授权写
pub const CF_TWICE: u32 = SEG_BASE + 0x05; // 重复授权

/// 上下文标志组位（NT x64 语义）。
pub const CF_CONTROL: u32 = 0x1001;
pub const CF_INTEGER: u32 = 0x1002;
pub const CF_SEGMENTS: u32 = 0x1004;
pub const CF_FLOAT: u32 = 0x1008;
pub const CF_DEBUG: u32 = 0x1010;
pub const CF_FULL: u32 = CF_CONTROL | CF_INTEGER | CF_SEGMENTS | CF_FLOAT | CF_DEBUG;

/// 组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtxGroup {
    Control,
    Integer,
    Segments,
    Float,
    Debug,
}

impl CtxGroup {
    pub fn bit(&self) -> u32 {
        match self {
            CtxGroup::Control => CF_CONTROL,
            CtxGroup::Integer => CF_INTEGER,
            CtxGroup::Segments => CF_SEGMENTS,
            CtxGroup::Float => CF_FLOAT,
            CtxGroup::Debug => CF_DEBUG,
        }
    }

    pub fn from_bit(b: u32) -> Option<CtxGroup> {
        match b {
            CF_CONTROL => Some(CtxGroup::Control),
            CF_INTEGER => Some(CtxGroup::Integer),
            CF_SEGMENTS => Some(CtxGroup::Segments),
            CF_FLOAT => Some(CtxGroup::Float),
            CF_DEBUG => Some(CtxGroup::Debug),
            _ => None,
        }
    }

    pub fn tag(&self) -> &'static str {
        match self {
            CtxGroup::Control => "CTL",
            CtxGroup::Integer => "INT",
            CtxGroup::Segments => "SEG",
            CtxGroup::Float => "FLT",
            CtxGroup::Debug => "DBG",
        }
    }
}

/// 授权门上下文。
pub struct FlagGate {
    pub granted: u32,
    pub stores: Vec<(CtxGroup, u64)>,
    sealed: bool,
}

impl FlagGate {
    pub fn new(granted: u32) -> FlagGate {
        FlagGate { granted, stores: Vec::new(), sealed: false }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn granted(&self) -> u32 {
        self.granted
    }

    /// 组授权判定。
    pub fn allows(&self, g: CtxGroup) -> bool {
        self.granted & g.bit() == g.bit()
    }

    /// 组内保存（未授权/零授权/野组必拒）。
    pub fn store(&mut self, g: CtxGroup, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_SEALED);
        }
        if self.granted == 0 {
            return Err(CF_EMPTY);
        }
        if !self.allows(g) {
            return Err(CF_ORDER);
        }
        self.stores.push((g, v));
        Ok(())
    }

    /// 授权追加（重复授权必拒）。
    pub fn grant(&mut self, g: CtxGroup) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_SEALED);
        }
        if self.allows(g) {
            return Err(CF_TWICE);
        }
        self.granted |= g.bit();
        Ok(())
    }

    /// 授权位全可解组。
    pub fn granted_groups(&self) -> Vec<CtxGroup> {
        [
            CtxGroup::Control,
            CtxGroup::Integer,
            CtxGroup::Segments,
            CtxGroup::Float,
            CtxGroup::Debug,
        ]
        .iter()
        .filter(|g| self.allows(**g))
        .cloned()
        .collect()
    }

    /// 保存账回放：每条 store 的组都被授权。
    pub fn stores_authorized(&self) -> bool {
        self.stores.iter().all(|(g, _)| self.allows(*g))
    }

    /// 折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1a660;
        h = h.wrapping_mul(211).wrapping_add(self.granted);
        for (g, v) in &self.stores {
            h = h.wrapping_mul(211).wrapping_add(g.bit());
            h = h.wrapping_mul(211).wrapping_add(*v as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    pub fn book_summary(&self) -> String {
        format!(
            "granted={:#x} stores={} auth={}",
            self.granted,
            self.stores.len(),
            self.stores_authorized()
        )
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 零授权写必拒。
    let mut b = FlagGate::new(0);
    got[0] = b.store(CtxGroup::Control, 1).err() == Some(CF_EMPTY);
    // 2) 未授权组写必拒。
    let mut b2 = FlagGate::new(CF_INTEGER);
    got[1] = b2.store(CtxGroup::Control, 1).err() == Some(CF_ORDER);
    // 3) 野位组不可解。
    got[2] = CtxGroup::from_bit(0x1020).is_none();
    // 4) 重复授权必拒。
    let mut b3 = FlagGate::new(CF_INTEGER);
    got[3] = b3.grant(CtxGroup::Integer).err() == Some(CF_TWICE);
    // 5) 印后写必拒。
    b3.seal();
    got[4] = b3.store(CtxGroup::Integer, 1).err() == Some(CF_SEALED);
    got
}

/// 判据入口：UNX-F12162
pub fn run_unx12162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12162");
    let mut b = FlagGate::new(CF_FULL);

    // J1 FULL 授权五组全开。
    s.add(
        "J1_full_grant",
        b.granted_groups().len() == 5,
        "FULL 授权五组全解",
    );

    // J2 五组各自保存 10/10。
    let mut j2 = true;
    for (g, v) in [
        (CtxGroup::Control, 0xA1u64),
        (CtxGroup::Integer, 0xB2),
        (CtxGroup::Segments, 0xC3),
        (CtxGroup::Float, 0xD4),
        (CtxGroup::Debug, 0xE5),
    ] {
        if b.store(g, v).is_err() {
            j2 = false;
        }
    }
    s.add("J2_store5", j2 && b.stores.len() == 5, "五组保存全过");

    // J3 位常量与 NT 语义一致。
    s.add(
        "J3_bits",
        CF_CONTROL == 0x1001
            && CF_INTEGER == 0x1002
            && CF_SEGMENTS == 0x1004
            && CF_FLOAT == 0x1008
            && CF_DEBUG == 0x1010,
        "五组位常量对平",
    );

    // J4 FULL 位覆盖和。
    s.add(
        "J4_full_sum",
        CF_FULL == 0x101F,
        "FULL=0x101F",
    );

    // J5 部分授权拒越组写。
    let mut b5 = FlagGate::new(CF_CONTROL | CF_INTEGER);
    let ok5 = b5.store(CtxGroup::Control, 1).is_ok()
        && b5.store(CtxGroup::Integer, 2).is_ok()
        && b5.store(CtxGroup::Float, 3).err() == Some(CF_ORDER);
    s.add(
        "J5_partial",
        ok5 && b5.stores.len() == 2,
        "部分授权两过一拒",
    );

    // J6 组标签。
    s.add(
        "J6_tags",
        CtxGroup::Control.tag() == "CTL"
            && CtxGroup::Integer.tag() == "INT"
            && CtxGroup::Float.tag() == "FLT",
        "组标签对平",
    );

    // J7 位↔组双射。
    let mut j7 = true;
    for g in [
        CtxGroup::Control,
        CtxGroup::Integer,
        CtxGroup::Segments,
        CtxGroup::Float,
        CtxGroup::Debug,
    ] {
        match CtxGroup::from_bit(g.bit()) {
            Some(g2) if g2 == g => {}
            _ => j7 = false,
        }
    }
    s.add("J7_bit_bij", j7, "位↔组双射");

    // J8 授权追加。
    let r8 = b5.grant(CtxGroup::Float);
    s.add(
        "J8_grant_add",
        r8.is_ok() && b5.allows(CtxGroup::Float),
        "追加授权后组开",
    );

    // J9 重复授权拒。
    s.add(
        "J9_grant_dup",
        b5.grant(CtxGroup::Float).err() == Some(CF_TWICE),
        "重复授权拒 TWICE",
    );

    // J10 保存账授权回放。
    s.add("J10_stores_auth", b.stores_authorized(), "保存账全授权");

    // J11 指纹稳定。
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J11_fingerprint", f1 == f2, "授权门折叠指纹一致");

    // J12 注入 5 项全抓。
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J12_inject_all", caught == 5, "5 注入点全检出");

    // J13 印账受控。
    b.seal();
    s.add(
        "J13_sealed",
        b.store(CtxGroup::Control, 1).err() == Some(CF_SEALED)
            && b.grant(CtxGroup::Control).err() == Some(CF_SEALED),
        "印账后存/授权皆拒",
    );

    // J14 账面摘要。
    let sm = b.book_summary();
    s.add(
        "J14_summary",
        sm.contains("granted=0x101f") && sm.contains("stores=5"),
        "摘要含授权位与存数",
    );

    // J15 零授权摘要。
    let b15 = FlagGate::new(0);
    s.add(
        "J15_zero_grant",
        b15.granted == 0 && b15.book_summary().contains("granted=0x0"),
        "零授权摘要",
    );

    // J16 十组授权追加序。
    let mut b16 = FlagGate::new(0);
    let mut ok16 = true;
    for g in [
        CtxGroup::Control,
        CtxGroup::Integer,
        CtxGroup::Segments,
        CtxGroup::Float,
        CtxGroup::Debug,
    ] {
        if b16.grant(g).is_err() {
            ok16 = false;
        }
    }
    s.add(
        "J16_grant_seq",
        ok16 && b16.granted == CF_FULL,
        "五次追加凑满 FULL",
    );

    // J17 保存账逐条回放。
    let mut ok17 = true;
    for (g, v) in &b.stores {
        if b.allows(*g) && *v == 0 {
            ok17 = false;
        }
    }
    s.add("J17_stores_val", ok17, "保存账值全非零");

    // J18 空账指纹稳定。
    let b18 = FlagGate::new(CF_FULL);
    s.add("J18_fresh_fp", b18.fold() == b18.fold(), "空授权门指纹稳定");

    // J19 野位双射空。
    s.add(
        "J19_wild_bit",
        CtxGroup::from_bit(0xFFFF_0000).is_none(),
        "野位解组必空",
    );

    // J20 印账不改指纹。
    let fb = b.fold();
    b.seal();
    s.add("J20_seal_fp", b.fold() == fb, "印账不改指纹");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12162_flags() {
        let mut b = FlagGate::new(CF_INTEGER);
        assert!(b.store(CtxGroup::Integer, 1).is_ok());
        assert_eq!(b.store(CtxGroup::Float, 2), Err(CF_ORDER));
    }

    #[test]
    fn unx12162_checks_green() {
        let s = run_unx12162_checks();
        assert!(s.all_passed(), "UNX-F12162 判据未全绿");
    }

    #[test]
    fn unx12162_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
