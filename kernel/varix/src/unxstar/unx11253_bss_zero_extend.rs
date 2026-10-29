//! UNX-F11253 bss 扩展与匿名页清零（0x1f350 段）
//!
//! 本模型只关心 PT_LOAD 段的“文件影像区”和“内存尾部”之间的差额。
//! 当 p_memsz 大于 p_filesz 时，多出来的字节必须由内核以零页补齐；
//! 若文件尾端停在页中间，同一页内文件字节之后的残余也必须是零。
//! 模型把每页的“文件已覆盖字节数”算出来，剩余长度就是该页的零字节
//! 贡献，逐页求和必须严格等于 p_memsz - p_filesz，任何一页少算或
//! 多算都会让审计拒绝。读取接口以 0xa5 代表文件字节、0 代表匿名零，
//! 对尾部每个偏移逐一取字节，零比例必须达到 100%。
//! 拒绝条件包括：段为空、文件长度超过内存长度、文件偏移与虚拟地址
//! 页内错位、对齐值不是 2 的幂、地址上溢、越界读取。账本按编号登记
//! 十二个样本并逐一观测，冻结前要重放全部恒等式。

use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x1f350;

/// 编号非法。
pub const BZ_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const BZ_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const BZ_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const BZ_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const BZ_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const BZ_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const BZ_EBAL: u32 = SEG_BASE + 0x07;
/// 空段。
pub const BZ_EEMPTY: u32 = SEG_BASE + 0x08;
/// 文件长度超过内存长度。
pub const BZ_EOVFL: u32 = SEG_BASE + 0x09;
/// 页内错位。
pub const BZ_EMISALIGN: u32 = SEG_BASE + 0x0a;
/// 地址上溢。
pub const BZ_ERANGE: u32 = SEG_BASE + 0x0b;
/// 对齐值非法。
pub const BZ_EALIGN: u32 = SEG_BASE + 0x0c;
/// 越界读取。
pub const BZ_EOOB: u32 = SEG_BASE + 0x0d;
/// 审计差额。
pub const BZ_EAUDIT: u32 = SEG_BASE + 0x0e;

/// 页大小。
pub const PAGE: u64 = 0x1000;
/// 单段最多页数。
pub const MAX_PAGES: u32 = 8;
/// 样本个数。
pub const SAMPLES: usize = 12;
/// 文件字节的合成填充值。
pub const MARKER: u8 = 0xa5;
/// 扫描上限，避免巨段样本拖死审计。
pub const MAX_SCAN: u64 = 0x10000;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个 PT_LOAD 段样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BssCase {
    pub p_vaddr: u64,
    pub p_offset: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl BssCase {
    /// 空样本：无 bss。
    pub const fn blank() -> BssCase {
        BssCase { p_vaddr: 0x40_0000, p_offset: 0, p_filesz: 0, p_memsz: 0, p_align: PAGE }
    }

    /// 构造样本。
    pub const fn make(vaddr: u64, offset: u64, filesz: u64, memsz: u64, align: u64) -> BssCase {
        BssCase { p_vaddr: vaddr, p_offset: offset, p_filesz: filesz, p_memsz: memsz, p_align: align }
    }
}

/// 扩展后的几何形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub delta: u64,
    pub pages: u32,
    pub zpages: u32,
    pub split: bool,
    pub lo: u64,
    pub hi: u64,
}

impl Extent {
    /// 空形状。
    pub const fn none() -> Extent {
        Extent { delta: 0, pages: 0, zpages: 0, split: false, lo: 0, hi: 0 }
    }
}

/// 检查对齐值：0 或大于等于页且为 2 的幂。
pub fn align_ok(a: u64) -> bool {
    if a == 0 {
        return true;
    }
    if a < PAGE {
        return false;
    }
    a & (a - 1) == 0
}

/// 计算段的扩展形状。
pub fn extend(c: &BssCase) -> Result<Extent, u32> {
    if c.p_memsz == 0 {
        return Err(BZ_EEMPTY);
    }
    if c.p_filesz > c.p_memsz {
        return Err(BZ_EOVFL);
    }
    if c.p_vaddr & (PAGE - 1) != c.p_offset & (PAGE - 1) {
        return Err(BZ_EMISALIGN);
    }
    if !align_ok(c.p_align) {
        return Err(BZ_EALIGN);
    }
    let hi = match c.p_vaddr.checked_add(c.p_memsz) {
        Some(v) => v,
        None => return Err(BZ_ERANGE),
    };
    let rounded = match c.p_memsz.checked_add(PAGE - 1) {
        Some(v) => v,
        None => return Err(BZ_ERANGE),
    };
    let pages = (rounded / PAGE) as u32;
    if pages == 0 || pages > MAX_PAGES {
        return Err(BZ_ERANGE);
    }
    let lo = match c.p_vaddr.checked_add(c.p_filesz) {
        Some(v) => v,
        None => return Err(BZ_ERANGE),
    };
    let mut zpages = 0u32;
    let mut p = 0u32;
    while p < pages {
        let base = (p as u64).wrapping_mul(PAGE);
        if base >= c.p_filesz {
            zpages = zpages.wrapping_add(1);
        }
        p += 1;
    }
    let delta = c.p_memsz - c.p_filesz;
    let split = delta > 0 && lo & (PAGE - 1) != 0;
    Ok(Extent { delta, pages, zpages, split, lo, hi })
}

/// 某一页内 (文件已覆盖字节数, 必须为零的字节数)。页号越界返回 None。
pub fn page_parts(c: &BssCase, page: u32) -> Option<(u32, u32)> {
    let e = match extend(c) {
        Ok(v) => v,
        Err(_) => return None,
    };
    if page >= e.pages {
        return None;
    }
    let base = (page as u64).wrapping_mul(PAGE);
    if base >= c.p_memsz {
        return None;
    }
    let page_len = core::cmp::min(PAGE, c.p_memsz - base);
    let file_in = if c.p_filesz > base {
        core::cmp::min(page_len, c.p_filesz - base)
    } else {
        0
    };
    let zero_in = page_len - file_in;
    Some((file_in as u32, zero_in as u32))
}

/// 逐页审计：返回 (零字节总数, 含部分零的页数, 整页全零的页数)。
/// 零字节总数必须与 p_memsz - p_filesz 严格相等，否则拒绝。
pub fn audit(c: &BssCase) -> Result<(u64, u32, u32), u32> {
    let e = extend(c)?;
    let mut sum = 0u64;
    let mut partial = 0u32;
    let mut full = 0u32;
    let mut p = 0u32;
    while p < e.pages {
        let (file_in, zero_in) = match page_parts(c, p) {
            Some(v) => v,
            None => return Err(BZ_EAUDIT),
        };
        let base = (p as u64).wrapping_mul(PAGE);
        let page_len = core::cmp::min(PAGE, c.p_memsz - base) as u32;
        if file_in as u64 + zero_in as u64 != page_len as u64 {
            return Err(BZ_EAUDIT);
        }
        if zero_in > 0 && zero_in < page_len {
            partial = partial.wrapping_add(1);
        }
        if zero_in == page_len && page_len > 0 {
            full = full.wrapping_add(1);
        }
        sum = sum.wrapping_add(zero_in as u64);
        p += 1;
    }
    if sum != e.delta {
        return Err(BZ_EAUDIT);
    }
    Ok((sum, partial, full))
}

/// 读段内一个偏移：文件区返回 MARKER，匿名区返回 0，越界拒绝。
pub fn read_byte(c: &BssCase, off: u64) -> Result<u8, u32> {
    if off >= c.p_memsz {
        return Err(BZ_EOOB);
    }
    if off < c.p_filesz {
        Ok(MARKER)
    } else {
        Ok(0)
    }
}

/// 对尾部逐字节扫描，返回 (检查字节数, 为零字节数)。
pub fn scan_zero(c: &BssCase) -> Result<(u32, u32), u32> {
    extend(c)?;
    let mut off = c.p_filesz;
    let mut checked = 0u32;
    let mut zeroed = 0u32;
    while off < c.p_memsz {
        if checked as u64 >= MAX_SCAN {
            return Err(BZ_EAUDIT);
        }
        match read_byte(c, off) {
            Ok(v) => {
                checked = checked.wrapping_add(1);
                if v == 0 {
                    zeroed = zeroed.wrapping_add(1);
                }
            }
            Err(_) => return Err(BZ_EOOB),
        }
        off = off.wrapping_add(1);
    }
    if checked != zeroed {
        return Err(BZ_EAUDIT);
    }
    Ok((checked, zeroed))
}

/// 样本规格。
pub fn spec_of(id: usize) -> BssCase {
    match id {
        0 => BssCase::make(0x40_0000, 0, 0x2000, 0x2000, PAGE),
        1 => BssCase::make(0x40_0000, 0, 0x1fff, 0x2000, PAGE),
        2 => BssCase::make(0x40_0000, 0, 0x2000, 0x3000, PAGE),
        3 => BssCase::make(0x40_0010, 0x10, 0x2010, 0x3010, PAGE),
        4 => BssCase::make(0x40_0100, 0, 0x1000, 0x2000, PAGE),
        5 => BssCase::make(0x40_0000, 0, 0x2000, 0x1000, PAGE),
        6 => BssCase::make(0x40_0000, 0, 0, 0, PAGE),
        7 => BssCase::make(0x40_0000, 0, 0x1000, u64::MAX, PAGE),
        8 => BssCase::make(0x40_0000, 0, 0x1000, 0x2000, 3),
        9 => BssCase::make(0x40_0000, 0, 0x1000, 0x4000, PAGE),
        10 => BssCase::make(0x40_0000, 0, 0, 0x1800, PAGE),
        11 => BssCase::make(0x40_0000, 0, 1, 0x1001, PAGE),
        _ => BssCase::blank(),
    }
}

/// 样本期望：(错误码, 差额, 页数, 全零页数, 审计零字节和)。
pub fn expected_of(id: usize) -> (u32, u64, u32, u32, u32, u64) {
    match id {
        0 => (0, 0, 2, 0, 0, 0),
        1 => (0, 1, 2, 0, 0, 1),
        2 => (0, 0x1000, 3, 1, 1, 0x1000),
        3 => (0, 0x1000, 4, 1, 1, 0x1000),
        4 => (BZ_EMISALIGN, 0, 0, 0, 0, 0),
        5 => (BZ_EOVFL, 0, 0, 0, 0, 0),
        6 => (BZ_EEMPTY, 0, 0, 0, 0, 0),
        7 => (BZ_ERANGE, 0, 0, 0, 0, 0),
        8 => (BZ_EALIGN, 0, 0, 0, 0, 0),
        9 => (0, 0x3000, 4, 3, 3, 0x3000),
        10 => (0, 0x1800, 2, 2, 2, 0x1800),
        11 => (0, 0x1000, 2, 1, 1, 0x1000),
        _ => (BZ_EID, 0, 0, 0, 0, 0),
    }
}

/// 账本中的一格。
#[derive(Clone, Copy)]
struct BssSlot {
    code: u8,
    pages: u8,
    zpages: u8,
    full: u8,
    seen: bool,
    delta: u64,
    sum: u64,
    spec: BssCase,
}

impl BssSlot {
    const fn zero() -> BssSlot {
        BssSlot {
            code: 0,
            pages: 0,
            zpages: 0,
            full: 0,
            seen: false,
            delta: 0,
            sum: 0,
            spec: BssCase::blank(),
        }
    }
}

/// bss 扩展账本。
pub struct BssLedger {
    slots: [BssSlot; SAMPLES],
    n: usize,
    accepted: u32,
    rejected: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
}

impl BssLedger {
    /// 空账本。
    pub const fn new() -> BssLedger {
        BssLedger {
            slots: [BssSlot::zero(); SAMPLES],
            n: 0,
            accepted: 0,
            rejected: 0,
            observed: 0,
            matched: 0,
            frozen: false,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejected = self.rejected.wrapping_add(1);
        code
    }

    fn duplicate(&self, c: &BssCase) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.spec == *c {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn commit(
        &mut self,
        c: &BssCase,
        code: u32,
        delta: u64,
        pages: u32,
        zpages: u32,
        full: u32,
        sum: u64,
    ) -> Result<u64, u32> {
        let at = self.n;
        if at >= SAMPLES {
            return Err(self.fail(BZ_EID));
        }
        if let Some(slot) = self.slots.get_mut(at) {
            slot.code = if code == 0 { 0 } else { code.wrapping_sub(SEG_BASE) as u8 };
            slot.delta = delta;
            slot.pages = pages as u8;
            slot.zpages = zpages as u8;
            slot.full = full as u8;
            slot.sum = sum;
            slot.seen = false;
            slot.spec = *c;
        }
        self.n = at + 1;
        self.accepted = self.accepted.wrapping_add(1);
        Ok(self.fold())
    }

    /// 登记一个段样本。
    pub fn register(&mut self, c: &BssCase) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BZ_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(BZ_EDUP));
        }
        match extend(c) {
            Ok(e) => match audit(c) {
                Ok((sum, _partial, full)) => {
                    self.commit(c, 0, e.delta, e.pages, e.zpages, full, sum)
                }
                Err(code) => Err(self.fail(code)),
            },
            Err(code) => Err(self.fail(code)),
        }
    }

    /// 登记并核对预期错误码；期望失败照样占一格。
    pub fn register_ruled(&mut self, c: &BssCase, want: u32) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BZ_EFROZEN));
        }
        if self.duplicate(c) {
            return Err(self.fail(BZ_EDUP));
        }
        match extend(c) {
            Ok(e) => {
                if want != 0 {
                    return Err(self.fail(BZ_EMISMATCH));
                }
                match audit(c) {
                    Ok((sum, _p, full)) => {
                        self.commit(c, 0, e.delta, e.pages, e.zpages, full, sum)
                    }
                    Err(code) => Err(self.fail(code)),
                }
            }
            Err(code) => {
                if want == 0 || want != code {
                    return Err(self.fail(BZ_EMISMATCH));
                }
                self.commit(c, code, 0, 0, 0, 0, 0)
            }
        }
    }

    /// 观测一格：必须存在、未观测，且六个字段与期望一致。
    pub fn observe(&mut self, id: usize) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BZ_EFROZEN));
        }
        if id >= self.n {
            return Err(self.fail(BZ_ENOBS));
        }
        let got = match self.slots.get(id) {
            Some(s) => *s,
            None => return Err(self.fail(BZ_ENOBS)),
        };
        if got.seen {
            return Err(self.fail(BZ_EDUP));
        }
        let (wc, wd, wp, wz, wf, ws) = expected_of(id);
        self.observed = self.observed.wrapping_add(1);
        if let Some(slot) = self.slots.get_mut(id) {
            slot.seen = true;
        }
        let code_ok = if wc == 0 {
            got.code == 0
        } else {
            got.code == (wc.wrapping_sub(SEG_BASE)) as u8
        };
        if !code_ok
            || got.delta != wd
            || got.pages as u32 != wp
            || got.zpages as u32 != wz
            || got.full as u32 != wf
            || got.sum != ws
        {
            return Err(self.fail(BZ_EMISMATCH));
        }
        self.matched = self.matched.wrapping_add(1);
        Ok(self.fold())
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 成功登记次数。
    pub fn accepted(&self) -> u32 {
        self.accepted
    }

    /// 拒绝次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 观测次数。
    pub fn observed(&self) -> u32 {
        self.observed
    }

    /// 观测通过次数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 冻结标志。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 记录到非零错误码的格数。
    pub fn errors(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code != 0 {
                    c = c.wrapping_add(1);
                }
            }
            i += 1;
        }
        c
    }

    /// 成功样本的差额总和。
    pub fn total_delta(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(s.delta);
                }
            }
            i += 1;
        }
        t
    }

    /// 成功样本的页数总和。
    pub fn total_pages(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(s.pages as u32);
                }
            }
            i += 1;
        }
        t
    }

    /// 成功样本的全零页总数。
    pub fn total_zpages(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(s.zpages as u32);
                }
            }
            i += 1;
        }
        t
    }

    /// 成功样本的审计零字节总和。
    pub fn total_zero(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    t = t.wrapping_add(s.sum);
                }
            }
            i += 1;
        }
        t
    }

    /// 跨页拆分（文件尾端不在页边界且存在差额）的样本数。
    pub fn split_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    if let Ok(e) = extend(&s.spec) {
                        if e.split {
                            c = c.wrapping_add(1);
                        }
                    }
                }
            }
            i += 1;
        }
        c
    }

    /// 逐格重算审计，验证登记的差额、页数与零字节和未被篡改。
    pub fn audit_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.code == 0 {
                    let e = match extend(&s.spec) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    let (sum, _partial, full) = match audit(&s.spec) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if e.delta != s.delta
                        || e.pages != s.pages as u32
                        || e.zpages != s.zpages as u32
                        || full != s.full as u32
                        || sum != s.sum
                    {
                        return false;
                    }
                }
            }
            i += 1;
        }
        true
    }

    /// 冻结核对：全登记、全观测、全匹配、差额守恒、审计可重算。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if self.n != SAMPLES || self.accepted != SAMPLES as u32 {
            return Err(self.fail(BZ_ENOBS));
        }
        if self.observed != SAMPLES as u32 || self.matched != SAMPLES as u32 {
            return Err(self.fail(BZ_EMISMATCH));
        }
        if self.errors() != 5 || !self.audit_ok() {
            return Err(self.fail(BZ_EAUDIT));
        }
        if self.total_delta() != 0x7801
            || self.total_zero() != self.total_delta()
            || self.total_pages() != 19
            || self.total_zpages() != 8
            || self.split_count() != 3
        {
            return Err(self.fail(BZ_EBAL));
        }
        Ok(self.fold())
    }

    /// 冻结：先重放再上锁。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(BZ_EFROZEN));
        }
        self.replay()?;
        self.frozen = true;
        Ok(self.fold())
    }

    /// 对全部字段做 FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.n as u64);
        h = mix(h, self.accepted as u64);
        h = mix(h, self.rejected as u64);
        h = mix(h, self.observed as u64);
        h = mix(h, self.matched as u64);
        h = mix(h, if self.frozen { 1 } else { 0 });
        let mut i = 0usize;
        while i < SAMPLES {
            if let Some(s) = self.slots.get(i) {
                h = mix(h, s.code as u64);
                h = mix(h, s.pages as u64);
                h = mix(h, s.zpages as u64);
                h = mix(h, s.full as u64);
                h = mix(h, if s.seen { 1 } else { 0 });
                h = mix(h, s.delta);
                h = mix(h, s.sum);
                h = mix(h, s.spec.p_vaddr);
                h = mix(h, s.spec.p_offset);
                h = mix(h, s.spec.p_filesz);
                h = mix(h, s.spec.p_memsz);
                h = mix(h, s.spec.p_align);
            }
            i += 1;
        }
        h
    }
}

fn mix(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建基准账本：登记十二个样本并逐一观测。
pub fn build_ledger() -> Result<BssLedger, u32> {
    let mut l = BssLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let c = spec_of(i);
        let (want, _, _, _, _, _) = expected_of(i);
        l.register_ruled(&c, want)?;
        i += 1;
    }
    let mut j = 0usize;
    while j < SAMPLES {
        l.observe(j)?;
        j += 1;
    }
    Ok(l)
}

/// 四路独立注入：越序、重复、越界、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut ok = [false; 4];
    let c0 = spec_of(0);

    let mut a = BssLedger::new();
    match a.observe(0) {
        Err(code) if code == BZ_ENOBS => ok[0] = a.rejected() == 1 && a.len() == 0,
        _ => {}
    }

    let mut b = BssLedger::new();
    let _ = b.register(&c0);
    match b.register(&c0) {
        Err(code) if code == BZ_EDUP => ok[1] = b.rejected() == 1 && b.len() == 1,
        _ => {}
    }

    let mut d = BssLedger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let (want, _, _, _, _, _) = expected_of(i);
        let _ = d.register_ruled(&spec_of(i), want);
        i += 1;
    }
    let extra = BssCase::make(0x50_0000, 0, 0x1000, 0x2000, PAGE);
    match d.register(&extra) {
        Err(code) if code == BZ_EID => ok[2] = d.rejected() == 1 && d.len() == SAMPLES,
        _ => {}
    }

    if let Ok(mut l) = build_ledger() {
        if l.freeze().is_ok() {
            match l.register(&c0) {
                Err(code) if code == BZ_EFROZEN => ok[3] = l.is_frozen() && l.rejected() == 1,
                _ => {}
            }
        }
    }
    ok
}

/// 十三项自检。
pub fn run_unx11253_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11253");

    let fresh = BssLedger::new();
    s.add(
        "B1-建场静默",
        fresh.len() == 0
            && fresh.is_empty()
            && fresh.rejected() == 0
            && fresh.observed() == 0
            && !fresh.is_frozen(),
        "新账本零格零拒绝",
    );

    let b2 = match extend(&spec_of(0)) {
        Ok(e) => {
            e.delta == 0
                && e.pages == 2
                && e.zpages == 0
                && !e.split
                && audit(&spec_of(0)) == Ok((0, 0, 0))
        }
        Err(_) => false,
    };
    s.add("B2-无bss样本零增量", b2, "filesz==memsz时没有零字节");

    let b3 = match extend(&spec_of(1)) {
        Ok(e) => {
            e.delta == 1
                && e.pages == 2
                && e.split
                && e.lo & (PAGE - 1) == 0xfff
                && page_parts(&spec_of(1), 1) == Some((0xfff, 1))
                && scan_zero(&spec_of(1)) == Ok((1, 1))
        }
        Err(_) => false,
    };
    s.add("B3-页内拆分一字节", b3, "文件尾端落在页内时同页余量归零");

    let b4 = match extend(&spec_of(2)) {
        Ok(e) => {
            e.delta == 0x1000
                && e.pages == 3
                && e.zpages == 1
                && !e.split
                && audit(&spec_of(2)) == Ok((0x1000, 0, 1))
                && page_parts(&spec_of(2), 2) == Some((0, 0x1000))
        }
        Err(_) => false,
    };
    s.add("B4-整页零页", b4, "文件结束后整页由匿名零填满");

    let b5 = match extend(&spec_of(3)) {
        Ok(e) => {
            e.delta == 0x1000
                && e.pages == 4
                && e.zpages == 1
                && e.split
                && read_byte(&spec_of(3), 0x200f) == Ok(MARKER)
                && read_byte(&spec_of(3), 0x2010) == Ok(0)
        }
        Err(_) => false,
    };
    s.add("B5-跨文件尾读数", b5, "文件末字节后一位立即读零");

    s.add(
        "B6-错位拒绝",
        extend(&spec_of(4)) == Err(BZ_EMISALIGN),
        "文件偏移与虚拟地址页内不一致",
    );
    s.add(
        "B7-文件超出拒绝",
        extend(&spec_of(5)) == Err(BZ_EOVFL),
        "filesz大于memsz",
    );
    s.add(
        "B8-空段拒绝",
        extend(&spec_of(6)) == Err(BZ_EEMPTY),
        "memsz为零",
    );
    s.add(
        "B9-地址上溢拒绝",
        extend(&spec_of(7)) == Err(BZ_ERANGE),
        "vaddr+memsz越过64位",
    );
    s.add(
        "B10-对齐非法拒绝",
        extend(&spec_of(8)) == Err(BZ_EALIGN),
        "p_align不是2的幂",
    );

    let built = build_ledger();
    let (b11, b12) = match &built {
        Ok(l) => {
            let mut scan_ok = true;
            let mut k = 0usize;
            while k < SAMPLES {
                let c = spec_of(k);
                if let Ok(e) = extend(&c) {
                    match scan_zero(&c) {
                        Ok((a, b)) => {
                            if a != b || a as u64 != e.delta {
                                scan_ok = false;
                            }
                        }
                        Err(_) => scan_ok = false,
                    }
                }
                k += 1;
            }
            let r = l.len() == SAMPLES
                && l.accepted() == SAMPLES as u32
                && l.rejected() == 0
                && l.errors() == 5
                && l.total_delta() == 0x7801
                && l.total_zero() == l.total_delta()
                && l.total_pages() == 19
                && l.total_zpages() == 8
                && l.split_count() == 3
                && l.audit_ok()
                && scan_ok;
            let mut m = match build_ledger() {
                Ok(v) => v,
                Err(_) => BssLedger::new(),
            };
            let f = match m.freeze() {
                Ok(_) => m.is_frozen() && m.freeze() == Err(BZ_EFROZEN) && m.rejected() == 1,
                Err(_) => false,
            };
            (r, f)
        }
        Err(_) => (false, false),
    };
    s.add("B11-账本守恒与审计", b11, "零和等于差额 0x7801，尾部读数100%为零");
    s.add("B12-冻结后二次拒绝", b12, "冻结成功且再次冻结返回错误");

    s.add(
        "B13-四路注入",
        inject_four() == [true, true, true, true],
        "越序 重复 越界 冻结四条拒绝路径均触发",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_bss_has_zero_delta() {
        let c = spec_of(0);
        let e = match extend(&c) {
            Ok(v) => v,
            Err(_) => Extent::none(),
        };
        assert_eq!(e.delta, 0);
        assert_eq!(e.pages, 2);
        assert!(!e.split);
        assert_eq!(audit(&c), Ok((0, 0, 0)));
        assert_eq!(page_parts(&c, 0), Some((0x1000, 0)));
    }

    #[test]
    fn partial_tail_and_full_zero_page() {
        let a = spec_of(1);
        assert_eq!(page_parts(&a, 1), Some((0xfff, 1)));
        let b = spec_of(2);
        assert_eq!(page_parts(&b, 2), Some((0, 0x1000)));
        assert_eq!(audit(&b), Ok((0x1000, 0, 1)));
    }

    #[test]
    fn tail_reads_are_zero() {
        let c = spec_of(9);
        let e = match extend(&c) {
            Ok(v) => v,
            Err(_) => Extent::none(),
        };
        assert_eq!(e.delta, 0x3000);
        assert_eq!(e.pages, 4);
        assert_eq!(e.zpages, 3);
        assert_eq!(read_byte(&c, 0x1000), Ok(0));
        assert_eq!(read_byte(&c, 0x3fff), Ok(0));
        assert_eq!(read_byte(&c, 0x4000), Err(BZ_EOOB));
        assert_eq!(scan_zero(&c), Ok((0x3000, 0x3000)));
    }

    #[test]
    fn refusal_codes_are_exact() {
        assert_eq!(extend(&spec_of(4)), Err(BZ_EMISALIGN));
        assert_eq!(extend(&spec_of(5)), Err(BZ_EOVFL));
        assert_eq!(extend(&spec_of(6)), Err(BZ_EEMPTY));
        assert_eq!(extend(&spec_of(7)), Err(BZ_ERANGE));
        assert_eq!(extend(&spec_of(8)), Err(BZ_EALIGN));
        assert!(align_ok(PAGE));
        assert!(align_ok(0));
        assert!(!align_ok(0x800));
        assert!(!align_ok(0x1800));
    }

    #[test]
    fn ledger_conserves_zero_bytes() {
        let mut l = match build_ledger() {
            Ok(v) => v,
            Err(_) => BssLedger::new(),
        };
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.observed(), SAMPLES as u32);
        assert_eq!(l.matched(), SAMPLES as u32);
        assert_eq!(l.errors(), 5);
        assert_eq!(l.total_delta(), 0x7801);
        assert_eq!(l.total_zero(), 0x7801);
        assert_eq!(l.total_pages(), 19);
        assert_eq!(l.total_zpages(), 8);
        assert_eq!(l.split_count(), 3);
        assert!(l.audit_ok());
        let h = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_eq!(l.replay(), Ok(h));
        assert_eq!(l.freeze(), Err(BZ_EFROZEN));
    }

    #[test]
    fn injections_and_duplicates() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = BssLedger::new();
        let c = spec_of(10);
        assert!(l.register(&c).is_ok());
        assert_eq!(l.register(&c), Err(BZ_EDUP));
        assert_eq!(l.len(), 1);
        assert_eq!(l.rejected(), 1);
        assert_eq!(l.observe(1), Err(BZ_ENOBS));
    }
}
