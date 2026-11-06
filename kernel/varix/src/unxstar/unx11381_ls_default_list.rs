//! UNX-F11381 ls 默认列表语义（cwd 列举）
//!
//! 无操作数时列当前目录；目录操作数不打印 `dir:` 头、不递归，条目按字节序
//! 排、按终端宽度分列对齐；非目录操作数一条一行直列。默认列表的全部语义都
//! 落在这三条上：**有序入表**、**按宽分列**、**目录与非目录分流**。
//!
//! 闸门次序：
//! - `open`：宽度小于 `MIN_W` → 宽度大于 `MAX_W`；
//! - `add`：空名 → 名字超长 → 重名 → 名字不升序 → 表满；
//! - `layout`：空目录不排版 → 表内名字不升序（防御）→ 宽度容不下一列；
//! - `direct`：空名 → 名字超长 → 目录操作数不许直列。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xedd0;

/// 终端宽度越界。
pub const LSD_EWIDTH: u32 = SEG_BASE + 0x01;
/// 名字为空。
pub const LSD_ENAME: u32 = SEG_BASE + 0x02;
/// 名字超长。
pub const LSD_ELONG: u32 = SEG_BASE + 0x03;
/// 名字重复。
pub const LSD_EDUP: u32 = SEG_BASE + 0x04;
/// 名字不升序。
pub const LSD_EORDER: u32 = SEG_BASE + 0x05;
/// 条目表满。
pub const LSD_EFULL: u32 = SEG_BASE + 0x06;
/// 空目录不可排版。
pub const LSD_EMPTY: u32 = SEG_BASE + 0x07;
/// 目录操作数不许直列。
pub const LSD_EDIR: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 12;
/// 名字字节上限。
pub const NAME_MAX: usize = 10;
/// 终端宽度下限。
pub const MIN_W: u16 = 4;
/// 终端宽度上限。
pub const MAX_W: u16 = 200;
/// 列间空档字节数。
pub const GAP: usize = 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：入表。
pub const OP_ADD: u8 = 1;
/// 操作码：排版。
pub const OP_LAYOUT: u8 = 2;
/// 操作码：非目录直列。
pub const OP_DIRECT: u8 = 3;
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

/// 一个目录条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    name: [u8; NAME_MAX],
    nlen: u8,
    is_dir: bool,
}

impl Entry {
    /// 空白条目。
    pub const fn blank() -> Entry {
        Entry { name: [0; NAME_MAX], nlen: 0, is_dir: false }
    }

    /// 名字字节。
    pub fn bytes(&self) -> &[u8] {
        match self.name.get(..self.nlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 拷入名字，返回实际字节数。
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

/// 默认列表。
#[derive(Clone, Copy, Debug)]
pub struct Ls {
    width: u16,
    e: [Entry; CAP],
    n: usize,
}

impl Ls {
    /// 按终端宽度开户。
    pub fn open(width: u16) -> Result<Ls, u32> {
        if width < MIN_W || width > MAX_W {
            return Err(LSD_EWIDTH);
        }
        Ok(Ls { width, e: [Entry::blank(); CAP], n: 0 })
    }

    /// 终端宽度。
    pub fn width(&self) -> u16 {
        self.width
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 第 i 个条目。
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 第 i 个条目的名字。
    pub fn name_at(&self, i: usize) -> Option<&[u8]> {
        match self.e.get(i) {
            Some(v) if i < self.n => Some(v.bytes()),
            _ => None,
        }
    }

    /// 是否目录条目。
    pub fn dir_at(&self, i: usize) -> bool {
        match self.e.get(i) {
            Some(v) if i < self.n => v.is_dir,
            _ => false,
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

    /// 最后一个名字。
    fn last_name(&self) -> Option<&[u8]> {
        if self.n == 0 {
            return None;
        }
        match self.e.get(self.n - 1) {
            Some(v) => Some(v.bytes()),
            None => None,
        }
    }

    /// 入表一条，返回累计条目数。
    ///
    /// 闸门次序：空名 → 超长 → 重名 → 不升序 → 表满。
    pub fn add(&mut self, name: &[u8], is_dir: bool) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(LSD_ENAME);
        }
        if name.len() > NAME_MAX {
            return Err(LSD_ELONG);
        }
        if self.find(name).is_some() {
            return Err(LSD_EDUP);
        }
        if let Some(prev) = self.last_name() {
            if !less(prev, name) {
                return Err(LSD_EORDER);
            }
        }
        if self.n >= CAP {
            return Err(LSD_EFULL);
        }
        let slot = match self.e.get_mut(self.n) {
            Some(v) => v,
            None => return Err(LSD_EFULL),
        };
        slot.nlen = put(&mut slot.name, name);
        slot.is_dir = is_dir;
        self.n += 1;
        Ok(self.n)
    }

    /// 计算行数：按最宽名字分列，宽度至少容纳一列。
    ///
    /// 闸门次序：空目录 → 表内不升序（防御）→ 宽度容不下一列。
    pub fn layout(&self) -> Result<usize, u32> {
        if self.n == 0 {
            return Err(LSD_EMPTY);
        }
        let mut maxlen = 0usize;
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(v) => {
                    if v.nlen as usize > maxlen {
                        maxlen = v.nlen as usize;
                    }
                }
                None => return Err(LSD_EORDER),
            }
            i += 1;
        }
        let mut j = 1usize;
        while j < self.n {
            let a = match self.e.get(j - 1) {
                Some(v) => v.bytes(),
                None => return Err(LSD_EORDER),
            };
            let b = match self.e.get(j) {
                Some(v) => v.bytes(),
                None => return Err(LSD_EORDER),
            };
            if !less(a, b) {
                return Err(LSD_EORDER);
            }
            j += 1;
        }
        let cell = maxlen.saturating_add(GAP);
        let cols = (self.width as usize) / cell;
        if cols == 0 {
            return Err(LSD_EWIDTH);
        }
        let rows = self.n.saturating_add(cols - 1) / cols;
        Ok(rows)
    }

    /// 非目录操作数直列：一条一行，返回行数。
    ///
    /// 闸门次序：空名 → 超长 → 目录操作数。
    pub fn direct(&self, is_dir: bool, name_len: usize) -> Result<u32, u32> {
        if name_len == 0 {
            return Err(LSD_ENAME);
        }
        if name_len > NAME_MAX {
            return Err(LSD_ELONG);
        }
        if is_dir {
            return Err(LSD_EDIR);
        }
        Ok(1)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.width as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.e.get(i) {
                let mut k = 0usize;
                let b = v.bytes();
                while k < b.len() {
                    if let Some(x) = b.get(k) {
                        h = fold(h, *x as u64);
                    }
                    k += 1;
                }
                h = fold(h, 0xff);
                h = fold(h, v.is_dir as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.width < MIN_W || self.width > MAX_W || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let v = match self.e.get(i) {
                Some(e) => *e,
                None => return false,
            };
            if v.nlen == 0 || v.nlen as usize > NAME_MAX {
                return false;
            }
            let mut j = 0usize;
            while j < i {
                let p = match self.e.get(j) {
                    Some(e) => *e,
                    None => return false,
                };
                if same_bytes(p.bytes(), v.bytes()) {
                    return false;
                }
                j += 1;
            }
            if i > 0 {
                match self.e.get(i - 1) {
                    Some(p) => {
                        if !less(p.bytes(), v.bytes()) {
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
            return Err(LSD_ESLOT);
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
            return Err(LSD_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 1, 1, 6];
        const CODES: [u32; 6] =
            [LSD_ENAME, LSD_ELONG, LSD_EDUP, LSD_EORDER, LSD_EFULL, LSD_EMPTY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSD_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSD_EVALUE);
                        }
                    }
                    None => return Err(LSD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSD_EVALUE);
                        }
                    }
                    None => return Err(LSD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSD_EBAL);
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
            return Err(LSD_EBAL);
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
            Err(LSD_ESEAL)
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
    let mut a = match Ls::open(60) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_ADD, a.add(b"bin", true)));
    let _ = l.register(cell(1, OP_ADD, a.add(b"boot", true)));
    let _ = l.register(cell(2, OP_ADD, a.add(b"etc", true)));
    let _ = l.register(cell(3, OP_ADD, a.add(b"home", true)));
    let _ = l.register(cell(4, OP_ADD, a.add(b"usr", true)));
    let _ = l.register(cell(5, OP_LAYOUT, a.layout()));
    let _ = l.register(word_cell(6, OP_DIRECT, a.direct(false, 4)));
    let _ = l.register(cell(7, OP_ADD, a.add(b"var", true)));

    let _ = l.register(cell(8, OP_ADD, a.add(b"", true)));
    let _ = l.register(cell(9, OP_ADD, a.add(b"averylongname", false)));
    let _ = l.register(cell(10, OP_ADD, a.add(b"bin", true)));
    let _ = l.register(cell(11, OP_ADD, a.add(b"aaa", true)));

    let mut i = 0u32;
    while i < 6 {
        let src: [u8; 3] = [b'z', b'0', b'0' + i as u8];
        let _ = a.add(&src, false);
        i += 1;
    }
    let _ = l.register(cell(12, OP_ADD, a.add(b"zzz", false)));

    let empty = match Ls::open(60) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(cell(13, OP_LAYOUT, empty.layout()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：有序入表、分列、直列。
    let mut a = match Ls::open(80) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.add(b"bin", true);
    let r2 = a.add(b"boot", true);
    let r3 = a.add(b"etc", true);
    let lay = a.layout();
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && lay == Ok(1)
        && a.name_at(1) == Some(&b"boot"[..])
        && a.dir_at(1)
        && a.direct(false, 4) == Ok(1)
        && a.width() == 80
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut b = match Ls::open(40) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let bad_lo = Ls::open(3).is_err();
    let bad_hi = Ls::open(201).is_err();
    let empty_name = b.add(b"", false) == Err(LSD_ENAME);
    let long_name = b.add(b"01234567890", false) == Err(LSD_ELONG);
    let first = b.add(b"mm", false) == Ok(1);
    let dup = b.add(b"mm", false) == Err(LSD_EDUP);
    let order = b.add(b"aa", false) == Err(LSD_EORDER);
    let untouched = b.count() == 1;
    let dir_direct = b.direct(true, 2) == Err(LSD_EDIR);
    let empty_layout = match Ls::open(20) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let no_rows = empty_layout.layout() == Err(LSD_EMPTY);
    out[2] = bad_lo
        && bad_hi
        && empty_name
        && long_name
        && first
        && dup
        && order
        && untouched
        && dir_direct
        && no_rows
        && b.consistent();

    // 三路：表满与宽度极限。
    let mut q = match Ls::open(120) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0usize;
    while i < CAP {
        let src: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
        let _ = q.add(&src, false);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.add(b"zz", false) == Err(LSD_EFULL);
    let mut narrow = match Ls::open(MIN_W) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = narrow.add(b"abcd", false);
    let q3 = narrow.layout() == Err(LSD_EWIDTH);
    let mut wide = match Ls::open(14) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = wide.add(b"aa", false);
    let _ = wide.add(b"bb", false);
    let _ = wide.add(b"cc", false);
    let _ = wide.add(b"dd", false);
    let q4 = wide.layout() == Ok(2);
    out[3] = q1 && q2 && q3 && q4 && q.consistent() && narrow.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11381_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11381.ls_default_list");
    let l = build_ledger();

    s.add_dyn(
        "LSD1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSD2-名字升序入表",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4)
            && l.sample(4).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "LSD3-表格行数按宽",
        l.sample(5).map(|x| x.value) == Some(1) && l.sample(7).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "LSD4-空名被拒",
        {
            let mut a = match Ls::open(40) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"", false) == Err(LSD_ENAME)
                && a.count() == 0
                && a.consistent()
                && l.sample(8).map(|x| x.code) == Some(LSD_ENAME)
        },
        "static",
    );

    s.add(
        "LSD5-超长名被拒",
        {
            let mut a = match Ls::open(40) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"0123456789a", false) == Err(LSD_ELONG)
                && a.count() == 0
                && l.sample(9).map(|x| x.code) == Some(LSD_ELONG)
        },
        "static",
    );

    s.add(
        "LSD6-重名被拒",
        {
            let mut a = match Ls::open(40) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one = a.add(b"kk", false) == Ok(1);
            let dup = a.add(b"kk", false) == Err(LSD_EDUP);
            one && dup && a.count() == 1 && l.sample(10).map(|x| x.code) == Some(LSD_EDUP)
        },
        "static",
    );

    s.add(
        "LSD7-乱序入表被拒",
        {
            let mut a = match Ls::open(40) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one = a.add(b"nn", false) == Ok(1);
            let low = a.add(b"aa", false) == Err(LSD_EORDER);
            let two = a.add(b"zz", false) == Ok(2);
            one && low && two && a.consistent() && l.sample(11).map(|x| x.code) == Some(LSD_EORDER)
        },
        "static",
    );

    s.add(
        "LSD8-默认列数按宽度",
        {
            let mut a = match Ls::open(12) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"abc", false);
            let _ = a.add(b"de", false);
            let _ = a.add(b"fg", false);
            let _ = a.add(b"hi", false);
            let rows = a.layout();
            let mut b = match Ls::open(40) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = b.add(b"abc", false);
            let _ = b.add(b"de", false);
            let _ = b.add(b"fg", false);
            let _ = b.add(b"hi", false);
            rows == Ok(2) && b.layout() == Ok(1) && a.consistent()
        },
        "static",
    );

    s.add(
        "LSD9-表满被拒",
        {
            let mut a = match Ls::open(120) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut i = 0usize;
            while i < CAP {
                let src: [u8; 3] = [b'n', b'0' + (i / 10) as u8, b'0' + (i % 10) as u8];
                let _ = a.add(&src, false);
                i += 1;
            }
            a.count() == CAP
                && a.add(b"zz", false) == Err(LSD_EFULL)
                && a.consistent()
                && l.sample(12).map(|x| x.code) == Some(LSD_EFULL)
        },
        "static",
    );

    s.add(
        "LSD10-空目录不排版",
        {
            let a = match Ls::open(80) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.layout() == Err(LSD_EMPTY)
                && l.sample(13).map(|x| x.code) == Some(LSD_EMPTY)
        },
        "static",
    );

    s.add(
        "LSD11-非目录参数直列",
        {
            let a = match Ls::open(80) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.direct(false, 1) == Ok(1)
                && a.direct(false, NAME_MAX) == Ok(1)
                && a.direct(false, 0) == Err(LSD_ENAME)
                && a.count() == 0
        },
        "static",
    );

    s.add(
        "LSD12-目录参数不得直列",
        {
            let mut a = match Ls::open(80) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"etc", true);
            a.direct(true, 3) == Err(LSD_EDIR) && a.count() == 1 && a.consistent()
        },
        "static",
    );

    s.add(
        "LSD13-冻结重放四注入",
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

    fn ls(w: u16) -> Ls {
        match Ls::open(w) {
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
        let s = run_unx11381_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn byte_order_only() {
        let mut a = ls(80);
        assert_eq!(a.add(b"B", false), Ok(1));
        assert_eq!(a.add(b"a", false), Ok(2));
        assert_eq!(a.add(b"aa", false), Ok(3));
        assert_eq!(a.add(b"b", false), Ok(4));
        assert_eq!(a.name_at(0), Some(&b"B"[..]));
        assert!(a.consistent());
    }

    #[test]
    fn width_gate() {
        assert_eq!(Ls::open(3).is_err(), true);
        assert_eq!(Ls::open(201).is_err(), true);
        assert_eq!(ls(4).width(), 4);
    }

    #[test]
    fn layout_counts() {
        let mut a = ls(30);
        let _ = a.add(b"aaa", false);
        let _ = a.add(b"bbb", false);
        let _ = a.add(b"ccc", false);
        let _ = a.add(b"ddd", false);
        let _ = a.add(b"eee", false);
        assert_eq!(a.layout(), Ok(1));
        let mut b = ls(10);
        let _ = b.add(b"aaa", false);
        let _ = b.add(b"bbb", false);
        let _ = b.add(b"ccc", false);
        assert_eq!(b.layout(), Ok(2));
    }

    #[test]
    fn order_gate() {
        let mut a = ls(80);
        assert_eq!(a.add(b"mid", false), Ok(1));
        assert_eq!(a.add(b"low", false), Err(LSD_EORDER));
        assert_eq!(a.add(b"mid", false), Err(LSD_EDUP));
        assert_eq!(a.count(), 1);
    }

    #[test]
    fn direct_operand() {
        let a = ls(80);
        assert_eq!(a.direct(false, 5), Ok(1));
        assert_eq!(a.direct(true, 5), Err(LSD_EDIR));
        assert_eq!(a.direct(false, 0), Err(LSD_ENAME));
        assert_eq!(a.direct(false, NAME_MAX + 1), Err(LSD_ELONG));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
