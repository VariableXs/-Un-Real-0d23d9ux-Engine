//! UNX-F11188 台账三处终态预写核验（总纲 / 根台账 / 域小结）（0x1f6d0 段）
//!
//! 纯功能模型：同一终态要落到三处台账——总纲、根台账、域小结。三处各写一锚，
//! 每处只许写一次（站点唯一），三锚必须同值且等于由批数据算出的规范锚。
//! 站点越界、同处重写、处序倒挂、锚值不符、三处不齐、封账后再写，一律拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6d0;
/// 处号越界
pub const TB_ESITE: u32 = SEG_BASE + 0x01;
/// 同处重写
pub const TB_EDUP: u32 = SEG_BASE + 0x02;
/// 处序倒挂
pub const TB_EORDER: u32 = SEG_BASE + 0x03;
/// 锚值不符 / 空锚
pub const TB_EANCHOR: u32 = SEG_BASE + 0x04;
/// 三处不齐
pub const TB_ESHORT: u32 = SEG_BASE + 0x05;
/// 已封账
pub const TB_EFROZEN: u32 = SEG_BASE + 0x06;
/// 空账
pub const TB_EEMPTY: u32 = SEG_BASE + 0x07;

/// 台账处数。
pub const SITES: usize = 3;
/// 处名：总纲 / 根台账 / 域小结。
pub const SITE_NAMES: [&str; SITES] = ["outline", "root_ledger", "domain_digest"];
/// 交接口径。
pub const CARDS: u64 = 200;
/// 每卡行数下限。
pub const LINES_PER_CARD: u64 = 250;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一处锚。
#[derive(Clone, Copy, Debug)]
pub struct SiteAnchor {
    pub site: u8,
    pub value: u64,
}

/// 三处台账预写账。
#[derive(Clone, Debug)]
pub struct TripleLedger {
    anchors: Vec<SiteAnchor>,
    sealed: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl TripleLedger {
    /// 空账。
    pub fn new() -> TripleLedger {
        TripleLedger {
            anchors: Vec::new(),
            sealed: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已写处数。
    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// 取某处锚值。
    pub fn anchor_at(&self, site: usize) -> Option<u64> {
        let mut i = 0usize;
        while i < self.anchors.len() {
            if self.anchors[i].site as usize == site {
                return Some(self.anchors[i].value);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一处锚：冻结→处号→重写→处序→空锚。
    pub fn stake(&mut self, site: usize, value: u64) -> Result<u64, u32> {
        if self.frozen {
            return self.fail(TB_EFROZEN);
        }
        if site >= SITES {
            return self.fail(TB_ESITE);
        }
        if self.anchor_at(site).is_some() {
            return self.fail(TB_EDUP);
        }
        if let Some(last) = self.anchors.last() {
            if site <= last.site as usize {
                return self.fail(TB_EORDER);
            }
        }
        if value == 0 {
            return self.fail(TB_EANCHOR);
        }
        self.anchors.push(SiteAnchor {
            site: site as u8,
            value,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 站点唯一：三处各恰一锚。
    pub fn sites_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.anchors.len() {
            let mut j = i.wrapping_add(1);
            while j < self.anchors.len() {
                if self.anchors[i].site == self.anchors[j].site {
                    return false;
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 三锚同值。
    pub fn uniform(&self) -> bool {
        if self.anchors.len() != SITES {
            return false;
        }
        let first = self.anchors[0].value;
        let mut i = 1usize;
        while i < self.anchors.len() {
            if self.anchors[i].value != first {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 三锚同值且等于规范锚。
    pub fn coherent(&self, canonical: u64) -> bool {
        self.uniform() && canonical != 0 && self.anchors[0].value == canonical
    }

    /// 账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.anchors.len() {
            h = (h ^ self.anchors[i].site as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ self.anchors[i].value).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.anchors.len() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 三处是否各已落锚。
    pub fn site_table(&self) -> [bool; SITES] {
        let mut t = [false; SITES];
        let mut s = 0usize;
        while s < SITES {
            t[s] = self.anchor_at(s).is_some();
            s = s.wrapping_add(1);
        }
        t
    }

    /// 封账：三处齐、站点唯一、三锚同值且等于规范锚。
    pub fn seal(&mut self, canonical: u64) -> Result<u64, u32> {
        if self.sealed.is_some() {
            return self.fail(TB_EFROZEN);
        }
        if self.anchors.is_empty() {
            return self.fail(TB_EEMPTY);
        }
        if self.anchors.len() != SITES {
            return self.fail(TB_ESHORT);
        }
        if !self.sites_unique() {
            return self.fail(TB_EDUP);
        }
        if !self.coherent(canonical) {
            return self.fail(TB_EANCHOR);
        }
        let v = self.fold();
        self.sealed = Some(v);
        Ok(v)
    }

    /// 封账后冻结：幂等锁值。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.sealed;
        }
        if self.sealed.is_none() && self.anchors.len() == SITES && self.sites_unique() {
            self.sealed = Some(self.fold());
        }
        self.frozen = true;
        self.sealed
    }

    /// 封账值。
    pub fn sealed_value(&self) -> Option<u64> {
        self.sealed
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 规范锚：由批号、处数、卡数、行数与处名共同算出，任何一处台账都该写它。
pub fn canonical_anchor() -> u64 {
    let mut h = FNV_OFF;
    h = (h ^ 11188u64).wrapping_mul(FNV_PRIME);
    h = (h ^ SITES as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ CARDS).wrapping_mul(FNV_PRIME);
    h = (h ^ CARDS.saturating_mul(LINES_PER_CARD)).wrapping_mul(FNV_PRIME);
    let mut s = 0usize;
    while s < SITES {
        if let Some(name) = SITE_NAMES.get(s) {
            let bytes = name.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                if let Some(b) = bytes.get(i) {
                    h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
                }
                i = i.wrapping_add(1);
            }
        }
        s = s.wrapping_add(1);
    }
    h
}

/// 预写三处：每处一锚，值取规范锚，再按规范锚封账。
pub fn build_ledger() -> (TripleLedger, bool) {
    let mut t = TripleLedger::new();
    let canonical = canonical_anchor();
    let mut s = 0usize;
    while s < SITES {
        if t.stake(s, canonical).is_err() {
            return (t, false);
        }
        s = s.wrapping_add(1);
    }
    let ok = t.seal(canonical).is_ok();
    (t, ok)
}

/// 四类注入：处号越界 / 同处重写 / 锚值不符与三处不齐 / 封账后写入。
pub fn inject_four() -> [bool; 4] {
    let canonical = canonical_anchor();

    // ① 处号越界
    let mut a0 = TripleLedger::new();
    let r0 = a0.stake(SITES, canonical).err() == Some(TB_ESITE)
        && a0.stake(SITES.wrapping_add(9), canonical).err() == Some(TB_ESITE);

    // ② 同处重写
    let mut a1 = TripleLedger::new();
    let s1 = a1.stake(1, canonical).is_ok();
    let r1 = s1 && a1.stake(1, canonical).err() == Some(TB_EDUP);

    // ③ 锚值不符 / 三处不齐 / 空锚
    let mut a2 = TripleLedger::new();
    let ok_a = a2.stake(0, canonical).is_ok();
    let short = a2.seal(canonical).err() == Some(TB_ESHORT);
    let ok_b = a2.stake(1, canonical).is_ok();
    let ok_c = a2.stake(2, canonical.wrapping_add(1)).is_ok();
    let mism = a2.seal(canonical).err() == Some(TB_EANCHOR);
    let mut a3 = TripleLedger::new();
    let blank = a3.stake(0, 0).err() == Some(TB_EANCHOR);
    let r2 = ok_a && short && ok_b && ok_c && mism && blank;

    // ④ 封账后写入与二次封账
    let (mut a4, ok4) = build_ledger();
    let locked = a4.freeze().is_some();
    let r3 = ok4
        && locked
        && a4.stake(0, canonical).err() == Some(TB_EFROZEN)
        && a4.seal(canonical).err() == Some(TB_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11188 台账三处终态预写核验。
pub fn run_unx11188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11188");
    let canonical = canonical_anchor();
    let (mut t, ok) = build_ledger();

    // T1 三处齐备
    let table = t.site_table();
    let mut all_sites = true;
    let mut i = 0usize;
    while i < SITES {
        if !table[i] {
            all_sites = false;
        }
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "T1-三处齐备",
        ok && all_sites && t.len() == SITES,
        &format!("sites={}/{} all={}", t.len(), SITES, all_sites),
    );

    // T2 站点唯一
    s.add(
        "T2-站点唯一",
        t.sites_unique(),
        "总纲 / 根台账 / 域小结各恰一锚，无同处重写残留",
    );

    // T3 三锚同值
    s.add_dyn(
        "T3-三锚同值",
        t.uniform(),
        &format!(
            "a0={:#x} a1={:#x} a2={:#x}",
            t.anchor_at(0).unwrap_or(0),
            t.anchor_at(1).unwrap_or(0),
            t.anchor_at(2).unwrap_or(0)
        ),
    );

    // T4 锚即规范锚
    s.add_dyn(
        "T4-锚即规范锚",
        t.coherent(canonical),
        &format!("canonical={:#x}", canonical),
    );

    // T5 封账值即指纹
    let sealed = t.sealed_value();
    s.add(
        "T5-封账值即指纹",
        sealed == Some(t.fold()) && t.freeze() == sealed,
        "封账值等于三锚指纹，冻结不改变它",
    );

    // T6 冻结幂等
    let f1 = t.freeze();
    let f2 = t.freeze();
    s.add("T6-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // T7 封账后拒写入
    s.add(
        "T7-封账后拒写入",
        t.is_frozen()
            && t.stake(0, canonical).err() == Some(TB_EFROZEN)
            && t.seal(canonical).err() == Some(TB_EFROZEN),
        "封账后既不许落锚也不许再封",
    );

    // T8 空账拒封
    let mut e = TripleLedger::new();
    s.add(
        "T8-空账拒封",
        e.seal(canonical).err() == Some(TB_EEMPTY) && e.is_empty(),
        "空账不得封",
    );

    // T9 三处不齐拒封
    let mut h = TripleLedger::new();
    let h0 = h.stake(0, canonical).is_ok();
    let h1 = h.stake(1, canonical).is_ok();
    s.add_dyn(
        "T9-三处不齐拒封",
        h0 && h1 && h.seal(canonical).err() == Some(TB_ESHORT),
        &format!("partial={}/{}", h.len(), SITES),
    );

    // T10 锚不符拒封
    let mut m = TripleLedger::new();
    let m0 = m.stake(0, canonical).is_ok();
    let m1 = m.stake(1, canonical).is_ok();
    let m2 = m.stake(2, canonical.wrapping_add(1)).is_ok();
    s.add(
        "T10-锚不符拒封",
        m0 && m1 && m2
            && m.seal(canonical).err() == Some(TB_EANCHOR)
            && !m.uniform(),
        "三锚不同值即拒封",
    );

    // T11 同处重写拒止
    let mut d = TripleLedger::new();
    let d0 = d.stake(0, canonical).is_ok();
    s.add(
        "T11-同处重写拒止",
        d0 && d.stake(0, canonical).err() == Some(TB_EDUP) && d.len() == 1,
        "同处只许一锚",
    );

    // T12 处号越界与处序倒挂
    let mut o = TripleLedger::new();
    let o0 = o.stake(2, canonical).is_ok();
    s.add(
        "T12-越界与倒挂拒止",
        o.stake(SITES, canonical).err() == Some(TB_ESITE)
            && o0
            && o.stake(1, canonical).err() == Some(TB_EORDER),
        "处号越界与回写更早处皆拒",
    );

    // T13 注入四拒 + 终盘明细（新账重算）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fresh, ok2) = build_ledger();
    let root = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "T13-注入四拒与终盘明细",
        all && ok2 && fresh.rejects == 0 && fresh.accepts == SITES as u64 && root != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11188_triple_anchor_flow() {
        let (mut t, ok) = build_ledger();
        assert!(ok);
        assert_eq!(t.len(), SITES);
        assert!(t.sites_unique());
        assert!(t.uniform());
        let c = canonical_anchor();
        assert!(t.coherent(c));
        assert_eq!(t.sealed_value(), Some(t.fold()));
        let v = t.freeze();
        assert_eq!(v, t.sealed_value());
        assert_eq!(t.freeze(), v);
        assert_eq!(t.stake(0, c), Err(TB_EFROZEN));
        assert_eq!(t.seal(c), Err(TB_EFROZEN));
    }

    #[test]
    fn f11188_site_gates() {
        let c = canonical_anchor();
        let mut t = TripleLedger::new();
        assert_eq!(t.stake(SITES, c), Err(TB_ESITE));
        assert_eq!(t.stake(0, 0), Err(TB_EANCHOR));
        assert!(t.stake(0, c).is_ok());
        assert_eq!(t.stake(0, c), Err(TB_EDUP));
        assert_eq!(t.stake(0, c), Err(TB_EDUP));
        assert_eq!(t.rejects, 4);
        assert_eq!(t.seal(c), Err(TB_ESHORT));
        assert_eq!(canonical_anchor(), c);
    }

    #[test]
    fn f11188_checks_green() {
        let cs = run_unx11188_checks();
        assert!(cs.all_passed());
    }
}
