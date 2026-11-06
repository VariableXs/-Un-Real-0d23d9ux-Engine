//! UNX-F11494 五工具 stdin 管道组合探针（0x20eb0 段）
//!
//! 管道不是五条独立命令，而是一条字节流依次穿过 echo、tr、cut、wc 四个阶段：
//! 每个阶段读上一阶段的全部字节、写下一阶段的缓冲区，中间的任何一次长度变化都
//! 会改变下游判据。echo 无条件补一个换行；tr 的升档只碰 a-z，其余字节原样穿透；
//! cut 按行保留前 n 列并在行尾重置列号；wc -c 把整段字节数折成十进制再加换行。
//!
//! 组合的合法性在入口处一次判完：阶段表非空，长度不超过五，首阶段必须是 echo，
//! 中段若再出现 echo 视为越位。各阶段的缓冲违约分成两类：echo 写出越界是 EOVER，
//! 其余阶段收到超过缓冲区的长度记录是 EBUF；两者都不允许截断输出后继续。
//!
//! 账本十四格：八格成功覆盖单阶段、列截断、字节计数与三段/四段组合；六格拒绝
//! 依次是 echo 溢出、空阶段表、零列截断、未知阶段、长度违约与阶段过多。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20eb0;

/// 编号非法。
pub const FP_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const FP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const FP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const FP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const FP_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const FP_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const FP_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const FP_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const FP_ESEAL: u32 = SEG_BASE + 0x09;
/// echo 写出越界。
pub const FP_EOVER: u32 = SEG_BASE + 0x0a;
/// 空阶段表。
pub const FP_EEMPTY: u32 = SEG_BASE + 0x0b;
/// 零列截断。
pub const FP_ECUT: u32 = SEG_BASE + 0x0c;
/// 未知阶段。
pub const FP_ESTAGE: u32 = SEG_BASE + 0x0d;
/// 长度记录越过缓冲。
pub const FP_EBUF: u32 = SEG_BASE + 0x0e;
/// 阶段过多。
pub const FP_ECOMBO: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 阶段：echo。
pub const OP_ECHO: u8 = 1;
/// 阶段：tr 升档。
pub const OP_UPPER: u8 = 2;
/// 阶段：cut 截列。
pub const OP_CUT: u8 = 3;
/// 阶段：wc -c。
pub const OP_WC: u8 = 4;
/// 操作：整链运行。
pub const OP_RUN: u8 = 5;

/// 管道缓冲容量。
pub const INPUT_CAP: usize = 64;
/// 阶段表上限。
pub const STAGE_CAP: usize = 5;
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

/// echo：原样写出并补一个换行。
pub fn echo(src: &[u8], out: &mut [u8]) -> Result<usize, u32> {
    let need = match src.len().checked_add(1) {
        Some(v) => v,
        None => return Err(FP_EOVER),
    };
    if need > out.len() {
        return Err(FP_EOVER);
    }
    let mut i = 0usize;
    while i < src.len() {
        let b = match src.get(i) {
            Some(&v) => v,
            None => return Err(FP_EOVER),
        };
        match out.get_mut(i) {
            Some(d) => *d = b,
            None => return Err(FP_EOVER),
        }
        i += 1;
    }
    match out.get_mut(src.len()) {
        Some(d) => *d = b'\n',
        None => return Err(FP_EOVER),
    }
    Ok(need)
}

/// tr：a-z 升档，其余穿透。
pub fn upper(buf: &mut [u8], len: usize) -> Result<usize, u32> {
    if len > buf.len() {
        return Err(FP_EBUF);
    }
    let mut i = 0usize;
    while i < len {
        let b = match buf.get(i) {
            Some(&v) => v,
            None => return Err(FP_EBUF),
        };
        if b >= b'a' && b <= b'z' {
            match buf.get_mut(i) {
                Some(d) => *d = b - 32,
                None => return Err(FP_EBUF),
            }
        }
        i += 1;
    }
    Ok(len)
}

/// cut：逐行保留前 n 列，换行照抄并重置列号。
pub fn cut(buf: &mut [u8], len: usize, n: usize) -> Result<usize, u32> {
    if n == 0 {
        return Err(FP_ECUT);
    }
    if len > buf.len() {
        return Err(FP_EBUF);
    }
    let mut r = 0usize;
    let mut w = 0usize;
    let mut col = 0usize;
    while r < len {
        let b = match buf.get(r) {
            Some(&v) => v,
            None => return Err(FP_EBUF),
        };
        if b == b'\n' {
            match buf.get_mut(w) {
                Some(d) => *d = b'\n',
                None => return Err(FP_EBUF),
            }
            w += 1;
            col = 0;
        } else if col < n {
            match buf.get_mut(w) {
                Some(d) => *d = b,
                None => return Err(FP_EBUF),
            }
            w += 1;
            col += 1;
        }
        r += 1;
    }
    Ok(w)
}

/// wc -c：把字节数折成十进制并补换行。
pub fn wc_c(buf: &mut [u8], len: usize) -> Result<usize, u32> {
    if len > buf.len() {
        return Err(FP_EBUF);
    }
    let mut digits = 1usize;
    let mut t = len;
    while t >= 10 {
        t /= 10;
        digits += 1;
    }
    let total = match digits.checked_add(1) {
        Some(v) => v,
        None => return Err(FP_EOVER),
    };
    if total > buf.len() {
        return Err(FP_EOVER);
    }
    let mut div = 1usize;
    let mut k = 1usize;
    while k < digits {
        div = div.saturating_mul(10);
        k += 1;
    }
    let mut v = len;
    let mut j = 0usize;
    while j < digits {
        let d = v / div;
        v %= div;
        if div > 1 {
            div /= 10;
        }
        match buf.get_mut(j) {
            Some(c) => *c = b'0' + d as u8,
            None => return Err(FP_EBUF),
        }
        j += 1;
    }
    match buf.get_mut(digits) {
        Some(c) => *c = b'\n',
        None => return Err(FP_EOVER),
    }
    Ok(total)
}

/// 整链：首段 echo，其后逐段折叠。
pub fn run(stages: &[u8], input: &[u8], cut_n: usize, out: &mut [u8]) -> Result<usize, u32> {
    if stages.is_empty() {
        return Err(FP_EEMPTY);
    }
    if stages.len() > STAGE_CAP {
        return Err(FP_ECOMBO);
    }
    let first = match stages.get(0) {
        Some(&s) => s,
        None => return Err(FP_EEMPTY),
    };
    if first != OP_ECHO {
        return Err(FP_ESTAGE);
    }
    let mut len = echo(input, out)?;
    let mut i = 1usize;
    while i < stages.len() {
        let st = match stages.get(i) {
            Some(&s) => s,
            None => return Err(FP_ESTAGE),
        };
        len = match st {
            OP_UPPER => upper(out, len)?,
            OP_CUT => cut(out, len, cut_n)?,
            OP_WC => wc_c(out, len)?,
            _ => return Err(FP_ESTAGE),
        };
        i += 1;
    }
    Ok(len)
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
            return Err(FP_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(FP_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(FP_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(FP_EORDER);
        }
        if self.len >= self.cap {
            return Err(FP_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(FP_EID),
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
            return Err(FP_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FP_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(FP_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(FP_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(FP_EMISMATCH);
                    }
                }
                None => return Err(FP_ENOBS),
            }
            i += 1;
        }
        let vals = [4u32, 1, 4, 6, 2, 2, 4, 5];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(FP_EMISMATCH);
                    }
                }
                _ => return Err(FP_EBAL),
            }
            k += 1;
        }
        let codes = [FP_EOVER, FP_EEMPTY, FP_ECUT, FP_ESTAGE, FP_EBUF, FP_ECOMBO];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FP_EMISMATCH);
                        }
                    }
                    None => return Err(FP_EBAL),
                },
                None => return Err(FP_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(FP_EBAL);
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
            return Err(FP_EFROZEN);
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
            return Err(FP_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(FP_ESEAL);
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

fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v as u32,
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
    let mut b: [u8; INPUT_CAP] = [0u8; INPUT_CAP];

    let _ = l.register(cell(0, OP_ECHO, echo(b"abc", &mut b)));
    let _ = l.register(cell(1, OP_ECHO, echo(b"", &mut b)));

    let mut u: [u8; 4] = [b'a', b'b', b'C', b'1'];
    let _ = l.register(cell(2, OP_UPPER, upper(&mut u, 4)));

    let mut c: [u8; 8] = [b'a', b'b', b'c', b'd', b'\n', b'x', b'y', b'\n'];
    let _ = l.register(cell(3, OP_CUT, cut(&mut c, 8, 2)));
    let _ = l.register(cell(4, OP_WC, wc_c(&mut b, 4)));
    let _ = l.register(cell(5, OP_RUN, run(&[OP_ECHO, OP_UPPER, OP_CUT, OP_WC], b"hello", 2, &mut b)));
    let _ = l.register(cell(6, OP_RUN, run(&[OP_ECHO, OP_UPPER], b"abc", 0, &mut b)));
    let _ = l.register(cell(7, OP_RUN, run(&[OP_ECHO, OP_CUT], b"abcdef", 4, &mut b)));

    let mut tiny: [u8; 4] = [0u8; 4];
    let _ = l.register(cell(8, OP_ECHO, echo(b"abcdef", &mut tiny)));
    let _ = l.register(cell(9, OP_RUN, run(&[], b"x", 0, &mut b)));
    let _ = l.register(cell(10, OP_CUT, cut(&mut c, 8, 0)));
    let _ = l.register(cell(11, OP_RUN, run(&[OP_ECHO, 9], b"x", 0, &mut b)));
    let _ = l.register(cell(12, OP_UPPER, upper(&mut b, INPUT_CAP + 1)));
    let many = [OP_ECHO, OP_UPPER, OP_CUT, OP_WC, OP_UPPER, OP_CUT];
    let _ = l.register(cell(13, OP_RUN, run(&many, b"x", 1, &mut b)));

    l
}

/// 四路注入：账本自洽、echo 换行、整链折叠、拒绝后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(FP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    let mut b: [u8; INPUT_CAP] = [0u8; INPUT_CAP];
    out[1] = echo(b"abc", &mut b) == Ok(4)
        && b.get(3) == Some(&b'\n')
        && echo(b"", &mut b) == Ok(1)
        && b.get(0) == Some(&b'\n');

    out[2] = run(&[OP_ECHO, OP_UPPER, OP_CUT, OP_WC], b"hello", 2, &mut b) == Ok(2)
        && b.get(0) == Some(&b'3')
        && run(&[OP_ECHO, OP_CUT], b"abcdef", 4, &mut b) == Ok(5)
        && b.get(4) == Some(&b'\n');

    let mut tiny: [u8; 4] = [0u8; 4];
    let overflow = echo(b"abcdef", &mut tiny) == Err(FP_EOVER);
    let stage = run(&[9], b"x", 1, &mut b) == Err(FP_ESTAGE);
    let zero = cut(&mut b, 0, 0) == Err(FP_ECUT);
    let recovered = run(&[OP_ECHO], b"ok", 0, &mut b) == Ok(3);
    out[3] = overflow && stage && zero && recovered;
    out
}

/// 十三判据。
pub fn run_unx11494_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11494.fivepipe");
    let l = build_ledger();

    s.add(
        "FP1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_ECHO),
        "static",
    );
    s.add_dyn(
        "FP2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    let mut b: [u8; INPUT_CAP] = [0u8; INPUT_CAP];
    s.add(
        "FP3-echo 补换行",
        echo(b"abc", &mut b) == Ok(4)
            && b.get(0) == Some(&b'a')
            && b.get(3) == Some(&b'\n')
            && echo(b"", &mut b) == Ok(1),
        "static",
    );
    let mut u: [u8; 4] = [b'a', b'b', b'C', b'1'];
    s.add(
        "FP4-tr 只升小写",
        upper(&mut u, 4) == Ok(4)
            && u.get(0) == Some(&b'A')
            && u.get(2) == Some(&b'C')
            && u.get(3) == Some(&b'1'),
        "static",
    );
    let mut c: [u8; 8] = [b'a', b'b', b'c', b'd', b'\n', b'x', b'y', b'\n'];
    s.add(
        "FP5-cut 逐行截列",
        cut(&mut c, 8, 2) == Ok(6)
            && c.get(0) == Some(&b'a')
            && c.get(1) == Some(&b'b')
            && c.get(2) == Some(&b'\n')
            && c.get(3) == Some(&b'x'),
        "static",
    );
    s.add(
        "FP6-wc 十进制收尾",
        wc_c(&mut b, 4) == Ok(2)
            && b.get(0) == Some(&b'4')
            && b.get(1) == Some(&b'\n')
            && wc_c(&mut b, 10) == Ok(3),
        "static",
    );
    s.add(
        "FP7-四段入 wc",
        run(&[OP_ECHO, OP_UPPER, OP_CUT, OP_WC], b"hello", 2, &mut b) == Ok(2)
            && b.get(0) == Some(&b'3')
            && run(&[OP_ECHO, OP_UPPER], b"abc", 0, &mut b) == Ok(4),
        "static",
    );
    let mut tiny: [u8; 4] = [0u8; 4];
    s.add(
        "FP8-echo 溢出",
        echo(b"abcdef", &mut tiny) == Err(FP_EOVER)
            && l.sample(8).map(|x| x.code) == Some(FP_EOVER),
        "static",
    );
    s.add(
        "FP9-空链与未知阶段",
        run(&[], b"x", 0, &mut b) == Err(FP_EEMPTY)
            && run(&[9], b"x", 0, &mut b) == Err(FP_ESTAGE)
            && run(&[OP_UPPER], b"x", 0, &mut b) == Err(FP_ESTAGE)
            && l.sample(9).map(|x| x.code) == Some(FP_EEMPTY)
            && l.sample(11).map(|x| x.code) == Some(FP_ESTAGE),
        "static",
    );
    s.add(
        "FP10-cut 零列",
        cut(&mut c, 8, 0) == Err(FP_ECUT)
            && l.sample(10).map(|x| x.code) == Some(FP_ECUT),
        "static",
    );
    s.add(
        "FP11-长度违约",
        upper(&mut b, INPUT_CAP + 1) == Err(FP_EBUF)
            && cut(&mut b, INPUT_CAP + 1, 1) == Err(FP_EBUF)
            && wc_c(&mut b, INPUT_CAP + 1) == Err(FP_EBUF)
            && l.sample(12).map(|x| x.code) == Some(FP_EBUF),
        "static",
    );
    let many = [OP_ECHO, OP_UPPER, OP_CUT, OP_WC, OP_UPPER, OP_CUT];
    s.add_dyn(
        "FP12-阶段过多",
        run(&many, b"x", 1, &mut b) == Err(FP_ECOMBO)
            && l.sample(13).map(|x| x.code) == Some(FP_ECOMBO),
        &format!("stages={} cap={}", many.len(), STAGE_CAP),
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FP_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "FP13-冻结重放与四路",
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
    fn echo_appends_newline() {
        let mut b = [0u8; 8];
        assert_eq!(echo(b"abc", &mut b), Ok(4));
        assert_eq!(b.get(3), Some(&b'\n'));
        assert_eq!(echo(b"", &mut b), Ok(1));
    }

    #[test]
    fn upper_only_touches_lowercase() {
        let mut b = [b'a', b'Z', b'0', b'm'];
        assert_eq!(upper(&mut b, 4), Ok(4));
        assert_eq!(b, *b"AZ0M");
    }

    #[test]
    fn cut_resets_columns_per_line() {
        let mut b = [b'a', b'b', b'c', b'\n', b'd', b'e', b'\n'];
        assert_eq!(cut(&mut b, 7, 2), Ok(6));
        assert_eq!(b.get(0), Some(&b'a'));
        assert_eq!(b.get(2), Some(&b'\n'));
        assert_eq!(b.get(3), Some(&b'd'));
        assert_eq!(cut(&mut b, 6, 0), Err(FP_ECUT));
    }

    #[test]
    fn wc_writes_decimal() {
        let mut b = [0u8; 64];
        assert_eq!(wc_c(&mut b, 0), Ok(2));
        assert_eq!(b.get(0), Some(&b'0'));
        assert_eq!(wc_c(&mut b, 42), Ok(3));
        assert_eq!(b.get(0), Some(&b'4'));
        assert_eq!(b.get(1), Some(&b'2'));
    }

    #[test]
    fn pipeline_runs_end_to_end() {
        let mut b = [0u8; INPUT_CAP];
        assert_eq!(
            run(&[OP_ECHO, OP_UPPER, OP_CUT, OP_WC], b"hello", 2, &mut b),
            Ok(2)
        );
        assert_eq!(b.get(0), Some(&b'3'));
        assert_eq!(run(&[], b"x", 0, &mut b), Err(FP_EEMPTY));
        assert_eq!(run(&[OP_UPPER], b"x", 0, &mut b), Err(FP_ESTAGE));
    }

    #[test]
    fn pipeline_rejects_overrun() {
        let mut b = [0u8; 4];
        assert_eq!(echo(b"abcdef", &mut b), Err(FP_EOVER));
        assert_eq!(upper(&mut b, 5), Err(FP_EBUF));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11494_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
