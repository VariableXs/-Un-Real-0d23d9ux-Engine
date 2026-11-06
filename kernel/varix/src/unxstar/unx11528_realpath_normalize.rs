//! UNX-F11528 realpath 路径归一化（0xf210 段）
//!
//! `realpath` 不是「把 `..` 抠掉」那么简单，它的语义是**逐分量重放**：
//!
//! 1. 取下一个分量，先判 `.` / `..` / 普通名字；
//! 2. 普通名字拼到当前路径后查树：
//!    - 目录 → 前进；
//!    - 符号链接 → **当前路径不动**，把链接目标拼回待处理串首部，跳数 +1；
//!    - 普通文件 → 只有当它是最后一段时才是合法落点，否则 `RP_ENOTDIR`；
//!    - 缺席 → 严格模式 `RP_ENOENT`，`-m` 模式按字面继续。
//!
//! 第 2 步的顺序决定了本模块最容易被写错的地方：`..` 必须在**前一个分量已被
//! 解析成链接之后**再作用。于是 `/etc/../x`（`/etc -> /usr/etc`）要变成
//! `/usr/x`，而不是 `/x`。把 `..` 提前折叠的实现在这里必错。
//!
//! 尾斜杠不单独处理，它自然地从「还有剩余串」里长出来：`/usr/bin/sh/` 在
//! `sh -> dash` 之后剩余 `/`，于是最后一个分量 `dash` 带着非空尾巴，被判
//! `RP_ENOTDIR` —— 与 `realpath(3)` 一致。
//!
//! 环用跳数截断（`MAX_HOPS`），不依赖访问集合；相对路径没有 cwd 可依，直接
//! `RP_EABS`，而不是默默按 `/` 拼。
//!
//! 闸门次序：
//!
//! - 模式 → 空串 → 相对 → 超长 → 非 UTF-8；
//! - 解析：不存在 → 中间是文件 → 环。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf210;

/// 参数非法（空路径、非 UTF-8）。
pub const RP_EINVAL: u32 = SEG_BASE + 0x01;
/// 相对路径无法归一。
pub const RP_EABS: u32 = SEG_BASE + 0x02;
/// 链接环。
pub const RP_ELOOP: u32 = SEG_BASE + 0x03;
/// 分量不存在。
pub const RP_ENOENT: u32 = SEG_BASE + 0x04;
/// 中间分量是普通文件。
pub const RP_ENOTDIR: u32 = SEG_BASE + 0x05;
/// 路径过长。
pub const RP_EPATH: u32 = SEG_BASE + 0x06;
/// 表满。
pub const RP_EFULL: u32 = SEG_BASE + 0x07;
/// 模式非法。
pub const RP_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RP_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RP_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RP_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RP_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RP_ENOBS: u32 = SEG_BASE + 0x0d;

/// 树容量。
pub const CAP: usize = 20;
/// 路径缓冲。
pub const NMAX: usize = 24;
/// 目标缓冲。
pub const TMAX: usize = 32;
/// 归一化后路径上限。
pub const PATHMAX: usize = 48;
/// 待处理串上限。
pub const RESTMAX: usize = 96;
/// 最大跳数。
pub const MAX_HOPS: u32 = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 目录。
pub const KIND_DIR: u8 = 1;
/// 普通文件。
pub const KIND_FILE: u8 = 2;
/// 符号链接。
pub const KIND_LINK: u8 = 3;

/// 严格模式：所有分量必须存在。
pub const MODE_STRICT: u8 = 1;
/// `-m`：缺席分量按字面保留。
pub const MODE_MISSING_OK: u8 = 2;

/// 操作码：全解。
pub const OP_CANON: u8 = 1;
/// 操作码：跳数。
pub const OP_HOPS: u8 = 2;
/// 操作码：类型。
pub const OP_KIND: u8 = 3;
/// 操作码：计数。
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

/// 把分量拼到当前路径后。
pub fn join(cur: &str, comp: &[u8]) -> Result<String, u32> {
    let text = match core::str::from_utf8(comp) {
        Ok(v) => v,
        Err(_) => return Err(RP_EINVAL),
    };
    let mut out = String::from(cur);
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    if !out.ends_with('/') {
        out.push('/');
    }
    out.push_str(text);
    if out.len() > PATHMAX {
        return Err(RP_EPATH);
    }
    Ok(out)
}

/// 树节点。
#[derive(Clone, Copy, Debug)]
pub struct Node {
    path: [u8; NMAX],
    plen: u8,
    target: [u8; TMAX],
    tlen: u8,
    kind: u8,
}

impl Node {
    /// 空白节点。
    pub const fn blank() -> Node {
        Node { path: [0; NMAX], plen: 0, target: [0; TMAX], tlen: 0, kind: 0 }
    }

    /// 路径。
    pub fn path(&self) -> &[u8] {
        self.path.get(..self.plen as usize).unwrap_or(&[])
    }

    /// 链接目标。
    pub fn target(&self) -> &[u8] {
        self.target.get(..self.tlen as usize).unwrap_or(&[])
    }

    /// 类型。
    pub fn kind(&self) -> u8 {
        self.kind
    }
}

/// 文件树（只登记目录、文件与链接，父目录隐式存在）。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    rows: [Node; CAP],
    len: usize,
    adds: u32,
    rejects: u32,
}

impl Tree {
    /// 空树。
    pub const fn new() -> Tree {
        Tree { rows: [Node::blank(); CAP], len: 0, adds: 0, rejects: 0 }
    }

    fn reject(&mut self, code: u32) -> Result<u32, u32> {
        self.rejects = self.rejects.saturating_add(1);
        Err(code)
    }

    /// 登记目录。
    pub fn dir(&mut self, path: &[u8]) -> Result<u32, u32> {
        self.insert(path, b"", KIND_DIR)
    }

    /// 登记文件。
    pub fn file(&mut self, path: &[u8]) -> Result<u32, u32> {
        self.insert(path, b"", KIND_FILE)
    }

    /// 登记链接。
    pub fn link(&mut self, path: &[u8], target: &[u8]) -> Result<u32, u32> {
        self.insert(path, target, KIND_LINK)
    }

    /// 直接给类型（用于校验 `RP_EMODE`）。
    pub fn raw(&mut self, path: &[u8], target: &[u8], kind: u8) -> Result<u32, u32> {
        self.insert(path, target, kind)
    }

    fn insert(&mut self, path: &[u8], target: &[u8], kind: u8) -> Result<u32, u32> {
        if path.is_empty() {
            return self.reject(RP_EINVAL);
        }
        if kind != KIND_DIR && kind != KIND_FILE && kind != KIND_LINK {
            return self.reject(RP_EMODE);
        }
        if path.len() > NMAX || target.len() > TMAX {
            return self.reject(RP_EPATH);
        }
        if path.first() != Some(&b'/') {
            return self.reject(RP_EABS);
        }
        if kind == KIND_LINK && target.is_empty() {
            return self.reject(RP_EINVAL);
        }
        if kind != KIND_LINK && !target.is_empty() {
            return self.reject(RP_EMODE);
        }
        if self.find(path).is_some() {
            return self.reject(RP_EINVAL);
        }
        if self.len >= CAP {
            return self.reject(RP_EFULL);
        }
        let mut n = Node::blank();
        n.plen = path.len() as u8;
        n.tlen = target.len() as u8;
        n.kind = kind;
        let mut i = 0usize;
        while i < path.len() {
            if let (Some(dst), Some(src)) = (n.path.get_mut(i), path.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        i = 0;
        while i < target.len() {
            if let (Some(dst), Some(src)) = (n.target.get_mut(i), target.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = n;
        }
        self.len += 1;
        self.adds = self.adds.saturating_add(1);
        Ok((self.len - 1) as u32)
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空树。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 个节点。
    pub fn node(&self, i: usize) -> Option<Node> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 按路径查找。
    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.rows.get(i) {
                if n.path() == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 类型。
    pub fn kind_at(&self, i: usize) -> u8 {
        match self.rows.get(i) {
            Some(n) => n.kind(),
            None => 0,
        }
    }

    /// 链接目标。
    pub fn target_at(&self, i: usize) -> Result<&[u8], u32> {
        match self.rows.get(i) {
            Some(n) => Ok(n.target()),
            None => Err(RP_ENOENT),
        }
    }

    /// 按路径取类型；缺席即 `RP_ENOENT`。
    pub fn kind_of(&self, path: &[u8]) -> Result<u8, u32> {
        match self.find(path) {
            Some(i) => Ok(self.kind_at(i)),
            None => Err(RP_ENOENT),
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
        if self.len > CAP || self.adds as usize != self.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let n = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if n.plen == 0 || n.plen as usize > NMAX || n.tlen as usize > TMAX {
                return false;
            }
            if n.path().first() != Some(&b'/') {
                return false;
            }
            match n.kind {
                KIND_DIR | KIND_FILE => {
                    if n.tlen != 0 {
                        return false;
                    }
                }
                KIND_LINK => {
                    if n.tlen == 0 {
                        return false;
                    }
                }
                _ => return false,
            }
            let mut j = i + 1;
            while j < self.len {
                if let Some(o) = self.rows.get(j) {
                    if o.path() == n.path() {
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

/// 逐分量重放，返回落点与跳数。
fn walk(tree: &Tree, path: &[u8], mode: u8) -> Result<(String, u32), u32> {
    if mode != MODE_STRICT && mode != MODE_MISSING_OK {
        return Err(RP_EMODE);
    }
    if path.is_empty() {
        return Err(RP_EINVAL);
    }
    if path.first() != Some(&b'/') {
        return Err(RP_EABS);
    }
    if path.len() > PATHMAX {
        return Err(RP_EPATH);
    }
    let text = match core::str::from_utf8(path) {
        Ok(v) => v,
        Err(_) => return Err(RP_EINVAL),
    };
    let mut cur = String::from("/");
    let mut rest = String::from(text);
    let mut hops = 0u32;
    loop {
        while rest.starts_with('/') {
            rest.remove(0);
        }
        if rest.is_empty() {
            break;
        }
        let end = match rest.find('/') {
            Some(v) => v,
            None => rest.len(),
        };
        let comp = match rest.get(..end) {
            Some(v) => String::from(v),
            None => String::new(),
        };
        let tail = match rest.get(end..) {
            Some(v) => String::from(v),
            None => String::new(),
        };
        if comp == "." {
            rest = tail;
            continue;
        }
        if comp == ".." {
            cur = String::from(parent(cur.as_str()));
            rest = tail;
            continue;
        }
        let cand = join(cur.as_str(), comp.as_bytes())?;
        let idx = tree.find(cand.as_bytes());
        let kind = match idx {
            Some(i) => tree.kind_at(i),
            None => 0,
        };
        match kind {
            KIND_DIR => {
                cur = cand;
                rest = tail;
            }
            KIND_LINK => {
                if hops >= MAX_HOPS {
                    return Err(RP_ELOOP);
                }
                hops = hops.saturating_add(1);
                let i = match idx {
                    Some(v) => v,
                    None => return Err(RP_ENOENT),
                };
                let target = tree.target_at(i)?;
                let t = match core::str::from_utf8(target) {
                    Ok(v) => v,
                    Err(_) => return Err(RP_EINVAL),
                };
                let mut nr = String::new();
                nr.push_str(t);
                nr.push_str(tail.as_str());
                if nr.len() > RESTMAX {
                    return Err(RP_EPATH);
                }
                rest = nr;
            }
            KIND_FILE => {
                if !tail.is_empty() {
                    return Err(RP_ENOTDIR);
                }
                cur = cand;
                break;
            }
            _ => {
                if mode != MODE_MISSING_OK {
                    return Err(RP_ENOENT);
                }
                cur = cand;
                rest = tail;
            }
        }
    }
    Ok((cur, hops))
}

/// 严格归一化（所有分量必须存在）。
pub fn realpath(tree: &Tree, path: &[u8]) -> Result<String, u32> {
    match walk(tree, path, MODE_STRICT) {
        Ok((p, _)) => Ok(p),
        Err(c) => Err(c),
    }
}

/// 带模式归一化。
pub fn realpath_mode(tree: &Tree, path: &[u8], mode: u8) -> Result<String, u32> {
    match walk(tree, path, mode) {
        Ok((p, _)) => Ok(p),
        Err(c) => Err(c),
    }
}

/// 跳数。
pub fn resolve_hops(tree: &Tree, path: &[u8]) -> Result<u32, u32> {
    match walk(tree, path, MODE_STRICT) {
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
            return Err(RP_ESLOT);
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
            return Err(RP_EBAL);
        }
        const VALS: [u32; 8] = [13, 15, 2, 13, 14, 10, 1, 1];
        const CODES: [u32; 6] =
            [RP_EABS, RP_EINVAL, RP_ELOOP, RP_ENOTDIR, RP_ENOTDIR, RP_ENOENT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RP_EBAL),
            };
            if s.slot as usize != i {
                return Err(RP_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RP_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RP_EVALUE);
                        }
                    }
                    None => return Err(RP_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RP_EVALUE);
                        }
                    }
                    None => return Err(RP_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RP_EBAL);
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
            return Err(RP_EBAL);
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
            Err(RP_ESEAL)
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

/// 样例树：`/bin` 与 `/etc` 指到别处，`/varlog` 是绝对链接，`/lib` 的落点不存在。
pub fn build_tree() -> Tree {
    let mut t = Tree::new();
    let _ = t.dir(b"/usr");
    let _ = t.dir(b"/usr/bin");
    let _ = t.dir(b"/usr/etc");
    let _ = t.dir(b"/usr/lib");
    let _ = t.dir(b"/var");
    let _ = t.dir(b"/var/log");
    let _ = t.link(b"/bin", b"/usr/bin");
    let _ = t.link(b"/etc", b"/usr/etc");
    let _ = t.link(b"/lib", b"/usr/lib");
    let _ = t.link(b"/varlog", b"/var/log");
    let _ = t.link(b"/usr/bin/sh", b"dash");
    let _ = t.link(b"/loopA", b"/loopB");
    let _ = t.link(b"/loopB", b"/loopA");
    let _ = t.file(b"/README");
    let _ = t.file(b"/usr/bin/dash");
    let _ = t.file(b"/usr/etc/issue");
    let _ = t.file(b"/var/log/syslog");
    t
}

/// 十四格账本：链归一、`..` 次序、`-m` 放行与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let t = build_tree();

    let _ = l.register(text_cell(0, OP_CANON, realpath(&t, b"/bin/sh")));
    let _ = l.register(text_cell(1, OP_CANON, realpath(&t, b"/varlog/syslog")));
    let _ = l.register(word_cell(2, OP_HOPS, resolve_hops(&t, b"/bin/sh")));
    let _ = l.register(text_cell(3, OP_CANON, realpath(&t, b"/usr/../bin/sh")));
    let _ = l.register(text_cell(4, OP_CANON, realpath(&t, b"/etc/issue")));

    let _ = l.register(word_cell(5, OP_CANON, realpath(&t, b"rel/x").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(6, OP_CANON, realpath(&t, b"").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(7, OP_CANON, realpath(&t, b"/loopA").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(8, OP_CANON, realpath(&t, b"/README/x").map(|v| v.len() as u32)));
    let _ = l.register(word_cell(9, OP_CANON, realpath(&t, b"/usr/bin/sh/").map(|v| v.len() as u32)));

    let _ = l.register(text_cell(10, OP_CANON, realpath_mode(&t, b"/lib/x", MODE_MISSING_OK)));
    let _ = l.register(word_cell(11, OP_CANON, realpath(&t, b"/lib/x").map(|v| v.len() as u32)));
    let _ = l.register(text_cell(12, OP_CANON, realpath(&t, b"/")));
    let _ = l.register(word_cell(13, OP_HOPS, resolve_hops(&t, b"/varlog/syslog")));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：两条链归一、`.` 丢弃、尾斜杠后的目录。
    let t = build_tree();
    let sh = realpath(&t, b"/bin/sh") == Ok(String::from("/usr/bin/dash"));
    let var = realpath(&t, b"/varlog/syslog") == Ok(String::from("/var/log/syslog"));
    let dot = realpath(&t, b"/./bin//./sh") == Ok(String::from("/usr/bin/dash"));
    let dir = realpath(&t, b"/bin/") == Ok(String::from("/usr/bin"));
    let hops = resolve_hops(&t, b"/bin/sh") == Ok(2);
    let rootp = realpath(&t, b"/") == Ok(String::from("/"));
    out[1] = sh && var && dot && dir && hops && rootp && t.consistent();

    // 二路：`..` 在链接之后生效，且严格/放行两模式可辨。
    let after = realpath(&t, b"/etc/../etc/issue") == Ok(String::from("/usr/etc/issue"));
    let before = realpath(&t, b"/usr/../bin/sh") == Ok(String::from("/usr/bin/dash"));
    let miss_ok = realpath_mode(&t, b"/lib/x", MODE_MISSING_OK) == Ok(String::from("/usr/lib/x"));
    let miss_strict = realpath(&t, b"/lib/x") == Err(RP_ENOENT);
    let bad_mode = realpath_mode(&t, b"/", 9) == Err(RP_EMODE);
    out[2] = after && before && miss_ok && miss_strict && bad_mode;

    // 三路：相对、空串、环、中间文件、尾斜杠、超长。
    let mut long = [b'a'; 60];
    long[0] = b'/';
    let rel = realpath(&t, b"rel") == Err(RP_EABS);
    let empty = realpath(&t, b"") == Err(RP_EINVAL);
    let ring = realpath(&t, b"/loopA") == Err(RP_ELOOP);
    let notdir = realpath(&t, b"/README/x") == Err(RP_ENOTDIR);
    let slash = realpath(&t, b"/usr/bin/sh/") == Err(RP_ENOTDIR);
    let deep = realpath(&t, &long) == Err(RP_EPATH);
    out[3] = rel && empty && ring && notdir && slash && deep && !t.is_empty();

    out
}

/// 十三项判据。
pub fn run_unx11528_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11528.realpath_normalize");

    s.add(
        "RP01-链接归一",
        {
            let t = build_tree();
            realpath(&t, b"/bin/sh") == Ok(String::from("/usr/bin/dash"))
                && realpath(&t, b"/varlog/syslog") == Ok(String::from("/var/log/syslog"))
                && resolve_hops(&t, b"/bin/sh") == Ok(2)
                && resolve_hops(&t, b"/varlog/syslog") == Ok(1)
        },
        "static",
    );

    s.add(
        "RP02-点与斜杠折叠",
        {
            let t = build_tree();
            realpath(&t, b"/./bin//./sh") == Ok(String::from("/usr/bin/dash"))
                && realpath(&t, b"///usr//bin/dash") == Ok(String::from("/usr/bin/dash"))
                && realpath(&t, b"/") == Ok(String::from("/"))
        },
        "static",
    );

    s.add(
        "RP03-上溯次序",
        {
            let t = build_tree();
            realpath(&t, b"/usr/../bin/sh") == Ok(String::from("/usr/bin/dash"))
                && realpath(&t, b"/etc/../etc/issue") == Ok(String::from("/usr/etc/issue"))
                && realpath(&t, b"/bin/../etc/issue") == Ok(String::from("/usr/etc/issue"))
        },
        "static",
    );

    s.add(
        "RP04-相对与空串",
        {
            let t = build_tree();
            realpath(&t, b"rel") == Err(RP_EABS)
                && realpath(&t, b"") == Err(RP_EINVAL)
                && realpath(&t, b"../x") == Err(RP_EABS)
        },
        "static",
    );

    s.add(
        "RP05-链接环",
        {
            let t = build_tree();
            realpath(&t, b"/loopA") == Err(RP_ELOOP)
                && realpath(&t, b"/loopB") == Err(RP_ELOOP)
                && resolve_hops(&t, b"/loopA") == Err(RP_ELOOP)
        },
        "static",
    );

    s.add(
        "RP06-中间文件",
        {
            let t = build_tree();
            realpath(&t, b"/README/x") == Err(RP_ENOTDIR)
                && realpath(&t, b"/README") == Ok(String::from("/README"))
                && t.kind_of(b"/README") == Ok(KIND_FILE)
        },
        "static",
    );

    s.add(
        "RP07-尾斜杠",
        {
            let t = build_tree();
            realpath(&t, b"/usr/bin/sh/") == Err(RP_ENOTDIR)
                && realpath(&t, b"/bin/") == Ok(String::from("/usr/bin"))
                && realpath(&t, b"/bin/./") == Ok(String::from("/usr/bin"))
        },
        "static",
    );

    s.add(
        "RP08-缺席两模式",
        {
            let t = build_tree();
            realpath(&t, b"/lib/x") == Err(RP_ENOENT)
                && realpath_mode(&t, b"/lib/x", MODE_MISSING_OK) == Ok(String::from("/usr/lib/x"))
                && realpath_mode(&t, b"/lib/x", MODE_STRICT) == Err(RP_ENOENT)
                && realpath_mode(&t, b"/", 0) == Err(RP_EMODE)
        },
        "static",
    );

    s.add(
        "RP09-超长拒绝",
        {
            let mut long = [b'a'; 60];
            long[0] = b'/';
            let mut name = [b'/'; 30];
            name[1] = b'q';
            let t = build_tree();
            realpath(&t, &long) == Err(RP_EPATH)
                && Tree::new().dir(&name) == Err(RP_EPATH)
                && Tree::new().dir(b"/a") == Ok(0)
        },
        "static",
    );

    s.add(
        "RP10-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && build_tree().consistent()
        },
        "static",
    );

    s.add(
        "RP11-错误码序",
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
                && got == [RP_EABS, RP_EINVAL, RP_ELOOP, RP_ENOTDIR, RP_ENOTDIR, RP_ENOENT]
        },
        "static",
    );

    s.add(
        "RP12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(RP_EBAL)
                && c == Err(RP_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(RP_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "RP13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_shape() {
        let t = build_tree();
        assert_eq!(t.len(), 17);
        assert!(t.consistent());
        assert_eq!(t.rejects(), 0);
        assert_eq!(t.kind_of(b"/bin"), Ok(KIND_LINK));
        assert_eq!(t.kind_of(b"/usr"), Ok(KIND_DIR));
        assert_eq!(t.kind_of(b"/nope"), Err(RP_ENOENT));
    }

    #[test]
    fn symlink_resolution() {
        let t = build_tree();
        assert_eq!(realpath(&t, b"/bin/sh"), Ok(String::from("/usr/bin/dash")));
        assert_eq!(realpath(&t, b"/varlog/syslog"), Ok(String::from("/var/log/syslog")));
        assert_eq!(resolve_hops(&t, b"/bin/sh"), Ok(2));
        assert_eq!(realpath(&t, b"/"), Ok(String::from("/")));
    }

    #[test]
    fn dotdot_after_link() {
        let t = build_tree();
        assert_eq!(realpath(&t, b"/usr/../bin/sh"), Ok(String::from("/usr/bin/dash")));
        assert_eq!(realpath(&t, b"/etc/../etc/issue"), Ok(String::from("/usr/etc/issue")));
        assert_eq!(realpath(&t, b"/./bin//./sh"), Ok(String::from("/usr/bin/dash")));
    }

    #[test]
    fn missing_and_modes() {
        let t = build_tree();
        assert_eq!(realpath(&t, b"/lib/x"), Err(RP_ENOENT));
        assert_eq!(realpath_mode(&t, b"/lib/x", MODE_MISSING_OK), Ok(String::from("/usr/lib/x")));
        assert_eq!(realpath_mode(&t, b"/", 3), Err(RP_EMODE));
    }

    #[test]
    fn structural_errors() {
        let t = build_tree();
        assert_eq!(realpath(&t, b"rel"), Err(RP_EABS));
        assert_eq!(realpath(&t, b""), Err(RP_EINVAL));
        assert_eq!(realpath(&t, b"/loopA"), Err(RP_ELOOP));
        assert_eq!(realpath(&t, b"/README/x"), Err(RP_ENOTDIR));
        assert_eq!(realpath(&t, b"/usr/bin/sh/"), Err(RP_ENOTDIR));
        assert_eq!(realpath(&t, b"/README"), Ok(String::from("/README")));
    }

    #[test]
    fn tree_limits() {
        let mut t = Tree::new();
        assert_eq!(t.dir(b"/a"), Ok(0));
        assert_eq!(t.dir(b"/a"), Err(RP_EINVAL));
        assert_eq!(t.dir(b"a"), Err(RP_EABS));
        assert_eq!(t.link(b"/b", b""), Err(RP_EINVAL));
        assert_eq!(t.raw(b"/c", b"/x", 9), Err(RP_EMODE));
        assert_eq!(t.raw(b"/c", b"", KIND_LINK), Err(RP_EINVAL));
        assert_eq!(t.rejects(), 5);
        assert!(t.consistent());
    }

    #[test]
    fn ledger_freeze() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(RP_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(RP_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(RP_ESEAL));
    }

}
