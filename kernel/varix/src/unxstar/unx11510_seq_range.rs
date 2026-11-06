//! UNX-F11510 seq 序列生成（等差/格式/-s/-w）（0xf0f0 段）
//!
//! `seq` 把"首项、步长、末项"三个整数折成一列数。它不是浮点工具，而是
//! **定点整数**工具：所有项都是 i64，步长不许为 0，首末项限制在 ±10^12、
//! 步长限制在 ±10^9，项数上限十万。这个边界不是畏难，而是为了让每一项
//! 都能用 `checked_*` 算出来、让总长能在 u32 里放得下——序列生成器一旦
//! 悄悄溢出，后面所有格式与分隔都失去意义。
//!
//! 项数由方向决定：步长为正时，首项大于末项是**空序列**（零项，不是错误）；
//! 步长为负时反之。项数为 `(|末项 - 首项| / |步长|) + 1`，超过十万是
//! `ELONG`，在开户时就拒，绝不"先生成再截断"。
//!
//! 操作数文法：`seq [选项] [首项 [步长]] 末项`。一个操作数是"首项默认为 1、
//! 步长默认为 1"；两个是"步长默认为 1"；三个全给。看起来像负数的词
//! （`-3`、`-10`）走数值分支而不是选项分支，因此负数不需要 `--` 也能写；
//! 真正的选项只有 `-w`、`-s`、`-f` 与终止符 `--`。选项重复、`-w` 与 `-f`
//! 同时出现、未知横线词都是 `EOPT`；`-s`/`-f` 缺操作数是 `EOPERAND`。
//!
//! 渲染规则：默认分隔串是换行，**每一项之后都写一次分隔串**（包括最后
//! 一项），所以默认输出以换行收尾。`-s` 换分隔串（允许空串），长度上限
//! 4 字节。`-w` 让所有项等宽：宽度取首项与末项里较宽的那个（含负号），
//! 不足补零，符号在零之前。`-f` 只认一个转换：`%d`、`%Nd`（空格右对齐）、
//! `%0Nd`（零填充），宽度 1..=9，前后可有字面量（各 8 字节），出现第二个
//! 转换是 `EMFMT`，没有转换也是 `EMFMT`，转换符不认识是 `EBADFMT`。
//!
//! 账本十四格：八格成功（末项解析、两项计数、负步长计数、默认总长、
//! 自定义分隔、等宽渲染、格式渲染、带前后缀格式），六格拒绝（未知选项、
//! 操作数过多、零步长、数值越界、未知转换、多重转换）。审计逐格复算，
//! 封印折叠全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0f0;

/// 未知或重复选项。
pub const SQ_EOPT: u32 = SEG_BASE + 0x01;
/// 操作数缺失或过多。
pub const SQ_EOPERAND: u32 = SEG_BASE + 0x02;
/// 步长为零。
pub const SQ_EZERO: u32 = SEG_BASE + 0x03;
/// 数值或步长越界。
pub const SQ_ERANGE: u32 = SEG_BASE + 0x04;
/// 项数或分量过长。
pub const SQ_ELONG: u32 = SEG_BASE + 0x05;
/// 转换符不认识。
pub const SQ_EBADFMT: u32 = SEG_BASE + 0x06;
/// 转换个数不是一。
pub const SQ_EMFMT: u32 = SEG_BASE + 0x07;
/// 输出缓冲不足。
pub const SQ_EBUF: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SQ_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SQ_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SQ_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SQ_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SQ_ENOBS: u32 = SEG_BASE + 0x0d;

/// 操作数绝对值上限。
pub const NUM_MAX: i64 = 1_000_000_000_000;
/// 步长绝对值上限。
pub const STEP_MAX: i64 = 1_000_000_000;
/// 项数上限。
pub const COUNT_MAX: u32 = 100_000;
/// 分隔串容量。
pub const SEP_CAP: usize = 4;
/// 格式前后缀容量。
pub const LIT_CAP: usize = 8;
/// 格式宽度上限。
pub const FMT_WIDTH: u32 = 9;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：解析操作数。
pub const OP_PLAN: u8 = 1;
/// 操作码：计数。
pub const OP_COUNT: u8 = 2;
/// 操作码：算总长。
pub const OP_LEN: u8 = 3;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 4;
/// 操作码：格式解析。
pub const OP_FMT: u8 = 5;

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

/// 分隔串。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sep {
    bytes: [u8; SEP_CAP],
    len: u8,
}

impl Sep {
    /// 换行分隔。
    pub const fn newline() -> Sep {
        Sep { bytes: [b'\n', 0, 0, 0], len: 1 }
    }

    /// 从字节串造分隔串。
    pub fn new(src: &[u8]) -> Result<Sep, u32> {
        if src.len() > SEP_CAP {
            return Err(SQ_ELONG);
        }
        let mut s = Sep { bytes: [0u8; SEP_CAP], len: 0 };
        s.len = copy(&mut s.bytes, src);
        Ok(s)
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        match self.bytes.get(..self.len as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空串。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 一条数字格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fmt {
    pre: [u8; LIT_CAP],
    plen: u8,
    post: [u8; LIT_CAP],
    qlen: u8,
    zero: bool,
    width: u8,
}

impl Fmt {
    /// 空白格式。
    pub const fn blank() -> Fmt {
        Fmt { pre: [0u8; LIT_CAP], plen: 0, post: [0u8; LIT_CAP], qlen: 0, zero: false, width: 0 }
    }

    /// 前缀。
    pub fn prefix(&self) -> &[u8] {
        match self.pre.get(..self.plen as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 后缀。
    pub fn suffix(&self) -> &[u8] {
        match self.post.get(..self.qlen as usize) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 宽度。
    pub fn width(&self) -> u32 {
        self.width as u32
    }

    /// 是否零填充。
    pub fn zero(&self) -> bool {
        self.zero
    }

    /// 一个值的展开长度。
    pub fn value_len(&self, v: i64) -> u32 {
        let body = signed_len(v);
        let body = if self.width as u32 > body { self.width as u32 } else { body };
        self.plen as u32 + body + self.qlen as u32
    }
}

/// 解析 `-f` 的数字格式。
pub fn parse_fmt(p: &[u8]) -> Result<Fmt, u32> {
    let mut f = Fmt::blank();
    let mut i = 0usize;
    while i < p.len() {
        match p.get(i).copied() {
            Some(b'%') => break,
            Some(b) => {
                if f.plen as usize >= LIT_CAP {
                    return Err(SQ_ELONG);
                }
                if let Some(c) = f.pre.get_mut(f.plen as usize) {
                    *c = b;
                }
                f.plen += 1;
                i += 1;
            }
            None => break,
        }
    }
    if i >= p.len() {
        return Err(SQ_EMFMT);
    }
    i += 1;
    let mut zero = false;
    if p.get(i).copied() == Some(b'0') {
        zero = true;
        i += 1;
    }
    let mut width = 0u32;
    let mut wd = 0u32;
    while wd < 2 {
        let b = match p.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        if !b.is_ascii_digit() {
            break;
        }
        width = width * 10 + (b - b'0') as u32;
        i += 1;
        wd += 1;
    }
    if width > FMT_WIDTH {
        return Err(SQ_EBADFMT);
    }
    match p.get(i).copied() {
        Some(b'd') => i += 1,
        Some(_) => return Err(SQ_EBADFMT),
        None => return Err(SQ_EBADFMT),
    }
    while i < p.len() {
        match p.get(i).copied() {
            Some(b'%') => return Err(SQ_EMFMT),
            Some(b) => {
                if f.qlen as usize >= LIT_CAP {
                    return Err(SQ_ELONG);
                }
                if let Some(c) = f.post.get_mut(f.qlen as usize) {
                    *c = b;
                }
                f.qlen += 1;
                i += 1;
            }
            None => break,
        }
    }
    if zero && width == 0 {
        return Err(SQ_EBADFMT);
    }
    f.zero = zero;
    f.width = width as u8;
    Ok(f)
}

/// 一个序列计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// 首项。
    pub first: i64,
    /// 步长。
    pub step: i64,
    /// 末项。
    pub last: i64,
    /// 分隔串。
    pub sep: Sep,
    /// 是否等宽。
    pub equal: bool,
    /// 数字格式。
    pub fmt: Option<Fmt>,
}

impl Plan {
    /// 项数。
    pub fn count(&self) -> u32 {
        raw_count(self.first, self.step, self.last) as u32
    }

    /// 第 i 项。
    pub fn term(&self, i: u32) -> Result<i64, u32> {
        let k = i as i64;
        let d = k.checked_mul(self.step).ok_or(SQ_ERANGE)?;
        self.first.checked_add(d).ok_or(SQ_ERANGE)
    }

    /// 等宽宽度。
    fn eq_width(&self) -> u32 {
        let a = signed_len(self.first);
        let b = signed_len(self.last);
        if a > b {
            a
        } else {
            b
        }
    }

    /// 一项的展开长度。
    fn term_len(&self, v: i64) -> u32 {
        match self.fmt {
            Some(f) => f.value_len(v),
            None => {
                let n = signed_len(v);
                if self.equal {
                    let w = self.eq_width();
                    if w > n {
                        w
                    } else {
                        n
                    }
                } else {
                    n
                }
            }
        }
    }

    /// 全序列输出总长。
    pub fn print_len(&self) -> Result<u32, u32> {
        let mut total = 0u32;
        let sep = self.sep.len() as u32;
        let mut i = 0u32;
        let n = self.count();
        while i < n {
            let v = self.term(i)?;
            total = total.saturating_add(self.term_len(v));
            total = total.saturating_add(sep);
            i += 1;
        }
        Ok(total)
    }

    /// 渲染整列。
    pub fn render(&self, dst: &mut [u8]) -> Result<u32, u32> {
        let need = self.print_len()? as usize;
        if need > dst.len() {
            return Err(SQ_EBUF);
        }
        let mut at = 0usize;
        let mut i = 0u32;
        let n = self.count();
        while i < n {
            let v = self.term(i)?;
            at = self.emit_term(dst, at, v);
            at = put(dst, at, self.sep.as_bytes());
            i += 1;
        }
        Ok(at as u32)
    }

    /// 写一项。
    fn emit_term(&self, dst: &mut [u8], at: usize, v: i64) -> usize {
        match self.fmt {
            Some(f) => {
                let mut cur = put(dst, at, f.prefix());
                cur = put_i64(dst, cur, v, f.width(), f.zero());
                put(dst, cur, f.suffix())
            }
            None => {
                let w = if self.equal { self.eq_width() } else { 0 };
                put_i64(dst, at, v, w, self.equal)
            }
        }
    }
}

/// 无校验计数（调用前已保证边界）。
fn raw_count(first: i64, step: i64, last: i64) -> u64 {
    if step > 0 {
        if first > last {
            0
        } else {
            ((last - first) / step) as u64 + 1
        }
    } else if first < last {
        0
    } else if step == 0 {
        0
    } else {
        ((first - last) / (-step)) as u64 + 1
    }
}

/// 带符号十进制长度。
fn signed_len(v: i64) -> u32 {
    let mag = if v < 0 { (v as u64).wrapping_neg() } else { v as u64 };
    let mut n = 1u32;
    let mut x = mag;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    if v < 0 {
        n + 1
    } else {
        n
    }
}

/// 解析一个十进制整数。
fn parse_num(t: &[u8]) -> Result<i64, u32> {
    let neg = t.get(0).copied() == Some(b'-');
    let mut i = if neg { 1usize } else { 0usize };
    if i >= t.len() {
        return Err(SQ_EOPERAND);
    }
    let mut acc = 0i64;
    while i < t.len() {
        let b = match t.get(i).copied() {
            Some(v) => v,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(SQ_EOPERAND);
        }
        acc = acc.checked_mul(10).ok_or(SQ_ERANGE)?;
        acc = acc.checked_add((b - b'0') as i64).ok_or(SQ_ERANGE)?;
        if acc > NUM_MAX {
            return Err(SQ_ERANGE);
        }
        i += 1;
    }
    Ok(if neg { -acc } else { acc })
}

/// 是否像数值（允许前导负号）。
fn looks_num(t: &[u8]) -> bool {
    match t.get(0).copied() {
        Some(b'-') => matches!(t.get(1).copied(), Some(b) if b.is_ascii_digit()),
        Some(b) => b.is_ascii_digit(),
        None => false,
    }
}

/// 解析命令行。
pub fn parse_args(args: &[&[u8]]) -> Result<Plan, u32> {
    let mut ops = [0i64; 3];
    let mut nops = 0usize;
    let mut sep = Sep::newline();
    let mut sep_set = false;
    let mut equal = false;
    let mut fmt: Option<Fmt> = None;
    let mut i = 0usize;
    let mut end = false;
    while i < args.len() {
        let t: &[u8] = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !end && t == b"--" {
            end = true;
            i += 1;
            continue;
        }
        if !end && t.get(0).copied() == Some(b'-') && !looks_num(t) {
            if t == b"-w" {
                if equal {
                    return Err(SQ_EOPT);
                }
                equal = true;
                i += 1;
            } else if t == b"-s" {
                if sep_set {
                    return Err(SQ_EOPT);
                }
                let v: &[u8] = match args.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(SQ_EOPERAND),
                };
                sep = Sep::new(v)?;
                sep_set = true;
                i += 2;
            } else if t == b"-f" {
                if fmt.is_some() {
                    return Err(SQ_EOPT);
                }
                let v: &[u8] = match args.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(SQ_EOPERAND),
                };
                fmt = Some(parse_fmt(v)?);
                i += 2;
            } else {
                return Err(SQ_EOPT);
            }
        } else {
            if nops >= 3 {
                return Err(SQ_EOPERAND);
            }
            let v = parse_num(t)?;
            if let Some(slot) = ops.get_mut(nops) {
                *slot = v;
            }
            nops += 1;
            i += 1;
        }
    }
    if nops == 0 {
        return Err(SQ_EOPERAND);
    }
    if equal && fmt.is_some() {
        return Err(SQ_EOPT);
    }
    let o0 = match ops.get(0) {
        Some(v) => *v,
        None => return Err(SQ_EOPERAND),
    };
    let o1 = match ops.get(1) {
        Some(v) => *v,
        None => 1,
    };
    let o2 = match ops.get(2) {
        Some(v) => *v,
        None => 1,
    };
    let (first, step, last) = match nops {
        1 => (1i64, 1i64, o0),
        2 => (o0, 1i64, o1),
        _ => (o0, o1, o2),
    };
    if step == 0 {
        return Err(SQ_EZERO);
    }
    if step > STEP_MAX || step < -STEP_MAX {
        return Err(SQ_ERANGE);
    }
    if raw_count(first, step, last) > COUNT_MAX as u64 {
        return Err(SQ_ELONG);
    }
    Ok(Plan { first, step, last, sep, equal, fmt })
}

/// 抄字节串，返回长度。
fn copy(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    i as u8
}

/// 写字节串。
fn put(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    let mut i = 0usize;
    while i < src.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), src.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    at + src.len()
}

/// 写 n 个同字节。
fn fill(dst: &mut [u8], at: usize, b: u8, n: u32) -> usize {
    let mut cur = at;
    let mut i = 0u32;
    while i < n {
        if let Some(d) = dst.get_mut(cur) {
            *d = b;
        }
        cur += 1;
        i += 1;
    }
    cur
}

/// 十进制数字倒出到 out。
fn digits_i64(v: i64, out: &mut [u8]) -> usize {
    let mag = if v < 0 { (v as u64).wrapping_neg() } else { v as u64 };
    let mut tmp = [0u8; 20];
    let mut n = 0usize;
    let mut x = mag;
    if x == 0 {
        if let Some(c) = tmp.get_mut(0) {
            *c = b'0';
        }
        n = 1;
    }
    while x > 0 {
        if let Some(c) = tmp.get_mut(n) {
            *c = b'0' + (x % 10) as u8;
        }
        n += 1;
        x /= 10;
    }
    let mut i = 0usize;
    while i < n {
        let c = match tmp.get(n - 1 - i) {
            Some(v) => *v,
            None => b'0',
        };
        if let Some(d) = out.get_mut(i) {
            *d = c;
        }
        i += 1;
    }
    n
}

/// 写一个整数，按需补零或补空格。
fn put_i64(dst: &mut [u8], at: usize, v: i64, width: u32, zero: bool) -> usize {
    let neg = v < 0;
    let mut dig = [0u8; 20];
    let dn = digits_i64(v, &mut dig);
    let sign = if neg { 1u32 } else { 0u32 };
    let body = sign + dn as u32;
    let total = if width > body { width } else { body };
    let pad = total - body;
    let mut cur = at;
    if !zero {
        cur = fill(dst, cur, b' ', pad);
    }
    if neg {
        cur = put(dst, cur, b"-");
    }
    if zero {
        cur = fill(dst, cur, b'0', pad);
    }
    let bytes: &[u8] = match dig.get(..dn) {
        Some(v) => v,
        None => return cur,
    };
    put(dst, cur, bytes)
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
            self.rejects += 1;
            return Err(SQ_ESLOT);
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
            return Err(SQ_EBAL);
        }
        const VALS: [u32; 8] = [5, 3, 4, 10, 10, 9, 8, 5];
        const CODES: [u32; 6] =
            [SQ_EOPT, SQ_EOPERAND, SQ_EZERO, SQ_ERANGE, SQ_EBADFMT, SQ_EMFMT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SQ_EBAL),
            };
            if s.slot as usize != i {
                return Err(SQ_ESLOT);
            }
            if s.op == 0 {
                return Err(SQ_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SQ_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SQ_EVALUE);
                        }
                    }
                    None => return Err(SQ_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(SQ_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SQ_EVALUE);
                        }
                    }
                    None => return Err(SQ_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SQ_EBAL);
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
            return Err(SQ_EBAL);
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
            Err(SQ_ESEAL)
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

/// 十四格账本：八个成功观测 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut out = [0u8; 64];

    let _ = l.register(word_cell(
        0,
        OP_PLAN,
        parse_args(&[b"5"]).map(|p| p.last as u32),
    ));
    let _ = l.register(word_cell(
        1,
        OP_COUNT,
        parse_args(&[b"3", b"5"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        2,
        OP_COUNT,
        parse_args(&[b"10", b"-2", b"4"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        3,
        OP_LEN,
        parse_args(&[b"5"]).and_then(|p| p.print_len()),
    ));
    let _ = l.register(word_cell(
        4,
        OP_RENDER,
        parse_args(&[b"-s", b",", b"5"]).and_then(|p| p.render(&mut out)),
    ));
    let _ = l.register(word_cell(
        5,
        OP_RENDER,
        parse_args(&[b"-w", b"8", b"10"]).and_then(|p| p.render(&mut out)),
    ));
    let _ = l.register(word_cell(
        6,
        OP_RENDER,
        parse_args(&[b"-f", b"%03d", b"1", b"2"]).and_then(|p| p.render(&mut out)),
    ));
    let _ = l.register(word_cell(
        7,
        OP_RENDER,
        parse_args(&[b"-f", b"n=%d;", b"7", b"7"]).and_then(|p| p.render(&mut out)),
    ));

    let _ = l.register(word_cell(
        8,
        OP_PLAN,
        parse_args(&[b"-x"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        9,
        OP_PLAN,
        parse_args(&[b"1", b"2", b"3", b"4"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        10,
        OP_PLAN,
        parse_args(&[b"1", b"0", b"5"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        11,
        OP_PLAN,
        parse_args(&[b"1", b"9999999999999"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        12,
        OP_FMT,
        parse_args(&[b"-f", b"%q"]).map(|p| p.count()),
    ));
    let _ = l.register(word_cell(
        13,
        OP_FMT,
        parse_args(&[b"-f", b"%d%d"]).map(|p| p.count()),
    ));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：项数与端点。
    let a = match parse_args(&[b"5"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let b = match parse_args(&[b"10", b"-2", b"4"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let c = match parse_args(&[b"5", b"3"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = a.count() == 5
        && a.term(0) == Ok(1)
        && a.term(4) == Ok(5)
        && b.count() == 4
        && b.term(0) == Ok(10)
        && b.term(3) == Ok(4)
        && c.count() == 0
        && c.print_len() == Ok(0);

    // 二路：分隔与等宽。
    let mut b1 = [0u8; 32];
    let mut b2 = [0u8; 32];
    let mut b3 = [0u8; 32];
    let r1 = match parse_args(&[b"5"]) {
        Ok(p) => p.render(&mut b1),
        Err(c) => Err(c),
    };
    let r2 = match parse_args(&[b"-s", b",", b"3"]) {
        Ok(p) => p.render(&mut b2),
        Err(c) => Err(c),
    };
    let r3 = match parse_args(&[b"-w", b"8", b"10"]) {
        Ok(p) => p.render(&mut b3),
        Err(c) => Err(c),
    };
    out[2] = r1 == Ok(10)
        && b1.get(..10) == Some(&b"1\n2\n3\n4\n5\n"[..])
        && r2 == Ok(6)
        && b2.get(..6) == Some(&b"1,2,3,"[..])
        && r3 == Ok(9)
        && b3.get(..9) == Some(&b"08\n09\n10\n"[..]);

    // 三路：各类拒绝。
    let mut tiny = [0u8; 4];
    let five = match parse_args(&[b"5"]) {
        Ok(p) => p,
        Err(_) => return out,
    };
    out[3] = parse_args(&[b"-x"]).is_err()
        && parse_args(&[b"-x"]) == Err(SQ_EOPT)
        && parse_args(&[]) == Err(SQ_EOPERAND)
        && parse_args(&[b"1", b"0", b"5"]) == Err(SQ_EZERO)
        && parse_args(&[b"1", b"9999999999999"]) == Err(SQ_ERANGE)
        && parse_args(&[b"-f", b"%q"]) == Err(SQ_EBADFMT)
        && parse_args(&[b"-f", b"abc"]) == Err(SQ_EMFMT)
        && parse_args(&[b"-s", b"12345"]) == Err(SQ_ELONG)
        && five.render(&mut tiny) == Err(SQ_EBUF);

    out
}

/// 十三项判据。
pub fn run_unx11510_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11510.seq_range");
    let l = build_ledger();

    s.add_dyn(
        "SQ01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SQ02-默认等差一",
        {
            let a = match parse_args(&[b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"2", b"4"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match parse_args(&[b"1", b"2", b"9"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.first == 1
                && a.step == 1
                && a.last == 5
                && a.count() == 5
                && b.first == 2
                && b.step == 1
                && b.last == 4
                && b.count() == 3
                && c.count() == 5
                && c.term(4) == Ok(9)
                && l.sample(0).map(|x| x.value) == Some(5)
        },
        "static",
    );

    s.add(
        "SQ03-负步长与空列",
        {
            let a = match parse_args(&[b"10", b"-2", b"4"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"5", b"3"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.count() == 4
                && a.term(0) == Ok(10)
                && a.term(1) == Ok(8)
                && a.term(2) == Ok(6)
                && a.term(3) == Ok(4)
                && b.count() == 0
                && b.print_len() == Ok(0)
                && l.sample(2).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "SQ04-计数与端点",
        {
            let a = match parse_args(&[b"1", b"1", b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"1", b"3", b"10"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match parse_args(&[b"1", b"1000", b"1"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let d = match parse_args(&[b"-5", b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.count() == 5
                && b.count() == 4
                && b.term(3) == Ok(10)
                && c.count() == 1
                && d.count() == 11
                && d.term(0) == Ok(-5)
                && d.term(10) == Ok(5)
        },
        "static",
    );

    s.add(
        "SQ05-默认换行收尾",
        {
            let p = match parse_args(&[b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut buf = [0u8; 32];
            p.print_len() == Ok(10)
                && p.render(&mut buf) == Ok(10)
                && buf.get(..10) == Some(&b"1\n2\n3\n4\n5\n"[..])
                && l.sample(3).map(|x| x.value) == Some(10)
        },
        "static",
    );

    s.add(
        "SQ06-自定义分隔串",
        {
            let a = match parse_args(&[b"-s", b",", b"3"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"-s", b"", b"3"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match parse_args(&[b"-s", b" | ", b"3"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut buf = [0u8; 32];
            a.render(&mut buf) == Ok(6)
                && buf.get(..6) == Some(&b"1,2,3,"[..])
                && b.render(&mut buf) == Ok(3)
                && buf.get(..3) == Some(&b"123"[..])
                && c.print_len() == Ok(12)
                && c.sep.as_bytes() == b" | "
                && l.sample(4).map(|x| x.value) == Some(10)
        },
        "static",
    );

    s.add(
        "SQ07-等宽补零",
        {
            let a = match parse_args(&[b"-w", b"8", b"10"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"-w", b"-2", b"2"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut buf = [0u8; 32];
            a.print_len() == Ok(9)
                && a.render(&mut buf) == Ok(9)
                && buf.get(..9) == Some(&b"08\n09\n10\n"[..])
                && b.count() == 5
                && b.print_len() == Ok(15)
                && b.render(&mut buf) == Ok(15)
                && buf.get(..15) == Some(&b"-2\n-1\n00\n01\n02\n"[..])
                && l.sample(5).map(|x| x.value) == Some(9)
        },
        "static",
    );

    s.add(
        "SQ08-格式转换",
        {
            let a = match parse_args(&[b"-f", b"%03d", b"1", b"2"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match parse_args(&[b"-f", b"n=%d;", b"7", b"7"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let c = match parse_args(&[b"-f", b"%d", b"5", b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut buf = [0u8; 32];
            a.render(&mut buf) == Ok(8)
                && buf.get(..8) == Some(&b"001\n002\n"[..])
                && b.render(&mut buf) == Ok(5)
                && buf.get(..5) == Some(&b"n=7;\n"[..])
                && c.render(&mut buf) == Ok(2)
                && buf.get(..2) == Some(&b"5\n"[..])
                && l.sample(6).map(|x| x.value) == Some(8)
                && l.sample(7).map(|x| x.value) == Some(5)
        },
        "static",
    );

    s.add(
        "SQ09-格式文法闸门",
        {
            parse_fmt(b"%q") == Err(SQ_EBADFMT)
                && parse_fmt(b"abc") == Err(SQ_EMFMT)
                && parse_fmt(b"%d%d") == Err(SQ_EMFMT)
                && parse_fmt(b"%0d") == Err(SQ_EBADFMT)
                && parse_fmt(b"%10d") == Err(SQ_EBADFMT)
                && parse_fmt(b"x%") == Err(SQ_EBADFMT)
                && parse_fmt(b"%3d") == Ok(Fmt {
                    pre: [0u8; LIT_CAP],
                    plen: 0,
                    post: [0u8; LIT_CAP],
                    qlen: 0,
                    zero: false,
                    width: 3,
                })
                && l.sample(12).map(|x| x.code) == Some(SQ_EBADFMT)
                && l.sample(13).map(|x| x.code) == Some(SQ_EMFMT)
        },
        "static",
    );

    s.add(
        "SQ10-操作数闸门",
        {
            parse_args(&[]) == Err(SQ_EOPERAND)
                && parse_args(&[b"1", b"2", b"3", b"4"]) == Err(SQ_EOPERAND)
                && parse_args(&[b"1", b"0", b"5"]) == Err(SQ_EZERO)
                && parse_args(&[b"-x"]) == Err(SQ_EOPT)
                && parse_args(&[b"-s"]) == Err(SQ_EOPERAND)
                && parse_args(&[b"-w", b"-w", b"5"]) == Err(SQ_EOPT)
                && parse_args(&[b"-w", b"-f", b"%d", b"5"]) == Err(SQ_EOPT)
                && parse_args(&[b"-f", b"%d", b"-f", b"%d", b"5"]) == Err(SQ_EOPT)
                && l.sample(8).map(|x| x.code) == Some(SQ_EOPT)
                && l.sample(9).map(|x| x.code) == Some(SQ_EOPERAND)
                && l.sample(10).map(|x| x.code) == Some(SQ_EZERO)
        },
        "static",
    );

    s.add(
        "SQ11-数值边界",
        {
            parse_args(&[b"9999999999999"]) == Err(SQ_ERANGE)
                && parse_args(&[b"-9999999999999"]) == Err(SQ_ERANGE)
                && parse_args(&[b"1", b"1000000001", b"5"]) == Err(SQ_ERANGE)
                && parse_args(&[b"1", b"1", b"1000000"]) == Err(SQ_ELONG)
                && parse_args(&[b"1x"]) == Err(SQ_EOPERAND)
                && parse_args(&[b"-"]) == Err(SQ_EOPT)
                && parse_args(&[b"--", b"5"]) == Ok(Plan {
                    first: 1,
                    step: 1,
                    last: 5,
                    sep: Sep::newline(),
                    equal: false,
                    fmt: None,
                })
                && l.sample(11).map(|x| x.code) == Some(SQ_ERANGE)
        },
        "static",
    );

    s.add(
        "SQ12-缓冲与分量边界",
        {
            let p = match parse_args(&[b"5"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut exact = [0u8; 10];
            let mut short = [0xaa; 9];
            let ok = p.render(&mut exact) == Ok(10) && exact.get(..10) == Some(&b"1\n2\n3\n4\n5\n"[..]);
            let bad = p.render(&mut short) == Err(SQ_EBUF);
            let intact = short.get(..9) == Some(&[0xaa; 9][..]);
            ok && bad
                && intact
                && parse_args(&[b"-s", b"12345"]) == Err(SQ_ELONG)
                && parse_args(&[b"-f", b"123456789%d"]) == Err(SQ_ELONG)
        },
        "static",
    );

    s.add(
        "SQ13-冻结重放四注入",
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

    fn plan(args: &[&[u8]]) -> Plan {
        match parse_args(args) {
            Ok(v) => v,
            Err(_) => panic!("plan"),
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
        let s = run_unx11510_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn defaults_and_terms() {
        let p = plan(&[b"5"]);
        assert_eq!((p.first, p.step, p.last), (1, 1, 5));
        assert_eq!(p.count(), 5);
        assert_eq!(p.term(0), Ok(1));
        assert_eq!(p.term(4), Ok(5));
        let q = plan(&[b"1", b"2", b"9"]);
        assert_eq!(q.count(), 5);
        assert_eq!(q.term(4), Ok(9));
        let r = plan(&[b"5", b"3"]);
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn negative_step() {
        let p = plan(&[b"10", b"-2", b"4"]);
        assert_eq!(p.count(), 4);
        assert_eq!(p.term(0), Ok(10));
        assert_eq!(p.term(3), Ok(4));
        let q = plan(&[b"-5", b"5"]);
        assert_eq!(q.count(), 11);
        assert_eq!(q.term(10), Ok(5));
    }

    #[test]
    fn render_sep_and_width() {
        let p = plan(&[b"3"]);
        let mut b = [0u8; 32];
        assert_eq!(p.render(&mut b), Ok(6));
        assert_eq!(&b[..6], b"1\n2\n3\n");
        let q = plan(&[b"-s", b",", b"3"]);
        assert_eq!(q.render(&mut b), Ok(6));
        assert_eq!(&b[..6], b"1,2,3,");
        let r = plan(&[b"-w", b"8", b"10"]);
        assert_eq!(r.render(&mut b), Ok(9));
        assert_eq!(&b[..9], b"08\n09\n10\n");
    }

    #[test]
    fn formats() {
        let p = plan(&[b"-f", b"%03d", b"1", b"2"]);
        let mut b = [0u8; 32];
        assert_eq!(p.render(&mut b), Ok(8));
        assert_eq!(&b[..8], b"001\n002\n");
        let q = plan(&[b"-f", b"n=%d;", b"7", b"7"]);
        assert_eq!(q.render(&mut b), Ok(5));
        assert_eq!(&b[..5], b"n=7;\n");
        assert_eq!(parse_fmt(b"%3d").map(|f| f.width()), Ok(3));
        assert_eq!(parse_fmt(b"%03d").map(|f| f.zero()), Ok(true));
    }

    #[test]
    fn arg_gates() {
        assert_eq!(parse_args(&[]), Err(SQ_EOPERAND));
        assert_eq!(parse_args(&[b"-x"]), Err(SQ_EOPT));
        assert_eq!(parse_args(&[b"1", b"0", b"5"]), Err(SQ_EZERO));
        assert_eq!(parse_args(&[b"1", b"9999999999999"]), Err(SQ_ERANGE));
        assert_eq!(parse_args(&[b"1", b"1", b"1000000"]), Err(SQ_ELONG));
        assert_eq!(parse_args(&[b"-f", b"%q"]), Err(SQ_EBADFMT));
        assert_eq!(parse_args(&[b"-f", b"abc"]), Err(SQ_EMFMT));
        assert_eq!(parse_args(&[b"-s", b"12345"]), Err(SQ_ELONG));
    }

    #[test]
    fn buffer_guard() {
        let p = plan(&[b"5"]);
        let mut short = [0xaa; 9];
        assert_eq!(p.render(&mut short), Err(SQ_EBUF));
        assert_eq!(&short[..], &[0xaa; 9][..]);
        let mut exact = [0u8; 10];
        assert_eq!(p.render(&mut exact), Ok(10));
        assert_eq!(&exact[..], b"1\n2\n3\n4\n5\n");
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
