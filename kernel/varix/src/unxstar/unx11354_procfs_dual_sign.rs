//! UNX-F11354 procfs 双线联签（0xec20 段）
//!
//! 同一个 procfs 快照会被两条互不相干的消费线读取：一条走 musl 的解析路径，
//! 一条走 glibc 的解析路径。两条线的实现细节完全不同，但它们读的是同一份内核
//! 事实，因此**联签**成立的条件是内容逐行一致，而不只是条数一致。
//!
//! 联签要回答三个层次的问题：
//!
//! 1. **同一时点**：两条线消费的是同一个快照版本。版本不同说明读取窗口跨越了
//!    一次更新，此时任何内容比较都没有意义（`ESTALE`）。
//! 2. **同一长度**：条数不同属于结构性分歧（`EMISMATCH`，错位点记为较短一侧的
//!    末尾）。
//! 3. **同一内容**：逐行比较键与值，第一处不同即为错位点（`EMISMATCH`）。
//!
//! 联签只允许发生一次，签完再签（`ERUN`）；任何一条线尚未消费就联签（`EREADY`）；
//! 两条线都是空的联签没有意义（`EEMPTY`）。
//!
//! 签完之后还可以**事后复核**：按各自保存的行重新计算内容指纹，与签入时保存的
//! 指纹比对，且重新拼出票号。任何一处对不上都说明签后被改过（`EMISMATCH`）。
//!
//! 闸门次序：
//! - `feed`：旁路非法 → 键为零 → 已消费 → 满员 → 键未升序；
//! - `consume`：旁路非法 → 已消费 → 版本为零；
//! - `cosign`：已联签 → 未就绪 → 空签 → 版本不一致 → 条数/内容。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec20;

/// 参数非法（旁路号、键为零、版本为零）。
pub const CS_EINVAL: u32 = SEG_BASE + 0x01;
/// 键未严格升序。
pub const CS_EORDER: u32 = SEG_BASE + 0x02;
/// 单条线满员。
pub const CS_EFULL: u32 = SEG_BASE + 0x03;
/// 已消费或已联签。
pub const CS_ERUN: u32 = SEG_BASE + 0x04;
/// 两条线快照版本不一致。
pub const CS_ESTALE: u32 = SEG_BASE + 0x05;
/// 内容不一致（含条数不一致）。
pub const CS_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 空签。
pub const CS_EEMPTY: u32 = SEG_BASE + 0x07;
/// 某条线尚未消费。
pub const CS_EREADY: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 复算值与记录不符。
pub const CS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 单条线容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 没有错位点。
pub const NO_GAP: u32 = 0xffff_ffff;

/// 旁路：musl 线。
pub const SIDE_MUSL: u8 = 1;
/// 旁路：glibc 线。
pub const SIDE_GLIBC: u8 = 2;

/// 操作码：喂一行。
pub const OP_FEED: u8 = 1;
/// 操作码：消费。
pub const OP_CONSUME: u8 = 2;
/// 操作码：联签。
pub const OP_COSIGN: u8 = 3;
/// 操作码：复核。
pub const OP_VERIFY: u8 = 4;

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

/// 一条消费线。
#[derive(Clone, Copy, Debug)]
pub struct Line {
    kind: u8,
    keys: [u32; CAP],
    vals: [u32; CAP],
    n: usize,
    ver: u32,
    digest: u64,
    consumed: bool,
}

impl Line {
    /// 新建一条线。
    pub const fn new(kind: u8) -> Line {
        Line {
            kind,
            keys: [0; CAP],
            vals: [0; CAP],
            n: 0,
            ver: 0,
            digest: 0,
            consumed: false,
        }
    }

    /// 线号。
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 快照版本。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 内容指纹。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 是否已消费。
    pub fn consumed(&self) -> bool {
        self.consumed
    }

    /// 第 i 行的键值。
    pub fn row_at(&self, i: usize) -> Option<(u32, u32)> {
        if i >= self.n {
            return None;
        }
        match (self.keys.get(i), self.vals.get(i)) {
            (Some(k), Some(v)) => Some((*k, *v)),
            _ => None,
        }
    }

    /// 最后一行键（空线为 0）。
    fn last_key(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        match self.keys.get(self.n - 1) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 喂一行。
    fn feed(&mut self, key: u32, val: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(CS_EINVAL);
        }
        if self.consumed {
            return Err(CS_ERUN);
        }
        if self.n >= CAP {
            return Err(CS_EFULL);
        }
        if key <= self.last_key() {
            return Err(CS_EORDER);
        }
        if let Some(slot) = self.keys.get_mut(self.n) {
            *slot = key;
        }
        if let Some(slot) = self.vals.get_mut(self.n) {
            *slot = val;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 消费：把行折成内容指纹。
    fn consume(&mut self, ver: u32) -> Result<usize, u32> {
        if self.consumed {
            return Err(CS_ERUN);
        }
        if ver == 0 {
            return Err(CS_EINVAL);
        }
        self.digest = self.compute();
        self.ver = ver;
        self.consumed = true;
        Ok(self.n)
    }

    /// 按当前行重算内容指纹（不含线号：线号只进票号）。
    fn compute(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let (Some(k), Some(v)) = (self.keys.get(i), self.vals.get(i)) {
                h = fold(h, *k as u64);
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        fold(h, self.n as u64)
    }

    /// 键是否严格升序。
    fn ordered(&self) -> bool {
        let mut i = 1usize;
        while i < self.n {
            if let (Some(a), Some(b)) = (self.keys.get(i - 1), self.keys.get(i)) {
                if *a >= *b {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 拼接票号（含两侧内容与线号）。
    fn ticket(&self, other: &Line) -> u64 {
        let mut h = fold(FNV_OFF, self.kind as u64);
        h = fold(h, other.kind as u64);
        h = fold(h, self.digest);
        h = fold(h, other.digest);
        fold(h, self.n as u64)
    }
}

/// 双线联签器。
#[derive(Clone, Copy, Debug)]
pub struct CoSign {
    a: Line,
    b: Line,
    signed: bool,
    ticket: u64,
    gap: u32,
}

impl CoSign {
    /// 新建联签器（musl 线与 glibc 线）。
    pub const fn new() -> CoSign {
        CoSign { a: Line::new(SIDE_MUSL), b: Line::new(SIDE_GLIBC), signed: false, ticket: 0, gap: NO_GAP }
    }

    /// 取一条线。
    fn line(&self, side: u8) -> Result<&Line, u32> {
        match side {
            SIDE_MUSL => Ok(&self.a),
            SIDE_GLIBC => Ok(&self.b),
            _ => Err(CS_EINVAL),
        }
    }

    /// 取一条线的可变引用。
    fn line_mut(&mut self, side: u8) -> Result<&mut Line, u32> {
        match side {
            SIDE_MUSL => Ok(&mut self.a),
            SIDE_GLIBC => Ok(&mut self.b),
            _ => Err(CS_EINVAL),
        }
    }

    /// 喂一行。
    pub fn feed(&mut self, side: u8, key: u32, val: u32) -> Result<usize, u32> {
        match self.line_mut(side) {
            Ok(l) => l.feed(key, val),
            Err(c) => Err(c),
        }
    }

    /// 消费一条线。
    pub fn consume(&mut self, side: u8, ver: u32) -> Result<usize, u32> {
        match self.line_mut(side) {
            Ok(l) => l.consume(ver),
            Err(c) => Err(c),
        }
    }

    /// 行数。
    pub fn line_len(&self, side: u8) -> Result<usize, u32> {
        match self.line(side) {
            Ok(l) => Ok(l.len()),
            Err(c) => Err(c),
        }
    }

    /// 第 i 行。
    pub fn row_at(&self, side: u8, i: usize) -> Result<(u32, u32), u32> {
        let l = match self.line(side) {
            Ok(v) => v,
            Err(c) => return Err(c),
        };
        match l.row_at(i) {
            Some(p) => Ok(p),
            None => Err(CS_ENOBS),
        }
    }

    /// 内容指纹。
    pub fn digest_of(&self, side: u8) -> Result<u64, u32> {
        match self.line(side) {
            Ok(l) => Ok(l.digest),
            Err(c) => Err(c),
        }
    }

    /// 是否已联签。
    pub fn signed(&self) -> bool {
        self.signed
    }

    /// 票号。
    pub fn ticket(&self) -> u64 {
        self.ticket
    }

    /// 错位点（无错位为 `NO_GAP`）。
    pub fn gap(&self) -> u32 {
        self.gap
    }

    /// 内容是否一致（仅联签后有意义）。
    pub fn equal(&self) -> bool {
        self.signed && self.a.digest == self.b.digest
    }

    /// 联签。
    pub fn cosign(&mut self) -> Result<usize, u32> {
        if self.signed {
            return Err(CS_ERUN);
        }
        if !self.a.consumed || !self.b.consumed {
            return Err(CS_EREADY);
        }
        if self.a.n == 0 || self.b.n == 0 {
            return Err(CS_EEMPTY);
        }
        if self.a.ver != self.b.ver {
            return Err(CS_ESTALE);
        }
        if self.a.n != self.b.n {
            self.gap = if self.a.n < self.b.n { self.a.n } else { self.b.n } as u32;
            return Err(CS_EMISMATCH);
        }
        let mut i = 0usize;
        while i < self.a.n {
            let ka = self.a.keys.get(i).copied();
            let kb = self.b.keys.get(i).copied();
            let va = self.a.vals.get(i).copied();
            let vb = self.b.vals.get(i).copied();
            if ka != kb || va != vb {
                self.gap = i as u32;
                return Err(CS_EMISMATCH);
            }
            i += 1;
        }
        self.gap = NO_GAP;
        self.ticket = self.a.ticket(&self.b);
        self.signed = true;
        Ok(self.a.n)
    }

    /// 事后复核：重算两侧指纹与票号。
    pub fn verify(&self) -> Result<usize, u32> {
        if !self.signed {
            return Err(CS_EREADY);
        }
        if self.a.compute() != self.a.digest || self.b.compute() != self.b.digest {
            return Err(CS_EMISMATCH);
        }
        let want = self.a.ticket(&self.b);
        if want != self.ticket {
            return Err(CS_EMISMATCH);
        }
        Ok(self.a.n)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.a.n > CAP || self.b.n > CAP {
            return false;
        }
        if !self.a.ordered() || !self.b.ordered() {
            return false;
        }
        if self.a.consumed != self.b.consumed {
            return true;
        }
        if self.signed {
            if !self.a.consumed || !self.b.consumed {
                return false;
            }
            if self.a.n != self.b.n || self.a.digest != self.b.digest {
                return false;
            }
            if self.a.ver != self.b.ver {
                return false;
            }
            if self.a.ticket(&self.b) != self.ticket {
                return false;
            }
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
            return Err(CS_ESLOT);
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
            return Err(CS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 2, 2, 2, 2];
        const CODES: [u32; 6] =
            [CS_EINVAL, CS_EORDER, CS_EFULL, CS_ERUN, CS_ESTALE, CS_EMISMATCH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CS_EBAL),
            };
            if s.slot as usize != i {
                return Err(CS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CS_EVALUE);
                        }
                    }
                    None => return Err(CS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CS_EVALUE);
                        }
                    }
                    None => return Err(CS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CS_EBAL);
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
            return Err(CS_EBAL);
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
            Err(CS_ESEAL)
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

/// 十四格账本：双线同源 → 各自消费 → 联签 → 复核 → 各类分歧。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = CoSign::new();

    let _ = l.register(cell(0, OP_FEED, c.feed(SIDE_MUSL, 1, 10)));
    let _ = l.register(cell(1, OP_FEED, c.feed(SIDE_MUSL, 2, 20)));
    let _ = l.register(cell(2, OP_FEED, c.feed(SIDE_GLIBC, 1, 10)));
    let _ = l.register(cell(3, OP_FEED, c.feed(SIDE_GLIBC, 2, 20)));
    let _ = l.register(cell(4, OP_CONSUME, c.consume(SIDE_MUSL, 5)));
    let _ = l.register(cell(5, OP_CONSUME, c.consume(SIDE_GLIBC, 5)));
    let _ = l.register(cell(6, OP_COSIGN, c.cosign()));
    let _ = l.register(cell(7, OP_VERIFY, c.verify()));

    let mut e = CoSign::new();
    let _ = l.register(cell(8, OP_FEED, e.feed(3, 1, 10)));

    let mut o = CoSign::new();
    let _ = o.feed(SIDE_GLIBC, 1, 10);
    let _ = o.feed(SIDE_GLIBC, 2, 20);
    let _ = l.register(cell(9, OP_FEED, o.feed(SIDE_GLIBC, 2, 30)));

    let mut f = CoSign::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = f.feed(SIDE_MUSL, i, i);
        i += 1;
    }
    let _ = l.register(cell(10, OP_FEED, f.feed(SIDE_MUSL, 99, 99)));

    let mut r = CoSign::new();
    let _ = r.feed(SIDE_MUSL, 1, 10);
    let _ = r.consume(SIDE_MUSL, 1);
    let _ = l.register(cell(11, OP_CONSUME, r.consume(SIDE_MUSL, 1)));

    let mut s = CoSign::new();
    let _ = s.feed(SIDE_MUSL, 1, 10);
    let _ = s.feed(SIDE_GLIBC, 1, 10);
    let _ = s.consume(SIDE_MUSL, 1);
    let _ = s.consume(SIDE_GLIBC, 2);
    let _ = l.register(cell(12, OP_COSIGN, s.cosign()));

    let mut m = CoSign::new();
    let _ = m.feed(SIDE_MUSL, 1, 10);
    let _ = m.feed(SIDE_GLIBC, 1, 11);
    let _ = m.consume(SIDE_MUSL, 1);
    let _ = m.consume(SIDE_GLIBC, 1);
    let _ = l.register(cell(13, OP_COSIGN, m.cosign()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：双线同源，签完可复核。
    let mut a = CoSign::new();
    let mut k = 1u32;
    while k <= 3 {
        let _ = a.feed(SIDE_MUSL, k, k * 10);
        let _ = a.feed(SIDE_GLIBC, k, k * 10);
        k += 1;
    }
    let ca = a.consume(SIDE_MUSL, 7);
    let cb = a.consume(SIDE_GLIBC, 7);
    let sg = a.cosign();
    let vf = a.verify();
    out[1] = ca == Ok(3)
        && cb == Ok(3)
        && sg == Ok(3)
        && vf == Ok(3)
        && a.signed()
        && a.equal()
        && a.ticket() != 0
        && a.gap() == NO_GAP
        && a.row_at(SIDE_MUSL, 2) == Ok((3, 30))
        && a.row_at(SIDE_MUSL, 3) == Err(CS_ENOBS)
        && a.consistent();

    // 二路：四类分歧各自可辨。
    let mut stale = CoSign::new();
    let _ = stale.feed(SIDE_MUSL, 1, 10);
    let _ = stale.feed(SIDE_GLIBC, 1, 10);
    let _ = stale.consume(SIDE_MUSL, 1);
    let _ = stale.consume(SIDE_GLIBC, 2);
    let st = stale.cosign() == Err(CS_ESTALE);

    let mut short = CoSign::new();
    let _ = short.feed(SIDE_MUSL, 1, 10);
    let _ = short.feed(SIDE_MUSL, 2, 20);
    let _ = short.feed(SIDE_GLIBC, 1, 10);
    let _ = short.consume(SIDE_MUSL, 1);
    let _ = short.consume(SIDE_GLIBC, 1);
    let sh = short.cosign() == Err(CS_EMISMATCH) && short.gap() == 1;

    let mut diff = CoSign::new();
    let _ = diff.feed(SIDE_MUSL, 1, 10);
    let _ = diff.feed(SIDE_MUSL, 2, 20);
    let _ = diff.feed(SIDE_GLIBC, 1, 10);
    let _ = diff.feed(SIDE_GLIBC, 2, 99);
    let _ = diff.consume(SIDE_MUSL, 1);
    let _ = diff.consume(SIDE_GLIBC, 1);
    let df = diff.cosign() == Err(CS_EMISMATCH) && diff.gap() == 1 && !diff.signed();

    let mut notyet = CoSign::new();
    let _ = notyet.feed(SIDE_MUSL, 1, 10);
    let _ = notyet.feed(SIDE_GLIBC, 1, 10);
    let _ = notyet.consume(SIDE_MUSL, 1);
    let ny = notyet.cosign() == Err(CS_EREADY);

    out[2] = st && sh && df && ny;

    // 三路：闸门与容量。
    let mut q = CoSign::new();
    let q1 = q.feed(0, 1, 1) == Err(CS_EINVAL);
    let q2 = q.feed(SIDE_MUSL, 0, 1) == Err(CS_EINVAL);
    let q3 = q.feed(SIDE_MUSL, 1, 1) == Ok(1);
    let q4 = q.feed(SIDE_MUSL, 1, 2) == Err(CS_EORDER);
    let q5 = q.feed(SIDE_MUSL, 2, 2) == Ok(2);
    let q6 = q.consume(SIDE_MUSL, 0) == Err(CS_EINVAL);
    let q7 = q.consume(SIDE_MUSL, 1) == Ok(2);
    let q8 = q.feed(SIDE_MUSL, 3, 3) == Err(CS_ERUN);
    let mut empty = CoSign::new();
    let _ = empty.consume(SIDE_MUSL, 1);
    let _ = empty.consume(SIDE_GLIBC, 1);
    let q9 = empty.cosign() == Err(CS_EEMPTY);
    let mut full = CoSign::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.feed(SIDE_GLIBC, i, i);
        i += 1;
    }
    let q10 = full.feed(SIDE_GLIBC, 99, 99) == Err(CS_EFULL);
    let mut twice = CoSign::new();
    let _ = twice.feed(SIDE_MUSL, 1, 1);
    let _ = twice.feed(SIDE_GLIBC, 1, 1);
    let _ = twice.consume(SIDE_MUSL, 1);
    let _ = twice.consume(SIDE_GLIBC, 1);
    let _ = twice.cosign();
    let q11 = twice.cosign() == Err(CS_ERUN);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q7 && q8 && q9 && q10 && q11 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11354_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11354.procfs_dual_sign");
    let l = build_ledger();

    s.add_dyn(
        "CS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CS2-双线逐行同源",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(1)
            && l.sample(3).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CS3-双线各消费两行",
        l.sample(4).map(|x| x.value) == Some(2) && l.sample(5).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CS4-联签两行",
        l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CS5-事后复核通过",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CS6-旁路号非法",
        l.sample(8).map(|x| x.code) == Some(CS_EINVAL),
        "static",
    );

    s.add(
        "CS7-键乱序被拒",
        l.sample(9).map(|x| x.code) == Some(CS_EORDER),
        "static",
    );

    s.add(
        "CS8-满员被拒",
        l.sample(10).map(|x| x.code) == Some(CS_EFULL),
        "static",
    );

    s.add(
        "CS9-重复消费被拒",
        l.sample(11).map(|x| x.code) == Some(CS_ERUN),
        "static",
    );

    s.add(
        "CS10-版本落后为STALE",
        l.sample(12).map(|x| x.code) == Some(CS_ESTALE),
        "static",
    );

    s.add(
        "CS11-同长异值为MISMATCH",
        l.sample(13).map(|x| x.code) == Some(CS_EMISMATCH),
        "static",
    );

    s.add(
        "CS12-错位点可定位且可签",
        {
            let mut a = CoSign::new();
            let _ = a.feed(SIDE_MUSL, 1, 10);
            let _ = a.feed(SIDE_MUSL, 2, 20);
            let _ = a.feed(SIDE_MUSL, 3, 30);
            let _ = a.feed(SIDE_GLIBC, 1, 10);
            let _ = a.feed(SIDE_GLIBC, 2, 99);
            let _ = a.feed(SIDE_GLIBC, 3, 30);
            let _ = a.consume(SIDE_MUSL, 4);
            let _ = a.consume(SIDE_GLIBC, 4);
            let bad = a.cosign() == Err(CS_EMISMATCH) && a.gap() == 1 && !a.signed();
            let mut b = CoSign::new();
            let _ = b.feed(SIDE_MUSL, 1, 10);
            let _ = b.feed(SIDE_MUSL, 2, 20);
            let _ = b.feed(SIDE_MUSL, 3, 30);
            let _ = b.feed(SIDE_GLIBC, 1, 10);
            let _ = b.feed(SIDE_GLIBC, 2, 20);
            let _ = b.feed(SIDE_GLIBC, 3, 30);
            let _ = b.consume(SIDE_MUSL, 4);
            let _ = b.consume(SIDE_GLIBC, 4);
            let good = b.cosign() == Ok(3) && b.verify() == Ok(3) && b.equal() && b.ticket() != 0;
            bad && good && b.consistent()
        },
        "static",
    );

    s.add(
        "CS13-冻结重放四注入",
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
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11354_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn identical_lines_sign() {
        let mut c = CoSign::new();
        assert_eq!(c.feed(SIDE_MUSL, 1, 7), Ok(1));
        assert_eq!(c.feed(SIDE_MUSL, 2, 8), Ok(2));
        assert_eq!(c.feed(SIDE_GLIBC, 1, 7), Ok(1));
        assert_eq!(c.feed(SIDE_GLIBC, 2, 8), Ok(2));
        assert_eq!(c.consume(SIDE_MUSL, 3), Ok(2));
        assert_eq!(c.consume(SIDE_GLIBC, 3), Ok(2));
        assert_eq!(c.cosign(), Ok(2));
        assert!(c.signed());
        assert!(c.equal());
        assert_ne!(c.ticket(), 0);
        assert_eq!(c.verify(), Ok(2));
        assert_eq!(c.digest_of(SIDE_MUSL), c.digest_of(SIDE_GLIBC));
    }

    #[test]
    fn stale_beats_content() {
        let mut c = CoSign::new();
        let _ = c.feed(SIDE_MUSL, 1, 1);
        let _ = c.feed(SIDE_GLIBC, 1, 2);
        let _ = c.consume(SIDE_MUSL, 1);
        let _ = c.consume(SIDE_GLIBC, 2);
        assert_eq!(c.cosign(), Err(CS_ESTALE));
        assert!(!c.signed());
    }

    #[test]
    fn gap_points_at_first_difference() {
        let mut c = CoSign::new();
        let _ = c.feed(SIDE_MUSL, 1, 5);
        let _ = c.feed(SIDE_MUSL, 2, 6);
        let _ = c.feed(SIDE_MUSL, 3, 7);
        let _ = c.feed(SIDE_GLIBC, 1, 5);
        let _ = c.feed(SIDE_GLIBC, 2, 6);
        let _ = c.feed(SIDE_GLIBC, 3, 8);
        let _ = c.consume(SIDE_MUSL, 1);
        let _ = c.consume(SIDE_GLIBC, 1);
        assert_eq!(c.cosign(), Err(CS_EMISMATCH));
        assert_eq!(c.gap(), 2);
    }

    #[test]
    fn length_difference_gap_is_shorter() {
        let mut c = CoSign::new();
        let _ = c.feed(SIDE_MUSL, 1, 1);
        let _ = c.feed(SIDE_GLIBC, 1, 1);
        let _ = c.feed(SIDE_GLIBC, 2, 2);
        let _ = c.consume(SIDE_MUSL, 1);
        let _ = c.consume(SIDE_GLIBC, 1);
        assert_eq!(c.cosign(), Err(CS_EMISMATCH));
        assert_eq!(c.gap(), 1);
    }

    #[test]
    fn ready_and_empty() {
        let mut c = CoSign::new();
        assert_eq!(c.cosign(), Err(CS_EREADY));
        let _ = c.consume(SIDE_MUSL, 1);
        let _ = c.consume(SIDE_GLIBC, 1);
        assert_eq!(c.cosign(), Err(CS_EEMPTY));
        assert!(c.consistent());
    }

    #[test]
    fn gates_and_capacity() {
        let mut c = CoSign::new();
        assert_eq!(c.feed(0, 1, 1), Err(CS_EINVAL));
        assert_eq!(c.feed(SIDE_MUSL, 0, 1), Err(CS_EINVAL));
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(c.feed(SIDE_MUSL, i, i), Ok(i as usize));
            i += 1;
        }
        assert_eq!(c.feed(SIDE_MUSL, 99, 1), Err(CS_EFULL));
        assert_eq!(c.consume(SIDE_MUSL, 0), Err(CS_EINVAL));
        assert_eq!(c.consume(SIDE_MUSL, 1), Ok(CAP));
        assert_eq!(c.consume(SIDE_MUSL, 1), Err(CS_ERUN));
        assert!(c.consistent());

        let mut o = CoSign::new();
        assert_eq!(o.feed(SIDE_GLIBC, 5, 1), Ok(1));
        assert_eq!(o.feed(SIDE_GLIBC, 5, 1), Err(CS_EORDER));
        assert_eq!(o.feed(SIDE_GLIBC, 4, 1), Err(CS_EORDER));
    }
}
