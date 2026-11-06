//! UNX-F11438 truncate 稀疏文件（0x20b20 段）
//!
//! 文件在本模型里由“逻辑大小 + 已分配块集合”组成，块号是 512 字节为单位的下标。
//! truncate 缩小文件时先算出新的块上界 `ceil(size/512)`，把所有越界块从集合里删掉，
//! 落在边界块内的部分数据保留整块（模型不模拟块内零填充），因此 1000 字节的文件
//! 仍然占两个块。truncate 扩大文件只改逻辑大小，不分配任何块，缺口读出来就是零，
//! 这就是稀疏文件的核心：st_size 增长而 st_blocks 不变。
//!
//! 写操作按区间覆盖块：起始偏移与结束偏移分别除以 512 得到首末块号，区间内每个块
//! 都插入集合，若写越过了当前大小则顺带扩展大小。块集合始终有序且去重，审计会
//! 重新验证这一点。punch hole 只删除覆盖给定偏移的那一个块，文件大小不变，洞在
//! 逻辑上仍然可读为零；对超出文件末尾的偏移打洞返回 EINVAL。
//!
//! 边界矩阵有三类。大小上限 EFBIG 同时约束 truncate 与 write 的终点；每文件块数
//! 上限 64，超出返回 ENOSPC 且不产生部分分配（模型先数块再插入）；只读文件返回
//! EACCES，缺失名字返回 ENOENT，非法打洞返回 EINVAL。truncate 到当前大小是合法
//! 的空操作，仍计入成功格与操作计数，但不改变块集合。
//!
//! 账本十五格十成一败，前六格演示“缩小删块、扩大留洞、写回分配、打洞去块”的
//! 完整往返，后五格覆盖错误路径。审计要求块号严格递增、块号小于块上界、大小不
//! 超过上限、只读标志不被写操作绕过，并且操作计数与日志长度一致。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b20;
/// 编号非法。
pub const TR_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const TR_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TR_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TR_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const TR_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const TR_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TR_EBAL: u32 = SEG_BASE + 0x07;
/// 节点不存在。
pub const TR_ENOENT: u32 = SEG_BASE + 0x08;
/// 只读文件。
pub const TR_EACCES: u32 = SEG_BASE + 0x09;
/// 文件过大。
pub const TR_EFBIG: u32 = SEG_BASE + 0x0a;
/// 参数非法。
pub const TR_EINVAL: u32 = SEG_BASE + 0x0b;
/// 块空间不足。
pub const TR_ENOSPC: u32 = SEG_BASE + 0x0c;
/// 票据不一致。
pub const TR_ESEAL: u32 = SEG_BASE + 0x0d;

/// 操作：截断。
pub const OP_TRUNC: u8 = 1;
/// 操作：写入。
pub const OP_WRITE: u8 = 2;
/// 操作：打洞。
pub const OP_PUNCH: u8 = 3;

/// 块大小。
pub const BLK: u32 = 512;
/// 文件大小上限。
pub const MAX_SIZE: u32 = 1 << 20;
/// 每文件块数上限。
pub const MAX_BLOCKS: usize = 64;
/// 名字容量。
pub const NAME_CAP: usize = 8;
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

fn block_ceil(size: u32) -> u32 {
    (size + BLK - 1) / BLK
}

/// 一个稀疏文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sparse {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub ro: bool,
    pub size: u32,
    pub blocks: Vec<u16>,
}

impl Sparse {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn last(&self) -> u16 {
        match self.blocks.last() {
            Some(v) => *v,
            None => 0,
        }
    }
}

/// 一个操作的结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub size: u32,
    pub blocks: u8,
    pub last: u16,
}

/// 卷。
pub struct Vol {
    files: Vec<Sparse>,
    journal: Vec<u8>,
    writes: u32,
    truncs: u32,
    punches: u32,
    rejects: u32,
}

impl Vol {
    pub fn new() -> Vol {
        let mut files = Vec::new();
        files.push(fresh(b"a", 4096, false));
        files.push(fresh(b"b", 0, false));
        files.push(fresh(b"ro", 1024, true));
        let mut v = Vol { files, journal: Vec::new(), writes: 0, truncs: 0, punches: 0, rejects: 0 };
        let mut i = 0u16;
        while i < 8 {
            if let Some(f) = v.files.get_mut(0) {
                f.blocks.push(i);
            }
            if let Some(f) = v.files.get_mut(2) {
                f.blocks.push(i);
            }
            i += 1;
        }
        if let Some(f) = v.files.get_mut(2) {
            f.size = 1024;
            f.blocks.truncate(2);
        }
        v
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                if f.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn file(&self, i: usize) -> Option<&Sparse> {
        self.files.get(i)
    }

    pub fn journal(&self) -> &[u8] {
        &self.journal
    }

    pub fn counts(&self) -> (u32, u32, u32) {
        (self.truncs, self.writes, self.punches)
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    fn snapshot(&self, i: usize) -> Result<Out, u32> {
        match self.files.get(i) {
            Some(f) => Ok(Out { size: f.size, blocks: f.blocks.len() as u8, last: f.last() }),
            None => Err(TR_ENOENT),
        }
    }

    fn insert_block(f: &mut Sparse, b: u16) -> bool {
        if f.blocks.len() >= MAX_BLOCKS && !f.blocks.contains(&b) {
            return false;
        }
        if f.blocks.contains(&b) {
            return true;
        }
        f.blocks.push(b);
        let mut i = f.blocks.len();
        while i > 1 {
            let cur = match f.blocks.get(i - 1) {
                Some(v) => *v,
                None => break,
            };
            let prev = match f.blocks.get(i - 2) {
                Some(v) => *v,
                None => break,
            };
            if prev <= cur {
                break;
            }
            f.blocks.swap(i - 1, i - 2);
            i -= 1;
        }
        true
    }

    /// 截断。
    pub fn truncate(&mut self, name: &[u8], size: u32) -> Result<Out, u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(TR_ENOENT);
            }
        };
        if size > MAX_SIZE {
            self.rejects += 1;
            return Err(TR_EFBIG);
        }
        if let Some(f) = self.files.get(i) {
            if f.ro {
                self.rejects += 1;
                return Err(TR_EACCES);
            }
        }
        if let Some(f) = self.files.get_mut(i) {
            f.size = size;
            let keep = block_ceil(size);
            f.blocks.retain(|b| (*b as u32) < keep);
        }
        self.truncs += 1;
        self.journal.push(OP_TRUNC);
        self.snapshot(i)
    }

    /// 在偏移处写入长度，必要时扩展大小。
    pub fn write(&mut self, name: &[u8], off: u32, len: u32) -> Result<Out, u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(TR_ENOENT);
            }
        };
        if len == 0 {
            self.writes += 1;
            self.journal.push(OP_WRITE);
            return self.snapshot(i);
        }
        let end = match off.checked_add(len) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(TR_EFBIG);
            }
        };
        if end > MAX_SIZE {
            self.rejects += 1;
            return Err(TR_EFBIG);
        }
        if let Some(f) = self.files.get(i) {
            if f.ro {
                self.rejects += 1;
                return Err(TR_EACCES);
            }
        }
        let first = off / BLK;
        let last = (end - 1) / BLK;
        let need = (last - first + 1) as usize;
        let have = match self.files.get(i) {
            Some(f) => {
                let mut n = 0usize;
                let mut k = first;
                while k <= last {
                    if !f.blocks.contains(&(k as u16)) {
                        n += 1;
                    }
                    k += 1;
                }
                n
            }
            None => 0,
        };
        let cur = match self.files.get(i) {
            Some(f) => f.blocks.len(),
            None => 0,
        };
        if cur + have > MAX_BLOCKS || need > MAX_BLOCKS {
            self.rejects += 1;
            return Err(TR_ENOSPC);
        }
        if let Some(f) = self.files.get_mut(i) {
            let mut k = first;
            while k <= last {
                if !Self::insert_block(f, k as u16) {
                    self.rejects += 1;
                    return Err(TR_ENOSPC);
                }
                k += 1;
            }
            if end > f.size {
                f.size = end;
            }
        }
        self.writes += 1;
        self.journal.push(OP_WRITE);
        self.snapshot(i)
    }

    /// 打洞：删除覆盖该偏移的块。
    pub fn punch(&mut self, name: &[u8], off: u32) -> Result<Out, u32> {
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(TR_ENOENT);
            }
        };
        if let Some(f) = self.files.get(i) {
            if f.ro {
                self.rejects += 1;
                return Err(TR_EACCES);
            }
            if off >= f.size {
                self.rejects += 1;
                return Err(TR_EINVAL);
            }
        }
        if let Some(f) = self.files.get_mut(i) {
            let b = (off / BLK) as u16;
            f.blocks.retain(|x| *x != b);
        }
        self.punches += 1;
        self.journal.push(OP_PUNCH);
        self.snapshot(i)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.files.is_empty() {
            return Err(TR_EBAL);
        }
        let ops = (self.truncs + self.writes + self.punches) as usize;
        if ops != self.journal.len() {
            return Err(TR_EBAL);
        }
        let mut i = 0usize;
        while i < self.files.len() {
            let f = match self.files.get(i) {
                Some(v) => v,
                None => return Err(TR_ENOBS),
            };
            if f.nlen == 0 || f.size > MAX_SIZE || f.blocks.len() > MAX_BLOCKS {
                return Err(TR_EBAL);
            }
            let keep = block_ceil(f.size);
            let mut prev: i32 = -1;
            let mut k = 0usize;
            while k < f.blocks.len() {
                let b = match f.blocks.get(k) {
                    Some(v) => *v as i32,
                    None => return Err(TR_ENOBS),
                };
                if b <= prev || (b as u32) >= keep {
                    return Err(TR_EBAL);
                }
                prev = b;
                k += 1;
            }
            let mut j = i + 1;
            while j < self.files.len() {
                if let Some(g) = self.files.get(j) {
                    if f.name() == g.name() {
                        return Err(TR_EBAL);
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
        while i < self.files.len() {
            if let Some(f) = self.files.get(i) {
                h = fold(h, f.size as u64);
                h = fold(h, f.ro as u64);
                h = fold(h, f.blocks.len() as u64);
                let mut k = 0usize;
                while k < f.blocks.len() {
                    if let Some(b) = f.blocks.get(k) {
                        h = fold(h, *b as u64);
                    }
                    k += 1;
                }
                let mut m = 0usize;
                while m < f.nlen as usize {
                    if let Some(b) = f.name.get(m) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    m += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.journal.len() as u64);
        h = fold(h, self.truncs as u64);
        h = fold(h, self.writes as u64);
        h = fold(h, self.punches as u64);
        fold(h, self.rejects as u64)
    }
}

fn fresh(name: &[u8], size: u32, ro: bool) -> Sparse {
    let mut f = Sparse { name: [0; NAME_CAP], nlen: 0, ro, size, blocks: Vec::new() };
    f.nlen = put(&mut f.name, name);
    f
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub op: u8,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub a: u32,
    pub b: u32,
    pub expect: Out,
    pub ok: bool,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub out: Out,
    pub code: u16,
}

fn zero_out() -> Out {
    Out { size: 0, blocks: 0, last: 0 }
}

/// 账本。
pub struct Ledger {
    v: Vol,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { v: Vol::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, op: u8, name: &[u8], a: u32, b: u32, expect: Out, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TR_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(TR_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(TR_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(TR_EORDER);
        }
        let r = if op == OP_TRUNC {
            self.v.truncate(name, a)
        } else if op == OP_WRITE {
            self.v.write(name, a, b)
        } else if op == OP_PUNCH {
            self.v.punch(name, a)
        } else {
            Err(TR_EINVAL)
        };
        let end = match r {
            Ok(o) => {
                if !ok || o != expect {
                    self.rejects += 1;
                    return Err(TR_EMISMATCH);
                }
                Ending { out: o, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(TR_EMISMATCH);
                }
                Ending { out: zero_out(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { op, name: [0; NAME_CAP], nlen: 0, a, b, expect, ok };
        c.nlen = put(&mut c.name, name);
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

    pub fn vol(&self) -> &Vol {
        &self.v
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
            return Err(TR_EBAL);
        }
        self.v.audit()
    }

    pub fn seal(&self) -> u64 {
        self.v.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TR_EFROZEN);
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
            Err(TR_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn o(size: u32, blocks: u8, last: u16) -> Out {
    Out { size, blocks, last }
}

/// 十五格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eac = (TR_EACCES & 0xffff) as u16;
    let eno = (TR_ENOENT & 0xffff) as u16;
    let efb = (TR_EFBIG & 0xffff) as u16;
    let ein = (TR_EINVAL & 0xffff) as u16;
    let _ = l.register(0, OP_TRUNC, b"a", 1000, 0, o(1000, 2, 1), true);
    let _ = l.register(1, OP_WRITE, b"a", 1024, 1024, o(2048, 4, 3), true);
    let _ = l.register(2, OP_TRUNC, b"a", 0, 0, o(0, 0, 0), true);
    let _ = l.register(3, OP_TRUNC, b"a", 5000, 0, o(5000, 0, 0), true);
    let _ = l.register(4, OP_WRITE, b"a", 0, 512, o(5000, 1, 0), true);
    let _ = l.register(5, OP_TRUNC, b"a", 4096, 0, o(4096, 1, 0), true);
    let _ = l.register(6, OP_TRUNC, b"a", 4096, 0, o(4096, 1, 0), true);
    let _ = l.register(7, OP_PUNCH, b"a", 0, 0, o(4096, 0, 0), true);
    let _ = l.register(8, OP_WRITE, b"b", 0, 4096, o(4096, 8, 7), true);
    let _ = l.register(9, OP_TRUNC, b"b", 2048, 0, o(2048, 4, 3), true);
    let _ = l.register(10, OP_TRUNC, b"ro", 0, 0, zero_out(), false);
    let _ = l.register(11, OP_TRUNC, b"gone", 0, 0, zero_out(), false);
    let _ = l.register(12, OP_TRUNC, b"b", MAX_SIZE + 1, 0, zero_out(), false);
    let _ = l.register(13, OP_WRITE, b"a", MAX_SIZE, 1, zero_out(), false);
    let _ = l.register(14, OP_PUNCH, b"a", 4096, 0, zero_out(), false);
    let _ = (eac, eno, efb, ein);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 10 && l.err_count() == 5 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut v = Vol::new();
    let t1 = v.truncate(b"a", 3000);
    let t2 = v.truncate(b"a", 6000);
    out[1] = t1 == Ok(o(3000, 6, 5)) && t2 == Ok(o(6000, 6, 5)) && v.file(0).map(|f| f.size) == Some(6000);

    let mut w = Vol::new();
    let w1 = w.write(b"b", 5120, 512);
    let w2 = w.write(b"b", 5120, 512);
    out[2] = w1 == Ok(o(5632, 1, 10)) && w2 == Ok(o(5632, 1, 10)) && w.journal() == [OP_WRITE, OP_WRITE];

    let mut p = Vol::new();
    let huge = p.write(b"b", 0, 40000);
    let ro = p.truncate(b"ro", 0);
    out[3] = huge == Err(TR_ENOSPC) && ro == Err(TR_EACCES) && p.truncate(b"b", 0).is_ok();
    out
}

/// 十三判据。
pub fn run_unx11438_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11438.truncate");
    let l = build_ledger();
    s.add("TR1-十五格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("TR2-十成五拒", l.ok_count() == 10 && l.err_count() == 5 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("TR3-缩小删块", l.ending(0).map(|e| e.out) == Some(o(1000, 2, 1)), "static");
    s.add("TR4-写回分配", l.ending(1).map(|e| e.out) == Some(o(2048, 4, 3)), "static");
    s.add("TR5-清零", l.ending(2).map(|e| e.out) == Some(o(0, 0, 0)), "static");
    s.add("TR6-扩大留洞", l.ending(3).map(|e| e.out) == Some(o(5000, 0, 0)), "static");
    s.add("TR7-洞中写块", l.ending(4).map(|e| e.out) == Some(o(5000, 1, 0)), "static");
    s.add("TR8-空操作截断", l.ending(5).map(|e| e.out) == Some(o(4096, 1, 0)) && l.ending(6).map(|e| e.out) == Some(o(4096, 1, 0)), "static");
    s.add("TR9-打洞", l.ending(7).map(|e| e.out) == Some(o(4096, 0, 0)), "static");
    s.add("TR10-连续区间", l.ending(8).map(|e| e.out) == Some(o(4096, 8, 7)) && l.ending(9).map(|e| e.out) == Some(o(2048, 4, 3)), "static");
    s.add("TR11-只读与缺失", l.ending(10).map(|e| e.code) == Some((TR_EACCES & 0xffff) as u16) && l.ending(11).map(|e| e.code) == Some((TR_ENOENT & 0xffff) as u16), "static");
    s.add("TR12-越界与打洞", l.ending(12).map(|e| e.code) == Some((TR_EFBIG & 0xffff) as u16) && l.ending(13).map(|e| e.code) == Some((TR_EFBIG & 0xffff) as u16) && l.ending(14).map(|e| e.code) == Some((TR_EINVAL & 0xffff) as u16) && l.vol().counts() == (6, 3, 1) && l.vol().journal().len() == 10, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TR_EFROZEN);
    let inj = inject_four();
    s.add_dyn("TR13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_keeps_partial_block() {
        let mut v = Vol::new();
        assert_eq!(v.truncate(b"a", 1000), Ok(o(1000, 2, 1)));
        assert_eq!(v.truncate(b"a", 1), Ok(o(1, 1, 0)));
        assert_eq!(v.truncate(b"a", 0), Ok(o(0, 0, 0)));
    }

    #[test]
    fn grow_is_sparse() {
        let mut v = Vol::new();
        assert_eq!(v.truncate(b"a", 0), Ok(o(0, 0, 0)));
        assert_eq!(v.truncate(b"a", 100000), Ok(o(100000, 0, 0)));
    }

    #[test]
    fn write_allocates_and_extends() {
        let mut v = Vol::new();
        assert_eq!(v.write(b"b", 0, 4096), Ok(o(4096, 8, 7)));
        assert_eq!(v.write(b"b", 4096, 512), Ok(o(4608, 9, 8)));
    }

    #[test]
    fn punch_removes_one_block() {
        let mut v = Vol::new();
        assert_eq!(v.punch(b"a", 1024), Ok(o(4096, 7, 7)));
        assert_eq!(v.punch(b"a", 999999), Err(TR_EINVAL));
    }

    #[test]
    fn limits() {
        let mut v = Vol::new();
        assert_eq!(v.truncate(b"a", MAX_SIZE + 1), Err(TR_EFBIG));
        assert_eq!(v.write(b"b", 0, 40000), Err(TR_ENOSPC));
        assert_eq!(v.truncate(b"ro", 0), Err(TR_EACCES));
        assert_eq!(v.truncate(b"none", 0), Err(TR_ENOENT));
    }

    #[test]
    fn zero_write_is_noop() {
        let mut v = Vol::new();
        assert_eq!(v.write(b"b", 12345, 0), Ok(o(0, 0, 0)));
        assert_eq!(v.journal().len(), 1);
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11438_checks().failed_count(), 0);
    }
}
