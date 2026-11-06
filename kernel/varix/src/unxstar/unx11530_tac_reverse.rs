//! UNX-F11530 tac 逆序输出（0xf230 段）
//!
//! `tac` 不是把整个文件倒着吐字节，而是「按行切分后按行逆序」。两者在最后一行
//! 有没有换行符时给出完全不同的结果，因此本模块把行表与尾换行标志分开保存：
//!
//! - **行表**：`[off, end)` 半开区间，按出现顺序登记；
//! - **尾换行**：输入以分隔符结尾时为真，输出时也以分隔符结尾；否则输出末尾
//!   不带分隔符。
//!
//! 分隔符默认 `\n`，允许改成任意单字节（`-s`）。零字节分隔符非法：它会让
//! 「切分」与「空串」不可区分，属于参数错误而不是策略选择。
//!
//! 行表容量有限，超出即 `TC_EFULL`，不会静默丢弃尾部行。行内容必须是合法
//! UTF-8 才能拼成输出串；含非法字节时报 `TC_EBYTE`，而不是替换成 U+FFFD ——
//! 替换会让「转储」变成「改写」。
//!
//! 闸门次序：
//!
//! - `open`：分隔符为零；
//! - `scan`：重复扫描；
//! - `render`：尚未扫描 → 行号越界 → 非法 UTF-8。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf230;

/// 参数非法。
pub const TC_EINVAL: u32 = SEG_BASE + 0x01;
/// 分隔符为零。
pub const TC_ESEP: u32 = SEG_BASE + 0x02;
/// 行表满。
pub const TC_EFULL: u32 = SEG_BASE + 0x03;
/// 尚未扫描或重复扫描。
pub const TC_EREADY: u32 = SEG_BASE + 0x04;
/// 行号越界。
pub const TC_ELINE: u32 = SEG_BASE + 0x05;
/// 行内容不是合法 UTF-8。
pub const TC_EBYTE: u32 = SEG_BASE + 0x06;
/// 空输入无可逆序。
pub const TC_EMPTY: u32 = SEG_BASE + 0x07;
/// 尾换行状态非法。
pub const TC_ETAIL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：扫描。
pub const OP_SCAN: u8 = 1;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：取行。
pub const OP_LINE: u8 = 3;
/// 操作码：统计。
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

/// 行表切分器。
#[derive(Clone, Copy, Debug)]
pub struct Tac<'a> {
    src: &'a [u8],
    sep: u8,
    offs: [u32; CAP],
    ends: [u32; CAP],
    n: usize,
    tail: bool,
    scanned: bool,
    bytes: u32,
}

impl<'a> Tac<'a> {
    /// 开户。
    pub fn open(src: &'a [u8], sep: u8) -> Result<Tac<'a>, u32> {
        if sep == 0 {
            return Err(TC_ESEP);
        }
        Ok(Tac {
            src,
            sep,
            offs: [0; CAP],
            ends: [0; CAP],
            n: 0,
            tail: false,
            scanned: false,
            bytes: 0,
        })
    }

    /// 分隔符。
    pub fn sep(&self) -> u8 {
        self.sep
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

    /// 输入是否以分隔符结尾。
    pub fn has_tail(&self) -> bool {
        self.tail
    }

    /// 累计字节数。
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// 切分。
    pub fn scan(&mut self) -> Result<usize, u32> {
        if self.scanned {
            return Err(TC_EREADY);
        }
        if self.src.is_empty() {
            self.scanned = true;
            return Err(TC_EMPTY);
        }
        let mut start = 0usize;
        let mut i = 0usize;
        while i < self.src.len() {
            let b = match self.src.get(i) {
                Some(v) => *v,
                None => break,
            };
            if b == self.sep {
                if self.n >= CAP {
                    return Err(TC_EFULL);
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
            if self.n >= CAP {
                return Err(TC_EFULL);
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

    /// 第 i 行（按出现顺序）。
    pub fn line(&self, i: usize) -> Result<&'a [u8], u32> {
        if !self.scanned {
            return Err(TC_EREADY);
        }
        if i >= self.n {
            return Err(TC_ELINE);
        }
        let s = match self.offs.get(i) {
            Some(v) => *v as usize,
            None => return Err(TC_ELINE),
        };
        let e = match self.ends.get(i) {
            Some(v) => *v as usize,
            None => return Err(TC_ELINE),
        };
        match self.src.get(s..e) {
            Some(v) => Ok(v),
            None => Err(TC_ELINE),
        }
    }

    /// 按逆序拼接。
    pub fn render(&self) -> Result<String, u32> {
        if !self.scanned {
            return Err(TC_EREADY);
        }
        if self.n == 0 {
            return Err(TC_EMPTY);
        }
        let mut out = String::new();
        let mut k = self.n;
        while k > 0 {
            k -= 1;
            let line = self.line(k)?;
            let text = match core::str::from_utf8(line) {
                Ok(v) => v,
                Err(_) => return Err(TC_EBYTE),
            };
            out.push_str(text);
            if k > 0 || self.tail {
                out.push(self.sep as char);
            }
        }
        Ok(out)
    }

    /// 逆序后的行数（恒等于行数）。
    pub fn reversed_len(&self) -> Result<usize, u32> {
        if !self.scanned {
            return Err(TC_EREADY);
        }
        Ok(self.n)
    }

    /// 行长度。
    pub fn line_len(&self, i: usize) -> Result<u32, u32> {
        match self.line(i) {
            Ok(v) => Ok(v.len() as u32),
            Err(c) => Err(c),
        }
    }

    /// 尾换行标志取反后重建输出（用于 `tac -b` 的「前置分隔符」形态）。
    pub fn retail(&self, want: bool) -> Result<u32, u32> {
        if !self.scanned {
            return Err(TC_EREADY);
        }
        if want == self.tail {
            return Err(TC_ETAIL);
        }
        Ok(if want { 1 } else { 0 })
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

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
            return Err(TC_ESLOT);
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
            return Err(TC_EBAL);
        }
        const VALS: [u32; 8] = [3, 3, 5, 3, 2, 0, 2, 2];
        const CODES: [u32; 6] = [TC_ESEP, TC_EREADY, TC_EMPTY, TC_ELINE, TC_EBYTE, TC_ETAIL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TC_EBAL),
            };
            if s.slot as usize != i {
                return Err(TC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TC_EVALUE);
                        }
                    }
                    None => return Err(TC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TC_EVALUE);
                        }
                    }
                    None => return Err(TC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TC_EBAL);
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
            return Err(TC_EBAL);
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
            Err(TC_ESEAL)
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

/// 十四格账本：三行两行切分、渲染、尾换行与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let text = b"one\ntwo\nthree\n";

    let mut t = match Tac::open(text, b'\n') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(0, OP_SCAN, t.scan()));
    let _ = l.register(word_cell(1, OP_LINE, t.line_len(0)));
    let _ = l.register(word_cell(2, OP_LINE, t.line_len(2)));
    let _ = l.register(word_cell(3, OP_STAT, t.reversed_len().map(|v| v as u32)));

    let mut plain = match Tac::open(b"one\ntwo", b'\n') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(4, OP_SCAN, plain.scan()));
    let _ = l.register(word_cell(5, OP_STAT, Ok(if plain.has_tail() { 1 } else { 0 })));
    let _ = l.register(word_cell(6, OP_STAT, plain.reversed_len().map(|v| v as u32)));

    let mut alt = match Tac::open(b"a:b", b':') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(count_cell(7, OP_SCAN, alt.scan()));

    let _ = l.register(Sample {
        slot: 8,
        op: OP_SCAN,
        ok: false,
        code: match Tac::open(b"x", 0) {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let late = match Tac::open(b"x", b'\n') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(Sample {
        slot: 9,
        op: OP_RENDER,
        ok: false,
        code: match late.render() {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let mut empty = match Tac::open(b"", b'\n') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(Sample {
        slot: 10,
        op: OP_SCAN,
        ok: false,
        code: match empty.scan() {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let _ = l.register(Sample {
        slot: 11,
        op: OP_LINE,
        ok: false,
        code: match t.line_len(9) {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let mut bad = match Tac::open(b"a\xffb\n", b'\n') {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = bad.scan();
    let _ = l.register(Sample {
        slot: 12,
        op: OP_RENDER,
        ok: false,
        code: match bad.render() {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let _ = l.register(Sample {
        slot: 13,
        op: OP_STAT,
        ok: false,
        code: match t.retail(t.has_tail()) {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三行带尾换行的逆序渲染。
    let mut a = match Tac::open(b"one\ntwo\nthree\n", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let scan = a.scan() == Ok(3);
    let tail = a.has_tail();
    let render = a.render() == Ok(String::from("three\ntwo\none\n"));
    let l0 = a.line(0) == Ok(&b"one"[..]);
    let l2 = a.line(2) == Ok(&b"three"[..]);

    // 二路：无尾换行与备用分隔符。
    let mut b = match Tac::open(b"one\ntwo", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let b_scan = b.scan() == Ok(2);
    let b_tail = !b.has_tail() && b.render() == Ok(String::from("two\none"));

    let mut c = match Tac::open(b"a:b:c", b':') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let c_scan = c.scan() == Ok(3);
    let c_render = c.render() == Ok(String::from("c:b:a"));
    out[1] = scan && tail && render && l0 && l2 && b_scan && b_tail && c_scan && c_render;

    // 三路：空输入、非法字节、行号越界。
    let mut e = match Tac::open(b"", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let empty = e.scan() == Err(TC_EMPTY) && e.render() == Err(TC_EMPTY);

    let mut bad = match Tac::open(b"a\xffb\n", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = bad.scan();
    let byte = bad.render() == Err(TC_EBYTE);

    let mut far = match Tac::open(b"a\nb\n", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = far.scan();
    let line = far.line(5) == Err(TC_ELINE) && far.line_len(1) == Ok(1);
    out[2] = empty && byte && line;

    // 四路：分隔符与重复扫描闸门、尾换行取反、冻结。
    let sep0 = matches!(Tac::open(b"x", 0), Err(TC_ESEP));
    let mut twice = match Tac::open(b"a\nb\n", b'\n') {
        Ok(v) => v,
        Err(_) => return out,
    };
    let first = twice.scan().is_ok();
    let second = twice.scan() == Err(TC_EREADY);
    let flip = twice.retail(!twice.has_tail());
    let same = twice.retail(twice.has_tail()) == Err(TC_ETAIL);

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = sep0
        && first
        && second
        && flip.is_ok()
        && same
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(TC_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11530_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11530.tac_reverse");

    s.add(
        "TC01-三行切分",
        {
            let mut t = match Tac::open(b"one\ntwo\nthree\n", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.scan() == Ok(3)
                && t.line(0) == Ok(&b"one"[..])
                && t.line(1) == Ok(&b"two"[..])
                && t.line(2) == Ok(&b"three"[..])
                && t.has_tail()
        },
        "static",
    );

    s.add(
        "TC02-逆序渲染",
        {
            let mut t = match Tac::open(b"one\ntwo\nthree\n", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = t.scan();
            t.render() == Ok(String::from("three\ntwo\none\n"))
                && t.reversed_len() == Ok(3)
                && t.bytes() == 14
        },
        "static",
    );

    s.add(
        "TC03-无尾换行",
        {
            let mut t = match Tac::open(b"one\ntwo", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.scan() == Ok(2)
                && !t.has_tail()
                && t.render() == Ok(String::from("two\none"))
        },
        "static",
    );

    s.add(
        "TC04-备用分隔符",
        {
            let mut t = match Tac::open(b"a:b:c", b':') {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.scan() == Ok(3)
                && t.sep() == b':'
                && t.render() == Ok(String::from("c:b:a"))
                && t.line(1) == Ok(&b"b"[..])
        },
        "static",
    );

    s.add(
        "TC05-单行输入",
        {
            let mut t = match Tac::open(b"solo", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.scan() == Ok(1) && !t.has_tail() && t.render() == Ok(String::from("solo"))
        },
        "static",
    );

    s.add(
        "TC06-行表容量",
        {
            let mut t = match Tac::open(b"1\n2\n3\n4\n5\n6\n7\n8\n9\n", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.scan() == Err(TC_EFULL) && t.len() == CAP
        },
        "static",
    );

    s.add(
        "TC07-空输入与零分隔符",
        {
            let mut e = match Tac::open(b"", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(Tac::open(b"x", 0), Err(TC_ESEP))
                && e.scan() == Err(TC_EMPTY)
                && e.render() == Err(TC_EMPTY)
        },
        "static",
    );

    s.add(
        "TC08-非法字节拒绝",
        {
            let mut t = match Tac::open(b"a\xffb\n", b'\n') {
                Ok(v) => v,
                Err(_) => return s,
            };
            let scan = t.scan() == Ok(1);
            scan && t.render() == Err(TC_EBYTE) && t.line(0).map(|l| l.len()) == Ok(3)
        },
        "static",
    );

    s.add(
        "TC09-账本审计",
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
        "TC10-账本错误行码序",
        {
            let l = build_ledger();
            let mut got = [0u32; 6];
            let mut ci = 0usize;
            let mut i = 0usize;
            let mut good = true;
            while i < l.len() {
                match l.sample(i) {
                    Some(smp) => {
                        if !smp.ok {
                            if let Some(dst) = got.get_mut(ci) {
                                *dst = smp.code;
                            }
                            ci += 1;
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good && ci == 6 && got == [TC_ESEP, TC_EREADY, TC_EMPTY, TC_ELINE, TC_EBYTE, TC_ETAIL]
        },
        "static",
    );

    s.add(
        "TC11-冻结与拒绝计数",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            a.is_ok() && b == Err(TC_EBAL) && c == Err(TC_ESLOT) && l.rejects() == 2
        },
        "static",
    );

    s.add(
        "TC12-重放指纹一致",
        {
            let l = build_ledger();
            let a = l.seal();
            let mut f = build_ledger();
            let b = f.freeze();
            a != 0 && l.replay() == Ok(a) && b == Ok(a) && f.root() == a
        },
        "static",
    );

    s.add(
        "TC13-冻结重放四注入",
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
    fn split_and_reverse() {
        let mut t = Tac::open(b"one\ntwo\nthree\n", b'\n').ok().unwrap();
        assert_eq!(t.scan(), Ok(3));
        assert!(t.has_tail());
        assert_eq!(t.line(0), Ok(&b"one"[..]));
        assert_eq!(t.render(), Ok(String::from("three\ntwo\none\n")));
    }

    #[test]
    fn no_tail_newline() {
        let mut t = Tac::open(b"one\ntwo", b'\n').ok().unwrap();
        assert_eq!(t.scan(), Ok(2));
        assert!(!t.has_tail());
        assert_eq!(t.render(), Ok(String::from("two\none")));
    }

    #[test]
    fn alternate_separator() {
        let mut t = Tac::open(b"a:b:c", b':').ok().unwrap();
        assert_eq!(t.scan(), Ok(3));
        assert_eq!(t.render(), Ok(String::from("c:b:a")));
    }

    #[test]
    fn capacity_and_empty() {
        let mut big = Tac::open(b"1\n2\n3\n4\n5\n6\n7\n8\n9\n", b'\n').ok().unwrap();
        assert_eq!(big.scan(), Err(TC_EFULL));
        assert_eq!(big.len(), CAP);
        let mut e = Tac::open(b"", b'\n').ok().unwrap();
        assert_eq!(e.scan(), Err(TC_EMPTY));
        assert_eq!(Tac::open(b"x", 0).err(), Some(TC_ESEP));
    }

    #[test]
    fn invalid_bytes_and_lines() {
        let mut t = Tac::open(b"a\xffb\n", b'\n').ok().unwrap();
        assert_eq!(t.scan(), Ok(1));
        assert_eq!(t.render(), Err(TC_EBYTE));
        let mut f = Tac::open(b"a\nb\n", b'\n').ok().unwrap();
        assert_eq!(f.scan(), Ok(2));
        assert_eq!(f.line(5), Err(TC_ELINE));
        assert_eq!(f.scan(), Err(TC_EREADY));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(TC_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(TC_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(TC_ESEAL));
    }
}
