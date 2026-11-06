//! UNX-F11531 字符逆序（0xf240 段）
//!
//! `rev` 的「字」有两种口径，本模块把两种都实现并显式区分：
//!
//! - **字模式**（`MODE_CHAR`）：按 UTF-8 码点逆序。`héllo -> olléh`，多字节
//!   字符整体搬动，不会把 `é` 的两字节拆开；
//! - **字节模式**（`MODE_BYTE`）：按字节逆序，但**只接受纯 ASCII**。理由很硬：
//!   把 0x80..0xFF 当字符塞进 `String` 会被重新编码成 `C2 xx`，输出与输入
//!   不再是同一串字节 —— 与其悄悄改名，不如直接 `RV_EBYTE` 拒绝。
//!
//! 行的切分与 `tac` 同构：按终止符切段、记录尾终止符，逆序只发生**段内**，
//! 行的先后次序不动。终止符可以是 `\n`，也可以是 `\0`（`rev -0`）；但必须是
//! ASCII —— 非 ASCII 终止符同样会被重编码，属参数错误。
//!
//! 解码分两关：先按首字节判长度（`0xC0/0xC1/0xF5..` 直接 `RV_ECHAR`），再对
//! 整行做一次 `from_utf8` 校验（续字节错、过长编码都落 `RV_EBYTE`）。两关分开，
//! 是为了让「首字节就不对」与「后续字节不对」在账本里可分辨。
//!
//! 闸门次序：
//!
//! - `open`：模式 → 空输入 → 终止符；
//! - `scan`：行数上限 → 单行长度；
//! - `render`：未扫描 → 逐行解码。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf240;

/// 参数非法（空输入）。
pub const RV_EINVAL: u32 = SEG_BASE + 0x01;
/// 非法 UTF-8 续字节或过长编码。
pub const RV_EBYTE: u32 = SEG_BASE + 0x02;
/// 非法首字节。
pub const RV_ECHAR: u32 = SEG_BASE + 0x03;
/// 终止符非法。
pub const RV_ETERM: u32 = SEG_BASE + 0x04;
/// 单行过长。
pub const RV_ELINE: u32 = SEG_BASE + 0x05;
/// 行数超限。
pub const RV_EFULL: u32 = SEG_BASE + 0x06;
/// 状态非法（未扫描或重复扫描）。
pub const RV_ESTATE: u32 = SEG_BASE + 0x07;
/// 模式非法。
pub const RV_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RV_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行数上限。
pub const CAP: usize = 12;
/// 单行长度上限。
pub const LINEMAX: usize = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 按码点逆序。
pub const MODE_CHAR: u8 = 1;
/// 按字节逆序（仅 ASCII）。
pub const MODE_BYTE: u8 = 2;

/// 操作码：单行。
pub const OP_LINE: u8 = 1;
/// 操作码：整段渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：计数。
pub const OP_COUNT: u8 = 3;
/// 操作码：观测。
pub const OP_STAT: u8 = 4;

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

/// 首字节对应的码点长度。
pub fn char_len(b: u8) -> Result<u8, u32> {
    match b {
        0x00..=0x7f => Ok(1),
        0xc2..=0xdf => Ok(2),
        0xe0..=0xef => Ok(3),
        0xf0..=0xf4 => Ok(4),
        _ => Err(RV_ECHAR),
    }
}

/// 单行逆序。
pub fn rev_line(line: &[u8], mode: u8) -> Result<String, u32> {
    if mode != MODE_CHAR && mode != MODE_BYTE {
        return Err(RV_EMODE);
    }
    if line.len() > LINEMAX {
        return Err(RV_ELINE);
    }
    let mut out = String::new();
    if mode == MODE_BYTE {
        let mut i = line.len();
        while i > 0 {
            i -= 1;
            match line.get(i) {
                Some(b) => {
                    if *b >= 0x80 {
                        return Err(RV_EBYTE);
                    }
                    out.push(*b as char);
                }
                None => break,
            }
        }
        return Ok(out);
    }
    let mut starts = [0u16; LINEMAX];
    let mut lens = [0u8; LINEMAX];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < line.len() {
        let b = match line.get(i) {
            Some(v) => *v,
            None => break,
        };
        let cl = char_len(b)?;
        let end = i + cl as usize;
        if end > line.len() {
            return Err(RV_EBYTE);
        }
        if let Some(dst) = starts.get_mut(n) {
            *dst = i as u16;
        }
        if let Some(dst) = lens.get_mut(n) {
            *dst = cl;
        }
        n += 1;
        i = end;
    }
    if core::str::from_utf8(line).is_err() {
        return Err(RV_EBYTE);
    }
    let mut k = n;
    while k > 0 {
        k -= 1;
        let s = match starts.get(k) {
            Some(v) => *v as usize,
            None => return Err(RV_EBYTE),
        };
        let l = match lens.get(k) {
            Some(v) => *v as usize,
            None => return Err(RV_EBYTE),
        };
        let slice = match line.get(s..s + l) {
            Some(v) => v,
            None => return Err(RV_EBYTE),
        };
        let text = match core::str::from_utf8(slice) {
            Ok(v) => v,
            Err(_) => return Err(RV_EBYTE),
        };
        out.push_str(text);
    }
    Ok(out)
}

/// 单行字符数（`MODE_BYTE` 下即字节数，仅 ASCII 合法）。
pub fn char_count(line: &[u8], mode: u8) -> Result<u32, u32> {
    if mode != MODE_CHAR && mode != MODE_BYTE {
        return Err(RV_EMODE);
    }
    if line.len() > LINEMAX {
        return Err(RV_ELINE);
    }
    if mode == MODE_BYTE {
        let mut i = 0usize;
        while i < line.len() {
            match line.get(i) {
                Some(b) => {
                    if *b >= 0x80 {
                        return Err(RV_EBYTE);
                    }
                }
                None => break,
            }
            i += 1;
        }
        return Ok(line.len() as u32);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < line.len() {
        let b = match line.get(i) {
            Some(v) => *v,
            None => break,
        };
        let cl = char_len(b)?;
        i += cl as usize;
        if i > line.len() {
            return Err(RV_EBYTE);
        }
        n = n.saturating_add(1);
    }
    if core::str::from_utf8(line).is_err() {
        return Err(RV_EBYTE);
    }
    Ok(n)
}

/// 逆序器。
#[derive(Clone, Copy, Debug)]
pub struct Rev<'a> {
    src: &'a [u8],
    term: u8,
    mode: u8,
    offs: [u32; CAP],
    ends: [u32; CAP],
    n: usize,
    tail: bool,
    scanned: bool,
    bytes: u32,
}

impl<'a> Rev<'a> {
    /// 开户：模式 → 空输入 → 终止符。
    pub fn open(src: &'a [u8], term: u8, mode: u8) -> Result<Rev<'a>, u32> {
        if mode != MODE_CHAR && mode != MODE_BYTE {
            return Err(RV_EMODE);
        }
        if src.is_empty() {
            return Err(RV_EINVAL);
        }
        if term >= 0x80 {
            return Err(RV_ETERM);
        }
        Ok(Rev {
            src,
            term,
            mode,
            offs: [0; CAP],
            ends: [0; CAP],
            n: 0,
            tail: false,
            scanned: false,
            bytes: 0,
        })
    }

    /// 模式。
    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// 终止符。
    pub fn term(&self) -> u8 {
        self.term
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已扫描。
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// 是否以终止符结尾。
    pub fn has_tail(&self) -> bool {
        self.tail
    }

    /// 累计字节。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 切分。
    pub fn scan(&mut self) -> Result<usize, u32> {
        if self.scanned {
            return Err(RV_ESTATE);
        }
        let mut start = 0usize;
        let mut i = 0usize;
        while i < self.src.len() {
            let b = match self.src.get(i) {
                Some(v) => *v,
                None => break,
            };
            if b == self.term {
                if i - start > LINEMAX {
                    return Err(RV_ELINE);
                }
                if self.n >= CAP {
                    return Err(RV_EFULL);
                }
                if let Some(dst) = self.offs.get_mut(self.n) {
                    *dst = start as u32;
                }
                if let Some(dst) = self.ends.get_mut(self.n) {
                    *dst = i as u32;
                }
                self.n += 1;
                start = i + 1;
            }
            i += 1;
        }
        self.tail = start == self.src.len();
        if start < self.src.len() {
            if self.src.len() - start > LINEMAX {
                return Err(RV_ELINE);
            }
            if self.n >= CAP {
                return Err(RV_EFULL);
            }
            if let Some(dst) = self.offs.get_mut(self.n) {
                *dst = start as u32;
            }
            if let Some(dst) = self.ends.get_mut(self.n) {
                *dst = self.src.len() as u32;
            }
            self.n += 1;
        }
        self.bytes = self.src.len() as u32;
        self.scanned = true;
        Ok(self.n)
    }

    /// 第 i 行（未逆序）。
    pub fn line(&self, i: usize) -> Result<&'a [u8], u32> {
        if !self.scanned {
            return Err(RV_ESTATE);
        }
        if i >= self.n {
            return Err(RV_ELINE);
        }
        let s = match self.offs.get(i) {
            Some(v) => *v as usize,
            None => return Err(RV_ELINE),
        };
        let e = match self.ends.get(i) {
            Some(v) => *v as usize,
            None => return Err(RV_ELINE),
        };
        match self.src.get(s..e) {
            Some(v) => Ok(v),
            None => Err(RV_ELINE),
        }
    }

    /// 第 i 行的逆序文本。
    pub fn line_rev(&self, i: usize) -> Result<String, u32> {
        match self.line(i) {
            Ok(v) => rev_line(v, self.mode),
            Err(c) => Err(c),
        }
    }

    /// 整段渲染（保留终止符与尾标志）。
    pub fn render(&self) -> Result<String, u32> {
        if !self.scanned {
            return Err(RV_ESTATE);
        }
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.n {
            let r = self.line_rev(i)?;
            out.push_str(r.as_str());
            if i + 1 < self.n || self.tail {
                out.push(self.term as char);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 逆序后的总字符数。
    pub fn chars(&self) -> Result<u32, u32> {
        if !self.scanned {
            return Err(RV_ESTATE);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let line = self.line(i)?;
            n = n.saturating_add(char_count(line, self.mode)?);
            i += 1;
        }
        Ok(n)
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<String, u32>` → 样本（观测值为字节长度）。
fn text_cell(slot: u8, op: u8, r: Result<String, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v.len() as u32 },
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
            return Err(RV_ESLOT);
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
            return Err(RV_EBAL);
        }
        const VALS: [u32; 8] = [3, 6, 5, 8, 3, 3, 5, 3];
        const CODES: [u32; 6] =
            [RV_EBYTE, RV_ESTATE, RV_ECHAR, RV_EINVAL, RV_EFULL, RV_ELINE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RV_EBAL),
            };
            if s.slot as usize != i {
                return Err(RV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RV_EVALUE);
                        }
                    }
                    None => return Err(RV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RV_EVALUE);
                        }
                    }
                    None => return Err(RV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RV_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RV_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(RV_ESEAL)
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

/// 十四格账本：字/字节两模式、多字节逆序、尾终止符与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(text_cell(0, OP_LINE, rev_line(b"abc", MODE_CHAR)));
    let _ = l.register(text_cell(1, OP_LINE, rev_line(b"h\xc3\xa9llo", MODE_CHAR)));
    let _ = l.register(word_cell(2, OP_COUNT, char_count(b"h\xc3\xa9llo", MODE_CHAR)));
    let _ = l.register(word_cell(3, OP_COUNT, char_count(b"h\xc3\xa9llo", MODE_BYTE)));

    let mut r = match Rev::open(b"abc\ndef\n", b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = r.scan();
    let _ = l.register(text_cell(4, OP_RENDER, r.render()));

    let late = match Rev::open(b"a\xc3(", b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(5, OP_RENDER, late.render().map(|v| v.len() as u32)));
    let _ = l.register(text_cell(6, OP_LINE, rev_line(b"\xff", MODE_CHAR)));
    let _ = l.register(word_cell(7, OP_STAT, Rev::open(b"", b'\n', MODE_CHAR).map(|_| 0)));

    let many = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n0\n1\n2\n3\n";
    let mut f = match Rev::open(many, b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(8, OP_STAT, f.scan().map(|v| v as u32)));

    let long = [b'a'; LINEMAX + 1];
    let mut g = match Rev::open(&long, b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(9, OP_STAT, g.scan().map(|v| v as u32)));

    let mut h = match Rev::open(b"ab\n", b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = h.scan();
    let _ = l.register(word_cell(10, OP_RENDER, h.render().map(|v| v.len() as u32)));

    let mut b = match Rev::open(b"ab\n", b'\n', MODE_BYTE) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = b.scan();
    let _ = l.register(text_cell(11, OP_RENDER, b.render()));

    let mut z = match Rev::open(b"ab\x00cd", 0, MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = z.scan();
    let _ = l.register(text_cell(12, OP_RENDER, z.render()));

    let _ = l.register(word_cell(13, OP_COUNT, char_count(b"abc", MODE_CHAR)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：ASCII 单行与整段逆序、尾终止符。
    let line = rev_line(b"abc", MODE_CHAR) == Ok(String::from("cba"));
    let mut r = match Rev::open(b"abc\ndef\n", b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let scan = r.scan() == Ok(2);
    let tail = r.has_tail();
    let render = r.render() == Ok(String::from("cba\nfed\n"));
    let chars = r.chars() == Ok(6);
    out[1] = line && scan && tail && render && chars && r.bytes() == 8;

    // 二路：多字节码点整体搬动，字节模式拒绝。
    let acc = rev_line(b"h\xc3\xa9llo", MODE_CHAR) == Ok(String::from("oll\u{e9}h"));
    let cnt = char_count(b"h\xc3\xa9llo", MODE_CHAR) == Ok(5)
        && char_count(b"h\xc3\xa9llo", MODE_BYTE) == Err(RV_EBYTE);
    let mut m = match Rev::open(b"h\xc3\xa9\n", b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = m.scan();
    let mr = m.render() == Ok(String::from("\u{e9}h\n"));
    let byte = rev_line(b"h\xc3\xa9llo", MODE_BYTE) == Err(RV_EBYTE);
    out[2] = acc && cnt && mr && byte;

    // 三路：七类拒绝与冻结重放。
    let echar = rev_line(b"\xff", MODE_CHAR) == Err(RV_ECHAR)
        && char_len(0xc0) == Err(RV_ECHAR)
        && char_len(0xf4) == Ok(4);
    let ebyte = rev_line(b"a\xc3(", MODE_CHAR) == Err(RV_EBYTE)
        && rev_line(b"a\xe0\x80", MODE_CHAR) == Err(RV_EBYTE);
    let einval = Rev::open(b"", b'\n', MODE_CHAR).is_err();
    let eterm = matches!(Rev::open(b"x", 0x80, MODE_CHAR), Err(RV_ETERM));
    let emode = matches!(Rev::open(b"x", b'\n', 9), Err(RV_EMODE)) && rev_line(b"x", 0) == Err(RV_EMODE);
    let long = [b'a'; LINEMAX + 1];
    let mut g = match Rev::open(&long, b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let eline = g.scan() == Err(RV_ELINE);
    let many = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n0\n1\n2\n3\n";
    let mut f = match Rev::open(many, b'\n', MODE_CHAR) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let efull = f.scan() == Err(RV_EFULL);
    let estate = f.render().is_err() || f.scan().is_err();

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = echar
        && ebyte
        && einval
        && eterm
        && emode
        && eline
        && efull
        && estate
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(RV_ESLOT)
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11531_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11531.rev_chars");

    s.add(
        "RV01-ASCII逆序",
        {
            rev_line(b"abc", MODE_CHAR) == Ok(String::from("cba"))
                && rev_line(b"", MODE_CHAR) == Ok(String::from(""))
                && rev_line(b"x", MODE_CHAR) == Ok(String::from("x"))
                && rev_line(b"abc", MODE_BYTE) == Ok(String::from("cba"))
        },
        "static",
    );

    s.add(
        "RV02-整段与尾标志",
        {
            let mut r = match Rev::open(b"abc\ndef\n", b'\n', MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let scan = r.scan() == Ok(2);
            scan && r.has_tail()
                && r.render() == Ok(String::from("cba\nfed\n"))
                && r.chars() == Ok(6)
                && r.bytes() == 8
                && r.len() == 2
        },
        "static",
    );

    s.add(
        "RV03-无尾终止符",
        {
            let mut r = match Rev::open(b"ab\ncd", b'\n', MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = r.scan();
            !r.has_tail() && r.render() == Ok(String::from("ba\ndc"))
        },
        "static",
    );

    s.add(
        "RV04-多字节搬动",
        {
            rev_line(b"h\xc3\xa9llo", MODE_CHAR) == Ok(String::from("oll\u{e9}h"))
                && char_count(b"h\xc3\xa9llo", MODE_CHAR) == Ok(5)
                && char_count(b"h\xc3\xa9llo", MODE_BYTE) == Err(RV_EBYTE)
                && char_len(b'a') == Ok(1)
                && char_len(0xc3) == Ok(2)
                && char_len(0xe9) == Ok(3)
                && char_len(0xf0) == Ok(4)
        },
        "static",
    );

    s.add(
        "RV05-字节模式仅ASCII",
        {
            rev_line(b"h\xc3\xa9llo", MODE_BYTE) == Err(RV_EBYTE)
                && char_count(b"h\xc3\xa9llo", MODE_BYTE) == Err(RV_EBYTE)
                && rev_line(b"xyz", MODE_BYTE) == Ok(String::from("zyx"))
                && Rev::open(b"x", b'\n', 9).is_err()
        },
        "static",
    );

    s.add(
        "RV06-终止符",
        {
            let mut z = match Rev::open(b"ab\x00cd", 0, MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = z.scan();
            z.term() == 0
                && z.render() == Ok(String::from("ba\x00dc"))
                && matches!(Rev::open(b"x", 0x80, MODE_CHAR), Err(RV_ETERM))
                && matches!(Rev::open(b"x", 0xff, MODE_CHAR), Err(RV_ETERM))
        },
        "static",
    );

    s.add(
        "RV07-空输入",
        {
            matches!(Rev::open(b"", b'\n', MODE_CHAR), Err(RV_EINVAL))
                && matches!(Rev::open(b"", b'\n', MODE_BYTE), Err(RV_EINVAL))
        },
        "static",
    );

    s.add(
        "RV08-首字节与续字节",
        {
            rev_line(b"\xff", MODE_CHAR) == Err(RV_ECHAR)
                && rev_line(b"\x80", MODE_CHAR) == Err(RV_ECHAR)
                && rev_line(b"a\xc3(", MODE_CHAR) == Err(RV_EBYTE)
                && char_len(0xc0) == Err(RV_ECHAR)
                && char_len(0xc1) == Err(RV_ECHAR)
                && char_len(0xf5) == Err(RV_ECHAR)
        },
        "static",
    );

    s.add(
        "RV09-行数上限",
        {
            let many = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n0\n1\n2\n3\n";
            let mut f = match Rev::open(many, b'\n', MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            f.scan() == Err(RV_EFULL) && f.len() < CAP + 1
        },
        "static",
    );

    s.add(
        "RV10-行宽与状态",
        {
            let long = [b'a'; LINEMAX + 1];
            let mut g = match Rev::open(&long, b'\n', MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let eline = g.scan() == Err(RV_ELINE);
            let mut h = match Rev::open(b"ab\n", b'\n', MODE_CHAR) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let estate = h.render() == Err(RV_ESTATE);
            let _ = h.scan();
            estate && h.scan() == Err(RV_ESTATE) && eline
        },
        "static",
    );

    s.add(
        "RV11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "RV12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(RV_EBAL)
                && c == Err(RV_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(RV_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "RV13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
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
    fn ascii_reversal() {
        assert_eq!(rev_line(b"abc", MODE_CHAR), Ok(String::from("cba")));
        assert_eq!(rev_line(b"", MODE_CHAR), Ok(String::from("")));
        assert_eq!(rev_line(b"abc", MODE_BYTE), Ok(String::from("cba")));
        assert_eq!(char_count(b"abc", MODE_CHAR), Ok(3));
    }

    #[test]
    fn multibyte_reversal() {
        assert_eq!(
            rev_line(b"h\xc3\xa9llo", MODE_CHAR),
            Ok(String::from("oll\u{e9}h"))
        );
        assert_eq!(char_count(b"h\xc3\xa9llo", MODE_CHAR), Ok(5));
        assert_eq!(char_count(b"h\xc3\xa9llo", MODE_BYTE), Err(RV_EBYTE));
        assert_eq!(rev_line(b"h\xc3\xa9llo", MODE_BYTE), Err(RV_EBYTE));
    }

    #[test]
    fn whole_source() {
        let mut r = Rev::open(b"abc\ndef\n", b'\n', MODE_CHAR).ok().unwrap();
        assert_eq!(r.scan(), Ok(2));
        assert!(r.has_tail());
        assert_eq!(r.render(), Ok(String::from("cba\nfed\n")));
        assert_eq!(r.bytes(), 8);

        let mut p = Rev::open(b"ab\ncd", b'\n', MODE_CHAR).ok().unwrap();
        assert_eq!(p.scan(), Ok(2));
        assert!(!p.has_tail());
        assert_eq!(p.render(), Ok(String::from("ba\ndc")));
    }

    #[test]
    fn nul_terminator() {
        let mut z = Rev::open(b"ab\x00cd", 0, MODE_CHAR).ok().unwrap();
        assert_eq!(z.scan(), Ok(2));
        assert_eq!(z.render(), Ok(String::from("ba\x00dc")));
        assert_eq!(Rev::open(b"x", 0x80, MODE_CHAR).err(), Some(RV_ETERM));
    }

    #[test]
    fn decode_errors() {
        assert_eq!(rev_line(b"\xff", MODE_CHAR), Err(RV_ECHAR));
        assert_eq!(rev_line(b"a\xc3(", MODE_CHAR), Err(RV_EBYTE));
        assert_eq!(char_len(0xc0), Err(RV_ECHAR));
        assert_eq!(char_len(0xf5), Err(RV_ECHAR));
        assert_eq!(Rev::open(b"", b'\n', MODE_CHAR).err(), Some(RV_EINVAL));
        assert_eq!(Rev::open(b"x", b'\n', 0).err(), Some(RV_EMODE));
    }

    #[test]
    fn limits_and_state() {
        let long = [b'a'; LINEMAX + 1];
        let mut g = Rev::open(&long, b'\n', MODE_CHAR).ok().unwrap();
        assert_eq!(g.scan(), Err(RV_ELINE));
        let many = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n0\n1\n2\n3\n";
        let mut f = Rev::open(many, b'\n', MODE_CHAR).ok().unwrap();
        assert_eq!(f.scan(), Err(RV_EFULL));
        let mut h = Rev::open(b"ab\n", b'\n', MODE_CHAR).ok().unwrap();
        assert_eq!(h.render(), Err(RV_ESTATE));
        assert_eq!(h.scan(), Ok(1));
        assert_eq!(h.scan(), Err(RV_ESTATE));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(RV_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(RV_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(RV_ESEAL));
    }
}
