//! UNX-F11373 判据快照库三元组接口（判据库/执行器/结果仓）
//!
//! 一条判据要穿过三个仓：**判据库**登记、**执行器**领取、**结果仓**落定。
//! 三元组接口只承认单向推进的相位，任何回退或跳步都拒收：
//!
//! 1. **登记闸门**：判据号越界、快照版本不符、重复登记、库满都在入口拦住，
//!    保证库里的条目都带同一个快照版本。
//! 2. **领取闸门**：只有「已登记」的条目能被执行器领取；已领取再领、已落定
//!    再领都属相位错乱。
//! 3. **落定闸门**：只有「已领取」的条目能写结果仓；重复落定必须显式拒收，
//!    否则结果仓会出现同号两份结论。
//!
//! 闸门次序：
//! - `submit`：号越界 → 版本不符 → 重复 → 库满；
//! - `dispatch`：未知条目 → 相位不是已登记；
//! - `settle`：未知条目 → 相位未领取 → 重复落定；
//! - `seal`：空库或仍有未落定条目。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed50;

/// 判据号或版本号越界。
pub const TR_ERANGE: u32 = SEG_BASE + 0x01;
/// 快照版本与本库不符。
pub const TR_ESTALE: u32 = SEG_BASE + 0x02;
/// 判据重复登记。
pub const TR_EDUP: u32 = SEG_BASE + 0x03;
/// 判据库已满。
pub const TR_EFULL: u32 = SEG_BASE + 0x04;
/// 判据不在库中。
pub const TR_EUNK: u32 = SEG_BASE + 0x05;
/// 相位错乱。
pub const TR_EPHASE: u32 = SEG_BASE + 0x06;
/// 重复落定。
pub const TR_ESETTLE: u32 = SEG_BASE + 0x07;
/// 条目不齐，不许收库。
pub const TR_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 判据库容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 判据号上界。
pub const ID_MAX: u32 = 32;

/// 相位：已登记。
pub const ST_LIB: u8 = 0;
/// 相位：已领取。
pub const ST_RUN: u8 = 1;
/// 相位：已落定。
pub const ST_DONE: u8 = 2;

/// 操作码：登记判据。
pub const OP_SUB: u8 = 1;
/// 操作码：执行器领取。
pub const OP_DISPATCH: u8 = 2;
/// 操作码：结果落仓。
pub const OP_SETTLE: u8 = 3;
/// 操作码：收库。
pub const OP_SEAL: u8 = 4;

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

/// 三元组中的一条判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// 判据号。
    pub id: u32,
    /// 快照版本。
    pub ver: u32,
    /// 相位。
    pub stage: u8,
}

/// 判据库 / 执行器 / 结果仓三元组。
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    row: [Item; CAP],
    n: usize,
    sver: u32,
    runs: u32,
    stored: u32,
}

impl Tri {
    /// 为快照版本 `ver` 开库。
    pub fn open(ver: u32) -> Result<Tri, u32> {
        if ver == 0 {
            return Err(TR_ERANGE);
        }
        Ok(Tri { row: [Item { id: 0, ver: 0, stage: ST_LIB }; CAP], n: 0, sver: ver, runs: 0, stored: 0 })
    }

    /// 快照版本。
    pub fn ver(&self) -> u32 {
        self.sver
    }

    /// 库中条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Item> {
        if i >= self.n {
            return None;
        }
        self.row.get(i).copied()
    }

    /// 执行器已领取次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 结果仓已落定条数。
    pub fn stored(&self) -> u32 {
        self.stored
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.row.get(i) {
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

    /// 相位。
    pub fn stage_of(&self, id: u32) -> Option<u8> {
        match self.find(id) {
            Some(i) => match self.row.get(i) {
                Some(e) => Some(e.stage),
                None => None,
            },
            None => None,
        }
    }

    /// 登记一条判据，返回库中条数。
    pub fn submit(&mut self, id: u32, ver: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(TR_ERANGE);
        }
        if ver != self.sver {
            return Err(TR_ESTALE);
        }
        if self.find(id).is_some() {
            return Err(TR_EDUP);
        }
        if self.n >= CAP {
            return Err(TR_EFULL);
        }
        if let Some(slot) = self.row.get_mut(self.n) {
            *slot = Item { id, ver, stage: ST_LIB };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 执行器领取一条已登记判据，返回累计领取次数。
    pub fn dispatch(&mut self, id: u32) -> Result<u32, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(TR_EUNK),
        };
        match self.row.get(at) {
            Some(e) => {
                if e.stage != ST_LIB {
                    return Err(TR_EPHASE);
                }
            }
            None => return Err(TR_EUNK),
        }
        if let Some(slot) = self.row.get_mut(at) {
            slot.stage = ST_RUN;
        }
        self.runs = self.runs.saturating_add(1);
        Ok(self.runs)
    }

    /// 结果落仓，返回累计落定条数。
    pub fn settle(&mut self, id: u32, code: u32) -> Result<u32, u32> {
        let _ = code;
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(TR_EUNK),
        };
        match self.row.get(at) {
            Some(e) => {
                if e.stage == ST_DONE {
                    return Err(TR_ESETTLE);
                }
                if e.stage != ST_RUN {
                    return Err(TR_EPHASE);
                }
            }
            None => return Err(TR_EUNK),
        }
        if let Some(slot) = self.row.get_mut(at) {
            slot.stage = ST_DONE;
        }
        self.stored = self.stored.saturating_add(1);
        Ok(self.stored)
    }

    /// 收库：空库或仍有未落定条目都不许收。
    pub fn seal(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(TR_EGAP);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.row.get(i) {
                Some(e) => {
                    if e.stage != ST_DONE {
                        return Err(TR_EGAP);
                    }
                }
                None => return Err(TR_EGAP),
            }
            i += 1;
        }
        Ok(self.stored)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.sver as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.row.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.stage as u64);
            }
            i += 1;
        }
        fold(h, self.stored as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.sver == 0 || self.n > CAP {
            return false;
        }
        let mut ran = 0u32;
        let mut done = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let e = match self.row.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.stage > ST_DONE {
                return false;
            }
            if e.stage >= ST_RUN {
                ran = ran.saturating_add(1);
            }
            if e.stage == ST_DONE {
                done = done.saturating_add(1);
            }
            let mut j = 0usize;
            while j < i {
                match self.row.get(j) {
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
        ran == self.runs && done == self.stored
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
            return Err(TR_ESLOT);
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
            return Err(TR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 1, 2, 2, 2, 3];
        const CODES: [u32; 6] = [TR_EDUP, TR_ESTALE, TR_ESETTLE, TR_EUNK, TR_EPHASE, TR_EGAP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TR_EBAL),
            };
            if s.slot as usize != i {
                return Err(TR_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TR_EVALUE);
                        }
                    }
                    None => return Err(TR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TR_EBAL);
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
            return Err(TR_EBAL);
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
            Err(TR_ESEAL)
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

/// 十四格账本：登记/领取/落定走通 → 重复/旧版本/重复落定/未知/相位/不齐。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = match Tri::open(7) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_SUB, t.submit(1, 7)));
    let _ = l.register(cell(1, OP_SUB, t.submit(2, 7)));
    let _ = l.register(word_cell(2, OP_DISPATCH, t.dispatch(1)));
    let _ = l.register(word_cell(3, OP_SETTLE, t.settle(1, 0)));
    let _ = l.register(cell(4, OP_SUB, t.submit(1, 7)));
    let _ = l.register(cell(5, OP_SUB, t.submit(3, 6)));
    let _ = l.register(word_cell(6, OP_DISPATCH, t.dispatch(2)));
    let _ = l.register(word_cell(7, OP_SETTLE, t.settle(2, 3)));
    let _ = l.register(word_cell(8, OP_SETTLE, t.settle(2, 3)));
    let _ = l.register(word_cell(9, OP_SETTLE, t.settle(9, 0)));
    let _ = l.register(word_cell(10, OP_DISPATCH, t.dispatch(1)));
    let _ = l.register(word_cell(11, OP_SEAL, t.seal()));
    let _ = l.register(cell(12, OP_SUB, t.submit(4, 7)));
    let _ = l.register(word_cell(13, OP_SEAL, t.seal()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：登记 → 领取 → 落定 → 收库全链通。
    let mut a = match Tri::open(5) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s1 = a.submit(1, 5);
    let s2 = a.submit(2, 5);
    let d1 = a.dispatch(1);
    let d2 = a.dispatch(2);
    let t1 = a.settle(1, 0);
    let t2 = a.settle(2, 9);
    out[1] = s1 == Ok(1)
        && s2 == Ok(2)
        && d1 == Ok(1)
        && d2 == Ok(2)
        && t1 == Ok(1)
        && t2 == Ok(2)
        && a.stage_of(1) == Some(ST_DONE)
        && a.stage_of(2) == Some(ST_DONE)
        && a.stage_of(9).is_none()
        && a.runs() == 2
        && a.stored() == 2
        && a.seal() == Ok(2)
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改相位。
    let mut b = match Tri::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let range = b.submit(0, 2) == Err(TR_ERANGE) && b.submit(ID_MAX + 1, 2) == Err(TR_ERANGE);
    let stale = b.submit(1, 1) == Err(TR_ESTALE);
    let empty = b.seal() == Err(TR_EGAP);
    let untouched = b.count() == 0;
    let ok1 = b.submit(1, 2) == Ok(1);
    let dup = b.submit(1, 2) == Err(TR_EDUP);
    let early = b.settle(1, 0) == Err(TR_EPHASE);
    let unk = b.dispatch(9) == Err(TR_EUNK);
    let h = b.fingerprint();
    let _ = b.dispatch(1);
    let re_disp = b.dispatch(1) == Err(TR_EPHASE);
    let _ = b.settle(1, 0);
    let re_set = b.settle(1, 0) == Err(TR_ESETTLE);
    let phase_kept = b.stage_of(1) == Some(ST_DONE);
    let gap = b.seal() == Ok(1);
    let mut c = match Tri::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = c.submit(i, 2);
        i += 1;
    }
    let full = c.submit((CAP + 1) as u32, 2) == Err(TR_EFULL);
    out[2] = range
        && stale
        && empty
        && untouched
        && ok1
        && dup
        && early
        && unk
        && h != 0
        && re_disp
        && re_set
        && phase_kept
        && gap
        && full
        && b.consistent()
        && c.consistent();

    // 三路：库满硬拒，落定后才可收。
    let mut q = match Tri::open(11) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.submit(i, 11);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.submit((CAP + 1) as u32, 11) == Err(TR_EFULL);
    let q3 = q.seal() == Err(TR_EGAP);
    let mut k = 1u32;
    while (k as usize) <= CAP {
        let _ = q.dispatch(k);
        let _ = q.settle(k, 0);
        k += 1;
    }
    let q4 = q.stored() == CAP as u32;
    let q5 = q.seal() == Ok(CAP as u32);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11373_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11373.triple_pipeline");
    let l = build_ledger();

    s.add_dyn(
        "TR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TR2-登记领取落定全链",
        {
            let mut a = match Tri::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let s1 = a.submit(1, 1);
            let d = a.dispatch(1);
            let t = a.settle(1, 7);
            s1 == Ok(1)
                && d == Ok(1)
                && t == Ok(1)
                && a.stage_of(1) == Some(ST_DONE)
                && a.seal() == Ok(1)
                && a.consistent()
                && l.sample(0).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "TR3-版本不符被拒",
        {
            let mut a = match Tri::open(4) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let old = a.submit(1, 3) == Err(TR_ESTALE);
            let new = a.submit(1, 5) == Err(TR_ESTALE);
            let ok = a.submit(1, 4) == Ok(1);
            old && new && ok && a.count() == 1 && l.sample(5).map(|x| x.code) == Some(TR_ESTALE)
        },
        "static",
    );

    s.add(
        "TR4-重复登记被拒",
        {
            let mut a = match Tri::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.submit(2, 1);
            a.submit(2, 1) == Err(TR_EDUP)
                && a.count() == 1
                && l.sample(4).map(|x| x.code) == Some(TR_EDUP)
        },
        "static",
    );

    s.add(
        "TR5-未领取不许落定",
        {
            let mut a = match Tri::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.submit(1, 1);
            a.settle(1, 0) == Err(TR_EPHASE)
                && a.stored() == 0
                && a.stage_of(1) == Some(ST_LIB)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TR6-重复落定被拒",
        {
            let mut a = match Tri::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.submit(1, 2);
            let _ = a.dispatch(1);
            let first = a.settle(1, 0) == Ok(1);
            let again = a.settle(1, 1) == Err(TR_ESETTLE);
            first && again && a.stored() == 1 && l.sample(8).map(|x| x.code) == Some(TR_ESETTLE)
        },
        "static",
    );

    s.add(
        "TR7-重复领取被拒",
        {
            let mut a = match Tri::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.submit(1, 2);
            let _ = a.dispatch(1);
            a.dispatch(1) == Err(TR_EPHASE)
                && a.runs() == 1
                && l.sample(10).map(|x| x.code) == Some(TR_EPHASE)
        },
        "static",
    );

    s.add(
        "TR8-未知判据被拒",
        {
            let mut a = match Tri::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let d = a.dispatch(9) == Err(TR_EUNK);
            let t = a.settle(9, 0) == Err(TR_EUNK);
            d && t && a.runs() == 0 && a.stored() == 0 && l.sample(9).map(|x| x.code) == Some(TR_EUNK)
        },
        "static",
    );

    s.add(
        "TR9-空库不许收",
        {
            let a = match Tri::open(3) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.seal() == Err(TR_EGAP) && a.count() == 0 && a.consistent()
        },
        "static",
    );

    s.add(
        "TR10-未落定不许收",
        {
            let mut a = match Tri::open(3) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.submit(1, 3);
            let _ = a.submit(2, 3);
            let _ = a.dispatch(1);
            let _ = a.settle(1, 0);
            let short = a.seal() == Err(TR_EGAP);
            let _ = a.dispatch(2);
            let _ = a.settle(2, 0);
            short && a.seal() == Ok(2) && l.sample(13).map(|x| x.code) == Some(TR_EGAP)
        },
        "static",
    );

    s.add(
        "TR11-号越界被拒",
        {
            let mut a = match Tri::open(6) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.submit(0, 6) == Err(TR_ERANGE)
                && a.submit(ID_MAX + 1, 6) == Err(TR_ERANGE)
                && a.count() == 0
                && matches!(Tri::open(0), Err(TR_ERANGE))
        },
        "static",
    );

    s.add(
        "TR12-库满硬上限",
        {
            let mut a = match Tri::open(8) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.submit(i, 8);
                i += 1;
            }
            a.count() == CAP
                && a.submit((CAP + 1) as u32, 8) == Err(TR_EFULL)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TR13-冻结重放四注入",
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

    fn tri(ver: u32) -> Tri {
        match Tri::open(ver) {
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
        let s = run_unx11373_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn stages_advance_once() {
        let mut a = tri(1);
        assert_eq!(a.submit(1, 1), Ok(1));
        assert_eq!(a.settle(1, 0), Err(TR_EPHASE));
        assert_eq!(a.dispatch(1), Ok(1));
        assert_eq!(a.dispatch(1), Err(TR_EPHASE));
        assert_eq!(a.settle(1, 0), Ok(1));
        assert_eq!(a.settle(1, 0), Err(TR_ESETTLE));
        assert!(a.consistent());
    }

    #[test]
    fn version_gate() {
        let mut a = tri(9);
        assert_eq!(a.submit(1, 8), Err(TR_ESTALE));
        assert_eq!(a.submit(1, 9), Ok(1));
        assert_eq!(a.ver(), 9);
    }

    #[test]
    fn seal_requires_all_done() {
        let mut a = tri(2);
        assert_eq!(a.seal(), Err(TR_EGAP));
        assert_eq!(a.submit(1, 2), Ok(1));
        assert_eq!(a.seal(), Err(TR_EGAP));
        let _ = a.dispatch(1);
        let _ = a.settle(1, 0);
        assert_eq!(a.seal(), Ok(1));
    }

    #[test]
    fn capacity_limit() {
        let mut a = tri(3);
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.submit(i, 3), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.submit((CAP + 1) as u32, 3), Err(TR_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
