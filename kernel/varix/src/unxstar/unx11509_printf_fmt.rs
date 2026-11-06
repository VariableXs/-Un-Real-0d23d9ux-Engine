//! UNX-F11509 printf 格式化输出（0xf0e0 段）
//!
//! `printf` 的骨架是一个小状态机：普通字节直接抄，`%` 之后进入**规格**，
//! 依次读旗标、宽度、精度、转换符，然后从参数表里取下一个值，按规格
//! 展开成字节。本卡把"解析规格"与"取值展开"拆开：第一遍只解析、验型、
//! 累加总长，第二遍才写缓冲。这样缓冲不足时一格不写，不会留下半截输出。
//!
//! 规格文法：`%[flags][width][.precision]conv`。旗标只认 `-`（左对齐）、
//! `0`（零填充）、`+`（正号），重复出现是 `EBADSPEC`；宽度最多两位且
//! 不超过 64；精度最多两位且不超过 20，`%.` 后没有数字是 `EBADSPEC`。
//! 转换符只认 `d i u x X o s c %`，其余是 `EBADFMT`；串尾孤立的 `%`
//! 是 `ETRUNC`。`+` 只许用于有符号十进制，用在 `u/x/X/o/s` 上是 `EFLAG`；
//! `0` 用在 `s` 上是 `EFLAG`；`c` 带精度、`%` 带任何修饰都是 `EBADSPEC`。
//!
//! 有符号转换把参数按二进制补码解释（`0xffff_ffff` 就是 `-1`），取绝对值
//! 用 `wrapping_neg`，因此 `i32::MIN` 也能打印。精度对整数是"最少位数"，
//! 值为 0 且精度为 0 时输出零个数字；宽度是"整段最小宽度"，零填充只在
//! 没有精度时生效，且符号永远在零之前（`%+05d` 的 42 是 `+0042`）。
//! 字符串的精度是"最多保留几个字节"，不做字符边界判断——这是字节级
//! 的 printf，不是 Unicode 排版器。
//!
//! 参数表是 `Arg::Int(u32)` 与 `Arg::Str(&[u8])` 的切片：类型不对是
//! `ETYPE`，参数用尽还要取是 `EARGNUM`，`%c` 的值超过 0xff 是 `ERANGE`。
//! 多余的参数被忽略——与 C 的 printf 一致，计数只跟着格式串走。
//!
//! 账本十四格：八格成功（十进制、零填充、左对齐、十六进制、八进制、
//! 精度、字符串、宽度加精度），六格拒绝（未知转换符、尾部百分号、
//! 非法旗标、缺参数、错类型、字符越界）。审计逐格复算，封印折叠全账。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0e0;

/// 未知转换符。
pub const PR_EBADFMT: u32 = SEG_BASE + 0x01;
/// 串尾孤立百分号。
pub const PR_ETRUNC: u32 = SEG_BASE + 0x02;
/// 规格结构非法。
pub const PR_EBADSPEC: u32 = SEG_BASE + 0x03;
/// 参数用尽。
pub const PR_EARGNUM: u32 = SEG_BASE + 0x04;
/// 参数类型不符。
pub const PR_ETYPE: u32 = SEG_BASE + 0x05;
/// 值超出转换范围。
pub const PR_ERANGE: u32 = SEG_BASE + 0x06;
/// 输出缓冲不足。
pub const PR_EBUF: u32 = SEG_BASE + 0x07;
/// 旗标与转换符不搭。
pub const PR_EFLAG: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PR_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PR_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 宽度上限。
pub const WIDTH_MAX: u32 = 64;
/// 精度上限。
pub const PREC_MAX: u32 = 20;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：渲染。
pub const OP_FORMAT: u8 = 1;
/// 操作码：字符串。
pub const OP_STR: u8 = 2;
/// 操作码：规格拒绝。
pub const OP_SPEC: u8 = 3;

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

/// 一个实参。
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    /// 整数值。
    Int(u32),
    /// 字节串。
    Str(&'a [u8]),
}

/// 一条转换规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// 左对齐。
    pub left: bool,
    /// 零填充。
    pub zero: bool,
    /// 正号。
    pub plus: bool,
    /// 最小宽度。
    pub width: u32,
    /// 精度。
    pub prec: Option<u32>,
    /// 转换符。
    pub conv: u8,
}

/// 解析 `%` 起的规格，返回规格与下一位置。
pub fn parse_spec(fmt: &[u8], start: usize) -> Result<(Spec, usize), u32> {
    let mut i = start + 1;
    let mut left = false;
    let mut zero = false;
    let mut plus = false;
    loop {
        match fmt.get(i).copied() {
            Some(b'-') => {
                if left {
                    return Err(PR_EBADSPEC);
                }
                left = true;
                i += 1;
            }
            Some(b'0') => {
                if zero {
                    return Err(PR_EBADSPEC);
                }
                zero = true;
                i += 1;
            }
            Some(b'+') => {
                if plus {
                    return Err(PR_EBADSPEC);
                }
                plus = true;
                i += 1;
            }
            _ => break,
        }
    }
    let mut width = 0u32;
    let mut wd = 0u32;
    while wd < 2 {
        let b = match fmt.get(i).copied() {
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
    if width > WIDTH_MAX {
        return Err(PR_EBADSPEC);
    }
    let mut prec = None;
    if fmt.get(i).copied() == Some(b'.') {
        i += 1;
        let mut p = 0u32;
        let mut pd = 0u32;
        while pd < 2 {
            let b = match fmt.get(i).copied() {
                Some(v) => v,
                None => break,
            };
            if !b.is_ascii_digit() {
                break;
            }
            p = p * 10 + (b - b'0') as u32;
            i += 1;
            pd += 1;
        }
        if pd == 0 {
            return Err(PR_EBADSPEC);
        }
        if p > PREC_MAX {
            return Err(PR_EBADSPEC);
        }
        prec = Some(p);
    }
    let conv = match fmt.get(i).copied() {
        Some(v) => v,
        None => return Err(PR_ETRUNC),
    };
    i += 1;
    if conv == b'%' {
        if left || zero || plus || width != 0 || prec.is_some() {
            return Err(PR_EBADSPEC);
        }
        return Ok((Spec { left, zero, plus, width, prec, conv }, i));
    }
    match conv {
        b'd' | b'i' => {}
        b'u' | b'x' | b'X' | b'o' => {
            if plus {
                return Err(PR_EFLAG);
            }
        }
        b's' => {
            if plus || zero {
                return Err(PR_EFLAG);
            }
        }
        b'c' => {
            if plus || zero || prec.is_some() {
                return Err(PR_EBADSPEC);
            }
        }
        _ => return Err(PR_EBADFMT),
    }
    Ok((Spec { left, zero, plus, width, prec, conv }, i))
}

/// 取整数实参。
fn want_int(a: Arg) -> Result<u32, u32> {
    match a {
        Arg::Int(v) => Ok(v),
        Arg::Str(_) => Err(PR_ETYPE),
    }
}

/// 取字符串实参。
fn want_str<'a>(a: Arg<'a>) -> Result<&'a [u8], u32> {
    match a {
        Arg::Int(_) => Err(PR_ETYPE),
        Arg::Str(v) => Ok(v),
    }
}

/// 进制位数（0 记一位）。
fn base_digits(mut v: u32, base: u32) -> u32 {
    let mut n = 1u32;
    while v >= base {
        v /= base;
        n += 1;
    }
    n
}

/// 整数展开后的长度。
fn int_len(sp: &Spec, v: u32, base: u32) -> u32 {
    let neg = sp.conv == b'd' || sp.conv == b'i';
    let signed = neg && (v as i32) < 0;
    let mag = if signed { v.wrapping_neg() } else { v };
    let mut dl = base_digits(mag, base);
    if let Some(p) = sp.prec {
        if p == 0 && mag == 0 {
            dl = 0;
        } else if p > dl {
            dl = p;
        }
    }
    let sign = if signed || sp.plus { 1 } else { 0 };
    let body = dl + sign;
    if sp.width > body {
        sp.width
    } else {
        body
    }
}

/// 一条规格展开后的长度。
fn spec_len(sp: &Spec, arg: Arg) -> Result<u32, u32> {
    match sp.conv {
        b'd' | b'i' => {
            let v = want_int(arg)?;
            Ok(int_len(sp, v, 10))
        }
        b'u' => {
            let v = want_int(arg)?;
            Ok(int_len(sp, v, 10))
        }
        b'x' | b'X' => {
            let v = want_int(arg)?;
            Ok(int_len(sp, v, 16))
        }
        b'o' => {
            let v = want_int(arg)?;
            Ok(int_len(sp, v, 8))
        }
        b's' => {
            let s = want_str(arg)?;
            let mut n = s.len() as u32;
            if let Some(p) = sp.prec {
                if p < n {
                    n = p;
                }
            }
            Ok(if sp.width > n { sp.width } else { n })
        }
        b'c' => {
            let v = want_int(arg)?;
            if v > 0xff {
                return Err(PR_ERANGE);
            }
            Ok(if sp.width > 1 { sp.width } else { 1 })
        }
        b'%' => Ok(1),
        _ => Err(PR_EBADFMT),
    }
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

/// 十进制/十六进制/八进制数字倒出到 out，返回位数。
fn digits_into(v: u32, base: u32, upper: bool, out: &mut [u8]) -> usize {
    let mut tmp = [0u8; 12];
    let mut n = 0usize;
    let mut x = v;
    if x == 0 {
        if let Some(c) = tmp.get_mut(0) {
            *c = b'0';
        }
        n = 1;
    }
    while x > 0 {
        let d = (x % base) as u8;
        let c = if d < 10 {
            b'0' + d
        } else if upper {
            b'A' + d - 10
        } else {
            b'a' + d - 10
        };
        if let Some(cell) = tmp.get_mut(n) {
            *cell = c;
        }
        n += 1;
        x /= base;
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

/// 写一条整数规格。
fn emit_int(dst: &mut [u8], at: usize, sp: &Spec, v: u32) -> usize {
    let signed_conv = sp.conv == b'd' || sp.conv == b'i';
    let neg = signed_conv && (v as i32) < 0;
    let mag = if neg { v.wrapping_neg() } else { v };
    let (base, upper) = match sp.conv {
        b'x' => (16u32, false),
        b'X' => (16u32, true),
        b'o' => (8u32, false),
        _ => (10u32, false),
    };
    let mut dig = [0u8; 12];
    let dn = digits_into(mag, base, upper, &mut dig);
    let mut dl = dn as u32;
    if let Some(p) = sp.prec {
        if p == 0 && mag == 0 {
            dl = 0;
        } else if p > dl {
            dl = p;
        }
    }
    let sign: u8 = if neg {
        b'-'
    } else if sp.plus {
        b'+'
    } else {
        0
    };
    let has_sign = sign != 0;
    let body = dl + if has_sign { 1 } else { 0 };
    let total = if sp.width > body { sp.width } else { body };
    let pad = total - body;
    let zpad = if sp.zero && !sp.left && sp.prec.is_none() { pad } else { 0 };
    let spad = pad - zpad;
    let mut cur = at;
    if !sp.left {
        cur = fill(dst, cur, b' ', spad);
    }
    if has_sign {
        cur = put(dst, cur, &[sign]);
    }
    cur = fill(dst, cur, b'0', zpad);
    if dl > dn as u32 {
        cur = fill(dst, cur, b'0', dl - dn as u32);
    }
    if dl > 0 {
        let body_bytes: &[u8] = match dig.get(..dn) {
            Some(v) => v,
            None => return cur,
        };
        cur = put(dst, cur, body_bytes);
    }
    if sp.left {
        cur = fill(dst, cur, b' ', spad);
    }
    cur
}

/// 写一条规格。
fn emit(dst: &mut [u8], at: usize, sp: &Spec, arg: Arg) -> usize {
    match sp.conv {
        b'd' | b'i' | b'u' | b'x' | b'X' | b'o' => match arg {
            Arg::Int(v) => emit_int(dst, at, sp, v),
            Arg::Str(_) => at,
        },
        b's' => match arg {
            Arg::Str(s) => {
                let mut n = s.len();
                if let Some(p) = sp.prec {
                    if (p as usize) < n {
                        n = p as usize;
                    }
                }
                let pad = sp.width.saturating_sub(n as u32);
                let mut cur = at;
                if !sp.left {
                    cur = fill(dst, cur, b' ', pad);
                }
                let body: &[u8] = match s.get(..n) {
                    Some(v) => v,
                    None => return cur,
                };
                cur = put(dst, cur, body);
                if sp.left {
                    cur = fill(dst, cur, b' ', pad);
                }
                cur
            }
            Arg::Int(_) => at,
        },
        b'c' => match arg {
            Arg::Int(v) => {
                let pad = sp.width.saturating_sub(1);
                let mut cur = at;
                if !sp.left {
                    cur = fill(dst, cur, b' ', pad);
                }
                cur = put(dst, cur, &[(v & 0xff) as u8]);
                if sp.left {
                    cur = fill(dst, cur, b' ', pad);
                }
                cur
            }
            Arg::Str(_) => at,
        },
        b'%' => put(dst, at, b"%"),
        _ => at,
    }
}

/// 按格式串渲染参数表。
pub fn sprintf(fmt: &[u8], args: &[Arg], dst: &mut [u8]) -> Result<u32, u32> {
    let mut need = 0u32;
    let mut ai = 0usize;
    let mut i = 0usize;
    while i < fmt.len() {
        match fmt.get(i).copied() {
            Some(b'%') => {
                let (sp, next) = parse_spec(fmt, i)?;
                if sp.conv != b'%' {
                    let a: Arg = match args.get(ai) {
                        Some(v) => *v,
                        None => return Err(PR_EARGNUM),
                    };
                    ai += 1;
                    need = need.saturating_add(spec_len(&sp, a)?);
                } else {
                    need = need.saturating_add(1);
                }
                i = next;
            }
            Some(_) => {
                need = need.saturating_add(1);
                i += 1;
            }
            None => break,
        }
    }
    if need as usize > dst.len() {
        return Err(PR_EBUF);
    }
    let mut at = 0usize;
    let mut ai2 = 0usize;
    let mut j = 0usize;
    while j < fmt.len() {
        match fmt.get(j).copied() {
            Some(b'%') => {
                let (sp, next) = parse_spec(fmt, j)?;
                if sp.conv == b'%' {
                    at = emit(dst, at, &sp, Arg::Int(0));
                } else {
                    let a: Arg = match args.get(ai2) {
                        Some(v) => *v,
                        None => return Err(PR_EARGNUM),
                    };
                    ai2 += 1;
                    at = emit(dst, at, &sp, a);
                }
                j = next;
            }
            Some(b) => {
                at = put(dst, at, &[b]);
                j += 1;
            }
            None => break,
        }
    }
    Ok(at as u32)
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
            return Err(PR_ESLOT);
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
            return Err(PR_EBAL);
        }
        const VALS: [u32; 8] = [2, 5, 6, 8, 2, 3, 3, 8];
        const CODES: [u32; 6] =
            [PR_EBADFMT, PR_ETRUNC, PR_EFLAG, PR_EARGNUM, PR_ETYPE, PR_ERANGE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PR_EBAL),
            };
            if s.slot as usize != i {
                return Err(PR_ESLOT);
            }
            if s.op == 0 {
                return Err(PR_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PR_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PR_EVALUE);
                        }
                    }
                    None => return Err(PR_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(PR_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PR_EVALUE);
                        }
                    }
                    None => return Err(PR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PR_EBAL);
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
            return Err(PR_EBAL);
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
            Err(PR_ESEAL)
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
        OP_FORMAT,
        sprintf(b"%d", &[Arg::Int(42)], &mut out),
    ));
    let _ = l.register(word_cell(
        1,
        OP_FORMAT,
        sprintf(b"%05d", &[Arg::Int(42)], &mut out),
    ));
    let _ = l.register(word_cell(
        2,
        OP_FORMAT,
        sprintf(b"%-5u|", &[Arg::Int(7)], &mut out),
    ));
    let _ = l.register(word_cell(
        3,
        OP_FORMAT,
        sprintf(b"%08x", &[Arg::Int(0xdead)], &mut out),
    ));
    let _ = l.register(word_cell(
        4,
        OP_FORMAT,
        sprintf(b"%o", &[Arg::Int(8)], &mut out),
    ));
    let _ = l.register(word_cell(
        5,
        OP_FORMAT,
        sprintf(b"%.3d", &[Arg::Int(5)], &mut out),
    ));
    let _ = l.register(word_cell(
        6,
        OP_STR,
        sprintf(b"%s!", &[Arg::Str(b"ok")], &mut out),
    ));
    let _ = l.register(word_cell(
        7,
        OP_STR,
        sprintf(b"[%6.2s]", &[Arg::Str(b"abcdef")], &mut out),
    ));

    let _ = l.register(word_cell(
        8,
        OP_SPEC,
        sprintf(b"%q", &[Arg::Int(1)], &mut out),
    ));
    let _ = l.register(word_cell(
        9,
        OP_SPEC,
        sprintf(b"abc%", &[Arg::Int(1)], &mut out),
    ));
    let _ = l.register(word_cell(
        10,
        OP_SPEC,
        sprintf(b"%+x", &[Arg::Int(1)], &mut out),
    ));
    let _ = l.register(word_cell(
        11,
        OP_SPEC,
        sprintf(b"%d %d", &[Arg::Int(1)], &mut out),
    ));
    let _ = l.register(word_cell(
        12,
        OP_SPEC,
        sprintf(b"%s", &[Arg::Int(1)], &mut out),
    ));
    let _ = l.register(word_cell(
        13,
        OP_SPEC,
        sprintf(b"%c", &[Arg::Int(0x100)], &mut out),
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

    // 一路：符号、零填充、宽度。
    let mut b1 = [0u8; 32];
    let mut b2 = [0u8; 32];
    let mut b3 = [0u8; 32];
    let r1 = sprintf(b"%d", &[Arg::Int(0xffff_ffff)], &mut b1);
    let r2 = sprintf(b"%+05d", &[Arg::Int(42)], &mut b2);
    let r3 = sprintf(b"%-5u|", &[Arg::Int(7)], &mut b3);
    out[1] = r1 == Ok(2)
        && b1.get(..2) == Some(&b"-1"[..])
        && r2 == Ok(5)
        && b2.get(..5) == Some(&b"+0042"[..])
        && r3 == Ok(6)
        && b3.get(..6) == Some(&b"7    |"[..]);

    // 二路：字符串、字符、百分号、零精度。
    let mut c1 = [0u8; 16];
    let mut c2 = [0u8; 16];
    let mut c3 = [0u8; 16];
    let mut c4 = [0u8; 16];
    let r4 = sprintf(b"%6.2s", &[Arg::Str(b"abcdef")], &mut c1);
    let r5 = sprintf(b"a%%b", &[Arg::Int(0)], &mut c2);
    let r6 = sprintf(b"%c", &[Arg::Int(65)], &mut c3);
    let r7 = sprintf(b"%.0d", &[Arg::Int(0)], &mut c4);
    out[2] = r4 == Ok(6)
        && c1.get(..6) == Some(&b"    ab"[..])
        && r5 == Ok(3)
        && c2.get(..3) == Some(&b"a%b"[..])
        && r6 == Ok(1)
        && c3.get(..1) == Some(&b"A"[..])
        && r7 == Ok(0);

    // 三路：八类拒绝各自可辨。
    let mut tiny = [0u8; 4];
    out[3] = sprintf(b"%q", &[Arg::Int(1)], &mut tiny) == Err(PR_EBADFMT)
        && sprintf(b"abc%", &[Arg::Int(1)], &mut tiny) == Err(PR_ETRUNC)
        && sprintf(b"%+x", &[Arg::Int(1)], &mut tiny) == Err(PR_EFLAG)
        && sprintf(b"%d %d", &[Arg::Int(1)], &mut tiny) == Err(PR_EARGNUM)
        && sprintf(b"%s", &[Arg::Int(1)], &mut tiny) == Err(PR_ETYPE)
        && sprintf(b"%c", &[Arg::Int(0x100)], &mut tiny) == Err(PR_ERANGE)
        && sprintf(b"%05d", &[Arg::Int(42)], &mut tiny) == Err(PR_EBUF)
        && sprintf(b"%.d", &[Arg::Int(1)], &mut tiny) == Err(PR_EBADSPEC);

    out
}

/// 十三项判据。
pub fn run_unx11509_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11509.printf_fmt");
    let l = build_ledger();

    s.add_dyn(
        "PR01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PR02-十进制与符号",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            sprintf(b"%d", &[Arg::Int(42)], &mut a) == Ok(2)
                && a.get(..2) == Some(&b"42"[..])
                && sprintf(b"%d", &[Arg::Int(0xffff_ffff)], &mut b) == Ok(2)
                && b.get(..2) == Some(&b"-1"[..])
                && sprintf(b"%+d", &[Arg::Int(7)], &mut c) == Ok(2)
                && c.get(..2) == Some(&b"+7"[..])
                && l.sample(0).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "PR03-零填充与左对齐",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            sprintf(b"%05d", &[Arg::Int(42)], &mut a) == Ok(5)
                && a.get(..5) == Some(&b"00042"[..])
                && sprintf(b"%-5u|", &[Arg::Int(7)], &mut b) == Ok(6)
                && b.get(..6) == Some(&b"7    |"[..])
                && sprintf(b"%+05d", &[Arg::Int(42)], &mut c) == Ok(5)
                && c.get(..5) == Some(&b"+0042"[..])
                && l.sample(1).map(|x| x.value) == Some(5)
                && l.sample(2).map(|x| x.value) == Some(6)
        },
        "static",
    );

    s.add(
        "PR04-十六进制大小写",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            sprintf(b"%x", &[Arg::Int(0xdead)], &mut a) == Ok(4)
                && a.get(..4) == Some(&b"dead"[..])
                && sprintf(b"%X", &[Arg::Int(0xbeef)], &mut b) == Ok(4)
                && b.get(..4) == Some(&b"BEEF"[..])
                && sprintf(b"%08X", &[Arg::Int(0x1a)], &mut c) == Ok(8)
                && c.get(..8) == Some(&b"0000001A"[..])
                && l.sample(3).map(|x| x.value) == Some(8)
        },
        "static",
    );

    s.add(
        "PR05-八进制与整数精度",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            let mut d = [0u8; 16];
            sprintf(b"%o", &[Arg::Int(8)], &mut a) == Ok(2)
                && a.get(..2) == Some(&b"10"[..])
                && sprintf(b"%.3d", &[Arg::Int(5)], &mut b) == Ok(3)
                && b.get(..3) == Some(&b"005"[..])
                && sprintf(b"%.0d", &[Arg::Int(0)], &mut c) == Ok(0)
                && sprintf(b"%.3x", &[Arg::Int(0x1f)], &mut d) == Ok(3)
                && d.get(..3) == Some(&b"01f"[..])
                && l.sample(4).map(|x| x.value) == Some(2)
                && l.sample(5).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "PR06-字符串宽度与精度",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            let mut d = [0u8; 16];
            sprintf(b"%s", &[Arg::Str(b"hello")], &mut a) == Ok(5)
                && a.get(..5) == Some(&b"hello"[..])
                && sprintf(b"%.2s", &[Arg::Str(b"abcdef")], &mut b) == Ok(2)
                && b.get(..2) == Some(&b"ab"[..])
                && sprintf(b"%6.2s", &[Arg::Str(b"abcdef")], &mut c) == Ok(6)
                && c.get(..6) == Some(&b"    ab"[..])
                && sprintf(b"[%6.2s]", &[Arg::Str(b"abcdef")], &mut d) == Ok(8)
                && d.get(..8) == Some(&b"[    ab]"[..])
                && sprintf(b"%s", &[Arg::Str(b"")], &mut a) == Ok(0)
                && l.sample(6).map(|x| x.value) == Some(3)
                && l.sample(7).map(|x| x.value) == Some(8)
        },
        "static",
    );

    s.add(
        "PR07-字符转换",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            sprintf(b"%c", &[Arg::Int(65)], &mut a) == Ok(1)
                && a.get(..1) == Some(&b"A"[..])
                && sprintf(b"%3c", &[Arg::Int(66)], &mut b) == Ok(3)
                && b.get(..3) == Some(&b"  B"[..])
                && sprintf(b"%c", &[Arg::Int(0x100)], &mut a) == Err(PR_ERANGE)
                && sprintf(b"%c", &[Arg::Str(b"x")], &mut a) == Err(PR_ETYPE)
        },
        "static",
    );

    s.add(
        "PR08-百分号直出",
        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut c = [0u8; 16];
            sprintf(b"a%%b", &[Arg::Int(0)], &mut a) == Ok(3)
                && a.get(..3) == Some(&b"a%b"[..])
                && sprintf(b"%d%%", &[Arg::Int(50)], &mut b) == Ok(3)
                && b.get(..3) == Some(&b"50%"[..])
                && sprintf(b"100%%", &[Arg::Int(0)], &mut c) == Ok(4)
                && c.get(..4) == Some(&b"100%"[..])
                && sprintf(b"%%", &[Arg::Int(0)], &mut c) == Ok(1)
        },
        "static",
    );

    s.add(
        "PR09-未知转换符",
        {
            let mut a = [0u8; 16];
            sprintf(b"%q", &[Arg::Int(1)], &mut a) == Err(PR_EBADFMT)
                && sprintf(b"%y", &[Arg::Int(1)], &mut a) == Err(PR_EBADFMT)
                && sprintf(b"%d%q", &[Arg::Int(1)], &mut a) == Err(PR_EBADFMT)
                && sprintf(b"%", &[Arg::Int(1)], &mut a) == Err(PR_ETRUNC)
                && sprintf(b"abc%", &[Arg::Int(1)], &mut a) == Err(PR_ETRUNC)
                && sprintf(b"%5", &[Arg::Int(1)], &mut a) == Err(PR_ETRUNC)
                && l.sample(8).map(|x| x.code) == Some(PR_EBADFMT)
        },
        "static",
    );

    s.add(
        "PR10-规格结构闸门",
        {
            let mut a = [0u8; 16];
            sprintf(b"%.d", &[Arg::Int(1)], &mut a) == Err(PR_EBADSPEC)
                && sprintf(b"%.21d", &[Arg::Int(1)], &mut a) == Err(PR_EBADSPEC)
                && sprintf(b"%.2c", &[Arg::Int(1)], &mut a) == Err(PR_EBADSPEC)
                && sprintf(b"%5%", &[Arg::Int(1)], &mut a) == Err(PR_EBADSPEC)
                && sprintf(b"%++d", &[Arg::Int(1)], &mut a) == Err(PR_EBADSPEC)
                && sprintf(b"%-05d", &[Arg::Int(42)], &mut a) == Ok(5)
                && a.get(..5) == Some(&b"42   "[..])
                && l.sample(9).map(|x| x.code) == Some(PR_ETRUNC)
        },
        "static",
    );

    s.add(
        "PR11-参数与类型闸门",
        {
            let mut a = [0u8; 16];
            sprintf(b"%d %d", &[Arg::Int(1)], &mut a) == Err(PR_EARGNUM)
                && sprintf(b"%s", &[Arg::Int(1)], &mut a) == Err(PR_ETYPE)
                && sprintf(b"%d", &[Arg::Str(b"x")], &mut a) == Err(PR_ETYPE)
                && sprintf(b"%d", &[Arg::Int(1), Arg::Int(2)], &mut a) == Ok(1)
                && a.get(..1) == Some(&b"1"[..])
                && sprintf(b"", &[], &mut a) == Ok(0)
                && l.sample(11).map(|x| x.code) == Some(PR_EARGNUM)
                && l.sample(12).map(|x| x.code) == Some(PR_ETYPE)
                && l.sample(13).map(|x| x.code) == Some(PR_ERANGE)
        },
        "static",
    );

    s.add(
        "PR12-缓冲边界",
        {
            let mut exact = [0u8; 5];
            let mut short = [0xaa; 4];
            let ok = sprintf(b"%05d", &[Arg::Int(42)], &mut exact) == Ok(5)
                && exact.get(..5) == Some(&b"00042"[..]);
            let bad = sprintf(b"%05d", &[Arg::Int(42)], &mut short) == Err(PR_EBUF);
            let intact = short.get(..4) == Some(&[0xaa; 4][..]);
            ok && bad && intact
        },
        "static",
    );

    s.add(
        "PR13-冻结重放四注入",
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
        let s = run_unx11509_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn decimal_sign() {
        let mut b = [0u8; 16];
        assert_eq!(sprintf(b"%d", &[Arg::Int(42)], &mut b), Ok(2));
        assert_eq!(&b[..2], b"42");
        assert_eq!(sprintf(b"%d", &[Arg::Int(0xffff_ffff)], &mut b), Ok(2));
        assert_eq!(&b[..2], b"-1");
        assert_eq!(sprintf(b"%+d", &[Arg::Int(7)], &mut b), Ok(2));
        assert_eq!(&b[..2], b"+7");
        assert_eq!(sprintf(b"%+05d", &[Arg::Int(42)], &mut b), Ok(5));
        assert_eq!(&b[..5], b"+0042");
    }

    #[test]
    fn hex_octal_precision() {
        let mut b = [0u8; 16];
        assert_eq!(sprintf(b"%08X", &[Arg::Int(0x1a)], &mut b), Ok(8));
        assert_eq!(&b[..8], b"0000001A");
        assert_eq!(sprintf(b"%o", &[Arg::Int(8)], &mut b), Ok(2));
        assert_eq!(&b[..2], b"10");
        assert_eq!(sprintf(b"%.3d", &[Arg::Int(5)], &mut b), Ok(3));
        assert_eq!(&b[..3], b"005");
        assert_eq!(sprintf(b"%.0d", &[Arg::Int(0)], &mut b), Ok(0));
    }

    #[test]
    fn strings_chars() {
        let mut b = [0u8; 16];
        assert_eq!(sprintf(b"[%6.2s]", &[Arg::Str(b"abcdef")], &mut b), Ok(8));
        assert_eq!(&b[..8], b"[    ab]");
        assert_eq!(sprintf(b"%3c", &[Arg::Int(66)], &mut b), Ok(3));
        assert_eq!(&b[..3], b"  B");
        assert_eq!(sprintf(b"a%%b", &[Arg::Int(0)], &mut b), Ok(3));
        assert_eq!(&b[..3], b"a%b");
    }

    #[test]
    fn spec_gates() {
        let mut b = [0u8; 16];
        assert_eq!(sprintf(b"%q", &[Arg::Int(1)], &mut b), Err(PR_EBADFMT));
        assert_eq!(sprintf(b"abc%", &[Arg::Int(1)], &mut b), Err(PR_ETRUNC));
        assert_eq!(sprintf(b"%+x", &[Arg::Int(1)], &mut b), Err(PR_EFLAG));
        assert_eq!(sprintf(b"%.d", &[Arg::Int(1)], &mut b), Err(PR_EBADSPEC));
        assert_eq!(sprintf(b"%d %d", &[Arg::Int(1)], &mut b), Err(PR_EARGNUM));
        assert_eq!(sprintf(b"%s", &[Arg::Int(1)], &mut b), Err(PR_ETYPE));
        assert_eq!(sprintf(b"%c", &[Arg::Int(0x100)], &mut b), Err(PR_ERANGE));
    }

    #[test]
    fn buffer_guard() {
        let mut short = [0xaa; 4];
        assert_eq!(
            sprintf(b"%05d", &[Arg::Int(42)], &mut short),
            Err(PR_EBUF)
        );
        assert_eq!(&short[..], &[0xaa; 4][..]);
        let mut exact = [0u8; 5];
        assert_eq!(sprintf(b"%05d", &[Arg::Int(42)], &mut exact), Ok(5));
        assert_eq!(&exact[..], b"00042");
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
