//! UNX-F11506 uname 旗族（-s/-r/-m/-a）（0xf0b0 段）
//!
//! `uname` 只有三个真正独立的字段：系统名（-s）、发行版（-r）、机器架构（-m）。
//! `-a` 不是第四个字段，而是"三位全开"的**合旗**——它必须与 `-srm` 产生完全
//! 相同的掩码，否则同一条命令会因为写法不同而输出不同。旗族因此用位掩码表示，
//! 解析的产物是 0..=7 的一个字节，渲染按 s→r→m 的固定次序取字段。
//!
//! 旗族解析的闸门次序：空串 → 单横线（裸 "-" 不是选项）→ `--all` 特例 →
//! 其余长选项一律拒（只认 --all）→ 裸词（没有横线）→ 短旗逐字符校验，
//! 任一字符不认识就整条拒，不做"部分生效"。同一字符重复出现是幂等的
//! （`-ss` 等价 `-s`），因为合旗本来就该幂等。
//!
//! 渲染侧的闸门：掩码为 0 或含 0..=7 之外的位 → `EBADMASK`；缓冲不足 →
//! `EBUF` 且一格不写。字段本身在开户时校验：空字段与超容字段都不许入账，
//! 因为 uname 的每一列都必须是可打印的非空串。
//!
//! 账本十四格：八格登记成功路径（默认掩码、三个单旗、一个合旗、`-a`、
//! `--all`、一次三字段渲染、一次位测试），六格登记各类拒绝（未知旗、未知长旗、
//! 裸词、单横线、非法掩码、缓冲不足）。审计逐格复算，封印折叠全账，
//! 冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf0b0;

/// 未知短旗。
pub const UN_EBADFLAG: u32 = SEG_BASE + 0x01;
/// 未知长旗（只认 --all）。
pub const UN_EBADLONG: u32 = SEG_BASE + 0x02;
/// 裸词不是选项。
pub const UN_EBARE: u32 = SEG_BASE + 0x03;
/// 空串没有旗。
pub const UN_EEMPTY: u32 = SEG_BASE + 0x04;
/// 单横线不是选项。
pub const UN_EDASH: u32 = SEG_BASE + 0x05;
/// 掩码非法。
pub const UN_EBADMASK: u32 = SEG_BASE + 0x06;
/// 渲染缓冲不足。
pub const UN_EBUF: u32 = SEG_BASE + 0x07;
/// 字段超出容量。
pub const UN_ELONG: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const UN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const UN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const UN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const UN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const UN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 字段容量。
pub const CAP: usize = 32;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 旗：系统名。
pub const FLAG_S: u8 = 0x01;
/// 旗：发行版。
pub const FLAG_R: u8 = 0x02;
/// 旗：机器架构。
pub const FLAG_M: u8 = 0x04;
/// 合旗：三位全开。
pub const FLAG_A: u8 = 0x07;
/// 默认旗（只报系统名）。
pub const FLAG_DEFAULT: u8 = FLAG_S;

/// 操作码：解析旗。
pub const OP_PARSE: u8 = 1;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 2;
/// 操作码：位测试。
pub const OP_HAS: u8 = 3;
/// 操作码：计字段。
pub const OP_FIELDS: u8 = 4;

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

/// 默认掩码。
pub const fn default_mask() -> u8 {
    FLAG_DEFAULT
}

/// 掩码里置位的字段个数。
pub fn field_count(mask: u8) -> u32 {
    let mut n = 0u32;
    if mask & FLAG_S != 0 {
        n += 1;
    }
    if mask & FLAG_R != 0 {
        n += 1;
    }
    if mask & FLAG_M != 0 {
        n += 1;
    }
    n
}

/// 掩码是否置了某旗。
pub fn has(mask: u8, flag: u8) -> bool {
    mask & flag != 0
}

/// 解析一条旗参数。
pub fn parse(arg: &[u8]) -> Result<u8, u32> {
    if arg.is_empty() {
        return Err(UN_EEMPTY);
    }
    if arg == b"-" {
        return Err(UN_EDASH);
    }
    if arg == b"--all" {
        return Ok(FLAG_A);
    }
    let dash0 = arg.get(0).copied() == Some(b'-');
    let dash1 = arg.get(1).copied() == Some(b'-');
    if dash0 && dash1 {
        return Err(UN_EBADLONG);
    }
    if !dash0 {
        return Err(UN_EBARE);
    }
    let mut mask = 0u8;
    let mut i = 1usize;
    while i < arg.len() {
        match arg.get(i).copied() {
            Some(b's') => mask |= FLAG_S,
            Some(b'r') => mask |= FLAG_R,
            Some(b'm') => mask |= FLAG_M,
            Some(b'a') => mask |= FLAG_A,
            _ => return Err(UN_EBADFLAG),
        }
        i += 1;
    }
    if mask == 0 {
        return Err(UN_EDASH);
    }
    Ok(mask)
}

/// 三个字段的标识串。
#[derive(Clone, Copy, Debug)]
pub struct Uname {
    sys: [u8; CAP],
    slen: u8,
    rel: [u8; CAP],
    rlen: u8,
    mach: [u8; CAP],
    mlen: u8,
}

impl Uname {
    /// 开户。
    pub fn new(sys: &[u8], rel: &[u8], mach: &[u8]) -> Result<Uname, u32> {
        if sys.is_empty() || rel.is_empty() || mach.is_empty() {
            return Err(UN_EEMPTY);
        }
        if sys.len() > CAP || rel.len() > CAP || mach.len() > CAP {
            return Err(UN_ELONG);
        }
        let mut u = Uname {
            sys: [0u8; CAP],
            slen: 0,
            rel: [0u8; CAP],
            rlen: 0,
            mach: [0u8; CAP],
            mlen: 0,
        };
        u.slen = fill(&mut u.sys, sys);
        u.rlen = fill(&mut u.rel, rel);
        u.mlen = fill(&mut u.mach, mach);
        Ok(u)
    }

    /// 系统名。
    pub fn sys(&self) -> Option<&[u8]> {
        self.sys.get(..self.slen as usize)
    }

    /// 发行版。
    pub fn rel(&self) -> Option<&[u8]> {
        self.rel.get(..self.rlen as usize)
    }

    /// 机器架构。
    pub fn mach(&self) -> Option<&[u8]> {
        self.mach.get(..self.mlen as usize)
    }

    /// 渲染总长。
    pub fn print_len(&self, mask: u8) -> Result<u32, u32> {
        if mask == 0 || mask & !FLAG_A != 0 {
            return Err(UN_EBADMASK);
        }
        let mut n = 0u32;
        let mut fields = 0u32;
        if mask & FLAG_S != 0 {
            n = n.saturating_add(self.slen as u32);
            fields += 1;
        }
        if mask & FLAG_R != 0 {
            n = n.saturating_add(self.rlen as u32);
            fields += 1;
        }
        if mask & FLAG_M != 0 {
            n = n.saturating_add(self.mlen as u32);
            fields += 1;
        }
        if fields > 1 {
            n = n.saturating_add(fields - 1);
        }
        Ok(n)
    }

    /// 按 s→r→m 次序渲染。
    pub fn render(&self, mask: u8, dst: &mut [u8]) -> Result<u32, u32> {
        let need = self.print_len(mask)? as usize;
        if dst.len() < need {
            return Err(UN_EBUF);
        }
        let mut at = 0usize;
        if mask & FLAG_S != 0 {
            at = put(dst, at, self.sys());
        }
        if mask & FLAG_R != 0 {
            if at > 0 {
                at = put(dst, at, Some(b" "));
            }
            at = put(dst, at, self.rel());
        }
        if mask & FLAG_M != 0 {
            if at > 0 {
                at = put(dst, at, Some(b" "));
            }
            at = put(dst, at, self.mach());
        }
        Ok(at as u32)
    }
}

/// 抄入字段并返回长度。
fn fill(dst: &mut [u8], src: &[u8]) -> u8 {
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
fn put(dst: &mut [u8], at: usize, src: Option<&[u8]>) -> usize {
    let bytes = match src {
        Some(v) => v,
        None => return at,
    };
    let mut i = 0usize;
    while i < bytes.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(at + i), bytes.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    at + bytes.len()
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

/// `Result<u8, u32>` → 样本。
fn mask_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
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
            return Err(UN_ESLOT);
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
            return Err(UN_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 2, 5, 7, 7, 18, 1];
        const CODES: [u32; 6] =
            [UN_EBADFLAG, UN_EBADLONG, UN_EBARE, UN_EDASH, UN_EBADMASK, UN_EBUF];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UN_EBAL),
            };
            if s.slot as usize != i {
                return Err(UN_ESLOT);
            }
            if s.op == 0 {
                return Err(UN_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(UN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(UN_EVALUE);
                        }
                    }
                    None => return Err(UN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(UN_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(UN_EVALUE);
                        }
                    }
                    None => return Err(UN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(UN_EBAL);
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
            return Err(UN_EBAL);
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
            Err(UN_ESEAL)
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

/// 十四格账本：默认掩码 → 三单旗 → 合旗 → -a → --all → 渲染 → 位测试 →
/// 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let u = match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(mask_cell(0, OP_PARSE, Ok(default_mask())));
    let _ = l.register(mask_cell(1, OP_PARSE, parse(b"-s")));
    let _ = l.register(mask_cell(2, OP_PARSE, parse(b"-r")));
    let _ = l.register(mask_cell(3, OP_PARSE, parse(b"-sm")));
    let _ = l.register(mask_cell(4, OP_PARSE, parse(b"-a")));
    let _ = l.register(mask_cell(5, OP_PARSE, parse(b"--all")));

    let mut buf = [0u8; 32];
    let _ = l.register(word_cell(6, OP_RENDER, u.render(FLAG_A, &mut buf)));
    let _ = l.register(word_cell(7, OP_HAS, Ok(has(FLAG_S | FLAG_M, FLAG_M) as u32)));

    let _ = l.register(mask_cell(8, OP_PARSE, parse(b"-z")));
    let _ = l.register(mask_cell(9, OP_PARSE, parse(b"--sys")));
    let _ = l.register(mask_cell(10, OP_PARSE, parse(b"s")));
    let _ = l.register(mask_cell(11, OP_PARSE, parse(b"-")));
    let _ = l.register(word_cell(12, OP_RENDER, u.render(0, &mut buf)));

    let mut tiny = [0u8; 8];
    let _ = l.register(word_cell(13, OP_RENDER, u.render(FLAG_A, &mut tiny)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：单旗、合旗与全开等价。
    out[1] = parse(b"-s") == Ok(1)
        && parse(b"-r") == Ok(2)
        && parse(b"-m") == Ok(4)
        && parse(b"-sm") == Ok(5)
        && parse(b"-srm") == Ok(7)
        && parse(b"-a") == Ok(7)
        && parse(b"--all") == Ok(7)
        && parse(b"-ss") == Ok(1)
        && parse(b"-as") == Ok(7)
        && default_mask() == 1
        && field_count(5) == 2;

    // 二路：渲染按 s→r→m，长度随掩码。
    let u = match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut buf = [0u8; 32];
    let all = u.render(7, &mut buf);
    let all_ok = all == Ok(18) && buf.get(..18) == Some(&b"Varix 0.1.0 x86_64"[..]);
    let mut b2 = [0u8; 32];
    let two = u.render(5, &mut b2);
    let mut b3 = [0u8; 32];
    let one = u.render(2, &mut b3);
    out[2] = all_ok
        && two == Ok(12)
        && b2.get(..12) == Some(&b"Varix x86_64"[..])
        && one == Ok(5)
        && b3.get(..5) == Some(&b"0.1.0"[..])
        && u.print_len(3) == Ok(11)
        && has(7, FLAG_R)
        && !has(2, FLAG_M);

    // 三路：六类失败各自可辨。
    let mut tiny = [0u8; 8];
    let long = [b'x'; CAP + 1];
    out[3] = parse(b"-z") == Err(UN_EBADFLAG)
        && parse(b"-sz") == Err(UN_EBADFLAG)
        && parse(b"--sys") == Err(UN_EBADLONG)
        && parse(b"s") == Err(UN_EBARE)
        && parse(b"") == Err(UN_EEMPTY)
        && parse(b"-") == Err(UN_EDASH)
        && u.render(0, &mut tiny) == Err(UN_EBADMASK)
        && u.render(8, &mut tiny) == Err(UN_EBADMASK)
        && u.render(7, &mut tiny) == Err(UN_EBUF)
        && matches!(Uname::new(b"Varix", &long, b"x86_64"), Err(UN_ELONG))
        && matches!(Uname::new(b"", b"0.1.0", b"x86_64"), Err(UN_EEMPTY));

    out
}

/// 十三项判据。
pub fn run_unx11506_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11506.uname_flags");
    let l = build_ledger();

    s.add_dyn(
        "UN01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "UN02-单旗与短旗组合",
        parse(b"-s") == Ok(1)
            && parse(b"-r") == Ok(2)
            && parse(b"-m") == Ok(4)
            && parse(b"-rm") == Ok(6)
            && parse(b"-srm") == Ok(7)
            && l.sample(3).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "UN03-合旗等同全开",
        parse(b"-a") == Ok(7)
            && parse(b"--all") == Ok(7)
            && parse(b"-as") == Ok(7)
            && parse(b"-a") == parse(b"-srm")
            && l.sample(4).map(|x| x.value) == Some(7),
        "static",
    );

    s.add(
        "UN04-默认只报系统名",
        default_mask() == 1
            && field_count(default_mask()) == 1
            && field_count(7) == 3
            && field_count(0) == 0
            && l.sample(0).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "UN05-未知短旗被拒",
        parse(b"-z") == Err(UN_EBADFLAG)
            && parse(b"-sz") == Err(UN_EBADFLAG)
            && parse(b"-szm") == Err(UN_EBADFLAG)
            && l.sample(8).map(|x| x.code) == Some(UN_EBADFLAG),
        "static",
    );

    s.add(
        "UN06-长旗只认all",
        parse(b"--sys") == Err(UN_EBADLONG)
            && parse(b"--") == Err(UN_EBADLONG)
            && parse(b"--allx") == Err(UN_EBADLONG)
            && parse(b"--all") == Ok(7)
            && l.sample(9).map(|x| x.code) == Some(UN_EBADLONG),
        "static",
    );

    s.add(
        "UN07-裸词不是选项",
        parse(b"s") == Err(UN_EBARE)
            && parse(b"sr") == Err(UN_EBARE)
            && parse(b"all") == Err(UN_EBARE)
            && l.sample(10).map(|x| x.code) == Some(UN_EBARE),
        "static",
    );

    s.add(
        "UN08-空与单横线被拒",
        parse(b"") == Err(UN_EEMPTY)
            && parse(b"-") == Err(UN_EDASH)
            && l.sample(11).map(|x| x.code) == Some(UN_EDASH),
        "static",
    );

    s.add(
        "UN09-渲染次序固定",
        {
            let u = match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut a = [0u8; 32];
            let mut b = [0u8; 32];
            let mut c = [0u8; 32];
            u.render(7, &mut a) == Ok(18)
                && a.get(..18) == Some(&b"Varix 0.1.0 x86_64"[..])
                && u.render(5, &mut b) == Ok(12)
                && b.get(..12) == Some(&b"Varix x86_64"[..])
                && u.render(2, &mut c) == Ok(5)
                && c.get(..5) == Some(&b"0.1.0"[..])
                && l.sample(6).map(|x| x.value) == Some(18)
        },
        "static",
    );

    s.add(
        "UN10-非法掩码被拒",
        {
            let u = match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut buf = [0u8; 32];
            u.render(0, &mut buf) == Err(UN_EBADMASK)
                && u.render(8, &mut buf) == Err(UN_EBADMASK)
                && u.print_len(9) == Err(UN_EBADMASK)
                && l.sample(12).map(|x| x.code) == Some(UN_EBADMASK)
        },
        "static",
    );

    s.add(
        "UN11-缓冲边界",
        {
            let u = match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut exact = [0u8; 18];
            let mut short = [0u8; 17];
            u.render(7, &mut exact) == Ok(18)
                && u.render(7, &mut short) == Err(UN_EBUF)
                && l.sample(13).map(|x| x.code) == Some(UN_EBUF)
        },
        "static",
    );

    s.add(
        "UN12-字段容量边界",
        {
            let exact = [b'x'; CAP];
            let over = [b'x'; CAP + 1];
            let ok = match Uname::new(&exact, &exact, &exact) {
                Ok(v) => v.print_len(7),
                Err(_) => Err(0),
            };
            ok == Ok(CAP as u32 * 3 + 2)
                && matches!(Uname::new(&over, b"r", b"m"), Err(UN_ELONG))
                && matches!(Uname::new(b"s", b"", b"m"), Err(UN_EEMPTY))
        },
        "static",
    );

    s.add(
        "UN13-冻结重放四注入",
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

    fn names() -> Uname {
        match Uname::new(b"Varix", b"0.1.0", b"x86_64") {
            Ok(v) => v,
            Err(_) => panic!("uname"),
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
        let s = run_unx11506_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn parse_clusters() {
        assert_eq!(parse(b"-s"), Ok(1));
        assert_eq!(parse(b"-r"), Ok(2));
        assert_eq!(parse(b"-m"), Ok(4));
        assert_eq!(parse(b"-sm"), Ok(5));
        assert_eq!(parse(b"-srm"), Ok(7));
        assert_eq!(parse(b"-a"), Ok(7));
        assert_eq!(parse(b"--all"), Ok(7));
        assert_eq!(parse(b"-ss"), Ok(1));
    }

    #[test]
    fn parse_gates() {
        assert_eq!(parse(b""), Err(UN_EEMPTY));
        assert_eq!(parse(b"-"), Err(UN_EDASH));
        assert_eq!(parse(b"--sys"), Err(UN_EBADLONG));
        assert_eq!(parse(b"s"), Err(UN_EBARE));
        assert_eq!(parse(b"-z"), Err(UN_EBADFLAG));
        assert_eq!(parse(b"-sz"), Err(UN_EBADFLAG));
    }

    #[test]
    fn render_order() {
        let u = names();
        let mut buf = [0u8; 32];
        assert_eq!(u.render(7, &mut buf), Ok(18));
        assert_eq!(&buf[..18], b"Varix 0.1.0 x86_64");
        assert_eq!(u.render(5, &mut buf), Ok(12));
        assert_eq!(&buf[..12], b"Varix x86_64");
        assert_eq!(u.render(4, &mut buf), Ok(6));
        assert_eq!(&buf[..6], b"x86_64");
    }

    #[test]
    fn mask_gates() {
        let u = names();
        let mut buf = [0u8; 32];
        assert_eq!(u.render(0, &mut buf), Err(UN_EBADMASK));
        assert_eq!(u.render(8, &mut buf), Err(UN_EBADMASK));
        assert_eq!(u.print_len(9), Err(UN_EBADMASK));
        assert_eq!(has(5, FLAG_M), true);
        assert_eq!(has(2, FLAG_M), false);
    }

    #[test]
    fn field_capacity() {
        let exact = [b'x'; CAP];
        let over = [b'x'; CAP + 1];
        let u = match Uname::new(&exact, &exact, &exact) {
            Ok(v) => v,
            Err(_) => panic!("uname"),
        };
        assert_eq!(u.print_len(7), Ok(98));
        assert_eq!(Uname::new(&over, b"r", b"m").is_err(), true);
        assert_eq!(Uname::new(b"", b"r", b"m").is_err(), true);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
