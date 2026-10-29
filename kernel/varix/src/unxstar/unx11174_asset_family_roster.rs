//! UNX-F11174 移交包三件（场/账/协议/基线全家当清单）（0x1f4a0 段）
//!
//! 模型：把域内全家当分四族登记——场、账、协议、基线。四族容量分别为
//! 六、六、五、五，合计二十二件。每件登记族号、族内件号、字节数、版本戳、
//! 内容摘要与在场位。与顺序清单不同，收件允许跨族交织：各族自己维护件号
//! 游标，族内必须从一号起逐件递增，不得跳号或重号；跨族则无先后要求。
//! 每件必须在场、字节非零、版本落在一至三且摘要可由四字段复算。
//! 收件记受理，族号越界、件号越界、重号、跳号、缺场、零字节与摘要不符
//! 一律拒止。终核要求四族各满容量、二十二件无漏，再折叠为单一指纹；
//! 二次冻结返回冻结码。探针的缺场件与零字节件由规则函数判定。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 四族全家当）。
const SEG_BASE: u32 = 0x1f4a0;

/// 族号越界。
pub const AS_EFAM: u32 = SEG_BASE + 0x01;
/// 件号越界。
pub const AS_EITEM: u32 = SEG_BASE + 0x02;
/// 同族件号重复。
pub const AS_EDUP: u32 = SEG_BASE + 0x03;
/// 族内跳号。
pub const AS_EORDER: u32 = SEG_BASE + 0x04;
/// 件缺场。
pub const AS_EABSENT: u32 = SEG_BASE + 0x05;
/// 字节为零或越上界。
pub const AS_EBYTES: u32 = SEG_BASE + 0x06;
/// 版本越界或摘要不符。
pub const AS_EHASH: u32 = SEG_BASE + 0x07;
/// 四族件数不齐。
pub const AS_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const AS_EFROZEN: u32 = SEG_BASE + 0x09;

/// 族数。
pub const FAMILIES: usize = 4;
/// 全场件数。
pub const ITEMS: usize = 22;
/// 单件字节上界。
pub const BYTES_MAX: u32 = 4096;
/// 版本上界。
pub const MAX_VERSION: u8 = 3;
/// 四族容量（场、账、协议、基线）。
pub const CAPS: [u8; FAMILIES] = [6, 6, 5, 5];

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 第 fam 族容量。
pub fn cap_of(fam: u8) -> u8 {
    match fam {
        0 => 6,
        1 => 6,
        2 => 5,
        3 => 5,
        _ => 0,
    }
}

/// 第 fam 族第 item 件的摘要。
pub fn hash_of(fam: u8, item: u8, bytes: u32, version: u8) -> u32 {
    let mut x = (fam as u32).wrapping_mul(0x9e37_79b1) ^ (item as u32);
    x = x.rotate_left(11) ^ bytes;
    x = x.wrapping_mul(0x85eb_ca6b) ^ (version as u32);
    if x == 0 {
        1
    } else {
        x
    }
}

/// 一件家当。
#[derive(Clone, Copy, Debug)]
pub struct Asset {
    pub family: u8,
    pub item: u8,
    pub bytes: u32,
    pub version: u8,
    pub hash: u32,
    pub present: bool,
}

impl Asset {
    /// 空件。
    pub const fn blank() -> Asset {
        Asset {
            family: 0,
            item: 0,
            bytes: 0,
            version: 0,
            hash: 0,
            present: false,
        }
    }

    /// 族号在四族内、件号在一至本族容量。
    pub fn label_ok(&self) -> bool {
        self.family < FAMILIES as u8 && self.item >= 1 && self.item <= cap_of(self.family)
    }

    /// 在场且字节非零不越上界。
    pub fn body_ok(&self) -> bool {
        self.present && self.bytes > 0 && self.bytes <= BYTES_MAX
    }

    /// 版本落在一至三。
    pub fn version_ok(&self) -> bool {
        self.version >= 1 && self.version <= MAX_VERSION
    }

    /// 摘要可由四字段复算。
    pub fn hash_ok(&self) -> bool {
        self.hash == hash_of(self.family, self.item, self.bytes, self.version)
    }
}

/// 四族家当账本。
#[derive(Clone, Copy, Debug)]
pub struct FamilyLedger {
    assets: [Asset; ITEMS],
    len: usize,
    accepts: u32,
    rejects: u32,
    lane_last: [u8; FAMILIES],
    lane_count: [u8; FAMILIES],
    frozen: bool,
    settled: Option<u64>,
}

impl FamilyLedger {
    /// 空账。
    pub const fn new() -> FamilyLedger {
        FamilyLedger {
            assets: [Asset::blank(); ITEMS],
            len: 0,
            accepts: 0,
            rejects: 0,
            lane_last: [0u8; FAMILIES],
            lane_count: [0u8; FAMILIES],
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收件数。
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

    /// 第 i 件（收件序）。
    pub fn asset_at(&self, i: usize) -> Option<Asset> {
        if i >= self.len {
            return None;
        }
        self.assets.get(i).copied()
    }

    /// 族内游标。
    pub fn lane_last(&self, fam: u8) -> Option<u8> {
        self.lane_last.get(fam as usize).copied()
    }

    /// 族内件数。
    pub fn lane_count(&self, fam: u8) -> Option<u8> {
        self.lane_count.get(fam as usize).copied()
    }

    /// 按族与件号查收件序。
    pub fn find(&self, fam: u8, item: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.assets.get(i) {
                if a.family == fam && a.item == item {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收件：冻结 → 族号 → 件号 → 重号 → 跳号 → 在场 → 字节 → 版本 → 摘要。
    pub fn register(&mut self, a: Asset) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(AS_EFROZEN));
        }
        if a.family >= FAMILIES as u8 {
            return Err(self.fail(AS_EFAM));
        }
        let cap = cap_of(a.family);
        if a.item == 0 || a.item > cap {
            return Err(self.fail(AS_EITEM));
        }
        if self.find(a.family, a.item).is_some() {
            return Err(self.fail(AS_EDUP));
        }
        let last = match self.lane_last(a.family) {
            Some(v) => v,
            None => return Err(self.fail(AS_EFAM)),
        };
        if a.item != last.wrapping_add(1) {
            return Err(self.fail(AS_EORDER));
        }
        if !a.present {
            return Err(self.fail(AS_EABSENT));
        }
        if a.bytes == 0 || a.bytes > BYTES_MAX {
            return Err(self.fail(AS_EBYTES));
        }
        if !a.version_ok() {
            return Err(self.fail(AS_EHASH));
        }
        if !a.hash_ok() {
            return Err(self.fail(AS_EHASH));
        }
        match self.assets.get_mut(self.len) {
            Some(slot) => *slot = a,
            None => return Err(self.fail(AS_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        if let Some(v) = self.lane_last.get_mut(a.family as usize) {
            *v = a.item;
        }
        if let Some(v) = self.lane_count.get_mut(a.family as usize) {
            *v = v.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 按族与件号观察收件：由规格生成在场件。
    pub fn observe(&mut self, fam: u8, item: u8) -> Result<u64, u32> {
        if fam >= FAMILIES as u8 {
            return Err(self.fail(AS_EFAM));
        }
        if item == 0 || item > cap_of(fam) {
            return Err(self.fail(AS_EITEM));
        }
        let a = asset_spec(fam, item);
        self.register(a)
    }

    /// 全账字节合计。
    pub fn bytes_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.assets.get(i) {
                sum = sum.wrapping_add(a.bytes);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 全账在场件数。
    pub fn present_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.assets.get(i) {
                if a.present {
                    n = n.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 某族是否恰为件号一至容量、逐件在场且摘要可复算。
    pub fn lane_ok(&self, fam: u8) -> bool {
        if fam >= FAMILIES as u8 {
            return false;
        }
        let cap = cap_of(fam);
        let mut item = 1u8;
        while item <= cap {
            let idx = match self.find(fam, item) {
                Some(i) => i,
                None => return false,
            };
            match self.assets.get(idx) {
                Some(a) => {
                    if !a.present || !a.body_ok() || !a.version_ok() || !a.hash_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            item = item.wrapping_add(1);
        }
        true
    }

    /// 四族齐备：各族件数等于容量、无多余件、无漏件。
    pub fn families_full(&self) -> bool {
        let mut fam = 0u8;
        let mut total = 0u32;
        while (fam as usize) < FAMILIES {
            let cap = cap_of(fam);
            match self.lane_count(fam) {
                Some(c) => {
                    if c != cap {
                        return false;
                    }
                    total = total.wrapping_add(c as u32);
                }
                None => return false,
            }
            if !self.lane_ok(fam) {
                return false;
            }
            fam = fam.wrapping_add(1);
        }
        total as usize == ITEMS && self.len == ITEMS
    }

    /// 版本戳全在域内且已入摘要。
    pub fn versions_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.assets.get(i) {
                Some(a) => {
                    if !a.version_ok() || !a.hash_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.assets.get(i) {
                h = (h ^ a.family as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.item as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.bytes as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.version as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.hash as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ if a.present { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let mut fam = 0usize;
        while fam < FAMILIES {
            if let Some(v) = self.lane_last.get(fam) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.lane_count.get(fam) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            fam = fam.wrapping_add(1);
        }
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：四族齐、逐件在场、摘要可复算、字节合计对平规格。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.families_full() {
            return Err(AS_ECOUNT);
        }
        if !self.versions_ok() {
            return Err(AS_EHASH);
        }
        let mut spec_bytes = 0u32;
        let mut fam = 0u8;
        while (fam as usize) < FAMILIES {
            let mut item = 1u8;
            while item <= cap_of(fam) {
                spec_bytes = spec_bytes.wrapping_add(asset_spec(fam, item).bytes);
                item = item.wrapping_add(1);
            }
            fam = fam.wrapping_add(1);
        }
        if self.bytes_total() != spec_bytes {
            return Err(AS_EBYTES);
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(AS_EFROZEN));
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

/// 族首件在全场平铺序号中的偏移。
pub fn base_of(fam: u8) -> usize {
    match fam {
        0 => 0,
        1 => 6,
        2 => 12,
        3 => 17,
        _ => 0,
    }
}

/// 族与件号换算为全场平铺序号（一基），越界为零。
pub fn flat_of(fam: u8, item: u8) -> usize {
    if fam >= FAMILIES as u8 || item == 0 || item > cap_of(fam) {
        return 0;
    }
    base_of(fam).wrapping_add(item as usize)
}

/// 平铺序号规格：族、件号、字节、版本。
pub fn spec_of(id: usize) -> (u8, u8, u32, u8) {
    if id == 0 || id > ITEMS {
        return (0, 0, 0, 0);
    }
    let (fam, item) = if id <= 6 {
        (0u8, id as u8)
    } else if id <= 12 {
        (1u8, (id - 6) as u8)
    } else if id <= 17 {
        (2u8, (id - 12) as u8)
    } else {
        (3u8, (id - 17) as u8)
    };
    let bytes = 128u32
        .wrapping_add((id as u32) * 16)
        .wrapping_add((fam as u32) * 7);
    let version = ((id % MAX_VERSION as usize) as u8).wrapping_add(1);
    (fam, item, bytes, version)
}

/// 按族与件号生成规格件。
pub fn asset_spec(fam: u8, item: u8) -> Asset {
    let mut a = Asset::blank();
    let flat = flat_of(fam, item);
    if flat == 0 {
        return a;
    }
    let spec = spec_of(flat);
    a.family = fam;
    a.item = item;
    a.bytes = spec.2;
    a.version = spec.3;
    a.hash = hash_of(fam, item, a.bytes, a.version);
    a.present = true;
    a
}

/// 构建标准全家当：逐件轮转四族交织收件。
pub fn build_ledger() -> FamilyLedger {
    let mut l = FamilyLedger::new();
    let mut item = 1u8;
    while item <= 6 {
        let mut fam = 0u8;
        while (fam as usize) < FAMILIES {
            if item <= cap_of(fam) {
                let _ = l.observe(fam, item);
            }
            fam = fam.wrapping_add(1);
        }
        item = item.wrapping_add(1);
    }
    l
}

/// 四探针：族/件号越界、重号/跳号、缺场/零字节/摘要、冻结后收。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = FamilyLedger::new();
    let _ = a.observe(0, 1);
    let mut bad_fam = asset_spec(0, 1);
    bad_fam.family = FAMILIES as u8;
    if a.register(bad_fam) == Err(AS_EFAM) && a.observe(0, cap_of(0) + 1) == Err(AS_EITEM) {
        r[0] = true;
    }

    let mut b = FamilyLedger::new();
    let b1 = asset_spec(0, 1);
    let _ = b.register(b1);
    if b.register(b1) == Err(AS_EDUP) && b.observe(0, 3) == Err(AS_EORDER) {
        r[1] = true;
    }

    let mut c = FamilyLedger::new();
    let _ = c.observe(1, 1);
    let mut gone = asset_spec(1, 2);
    gone.present = false;
    let mut zero = asset_spec(1, 2);
    zero.bytes = 0;
    let mut forged = asset_spec(1, 2);
    forged.hash ^= 1;
    if c.register(gone) == Err(AS_EABSENT)
        && c.register(zero) == Err(AS_EBYTES)
        && c.register(forged) == Err(AS_EHASH)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(AS_EFROZEN)
        && d.register(asset_spec(0, 1)) == Err(AS_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11174");
    let base = build_ledger();

    let mut lanes = true;
    let mut fam = 0u8;
    while (fam as usize) < FAMILIES {
        let cap = cap_of(fam);
        if base.lane_last(fam) != Some(cap) || base.lane_count(fam) != Some(cap) {
            lanes = false;
        }
        let mut item = 1u8;
        while item <= cap {
            if base.find(fam, item).is_none() {
                lanes = false;
            }
            item = item.wrapping_add(1);
        }
        fam = fam.wrapping_add(1);
    }
    s.add(
        "S1-逐件对平",
        base.len() == ITEMS
            && base.accepted() == ITEMS as u32
            && base.rejected() == 0
            && lanes,
        "二十二件全收，四族件号与件数逐族对平",
    );

    s.add(
        "S2-族内连续",
        base.lane_ok(0) && base.lane_ok(1) && base.lane_ok(2) && base.lane_ok(3),
        "四族各自从一号起逐件无跳号",
    );

    s.add(
        "S3-实体核销零漏件",
        base.present_count() == ITEMS as u32 && base.len() == ITEMS,
        "二十二件全部在场，无一漏登",
    );

    s.add(
        "S4-四族齐全",
        base.families_full(),
        "四族件数各等于容量且无多余无缺漏",
    );

    let mut spec_bytes = 0u32;
    let mut id = 1usize;
    while id <= ITEMS {
        spec_bytes = spec_bytes.wrapping_add(spec_of(id).2);
        id = id.wrapping_add(1);
    }
    s.add(
        "S5-字节对平",
        base.bytes_total() == spec_bytes,
        "全家当字节合计等于规格合计",
    );

    let mut per_item = true;
    let mut flat = 1usize;
    while flat <= ITEMS {
        let (f, it, bytes, version) = spec_of(flat);
        match base.find(f, it) {
            Some(idx) => match base.asset_at(idx) {
                Some(a) => {
                    if a.bytes != bytes || a.version != version || !a.hash_ok() {
                        per_item = false;
                    }
                }
                None => per_item = false,
            },
            None => per_item = false,
        }
        flat = flat.wrapping_add(1);
    }
    s.add("S6-逐件规格对平", per_item, "每件字节版本摘要均等于规格");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(AS_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放四族逐件对平且指纹一致",
    );

    s.add(
        "S10-版本戳在域内",
        base.versions_ok(),
        "每件版本落在一至三且已入摘要",
    );

    let mut la = FamilyLedger::new();
    let _ = la.observe(0, 1);
    let mut bad_fam = asset_spec(0, 1);
    bad_fam.family = FAMILIES as u8;
    let f0 = la.register(bad_fam);
    let f1 = la.observe(0, cap_of(0) + 1);
    s.add(
        "X11-族件越界",
        f0 == Err(AS_EFAM) && f1 == Err(AS_EITEM) && la.accepted() == 1 && la.rejected() == 2,
        "族号四与件号七分别越界拒收",
    );

    let mut lb = FamilyLedger::new();
    let b1 = asset_spec(0, 1);
    let _ = lb.register(b1);
    let dup = lb.register(b1);
    let gap = lb.observe(0, 3);
    s.add(
        "X12-重号跳号",
        dup == Err(AS_EDUP) && gap == Err(AS_EORDER) && lb.accepted() == 1 && lb.rejected() == 2,
        "重收一号判重号、直收三号判跳号",
    );

    let mut lc = FamilyLedger::new();
    let _ = lc.observe(1, 1);
    let mut gone = asset_spec(1, 2);
    gone.present = false;
    let mut zero = asset_spec(1, 2);
    zero.bytes = 0;
    let mut forged = asset_spec(1, 2);
    forged.hash ^= 1;
    let g = lc.register(gone);
    let z = lc.register(zero);
    let h = lc.register(forged);
    s.add(
        "X13-缺场零字节摘要",
        g == Err(AS_EABSENT)
            && z == Err(AS_EBYTES)
            && h == Err(AS_EHASH)
            && lc.accepted() == 1
            && lc.rejected() == 3,
        "缺场、零字节、摘要不符分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11174_roster_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), ITEMS);
        assert_eq!(l.accepted(), ITEMS as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.families_full());
        assert_eq!(l.present_count(), ITEMS as u32);
        assert!(l.versions_ok());
        assert_eq!(l.lane_count(0), Some(6));
        assert_eq!(l.lane_count(2), Some(5));
        assert_eq!(l.lane_last(3), Some(5));
        assert!(l.asset_at(ITEMS).is_none());
        assert!(l.find(0, 1).is_some());
        assert!(l.find(0, 7).is_none());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11174_spec_map() {
        assert_eq!(spec_of(0), (0, 0, 0, 0));
        assert_eq!(spec_of(ITEMS + 1), (0, 0, 0, 0));
        assert_eq!(flat_of(0, 1), 1);
        assert_eq!(flat_of(1, 1), 7);
        assert_eq!(flat_of(2, 1), 13);
        assert_eq!(flat_of(3, 5), 22);
        assert_eq!(flat_of(4, 1), 0);
        let (f, i, _, v) = spec_of(22);
        assert_eq!((f, i), (3, 5));
        assert!(v >= 1 && v <= MAX_VERSION);
        let mut sum = 0u32;
        let mut id = 1usize;
        while id <= ITEMS {
            sum = sum.wrapping_add(asset_spec(spec_of(id).0, spec_of(id).1).bytes);
            id = id.wrapping_add(1);
        }
        assert_eq!(build_ledger().bytes_total(), sum);
    }

    #[test]
    fn f11174_gates() {
        let mut a = FamilyLedger::new();
        assert!(a.observe(0, 1).is_ok());
        assert!(a.observe(1, 1).is_ok());
        assert_eq!(a.observe(0, 1), Err(AS_EDUP));
        assert_eq!(a.observe(0, 4), Err(AS_EORDER));
        assert_eq!(a.observe(FAMILIES as u8, 1), Err(AS_EFAM));
        assert_eq!(a.observe(3, 6), Err(AS_EITEM));
        let mut forged = asset_spec(0, 2);
        forged.version = MAX_VERSION + 1;
        assert_eq!(a.register(forged), Err(AS_EHASH));
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 5);
    }

    #[test]
    fn f11174_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(AS_EFROZEN));
        assert_eq!(l.register(asset_spec(0, 1)), Err(AS_EFROZEN));
    }

    #[test]
    fn f11174_checks_green() {
        let s = run_unx11174_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
