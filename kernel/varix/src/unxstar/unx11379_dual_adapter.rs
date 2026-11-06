//! UNX-F11379 快照库双机同码适配器（S1/M7）
//!
//! 同一批判据在 S1 与 M7 两台机器上各跑一遍，快照要分档比对：**逐字节**档
//! 一位都不许差，**关键列**档允许已声明的动态位差异。适配器先把每台机器的
//! 指纹收进来，再按档裁定：
//!
//! 1. **档位闸门**：只有逐字节（0）与关键列（1）两档；档位越档拒收。
//! 2. **掩码闸门**：逐字节档不许带动态掩码；带了就是自相矛盾的判据。
//! 3. **裁定闸门**：未观测不许裁定；掩码之外的位有差异即 `EMISS`，掩码
//!    之内的差异按豁免位计入并放行。
//!
//! 闸门次序：
//! - `pair`：判据号越界 → 档位越档 → 掩码与档位矛盾 → 重复 → 表满；
//! - `observe`：未知判据 → 重复观测 → 空指纹；
//! - `judge`：未知判据 → 尚未观测 → 重复裁定 → 掩码外差异。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xedb0;

/// 判据号越界。
pub const DA_ERANGE: u32 = SEG_BASE + 0x01;
/// 档位越档。
pub const DA_EMODE: u32 = SEG_BASE + 0x02;
/// 掩码与档位矛盾。
pub const DA_EMASK: u32 = SEG_BASE + 0x03;
/// 快照指纹为空。
pub const DA_EGAP: u32 = SEG_BASE + 0x04;
/// 重复配对或重复观测。
pub const DA_EDUP: u32 = SEG_BASE + 0x05;
/// 配对表已满。
pub const DA_EFULL: u32 = SEG_BASE + 0x06;
/// 判据未配对。
pub const DA_EUNK: u32 = SEG_BASE + 0x07;
/// 掩码外仍有差异。
pub const DA_EMISS: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 配对表容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 判据号上界。
pub const ID_MAX: u32 = 64;

/// 档位：逐字节。
pub const TIER_BYTE: u32 = 0;
/// 档位：关键列。
pub const TIER_KEY: u32 = 1;

/// 操作码：配对。
pub const OP_PAIR: u8 = 1;
/// 操作码：观测双机指纹。
pub const OP_OBSERVE: u8 = 2;
/// 操作码：裁定。
pub const OP_JUDGE: u8 = 3;
/// 操作码：收表。
pub const OP_FINISH: u8 = 4;

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

/// 一条双机配对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 判据号。
    pub id: u32,
    /// 档位。
    pub tier: u32,
    /// 动态位掩码。
    pub mask: u32,
    /// S1 指纹。
    pub s1: u32,
    /// M7 指纹。
    pub m7: u32,
    /// 是否已观测。
    pub seen: bool,
    /// 是否已裁定。
    pub judged: bool,
}

/// 双机同码适配器。
#[derive(Clone, Copy, Debug)]
pub struct Ad {
    r: [Row; CAP],
    n: usize,
    same: u32,
    diff: u32,
}

impl Ad {
    /// 空表。
    pub const fn new() -> Ad {
        Ad {
            r: [Row { id: 0, tier: 0, mask: 0, s1: 0, m7: 0, seen: false, judged: false }; CAP],
            n: 0,
            same: 0,
            diff: 0,
        }
    }

    /// 配对数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 同码条数。
    pub fn same(&self) -> u32 {
        self.same
    }

    /// 异码条数。
    pub fn diff(&self) -> u32 {
        self.diff
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Row> {
        if i >= self.n {
            return None;
        }
        self.r.get(i).copied()
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.r.get(i) {
                Some(e) => {
                    if e.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 配一条判据的档位与动态掩码。
    pub fn pair(&mut self, id: u32, tier: u32, mask: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(DA_ERANGE);
        }
        if tier > TIER_KEY {
            return Err(DA_EMODE);
        }
        if tier == TIER_BYTE && mask != 0 {
            return Err(DA_EMASK);
        }
        if self.find(id).is_some() {
            return Err(DA_EDUP);
        }
        if self.n >= CAP {
            return Err(DA_EFULL);
        }
        if let Some(slot) = self.r.get_mut(self.n) {
            *slot = Row { id, tier, mask, s1: 0, m7: 0, seen: false, judged: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 观测两台机器的指纹，返回已观测条数。
    pub fn observe(&mut self, id: u32, s1: u32, m7: u32) -> Result<u32, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(DA_EUNK),
        };
        match self.r.get(at) {
            Some(e) => {
                if e.seen {
                    return Err(DA_EDUP);
                }
            }
            None => return Err(DA_EUNK),
        }
        if s1 == 0 || m7 == 0 {
            return Err(DA_EGAP);
        }
        if let Some(slot) = self.r.get_mut(at) {
            slot.s1 = s1;
            slot.m7 = m7;
            slot.seen = true;
        }
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.r.get(i) {
                if e.seen {
                    c = c.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(c)
    }

    /// 裁定一条：掩码外零差异才算同码。
    pub fn judge(&mut self, id: u32) -> Result<u32, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(DA_EUNK),
        };
        let row = match self.r.get(at) {
            Some(v) => *v,
            None => return Err(DA_EUNK),
        };
        if !row.seen {
            return Err(DA_EGAP);
        }
        if row.judged {
            return Err(DA_EDUP);
        }
        let xor = row.s1 ^ row.m7;
        let outside = xor & !row.mask;
        if let Some(slot) = self.r.get_mut(at) {
            slot.judged = true;
        }
        if outside != 0 {
            self.diff = self.diff.saturating_add(1);
            Err(DA_EMISS)
        } else {
            self.same = self.same.saturating_add(1);
            Ok(xor & row.mask)
        }
    }

    /// 收表：全部裁定后才许收。
    pub fn finish(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.n {
            match self.r.get(i) {
                Some(e) => {
                    if !e.judged {
                        return Err(DA_EGAP);
                    }
                }
                None => return Err(DA_EGAP),
            }
            i += 1;
        }
        Ok(self.same)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.r.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.tier as u64);
                h = fold(h, e.mask as u64);
                h = fold(h, e.s1 as u64);
                h = fold(h, e.m7 as u64);
                h = fold(h, e.judged as u64);
            }
            i += 1;
        }
        fold(h, self.same as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut judged = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let e = match self.r.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.tier > TIER_KEY {
                return false;
            }
            if e.tier == TIER_BYTE && e.mask != 0 {
                return false;
            }
            if e.judged {
                judged = judged.saturating_add(1);
                if !e.seen {
                    return false;
                }
            }
            let mut j = 0usize;
            while j < i {
                match self.r.get(j) {
                    Some(p) => {
                        if p.id == e.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        judged == self.same.saturating_add(self.diff)
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
            self.rejects += 1;
            return Err(DA_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
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
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DA_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 2, 3, 0, 15];
        const CODES: [u32; 6] = [DA_ERANGE, DA_EMODE, DA_EMASK, DA_EDUP, DA_EUNK, DA_EMISS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DA_EBAL),
            };
            if s.slot as usize != i {
                return Err(DA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DA_EVALUE);
                        }
                    }
                    None => return Err(DA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DA_EVALUE);
                        }
                    }
                    None => return Err(DA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DA_EBAL);
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
            Err(DA_ESEAL)
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

/// 十四格账本：三档配对 → 越界/越档/掩码矛盾 → 三台观测 → 重复/未知 →
/// 同码/掩码内差异/掩码外差异。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = Ad::new();

    let _ = l.register(cell(0, OP_PAIR, a.pair(1, TIER_BYTE, 0)));
    let _ = l.register(cell(1, OP_PAIR, a.pair(2, TIER_KEY, 0x0f)));
    let _ = l.register(cell(2, OP_PAIR, a.pair(3, TIER_KEY, 0x00)));
    let _ = l.register(cell(3, OP_PAIR, a.pair(0, TIER_BYTE, 0)));
    let _ = l.register(cell(4, OP_PAIR, a.pair(4, TIER_KEY + 1, 0)));
    let _ = l.register(cell(5, OP_PAIR, a.pair(5, TIER_BYTE, 0x0f)));
    let _ = l.register(word_cell(6, OP_OBSERVE, a.observe(1, 0xaa, 0xaa)));
    let _ = l.register(word_cell(7, OP_OBSERVE, a.observe(2, 0x10, 0x1f)));
    let _ = l.register(word_cell(8, OP_OBSERVE, a.observe(3, 0x20, 0x21)));
    let _ = l.register(word_cell(9, OP_OBSERVE, a.observe(1, 0, 0)));
    let _ = l.register(word_cell(10, OP_OBSERVE, a.observe(9, 0, 0)));
    let _ = l.register(word_cell(11, OP_JUDGE, a.judge(1)));
    let _ = l.register(word_cell(12, OP_JUDGE, a.judge(2)));
    let _ = l.register(word_cell(13, OP_JUDGE, a.judge(3)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：逐字节同码 + 关键列掩码内差异。
    let mut a = Ad::new();
    let p1 = a.pair(1, TIER_BYTE, 0);
    let p2 = a.pair(2, TIER_KEY, 0x0f);
    let o1 = a.observe(1, 0x1234, 0x1234);
    let o2 = a.observe(2, 0xaa, 0xab);
    let j1 = a.judge(1);
    let j2 = a.judge(2);
    out[1] = p1 == Ok(1)
        && p2 == Ok(2)
        && o1 == Ok(1)
        && o2 == Ok(2)
        && j1 == Ok(0)
        && j2 == Ok(0x01)
        && a.same() == 2
        && a.diff() == 0
        && a.finish() == Ok(2)
        && a.seal() != 0
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改配对表。
    let mut b = Ad::new();
    let range = b.pair(0, TIER_BYTE, 0) == Err(DA_ERANGE);
    let mode = b.pair(1, TIER_KEY + 1, 0) == Err(DA_EMODE);
    let mask = b.pair(1, TIER_BYTE, 1) == Err(DA_EMASK);
    let untouched = b.count() == 0;
    let p = b.pair(1, TIER_KEY, 0x0f) == Ok(1);
    let dup = b.pair(1, TIER_KEY, 0) == Err(DA_EDUP);
    let gap = b.observe(1, 0, 0) == Err(DA_EGAP);
    let obs = b.observe(1, 0x30, 0x40) == Ok(1);
    let redo = b.observe(1, 0x30, 0x40) == Err(DA_EDUP);
    let unknown = b.observe(9, 0x30, 0x31) == Err(DA_EUNK);
    let miss = b.judge(1) == Err(DA_EMISS) && b.diff() == 1;
    let h = b.seal();
    let again = b.judge(1) == Err(DA_EDUP) && b.seal() == h;
    let mut c = Ad::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = c.pair(i, TIER_BYTE, 0);
        i += 1;
    }
    let full = c.pair((CAP + 1) as u32, TIER_BYTE, 0) == Err(DA_EFULL);
    out[2] = range
        && mode
        && mask
        && untouched
        && p
        && dup
        && gap
        && obs
        && redo
        && unknown
        && miss
        && again
        && full
        && b.consistent()
        && c.consistent();

    // 三路：配对表满，未裁定不许收表。
    let mut q = Ad::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.pair(i, TIER_KEY, 0);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.pair((CAP + 1) as u32, TIER_KEY, 0) == Err(DA_EFULL);
    let q3 = q.finish() == Err(DA_EGAP);
    let mut k = 1u32;
    while (k as usize) <= CAP {
        let _ = q.observe(k, k, k);
        let _ = q.judge(k);
        k += 1;
    }
    let q4 = q.finish() == Ok(CAP as u32);
    out[3] = q1 && q2 && q3 && q4 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11379_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11379.dual_adapter");
    let l = build_ledger();

    s.add_dyn(
        "DA1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DA2-逐字节档零差异",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_BYTE, 0);
            let _ = a.observe(1, 0x55, 0x55);
            let j = a.judge(1);
            j == Ok(0)
                && a.same() == 1
                && a.diff() == 0
                && a.finish() == Ok(1)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DA3-关键列掩码内差异",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_KEY, 0x0f);
            let _ = a.observe(1, 0x10, 0x1f);
            a.judge(1) == Ok(0x0f)
                && a.same() == 1
                && a.diff() == 0
                && l.sample(12).map(|x| x.value) == Some(15)
        },
        "static",
    );

    s.add(
        "DA4-掩码外差异被拒",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_KEY, 0x0f);
            let _ = a.observe(1, 0x10, 0x20);
            a.judge(1) == Err(DA_EMISS)
                && a.diff() == 1
                && a.same() == 0
                && l.sample(13).map(|x| x.code) == Some(DA_EMISS)
        },
        "static",
    );

    s.add(
        "DA5-档位越档被拒",
        {
            let mut a = Ad::new();
            a.pair(1, TIER_KEY + 1, 0) == Err(DA_EMODE)
                && a.pair(1, TIER_KEY + 9, 0) == Err(DA_EMODE)
                && a.count() == 0
                && l.sample(4).map(|x| x.code) == Some(DA_EMODE)
        },
        "static",
    );

    s.add(
        "DA6-逐字节带掩码被拒",
        {
            let mut a = Ad::new();
            a.pair(1, TIER_BYTE, 0x0f) == Err(DA_EMASK)
                && a.pair(1, TIER_KEY, 0x0f) == Ok(1)
                && a.count() == 1
                && l.sample(5).map(|x| x.code) == Some(DA_EMASK)
        },
        "static",
    );

    s.add(
        "DA7-判据号越界被拒",
        {
            let mut a = Ad::new();
            a.pair(0, TIER_BYTE, 0) == Err(DA_ERANGE)
                && a.pair(ID_MAX + 1, TIER_BYTE, 0) == Err(DA_ERANGE)
                && a.count() == 0
                && l.sample(3).map(|x| x.code) == Some(DA_ERANGE)
        },
        "static",
    );

    s.add(
        "DA8-重复配对被拒",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_BYTE, 0);
            a.pair(1, TIER_KEY, 0) == Err(DA_EDUP) && a.count() == 1 && l.sample(9).map(|x| x.code) == Some(DA_EDUP)
        },
        "static",
    );

    s.add(
        "DA9-空指纹不许观测",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_KEY, 0);
            a.observe(1, 0, 1) == Err(DA_EGAP)
                && a.observe(1, 1, 0) == Err(DA_EGAP)
                && a.at(0).map(|r| r.seen) == Some(false)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DA10-未配对不许裁定",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_BYTE, 0);
            let early = a.judge(1) == Err(DA_EGAP);
            let none = a.judge(9) == Err(DA_EUNK);
            let _ = a.observe(1, 5, 5);
            early && none && a.judge(1) == Ok(0) && l.sample(10).map(|x| x.code) == Some(DA_EUNK)
        },
        "static"
    );

    s.add(
        "DA11-配对表满硬上限",
        {
            let mut a = Ad::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.pair(i, TIER_KEY, 0);
                i += 1;
            }
            a.count() == CAP
                && a.pair((CAP + 1) as u32, TIER_KEY, 0) == Err(DA_EFULL)
                && a.consistent()
        },
        "static"
    );

    s.add(
        "DA12-未裁全不许收表",
        {
            let mut a = Ad::new();
            let _ = a.pair(1, TIER_BYTE, 0);
            let _ = a.pair(2, TIER_BYTE, 0);
            let _ = a.observe(1, 1, 1);
            let _ = a.observe(2, 2, 2);
            let early = a.finish() == Err(DA_EGAP);
            let _ = a.judge(1);
            let half = a.finish() == Err(DA_EGAP);
            let _ = a.judge(2);
            early && half && a.finish() == Ok(2) && a.consistent()
        },
        "static"
    );

    s.add(
        "DA13-冻结重放四注入",
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

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11379_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn byte_tier_exact() {
        let mut a = Ad::new();
        assert_eq!(a.pair(1, TIER_BYTE, 0), Ok(1));
        assert_eq!(a.observe(1, 9, 9), Ok(1));
        assert_eq!(a.judge(1), Ok(0));
        assert_eq!(a.finish(), Ok(1));
    }

    #[test]
    fn key_tier_mask() {
        let mut a = Ad::new();
        assert_eq!(a.pair(1, TIER_KEY, 0x0f), Ok(1));
        assert_eq!(a.observe(1, 0x1f, 0x10), Ok(1));
        assert_eq!(a.judge(1), Ok(0x0f));
        assert_eq!(a.same(), 1);

        let mut b = Ad::new();
        assert_eq!(b.pair(1, TIER_KEY, 0xf0), Ok(1));
        assert_eq!(b.observe(1, 0x1f, 0x10), Ok(1));
        assert_eq!(b.judge(1), Err(DA_EMISS));
    }

    #[test]
    fn tier_and_mask_gates() {
        let mut a = Ad::new();
        assert_eq!(a.pair(1, 2, 0), Err(DA_EMODE));
        assert_eq!(a.pair(1, TIER_BYTE, 1), Err(DA_EMASK));
        assert_eq!(a.pair(0, TIER_BYTE, 0), Err(DA_ERANGE));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn observe_once() {
        let mut a = Ad::new();
        let _ = a.pair(1, TIER_KEY, 0);
        assert_eq!(a.observe(1, 0, 0), Err(DA_EGAP));
        assert_eq!(a.observe(1, 3, 3), Ok(1));
        assert_eq!(a.observe(1, 4, 4), Err(DA_EDUP));
        assert_eq!(a.observe(9, 1, 1), Err(DA_EUNK));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
