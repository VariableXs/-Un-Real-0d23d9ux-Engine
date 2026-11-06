//! UNX-F11359 procfs 判据集双机同码汇总（0xec70 段）
//!
//! 同一套判据要在两台机器上各跑一遍，再判断两台机器的结论是否一致。汇总账
//! 只接受**同一快照版本**、**按判据序号递增**、**两机代码完全相同**的条目：
//!
//! 1. **版本闸门**：条目携带快照版本，版本与开户版本不符即 `ESTALE`。拿旧
//!    快照的判据去汇总新快照，条目和结论会错位。
//! 2. **同码闸门**：`a != b` 一律拒绝并只记一次 `diff`，**不入表**。汇总账里
//!    存着的每一条都必须是两机一致的结论，矛盾不许留下半条记录。
//! 3. **齐套闸门**：`finish` 要求条目数恰为 `WANT`。少一条是漏跑，多一条是
//!    多算，两者都不允许宣称「双机同码」。
//!
//! 闸门次序：
//! - `record`：版本不符 → 下标越界 → 重复序号 → 序号不升序 → 两机代码不同 →
//!   表满；
//! - `finish`：条目数不等于 `WANT`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec70;

/// 版本号非法。
pub const DC_EINVAL: u32 = SEG_BASE + 0x01;
/// 判据序号越界。
pub const DC_ERANGE: u32 = SEG_BASE + 0x02;
/// 判据序号重复。
pub const DC_EDUP: u32 = SEG_BASE + 0x03;
/// 判据序号不升序。
pub const DC_EORDER: u32 = SEG_BASE + 0x04;
/// 快照版本与本账不符。
pub const DC_ESTALE: u32 = SEG_BASE + 0x05;
/// 两台机器同一判据代码不同。
pub const DC_EDIFF: u32 = SEG_BASE + 0x06;
/// 条目表满。
pub const DC_EFULL: u32 = SEG_BASE + 0x07;
/// 条目不齐，不能宣称同码。
pub const DC_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 16;
/// 判据总条数。
pub const WANT: usize = 13;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 判据序号上界。
pub const IDX_MAX: u32 = 255;

/// 操作码：登记条目。
pub const OP_REC: u8 = 1;
/// 操作码：观察下一个待登记序号。
pub const OP_NEXT: u8 = 2;
/// 操作码：宣称同码。
pub const OP_FINISH: u8 = 3;
/// 操作码：观察差异计数。
pub const OP_OBS: u8 = 4;

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

/// 一条双机判据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    /// 判据序号。
    pub idx: u32,
    /// 甲机代码。
    pub a: u32,
    /// 乙机代码。
    pub b: u32,
}

/// 双机同码汇总账。
#[derive(Clone, Copy, Debug)]
pub struct Agg {
    ver: u32,
    p: [Pair; CAP],
    n: usize,
    diffs: u32,
}

impl Agg {
    /// 为快照版本 `ver` 开户。
    pub fn open(ver: u32) -> Result<Agg, u32> {
        if ver == 0 {
            return Err(DC_EINVAL);
        }
        Ok(Agg { ver, p: [Pair { idx: 0, a: 0, b: 0 }; CAP], n: 0, diffs: 0 })
    }

    /// 快照版本。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 条。
    pub fn item_at(&self, i: usize) -> Option<Pair> {
        if i >= self.n {
            return None;
        }
        self.p.get(i).copied()
    }

    /// 被拒的两机差异次数。
    pub fn diff_count(&self) -> u32 {
        self.diffs
    }

    /// 最后一个登记序号。
    pub fn last_idx(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        self.p.get(self.n - 1).map(|e| e.idx)
    }

    /// 下一个待登记序号。
    pub fn next_index(&self) -> Result<u32, u32> {
        match self.last_idx() {
            None => Ok(1),
            Some(v) => {
                let n = v.saturating_add(1);
                if n > IDX_MAX {
                    Err(DC_EGAP)
                } else {
                    Ok(n)
                }
            }
        }
    }

    /// 按序号找条目。
    fn find(&self, idx: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.p.get(i) {
                Some(e) => {
                    if e.idx == idx {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记一条双机判据，返回累计条数。
    pub fn record(&mut self, idx: u32, ver: u32, a: u32, b: u32) -> Result<usize, u32> {
        if ver != self.ver {
            return Err(DC_ESTALE);
        }
        if idx == 0 || idx > IDX_MAX {
            return Err(DC_ERANGE);
        }
        if self.find(idx).is_some() {
            return Err(DC_EDUP);
        }
        match self.last_idx() {
            Some(v) => {
                if idx < v {
                    return Err(DC_EORDER);
                }
            }
            None => {}
        }
        if a != b {
            self.diffs = self.diffs.saturating_add(1);
            return Err(DC_EDIFF);
        }
        if self.n >= CAP {
            return Err(DC_EFULL);
        }
        if let Some(slot) = self.p.get_mut(self.n) {
            *slot = Pair { idx, a, b };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 宣称双机同码；条目数必须恰为 `WANT`。
    pub fn finish(&self) -> Result<u32, u32> {
        if self.n != WANT {
            return Err(DC_EGAP);
        }
        Ok(WANT as u32)
    }

    /// 汇总指纹（只覆盖已入表条目）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.ver as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.p.get(i) {
                h = fold(h, e.idx as u64);
                h = fold(h, e.a as u64);
            }
            i += 1;
        }
        fold(h, self.diffs as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.ver == 0 || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let e = match self.p.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.idx == 0 || e.idx > IDX_MAX || e.a != e.b {
                return false;
            }
            if i > 0 {
                match self.p.get(i - 1) {
                    Some(prev) => {
                        if prev.idx >= e.idx {
                            return false;
                        }
                    }
                    None => return false,
                }
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
            return Err(DC_ESLOT);
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
            return Err(DC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 9];
        const CODES: [u32; 6] = [DC_ESTALE, DC_EDUP, DC_EORDER, DC_EDIFF, DC_EFULL, DC_EGAP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DC_EBAL),
            };
            if s.slot as usize != i {
                return Err(DC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DC_EVALUE);
                        }
                    }
                    None => return Err(DC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DC_EVALUE);
                        }
                    }
                    None => return Err(DC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DC_EBAL);
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
            return Err(DC_EBAL);
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
            Err(DC_ESEAL)
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

/// 十四格账本：七条同码登记 → 版本/重复/乱序/异码/表满/不齐。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = match Agg::open(7) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_REC, a.record(1, 7, 0xa1, 0xa1)));
    let _ = l.register(cell(1, OP_REC, a.record(2, 7, 0xb2, 0xb2)));
    let _ = l.register(cell(2, OP_REC, a.record(3, 7, 0, 0)));
    let _ = l.register(cell(3, OP_REC, a.record(4, 7, 0xc4, 0xc4)));
    let _ = l.register(cell(4, OP_REC, a.record(5, 7, 0, 0)));
    let _ = l.register(cell(5, OP_REC, a.record(6, 7, 0xd6, 0xd6)));
    let _ = l.register(cell(6, OP_REC, a.record(8, 7, 0, 0)));
    let _ = l.register(word_cell(7, OP_NEXT, a.next_index()));

    let _ = l.register(cell(8, OP_REC, a.record(8, 6, 5, 5)));
    let _ = l.register(cell(9, OP_REC, a.record(5, 7, 5, 5)));
    let _ = l.register(cell(10, OP_REC, a.record(7, 7, 5, 5)));
    let _ = l.register(cell(11, OP_REC, a.record(9, 7, 3, 4)));

    let mut i = 9u32;
    while i < CAP as u32 + 2 {
        let _ = a.record(i, 7, 0, 0);
        i += 1;
    }
    let _ = l.register(cell(12, OP_REC, a.record(18, 7, 0, 0)));
    let _ = l.register(word_cell(13, OP_FINISH, a.finish()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：同码登记、序号推进、下一序号。
    let mut a = match Agg::open(5) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.record(1, 5, 0, 0);
    let r2 = a.record(2, 5, 9, 9);
    let r3 = a.record(3, 5, 0, 0);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && a.item_at(1) == Some(Pair { idx: 2, a: 9, b: 9 })
        && a.last_idx() == Some(3)
        && a.next_index() == Ok(4)
        && a.diff_count() == 0
        && a.ver() == 5
        && a.consistent();

    // 二路：八类失败各自可辨。
    let bad_open = matches!(Agg::open(0), Err(DC_EINVAL));
    let mut b = match Agg::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let stale = b.record(1, 1, 0, 0) == Err(DC_ESTALE);
    let idx_zero = b.record(0, 2, 0, 0) == Err(DC_ERANGE);
    let idx_high = b.record(IDX_MAX + 1, 2, 0, 0) == Err(DC_ERANGE);
    let untouched = b.count() == 0 && b.diff_count() == 0;
    let first = b.record(3, 2, 7, 7) == Ok(1);
    let lower = b.record(2, 2, 7, 7) == Err(DC_EORDER);
    let dup = b.record(3, 2, 7, 7) == Err(DC_EDUP);
    let diff = b.record(4, 2, 1, 2) == Err(DC_EDIFF) && b.diff_count() == 1;
    let stayed = b.count() == 1;
    let early = b.finish() == Err(DC_EGAP);
    let next = b.next_index() == Ok(4);
    out[2] = bad_open
        && stale
        && idx_zero
        && idx_high
        && untouched
        && first
        && lower
        && dup
        && diff
        && stayed
        && early
        && next
        && b.consistent();

    // 三路：凑齐 WANT 条才许收，表满仍拒。
    let mut q = match Agg::open(11) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 1u32;
    while (i as usize) <= WANT {
        let _ = q.record(i, 11, i, i);
        i += 1;
    }
    let q1 = q.count() == WANT;
    let q2 = q.finish() == Ok(WANT as u32);
    let q3 = q.record((WANT + 1) as u32, 11, 0, 0) == Ok(WANT + 1);
    let q4 = q.finish() == Err(DC_EGAP);
    let mut big = match Agg::open(3) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut k = 1u32;
    while (k as usize) <= CAP {
        let _ = big.record(k, 3, 0, 0);
        k += 1;
    }
    let q5 = big.count() == CAP;
    let q6 = big.record((CAP + 1) as u32, 3, 0, 0) == Err(DC_EFULL);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && big.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11359_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11359.procfs_judge_dual");
    let l = build_ledger();

    s.add_dyn(
        "DC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DC2-条目按序累计",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "DC3-下一序号推进",
        l.sample(7).map(|x| x.value) == Some(9),
        "static",
    );

    s.add(
        "DC4-版本不符被拒",
        {
            let mut a = match Agg::open(9) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let old = a.record(1, 8, 0, 0) == Err(DC_ESTALE);
            let new = a.record(1, 10, 0, 0) == Err(DC_ESTALE);
            let ok = a.record(1, 9, 0, 0) == Ok(1);
            old && new && ok
                && a.count() == 1
                && a.diff_count() == 0
                && a.item_at(0) == Some(Pair { idx: 1, a: 0, b: 0 })
                && l.sample(8).map(|x| x.code) == Some(DC_ESTALE)
        },
        "static",
    );

    s.add(
        "DC5-重复序号被拒",
        l.sample(9).map(|x| x.code) == Some(DC_EDUP),
        "static",
    );

    s.add(
        "DC6-倒序序号被拒",
        l.sample(10).map(|x| x.code) == Some(DC_EORDER),
        "static",
    );

    s.add(
        "DC7-两机异码被拒",
        {
            let mut a = match Agg::open(4) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.record(1, 4, 6, 6);
            let bad = a.record(2, 4, 6, 7);
            bad == Err(DC_EDIFF)
                && a.count() == 1
                && a.diff_count() == 1
                && a.item_at(1).is_none()
                && a.consistent()
                && l.sample(11).map(|x| x.code) == Some(DC_EDIFF)
        },
        "static",
    );

    s.add(
        "DC8-表满被拒",
        l.sample(12).map(|x| x.code) == Some(DC_EFULL),
        "static",
    );

    s.add(
        "DC9-不齐不许收",
        {
            let mut a = match Agg::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let early = a.finish() == Err(DC_EGAP);
            let mut i = 1u32;
            while (i as usize) < WANT {
                let _ = a.record(i, 1, 0, 0);
                i += 1;
            }
            let short = a.finish() == Err(DC_EGAP);
            let _ = a.record((WANT) as u32, 1, 0, 0);
            let done = a.finish() == Ok(WANT as u32);
            early && short && done && l.sample(13).map(|x| x.code) == Some(DC_EGAP)
        },
        "static",
    );

    s.add(
        "DC10-序号越界被拒",
        {
            let mut a = match Agg::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(Agg::open(0), Err(DC_EINVAL))
                && a.record(0, 2, 0, 0) == Err(DC_ERANGE)
                && a.record(IDX_MAX + 1, 2, 0, 0) == Err(DC_ERANGE)
                && a.count() == 0
                && a.next_index() == Ok(1)
        },
        "static",
    );

    s.add(
        "DC11-异码不留半条",
        {
            let mut a = match Agg::open(6) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.record(1, 6, 1, 1);
            let _ = a.record(2, 6, 2, 3);
            let _ = a.record(3, 6, 5, 5);
            a.count() == 2
                && a.diff_count() == 1
                && a.last_idx() == Some(3)
                && a.item_at(1) == Some(Pair { idx: 3, a: 5, b: 5 })
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DC12-满表升序查验",
        {
            let mut a = match Agg::open(3) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.record(i, 3, 0, 0);
                i += 1;
            }
            let mut j = 0usize;
            let mut ok = true;
            while j < a.count() {
                match a.item_at(j) {
                    Some(p) => {
                        if p.idx as usize != j + 1 || p.a != p.b {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                j += 1;
            }
            a.count() == CAP
                && ok
                && a.record((CAP + 1) as u32, 3, 0, 0) == Err(DC_EFULL)
                && a.finish() == Err(DC_EGAP)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "DC13-冻结重放四注入",
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

    fn agg(ver: u32) -> Agg {
        match Agg::open(ver) {
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
        let s = run_unx11359_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn same_codes_only() {
        let mut a = agg(1);
        assert_eq!(a.record(1, 1, 0, 0), Ok(1));
        assert_eq!(a.record(2, 1, 4, 5), Err(DC_EDIFF));
        assert_eq!(a.count(), 1);
        assert_eq!(a.diff_count(), 1);
        assert_eq!(a.record(2, 1, 5, 5), Ok(2));
        assert!(a.consistent());
    }

    #[test]
    fn version_gate() {
        let mut a = agg(12);
        assert_eq!(a.record(1, 11, 0, 0), Err(DC_ESTALE));
        assert_eq!(a.record(1, 13, 0, 0), Err(DC_ESTALE));
        assert_eq!(a.count(), 0);
        assert_eq!(a.record(1, 12, 0, 0), Ok(1));
        assert_eq!(a.ver(), 12);
    }

    #[test]
    fn ordering_and_dup() {
        let mut a = agg(2);
        assert_eq!(a.record(5, 2, 0, 0), Ok(1));
        assert_eq!(a.record(5, 2, 0, 0), Err(DC_EDUP));
        assert_eq!(a.record(4, 2, 0, 0), Err(DC_EORDER));
        assert_eq!(a.record(6, 2, 0, 0), Ok(2));
        assert_eq!(a.last_idx(), Some(6));
        assert_eq!(a.next_index(), Ok(7));
    }

    #[test]
    fn capacity_limit() {
        let mut a = agg(3);
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.record(i, 3, 0, 0), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.record((CAP + 1) as u32, 3, 0, 0), Err(DC_EFULL));
        assert_eq!(a.record((CAP + 1) as u32, 3, 1, 2), Err(DC_EDIFF));
        assert_eq!(a.count(), CAP);
        assert!(a.consistent());
    }

    #[test]
    fn finish_requires_exact_count() {
        let mut a = agg(8);
        assert_eq!(a.finish(), Err(DC_EGAP));
        let mut i = 1u32;
        while (i as usize) <= WANT {
            let _ = a.record(i, 8, 0, 0);
            i += 1;
        }
        assert_eq!(a.finish(), Ok(WANT as u32));
        let _ = a.record((WANT + 1) as u32, 8, 0, 0);
        assert_eq!(a.finish(), Err(DC_EGAP));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
