//! UNX-F11523 cksum/sum POSIX 校验
//!
//! 三种「老派校验和」在同一张卡上对照，因为它们经常被同一个脚本混用：
//!
//! - **`cksum`**：POSIX 规定的 CRC-32，多项式 `0x04C11DB7`、**高位先行**、初值
//!   0、不反射，算完正文后还要把**文件长度**按小端逐字节再喂一遍，最后取反。
//!   长度参与运算是它和 CRC-32/ISO-HDLC 最大的不同——同样内容、不同长度必然给
//!   出不同结果，这也让它天生抗「尾部截断」；
//! - **`sum -r`（BSD）**：每字节先把 16 位累加器右旋一位、再加上该字节，结果留在
//!   16 位里。它是个旋转校验和，字节序变化不会简单抵消；
//! - **`sum -s`（SysV）**：纯字节和，但用 **16 位末端进位**折叠
//!   （`s = (s & 0xffff) + (s >> 16)`），所以 65536 字节的和不等于 0。
//!
//! 三者都带一个**块数**（1024 字节一块，向上取整）。校验行是
//! `<校验和> <块数> <名字>`，字段之间一个空格，名字可以含空格（取到行尾）。
//!
//! 闸门次序：算法名（`CK_EALG`）→ 多项式（`CK_EPOLY`）→ 容量（`CK_ECAP`）→
//! 行字段（`CK_EFMT`）→ 十进制（`CK_EDEC`）→ 校验和（`CK_ESUM`）→ 块数
//! （`CK_ESIZE`）。多项式在容量之前：多项式非法时，算多少字节都是错的。
//!
//! 多项式闸门认两种写法：显式带首项的 33 位多项式（`0x8000_0000` 置位），或
//! 标准 CRC-32 的 `0x04C11DB7`——后者的 x^32 首项在 32 位寄存器表示里是隐式的，
//! 表驱动移位照样收敛。其余（含 0）一律拒绝。
//!
//! 账本语义：十四格 = 八格成功（三个固定向量、两种 sum、两档块数、名字长）+
//! 六格失败（未知算法、坏多项式、超容量、字段数、十进制、块数不符）。`audit`
//! 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1c0;

/// 算法名未知。
pub const CK_EALG: u32 = SEG_BASE + 0x01;
/// 多项式非法。
pub const CK_EPOLY: u32 = SEG_BASE + 0x02;
/// 输入越过一次可算容量。
pub const CK_ECAP: u32 = SEG_BASE + 0x03;
/// 校验行字段数不对。
pub const CK_EFMT: u32 = SEG_BASE + 0x04;
/// 十进制字段非法。
pub const CK_EDEC: u32 = SEG_BASE + 0x05;
/// 校验和与实测不符。
pub const CK_ESUM: u32 = SEG_BASE + 0x06;
/// 块数与实测不符。
pub const CK_ESIZE: u32 = SEG_BASE + 0x07;
/// 同名重复校验。
pub const CK_EDUP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CK_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CK_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CK_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CK_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CK_ENOBS: u32 = SEG_BASE + 0x0d;

/// POSIX cksum。
pub const ALG_CRC: u8 = 0;
/// BSD `sum -r`。
pub const ALG_BSD: u8 = 1;
/// SysV `sum -s`。
pub const ALG_SYSV: u8 = 2;

/// CRC 多项式（高位先行）。
pub const POLY: u32 = 0x04c1_1db7;
/// 块大小。
pub const BLOCK: usize = 1024;
/// 一次可算字节上限。
pub const CAP: usize = 4096;
/// 校验清单容量。
pub const CHECKS: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：计算。
pub const OP_CALC: u8 = 1;
/// 操作码：解析。
pub const OP_PARSE: u8 = 2;
/// 操作码：核对。
pub const OP_VERIFY: u8 = 3;

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

/// 按多项式造 256 项表。
const fn make_table(poly: u32) -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ poly
            } else {
                c << 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

/// 标准多项式表。
static TABLE: [u32; 256] = make_table(POLY);

/// 一次计算的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// 校验和。
    pub sum: u32,
    /// 块数（1024 字节一块，向上取整）。
    pub blocks: u32,
    /// 字节数。
    pub len: u32,
}

/// 算法名 → 编号。
pub fn by_name(name: &str) -> Result<u8, u32> {
    match name {
        "cksum" | "crc" => Ok(ALG_CRC),
        "sum" | "bsd" | "sum-r" => Ok(ALG_BSD),
        "sysv" | "sum-s" => Ok(ALG_SYSV),
        _ => Err(CK_EALG),
    }
}

/// 块数：向上取整，零字节为零块。
pub fn blocks(len: usize) -> u32 {
    let mut n = len / BLOCK;
    if len % BLOCK != 0 {
        n += 1;
    }
    n as u32
}

/// 喂一段字节（表驱动，高位先行）。
fn feed(mut c: u32, data: &[u8], t: &[u32; 256]) -> u32 {
    let mut i = 0usize;
    while i < data.len() {
        let b = match data.get(i) {
            Some(v) => *v,
            None => break,
        };
        let idx = (((c >> 24) ^ b as u32) & 0xff) as usize;
        let tv = match t.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        c = (c << 8) ^ tv;
        i += 1;
    }
    c
}

/// 多项式闸门：显式首项，或标准 CRC-32 多项式（隐式 x^32）。
pub fn poly_ok(poly: u32) -> bool {
    poly == POLY || poly & 0x8000_0000 != 0
}

/// 裸 CRC（不含长度、不取反）。
pub fn crc_raw(data: &[u8], poly: u32) -> Result<u32, u32> {
    if !poly_ok(poly) {
        return Err(CK_EPOLY);
    }
    if data.len() > CAP {
        return Err(CK_ECAP);
    }
    let t = if poly == POLY {
        TABLE
    } else {
        make_table(poly)
    };
    Ok(feed(0, data, &t))
}

/// POSIX cksum：正文 + 小端长度（至少一字节）+ 取反。
pub fn cksum(data: &[u8], poly: u32) -> Result<u32, u32> {
    if !poly_ok(poly) {
        return Err(CK_EPOLY);
    }
    if data.len() > CAP {
        return Err(CK_ECAP);
    }
    let t = if poly == POLY {
        TABLE
    } else {
        make_table(poly)
    };
    let mut c = feed(0, data, &t);
    // 长度按小端逐字节喂入；零字节的输入喂一个 0，保证「长度参与运算」始终成立。
    let mut n = data.len();
    if n == 0 {
        c = feed(c, &[0], &t);
    } else {
        while n > 0 {
            c = feed(c, &[(n & 0xff) as u8], &t);
            n >>= 8;
        }
    }
    Ok(!c)
}

/// BSD `sum -r`：右旋一位后加字节。
pub fn bsd(data: &[u8]) -> Result<u32, u32> {
    if data.len() > CAP {
        return Err(CK_ECAP);
    }
    let mut s = 0u32;
    let mut i = 0usize;
    while i < data.len() {
        let b = match data.get(i) {
            Some(v) => *v as u32,
            None => break,
        };
        s = (s >> 1) | ((s & 1) << 15);
        s = s.wrapping_add(b) & 0xffff;
        i += 1;
    }
    Ok(s)
}

/// SysV `sum -s`：16 位末端进位折叠。
pub fn sysv(data: &[u8]) -> Result<u32, u32> {
    if data.len() > CAP {
        return Err(CK_ECAP);
    }
    let mut s = 0u32;
    let mut i = 0usize;
    while i < data.len() {
        let b = match data.get(i) {
            Some(v) => *v as u32,
            None => break,
        };
        s = s.wrapping_add(b);
        s = (s & 0xffff) + (s >> 16);
        i += 1;
    }
    Ok(s & 0xffff)
}

/// 按算法一次算完。
pub fn calc(alg: u8, data: &[u8]) -> Result<Record, u32> {
    if data.len() > CAP {
        return Err(CK_ECAP);
    }
    let sum = match alg {
        ALG_CRC => cksum(data, POLY)?,
        ALG_BSD => bsd(data)?,
        ALG_SYSV => sysv(data)?,
        _ => return Err(CK_EALG),
    };
    Ok(Record { sum, blocks: blocks(data.len()), len: data.len() as u32 })
}

/// 解析后的校验行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    /// 声明的校验和。
    pub sum: u32,
    /// 声明的块数。
    pub blocks: u32,
    /// 名字（可含空格）。
    pub name: &'static str,
}

/// 十进制解析（不认符号、不认空）。
fn dec(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(CK_EDEC);
    }
    let b = s.as_bytes();
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(CK_EDEC),
        };
        if !c.is_ascii_digit() {
            return Err(CK_EDEC);
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(CK_EDEC),
        };
        v = match v.checked_add((c - b'0') as u32) {
            Some(x) => x,
            None => return Err(CK_EDEC),
        };
        i += 1;
    }
    Ok(v)
}

/// 解析 `<校验和> <块数> <名字>`。
pub fn parse_line(line: &'static str) -> Result<Line, u32> {
    let b = line.as_bytes();
    let mut s1 = 0usize;
    while s1 < b.len() && b.get(s1) != Some(&b' ') {
        s1 += 1;
    }
    if s1 == 0 || s1 >= b.len() {
        return Err(CK_EFMT);
    }
    let mut s2 = s1 + 1;
    while s2 < b.len() && b.get(s2) != Some(&b' ') {
        s2 += 1;
    }
    if s2 >= b.len() {
        return Err(CK_EFMT);
    }
    let f1 = match line.get(..s1) {
        Some(v) => v,
        None => return Err(CK_EFMT),
    };
    let f2 = match line.get(s1 + 1..s2) {
        Some(v) => v,
        None => return Err(CK_EFMT),
    };
    let name = match line.get(s2 + 1..) {
        Some(v) => v,
        None => return Err(CK_EFMT),
    };
    if name.is_empty() {
        return Err(CK_EFMT);
    }
    Ok(Line { sum: dec(f1)?, blocks: dec(f2)?, name })
}

/// 核对一行的算法结论。
pub fn verify(alg: u8, line: &'static str, data: &[u8]) -> Result<(), u32> {
    let l = parse_line(line)?;
    let r = calc(alg, data)?;
    if r.sum != l.sum {
        return Err(CK_ESUM);
    }
    if r.blocks != l.blocks {
        return Err(CK_ESIZE);
    }
    Ok(())
}

/// 校验清单。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    alg: u8,
    names: [&'static str; CHECKS],
    n: usize,
    ok: usize,
}

impl Checker {
    /// 指定算法的空清单。
    pub const fn new(alg: u8) -> Checker {
        Checker { alg, names: [""; CHECKS], n: 0, ok: 0 }
    }

    /// 算法。
    pub fn alg(&self) -> u8 {
        self.alg
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 通过条数。
    pub fn ok_count(&self) -> usize {
        self.ok
    }

    /// 核对一行。
    pub fn check(&mut self, line: &'static str, data: &[u8]) -> Result<(), u32> {
        let l = parse_line(line)?;
        let mut i = 0usize;
        while i < self.n {
            if self.names.get(i) == Some(&l.name) {
                return Err(CK_EDUP);
            }
            i += 1;
        }
        if self.n >= CHECKS {
            return Err(CK_ECAP);
        }
        verify(self.alg, line, data)?;
        if let Some(dst) = self.names.get_mut(self.n) {
            *dst = l.name;
        }
        self.n += 1;
        self.ok += 1;
        Ok(())
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
            return Err(CK_ESLOT);
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

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CK_EBAL);
        }
        const VALS: [u32; 8] =
            [4_294_967_295, 1_219_131_554, 930_766_865, 16_556, 294, 2, 3, 0];
        const CODES: [u32; 6] =
            [CK_EALG, CK_EPOLY, CK_ECAP, CK_EFMT, CK_EDEC, CK_ESIZE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CK_EBAL),
            };
            if s.slot as usize != i {
                return Err(CK_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CK_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CK_EVALUE);
                        }
                    }
                    None => return Err(CK_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CK_EVALUE);
                        }
                    }
                    None => return Err(CK_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CK_EBAL);
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
            return Err(CK_EBAL);
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
            Err(CK_ESEAL)
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

/// "abc" 的 POSIX 校验行。
const LABC: &str = "1219131554 1 abc";
/// "abc" 的裸 CRC（不含长度、不取反）。
const CRC_ABC: u32 = 0x2c17_398c;

/// 四 KB 以上的输入，用于顶容量闸门。
const BIG: [u8; CAP + 1] = [0x61; CAP + 1];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_CALC, cksum(b"", POLY)));
    let _ = l.register(word_cell(1, OP_CALC, cksum(b"abc", POLY)));
    let _ = l.register(word_cell(2, OP_CALC, cksum(b"123456789", POLY)));
    let _ = l.register(word_cell(3, OP_CALC, bsd(b"abc")));
    let _ = l.register(word_cell(4, OP_CALC, sysv(b"abc")));
    let _ = l.register(word_cell(5, OP_CALC, Ok(blocks(BLOCK + 1))));
    let _ = l.register(word_cell(6, OP_PARSE, parse_line(LABC).map(|x| x.name.len() as u32)));
    let _ = l.register(word_cell(7, OP_CALC, Ok(blocks(0))));

    let _ = l.register(word_cell(8, OP_CALC, by_name("md5").map(|a| a as u32)));
    let _ = l.register(word_cell(9, OP_CALC, crc_raw(b"abc", 0x7fff_ffff)));
    let _ = l.register(word_cell(10, OP_CALC, calc(ALG_CRC, &BIG).map(|r| r.sum)));
    let _ = l.register(word_cell(11, OP_PARSE, {
        let bad: &'static str = "1219131554 abc";
        parse_line(bad).map(|x| x.name.len() as u32)
    }));
    let _ = l.register(word_cell(12, OP_PARSE, {
        let bad: &'static str = "1219131554x 1 abc";
        parse_line(bad).map(|x| x.name.len() as u32)
    }));
    let _ = l.register(word_cell(13, OP_VERIFY, {
        let bad: &'static str = "1219131554 2 abc";
        verify(ALG_CRC, bad, b"abc").map(|_| 0)
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三个 POSIX 固定向量。
    out[1] = cksum(b"", POLY) == Ok(4_294_967_295)
        && cksum(b"abc", POLY) == Ok(1_219_131_554)
        && cksum(b"123456789", POLY) == Ok(930_766_865)
        && crc_raw(b"", POLY) == Ok(0)
        && crc_raw(b"abc", POLY) == Ok(CRC_ABC);

    // 二路：两种 sum 与块数。
    out[2] = bsd(b"abc") == Ok(16_556)
        && sysv(b"abc") == Ok(294)
        && blocks(0) == 0
        && blocks(BLOCK) == 1
        && blocks(BLOCK + 1) == 2
        && calc(ALG_SYSV, b"abc").map(|r| (r.sum, r.blocks, r.len)) == Ok((294, 1, 3));

    // 三路：长度参与 CRC。
    out[3] = cksum(b"ab", POLY) != cksum(b"abc", POLY)
        && cksum(b"abc", POLY) != cksum(b"abcd", POLY)
        && calc(ALG_CRC, &BIG) == Err(CK_ECAP)
        && crc_raw(b"abc", 0) == Err(CK_EPOLY)
        && by_name("sum -s") == Err(CK_EALG)
        && by_name("sysv") == Ok(ALG_SYSV)
        && parse_line("1219131554 1 a b") == Ok(Line { sum: 1_219_131_554, blocks: 1, name: "a b" })
        && verify(ALG_CRC, LABC, b"abc") == Ok(())
        && verify(ALG_CRC, LABC, b"abd") == Err(CK_ESUM)
        && {
            let mut c = Checker::new(ALG_CRC);
            c.check(LABC, b"abc").is_ok() && c.check(LABC, b"abc") == Err(CK_EDUP)
        };

    out
}

/// 十三项判据。
pub fn run_unx11523_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11523.cksum_posix");
    let l = build_ledger();

    s.add_dyn(
        "CK1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CK2-POSIX固定向量",
        cksum(b"", POLY) == Ok(4_294_967_295)
            && cksum(b"abc", POLY) == Ok(1_219_131_554)
            && cksum(b"123456789", POLY) == Ok(930_766_865)
            && crc_raw(b"", POLY) == Ok(0)
            && l.sample(0).map(|x| x.value) == Some(4_294_967_295)
            && l.sample(1).map(|x| x.value) == Some(1_219_131_554)
            && l.sample(2).map(|x| x.value) == Some(930_766_865),
        "static",
    );

    s.add(
        "CK3-BSD旋转和",
        bsd(b"abc") == Ok(16_556)
            && bsd(b"") == Ok(0)
            && bsd(b"a") == Ok(97)
            && bsd(b"123456789") == Ok(53_615)
            && l.sample(3).map(|x| x.value) == Some(16_556),
        "static",
    );

    s.add(
        "CK4-SysV折叠和",
        sysv(b"abc") == Ok(294)
            && sysv(b"") == Ok(0)
            && sysv(b"123456789") == Ok(477)
            && sysv(&[0xff; 257]) == Ok(0xffff)
            && l.sample(4).map(|x| x.value) == Some(294),
        "static",
    );

    s.add(
        "CK5-块数向上取整",
        blocks(0) == 0
            && blocks(1) == 1
            && blocks(BLOCK) == 1
            && blocks(BLOCK + 1) == 2
            && calc(ALG_BSD, b"").map(|r| r.blocks) == Ok(0)
            && l.sample(5).map(|x| x.value) == Some(2)
            && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "CK6-长度参与CRC",
        crc_raw(b"abc", POLY) != cksum(b"abc", POLY)
            && cksum(b"abc", POLY) != cksum(b"abcd", POLY)
            && cksum(b"ab", POLY) != cksum(b"abc", POLY)
            && crc_raw(b"abc", POLY) == Ok(CRC_ABC),
        "static",
    );

    s.add(
        "CK7-校验行解析",
        parse_line(LABC) == Ok(Line { sum: 1_219_131_554, blocks: 1, name: "abc" })
            && parse_line("0 0 empty") == Ok(Line { sum: 0, blocks: 0, name: "empty" })
            && parse_line("930766865 2 a b c")
                == Ok(Line { sum: 930_766_865, blocks: 2, name: "a b c" })
            && l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "CK8-十进制闸门",
        {
            let sign: &'static str = "-1 1 abc";
            let alpha: &'static str = "12x 1 abc";
            let big: &'static str = "4294967296 1 abc";
            let empty: &'static str = " 1 abc";
            dec("0") == Ok(0)
                && dec("4294967295") == Ok(4_294_967_295)
                && parse_line(sign) == Err(CK_EDEC)
                && parse_line(alpha) == Err(CK_EDEC)
                && parse_line(big) == Err(CK_EDEC)
                && parse_line(empty) == Err(CK_EFMT)
                && l.sample(12).map(|x| x.code) == Some(CK_EDEC)
        },
        "static",
    );

    s.add(
        "CK9-摘要与块数不符",
        {
            let wrong_sum: &'static str = "1219131555 1 abc";
            let wrong_blocks: &'static str = "1219131554 2 abc";
            verify(ALG_CRC, LABC, b"abc").is_ok()
                && verify(ALG_CRC, wrong_sum, b"abc") == Err(CK_ESUM)
                && verify(ALG_CRC, wrong_blocks, b"abc") == Err(CK_ESIZE)
                && verify(ALG_CRC, LABC, b"abd") == Err(CK_ESUM)
                && l.sample(13).map(|x| x.code) == Some(CK_ESIZE)
        },
        "static",
    );

    s.add(
        "CK10-算法名",
        by_name("cksum") == Ok(ALG_CRC)
            && by_name("crc") == Ok(ALG_CRC)
            && by_name("sum") == Ok(ALG_BSD)
            && by_name("sum-r") == Ok(ALG_BSD)
            && by_name("sysv") == Ok(ALG_SYSV)
            && by_name("sum-s") == Ok(ALG_SYSV)
            && by_name("md5") == Err(CK_EALG)
            && calc(3, b"abc") == Err(CK_EALG)
            && l.sample(8).map(|x| x.code) == Some(CK_EALG),
        "static",
    );

    s.add(
        "CK11-多项式与容量",
        crc_raw(b"abc", 0) == Err(CK_EPOLY)
            && crc_raw(b"abc", 0x7fff_ffff) == Err(CK_EPOLY)
            && crc_raw(b"abc", 0x8000_0000).is_ok()
            && cksum(&BIG, POLY) == Err(CK_ECAP)
            && bsd(&BIG) == Err(CK_ECAP)
            && sysv(&BIG) == Err(CK_ECAP)
            && cksum(&BIG[..CAP], POLY).is_ok()
            && l.sample(9).map(|x| x.code) == Some(CK_EPOLY)
            && l.sample(10).map(|x| x.code) == Some(CK_ECAP),
        "static",
    );

    s.add(
        "CK12-重名与清单",
        {
            let mut c = Checker::new(ALG_CRC);
            c.check(LABC, b"abc").is_ok()
                && c.check(LABC, b"abc") == Err(CK_EDUP)
                && c.check("1219131554 1 abd", b"abd") == Err(CK_ESUM)
                && c.len() == 1
                && c.ok_count() == 1
                && c.alg() == ALG_CRC
        },
        "static",
    );

    s.add(
        "CK13-冻结重放四注入",
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
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11523_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn posix_vectors() {
        assert_eq!(cksum(b"", POLY), Ok(4_294_967_295));
        assert_eq!(cksum(b"abc", POLY), Ok(1_219_131_554));
        assert_eq!(cksum(b"123456789", POLY), Ok(930_766_865));
    }

    #[test]
    fn sum_vectors() {
        assert_eq!(bsd(b"abc"), Ok(16_556));
        assert_eq!(sysv(b"abc"), Ok(294));
        assert_eq!(blocks(1025), 2);
        assert_eq!(blocks(1024), 1);
        assert_eq!(blocks(0), 0);
    }

    #[test]
    fn line_gates() {
        assert_eq!(
            parse_line(LABC),
            Ok(Line { sum: 1_219_131_554, blocks: 1, name: "abc" })
        );
        assert_eq!(parse_line("1219131554 abc"), Err(CK_EFMT));
        assert_eq!(parse_line("1219131554x 1 abc"), Err(CK_EDEC));
        assert_eq!(verify(ALG_CRC, "1219131554 2 abc", b"abc"), Err(CK_ESIZE));
    }

    #[test]
    fn gates() {
        assert_eq!(by_name("md5"), Err(CK_EALG));
        assert_eq!(crc_raw(b"abc", 0), Err(CK_EPOLY));
        assert_eq!(calc(ALG_CRC, &BIG), Err(CK_ECAP));
        let mut c = Checker::new(ALG_CRC);
        assert_eq!(c.check(LABC, b"abc"), Ok(()));
        assert_eq!(c.check(LABC, b"abc"), Err(CK_EDUP));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
