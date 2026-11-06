//! UNX-F11377 快照库版本化与语料清单
//!
//! 快照库里的每条判据都要能溯源：**母版号 + 语料档 + 内容指纹**，三件缺一
//! 即悬空引用。清单还绑定一个快照版本，版本不符的清单不能拿来对账：
//!
//! 1. **版本闸门**：开户版本为 0 拒收；`reconcile` 的版本必须与开户版本相同。
//! 2. **溯源闸门**：母版号为 0 或语料档为 0 都算悬空引用，登记时直接拒。
//! 3. **绑定闸门**：每条判据只许绑定一次内容指纹；零指纹不算绑定，重复
//!    绑定拒收。未全部绑定的清单不许宣称完整。
//!
//! 闸门次序：
//! - `add`：判据号越界 → 母版/语料悬空 → 重复 → 清单满；
//! - `bind`：判据未知 → 指纹为零或已绑定；
//! - `reconcile`：版本不符 → 仍有未绑定条目。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed90;

/// 版本号为 0。
pub const RC_EINVAL: u32 = SEG_BASE + 0x01;
/// 判据号越界。
pub const RC_ERANGE: u32 = SEG_BASE + 0x02;
/// 母版号/语料档悬空。
pub const RC_EDANG: u32 = SEG_BASE + 0x03;
/// 判据重复登记。
pub const RC_EDUP: u32 = SEG_BASE + 0x04;
/// 清单已满。
pub const RC_EFULL: u32 = SEG_BASE + 0x05;
/// 绑定非法或重复绑定。
pub const RC_EBIND: u32 = SEG_BASE + 0x06;
/// 清单版本与本账不符。
pub const RC_ESTALE: u32 = SEG_BASE + 0x07;
/// 清单不完整。
pub const RC_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 清单容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 判据号上界。
pub const ID_MAX: u32 = 64;

/// 操作码：登记判据。
pub const OP_ADD: u8 = 1;
/// 操作码：绑定语料指纹。
pub const OP_BIND: u8 = 2;
/// 操作码：版本对账。
pub const OP_RECON: u8 = 3;
/// 操作码：溯源查询。
pub const OP_TRACE: u8 = 4;

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

/// 清单里的一条判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 判据号。
    pub id: u32,
    /// 母版号。
    pub master: u32,
    /// 语料档号。
    pub corpus: u32,
    /// 内容指纹（0 表示未绑定）。
    pub hash: u32,
}

/// 版本化语料清单。
#[derive(Clone, Copy, Debug)]
pub struct Man {
    ver: u32,
    e: [Ent; CAP],
    n: usize,
}

impl Man {
    /// 为快照版本 `ver` 开户。
    pub fn open(ver: u32) -> Result<Man, u32> {
        if ver == 0 {
            return Err(RC_EINVAL);
        }
        Ok(Man { ver, e: [Ent { id: 0, master: 0, corpus: 0, hash: 0 }; CAP], n: 0 })
    }

    /// 快照版本。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Ent> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 已绑定条数。
    pub fn bound(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.e.get(i) {
                if e.hash != 0 {
                    c = c.saturating_add(1);
                }
            }
            i += 1;
        }
        c
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
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

    /// 登记一条判据（必须能溯源到母版号与语料档）。
    pub fn add(&mut self, id: u32, master: u32, corpus: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(RC_ERANGE);
        }
        if master == 0 || corpus == 0 {
            return Err(RC_EDANG);
        }
        if self.find(id).is_some() {
            return Err(RC_EDUP);
        }
        if self.n >= CAP {
            return Err(RC_EFULL);
        }
        if let Some(slot) = self.e.get_mut(self.n) {
            *slot = Ent { id, master, corpus, hash: 0 };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 绑定内容指纹，返回已绑定条数。
    pub fn bind(&mut self, id: u32, hash: u32) -> Result<u32, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(RC_ERANGE),
        };
        if hash == 0 {
            return Err(RC_EBIND);
        }
        match self.e.get(at) {
            Some(e) => {
                if e.hash != 0 {
                    return Err(RC_EBIND);
                }
            }
            None => return Err(RC_ERANGE),
        }
        if let Some(slot) = self.e.get_mut(at) {
            slot.hash = hash;
        }
        Ok(self.bound())
    }

    /// 版本对账：版本相符且全部绑定才算完整。
    pub fn reconcile(&self, ver: u32) -> Result<u32, u32> {
        if ver != self.ver {
            return Err(RC_ESTALE);
        }
        if self.bound() != self.n as u32 {
            return Err(RC_EGAP);
        }
        Ok(self.n as u32)
    }

    /// 溯源：返回母版号。
    pub fn trace(&self, id: u32) -> Result<u32, u32> {
        match self.find(id) {
            Some(at) => match self.e.get(at) {
                Some(e) => Ok(e.master),
                None => Err(RC_ERANGE),
            },
            None => Err(RC_ERANGE),
        }
    }

    /// 清单指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.ver as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.e.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.master as u64);
                h = fold(h, e.corpus as u64);
                h = fold(h, e.hash as u64);
            }
            i += 1;
        }
        h
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.ver == 0 || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let e = match self.e.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.master == 0 || e.corpus == 0 {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.e.get(j) {
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
            return Err(RC_ESLOT);
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
            return Err(RC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 101, 2, 3, 3];
        const CODES: [u32; 6] = [RC_ERANGE, RC_EDANG, RC_EBIND, RC_ESTALE, RC_EGAP, RC_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RC_EBAL),
            };
            if s.slot as usize != i {
                return Err(RC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RC_EVALUE);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RC_EVALUE);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RC_EBAL);
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
            return Err(RC_EBAL);
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
            Err(RC_ESEAL)
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

/// 十四格账本：登记/绑定/溯源/对账 → 越界/悬空/重复绑定/旧版本/不齐/满表。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = match Man::open(3) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_ADD, m.add(1, 100, 7)));
    let _ = l.register(cell(1, OP_ADD, m.add(2, 101, 8)));
    let _ = l.register(cell(2, OP_ADD, m.add(0, 100, 7)));
    let _ = l.register(cell(3, OP_ADD, m.add(3, 0, 9)));
    let _ = l.register(cell(4, OP_ADD, m.add(3, 102, 9)));
    let _ = l.register(word_cell(5, OP_BIND, m.bind(1, 0xaa)));
    let _ = l.register(word_cell(6, OP_BIND, m.bind(1, 0xbb)));
    let _ = l.register(word_cell(7, OP_TRACE, m.trace(2)));
    let _ = l.register(word_cell(8, OP_RECON, m.reconcile(4)));
    let _ = l.register(word_cell(9, OP_RECON, m.reconcile(3)));
    let _ = l.register(word_cell(10, OP_BIND, m.bind(2, 0xbb)));
    let _ = l.register(word_cell(11, OP_BIND, m.bind(3, 0xcc)));
    let _ = l.register(word_cell(12, OP_RECON, m.reconcile(3)));

    let _ = m.add(4, 103, 10);
    let _ = m.add(5, 104, 11);
    let _ = m.add(6, 105, 12);
    let _ = l.register(cell(13, OP_ADD, m.add(7, 106, 13)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：登记 → 绑定 → 溯源 → 对账。
    let mut a = match Man::open(5) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let a1 = a.add(1, 200, 30);
    let a2 = a.add(2, 201, 31);
    let b1 = a.bind(1, 0x11);
    let b2 = a.bind(2, 0x22);
    out[1] = a1 == Ok(1)
        && a2 == Ok(2)
        && b1 == Ok(1)
        && b2 == Ok(2)
        && a.trace(1) == Ok(200)
        && a.trace(2) == Ok(201)
        && a.bound() == 2
        && a.reconcile(5) == Ok(2)
        && a.ver() == 5
        && a.seal() != 0
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改清单。
    let mut b = match Man::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let open0 = matches!(Man::open(0), Err(RC_EINVAL));
    let range = b.add(0, 1, 1) == Err(RC_ERANGE) && b.add(ID_MAX + 1, 1, 1) == Err(RC_ERANGE);
    let dang = b.add(1, 0, 1) == Err(RC_EDANG) && b.add(1, 1, 0) == Err(RC_EDANG);
    let untouched = b.count() == 0;
    let ok1 = b.add(1, 10, 1) == Ok(1);
    let dup = b.add(1, 10, 1) == Err(RC_EDUP);
    let stale = b.reconcile(3) == Err(RC_ESTALE);
    let gap = b.reconcile(2) == Err(RC_EGAP);
    let bad_bind = b.bind(1, 0) == Err(RC_EBIND);
    let trace_bad = b.trace(9) == Err(RC_ERANGE);
    let h = b.seal();
    let _ = b.bind(1, 0x31);
    let rebind = b.bind(1, 0x32) == Err(RC_EBIND);
    let mut c = match Man::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = c.add(i, i, i);
        i += 1;
    }
    let full = c.add((CAP + 1) as u32, 1, 1) == Err(RC_EFULL);
    out[2] = open0
        && range
        && dang
        && untouched
        && ok1
        && dup
        && stale
        && gap
        && bad_bind
        && trace_bad
        && h != 0
        && rebind
        && full
        && b.consistent()
        && c.consistent();

    // 三路：满表硬拒，绑定齐了才能对账。
    let mut q = match Man::open(9) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.add(i, 300 + i, i);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.add((CAP + 1) as u32, 1, 1) == Err(RC_EFULL);
    let q3 = q.reconcile(9) == Err(RC_EGAP);
    let mut k = 1u32;
    while (k as usize) <= CAP {
        let _ = q.bind(k, k);
        k += 1;
    }
    let q4 = q.bound() == CAP as u32;
    let q5 = q.reconcile(9) == Ok(CAP as u32);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11377_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11377.snapshot_corpus");
    let l = build_ledger();

    s.add_dyn(
        "RC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RC2-登记绑定溯源对账",
        {
            let mut a = match Man::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 10, 1);
            let _ = a.bind(1, 0xabc);
            a.trace(1) == Ok(10)
                && a.bound() == 1
                && a.reconcile(1) == Ok(1)
                && a.seal() != 0
                && a.consistent()
                && l.sample(7).map(|x| x.value) == Some(101)
        },
        "static",
    );

    s.add(
        "RC3-版本为零拒开户",
        {
            matches!(Man::open(0), Err(RC_EINVAL))
                && match Man::open(1) {
                    Ok(v) => v.ver() == 1,
                    Err(_) => false,
                }
        },
        "static",
    );

    s.add(
        "RC4-版本不符拒对账",
        {
            let mut a = match Man::open(4) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 1, 1);
            let _ = a.bind(1, 1);
            a.reconcile(3) == Err(RC_ESTALE)
                && a.reconcile(4) == Ok(1)
                && l.sample(8).map(|x| x.code) == Some(RC_ESTALE)
        },
        "static"
    );

    s.add(
        "RC5-母版语料悬空拒收",
        {
            let mut a = match Man::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(1, 0, 1) == Err(RC_EDANG)
                && a.add(1, 1, 0) == Err(RC_EDANG)
                && a.count() == 0
                && l.sample(3).map(|x| x.code) == Some(RC_EDANG)
        },
        "static"
    );

    s.add(
        "RC6-重复判据拒收",
        {
            let mut a = match Man::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(2, 20, 2);
            a.add(2, 21, 3) == Err(RC_EDUP) && a.count() == 1
        },
        "static"
    );

    s.add(
        "RC7-重复绑定拒收",
        {
            let mut a = match Man::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 10, 1);
            let first = a.bind(1, 0x11) == Ok(1);
            let again = a.bind(1, 0x12) == Err(RC_EBIND);
            let zero = a.bind(2, 0x13) == Err(RC_ERANGE);
            first && again && zero && a.bound() == 1 && l.sample(6).map(|x| x.code) == Some(RC_EBIND)
        },
        "static"
    );

    s.add(
        "RC8-未绑全不许对账",
        {
            let mut a = match Man::open(3) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 10, 1);
            let _ = a.add(2, 11, 2);
            let early = a.reconcile(3) == Err(RC_EGAP);
            let _ = a.bind(1, 1);
            let half = a.reconcile(3) == Err(RC_EGAP);
            let _ = a.bind(2, 2);
            early && half && a.reconcile(3) == Ok(2) && l.sample(9).map(|x| x.code) == Some(RC_EGAP)
        },
        "static"
    );

    s.add(
        "RC9-判据号越界拒收",
        {
            let mut a = match Man::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(0, 1, 1) == Err(RC_ERANGE)
                && a.add(ID_MAX + 1, 1, 1) == Err(RC_ERANGE)
                && a.trace(9) == Err(RC_ERANGE)
                && a.count() == 0
                && l.sample(2).map(|x| x.code) == Some(RC_ERANGE)
        },
        "static"
    );

    s.add(
        "RC10-清单满硬上限",
        {
            let mut a = match Man::open(6) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.add(i, 100 + i, i);
                i += 1;
            }
            a.count() == CAP
                && a.add((CAP + 1) as u32, 1, 1) == Err(RC_EFULL)
                && a.consistent()
                && l.sample(13).map(|x| x.code) == Some(RC_EFULL)
        },
        "static"
    );

    s.add(
        "RC11-溯源母版号可读",
        {
            let mut a = match Man::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 70, 1);
            let _ = a.add(2, 71, 2);
            a.trace(1) == Ok(70)
                && a.trace(2) == Ok(71)
                && a.trace(3) == Err(RC_ERANGE)
                && a.at(1).map(|e| e.corpus) == Some(2)
        },
        "static"
    );

    s.add(
        "RC12-绑定计数单调",
        {
            let mut a = match Man::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(1, 10, 1);
            let _ = a.add(2, 11, 2);
            a.bound() == 0
                && a.bind(1, 1) == Ok(1)
                && a.bind(2, 2) == Ok(2)
                && a.bound() == 2
                && a.consistent()
        },
        "static"
    );

    s.add(
        "RC13-冻结重放四注入",
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

    fn man(ver: u32) -> Man {
        match Man::open(ver) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

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
        let s = run_unx11377_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn traceability() {
        let mut m = man(1);
        assert_eq!(m.add(1, 50, 3), Ok(1));
        assert_eq!(m.trace(1), Ok(50));
        assert_eq!(m.bind(1, 0x99), Ok(1));
        assert_eq!(m.reconcile(1), Ok(1));
        assert!(m.consistent());
    }

    #[test]
    fn dangling_rejected() {
        let mut m = man(1);
        assert_eq!(m.add(1, 0, 1), Err(RC_EDANG));
        assert_eq!(m.add(1, 1, 0), Err(RC_EDANG));
        assert_eq!(m.count(), 0);
    }

    #[test]
    fn version_reconcile() {
        let mut m = man(7);
        assert_eq!(m.add(1, 1, 1), Ok(1));
        assert_eq!(m.bind(1, 5), Ok(1));
        assert_eq!(m.reconcile(6), Err(RC_ESTALE));
        assert_eq!(m.reconcile(7), Ok(1));
    }

    #[test]
    fn bind_once() {
        let mut m = man(1);
        let _ = m.add(1, 1, 1);
        assert_eq!(m.bind(1, 0), Err(RC_EBIND));
        assert_eq!(m.bind(1, 2), Ok(1));
        assert_eq!(m.bind(1, 3), Err(RC_EBIND));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
