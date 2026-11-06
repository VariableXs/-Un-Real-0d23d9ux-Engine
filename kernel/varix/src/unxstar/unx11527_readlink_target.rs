//! UNX-F11527 readlink 链接目标读取（-f 归一）（0xf200 段）
//!
//! `readlink` 与 `readlink -f` 是两件不同的事，本模块把它们拆成两条入口：
//!
//! - **单跳**（`readlink`）：查表取出链接**字面目标**，不做任何改写。目标可以是
//!   相对路径（例如 `/etc/issue -> issue.real`），读出来就是 `issue.real`；
//! - **全解**（`readlink -f`）：反复把目标按「相对则拼接父目录、绝对则直接替换」
//!   的规则卷进去，直到落点既不是链接也不在表里，最后给出归一化绝对路径。
//!
//! 因此本模块的真正难点不在「读」，而在**归一文法**：
//!
//! - 必须以 `/` 开头，相对路径直接 `RL_EABS`；
//! - 空串是参数错误（`RL_EINVAL`），不是「当前目录」；
//! - `//` 折叠、`.` 丢弃、`..` 弹出上一级；根目录上再 `..` 仍是根，不会越界；
//! - 归一后的长度受 `PATHMAX` 约束，超出即 `RL_EPATH`；
//! - 非 UTF-8 分量拒绝，不做替换。
//!
//! 链接环用**跳数**而不是「访问集合」来判定：跳数是最小充分条件，且不需要
//! 额外的集合存储。环一旦形成必然在 `MAX_HOPS` 内被截住（`RL_ELOOP`）。
//!
//! 闸门次序：
//!
//! - `normalize`：空串 → 相对 → 超长 → 非 UTF-8 → 分量语法；
//! - `readlink`：不存在 → 不是链接 → 目标非 UTF-8；
//! - `canonical`：归一化失败直接透传，其后逐跳判环。
//!
//! 表内的「文件」用空目标 + `KIND_FILE` 表示，于是「存在但不是链接」和「不存在」
//! 是两个可分辨的失败，而不是同一个 `ENOENT`。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf200;

/// 参数非法（空路径、空目标、重名）。
pub const RL_EINVAL: u32 = SEG_BASE + 0x01;
/// 路径不存在。
pub const RL_ENOENT: u32 = SEG_BASE + 0x02;
/// 存在但不是符号链接。
pub const RL_ENOTLNK: u32 = SEG_BASE + 0x03;
/// 链接环。
pub const RL_ELOOP: u32 = SEG_BASE + 0x04;
/// 名字或目标过长。
pub const RL_EPATH: u32 = SEG_BASE + 0x05;
/// 相对路径无法归一。
pub const RL_EABS: u32 = SEG_BASE + 0x06;
/// 表满。
pub const RL_EFULL: u32 = SEG_BASE + 0x07;
/// 条目类型非法。
pub const RL_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 表容量。
pub const CAP: usize = 12;
/// 名字缓冲。
pub const NMAX: usize = 24;
/// 目标缓冲。
pub const TMAX: usize = 40;
/// 归一化后路径上限。
pub const PATHMAX: usize = 48;
/// 最大跳数。
pub const MAX_HOPS: u32 = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 普通文件。
pub const KIND_FILE: u8 = 1;
/// 符号链接。
pub const KIND_LINK: u8 = 2;

/// 操作码：单跳读取。
pub const OP_READ: u8 = 1;
/// 操作码：全解。
pub const OP_CANON: u8 = 2;
/// 操作码：归一。
pub const OP_NORM: u8 = 3;
/// 操作码：表观测。
pub const OP_STAT: u8 = 4;

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

/// 路径是否绝对。
pub fn is_abs(p: &[u8]) -> bool {
    p.first() == Some(&b'/')
}

/// 弹掉最后一个分量（含其前导斜杠）。
fn pop_component(out: &mut String) {
    let mut n = out.len();
    loop {
        if n == 0 {
            break;
        }
        match out.as_bytes().get(n - 1) {
            Some(b) => {
                if *b == b'/' {
                    break;
                }
            }
            None => break,
        }
        n -= 1;
    }
    while n > 0 && out.as_bytes().get(n - 1) == Some(&b'/') {
        n -= 1;
    }
    out.truncate(n);
}

/// 归一化绝对路径。
pub fn normalize(p: &[u8]) -> Result<String, u32> {
    if p.is_empty() {
        return Err(RL_EINVAL);
    }
    if !is_abs(p) {
        return Err(RL_EABS);
    }
    if p.len() > PATHMAX {
        return Err(RL_EPATH);
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < p.len() {
        let b = match p.get(i) {
            Some(v) => *v,
            None => break,
        };
        if b == b'/' {
            i += 1;
            continue;
        }
        let start = i;
        while i < p.len() {
            match p.get(i) {
                Some(v) => {
                    if *v == b'/' {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        let comp = match p.get(start..i) {
            Some(v) => v,
            None => break,
        };
        if comp == b"." {
            continue;
        }
        if comp == b".." {
            pop_component(&mut out);
            continue;
        }
        let text = match core::str::from_utf8(comp) {
            Ok(v) => v,
            Err(_) => return Err(RL_EINVAL),
        };
        out.push('/');
        out.push_str(text);
        if out.len() > PATHMAX {
            return Err(RL_EPATH);
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    Ok(out)
}

/// 父目录（根返回 `/`）。
pub fn parent(p: &str) -> &str {
    let b = p.as_bytes();
    let mut i = b.len();
    while i > 0 {
        if b.get(i - 1) == Some(&b'/') {
            let mut j = i - 1;
            while j > 0 && b.get(j - 1) == Some(&b'/') {
                j -= 1;
            }
            if j == 0 {
                return "/";
            }
            return match p.get(..j) {
                Some(v) => v,
                None => "/",
            };
        }
        i -= 1;
    }
    "/"
}

/// 最后一个分量（全斜杠返回空串）。
pub fn base_name(p: &str) -> &str {
    let b = p.as_bytes();
    let mut end = b.len();
    while end > 0 && b.get(end - 1) == Some(&b'/') {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && b.get(start - 1) != Some(&b'/') {
        start -= 1;
    }
    match p.get(start..end) {
        Some(v) => v,
        None => "",
    }
}

/// 相对目标拼到基目录上再归一。
pub fn join(base: &str, target: &[u8]) -> Result<String, u32> {
    if is_abs(target) {
        return normalize(target);
    }
    let mut buf = String::from(base);
    if buf.len() > 1 && buf.ends_with('/') {
        buf.pop();
    }
    if !buf.ends_with('/') {
        buf.push('/');
    }
    let text = match core::str::from_utf8(target) {
        Ok(v) => v,
        Err(_) => return Err(RL_EINVAL),
    };
    buf.push_str(text);
    normalize(buf.as_bytes())
}

/// 表内条目。
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    name: [u8; NMAX],
    nlen: u8,
    target: [u8; TMAX],
    tlen: u8,
    kind: u8,
}

impl Entry {
    /// 空白条目。
    pub const fn blank() -> Entry {
        Entry { name: [0; NMAX], nlen: 0, target: [0; TMAX], tlen: 0, kind: 0 }
    }

    /// 名字。
    pub fn name(&self) -> &[u8] {
        self.name.get(..self.nlen as usize).unwrap_or(&[])
    }

    /// 目标。
    pub fn target(&self) -> &[u8] {
        self.target.get(..self.tlen as usize).unwrap_or(&[])
    }

    /// 类型。
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// 是否符号链接。
    pub fn is_link(&self) -> bool {
        self.kind == KIND_LINK
    }
}

/// 链接表。
#[derive(Clone, Copy, Debug)]
pub struct Table {
    rows: [Entry; CAP],
    len: usize,
    adds: u32,
    rejects: u32,
}

impl Table {
    /// 空表。
    pub const fn new() -> Table {
        Table { rows: [Entry::blank(); CAP], len: 0, adds: 0, rejects: 0 }
    }

    fn reject(&mut self, code: u32) -> Result<u32, u32> {
        self.rejects = self.rejects.saturating_add(1);
        Err(code)
    }

    /// 登记一条链接。
    pub fn link(&mut self, name: &[u8], target: &[u8]) -> Result<u32, u32> {
        self.insert(name, target, KIND_LINK)
    }

    /// 登记一个普通文件（目标恒为空）。
    pub fn file(&mut self, name: &[u8]) -> Result<u32, u32> {
        self.insert(name, b"", KIND_FILE)
    }

    /// 直接给类型（用于校验 `RL_EMODE`）。
    pub fn raw(&mut self, name: &[u8], target: &[u8], kind: u8) -> Result<u32, u32> {
        self.insert(name, target, kind)
    }

    fn insert(&mut self, name: &[u8], target: &[u8], kind: u8) -> Result<u32, u32> {
        if name.is_empty() {
            return self.reject(RL_EINVAL);
        }
        if kind != KIND_LINK && kind != KIND_FILE {
            return self.reject(RL_EMODE);
        }
        if name.len() > NMAX || target.len() > TMAX {
            return self.reject(RL_EPATH);
        }
        if kind == KIND_LINK && target.is_empty() {
            return self.reject(RL_EINVAL);
        }
        if kind == KIND_FILE && !target.is_empty() {
            return self.reject(RL_EMODE);
        }
        if self.find(name).is_some() {
            return self.reject(RL_EINVAL);
        }
        if self.len >= CAP {
            return self.reject(RL_EFULL);
        }
        let mut e = Entry::blank();
        e.nlen = name.len() as u8;
        e.tlen = target.len() as u8;
        e.kind = kind;
        let mut i = 0usize;
        while i < name.len() {
            if let (Some(dst), Some(src)) = (e.name.get_mut(i), name.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        i = 0;
        while i < target.len() {
            if let (Some(dst), Some(src)) = (e.target.get_mut(i), target.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        self.adds = self.adds.saturating_add(1);
        Ok((self.len - 1) as u32)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 条。
    pub fn entry(&self, i: usize) -> Option<Entry> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按名字查找。
    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(e) = self.rows.get(i) {
                if e.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 名字对应类型；不存在即 `RL_ENOENT`。
    pub fn kind_of(&self, name: &[u8]) -> Result<u8, u32> {
        match self.find(name) {
            Some(i) => match self.rows.get(i) {
                Some(e) => Ok(e.kind()),
                None => Err(RL_ENOENT),
            },
            None => Err(RL_ENOENT),
        }
    }

    /// 成功登记次数。
    pub fn adds(&self) -> u32 {
        self.adds
    }

    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.len > CAP {
            return false;
        }
        if self.adds as usize != self.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let e = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.nlen == 0 || e.nlen as usize > NMAX || e.tlen as usize > TMAX {
                return false;
            }
            if !is_abs(e.name()) {
                return false;
            }
            if e.kind == KIND_LINK {
                if e.tlen == 0 {
                    return false;
                }
            } else if e.kind == KIND_FILE {
                if e.tlen != 0 {
                    return false;
                }
            } else {
                return false;
            }
            let mut j = i + 1;
            while j < self.len {
                if let Some(o) = self.rows.get(j) {
                    if o.name() == e.name() {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 单跳读取链接目标。
pub fn readlink(tab: &Table, path: &[u8]) -> Result<String, u32> {
    let i = match tab.find(path) {
        Some(v) => v,
        None => return Err(RL_ENOENT),
    };
    let e = match tab.entry(i) {
        Some(v) => v,
        None => return Err(RL_ENOENT),
    };
    if !e.is_link() {
        return Err(RL_ENOTLNK);
    }
    let t = e.target();
    let text = match core::str::from_utf8(t) {
        Ok(v) => v,
        Err(_) => return Err(RL_EINVAL),
    };
    Ok(String::from(text))
}

/// 逐跳全解，返回落点与跳数。
fn walk(tab: &Table, path: &[u8]) -> Result<(String, u32), u32> {
    let mut cur = normalize(path)?;
    let mut hops = 0u32;
    loop {
        let idx = match tab.find(cur.as_bytes()) {
            Some(v) => v,
            None => return Ok((cur, hops)),
        };
        let e = match tab.entry(idx) {
            Some(v) => v,
            None => return Ok((cur, hops)),
        };
        if !e.is_link() {
            return Ok((cur, hops));
        }
        if hops >= MAX_HOPS {
            return Err(RL_ELOOP);
        }
        hops = hops.saturating_add(1);
        let base = String::from(parent(cur.as_str()));
        let target = e.target();
        cur = join(base.as_str(), target)?;
    }
}

/// `readlink -f`：归一化并全解。
pub fn canonical(tab: &Table, path: &[u8]) -> Result<String, u32> {
    match walk(tab, path) {
        Ok((p, _)) => Ok(p),
        Err(c) => Err(c),
    }
}

/// 解链跳数。
pub fn resolve_hops(tab: &Table, path: &[u8]) -> Result<u32, u32> {
    match walk(tab, path) {
        Ok((_, n)) => Ok(n),
        Err(c) => Err(c),
    }
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

/// `Result<String, u32>` → 样本（观测值为字符串长度）。
fn text_cell(slot: u8, op: u8, r: Result<String, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v.len() as u32 },
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RL_ESLOT);
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
            return Err(RL_EBAL);
        }
        const VALS: [u32; 8] = [13, 6, 18, 2, 8, 1, 10, 2];
        const CODES: [u32; 6] =
            [RL_ENOTLNK, RL_ENOENT, RL_EABS, RL_ELOOP, RL_EINVAL, RL_EPATH];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RL_EBAL),
            };
            if s.slot as usize != i {
                return Err(RL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RL_EVALUE);
                        }
                    }
                    None => return Err(RL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RL_EVALUE);
                        }
                    }
                    None => return Err(RL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RL_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(RL_ESEAL)
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

/// 样例表：两跳链、相对目标、文件、一对互为目标的环、自环。
pub fn build_table() -> Table {
    let mut t = Table::new();
    let _ = t.link(b"/bin/sh", b"/usr/bin/dash");
    let _ = t.link(b"/usr/bin/dash", b"/opt/dash/bin/dash");
    let _ = t.file(b"/opt/dash/bin/dash");
    let _ = t.link(b"/etc/issue", b"issue.real");
    let _ = t.file(b"/etc/passwd");
    let _ = t.link(b"/a", b"/b");
    let _ = t.link(b"/b", b"/a");
    let _ = t.link(b"/self", b"/self");
    t
}

/// 十四格账本：三跳读取、归一、全解、跳数与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let t = build_table();

    let _ = l.register(text_cell(0, OP_READ, readlink(&t, b"/bin/sh")));
    let _ = l.register(text_cell(1, OP_NORM, normalize(b"/a//b/./c/../d")));
    let _ = l.register(text_cell(2, OP_CANON, canonical(&t, b"/bin/sh")));
    let _ = l.register(word_cell(3, OP_STAT, resolve_hops(&t, b"/bin/sh")));
    let _ = l.register(word_cell(4, OP_STAT, Ok(t.len() as u32)));

    let _ = l.register(word_cell(5, OP_READ, readlink(&t, b"/etc/passwd").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(6, OP_READ, readlink(&t, b"/nope").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(7, OP_CANON, canonical(&t, b"a/b").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(8, OP_CANON, canonical(&t, b"/self").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(9, OP_NORM, normalize(b"").map(|v| v.len() as u32)));

    let _ = l.register(text_cell(10, OP_NORM, normalize(b"/a/b/../..")));
    let _ = l.register(text_cell(11, OP_READ, readlink(&t, b"/etc/issue")));
    let _ = l.register(word_cell(12, OP_STAT, t.kind_of(b"/etc/issue").map(|v| v as u32)));

    let mut long = [b'a'; 60];
    long[0] = b'/';
    let _ = l.register(word_cell(13, OP_NORM, normalize(&long).map(|v| v.len() as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：单跳 / 相对目标拼接 / 归一 / 根上溯。
    let mut t = Table::new();
    let a = t.link(b"/bin/sh", b"dash");
    let b = t.link(b"/etc/issue", b"issue.real");
    let f = t.file(b"/etc/passwd");
    let c = t.file(b"/dash");
    let read = readlink(&t, b"/bin/sh") == Ok(String::from("dash"));
    let can = canonical(&t, b"/etc/issue") == Ok(String::from("/etc/issue.real"));
    let hop = resolve_hops(&t, b"/etc/issue") == Ok(1);
    let norm = normalize(b"/a//b/./c/../d") == Ok(String::from("/a/b/d"));
    let root = normalize(b"/../..") == Ok(String::from("/"));
    let base = base_name("/etc/issue") == "issue";
    let par = parent("/etc/issue") == "/etc";
    out[1] = a == Ok(0)
        && b == Ok(1)
        && f == Ok(2)
        && c == Ok(3)
        && read
        && can
        && hop
        && norm
        && root
        && base
        && par
        && t.consistent();

    // 二路：六类拒绝各自可辨。
    let mut s = Table::new();
    let _ = s.link(b"/bin/sh", b"/usr/bin/dash");
    let _ = s.file(b"/etc/passwd");
    let mut ring = Table::new();
    let _ = ring.link(b"/self", b"/self");
    let mut long = [b'a'; 60];
    long[0] = b'/';
    out[2] = readlink(&s, b"/etc/passwd") == Err(RL_ENOTLNK)
        && readlink(&s, b"/nope") == Err(RL_ENOENT)
        && normalize(b"rel") == Err(RL_EABS)
        && normalize(b"") == Err(RL_EINVAL)
        && canonical(&ring, b"/self") == Err(RL_ELOOP)
        && normalize(&long) == Err(RL_EPATH);

    // 三路：表满、重名、非法类型、冻结重放。
    let mut q = Table::new();
    let mut all = true;
    let mut i = 0u32;
    while i < CAP as u32 {
        let name = [b'/', b'k', b'0' + i as u8];
        if q.link(&name, b"/z").is_err() {
            all = false;
        }
        i += 1;
    }
    let full = q.link(b"/kA", b"/z") == Err(RL_EFULL);
    let dup = q.link(b"/k0", b"/z") == Err(RL_EINVAL);
    let mode = q.raw(b"/m", b"/z", 7) == Err(RL_EMODE);
    let rej = q.rejects() == 3;

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = all
        && full
        && dup
        && mode
        && rej
        && q.consistent()
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(RL_ESLOT)
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11527_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11527.readlink_target");

    s.add(
        "RL01-单跳读取",
        {
            let t = build_table();
            readlink(&t, b"/bin/sh") == Ok(String::from("/usr/bin/dash"))
                && readlink(&t, b"/etc/issue") == Ok(String::from("issue.real"))
                && readlink(&t, b"/self") == Ok(String::from("/self"))
        },
        "static",
    );

    s.add(
        "RL02-非链接与缺席",
        {
            let t = build_table();
            readlink(&t, b"/etc/passwd") == Err(RL_ENOTLNK)
                && readlink(&t, b"/nope") == Err(RL_ENOENT)
                && t.kind_of(b"/etc/passwd") == Ok(KIND_FILE)
                && t.kind_of(b"/bin/sh") == Ok(KIND_LINK)
        },
        "static",
    );

    s.add(
        "RL03-归一文法",
        {
            normalize(b"/a//b/./c/../d") == Ok(String::from("/a/b/d"))
                && normalize(b"/") == Ok(String::from("/"))
                && normalize(b"//x") == Ok(String::from("/x"))
                && normalize(b"/a/b/../..") == Ok(String::from("/"))
                && normalize(b"/../..") == Ok(String::from("/"))
        },
        "static",
    );

    s.add(
        "RL04-相对与空串",
        {
            normalize(b"a/b") == Err(RL_EABS)
                && normalize(b"") == Err(RL_EINVAL)
                && normalize(b".") == Err(RL_EABS)
        },
        "static",
    );

    s.add(
        "RL05-两跳全解",
        {
            let t = build_table();
            canonical(&t, b"/bin/sh") == Ok(String::from("/opt/dash/bin/dash"))
                && resolve_hops(&t, b"/bin/sh") == Ok(2)
                && canonical(&t, b"/usr/bin/dash") == Ok(String::from("/opt/dash/bin/dash"))
                && resolve_hops(&t, b"/usr/bin/dash") == Ok(1)
        },
        "static",
    );

    s.add(
        "RL06-相对目标拼接",
        {
            let t = build_table();
            canonical(&t, b"/etc/issue") == Ok(String::from("/etc/issue.real"))
                && resolve_hops(&t, b"/etc/issue") == Ok(1)
                && join("/etc", b"issue.real") == Ok(String::from("/etc/issue.real"))
                && join("/", b"x") == Ok(String::from("/x"))
        },
        "static",
    );

    s.add(
        "RL07-链接环截断",
        {
            let t = build_table();
            canonical(&t, b"/self") == Err(RL_ELOOP)
                && canonical(&t, b"/a") == Err(RL_ELOOP)
                && resolve_hops(&t, b"/a") == Err(RL_ELOOP)
        },
        "static",
    );

    s.add(
        "RL08-超长拒绝",
        {
            let mut long = [b'a'; 60];
            long[0] = b'/';
            let mut name = [b'/'; 30];
            name[1] = b'q';
            normalize(&long) == Err(RL_EPATH)
                && Table::new().link(&name, b"/x") == Err(RL_EPATH)
        },
        "static",
    );

    s.add(
        "RL09-表满与重名",
        {
            let mut t = Table::new();
            let mut i = 0u32;
            let mut ok = true;
            while i < CAP as u32 {
                let name = [b'/', b'n', b'0' + i as u8];
                if t.link(&name, b"/t").is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok && t.len() == CAP
                && t.link(b"/zz", b"/t") == Err(RL_EFULL)
                && t.link(b"/n0", b"/t") == Err(RL_EINVAL)
                && t.raw(b"/m", b"/t", 0) == Err(RL_EMODE)
                && t.link(b"/el", b"") == Err(RL_EINVAL)
                && t.rejects() == 4
                && t.consistent()
        },
        "static",
    );

    s.add(
        "RL10-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && build_table().consistent()
        },
        "static",
    );

    s.add(
        "RL11-错误码序",
        {
            let l = build_ledger();
            let mut got = [0u32; 6];
            let mut ci = 0usize;
            let mut i = 0usize;
            let mut good = true;
            while i < l.len() {
                match l.sample(i) {
                    Some(smp) => {
                        if !smp.ok {
                            if let Some(dst) = got.get_mut(ci) {
                                *dst = smp.code;
                            }
                            ci += 1;
                        }
                    }
                    None => good = false,
                }
                i += 1;
            }
            good
                && ci == 6
                && got == [RL_ENOTLNK, RL_ENOENT, RL_EABS, RL_ELOOP, RL_EINVAL, RL_EPATH]
        },
        "static",
    );

    s.add(
        "RL12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen() && a.is_ok() && b == Err(RL_EBAL) && c == Err(RL_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(RL_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "RL13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shape() {
        let t = build_table();
        assert_eq!(t.len(), 8);
        assert!(t.consistent());
        assert_eq!(t.rejects(), 0);
        assert!(t.find(b"/bin/sh").is_some());
        assert!(t.find(b"/nope").is_none());
    }

    #[test]
    fn readlink_single_hop() {
        let t = build_table();
        assert_eq!(readlink(&t, b"/bin/sh"), Ok(String::from("/usr/bin/dash")));
        assert_eq!(readlink(&t, b"/etc/issue"), Ok(String::from("issue.real")));
        assert_eq!(readlink(&t, b"/etc/passwd"), Err(RL_ENOTLNK));
        assert_eq!(readlink(&t, b"/nope"), Err(RL_ENOENT));
    }

    #[test]
    fn normalize_rules() {
        assert_eq!(normalize(b"/a//b/./c/../d"), Ok(String::from("/a/b/d")));
        assert_eq!(normalize(b"/"), Ok(String::from("/")));
        assert_eq!(normalize(b"/a/b/../.."), Ok(String::from("/")));
        assert_eq!(normalize(b"/../.."), Ok(String::from("/")));
        assert_eq!(normalize(b"a/b"), Err(RL_EABS));
        assert_eq!(normalize(b""), Err(RL_EINVAL));
    }

    #[test]
    fn canonical_chains() {
        let t = build_table();
        assert_eq!(canonical(&t, b"/bin/sh"), Ok(String::from("/opt/dash/bin/dash")));
        assert_eq!(resolve_hops(&t, b"/bin/sh"), Ok(2));
        assert_eq!(canonical(&t, b"/etc/issue"), Ok(String::from("/etc/issue.real")));
        assert_eq!(resolve_hops(&t, b"/etc/issue"), Ok(1));
        assert_eq!(canonical(&t, b"/self"), Err(RL_ELOOP));
    }

    #[test]
    fn helpers() {
        assert_eq!(parent("/etc/issue"), "/etc");
        assert_eq!(parent("/x"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(base_name("/etc/issue"), "issue");
        assert_eq!(base_name("/a/b/"), "b");
        assert_eq!(join("/etc", b"issue.real"), Ok(String::from("/etc/issue.real")));
        assert_eq!(join("/", b"x"), Ok(String::from("/x")));
        assert_eq!(join("/etc", b"/abs"), Ok(String::from("/abs")));
    }

    #[test]
    fn table_limits() {
        let mut t = Table::new();
        let mut i = 0u32;
        while i < CAP as u32 {
            let name = [b'/', b'n', b'0' + i as u8];
            assert!(t.link(&name, b"/t").is_ok());
            i += 1;
        }
        assert_eq!(t.link(b"/zz", b"/t"), Err(RL_EFULL));
        assert_eq!(t.link(b"/n0", b"/t"), Err(RL_EINVAL));
        assert_eq!(t.link(b"/el", b""), Err(RL_EINVAL));
        assert_eq!(t.raw(b"/m", b"/t", 9), Err(RL_EMODE));
        assert_eq!(t.rejects(), 4);
        assert!(t.is_empty() == false);
        assert!(t.consistent());
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(RL_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(RL_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(RL_ESEAL));
        assert!(l.audit().is_ok());
    }

}
