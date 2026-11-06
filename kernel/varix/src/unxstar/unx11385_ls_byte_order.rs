//! UNX-F11385 ls 排序与 LC_ALL=C 字节序锚
//!
//! 排序锚在 **C locale 的字节序**：逐字节无符号比较，数字 < 大写 < 小写，
//! 不做任何语言相关的合字/重音折叠。locale 变体差异只登记不展开：非 C 的
//! locale 直接拒，非 ASCII 名字也直接拒，避免「看起来排好了」却与基准机不同。
//!
//! 闸门次序：
//! - `open`：locale 不是 C；
//! - `add`：空名或超长 → 含非 ASCII 字节 → 重名 → 表满；
//! - `sort`：空表零移位（不算失败），逐位插入并记录移位次数；
//! - `rank`：未排序不得取秩 → 名字不在表内。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee10;

/// 非 C locale。
pub const LSS_ELOCALE: u32 = SEG_BASE + 0x01;
/// 名字为空。
pub const LSS_ENAME: u32 = SEG_BASE + 0x02;
/// 名字超长。
pub const LSS_ELONG: u32 = SEG_BASE + 0x03;
/// 名字重复。
pub const LSS_EDUP: u32 = SEG_BASE + 0x04;
/// 名字含非 ASCII 字节。
pub const LSS_ENONASC: u32 = SEG_BASE + 0x05;
/// 条目表满。
pub const LSS_EFULL: u32 = SEG_BASE + 0x06;
/// 未排序不得取秩。
pub const LSS_EUNSORT: u32 = SEG_BASE + 0x07;
/// 名字不在表内。
pub const LSS_ENOENT: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 12;
/// 名字字节上限。
pub const NAME_MAX: usize = 12;
/// C locale 编号。
pub const LOCALE_C: u8 = 0;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：入表。
pub const OP_ADD: u8 = 1;
/// 操作码：排序。
pub const OP_SORT: u8 = 2;
/// 操作码：取秩。
pub const OP_RANK: u8 = 3;
/// 操作码：开户。
pub const OP_OPEN: u8 = 4;

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

/// 一个待排序名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    b: [u8; NAME_MAX],
    nlen: u8,
}

impl Name {
    /// 空白。
    pub const fn blank() -> Name {
        Name { b: [0; NAME_MAX], nlen: 0 }
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        match self.b.get(..self.nlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 拷入名字。
fn put(dst: &mut [u8; NAME_MAX], src: &[u8]) -> u8 {
    let n = if src.len() > NAME_MAX { NAME_MAX } else { src.len() };
    let mut i = 0usize;
    while i < n {
        if let (Some(d), Some(s)) = (dst.get_mut(i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    n as u8
}

/// 字节序比较：`a < b`。
fn less(a: &[u8], b: &[u8]) -> bool {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        let x = match a.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x != y {
            return x < y;
        }
        i += 1;
    }
    a.len() < b.len()
}

/// 字节序相等。
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 是否全 ASCII。
fn ascii_only(b: &[u8]) -> bool {
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(x) => {
                if *x >= 0x80 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 字节序排序表。
#[derive(Clone, Copy, Debug)]
pub struct Ord {
    locale: u8,
    e: [Name; CAP],
    n: usize,
    sorted: bool,
    shifts: u32,
}

impl Ord {
    /// 按 locale 开户。
    pub fn open(locale: u8) -> Result<Ord, u32> {
        if locale != LOCALE_C {
            return Err(LSS_ELOCALE);
        }
        Ok(Ord { locale, e: [Name::blank(); CAP], n: 0, sorted: true, shifts: 0 })
    }

    /// locale。
    pub fn locale(&self) -> u8 {
        self.locale
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已排序。
    pub fn is_sorted(&self) -> bool {
        self.sorted
    }

    /// 累计移位次数。
    pub fn shifts(&self) -> u32 {
        self.shifts
    }

    /// 第 i 个名字。
    pub fn at(&self, i: usize) -> Option<&[u8]> {
        match self.e.get(i) {
            Some(v) if i < self.n => Some(v.bytes()),
            _ => None,
        }
    }

    /// 按名字找下标。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(v) => {
                    if same_bytes(v.bytes(), name) {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 入表一条，返回累计条目数。
    ///
    /// 闸门次序：空名/超长 → 非 ASCII → 重名 → 表满。
    pub fn add(&mut self, name: &[u8]) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(LSS_ENAME);
        }
        if name.len() > NAME_MAX {
            return Err(LSS_ELONG);
        }
        if !ascii_only(name) {
            return Err(LSS_ENONASC);
        }
        if self.find(name).is_some() {
            return Err(LSS_EDUP);
        }
        if self.n >= CAP {
            return Err(LSS_EFULL);
        }
        let slot = match self.e.get_mut(self.n) {
            Some(v) => v,
            None => return Err(LSS_EFULL),
        };
        slot.b = [0; NAME_MAX];
        slot.nlen = put(&mut slot.b, name);
        self.n += 1;
        self.sorted = self.n <= 1;
        Ok(self.n)
    }

    /// 插入排序，返回本次移位次数。
    ///
    /// 闸门：空表零移位。
    pub fn sort(&mut self) -> Result<u32, u32> {
        let mut moved = 0u32;
        let mut i = 1usize;
        while i < self.n {
            let cur = match self.e.get(i) {
                Some(v) => *v,
                None => return Err(LSS_ENOENT),
            };
            let mut j = i;
            while j > 0 {
                let prev = match self.e.get(j - 1) {
                    Some(v) => *v,
                    None => return Err(LSS_ENOENT),
                };
                if less(prev.bytes(), cur.bytes()) {
                    break;
                }
                if let Some(dst) = self.e.get_mut(j) {
                    *dst = prev;
                }
                j -= 1;
                moved = moved.saturating_add(1);
            }
            if let Some(dst) = self.e.get_mut(j) {
                *dst = cur;
            }
            i += 1;
        }
        self.shifts = self.shifts.saturating_add(moved);
        self.sorted = true;
        Ok(moved)
    }

    /// 名字的秩（从 0 起）。
    ///
    /// 闸门次序：未排序 → 空名/超长 → 不在表内。
    pub fn rank(&self, name: &[u8]) -> Result<u32, u32> {
        if !self.sorted {
            return Err(LSS_EUNSORT);
        }
        if name.is_empty() || name.len() > NAME_MAX || !ascii_only(name) {
            return Err(LSS_ENOENT);
        }
        match self.find(name) {
            Some(i) => Ok(i as u32),
            None => Err(LSS_ENOENT),
        }
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.locale as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.e.get(i) {
                let b = v.bytes();
                let mut k = 0usize;
                while k < b.len() {
                    if let Some(x) = b.get(k) {
                        h = fold(h, *x as u64);
                    }
                    k += 1;
                }
                h = fold(h, 0xff);
            }
            i += 1;
        }
        h = fold(h, self.sorted as u64);
        fold(h, self.shifts as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.locale != LOCALE_C || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let v = match self.e.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if v.nlen == 0 || v.nlen as usize > NAME_MAX || !ascii_only(v.bytes()) {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                match self.e.get(j) {
                    Some(p) => {
                        if same_bytes(p.bytes(), v.bytes()) {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            if self.sorted && i > 0 {
                match self.e.get(i - 1) {
                    Some(p) => {
                        if less(v.bytes(), p.bytes()) {
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
            return Err(LSS_ESLOT);
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
            return Err(LSS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 1, 4, 3, 0];
        const CODES: [u32; 6] =
            [LSS_ENAME, LSS_ELONG, LSS_EDUP, LSS_ENONASC, LSS_ELOCALE, LSS_EUNSORT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSS_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSS_EVALUE);
                        }
                    }
                    None => return Err(LSS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSS_EVALUE);
                        }
                    }
                    None => return Err(LSS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSS_EBAL);
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
            return Err(LSS_EBAL);
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
            Err(LSS_ESEAL)
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

/// 十四格账本：8 允 + 6 拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_ADD, a.add(b"Makefile")));
    let _ = l.register(cell(1, OP_ADD, a.add(b"bin")));
    let _ = l.register(cell(2, OP_ADD, a.add(b"README")));
    let _ = l.register(word_cell(3, OP_SORT, a.sort()));
    let _ = l.register(word_cell(4, OP_RANK, a.rank(b"README")));
    let _ = l.register(cell(5, OP_ADD, a.add(b"0abc")));
    let _ = l.register(word_cell(6, OP_SORT, a.sort()));
    let _ = l.register(word_cell(7, OP_RANK, a.rank(b"0abc")));

    let _ = l.register(cell(8, OP_ADD, a.add(b"")));
    let _ = l.register(cell(9, OP_ADD, a.add(b"abcdefghijklm")));
    let _ = l.register(cell(10, OP_ADD, a.add(b"bin")));
    let _ = l.register(cell(11, OP_ADD, a.add(b"caf\xc3\xa9")));
    let _ = l.register(word_cell(12, OP_OPEN, Ord::open(1).map(|v| v.locale() as u32)));

    let mut r = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = r.add(b"a");
    let _ = r.add(b"b");
    let _ = l.register(word_cell(13, OP_RANK, r.rank(b"a")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：C 字节序、数字 < 大写 < 小写。
    let mut a = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.add(b"a");
    let r2 = a.add(b"Z");
    let r3 = a.add(b"0z");
    let before = a.rank(b"a") == Err(LSS_EUNSORT);
    let mv = a.sort();
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && before
        && mv == Ok(3)
        && a.at(0) == Some(&b"0z"[..])
        && a.at(1) == Some(&b"Z"[..])
        && a.at(2) == Some(&b"a"[..])
        && a.rank(b"Z") == Ok(1)
        && a.is_sorted()
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let empty = b.add(b"") == Err(LSS_ENAME);
    let long = b.add(b"abcdefghijklm") == Err(LSS_ELONG);
    let nonasc = b.add(b"caf\xc3\xa9") == Err(LSS_ENONASC);
    let first = b.add(b"b") == Ok(1);
    let dup = b.add(b"b") == Err(LSS_EDUP);
    let missing = b.rank(b"zz") == Err(LSS_ENOENT);
    let small = b.rank(b"") == Err(LSS_ENOENT);
    let untouched = b.count() == 1;
    let bad_locale = Ord::open(1).is_err();
    let mut f = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0u32;
    while (i as usize) < CAP {
        let name: [u8; 2] = [b'n', b'0' + i as u8];
        let _ = f.add(&name);
        i += 1;
    }
    let full = f.add(b"zz") == Err(LSS_EFULL);
    out[2] = empty
        && long
        && nonasc
        && first
        && dup
        && missing
        && small
        && untouched
        && bad_locale
        && full
        && b.consistent()
        && f.consistent();

    // 三路：表满与移位下限/上限。
    let mut q = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0u32;
    while (i as usize) < CAP {
        let name: [u8; 4] = [b'm', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8, b'x'];
        let _ = q.add(&name);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.add(b"zzz") == Err(LSS_EFULL);
    let q3 = q.sort() == Ok(0);
    let mut e = match Ord::open(LOCALE_C) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let q4 = e.sort() == Ok(0);
    let q5 = e.rank(b"a") == Err(LSS_ENOENT);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11385_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11385.ls_byte_order");
    let l = build_ledger();

    s.add_dyn(
        "LSS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSS2-字节序排序结果",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"Makefile");
            let _ = a.add(b"bin");
            let _ = a.add(b"README");
            let _ = a.sort();
            a.at(0) == Some(&b"Makefile"[..])
                && a.at(1) == Some(&b"README"[..])
                && a.at(2) == Some(&b"bin"[..])
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSS3-排序移位计数",
        l.sample(3).map(|x| x.value) == Some(1)
            && l.sample(4).map(|x| x.value) == Some(1)
            && l.sample(6).map(|x| x.value) == Some(3)
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "LSS4-空名被拒",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"") == Err(LSS_ENAME)
                && a.count() == 0
                && l.sample(8).map(|x| x.code) == Some(LSS_ENAME)
        },
        "static",
    );

    s.add(
        "LSS5-超长名被拒",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"abcdefghijklm") == Err(LSS_ELONG)
                && a.count() == 0
                && l.sample(9).map(|x| x.code) == Some(LSS_ELONG)
        },
        "static",
    );

    s.add(
        "LSS6-重名被拒",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one = a.add(b"bin") == Ok(1);
            let dup = a.add(b"bin") == Err(LSS_EDUP);
            one && dup && a.count() == 1 && l.sample(10).map(|x| x.code) == Some(LSS_EDUP)
        },
        "static",
    );

    s.add(
        "LSS7-非 ASCII 被拒",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"caf\xc3\xa9") == Err(LSS_ENONASC)
                && a.add(b"\x80") == Err(LSS_ENONASC)
                && a.count() == 0
                && l.sample(11).map(|x| x.code) == Some(LSS_ENONASC)
        },
        "static",
    );

    s.add(
        "LSS8-非 C locale 被拒",
        {
            let mut ok = Ord::open(1).is_err() && Ord::open(2).is_err();
            if let Ok(v) = Ord::open(1) {
                ok = v.count() == 0;
            }
            ok && l.sample(12).map(|x| x.code) == Some(LSS_ELOCALE)
        },
        "static",
    );

    s.add(
        "LSS9-未排序不得取秩",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"b");
            let _ = a.add(b"a");
            let early = a.rank(b"a") == Err(LSS_EUNSORT);
            let _ = a.sort();
            early && a.rank(b"a") == Ok(0) && l.sample(13).map(|x| x.code) == Some(LSS_EUNSORT)
        },
        "static",
    );

    s.add(
        "LSS10-未知名取秩",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"b");
            let _ = a.sort();
            a.rank(b"zz") == Err(LSS_ENOENT)
                && a.rank(b"") == Err(LSS_ENOENT)
                && a.rank(b"abcdefghijklm") == Err(LSS_ENOENT)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSS11-表满被拒",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 0u32;
            while (i as usize) < CAP {
                let name: [u8; 2] = [b'n', b'0' + i as u8];
                let _ = a.add(&name);
                i += 1;
            }
            a.count() == CAP
                && a.add(b"zz") == Err(LSS_EFULL)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSS12-大小写数字混排锚",
        {
            let mut a = match Ord::open(LOCALE_C) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"a");
            let _ = a.add(b"Z");
            let _ = a.add(b"0z");
            let _ = a.add(b"A0");
            let mv = a.sort();
            mv == Ok(5)
                && a.at(0) == Some(&b"0z"[..])
                && a.at(1) == Some(&b"A0"[..])
                && a.at(2) == Some(&b"Z"[..])
                && a.at(3) == Some(&b"a"[..])
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSS13-冻结重放四注入",
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
        match Ord::open(LOCALE_C) {
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
        let s = run_unx11385_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn byte_order_anchor() {
        let mut a = ord();
        assert_eq!(a.add(b"a"), Ok(1));
        assert_eq!(a.add(b"Z"), Ok(2));
        assert_eq!(a.add(b"0z"), Ok(3));
        assert_eq!(a.sort(), Ok(3));
        assert_eq!(a.at(0), Some(&b"0z"[..]));
        assert_eq!(a.at(1), Some(&b"Z"[..]));
        assert_eq!(a.at(2), Some(&b"a"[..]));
        assert!(a.consistent());
    }

    #[test]
    fn shift_count_monotone() {
        let mut a = ord();
        assert_eq!(a.add(b"Makefile"), Ok(1));
        assert_eq!(a.add(b"bin"), Ok(2));
        assert_eq!(a.add(b"README"), Ok(3));
        assert_eq!(a.sort(), Ok(1));
        assert_eq!(a.add(b"0abc"), Ok(4));
        assert_eq!(a.sort(), Ok(3));
        assert_eq!(a.shifts(), 4);
    }

    #[test]
    fn non_ascii_rejected() {
        let mut a = ord();
        assert_eq!(a.add(b"caf\xc3\xa9"), Err(LSS_ENONASC));
        assert_eq!(a.add(b"\x80"), Err(LSS_ENONASC));
        assert_eq!(a.count(), 0);
    }

    #[test]
    fn rank_requires_sort() {
        let mut a = ord();
        assert_eq!(a.add(b"b"), Ok(1));
        assert_eq!(a.add(b"a"), Ok(2));
        assert_eq!(a.rank(b"a"), Err(LSS_EUNSORT));
        assert_eq!(a.sort(), Ok(1));
        assert_eq!(a.rank(b"a"), Ok(0));
        assert_eq!(a.rank(b"zz"), Err(LSS_ENOENT));
    }

    #[test]
    fn capacity_limit() {
        let mut a = ord();
        let mut i = 0usize;
        while i < CAP {
            let name: [u8; 2] = [b'n', b'0' + i as u8];
            assert_eq!(a.add(&name), Ok(i + 1));
            i += 1;
        }
        assert_eq!(a.add(b"zz"), Err(LSS_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
