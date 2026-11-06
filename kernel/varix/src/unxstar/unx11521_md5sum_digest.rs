//! UNX-F11521 md5sum 摘要与校验行格式
//!
//! 本模块做两件事：算 MD5，和读**校验行**。
//!
//! **摘要**按 RFC 1321 实现：512 位分块、小端填长、四轮 16 步，每步
//! `a = b + ((a + F(b,c,d) + X[k] + T[i]) <<< s)`。四轮分别是 F（与/或/非）、
//! G（择多）、H（异或）、I（`c ^ (b | !d)`），位移表依次是
//! `7/12/17/22`、`5/9/14/20`、`4/11/16/23`、`6/10/15/21`。`update` 可增量喂，
//! 摘要只取决于全部字节与总长，因此「一次喂完」与「按 1 字节喂」必须一致——
//! 这是本模块最重要的一条自检。
//!
//! **校验行**的格式是 `<32 位十六进制><空格><模式><名字>`：
//!
//! - 模式是空格表示文本模式、`*` 表示二进制模式，两者**摘要算法相同**，区别
//!   只在读文件时要不要转码，所以本实现只记录标志位；
//! - 行首带 `\` 表示名字里有需要转义的字节：`\\` 是反斜杠、`\n` 是换行；其它
//!   转义一律非法（`MD_EESC`）。转义只作用于**名字**，不作用于摘要；
//! - 名字可以为空吗？不行（`MD_ENAME`）——空名字无法定位文件，校验行就失去了
//!   意义。
//!
//! 闸门次序（`parse_line`）：行长（`MD_ELEN`）→ 32 位十六进制（`MD_EHEX`）→
//! 分隔空格（`MD_EFMT`）→ 模式字符（`MD_EMODE`）→ 名字非空（`MD_ENAME`）→
//! 转义（`MD_EESC`）。先看「是不是一行摘要」，再看「这一行说了什么」。
//!
//! 校验器 `Checker` 另外管两件事：同名重复校验（`MD_EDUP`）与摘要不符
//! （`MD_ESUM`）。重复名字在清单里几乎总是复制粘贴事故，宁可拒。
//!
//! 账本语义：十四格 = 八格成功（摘要首字、名字长、校验通过、摘要长、反转义长、
//! 四向量、二进制标志、行总长）+ 六格失败（十六进制、缺分隔、模式、空名、坏转
//! 义、摘要不符）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重
//! 建比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1a0;

/// 校验行缺分隔空格。
pub const MD_EFMT: u32 = SEG_BASE + 0x01;
/// 摘要字段不是 32 位十六进制。
pub const MD_EHEX: u32 = SEG_BASE + 0x02;
/// 名字为空。
pub const MD_ENAME: u32 = SEG_BASE + 0x03;
/// 模式字符非法。
pub const MD_EMODE: u32 = SEG_BASE + 0x04;
/// 行超过长度上限。
pub const MD_ELEN: u32 = SEG_BASE + 0x05;
/// 转义序列非法。
pub const MD_EESC: u32 = SEG_BASE + 0x06;
/// 摘要与实测不符。
pub const MD_ESUM: u32 = SEG_BASE + 0x07;
/// 同名重复校验。
pub const MD_EDUP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const MD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const MD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const MD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const MD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const MD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 校验行长度上限。
pub const LINE_MAX: usize = 128;
/// 名字缓冲上限。
pub const NAME_MAX: usize = 32;
/// 一次校验清单容量。
pub const CHECKS: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：摘要。
pub const OP_DIGEST: u8 = 1;
/// 操作码：解析。
pub const OP_PARSE: u8 = 2;
/// 操作码：校验。
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

/// 每轮位移表。
const SHIFT: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// 正弦常数表 `floor(2^32 * |sin(i+1)|)`。
const SINE: [u32; 64] = [
    0xd76a_a478, 0xe8c7_b756, 0x2420_70db, 0xc1bd_ceee,
    0xf57c_0faf, 0x4787_c62a, 0xa830_4613, 0xfd46_9501,
    0x6980_98d8, 0x8b44_f7af, 0xffff_5bb1, 0x895c_d7be,
    0x6b90_1122, 0xfd98_7193, 0xa679_438e, 0x49b4_0821,
    0xf61e_2562, 0xc040_b340, 0x265e_5a51, 0xe9b6_c7aa,
    0xd62f_105d, 0x0244_1453, 0xd8a1_e681, 0xe7d3_fbc8,
    0x21e1_cde6, 0xc337_07d6, 0xf4d5_0d87, 0x455a_14ed,
    0xa9e3_e905, 0xfcef_a3f8, 0x676f_02d9, 0x8d2a_4c8a,
    0xfffa_3942, 0x8771_f681, 0x6d9d_6122, 0xfde5_380c,
    0xa4be_ea44, 0x4bde_cfa9, 0xf6bb_4b60, 0xbebf_bc70,
    0x289b_7ec6, 0xeaa1_27fa, 0xd4ef_3085, 0x0488_1d05,
    0xd9d4_d039, 0xe6db_99e5, 0x1fa2_7cf8, 0xc4ac_5665,
    0xf429_2244, 0x432a_ff97, 0xab94_23a7, 0xfc93_a039,
    0x655b_59c3, 0x8f0c_cc92, 0xffef_f47d, 0x8584_5dd1,
    0x6fa8_7e4f, 0xfe2c_e6e0, 0xa301_4314, 0x4e08_11a1,
    0xf753_7e82, 0xbd3a_f235, 0x2ad7_d2bb, 0xeb86_d391,
];

/// 小端取 32 位字。
fn le_word(b: &[u8], i: usize) -> u32 {
    let mut v = 0u32;
    let mut k = 0usize;
    while k < 4 {
        let byte = match b.get(i + k) {
            Some(x) => *x as u32,
            None => 0,
        };
        v |= byte << (8 * k as u32);
        k += 1;
    }
    v
}

/// 压一块。
fn compress(st: &mut [u32; 4], block: &[u8; 64]) {
    let mut m = [0u32; 16];
    let mut i = 0usize;
    while i < 16 {
        m[i] = le_word(block, i * 4);
        i += 1;
    }
    let (mut a, mut b, mut c, mut d) = (st[0], st[1], st[2], st[3]);
    let mut i = 0usize;
    while i < 64 {
        let (f, g) = if i < 16 {
            ((b & c) | (!b & d), i)
        } else if i < 32 {
            ((d & b) | (!d & c), (5 * i + 1) % 16)
        } else if i < 48 {
            (b ^ c ^ d, (3 * i + 5) % 16)
        } else {
            (c ^ (b | !d), (7 * i) % 16)
        };
        let s = match SHIFT.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let t = match SINE.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let gv = match m.get(g) {
            Some(v) => *v,
            None => 0,
        };
        let tmp = a
            .wrapping_add(f)
            .wrapping_add(gv)
            .wrapping_add(t)
            .rotate_left(s)
            .wrapping_add(b);
        a = d;
        d = c;
        c = b;
        b = tmp;
        i += 1;
    }
    st[0] = st[0].wrapping_add(a);
    st[1] = st[1].wrapping_add(b);
    st[2] = st[2].wrapping_add(c);
    st[3] = st[3].wrapping_add(d);
}

/// 十六进制小写转换。
fn nibble(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'a' + (v - 10)
    }
}

/// 十六进制值。
fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 十六字节摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest {
    bytes: [u8; 16],
}

impl Digest {
    /// 空白摘要。
    pub const fn blank() -> Digest {
        Digest { bytes: [0; 16] }
    }

    /// 原始字节。
    pub fn bytes(&self) -> &[u8; 16] {
        &self.bytes
    }

    /// 32 位小写十六进制。
    pub fn hex(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut i = 0usize;
        while i < 16 {
            let b = match self.bytes.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if let Some(dst) = out.get_mut(i * 2) {
                *dst = nibble(b >> 4);
            }
            if let Some(dst) = out.get_mut(i * 2 + 1) {
                *dst = nibble(b & 0x0f);
            }
            i += 1;
        }
        out
    }

    /// 第 i 个 32 位字（大端读法，便于观测）。
    pub fn word(&self, i: usize) -> u32 {
        let mut v = 0u32;
        let mut k = 0usize;
        while k < 4 {
            let b = match self.bytes.get(i * 4 + k) {
                Some(x) => *x as u32,
                None => 0,
            };
            v = (v << 8) | b;
            k += 1;
        }
        v
    }

    /// 按十六进制字面量比对。
    pub fn eq_hex(&self, want: &[u8]) -> bool {
        let h = self.hex();
        if want.len() != 32 {
            return false;
        }
        let mut i = 0usize;
        while i < 32 {
            match (h.get(i), want.get(i)) {
                (Some(x), Some(y)) => {
                    if *x != *y {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 增量摘要器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Md5 {
    st: [u32; 4],
    len: u64,
    buf: [u8; 64],
    fill: usize,
}

impl Md5 {
    /// 起始状态。
    pub const fn new() -> Md5 {
        Md5 {
            st: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476],
            len: 0,
            buf: [0; 64],
            fill: 0,
        }
    }

    /// 喂入数据。
    pub fn update(&mut self, data: &[u8]) {
        let mut i = 0usize;
        while i < data.len() {
            let room = 64usize.saturating_sub(self.fill);
            let left = data.len().saturating_sub(i);
            let take = if room < left { room } else { left };
            let mut k = 0usize;
            while k < take {
                if let (Some(dst), Some(src)) = (self.buf.get_mut(self.fill + k), data.get(i + k)) {
                    *dst = *src;
                }
                k += 1;
            }
            self.fill = self.fill.saturating_add(take);
            i = i.saturating_add(take);
            if self.fill == 64 {
                compress(&mut self.st, &self.buf);
                self.fill = 0;
            }
        }
        self.len = self.len.saturating_add(data.len() as u64);
    }

    /// 已喂字节数。
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 是否还没喂。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 收尾出摘要。填 0x80、补零到 56、尾部小端写位数。
    pub fn finish(mut self) -> Digest {
        let bits = self.len.wrapping_mul(8);
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        let rem = self.fill;
        let need = if rem < 56 { 56 - rem } else { 120 - rem };
        self.update(&pad[..need]);
        let mut tail = [0u8; 8];
        let mut i = 0usize;
        while i < 8 {
            tail[i] = ((bits >> (8 * i as u32)) & 0xff) as u8;
            i += 1;
        }
        self.update(&tail);
        let mut out = [0u8; 16];
        let mut j = 0usize;
        while j < 4 {
            let w = match self.st.get(j) {
                Some(v) => *v,
                None => 0,
            };
            let mut k = 0usize;
            while k < 4 {
                if let Some(dst) = out.get_mut(j * 4 + k) {
                    *dst = ((w >> (8 * k as u32)) & 0xff) as u8;
                }
                k += 1;
            }
            j += 1;
        }
        Digest { bytes: out }
    }
}

/// 一次算完。
pub fn md5(data: &[u8]) -> Digest {
    let mut m = Md5::new();
    m.update(data);
    m.finish()
}

/// 反转义后的名字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; NAME_MAX],
    len: usize,
}

impl Name {
    /// 字节。
    pub fn as_bytes(&self) -> &[u8] {
        match self.buf.get(..self.len) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 反转义名字：`\\` → `\`，`\n` → 换行，其余转义非法。
pub fn unescape(raw: &str) -> Result<Name, u32> {
    let b = raw.as_bytes();
    let mut out = Name { buf: [0; NAME_MAX], len: 0 };
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        let byte = if c == b'\\' {
            i += 1;
            match b.get(i) {
                Some(b'\\') => b'\\',
                Some(b'n') => b'\n',
                _ => return Err(MD_EESC),
            }
        } else {
            c
        };
        if out.len >= NAME_MAX {
            return Err(MD_ELEN);
        }
        if let Some(dst) = out.buf.get_mut(out.len) {
            *dst = byte;
        }
        out.len += 1;
        i += 1;
    }
    Ok(out)
}

/// 解析后的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SumLine {
    /// 摘要。
    pub digest: Digest,
    /// 二进制模式（`*`）。
    pub binary: bool,
    /// 名字带转义（行首 `\`）。
    pub escaped: bool,
    /// 原始名字（未反转义）。
    pub name: &'static str,
}

impl SumLine {
    /// 名字字节数。
    pub fn name_len(&self) -> usize {
        self.name.len()
    }
}

/// 解析校验行。
pub fn parse_line(line: &'static str) -> Result<SumLine, u32> {
    if line.len() > LINE_MAX {
        return Err(MD_ELEN);
    }
    let b = line.as_bytes();
    let mut at = 0usize;
    let mut escaped = false;
    if b.get(0) == Some(&b'\\') {
        escaped = true;
        at = 1;
    }
    if b.len() < at + 32 {
        return Err(MD_EHEX);
    }
    if b.len() < at + 34 {
        return Err(MD_EFMT);
    }
    let mut bytes = [0u8; 16];
    let mut i = 0usize;
    while i < 16 {
        let hi = match b.get(at + i * 2).and_then(|c| hexval(*c)) {
            Some(v) => v,
            None => return Err(MD_EHEX),
        };
        let lo = match b.get(at + i * 2 + 1).and_then(|c| hexval(*c)) {
            Some(v) => v,
            None => return Err(MD_EHEX),
        };
        if let Some(dst) = bytes.get_mut(i) {
            *dst = (hi << 4) | lo;
        }
        i += 1;
    }
    let sep = match b.get(at + 32) {
        Some(v) => *v,
        None => return Err(MD_EFMT),
    };
    if sep != b' ' {
        return Err(MD_EFMT);
    }
    let mode = match b.get(at + 33) {
        Some(v) => *v,
        None => return Err(MD_EMODE),
    };
    let binary = match mode {
        b' ' => false,
        b'*' => true,
        _ => return Err(MD_EMODE),
    };
    let name = match line.get(at + 34..) {
        Some(v) => v,
        None => return Err(MD_ENAME),
    };
    if name.is_empty() {
        return Err(MD_ENAME);
    }
    Ok(SumLine { digest: Digest { bytes }, binary, escaped, name })
}

/// 校验清单。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    names: [&'static str; CHECKS],
    n: usize,
    ok: usize,
}

impl Checker {
    /// 空清单。
    pub const fn new() -> Checker {
        Checker { names: [""; CHECKS], n: 0, ok: 0 }
    }

    /// 已校验条数。
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

    /// 校验一行。
    pub fn check(&mut self, line: &'static str, data: &[u8]) -> Result<(), u32> {
        let l = parse_line(line)?;
        let mut i = 0usize;
        while i < self.n {
            if self.names.get(i) == Some(&l.name) {
                return Err(MD_EDUP);
            }
            i += 1;
        }
        let got = md5(data);
        if got != l.digest {
            return Err(MD_ESUM);
        }
        if self.n >= CHECKS {
            return Err(MD_ELEN);
        }
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
            return Err(MD_ESLOT);
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
            return Err(MD_EBAL);
        }
        const VALS: [u32; 8] = [0x9001_5098, 3, 1, 16, 3, 4, 1, 37];
        const CODES: [u32; 6] =
            [MD_EHEX, MD_EFMT, MD_EMODE, MD_ENAME, MD_EESC, MD_ESUM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MD_EBAL),
            };
            if s.slot as usize != i {
                return Err(MD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MD_EVALUE);
                        }
                    }
                    None => return Err(MD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MD_EVALUE);
                        }
                    }
                    None => return Err(MD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MD_EBAL);
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
            return Err(MD_EBAL);
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
            Err(MD_ESEAL)
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

/// 文本模式校验行（"abc" 的摘要）。
const LINE_TXT: &str = "900150983cd24fb0d6963f7d28e17f72  abc";
/// 二进制模式校验行。
const LINE_BIN: &str = "900150983cd24fb0d6963f7d28e17f72 *abc";
/// 转义名字校验行。
const LINE_ESC: &str = "\\900150983cd24fb0d6963f7d28e17f72  a\\\\b";
/// 超长行（129 字节，超 `LINE_MAX` 一格）。
const LONG_LINE: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

/// 四组固定向量。
const VEC: [(&str, &[u8]); 4] = [
    ("", b"d41d8cd98f00b204e9800998ecf8427e"),
    ("a", b"0cc175b9c0f1b6a831c399e269772661"),
    ("abc", b"900150983cd24fb0d6963f7d28e17f72"),
    ("message digest", b"f96b697d7cb7938d525a2f31aaf161d0"),
];

/// 四组固定向量的校验行（名字与数据一一对应）。
const VEC_LINES: [(&str, &[u8]); 4] = [
    ("d41d8cd98f00b204e9800998ecf8427e  empty", b""),
    ("0cc175b9c0f1b6a831c399e269772661  a", b"a"),
    (LINE_TXT, b"abc"),
    ("f96b697d7cb7938d525a2f31aaf161d0  message digest", b"message digest"),
];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_DIGEST, Ok(md5(b"abc").word(0))));
    let _ = l.register(word_cell(
        1,
        OP_PARSE,
        parse_line(LINE_TXT).map(|x| x.name_len() as u32),
    ));
    let _ = l.register(word_cell(2, OP_VERIFY, {
        let mut c = Checker::new();
        c.check(LINE_TXT, b"abc").map(|_| c.len() as u32)
    }));
    let _ = l.register(word_cell(3, OP_DIGEST, Ok(md5(b"abc").bytes().len() as u32)));
    let _ = l.register(word_cell(
        4,
        OP_PARSE,
        unescape("a\\\\b").map(|n| n.len() as u32),
    ));
    let _ = l.register(word_cell(5, OP_VERIFY, {
        let mut c = Checker::new();
        let mut i = 0usize;
        while i < VEC_LINES.len() {
            match VEC_LINES.get(i) {
                Some((line, data)) => {
                    let _ = c.check(line, data);
                }
                None => break,
            }
            i += 1;
        }
        Ok(c.ok_count() as u32)
    }));
    let _ = l.register(word_cell(6, OP_PARSE, parse_line(LINE_BIN).map(|x| x.binary as u32)));
    let _ = l.register(word_cell(7, OP_PARSE, Ok(LINE_TXT.len() as u32)));

    let _ = l.register(word_cell(8, OP_PARSE, {
        let bad: &'static str = "900150983cd24fb0d6963f7d28e17f7z  abc";
        parse_line(bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(9, OP_PARSE, {
        let bad: &'static str = "900150983cd24fb0d6963f7d28e17f72\u{9}abc";
        parse_line(bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(10, OP_PARSE, {
        let bad: &'static str = "900150983cd24fb0d6963f7d28e17f72 xabc";
        parse_line(bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(11, OP_PARSE, {
        let bad: &'static str = "900150983cd24fb0d6963f7d28e17f72  ";
        parse_line(bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(12, OP_PARSE, unescape("a\\tb").map(|n| n.len() as u32)));
    let _ = l.register(word_cell(13, OP_VERIFY, {
        let mut c = Checker::new();
        let bad: &'static str = "00000000000000000000000000000000  abc";
        c.check(bad, b"abc").map(|_| c.len() as u32)
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

    // 一路：四个固定向量。
    let mut i = 0usize;
    let mut all = true;
    while i < VEC.len() {
        match VEC.get(i) {
            Some((data, want)) => {
                if !md5(data.as_bytes()).eq_hex(want) {
                    all = false;
                }
            }
            None => all = false,
        }
        i += 1;
    }
    out[1] = all && md5(b"").eq_hex(b"d41d8cd98f00b204e9800998ecf8427e");

    // 二路：增量与一次喂完一致（跨 512 位边界）。
    let mut chunk = [0u8; 65];
    let mut k = 0usize;
    while k < chunk.len() {
        chunk[k] = (k as u8).wrapping_mul(7).wrapping_add(3);
        k += 1;
    }
    let one = md5(&chunk);
    let mut m = Md5::new();
    let mut j = 0usize;
    while j < chunk.len() {
        let take = if j % 3 == 0 { 1 } else { 4 };
        let end = if j + take < chunk.len() { j + take } else { chunk.len() };
        match chunk.get(j..end) {
            Some(s) => m.update(s),
            None => m.update(&[]),
        }
        j = end;
    }
    let inc = m.finish();
    out[2] = one == inc
        && one.word(0) != 0
        && one.hex().len() == 32
        && parse_line(LINE_TXT).map(|x| x.digest) == Ok(md5(b"abc"))
        && {
            let mut c = Checker::new();
            c.check(LINE_TXT, b"abc").is_ok() && c.check(LINE_BIN, b"abc").is_err()
        };

    // 三路：格式与转义闸门。
    let bad_hex: &'static str = "900150983cd24fb0d6963f7d28e17f7z  abc";
    let one_sep: &'static str = "900150983cd24fb0d6963f7d28e17f72 abc";
    let tab_sep: &'static str = "900150983cd24fb0d6963f7d28e17f72\u{9}abc";
    let bad_mode: &'static str = "900150983cd24fb0d6963f7d28e17f72 xabc";
    let no_name: &'static str = "900150983cd24fb0d6963f7d28e17f72  ";
    out[3] = parse_line(bad_hex) == Err(MD_EHEX)
        && parse_line(tab_sep) == Err(MD_EFMT)
        && parse_line(one_sep) == Err(MD_EMODE)
        && parse_line(bad_mode) == Err(MD_EMODE)
        && parse_line(no_name) == Err(MD_ENAME)
        && unescape("a\\tb") == Err(MD_EESC)
        && unescape("a\\\\b").map(|n| n.len()) == Ok(3)
        && parse_line(LINE_ESC).map(|x| (x.escaped, x.name_len())) == Ok((true, 4))
        && {
            let mut c = Checker::new();
            let bad_sum: &'static str = "00000000000000000000000000000000  abc";
            c.check(bad_sum, b"abc") == Err(MD_ESUM)
                && c.check(LINE_TXT, b"abc").is_ok()
                && c.check(LINE_TXT, b"abc") == Err(MD_EDUP)
        };

    out
}

/// 十三项判据。
pub fn run_unx11521_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11521.md5sum_digest");
    let l = build_ledger();

    s.add_dyn(
        "MD1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MD2-摘要十六字节",
        md5(b"abc").eq_hex(b"900150983cd24fb0d6963f7d28e17f72")
            && md5(b"abc").bytes().len() == 16
            && md5(b"abc").hex().len() == 32
            && md5(b"abc").word(0) == 0x9001_5098
            && l.sample(0).map(|x| x.value) == Some(0x9001_5098),
        "static",
    );

    s.add(
        "MD3-空串与单字节",
        md5(b"").eq_hex(b"d41d8cd98f00b204e9800998ecf8427e")
            && md5(b"a").eq_hex(b"0cc175b9c0f1b6a831c399e269772661")
            && md5(b"abc") != md5(b"abd")
            && md5(b"") != md5(b"a"),
        "static",
    );

    s.add(
        "MD4-分块边界一致",
        {
            let mut all = true;
            let mut n = 0usize;
            while n <= 65 {
                let mut buf = [0u8; 65];
                let mut k = 0usize;
                while k < n {
                    buf[k] = (k as u8).wrapping_mul(11).wrapping_add(5);
                    k += 1;
                }
                let one = md5(&buf[..n]);
                let mut m = Md5::new();
                let mut j = 0usize;
                while j < n {
                    let end = if j + 7 < n { j + 7 } else { n };
                    match buf.get(j..end) {
                        Some(x) => m.update(x),
                        None => m.update(&[]),
                    }
                    j = end;
                }
                if m.finish() != one {
                    all = false;
                }
                n += 1;
            }
            all
        },
        "static",
    );

    s.add(
        "MD5-校验行两种模式",
        parse_line(LINE_TXT).map(|x| (x.binary, x.escaped, x.name)) == Ok((false, false, "abc"))
            && parse_line(LINE_BIN).map(|x| (x.binary, x.name)) == Ok((true, "abc"))
            && parse_line(LINE_TXT).map(|x| x.name_len()) == Ok(3)
            && l.sample(6).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "MD6-转义名字",
        {
            let esc = parse_line(LINE_ESC);
            unescape("a\\\\b").map(|n| n.len()) == Ok(3)
                && unescape("a\\nb").map(|n| n.len()) == Ok(3)
                && unescape("a\\\\nb").map(|n| n.len()) == Ok(4)
                && unescape("a\\tb") == Err(MD_EESC)
                && unescape("ab\\") == Err(MD_EESC)
                && esc.map(|x| (x.escaped, x.name_len())) == Ok((true, 4))
                && l.sample(4).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "MD7-摘要字段闸门",
        {
            let short: &'static str = "900150983cd24fb0d6963f7d28e17f7  abc";
            let bad: &'static str = "900150983cd24fb0d6963f7d28e17f7z  abc";
            parse_line(short) == Err(MD_EHEX)
                && parse_line(bad) == Err(MD_EHEX)
                && parse_line("00000000000000000000000000000000  abc").is_ok()
                && parse_line("900150983CD24FB0D6963F7D28E17F72  abc").is_ok()
                && l.sample(8).map(|x| x.code) == Some(MD_EHEX)
        },
        "static",
    );

    s.add(
        "MD8-格式与名字闸门",
        {
            let no_sep: &'static str = "900150983cd24fb0d6963f7d28e17f72 abc";
            let tab: &'static str = "900150983cd24fb0d6963f7d28e17f72\tabc";
            let bad_mode: &'static str = "900150983cd24fb0d6963f7d28e17f72 xabc";
            let no_name: &'static str = "900150983cd24fb0d6963f7d28e17f72  ";
            parse_line(no_sep) == Err(MD_EMODE)
                && parse_line(tab) == Err(MD_EFMT)
                && parse_line(bad_mode) == Err(MD_EMODE)
                && parse_line(no_name) == Err(MD_ENAME)
                && l.sample(9).map(|x| x.code) == Some(MD_EFMT)
                && l.sample(10).map(|x| x.code) == Some(MD_EMODE)
                && l.sample(11).map(|x| x.code) == Some(MD_ENAME)
        },
        "static",
    );

    s.add(
        "MD9-校验不符即拒",
        {
            let mut c = Checker::new();
            let bad: &'static str = "00000000000000000000000000000000  zzz";
            c.check(bad, b"zzz") == Err(MD_ESUM)
                && c.is_empty()
                && c.check(LINE_TXT, b"abc").is_ok()
                && c.check(bad, b"zzz") == Err(MD_ESUM)
                && c.check(LINE_TXT, b"abcd") == Err(MD_EDUP)
                && c.len() == 1
                && l.sample(13).map(|x| x.code) == Some(MD_ESUM)
        },
        "static",
    );

    s.add(
        "MD10-同名重复拒",
        {
            let mut c = Checker::new();
            c.check(LINE_TXT, b"abc").is_ok()
                && c.check(LINE_BIN, b"abc") == Err(MD_EDUP)
                && c.len() == 1
                && c.ok_count() == 1
                && l.sample(2).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "MD11-行长上限",
        {
            parse_line(LONG_LINE) == Err(MD_ELEN) && parse_line(LINE_TXT).is_ok()
        },
        "static",
    );

    s.add(
        "MD12-四向量固定",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < VEC.len() {
                match VEC.get(i) {
                    Some((data, want)) => {
                        if !md5(data.as_bytes()).eq_hex(want) {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && l.sample(5).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "MD13-冻结重放四注入",
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
        let s = run_unx11521_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn known_vectors() {
        assert!(md5(b"").eq_hex(b"d41d8cd98f00b204e9800998ecf8427e"));
        assert!(md5(b"a").eq_hex(b"0cc175b9c0f1b6a831c399e269772661"));
        assert!(md5(b"abc").eq_hex(b"900150983cd24fb0d6963f7d28e17f72"));
        assert!(md5(b"message digest").eq_hex(b"f96b697d7cb7938d525a2f31aaf161d0"));
    }

    #[test]
    fn incremental_matches() {
        let mut buf = [0u8; 100];
        for i in 0..100 {
            buf[i] = i as u8;
        }
        let mut m = Md5::new();
        let mut j = 0;
        while j < 100 {
            let end = if j + 7 < 100 { j + 7 } else { 100 };
            m.update(&buf[j..end]);
            j = end;
        }
        assert_eq!(m.finish(), md5(&buf));
    }

    #[test]
    fn line_parsing() {
        assert_eq!(
            parse_line(LINE_TXT).map(|x| (x.binary, x.name)),
            Ok((false, "abc"))
        );
        assert_eq!(parse_line(LINE_BIN).map(|x| x.binary), Ok(true));
        assert_eq!(
            parse_line("900150983cd24fb0d6963f7d28e17f72  "),
            Err(MD_ENAME)
        );
        assert_eq!(
            parse_line("900150983cd24fb0d6963f7d28e17f7z  abc"),
            Err(MD_EHEX)
        );
    }

    #[test]
    fn checker_gates() {
        let mut c = Checker::new();
        assert_eq!(c.check(LINE_TXT, b"abc"), Ok(()));
        assert_eq!(c.check(LINE_BIN, b"abc"), Err(MD_EDUP));
        assert_eq!(
            c.check("00000000000000000000000000000000  z", b"z"),
            Err(MD_ESUM)
        );
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}