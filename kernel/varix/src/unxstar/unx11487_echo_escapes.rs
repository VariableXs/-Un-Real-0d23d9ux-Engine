//! UNX-F11487 echo -e 转义序列集（0x20e40 段）
//!
//! `echo -e` 的转义表小得可以背下来，但每一条都有边界：单字母转义 `\a` 到
//! `\v` 加上 `\\` 查表即得；`\0NNN` 是零后最多三位八进制，值域 0..255，超过
//! 就落到下一轮（本模型把超界当错误 EOCT，避免静默截断）；`\xHH` 必须至少带
//! 一位十六进制，最多两位，缺位或非法位都是 EHEX。`\c` 不是字节而是控制：
//! 它终止整个输出，连结尾换行也不发。
//!
//! 解码器分宽容与严格两档：宽容档在输入以孤立反斜杠结尾时把它原样输出，
//! 未知字母转义也保留反斜杠与字母——这正是 echo 的行为；严格档把孤立反斜杠
//! 判为 ELONE，用于抓“上游把转义切断了”的场景。两档共用同一张表和同一套
//! 容量检查，区别只在一处出口。
//!
//! 账本十四格：八格成功覆盖单字母查表、八进制、十六进制、整串解码与 `\c`
//! 截断；六格拒绝依次是输出溢出、八进制超界、十六进制缺位、未知字母、孤立
//! 反斜杠与输入超长。审计逐格复算字节值，不只看长度。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e40;

/// 编号非法。
pub const EE_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const EE_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const EE_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const EE_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const EE_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const EE_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const EE_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const EE_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const EE_ESEAL: u32 = SEG_BASE + 0x09;
/// 输出缓冲不足。
pub const EE_EOVER: u32 = SEG_BASE + 0x0a;
/// 八进制超界。
pub const EE_EOCT: u32 = SEG_BASE + 0x0b;
/// 十六进制缺位或非法。
pub const EE_EHEX: u32 = SEG_BASE + 0x0c;
/// 未知字母转义。
pub const EE_EUNK: u32 = SEG_BASE + 0x0d;
/// 孤立反斜杠。
pub const EE_ELONE: u32 = SEG_BASE + 0x0e;
/// 输入超长。
pub const EE_EINLEN: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：单字母查表。
pub const OP_SIMPLE: u8 = 1;
/// 操作：八进制求值。
pub const OP_OCT: u8 = 2;
/// 操作：十六进制求值。
pub const OP_HEX: u8 = 3;
/// 操作：整串解码。
pub const OP_DECODE: u8 = 4;
/// 操作：截断解码。
pub const OP_STOP: u8 = 5;

/// 输入容量。
pub const INPUT_CAP: usize = 64;
/// 八进制位数。
pub const OCT_DIGITS: usize = 3;
/// 十六进制位数。
pub const HEX_DIGITS: usize = 2;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入。
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

/// 单字母转义查表。
pub fn simple(c: u8) -> Result<u8, u32> {
    match c {
        b'a' => Ok(0x07),
        b'b' => Ok(0x08),
        b'e' => Ok(0x1b),
        b'f' => Ok(0x0c),
        b'n' => Ok(0x0a),
        b'r' => Ok(0x0d),
        b't' => Ok(0x09),
        b'v' => Ok(0x0b),
        b'\\' => Ok(0x5c),
        _ => Err(EE_EUNK),
    }
}

/// 八进制求值：最多三位，超过 255 报错。
pub fn octal(d: &[u8]) -> Result<u8, u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < d.len() && i < OCT_DIGITS {
        let c = match d.get(i) {
            Some(&c) => c,
            None => break,
        };
        if !(b'0'..=b'7').contains(&c) {
            return Err(EE_EOCT);
        }
        v = v * 8 + u32::from(c - b'0');
        i += 1;
    }
    if v > 0xff {
        return Err(EE_EOCT);
    }
    Ok(v as u8)
}

fn nibble(c: u8) -> Result<u32, u32> {
    match c {
        b'0'..=b'9' => Ok(u32::from(c - b'0')),
        b'a'..=b'f' => Ok(u32::from(c - b'a') + 10),
        b'A'..=b'F' => Ok(u32::from(c - b'A') + 10),
        _ => Err(EE_EHEX),
    }
}

/// 十六进制求值：一至两位。
pub fn hex(d: &[u8]) -> Result<u8, u32> {
    if d.is_empty() || d.len() > HEX_DIGITS {
        return Err(EE_EHEX);
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < d.len() {
        let c = match d.get(i) {
            Some(&c) => c,
            None => break,
        };
        v = (v << 4) | nibble(c)?;
        i += 1;
    }
    Ok(v as u8)
}

fn put(out: &mut [u8], n: &mut usize, b: u8) -> Result<(), u32> {
    match out.get_mut(*n) {
        Some(s) => {
            *s = b;
            *n += 1;
            Ok(())
        }
        None => Err(EE_EOVER),
    }
}

fn decode_impl(src: &[u8], out: &mut [u8], strict: bool) -> Result<(u32, bool), u32> {
    if src.len() > INPUT_CAP {
        return Err(EE_EINLEN);
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < src.len() {
        let b = match src.get(i) {
            Some(&v) => v,
            None => break,
        };
        if b != b'\\' {
            put(out, &mut n, b)?;
            i += 1;
            continue;
        }
        let next = src.get(i + 1).copied();
        let c = match next {
            Some(v) => v,
            None => {
                if strict {
                    return Err(EE_ELONE);
                }
                put(out, &mut n, b'\\')?;
                i += 1;
                continue;
            }
        };
        if c == b'c' {
            return Ok((n as u32, true));
        }
        if c == b'0' {
            let mut k = i + 2;
            let mut tmp = [0u8; OCT_DIGITS];
            let mut m = 0usize;
            while m < OCT_DIGITS {
                match src.get(k) {
                    Some(&d) if (b'0'..=b'7').contains(&d) => {
                        tmp[m] = d;
                        m += 1;
                        k += 1;
                    }
                    _ => break,
                }
            }
            let v = octal(tmp.get(..m).unwrap_or(&[]))?;
            put(out, &mut n, v)?;
            i = k;
            continue;
        }
        if c == b'x' {
            let mut k = i + 2;
            let mut tmp = [0u8; HEX_DIGITS];
            let mut m = 0usize;
            while m < HEX_DIGITS {
                match src.get(k) {
                    Some(&d) if d.is_ascii_hexdigit() => {
                        tmp[m] = d;
                        m += 1;
                        k += 1;
                    }
                    _ => break,
                }
            }
            let v = hex(tmp.get(..m).unwrap_or(&[]))?;
            put(out, &mut n, v)?;
            i = k;
            continue;
        }
        match simple(c) {
            Ok(v) => {
                put(out, &mut n, v)?;
                i += 2;
            }
            Err(_) => {
                put(out, &mut n, b'\\')?;
                put(out, &mut n, c)?;
                i += 2;
            }
        }
    }
    Ok((n as u32, false))
}

/// 宽容解码：孤立反斜杠与未知转义原样保留。
pub fn decode(src: &[u8], out: &mut [u8]) -> Result<(u32, bool), u32> {
    decode_impl(src, out, false)
}

/// 严格解码：孤立反斜杠判为 ELONE。
pub fn decode_strict(src: &[u8], out: &mut [u8]) -> Result<(u32, bool), u32> {
    decode_impl(src, out, true)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    cap: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap: SAMPLES,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            cap,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：冻结 → 编号 → 重复 → 顺序 → 容量。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(EE_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(EE_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(EE_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(EE_EORDER);
        }
        if self.len >= self.cap {
            return Err(EE_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(EE_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(EE_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EE_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(EE_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(EE_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(EE_EMISMATCH);
                    }
                }
                None => return Err(EE_ENOBS),
            }
            i += 1;
        }
        let vals = [9u32, 11, 65, 0, 65, 255, 4, 2];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(EE_EMISMATCH);
                    }
                }
                _ => return Err(EE_EBAL),
            }
            k += 1;
        }
        let codes = [EE_EOVER, EE_EOCT, EE_EHEX, EE_EUNK, EE_ELONE, EE_EINLEN];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EE_EMISMATCH);
                        }
                    }
                    None => return Err(EE_EBAL),
                },
                None => return Err(EE_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(EE_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
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

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(EE_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(EE_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(EE_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut buf = [0u8; 32];

    let _ = l.register(cell(0, OP_SIMPLE, simple(b't').map(u32::from)));
    let _ = l.register(cell(1, OP_SIMPLE, simple(b'v').map(u32::from)));
    let _ = l.register(cell(2, OP_OCT, octal(b"101").map(u32::from)));
    let _ = l.register(cell(3, OP_OCT, octal(b"0").map(u32::from)));
    let _ = l.register(cell(4, OP_HEX, hex(b"41").map(u32::from)));
    let _ = l.register(cell(5, OP_HEX, hex(b"ff").map(u32::from)));
    let _ = l.register(cell(6, OP_DECODE, decode(b"a\\tb\\n", &mut buf).map(|(n, _)| n)));
    let _ = l.register(cell(7, OP_STOP, decode(b"ab\\ccd", &mut buf).map(|(n, _)| n)));

    let _ = l.register(cell(8, OP_DECODE, decode(b"a\\tb", &mut [0u8; 2]).map(|(n, _)| n)));
    let _ = l.register(cell(9, OP_OCT, octal(b"777").map(u32::from)));
    let _ = l.register(cell(10, OP_HEX, hex(b"").map(u32::from)));
    let _ = l.register(cell(11, OP_SIMPLE, simple(b'q').map(u32::from)));
    let _ = l.register(cell(12, OP_DECODE, decode_strict(b"ab\\", &mut buf).map(|(n, _)| n)));
    let long = [b'a'; 65];
    let _ = l.register(cell(13, OP_DECODE, decode(&long, &mut buf).map(|(n, _)| n)));

    l
}

/// 四路注入：账本自洽、查表、截断、溢出后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(EE_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    out[1] = simple(b't') == Ok(9)
        && simple(b'e') == Ok(27)
        && simple(b'\\') == Ok(92)
        && octal(b"101") == Ok(65)
        && hex(b"ff") == Ok(255);

    let mut b = [0u8; 8];
    out[2] = decode(b"ab\\ccd", &mut b) == Ok((2, true))
        && b.get(0) == Some(&b'a')
        && b.get(1) == Some(&b'b');

    let mut small = [0u8; 2];
    let over = decode(b"a\\tb", &mut small);
    let mut big = [0u8; 8];
    let fits = decode(b"a\\tb", &mut big);
    out[3] = over == Err(EE_EOVER) && fits == Ok((3, false)) && big.get(2) == Some(&b'b');
    out
}

/// 十三判据。
pub fn run_unx11487_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11487.echoescapes");
    let l = build_ledger();

    s.add(
        "EE1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SIMPLE),
        "static",
    );
    s.add_dyn(
        "EE2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "EE3-单字母表",
        simple(b'a') == Ok(7)
            && simple(b'b') == Ok(8)
            && simple(b'e') == Ok(27)
            && simple(b'f') == Ok(12)
            && simple(b'n') == Ok(10)
            && simple(b'r') == Ok(13)
            && simple(b't') == Ok(9)
            && simple(b'v') == Ok(11),
        "static",
    );
    s.add(
        "EE4-八进制",
        octal(b"101") == Ok(65)
            && octal(b"0") == Ok(0)
            && octal(b"7") == Ok(7)
            && octal(b"377") == Ok(255),
        "static",
    );
    s.add(
        "EE5-十六进制",
        hex(b"41") == Ok(65)
            && hex(b"ff") == Ok(255)
            && hex(b"4") == Ok(4)
            && hex(b"FF") == Ok(255),
        "static",
    );
    s.add(
        "EE6-整串解码",
        {
            let mut b = [0u8; 16];
            decode(b"a\\tb\\n", &mut b) == Ok((4, false))
                && b.get(0..4) == Some(&b"a\tb\n"[..])
                && l.sample(6).map(|x| x.value) == Some(4)
        },
        "static",
    );
    s.add(
        "EE7-截断语义",
        {
            let mut b = [0u8; 16];
            decode(b"ab\\ccd", &mut b) == Ok((2, true))
                && b.get(0..2) == Some(&b"ab"[..])
                && decode_strict(b"ab\\ccd", &mut b) == Ok((2, true))
        },
        "static",
    );
    s.add(
        "EE8-未知转义保留",
        {
            let mut b = [0u8; 16];
            decode(b"\\q", &mut b) == Ok((2, false))
                && b.get(0..2) == Some(&b"\\q"[..])
        },
        "static",
    );
    s.add(
        "EE9-输出溢出",
        decode(b"a\\tb", &mut [0u8; 2]) == Err(EE_EOVER)
            && l.sample(8).map(|x| x.code) == Some(EE_EOVER),
        "static",
    );
    s.add(
        "EE10-进制越界",
        octal(b"777") == Err(EE_EOCT)
            && hex(b"") == Err(EE_EHEX)
            && hex(b"gg") == Err(EE_EHEX)
            && l.sample(9).map(|x| x.code) == Some(EE_EOCT)
            && l.sample(10).map(|x| x.code) == Some(EE_EHEX),
        "static",
    );
    s.add(
        "EE11-未知字母与孤立反斜杠",
        simple(b'q') == Err(EE_EUNK)
            && decode_strict(b"ab\\", &mut [0u8; 8]) == Err(EE_ELONE)
            && decode(b"ab\\", &mut [0u8; 8]) == Ok((3, false))
            && l.sample(11).map(|x| x.code) == Some(EE_EUNK)
            && l.sample(12).map(|x| x.code) == Some(EE_ELONE),
        "static",
    );
    s.add(
        "EE12-输入上限",
        {
            let long = [b'a'; 65];
            decode(&long, &mut [0u8; 96]) == Err(EE_EINLEN)
                && l.sample(13).map(|x| x.code) == Some(EE_EINLEN)
                && l.sample(13).map(|x| x.value) == Some(0)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(EE_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "EE13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && f.root() == root && inj == [true; 4],
        &format!("root={:016x} inj={:?}", root, inj),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_table() {
        assert_eq!(simple(b't'), Ok(9));
        assert_eq!(simple(b'v'), Ok(11));
        assert_eq!(simple(b'\\'), Ok(92));
        assert_eq!(simple(b'z'), Err(EE_EUNK));
    }

    #[test]
    fn octal_bounds() {
        assert_eq!(octal(b"101"), Ok(65));
        assert_eq!(octal(b"377"), Ok(255));
        assert_eq!(octal(b"400"), Err(EE_EOCT));
    }

    #[test]
    fn hex_bounds() {
        assert_eq!(hex(b"41"), Ok(65));
        assert_eq!(hex(b"FF"), Ok(255));
        assert_eq!(hex(b""), Err(EE_EHEX));
        assert_eq!(hex(b"123"), Err(EE_EHEX));
    }

    #[test]
    fn decode_mixed() {
        let mut b = [0u8; 16];
        assert_eq!(decode(b"a\\tb\\n", &mut b), Ok((4, false)));
        assert_eq!(b.get(0..4), Some(&b"a\tb\n"[..]));
        assert_eq!(decode(b"\\0101", &mut b), Ok((1, false)));
        assert_eq!(b.get(0), Some(&b'A'));
    }

    #[test]
    fn stop_cuts_output() {
        let mut b = [0u8; 16];
        assert_eq!(decode(b"ab\\ccd", &mut b), Ok((2, true)));
        assert_eq!(decode(b"\\c", &mut b), Ok((0, true)));
    }

    #[test]
    fn strict_rejects_lone_backslash() {
        assert_eq!(decode_strict(b"ab\\", &mut [0u8; 8]), Err(EE_ELONE));
        assert_eq!(decode(b"ab\\", &mut [0u8; 8]), Ok((3, false)));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11487_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
