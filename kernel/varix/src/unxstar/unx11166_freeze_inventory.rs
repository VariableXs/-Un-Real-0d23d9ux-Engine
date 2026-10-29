//! UNX-F11166 冻结件清单终版（域冻结件总登记）（0x1f420 段）
//!
//! 纯功能模型：域冻结件总登记逐件落账，八件冻结件各带类别、版本戳与内容
//! 哈希。终版要求八件齐、版本戳恒为一且零漂移、哈希互异非零；清单锁定后
//! 不得再登记或改版。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f420;

/// 件号越界。
pub const FI_ENO: u32 = SEG_BASE + 0x01;
/// 件号重复。
pub const FI_EDUP: u32 = SEG_BASE + 0x02;
/// 件序倒挂。
pub const FI_EORDER: u32 = SEG_BASE + 0x03;
/// 类别越出四类（形状违规）。
pub const FI_EKIND: u32 = SEG_BASE + 0x04;
/// 版本戳漂移。
pub const FI_EVER: u32 = SEG_BASE + 0x05;
/// 内容哈希为零或重复。
pub const FI_EHASH: u32 = SEG_BASE + 0x06;
/// 清单件数不齐。
pub const FI_ECOUNT: u32 = SEG_BASE + 0x07;
/// 终值已锁定。
pub const FI_EFROZEN: u32 = SEG_BASE + 0x08;

/// 冻结件总数。
pub const ITEMS: usize = 8;
/// 件类别数。
pub const KINDS: u8 = 4;
/// 终版版本戳。
pub const WANT_VERSION: u8 = 1;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一件冻结件。
#[derive(Clone, Copy, Debug)]
pub struct FreezeItem {
    pub id: u8,
    pub kind: u8,
    pub version: u8,
    pub hash: u32,
}

impl FreezeItem {
    /// 类别在域内。
    pub fn kind_ok(&self) -> bool {
        self.kind < KINDS
    }

    /// 版本戳零漂移。
    pub fn version_ok(&self) -> bool {
        self.version == WANT_VERSION
    }

    /// 哈希可用。
    pub fn hash_ok(&self) -> bool {
        self.hash != 0
    }
}

/// 冻结件清单。
#[derive(Clone, Debug)]
pub struct FreezeInventory {
    items: Vec<FreezeItem>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl FreezeInventory {
    /// 空清单。
    pub fn new() -> FreezeInventory {
        FreezeInventory {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已登记件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空清单。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按件号取件。
    pub fn item(&self, id: usize) -> Option<FreezeItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 哈希是否已在册。
    pub fn hash_seen(&self, hash: u32) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].hash == hash {
                return true;
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 缺件表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 1usize;
        while id <= ITEMS {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 件序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].id <= self.items[i - 1].id {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
}

impl FreezeInventory {
    /// 登记一件：冻结 → 越界 → 重记 → 倒序 → 类别 → 版本 → 哈希。
    pub fn record(&mut self, id: usize, kind: u8, version: u8, hash: u32) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(FI_EFROZEN);
        }
        if id == 0 || id > ITEMS {
            return self.fail(FI_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(FI_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(FI_EORDER);
            }
        }
        let it = FreezeItem {
            id: id as u8,
            kind,
            version,
            hash,
        };
        if !it.kind_ok() {
            return self.fail(FI_EKIND);
        }
        if !it.version_ok() {
            return self.fail(FI_EVER);
        }
        if !it.hash_ok() || self.hash_seen(hash) {
            return self.fail(FI_EHASH);
        }
        self.items.push(it);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 版本戳是否全部零漂移。
    pub fn versions_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].version_ok() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 哈希是否两两互异且非零。
    pub fn hashes_distinct(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].hash_ok() {
                return false;
            }
            let mut j = i + 1;
            while j < self.items.len() {
                if self.items[i].hash == self.items[j].hash {
                    return false;
                }
                j = j.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 类别分布计数。
    pub fn kind_count(&self, kind: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].kind == kind {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 清单指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.kind as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.version as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.hash as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME)
    }
}

impl FreezeInventory {
    /// 终版：八件齐 → 版本零漂移 → 哈希互异。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(FI_EFROZEN);
        }
        if self.items.len() != ITEMS || !self.gaps().is_empty() || !self.order_strict() {
            return self.fail(FI_ECOUNT);
        }
        if !self.versions_ok() {
            return self.fail(FI_EVER);
        }
        if !self.hashes_distinct() {
            return self.fail(FI_EHASH);
        }
        let v = self.fold();
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
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

/// 构建标准冻结件清单：八件、版本恒一、哈希互异。
pub fn build_final() -> FreezeInventory {
    let mut inv = FreezeInventory::new();
    let mut id = 1usize;
    while id <= ITEMS {
        let kind = (id % KINDS as usize) as u8;
        let hash = 0x1000u32.wrapping_add((id as u32).wrapping_mul(0x101));
        let _ = inv.record(id, kind, WANT_VERSION, hash);
        id += 1;
    }
    let _ = inv.settle();
    inv
}

/// 四探针：越界、重复/倒序、版本漂移/哈希违例、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut i0 = FreezeInventory::new();
    let _ = i0.record(1, 0, WANT_VERSION, 0x11);
    if i0.record(0, 0, WANT_VERSION, 0x12) == Err(FI_ENO)
        && i0.record(ITEMS + 1, 0, WANT_VERSION, 0x13) == Err(FI_ENO)
    {
        r[0] = true;
    }

    let mut i1 = FreezeInventory::new();
    let _ = i1.record(1, 0, WANT_VERSION, 0x21);
    let _ = i1.record(2, 1, WANT_VERSION, 0x22);
    if i1.record(1, 0, WANT_VERSION, 0x23) == Err(FI_EDUP) {
        let mut i1b = FreezeInventory::new();
        let _ = i1b.record(5, 0, WANT_VERSION, 0x25);
        if i1b.record(3, 0, WANT_VERSION, 0x26) == Err(FI_EORDER) {
            r[1] = true;
        }
    }

    let mut i2 = FreezeInventory::new();
    let _ = i2.record(1, 0, WANT_VERSION, 0x31);
    if i2.record(2, 0, 2, 0x32) == Err(FI_EVER)
        && i2.record(2, 0, WANT_VERSION, 0) == Err(FI_EHASH)
    {
        r[2] = true;
    }

    let mut i3 = build_final();
    if i3.is_frozen()
        && i3.record(ITEMS + 1, 0, WANT_VERSION, 0x99) == Err(FI_EFROZEN)
        && i3.settle() == Err(FI_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11166");

    let base = build_final();
    s.add(
        "X1-清单构建",
        base.len() == ITEMS && base.accepts == 8 && base.rejects == 0 && base.is_frozen(),
        "八件全收、零拒收且已锁定",
    );
    s.add(
        "X2-件号连续",
        base.gaps().is_empty() && base.order_strict(),
        "件号 1..=8 无缺且严格递增",
    );
    s.add(
        "X3-版本零漂移",
        base.versions_ok(),
        "八件版本戳恒为一",
    );
    s.add(
        "X4-哈希互异",
        base.hashes_distinct(),
        "八件哈希非零且两两互异",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = FreezeInventory::new();
    let _ = fl.record(1, 0, WANT_VERSION, 0x55);
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复倒序/版本哈希/冻结后写四码可达",
    );

    let mut ld = FreezeInventory::new();
    let _ = ld.record(1, 0, WANT_VERSION, 0x61);
    let _ = ld.record(2, 1, WANT_VERSION, 0x62);
    let d = ld.record(1, 0, WANT_VERSION, 0x63);
    s.add(
        "X8-重复拒收",
        d == Err(FI_EDUP) && ld.accepts == 2 && ld.rejects == 1,
        "重复件号拒收且计数精确",
    );

    let mut lr = FreezeInventory::new();
    let _ = lr.record(1, 0, WANT_VERSION, 0x71);
    let r0 = lr.record(0, 0, WANT_VERSION, 0x72);
    let r1 = lr.record(ITEMS + 1, 0, WANT_VERSION, 0x73);
    s.add(
        "X9-越界拒收",
        r0 == Err(FI_ENO) && r1 == Err(FI_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "件号 0 与 9 均判越界",
    );

    let mut lo = FreezeInventory::new();
    let _ = lo.record(5, 0, WANT_VERSION, 0x75);
    let o = lo.record(3, 0, WANT_VERSION, 0x76);
    s.add(
        "X10-倒序拒收",
        o == Err(FI_EORDER) && lo.accepts == 1 && lo.rejects == 1,
        "5 后写 3 判倒序",
    );

    let mut lv = FreezeInventory::new();
    let _ = lv.record(1, 0, WANT_VERSION, 0x81);
    let ver = lv.record(2, 0, 2, 0x82);
    let hsh = lv.record(2, 0, WANT_VERSION, 0x81);
    s.add(
        "X11-版本与哈希",
        ver == Err(FI_EVER) && hsh == Err(FI_EHASH) && lv.accepts == 1 && lv.rejects == 2,
        "版本漂移与哈希重复分别拒收",
    );

    let mut lc = FreezeInventory::new();
    let mut id = 1usize;
    while id < ITEMS {
        let _ = lc.record(id, 0, WANT_VERSION, 0x900 + id as u32);
        id += 1;
    }
    let c = lc.settle();
    s.add(
        "X12-件数终核",
        c == Err(FI_ECOUNT) && lc.len() == 7 && lc.gaps() == Vec::from([8u32]),
        "缺第 8 件即判件数不齐",
    );

    let mut z = build_final();
    let zf = z.freeze();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && z.accepts == 8 && z.rejects == 0,
        &format!("acc={} rej={} kinds={}", z.accepts, z.rejects, z.kind_count(0)),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11166_flow() {
        let mut inv = build_final();
        assert_eq!(inv.len(), 8);
        assert!(inv.versions_ok());
        assert!(inv.hashes_distinct());
        assert!(inv.gaps().is_empty());
        assert!(inv.order_strict());
        assert_eq!(inv.kind_count(0), 2);
        assert!(inv.is_frozen());
        assert_eq!((inv.accepts, inv.rejects), (8, 0));
        let v = inv.settled_value();
        assert!(v.is_some());
        assert_eq!(inv.freeze(), v);
        assert_eq!(inv.freeze(), v);
        assert_eq!(inv.item(1).map(|i| i.version), Some(1));
        assert_eq!(inv.item(8).map(|i| i.id), Some(8));
        assert!(inv.item(9).is_none());
        assert!(inv.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11166_gates() {
        let mut l = FreezeInventory::new();
        assert!(l.record(1, 0, WANT_VERSION, 0x11).is_ok());
        assert!(l.record(2, 1, WANT_VERSION, 0x12).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(1, 0, WANT_VERSION, 0x13), Err(FI_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 1));
        assert!(l.record(5, 0, WANT_VERSION, 0x15).is_ok());
        assert_eq!((l.accepts, l.rejects), (3, 1));
        assert_eq!(l.record(3, 0, WANT_VERSION, 0x16), Err(FI_EORDER));
        assert_eq!((l.accepts, l.rejects), (3, 2));
        assert_eq!(l.record(6, 0, 2, 0x17), Err(FI_EVER));
        assert_eq!((l.accepts, l.rejects), (3, 3));
        assert_eq!(l.record(6, 0, WANT_VERSION, 0), Err(FI_EHASH));
        assert_eq!((l.accepts, l.rejects), (3, 4));
        assert_eq!(l.record(0, 0, WANT_VERSION, 0x18), Err(FI_ENO));
        assert_eq!(l.record(ITEMS + 1, 0, WANT_VERSION, 0x19), Err(FI_ENO));
        assert_eq!((l.accepts, l.rejects), (3, 6));
        assert_eq!(l.settle(), Err(FI_ECOUNT));
        assert_eq!((l.accepts, l.rejects), (3, 7));

        let mut short = FreezeInventory::new();
        let mut id = 1usize;
        while id < ITEMS {
            assert!(short
                .record(id, 0, WANT_VERSION, 0x900 + id as u32)
                .is_ok());
            id += 1;
        }
        assert_eq!(short.len(), 7);
        assert_eq!(short.gaps(), Vec::from([8u32]));
        assert_eq!(short.settle(), Err(FI_ECOUNT));
        assert_eq!((short.accepts, short.rejects), (7, 1));

        let mut done = build_final();
        assert_eq!(done.settle(), Err(FI_EFROZEN));
        assert_eq!(
            done.record(ITEMS + 1, 0, WANT_VERSION, 0x99),
            Err(FI_EFROZEN)
        );
        assert_eq!((done.accepts, done.rejects), (8, 2));
    }

    #[test]
    fn f11166_checks_green() {
        let s = run_unx11166_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
