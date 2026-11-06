//! UNX-F11492 xargs -n/-I/-0 占位族（0x20e90 段）
//!
//! 三个选项各管一件事，但互相牵制。`-n N` 把词表按 N 个一组切开，组数向上取整，
//! N 为零没有意义直接 EN0；`-0` 换掉分隔符，只认 NUL，空白与引号都退化成普通
//! 字节，字段数由分隔符个数决定；`-I 标记` 则把整行当一个参数，并把命令行里
//! 每一处标记替换成该参数。标记为空或超过四个字节是 EMARK——空标记会让替换
//! 在零长度上前进，必然死循环。
//!
//! `-I` 隐含“一次一行”，与 `-n` 同时出现语义冲突：一个说一次多个参数，一个说
//! 一次一个参数，实现只能二选一，所以解析期就 EMIX。替换本身要数次数：超过
//! 八处 EPLACE，避免模板异常放大命令长度；值里夹 NUL 是 ENULV（execve 的参数
//! 不可能含 NUL）；输出缓冲不足 EOVER。
//!
//! 账本十四格：八格成功覆盖 NUL 切分、空格保留、三处替换、两处替换、无标记、
//! 五个词按二切三批、四个词按三切两批与按行切分；六格拒绝依次是空标记、值含
//! NUL、N 为零、选项冲突、输出不足与替换次数超限。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e90;

/// 编号非法。
pub const XP_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const XP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const XP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const XP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const XP_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const XP_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const XP_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const XP_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const XP_ESEAL: u32 = SEG_BASE + 0x09;
/// 标记非法。
pub const XP_EMARK: u32 = SEG_BASE + 0x0a;
/// 值含 NUL。
pub const XP_ENULV: u32 = SEG_BASE + 0x0b;
/// 参数上限为零。
pub const XP_EN0: u32 = SEG_BASE + 0x0c;
/// 选项冲突。
pub const XP_EMIX: u32 = SEG_BASE + 0x0d;
/// 输出或字段容量不足。
pub const XP_EOVER: u32 = SEG_BASE + 0x0e;
/// 替换次数超限。
pub const XP_EPLACE: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：NUL 切分。
pub const OP_NUL: u8 = 1;
/// 操作：标记替换。
pub const OP_SUB: u8 = 2;
/// 操作：调用次数。
pub const OP_RUNS: u8 = 3;
/// 操作：按行切分。
pub const OP_LINES: u8 = 4;

/// 标记长度上限。
pub const MARK_CAP: usize = 4;
/// 字段容量。
pub const FIELD_CAP: usize = 8;
/// 替换次数上限。
pub const MAX_REPL: u32 = 8;
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

fn put(out: &mut [u8], n: &mut usize, b: u8) -> Result<(), u32> {
    match out.get_mut(*n) {
        Some(s) => {
            *s = b;
            *n += 1;
            Ok(())
        }
        None => Err(XP_EOVER),
    }
}

/// NUL 切分：空字段跳过。
pub fn split_nul<'a>(input: &'a [u8], out: &mut [&'a [u8]]) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= input.len() {
        let at_end = i == input.len();
        let is_sep = input.get(i) == Some(&0);
        if at_end || is_sep {
            if i > start {
                if n >= FIELD_CAP || n >= out.len() {
                    return Err(XP_EOVER);
                }
                match out.get_mut(n) {
                    Some(slot) => *slot = input.get(start..i).unwrap_or(&[]),
                    None => return Err(XP_EOVER),
                }
                n += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 按行切分：空行跳过。
pub fn split_lines<'a>(input: &'a [u8], out: &mut [&'a [u8]]) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= input.len() {
        let at_end = i == input.len();
        let is_sep = input.get(i) == Some(&b'\n');
        if at_end || is_sep {
            if i > start {
                if n >= FIELD_CAP || n >= out.len() {
                    return Err(XP_EOVER);
                }
                match out.get_mut(n) {
                    Some(slot) => *slot = input.get(start..i).unwrap_or(&[]),
                    None => return Err(XP_EOVER),
                }
                n += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 标记替换：逐处替换，次数封顶。
pub fn substitute(tmpl: &[u8], marker: &[u8], value: &[u8], out: &mut [u8]) -> Result<u32, u32> {
    if marker.is_empty() || marker.len() > MARK_CAP {
        return Err(XP_EMARK);
    }
    let mut k = 0usize;
    while k < value.len() {
        if value.get(k) == Some(&0) {
            return Err(XP_ENULV);
        }
        k += 1;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    let mut repl = 0u32;
    while i < tmpl.len() {
        let hit = match i.checked_add(marker.len()) {
            Some(end) => match tmpl.get(i..end) {
                Some(s) => s == marker,
                None => false,
            },
            None => false,
        };
        if hit {
            repl += 1;
            if repl > MAX_REPL {
                return Err(XP_EPLACE);
            }
            let mut q = 0usize;
            while q < value.len() {
                let b = match value.get(q) {
                    Some(&v) => v,
                    None => break,
                };
                put(out, &mut n, b)?;
                q += 1;
            }
            i += marker.len();
        } else {
            let b = match tmpl.get(i) {
                Some(&v) => v,
                None => break,
            };
            put(out, &mut n, b)?;
            i += 1;
        }
    }
    Ok(n as u32)
}

/// 调用次数：向上取整，零词仍一次。
pub fn invocations(words: usize, n: usize) -> Result<u32, u32> {
    if n == 0 {
        return Err(XP_EN0);
    }
    if words == 0 {
        return Ok(1);
    }
    let q = words / n;
    let r = words % n;
    Ok((q + if r > 0 { 1 } else { 0 }) as u32)
}

/// 选项互斥：-I 与 -n 不能同时给。
pub fn check_opts(n: usize, repl: Option<&[u8]>) -> Result<u8, u32> {
    if repl.is_some() && n > 0 {
        return Err(XP_EMIX);
    }
    if n == 0 && repl.is_none() {
        return Err(XP_EN0);
    }
    Ok(if repl.is_some() { 1 } else { 0 })
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
            return Err(XP_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(XP_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(XP_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(XP_EORDER);
        }
        if self.len >= self.cap {
            return Err(XP_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(XP_EID),
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
            return Err(XP_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(XP_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(XP_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(XP_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(XP_EMISMATCH);
                    }
                }
                None => return Err(XP_ENOBS),
            }
            i += 1;
        }
        let vals = [3u32, 2, 8, 2, 4, 3, 2, 3];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(XP_EMISMATCH);
                    }
                }
                _ => return Err(XP_EBAL),
            }
            k += 1;
        }
        let codes = [XP_EMARK, XP_ENULV, XP_EN0, XP_EMIX, XP_EOVER, XP_EPLACE];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(XP_EMISMATCH);
                        }
                    }
                    None => return Err(XP_EBAL),
                },
                None => return Err(XP_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(XP_EBAL);
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
            return Err(XP_EFROZEN);
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
            return Err(XP_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(XP_ESEAL);
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
    let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
    let mut buf = [0u8; 32];

    let _ = l.register(cell(
        0,
        OP_NUL,
        split_nul(b"a\0b\0\0c", &mut f).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        1,
        OP_NUL,
        split_nul(b"a b\0c d", &mut f).map(|n| n as u32),
    ));
    let _ = l.register(cell(
        2,
        OP_SUB,
        substitute(b"pre{}post", b"{}", b"X", &mut buf),
    ));
    let _ = l.register(cell(
        3,
        OP_SUB,
        substitute(b"{}{}", b"{}", b"X", &mut buf),
    ));
    let _ = l.register(cell(
        4,
        OP_SUB,
        substitute(b"none", b"{}", b"X", &mut buf),
    ));
    let _ = l.register(cell(5, OP_RUNS, invocations(5, 2)));
    let _ = l.register(cell(6, OP_RUNS, invocations(4, 3)));
    let _ = l.register(cell(
        7,
        OP_LINES,
        split_lines(b"a\nb c\nd\n", &mut f).map(|n| n as u32),
    ));

    let _ = l.register(cell(
        8,
        OP_SUB,
        substitute(b"a{}b", b"", b"X", &mut buf),
    ));
    let _ = l.register(cell(
        9,
        OP_SUB,
        substitute(b"a{}b", b"{}", b"x\0y", &mut buf),
    ));
    let _ = l.register(cell(10, OP_RUNS, invocations(5, 0)));
    let _ = l.register(cell(11, OP_RUNS, check_opts(2, Some(b"{}")).map(u32::from)));
    let _ = l.register(cell(
        12,
        OP_SUB,
        substitute(b"pre{}post", b"{}", b"X", &mut buf[..7]),
    ));
    let _ = l.register(cell(
        13,
        OP_SUB,
        substitute(
            b"{}{}{}{}{}{}{}{}{}",
            b"{}",
            b"z",
            &mut buf,
        ),
    ));

    l
}

/// 四路注入：账本自洽、切分、替换、冲突后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(XP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
    let nul = split_nul(b"one\0two three\0", &mut f);
    let first = f.get(0).copied();
    let lines = split_lines(b"a\nb c\nd\n", &mut f);
    out[1] = nul == Ok(2) && first == Some(&b"one"[..]) && lines == Ok(3);

    let mut b = [0u8; 32];
    out[2] = substitute(b"pre{}post", b"{}", b"X", &mut b) == Ok(8)
        && b.get(0..8) == Some(&b"preXpost"[..])
        && substitute(b"{}{}", b"{}", b"XY", &mut b) == Ok(4)
        && b.get(0..4) == Some(&b"XYXY"[..]);

    let mixed = check_opts(2, Some(b"{}")) == Err(XP_EMIX);
    let single = check_opts(0, Some(b"{}")) == Ok(1);
    let tiny = substitute(b"pre{}post", b"{}", b"X", &mut [0u8; 7]) == Err(XP_EOVER);
    let roomy = substitute(b"pre{}post", b"{}", b"X", &mut b) == Ok(8);
    out[3] = mixed && single && tiny && roomy;
    out
}

/// 十三判据。
pub fn run_unx11492_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11492.xargsplace");
    let l = build_ledger();

    s.add(
        "XP1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_NUL),
        "static",
    );
    s.add_dyn(
        "XP2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "XP3-NUL 切分与空格保留",
        {
            let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
            split_nul(b"a\0b\0\0c", &mut f) == Ok(3)
                && f.get(1).copied() == Some(&b"b"[..])
                && f.get(2).copied() == Some(&b"c"[..])
                && split_nul(b"a b\0c d", &mut f) == Ok(2)
                && f.get(0).copied() == Some(&b"a b"[..])
        },
        "static",
    );
    s.add(
        "XP4-标记替换",
        {
            let mut b = [0u8; 32];
            substitute(b"pre{}post", b"{}", b"X", &mut b) == Ok(8)
                && b.get(0..8) == Some(&b"preXpost"[..])
                && substitute(b"{}{}", b"{}", b"X", &mut b) == Ok(2)
                && substitute(b"none", b"{}", b"X", &mut b) == Ok(4)
        },
        "static",
    );
    s.add(
        "XP5--n 向上取整",
        invocations(5, 2) == Ok(3)
            && invocations(4, 3) == Ok(2)
            && invocations(6, 3) == Ok(2)
            && invocations(0, 3) == Ok(1)
            && l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );
    s.add(
        "XP6-按行切分",
        {
            let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
            split_lines(b"a\nb c\nd\n", &mut f) == Ok(3)
                && f.get(1).copied() == Some(&b"b c"[..])
                && l.sample(7).map(|x| x.value) == Some(3)
        },
        "static",
    );
    s.add(
        "XP7-标记闸门",
        substitute(b"a{}b", b"", b"X", &mut [0u8; 16]) == Err(XP_EMARK)
            && substitute(b"a{}b", b"toolong", b"X", &mut [0u8; 16]) == Err(XP_EMARK)
            && l.sample(8).map(|x| x.code) == Some(XP_EMARK),
        "static",
    );
    s.add(
        "XP8-值含 NUL",
        substitute(b"a{}b", b"{}", b"x\0y", &mut [0u8; 16]) == Err(XP_ENULV)
            && l.sample(9).map(|x| x.code) == Some(XP_ENULV),
        "static",
    );
    s.add(
        "XP9-N 为零",
        invocations(5, 0) == Err(XP_EN0)
            && check_opts(0, None) == Err(XP_EN0)
            && l.sample(10).map(|x| x.code) == Some(XP_EN0),
        "static",
    );
    s.add(
        "XP10-选项互斥",
        check_opts(2, Some(b"{}")) == Err(XP_EMIX)
            && check_opts(0, Some(b"{}")) == Ok(1)
            && check_opts(3, None) == Ok(0)
            && l.sample(11).map(|x| x.code) == Some(XP_EMIX),
        "static",
    );
    s.add(
        "XP11-容量不足",
        substitute(b"pre{}post", b"{}", b"X", &mut [0u8; 7]) == Err(XP_EOVER)
            && {
                let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
                split_nul(b"a\0b\0c\0d\0e\0f\0g\0h\0i", &mut f) == Err(XP_EOVER)
            }
            && l.sample(12).map(|x| x.code) == Some(XP_EOVER),
        "static",
    );
    s.add(
        "XP12-替换次数封顶",
        substitute(b"{}{}{}{}{}{}{}{}{}", b"{}", b"z", &mut [0u8; 32]) == Err(XP_EPLACE)
            && substitute(b"{}{}{}{}{}{}{}{}", b"{}", b"z", &mut [0u8; 32]) == Ok(8)
            && l.sample(13).map(|x| x.code) == Some(XP_EPLACE),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(XP_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "XP13-冻结重放与四路",
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
    fn nul_split_keeps_spaces() {
        let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
        assert_eq!(split_nul(b"a b\0c d", &mut f), Ok(2));
        assert_eq!(f.get(0).copied(), Some(&b"a b"[..]));
    }

    #[test]
    fn lines_split() {
        let mut f: [&[u8]; FIELD_CAP] = [b""; FIELD_CAP];
        assert_eq!(split_lines(b"a\nb c\nd\n", &mut f), Ok(3));
        assert_eq!(f.get(2).copied(), Some(&b"d"[..]));
    }

    #[test]
    fn substitute_all_occurrences() {
        let mut b = [0u8; 32];
        assert_eq!(substitute(b"{}{}", b"{}", b"XY", &mut b), Ok(4));
        assert_eq!(b.get(0..4), Some(&b"XYXY"[..]));
        assert_eq!(substitute(b"none", b"{}", b"X", &mut b), Ok(4));
    }

    #[test]
    fn marker_gates() {
        assert_eq!(substitute(b"a{}b", b"", b"X", &mut [0u8; 8]), Err(XP_EMARK));
        assert_eq!(
            substitute(b"a{}b", b"toolong", b"X", &mut [0u8; 8]),
            Err(XP_EMARK)
        );
        assert_eq!(
            substitute(b"a{}b", b"{}", b"x\0y", &mut [0u8; 8]),
            Err(XP_ENULV)
        );
    }

    #[test]
    fn invocation_counts() {
        assert_eq!(invocations(5, 2), Ok(3));
        assert_eq!(invocations(4, 3), Ok(2));
        assert_eq!(invocations(0, 3), Ok(1));
        assert_eq!(invocations(5, 0), Err(XP_EN0));
    }

    #[test]
    fn option_conflict() {
        assert_eq!(check_opts(2, Some(b"{}")), Err(XP_EMIX));
        assert_eq!(check_opts(0, Some(b"{}")), Ok(1));
        assert_eq!(check_opts(0, None), Err(XP_EN0));
    }

    #[test]
    fn replacement_cap() {
        assert_eq!(
            substitute(b"{}{}{}{}{}{}{}{}{}", b"{}", b"z", &mut [0u8; 32]),
            Err(XP_EPLACE)
        );
        assert_eq!(
            substitute(b"{}{}{}{}{}{}{}{}", b"{}", b"z", &mut [0u8; 32]),
            Ok(8)
        );
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11492_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
