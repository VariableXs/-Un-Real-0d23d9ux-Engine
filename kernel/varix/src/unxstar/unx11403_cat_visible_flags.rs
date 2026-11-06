//! UNX-F11403 cat -E/-T/-A 空白呈现族（0xef30 段）
//!
//! `cat` 的三个「可见化」选项改变的是**字节的呈现**，不是字节的语义：
//!
//! - `-E`：在每个换行符前先输出一个 `$`，于是行尾可见；
//! - `-T`：把制表符写成 `^I`，于是缩进不再吃掉列宽；
//! - `-A`：等价于 `-vET`，除制表与换行外的控制字节也写成 `^X`，DEL 写成
//!   `^?`，高位字节写成 `M-` 加低位呈现。
//!
//! 组合旗的关键在于**同时生效且互不吞并**：`-A` 里的 `T` 只处理 `\t`，`E` 只
//! 处理 `\n`，`V` 处理其余控制字节；三者作用于不同的字节集合，因此 `-A` 的
//! 输出等于 `-E`、`-T`、`-v` 三者输出再拼接的组合效果——这是与基准机逐字节
//! 一致的前提。
//!
//! 另一个容易被忽略的语义是**标志位在首个输出字节之后即锁定**。真实 cat 的
//! 选项在启动时解析完毕，运行期不会改变；若允许在输出中途换旗，已经写出的
//! 前半个流与后半个流就用了两套呈现规则，快照无法与基准机对齐。故本模块在
//! `outn > 0` 之后拒绝改旗，判 `EFLAG`。
//!
//! 错误类：`EINVAL` 是编号/掩码形状错；`EFLAG` 是改旗时机错；`ENOENT` 是源
//! 不在册；`EISDIR` 是源不可读；`EBUSY` 是源被占用；`EIO` 是源读错；`ENOSPC`
//! 是输出写满（部分输出保留）；`EFULL` 是源表满。
//!
//! 闸门次序（`feed`）：编号 → 在册 → 目录 → 占用 → 读错 → 变换输出。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef30;

/// 形状错：编号为 0、种类越界、旗掩码含未知位。
pub const CE_EINVAL: u32 = SEG_BASE + 0x01;
/// 已开始输出后改旗。
pub const CE_EFLAG: u32 = SEG_BASE + 0x02;
/// 源不在册。
pub const CE_ENOENT: u32 = SEG_BASE + 0x03;
/// 源是目录，不可读。
pub const CE_EISDIR: u32 = SEG_BASE + 0x04;
/// 源被占用。
pub const CE_EBUSY: u32 = SEG_BASE + 0x05;
/// 源读出错。
pub const CE_EIO: u32 = SEG_BASE + 0x06;
/// 输出缓冲写满。
pub const CE_ENOSPC: u32 = SEG_BASE + 0x07;
/// 源登记表满。
pub const CE_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CE_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CE_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 旗：行尾 `$`。
pub const F_E: u8 = 0x01;
/// 旗：制表 `^I`。
pub const F_T: u8 = 0x02;
/// 旗：控制字节可见。
pub const F_V: u8 = 0x04;
/// 旗：全部可见（`-A`）。
pub const F_A: u8 = F_E | F_T | F_V;

/// 源登记表容量（即 `CAP`）。
pub const CAP: usize = 6;
/// 单件内容上限。
pub const MAXLEN: usize = 24;
/// 输出缓冲容量。
pub const OCAP: usize = 160;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 种类：普通文件。
pub const KFILE: u8 = 0;
/// 种类：目录。
pub const KDIR: u8 = 1;

/// 操作码：登记源。
pub const OP_ADD: u8 = 1;
/// 操作码：设旗。
pub const OP_FLAGS: u8 = 2;
/// 操作码：读一件。
pub const OP_FEED: u8 = 3;
/// 操作码：读旗掩码。
pub const OP_MASK: u8 = 4;

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

/// 可见化输出会话。
#[derive(Clone, Copy, Debug)]
pub struct Vis {
    src: [Src; CAP],
    n: usize,
    out: [u8; OCAP],
    outn: usize,
    flags: u8,
    feeds: u32,
    rejects: u32,
}

impl Vis {
    /// 空会话。
    pub const fn new() -> Vis {
        Vis {
            src: [Src::blank(); CAP],
            n: 0,
            out: [0u8; OCAP],
            outn: 0,
            flags: 0,
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

    /// 当前旗掩码。
    pub fn flags(&self) -> u8 {
        self.flags
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

    /// 登记一个源。
    pub fn add(&mut self, id: u32, kind: u8, text: &str) -> Result<usize, u32> {
        self.add_bytes(id, kind, text.as_bytes())
    }

    /// 按原始字节登记（高位字节无法用 `&str` 表达）。
    pub fn add_bytes(&mut self, id: u32, kind: u8, bytes: &[u8]) -> Result<usize, u32> {
        if id == 0 || kind > KDIR || bytes.len() > MAXLEN {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if self.find(id).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EBUSY);
        }
        if self.n >= CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EFULL);
        }
        let mut x = Src::blank();
        x.id = id;
        x.kind = kind;
        x.live = true;
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
            None => return Err(CE_ENOENT),
        };
        let cur = match self.src.get(idx) {
            Some(v) => *v,
            None => return Err(CE_ENOENT),
        };
        if let Some(dst) = self.src.get_mut(idx) {
            *dst = Src { busy: busy || cur.busy, poisoned: bad || cur.poisoned, ..cur };
        }
        Ok(self.n)
    }

    /// 设旗。掩码只允许 `F_E|F_T|F_V`；已开始输出后改旗判 `EFLAG`。
    pub fn set_flags(&mut self, mask: u8) -> Result<usize, u32> {
        if mask & !F_A != 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if self.outn > 0 && mask != self.flags {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EFLAG);
        }
        self.flags = mask;
        Ok(mask.count_ones() as usize)
    }

    fn push_byte(&mut self, c: u8) -> Result<(), u32> {
        if self.outn >= OCAP {
            return Err(CE_ENOSPC);
        }
        if let Some(d) = self.out.get_mut(self.outn) {
            *d = c;
        }
        self.outn += 1;
        Ok(())
    }

    fn push_str(&mut self, s: &str) -> Result<(), u32> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            match b.get(i) {
                Some(c) => self.push_byte(*c)?,
                None => break,
            }
            i += 1;
        }
        Ok(())
    }

    /// 呈现一个字节。
    fn emit(&mut self, c: u8) -> Result<(), u32> {
        if c == b'\t' {
            if self.flags & F_T != 0 {
                return self.push_str("^I");
            }
            return self.push_byte(c);
        }
        if c == b'\n' {
            if self.flags & F_E != 0 {
                self.push_byte(b'$')?;
            }
            return self.push_byte(c);
        }
        if self.flags & F_V != 0 {
            if c < 0x20 {
                self.push_byte(b'^')?;
                return self.push_byte(c + 64);
            }
            if c == 0x7f {
                return self.push_str("^?");
            }
            if c >= 0x80 {
                self.push_str("M-")?;
                let low = c - 0x80;
                if low < 0x20 {
                    self.push_byte(b'^')?;
                    return self.push_byte(low + 64);
                }
                if low == 0x7f {
                    return self.push_str("^?");
                }
                return self.push_byte(low);
            }
        }
        self.push_byte(c)
    }

    /// 按当前旗呈现一段文本。
    pub fn feed_text(&mut self, text: &str) -> Result<usize, u32> {
        let b = text.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            let c = match b.get(i) {
                Some(v) => *v,
                None => break,
            };
            self.emit(c)?;
            i += 1;
        }
        Ok(self.outn)
    }

    /// 读一件。闸门：编号 → 在册 → 目录 → 占用 → 读错 → 呈现。
    pub fn feed(&mut self, id: u32) -> Result<usize, u32> {
        if id == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EINVAL);
        }
        let s = match self.find(id) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CE_ENOENT);
            }
        };
        if s.kind == KDIR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EISDIR);
        }
        if s.busy {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EBUSY);
        }
        if s.poisoned {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CE_EIO);
        }
        let mut i = 0usize;
        while i < s.n {
            let c = match s.b.get(i) {
                Some(v) => *v,
                None => break,
            };
            let r = self.emit(c);
            if r.is_err() {
                self.rejects = self.rejects.saturating_add(1);
                return Err(CE_ENOSPC);
            }
            i += 1;
        }
        self.feeds = self.feeds.saturating_add(1);
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

    /// 不变式：源数、输出、旗掩码自洽。
    pub fn consistent(&self) -> bool {
        if self.n > CAP || self.outn > OCAP || self.flags & !F_A != 0 {
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

/// 基准输出：`-A` 下制表、控制字节与行尾全部可见。
pub const BASELINE: &str = "a^Ib$\nx^Ay$\n";

/// 两件样例会话，旗为 `-A`。
pub fn seeded() -> Vis {
    let mut v = Vis::new();
    let _ = v.set_flags(F_A);
    let _ = v.add(1, KFILE, "a\tb\n");
    let _ = v.add(2, KFILE, "x\u{1}y\n");
    v
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
            return Err(CE_ESLOT);
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
            return Err(CE_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 6, 12, 7, 3, 18];
        const CODES: [u32; 6] = [CE_EINVAL, CE_EFLAG, CE_ENOENT, CE_EISDIR, CE_EBUSY, CE_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CE_EBAL),
            };
            if s.slot as usize != i {
                return Err(CE_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CE_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CE_EVALUE);
                        }
                    }
                    None => return Err(CE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CE_EVALUE);
                        }
                    }
                    None => return Err(CE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CE_EBAL);
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
            return Err(CE_EBAL);
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
            Err(CE_ESEAL)
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

/// 十四格账本：两源 + 设三旗 + 两次呈现 + 读旗 + 第三源 + 呈现 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut v = Vis::new();

    let _ = l.register(cell(0, OP_ADD, v.add(1, KFILE, "a\tb\n")));
    let _ = l.register(cell(1, OP_ADD, v.add(2, KFILE, "x\u{1}y\n")));
    let _ = l.register(cell(2, OP_FLAGS, v.set_flags(F_A)));
    let _ = l.register(cell(3, OP_FEED, v.feed(1)));
    let _ = l.register(cell(4, OP_FEED, v.feed(2)));
    let _ = l.register(word_cell(5, OP_MASK, Ok(v.flags() as u32)));
    let _ = l.register(cell(6, OP_ADD, v.add(3, KFILE, "d\te\n")));
    let _ = l.register(cell(7, OP_FEED, v.feed(3)));

    let mut a = Vis::new();
    let _ = l.register(cell(8, OP_ADD, a.add(0, KFILE, "x")));

    let mut b = Vis::new();
    let _ = b.add(1, KFILE, "x");
    let _ = b.feed(1);
    let _ = l.register(cell(9, OP_FLAGS, b.set_flags(F_E)));

    let mut c = Vis::new();
    let _ = l.register(cell(10, OP_FEED, c.feed(77)));

    let mut d = Vis::new();
    let _ = d.add(1, KDIR, "");
    let _ = l.register(cell(11, OP_FEED, d.feed(1)));

    let mut e = Vis::new();
    let _ = e.add(1, KFILE, "x");
    let _ = e.mark_busy(1);
    let _ = l.register(cell(12, OP_FEED, e.feed(1)));

    let mut f = Vis::new();
    let mut i = 1u32;
    while f.len() < CAP {
        let _ = f.add(i, KFILE, "q");
        i += 1;
    }
    let _ = l.register(cell(13, OP_ADD, f.add(90, KFILE, "q")));

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

    // 二路：-A 组合旗与基准逐字节一致，三旗各自独立生效。
    let mut v = seeded();
    let one = v.feed(1);
    let two = v.feed(2);
    let mut e = Vis::new();
    let _ = e.set_flags(F_E);
    let _ = e.add(1, KFILE, "a\tb\n");
    let e_only = e.feed(1) == Ok(5) && e.out_eq("a\tb$\n");
    let mut t = Vis::new();
    let _ = t.set_flags(F_T);
    let _ = t.add(1, KFILE, "a\tb\n");
    let t_only = t.feed(1) == Ok(5) && t.out_eq("a^Ib\n");
    out[1] = one == Ok(6)
        && two == Ok(12)
        && v.out_eq(BASELINE)
        && v.flags() == F_A
        && v.feeds() == 2
        && e_only
        && t_only
        && v.consistent();

    // 三路：六类问题彼此可辨。
    let mut a = Vis::new();
    let inv = a.add(0, KFILE, "x") == Err(CE_EINVAL);
    let flag = a.set_flags(0x40) == Err(CE_EINVAL);
    let miss = a.feed(9) == Err(CE_ENOENT);
    let _ = a.add(1, KDIR, "");
    let dir = a.feed(1) == Err(CE_EISDIR);
    let _ = a.add(2, KFILE, "z");
    let _ = a.mark_busy(2);
    let busy = a.feed(2) == Err(CE_EBUSY);
    let _ = a.add(3, KFILE, "w");
    let _ = a.poison(3);
    let io = a.feed(3) == Err(CE_EIO);
    out[2] = inv && flag && miss && dir && busy && io && a.consistent();

    // 四路：闸门次序、改旗锁与容量。
    let mut q = Vis::new();
    let g1 = q.feed(0) == Err(CE_EINVAL);
    let g2 = q.feed(9) == Err(CE_ENOENT);
    let _ = q.add(1, KDIR, "");
    let _ = q.mark_busy(1);
    let g3 = q.feed(1) == Err(CE_EISDIR);
    let _ = q.add(2, KFILE, "ab\n");
    let g4 = q.set_flags(F_A) == Ok(3);
    let g5 = q.feed(2) == Ok(4);
    let g6 = q.set_flags(F_E) == Err(CE_EFLAG);
    let g7 = q.set_flags(F_A) == Ok(3);
    let mut full = Vis::new();
    let mut i = 1u32;
    while full.len() < CAP {
        let _ = full.add(i, KFILE, "q");
        i += 1;
    }
    let g8 = full.add(99, KFILE, "q") == Err(CE_EFULL);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && q.consistent() && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11403_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11403.cat_visible_flags");
    let l = build_ledger();

    s.add_dyn(
        "CE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CE2-行尾美元符",
        l.sample(3).map(|x| x.value) == Some(6)
            && {
                let mut v = Vis::new();
                let _ = v.set_flags(F_E);
                let _ = v.add(1, KFILE, "ab\ncd\n");
                v.feed(1) == Ok(8) && v.out_eq("ab$\ncd$\n")
            },
        "static",
    );

    s.add(
        "CE3-制表写成脱字符I",
        l.sample(4).map(|x| x.value) == Some(12)
            && {
                let mut v = Vis::new();
                let _ = v.set_flags(F_T);
                let _ = v.add(1, KFILE, "\t\tx");
                v.feed(1) == Ok(5) && v.out_eq("^I^Ix")
            },
        "static",
    );

    s.add(
        "CE4-全符控制字节呈现",
        {
            let mut v = Vis::new();
            let _ = v.set_flags(F_A);
            let _ = v.add_bytes(1, KFILE, &[0x01, 0x7f, 0x80]);
            v.feed(1) == Ok(8) && v.out_eq("^A^?M-^@")
        },
        "static",
    );

    s.add(
        "CE5-组合旗与基准一致",
        l.sample(5).map(|x| x.value) == Some(7)
            && {
                let mut v = seeded();
                let _ = v.feed(1);
                let _ = v.feed(2);
                v.out_eq(BASELINE)
                    && v.digest() == {
                        let mut w = Vis::new();
                        let _ = w.set_flags(F_A);
                        let _ = w.add(9, KFILE, "a\tb\n");
                        let _ = w.add(8, KFILE, "x\u{1}y\n");
                        let _ = w.feed(9);
                        let _ = w.feed(8);
                        w.digest()
                    }
            },
        "static",
    );

    s.add(
        "CE6-输出后改旗EFLAG",
        l.sample(9).map(|x| x.code) == Some(CE_EFLAG)
            && {
                let mut v = Vis::new();
                let _ = v.add(1, KFILE, "x");
                let first = v.feed(1) == Ok(1);
                let same = v.set_flags(0) == Ok(0);
                first && same && v.set_flags(F_E) == Err(CE_EFLAG) && v.flags() == 0
            },
        "static",
    );

    s.add(
        "CE7-形状EINVAL",
        l.sample(8).map(|x| x.code) == Some(CE_EINVAL)
            && {
                let mut v = Vis::new();
                v.add(0, KFILE, "x") == Err(CE_EINVAL)
                    && v.add(1, 9, "x") == Err(CE_EINVAL)
                    && v.set_flags(0x40) == Err(CE_EINVAL)
                    && v.flags() == 0
            },
        "static",
    );

    s.add(
        "CE8-缺源ENOENT",
        l.sample(10).map(|x| x.code) == Some(CE_ENOENT)
            && {
                let mut v = seeded();
                v.feed(77) == Err(CE_ENOENT) && v.out_len() == 0
            },
        "static",
    );

    s.add(
        "CE9-目录EISDIR",
        l.sample(11).map(|x| x.code) == Some(CE_EISDIR)
            && {
                let mut v = Vis::new();
                let _ = v.add(1, KDIR, "");
                v.feed(1) == Err(CE_EISDIR)
            },
        "static",
    );

    s.add(
        "CE10-占用EBUSY",
        l.sample(12).map(|x| x.code) == Some(CE_EBUSY)
            && {
                let mut v = seeded();
                let _ = v.mark_busy(1);
                v.feed(1) == Err(CE_EBUSY) && v.feed(2) == Ok(6)
            },
        "static",
    );

    s.add(
        "CE11-读错EIO",
        {
            let mut v = seeded();
            let _ = v.poison(1);
            let a = v.feed(1) == Err(CE_EIO);
            let b = v.feed(2) == Ok(6);
            a && b && v.out_eq("x^Ay$\n")
        },
        "static",
    );

    s.add(
        "CE12-输出满与登记满",
        l.sample(13).map(|x| x.code) == Some(CE_EFULL)
            && {
                let mut v = Vis::new();
                let _ = v.set_flags(F_A);
                let _ = v.add(1, KFILE, "\t\t\t\t\t\t\t\t");
                let mut i = 0usize;
                let mut all = true;
                while i < 10 {
                    if v.feed(1) != Ok(16 * (i + 1)) {
                        all = false;
                    }
                    i += 1;
                }
                all && v.out_len() == 160
                    && v.feed(1).is_err()
                    && v.out_len() == OCAP
            },
        "static",
    );

    s.add(
        "CE13-冻结重放四注入",
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
        let s = run_unx11403_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn flags_compose() {
        let mut v = seeded();
        assert_eq!(v.feed(1), Ok(6));
        assert_eq!(v.feed(2), Ok(12));
        assert!(v.out_eq(BASELINE));
        assert_eq!(v.flags(), F_A);
    }

    #[test]
    fn each_flag_is_independent() {
        let mut e = Vis::new();
        let _ = e.set_flags(F_E);
        let _ = e.add(1, KFILE, "a\tb\n");
        assert_eq!(e.feed(1), Ok(5));
        assert!(e.out_eq("a\tb$\n"));

        let mut t = Vis::new();
        let _ = t.set_flags(F_T);
        let _ = t.add(1, KFILE, "a\tb\n");
        assert_eq!(t.feed(1), Ok(5));
        assert!(t.out_eq("a^Ib\n"));

        let mut v = Vis::new();
        let _ = v.set_flags(F_V);
        let _ = v.add_bytes(1, KFILE, &[0x01, 0x7f]);
        assert_eq!(v.feed(1), Ok(4));
        assert!(v.out_eq("^A^?"));
    }

    #[test]
    fn flags_lock_after_output() {
        let mut v = Vis::new();
        let _ = v.add(1, KFILE, "x");
        let _ = v.feed(1);
        assert_eq!(v.set_flags(F_E), Err(CE_EFLAG));
        assert_eq!(v.set_flags(0), Ok(0));
        assert_eq!(v.flags(), 0);
    }

    #[test]
    fn high_bytes_render_meta() {
        let mut v = Vis::new();
        let _ = v.set_flags(F_A);
        let _ = v.add_bytes(1, KFILE, &[0x80, 0x81]);
        let _ = v.feed(1);
        assert!(v.out_eq("M-^@M-^A"));
    }

    #[test]
    fn gates_follow_documented_order() {
        let mut v = Vis::new();
        assert_eq!(v.feed(0), Err(CE_EINVAL));
        assert_eq!(v.feed(5), Err(CE_ENOENT));
        let _ = v.add(1, KDIR, "");
        let _ = v.mark_busy(1);
        assert_eq!(v.feed(1), Err(CE_EISDIR));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
