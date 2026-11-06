//! UNX-F11402 cat -n 行号标驻语义（0xef20 段）
//!
//! `cat -n` 的输出格式只有一条规则，却常被实现错：**行号右对齐占 6 列，后跟
//! 一个制表符，再跟该行内容；行号从 1 起，跨文件连续**。
//!
//! ```text
//!      1<TAB>a
//!      2<TAB>bb
//! ```
//!
//! 三个容易踩的坑：
//!
//! - 行号是「行首标记」，不是「行首前缀」：编号只在**行首**出现一次。若上一
//!   件末尾没有换行，下一件的首字节是续行，不重新编号——`cat -n a b` 在语义
//!   上先把两件拼成一个字节流，再逐行加号；
//! - 空文件不产生空行，也就不占号。编号只随「真正开始了新的一行」递增；
//! - 文件末尾缺少换行时，输出也缺少换行。格式化器绝不替用户补一个 `\n`。
//!
//! 错误类为何分开：`EINVAL` 是编号或种类这样的形状错；`ETEXT` 是单件内容超过
//! 行缓冲上限（要拆件或改上限）；`ENOENT` 是源不在册；`EISDIR` 是源不可读；
//! `EBUSY` 是源被占用（等释放）；`EIO` 是源读错（查介质）；`ENOSPC` 是输出缓冲
//! 写满（已写部分保留）；`EFULL` 是源表满。它们指向完全不同的修复动作，
//! 合并任何两类都会让排障走错方向。
//!
//! 闸门次序（`feed`）：编号 → 在册 → 目录 → 占用 → 读错 → 内容上限 → 格式化。
//! 内容上限放在读错之后：源不可读时，讨论它有多长没有意义。格式化输出的
//! 容量错误发生在写的过程中，因此 `ENOSPC` 可能带着一段已编号的部分输出。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef20;

/// 形状错：编号为 0 或种类越界。
pub const CN_EINVAL: u32 = SEG_BASE + 0x01;
/// 单件内容超出行缓冲上限。
pub const CN_ETEXT: u32 = SEG_BASE + 0x02;
/// 源不在册。
pub const CN_ENOENT: u32 = SEG_BASE + 0x03;
/// 源是目录，不可读。
pub const CN_EISDIR: u32 = SEG_BASE + 0x04;
/// 源被占用。
pub const CN_EBUSY: u32 = SEG_BASE + 0x05;
/// 源读出错。
pub const CN_EIO: u32 = SEG_BASE + 0x06;
/// 输出缓冲写满。
pub const CN_ENOSPC: u32 = SEG_BASE + 0x07;
/// 源登记表满。
pub const CN_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 源登记表容量（即 `CAP`）。
pub const CAP: usize = 6;
/// 单件内容上限。
pub const MAXLEN: usize = 24;
/// 输出缓冲容量。
pub const OCAP: usize = 192;
/// 行号列宽。
pub const WIDTH: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 种类：普通文件。
pub const KFILE: u8 = 0;
/// 种类：目录。
pub const KDIR: u8 = 1;

/// 操作码：登记源。
pub const OP_ADD: u8 = 1;
/// 操作码：读一件并编号。
pub const OP_FEED: u8 = 2;
/// 操作码：读行号计数。
pub const OP_LINE: u8 = 3;
/// 操作码：读输出长度。
pub const OP_LEN: u8 = 4;

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
}

/// 带行号的输出会话。
#[derive(Clone, Copy, Debug)]
pub struct CatN {
    src: [Src; CAP],
    n: usize,
    out: [u8; OCAP],
    outn: usize,
    line: u32,
    mid: bool,
    feeds: u32,
    rejects: u32,
}

impl CatN {
    /// 空会话。
    pub const fn new() -> CatN {
        CatN {
            src: [Src::blank(); CAP],
            n: 0,
            out: [0u8; OCAP],
            outn: 0,
            line: 0,
            mid: false,
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

    /// 已开始的行数。
    pub fn line(&self) -> u32 {
        self.line
    }

    /// 是否停在一行中间（上一件末尾没有换行）。
    pub fn mid(&self) -> bool {
        self.mid
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

    /// 登记一个源。闸门：形状 → 内容上限 → 重复 → 容量。
    pub fn add(&mut self, id: u32, kind: u8, text: &str) -> Result<usize, u32> {
        if id == 0 || kind > KDIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EINVAL);
        }
        if text.len() > MAXLEN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_ETEXT);
        }
        if self.find(id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EBUSY);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EFULL);
        }
        let mut x = Src::blank();
        x.id = id;
        x.kind = kind;
        x.live = true;
        let bytes = text.as_bytes();
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
        self.mark(id, true, false)
    }

    /// 标记读错。
    pub fn poison(&mut self, id: u32) -> Result<usize, u32> {
        self.mark(id, false, true)
    }

    fn mark(&mut self, id: u32, busy: bool, bad: bool) -> Result<usize, u32> {
        let mut i = 0usize;
        let mut at = None;
        while i < self.n {
            if let Some(s) = self.src.get(i) {
                if s.live && s.id == id {
                    at = Some(i);
                }
            }
            i += 1;
        }
        let idx = match at {
            Some(v) => v,
            None => return Err(CN_ENOENT),
        };
        let cur = match self.src.get(idx) {
            Some(v) => *v,
            None => return Err(CN_ENOENT),
        };
        if let Some(dst) = self.src.get_mut(idx) {
            *dst = Src { busy: busy || cur.busy, poisoned: bad || cur.poisoned, ..cur };
        }
        Ok(self.n)
    }

    fn push_byte(&mut self, c: u8) -> Result<(), u32> {
        if self.outn >= OCAP {
            return Err(CN_ENOSPC);
        }
        if let Some(d) = self.out.get_mut(self.outn) {
            *d = c;
        }
        self.outn += 1;
        Ok(())
    }

    fn emit_number(&mut self) -> Result<(), u32> {
        let mut digits = [0u8; 10];
        let mut dn = 0usize;
        let mut x = self.line.saturating_add(1);
        loop {
            if let Some(d) = digits.get_mut(dn) {
                *d = b'0' + (x % 10) as u8;
            }
            dn += 1;
            x /= 10;
            if x == 0 || dn >= 10 {
                break;
            }
        }
        let pad = if dn >= WIDTH { 0 } else { WIDTH - dn };
        let mut k = 0usize;
        while k < pad {
            self.push_byte(b' ')?;
            k += 1;
        }
        let mut j = dn;
        while j > 0 {
            j -= 1;
            let d = match digits.get(j) {
                Some(v) => *v,
                None => return Err(CN_EVALUE),
            };
            self.push_byte(d)?;
        }
        self.push_byte(b'\t')?;
        self.line = self.line.saturating_add(1);
        Ok(())
    }

    /// 直接格式化一段文本（源读出的字节流）。
    ///
    /// 行首才编号；行号与内容之间是一个制表符；不补末尾换行。
    pub fn feed_text(&mut self, text: &str) -> Result<usize, u32> {
        let bytes = text.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = match bytes.get(i) {
                Some(v) => *v,
                None => break,
            };
            if !self.mid {
                self.emit_number()?;
            }
            self.push_byte(c)?;
            if c == b'\n' {
                self.mid = false;
            } else {
                self.mid = true;
            }
            i += 1;
        }
        Ok(self.outn)
    }

    /// 读一件并编号。
    ///
    /// 闸门：编号 → 在册 → 目录 → 占用 → 读错 → 内容上限 → 格式化。
    pub fn feed(&mut self, id: u32) -> Result<usize, u32> {
        if id == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EINVAL);
        }
        let s = match self.find(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CN_ENOENT);
            }
        };
        if s.kind == KDIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EISDIR);
        }
        if s.busy {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EBUSY);
        }
        if s.poisoned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_EIO);
        }
        if s.n > MAXLEN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CN_ETEXT);
        }
        let mut piece = [0u8; MAXLEN];
        let mut m = 0usize;
        while m < s.n {
            match s.b.get(m) {
                Some(c) => {
                    if let Some(d) = piece.get_mut(m) {
                        *d = *c;
                    }
                }
                None => break,
            }
            m += 1;
        }
        let r = self.feed_bytes(&piece, m);
        if r.is_err() {
            self.rejects = self.rejects.saturating_add(1);
            return r.map(|_| self.outn);
        }
        self.feeds = self.feeds.saturating_add(1);
        Ok(self.outn)
    }

    fn feed_bytes(&mut self, b: &[u8], n: usize) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < n {
            let c = match b.get(i) {
                Some(v) => *v,
                None => break,
            };
            if !self.mid {
                self.emit_number()?;
            }
            self.push_byte(c)?;
            if c == b'\n' {
                self.mid = false;
            } else {
                self.mid = true;
            }
            i += 1;
        }
        Ok(self.outn)
    }

    /// 输出是否等于字面量。
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

    /// 不变式：源数、输出长度、行号与 mid 状态自洽。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.outn > OCAP {
            return false;
        }
        if self.mid && self.outn == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.src.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if !a.live || a.id == 0 || a.n > MAXLEN || a.kind > KDIR {
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

/// 基准输出：两行编号，跨件续行后编号 3 与 4。
pub const BASELINE: &str = "     1\ta\n     2\tbb\n     3\tccc\n     4\tddd\n";

/// 三件样例会话。
pub fn seeded() -> CatN {
    let mut c = CatN::new();
    let _ = c.add(1, KFILE, "a\nbb\n");
    let _ = c.add(2, KFILE, "ccc");
    let _ = c.add(3, KFILE, "\nddd\n");
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
            return Err(CN_ESLOT);
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
            return Err(CN_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 19, 29, 41, 4, 60];
        const CODES: [u32; 6] = [CN_EINVAL, CN_ETEXT, CN_ENOENT, CN_EISDIR, CN_EBUSY, CN_EIO];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CN_EBAL),
            };
            if s.slot as usize != i {
                return Err(CN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CN_EVALUE);
                        }
                    }
                    None => return Err(CN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CN_EVALUE);
                        }
                    }
                    None => return Err(CN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CN_EBAL);
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
            return Err(CN_EBAL);
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
            Err(CN_ESEAL)
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

/// 十四格账本：三源登记 + 三次编号读出 + 行号 + 再读一件 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = CatN::new();

    let _ = l.register(cell(0, OP_ADD, c.add(1, KFILE, "a\nbb\n")));
    let _ = l.register(cell(1, OP_ADD, c.add(2, KFILE, "ccc")));
    let _ = l.register(cell(2, OP_ADD, c.add(3, KFILE, "\nddd\n")));
    let _ = l.register(cell(3, OP_FEED, c.feed(1)));
    let _ = l.register(cell(4, OP_FEED, c.feed(2)));
    let _ = l.register(cell(5, OP_FEED, c.feed(3)));
    let _ = l.register(word_cell(6, OP_LINE, Ok(c.line())));
    let _ = l.register(cell(7, OP_FEED, c.feed(1)));

    let mut a = CatN::new();
    let _ = l.register(cell(8, OP_ADD, a.add(0, KFILE, "x")));

    let mut b = CatN::new();
    let _ = l.register(cell(9, OP_ADD, b.add(1, KFILE, "abcdefghijklmnopqrstuvwxy")));

    let mut c1 = CatN::new();
    let _ = l.register(cell(10, OP_FEED, c1.feed(42)));

    let mut d = CatN::new();
    let _ = d.add(1, KDIR, "");
    let _ = l.register(cell(11, OP_FEED, d.feed(1)));

    let mut e = CatN::new();
    let _ = e.add(1, KFILE, "x");
    let _ = e.mark_busy(1);
    let _ = l.register(cell(12, OP_FEED, e.feed(1)));

    let mut f = CatN::new();
    let _ = f.add(1, KFILE, "x");
    let _ = f.poison(1);
    let _ = l.register(cell(13, OP_FEED, f.feed(1)));

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

    // 二路：三件跨件连续编号，与基准逐字节一致。
    let mut c = seeded();
    let a = c.feed(1);
    let b = c.feed(2);
    let d = c.feed(3);
    out[1] = a == Ok(19)
        && b == Ok(29)
        && d == Ok(41)
        && c.line() == 4
        && !c.mid()
        && c.out_eq(BASELINE)
        && c.feeds() == 3
        && c.consistent();

    // 三路：八类问题彼此可辨。
    let mut a = CatN::new();
    let inv = a.add(0, KFILE, "x") == Err(CN_EINVAL);
    let text = a.add(1, KFILE, "abcdefghijklmnopqrstuvwxy") == Err(CN_ETEXT);
    let miss = a.feed(9) == Err(CN_ENOENT);
    let _ = a.add(2, KDIR, "");
    let dir = a.feed(2) == Err(CN_EISDIR);
    let _ = a.add(3, KFILE, "z");
    let _ = a.mark_busy(3);
    let busy = a.feed(3) == Err(CN_EBUSY);
    let _ = a.poison(3);
    let still = a.feed(3) == Err(CN_EBUSY);
    let _ = a.add(4, KFILE, "w");
    let _ = a.poison(4);
    let io = a.feed(4) == Err(CN_EIO);
    out[2] = inv && text && miss && dir && busy && still && io && a.consistent();

    // 四路：闸门次序、容量与部分输出。
    let mut q = CatN::new();
    let g1 = q.feed(0) == Err(CN_EINVAL);
    let g2 = q.feed(9) == Err(CN_ENOENT);
    let _ = q.add(1, KDIR, "");
    let _ = q.mark_busy(1);
    let g3 = q.feed(1) == Err(CN_EISDIR);
    let _ = q.add(2, KFILE, "abcdefghijklmnopqrstuvwx");
    let g4 = q.feed_text("abcdefghijklmnopqrstuvwxyz") == Ok(6 + 1 + 26);
    let mut k = 0usize;
    let mut five = true;
    while k < 5 {
        if q.feed(2) != Ok(33 + 24 * (k + 1)) {
            five = false;
        }
        k += 1;
    }
    let g5 = five && q.out_len() == 153 && q.feed(2) == Ok(177);
    let g6 = q.feed(2) == Err(CN_ENOSPC) && q.out_len() == OCAP;
    let mut full = CatN::new();
    let mut i = 1u32;
    while full.len() < CAP {
        let _ = full.add(i, KFILE, "q");
        i += 1;
    }
    let g7 = full.add(90, KFILE, "q") == Err(CN_EFULL);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11402_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11402.cat_number_lines");
    let l = build_ledger();

    s.add_dyn(
        "CN1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CN2-六位右对齐加制表",
        l.sample(3).map(|x| x.value) == Some(19)
            && {
                let mut c = CatN::new();
                let _ = c.add(1, KFILE, "a\n");
                c.feed(1) == Ok(9) && c.out_eq("     1\ta\n")
            },
        "static",
    );

    s.add(
        "CN3-跨件连续编号",
        l.sample(4).map(|x| x.value) == Some(29)
            && l.sample(5).map(|x| x.value) == Some(41)
            && {
                let mut c = seeded();
                let _ = c.feed(1);
                let _ = c.feed(2);
                let _ = c.feed(3);
                c.out_eq(BASELINE) && c.line() == 4
            },
        "static",
    );

    s.add(
        "CN4-中行续接不重编号",
        {
            let mut c = CatN::new();
            let _ = c.add(1, KFILE, "abc");
            let _ = c.add(2, KFILE, "def\n");
            let one = c.feed(1);
            let two = c.feed(2);
            one == Ok(6 + 1 + 3)
                && two == Ok(6 + 1 + 7)
                && c.out_eq("     1\tabcdef\n")
                && c.line() == 1
        },
        "static",
    );

    s.add(
        "CN5-空件不占号",
        {
            let mut c = CatN::new();
            let _ = c.add(1, KFILE, "");
            let _ = c.add(2, KFILE, "x\n");
            c.feed(1) == Ok(0)
                && c.line() == 0
                && c.feed(2) == Ok(9)
                && c.line() == 1
                && c.out_eq("     1\tx\n")
        },
        "static",
    );

    s.add(
        "CN6-编号零EINVAL",
        l.sample(8).map(|x| x.code) == Some(CN_EINVAL)
            && {
                let mut c = CatN::new();
                c.add(0, KFILE, "x") == Err(CN_EINVAL)
                    && c.add(1, 9, "x") == Err(CN_EINVAL)
                    && c.len() == 0
            },
        "static",
    );

    s.add(
        "CN7-内容超限ETEXT",
        l.sample(9).map(|x| x.code) == Some(CN_ETEXT)
            && {
                let mut c = CatN::new();
                c.add(1, KFILE, "abcdefghijklmnopqrstuvwxy") == Err(CN_ETEXT)
                    && c.add(1, KFILE, "abcdefghijklmnopqrstuvwx") == Ok(1)
            },
        "static",
    );

    s.add(
        "CN8-缺源ENOENT",
        l.sample(10).map(|x| x.code) == Some(CN_ENOENT)
            && {
                let mut c = seeded();
                c.feed(9) == Err(CN_ENOENT) && c.out_len() == 0 && c.line() == 0
            },
        "static",
    );

    s.add(
        "CN9-目录EISDIR",
        l.sample(11).map(|x| x.code) == Some(CN_EISDIR)
            && {
                let mut c = CatN::new();
                let _ = c.add(1, KDIR, "");
                c.feed(1) == Err(CN_EISDIR)
            },
        "static",
    );

    s.add(
        "CN10-占用EBUSY",
        l.sample(12).map(|x| x.code) == Some(CN_EBUSY)
            && {
                let mut c = seeded();
                let _ = c.mark_busy(1);
                c.feed(1) == Err(CN_EBUSY) && c.feed(2) == Ok(10)
            },
        "static",
    );

    s.add(
        "CN11-读错EIO",
        l.sample(13).map(|x| x.code) == Some(CN_EIO)
            && {
                let mut c = seeded();
                let _ = c.poison(1);
                c.feed(1) == Err(CN_EIO) && c.feed_text("ok") == Ok(9)
            },
        "static",
    );

    s.add(
        "CN12-输出满保留部分",
        {
            let mut c = CatN::new();
            let _ = c.add(1, KFILE, "abcdefghij\n");
            let mut i = 0usize;
            let mut all = true;
            while i < 10 {
                if c.feed(1) != Ok(18 * (i + 1)) {
                    all = false;
                }
                i += 1;
            }
            all && c.feed(1) == Err(CN_ENOSPC) && c.out_len() == OCAP && c.consistent()
        },
        "static",
    );

    s.add(
        "CN13-冻结重放四注入",
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
        let s = run_unx11402_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn numbers_continue_across_files() {
        let mut c = seeded();
        assert_eq!(c.feed(1), Ok(19));
        assert_eq!(c.feed(2), Ok(29));
        assert_eq!(c.feed(3), Ok(41));
        assert!(c.out_eq(BASELINE));
        assert_eq!(c.line(), 4);
        assert!(!c.mid());
    }

    #[test]
    fn width_is_six_and_tab_follows() {
        let mut c = CatN::new();
        let _ = c.add(1, KFILE, "x");
        let _ = c.feed(1);
        assert!(c.out_eq("     1\tx"));
    }

    #[test]
    fn no_trailing_newline_is_preserved() {
        let mut c = CatN::new();
        let _ = c.add(1, KFILE, "x");
        let _ = c.feed(1);
        assert!(c.mid());
        assert_eq!(c.out_len(), 8);
        assert!(!c.out_eq("     1\tx\n"));
    }

    #[test]
    fn partial_line_merges() {
        let mut c = CatN::new();
        let _ = c.add(1, KFILE, "abc");
        let _ = c.add(2, KFILE, "def\n");
        let _ = c.feed(1);
        let _ = c.feed(2);
        assert!(c.out_eq("     1\tabcdef\n"));
        assert_eq!(c.line(), 1);
    }

    #[test]
    fn empty_file_takes_no_number() {
        let mut c = CatN::new();
        let _ = c.add(1, KFILE, "");
        let _ = c.add(2, KFILE, "y\n");
        assert_eq!(c.feed(1), Ok(0));
        assert_eq!(c.line(), 0);
        assert_eq!(c.feed(2), Ok(9));
        assert!(c.out_eq("     1\ty\n"));
    }

    #[test]
    fn gates_precede_formatting() {
        let mut c = CatN::new();
        let _ = c.add(1, KDIR, "");
        let _ = c.mark_busy(1);
        assert_eq!(c.feed(1), Err(CN_EISDIR));
        assert_eq!(c.feed(2), Err(CN_ENOENT));
        assert_eq!(c.feed(0), Err(CN_EINVAL));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
