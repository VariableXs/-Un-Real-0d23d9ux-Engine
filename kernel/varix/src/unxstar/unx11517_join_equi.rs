//! UNX-F11517 join 等值连接（-1/-2/-o/-t）
//!
//! `join` 是「两个**已排序**文件按等值键做关系连接」的最小实现。它的难点不在
//! 连接本身，而在三处容易被忽略的约定：
//!
//! 1. **两侧都必须按连接键升序**。`join` 不做排序、也不建哈希表，它是双指针
//!    归并：一旦输入乱序，双指针会静默漏配。因此本实现在连之前先扫一遍键列，
//!    发现逆序直接报 `JN_EORDER`——宁可拒，不给出「少了三行」的结果；
//! 2. **缺省输出是「键 + 甲侧其余字段 + 乙侧其余字段」**，两边的键各只出一次；
//!    `-o` 一旦给出就完全接管输出列，`0` 表示键、`1.N` / `2.N` 表示某侧第 N 个
//!    字段（1 基）。缺字段按空串计，但**列还在**——列数少了会让下游错位；
//! 3. **缺省是内连接**：只有配上的键才出行。`-a1` / `-a2` 补出单侧行（另一侧
//!    字段全空），`-v1` / `-v2` 反过来只出单侧行。三者可以叠加，语义是「并集
//!    减去已配对」。
//!
//! 字段切分有两套规则，`-t` 决定用哪套：
//!
//! - **缺省（空白串）**：跳过前导空白，字段之间是**一段**连续空白，空字段不存在；
//! - **`-t C`（单字节）**：每个分隔字节都切一刀，`a::b` 是三个字段（中间为空）。
//!
//! 这两套规则对同一行给出不同字段数，所以 `-t' '` 与不带 `-t` 不是一回事：前者
//! 把连续空格切成空字段，后者当成一个分隔。
//!
//! 闸门次序（`Spec::build`）：连接字段号 → 分隔符（必须单字节）→ `-o` 列表
//! （逐项校验 file∈{0,1,2}、field≤4，然后才判容量）。先报项非法再报容量：
//! 写错列号比写多半列更常见。
//!
//! 账本语义：十四格 = 八格成功（内连接、外连接、反连接、`-o`、`-t`、重复键
//! 笛卡尔积、两种字节账）+ 六格失败。`audit` 认「八值六码」，`seal` 做 FNV-1a
//! 折叠，`replay` 重建比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf160;

/// 分隔符非法。
pub const JN_ESEP: u32 = SEG_BASE + 0x01;
/// 连接字段号越界。
pub const JN_EFIELD: u32 = SEG_BASE + 0x02;
/// 输入未按键升序。
pub const JN_EORDER: u32 = SEG_BASE + 0x03;
/// `-o` 项非法。
pub const JN_EOUT: u32 = SEG_BASE + 0x04;
/// `-o` 为空或超容量。
pub const JN_EOUTCAP: u32 = SEG_BASE + 0x05;
/// 定长表满（行表或字段表）。
pub const JN_EFULL: u32 = SEG_BASE + 0x06;
/// 行内缺连接字段。
pub const JN_EKEY: u32 = SEG_BASE + 0x07;
/// 输出越过字节预算。
pub const JN_EBYTES: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const JN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const JN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const JN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const JN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const JN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 单行字段上限。
pub const FCAP: usize = 4;
/// 单侧行数上限。
pub const ROWS: usize = 8;
/// `-o` 列数上限。
pub const OCAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 单行输出字节上限。
pub const LINE_MAX: u32 = 64;
/// 单次输出总字节上限。
pub const OUT_CAP: u32 = 512;

/// 操作码：构造。
pub const OP_BUILD: u8 = 1;
/// 操作码：连接。
pub const OP_RUN: u8 = 2;
/// 操作码：切分。
pub const OP_SPLIT: u8 = 3;

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

/// 空白字节。
fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// 定长取字节（越界给 0，调用方先用 `i < len` 卡住）。
fn byte_at(s: &[u8], i: usize) -> u8 {
    match s.get(i) {
        Some(v) => *v,
        None => 0,
    }
}

/// 一列输出：`file` 0 表示键，1/2 表示甲侧/乙侧，`field` 为 1 基字段号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    /// 侧别。
    pub file: u8,
    /// 字段号。
    pub field: u8,
}

impl Out {
    /// 空白项。
    pub const fn blank() -> Out {
        Out { file: 0, field: 0 }
    }
}

/// 选项构造器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    f1: usize,
    f2: usize,
    sep: u32,
    ostr: &'static str,
    has_o: bool,
    a1: bool,
    a2: bool,
    v1: bool,
    v2: bool,
}

impl Spec {
    /// 缺省：键取第 1 字段、空白串切分、内连接、缺省输出。
    pub const fn new() -> Spec {
        Spec {
            f1: 1,
            f2: 1,
            sep: 0,
            ostr: "",
            has_o: false,
            a1: false,
            a2: false,
            v1: false,
            v2: false,
        }
    }

    /// `-1 N -2 M`。
    pub fn fields(self, a: usize, b: usize) -> Spec {
        let mut o = self;
        o.f1 = a;
        o.f2 = b;
        o
    }

    /// `-t C`；`0` 表示「空白串」模式（缺省）。
    pub fn sep(self, c: u32) -> Spec {
        let mut o = self;
        o.sep = c;
        o
    }

    /// `-o` 列表。
    pub fn outputs(self, spec: &'static str) -> Spec {
        let mut o = self;
        o.ostr = spec;
        o.has_o = true;
        o
    }

    /// `-a1` / `-a2`。
    pub fn all(self, one: bool, two: bool) -> Spec {
        let mut o = self;
        o.a1 = one;
        o.a2 = two;
        o
    }

    /// `-v1` / `-v2`。
    pub fn only(self, one: bool, two: bool) -> Spec {
        let mut o = self;
        o.v1 = one;
        o.v2 = two;
        o
    }

    /// 解析一项 `-o`。
    fn one_out(seg: &'static str) -> Result<Out, u32> {
        if seg == "0" {
            return Ok(Out { file: 0, field: 0 });
        }
        let dot = match seg.find('.') {
            Some(p) => p,
            None => return Err(JN_EOUT),
        };
        let lhs = match seg.get(..dot) {
            Some(v) => v,
            None => return Err(JN_EOUT),
        };
        let rhs = match seg.get(dot + 1..) {
            Some(v) => v,
            None => return Err(JN_EOUT),
        };
        let file = match lhs {
            "1" => 1u8,
            "2" => 2u8,
            _ => return Err(JN_EOUT),
        };
        if rhs.is_empty() || rhs.len() > 2 {
            return Err(JN_EOUT);
        }
        let mut f = 0u32;
        let b = rhs.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            let c = byte_at(b, i);
            if !c.is_ascii_digit() {
                return Err(JN_EOUT);
            }
            f = match f.checked_mul(10) {
                Some(x) => x,
                None => return Err(JN_EOUT),
            };
            f = match f.checked_add((c - b'0') as u32) {
                Some(x) => x,
                None => return Err(JN_EOUT),
            };
            i += 1;
        }
        if f == 0 || f > FCAP as u32 {
            return Err(JN_EOUT);
        }
        Ok(Out { file, field: f as u8 })
    }

    /// 定型并过闸。
    pub fn build(&self) -> Result<Join, u32> {
        if self.f1 == 0 || self.f1 > FCAP || self.f2 == 0 || self.f2 > FCAP {
            return Err(JN_EFIELD);
        }
        if self.sep != 0 && self.sep > 0xff {
            return Err(JN_ESEP);
        }
        let mut outs = [Out::blank(); OCAP];
        let mut nout = 0usize;
        if self.has_o {
            if self.ostr.is_empty() {
                return Err(JN_EOUTCAP);
            }
            let mut it = self.ostr.split(',');
            while let Some(seg) = it.next() {
                let o = Spec::one_out(seg)?;
                if nout >= OCAP {
                    return Err(JN_EOUTCAP);
                }
                if let Some(dst) = outs.get_mut(nout) {
                    *dst = o;
                }
                nout += 1;
            }
            if nout == 0 {
                return Err(JN_EOUTCAP);
            }
        }
        Ok(Join {
            f1: self.f1,
            f2: self.f2,
            sep: self.sep as u8,
            outs,
            nout,
            a1: self.a1 || self.v1,
            a2: self.a2 || self.v2,
            v1: self.v1,
            v2: self.v2,
        })
    }
}

/// 一次连接的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 配上的行对数。
    pub pairs: u32,
    /// 输出总字节。
    pub bytes: u32,
}

/// 定型后的连接器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Join {
    f1: usize,
    f2: usize,
    sep: u8,
    outs: [Out; OCAP],
    nout: usize,
    a1: bool,
    a2: bool,
    v1: bool,
    v2: bool,
}

impl Join {
    /// 甲侧键字段号。
    pub fn key_a(&self) -> usize {
        self.f1
    }

    /// 乙侧键字段号。
    pub fn key_b(&self) -> usize {
        self.f2
    }

    /// 分隔符字节；0 表示空白串模式。
    pub fn sep(&self) -> u8 {
        self.sep
    }

    /// `-o` 列数；0 表示缺省输出。
    pub fn out_count(&self) -> usize {
        self.nout
    }

    /// 第 i 个 `-o` 列。
    pub fn out_at(&self, i: usize) -> Option<Out> {
        if i >= self.nout {
            return None;
        }
        self.outs.get(i).copied()
    }

    /// 按分隔符规则切一行。
    pub fn split(&self, line: &'static str) -> Result<([&'static str; FCAP], usize), u32> {
        let b = line.as_bytes();
        let mut out: [&'static str; FCAP] = [""; FCAP];
        let mut n = 0usize;
        let mut i = 0usize;
        if self.sep == 0 {
            while i < b.len() && is_blank(byte_at(b, i)) {
                i += 1;
            }
            while i < b.len() {
                let start = i;
                while i < b.len() && !is_blank(byte_at(b, i)) {
                    i += 1;
                }
                if n >= FCAP {
                    return Err(JN_EFULL);
                }
                match line.get(start..i) {
                    Some(s) => {
                        if let Some(dst) = out.get_mut(n) {
                            *dst = s;
                        }
                    }
                    None => return Err(JN_EKEY),
                }
                n += 1;
                while i < b.len() && is_blank(byte_at(b, i)) {
                    i += 1;
                }
            }
        } else {
            let mut start = 0usize;
            loop {
                let mut j = i;
                while j < b.len() && byte_at(b, j) != self.sep {
                    j += 1;
                }
                if n >= FCAP {
                    return Err(JN_EFULL);
                }
                match line.get(start..j) {
                    Some(s) => {
                        if let Some(dst) = out.get_mut(n) {
                            *dst = s;
                        }
                    }
                    None => return Err(JN_EKEY),
                }
                n += 1;
                if j >= b.len() {
                    break;
                }
                j += 1;
                start = j;
                i = j;
            }
        }
        Ok((out, n))
    }

    /// 单行输出字节（列间一个空格，行尾一个换行）。
    fn line_bytes(
        &self,
        key: &str,
        fa: &[&'static str; FCAP],
        na: usize,
        fb: &[&'static str; FCAP],
        nb: usize,
    ) -> Result<u32, u32> {
        let mut cols = 0u32;
        let mut b = 0u32;
        if self.nout == 0 {
            b = b.saturating_add(key.len() as u32);
            cols = 1;
            let mut i = 0usize;
            while i < na {
                if i + 1 != self.f1 {
                    if let Some(v) = fa.get(i) {
                        b = b.saturating_add(v.len() as u32);
                        cols += 1;
                    }
                }
                i += 1;
            }
            let mut j = 0usize;
            while j < nb {
                if j + 1 != self.f2 {
                    if let Some(v) = fb.get(j) {
                        b = b.saturating_add(v.len() as u32);
                        cols += 1;
                    }
                }
                j += 1;
            }
        } else {
            let mut o = 0usize;
            while o < self.nout {
                let oc = match self.outs.get(o) {
                    Some(v) => *v,
                    None => return Err(JN_EOUT),
                };
                let add = match oc.file {
                    0 => key.len() as u32,
                    1 => {
                        let idx = (oc.field as usize).saturating_sub(1);
                        if idx < na {
                            match fa.get(idx) {
                                Some(v) => v.len() as u32,
                                None => 0,
                            }
                        } else {
                            0
                        }
                    }
                    _ => {
                        let idx = (oc.field as usize).saturating_sub(1);
                        if idx < nb {
                            match fb.get(idx) {
                                Some(v) => v.len() as u32,
                                None => 0,
                            }
                        } else {
                            0
                        }
                    }
                };
                b = b.saturating_add(add);
                cols += 1;
                o += 1;
            }
            if cols == 0 {
                cols = 1;
            }
        }
        b = b.saturating_add(cols.saturating_sub(1));
        b = b.saturating_add(1);
        if b > LINE_MAX {
            return Err(JN_EBYTES);
        }
        Ok(b)
    }

    /// 双指针等值连接。
    pub fn run(&self, a: &[&'static str], b: &[&'static str]) -> Result<Report, u32> {
        if a.len() > ROWS || b.len() > ROWS {
            return Err(JN_EFULL);
        }
        let mut ka: [&'static str; ROWS] = [""; ROWS];
        let mut ra: [[&'static str; FCAP]; ROWS] = [[""; FCAP]; ROWS];
        let mut na: [usize; ROWS] = [0; ROWS];
        let mut i = 0usize;
        while i < a.len() {
            let line = match a.get(i) {
                Some(v) => *v,
                None => break,
            };
            let (f, n) = self.split(line)?;
            if n < self.f1 {
                return Err(JN_EKEY);
            }
            let k = match f.get(self.f1 - 1) {
                Some(v) => *v,
                None => return Err(JN_EKEY),
            };
            if i > 0 {
                if let Some(prev) = ka.get(i - 1) {
                    if k < *prev {
                        return Err(JN_EORDER);
                    }
                }
            }
            if let Some(dst) = ka.get_mut(i) {
                *dst = k;
            }
            if let Some(dst) = ra.get_mut(i) {
                *dst = f;
            }
            if let Some(dst) = na.get_mut(i) {
                *dst = n;
            }
            i += 1;
        }
        let mut kb: [&'static str; ROWS] = [""; ROWS];
        let mut rb: [[&'static str; FCAP]; ROWS] = [[""; FCAP]; ROWS];
        let mut nb: [usize; ROWS] = [0; ROWS];
        let mut j = 0usize;
        while j < b.len() {
            let line = match b.get(j) {
                Some(v) => *v,
                None => break,
            };
            let (f, n) = self.split(line)?;
            if n < self.f2 {
                return Err(JN_EKEY);
            }
            let k = match f.get(self.f2 - 1) {
                Some(v) => *v,
                None => return Err(JN_EKEY),
            };
            if j > 0 {
                if let Some(prev) = kb.get(j - 1) {
                    if k < *prev {
                        return Err(JN_EORDER);
                    }
                }
            }
            if let Some(dst) = kb.get_mut(j) {
                *dst = k;
            }
            if let Some(dst) = rb.get_mut(j) {
                *dst = f;
            }
            if let Some(dst) = nb.get_mut(j) {
                *dst = n;
            }
            j += 1;
        }

        let empty: [&'static str; FCAP] = [""; FCAP];
        let mut r = Report { pairs: 0, bytes: 0 };
        let mut x = 0usize;
        let mut y = 0usize;
        while x < a.len() && y < b.len() {
            let kx = match ka.get(x) {
                Some(v) => *v,
                None => break,
            };
            let ky = match kb.get(y) {
                Some(v) => *v,
                None => break,
            };
            if kx < ky {
                if self.a1 {
                    let add = self.line_bytes(kx, match ra.get(x) {
                        Some(v) => v,
                        None => return Err(JN_EKEY),
                    }, na[x], &empty, 0)?;
                    r.bytes = r.bytes.saturating_add(add);
                    if r.bytes > OUT_CAP {
                        return Err(JN_EBYTES);
                    }
                }
                x += 1;
            } else if ky < kx {
                if self.a2 {
                    let add = self.line_bytes(ky, &empty, 0, match rb.get(y) {
                        Some(v) => v,
                        None => return Err(JN_EKEY),
                    }, nb[y])?;
                    r.bytes = r.bytes.saturating_add(add);
                    if r.bytes > OUT_CAP {
                        return Err(JN_EBYTES);
                    }
                }
                y += 1;
            } else {
                let mut xe = x;
                while xe < a.len() {
                    match ka.get(xe) {
                        Some(v) => {
                            if *v != kx {
                                break;
                            }
                        }
                        None => break,
                    }
                    xe += 1;
                }
                let mut ye = y;
                while ye < b.len() {
                    match kb.get(ye) {
                        Some(v) => {
                            if *v != kx {
                                break;
                            }
                        }
                        None => break,
                    }
                    ye += 1;
                }
                let mut p = x;
                while p < xe {
                    let mut q = y;
                    while q < ye {
                        r.pairs = r.pairs.saturating_add(1);
                        if !self.v1 && !self.v2 {
                            let add = self.line_bytes(
                                kx,
                                match ra.get(p) {
                                    Some(v) => v,
                                    None => return Err(JN_EKEY),
                                },
                                na[p],
                                match rb.get(q) {
                                    Some(v) => v,
                                    None => return Err(JN_EKEY),
                                },
                                nb[q],
                            )?;
                            r.bytes = r.bytes.saturating_add(add);
                            if r.bytes > OUT_CAP {
                                return Err(JN_EBYTES);
                            }
                        }
                        q += 1;
                    }
                    p += 1;
                }
                x = xe;
                y = ye;
            }
        }
        while x < a.len() {
            if self.a1 {
                let kx = match ka.get(x) {
                    Some(v) => *v,
                    None => break,
                };
                let add = self.line_bytes(kx, match ra.get(x) {
                    Some(v) => v,
                    None => return Err(JN_EKEY),
                }, na[x], &empty, 0)?;
                r.bytes = r.bytes.saturating_add(add);
                if r.bytes > OUT_CAP {
                    return Err(JN_EBYTES);
                }
            }
            x += 1;
        }
        while y < b.len() {
            if self.a2 {
                let ky = match kb.get(y) {
                    Some(v) => *v,
                    None => break,
                };
                let add = self.line_bytes(ky, &empty, 0, match rb.get(y) {
                    Some(v) => v,
                    None => return Err(JN_EKEY),
                }, nb[y])?;
                r.bytes = r.bytes.saturating_add(add);
                if r.bytes > OUT_CAP {
                    return Err(JN_EBYTES);
                }
            }
            y += 1;
        }
        Ok(r)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.f1 == 0 || self.f1 > FCAP || self.f2 == 0 || self.f2 > FCAP {
            return false;
        }
        if self.nout > OCAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.nout {
            match self.outs.get(i) {
                Some(o) => {
                    if o.file > 2 {
                        return false;
                    }
                    if o.file != 0 && (o.field == 0 || o.field as usize > FCAP) {
                        return false;
                    }
                }
                None => return false,
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
            return Err(JN_ESLOT);
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
            return Err(JN_EBAL);
        }
        const VALS: [u32; 8] = [3, 3, 3, 19, 3, 3, 25, 30];
        const CODES: [u32; 6] =
            [JN_ESEP, JN_EFIELD, JN_EORDER, JN_EOUT, JN_EOUTCAP, JN_EKEY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(JN_EBAL),
            };
            if s.slot as usize != i {
                return Err(JN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(JN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(JN_EVALUE);
                        }
                    }
                    None => return Err(JN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(JN_EVALUE);
                        }
                    }
                    None => return Err(JN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(JN_EBAL);
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
            return Err(JN_EBAL);
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
            Err(JN_ESEAL)
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

/// 甲侧：键 1、2、2、4。
const A: [&str; 4] = ["1 a1", "2 a2", "2 a22", "4 a4"];
/// 乙侧：键 1、2、3。
const B: [&str; 3] = ["1 b1", "2 b2", "3 b3"];
/// 逗号版甲侧。
const AC: [&str; 4] = ["1,a1", "2,a2", "2,a22", "4,a4"];
/// 逗号版乙侧。
const BC: [&str; 3] = ["1,b1", "2,b2", "3,b3"];
/// 乱序甲侧。
const BAD: [&str; 2] = ["2 x", "1 y"];

/// 起手。
fn mk(spec: Spec) -> Result<Join, u32> {
    spec.build()
}

/// 十四格账本：内连接 → 外连接 → 反连接 → `-o` → `-t` → 笛卡尔 → 两档字节。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let base = match mk(Spec::new()) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_RUN, base.run(&A, &B).map(|r| r.pairs)));
    let _ = l.register(word_cell(
        1,
        OP_RUN,
        mk(Spec::new().all(true, false)).and_then(|j| j.run(&A, &B)).map(|r| r.pairs),
    ));
    let _ = l.register(word_cell(
        2,
        OP_RUN,
        mk(Spec::new().only(false, true)).and_then(|j| j.run(&A, &B)).map(|r| r.pairs),
    ));
    let _ = l.register(word_cell(
        3,
        OP_RUN,
        mk(Spec::new().outputs("1.2,2.2"))
            .and_then(|j| j.run(&A, &B))
            .map(|r| r.bytes),
    ));
    let _ = l.register(word_cell(
        4,
        OP_RUN,
        mk(Spec::new().sep(b',' as u32))
            .and_then(|j| j.run(&AC, &BC))
            .map(|r| r.pairs),
    ));
    let _ = l.register(word_cell(
        5,
        OP_RUN,
        mk(Spec::new().all(false, true)).and_then(|j| j.run(&A, &B)).map(|r| r.pairs),
    ));
    let _ = l.register(word_cell(6, OP_RUN, base.run(&A, &B).map(|r| r.bytes)));
    let _ = l.register(word_cell(
        7,
        OP_RUN,
        mk(Spec::new().all(true, false)).and_then(|j| j.run(&A, &B)).map(|r| r.bytes),
    ));

    let _ = l.register(word_cell(
        8,
        OP_BUILD,
        mk(Spec::new().sep(0x1f600)).map(|_| 0),
    ));
    let _ = l.register(word_cell(9, OP_BUILD, mk(Spec::new().fields(0, 1)).map(|_| 0)));
    let _ = l.register(word_cell(10, OP_RUN, base.run(&BAD, &B).map(|r| r.pairs)));
    let _ = l.register(word_cell(
        11,
        OP_BUILD,
        mk(Spec::new().outputs("3.1")).map(|_| 0),
    ));
    let _ = l.register(word_cell(12, OP_BUILD, mk(Spec::new().outputs("")).map(|_| 0)));
    let _ = l.register(word_cell(13, OP_RUN, base.run(&[""], &B).map(|r| r.pairs)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：内连接与笛卡尔积。
    let base = match mk(Spec::new()) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let r = base.run(&A, &B);
    let one = match r {
        Ok(v) => v,
        Err(_) => return out,
    };
    let dup: [&str; 2] = ["2 p", "2 q"];
    let b2: [&str; 1] = ["2 r"];
    let cart = base.run(&dup, &b2);
    out[1] = one.pairs == 3
        && one.bytes == 25
        && cart == Ok(Report { pairs: 2, bytes: 12 })
        && base.consistent();

    // 二路：外连接与反连接。
    let outer = match mk(Spec::new().all(true, true)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let o = match outer.run(&A, &B) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let only_a = match mk(Spec::new().only(true, false)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let oa = match only_a.run(&A, &B) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = o.pairs == 3
        && o.bytes == 25 + 5 + 5
        && oa.pairs == 3
        && oa.bytes == 5
        && only_a.consistent();

    // 三路：两种切分规则。
    let comma = match mk(Spec::new().sep(b',' as u32)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let colon = match mk(Spec::new().sep(b':' as u32)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let spaced = base.split(" a  b\tc ").map(|(_, n)| n as u32);
    let tabbed = comma.split("a,,b").map(|(_, n)| n as u32);
    out[3] = spaced == Ok(3)
        && tabbed == Ok(3)
        && colon.split("a::b").map(|(f, _)| f) == Ok(["a", "", "b", ""])
        && base.split("a::b").map(|(_, n)| n as u32) == Ok(1)
        && base.split("a b c d e").is_err();

    out
}

/// 十三项判据。
pub fn run_unx11517_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11517.join_equi");
    let l = build_ledger();

    s.add_dyn(
        "JN1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "JN2-内连接只出配对",
        {
            let j = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let r = j.run(&A, &B);
            matches!(r, Ok(ref v) if v.pairs == 3)
                && l.sample(0).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "JN3-重复键笛卡尔积",
        {
            let j = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a: [&str; 3] = ["2 p", "2 q", "2 r"];
            let b: [&str; 2] = ["2 x", "2 y"];
            let r = j.run(&a, &b);
            let one: [&str; 2] = ["1 p", "1 q"];
            let two: [&str; 2] = ["1 x", "1 y"];
            matches!(r, Ok(ref v) if v.pairs == 6)
                && matches!(j.run(&one, &two), Ok(ref v) if v.pairs == 4)
        },
        "static",
    );

    s.add(
        "JN4-外连接补单侧",
        {
            let o = match mk(Spec::new().all(true, true)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let tail = match mk(Spec::new().all(false, true)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(o.run(&A, &B), Ok(ref v) if v.pairs == 3)
                && matches!(tail.run(&A, &B), Ok(ref v) if v.pairs == 3)
                && l.sample(1).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "JN5-反连接只出单侧",
        {
            let v = match mk(Spec::new().only(true, false)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let w = match mk(Spec::new().only(false, true)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(v.run(&A, &B), Ok(ref r) if r.pairs == 3 && r.bytes == 5)
                && matches!(w.run(&A, &B), Ok(ref r) if r.pairs == 3 && r.bytes == 5)
                && l.sample(2).map(|x| x.value) == Some(3)
                && l.sample(5).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "JN6-o列接管输出",
        {
            let j = match mk(Spec::new().outputs("1.2,2.2")) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let k = match mk(Spec::new().outputs("0,1.1,2.1")) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(j.run(&A, &B), Ok(ref r) if r.bytes == 19)
                && j.out_count() == 2
                && j.out_at(0) == Some(Out { file: 1, field: 2 })
                && k.out_count() == 3
                && k.out_at(0) == Some(Out { file: 0, field: 0 })
                && l.sample(3).map(|x| x.value) == Some(19)
        },
        "static"
    );

    s.add(
        "JN7-单字节分隔符",
        {
            let c = match mk(Spec::new().sep(b',' as u32)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(c.run(&AC, &BC), Ok(ref r) if r.pairs == 3)
                && c.sep() == b','
                && c.split("1,a1").map(|(_, n)| n as u32) == Ok(2)
                && l.sample(4).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "JN8-空白串与单空格之别",
        {
            let b0 = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let sp = match mk(Spec::new().sep(b' ' as u32)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            b0.split("a  b").map(|(_, n)| n as u32) == Ok(2)
                && sp.split("a  b").map(|(_, n)| n as u32) == Ok(3)
                && b0.split("  a ").map(|(f, _)| f) == Ok(["a", "", "", ""])
                && sp.split(" a").map(|(f, _)| f) == Ok(["", "a", "", ""])
        },
        "static",
    );

    s.add(
        "JN9-乱序必须先报",
        {
            let j = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            j.run(&BAD, &B) == Err(JN_EORDER)
                && j.run(&A, &["2 x", "1 y"]) == Err(JN_EORDER)
                && j.run(&B, &B).is_ok()
                && l.sample(10).map(|x| x.code) == Some(JN_EORDER)
        },
        "static",
    );

    s.add(
        "JN10-构造四门",
        mk(Spec::new().sep(0x1f600)) == Err(JN_ESEP)
            && mk(Spec::new().fields(0, 1)) == Err(JN_EFIELD)
            && mk(Spec::new().fields(1, 5)) == Err(JN_EFIELD)
            && mk(Spec::new().outputs("3.1")) == Err(JN_EOUT)
            && mk(Spec::new().outputs("1.9")) == Err(JN_EOUT)
            && mk(Spec::new().outputs("1")) == Err(JN_EOUT)
            && mk(Spec::new().outputs("")) == Err(JN_EOUTCAP)
            && mk(Spec::new().outputs("0,0,0,0,0")) == Err(JN_EOUTCAP)
            && l.sample(8).map(|x| x.code) == Some(JN_ESEP)
            && l.sample(9).map(|x| x.code) == Some(JN_EFIELD)
            && l.sample(11).map(|x| x.code) == Some(JN_EOUT)
            && l.sample(12).map(|x| x.code) == Some(JN_EOUTCAP),
        "static",
    );

    s.add(
        "JN11-缺键与表满",
        {
            let j = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let nine: [&str; 9] =
                ["1 a", "1 b", "1 c", "1 d", "1 e", "1 f", "1 g", "1 h", "1 i"];
            j.run(&[""], &B) == Err(JN_EKEY)
                && j.run(&A, &[""]) == Err(JN_EKEY)
                && j.run(&nine, &B) == Err(JN_EFULL)
                && j.run(&A, &nine) == Err(JN_EFULL)
                && j.run(&A, &["1 a b c d"]) == Err(JN_EFULL)
                && l.sample(13).map(|x| x.code) == Some(JN_EKEY)
        },
        "static",
    );

    s.add(
        "JN12-字节预算",
        {
            let j = match mk(Spec::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let long: [&str; 1] = ["1 0123456789012345678901234567890123456789012345678901234567890"];
            matches!(j.run(&long, &["1 b"]), Err(JN_EBYTES))
                && matches!(j.run(&A, &B), Ok(ref r) if r.bytes == 25)
        },
        "static",
    );

    s.add(
        "JN13-冻结重放四注入",
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

    fn j(spec: Spec) -> Join {
        match spec.build() {
            Ok(v) => v,
            Err(_) => panic!("build"),
        }
    }

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
        let s = run_unx11517_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn inner_join_counts() {
        let j = j(Spec::new());
        match j.run(&A, &B) {
            Ok(r) => {
                assert_eq!(r.pairs, 3);
                assert_eq!(r.bytes, 25);
            }
            Err(_) => panic!("run"),
        }
    }

    #[test]
    fn outer_and_anti() {
        let o = j(Spec::new().all(true, true));
        match o.run(&A, &B) {
            Ok(r) => assert_eq!(r.pairs, 3),
            Err(_) => panic!("run"),
        }
        let v = j(Spec::new().only(true, false));
        match v.run(&A, &B) {
            Ok(r) => {
                assert_eq!(r.pairs, 3);
                assert_eq!(r.bytes, 5);
            }
            Err(_) => panic!("run"),
        }
    }

    #[test]
    fn split_rules_differ() {
        let b = j(Spec::new());
        let s = j(Spec::new().sep(b' ' as u32));
        assert_eq!(b.split("a  b").map(|(_, n)| n), Ok(2));
        assert_eq!(s.split("a  b").map(|(_, n)| n), Ok(3));
        assert_eq!(b.split("a\tb c").map(|(_, n)| n), Ok(3));
        assert_eq!(b.split("a b c d e"), Err(JN_EFULL));
    }

    #[test]
    fn order_gate_first() {
        let b = j(Spec::new());
        assert_eq!(b.run(&BAD, &B), Err(JN_EORDER));
        assert_eq!(b.run(&A, &[""]) , Err(JN_EKEY));
    }

    #[test]
    fn build_gates() {
        assert_eq!(Spec::new().sep(0x1f600).build(), Err(JN_ESEP));
        assert_eq!(Spec::new().fields(0, 1).build(), Err(JN_EFIELD));
        assert_eq!(Spec::new().outputs("3.1").build(), Err(JN_EOUT));
        assert_eq!(Spec::new().outputs("1.5").build(), Err(JN_EOUT));
        assert_eq!(Spec::new().outputs("").build(), Err(JN_EOUTCAP));
        assert!(Spec::new().outputs("0,1.1,2.4").build().is_ok());
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
