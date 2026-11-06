//! UNX-F11371 _init/_fini 与构造/析构调用序
//!
//! 装载器按依赖关系调用 `_init`：**依赖先于本体构造**；卸载时按严格逆序
//! 调用 `_fini`：**本体先于依赖析构**。本模块把这条调用序做成可判定的栈：
//!
//! 1. **依赖闸门**：`init` 声明的依赖必须已经在栈上。拿一个还没构造的
//!    依赖去构造本体，等于让本体跑在半成品之上。
//! 2. **逆序闸门**：`fini` 只允许弹出栈顶。跳过栈顶去析构底下的本体，
//!    会把仍在使用它的依赖悬空。
//! 3. **清场闸门**：`finish` 要求栈已完全清空；还有本体没析构就不许宣称
//!    「装卸平衡」。
//!
//! 闸门次序：
//! - `init`：本体号越界 → 依赖号与本体同号 → 本体重复 → 依赖未装载 →
//!   栈满；
//! - `fini`：本体未知 → 非栈顶（逆序违规）；
//! - `finish`：栈非空。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed30;

/// 本体号非法。
pub const IN_EINVAL: u32 = SEG_BASE + 0x01;
/// 本体号越界。
pub const IN_ERANGE: u32 = SEG_BASE + 0x02;
/// 本体重复装载。
pub const IN_EDUP: u32 = SEG_BASE + 0x03;
/// 依赖尚未构造。
pub const IN_EDEP: u32 = SEG_BASE + 0x04;
/// 析构未按逆序。
pub const IN_EORDER: u32 = SEG_BASE + 0x05;
/// 本体未知。
pub const IN_EUNK: u32 = SEG_BASE + 0x06;
/// 调用栈已满。
pub const IN_EFULL: u32 = SEG_BASE + 0x07;
/// 栈未清空，不许收工。
pub const IN_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const IN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const IN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const IN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const IN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const IN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 构造栈容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 本体号上界。
pub const ID_MAX: u32 = 64;

/// 操作码：构造一个本体。
pub const OP_INIT: u8 = 1;
/// 操作码：析构一个本体。
pub const OP_FINI: u8 = 2;
/// 操作码：宣称装卸平衡。
pub const OP_FINISH: u8 = 3;
/// 操作码：观察本体深度。
pub const OP_DEPTH: u8 = 4;

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

/// 构造栈上的一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// 本体号。
    pub id: u32,
    /// 依赖本体号（0 表示无依赖）。
    pub dep: u32,
}

/// 装载调用序账。
#[derive(Clone, Copy, Debug)]
pub struct Ord {
    row: [Step; CAP],
    n: usize,
    closed: u32,
}

impl Ord {
    /// 空栈。
    pub const fn new() -> Ord {
        Ord { row: [Step { id: 0, dep: 0 }; CAP], n: 0, closed: 0 }
    }

    /// 栈深。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 已成功析构的本体数。
    pub fn closed_count(&self) -> u32 {
        self.closed
    }

    /// 第 i 格。
    pub fn at(&self, i: usize) -> Option<Step> {
        if i >= self.n {
            return None;
        }
        self.row.get(i).copied()
    }

    /// 按本体号找栈位。
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

    /// 本体在栈中的深度（1 起）。
    pub fn depth_of(&self, id: u32) -> Option<usize> {
        match self.find(id) {
            Some(i) => Some(i + 1),
            None => None,
        }
    }

    /// 构造一个本体；依赖必须已经在栈上。
    pub fn init(&mut self, id: u32, dep: u32) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(IN_ERANGE);
        }
        if dep == id || dep > ID_MAX {
            return Err(IN_EINVAL);
        }
        if self.find(id).is_some() {
            return Err(IN_EDUP);
        }
        if dep != 0 && self.find(dep).is_none() {
            return Err(IN_EDEP);
        }
        if self.n >= CAP {
            return Err(IN_EFULL);
        }
        if let Some(slot) = self.row.get_mut(self.n) {
            *slot = Step { id, dep };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 析构栈顶本体；逆序之外的调用一律拒绝。
    pub fn fini(&mut self, id: u32) -> Result<usize, u32> {
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(IN_EUNK),
        };
        if at + 1 != self.n {
            return Err(IN_EORDER);
        }
        self.n -= 1;
        self.closed = self.closed.saturating_add(1);
        Ok(self.n)
    }

    /// 宣称装卸平衡：栈必须已清空。
    pub fn finish(&self) -> Result<u32, u32> {
        if self.n != 0 {
            return Err(IN_EGAP);
        }
        Ok(self.closed)
    }

    /// 折入调用序的指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.row.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.dep as u64);
            }
            i += 1;
        }
        fold(h, self.closed as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let e = match self.row.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX || e.dep > ID_MAX || e.dep == e.id {
                return false;
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
            if e.dep != 0 {
                let mut found = false;
                let mut k = 0usize;
                while k < i {
                    match self.row.get(k) {
                        Some(p) => {
                            if p.id == e.dep {
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
            return Err(IN_ESLOT);
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
            return Err(IN_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 3, 2, 1, 8];
        const CODES: [u32; 6] = [IN_EORDER, IN_EUNK, IN_EDUP, IN_EDEP, IN_ERANGE, IN_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(IN_EBAL),
            };
            if s.slot as usize != i {
                return Err(IN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(IN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(IN_EVALUE);
                        }
                    }
                    None => return Err(IN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(IN_EVALUE);
                        }
                    }
                    None => return Err(IN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(IN_EBAL);
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
            return Err(IN_EBAL);
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
            Err(IN_ESEAL)
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

/// 十四格账本：依赖链装卸 → 逆序/未知/重复/缺依赖/越界/栈满。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut o = Ord::new();

    let _ = l.register(cell(0, OP_INIT, o.init(1, 0)));
    let _ = l.register(cell(1, OP_INIT, o.init(2, 1)));
    let _ = l.register(cell(2, OP_INIT, o.init(3, 2)));
    let _ = l.register(cell(3, OP_FINI, o.fini(3)));
    let _ = l.register(cell(4, OP_INIT, o.init(4, 2)));
    let _ = l.register(cell(5, OP_FINI, o.fini(1)));
    let _ = l.register(cell(6, OP_FINI, o.fini(4)));
    let _ = l.register(cell(7, OP_FINI, o.fini(2)));
    let _ = l.register(cell(8, OP_FINI, o.fini(2)));
    let _ = l.register(cell(9, OP_INIT, o.init(1, 0)));
    let _ = l.register(cell(10, OP_INIT, o.init(6, 7)));
    let _ = l.register(cell(11, OP_INIT, o.init(0, 0)));

    let mut i = 2u32;
    while i < CAP as u32 {
        let _ = o.init(i, i - 1);
        i += 1;
    }
    let _ = l.register(cell(12, OP_INIT, o.init(8, 7)));
    let _ = l.register(cell(13, OP_INIT, o.init(9, 8)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：依赖先构造、本体先析构。
    let mut a = Ord::new();
    let r1 = a.init(1, 0);
    let r2 = a.init(2, 1);
    let r3 = a.init(3, 2);
    let d3 = a.depth_of(3);
    let f3 = a.fini(3);
    let f2 = a.fini(2);
    let f1 = a.fini(1);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && d3 == Some(3)
        && f3 == Ok(2)
        && f2 == Ok(1)
        && f1 == Ok(0)
        && a.count() == 0
        && a.closed_count() == 3
        && a.finish() == Ok(3)
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Ord::new();
    let range_zero = b.init(0, 0) == Err(IN_ERANGE);
    let range_high = b.init(ID_MAX + 1, 0) == Err(IN_ERANGE);
    let self_dep = b.init(3, 3) == Err(IN_EINVAL);
    let untouched = b.count() == 0;
    let build = b.init(1, 0) == Ok(1) && b.init(2, 1) == Ok(2);
    let dup = b.init(2, 1) == Err(IN_EDUP);
    let missing = b.init(5, 4) == Err(IN_EDEP);
    let order = b.fini(1) == Err(IN_EORDER);
    let unknown = b.fini(9) == Err(IN_EUNK);
    let early = b.finish() == Err(IN_EGAP);
    let stayed = b.count() == 2 && b.closed_count() == 0;
    let pop = b.fini(2) == Ok(1) && b.fini(1) == Ok(0);
    out[2] = range_zero
        && range_high
        && self_dep
        && untouched
        && build
        && dup
        && missing
        && order
        && unknown
        && early
        && stayed
        && pop
        && b.consistent();

    // 三路：栈满拒装，清空后可复用。
    let mut q = Ord::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.init(i, i - 1);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.init((CAP + 1) as u32, CAP as u32) == Err(IN_EFULL);
    let mut k = CAP as u32;
    while k > 0 {
        let _ = q.fini(k);
        k -= 1;
    }
    let q3 = q.count() == 0 && q.finish() == Ok(CAP as u32);
    let q4 = q.init(1, 0) == Ok(1);
    out[3] = q1 && q2 && q3 && q4 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11371_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11371.init_fini_order");
    let l = build_ledger();

    s.add_dyn(
        "IN1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "IN2-依赖先于本体构造",
        {
            let mut a = Ord::new();
            let r1 = a.init(1, 0);
            let r2 = a.init(2, 1);
            let r3 = a.init(3, 2);
            r1 == Ok(1)
                && r2 == Ok(2)
                && r3 == Ok(3)
                && a.at(0) == Some(Step { id: 1, dep: 0 })
                && a.at(2) == Some(Step { id: 3, dep: 2 })
                && a.depth_of(3) == Some(3)
                && a.depth_of(9).is_none()
                && a.consistent()
        },
        "static",
    );

    s.add(
        "IN3-本体先于依赖析构",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            let _ = a.init(2, 1);
            let _ = a.init(3, 2);
            let x = a.fini(3) == Ok(2);
            let y = a.fini(2) == Ok(1);
            let z = a.fini(1) == Ok(0);
            x && y && z && a.closed_count() == 3 && a.finish() == Ok(3) && a.consistent()
        },
        "static",
    );

    s.add(
        "IN4-重复本体被拒",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            a.init(1, 0) == Err(IN_EDUP)
                && a.count() == 1
                && l.sample(9).map(|x| x.code) == Some(IN_EDUP)
        },
        "static",
    );

    s.add(
        "IN5-依赖未装载被拒",
        {
            let mut a = Ord::new();
            let bad = a.init(5, 4) == Err(IN_EDEP);
            let ok = a.init(4, 0) == Ok(1);
            let then = a.init(5, 4) == Ok(2);
            bad && ok && then && a.depth_of(5) == Some(2) && a.consistent()
        },
        "static",
    );

    s.add(
        "IN6-逆序析构被拒",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            let _ = a.init(2, 1);
            a.fini(1) == Err(IN_EORDER)
                && a.count() == 2
                && a.fini(2) == Ok(1)
                && a.fini(1) == Ok(0)
                && l.sample(5).map(|x| x.code) == Some(IN_EORDER)
        },
        "static",
    );

    s.add(
        "IN7-未知本体被拒",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            a.fini(7) == Err(IN_EUNK)
                && a.count() == 1
                && a.closed_count() == 0
                && l.sample(8).map(|x| x.code) == Some(IN_EUNK)
        },
        "static",
    );

    s.add(
        "IN8-栈满被拒",
        {
            let mut a = Ord::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.init(i, i - 1);
                i += 1;
            }
            a.count() == CAP
                && a.init((CAP + 1) as u32, CAP as u32) == Err(IN_EFULL)
                && a.consistent()
                && l.sample(13).map(|x| x.code) == Some(IN_EFULL)
        },
        "static",
    );

    s.add(
        "IN9-本体号越界被拒",
        {
            let mut a = Ord::new();
            a.init(0, 0) == Err(IN_ERANGE)
                && a.init(ID_MAX + 1, 0) == Err(IN_ERANGE)
                && a.count() == 0
                && a.finish() == Ok(0)
                && l.sample(11).map(|x| x.code) == Some(IN_ERANGE)
        },
        "static",
    );

    s.add(
        "IN10-栈未清不许收工",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            let early = a.finish() == Err(IN_EGAP);
            let _ = a.fini(1);
            early && a.finish() == Ok(1) && a.consistent()
        },
        "static",
    );

    s.add(
        "IN11-失败不改状态",
        {
            let mut a = Ord::new();
            let _ = a.init(1, 0);
            let _ = a.init(2, 1);
            let before = a.seal();
            let e1 = a.init(2, 1) == Err(IN_EDUP);
            let e2 = a.init(5, 4) == Err(IN_EDEP);
            let e3 = a.fini(1) == Err(IN_EORDER);
            let e4 = a.fini(9) == Err(IN_EUNK);
            let e5 = a.init(3, 3) == Err(IN_EINVAL);
            let after = a.seal();
            e1 && e2 && e3 && e4 && e5 && before == after && a.count() == 2 && a.consistent()
        },
        "static",
    );

    s.add(
        "IN12-满栈清空可复用",
        {
            let mut a = Ord::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.init(i, i - 1);
                i += 1;
            }
            let full = a.init((CAP + 1) as u32, CAP as u32) == Err(IN_EFULL);
            let mut k = CAP as u32;
            let mut rev = true;
            while k > 0 {
                if a.fini(k) != Ok((k - 1) as usize) {
                    rev = false;
                }
                k -= 1;
            }
            full && rev
                && a.count() == 0
                && a.finish() == Ok(CAP as u32)
                && a.init(1, 0) == Ok(1)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "IN13-冻结重放四注入",
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

    fn ord() -> Ord {
        Ord::new()
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
        let s = run_unx11371_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn dependency_before_body() {
        let mut a = ord();
        assert_eq!(a.init(2, 1), Err(IN_EDEP));
        assert_eq!(a.init(1, 0), Ok(1));
        assert_eq!(a.init(2, 1), Ok(2));
        assert_eq!(a.at(1), Some(Step { id: 2, dep: 1 }));
        assert!(a.consistent());
    }

    #[test]
    fn reverse_teardown_only() {
        let mut a = ord();
        assert_eq!(a.init(1, 0), Ok(1));
        assert_eq!(a.init(2, 1), Ok(2));
        assert_eq!(a.fini(1), Err(IN_EORDER));
        assert_eq!(a.fini(2), Ok(1));
        assert_eq!(a.fini(1), Ok(0));
        assert_eq!(a.finish(), Ok(2));
    }

    #[test]
    fn rejection_leaves_state() {
        let mut a = ord();
        assert_eq!(a.init(1, 0), Ok(1));
        let h = a.seal();
        assert_eq!(a.init(0, 0), Err(IN_ERANGE));
        assert_eq!(a.init(3, 3), Err(IN_EINVAL));
        assert_eq!(a.init(1, 0), Err(IN_EDUP));
        assert_eq!(a.fini(4), Err(IN_EUNK));
        assert_eq!(a.seal(), h);
    }

    #[test]
    fn capacity_is_hard() {
        let mut a = ord();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.init(i, i - 1), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.init((CAP + 1) as u32, CAP as u32), Err(IN_EFULL));
        assert_eq!(a.count(), CAP);
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
