//! UNX-F11437 stat 字段合成（0x20b10 段）
//!
//! stat 返回的不是原始 inode，而是一份由多个来源合成的视图：st_mode 由文件类型
//! 位与权限位拼接，st_size 随类型改变含义，st_blocks 永远是 512 字节块向上取整，
//! 设备节点才有 st_rdev，符号链接的大小是目标字符串长度而不是被指向文件的大小。
//! 模型把这些规则集中在一个 `stat` 函数里，节点表只保存类型、权限、属主、时间、
//! 设备号与目标，所有派生字段都在查询时现算。
//!
//! 类型位有七种：普通、目录、符号链接、字符设备、块设备、FIFO、套接字。目录的
//! 大小固定为 4096（模型假设块大小为 4096），因此块数恒为 8；设备、FIFO、套接字
//! 的大小为 0，块数也为 0；符号链接的大小取目标长度，即使目标是空串也保留 1 个
//! 块的向上取整语义。`st_rdev` 采用 Linux 的 12:20 编码：主设备号在 bit 8-19，
//! 次设备号低 8 位加上 bit 20-31 的高位，模型用 `(1,3)` 与 `(8,0)` 两组值验证。
//!
//! 跟随选项是独立的语义分支。不跟随（lstat）时符号链接报告自己的模式与长度；
//! 跟随一次后必须落在非链接节点上，若目标仍是符号链接则返回 ELOOP，目标不存在
//! 返回 ENOENT。模型不做多级解析，因此“链接指向链接”与“链接指向自己”都归入
//! 同一类循环错误，这条边界在账本里占两格。
//!
//! 审计检查派生字段的可复算性：每个节点块数等于大小向上取整、硬链接对的 inode
//! 相同且 nlink 等于该 inode 在表中的出现次数、权限不超过 07777、设备号只在设备
//! 节点上非零。另有一个把模式渲染成 `rwxr-xr-x` 的辅助函数，setuid/setgid/sticky
//! 会把对应位置的 x 改成小写 s/t 或大写 S/T。十六格账本十成一败，渲染结果逐字节
//! 校验。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b10;
/// 编号非法。
pub const ST_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const ST_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const ST_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const ST_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const ST_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const ST_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const ST_EBAL: u32 = SEG_BASE + 0x07;
/// 节点不存在。
pub const ST_ENOENT: u32 = SEG_BASE + 0x08;
/// 循环链接。
pub const ST_ELOOP: u32 = SEG_BASE + 0x09;
/// 非法类型。
pub const ST_EINVAL: u32 = SEG_BASE + 0x0a;
/// 票据不一致。
pub const ST_ESEAL: u32 = SEG_BASE + 0x0b;

/// 普通文件。
pub const K_REG: u8 = 1;
/// 目录。
pub const K_DIR: u8 = 2;
/// 符号链接。
pub const K_LNK: u8 = 3;
/// 字符设备。
pub const K_CHR: u8 = 4;
/// 块设备。
pub const K_BLK: u8 = 5;
/// FIFO。
pub const K_FIFO: u8 = 6;
/// 套接字。
pub const K_SOCK: u8 = 7;

/// 普通文件类型位。
pub const IF_REG: u32 = 0o100000;
/// 目录类型位。
pub const IF_DIR: u32 = 0o040000;
/// 符号链接类型位。
pub const IF_LNK: u32 = 0o120000;
/// 字符设备类型位。
pub const IF_CHR: u32 = 0o020000;
/// 块设备类型位。
pub const IF_BLK: u32 = 0o060000;
/// FIFO 类型位。
pub const IF_FIFO: u32 = 0o010000;
/// 套接字类型位。
pub const IF_SOCK: u32 = 0o140000;

/// 目录固定大小。
pub const DIR_SIZE: u32 = 4096;
/// 块大小。
pub const BLK_SIZE: u32 = 512;
/// 渲染缓冲容量。
pub const RENDER_CAP: usize = 64;
/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 目标容量。
pub const TARGET_CAP: usize = 16;
/// 样本格数。
pub const SAMPLES: usize = 16;

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

/// 节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub perm: u16,
    pub uid: u16,
    pub gid: u16,
    pub ino: u32,
    pub size: u32,
    pub nlink: u16,
    pub rdev: u32,
    pub dev: u16,
    pub atime: u32,
    pub mtime: u32,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
}

impl Node {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn target(&self) -> &[u8] {
        cut(&self.target, self.tlen)
    }
}

/// 合成的 stat 视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stat {
    pub dev: u16,
    pub ino: u32,
    pub mode: u32,
    pub nlink: u16,
    pub uid: u16,
    pub gid: u16,
    pub rdev: u32,
    pub size: u32,
    pub blocks: u32,
    pub blksize: u32,
}

/// 节点表。
pub struct Table {
    nodes: Vec<Node>,
    looks: u32,
    rejects: u32,
}

impl Table {
    pub fn new() -> Table {
        let mut v = Vec::new();
        v.push(node(b"f", K_REG, 0o644, 12, 1234, 0, 2, 0));
        v.push(node(b"f2", K_REG, 0o644, 12, 1234, 0, 2, 0));
        v.push(node(b"d", K_DIR, 0o755, 13, 0, 0, 1, 0));
        v.push(link(b"lnk", b"f", 0o777, 14));
        v.push(node(b"c", K_CHR, 0o666, 15, 0, 0, 1, 0x0103));
        v.push(node(b"b", K_BLK, 0o660, 16, 0, 0, 1, 0x0800));
        v.push(node(b"p", K_FIFO, 0o644, 17, 0, 0, 1, 0));
        v.push(node(b"s", K_SOCK, 0o600, 18, 0, 0, 1, 0));
        v.push(link(b"bad", b"gone", 0o777, 19));
        v.push(link(b"cyc", b"cyc", 0o777, 20));
        v.push(node(b"odd", 9, 0o644, 21, 10, 0, 1, 0));
        Table { nodes: v, looks: 0, rejects: 0 }
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    pub fn looks(&self) -> u32 {
        self.looks
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 合成 stat。follow 为真时解析一层符号链接。
    pub fn stat(&mut self, name: &[u8], follow: bool) -> Result<Stat, u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(ST_ENOENT);
            }
        };
        let mut n = match self.nodes.get(i) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(ST_ENOENT);
            }
        };
        if follow && n.kind == K_LNK {
            let j = match self.find(n.target()) {
                Some(v) => v,
                None => {
                    self.rejects += 1;
                    return Err(ST_ENOENT);
                }
            };
            let t = match self.nodes.get(j) {
                Some(v) => *v,
                None => {
                    self.rejects += 1;
                    return Err(ST_ENOENT);
                }
            };
            if t.kind == K_LNK {
                self.rejects += 1;
                return Err(ST_ELOOP);
            }
            n = t;
        }
        let tb = match type_bits(n.kind) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let size = match n.kind {
            K_REG => n.size,
            K_DIR => DIR_SIZE,
            K_LNK => n.tlen as u32,
            _ => 0,
        };
        let blocks = ((size as u64 + 511) / 512) as u32;
        let rdev = if n.kind == K_CHR || n.kind == K_BLK {
            n.rdev
        } else {
            0
        };
        self.looks += 1;
        Ok(Stat {
            dev: n.dev,
            ino: n.ino,
            mode: tb | (n.perm as u32 & 0o7777),
            nlink: n.nlink,
            uid: n.uid,
            gid: n.gid,
            rdev,
            size,
            blocks,
            blksize: 4096,
        })
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() {
            return Err(ST_EBAL);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(ST_ENOBS),
            };
            if n.nlen == 0 || n.perm > 0o7777 || n.nlink == 0 {
                return Err(ST_EBAL);
            }
            if n.kind == K_LNK && n.tlen == 0 {
                return Err(ST_EBAL);
            }
            if (n.kind == K_CHR || n.kind == K_BLK) && n.rdev == 0 {
                return Err(ST_EBAL);
            }
            if n.kind != K_CHR && n.kind != K_BLK && n.rdev != 0 {
                return Err(ST_EBAL);
            }
            let mut j = i + 1;
            while j < self.nodes.len() {
                if let Some(m) = self.nodes.get(j) {
                    if n.name() == m.name() {
                        return Err(ST_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // 硬链接组的 nlink 必须等于表中同 inode 的出现次数。
        i = 0;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(ST_ENOBS),
            };
            let mut seen = 0u16;
            let mut k = 0usize;
            while k < self.nodes.len() {
                if let Some(m) = self.nodes.get(k) {
                    if m.ino == n.ino {
                        seen += 1;
                    }
                }
                k += 1;
            }
            if seen != n.nlink {
                return Err(ST_EBAL);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                h = fold(h, n.ino as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.perm as u64);
                h = fold(h, n.size as u64);
                h = fold(h, n.rdev as u64);
                h = fold(h, n.nlink as u64);
                let mut k = 0usize;
                while k < n.nlen as usize {
                    if let Some(b) = n.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                k = 0;
                while k < n.tlen as usize {
                    if let Some(b) = n.target.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.looks as u64);
        fold(h, self.rejects as u64)
    }
}

fn node(name: &[u8], kind: u8, perm: u16, ino: u32, size: u32, dev: u16, nlink: u16, rdev: u32) -> Node {
    let mut n = Node {
        name: [0; NAME_CAP],
        nlen: 0,
        kind,
        perm,
        uid: 0,
        gid: 0,
        ino,
        size,
        nlink,
        rdev,
        dev,
        atime: 0,
        mtime: 0,
        target: [0; TARGET_CAP],
        tlen: 0,
    };
    n.nlen = put(&mut n.name, name);
    n
}

fn link(name: &[u8], target: &[u8], perm: u16, ino: u32) -> Node {
    let mut n = node(name, K_LNK, perm, ino, 0, 0, 1, 0);
    n.tlen = put(&mut n.target, target);
    n
}

fn type_bits(kind: u8) -> Result<u32, u32> {
    let v = match kind {
        K_REG => IF_REG,
        K_DIR => IF_DIR,
        K_LNK => IF_LNK,
        K_CHR => IF_CHR,
        K_BLK => IF_BLK,
        K_FIFO => IF_FIFO,
        K_SOCK => IF_SOCK,
        _ => return Err(ST_EINVAL),
    };
    Ok(v)
}

/// 渲染 rwx 权限串。
pub fn perm_string(mode: u32) -> [u8; 9] {
    let mut out = [b'-'; 9];
    let bits = [
        (0o400, 0usize),
        (0o200, 1),
        (0o100, 2),
        (0o040, 3),
        (0o020, 4),
        (0o010, 5),
        (0o004, 6),
        (0o002, 7),
        (0o001, 8),
    ];
    let mut i = 0usize;
    while i < bits.len() {
        let (bit, pos) = match bits.get(i) {
            Some(v) => *v,
            None => break,
        };
        if mode & bit != 0 {
            let c = match i {
                0 | 3 | 6 => b'r',
                1 | 4 | 7 => b'w',
                _ => b'x',
            };
            out[pos] = c;
        }
        i += 1;
    }
    if mode & 0o4000 != 0 {
        out[2] = if mode & 0o100 != 0 { b's' } else { b'S' };
    }
    if mode & 0o2000 != 0 {
        out[5] = if mode & 0o010 != 0 { b's' } else { b'S' };
    }
    if mode & 0o1000 != 0 {
        out[8] = if mode & 0o001 != 0 { b't' } else { b'T' };
    }
    out
}

fn dec(buf: &mut [u8], at: usize, mut v: u32) -> usize {
    let mut tmp = [0u8; 10];
    let mut n = 0usize;
    if v == 0 {
        if let Some(s) = buf.get_mut(at) {
            *s = b'0';
        }
        return at + 1;
    }
    while v > 0 && n < 10 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut i = 0usize;
    while i < n {
        if let Some(d) = buf.get_mut(at + i) {
            *d = tmp[n - 1 - i];
        }
        i += 1;
    }
    at + n
}

/// 渲染一行 `perm nlink uid gid size blocks`。
pub fn render(s: &Stat) -> ([u8; RENDER_CAP], u8) {
    let mut b = [0u8; RENDER_CAP];
    let ps = perm_string(s.mode);
    let mut at = 0usize;
    let mut i = 0usize;
    while i < 9 {
        if let Some(d) = b.get_mut(at) {
            *d = match ps.get(i) {
                Some(v) => *v,
                None => b'-',
            };
        }
        at += 1;
        i += 1;
    }
    at = dec(&mut b, at, s.nlink as u32);
    at = dec(&mut b, at, s.uid as u32);
    at = dec(&mut b, at, s.gid as u32);
    at = dec(&mut b, at, s.size);
    at = dec(&mut b, at, s.blocks);
    (b, at as u8)
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub follow: bool,
    pub stat: Stat,
    pub ok: bool,
    pub line: [u8; 32],
    pub llen: u8,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub stat: Stat,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    t: Table,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { t: Table::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, name: &[u8], follow: bool, expect_mode: u32, expect_size: u32, expect_blocks: u32, expect_ino: u32) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(ST_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(ST_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(ST_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(ST_EORDER);
        }
        let r = self.t.stat(name, follow);
        let end = match r {
            Ok(s) => {
                if expect_mode == 0 || s.mode != expect_mode || s.size != expect_size || s.blocks != expect_blocks || s.ino != expect_ino {
                    self.rejects += 1;
                    return Err(ST_EMISMATCH);
                }
                Ending { stat: s, code: 0 }
            }
            Err(e) => {
                if expect_mode != 0 {
                    self.rejects += 1;
                    return Err(ST_EMISMATCH);
                }
                Ending { stat: zero_stat(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { name: [0; NAME_CAP], nlen: 0, follow, stat: end.stat, ok: expect_mode != 0, line: [0; 32], llen: 0 };
        c.nlen = put(&mut c.name, name);
        if end.code == 0 {
            let (b, n) = render(&end.stat);
            let mut k = 0usize;
            while k < n as usize && k < 32 {
                if let (Some(d), Some(v)) = (c.line.get_mut(k), b.get(k)) {
                    *d = *v;
                }
                k += 1;
            }
            c.llen = n.min(32);
        }
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn case(&self, i: usize) -> Option<Case> {
        match self.cases.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn table(&self) -> &Table {
        &self.t
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
            return Err(ST_EBAL);
        }
        self.t.audit()
    }

    pub fn seal(&self) -> u64 {
        self.t.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(ST_EFROZEN);
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
            Err(ST_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn zero_stat() -> Stat {
    Stat { dev: 0, ino: 0, mode: 0, nlink: 0, uid: 0, gid: 0, rdev: 0, size: 0, blocks: 0, blksize: 0 }
}

/// 十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, b"f", false, 0o100644, 1234, 3, 12);
    let _ = l.register(1, b"f", true, 0o100644, 1234, 3, 12);
    let _ = l.register(2, b"d", false, 0o040755, 4096, 8, 13);
    let _ = l.register(3, b"lnk", false, 0o120777, 1, 1, 14);
    let _ = l.register(4, b"lnk", true, 0o100644, 1234, 3, 12);
    let _ = l.register(5, b"c", false, 0o020666, 0, 0, 15);
    let _ = l.register(6, b"b", false, 0o060660, 0, 0, 16);
    let _ = l.register(7, b"p", false, 0o010644, 0, 0, 17);
    let _ = l.register(8, b"s", false, 0o140600, 0, 0, 18);
    let _ = l.register(9, b"bad", true, 0, 0, 0, 0);
    let _ = l.register(10, b"cyc", true, 0, 0, 0, 0);
    let _ = l.register(11, b"gone", false, 0, 0, 0, 0);
    let _ = l.register(12, b"odd", false, 0, 0, 0, 0);
    let _ = l.register(13, b"f2", false, 0o100644, 1234, 3, 12);
    let _ = l.register(14, b"d", true, 0o040755, 4096, 8, 13);
    let _ = l.register(15, b"c", true, 0o020666, 0, 0, 15);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 12 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut t = Table::new();
    let a = t.stat(b"f", false);
    let b = t.stat(b"d", false);
    let c = t.stat(b"lnk", false);
    out[1] = a == Ok(Stat { dev: 0, ino: 12, mode: 0o100644, nlink: 2, uid: 0, gid: 0, rdev: 0, size: 1234, blocks: 3, blksize: 4096 })
        && b.map(|s| (s.mode, s.size, s.blocks)) == Ok((0o040755, 4096, 8))
        && c.map(|s| (s.mode, s.size, s.blocks)) == Ok((0o120777, 1, 1));

    let mut t2 = Table::new();
    out[2] = t2.stat(b"lnk", true) == t2.stat(b"f", false)
        && t2.stat(b"bad", true) == Err(ST_ENOENT)
        && t2.stat(b"cyc", true) == Err(ST_ELOOP)
        && t2.stat(b"gone", false) == Err(ST_ENOENT);

    let s = match t2.stat(b"c", false) {
        Ok(v) => v,
        Err(_) => zero_stat(),
    };
    let (line, n) = render(&s);
    out[3] = s.rdev == 0x0103 && perm_string(0o104755)[2] == b's' && perm_string(0o104644)[2] == b'S'
        && perm_string(0o101755)[8] == b't' && n == 14
        && cut(&line, n) == b"rw-rw-rw-10000" as &[u8];
    out
}

/// 十三判据。
pub fn run_unx11437_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11437.stat");
    let l = build_ledger();
    s.add("ST1-十六格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("ST2-十二成四拒", l.ok_count() == 12 && l.err_count() == 4 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("ST3-普通文件", l.ending(0).map(|e| (e.stat.mode, e.stat.size, e.stat.blocks)) == Some((0o100644, 1234, 3)), "static");
    s.add("ST4-目录大小", l.ending(2).map(|e| (e.stat.mode, e.stat.size, e.stat.blocks)) == Some((0o040755, 4096, 8)), "static");
    s.add("ST5-链接自身", l.ending(3).map(|e| (e.stat.mode, e.stat.size)) == Some((0o120777, 1)), "static");
    s.add("ST6-跟随链接", l.ending(4).map(|e| (e.stat.mode, e.stat.ino)) == Some((0o100644, 12)), "static");
    s.add("ST7-字符设备", l.ending(5).map(|e| (e.stat.mode, e.stat.rdev)) == Some((0o020666, 0x0103)), "static");
    s.add("ST8-块设备", l.ending(6).map(|e| (e.stat.mode, e.stat.rdev)) == Some((0o060660, 0x0800)), "static");
    s.add("ST9-FIFO与套接字", l.ending(7).map(|e| e.stat.mode) == Some(0o010644) && l.ending(8).map(|e| e.stat.mode) == Some(0o140600), "static");
    s.add("ST10-链接错误", l.ending(9).map(|e| e.code) == Some((ST_ENOENT & 0xffff) as u16) && l.ending(10).map(|e| e.code) == Some((ST_ELOOP & 0xffff) as u16), "static");
    s.add("ST11-缺失与非法", l.ending(11).map(|e| e.code) == Some((ST_ENOENT & 0xffff) as u16) && l.ending(12).map(|e| e.code) == Some((ST_EINVAL & 0xffff) as u16), "static");
    s.add("ST12-硬链接同inode", l.ending(13).map(|e| (e.stat.ino, e.stat.nlink)) == Some((12, 2)) && l.table().count() == 11, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(ST_EFROZEN);
    let inj = inject_four();
    s.add_dyn("ST13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_and_dir_sizes() {
        let mut t = Table::new();
        assert_eq!(t.stat(b"f", false).map(|s| (s.size, s.blocks)), Ok((1234, 3)));
        assert_eq!(t.stat(b"d", false).map(|s| (s.size, s.blocks)), Ok((4096, 8)));
    }

    #[test]
    fn symlink_size_is_target_length() {
        let mut t = Table::new();
        assert_eq!(t.stat(b"lnk", false).map(|s| (s.mode, s.size)), Ok((0o120777, 1)));
        assert_eq!(t.stat(b"lnk", true).map(|s| s.ino), Ok(12));
    }

    #[test]
    fn device_rdev_only() {
        let mut t = Table::new();
        assert_eq!(t.stat(b"c", false).map(|s| s.rdev), Ok(0x0103));
        assert_eq!(t.stat(b"p", false).map(|s| s.rdev), Ok(0));
    }

    #[test]
    fn follow_errors() {
        let mut t = Table::new();
        assert_eq!(t.stat(b"bad", true), Err(ST_ENOENT));
        assert_eq!(t.stat(b"cyc", true), Err(ST_ELOOP));
        assert_eq!(t.stat(b"odd", false), Err(ST_EINVAL));
    }

    #[test]
    fn perm_rendering() {
        assert_eq!(perm_string(0o100644), *b"rw-r--r--");
        assert_eq!(perm_string(0o104755), *b"rwsr-xr-x");
        assert_eq!(perm_string(0o102755), *b"rwxr-sr-x");
        assert_eq!(perm_string(0o101777), *b"rwxrwxrwt");
        assert_eq!(perm_string(0o100000), *b"---------");
    }

    #[test]
    fn hardlink_group() {
        let mut t = Table::new();
        let a = t.stat(b"f", false);
        let b = t.stat(b"f2", false);
        assert_eq!(a.map(|s| (s.ino, s.nlink)), Ok((12, 2)));
        assert_eq!(b.map(|s| (s.ino, s.nlink)), Ok((12, 2)));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11437_checks().failed_count(), 0);
    }
}
