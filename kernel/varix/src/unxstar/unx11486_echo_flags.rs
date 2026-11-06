//! UNX-F11486 echo 输出与 -n/-e 旗（0x20e30 段）
//!
//! echo 的参数处理只有一条容易写错的规则：旗只在正文之前生效。实现从头扫描
//! argv，每个以 `-` 开头且长度大于一的 token 都先被尝试当作旗束；束内只允许
//! `n`、`e`、`E` 三个字母，出现任何别的字符，这个 token 就整体退回正文——所以
//! `echo -x` 打印 `-x` 而不是报错。扫描在第一个非旗 token 处停下，其下标就是
//! 正文起点，后面即使再出现 `-n` 也只是字面量。
//!
//! 旗束按字母顺序逐位生效，`-nEe` 与 `-en` 的结果不同：`E` 清除转义位，之后
//! 的 `e` 又会把它置回。这正是 `echo -e -E` 关闭转义、`echo -E -e` 打开转义的
//! 原因，账本用三格分别覆盖单旗、组合旗和清除序。
//!
//! 输出阶段先做容量算术（正文总长 + 分隔空格），超容量返回 EOVER，单参数超
//! 长返回 ETXT，内嵌 NUL 返回 ENUL（argv 里不可能出现 NUL，出现即上游违约）。
//! 账本十四格：八格成功覆盖扫描、旗值、整串渲染、关换行、转义渲染与空参数占位；
//! 六格拒绝依次是输出溢出、非法旗位、NUL、参数超长、文本总长超限与参数个数上限。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e30;

/// 编号非法。
pub const EC_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const EC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const EC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const EC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const EC_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const EC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const EC_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const EC_EFULL: u32 = SEG_BASE + 0x08;
/// 票号不一致。
pub const EC_ESEAL: u32 = SEG_BASE + 0x09;
/// 输出缓冲不足。
pub const EC_EOVER: u32 = SEG_BASE + 0x0a;
/// 旗位非法。
pub const EC_EFLAG: u32 = SEG_BASE + 0x0b;
/// 内嵌 NUL。
pub const EC_ENUL: u32 = SEG_BASE + 0x0c;
/// 单参数超长。
pub const EC_ETXT: u32 = SEG_BASE + 0x0d;
/// 文本总长超限。
pub const EC_ELONG: u32 = SEG_BASE + 0x0e;
/// 参数个数超限。
pub const EC_ENARGS: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：扫描旗束。
pub const OP_SCAN: u8 = 1;
/// 操作：取旗值。
pub const OP_FLAG: u8 = 2;
/// 操作：整串渲染。
pub const OP_RENDER: u8 = 3;
/// 操作：无旗拼接。
pub const OP_JOIN: u8 = 4;

/// 关换行位。
pub const F_NONL: u8 = 0x01;
/// 开启转义位。
pub const F_ESC: u8 = 0x02;
/// 合法旗位掩码。
pub const FLAG_MASK: u8 = 0x03;

/// 单参数容量。
pub const ARG_CAP: usize = 8;
/// 正文总长容量。
pub const TEXT_CAP: usize = 64;
/// 参数个数上限。
pub const MAX_ARGS: usize = 16;
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

/// 解析旗束：返回（置位，清位）；非旗 token 返回 None。
fn bundle(a: &[u8]) -> Option<(u8, u8)> {
    if a.len() < 2 || a.first() != Some(&b'-') {
        return None;
    }
    let mut set = 0u8;
    let mut clr = 0u8;
    let mut i = 1usize;
    while i < a.len() {
        match a.get(i) {
            Some(&b'n') => set |= F_NONL,
            Some(&b'e') => {
                set |= F_ESC;
                clr &= !F_ESC;
            }
            Some(&b'E') => {
                clr |= F_ESC;
                set &= !F_ESC;
            }
            _ => return None,
        }
        i += 1;
    }
    Some((set, clr))
}

/// 扫描：返回（累计旗值，正文首下标）。
pub fn parse(args: &[&[u8]]) -> Result<(u8, usize), u32> {
    if args.len() > MAX_ARGS {
        return Err(EC_ENARGS);
    }
    let mut f = 0u8;
    let mut i = 0usize;
    loop {
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        match bundle(a) {
            Some((set, clr)) => {
                f = (f | set) & !clr;
                i += 1;
            }
            None => break,
        }
    }
    Ok((f, i))
}

fn put(out: &mut [u8], n: &mut usize, b: u8) -> Result<(), u32> {
    match out.get_mut(*n) {
        Some(s) => {
            *s = b;
            *n += 1;
            Ok(())
        }
        None => Err(EC_EOVER),
    }
}

fn check_text(args: &[&[u8]]) -> Result<usize, u32> {
    if args.len() > MAX_ARGS {
        return Err(EC_ENARGS);
    }
    let mut text = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(EC_EBAL),
        };
        text = match text.checked_add(a.len()) {
            Some(v) => v,
            None => return Err(EC_ELONG),
        };
        i += 1;
    }
    text = match text.checked_add(args.len().saturating_sub(1)) {
        Some(v) => v,
        None => return Err(EC_ELONG),
    };
    if text > TEXT_CAP {
        return Err(EC_ELONG);
    }
    Ok(text)
}

/// 无旗拼接：空格分隔，不追加换行。
pub fn join(args: &[&[u8]], out: &mut [u8]) -> Result<u32, u32> {
    let _ = check_text(args)?;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(EC_EBAL),
        };
        if a.len() > ARG_CAP {
            return Err(EC_ETXT);
        }
        let mut k = 0usize;
        while k < a.len() {
            match a.get(k) {
                Some(&0) => return Err(EC_ENUL),
                Some(_) => {}
                None => return Err(EC_EBAL),
            }
            k += 1;
        }
        i += 1;
    }
    let mut n = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        if i > 0 {
            put(out, &mut n, b' ')?;
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(EC_EBAL),
        };
        let mut k = 0usize;
        while k < a.len() {
            let b = match a.get(k) {
                Some(v) => *v,
                None => return Err(EC_EBAL),
            };
            put(out, &mut n, b)?;
            k += 1;
        }
        i += 1;
    }
    Ok(n as u32)
}

/// 渲染：按旗位翻译 `\n`/`\t`/`\\` 与 `\c`，其余反斜杠原样保留。
pub fn render(args: &[&[u8]], flags: u8, out: &mut [u8]) -> Result<u32, u32> {
    if flags & !FLAG_MASK != 0 {
        return Err(EC_EFLAG);
    }
    let _ = check_text(args)?;
    let mut n = 0usize;
    let mut stop = false;
    let mut i = 0usize;
    while i < args.len() && !stop {
        if i > 0 {
            put(out, &mut n, b' ')?;
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(EC_EBAL),
        };
        let mut k = 0usize;
        while k < a.len() {
            let b = match a.get(k) {
                Some(v) => *v,
                None => return Err(EC_EBAL),
            };
            if b == 0 {
                return Err(EC_ENUL);
            }
            if flags & F_ESC != 0 && b == b'\\' {
                match a.get(k + 1) {
                    Some(&b'n') => {
                        put(out, &mut n, b'\n')?;
                        k += 2;
                        continue;
                    }
                    Some(&b't') => {
                        put(out, &mut n, b'\t')?;
                        k += 2;
                        continue;
                    }
                    Some(&b'\\') => {
                        put(out, &mut n, b'\\')?;
                        k += 2;
                        continue;
                    }
                    Some(&b'c') => {
                        stop = true;
                        break;
                    }
                    _ => {
                        put(out, &mut n, b'\\')?;
                        k += 1;
                        continue;
                    }
                }
            }
            put(out, &mut n, b)?;
            k += 1;
        }
        i += 1;
    }
    if !stop && flags & F_NONL == 0 {
        put(out, &mut n, b'\n')?;
    }
    Ok(n as u32)
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
            return Err(EC_EFROZEN);
        }
        if (row.slot as usize) >= SAMPLES {
            return Err(EC_EID);
        }
        if (row.slot as usize) < self.len {
            return Err(EC_EDUP);
        }
        if (row.slot as usize) != self.len {
            return Err(EC_EORDER);
        }
        if self.len >= self.cap {
            return Err(EC_EFULL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(EC_EID),
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
            return Err(EC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EC_EBAL);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(EC_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(EC_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(EC_EMISMATCH);
                    }
                }
                None => return Err(EC_ENOBS),
            }
            i += 1;
        }
        let vals = [1u32, 1, 3, 0, 6, 5, 4, 4];
        let mut k = 0usize;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(EC_EMISMATCH);
                    }
                }
                _ => return Err(EC_EBAL),
            }
            k += 1;
        }
        let codes = [EC_EOVER, EC_EFLAG, EC_ENUL, EC_ETXT, EC_ELONG, EC_ENARGS];
        let mut c = 0usize;
        let mut j = 8usize;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EC_EMISMATCH);
                        }
                    }
                    None => return Err(EC_EBAL),
                },
                None => return Err(EC_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(EC_EBAL);
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
            return Err(EC_EFROZEN);
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
            return Err(EC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(EC_ESEAL);
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

    let _ = l.register(cell(0, OP_SCAN, parse(&[b"-n", b"hi"]).map(|(_, i)| i as u32)));
    let _ = l.register(cell(1, OP_FLAG, parse(&[b"-n", b"hi"]).map(|(f, _)| f as u32)));
    let _ = l.register(cell(2, OP_FLAG, parse(&[b"-ne", b"x"]).map(|(f, _)| f as u32)));
    let _ = l.register(cell(3, OP_FLAG, parse(&[b"-e", b"-E", b"x"]).map(|(f, _)| f as u32)));
    let _ = l.register(cell(4, OP_RENDER, render(&[b"hello"], 0, &mut buf)));
    let _ = l.register(cell(5, OP_RENDER, render(&[b"hello"], F_NONL, &mut buf)));
    let _ = l.register(cell(6, OP_RENDER, render(&[b"a\\nb"], F_ESC, &mut buf)));
    let _ = l.register(cell(7, OP_JOIN, join(&[b"a", b"", b"b"], &mut buf)));

    let _ = l.register(cell(8, OP_RENDER, render(&[b"hello"], 0, &mut [0u8; 4])));
    let _ = l.register(cell(9, OP_RENDER, render(&[b"x"], 0x04, &mut buf)));
    let _ = l.register(cell(10, OP_JOIN, join(&[b"a\0b"], &mut buf)));
    let _ = l.register(cell(11, OP_JOIN, join(&[b"123456789"], &mut buf)));
    let nine: [&[u8]; 9] = [
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
        b"abcdefgh",
    ];
    let _ = l.register(cell(12, OP_JOIN, join(&nine, &mut buf)));
    let many: [&[u8]; 17] = [
        b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x",
        b"x", b"x", b"x",
    ];
    let _ = l.register(cell(13, OP_SCAN, parse(&many).map(|(_, i)| i as u32)));

    l
}

/// 四路注入：账本自洽、旗序、转义、溢出后恢复。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = l.freeze() == Err(EC_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && again && l.rejects() == 1;

    out[1] = parse(&[b"-ne", b"x"]) == Ok((F_NONL | F_ESC, 1))
        && parse(&[b"-e", b"-E", b"x"]) == Ok((0, 2))
        && parse(&[b"-nEe", b"x"]) == Ok((F_NONL | F_ESC, 1))
        && parse(&[b"-", b"x"]) == Ok((0, 0));

    let mut b = [0u8; 16];
    out[2] = render(&[b"a\\tb"], F_ESC, &mut b) == Ok(4)
        && b.get(1) == Some(&b'\t')
        && b.get(3) == Some(&b'\n');

    let mut small = [0u8; 3];
    let over = render(&[b"hello"], 0, &mut small);
    let mut big = [0u8; 16];
    let fits = render(&[b"hello"], 0, &mut big);
    out[3] = over == Err(EC_EOVER)
        && fits == Ok(6)
        && big.get(0) == Some(&b'h')
        && big.get(5) == Some(&b'\n');
    out
}

/// 十三判据。
pub fn run_unx11486_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11486.echoflags");
    let l = build_ledger();

    s.add(
        "EC1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SCAN),
        "static",
    );
    s.add_dyn(
        "EC2-八允六拒分布",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={} audit={:?}", l.ok_count(), l.err_count(), l.audit()),
    );
    s.add(
        "EC3-正文起点扫描",
        parse(&[b"-n", b"hi"]) == Ok((F_NONL, 1)) && parse(&[b"hi", b"-n"]) == Ok((0, 0)),
        "static",
    );
    s.add(
        "EC4-默认换行",
        {
            let mut b = [0u8; 16];
            render(&[b"hello"], 0, &mut b) == Ok(6)
                && b.get(5) == Some(&b'\n')
                && l.sample(4).map(|x| x.value) == Some(6)
        },
        "static",
    );
    s.add(
        "EC5-关换行",
        {
            let mut b = [0u8; 16];
            render(&[b"hello"], F_NONL, &mut b) == Ok(5)
                && b.get(4) == Some(&b'o')
                && l.sample(5).map(|x| x.value) == Some(5)
        },
        "static",
    );
    s.add(
        "EC6-转义渲染",
        {
            let mut b = [0u8; 16];
            render(&[b"a\\nb"], F_ESC, &mut b) == Ok(4)
                && b.get(0..4) == Some(&b"a\nb\n"[..])
        },
        "static",
    );
    s.add(
        "EC7-旗束顺序",
        parse(&[b"-ne", b"x"]) == Ok((F_NONL | F_ESC, 1))
            && parse(&[b"-e", b"-E", b"x"]) == Ok((0, 2))
            && parse(&[b"-nEe", b"x"]) == Ok((F_NONL | F_ESC, 1))
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(0),
        "static",
    );
    s.add(
        "EC8-空参数占位",
        {
            let mut b = [0u8; 16];
            join(&[b"a", b"", b"b"], &mut b) == Ok(4)
                && b.get(0..4) == Some(&b"a  b"[..])
                && l.sample(7).map(|x| x.value) == Some(4)
        },
        "static",
    );
    s.add(
        "EC9-输出溢出",
        render(&[b"hello"], 0, &mut [0u8; 4]) == Err(EC_EOVER)
            && l.sample(8).map(|x| x.code) == Some(EC_EOVER)
            && l.sample(8).map(|x| x.value) == Some(0),
        "static",
    );
    s.add(
        "EC10-参数个数上限",
        {
            let many: [&[u8]; 17] = [
                b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x", b"x",
                b"x", b"x", b"x", b"x",
            ];
            parse(&many) == Err(EC_ENARGS) && l.sample(13).map(|x| x.code) == Some(EC_ENARGS)
        },
        "static",
    );
    s.add(
        "EC11-NUL 与单参超长",
        join(&[b"a\0b"], &mut [0u8; 16]) == Err(EC_ENUL)
            && join(&[b"123456789"], &mut [0u8; 16]) == Err(EC_ETXT)
            && l.sample(10).map(|x| x.code) == Some(EC_ENUL)
            && l.sample(11).map(|x| x.code) == Some(EC_ETXT),
        "static",
    );
    s.add(
        "EC12-旗位非法与文本超限",
        render(&[b"x"], 0x04, &mut [0u8; 16]) == Err(EC_EFLAG)
            && l.sample(9).map(|x| x.code) == Some(EC_EFLAG)
            && l.sample(12).map(|x| x.code) == Some(EC_ELONG),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(EC_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "EC13-冻结重放与四路",
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
    fn flags_only_prefix() {
        assert_eq!(parse(&[b"-n", b"-e", b"x"]), Ok((F_NONL | F_ESC, 2)));
        assert_eq!(parse(&[b"x", b"-n"]), Ok((0, 0)));
        assert_eq!(parse(&[b"-x", b"y"]), Ok((0, 0)));
    }

    #[test]
    fn clear_and_set_interact() {
        assert_eq!(parse(&[b"-e", b"-E"]), Ok((0, 2)));
        assert_eq!(parse(&[b"-E", b"-e"]), Ok((F_ESC, 2)));
        assert_eq!(parse(&[b"-nEe"]), Ok((F_NONL | F_ESC, 1)));
    }

    #[test]
    fn newline_control() {
        let mut b = [0u8; 16];
        assert_eq!(render(&[b"ab"], 0, &mut b), Ok(3));
        assert_eq!(render(&[b"ab"], F_NONL, &mut b), Ok(2));
    }

    #[test]
    fn escapes_only_with_e() {
        let mut b = [0u8; 16];
        assert_eq!(render(&[b"a\\tb"], 0, &mut b), Ok(5));
        assert_eq!(render(&[b"a\\tb"], F_ESC, &mut b), Ok(4));
        assert_eq!(render(&[b"x\\cy"], F_ESC, &mut b), Ok(1));
    }

    #[test]
    fn join_keeps_empty_fields() {
        let mut b = [0u8; 16];
        assert_eq!(join(&[b"a", b"", b"b"], &mut b), Ok(4));
        assert_eq!(b.get(0..4), Some(&b"a  b"[..]));
    }

    #[test]
    fn overflow_is_bounded() {
        assert_eq!(render(&[b"hello"], 0, &mut [0u8; 4]), Err(EC_EOVER));
        assert_eq!(render(&[b"x"], 0x80, &mut [0u8; 8]), Err(EC_EFLAG));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11486_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(inject_four(), [true; 4]);
    }
}
