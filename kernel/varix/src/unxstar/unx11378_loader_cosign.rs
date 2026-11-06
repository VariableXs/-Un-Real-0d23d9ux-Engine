//! UNX-F11378 dlopen 与 ld.so 装载面联签
//!
//! `dlopen` 与 `ld.so` 是两个入口，但必须共用**同一台装载引擎**：同一份
//! 依赖链、同一份对象视图。联签就是把两个入口给出的对象链指纹拿来比对：
//!
//! 1. **装载闸门**：对象号越界、重复装载、依赖缺失都在入口拦住；依赖必须
//!    先于本体在场，链上任何一环缺失都算装载失败。
//! 2. **卸载闸门**：仍有对象依赖它时不许卸；卸载后对象链视图必须同步收缩，
//!    两个入口看到的条数才会一致。
//! 3. **联签闸门**：两入口指纹为空是 `EHASH`（没签可联），指纹不等是
//!    `EDIFF`（两个入口看到的不是同一份链）。
//!
//! 闸门次序：
//! - `load`：对象号越界 → 重复 → 依赖缺失 → 链满；
//! - `drop`：对象未知 → 仍被依赖；
//! - `cosign`：空链 → 指纹为空 → 指纹不等。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeda0;

/// 对象号越界或未知。
pub const LG_ERANGE: u32 = SEG_BASE + 0x01;
/// 对象重复装载。
pub const LG_EDUP: u32 = SEG_BASE + 0x02;
/// 依赖缺失。
pub const LG_EMISS: u32 = SEG_BASE + 0x03;
/// 对象链已满。
pub const LG_EFULL: u32 = SEG_BASE + 0x04;
/// 仍被依赖，不许卸。
pub const LG_EBUSY: u32 = SEG_BASE + 0x05;
/// 联签指纹为空。
pub const LG_EHASH: u32 = SEG_BASE + 0x06;
/// 两入口指纹不等。
pub const LG_EDIFF: u32 = SEG_BASE + 0x07;
/// 空链无法联签。
pub const LG_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LG_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LG_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LG_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LG_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LG_ENOBS: u32 = SEG_BASE + 0x0d;

/// 对象链容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 对象号上界。
pub const ID_MAX: u32 = 64;

/// 操作码：装载对象。
pub const OP_LOAD: u8 = 1;
/// 操作码：卸载对象。
pub const OP_DROP: u8 = 2;
/// 操作码：两入口联签。
pub const OP_COSIGN: u8 = 3;
/// 操作码：对象链自证。
pub const OP_AGREE: u8 = 4;

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

/// 对象链上的一个对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 对象号。
    pub id: u32,
    /// 直接依赖（0 表示无）。
    pub need: u32,
}

/// 装载引擎的对象链视图。
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    o: [Obj; CAP],
    n: usize,
}

impl Chain {
    /// 空链。
    pub const fn new() -> Chain {
        Chain { o: [Obj { id: 0, need: 0 }; CAP], n: 0 }
    }

    /// 链长。
    pub fn n(&self) -> usize {
        self.n
    }

    /// 第 i 个对象。
    pub fn at(&self, i: usize) -> Option<Obj> {
        if i >= self.n {
            return None;
        }
        self.o.get(i).copied()
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.o.get(i) {
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

    /// 装载一个对象，依赖必须已在链上。
    pub fn load(&mut self, id: u32, need: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(LG_ERANGE);
        }
        if self.find(id).is_some() {
            return Err(LG_EDUP);
        }
        if need != 0 && self.find(need).is_none() {
            return Err(LG_EMISS);
        }
        if self.n >= CAP {
            return Err(LG_EFULL);
        }
        if let Some(slot) = self.o.get_mut(self.n) {
            *slot = Obj { id, need };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 卸载一个对象；仍被依赖则拒。
    pub fn drop_obj(&mut self, id: u32) -> Result<usize, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(LG_ERANGE),
        };
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.o.get(i) {
                if e.id != id && e.need == id {
                    return Err(LG_EBUSY);
                }
            }
            i += 1;
        }
        let mut k = at;
        while k + 1 < self.n {
            let next = match self.o.get(k + 1) {
                Some(v) => *v,
                None => return Err(LG_ERANGE),
            };
            if let Some(slot) = self.o.get_mut(k) {
                *slot = next;
            }
            k += 1;
        }
        self.n -= 1;
        Ok(self.n)
    }

    /// 两入口联签：指纹必须非空且相等。
    pub fn cosign(&self, a: u32, b: u32) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(LG_EGAP);
        }
        if a == 0 || b == 0 {
            return Err(LG_EHASH);
        }
        if a != b {
            return Err(LG_EDIFF);
        }
        Ok(self.n as u32)
    }

    /// 对象链自证：非空才可宣称装载面就绪。
    pub fn agree(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(LG_EGAP);
        }
        Ok(self.n as u32)
    }

    /// 链指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.o.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.need as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let e = match self.o.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.need > ID_MAX || e.need == e.id {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.o.get(j) {
                    Some(p) => {
                        if p.id == e.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            if e.need != 0 {
                let mut found = false;
                let mut k = 0usize;
                while k < i {
                    match self.o.get(k) {
                        Some(p) => {
                            if p.id == e.need {
                                found = true;
                            }
                        }
                        None => return false,
                    }
                    k += 1;
                }
                if !found {
                    return false;
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
            return Err(LG_ESLOT);
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
            return Err(LG_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 1, 2, 3, 4];
        const CODES: [u32; 6] = [LG_EDUP, LG_EMISS, LG_ERANGE, LG_EBUSY, LG_EDIFF, LG_EHASH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LG_EBAL),
            };
            if s.slot as usize != i {
                return Err(LG_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LG_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LG_EVALUE);
                        }
                    }
                    None => return Err(LG_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LG_EVALUE);
                        }
                    }
                    None => return Err(LG_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LG_EBAL);
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
            return Err(LG_EBAL);
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
            Err(LG_ESEAL)
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

/// 十四格账本：依赖链装载 → 重复/缺依赖/越界 → 忙卸/逆序卸 → 联签不等/空签
/// → 重装补链。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Chain::new();

    let _ = l.register(cell(0, OP_LOAD, c.load(1, 0)));
    let _ = l.register(cell(1, OP_LOAD, c.load(2, 1)));
    let _ = l.register(cell(2, OP_LOAD, c.load(3, 1)));
    let _ = l.register(cell(3, OP_LOAD, c.load(2, 0)));
    let _ = l.register(cell(4, OP_LOAD, c.load(4, 9)));
    let _ = l.register(cell(5, OP_LOAD, c.load(0, 0)));
    let _ = l.register(cell(6, OP_DROP, c.drop_obj(1)));
    let _ = l.register(cell(7, OP_DROP, c.drop_obj(3)));
    let _ = l.register(cell(8, OP_DROP, c.drop_obj(2)));
    let _ = l.register(word_cell(9, OP_COSIGN, c.cosign(0xaa, 0xab)));
    let _ = l.register(word_cell(10, OP_COSIGN, c.cosign(0, 0xaa)));
    let _ = l.register(cell(11, OP_LOAD, c.load(4, 1)));
    let _ = l.register(cell(12, OP_LOAD, c.load(5, 4)));
    let _ = l.register(cell(13, OP_LOAD, c.load(6, 5)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：依赖链装载、两入口同签、逆序卸载。
    let mut a = Chain::new();
    let r1 = a.load(1, 0);
    let r2 = a.load(2, 1);
    let r3 = a.load(3, 1);
    let cos = a.cosign(0x55, 0x55);
    let agree = a.agree();
    let d3 = a.drop_obj(3);
    let d2 = a.drop_obj(2);
    let d1 = a.drop_obj(1);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && cos == Ok(3)
        && agree == Ok(3)
        && d3 == Ok(2)
        && d2 == Ok(1)
        && d1 == Ok(0)
        && a.n() == 0
        && a.agree() == Err(LG_EGAP)
        && a.seal() != 0
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改链长。
    let mut b = Chain::new();
    let empty = b.agree() == Err(LG_EGAP);
    let range = b.load(0, 0) == Err(LG_ERANGE) && b.load(ID_MAX + 1, 0) == Err(LG_ERANGE);
    let ok1 = b.load(1, 0) == Ok(1);
    let dup = b.load(1, 0) == Err(LG_EDUP);
    let miss = b.load(2, 9) == Err(LG_EMISS);
    let mut i = 2u32;
    while (i as usize) <= CAP {
        let _ = b.load(i, i - 1);
        i += 1;
    }
    let full = b.load((CAP + 1) as u32, CAP as u32) == Err(LG_EFULL);
    let busy = b.drop_obj(1) == Err(LG_EBUSY);
    let hash = b.cosign(0, 0x1) == Err(LG_EHASH);
    let diff = b.cosign(0x1, 0x2) == Err(LG_EDIFF);
    let kept = b.n() == CAP;
    out[2] = empty
        && range
        && ok1
        && dup
        && miss
        && full
        && busy
        && hash
        && diff
        && kept
        && b.consistent();

    // 三路：链满硬拒，逆序卸载把名额全部回收。
    let mut q = Chain::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.load(i, i - 1);
        i += 1;
    }
    let q1 = q.n() == CAP;
    let q2 = q.load((CAP + 1) as u32, CAP as u32) == Err(LG_EFULL);
    let mut k = CAP as u32;
    let mut rev = true;
    while k > 0 {
        if q.drop_obj(k) != Ok((k - 1) as usize) {
            rev = false;
        }
        k -= 1;
    }
    let q3 = rev && q.n() == 0 && q.agree() == Err(LG_EGAP);
    let q4 = q.load(1, 0) == Ok(1);
    out[3] = q1 && q2 && q3 && q4 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11378_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11378.loader_cosign");
    let l = build_ledger();

    let _ = &l;
    s.add_dyn(
        "LG1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LG2-依赖链按序装载",
        {
            let mut a = Chain::new();
            let r1 = a.load(1, 0);
            let r2 = a.load(2, 1);
            let r3 = a.load(3, 2);
            r1 == Ok(1)
                && r2 == Ok(2)
                && r3 == Ok(3)
                && a.at(2) == Some(Obj { id: 3, need: 2 })
                && a.agree() == Ok(3)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LG3-重复装载被拒",
        {
            let mut a = Chain::new();
            let _ = a.load(1, 0);
            a.load(1, 0) == Err(LG_EDUP)
                && a.n() == 1
                && l.sample(3).map(|x| x.code) == Some(LG_EDUP)
        },
        "static",
    );

    s.add(
        "LG4-依赖缺失被拒",
        {
            let mut a = Chain::new();
            let bad = a.load(2, 9) == Err(LG_EMISS);
            let ok = a.load(1, 0) == Ok(1);
            let then = a.load(2, 1) == Ok(2);
            bad && ok && then && a.n() == 2 && l.sample(4).map(|x| x.code) == Some(LG_EMISS)
        },
        "static",
    );

    s.add(
        "LG5-对象号越界被拒",
        {
            let mut a = Chain::new();
            a.load(0, 0) == Err(LG_ERANGE)
                && a.load(ID_MAX + 1, 0) == Err(LG_ERANGE)
                && a.drop_obj(9) == Err(LG_ERANGE)
                && a.n() == 0
                && l.sample(5).map(|x| x.code) == Some(LG_ERANGE)
        },
        "static",
    );

    s.add(
        "LG6-仍被依赖不许卸",
        {
            let mut a = Chain::new();
            let _ = a.load(1, 0);
            let _ = a.load(2, 1);
            let busy = a.drop_obj(1) == Err(LG_EBUSY);
            let ok = a.drop_obj(2) == Ok(1);
            let then = a.drop_obj(1) == Ok(0);
            busy && ok && then && l.sample(6).map(|x| x.code) == Some(LG_EBUSY)
        },
        "static",
    );

    s.add(
        "LG7-两入口指纹不等",
        {
            let mut a = Chain::new();
            let _ = a.load(1, 0);
            a.cosign(0x10, 0x11) == Err(LG_EDIFF)
                && a.cosign(0x10, 0x10) == Ok(1)
                && l.sample(9).map(|x| x.code) == Some(LG_EDIFF)
        },
        "static",
    );

    s.add(
        "LG8-空指纹不许联签",
        {
            let mut a = Chain::new();
            let _ = a.load(1, 0);
            a.cosign(0, 0x10) == Err(LG_EHASH)
                && a.cosign(0x10, 0) == Err(LG_EHASH)
                && l.sample(10).map(|x| x.code) == Some(LG_EHASH)
        },
        "static",
    );

    s.add(
        "LG9-空链无法联签",
        {
            let mut a = Chain::new();
            let c = a.cosign(0x10, 0x10) == Err(LG_EGAP);
            let g = a.agree() == Err(LG_EGAP);
            let _ = a.load(1, 0);
            c && g && a.agree() == Ok(1) && a.consistent()
        },
        "static",
    );

    s.add(
        "LG10-链满硬上限",
        {
            let mut a = Chain::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.load(i, i - 1);
                i += 1;
            }
            a.n() == CAP
                && a.load((CAP + 1) as u32, CAP as u32) == Err(LG_EFULL)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LG11-逆序卸载可清链",
        {
            let mut a = Chain::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.load(i, i - 1);
                i += 1;
            }
            let mut k = CAP as u32;
            let mut ok = true;
            while k > 0 {
                if a.drop_obj(k) != Ok((k - 1) as usize) {
                    ok = false;
                }
                k -= 1;
            }
            ok && a.n() == 0 && a.agree() == Err(LG_EGAP) && a.consistent()
        },
        "static",
    );

    s.add(
        "LG12-同签返回链长",
        {
            let mut a = Chain::new();
            let _ = a.load(1, 0);
            let _ = a.load(2, 1);
            let _ = a.load(3, 1);
            a.cosign(0x22, 0x22) == Ok(3)
                && a.agree() == Ok(3)
                && a.seal() != 0
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LG13-冻结重放四注入",
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
        let s = run_unx11378_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn dependency_chain() {
        let mut c = Chain::new();
        assert_eq!(c.load(2, 1), Err(LG_EMISS));
        assert_eq!(c.load(1, 0), Ok(1));
        assert_eq!(c.load(2, 1), Ok(2));
        assert_eq!(c.cosign(7, 7), Ok(2));
        assert!(c.consistent());
    }

    #[test]
    fn busy_object_cannot_drop() {
        let mut c = Chain::new();
        let _ = c.load(1, 0);
        let _ = c.load(2, 1);
        assert_eq!(c.drop_obj(1), Err(LG_EBUSY));
        assert_eq!(c.drop_obj(2), Ok(1));
        assert_eq!(c.drop_obj(1), Ok(0));
    }

    #[test]
    fn cosign_rules() {
        let mut c = Chain::new();
        assert_eq!(c.cosign(1, 1), Err(LG_EGAP));
        let _ = c.load(1, 0);
        assert_eq!(c.cosign(0, 1), Err(LG_EHASH));
        assert_eq!(c.cosign(1, 2), Err(LG_EDIFF));
        assert_eq!(c.cosign(1, 1), Ok(1));
    }

    #[test]
    fn capacity_and_reclaim() {
        let mut c = Chain::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(c.load(i, i - 1), Ok(i as usize));
            i += 1;
        }
        assert_eq!(c.load((CAP + 1) as u32, CAP as u32), Err(LG_EFULL));
        let mut k = CAP as u32;
        while k > 0 {
            assert_eq!(c.drop_obj(k), Ok((k - 1) as usize));
            k -= 1;
        }
        assert_eq!(c.n(), 0);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
