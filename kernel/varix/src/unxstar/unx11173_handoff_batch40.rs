//! UNX-F11173 移交包二件（批册 40 册打包清单）（0x1f490 段）
//!
//! 模型：四十册批册分装八包，单包容量六册，奇包装六册、偶包装四册，八包
//! 合计四十册。每包登记包号、首册号、成员掩码、册数、各册实体号与包重。
//! 规则：包号一至八且严格递增；首册号须接上一包末册之后，首包从一号起；
//! 掩码必须恰为容量的低位置满，不得有残位；册数须等于掩码位数；实体号
//! 全包唯一且非零；包重须等于规格值；包封由包号、掩码与包重复算。
//! 收包记受理，越界、重复、倒序、首册断档、掩码残位、实体号冲突与重量
//! 不符一律拒止。终核要求四十个册号全覆盖且互不重叠，再折叠为单一指纹；
//! 二次冻结返回冻结码。探针的掩码残位与重复实体号由规则函数判定。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 批册移交包）。
const SEG_BASE: u32 = 0x1f490;

/// 包号越界。
pub const BP_ENO: u32 = SEG_BASE + 0x01;
/// 包号重复。
pub const BP_EDUP: u32 = SEG_BASE + 0x02;
/// 包号倒序。
pub const BP_EORDER: u32 = SEG_BASE + 0x03;
/// 首册号不接前包。
pub const BP_EFIRST: u32 = SEG_BASE + 0x04;
/// 掩码残位或册数不符。
pub const BP_EMASK: u32 = SEG_BASE + 0x05;
/// 实体号为零或跨包重复。
pub const BP_ESTOCK: u32 = SEG_BASE + 0x06;
/// 包重不符规格。
pub const BP_EWEIGHT: u32 = SEG_BASE + 0x07;
/// 册数覆盖不齐。
pub const BP_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const BP_EFROZEN: u32 = SEG_BASE + 0x09;

/// 包数。
pub const BUNDLES: usize = 8;
/// 单包容量。
pub const MEMBERS: usize = 6;
/// 册总数。
pub const VOLS: usize = 40;
/// 奇包册数。
pub const ODD_SIZE: u8 = 6;
/// 偶包册数。
pub const EVEN_SIZE: u8 = 4;
/// 包重基数。
pub const WEIGHT_BASE: u32 = 1000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 第 no 包的册数：奇六偶四。
pub fn bundle_size(no: u8) -> u8 {
    if no == 0 || no as usize > BUNDLES {
        return 0;
    }
    if no & 1 == 1 {
        ODD_SIZE
    } else {
        EVEN_SIZE
    }
}

/// 第 no 包的首册号（首包为一）。
pub fn first_id(no: u8) -> u8 {
    if no == 0 || no as usize > BUNDLES {
        return 0;
    }
    let mut first = 1u8;
    let mut k = 1u8;
    while k < no {
        first = first.wrapping_add(bundle_size(k));
        k = k.wrapping_add(1);
    }
    first
}

/// 包封：包号、掩码与包重卷成 32 位。
pub fn seal_of(no: u8, mask: u8, weight: u32) -> u32 {
    let mut x = (no as u32).wrapping_mul(0x9e37_79b1) ^ (mask as u32);
    x = x.rotate_left(9) ^ weight;
    let y = x.wrapping_mul(0x85eb_ca6b);
    if y == 0 {
        1
    } else {
        y
    }
}

/// 一包批册。
#[derive(Clone, Copy, Debug)]
pub struct Bundle {
    pub no: u8,
    pub first: u8,
    pub mask: u8,
    pub count: u8,
    pub weight: u32,
    pub seal: u32,
    pub stocks: [u16; MEMBERS],
}

impl Bundle {
    /// 空包。
    pub const fn blank() -> Bundle {
        Bundle {
            no: 0,
            first: 0,
            mask: 0,
            count: 0,
            weight: 0,
            seal: 0,
            stocks: [0u16; MEMBERS],
        }
    }

    /// 第 i 个实体号。
    pub fn stock_at(&self, i: usize) -> Option<u16> {
        self.stocks.get(i).copied()
    }

    /// 容量对应的满掩码。
    pub fn full_mask(&self) -> u8 {
        let size = bundle_size(self.no);
        if size == 0 || size as usize > MEMBERS {
            return 0;
        }
        ((1u16 << size) - 1) as u8
    }

    /// 掩码恰为满位且册数等于位数。
    pub fn mask_ok(&self) -> bool {
        let full = self.full_mask();
        if full == 0 || self.mask != full {
            return false;
        }
        let bits = self.mask.count_ones() as u8;
        self.count == bits && bits as usize == bundle_size(self.no) as usize
    }

    /// 包内前 count 个实体号非零。
    pub fn stocks_ok(&self) -> bool {
        if self.count as usize > MEMBERS {
            return false;
        }
        let mut i = 0usize;
        while i < self.count as usize {
            match self.stocks.get(i) {
                Some(s) => {
                    if *s == 0 {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 包重等于规格。
    pub fn weight_ok(&self) -> bool {
        self.weight == spec_of(self.no as usize).2
    }

    /// 包封可复算。
    pub fn seal_ok(&self) -> bool {
        self.seal == seal_of(self.no, self.mask, self.weight)
    }
}

/// 批册打包账本。
#[derive(Clone, Copy, Debug)]
pub struct BatchLedger {
    bundles: [Bundle; BUNDLES],
    len: usize,
    accepts: u32,
    rejects: u32,
    frozen: bool,
    settled: Option<u64>,
}

impl BatchLedger {
    /// 空账。
    pub const fn new() -> BatchLedger {
        BatchLedger {
            bundles: [Bundle::blank(); BUNDLES],
            len: 0,
            accepts: 0,
            rejects: 0,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收包数。
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

    /// 末包。
    pub fn last(&self) -> Option<Bundle> {
        if self.len == 0 {
            return None;
        }
        self.bundles.get(self.len - 1).copied()
    }

    /// 第 i 包。
    pub fn bundle_at(&self, i: usize) -> Option<Bundle> {
        if i >= self.len {
            return None;
        }
        self.bundles.get(i).copied()
    }

    /// 按包号查序号。
    pub fn find(&self, no: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bundles.get(i) {
                if b.no == no {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 实体号是否已被别的包占用。
    fn stock_taken(&self, stock: u16, skip_no: u8) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bundles.get(i) {
                if b.no != skip_no {
                    let mut j = 0usize;
                    while j < b.count as usize {
                        if let Some(s) = b.stocks.get(j) {
                            if *s == stock {
                                return true;
                            }
                        }
                        j = j.wrapping_add(1);
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        false
    }

    /// 收包：冻结 → 越界 → 重复 → 倒序 → 首册 → 掩码 → 实体 → 重量 → 封。
    pub fn register(&mut self, b: Bundle) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(BP_EFROZEN));
        }
        if b.no == 0 || b.no as usize > BUNDLES {
            return Err(self.fail(BP_ENO));
        }
        if self.find(b.no).is_some() {
            return Err(self.fail(BP_EDUP));
        }
        let expected_first = match self.last() {
            Some(prev) => {
                if b.no <= prev.no {
                    return Err(self.fail(BP_EORDER));
                }
                prev.first.wrapping_add(prev.count)
            }
            None => 1,
        };
        if b.first != expected_first {
            return Err(self.fail(BP_EFIRST));
        }
        if !b.mask_ok() {
            return Err(self.fail(BP_EMASK));
        }
        if !b.stocks_ok() {
            return Err(self.fail(BP_ESTOCK));
        }
        let mut i = 0usize;
        while i < b.count as usize {
            match b.stocks.get(i) {
                Some(s) => {
                    if self.stock_taken(*s, b.no) {
                        return Err(self.fail(BP_ESTOCK));
                    }
                }
                None => return Err(self.fail(BP_ESTOCK)),
            }
            i = i.wrapping_add(1);
        }
        if !b.weight_ok() {
            return Err(self.fail(BP_EWEIGHT));
        }
        if !b.seal_ok() {
            return Err(self.fail(BP_EWEIGHT));
        }
        match self.bundles.get_mut(self.len) {
            Some(slot) => *slot = b,
            None => return Err(self.fail(BP_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按包号观察收包：规格与实体号按全局序生成。
    pub fn observe(&mut self, no: usize) -> Result<u64, u32> {
        if no == 0 || no > BUNDLES {
            return Err(self.fail(BP_ENO));
        }
        let n = no as u8;
        let mut b = Bundle::blank();
        b.no = n;
        b.first = first_id(n);
        b.count = bundle_size(n);
        b.mask = ((1u16 << b.count) - 1) as u8;
        b.weight = spec_of(no).2;
        let base = b.first.wrapping_sub(1);
        let mut i = 0usize;
        while i < b.count as usize {
            if let Some(s) = b.stocks.get_mut(i) {
                *s = (base as u16).wrapping_add(i as u16).wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        b.seal = seal_of(b.no, b.mask, b.weight);
        self.register(b)
    }

    /// 全账册数。
    pub fn volumes(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bundles.get(i) {
                n = n.wrapping_add(b.count as u32);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 全账包重合计。
    pub fn weight_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bundles.get(i) {
                n = n.wrapping_add(b.weight);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 册号覆盖位图（第 k 册占第 k-1 位）。
    pub fn coverage(&self) -> u64 {
        let mut bits = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bundles.get(i) {
                let mut k = 0usize;
                while k < b.count as usize {
                    let id = (b.first as usize).wrapping_add(k);
                    if id >= 1 && id <= 64 {
                        bits |= 1u64 << (id - 1);
                    }
                    k = k.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        bits
    }

    /// 四十册全覆盖。
    pub fn all_covered(&self) -> bool {
        self.coverage() == (1u64 << VOLS) - 1
    }

    /// 实体号全账唯一且非零。
    pub fn stocks_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            let bi = match self.bundles.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let mut a = 0usize;
            while a < bi.count as usize {
                let sa = match bi.stocks.get(a) {
                    Some(v) => *v,
                    None => return false,
                };
                if sa == 0 {
                    return false;
                }
                let mut j = 0usize;
                while j < self.len {
                    let bj = match self.bundles.get(j) {
                        Some(v) => *v,
                        None => return false,
                    };
                    let mut c = 0usize;
                    while c < bj.count as usize {
                        let sb = match bj.stocks.get(c) {
                            Some(v) => *v,
                            None => return false,
                        };
                        if i != j || a != c {
                            if sa == sb {
                                return false;
                            }
                        }
                        c = c.wrapping_add(1);
                    }
                    j = j.wrapping_add(1);
                }
                a = a.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 每包号连续、首册接续、掩码满位、重量与封可复算。
    pub fn bundles_ok(&self) -> bool {
        let mut expected = 1u8;
        let mut i = 0usize;
        while i < self.len {
            match self.bundles.get(i) {
                Some(b) => {
                    if b.no as usize != i.wrapping_add(1) {
                        return false;
                    }
                    if b.first != expected {
                        return false;
                    }
                    if !b.mask_ok() || !b.stocks_ok() || !b.weight_ok() || !b.seal_ok() {
                        return false;
                    }
                    expected = b.first.wrapping_add(b.count);
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
            if let Some(b) = self.bundles.get(i) {
                h = (h ^ b.no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.first as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.mask as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.count as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.weight as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ b.seal as u64).wrapping_mul(FNV_PRIME);
                let mut j = 0usize;
                while j < MEMBERS {
                    if let Some(s) = b.stocks.get(j) {
                        h = (h ^ *s as u64).wrapping_mul(FNV_PRIME);
                    }
                    j = j.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：八包齐、包号连续、逐包规则全过、四十册全覆盖。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != BUNDLES {
            return Err(BP_ECOUNT);
        }
        if !self.bundles_ok() {
            return Err(BP_EFIRST);
        }
        if !self.stocks_unique() {
            return Err(BP_ESTOCK);
        }
        if self.volumes() as usize != VOLS || !self.all_covered() {
            return Err(BP_ECOUNT);
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(BP_EFROZEN));
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

/// 第 no 包规格：首册、册数、包重。
pub fn spec_of(no: usize) -> (u8, u8, u32) {
    if no == 0 || no > BUNDLES {
        return (0, 0, 0);
    }
    let n = no as u8;
    let first = first_id(n);
    let size = bundle_size(n);
    let weight = WEIGHT_BASE
        .wrapping_add((first as u32) * 7)
        .wrapping_add((size as u32) * 13);
    (first, size, weight)
}

/// 规格生成一包（实体号从首册起顺序编）。
fn valid_bundle(no: u8) -> Bundle {
    let mut b = Bundle::blank();
    if no == 0 || no as usize > BUNDLES {
        return b;
    }
    let spec = spec_of(no as usize);
    b.no = no;
    b.first = spec.0;
    b.count = spec.1;
    b.mask = ((1u16 << b.count) - 1) as u8;
    b.weight = spec.2;
    let mut i = 0usize;
    while i < b.count as usize {
        if let Some(s) = b.stocks.get_mut(i) {
            *s = (b.first as u16).wrapping_add(i as u16);
        }
        i = i.wrapping_add(1);
    }
    b.seal = seal_of(b.no, b.mask, b.weight);
    b
}

/// 重定基包的册：保留包号与规格重量，首册与实体号改从给定处起。
fn rebased(no: u8, first: u8) -> Bundle {
    let mut b = valid_bundle(no);
    b.first = first;
    let mut i = 0usize;
    while i < b.count as usize {
        if let Some(st) = b.stocks.get_mut(i) {
            *st = (first as u16).wrapping_add(i as u16);
        }
        i = i.wrapping_add(1);
    }
    b.seal = seal_of(b.no, b.mask, b.weight);
    b
}

/// 构建标准批册账：八包顺序收包。
pub fn build_ledger() -> BatchLedger {
    let mut l = BatchLedger::new();
    let mut no = 1usize;
    while no <= BUNDLES {
        let _ = l.observe(no);
        no = no.wrapping_add(1);
    }
    l
}

/// 四探针：越界、重复/倒序、掩码残位/实体冲突/重量不符、冻结后收。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = BatchLedger::new();
    let _ = a.register(valid_bundle(1));
    if a.register(valid_bundle(0)) == Err(BP_ENO)
        && a.register(valid_bundle((BUNDLES + 1) as u8)) == Err(BP_ENO)
    {
        r[0] = true;
    }

    let mut b = BatchLedger::new();
    let lead = rebased(3, 1);
    let _ = b.register(lead);
    if b.register(lead) == Err(BP_EDUP) && b.register(valid_bundle(2)) == Err(BP_EORDER) {
        r[1] = true;
    }

    let mut c = BatchLedger::new();
    let _ = c.register(valid_bundle(1));
    let mut masked = valid_bundle(2);
    masked.mask ^= 1;
    let mut doubled = valid_bundle(2);
    if let Some(s) = doubled.stocks.get_mut(0) {
        *s = 1;
    }
    let mut heavy = valid_bundle(2);
    heavy.weight = heavy.weight.wrapping_add(1);
    if c.register(masked) == Err(BP_EMASK)
        && c.register(doubled) == Err(BP_ESTOCK)
        && c.register(heavy) == Err(BP_EWEIGHT)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(BP_EFROZEN)
        && d.register(valid_bundle(1)) == Err(BP_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11173");
    let base = build_ledger();

    let mut packed = base.len() == BUNDLES && base.accepted() == BUNDLES as u32 && base.rejected() == 0;
    let mut i = 0usize;
    while i < base.len() {
        match base.bundle_at(i) {
            Some(b) => {
                let spec = spec_of(i.wrapping_add(1));
                if b.no as usize != i.wrapping_add(1) || b.first != spec.0 || b.count != spec.1 {
                    packed = false;
                }
            }
            None => packed = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("S1-逐包规格对平", packed, "八包收齐，逐包首册与册数等于规格");

    let mut expected = 1u8;
    let mut chain = true;
    let mut j = 0usize;
    while j < base.len() {
        match base.bundle_at(j) {
            Some(b) => {
                if b.first != expected {
                    chain = false;
                }
                expected = b.first.wrapping_add(b.count);
            }
            None => chain = false,
        }
        j = j.wrapping_add(1);
    }
    s.add(
        "S2-首册接续",
        chain && expected as usize == VOLS + 1,
        "首包从一号起，逐包首册接前包末册，末包止于四十",
    );

    s.add(
        "S3-四十册覆盖",
        base.volumes() as usize == VOLS
            && base.all_covered()
            && base.coverage().count_ones() as usize == VOLS,
        "册数合计四十且册号位图四十位全置、无重叠",
    );

    let mut masks = true;
    let mut k = 0usize;
    while k < base.len() {
        match base.bundle_at(k) {
            Some(b) => {
                if b.mask != b.full_mask() || !b.mask_ok() {
                    masks = false;
                }
            }
            None => masks = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S4-掩码满位", masks, "每包掩码恰为容量低位置满");

    s.add(
        "S5-实体号唯一",
        base.stocks_unique() && base.volumes() as usize == VOLS,
        "四十个实体号全账互异且非零",
    );

    let mut spec_w = 0u32;
    let mut n = 1usize;
    while n <= BUNDLES {
        spec_w = spec_w.wrapping_add(spec_of(n).2);
        n = n.wrapping_add(1);
    }
    s.add(
        "S6-包重对平",
        base.weight_total() == spec_w,
        "八包包重合计等于规格合计",
    );

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(BP_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放八包逐包对平且指纹一致",
    );

    let mut seals = true;
    let mut m = 0usize;
    while m < base.len() {
        match base.bundle_at(m) {
            Some(b) => {
                if !b.seal_ok() || b.seal == 0 {
                    seals = false;
                }
            }
            None => seals = false,
        }
        m = m.wrapping_add(1);
    }
    s.add("S10-包封可复算", seals, "每包包封非零且由包号掩码包重复算");

    let mut la = BatchLedger::new();
    let _ = la.register(valid_bundle(1));
    let r0 = la.register(valid_bundle(0));
    let r1 = la.register(valid_bundle((BUNDLES + 1) as u8));
    s.add(
        "X11-越界拒收",
        r0 == Err(BP_ENO) && r1 == Err(BP_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "包号 0 与 9 均判越界",
    );

    let mut lb = BatchLedger::new();
    let lead = rebased(3, 1);
    let first = lb.register(lead);
    let dup = lb.register(lead);
    let ord = lb.register(valid_bundle(2));
    s.add(
        "X12-重复倒序",
        first.is_ok()
            && dup == Err(BP_EDUP)
            && ord == Err(BP_EORDER)
            && lb.accepted() == 1
            && lb.rejected() == 2,
        "先收三号，重收判重复、补收二号判倒序",
    );

    let mut lc = BatchLedger::new();
    let _ = lc.register(valid_bundle(1));
    let mut masked = valid_bundle(2);
    masked.mask ^= 1;
    let mut doubled = valid_bundle(2);
    if let Some(st) = doubled.stocks.get_mut(0) {
        *st = 1;
    }
    let mut heavy = valid_bundle(2);
    heavy.weight = heavy.weight.wrapping_add(1);
    let m1 = lc.register(masked);
    let m2 = lc.register(doubled);
    let m3 = lc.register(heavy);
    s.add(
        "X13-掩码实体重量",
        m1 == Err(BP_EMASK)
            && m2 == Err(BP_ESTOCK)
            && m3 == Err(BP_EWEIGHT)
            && lc.accepted() == 1
            && lc.rejected() == 3,
        "掩码残位、实体号冲突、包重不符分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11173_batch_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), BUNDLES);
        assert_eq!(l.accepted(), BUNDLES as u32);
        assert_eq!(l.rejected(), 0);
        assert_eq!(l.volumes() as usize, VOLS);
        assert!(l.all_covered());
        assert!(l.stocks_unique());
        assert!(l.bundles_ok());
        assert!(l.weight_total() > 0);
        assert_eq!(l.bundle_at(0).map(|b| b.first), Some(1));
        assert_eq!(l.last().map(|b| b.count), Some(EVEN_SIZE));
        assert!(l.bundle_at(BUNDLES).is_none());
        assert!(l.find(8).is_some());
        assert!(l.find(9).is_none());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11173_spec_shape() {
        assert_eq!(spec_of(0), (0, 0, 0));
        assert_eq!(spec_of(BUNDLES + 1), (0, 0, 0));
        let mut total = 0u8;
        let mut no = 1usize;
        while no <= BUNDLES {
            let (first, size, weight) = spec_of(no);
            assert_eq!(first, total.wrapping_add(1));
            assert!(size == ODD_SIZE || size == EVEN_SIZE);
            assert!(weight > WEIGHT_BASE);
            total = total.wrapping_add(size);
            no = no.wrapping_add(1);
        }
        assert_eq!(total as usize, VOLS);
        assert_eq!(bundle_size(0), 0);
        assert_eq!(bundle_size(9), 0);
        assert_eq!(first_id(0), 0);
    }

    #[test]
    fn f11173_gates() {
        let mut a = BatchLedger::new();
        assert!(a.register(valid_bundle(1)).is_ok());
        let mut masked = valid_bundle(2);
        masked.mask ^= 1;
        assert_eq!(a.register(masked), Err(BP_EMASK));
        let mut doubled = valid_bundle(2);
        if let Some(st) = doubled.stocks.get_mut(1) {
            *st = 3;
        }
        assert_eq!(a.register(doubled), Err(BP_ESTOCK));
        let mut heavy = valid_bundle(2);
        heavy.weight = heavy.weight.wrapping_add(5);
        assert_eq!(a.register(heavy), Err(BP_EWEIGHT));
        let mut early = valid_bundle(2);
        early.first = 2;
        assert_eq!(a.register(early), Err(BP_EFIRST));
        assert_eq!(a.accepted(), 1);
        assert_eq!(a.rejected(), 4);
    }

    #[test]
    fn f11173_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        assert!(!l.is_frozen());
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(BP_EFROZEN));
        assert_eq!(l.register(valid_bundle(1)), Err(BP_EFROZEN));
    }

    #[test]
    fn f11173_checks_green() {
        let s = run_unx11173_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}


