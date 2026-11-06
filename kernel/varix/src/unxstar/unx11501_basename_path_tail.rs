//! UNX-F11501 basename 路径末段提取（0xf060 段）
//!
//! `basename` 的字符串形态在 POSIX 里只有四条边界：空串的名字是 "."；全是斜杠
//! 的名字是 "/"；尾随斜杠先剥掉再找分隔符，所以 "a/b/" 取 "b"；名字里没有斜杠
//! 时整段就是名字。字符串形态**永不失败**，它总能把任意输入折成一个可打印名字。
//!
//! 本模块在字符串形态之外再立一份**记录形态** `Tail`：它不只给出名字，还记住
//! 名字在输入里的起止偏移、剥掉了几个尾随斜杠、以及这段名字是不是 "." / ".."。
//! 记录形态要求"真的有一个组件"，因此根路径（含纯斜杠串）返回 `EROOT`——这与
//! 字符串形态把根折成 "/" 并不矛盾：前者是给人看的名字，后者是给账目用的区间。
//!
//! 后缀剥离（`basename path suffix`）只剥一次，且遵守两条反直觉规则：
//! - 后缀与名字完全相等时不剥（"a.txt" 配后缀 "a.txt" 仍是 "a.txt"）；
//! - 后缀含 '/' 一律拒绝，含斜杠的"后缀"不是名字的一部分。
//!
//! 闸门次序（`extract`）：空路径 → 路径超容 → 后缀含斜杠 → 后缀超容 →
//! 无组件（根）。`strict` 另加两道语义闸门：点条目 → 隐藏名，次序不可交换
//! （"." 与 ".." 也以点开头，先判点条目才不会把语义错误归类成"隐藏"）。
//!
//! 账本十四格：八格登记成功路径（三次字符串取名、三次记录提取、一次深度计数、
//! 一次定容渲染），六格登记各类拒绝（空路径、根无组件、后缀含斜杠、路径超容、
//! 点条目、隐藏名）。记录形态的观测值编码为 `start * 100 + len`，一眼能读出
//! 偏移与长度；审计逐格复算，封印折叠全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf060;

/// 空路径没有组件可提。
pub const BN_EEMPTY: u32 = SEG_BASE + 0x01;
/// 路径超出容量。
pub const BN_ELONG: u32 = SEG_BASE + 0x02;
/// 后缀里含分隔符。
pub const BN_ESUFFIX: u32 = SEG_BASE + 0x03;
/// 后缀超出容量。
pub const BN_ESUFLEN: u32 = SEG_BASE + 0x04;
/// 根路径没有末段。
pub const BN_EROOT: u32 = SEG_BASE + 0x05;
/// 点条目不是具体名字。
pub const BN_EDOT: u32 = SEG_BASE + 0x06;
/// 输出缓冲不足。
pub const BN_EBUF: u32 = SEG_BASE + 0x07;
/// 隐藏名不是可见组件。
pub const BN_EHIDDEN: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const BN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const BN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const BN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const BN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const BN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 路径容量（含后缀）。
pub const CAP: usize = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：记录形态提取。
pub const OP_EXTRACT: u8 = 1;
/// 操作码：字符串形态取名。
pub const OP_NAME: u8 = 2;
/// 操作码：严格组件校验。
pub const OP_STRICT: u8 = 3;
/// 操作码：定容渲染。
pub const OP_RENDER: u8 = 4;
/// 操作码：前缀深度计数。
pub const OP_DEPTH: u8 = 5;

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

/// 剥尾随斜杠后的区间与剥掉的个数。
fn tail_bounds(p: &[u8]) -> (usize, u16) {
    let mut end = p.len();
    let mut trailing = 0u16;
    while end > 0 {
        match p.get(end - 1).copied() {
            Some(b'/') => {
                end -= 1;
                trailing = trailing.saturating_add(1);
            }
            _ => break,
        }
    }
    (end, trailing)
}

/// 在 `[0, end)` 里找末段的起点。
fn tail_start(p: &[u8], end: usize) -> usize {
    let mut start = end;
    while start > 0 {
        match p.get(start - 1).copied() {
            Some(b'/') => break,
            _ => start -= 1,
        }
    }
    start
}

/// `[start, start + len)` 是否为点条目。
fn is_dot(p: &[u8], start: usize, len: usize) -> bool {
    if len == 1 {
        return p.get(start).copied() == Some(b'.');
    }
    if len == 2 {
        return p.get(start).copied() == Some(b'.')
            && p.get(start + 1).copied() == Some(b'.');
    }
    false
}

/// 字符串形态：末段名字，永不失败。
pub fn basename(p: &[u8]) -> &[u8] {
    if p.is_empty() {
        return b".";
    }
    let (end, _) = tail_bounds(p);
    if end == 0 {
        return b"/";
    }
    let start = tail_start(p, end);
    match p.get(start..end) {
        Some(s) => s,
        None => b".",
    }
}

/// 字符串形态：剥一次后缀。
pub fn basename_suffix<'a>(p: &'a [u8], suf: &[u8]) -> &'a [u8] {
    let base = basename(p);
    if suf.is_empty() || base.len() <= suf.len() {
        return base;
    }
    if base.ends_with(suf) {
        match base.get(..base.len() - suf.len()) {
            Some(s) => s,
            None => base,
        }
    } else {
        base
    }
}

/// 记录形态：名字区间 + 尾随斜杠数 + 点条目标志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tail {
    /// 名字在输入里的起点。
    pub start: u16,
    /// 名字（剥后缀后）的长度。
    pub len: u16,
    /// 剥掉的尾随斜杠数。
    pub trailing: u16,
    /// 名字是否为 "." 或 ".."。
    pub dot: bool,
}

impl Tail {
    /// 观测值编码：`start * 100 + len`。
    pub fn word(&self) -> u32 {
        (self.start as u32) * 100 + self.len as u32
    }

    /// 名字终点（不含）。
    pub fn end(&self) -> u32 {
        self.start as u32 + self.len as u32
    }
}

/// 记录形态提取。
pub fn extract(p: &[u8], suf: &[u8]) -> Result<Tail, u32> {
    if p.is_empty() {
        return Err(BN_EEMPTY);
    }
    if p.len() > CAP {
        return Err(BN_ELONG);
    }
    let mut k = 0usize;
    while k < suf.len() {
        match suf.get(k).copied() {
            Some(b'/') => return Err(BN_ESUFFIX),
            _ => {}
        }
        k += 1;
    }
    if suf.len() > CAP {
        return Err(BN_ESUFLEN);
    }
    let (end, trailing) = tail_bounds(p);
    if end == 0 {
        return Err(BN_EROOT);
    }
    let start = tail_start(p, end);
    let mut len = end - start;
    if !suf.is_empty() && len > suf.len() {
        if let Some(name) = p.get(start..end) {
            if name.ends_with(suf) {
                len -= suf.len();
            }
        }
    }
    Ok(Tail {
        start: start as u16,
        len: len as u16,
        trailing,
        dot: is_dot(p, start, len),
    })
}

/// 严格组件：拒绝根、点条目与隐藏名。
pub fn strict(p: &[u8]) -> Result<&[u8], u32> {
    if p.is_empty() {
        return Err(BN_EEMPTY);
    }
    if p.len() > CAP {
        return Err(BN_ELONG);
    }
    let base = basename(p);
    if base == b"/" {
        return Err(BN_EROOT);
    }
    if base == b"." || base == b".." {
        return Err(BN_EDOT);
    }
    if base.len() > 1 && base.first().copied() == Some(b'.') {
        return Err(BN_EHIDDEN);
    }
    Ok(base)
}

/// 前缀深度：名字之前的分隔符个数。
pub fn depth(p: &[u8]) -> Result<u32, u32> {
    if p.is_empty() {
        return Err(BN_EEMPTY);
    }
    if p.len() > CAP {
        return Err(BN_ELONG);
    }
    let (end, _) = tail_bounds(p);
    if end == 0 {
        return Err(BN_EROOT);
    }
    let start = tail_start(p, end);
    let mut n = 0u32;
    let mut i = 0usize;
    while i < start {
        if p.get(i).copied() == Some(b'/') {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    Ok(n)
}

/// 定容渲染末段名字。
pub fn render(p: &[u8], dst: &mut [u8]) -> Result<u32, u32> {
    let base = basename(p);
    if dst.len() < base.len() {
        return Err(BN_EBUF);
    }
    let mut i = 0usize;
    while i < base.len() {
        if let (Some(d), Some(s)) = (dst.get_mut(i), base.get(i)) {
            *d = *s;
        }
        i += 1;
    }
    Ok(base.len() as u32)
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

/// `Result<Tail, u32>` → 样本。
fn tail_cell(slot: u8, op: u8, r: Result<Tail, u32>) -> Sample {
    match r {
        Ok(t) => Sample { slot, op, ok: true, code: 0, value: t.word() },
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
            return Err(BN_ESLOT);
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
            return Err(BN_EBAL);
        }
        const VALS: [u32; 8] = [3, 506, 205, 1, 1, 403, 2, 5];
        const CODES: [u32; 6] =
            [BN_EEMPTY, BN_EROOT, BN_ESUFFIX, BN_ELONG, BN_EDOT, BN_EHIDDEN];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BN_EBAL),
            };
            if s.slot as usize != i {
                return Err(BN_ESLOT);
            }
            if s.op == 0 {
                return Err(BN_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BN_EVALUE);
                        }
                    }
                    None => return Err(BN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(BN_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BN_EVALUE);
                        }
                    }
                    None => return Err(BN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(BN_EBAL);
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
            return Err(BN_EBAL);
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
            Err(BN_ESEAL)
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

/// 十四格账本：三次取名 → 三次提取 → 一次深度 → 一次渲染 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let over = [b'a'; CAP + 16];

    let _ = l.register(word_cell(0, OP_NAME, Ok(basename(b"usr/bin/awk").len() as u32)));
    let _ = l.register(tail_cell(1, OP_EXTRACT, extract(b"/etc/passwd", b".conf")));
    let _ = l.register(tail_cell(2, OP_EXTRACT, extract(b"x/y.tar.gz", b".gz")));
    let _ = l.register(word_cell(3, OP_NAME, Ok(basename(b"a//b///").len() as u32)));
    let _ = l.register(word_cell(4, OP_NAME, Ok(basename(b"///").len() as u32)));
    let _ = l.register(tail_cell(5, OP_EXTRACT, extract(b"dir/sub/", b"")));
    let _ = l.register(word_cell(6, OP_DEPTH, depth(b"usr/bin/awk")));

    let mut buf = [0u8; 8];
    let _ = l.register(word_cell(7, OP_RENDER, render(b"/etc/hosts", &mut buf)));

    let _ = l.register(tail_cell(8, OP_EXTRACT, extract(b"", b"")));
    let _ = l.register(tail_cell(9, OP_EXTRACT, extract(b"/", b"")));
    let _ = l.register(tail_cell(10, OP_EXTRACT, extract(b"a/b", b"b/c")));
    let _ = l.register(tail_cell(11, OP_EXTRACT, extract(&over, b"")));
    let _ = l.register(word_cell(12, OP_STRICT, strict(b"a/..").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(13, OP_STRICT, strict(b"d/.git").map(|v| v.len() as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：字符串形态的四条边界。
    out[1] = basename(b"") == b"."
        && basename(b"///") == b"/"
        && basename(b"//") == b"/"
        && basename(b"a/b/") == b"b"
        && basename(b"a//b///") == b"b"
        && basename(b"x") == b"x"
        && basename_suffix(b"y.tar.gz", b".gz") == b"y.tar"
        && basename_suffix(b"a.txt", b"a.txt") == b"a.txt"
        && basename_suffix(b"a.txt", b"xt") == b"a.t";

    // 二路：记录形态的偏移、尾随与点标志。
    let ok = match (extract(b"a//b///", b""), extract(b"a/.", b""), extract(b"a/..", b"")) {
        (Ok(t1), Ok(t2), Ok(t3)) => {
            t1.start == 3
                && t1.len == 1
                && t1.trailing == 3
                && !t1.dot
                && t1.word() == 301
                && t1.end() == 4
                && t2.dot
                && t3.dot
                && t3.len == 2
        }
        _ => false,
    };
    out[2] = ok
        && depth(b"usr/bin/awk") == Ok(2)
        && depth(b"a") == Ok(0)
        && extract(b"/etc/passwd", b"passwd") == Ok(Tail { start: 5, len: 6, trailing: 0, dot: false })
        && extract(b"dir/sub//", b"").map(|t| (t.start, t.len, t.trailing)) == Ok((4, 3, 2));

    // 三路：八类失败各自可辨。
    let over = [b'a'; CAP + 16];
    let long_suf = [b'x'; CAP + 4];
    let mut small = [0u8; 2];
    out[3] = extract(b"", b"") == Err(BN_EEMPTY)
        && extract(&over, b"") == Err(BN_ELONG)
        && extract(b"a/b", b"x/y") == Err(BN_ESUFFIX)
        && extract(b"a/b", &long_suf) == Err(BN_ESUFLEN)
        && extract(b"///", b"") == Err(BN_EROOT)
        && strict(b".") == Err(BN_EDOT)
        && strict(b"d/.git") == Err(BN_EHIDDEN)
        && strict(b"//") == Err(BN_EROOT)
        && render(b"abc", &mut small) == Err(BN_EBUF)
        && render(b"abc", &mut [0u8; 3]) == Ok(3);

    out
}

/// 十三项判据。
pub fn run_unx11501_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11501.basename_tail");
    let l = build_ledger();

    s.add_dyn(
        "BN01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "BN02-末段取尾剥斜杠",
        {
            let t = extract(b"a//b///", b"");
            t == Ok(Tail { start: 3, len: 1, trailing: 3, dot: false })
                && basename(b"a//b///") == b"b"
                && l.sample(3).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "BN03-全斜杠归一",
        basename(b"///") == b"/"
            && basename(b"//") == b"/"
            && basename(b"") == b"."
            && basename(b"a//") == b"a"
            && l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "BN04-后缀只剥一次",
        basename_suffix(b"y.tar.gz", b".gz") == b"y.tar"
            && basename_suffix(b"a.txt", b".txt") == b"a"
            && basename_suffix(b"a.txt", b"a.txt") == b"a.txt"
            && basename_suffix(b"a.txt", b"xt") == b"a.t"
            && basename_suffix(b"a.txt", b"") == b"a.txt",
        "static",
    );

    s.add(
        "BN05-根无末段可取",
        extract(b"/", b"") == Err(BN_EROOT)
            && extract(b"///", b"") == Err(BN_EROOT)
            && basename(b"/") == b"/"
            && l.sample(9).map(|x| x.code) == Some(BN_EROOT),
        "static",
    );

    s.add(
        "BN06-后缀含斜杠被拒",
        extract(b"a/b", b"b/c") == Err(BN_ESUFFIX)
            && extract(b"a/b", b"/") == Err(BN_ESUFFIX)
            && l.sample(10).map(|x| x.code) == Some(BN_ESUFFIX),
        "static",
    );

    s.add(
        "BN07-路径容量边界",
        {
            let exact = [b'z'; CAP];
            let over = [b'z'; CAP + 1];
            extract(&exact, b"").map(|t| t.len as usize) == Ok(CAP)
                && extract(&over, b"") == Err(BN_ELONG)
                && l.sample(11).map(|x| x.code) == Some(BN_ELONG)
        },
        "static",
    );

    s.add(
        "BN08-点条目被拒",
        strict(b".") == Err(BN_EDOT)
            && strict(b"a/..") == Err(BN_EDOT)
            && strict(b"./..") == Err(BN_EDOT)
            && extract(b"a/.", b"").map(|t| t.dot) == Ok(true)
            && l.sample(12).map(|x| x.code) == Some(BN_EDOT),
        "static",
    );

    s.add(
        "BN09-隐藏名被拒",
        strict(b"d/.git") == Err(BN_EHIDDEN)
            && strict(b"d/.a") == Err(BN_EHIDDEN)
            && strict(b"d/a.") == Ok(b"a.")
            && l.sample(13).map(|x| x.code) == Some(BN_EHIDDEN),
        "static",
    );

    s.add(
        "BN10-输出缓冲边界",
        {
            let mut exact = [0u8; 3];
            let mut small = [0u8; 2];
            render(b"abc", &mut exact) == Ok(3)
                && exact == [b'a', b'b', b'c']
                && render(b"abc", &mut small) == Err(BN_EBUF)
                && render(b"", &mut small) == Ok(1)
        },
        "static",
    );

    s.add(
        "BN11-尾随斜杠记账",
        {
            let t = extract(b"dir/sub//", b"");
            t == Ok(Tail { start: 4, len: 3, trailing: 2, dot: false })
                && extract(b"dir/sub/", b"").map(|x| x.trailing) == Ok(1)
                && extract(b"dir/sub", b"").map(|x| x.trailing) == Ok(0)
                && l.sample(5).map(|x| x.value) == Some(403)
        },
        "static",
    );

    s.add(
        "BN12-前缀深度计数",
        depth(b"a") == Ok(0)
            && depth(b"/a") == Ok(1)
            && depth(b"usr/bin/awk") == Ok(2)
            && depth(b"/") == Err(BN_EROOT)
            && depth(b"") == Err(BN_EEMPTY)
            && l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "BN13-冻结重放四注入",
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
        let s = run_unx11501_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn string_edges() {
        assert_eq!(basename(b""), b".");
        assert_eq!(basename(b"/"), b"/");
        assert_eq!(basename(b"///"), b"/");
        assert_eq!(basename(b"a/b/"), b"b");
        assert_eq!(basename(b"abc"), b"abc");
        assert_eq!(basename_suffix(b"x/y.tar.gz", b".gz"), b"y.tar");
        assert_eq!(basename_suffix(b"a.txt", b"a.txt"), b"a.txt");
    }

    #[test]
    fn suffix_rules() {
        assert_eq!(basename_suffix(b"a.txt", b".txt"), b"a");
        assert_eq!(basename_suffix(b"a.txt", b"xt"), b"a.t");
        assert_eq!(basename_suffix(b"a.txt", b"a.tx"), b"a.txt");
        assert_eq!(basename_suffix(b"a.txt", b"a.txt.bak"), b"a.txt");
    }

    #[test]
    fn record_fields() {
        let t = extract(b"dir/sub//", b"").expect("tail");
        assert_eq!(t.start, 4);
        assert_eq!(t.len, 3);
        assert_eq!(t.trailing, 2);
        assert_eq!(t.word(), 403);
        assert!(!t.dot);
        let d = extract(b"/a/./", b"").expect("tail");
        assert_eq!(d.start, 3);
        assert!(d.dot);
    }

    #[test]
    fn strict_gates() {
        assert_eq!(strict(b""), Err(BN_EEMPTY));
        assert_eq!(strict(b"/"), Err(BN_EROOT));
        assert_eq!(strict(b"."), Err(BN_EDOT));
        assert_eq!(strict(b".."), Err(BN_EDOT));
        assert_eq!(strict(b"d/.git"), Err(BN_EHIDDEN));
        assert_eq!(strict(b"d/a."), Ok(&b"a."[..]));
    }

    #[test]
    fn buffer_and_limits() {
        let mut small = [0u8; 2];
        assert_eq!(render(b"abc", &mut small), Err(BN_EBUF));
        let mut exact = [0u8; 3];
        assert_eq!(render(b"abc", &mut exact), Ok(3));
        assert_eq!(exact, [b'a', b'b', b'c']);
        let over = [b'q'; CAP + 1];
        assert_eq!(extract(&over, b""), Err(BN_ELONG));
        assert_eq!(depth(&over), Err(BN_ELONG));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
