//! UNX-F11618 坏 ELF 拒收文案逐字对照（内核 / ld.so 双源）
//!
//! 镜面：同一份坏 ELF，内核装载器与 ld.so 各自会吐一行拒收文案。两源自
//! 说自话时用户只能看到半句真话；两源打架时用户看到两句互相矛盾的假话。
//! 本单把「双源文案关系」收成四态并逐字核对：
//!
//! 1. `agree`——两源逐字节同文，视为一个声音；
//! 2. `kernel-only`——只有内核发声，ld.so 沉默；
//! 3. `ldso-only`——只有 ld.so 发声，内核沉默；
//! 4. `conflict`——两源都发声且不同文，必须显式登记，不许默认合并。
//!
//! 判序要害：先比字节（同文/异文），再判模板（前缀 `./prog: `、行尾换行、
//! 无内嵌 NUL），最后对申报关系。模板违规优先于关系错判——一行不合模板
//! 的文案即使「说对了」也不可放行，否则下游解析器会把两源都读坏。
//!
//! 语料十四槽：八活（agree×2 / kernel-only×2 / ldso-only×2 / conflict×2），
//! 六死（同文判错×2 / 越界发言×2 / 冲突漏判×1 / 模板违规×1）。
//!
//! 闸门次序：名称与段基址 → 码族与判面 → 双源比较 → 逐槽对判 → 语料互异 →
//! 汇总计数 → 渲染逐字 → 红档显式 → 文案逐字 → 槽号与覆盖 → 校验器与账本 →
//! 对账冻结 → 冻结重放四注入。
//!
//! 契约：纯 `no_std` 函数式代码，无 `unsafe`，无 `unwrap`/`expect`/
//! `panic!`，失败一律 `Result<_, u32>`；只读、无全局态。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 本单段基址。
pub const SEG_BASE: u32 = 0xfc10;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// ENOEXEC。
pub const ENOEXEC: i32 = 8;

/// 文案前缀。
pub const PREFIX: &str = "./prog: ";

/// 行尾字节。
pub const NEWLINE: u8 = b'\n';

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 同文判错。
pub const MT_EBYTE: u32 = SEG_BASE + 0x01;
/// 越界发言（内核）。
pub const MT_EKERNEL: u32 = SEG_BASE + 0x02;
/// 越界发言（ld.so）。
pub const MT_ELDSO: u32 = SEG_BASE + 0x03;
/// 冲突漏判。
pub const MT_ECONF: u32 = SEG_BASE + 0x04;
/// 模板违规。
pub const MT_ETEXT: u32 = SEG_BASE + 0x05;
/// 双源皆默。
pub const MT_EPAIR: u32 = SEG_BASE + 0x06;
/// 状态违例。
pub const MT_ESTATE: u32 = SEG_BASE + 0x07;
/// 红档。
pub const MT_EVER: u32 = SEG_BASE + 0x08;
/// 账不平。
pub const MT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽错位。
pub const MT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const MT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 封存违例。
pub const MT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 未观测。
pub const MT_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 取对。
pub const OP_PAIR: u32 = 1;
/// 比较。
pub const OP_COMPARE: u32 = 2;
/// 模板。
pub const OP_TEMPLATE: u32 = 3;
/// 文案。
pub const OP_TEXT: u32 = 4;
/// 汇总。
pub const OP_REPORT: u32 = 5;
/// 对判。
pub const OP_JUDGE: u32 = 6;
/// 封存。
pub const OP_SEAL: u32 = 7;

// ---------------------------------------------------------------------------
// 指纹
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折一个字节。
pub const fn fold_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(FNV_PRIME)
}

/// 折一个字。
pub const fn fold_word(h: u64, w: u64) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 8 {
        x = fold_byte(x, ((w >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 折一个字符串（字节 + 长度）。
pub fn fold_str(h: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut x = h;
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(v) => x = fold_byte(x, *v),
            None => break,
        }
        i += 1;
    }
    fold_word(x, b.len() as u64)
}

/// 十进制文本。
pub fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 十六进制文本（带 `0x`，小写）。
pub fn hex_text(v: u64) -> String {
    let mut buf = [0u8; 16];
    let mut n = v;
    let mut i = 16usize;
    if n == 0 {
        return String::from("0x0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        let d = (n & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + (d - 10) };
        n >>= 4;
    }
    let mut s = String::from("0x");
    while i < 16 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 有符号十进制文本。
pub fn i64_text(v: i64) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(&u32_text(v.unsigned_abs() as u32));
        s
    } else {
        u32_text(v as u32)
    }
}

// ---------------------------------------------------------------------------
// 双源关系
// ---------------------------------------------------------------------------

/// 双源文案四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 两源同文。
    Agree,
    /// 仅内核发声。
    KernelOnly,
    /// 仅 ld.so 发声。
    LdsoOnly,
    /// 两源异文。
    Conflict,
}

impl Verdict {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Verdict::Agree => "agree",
            Verdict::KernelOnly => "kernel-only",
            Verdict::LdsoOnly => "ldso-only",
            Verdict::Conflict => "conflict",
        }
    }

    /// 序号。
    pub const fn index(&self) -> u32 {
        match self {
            Verdict::Agree => 0,
            Verdict::KernelOnly => 1,
            Verdict::LdsoOnly => 2,
            Verdict::Conflict => 3,
        }
    }

    /// 序号反查。
    pub const fn from_index(i: u32) -> Option<Verdict> {
        match i {
            0 => Some(Verdict::Agree),
            1 => Some(Verdict::KernelOnly),
            2 => Some(Verdict::LdsoOnly),
            3 => Some(Verdict::Conflict),
            _ => None,
        }
    }

    /// 内核是否发声。
    pub const fn speaks_kernel(&self) -> bool {
        !matches!(self, Verdict::LdsoOnly)
    }

    /// ld.so 是否发声。
    pub const fn speaks_ldso(&self) -> bool {
        !matches!(self, Verdict::KernelOnly)
    }

    /// 是否双源同发。
    pub const fn both_speak(&self) -> bool {
        matches!(self, Verdict::Agree | Verdict::Conflict)
    }

    /// 是否单源。
    pub const fn single(&self) -> bool {
        matches!(self, Verdict::KernelOnly | Verdict::LdsoOnly)
    }
}

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// 通过。
    Ok,
    /// 同文判错。
    Byte,
    /// 越界发言（内核）。
    Kernel,
    /// 越界发言（ld.so）。
    Ldso,
    /// 冲突漏判。
    Conf,
    /// 模板违规。
    Text,
    /// 双源皆默。
    Pair,
}

impl Fault {
    /// 名称。
    pub const fn name(&self) -> &'static str {
        match self {
            Fault::Ok => "ok",
            Fault::Byte => "byte",
            Fault::Kernel => "kernel",
            Fault::Ldso => "ldso",
            Fault::Conf => "conflict",
            Fault::Text => "text",
            Fault::Pair => "pair",
        }
    }

    /// 错误码。
    pub const fn code(&self) -> u32 {
        match self {
            Fault::Ok => 0,
            Fault::Byte => MT_EBYTE,
            Fault::Kernel => MT_EKERNEL,
            Fault::Ldso => MT_ELDSO,
            Fault::Conf => MT_ECONF,
            Fault::Text => MT_ETEXT,
            Fault::Pair => MT_EPAIR,
        }
    }

    /// errno。
    pub const fn errno(&self) -> i32 {
        match self {
            Fault::Ok => 0,
            _ => ENOEXEC,
        }
    }

    /// 是否拒收面。
    pub const fn alive(&self) -> bool {
        !matches!(self, Fault::Ok)
    }
}

// ---------------------------------------------------------------------------
// 比较原语
// ---------------------------------------------------------------------------

/// 两源关系（两源皆默时 `None`）。
pub fn compare(kernel: &str, ldso: &str) -> Option<Verdict> {
    let ke = kernel.is_empty();
    let le = ldso.is_empty();
    if ke && le {
        return None;
    }
    if ke {
        return Some(Verdict::LdsoOnly);
    }
    if le {
        return Some(Verdict::KernelOnly);
    }
    if kernel.as_bytes() == ldso.as_bytes() {
        Some(Verdict::Agree)
    } else {
        Some(Verdict::Conflict)
    }
}

/// 首个不同字节的下标（等长且相同为 `None`，长度不同取短者长度）。
pub fn byte_diff(kernel: &str, ldso: &str) -> Option<usize> {
    let a = kernel.as_bytes();
    let b = ldso.as_bytes();
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return Some(i),
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return Some(i),
        };
        if x != y {
            return Some(i);
        }
        i += 1;
    }
    if a.len() == b.len() {
        None
    } else {
        Some(n)
    }
}

/// 长度差（ld.so 减内核）。
pub fn len_delta(kernel: &str, ldso: &str) -> i64 {
    ldso.len() as i64 - kernel.len() as i64
}

/// 文案模板：空串或（前缀 `./prog: `、行尾换行、无内嵌 NUL）。
pub fn template_ok(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let b = s.as_bytes();
    let p = PREFIX.as_bytes();
    if b.len() < p.len() + 1 {
        return false;
    }
    let mut i = 0usize;
    while i < p.len() {
        let x = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != p[i] {
            return false;
        }
        i += 1;
    }
    let last = match b.get(b.len() - 1) {
        Some(v) => *v,
        None => return false,
    };
    if last != NEWLINE {
        return false;
    }
    let mut j = 0usize;
    while j < b.len() {
        match b.get(j) {
            Some(v) => {
                if *v == 0 {
                    return false;
                }
            }
            None => return false,
        }
        j += 1;
    }
    true
}

/// 首行（不改变内容，仅截到首个换行之后）。
pub fn first_line(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(v) => {
                if *v == NEWLINE {
                    return match s.get(0..=i) {
                        Some(x) => x,
                        None => s,
                    };
                }
            }
            None => break,
        }
        i += 1;
    }
    s
}

/// 行数（换行计数；空串为 0）。
pub fn line_count(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            if *v == NEWLINE {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 一对槽
// ---------------------------------------------------------------------------

/// 一条双源对照槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    /// 槽名。
    pub name: &'static str,
    /// 内核文案。
    pub kernel_text: &'static str,
    /// ld.so 文案。
    pub ldso_text: &'static str,
    /// 申报关系。
    pub claim: Verdict,
}

impl Pair {
    /// 构造。
    pub const fn new(
        name: &'static str,
        kernel_text: &'static str,
        ldso_text: &'static str,
        claim: Verdict,
    ) -> Pair {
        Pair { name, kernel_text, ldso_text, claim }
    }

    /// 实测关系。
    pub fn actual(&self) -> Option<Verdict> {
        compare(self.kernel_text, self.ldso_text)
    }

    /// 判面。
    pub fn judge(&self) -> Fault {
        judge(self)
    }

    /// 期望错误码（通过为 0）。
    pub fn expected(&self) -> u32 {
        self.judge().code()
    }

    /// 两源皆默。
    pub const fn silent(&self) -> bool {
        self.kernel_text.is_empty() && self.ldso_text.is_empty()
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_str(h, self.name);
        h = fold_str(h, self.kernel_text);
        h = fold_str(h, self.ldso_text);
        fold_word(h, self.claim.index() as u64)
    }
}

/// 对判：先比字节，再判模板，最后对申报关系。
pub fn judge(p: &Pair) -> Fault {
    let actual = match compare(p.kernel_text, p.ldso_text) {
        Some(v) => v,
        None => return Fault::Pair,
    };
    if !template_ok(p.kernel_text) || !template_ok(p.ldso_text) {
        return Fault::Text;
    }
    if actual == p.claim {
        return Fault::Ok;
    }
    match p.claim {
        Verdict::Agree => Fault::Byte,
        Verdict::KernelOnly => Fault::Kernel,
        Verdict::LdsoOnly => Fault::Ldso,
        Verdict::Conflict => Fault::Conf,
    }
}

/// 实测关系或错误码。
pub fn judge_verdict(p: &Pair) -> Result<Verdict, u32> {
    match p.judge() {
        Fault::Ok => match p.actual() {
            Some(v) => Ok(v),
            None => Err(MT_EPAIR),
        },
        f => Err(f.code()),
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 十四槽。
pub const PAIRS: [Pair; CORPUS] = [
    // 八活：四态各二。
    Pair::new(
        "magic-agree",
        "./prog: cannot execute: bad ELF magic\n",
        "./prog: cannot execute: bad ELF magic\n",
        Verdict::Agree,
    ),
    Pair::new(
        "class-agree",
        "./prog: cannot execute: ELF class mismatch\n",
        "./prog: cannot execute: ELF class mismatch\n",
        Verdict::Agree,
    ),
    Pair::new(
        "interp-kernel",
        "./prog: cannot execute: interpreter missing\n",
        "",
        Verdict::KernelOnly,
    ),
    Pair::new(
        "needed-kernel",
        "./prog: cannot execute: needed library missing\n",
        "",
        Verdict::KernelOnly,
    ),
    Pair::new(
        "phent-ldso",
        "",
        "./prog: ld.so: unsupported phentsize\n",
        Verdict::LdsoOnly,
    ),
    Pair::new(
        "reloc-ldso",
        "",
        "./prog: ld.so: unresolved relocation\n",
        Verdict::LdsoOnly,
    ),
    Pair::new(
        "type-conflict",
        "./prog: cannot execute: bad e_type\n",
        "./prog: ld.so: unexpected e_type\n",
        Verdict::Conflict,
    ),
    Pair::new(
        "segment-conflict",
        "./prog: cannot execute: segment beyond EOF\n",
        "./prog: ld.so: LOAD segment beyond EOF\n",
        Verdict::Conflict,
    ),
    // 六死：同文判错二、越界发言二、冲突漏判一、模板违规一。
    Pair::new(
        "magic-byte",
        "./prog: cannot execute: bad ELF magic\n",
        "./prog: cannot execute: bad ELF magik\n",
        Verdict::Agree,
    ),
    Pair::new(
        "phoff-byte",
        "./prog: cannot execute: bad phoff\n",
        "./prog: cannot execute: Bad phoff\n",
        Verdict::Agree,
    ),
    Pair::new(
        "phent-kernel",
        "./prog: cannot execute: bad phentsize\n",
        "./prog: ld.so: bad phentsize\n",
        Verdict::KernelOnly,
    ),
    Pair::new(
        "class-ldso",
        "./prog: cannot execute: ELF class mismatch\n",
        "./prog: ld.so: ELF class mismatch\n",
        Verdict::LdsoOnly,
    ),
    Pair::new(
        "needed-conflict",
        "./prog: cannot execute: needed library missing\n",
        "./prog: cannot execute: needed library missing\n",
        Verdict::Conflict,
    ),
    Pair::new(
        "template-text",
        "cannot execute: bad interpreter\n",
        "",
        Verdict::KernelOnly,
    ),
];

/// 逐槽期望判面。
pub const EXPECTED: [Fault; CORPUS] = [
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Ok,
    Fault::Byte,
    Fault::Byte,
    Fault::Kernel,
    Fault::Ldso,
    Fault::Conf,
    Fault::Text,
];

/// 取一槽。
pub fn slot_at(i: usize) -> Option<&'static Pair> {
    PAIRS.get(i)
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 汇总面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 总数。
    pub total: u32,
    /// 通过数。
    pub accept: u32,
    /// 拒收数。
    pub reject: u32,
    /// 同文计数。
    pub agree: u32,
    /// 仅内核计数。
    pub kernel: u32,
    /// 仅 ld.so 计数。
    pub ldso: u32,
    /// 冲突计数。
    pub conflict: u32,
    /// 命中数。
    pub matched: u32,
}

impl Report {
    /// 空面。
    pub const fn new() -> Report {
        Report { total: 0, accept: 0, reject: 0, agree: 0, kernel: 0, ldso: 0, conflict: 0, matched: 0 }
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 命中率（千分比，四舍五入）。
    pub const fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.matched * 1000 + self.total / 2) / self.total
    }

    /// 四态计数是否等于通过数。
    pub const fn balanced(&self) -> bool {
        self.agree + self.kernel + self.ldso + self.conflict == self.accept
    }

    /// 是否全绿。
    pub const fn green(&self) -> bool {
        self.total == CORPUS as u32 && self.matched == self.total && self.balanced()
    }

    /// 闸门。
    pub const fn gate(&self) -> Result<(), u32> {
        if self.total != CORPUS as u32 {
            return Err(MT_ENOBS);
        }
        if self.matched != self.total {
            return Err(MT_EVER);
        }
        if !self.balanced() {
            return Err(MT_EBAL);
        }
        Ok(())
    }

    /// 计一槽。
    pub fn tally(&mut self, p: &Pair) {
        self.total += 1;
        if p.judge() == Fault::Ok {
            self.accept += 1;
            match p.claim {
                Verdict::Agree => self.agree += 1,
                Verdict::KernelOnly => self.kernel += 1,
                Verdict::LdsoOnly => self.ldso += 1,
                Verdict::Conflict => self.conflict += 1,
            }
        } else {
            self.reject += 1;
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("textpair corpus=");
        s.push_str(&u32_text(self.total));
        s.push_str(" accept=");
        s.push_str(&u32_text(self.accept));
        s.push_str(" reject=");
        s.push_str(&u32_text(self.reject));
        s.push_str(" agree=");
        s.push_str(&u32_text(self.agree));
        s.push_str(" kernel=");
        s.push_str(&u32_text(self.kernel));
        s.push_str(" ldso=");
        s.push_str(&u32_text(self.ldso));
        s.push_str(" conflict=");
        s.push_str(&u32_text(self.conflict));
        s.push_str(" match=");
        s.push_str(&u32_text(self.matched));
        s.push_str(" rate=");
        s.push_str(&u32_text(self.rate()));
        s.push_str("ppm\n");
        s
    }
}

/// 全跑。
pub fn run() -> Result<Report, u32> {
    run_with(None)
}

/// 带红档覆盖的全跑。
pub fn run_with(over: Option<(usize, u32)>) -> Result<Report, u32> {
    if let Some((i, _)) = over {
        if i >= CORPUS {
            return Err(MT_ESLOT);
        }
    }
    let mut r = Report::new();
    let mut i = 0usize;
    while i < PAIRS.len() {
        let p = match PAIRS.get(i) {
            Some(v) => *v,
            None => return Err(MT_ESLOT),
        };
        r.tally(&p);
        let overridden = match over {
            Some((k, _)) => k == i,
            None => false,
        };
        if p.judge() == EXPECTED[i] && !overridden {
            r.matched += 1;
        }
        i += 1;
    }
    Ok(r)
}

/// 覆盖率闸门。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(MT_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 单行诊断。
pub fn message(p: &Pair) -> String {
    match judge(p) {
        Fault::Ok => {
            let mut s = String::from("ok: ");
            s.push_str(p.name);
            s.push(' ');
            s.push_str(p.claim.name());
            s.push('\n');
            s
        }
        Fault::Text => {
            let mut s = String::from("./prog: textpair ");
            s.push_str(p.name);
            s.push_str(": template violation\n");
            s
        }
        Fault::Pair => {
            let mut s = String::from("./prog: textpair ");
            s.push_str(p.name);
            s.push_str(": both sources silent\n");
            s
        }
        _ => {
            let got = match compare(p.kernel_text, p.ldso_text) {
                Some(v) => v.name(),
                None => "none",
            };
            let mut s = String::from("./prog: textpair ");
            s.push_str(p.name);
            s.push_str(": got ");
            s.push_str(got);
            s.push_str(", want ");
            s.push_str(p.claim.name());
            s.push('\n');
            s
        }
    }
}

/// 差异描述（含首异字节与长度差）。
pub fn diff_text(p: &Pair) -> String {
    let mut s = String::from("./prog: textpair ");
    s.push_str(p.name);
    match byte_diff(p.kernel_text, p.ldso_text) {
        Some(i) => {
            s.push_str(": first diff at ");
            s.push_str(&u32_text(i as u32));
            s.push_str(", delta=");
            s.push_str(&i64_text(len_delta(p.kernel_text, p.ldso_text)));
            s.push('\n');
        }
        None => {
            s.push_str(": identical\n");
        }
    }
    s
}

/// 短名。
pub fn short(f: Fault) -> &'static str {
    f.name()
}

// ---------------------------------------------------------------------------
// 校验器（单次）
// ---------------------------------------------------------------------------

/// 单次校验器。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    used: bool,
    last: u32,
    steps: u32,
}

impl Checker {
    /// 新建。
    pub const fn new() -> Checker {
        Checker { used: false, last: 0, steps: 0 }
    }

    /// 是否已用。
    pub const fn bound(&self) -> bool {
        self.used
    }

    /// 步数。
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// 末码。
    pub const fn last(&self) -> u32 {
        self.last
    }

    /// 判一槽。
    pub fn check(&mut self, p: &Pair) -> Result<Verdict, u32> {
        if self.used {
            self.last = MT_ESTATE;
            return Err(MT_ESTATE);
        }
        self.used = true;
        self.steps = 1;
        match judge_verdict(p) {
            Ok(v) => {
                self.last = 0;
                Ok(v)
            }
            Err(c) => {
                self.last = c;
                Err(c)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u32,
    /// 操作码。
    pub op: u32,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 数值。
    pub value: u32,
}

impl Sample {
    /// 空格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 新账。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MT_ESEAL);
        }
        if self.len >= SAMPLES {
            return Err(MT_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
            self.len += 1;
            Ok(())
        } else {
            Err(MT_ESLOT)
        }
    }

    /// 格数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
    pub fn sample(&self, i: usize) -> Option<&Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i)
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
            return Err(MT_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 2, 2, 2, 929];
        const CODES: [u32; 6] = [
            MT_EBYTE, MT_EBYTE, MT_EKERNEL, MT_ELDSO, MT_ECONF, MT_ETEXT,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MT_EBAL),
            };
            if s.slot as usize != i {
                return Err(MT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MT_EVALUE);
                        }
                    }
                    None => return Err(MT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MT_EVALUE);
                        }
                    }
                    None => return Err(MT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MT_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MT_ESEAL);
        }
        if self.len != SAMPLES {
            return Err(MT_EBAL);
        }
        self.audit()?;
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&mut self) -> Result<u64, u32> {
        if !self.frozen {
            return Err(MT_ESTATE);
        }
        let again = self.seal();
        if again != self.root {
            return Err(MT_ESEAL);
        }
        Ok(self.root)
    }

    /// 根。
    pub const fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒收次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 计数格。
fn count_cell(slot: u32, op: u32, v: Result<usize, u32>) -> Sample {
    match v {
        Ok(n) => Sample { slot, op, ok: true, code: 0, value: n as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 错误格。
fn err_cell(slot: u32, op: u32, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 造账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_REPORT, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_REPORT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REPORT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_COMPARE, r.map(|x| x.agree as usize)));

    let _ = l.register(count_cell(4, OP_COMPARE, r.map(|x| x.kernel as usize)));
    let _ = l.register(count_cell(5, OP_COMPARE, r.map(|x| x.ldso as usize)));
    let _ = l.register(count_cell(6, OP_COMPARE, r.map(|x| x.conflict as usize)));

    let red = run_with(Some((0, MT_EVER)));
    let _ = l.register(count_cell(7, OP_TEXT, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_JUDGE, PAIRS[8].expected()));
    let _ = l.register(err_cell(9, OP_JUDGE, PAIRS[9].expected()));
    let _ = l.register(err_cell(10, OP_JUDGE, PAIRS[10].expected()));
    let _ = l.register(err_cell(11, OP_JUDGE, PAIRS[11].expected()));
    let _ = l.register(err_cell(12, OP_JUDGE, PAIRS[12].expected()));
    let _ = l.register(err_cell(13, OP_JUDGE, PAIRS[13].expected()));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名称与段基址。
fn name_ok() -> bool {
    let fnames = [
        Fault::Ok.name(),
        Fault::Byte.name(),
        Fault::Kernel.name(),
        Fault::Ldso.name(),
        Fault::Conf.name(),
        Fault::Text.name(),
        Fault::Pair.name(),
    ];
    let mut i = 0usize;
    while i < fnames.len() {
        if fnames[i].is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < fnames.len() {
            if fnames[i] == fnames[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    let vnames = [
        Verdict::Agree.name(),
        Verdict::KernelOnly.name(),
        Verdict::LdsoOnly.name(),
        Verdict::Conflict.name(),
    ];
    let mut k = 0usize;
    while k < vnames.len() {
        if vnames[k].is_empty() {
            return false;
        }
        let mut m = k + 1;
        while m < vnames.len() {
            if vnames[k] == vnames[m] {
                return false;
            }
            m += 1;
        }
        k += 1;
    }
    SEG_BASE == 0xfc10
        && MT_EBYTE == SEG_BASE + 0x01
        && MT_EKERNEL == SEG_BASE + 0x02
        && MT_ELDSO == SEG_BASE + 0x03
        && MT_ECONF == SEG_BASE + 0x04
        && MT_ETEXT == SEG_BASE + 0x05
        && MT_EPAIR == SEG_BASE + 0x06
        && MT_ESTATE == SEG_BASE + 0x07
        && MT_EVER == SEG_BASE + 0x08
        && MT_EBAL == SEG_BASE + 0x09
        && MT_ESLOT == SEG_BASE + 0x0a
        && MT_EVALUE == SEG_BASE + 0x0b
        && MT_ESEAL == SEG_BASE + 0x0c
        && MT_ENOBS == SEG_BASE + 0x0d
}

/// 码族与判面。
fn codes_ok() -> bool {
    let a = Verdict::Agree.index() == 0 && Verdict::Conflict.index() == 3;
    let b = Verdict::from_index(0) == Some(Verdict::Agree)
        && Verdict::from_index(1) == Some(Verdict::KernelOnly)
        && Verdict::from_index(2) == Some(Verdict::LdsoOnly)
        && Verdict::from_index(3) == Some(Verdict::Conflict)
        && Verdict::from_index(4).is_none();
    let c = Verdict::Agree.speaks_kernel()
        && Verdict::Agree.speaks_ldso()
        && Verdict::KernelOnly.speaks_kernel()
        && !Verdict::KernelOnly.speaks_ldso()
        && !Verdict::LdsoOnly.speaks_kernel()
        && Verdict::LdsoOnly.speaks_ldso();
    let d = Verdict::Agree.both_speak() && Verdict::Conflict.both_speak();
    let e = !Verdict::Agree.single() && Verdict::KernelOnly.single() && Verdict::LdsoOnly.single();
    let f = Fault::Ok.code() == 0 && Fault::Byte.code() == MT_EBYTE;
    let g = Fault::Kernel.code() == MT_EKERNEL
        && Fault::Ldso.code() == MT_ELDSO
        && Fault::Conf.code() == MT_ECONF
        && Fault::Text.code() == MT_ETEXT
        && Fault::Pair.code() == MT_EPAIR;
    let h = Fault::Ok.errno() == 0
        && Fault::Byte.errno() == ENOEXEC
        && Fault::Pair.errno() == ENOEXEC
        && !Fault::Ok.alive()
        && Fault::Text.alive();
    a && b && c && d && e && f && g && h
}

/// 双源比较。
fn compare_ok() -> bool {
    let a = compare("", "").is_none();
    let b = compare("x", "").as_ref() == Some(&Verdict::KernelOnly);
    let c = compare("", "y").as_ref() == Some(&Verdict::LdsoOnly);
    let d = compare("same", "same").as_ref() == Some(&Verdict::Agree);
    let e = compare("left", "right").as_ref() == Some(&Verdict::Conflict);
    let f = template_ok("") && template_ok("./prog: x\n");
    let g = !template_ok("x\n") && !template_ok("./prog: x") && !template_ok("./prog: a\0\n");
    let h = byte_diff("abc", "abd") == Some(2)
        && byte_diff("ab", "abc") == Some(2)
        && byte_diff("ab", "ab").is_none()
        && byte_diff("", "").is_none();
    let i = len_delta("ab", "abc") == 1 && len_delta("abc", "ab") == -1 && len_delta("x", "x") == 0;
    let j = first_line("./prog: a\nsecond") == "./prog: a\n"
        && first_line("./prog: a") == "./prog: a"
        && first_line("") == "";
    let k = line_count("") == 0
        && line_count("./prog: a\n") == 1
        && line_count("./prog: a\n./prog: b\n") == 2;
    a && b && c && d && e && f && g && h && i && j && k
}

/// 逐槽对判。
fn judge_ok() -> bool {
    let a = judge(&PAIRS[0]) == Fault::Ok;
    let b = judge(&PAIRS[8]) == Fault::Byte;
    let c = judge(&PAIRS[10]) == Fault::Kernel;
    let d = judge(&PAIRS[11]) == Fault::Ldso;
    let e = judge(&PAIRS[12]) == Fault::Conf;
    let f = judge(&PAIRS[13]) == Fault::Text;
    let g = Pair::new("silent", "", "", Verdict::Agree).judge() == Fault::Pair;
    let h = judge_verdict(&PAIRS[5]) == Ok(Verdict::LdsoOnly);
    let i = judge_verdict(&PAIRS[13]) == Err(MT_ETEXT);
    let j = PAIRS[0].expected() == 0 && PAIRS[13].expected() == MT_ETEXT;
    a && b && c && d && e && f && g && h && i && j
}

/// 语料互异。
fn unique() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        let a = match PAIRS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let mut j = i + 1;
        while j < PAIRS.len() {
            let b = match PAIRS.get(j) {
                Some(v) => v,
                None => return false,
            };
            if a.name == b.name {
                return false;
            }
            if a.kernel_text == b.kernel_text
                && a.ldso_text == b.ldso_text
                && a.claim == b.claim
            {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注齐全。
fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        match PAIRS.get(i) {
            Some(p) => {
                if p.name.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽判面表。
fn table_ok() -> bool {
    let mut i = 0usize;
    while i < PAIRS.len() {
        let p = match PAIRS.get(i) {
            Some(v) => v,
            None => return false,
        };
        let want = match EXPECTED.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if p.judge() != want {
            return false;
        }
        if p.judge() == Fault::Ok {
            match p.actual() {
                Some(v) => {
                    if v != p.claim {
                        return false;
                    }
                }
                None => return false,
            }
        }
        i += 1;
    }
    true
}

/// 汇总。
fn corpus_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.total == 14 && r.accept == 8 && r.reject == 6 && r.matched == 14;
    let b = r.agree == 2 && r.kernel == 2 && r.ldso == 2 && r.conflict == 2;
    let c = r.rate() == 1000 && r.green() && r.gate() == Ok(());
    let d = table_ok() && unique() && notes_ok();
    a && b && c && d
}

/// 渲染。
fn render_ok() -> bool {
    let r = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    r.render().as_str()
        == "textpair corpus=14 accept=8 reject=6 agree=2 kernel=2 ldso=2 conflict=2 match=14 rate=1000ppm\n"
}

/// 红档。
fn reds_ok() -> bool {
    let r = match run_with(Some((0, MT_EVER))) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.matched == CORPUS as u32 - 1 && r.rate() == 929;
    let b = r.gate() == Err(MT_EVER) && !r.green();
    let c = run_with(Some((CORPUS, MT_EVER))).err() == Some(MT_ESLOT);
    let d = run_with(Some((CORPUS - 1, MT_EVER))) == Ok(Report {
        total: 14,
        accept: 8,
        reject: 6,
        agree: 2,
        kernel: 2,
        ldso: 2,
        conflict: 2,
        matched: 13,
    });
    a && b && c && d
}

/// 文案。
fn message_ok() -> bool {
    let a = message(&PAIRS[0]).as_str() == "ok: magic-agree agree\n";
    let b = message(&PAIRS[2]).as_str() == "ok: interp-kernel kernel-only\n";
    let c = message(&PAIRS[5]).as_str() == "ok: reloc-ldso ldso-only\n";
    let d = message(&PAIRS[6]).as_str() == "ok: type-conflict conflict\n";
    let e = message(&PAIRS[10]).as_str()
        == "./prog: textpair phent-kernel: got conflict, want kernel-only\n";
    let f = message(&PAIRS[13]).as_str()
        == "./prog: textpair template-text: template violation\n";
    let g = message(&Pair::new("silent", "", "", Verdict::Agree)).as_str()
        == "./prog: textpair silent: both sources silent\n";
    let h = diff_text(&PAIRS[8]).as_str()
        == "./prog: textpair magic-byte: first diff at 36, delta=0\n";
    let i = diff_text(&PAIRS[0]).as_str() == "./prog: textpair magic-agree: identical\n";
    a && b && c && d && e && f && g && h && i
}

/// 槽号与覆盖。
fn slots_ok() -> bool {
    let a = match slot_at(0) {
        Some(p) => p.name == "magic-agree" && p.claim == Verdict::Agree,
        None => false,
    };
    let b = match slot_at(13) {
        Some(p) => p.name == "template-text" && p.expected() == MT_ETEXT,
        None => false,
    };
    let c = slot_at(CORPUS).is_none();
    let d = cover(CORPUS) == Ok(()) && cover(1) == Err(MT_ENOBS);
    let e = short(Fault::Byte) == "byte" && short(Fault::Ok) == "ok";
    let f = match slot_at(7) {
        Some(p) => p.name == "segment-conflict" && p.judge() == Fault::Ok,
        None => false,
    };
    a && b && c && d && e && f
}

/// 校验器。
fn checker_ok() -> bool {
    let mut c = Checker::new();
    let a = !c.bound() && c.steps() == 0;
    let b = c.check(&PAIRS[0]) == Ok(Verdict::Agree) && c.last() == 0;
    let d = c.check(&PAIRS[0]).err() == Some(MT_ESTATE);
    let mut w = Checker::new();
    let e = w.check(&PAIRS[8]).err() == Some(MT_EBYTE);
    let f = w.last() == MT_EBYTE && w.steps() == 1;
    a && b && d && e && f
}

/// 账本。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(()) && !l.is_empty() && l.sample(SAMPLES).is_none();
    let mut m = build_ledger();
    let c = match m.freeze() {
        Ok(r) => r != 0 && m.root() == r && m.is_frozen() && m.replay() == Ok(r),
        Err(_) => false,
    };
    let d = m.register(Sample::blank()).err() == Some(MT_ESEAL) && m.rejects() == 1;
    let e = m.freeze().err() == Some(MT_ESEAL) && m.rejects() == 2;
    let mut n = Ledger::new();
    let f = n.replay().err() == Some(MT_ESTATE);
    let g = n.register(Sample::blank()).is_ok() && n.len() == 1;
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && codes_ok();
    out[1] = compare_ok() && judge_ok() && table_ok();
    out[2] = corpus_ok() && render_ok() && reds_ok() && message_ok();
    out[3] = slots_ok() && checker_ok() && ledger_ok() && unique() && notes_ok();
    out
}

/// 判据集合。
pub fn run_unx11618_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11618_text_pair");

    s.add(
        "TP01-名称与段基址",
        {
            name_ok()
        },
        "static",
    );

    s.add(
        "TP02-码族与判面",
        {
            codes_ok()
        },
        "static",
    );

    s.add(
        "TP03-双源比较",
        {
            compare_ok()
        },
        "static",
    );

    s.add(
        "TP04-逐槽对判",
        {
            judge_ok()
        },
        "static",
    );

    s.add(
        "TP05-语料互异",
        {
            unique() && notes_ok()
        },
        "static",
    );

    s.add(
        "TP06-逐槽判面表",
        {
            table_ok()
        },
        "static",
    );

    s.add(
        "TP07-汇总与渲染",
        {
            corpus_ok() && render_ok()
        },
        "static",
    );

    s.add(
        "TP08-红档显式",
        {
            reds_ok()
        },
        "static",
    );

    s.add(
        "TP09-文案逐字",
        {
            message_ok()
        },
        "static",
    );

    s.add(
        "TP10-槽号与覆盖",
        {
            slots_ok()
        },
        "static",
    );

    s.add(
        "TP11-校验器与账本",
        {
            checker_ok() && ledger_ok()
        },
        "static",
    );

    s.add(
        "TP12-对账冻结",
        {
            let l = build_ledger();
            l.audit() == Ok(())
        },
        "static",
    );

    s.add(
        "TP13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let a = l.audit() == Ok(());
            let b = match l.freeze() {
                Ok(r) => r != 0 && l.is_frozen() && l.replay() == Ok(r),
                Err(_) => false,
            };
            let inj = inject_four();
            a && b && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unx11618 test fail")
    }

    #[test]
    fn verdict_names() {
        assert_eq!(Verdict::Agree.name(), "agree");
        assert_eq!(Verdict::KernelOnly.name(), "kernel-only");
        assert_eq!(Verdict::LdsoOnly.name(), "ldso-only");
        assert_eq!(Verdict::Conflict.name(), "conflict");
    }

    #[test]
    fn verdict_index_roundtrip() {
        assert_eq!(Verdict::Agree.index(), 0);
        assert_eq!(Verdict::Conflict.index(), 3);
        assert_eq!(Verdict::from_index(0), Some(Verdict::Agree));
        assert_eq!(Verdict::from_index(3), Some(Verdict::Conflict));
        assert_eq!(Verdict::from_index(4), None);
    }

    #[test]
    fn verdict_speaks() {
        assert!(Verdict::Agree.speaks_kernel() && Verdict::Agree.speaks_ldso());
        assert!(Verdict::KernelOnly.speaks_kernel() && !Verdict::KernelOnly.speaks_ldso());
        assert!(!Verdict::LdsoOnly.speaks_kernel() && Verdict::LdsoOnly.speaks_ldso());
        assert!(Verdict::Agree.both_speak() && !Verdict::Agree.single());
        assert!(Verdict::KernelOnly.single());
    }

    #[test]
    fn fault_codes() {
        assert_eq!(Fault::Ok.code(), 0);
        assert_eq!(Fault::Byte.code(), MT_EBYTE);
        assert_eq!(Fault::Kernel.code(), MT_EKERNEL);
        assert_eq!(Fault::Ldso.code(), MT_ELDSO);
        assert_eq!(Fault::Conf.code(), MT_ECONF);
        assert_eq!(Fault::Text.code(), MT_ETEXT);
        assert_eq!(Fault::Pair.code(), MT_EPAIR);
    }

    #[test]
    fn fault_errno() {
        assert_eq!(Fault::Ok.errno(), 0);
        assert_eq!(Fault::Byte.errno(), 8);
        assert_eq!(Fault::Text.errno(), 8);
        assert!(!Fault::Ok.alive());
        assert!(Fault::Conf.alive());
    }

    #[test]
    fn compare_table() {
        assert!(compare("", "").is_none());
        assert_eq!(compare("a", ""), Some(Verdict::KernelOnly));
        assert_eq!(compare("", "b"), Some(Verdict::LdsoOnly));
        assert_eq!(compare("ab", "ab"), Some(Verdict::Agree));
        assert_eq!(compare("ab", "ac"), Some(Verdict::Conflict));
    }

    #[test]
    fn template_rules() {
        assert!(template_ok(""));
        assert!(template_ok("./prog: x\n"));
        assert!(!template_ok("./prog: x"));
        assert!(!template_ok("x\n"));
        assert!(!template_ok("./prog: \n") == false);
        assert!(!template_ok("./prog: a\0\n"));
    }

    #[test]
    fn byte_diff_table() {
        assert_eq!(byte_diff("abc", "abd"), Some(2));
        assert_eq!(byte_diff("abc", "ab"), Some(2));
        assert_eq!(byte_diff("ab", "abc"), Some(2));
        assert_eq!(byte_diff("ab", "ab"), None);
        assert_eq!(byte_diff("", ""), None);
    }

    #[test]
    fn length_helpers() {
        assert_eq!(len_delta("ab", "abc"), 1);
        assert_eq!(len_delta("abc", "ab"), -1);
        assert_eq!(len_delta("x", "x"), 0);
        assert_eq!(line_count(""), 0);
        assert_eq!(line_count("./prog: a\n"), 1);
        assert_eq!(first_line("./prog: a\nb"), "./prog: a\n");
    }

    #[test]
    fn pair_actual() {
        assert_eq!(PAIRS[0].actual(), Some(Verdict::Agree));
        assert_eq!(PAIRS[2].actual(), Some(Verdict::KernelOnly));
        assert_eq!(PAIRS[4].actual(), Some(Verdict::LdsoOnly));
        assert_eq!(PAIRS[6].actual(), Some(Verdict::Conflict));
        assert!(Pair::new("s", "", "", Verdict::Agree).silent());
    }

    #[test]
    fn judge_accept_slots() {
        let mut i = 0usize;
        while i < 8 {
            assert_eq!(PAIRS[i].judge(), Fault::Ok);
            assert_eq!(PAIRS[i].expected(), 0);
            i += 1;
        }
    }

    #[test]
    fn judge_reject_slots() {
        assert_eq!(PAIRS[8].judge(), Fault::Byte);
        assert_eq!(PAIRS[9].judge(), Fault::Byte);
        assert_eq!(PAIRS[10].judge(), Fault::Kernel);
        assert_eq!(PAIRS[11].judge(), Fault::Ldso);
        assert_eq!(PAIRS[12].judge(), Fault::Conf);
        assert_eq!(PAIRS[13].judge(), Fault::Text);
    }

    #[test]
    fn silent_pair_slot() {
        let p = Pair::new("silent", "", "", Verdict::Agree);
        assert_eq!(p.judge(), Fault::Pair);
        assert_eq!(p.expected(), MT_EPAIR);
    }

    #[test]
    fn pairs_unique() {
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(PAIRS.len(), CORPUS);
    }

    #[test]
    fn pair_seal() {
        assert_eq!(PAIRS[0].seal(), PAIRS[0].seal());
        assert_ne!(PAIRS[0].seal(), PAIRS[1].seal());
        assert_ne!(PAIRS[0].seal(), PAIRS[8].seal());
    }

    #[test]
    fn report_counts() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.total, 14);
        assert_eq!(r.accept, 8);
        assert_eq!(r.reject, 6);
        assert_eq!(r.agree, 2);
        assert_eq!(r.kernel, 2);
        assert_eq!(r.ldso, 2);
        assert_eq!(r.conflict, 2);
        assert_eq!(r.matched, 14);
        assert_eq!(r.rate(), 1000);
        assert_eq!(r.gate(), Ok(()));
    }

    #[test]
    fn report_render_exact() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            r.render().as_str(),
            "textpair corpus=14 accept=8 reject=6 agree=2 kernel=2 ldso=2 conflict=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn report_balanced() {
        let r = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(r.balanced() && r.green());
        let mut bad = Report::new();
        bad.total = CORPUS as u32;
        bad.matched = CORPUS as u32;
        bad.accept = 8;
        bad.agree = 2;
        assert!(!bad.balanced());
        assert_eq!(bad.gate(), Err(MT_EBAL));
        assert_eq!(Report::new().rate(), 0);
        assert!(Report::new().is_empty());
    }

    #[test]
    fn red_override() {
        let r = match run_with(Some((0, MT_EVER))) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(r.matched, 13);
        assert_eq!(r.rate(), 929);
        assert_eq!(r.gate(), Err(MT_EVER));
        assert_eq!(run_with(Some((14, MT_EVER))).err(), Some(MT_ESLOT));
    }

    #[test]
    fn message_exact() {
        assert_eq!(message(&PAIRS[0]).as_str(), "ok: magic-agree agree\n");
        assert_eq!(message(&PAIRS[2]).as_str(), "ok: interp-kernel kernel-only\n");
        assert_eq!(message(&PAIRS[6]).as_str(), "ok: type-conflict conflict\n");
        assert_eq!(
            message(&PAIRS[10]).as_str(),
            "./prog: textpair phent-kernel: got conflict, want kernel-only\n"
        );
    }

    #[test]
    fn message_reject_exact() {
        assert_eq!(
            message(&PAIRS[13]).as_str(),
            "./prog: textpair template-text: template violation\n"
        );
        assert_eq!(
            message(&Pair::new("silent", "", "", Verdict::Agree)).as_str(),
            "./prog: textpair silent: both sources silent\n"
        );
        assert_eq!(
            diff_text(&PAIRS[8]).as_str(),
            "./prog: textpair magic-byte: first diff at 36, delta=0\n"
        );
    }

    #[test]
    fn checker_once() {
        let mut c = Checker::new();
        assert_eq!(c.check(&PAIRS[0]), Ok(Verdict::Agree));
        assert_eq!(c.check(&PAIRS[0]).err(), Some(MT_ESTATE));
        assert_eq!(c.steps(), 1);
    }

    #[test]
    fn checker_wrong() {
        let mut c = Checker::new();
        assert_eq!(c.check(&PAIRS[8]).err(), Some(MT_EBYTE));
        assert_eq!(c.last(), MT_EBYTE);
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(100).is_none());
        assert!(!l.is_empty());
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_ne!(r, 0);
        assert!(l.is_frozen());
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.register(Sample::blank()).err(), Some(MT_ESEAL));
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.freeze().err(), Some(MT_ESEAL));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0], "inject0");
        assert!(out[1], "inject1");
        assert!(out[2], "inject2");
        assert!(out[3], "inject3");
    }

    #[test]
    fn slot_bounds() {
        assert!(slot_at(0).is_some());
        assert!(slot_at(CORPUS - 1).is_some());
        assert!(slot_at(CORPUS).is_none());
        assert_eq!(short(Fault::Ok), "ok");
        assert_eq!(short(Fault::Conf), "conflict");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0).as_str(), "0");
        assert_eq!(u32_text(929).as_str(), "929");
        assert_eq!(hex_text(0xfc11).as_str(), "0xfc11");
        assert_eq!(i64_text(-9).as_str(), "-9");
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11618_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
