//! UNX-F11418 三工具判据入册登记（语料清单挂接）
//!
//! 判据不是孤立条目：每一条都要挂到语料清单里的同号条目上，并且工具、摘要
//! 必须与语料条目逐字一致。入册账因此分两层：
//!
//! 1. **语料层**（`Corpus`）：编号唯一、摘要非零、工具合法；语料是判据的
//!    唯一真值来源，先有语料才能谈入册。
//! 2. **判据层**（`Registry`）：编号不得重复、档位只在 0..=2、语料必须存在、
//!    工具的归属必须一致、摘要必须逐位相等；表满时拒绝新条目。
//!
//! 收册（`finish`）还要求三件工具**至少各有一条**判据：只登记 cat 的判据
//! 不能宣称「三工具已入册」，这是覆盖闸门而不是计数闸门。
//!
//! 闸门次序（`register`）：工具合法 → 编号范围 → 编号重复 → 档位 → 语料
//! 存在 → 工具同源 → 摘要相等 → 表满。
//!
//! 账本契约：十四格，八成功六失败；成功格记录语料/判据计数与收册条数，
//! 失败格覆盖重复/档位/摘要/工具/语料/编号六类，表满与覆盖在判据与注入里
//! 单独验证。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf020;

/// 未知工具号。
pub const CR_ETOOL: u32 = SEG_BASE + 0x01;
/// 编号为零或越界。
pub const CR_EID: u32 = SEG_BASE + 0x02;
/// 判据编号重复。
pub const CR_EDUP: u32 = SEG_BASE + 0x03;
/// 档位越界。
pub const CR_ETIER: u32 = SEG_BASE + 0x04;
/// 摘要与语料不符或为零。
pub const CR_EDIGEST: u32 = SEG_BASE + 0x05;
/// 语料条目缺失或不同源。
pub const CR_ECORPUS: u32 = SEG_BASE + 0x06;
/// 判据表满。
pub const CR_EFULL: u32 = SEG_BASE + 0x07;
/// 三工具未全覆盖，不能收册。
pub const CR_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 语料表容量。
pub const CAP: usize = 8;
/// 判据表容量。
pub const REG_CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 编号上界。
pub const ID_MAX: u16 = 99;
/// 档位上界。
pub const TIER_MAX: u8 = 2;

/// 工具：cat。
pub const TOOL_CAT: u8 = 1;
/// 工具：mv。
pub const TOOL_MV: u8 = 2;
/// 工具：chmod。
pub const TOOL_CHMOD: u8 = 3;

/// 操作码：语料入册。
pub const OP_CORPUS: u8 = 1;
/// 操作码：判据登记。
pub const OP_REG: u8 = 2;
/// 操作码：收册。
pub const OP_FIN: u8 = 3;
/// 操作码：覆盖计数。
pub const OP_COV: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 工具号是否合法。
fn tool_ok(tool: u8) -> bool {
    tool == TOOL_CAT || tool == TOOL_MV || tool == TOOL_CHMOD
}

/// 一条语料条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 条目编号。
    pub id: u16,
    /// 内容摘要。
    pub digest: u32,
    /// 所属工具。
    pub tool: u8,
}

/// 语料清单。
#[derive(Clone, Copy, Debug)]
pub struct Corpus {
    rows: [Entry; CAP],
    n: usize,
}

impl Corpus {
    /// 空清单。
    pub const fn new() -> Corpus {
        Corpus { rows: [Entry { id: 0, digest: 0, tool: 0 }; CAP], n: 0 }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空清单。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 条。
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按编号取条目。
    pub fn entry(&self, id: u16) -> Option<Entry> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.rows.get(i) {
                if e.id == id {
                    return Some(*e);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 语料入册。
    ///
    /// 闸门次序：编号 → 工具 → 摘要 → 重复 → 表满。
    pub fn add(&mut self, id: u16, digest: u32, tool: u8) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(CR_EID);
        }
        if !tool_ok(tool) {
            return Err(CR_ETOOL);
        }
        if digest == 0 {
            return Err(CR_EDIGEST);
        }
        if self.entry(id).is_some() {
            return Err(CR_EDUP);
        }
        if self.n >= CAP {
            return Err(CR_EFULL);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Entry { id, digest, tool };
        }
        self.n = self.n.saturating_add(1);
        Ok(self.n)
    }
}

/// 一条判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crit {
    /// 判据编号（与语料条目同号）。
    pub id: u16,
    /// 工具。
    pub tool: u8,
    /// 档位 0..=2。
    pub tier: u8,
    /// 摘要。
    pub digest: u32,
}

/// 判据登记册。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    rows: [Crit; REG_CAP],
    n: usize,
}

impl Registry {
    /// 空册。
    pub const fn new() -> Registry {
        Registry { rows: [Crit { id: 0, tool: 0, tier: 0, digest: 0 }; REG_CAP], n: 0 }
    }

    /// 判据数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 条。
    pub fn crit_at(&self, i: usize) -> Option<Crit> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 找编号。
    fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.id == id {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 某工具的判据条数。
    pub fn coverage(&self, tool: u8) -> Result<usize, u32> {
        if !tool_ok(tool) {
            return Err(CR_ETOOL);
        }
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.tool == tool {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        Ok(k)
    }

    /// 登记一条判据。
    ///
    /// 闸门次序：工具 → 编号 → 重复 → 档位 → 语料存在 → 工具同源 → 摘要 →
    /// 表满。
    pub fn register(
        &mut self,
        corpus: &Corpus,
        id: u16,
        tool: u8,
        tier: u8,
        digest: u32,
    ) -> Result<usize, u32> {
        if !tool_ok(tool) {
            return Err(CR_ETOOL);
        }
        if id == 0 || id > ID_MAX {
            return Err(CR_EID);
        }
        if self.find(id).is_some() {
            return Err(CR_EDUP);
        }
        if tier > TIER_MAX {
            return Err(CR_ETIER);
        }
        let e = match corpus.entry(id) {
            Some(v) => v,
            None => return Err(CR_ECORPUS),
        };
        if e.tool != tool {
            return Err(CR_ECORPUS);
        }
        if e.digest != digest {
            return Err(CR_EDIGEST);
        }
        if self.n >= REG_CAP {
            return Err(CR_EFULL);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Crit { id, tool, tier, digest };
        }
        self.n = self.n.saturating_add(1);
        Ok(self.n)
    }

    /// 收册：判据数不少于 3，且三工具各至少一条。
    pub fn finish(&self) -> Result<u32, u32> {
        if self.n < 3 {
            return Err(CR_EGAP);
        }
        let mut tool = 1u8;
        while tool <= 3 {
            match self.coverage(tool) {
                Ok(0) => return Err(CR_EGAP),
                Ok(_) => {}
                Err(c) => return Err(c),
            }
            tool = tool.saturating_add(1);
        }
        Ok(self.n as u32)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > REG_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let c = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if c.id == 0 || c.id > ID_MAX || !tool_ok(c.tool) || c.tier > TIER_MAX || c.digest == 0 {
                return false;
            }
            if self.find(c.id) != Some(i) {
                return false;
            }
            let mut j = i.saturating_add(1);
            while j < self.n {
                if let Some(w) = self.rows.get(j) {
                    if w.id == c.id {
                        return false;
                    }
                }
                j = j.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        true
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CR_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 1, 2, 3, 3];
        const CODES: [u32; 6] =
            [CR_EDUP, CR_ETIER, CR_EDIGEST, CR_ETOOL, CR_ECORPUS, CR_EID];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(CR_EBAL),
            };
            if v.slot as usize != i {
                return Err(CR_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(CR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(CR_EVALUE);
                        }
                    }
                    None => return Err(CR_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(CR_EVALUE);
                        }
                    }
                    None => return Err(CR_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(CR_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CR_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(CR_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：四条语料 → 三条判据 → 收册 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Corpus::new();
    let mut r = Registry::new();

    let _ = l.register(cell(0, OP_CORPUS, c.add(1, 0xa1, TOOL_CAT)));
    let _ = l.register(cell(1, OP_CORPUS, c.add(2, 0xb2, TOOL_MV)));
    let _ = l.register(cell(2, OP_CORPUS, c.add(3, 0xc3, TOOL_CHMOD)));
    let _ = l.register(cell(3, OP_CORPUS, c.add(4, 0xa4, TOOL_CAT)));
    let _ = l.register(cell(4, OP_REG, r.register(&c, 1, TOOL_CAT, 0, 0xa1)));
    let _ = l.register(cell(5, OP_REG, r.register(&c, 2, TOOL_MV, 1, 0xb2)));
    let _ = l.register(cell(6, OP_REG, r.register(&c, 3, TOOL_CHMOD, 2, 0xc3)));
    let _ = l.register(word_cell(7, OP_FIN, r.finish()));

    let _ = l.register(cell(8, OP_REG, r.register(&c, 1, TOOL_CAT, 0, 0xa1)));
    let _ = l.register(cell(9, OP_REG, r.register(&c, 4, TOOL_CAT, 3, 0xa4)));
    let _ = l.register(cell(10, OP_REG, r.register(&c, 4, TOOL_CAT, 0, 0xdead)));
    let _ = l.register(cell(11, OP_REG, r.register(&c, 1, 9, 0, 0xa1)));
    let _ = l.register(cell(12, OP_REG, r.register(&c, 9, TOOL_CAT, 0, 0x99)));
    let _ = l.register(cell(13, OP_REG, r.register(&c, 0, TOOL_CAT, 0, 0xa1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：四语料挂三判据，齐套收册。
    let mut c = Corpus::new();
    let _ = c.add(1, 0xa1, TOOL_CAT);
    let _ = c.add(2, 0xb2, TOOL_MV);
    let _ = c.add(3, 0xc3, TOOL_CHMOD);
    let _ = c.add(4, 0xa4, TOOL_CAT);
    let mut r = Registry::new();
    let _ = r.register(&c, 1, TOOL_CAT, 0, 0xa1);
    let _ = r.register(&c, 2, TOOL_MV, 1, 0xb2);
    let _ = r.register(&c, 3, TOOL_CHMOD, 2, 0xc3);
    let _ = r.register(&c, 4, TOOL_CAT, 0, 0xa4);
    out[1] = c.len() == 4
        && r.len() == 4
        && r.finish() == Ok(4)
        && r.coverage(TOOL_CAT) == Ok(2)
        && r.coverage(TOOL_MV) == Ok(1)
        && r.coverage(TOOL_CHMOD) == Ok(1)
        && r.crit_at(3) == Some(Crit { id: 4, tool: TOOL_CAT, tier: 0, digest: 0xa4 })
        && c.entry(2) == Some(Entry { id: 2, digest: 0xb2, tool: TOOL_MV })
        && r.consistent();

    // 二路：八类失败各自可辨。
    let mut d = Corpus::new();
    let _ = d.add(1, 0x11, TOOL_CAT);
    let _ = d.add(2, 0x22, TOOL_CAT);
    let mut g = Registry::new();
    let tool = g.register(&d, 1, 9, 0, 0x11) == Err(CR_ETOOL);
    let id_zero = g.register(&d, 0, TOOL_CAT, 0, 0x11) == Err(CR_EID);
    let id_high = g.register(&d, ID_MAX + 1, TOOL_CAT, 0, 0x11) == Err(CR_EID);
    let ok1 = g.register(&d, 1, TOOL_CAT, 0, 0x11) == Ok(1);
    let dup = g.register(&d, 1, TOOL_CAT, 0, 0x11) == Err(CR_EDUP);
    let tier = g.register(&d, 2, TOOL_CAT, 3, 0x22) == Err(CR_ETIER);
    let digest = g.register(&d, 2, TOOL_CAT, 0, 0x12) == Err(CR_EDIGEST);
    let missing = g.register(&d, 5, TOOL_CAT, 0, 0x11) == Err(CR_ECORPUS);
    let wrong_tool = g.register(&d, 2, TOOL_MV, 0, 0x22) == Err(CR_ECORPUS);
    let gap = g.finish() == Err(CR_EGAP);

    let mut full_c = Corpus::new();
    let mut k = 0u16;
    while k < CAP as u16 {
        let _ = full_c.add(k + 1, 0x100 + k as u32, TOOL_CAT);
        k = k.saturating_add(1);
    }
    let mut full_r = Registry::new();
    let mut j = 0u16;
    while j < REG_CAP as u16 {
        let _ = full_r.register(&full_c, j + 1, TOOL_CAT, 0, 0x100 + j as u32);
        j = j.saturating_add(1);
    }
    let full = full_r.register(&full_c, (REG_CAP + 1) as u16, TOOL_CAT, 0, 0x100 + REG_CAP as u32)
        == Err(CR_EFULL);
    out[2] = tool
        && id_zero
        && id_high
        && ok1
        && dup
        && tier
        && digest
        && missing
        && wrong_tool
        && gap
        && full
        && full_r.len() == REG_CAP;

    // 三路：摘要与工具的双重绑定、覆盖计数。
    let mut q = Corpus::new();
    let _ = q.add(1, 0xa1, TOOL_CAT);
    let _ = q.add(2, 0xb2, TOOL_MV);
    let _ = q.add(3, 0xc3, TOOL_CHMOD);
    let mut w = Registry::new();
    let _ = w.register(&q, 1, TOOL_CAT, 0, 0xa1);
    let mismatch_digest = w.register(&q, 2, TOOL_MV, 0, 0xa1) == Err(CR_EDIGEST);
    let mismatch_tool = w.register(&q, 2, TOOL_CHMOD, 0, 0xb2) == Err(CR_ECORPUS);
    let _ = w.register(&q, 2, TOOL_MV, 0, 0xb2);
    let partial = w.finish() == Err(CR_EGAP);
    let _ = w.register(&q, 3, TOOL_CHMOD, 0, 0xc3);
    out[3] = mismatch_digest
        && mismatch_tool
        && partial
        && w.finish() == Ok(3)
        && w.coverage(TOOL_CAT) == Ok(1)
        && w.coverage(TOOL_MV) == Ok(1)
        && w.coverage(TOOL_CHMOD) == Ok(1)
        && w.coverage(9) == Err(CR_ETOOL)
        && w.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11418_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11418.three_tool_criteria_register");
    let l = build_ledger();

    s.add_dyn(
        "CR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CR2-语料入册计数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "CR3-判据挂接语料",
        l.sample(4).map(|x| x.value) == Some(1)
            && l.sample(5).map(|x| x.value) == Some(2)
            && l.sample(6).map(|x| x.value) == Some(3)
            && l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "CR4-三工具齐套才收",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0x11, TOOL_CAT);
            let _ = c.add(2, 0x22, TOOL_MV);
            let _ = c.add(3, 0x33, TOOL_CHMOD);
            let mut r = Registry::new();
            let empty = r.finish() == Err(CR_EGAP);
            let _ = r.register(&c, 1, TOOL_CAT, 0, 0x11);
            let _ = r.register(&c, 2, TOOL_MV, 0, 0x22);
            let partial = r.finish() == Err(CR_EGAP);
            let _ = r.register(&c, 3, TOOL_CHMOD, 0, 0x33);
            empty && partial && r.finish() == Ok(3)
        },
        "static",
    );

    s.add(
        "CR5-编号范围闸门",
        {
            let mut c = Corpus::new();
            let mut r = Registry::new();
            let zero = r.register(&c, 0, TOOL_CAT, 0, 1) == Err(CR_EID);
            let high = r.register(&c, ID_MAX + 1, TOOL_CAT, 0, 1) == Err(CR_EID);
            let _ = c.add(1, 1, TOOL_CAT);
            zero && high
                && r.register(&c, 1, TOOL_CAT, 0, 1) == Ok(1)
                && l.sample(13).map(|x| x.code) == Some(CR_EID)
        },
        "static",
    );

    s.add(
        "CR6-工具闸门",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 1, TOOL_CAT);
            let mut r = Registry::new();
            r.register(&c, 1, 0, 0, 1) == Err(CR_ETOOL)
                && r.register(&c, 1, 4, 0, 1) == Err(CR_ETOOL)
                && c.add(2, 2, 0) == Err(CR_ETOOL)
                && l.sample(11).map(|x| x.code) == Some(CR_ETOOL)
        },
        "static",
    );

    s.add(
        "CR7-档位闸门",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0x11, TOOL_CAT);
            let mut r = Registry::new();
            r.register(&c, 1, TOOL_CAT, 3, 0x11) == Err(CR_ETIER)
                && r.register(&c, 1, TOOL_CAT, TIER_MAX + 1, 0x11) == Err(CR_ETIER)
                && r.register(&c, 1, TOOL_CAT, TIER_MAX, 0x11) == Ok(1)
                && l.sample(9).map(|x| x.code) == Some(CR_ETIER)
        },
        "static"
    );

    s.add(
        "CR8-重复闸门",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0x11, TOOL_CAT);
            let mut r = Registry::new();
            let _ = r.register(&c, 1, TOOL_CAT, 0, 0x11);
            let dup = r.register(&c, 1, TOOL_CAT, 3, 0x99) == Err(CR_EDUP);
            dup && r.len() == 1 && l.sample(8).map(|x| x.code) == Some(CR_EDUP)
        },
        "static",
    );

    s.add(
        "CR9-摘要逐位相等",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0xa1, TOOL_CAT);
            let mut r = Registry::new();
            r.register(&c, 1, TOOL_CAT, 0, 0xa2) == Err(CR_EDIGEST)
                && r.register(&c, 1, TOOL_CAT, 0, 0) == Err(CR_EDIGEST)
                && r.register(&c, 1, TOOL_CAT, 0, 0xa1) == Ok(1)
                && l.sample(10).map(|x| x.code) == Some(CR_EDIGEST)
        },
        "static",
    );

    s.add(
        "CR10-语料缺失与不同源",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0x11, TOOL_CAT);
            let _ = c.add(2, 0x22, TOOL_MV);
            let mut r = Registry::new();
            let miss = r.register(&c, 9, TOOL_CAT, 0, 0x11) == Err(CR_ECORPUS);
            let wrong = r.register(&c, 2, TOOL_CAT, 0, 0x22) == Err(CR_ECORPUS);
            let ok = r.register(&c, 2, TOOL_MV, 0, 0x22) == Ok(1);
            miss && wrong && ok && l.sample(12).map(|x| x.code) == Some(CR_ECORPUS)
        },
        "static",
    );

    s.add(
        "CR11-判据表满",
        {
            let mut c = Corpus::new();
            let mut k = 0u16;
            while k < CAP as u16 {
                let _ = c.add(k + 1, 0x100 + k as u32, TOOL_CAT);
                k = k.saturating_add(1);
            }
            let mut r = Registry::new();
            let mut j = 0u16;
            while j < REG_CAP as u16 {
                let _ = r.register(&c, j + 1, TOOL_CAT, 0, 0x100 + j as u32);
                j = j.saturating_add(1);
            }
            r.len() == REG_CAP
                && r.register(&c, (REG_CAP + 1) as u16, TOOL_CAT, 0, 0x100 + REG_CAP as u32)
                    == Err(CR_EFULL)
                && r.register(&c, (REG_CAP + 2) as u16, TOOL_CAT, 9, 0x100 + (REG_CAP + 1) as u32)
                    == Err(CR_ETIER)
        },
        "static",
    );

    s.add(
        "CR12-覆盖计数与不变式",
        {
            let mut c = Corpus::new();
            let _ = c.add(1, 0x11, TOOL_CAT);
            let _ = c.add(2, 0x12, TOOL_CAT);
            let _ = c.add(3, 0x33, TOOL_CHMOD);
            let mut r = Registry::new();
            let _ = r.register(&c, 1, TOOL_CAT, 0, 0x11);
            let _ = r.register(&c, 2, TOOL_CAT, 1, 0x12);
            let _ = r.register(&c, 3, TOOL_CHMOD, 2, 0x33);
            r.coverage(TOOL_CAT) == Ok(2)
                && r.coverage(TOOL_MV) == Ok(0)
                && r.coverage(TOOL_CHMOD) == Ok(1)
                && r.coverage(0) == Err(CR_ETOOL)
                && r.finish() == Err(CR_EGAP)
                && r.consistent()
                && r.crit_at(1) == Some(Crit { id: 2, tool: TOOL_CAT, tier: 1, digest: 0x12 })
        },
        "static",
    );

    s.add(
        "CR13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Corpus {
        let mut c = Corpus::new();
        assert_eq!(c.add(1, 0x11, TOOL_CAT), Ok(1));
        assert_eq!(c.add(2, 0x22, TOOL_MV), Ok(2));
        assert_eq!(c.add(3, 0x33, TOOL_CHMOD), Ok(3));
        c
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11418_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn corpus_gates() {
        let mut c = Corpus::new();
        assert_eq!(c.add(0, 1, TOOL_CAT), Err(CR_EID));
        assert_eq!(c.add(1, 0, TOOL_CAT), Err(CR_EDIGEST));
        assert_eq!(c.add(1, 1, 9), Err(CR_ETOOL));
        assert_eq!(c.add(1, 1, TOOL_CAT), Ok(1));
        assert_eq!(c.add(1, 2, TOOL_CAT), Err(CR_EDUP));
        assert_eq!(c.entry(1), Some(Entry { id: 1, digest: 1, tool: TOOL_CAT }));
    }

    #[test]
    fn register_gates() {
        let c = corpus();
        let mut r = Registry::new();
        assert_eq!(r.register(&c, 9, TOOL_CAT, 0, 0x11), Err(CR_ECORPUS));
        assert_eq!(r.register(&c, 1, TOOL_MV, 0, 0x11), Err(CR_ECORPUS));
        assert_eq!(r.register(&c, 1, TOOL_CAT, 0, 0x12), Err(CR_EDIGEST));
        assert_eq!(r.register(&c, 1, TOOL_CAT, 3, 0x11), Err(CR_ETIER));
        assert_eq!(r.register(&c, 1, TOOL_CAT, 0, 0x11), Ok(1));
        assert_eq!(r.register(&c, 1, TOOL_CAT, 0, 0x11), Err(CR_EDUP));
    }

    #[test]
    fn coverage_and_finish() {
        let c = corpus();
        let mut r = Registry::new();
        assert_eq!(r.finish(), Err(CR_EGAP));
        assert_eq!(r.register(&c, 1, TOOL_CAT, 0, 0x11), Ok(1));
        assert_eq!(r.register(&c, 2, TOOL_MV, 0, 0x22), Ok(2));
        assert_eq!(r.finish(), Err(CR_EGAP));
        assert_eq!(r.coverage(TOOL_CHMOD), Ok(0));
        assert_eq!(r.register(&c, 3, TOOL_CHMOD, 0, 0x33), Ok(3));
        assert_eq!(r.finish(), Ok(3));
        assert_eq!(r.coverage(TOOL_CHMOD), Ok(1));
    }

    #[test]
    fn capacity_and_digest_binding() {
        let mut c = Corpus::new();
        let mut k = 0u16;
        while k < CAP as u16 {
            assert_eq!(c.add(k + 1, 0x100 + k as u32, TOOL_CAT), Ok(k as usize + 1));
            k += 1;
        }
        let mut r = Registry::new();
        let mut j = 0u16;
        while j < REG_CAP as u16 {
            assert_eq!(r.register(&c, j + 1, TOOL_CAT, 0, 0x100 + j as u32), Ok(j as usize + 1));
            j += 1;
        }
        assert_eq!(
            r.register(&c, (REG_CAP + 1) as u16, TOOL_CAT, 0, 0x100 + REG_CAP as u32),
            Err(CR_EFULL)
        );
        assert!(r.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
