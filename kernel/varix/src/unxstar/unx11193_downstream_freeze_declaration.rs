//! UNX-F11193 下游冻结声明账（C4 冻结接口移交声明）（0x1f5e0 段）
//!
//! 纯功能模型：C4 面每个冻结件向移交接管方出具一份声明——声明值须等于该件
//! 的规范冻结值；每份声明须挂至少一条消费指南，指南引用不得悬空（指向未声明
//! 件即悬空）。逐件声明对平且指南零悬空方可封账，封账后声明不可改写。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f5e0;
/// 件号越界
pub const DL_EITEM: u32 = SEG_BASE + 0x01;
/// 重复声明
pub const DL_EDUP: u32 = SEG_BASE + 0x02;
/// 声明值不符规范冻结值
pub const DL_EVALUE: u32 = SEG_BASE + 0x03;
/// 指南引用未声明件（悬空）
pub const DL_EDANGLING: u32 = SEG_BASE + 0x04;
/// 重复消费方
pub const DL_EGUIDE: u32 = SEG_BASE + 0x05;
/// 封账后改写
pub const DL_EFROZEN: u32 = SEG_BASE + 0x06;
/// 声明缺指南
pub const DL_EBARE: u32 = SEG_BASE + 0x07;

/// 冻结件数
pub const ITEMS: usize = 10;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一件冻结件的规范冻结值（声明须与之相等）。
pub fn canonical_value(no: u16) -> Result<u64, u32> {
    if (no as usize) >= ITEMS {
        return Err(DL_EITEM);
    }
    let mut h = FNV_OFF;
    h = (h ^ 0xf000_0000u64).wrapping_mul(FNV_PRIME);
    h = (h ^ no as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ 0x5a5a_5a5au64).wrapping_mul(FNV_PRIME);
    Ok(h)
}

/// 一份冻结声明：值 + 消费指南引用表。
#[derive(Clone, Debug)]
pub struct FreezeDecl {
    pub no: u16,
    pub value: u64,
    pub guides: Vec<u16>,
}

impl FreezeDecl {
    fn new(no: u16, value: u64) -> Self {
        FreezeDecl { no, value, guides: Vec::new() }
    }

    /// 指南条数。
    pub fn guide_count(&self) -> u64 {
        self.guides.len() as u64
    }

    /// 是否已挂指南。
    pub fn covered(&self) -> bool {
        !self.guides.is_empty()
    }

    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.value).wrapping_mul(FNV_PRIME);
        let mut i = 0;
        while i < self.guides.len() {
            h = (h ^ self.guides[i] as u64).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h = (h ^ self.guides.len() as u64).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 下游冻结声明账。
#[derive(Clone, Debug)]
pub struct DownstreamLedger {
    decls: Vec<FreezeDecl>,
    sealed: Option<u64>,
    pub declares: u64,
    pub guides: u64,
    pub rejects: u64,
}

impl DownstreamLedger {
    /// 空账。
    pub fn new() -> Self {
        DownstreamLedger { decls: Vec::new(), sealed: None, declares: 0, guides: 0, rejects: 0 }
    }

    /// 定位声明下标。
    fn locate(&self, no: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.decls.len() {
            if self.decls[i].no == no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

}

impl DownstreamLedger {
    fn guard(&self) -> Result<(), u32> {
        if self.sealed.is_some() {
            return Err(DL_EFROZEN);
        }
        Ok(())
    }

    /// 出具声明：值须等于规范冻结值，同件不得重复声明。
    pub fn declare(&mut self, no: u16, value: u64) -> Result<u64, u32> {
        self.guard()?;
        let canon = canonical_value(no)?;
        if self.locate(no).is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EDUP);
        }
        if value != canon {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EVALUE);
        }
        self.decls.push(FreezeDecl::new(no, value));
        self.declares = self.declares.wrapping_add(1);
        Ok(self.declares)
    }

    /// 挂一条消费指南：被引件须已声明，同一消费方不得重复挂。
    pub fn attach_guide(&mut self, no: u16, consumer: u16) -> Result<u64, u32> {
        self.guard()?;
        // 消费方须是已声明的冻结件，否则指南悬空。
        if (consumer as usize) >= ITEMS {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EITEM);
        }
        if self.locate(consumer).is_none() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EDANGLING);
        }
        let ix = match self.locate(no) {
            Some(ix) => ix,
            None => {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(DL_EDANGLING);
            }
        };
        if let Some(d) = self.decls.get_mut(ix) {
            if d.guides.contains(&consumer) {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(DL_EGUIDE);
            }
            d.guides.push(consumer);
            self.guides = self.guides.wrapping_add(1);
            return Ok(self.guides);
        }
        Err(DL_EITEM)
    }

    /// 取一件声明。
    pub fn decl(&self, no: u16) -> Result<&FreezeDecl, u32> {
        match self.locate(no) {
            Some(ix) => match self.decls.get(ix) {
                Some(d) => Ok(d),
                None => Err(DL_EITEM),
            },
            None => Err(DL_EDANGLING),
        }
    }

    /// 已声明件数。
    pub fn declared(&self) -> u64 {
        self.decls.len() as u64
    }

    /// 指南条数。
    pub fn guide_total(&self) -> u64 {
        self.guides
    }

    /// 无指南之声明数（指南覆盖缺口）。
    pub fn bare(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.decls.len() {
            if !self.decls[i].covered() {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }

    /// 悬空指南数：指南引用的件号不存在或未声明。
    pub fn dangling(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.decls.len() {
            let d = &self.decls[i];
            let mut j = 0;
            while j < d.guides.len() {
                let target = d.guides[j];
                if (target as usize) >= ITEMS || self.locate(target).is_none() {
                    n = n.wrapping_add(1);
                }
                j += 1;
            }
            i += 1;
        }
        n
    }

    /// 逐件声明是否对平（值等于规范冻结值）。
    pub fn paired(&self) -> bool {
        if self.decls.is_empty() {
            return false;
        }
        let mut i = 0;
        while i < self.decls.len() {
            let d = &self.decls[i];
            match canonical_value(d.no) {
                Ok(c) if c == d.value => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 封账：逐件对平、指南零悬空、无裸声明方可封。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed.is_some() {
            return Err(DL_EFROZEN);
        }
        if !self.paired() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EVALUE);
        }
        if self.dangling() > 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EDANGLING);
        }
        if self.bare() > 0 {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(DL_EBARE);
        }
        let v = self.fold();
        self.sealed = Some(v);
        Ok(v)
    }

    /// 账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.decls.len() {
            h = (h ^ self.decls[i].fold()).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h = (h ^ self.declares).wrapping_mul(FNV_PRIME);
        h = (h ^ self.guides).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（封账即冻结；未封也可取当前指纹作占位）。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.sealed.is_none() {
            self.sealed = Some(self.fold());
        }
        self.sealed
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed.is_some()
    }
}

/// 标准装配：十件逐件声明并各挂两条消费指南后封账。
pub fn build_downstream() -> (DownstreamLedger, bool) {
    let mut l = DownstreamLedger::new();
    let mut no = 0u16;
    while (no as usize) < ITEMS {
        let v = match canonical_value(no) {
            Ok(v) => v,
            Err(_) => return (l, false),
        };
        if l.declare(no, v).is_err() {
            return (l, false);
        }
        no = no.wrapping_add(1);
    }
    let mut n2 = 0u16;
    while (n2 as usize) < ITEMS {
        let c1 = (((n2 as usize) + 1) % ITEMS) as u16;
        let c2 = (((n2 as usize) + 2) % ITEMS) as u16;
        if l.attach_guide(n2, c1).is_err() || l.attach_guide(n2, c2).is_err() {
            return (l, false);
        }
        n2 = n2.wrapping_add(1);
    }
    let ok = l.seal().is_ok();
    (l, ok)
}

/// 四类注入：件号越界 / 声明值不符 / 指南悬空 / 裸声明封账，全部须拒止。
pub fn inject_four() -> [bool; 4] {
    // ① 件号越界
    let mut l0 = DownstreamLedger::new();
    let a = l0.declare(ITEMS as u16, 1).err() == Some(DL_EITEM)
        && canonical_value(ITEMS as u16).is_err();

    // ② 声明值不符规范冻结值
    let mut l1 = DownstreamLedger::new();
    let b = l1.declare(0, 0x1234).err() == Some(DL_EVALUE) && l1.declared() == 0;

    // ③ 指南悬空：向未声明件挂指南
    let mut l2 = DownstreamLedger::new();
    let c = l2.attach_guide(3, 7).err() == Some(DL_EDANGLING) && l2.guide_total() == 0;

    // ④ 裸声明封账
    let mut l3 = DownstreamLedger::new();
    let mut no = 0u16;
    while (no as usize) < ITEMS {
        if let Ok(v) = canonical_value(no) {
            let _ = l3.declare(no, v);
        }
        no = no.wrapping_add(1);
    }
    let d = l3.seal().err() == Some(DL_EBARE) && l3.bare() == ITEMS as u64;

    [a, b, c, d]
}

/// 判据台：下游冻结声明账的十三条判据。
pub fn run_unx11193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11193");

    let (mut l, built) = build_downstream();

    // J1 冻结件逐件声明对平
    s.add(
        "J1-逐件声明对平",
        built && l.is_sealed() && l.paired() && l.declared() == ITEMS as u64,
        "十件声明值皆等于规范冻结值",
    );

    // J2 消费指南零悬空
    s.add(
        "J2-指南零悬空",
        l.dangling() == 0 && l.bare() == 0,
        "指南全可解析且每件皆有消费方",
    );

    // J3 指南覆盖满
    s.add(
        "J3-指南覆盖满",
        l.guide_total() == (ITEMS * 2) as u64 && l.decl(0).map(|d| d.covered()).unwrap_or(false),
        "每件两条指南，覆盖无缺口",
    );

    // J4 封账指纹落定
    let mut probe = l.clone();
    let before = probe.fold();
    s.add(
        "J4-封账指纹",
        l.fold() == before && l.freeze() == Some(before) && l.freeze() == Some(before),
        "封账值等于账指纹且冻结幂等",
    );

    // J5 封账后拒改
    s.add(
        "J5-封账拒改",
        probe.declare(0, canonical_value(0).unwrap_or(0)).err() == Some(DL_EFROZEN)
            && probe.attach_guide(0, 0x999).err() == Some(DL_EFROZEN)
            && probe.seal() == Err(DL_EFROZEN),
        "封账后声明/挂指南/再封三路皆拒",
    );

    // J6 重复声明拒止
    let mut d = DownstreamLedger::new();
    let c0 = canonical_value(1).unwrap_or(0);
    let first = d.declare(1, c0).is_ok();
    s.add(
        "J6-重复声明",
        first && d.declare(1, c0).err() == Some(DL_EDUP) && d.declared() == 1,
        "同件二次声明拒止且不重复落账",
    );

    // J7 声明值不符拒止
    let mut v = DownstreamLedger::new();
    s.add(
        "J7-声明值不符",
        v.declare(2, 0xdead).err() == Some(DL_EVALUE) && v.rejects == 1,
        "错值声明拒止且拒绝入账",
    );

    // J8 重复消费方拒止
    let mut g = DownstreamLedger::new();
    let ok0 = g.declare(0, canonical_value(0).unwrap_or(0)).is_ok();
    let ok9 = g.declare(9, canonical_value(9).unwrap_or(0)).is_ok();
    let g1 = g.attach_guide(0, 9).is_ok();
    s.add(
        "J8-重复消费方",
        ok0 && ok9 && g1 && g.attach_guide(0, 9).err() == Some(DL_EGUIDE) && g.guide_total() == 1,
        "同消费方二次挂指南拒止",
    );

    // J9 悬空指南拒止
    let mut dd = DownstreamLedger::new();
    s.add(
        "J9-悬空指南拒止",
        dd.attach_guide(4, 1).err() == Some(DL_EDANGLING) && dd.guide_total() == 0,
        "未声明件不得挂指南",
    );

    // J10 空账封账拒止
    let mut e = DownstreamLedger::new();
    s.add(
        "J10-空账拒封",
        e.seal() == Err(DL_EVALUE) && !e.paired(),
        "空账无声明不可封",
    );

    // J11 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i += 1;
    }
    s.add("J11-注入四拒", all, "越界/错值/悬空/裸声明封账四路全拒");

    // J12 半程封账拒止
    let mut h = DownstreamLedger::new();
    if let Ok(cv0) = canonical_value(0) {
        let _ = h.declare(0, cv0);
    }
    s.add(
        "J12-半程拒封",
        h.seal() == Err(DL_EBARE) && h.bare() == 1,
        "声明齐而指南缺者不得封",
    );

    // J13 动态明细
    s.add_dyn(
        "J13-声明明细",
        l.rejects == 0 && l.declared() == ITEMS as u64,
        &format!(
            "decls={} guides={} bare={} dangling={} root={:#x}",
            l.declared(),
            l.guide_total(),
            l.bare(),
            l.dangling(),
            l.fold()
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11193_downstream_flow() {
        let (l, ok) = build_downstream();
        assert!(ok);
        assert!(l.is_sealed());
        assert!(l.paired());
        assert_eq!(l.dangling(), 0);
        assert_eq!(l.declared(), ITEMS as u64);
        assert_eq!(l.guide_total(), (ITEMS * 2) as u64);
        assert!(inject_four().iter().all(|h| *h));

        let mut e = DownstreamLedger::new();
        assert_eq!(e.seal(), Err(DL_EVALUE));
        assert_eq!(e.attach_guide(0, 1), Err(DL_EDANGLING));
    }

    #[test]
    fn unx11193_checks_green() {
        let s = run_unx11193_checks();
        assert!(s.all_passed(), "UNX-F11193 判据未全绿");
    }
}
