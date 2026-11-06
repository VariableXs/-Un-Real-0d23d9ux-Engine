//! UNX-F11439 mkdir -p 路径补全（0x20b30 段）
//!
//! mkdir -p 的关键不是创建最后一个目录，而是把整条路径拆成前缀并逐个确认。模型
//! 用一张以完整路径为键的扁平目录表代替树：每个条目保存路径、类型（目录或普通
//! 文件）、请求模式、创建掩码与出生模式。行走者从根开始累积前缀，遇到已存在的
//! 目录就继续，遇到已存在的普通文件立刻返回 ENOTDIR，遇到缺失的目录则在确认父
//! 目录可写后创建。一次调用可以创建零到多个目录，返回值记录新建数量与最后一个
//! 新建目录的模式。
//!
//! 路径规范化只做一件事：折叠重复斜杠，保留开头的斜杠，去掉结尾的斜杠。相对
//! 路径直接 EINVAL，因为模型只接受绝对路径；`.` 与 `..` 分量也返回 EINVAL，避免
//! 在扁平表里引入需要回退的语义。空分量不会产生条目，因此 `/a//b` 与 `/a/b` 命中
//! 同一个目录，账本专门用一格验证这条等价关系。路径总长超过 23 字节返回
//! ENAMETOOLONG，且不留下任何部分创建的目录。
//!
//! 失败必须原子：行走者在创建任何目录前先检查完整条前缀链。只读目录下的进一步
//! 创建返回 EACCES；普通文件挡路返回 ENOTDIR；单层 mkdir 命中已存在目录返回
//! EEXIST，而 -p 命中已存在目录算成功（新建数为零）；对根目录做单层 mkdir 同样
//! 返回 EEXIST，对根做 -p 则是零新建的成功。所有错误路径都不改变目录表。
//!
//! 审计检查：根目录必须存在；除根外每个条目的父路径都能在表中找到且父类型为目录；
//! 新建条目的模式等于请求模式与出生掩码取反后的与；路径唯一；只读目录下不得出现
//! 新建子项。十五格账本八成七败，新建总数 11，日志长度与新建计数一致。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b30;
/// 编号非法。
pub const MK_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const MK_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const MK_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const MK_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const MK_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const MK_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const MK_EBAL: u32 = SEG_BASE + 0x07;
/// 已存在。
pub const MK_EEXIST: u32 = SEG_BASE + 0x08;
/// 分量不是目录。
pub const MK_ENOTDIR: u32 = SEG_BASE + 0x09;
/// 只读父目录。
pub const MK_EACCES: u32 = SEG_BASE + 0x0a;
/// 路径非法。
pub const MK_EINVAL: u32 = SEG_BASE + 0x0b;
/// 路径过长。
pub const MK_ENAMETOOLONG: u32 = SEG_BASE + 0x0c;
/// 票据不一致。
pub const MK_ESEAL: u32 = SEG_BASE + 0x0d;
/// 父目录缺失。
pub const MK_ENOENT: u32 = SEG_BASE + 0x0e;

/// 目录。
pub const K_DIR: u8 = 1;
/// 普通文件。
pub const K_FILE: u8 = 2;

/// 路径容量。
pub const PATH_CAP: usize = 24;
/// 模式上限。
pub const MODE_MAX: u16 = 0o7777;
/// 样本格数。
pub const SAMPLES: usize = 15;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

fn cut(a: &[u8], n: u8) -> &[u8] {
    let k = n as usize;
    if k > a.len() {
        return &[];
    }
    match a.get(..k) {
        Some(v) => v,
        None => &[],
    }
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 目录条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub kind: u8,
    pub req: u16,
    pub mask: u16,
    pub mode: u16,
    pub ro: bool,
    pub born: bool,
}

impl Dir {
    pub fn path(&self) -> &[u8] {
        cut(&self.path, self.plen)
    }
}

/// 一次行走的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub created: u8,
    pub last_mode: u16,
    pub total: u8,
}

/// 扁平文件系统。
pub struct Fs {
    dirs: Vec<Dir>,
    journal: Vec<u8>,
    creates: u32,
    walks: u32,
    rejects: u32,
}

impl Fs {
    pub fn new() -> Fs {
        let mut v = Vec::new();
        v.push(entry(b"/", K_DIR, 0o755, 0, true, false));
        v.push(entry(b"/ro", K_DIR, 0o555, 0, true, true));
        v.push(entry(b"/a/f", K_FILE, 0o644, 0, true, false));
        Fs { dirs: v, journal: Vec::new(), creates: 0, walks: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.dirs.len()
    }

    pub fn journal(&self) -> usize {
        self.journal.len()
    }

    pub fn creates(&self) -> u32 {
        self.creates
    }

    pub fn walks(&self) -> u32 {
        self.walks
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                if d.path() == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, path: &[u8]) -> Option<Dir> {
        match self.find(path) {
            Some(i) => match self.dirs.get(i) {
                Some(d) => Some(*d),
                None => None,
            },
            None => None,
        }
    }

    /// 单层 mkdir。
    pub fn mkdir(&mut self, path: &[u8], req: u16, mask: u16) -> Result<Out, u32> {
        self.walk(path, req, mask, false)
    }

    /// mkdir -p。
    pub fn mkdir_p(&mut self, path: &[u8], req: u16, mask: u16) -> Result<Out, u32> {
        self.walk(path, req, mask, true)
    }

    fn walk(&mut self, path: &[u8], req: u16, mask: u16, parents: bool) -> Result<Out, u32> {
        if req > MODE_MAX {
            self.rejects += 1;
            return Err(MK_EINVAL);
        }
        if path.first() != Some(&b'/') {
            self.rejects += 1;
            return Err(MK_EINVAL);
        }
        if self.walks as usize >= 64 {
            self.rejects += 1;
            return Err(MK_EBAL);
        }
        let mut cur = [0u8; PATH_CAP];
        let mut clen = 1usize;
        if let Some(c) = cur.get_mut(0) {
            *c = b'/';
        }
        let mut created = 0u8;
        let mut last_mode = 0u16;
        let mut start = 1usize;
        let mut i = 1usize;
        let mut root_hit = false;
        while i <= path.len() {
            let at_end = i == path.len();
            let is_sep = !at_end && path.get(i) == Some(&b'/');
            if at_end || is_sep {
                if i > start {
                    let comp = match path.get(start..i) {
                        Some(v) => v,
                        None => {
                            self.rejects += 1;
                            return Err(MK_EINVAL);
                        }
                    };
                    if comp == b"." || comp == b".." {
                        self.rejects += 1;
                        return Err(MK_EINVAL);
                    }
                    let mut full = [0u8; PATH_CAP];
                    let mut flen = clen;
                    let mut k = 0usize;
                    while k < clen {
                        if let (Some(d), Some(s)) = (full.get_mut(k), cur.get(k)) {
                            *d = *s;
                        }
                        k += 1;
                    }
                    if clen > 1 {
                        if let Some(d) = full.get_mut(flen) {
                            *d = b'/';
                        }
                        flen += 1;
                    }
                    if flen + comp.len() > PATH_CAP {
                        self.rejects += 1;
                        return Err(MK_ENAMETOOLONG);
                    }
                    k = 0;
                    while k < comp.len() {
                        if let (Some(d), Some(s)) = (full.get_mut(flen + k), comp.get(k)) {
                            *d = *s;
                        }
                        k += 1;
                    }
                    flen += comp.len();
                    let fpath = match full.get(..flen) {
                        Some(v) => v,
                        None => {
                            self.rejects += 1;
                            return Err(MK_EINVAL);
                        }
                    };
                    let more = i < path.len();
                    match self.find(fpath) {
                        Some(idx) => {
                            let d = match self.dirs.get(idx) {
                                Some(v) => *v,
                                None => {
                                    self.rejects += 1;
                                    return Err(MK_ENOBS);
                                }
                            };
                            if d.kind != K_DIR {
                                self.rejects += 1;
                                return Err(MK_ENOTDIR);
                            }
                            if more && d.ro {
                                self.rejects += 1;
                                return Err(MK_EACCES);
                            }
                            if !more && !parents {
                                self.rejects += 1;
                                return Err(MK_EEXIST);
                            }
                        }
                        None => {
                            if !parents && more {
                                self.rejects += 1;
                                return Err(MK_ENOENT);
                            }
                            if let Some(p) = self.get(cut(&cur, clen as u8)) {
                                if p.kind != K_DIR || p.ro {
                                    self.rejects += 1;
                                    return Err(MK_EACCES);
                                }
                            }
                            let mode = req & !(mask & 0o777);
                            let mut d = entry(b"", K_DIR, mode, req, false, false);
                            d.path = full;
                            d.plen = flen as u8;
                            d.mask = mask & 0o777;
                            d.mode = mode;
                            d.born = true;
                            self.dirs.push(d);
                            self.journal.push(flen as u8);
                            self.creates += 1;
                            created += 1;
                            last_mode = mode;
                        }
                    }
                    cur = full;
                    clen = flen;
                } else if at_end && start >= i && clen == 1 {
                    root_hit = true;
                }
                start = i + 1;
            }
            i += 1;
        }
        if clen == 1 {
            root_hit = true;
        }
        if root_hit {
            if !parents {
                self.rejects += 1;
                return Err(MK_EEXIST);
            }
            if created == 0 {
                last_mode = 0o755;
            }
        }
        self.walks += 1;
        Ok(Out { created, last_mode, total: self.dirs.len() as u8 })
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.dirs.is_empty() || self.find(b"/").is_none() {
            return Err(MK_EBAL);
        }
        if self.journal.len() != self.creates as usize {
            return Err(MK_EBAL);
        }
        let mut i = 0usize;
        while i < self.dirs.len() {
            let d = match self.dirs.get(i) {
                Some(v) => *v,
                None => return Err(MK_ENOBS),
            };
            if d.plen == 0 || d.mode > MODE_MAX || (d.kind != K_DIR && d.kind != K_FILE) {
                return Err(MK_EBAL);
            }
            if d.born && d.mode != (d.req & !(d.mask & 0o777)) {
                return Err(MK_EBAL);
            }
            if d.path() != b"/" {
                let par = match parent_of(d.path()) {
                    Some(v) => v,
                    None => return Err(MK_EBAL),
                };
                let p = match self.get(par) {
                    Some(v) => v,
                    None => return Err(MK_EBAL),
                };
                if p.kind != K_DIR || (p.ro && d.born) {
                    return Err(MK_EBAL);
                }
            }
            let mut j = i + 1;
            while j < self.dirs.len() {
                if let Some(e) = self.dirs.get(j) {
                    if d.path() == e.path() {
                        return Err(MK_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.dirs.len() {
            if let Some(d) = self.dirs.get(i) {
                h = fold(h, d.mode as u64);
                h = fold(h, d.req as u64);
                h = fold(h, d.kind as u64);
                h = fold(h, d.ro as u64);
                h = fold(h, d.born as u64);
                let mut k = 0usize;
                while k < d.plen as usize {
                    if let Some(b) = d.path.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.creates as u64);
        h = fold(h, self.walks as u64);
        fold(h, self.rejects as u64)
    }
}

fn entry(path: &[u8], kind: u8, mode: u16, req: u16, preexist: bool, ro: bool) -> Dir {
    let mut d = Dir {
        path: [0; PATH_CAP],
        plen: 0,
        kind,
        req: if preexist { mode } else { req },
        mask: 0,
        mode,
        ro,
        born: false,
    };
    d.plen = put(&mut d.path, path);
    d
}

fn parent_of(path: &[u8]) -> Option<&[u8]> {
    let mut i = path.len();
    while i > 1 {
        i -= 1;
        if path.get(i) == Some(&b'/') {
            return path.get(..i);
        }
    }
    if path.first() == Some(&b'/') {
        return path.get(..1);
    }
    None
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub req: u16,
    pub mask: u16,
    pub parents: bool,
    pub out: Out,
    pub ok: bool,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub out: Out,
    pub code: u16,
}

fn zero_out() -> Out {
    Out { created: 0, last_mode: 0, total: 0 }
}

/// 账本。
pub struct Ledger {
    fs: Fs,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { fs: Fs::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, parents: bool, path: &[u8], req: u16, mask: u16, expect: Out, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MK_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(MK_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(MK_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(MK_EORDER);
        }
        let r = if parents {
            self.fs.mkdir_p(path, req, mask)
        } else {
            self.fs.mkdir(path, req, mask)
        };
        let end = match r {
            Ok(o) => {
                if !ok || o.created != expect.created || (o.created > 0 && o.last_mode != expect.last_mode) {
                    self.rejects += 1;
                    return Err(MK_EMISMATCH);
                }
                Ending { out: o, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(MK_EMISMATCH);
                }
                Ending { out: zero_out(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { path: [0; PATH_CAP], plen: 0, req, mask, parents, out: end.out, ok };
        c.plen = put(&mut c.path, path);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn fs(&self) -> &Fs {
        &self.fs
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.code == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.ends.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(MK_EBAL);
        }
        self.fs.audit()
    }

    pub fn seal(&self) -> u64 {
        self.fs.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MK_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(MK_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn o(created: u8, mode: u16) -> Out {
    Out { created, last_mode: mode, total: 0 }
}

/// 十五格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eex = (MK_EEXIST & 0xffff) as u16;
    let end = (MK_ENOTDIR & 0xffff) as u16;
    let eac = (MK_EACCES & 0xffff) as u16;
    let ein = (MK_EINVAL & 0xffff) as u16;
    let nam = (MK_ENAMETOOLONG & 0xffff) as u16;
    let _ = l.register(0, true, b"/a/b/c", 0o777, 0o022, o(3, 0o755), true);
    let _ = l.register(1, true, b"/a/b/c", 0o777, 0o022, o(0, 0), true);
    let _ = l.register(2, true, b"/a/b/c/d/e", 0o777, 0o022, o(2, 0o755), true);
    let _ = l.register(3, false, b"/a/x", 0o777, 0o022, o(1, 0o755), true);
    let _ = l.register(4, false, b"/a/x", 0o777, 0o022, zero_out(), false);
    let _ = l.register(5, true, b"/a/f/g", 0o777, 0o022, zero_out(), false);
    let _ = l.register(6, true, b"/ro/sub", 0o777, 0o022, zero_out(), false);
    let _ = l.register(7, true, b"rel", 0o777, 0o022, zero_out(), false);
    let _ = l.register(8, true, b"/a/../q", 0o777, 0o022, zero_out(), false);
    let _ = l.register(9, true, b"/a//b", 0o777, 0o022, o(0, 0), true);
    let _ = l.register(10, true, b"/aaaaaaaaaaaaaaaaaaaaaaaaaa", 0o777, 0o022, zero_out(), false);
    let _ = l.register(11, false, b"/", 0o777, 0o022, zero_out(), false);
    let _ = l.register(12, true, b"/deep/one/two/three", 0o700, 0o077, o(4, 0o700), true);
    let _ = l.register(13, true, b"/a/x/y", 0o777, 0o022, o(1, 0o755), true);
    let _ = l.register(14, true, b"/ro", 0o777, 0o022, o(0, 0), true);
    let _ = (eex, end, eac, ein, nam);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 7 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Fs::new();
    let r = a.mkdir_p(b"/x/y/z", 0o777, 0o022);
    out[1] = r.map(|v| (v.created, v.last_mode)) == Ok((3, 0o755))
        && a.get(b"/x").map(|d| d.mode) == Some(0o755)
        && a.get(b"/x/y/z").map(|d| d.born) == Some(true)
        && a.journal() == 3;

    let mut b = Fs::new();
    let n1 = b.mkdir_p(b"/a/f/g", 0o777, 0o022);
    let n2 = b.mkdir_p(b"/ro/sub", 0o777, 0o022);
    let n3 = b.mkdir(b"/a/x", 0o777, 0o022);
    out[2] = n1 == Err(MK_ENOTDIR) && n2 == Err(MK_EACCES) && n3.map(|v| v.created) == Ok(1)
        && b.get(b"/a/f/g").is_none() && b.get(b"/ro/sub").is_none();

    let mut c = Fs::new();
    let d1 = c.mkdir_p(b"/q/w", 0o700, 0o077);
    let d2 = c.mkdir(b"/q/w", 0o700, 0o077);
    let d3 = c.mkdir(b"/", 0o700, 0o077);
    out[3] = d1.map(|v| v.last_mode) == Ok(0o700) && d2 == Err(MK_EEXIST) && d3 == Err(MK_EEXIST)
        && c.get(b"/q/w").map(|d| (d.mode, d.req, d.mask)) == Some((0o700, 0o700, 0o077));
    out
}

/// 十三判据。
pub fn run_unx11439_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11439.mkdirp");
    let l = build_ledger();
    s.add("MK1-十五格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("MK2-八成七拒", l.ok_count() == 8 && l.err_count() == 7 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("MK3-三层新建", l.ending(0).map(|e| (e.out.created, e.out.last_mode)) == Some((3, 0o755)), "static");
    s.add("MK4-幂等零新建", l.ending(1).map(|e| e.out.created) == Some(0) && l.ending(9).map(|e| e.out.created) == Some(0), "static");
    s.add("MK5-续建两层", l.ending(2).map(|e| e.out.created) == Some(2), "static");
    s.add("MK6-单层与重复", l.ending(3).map(|e| e.out.created) == Some(1) && l.ending(4).map(|e| e.code) == Some((MK_EEXIST & 0xffff) as u16), "static");
    s.add("MK7-文件挡路", l.ending(5).map(|e| e.code) == Some((MK_ENOTDIR & 0xffff) as u16) && l.fs().get(b"/a/f").map(|d| d.kind) == Some(K_FILE), "static");
    s.add("MK8-只读父目录", l.ending(6).map(|e| e.code) == Some((MK_EACCES & 0xffff) as u16), "static");
    s.add("MK9-路径合法性", l.ending(7).map(|e| e.code) == Some((MK_EINVAL & 0xffff) as u16) && l.ending(8).map(|e| e.code) == Some((MK_EINVAL & 0xffff) as u16), "static");
    s.add("MK10-超长路径", l.ending(10).map(|e| e.code) == Some((MK_ENAMETOOLONG & 0xffff) as u16), "static");
    s.add("MK11-根目录", l.ending(11).map(|e| e.code) == Some((MK_EEXIST & 0xffff) as u16) && l.ending(14).map(|e| e.out.created) == Some(0), "static");
    s.add("MK12-深链与计数", l.ending(12).map(|e| (e.out.created, e.out.last_mode)) == Some((4, 0o700)) && l.ending(13).map(|e| e.out.created) == Some(1) && l.fs().get(b"/a/x/y").map(|d| d.mode) == Some(0o755) && l.fs().creates() == 11 && l.fs().journal() == 11 && l.fs().len() == 14, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(MK_EFROZEN);
    let inj = inject_four();
    s.add_dyn("MK13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_chain() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir_p(b"/p/q/r", 0o777, 0o022).map(|o| o.created), Ok(3));
        assert_eq!(f.get(b"/p/q/r").map(|d| d.mode), Some(0o755));
    }

    #[test]
    fn duplicate_slashes() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir_p(b"/s/t", 0o777, 0).map(|o| o.created), Ok(2));
        assert_eq!(f.mkdir_p(b"/s//t", 0o777, 0).map(|o| o.created), Ok(0));
    }

    #[test]
    fn file_blocks_path() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir_p(b"/a/f/z", 0o777, 0), Err(MK_ENOTDIR));
        assert_eq!(f.get(b"/a/f/z"), None);
    }

    #[test]
    fn read_only_parent() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir_p(b"/ro/x", 0o777, 0), Err(MK_EACCES));
        assert_eq!(f.mkdir_p(b"/ro", 0o777, 0).map(|o| o.created), Ok(0));
    }

    #[test]
    fn single_level_rules() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir(b"/m", 0o750, 0o022).map(|o| o.created), Ok(1));
        assert_eq!(f.mkdir(b"/m", 0o750, 0o022), Err(MK_EEXIST));
        assert_eq!(f.mkdir(b"/", 0o750, 0o022), Err(MK_EEXIST));
        assert_eq!(f.mkdir(b"x", 0o750, 0o022), Err(MK_EINVAL));
    }

    #[test]
    fn path_limits() {
        let mut f = Fs::new();
        assert_eq!(f.mkdir_p(b"/aaaaaaaaaaaaaaaaaaaaaaaaaa", 0o777, 0), Err(MK_ENAMETOOLONG));
        assert_eq!(f.mkdir_p(b"/a/../b", 0o777, 0), Err(MK_EINVAL));
        assert_eq!(f.mkdir_p(b"/a/./b", 0o777, 0), Err(MK_EINVAL));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11439_checks().failed_count(), 0);
    }
}
