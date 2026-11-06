//! UNX-F11502 dirname 目录前缀提取（0xf070 段）
//!
//! `dirname` 的字符串形态有四条边界：空串的目录是 "."；没有斜杠的相对名字目录
//! 也是 "."；根与纯斜杠串的目录仍是根；剥掉尾段之后剩下的前缀要再剥一次尾随
//! 斜杠，所以 "/a/b/" 的目录是 "/a" 而不是 "/a/"。另有一条实现相关的特例：输入
//! 恰好以两个斜杠开头、且前缀被剥空时，结果保留为 "//"（"//x" 的目录是 "//"），
//! 三个以上斜杠则折叠成单个根 "/"。
//!
//! 本模块在字符串形态之外再立一份**区间形态** `Head`：给出目录前缀在输入里的
//! 起点与长度、该前缀的组件深度、以及它是不是根。区间形态要求路径**绝对**
//! （相对路径的目录前缀是编译期常量 "."，没有可记账的区间），并要求路径里不含
//! ".." 组件——上溯必须走 `up`，不能让 ".." 混在区间里让深度账失真。
//!
//! 上溯（`up path n`）是深度账的核心：n 不得超过路径深度，越过根返回 `EUP`；
//! 恰好走到根时返回根区间并置 `root` 标志，深度归零。组件深度有上限
//! （16 段），超限返回 `EDEPTH`——深度不是随便多大的数，它是账本里的一列。
//!
//! 闸门次序（`head`/`up`/`depth` 同一条）：空路径 → 超容 → 非绝对 → 含 ".."
//! → 深度超限 → 越过根（仅 `up`）。`parent` 在深度为零时另报 `EROOT`：根本身
//! 没有父目录，这个错误与"上溯越界"分开记。
//!
//! 账本十四格：八格登记成功路径（头部区间、根、相对目录、字符串形态、两次上溯、
//! 深度计数、定容渲染），六格登记各类拒绝（相对、越根、点点点、空、缓冲不足、
//! 深度超限）。`Head` 的观测值编码为 `depth * 100 + len`；审计逐格复算，封印
//! 折叠全账，冻结后拒绝登记并计数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf070;

/// 空路径没有目录前缀。
pub const DN_EEMPTY: u32 = SEG_BASE + 0x01;
/// 路径超出容量。
pub const DN_ELONG: u32 = SEG_BASE + 0x02;
/// 区间形态要求绝对路径。
pub const DN_EREL: u32 = SEG_BASE + 0x03;
/// 路径含 ".." 组件。
pub const DN_EDOTDOT: u32 = SEG_BASE + 0x04;
/// 上溯越过根。
pub const DN_EUP: u32 = SEG_BASE + 0x05;
/// 输出缓冲不足。
pub const DN_EBUF: u32 = SEG_BASE + 0x06;
/// 组件深度超限。
pub const DN_EDEPTH: u32 = SEG_BASE + 0x07;
/// 根没有父目录。
pub const DN_EROOT: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DN_ENOBS: u32 = SEG_BASE + 0x0d;

/// 路径容量。
pub const CAP: usize = 64;
/// 组件深度上限。
pub const DEPTH_MAX: u32 = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：头部区间。
pub const OP_HEAD: u8 = 1;
/// 操作码：字符串形态。
pub const OP_DIR: u8 = 2;
/// 操作码：上溯。
pub const OP_UP: u8 = 3;
/// 操作码：深度计数。
pub const OP_DEPTH: u8 = 4;
/// 操作码：定容渲染。
pub const OP_RENDER: u8 = 5;

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

/// 剥尾随斜杠后的终点与剥掉的个数。
fn strip_tail(p: &[u8]) -> (usize, usize) {
    let mut end = p.len();
    let mut n = 0usize;
    while end > 0 && p.get(end - 1).copied() == Some(b'/') {
        end -= 1;
        n += 1;
    }
    (end, n)
}

/// 纯斜杠输入的根文本。
fn root_text(p: &[u8]) -> &[u8] {
    if p.len() == 2 {
        b"//"
    } else {
        b"/"
    }
}

/// `p[..end)` 里的非空组件个数。
fn count_in(p: &[u8], end: usize) -> u32 {
    let mut n = 0u32;
    let mut seg = 0usize;
    let mut i = 0usize;
    while i <= end {
        if i == end || p.get(i).copied() == Some(b'/') {
            if i > seg {
                n = n.saturating_add(1);
            }
            seg = i + 1;
        }
        i += 1;
    }
    n
}

/// 路径里是否含 ".." 组件。
fn has_dotdot(p: &[u8]) -> bool {
    let mut seg = 0usize;
    let mut i = 0usize;
    while i <= p.len() {
        if i == p.len() || p.get(i).copied() == Some(b'/') {
            if i == seg + 2
                && p.get(seg).copied() == Some(b'.')
                && p.get(seg + 1).copied() == Some(b'.')
            {
                return true;
            }
            seg = i + 1;
        }
        i += 1;
    }
    false
}

/// 区间形态的公共闸门。
fn validate(p: &[u8]) -> Result<(), u32> {
    if p.is_empty() {
        return Err(DN_EEMPTY);
    }
    if p.len() > CAP {
        return Err(DN_ELONG);
    }
    if p.first().copied() != Some(b'/') {
        return Err(DN_EREL);
    }
    if has_dotdot(p) {
        return Err(DN_EDOTDOT);
    }
    Ok(())
}

/// 目录前缀区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    /// 前缀在输入里的起点。
    pub start: u16,
    /// 前缀长度。
    pub len: u16,
    /// 前缀的组件深度。
    pub depth: u16,
    /// 是否为根。
    pub root: bool,
}

impl Head {
    /// 观测值编码：`depth * 100 + len`。
    pub fn word(&self) -> u32 {
        (self.depth as u32) * 100 + self.len as u32
    }

    /// 取回前缀切片。
    pub fn text<'a>(&self, p: &'a [u8]) -> Option<&'a [u8]> {
        let start = self.start as usize;
        let end = start + self.len as usize;
        p.get(start..end)
    }
}

/// 字符串形态：目录前缀，永不失败。
pub fn dirname(p: &[u8]) -> &[u8] {
    if p.is_empty() {
        return b".";
    }
    let (end, _) = strip_tail(p);
    if end == 0 {
        return root_text(p);
    }
    let mut s = end;
    while s > 0 && p.get(s - 1).copied() != Some(b'/') {
        s -= 1;
    }
    if s == 0 {
        return b".";
    }
    let mut e = s - 1;
    while e > 0 && p.get(e - 1).copied() == Some(b'/') {
        e -= 1;
    }
    if e == 0 {
        if s == 2 && p.len() >= 2 {
            return b"//";
        }
        return b"/";
    }
    match p.get(..e) {
        Some(v) => v,
        None => b".",
    }
}

/// 区间形态：目录前缀。
pub fn head(p: &[u8]) -> Result<Head, u32> {
    validate(p)?;
    let (end, _) = strip_tail(p);
    if end == 0 {
        let len = if p.len() == 2 { 2 } else { 1 };
        return Ok(Head { start: 0, len, depth: 0, root: true });
    }
    let mut s = end;
    while s > 0 && p.get(s - 1).copied() != Some(b'/') {
        s -= 1;
    }
    let mut e = if s > 0 { s - 1 } else { 0 };
    while e > 0 && p.get(e - 1).copied() == Some(b'/') {
        e -= 1;
    }
    if e == 0 {
        let doubled = s == 2 && p.get(0).copied() == Some(b'/') && p.get(1).copied() == Some(b'/');
        let len = if doubled { 2 } else { 1 };
        return Ok(Head { start: 0, len, depth: 0, root: true });
    }
    let depth = count_in(p, e);
    if depth > DEPTH_MAX {
        return Err(DN_EDEPTH);
    }
    Ok(Head { start: 0, len: e as u16, depth: depth as u16, root: false })
}

/// 组件深度。
pub fn depth(p: &[u8]) -> Result<u32, u32> {
    validate(p)?;
    let (end, _) = strip_tail(p);
    let n = count_in(p, end);
    if n > DEPTH_MAX {
        return Err(DN_EDEPTH);
    }
    Ok(n)
}

/// 上溯 n 级。
pub fn up(p: &[u8], n: u32) -> Result<Head, u32> {
    validate(p)?;
    let d = depth(p)?;
    if n > d {
        return Err(DN_EUP);
    }
    let (end, _) = strip_tail(p);
    let mut pos = end;
    let mut k = 0u32;
    while k < n {
        while pos > 0 && p.get(pos - 1).copied() != Some(b'/') {
            pos -= 1;
        }
        while pos > 0 && p.get(pos - 1).copied() == Some(b'/') {
            pos -= 1;
        }
        k += 1;
    }
    if pos == 0 {
        let doubled = p.len() == 2
            && p.get(0).copied() == Some(b'/')
            && p.get(1).copied() == Some(b'/');
        let len = if doubled { 2 } else { 1 };
        return Ok(Head { start: 0, len, depth: 0, root: true });
    }
    Ok(Head { start: 0, len: pos as u16, depth: (d - n) as u16, root: false })
}

/// 父目录；根没有父目录。
pub fn parent(p: &[u8]) -> Result<Head, u32> {
    validate(p)?;
    if depth(p)? == 0 {
        return Err(DN_EROOT);
    }
    up(p, 1)
}

/// 定容渲染字符串形态。
pub fn render(p: &[u8], dst: &mut [u8]) -> Result<u32, u32> {
    let d = dirname(p);
    if dst.len() < d.len() {
        return Err(DN_EBUF);
    }
    let mut i = 0usize;
    while i < d.len() {
        if let (Some(slot), Some(src)) = (dst.get_mut(i), d.get(i)) {
            *slot = *src;
        }
        i += 1;
    }
    Ok(d.len() as u32)
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

/// `Result<Head, u32>` → 样本。
fn head_cell(slot: u8, op: u8, r: Result<Head, u32>) -> Sample {
    match r {
        Ok(h) => Sample { slot, op, ok: true, code: 0, value: h.word() },
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
            return Err(DN_ESLOT);
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
            return Err(DN_EBAL);
        }
        const VALS: [u32; 8] = [208, 1, 1, 8, 204, 1, 4, 4];
        const CODES: [u32; 6] =
            [DN_EREL, DN_EUP, DN_EDOTDOT, DN_EEMPTY, DN_EBUF, DN_EDEPTH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DN_EBAL),
            };
            if s.slot as usize != i {
                return Err(DN_ESLOT);
            }
            if s.op == 0 {
                return Err(DN_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DN_EVALUE);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(DN_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DN_EVALUE);
                        }
                    }
                    None => return Err(DN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DN_EBAL);
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
            return Err(DN_EBAL);
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
            Err(DN_ESEAL)
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

/// 十四格账本：头部 → 根 → 字符串形态 → 上溯 → 深度 → 渲染 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(head_cell(0, OP_HEAD, head(b"/usr/bin/awk")));
    let _ = l.register(head_cell(1, OP_HEAD, head(b"/")));
    let _ = l.register(word_cell(2, OP_DIR, Ok(dirname(b"a").len() as u32)));
    let _ = l.register(word_cell(3, OP_DIR, Ok(dirname(b"/usr/bin/awk").len() as u32)));
    let _ = l.register(head_cell(4, OP_UP, up(b"/a/b/c", 1)));
    let _ = l.register(head_cell(5, OP_UP, up(b"/a/b/c", 3)));
    let _ = l.register(word_cell(6, OP_DEPTH, depth(b"/a/b/c/d")));

    let mut buf = [0u8; 8];
    let _ = l.register(word_cell(7, OP_RENDER, render(b"/a/b/c", &mut buf)));

    let _ = l.register(head_cell(8, OP_UP, up(b"a/b", 1)));
    let _ = l.register(head_cell(9, OP_UP, up(b"/a", 9)));
    let _ = l.register(head_cell(10, OP_UP, up(b"/a/../b", 1)));
    let _ = l.register(head_cell(11, OP_HEAD, head(b"")));

    let mut small = [0u8; 2];
    let _ = l.register(word_cell(12, OP_RENDER, render(b"/a/b/c", &mut small)));

    let mut deep = [b'/'; 36];
    let mut i = 0usize;
    while i < 18 {
        let at = 1 + i * 2;
        if let Some(slot) = deep.get_mut(at) {
            *slot = b'a';
        }
        i += 1;
    }
    let _ = l.register(head_cell(13, OP_HEAD, head(&deep)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：字符串形态的边界。
    out[1] = dirname(b"") == b"."
        && dirname(b"a") == b"."
        && dirname(b"a/b") == b"a"
        && dirname(b"/a/b/") == b"/a"
        && dirname(b"/a") == b"/"
        && dirname(b"/") == b"/"
        && dirname(b"///") == b"/"
        && dirname(b"//") == b"//"
        && dirname(b"//x") == b"//"
        && dirname(b"///x") == b"/";

    // 二路：区间、深度与上溯三者自洽。
    let h = head(b"/usr/bin/awk");
    let ok = match h {
        Ok(v) => {
            v.text(b"/usr/bin/awk") == Some(&b"/usr/bin"[..])
                && v.word() == 208
                && v.depth == 2
                && !v.root
        }
        Err(_) => false,
    };
    let anc = match up(b"/a/b/c", 2) {
        Ok(v) => v.len == 2 && v.depth == 1,
        Err(_) => false,
    };
    out[2] = ok
        && depth(b"/a/b/c/d") == Ok(4)
        && depth(b"/") == Ok(0)
        && depth(b"//") == Ok(0)
        && anc
        && up(b"/a/b/c", 3).map(|v| v.root) == Ok(true)
        && parent(b"/a").map(|v| v.len) == Ok(1)
        && parent(b"/") == Err(DN_EROOT);

    // 三路：八类失败各自可辨。
    let over = [b'/'; 80];
    let mut small = [0u8; 2];
    let mut deep = [b'/'; 36];
    let mut i = 0usize;
    while i < 18 {
        let at = 1 + i * 2;
        if let Some(slot) = deep.get_mut(at) {
            *slot = b'a';
        }
        i += 1;
    }
    out[3] = head(b"") == Err(DN_EEMPTY)
        && head(&over) == Err(DN_ELONG)
        && head(b"a/b") == Err(DN_EREL)
        && head(b"/a/../b") == Err(DN_EDOTDOT)
        && up(b"/a", 9) == Err(DN_EUP)
        && render(b"/a/b/c", &mut small) == Err(DN_EBUF)
        && head(&deep) == Err(DN_EDEPTH)
        && parent(b"/") == Err(DN_EROOT);

    out
}

/// 十三项判据。
pub fn run_unx11502_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11502.dirname_head");
    let l = build_ledger();

    s.add_dyn(
        "DN01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DN02-剥尾段再剥斜杠",
        dirname(b"/usr/bin/awk") == b"/usr/bin"
            && dirname(b"/a/b/") == b"/a"
            && dirname(b"a/b") == b"a"
            && dirname(b"/a//b") == b"/a"
            && l.sample(3).map(|x| x.value) == Some(8),
        "static",
    );

    s.add(
        "DN03-无斜杠归点",
        dirname(b"a") == b"."
            && dirname(b"") == b"."
            && dirname(b"abc") == b"."
            && l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DN04-根与双斜杠特例",
        dirname(b"/") == b"/"
            && dirname(b"//") == b"//"
            && dirname(b"//x") == b"//"
            && dirname(b"///x") == b"/"
            && head(b"//x").map(|h| h.len) == Ok(2)
            && head(b"///x").map(|h| h.len) == Ok(1),
        "static",
    );

    s.add(
        "DN05-头部区间可回取",
        {
            let h = match head(b"/usr/bin/awk") {
                Ok(v) => v,
                Err(_) => return s,
            };
            h.text(b"/usr/bin/awk") == Some(&b"/usr/bin"[..])
                && h.start == 0
                && h.len == 8
                && h.depth == 2
                && !h.root
                && l.sample(0).map(|x| x.value) == Some(208)
        },
        "static",
    );

    s.add(
        "DN06-深度按组件计",
        depth(b"/a/b/c/d") == Ok(4)
            && depth(b"/a") == Ok(1)
            && depth(b"/") == Ok(0)
            && depth(b"/a//b") == Ok(2)
            && l.sample(6).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "DN07-上溯一级",
        {
            let h = match up(b"/a/b/c", 1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            h.text(b"/a/b/c") == Some(&b"/a/b"[..])
                && h.depth == 2
                && !h.root
                && up(b"/a/b/c", 0).map(|v| v.depth) == Ok(3)
                && l.sample(4).map(|x| x.value) == Some(204)
        },
        "static",
    );

    s.add(
        "DN08-上溯到根",
        {
            let r = match up(b"/a/b/c", 3) {
                Ok(v) => v,
                Err(_) => return s,
            };
            r.root
                && r.depth == 0
                && r.text(b"/a/b/c") == Some(&b"/"[..])
                && up(b"/a/b/c", 4) == Err(DN_EUP)
                && l.sample(5).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "DN09-区间拒相对路径",
        head(b"a/b") == Err(DN_EREL)
            && up(b"a", 1) == Err(DN_EREL)
            && depth(b"a") == Err(DN_EREL)
            && l.sample(8).map(|x| x.code) == Some(DN_EREL),
        "static",
    );

    s.add(
        "DN10-点点点不许入区间",
        head(b"/a/../b") == Err(DN_EDOTDOT)
            && up(b"/a/../b", 1) == Err(DN_EDOTDOT)
            && depth(b"/..") == Err(DN_EDOTDOT)
            && l.sample(10).map(|x| x.code) == Some(DN_EDOTDOT),
        "static",
    );

    s.add(
        "DN11-根没有父目录",
        parent(b"/") == Err(DN_EROOT)
            && parent(b"//") == Err(DN_EROOT)
            && parent(b"/a").map(|h| h.root) == Ok(true)
            && parent(b"/a/b").map(|h| h.depth) == Ok(1),
        "static",
    );

    s.add(
        "DN12-缓冲与深度上限",
        {
            let mut small = [0u8; 2];
            let mut exact = [0u8; 4];
            let mut deep = [b'/'; 36];
            let mut i = 0usize;
            while i < 18 {
                let at = 1 + i * 2;
                if let Some(slot) = deep.get_mut(at) {
                    *slot = b'a';
                }
                i += 1;
            }
            render(b"/a/b/c", &mut exact) == Ok(4)
                && exact == [b'/', b'a', b'/', b'b']
                && render(b"/a/b/c", &mut small) == Err(DN_EBUF)
                && depth(&deep) == Err(DN_EDEPTH)
                && l.sample(12).map(|x| x.code) == Some(DN_EBUF)
        },
        "static",
    );

    s.add(
        "DN13-冻结重放四注入",
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
        let s = run_unx11502_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn string_form_edges() {
        assert_eq!(dirname(b""), b".");
        assert_eq!(dirname(b"abc"), b".");
        assert_eq!(dirname(b"a/b"), b"a");
        assert_eq!(dirname(b"/a/b/"), b"/a");
        assert_eq!(dirname(b"/"), b"/");
        assert_eq!(dirname(b"//"), b"//");
        assert_eq!(dirname(b"//x"), b"//");
        assert_eq!(dirname(b"///x"), b"/");
    }

    #[test]
    fn head_and_depth() {
        let h = head(b"/usr/bin/awk").expect("head");
        assert_eq!(h.text(b"/usr/bin/awk"), Some(&b"/usr/bin"[..]));
        assert_eq!(h.depth, 2);
        assert_eq!(h.word(), 208);
        assert_eq!(depth(b"/a/b/c/d"), Ok(4));
        assert_eq!(depth(b"/a//b"), Ok(2));
        assert_eq!(depth(b"//"), Ok(0));
    }

    #[test]
    fn walk_up() {
        let one = up(b"/a/b/c", 1).expect("up");
        assert_eq!(one.text(b"/a/b/c"), Some(&b"/a/b"[..]));
        assert_eq!(one.depth, 2);
        let all = up(b"/a/b/c", 3).expect("up");
        assert!(all.root);
        assert_eq!(all.depth, 0);
        assert_eq!(up(b"/a/b/c", 4), Err(DN_EUP));
        assert_eq!(up(b"/a/b/c", 0).map(|h| h.depth), Ok(3));
    }

    #[test]
    fn gate_order() {
        assert_eq!(head(b""), Err(DN_EEMPTY));
        assert_eq!(head(b"a"), Err(DN_EREL));
        assert_eq!(head(b"/a/../b"), Err(DN_EDOTDOT));
        assert_eq!(parent(b"/"), Err(DN_EROOT));
        assert_eq!(parent(b"/a").map(|h| h.root), Ok(true));
    }

    #[test]
    fn depth_limit() {
        let mut deep = [b'/'; 36];
        let mut i = 0usize;
        while i < 18 {
            let at = 1 + i * 2;
            deep[at] = b'a';
            i += 1;
        }
        assert_eq!(depth(&deep), Err(DN_EDEPTH));
        assert_eq!(head(&deep), Err(DN_EDEPTH));
    }

    #[test]
    fn buffer_exact_and_short() {
        let mut exact = [0u8; 4];
        assert_eq!(render(b"/a/b/c", &mut exact), Ok(4));
        assert_eq!(exact, [b'/', b'a', b'/', b'b']);
        let mut small = [0u8; 2];
        assert_eq!(render(b"/a/b/c", &mut small), Err(DN_EBUF));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
