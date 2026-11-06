//! UNX-F11401 cat 基本拼接输出语义（0xef10 段）
//!
//! `cat a b c` 的语义朴素到容易被忽略：**按参数序逐件读出，首尾相接，不插
//! 分隔符、不补换行**。三处细节决定了它与基准机是否逐字节一致：
//!
//! - 拼接次序就是 argv 次序，不是目录序、不是登记序。同一个文件可以在参数里
//!   出现两次，内容就应当出现两次；
//! - 每个文件读到 EOF 后立刻切下一个，**不在文件边界插入任何字节**。因此若
//!   前一件末尾没有换行，下一件的首行会与它黏在同一行上；
//! - 输出缓冲是有限的。一旦写不下，`cat` 已经写出的部分保留在输出里——这与
//!   「先算总长再输出」的实现不同，也正是「部分输出 + `ENOSPC`」的由来。
//!
//! 本模块把「源」抽象成登记表：文件、目录、标准输入三种。目录不是可读源，
//! 读到目录判 `EISDIR`；被占用的源判 `EBUSY`；被标记读错的源判 `EIO`；
//! 登记表满判 `EFULL`；输出缓冲写满判 `ENOSPC`。这些错误类不能合并：
//! `EISDIR` 要换源，`EBUSY` 要等占用解除，`EIO` 要查介质，`ENOSPC` 要清缓冲，
//! `EFULL` 要停手不再登记。`ENOENT` 则是源压根不在册，和「在册但不可读」是
//! 两回事。
//!
//! 闸门次序（`feed`）：编号 → 在册 → 目录 → 占用 → 读错 → 缓冲容量。次序
//! 即判据：编号为 0 且不存在必须报 `EINVAL`；目录且被占用必须报 `EISDIR`
//! （先判断它根本不可读）；占用且读错必须报 `EBUSY`（先解除占用再谈介质）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef10;

/// 形状错：编号为 0、种类越界、参数个数越界。
pub const CT_EINVAL: u32 = SEG_BASE + 0x01;
/// 源不在册。
pub const CT_ENOENT: u32 = SEG_BASE + 0x02;
/// 源是目录，不可读。
pub const CT_EISDIR: u32 = SEG_BASE + 0x03;
/// 源读出错。
pub const CT_EIO: u32 = SEG_BASE + 0x04;
/// 输出缓冲写满。
pub const CT_ENOSPC: u32 = SEG_BASE + 0x05;
/// 源登记表满。
pub const CT_EFULL: u32 = SEG_BASE + 0x06;
/// 源被占用。
pub const CT_EBUSY: u32 = SEG_BASE + 0x07;
/// 源编号重复登记。
pub const CT_EPERM: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 一次最多接受的参数件数。
pub const ACAP: usize = 4;
/// 源登记表容量（即 `CAP`）。
pub const CAP: usize = 6;
/// 单件内容上限。
pub const MAXLEN: usize = 24;
/// 输出缓冲容量。
pub const OCAP: usize = 128;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 种类：普通文件。
pub const KFILE: u8 = 0;
/// 种类：目录。
pub const KDIR: u8 = 1;
/// 种类：标准输入。
pub const KSTDIN: u8 = 2;

/// 操作码：登记源。
pub const OP_ADD: u8 = 1;
/// 操作码：读一件。
pub const OP_FEED: u8 = 2;
/// 操作码：按参数序播放。
pub const OP_PLAY: u8 = 3;
/// 操作码：读累计读出件数。
pub const OP_STAT: u8 = 4;
/// 操作码：清输出。
pub const OP_CLEAR: u8 = 5;

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

/// 一个可读源。
#[derive(Clone, Copy, Debug)]
pub struct Src {
    /// 编号。
    pub id: u32,
    /// 种类。
    pub kind: u8,
    /// 内容。
    pub b: [u8; MAXLEN],
    /// 内容长度。
    pub n: usize,
    /// 是否在册。
    pub live: bool,
    /// 是否被占用。
    pub busy: bool,
    /// 是否读错。
    pub poisoned: bool,
}

impl Src {
    /// 空槽。
    pub const fn blank() -> Src {
        Src { id: 0, kind: 0, b: [0u8; MAXLEN], n: 0, live: false, busy: false, poisoned: false }
    }

    /// 内容是否等于字面量。
    pub fn eq_str(&self, s: &str) -> bool {
        let rhs = s.as_bytes();
        if self.n != rhs.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match rhs.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 一次 cat 会话：源表 + 输出缓冲。
#[derive(Clone, Copy, Debug)]
pub struct Cat {
    src: [Src; CAP],
    n: usize,
    out: [u8; OCAP],
    outn: usize,
    feeds: u32,
    rejects: u32,
}

impl Cat {
    /// 空会话。
    pub const fn new() -> Cat {
        Cat {
            src: [Src::blank(); CAP],
            n: 0,
            out: [0u8; OCAP],
            outn: 0,
            feeds: 0,
            rejects: 0,
        }
    }

    /// 在册源数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否没有源。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 输出长度。
    pub fn out_len(&self) -> usize {
        self.outn
    }

    /// 累计读出件数。
    pub fn feeds(&self) -> u32 {
        self.feeds
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 按编号找源。
    pub fn find(&self, id: u32) -> Option<Src> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.src.get(i) {
                if s.live && s.id == id {
                    return Some(*s);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一个源。闸门：编号形状 → 种类 → 重复 → 容量。
    pub fn add(&mut self, id: u32, kind: u8, b: &str) -> Result<usize, u32> {
        if id == 0 || kind > KSTDIN || b.len() > MAXLEN {
            return Err(CT_EINVAL);
        }
        if self.find(id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EPERM);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EFULL);
        }
        let mut x = Src::blank();
        x.id = id;
        x.kind = kind;
        x.live = true;
        let bytes = b.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes.get(i) {
                Some(c) => {
                    if let Some(d) = x.b.get_mut(i) {
                        *d = *c;
                    }
                }
                None => break,
            }
            i += 1;
        }
        x.n = bytes.len();
        if let Some(dst) = self.src.get_mut(self.n) {
            *dst = x;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 标记占用。
    pub fn mark_busy(&mut self, id: u32) -> Result<usize, u32> {
        let i = self.index_of(id)?;
        let mut x = match self.src.get(i) {
            Some(v) => *v,
            None => return Err(CT_ENOENT),
        };
        x.busy = true;
        if let Some(dst) = self.src.get_mut(i) {
            *dst = x;
        }
        Ok(self.n)
    }

    /// 标记读错。
    pub fn poison(&mut self, id: u32) -> Result<usize, u32> {
        let i = self.index_of(id)?;
        let mut x = match self.src.get(i) {
            Some(v) => *v,
            None => return Err(CT_ENOENT),
        };
        x.poisoned = true;
        if let Some(dst) = self.src.get_mut(i) {
            *dst = x;
        }
        Ok(self.n)
    }

    fn index_of(&self, id: u32) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.src.get(i) {
                if s.live && s.id == id {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(CT_ENOENT)
    }

    fn push_byte(&mut self, c: u8) -> Result<(), u32> {
        if self.outn >= OCAP {
            return Err(CT_ENOSPC);
        }
        if let Some(d) = self.out.get_mut(self.outn) {
            *d = c;
        }
        self.outn += 1;
        Ok(())
    }

    fn push_src(&mut self, s: &Src) -> Result<(), u32> {
        let mut i = 0usize;
        while i < s.n {
            let c = match s.b.get(i) {
                Some(v) => *v,
                None => break,
            };
            self.push_byte(c)?;
            i += 1;
        }
        Ok(())
    }

    /// 清空输出缓冲，源表与计数保留。
    pub fn clear(&mut self) -> usize {
        let mut i = 0usize;
        while i < self.outn {
            if let Some(d) = self.out.get_mut(i) {
                *d = 0;
            }
            i += 1;
        }
        self.outn = 0;
        self.outn
    }

    /// 读一件。
    ///
    /// 闸门：编号 → 在册 → 目录 → 占用 → 读错 → 缓冲容量。已写出的部分在
    /// 失败时保留，这是 cat 的真实行为。
    pub fn feed(&mut self, id: u32) -> Result<usize, u32> {
        if id == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EINVAL);
        }
        let s = match self.find(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CT_ENOENT);
            }
        };
        if s.kind == KDIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EISDIR);
        }
        if s.busy {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EBUSY);
        }
        if s.poisoned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EIO);
        }
        let r = self.push_src(&s);
        if r.is_err() {
            self.rejects = self.rejects.saturating_add(1);
            return r.map(|_| self.outn);
        }
        self.feeds = self.feeds.saturating_add(1);
        Ok(self.outn)
    }

    /// 按参数序播放：先清空输出，再逐件读出。
    pub fn play(&mut self, ids: [u32; ACAP], count: usize) -> Result<usize, u32> {
        if count == 0 || count > ACAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EINVAL);
        }
        let _ = self.clear();
        let mut i = 0usize;
        while i < count {
            let id = match ids.get(i) {
                Some(v) => *v,
                None => return Err(CT_EINVAL),
            };
            let _ = self.feed(id)?;
            i += 1;
        }
        Ok(self.outn)
    }

    /// 输出缓冲是否与字面量逐字节相同。
    pub fn out_eq(&self, s: &str) -> bool {
        let rhs = s.as_bytes();
        if self.outn != rhs.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.outn {
            let a = match self.out.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match rhs.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != c {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 输出指纹。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.outn {
            if let Some(c) = self.out.get(i) {
                h = fold(h, *c as u64);
            }
            i += 1;
        }
        h
    }

    /// 不变式：源数不越容量、编号非零且互不相同、输出长度不越缓冲。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.outn > OCAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.src.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !a.live || a.id == 0 || a.n > MAXLEN || a.kind > KSTDIN {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                if let Some(b) = self.src.get(j) {
                    if b.live && b.id == a.id {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 基准机输出：三件按参数序首尾相接。
pub const BASELINE: &str = "abcdef";

/// 三件样例会话。
pub fn seeded() -> Cat {
    let mut c = Cat::new();
    let _ = c.add(1, KFILE, "ab");
    let _ = c.add(2, KFILE, "cd");
    let _ = c.add(3, KFILE, "ef");
    c
}

/// 一格观测。
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_ESLOT);
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
            return Err(CT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 4, 6, 4, 5];
        const CODES: [u32; 6] = [CT_EINVAL, CT_ENOENT, CT_EISDIR, CT_EIO, CT_EFULL, CT_EPERM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CT_EBAL),
            };
            if s.slot as usize != i {
                return Err(CT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CT_EVALUE);
                        }
                    }
                    None => return Err(CT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CT_EBAL);
        }
        Ok(())
    }

    /// 指纹（只折内容行）。
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
        h
    }

    /// 冻结并记根。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CT_EBAL);
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
            Err(CT_ESEAL)
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

/// 十四格账本：三源登记 + 三次逐件 + 两次播放 + 计数 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Cat::new();

    let _ = l.register(cell(0, OP_ADD, c.add(1, KFILE, "ab")));
    let _ = l.register(cell(1, OP_ADD, c.add(2, KFILE, "cd")));
    let _ = l.register(cell(2, OP_ADD, c.add(3, KFILE, "ef")));
    let _ = l.register(cell(3, OP_FEED, c.feed(1)));
    let _ = l.register(cell(4, OP_FEED, c.feed(2)));
    let _ = l.register(cell(5, OP_FEED, c.feed(3)));
    let _ = l.register(cell(6, OP_PLAY, c.play([2, 1, 0, 0], 2)));
    let _ = l.register(word_cell(7, OP_STAT, Ok(c.feeds())));

    let mut a = Cat::new();
    let _ = l.register(cell(8, OP_ADD, a.add(0, KFILE, "x")));

    let mut b = Cat::new();
    let _ = l.register(cell(9, OP_FEED, b.feed(99)));

    let mut d = Cat::new();
    let _ = d.add(4, KDIR, "");
    let _ = l.register(cell(10, OP_FEED, d.feed(4)));

    let mut e = Cat::new();
    let _ = e.add(5, KFILE, "z");
    let _ = e.poison(5);
    let _ = l.register(cell(11, OP_FEED, e.feed(5)));

    let mut f = Cat::new();
    let mut i = 1u32;
    while f.len() < CAP {
        let _ = f.add(i, KFILE, "q");
        i += 1;
    }
    let _ = l.register(cell(12, OP_ADD, f.add(90, KFILE, "q")));

    let mut g = Cat::new();
    let _ = g.add(7, KFILE, "s");
    let _ = l.register(cell(13, OP_ADD, g.add(7, KFILE, "t")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 一路：账本形状、对账、重放、指纹。
    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && rep == Ok(root)
        && root != 0;

    // 二路：三件按参数序拼接，与基准逐字节一致，重复件重复输出。
    let mut c = seeded();
    let a = c.play([1, 2, 3, 0], 3);
    let same = c.out_eq(BASELINE);
    let again = c.play([3, 1, 3, 0], 3);
    out[1] = a == Ok(6)
        && same
        && again == Ok(6)
        && c.out_eq("efabef")
        && c.feeds() == 6
        && c.len() == 3
        && c.consistent();

    // 三路：五类失败彼此可辨。
    let mut a = Cat::new();
    let inv = a.add(0, KFILE, "x") == Err(CT_EINVAL);
    let miss = a.feed(4) == Err(CT_ENOENT);
    let _ = a.add(1, KDIR, "");
    let dir = a.feed(1) == Err(CT_EISDIR);
    let _ = a.add(2, KFILE, "y");
    let _ = a.mark_busy(2);
    let busy = a.feed(2) == Err(CT_EBUSY);
    let _ = a.poison(2);
    let io = a.feed(2) == Err(CT_EBUSY);
    let _ = a.add(3, KFILE, "z");
    let _ = a.poison(3);
    let eio = a.feed(3) == Err(CT_EIO);
    out[2] = inv && miss && dir && busy && io && eio && a.consistent();

    // 四路：闸门次序、容量与输出上限。
    let mut q = Cat::new();
    let g1 = q.feed(0) == Err(CT_EINVAL);
    let g2 = q.feed(9) == Err(CT_ENOENT);
    let _ = q.add(1, KDIR, "");
    let _ = q.mark_busy(1);
    let g3 = q.feed(1) == Err(CT_EISDIR);
    let _ = q.add(2, KFILE, "abcdefghijklmnopqrstuvwx");
    let mut k = 0usize;
    let mut four = true;
    while k < 5 {
        if q.feed(2) != Ok(24 * (k + 1)) {
            four = false;
        }
        k += 1;
    }
    let g4 = four && q.out_len() == 120;
    let g5 = q.feed(2) == Err(CT_ENOSPC) && q.out_len() == OCAP;
    let g6 = q.play([0; ACAP], 0) == Err(CT_EINVAL);
    let g7 = q.play([0; ACAP], ACAP + 1) == Err(CT_EINVAL);
    let mut full = Cat::new();
    let mut i = 1u32;
    while full.len() < CAP {
        let _ = full.add(i, KFILE, "q");
        i += 1;
    }
    let g8 = full.add(99, KFILE, "q") == Err(CT_EFULL);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11401_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11401.cat_concat_order");
    let l = build_ledger();

    s.add_dyn(
        "CT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CT2-参数序逐件读出",
        l.sample(3).map(|x| x.value) == Some(2)
            && l.sample(4).map(|x| x.value) == Some(4)
            && l.sample(5).map(|x| x.value) == Some(6)
            && {
                let mut c = seeded();
                c.play([1, 2, 3, 0], 3) == Ok(6) && c.out_eq(BASELINE)
            },
        "static",
    );

    s.add(
        "CT3-基准机逐字节一致",
        BASELINE == "abcdef"
            && {
                let mut c = seeded();
                let _ = c.play([1, 2, 3, 0], 3);
                c.out_eq(BASELINE)
                    && c.out_eq("abcdef")
                    && !c.out_eq("abcdef\n")
                    && c.digest() == {
                        let mut d = Cat::new();
                        let _ = d.add(9, KFILE, "abcdef");
                        let _ = d.feed(9);
                        d.digest()
                    }
            },
        "static",
    );

    s.add(
        "CT4-重复件重复输出",
        {
            let mut c = seeded();
            let _ = c.play([2, 2, 0, 0], 2);
            c.out_eq("cdcd") && c.out_len() == 4 && c.feeds() == 2
        },
        "static",
    );

    s.add(
        "CT5-缺源ENOENT",
        l.sample(9).map(|x| x.code) == Some(CT_ENOENT)
            && {
                let mut c = seeded();
                c.feed(9) == Err(CT_ENOENT) && c.out_len() == 0 && c.rejects() == 1
            },
        "static",
    );

    s.add(
        "CT6-目录EISDIR",
        l.sample(10).map(|x| x.code) == Some(CT_EISDIR)
            && {
                let mut c = Cat::new();
                let _ = c.add(1, KDIR, "");
                c.feed(1) == Err(CT_EISDIR)
            },
        "static",
    );

    s.add(
        "CT7-读错EIO",
        l.sample(11).map(|x| x.code) == Some(CT_EIO)
            && {
                let mut c = seeded();
                let _ = c.poison(1);
                c.feed(1) == Err(CT_EIO) && c.feed(2) == Ok(2) && c.feeds() == 1
            },
        "static",
    );

    s.add(
        "CT8-输出满ENOSPC",
        {
            let mut d = Cat::new();
            let _ = d.add(1, KFILE, "abcdefghijklmnopqrstuvwx");
            let mut k = 0usize;
            let mut all = true;
            while k < 5 {
                if d.feed(1) != Ok(24 * (k + 1)) {
                    all = false;
                }
                k += 1;
            }
            all && d.out_len() == 120
                && d.feed(1) == Err(CT_ENOSPC)
                && d.out_len() == OCAP
                && d.feeds() == 5
        },
        "static",
    );

    s.add(
        "CT9-登记满EFULL",
        l.sample(12).map(|x| x.code) == Some(CT_EFULL)
            && {
                let mut c = seeded();
                let _ = c.add(4, KFILE, "g");
                let _ = c.add(5, KFILE, "h");
                let _ = c.add(6, KFILE, "i");
                c.len() == CAP && c.add(7, KFILE, "j") == Err(CT_EFULL) && c.consistent()
            },
        "static",
    );

    s.add(
        "CT10-占用EBUSY",
        l.sample(2).map(|x| x.value) == Some(3)
            && {
                let mut c = seeded();
                let _ = c.mark_busy(1);
                c.feed(1) == Err(CT_EBUSY) && c.feed(2) == Ok(2)
            },
        "static",
    );

    s.add(
        "CT11-空参数EINVAL",
        l.sample(8).map(|x| x.code) == Some(CT_EINVAL)
            && {
                let mut c = seeded();
                c.feed(0) == Err(CT_EINVAL)
                    && c.play([0; ACAP], 0) == Err(CT_EINVAL)
                    && c.play([0; ACAP], ACAP + 1) == Err(CT_EINVAL)
            },
        "static",
    );

    s.add(
        "CT12-以序换序即换字节",
        {
            let mut c = seeded();
            let _ = c.play([1, 2, 3, 0], 3);
            let forward = c.digest();
            let _ = c.play([3, 2, 1, 0], 3);
            let backward = c.digest();
            forward != backward
                && c.out_eq("efcdab")
                && c.feeds() == 6
                && c.consistent()
        },
        "static",
    );

    s.add(
        "CT13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && f.root() == f.seal()
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
        let s = run_unx11401_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn order_is_argument_order() {
        let mut c = seeded();
        assert_eq!(c.play([1, 2, 3, 0], 3), Ok(6));
        assert!(c.out_eq("abcdef"));
        assert_eq!(c.play([3, 2, 1, 0], 3), Ok(6));
        assert!(c.out_eq("efcdab"));
        assert_eq!(c.play([2, 2, 0, 0], 2), Ok(4));
        assert!(c.out_eq("cdcd"));
    }

    #[test]
    fn boundary_concatenation_has_no_separator() {
        let mut c = Cat::new();
        let _ = c.add(1, KFILE, "a");
        let _ = c.add(2, KFILE, "b");
        assert_eq!(c.play([1, 2, 0, 0], 2), Ok(2));
        assert!(c.out_eq("ab"));
    }

    #[test]
    fn dir_and_missing_are_distinct() {
        let mut c = Cat::new();
        let _ = c.add(1, KDIR, "");
        assert_eq!(c.feed(1), Err(CT_EISDIR));
        assert_eq!(c.feed(2), Err(CT_ENOENT));
        assert_eq!(c.rejects(), 2);
    }

    #[test]
    fn partial_output_survives_space_error() {
        let mut c = Cat::new();
        let _ = c.add(1, KFILE, "abcdefghijklmnopqrstuvwx");
        let mut k = 0usize;
        while k < 5 {
            assert_eq!(c.feed(1), Ok((k + 1) * 24));
            k += 1;
        }
        assert_eq!(c.feed(1), Err(CT_ENOSPC));
        assert_eq!(c.out_len(), OCAP);
    }

    #[test]
    fn busy_precedes_io() {
        let mut c = seeded();
        let _ = c.mark_busy(1);
        let _ = c.poison(1);
        assert_eq!(c.feed(1), Err(CT_EBUSY));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
