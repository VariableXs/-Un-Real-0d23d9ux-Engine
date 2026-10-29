//! UNX-F11172 移交包一件（深化册 40 册打包清单）（0x1f480 段）
//!
//! 模型：一份 40 册深化册的打包清单。每册登记册号、页数、字节数、在包内的
//! 累计偏移、内容摘要与封条位。规则：册号须在一至四十；册序严格递增且不重；
//! 每册页数、字节数、摘要均非零且已封条；册的偏移必须等于上一册偏移加本册
//! 前字节，首册偏移为零——偏移链就是打包的顺序凭据。清单头部另记申报总字节，
//! 终核时全册字节合计必须与申报值逐字节对平。收册记受理，越界、重复、倒序、
//! 偏移断、实体不全与冻结后收册记拒止。终核重放逐册对平后折叠为单一指纹，
//! 二次冻结返回冻结码；拒收只针对结构，实体字段的越界值由规则函数判定。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 深化册移交包）。
const SEG_BASE: u32 = 0x1f480;

/// 册号越界。
pub const HP_ENO: u32 = SEG_BASE + 0x01;
/// 册号重复。
pub const HP_EDUP: u32 = SEG_BASE + 0x02;
/// 册号倒序。
pub const HP_EORDER: u32 = SEG_BASE + 0x03;
/// 偏移链断裂。
pub const HP_EOFF: u32 = SEG_BASE + 0x04;
/// 实体字段不全（页/字节/摘要/封条）。
pub const HP_EBODY: u32 = SEG_BASE + 0x05;
/// 申报总量越界或未申报。
pub const HP_EDECL: u32 = SEG_BASE + 0x06;
/// 字节合计与申报不符。
pub const HP_ESUM: u32 = SEG_BASE + 0x07;
/// 册数不齐。
pub const HP_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const HP_EFROZEN: u32 = SEG_BASE + 0x09;

/// 册数。
pub const VOLS: usize = 40;
/// 单册页数下界。
pub const MIN_PAGES: u16 = 8;
/// 单册页数上界（含）。
pub const MAX_PAGES: u16 = 240;
/// 单册字节基数。
pub const BYTES_BASE: u32 = 2048;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一册深化册的打包实体。
#[derive(Clone, Copy, Debug)]
pub struct Volume {
    pub id: u8,
    pub pages: u16,
    pub bytes: u32,
    pub offset: u32,
    pub digest: u32,
    pub sealed: bool,
}

impl Volume {
    /// 空册。
    pub const fn blank() -> Volume {
        Volume {
            id: 0,
            pages: 0,
            bytes: 0,
            offset: 0,
            digest: 0,
            sealed: false,
        }
    }

    /// 实体字段齐备：页数与字节非零、摘要非零、已封条。
    pub fn body_ok(&self) -> bool {
        self.pages >= MIN_PAGES
            && self.pages <= MAX_PAGES
            && self.bytes >= BYTES_BASE
            && self.digest != 0
            && self.sealed
    }

    /// 本册末端偏移。
    pub fn end(&self) -> u32 {
        self.offset.wrapping_add(self.bytes)
    }

    /// 摘要与册号、页数、字节卷成 32 位值。
    pub fn digest_of(id: u8, pages: u16, bytes: u32) -> u32 {
        let mut x = (id as u32).wrapping_mul(0x9e37_79b1) ^ (pages as u32);
        x = x.rotate_left(7) ^ bytes;
        let y = x.wrapping_mul(0x85eb_ca6b);
        if y == 0 {
            1
        } else {
            y
        }
    }

    /// 摘要自洽。
    pub fn digest_ok(&self) -> bool {
        self.digest == Volume::digest_of(self.id, self.pages, self.bytes)
    }
}

/// 打包清单账本。
#[derive(Clone, Copy, Debug)]
pub struct PackLedger {
    vols: [Volume; VOLS],
    len: usize,
    accepts: u32,
    rejects: u32,
    declared: u32,
    frozen: bool,
    settled: Option<u64>,
}

impl PackLedger {
    /// 空清单。
    pub const fn new() -> PackLedger {
        PackLedger {
            vols: [Volume::blank(); VOLS],
            len: 0,
            accepts: 0,
            rejects: 0,
            declared: 0,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收册数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 申报总字节。
    pub fn declared(&self) -> u32 {
        self.declared
    }

    /// 末册（空单为 None）。
    pub fn last(&self) -> Option<Volume> {
        if self.len == 0 {
            return None;
        }
        self.vols.get(self.len - 1).copied()
    }

    /// 第 i 册（零基序号）。
    pub fn vol_at(&self, i: usize) -> Option<Volume> {
        if i >= self.len {
            return None;
        }
        self.vols.get(i).copied()
    }

    /// 按册号查序号。
    pub fn find(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.vols.get(i) {
                if v.id == id {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 申报总字节：非零且不越 32 位余量。
    pub fn declare(&mut self, total: u32) -> Result<u32, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(HP_EFROZEN));
        }
        if total == 0 {
            return Err(self.fail(HP_EDECL));
        }
        self.declared = total;
        Ok(total)
    }

    /// 收册：冻结 → 越界 → 重复 → 倒序 → 偏移 → 实体 → 摘要。
    pub fn register(&mut self, v: Volume) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(HP_EFROZEN));
        }
        if v.id == 0 || v.id as usize > VOLS {
            return Err(self.fail(HP_ENO));
        }
        if self.find(v.id).is_some() {
            return Err(self.fail(HP_EDUP));
        }
        let expected = match self.last() {
            Some(last) => {
                if v.id <= last.id {
                    return Err(self.fail(HP_EORDER));
                }
                last.end()
            }
            None => 0,
        };
        if v.offset != expected {
            return Err(self.fail(HP_EOFF));
        }
        if !v.body_ok() {
            return Err(self.fail(HP_EBODY));
        }
        if !v.digest_ok() {
            return Err(self.fail(HP_EBODY));
        }
        match self.vols.get_mut(self.len) {
            Some(slot) => *slot = v,
            None => return Err(self.fail(HP_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按号观察收册：由规格生成实体，偏移接在册尾。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if id == 0 || id > VOLS {
            return Err(self.fail(HP_ENO));
        }
        let expected = match self.last() {
            Some(last) => last.end(),
            None => 0,
        };
        let v = vol_spec(id as u8, expected);
        self.register(v)
    }

    /// 全单字节合计。
    pub fn bytes_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.vols.get(i) {
                sum = sum.wrapping_add(v.bytes);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全单页数合计。
    pub fn pages_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.vols.get(i) {
                sum = sum.wrapping_add(v.pages as u32);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 已封条册数（核销实体数）。
    pub fn sealed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.vols.get(i) {
                if v.sealed {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 偏移链：首册零偏移，逐册偏移等于前册末端。
    pub fn offsets_ok(&self) -> bool {
        let mut expected = 0u32;
        let mut i = 0usize;
        while i < self.len {
            match self.vols.get(i) {
                Some(v) => {
                    if v.offset != expected {
                        return false;
                    }
                    expected = v.end();
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 实体与摘要全齐。
    pub fn bodies_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.vols.get(i) {
                Some(v) => {
                    if !v.body_ok() || !v.digest_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 册号无缺无重：第 i 册册号恰为 i+1。
    pub fn ids_contiguous(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.vols.get(i) {
                Some(v) => {
                    if v.id as usize != i.wrapping_add(1) {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }
}

impl PackLedger {
    /// 清单指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.vols.get(i) {
                h = (h ^ v.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ v.pages as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ v.bytes as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ v.offset as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ v.digest as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ if v.sealed { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.declared as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：四十册齐、号连续、偏移接续、实体齐、申报对平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != VOLS || !self.ids_contiguous() {
            return Err(HP_ECOUNT);
        }
        if !self.offsets_ok() {
            return Err(HP_EOFF);
        }
        if !self.bodies_ok() {
            return Err(HP_EBODY);
        }
        if self.declared == 0 {
            return Err(HP_EDECL);
        }
        if self.bytes_total() != self.declared {
            return Err(HP_ESUM);
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(HP_EFROZEN));
        }
        let v = match self.replay() {
            Ok(v) => v,
            Err(e) => return Err(self.fail(e)),
        };
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 第 id 册规格：字节、页数、包内偏移。偏移为前 id-1 册字节累加。
pub fn spec_of(id: usize) -> (u32, u16, u32) {
    if id == 0 || id > VOLS {
        return (0, 0, 0);
    }
    let mut offset = 0u32;
    let mut k = 1usize;
    while k < id {
        offset = offset.wrapping_add(BYTES_BASE.wrapping_add((k as u32) * 96));
        k = k.wrapping_add(1);
    }
    let bytes = BYTES_BASE.wrapping_add((id as u32) * 96);
    let pages = MIN_PAGES + (id as u16) * 3;
    (bytes, pages, offset)
}

/// 由规格生成一册，偏移由清单现算。
fn vol_spec(id: u8, offset: u32) -> Volume {
    let mut v = Volume::blank();
    let spec = spec_of(id as usize);
    v.id = id;
    v.pages = spec.1;
    v.bytes = spec.0;
    v.offset = offset;
    v.digest = Volume::digest_of(id, spec.1, spec.0);
    v.sealed = true;
    v
}

/// 构建标准清单：四十册顺序打包，申报总字节等于规格合计。
pub fn build_ledger() -> PackLedger {
    let mut l = PackLedger::new();
    let mut id = 1usize;
    while id <= VOLS {
        let _ = l.observe(id);
        id = id.wrapping_add(1);
    }
    let total = l.bytes_total();
    let _ = l.declare(total);
    l
}

/// 探针册：摘要按字段现算，偏移可故意给错以触达偏移链拒收。
fn probe_vol(id: u8, offset: u32, pages: u16, sealed: bool) -> Volume {
    let mut v = Volume::blank();
    v.id = id;
    v.pages = pages;
    v.bytes = BYTES_BASE;
    v.offset = offset;
    v.sealed = sealed;
    v.digest = Volume::digest_of(id, pages, v.bytes);
    v
}

/// 四探针：越界、重复/倒序、偏移断/实体不全、冻结后收，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = PackLedger::new();
    let _ = a.register(probe_vol(1, 0, MIN_PAGES, true));
    if a.register(probe_vol(0, BYTES_BASE, MIN_PAGES, true)) == Err(HP_ENO)
        && a.register(probe_vol((VOLS + 1) as u8, BYTES_BASE, MIN_PAGES, true)) == Err(HP_ENO)
    {
        r[0] = true;
    }

    let mut b = PackLedger::new();
    let b2 = probe_vol(2, 0, MIN_PAGES, true);
    let _ = b.register(b2);
    if b.register(b2) == Err(HP_EDUP)
        && b.register(probe_vol(1, BYTES_BASE, MIN_PAGES, true)) == Err(HP_EORDER)
    {
        r[1] = true;
    }

    let mut c = PackLedger::new();
    let c1 = probe_vol(1, 0, MIN_PAGES, true);
    let _ = c.register(c1);
    if c.register(probe_vol(2, 0, MIN_PAGES, true)) == Err(HP_EOFF)
        && c.register(probe_vol(2, BYTES_BASE, 0, true)) == Err(HP_EBODY)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(HP_EFROZEN)
        && d.register(probe_vol(1, 0, MIN_PAGES, true)) == Err(HP_EFROZEN)
        && d.declare(1) == Err(HP_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11172");
    let base = build_ledger();

    s.add(
        "S1-清单逐册对平",
        base.len() == VOLS
            && base.accepted() == VOLS as u32
            && base.rejected() == 0
            && base.ids_contiguous(),
        "四十册全收且册号恰为一至四十",
    );

    let first_zero = match base.vol_at(0) {
        Some(v) => v.offset == 0,
        None => false,
    };
    let tail_ok = match base.last() {
        Some(v) => v.end() == base.bytes_total(),
        None => false,
    };
    s.add(
        "S2-偏移链连续",
        first_zero && base.offsets_ok() && tail_ok,
        "首册零偏移，逐册偏移接前册末端，册尾等于总量",
    );

    s.add(
        "S3-实体齐备",
        base.bodies_ok(),
        "每册页数字节摘要非零且已封条",
    );

    let mut spec_bytes = 0u32;
    let mut spec_pages = 0u32;
    let mut id = 1usize;
    while id <= VOLS {
        let (b, p, _) = spec_of(id);
        spec_bytes = spec_bytes.wrapping_add(b);
        spec_pages = spec_pages.wrapping_add(p as u32);
        id = id.wrapping_add(1);
    }
    s.add(
        "S4-四十册核销",
        base.sealed_count() == VOLS as u32
            && base.bytes_total() == base.declared()
            && base.declared() == spec_bytes,
        "四十册全部封条，字节合计与申报及规格三方相等",
    );

    s.add(
        "S5-页数对平",
        base.pages_total() == spec_pages,
        "全单页数合计与规格页数合计相等",
    );

    let mut per_vol = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.vol_at(i) {
            Some(v) => {
                let (b, p, _) = spec_of(i.wrapping_add(1));
                if v.bytes != b || v.pages != p {
                    per_vol = false;
                }
            }
            None => per_vol = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("S6-逐册规格对平", per_vol, "每册字节与页数均等于规格值");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(HP_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single, "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放四十册逐项对平且指纹一致",
    );

    let mut undecl = PackLedger::new();
    let mut k = 1usize;
    while k <= VOLS {
        let _ = undecl.observe(k);
        k = k.wrapping_add(1);
    }
    let no_decl = undecl.freeze();
    let mut fresh = PackLedger::new();
    let zero_decl = fresh.declare(0);
    s.add(
        "S10-申报闸",
        no_decl == Err(HP_EDECL) && zero_decl == Err(HP_EDECL),
        "未申报与零申报均判申报越界",
    );

    let mut la = PackLedger::new();
    let _ = la.register(probe_vol(1, 0, MIN_PAGES, true));
    let r0 = la.register(probe_vol(0, BYTES_BASE, MIN_PAGES, true));
    let r1 = la.register(probe_vol((VOLS + 1) as u8, BYTES_BASE, MIN_PAGES, true));
    s.add(
        "X11-越界拒收",
        r0 == Err(HP_ENO) && r1 == Err(HP_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "册号 0 与 41 均判越界",
    );

    let mut lb = PackLedger::new();
    let b2 = probe_vol(2, 0, MIN_PAGES, true);
    let _ = lb.register(b2);
    let dup = lb.register(b2);
    let ord = lb.register(probe_vol(1, BYTES_BASE, MIN_PAGES, true));
    s.add(
        "X12-重复倒序",
        dup == Err(HP_EDUP) && ord == Err(HP_EORDER) && lb.accepted() == 1 && lb.rejected() == 2,
        "重收二号判重复、二号后写一号判倒序",
    );

    let mut lc = PackLedger::new();
    let _ = lc.register(probe_vol(1, 0, MIN_PAGES, true));
    let off = lc.register(probe_vol(2, 0, MIN_PAGES, true));
    let body = lc.register(probe_vol(2, BYTES_BASE, 0, true));
    let seal = lc.register(probe_vol(2, BYTES_BASE, MIN_PAGES, false));
    s.add(
        "X13-偏移与实体",
        off == Err(HP_EOFF)
            && body == Err(HP_EBODY)
            && seal == Err(HP_EBODY)
            && lc.accepted() == 1
            && lc.rejected() == 3,
        "偏移断、零页、未封条分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11172_pack_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), VOLS);
        assert_eq!(l.accepted(), VOLS as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.ids_contiguous());
        assert!(l.offsets_ok());
        assert!(l.bodies_ok());
        assert_eq!(l.sealed_count(), VOLS as u32);
        assert_eq!(l.bytes_total(), l.declared());
        assert!(l.declared() > 0);
        assert_eq!(l.vol_at(0).map(|v| v.offset), Some(0));
        assert!(l.vol_at(VOLS).is_none());
        assert!(l.find(40).is_some());
        assert!(l.find(41).is_none());
    }

    #[test]
    fn f11172_spec_offsets() {
        assert_eq!(spec_of(0), (0, 0, 0));
        assert_eq!(spec_of(VOLS + 1), (0, 0, 0));
        let (b1, p1, o1) = spec_of(1);
        assert_eq!(o1, 0);
        assert_eq!(b1, BYTES_BASE + 96);
        assert!(p1 >= MIN_PAGES && p1 <= MAX_PAGES);
        let (b2, _, o2) = spec_of(2);
        assert_eq!(o2, b1);
        assert_eq!(b2, BYTES_BASE + 192);
        assert_eq!(build_ledger().bytes_total(), {
            let mut sum = 0u32;
            let mut id = 1usize;
            while id <= VOLS {
                sum = sum.wrapping_add(spec_of(id).0);
                id = id.wrapping_add(1);
            }
            sum
        });
    }

    #[test]
    fn f11172_gates() {
        let mut a = PackLedger::new();
        assert!(a.register(probe_vol(1, 0, MIN_PAGES, true)).is_ok());
        assert_eq!(a.register(probe_vol(2, 0, MIN_PAGES, true)), Err(HP_EOFF));
        assert_eq!(a.register(probe_vol(2, BYTES_BASE, 0, true)), Err(HP_EBODY));
        assert_eq!(a.register(probe_vol(2, BYTES_BASE, MIN_PAGES, false)), Err(HP_EBODY));
        assert!(a.register(probe_vol(2, BYTES_BASE, MIN_PAGES, true)).is_ok());
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 3);
    }

    #[test]
    fn f11172_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(HP_EFROZEN));
        assert_eq!(l.declare(9), Err(HP_EFROZEN));
    }

    #[test]
    fn f11172_checks_green() {
        let s = run_unx11172_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
