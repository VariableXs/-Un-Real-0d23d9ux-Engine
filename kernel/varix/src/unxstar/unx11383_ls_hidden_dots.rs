//! UNX-F11383 ls -a/-A 隐藏项语义
//!
//! 三种呈现模式：默认（点项全隐）、`-a`（全显，含 `.` 与 `..`）、`-A`（点项
//! 显、`.` 与 `..` 不显）。差别只在**呈现**，不在入表：同一份目录内容在三种
//! 模式下入表的条目一样，可见序却不同。
//!
//! 闸门次序：
//! - `open`：模式不在 {默认, -a, -A}；
//! - `add`：空名 → 超长 → 保留名只在 `-a` 下可入表 → 重名 → 表满；
//! - `visible_count`：`-a` 缺 `.` 或 `..` → `-A` 混入 `.` 或 `..`；
//! - `nth_visible`：先过 `visible_count` 的两道点项闸门 → 序越界。
//!
//! 账本形状：14 格 = 8 允 + 6 拒，6 拒覆盖 6 个不同域错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xedf0;

/// 呈现模式非法。
pub const LSH_EMODE: u32 = SEG_BASE + 0x01;
/// 名字为空。
pub const LSH_ENAME: u32 = SEG_BASE + 0x02;
/// 名字超长。
pub const LSH_ELONG: u32 = SEG_BASE + 0x03;
/// 重名。
pub const LSH_EDUP: u32 = SEG_BASE + 0x04;
/// 保留名在非全显模式被拒。
pub const LSH_ERESV: u32 = SEG_BASE + 0x05;
/// 条目表满。
pub const LSH_EFULL: u32 = SEG_BASE + 0x06;
/// 点项不齐。
pub const LSH_EDOTS: u32 = SEG_BASE + 0x07;
/// 可见序越界。
pub const LSH_EGAP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LSH_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LSH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LSH_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LSH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LSH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 10;
/// 名字字节上限。
pub const NAME_MAX: usize = 12;
/// 默认模式：点项全隐。
pub const MODE_DEFAULT: u8 = 0;
/// `-a`：全显。
pub const MODE_ALL: u8 = 1;
/// `-A`：点项显、`.` 与 `..` 不显。
pub const MODE_ALMOST: u8 = 2;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：入表。
pub const OP_ADD: u8 = 1;
/// 操作码：可见计数。
pub const OP_COUNT: u8 = 2;
/// 操作码：第 k 个可见项。
pub const OP_NTH: u8 = 3;
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

/// 是否 `.` 或 `..`。
fn reserved(name: &[u8]) -> bool {
    same_bytes(name, b".") || same_bytes(name, b"..")
}

/// 是否点项。
fn dotted(name: &[u8]) -> bool {
    match name.first() {
        Some(x) => *x == b'.',
        None => false,
    }
}

/// 隐藏项呈现。
#[derive(Clone, Copy, Debug)]
pub struct Sel {
    mode: u8,
    e: [Entry; CAP],
    n: usize,
}

impl Sel {
    /// 按呈现模式开户。
    pub fn open(mode: u8) -> Result<Sel, u32> {
        if mode > MODE_ALMOST {
            return Err(LSH_EMODE);
        }
        Ok(Sel { mode, e: [Entry::blank(); CAP], n: 0 })
    }

    /// 呈现模式。
    pub fn mode(&self) -> u8 {
        self.mode
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
    /// 闸门次序：空名 → 超长 → 保留名（非 `-a`）→ 重名 → 表满。
    pub fn add(&mut self, name: &[u8], is_dir: bool) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(LSH_ENAME);
        }
        if name.len() > NAME_MAX {
            return Err(LSH_ELONG);
        }
        if reserved(name) && self.mode != MODE_ALL {
            return Err(LSH_ERESV);
        }
        if self.find(name).is_some() {
            return Err(LSH_EDUP);
        }
        if self.n >= CAP {
            return Err(LSH_EFULL);
        }
        let slot = match self.e.get_mut(self.n) {
            Some(v) => v,
            None => return Err(LSH_EFULL),
        };
        slot.nlen = put(&mut slot.name, name);
        slot.is_dir = is_dir;
        self.n += 1;
        Ok(self.n)
    }

    /// 单条是否呈现。
    pub fn visible(&self, name: &[u8]) -> bool {
        if reserved(name) {
            return self.mode == MODE_ALL;
        }
        if dotted(name) {
            return self.mode != MODE_DEFAULT;
        }
        true
    }

    /// 可见条目数。
    ///
    /// 闸门次序：`-a` 缺 `.` 或 `..` → `-A` 混入 `.` 或 `..`。
    pub fn visible_count(&self) -> Result<usize, u32> {
        let dot = self.find(b".").is_some();
        let dotdot = self.find(b"..").is_some();
        if self.mode == MODE_ALL && (!dot || !dotdot) {
            return Err(LSH_EDOTS);
        }
        if self.mode == MODE_ALMOST && (dot || dotdot) {
            return Err(LSH_EDOTS);
        }
        let mut c = 0usize;
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(v) => {
                    if self.visible(v.bytes()) {
                        c += 1;
                    }
                }
                None => return Err(LSH_EDOTS),
            }
            i += 1;
        }
        Ok(c)
    }

    /// 第 k 个可见条目的入表下标。
    ///
    /// 闸门次序：点项齐全（同 `visible_count`）→ k 越界。
    pub fn nth_visible(&self, k: usize) -> Result<u32, u32> {
        let total = self.visible_count()?;
        if k >= total {
            return Err(LSH_EGAP);
        }
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let v = match self.e.get(i) {
                Some(x) => *x,
                None => return Err(LSH_EGAP),
            };
            if self.visible(v.bytes()) {
                if seen == k {
                    return Ok(i as u32);
                }
                seen += 1;
            }
            i += 1;
        }
        Err(LSH_EGAP)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.mode as u64);
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
                h = fold(h, v.is_dir as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.mode > MODE_ALMOST || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let v = match self.e.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if v.nlen == 0 || v.nlen as usize > NAME_MAX {
                return false;
            }
            if self.mode != MODE_ALL && reserved(v.bytes()) {
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
            return Err(LSH_ESLOT);
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
            return Err(LSH_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 5, 1, 6];
        const CODES: [u32; 6] =
            [LSH_ENAME, LSH_ELONG, LSH_EDUP, LSH_EMODE, LSH_ERESV, LSH_EDOTS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LSH_EBAL),
            };
            if s.slot as usize != i {
                return Err(LSH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LSH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LSH_EVALUE);
                        }
                    }
                    None => return Err(LSH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LSH_EVALUE);
                        }
                    }
                    None => return Err(LSH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LSH_EBAL);
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
            return Err(LSH_EBAL);
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
            Err(LSH_ESEAL)
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
    let mut a = match Sel::open(MODE_ALL) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_ADD, a.add(b".", true)));
    let _ = l.register(cell(1, OP_ADD, a.add(b"..", true)));
    let _ = l.register(cell(2, OP_ADD, a.add(b".bashrc", false)));
    let _ = l.register(cell(3, OP_ADD, a.add(b".profile", false)));
    let _ = l.register(cell(4, OP_ADD, a.add(b"bin", true)));
    let _ = l.register(cell(5, OP_COUNT, a.visible_count()));
    let _ = l.register(word_cell(6, OP_NTH, a.nth_visible(1)));
    let _ = l.register(cell(7, OP_ADD, a.add(b"usr", true)));

    let _ = l.register(cell(8, OP_ADD, a.add(b"", false)));
    let _ = l.register(cell(9, OP_ADD, a.add(b".averylongname", false)));
    let _ = l.register(cell(10, OP_ADD, a.add(b"bin", true)));
    let _ = l.register(word_cell(
        11,
        OP_OPEN,
        Sel::open(3).map(|v| v.mode() as u32),
    ));

    let mut b = match Sel::open(MODE_DEFAULT) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(cell(12, OP_ADD, b.add(b".", true)));

    let mut e = match Sel::open(MODE_ALL) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = e.add(b"only", false);
    let _ = l.register(cell(13, OP_COUNT, e.visible_count()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：`-a` 全显、可见序与入表序一致。
    let mut a = match Sel::open(MODE_ALL) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r1 = a.add(b".", true);
    let r2 = a.add(b"..", true);
    let r3 = a.add(b".bashrc", false);
    let r4 = a.add(b"bin", true);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && r4 == Ok(4)
        && a.visible_count() == Ok(4)
        && a.nth_visible(3) == Ok(3)
        && a.visible(b".bashrc")
        && a.visible(b"bin")
        && a.mode() == MODE_ALL
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改状态。
    let mut d = match Sel::open(MODE_DEFAULT) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let empty = d.add(b"", false) == Err(LSH_ENAME);
    let long = d.add(b".abcdefghijklm", false) == Err(LSH_ELONG);
    let resv = d.add(b".", true) == Err(LSH_ERESV);
    let first = d.add(b".cfg", false) == Ok(1);
    let dup = d.add(b".cfg", false) == Err(LSH_EDUP);
    let untouched = d.count() == 1;
    let bad_mode = Sel::open(3).is_err();
    let mut f = match Sel::open(MODE_DEFAULT) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut i = 0u32;
    while (i as usize) < CAP {
        let name: [u8; 2] = [b'n', b'0' + i as u8];
        let _ = f.add(&name, false);
        i += 1;
    }
    let full = f.add(b"zz", false) == Err(LSH_EFULL);
    let mut g = match Sel::open(MODE_ALL) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = g.add(b"only", false);
    let dots = g.visible_count() == Err(LSH_EDOTS);
    let gap = g.nth_visible(0) == Err(LSH_EDOTS);
    out[2] = empty
        && long
        && resv
        && first
        && dup
        && untouched
        && bad_mode
        && full
        && dots
        && gap
        && d.consistent()
        && f.consistent();

    // 三路：点项齐全才许计数，可见序越界仍拒。
    let mut a = match Sel::open(MODE_ALL) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let q1 = a.visible_count() == Err(LSH_EDOTS);
    let _ = a.add(b".", true);
    let q2 = a.visible_count() == Err(LSH_EDOTS);
    let _ = a.add(b"..", true);
    let q3 = a.visible_count() == Ok(2);
    let q4 = a.nth_visible(2) == Err(LSH_EGAP);
    let mut h = match Sel::open(MODE_ALMOST) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = h.add(b".hidden", false);
    let _ = h.add(b"plain", false);
    let q5 = h.visible_count() == Ok(2);
    let q6 = h.nth_visible(0) == Ok(0);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && a.consistent() && h.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11383_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11383.ls_hidden_dots");
    let l = build_ledger();

    s.add_dyn(
        "LSH1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LSH2-全显模式条目全见",
        l.sample(4).map(|x| x.value) == Some(5) && l.sample(5).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "LSH3-可见序按入表序",
        l.sample(6).map(|x| x.value) == Some(1) && l.sample(7).map(|x| x.value) == Some(6),
        "static",
    );

    s.add(
        "LSH4-默认模式漏点",
        {
            let mut a = match Sel::open(MODE_DEFAULT) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b".cfg", false);
            let _ = a.add(b"bin", true);
            a.visible_count() == Ok(1)
                && a.nth_visible(0) == Ok(1)
                && !a.visible(b".cfg")
                && a.visible(b"bin")
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSH5-空名被拒",
        {
            let mut a = match Sel::open(MODE_ALL) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b"", false) == Err(LSH_ENAME)
                && a.count() == 0
                && l.sample(8).map(|x| x.code) == Some(LSH_ENAME)
        },
        "static",
    );

    s.add(
        "LSH6-超长名被拒",
        {
            let mut a = match Sel::open(MODE_ALL) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.add(b".averylongname", false) == Err(LSH_ELONG)
                && a.count() == 0
                && l.sample(9).map(|x| x.code) == Some(LSH_ELONG)
        },
        "static",
    );

    s.add(
        "LSH7-重名被拒",
        {
            let mut a = match Sel::open(MODE_ALL) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one = a.add(b"bin", true) == Ok(1);
            let dup = a.add(b"bin", true) == Err(LSH_EDUP);
            one && dup && a.count() == 1 && l.sample(10).map(|x| x.code) == Some(LSH_EDUP)
        },
        "static",
    );

    s.add(
        "LSH8-非法模式被拒",
        {
            let mut ok = Sel::open(3).is_err() && Sel::open(0).is_ok() && Sel::open(2).is_ok();
            if let Ok(v) = Sel::open(3) {
                ok = v.count() == 0;
            }
            ok && l.sample(11).map(|x| x.code) == Some(LSH_EMODE)
        },
        "static",
    );

    s.add(
        "LSH9-保留名被拒",
        {
            let mut d = match Sel::open(MODE_DEFAULT) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut h = match Sel::open(MODE_ALMOST) {
                Ok(v) => v,
                Err(_) => return s,
            };
            d.add(b".", true) == Err(LSH_ERESV)
                && d.add(b"..", true) == Err(LSH_ERESV)
                && h.add(b"..", true) == Err(LSH_ERESV)
                && d.count() == 0
                && h.count() == 0
                && l.sample(12).map(|x| x.code) == Some(LSH_ERESV)
        },
        "static",
    );

    s.add(
        "LSH10-点项不齐被拒",
        {
            let mut a = match Sel::open(MODE_ALL) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b"only", false);
            let early = a.visible_count() == Err(LSH_EDOTS);
            let _ = a.add(b".", true);
            let half = a.visible_count() == Err(LSH_EDOTS);
            let _ = a.add(b"..", true);
            early && half && a.visible_count() == Ok(3) && l.sample(13).map(|x| x.code) == Some(LSH_EDOTS)
        },
        "static",
    );

    s.add(
        "LSH11-可见序越界",
        {
            let mut a = match Sel::open(MODE_ALMOST) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b".cfg", false);
            let _ = a.add(b"bin", true);
            a.nth_visible(2) == Err(LSH_EGAP)
                && a.nth_visible(1) == Ok(1)
                && a.nth_visible(0) == Ok(0)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "LSH12-计数与可见序一致",
        {
            let mut a = match Sel::open(MODE_ALL) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = a.add(b".", true);
            let _ = a.add(b"..", true);
            let _ = a.add(b".local", false);
            let _ = a.add(b"bin", true);
            let _ = a.add(b"lib", true);
            let c = a.visible_count();
            let mut i = 0usize;
            let mut ok = c == Ok(5);
            while i < 5 {
                if a.nth_visible(i) != Ok(i as u32) {
                    ok = false;
                }
                i += 1;
            }
            ok && a.nth_visible(5) == Err(LSH_EGAP) && a.consistent()
        },
        "static",
    );

    s.add(
        "LSH13-冻结重放四注入",
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

    fn sel(mode: u8) -> Sel {
        match Sel::open(mode) {
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
        let s = run_unx11383_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn all_mode_shows_everything() {
        let mut a = sel(MODE_ALL);
        assert_eq!(a.add(b".", true), Ok(1));
        assert_eq!(a.add(b"..", true), Ok(2));
        assert_eq!(a.add(b".cfg", false), Ok(3));
        assert_eq!(a.add(b"bin", true), Ok(4));
        assert_eq!(a.visible_count(), Ok(4));
        assert!(a.visible(b"."));
        assert!(a.consistent());
    }

    #[test]
    fn almost_mode_hides_dots_only() {
        let mut a = sel(MODE_ALMOST);
        assert_eq!(a.add(b".", true), Err(LSH_ERESV));
        assert_eq!(a.add(b".cfg", false), Ok(1));
        assert_eq!(a.add(b"bin", true), Ok(2));
        assert_eq!(a.visible_count(), Ok(2));
        assert!(a.visible(b".cfg"));
        assert!(!a.visible(b"."));
    }

    #[test]
    fn default_mode_hides_all_dots() {
        let mut a = sel(MODE_DEFAULT);
        assert_eq!(a.add(b".", true), Err(LSH_ERESV));
        assert_eq!(a.add(b".cfg", false), Ok(1));
        assert_eq!(a.add(b"bin", true), Ok(2));
        assert_eq!(a.visible_count(), Ok(1));
        assert_eq!(a.nth_visible(0), Ok(1));
        assert_eq!(a.nth_visible(1), Err(LSH_EGAP));
    }

    #[test]
    fn dots_gate() {
        let mut a = sel(MODE_ALL);
        assert_eq!(a.add(b"only", false), Ok(1));
        assert_eq!(a.visible_count(), Err(LSH_EDOTS));
        assert_eq!(a.add(b".", true), Ok(2));
        assert_eq!(a.visible_count(), Err(LSH_EDOTS));
        assert_eq!(a.add(b"..", true), Ok(3));
        assert_eq!(a.visible_count(), Ok(3));
    }

    #[test]
    fn capacity_limit() {
        let mut a = sel(MODE_DEFAULT);
        let mut i = 0usize;
        while i < CAP {
            let name: [u8; 2] = [b'n', b'0' + i as u8];
            assert_eq!(a.add(&name, false), Ok(i + 1));
            i += 1;
        }
        assert_eq!(a.add(b"zz", false), Err(LSH_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
