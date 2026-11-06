//! UNX-F11430 mv 跨设备迁移（0x20aa0 段）
//!
//! `rename` 只改目录项，一旦源与目标不在同一设备就返回 EXDEV，用户态 `mv` 必须
//! 退化成“复制内容 → 校验 → 删源 → 落账”的四阶段流水线。本模型把流水线做成
//! 显式日志：COPY、VERIFY、UNLINK、COMMIT 各占一格，任何失败都必须停在某个
//! 阶段之前，并保证源文件仍可读、目标不留半成品。
//!
//! 空间是这条路径上最容易出事的地方。模型给每个设备维护剩余块数：COPY 之前先
//! 检查配额，不足则只写 COPY 阶段并返回 ENOSPC，源不删、目标不建；若写入后校验
//! 发现内容损坏，则返回 EIO，丢弃目标半成品并让源保持原位。只有四阶段全部走完
//! 才允许源名字消失。
//!
//! 硬链接让“删源”并不等于“释放 inode”：nlink 大于 1 时只摘掉一个名字，inode
//! 仍被其它目录项引用，freed 计数不应增加。模型用相邻两格对照这一差别，并把
//! nlink 的迁移写进签名，防止把“删名字”误当成“删数据”。
//!
//! 同设备的请求直接走改名快路径：日志为空、复制字节数为零、返回 0。账本十四格
//! 中五格成功（含一次同设备快路径与一次覆盖目标），九格失败覆盖缺失、权限、
//! 目录、配额、重名、保护目标、损坏与重复迁移。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20aa0;
/// 编号非法。
pub const MV_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const MV_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const MV_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const MV_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const MV_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const MV_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 记账失衡。
pub const MV_EBAL: u32 = SEG_BASE + 0x07;
/// 名字非法。
pub const MV_ENAME: u32 = SEG_BASE + 0x08;
/// 源不存在。
pub const MV_ENOENT: u32 = SEG_BASE + 0x09;
/// 不可变源。
pub const MV_EPERM: u32 = SEG_BASE + 0x0a;
/// 源是目录。
pub const MV_EISDIR: u32 = SEG_BASE + 0x0b;
/// 不可读。
pub const MV_EACCES: u32 = SEG_BASE + 0x0c;
/// 目标设备空间不足。
pub const MV_ENOSPC: u32 = SEG_BASE + 0x0d;
/// 目标已存在。
pub const MV_EEXIST: u32 = SEG_BASE + 0x0e;
/// 校验失败。
pub const MV_EIO: u32 = SEG_BASE + 0x0f;

/// 阶段：复制。
pub const P_COPY: u8 = 0;
/// 阶段：校验。
pub const P_VERIFY: u8 = 1;
/// 阶段：删源。
pub const P_UNLINK: u8 = 2;
/// 阶段：落账。
pub const P_COMMIT: u8 = 3;

/// 普通文件。
pub const K_REG: u8 = 1;
/// 目录。
pub const K_DIR: u8 = 2;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 设备数。
pub const DEVS: usize = 3;
/// 样本格数。
pub const SAMPLES: usize = 14;

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

/// 源文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Src {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub dev: u8,
    pub size: u16,
    pub nlink: u16,
    pub kind: u8,
    pub immutable: bool,
    pub readable: bool,
    pub corrupt: bool,
    pub present: bool,
}

impl Src {
    pub fn make(name: &[u8], dev: u8, size: u16, nlink: u16) -> Src {
        let mut n = [0u8; NAME_CAP];
        let len = put(&mut n, name);
        Src { name: n, nlen: len, dev, size, nlink, kind: K_REG, immutable: false, readable: true, corrupt: false, present: true }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 目标目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dst {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub dev: u8,
    pub size: u16,
    pub protected: bool,
    pub committed: bool,
}

impl Dst {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 迁移器。
pub struct Migrator {
    srcs: Vec<Src>,
    dsts: Vec<Dst>,
    journal: Vec<u8>,
    free: [u16; DEVS],
    copied: u32,
    discarded: u32,
    freed: u32,
    rejects: u32,
}

impl Migrator {
    pub fn new(srcs: Vec<Src>, dsts: Vec<Dst>, free: [u16; DEVS]) -> Migrator {
        Migrator { srcs, dsts, journal: Vec::new(), free, copied: 0, discarded: 0, freed: 0, rejects: 0 }
    }

    pub fn src(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.srcs.len() {
            if let Some(s) = self.srcs.get(i) {
                if s.present && s.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn src_at(&self, i: usize) -> Option<Src> {
        match self.srcs.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn dst_at(&self, i: usize) -> Option<Dst> {
        match self.dsts.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn dst_len(&self) -> usize {
        self.dsts.len()
    }

    pub fn dst_find(&self, dev: u8, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dsts.len() {
            if let Some(d) = self.dsts.get(i) {
                if d.dev == dev && d.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    pub fn phase(&self, i: usize) -> Option<u8> {
        match self.journal.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn copied(&self) -> u32 {
        self.copied
    }

    pub fn discarded(&self) -> u32 {
        self.discarded
    }

    pub fn freed(&self) -> u32 {
        self.freed
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn free(&self, dev: u8) -> u16 {
        match self.free.get(dev as usize) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn commit_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.dsts.len() {
            if let Some(d) = self.dsts.get(i) {
                if d.committed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 迁移一个名字到目标设备。返回复制字节数（同设备快路径为 0）。
    pub fn migrate(&mut self, name: &[u8], dst_dev: u8, dst: &[u8], force: bool) -> Result<u16, u32> {
        if name.is_empty() || dst.is_empty() || name.len() > NAME_CAP || dst.len() > NAME_CAP || (dst_dev as usize) >= DEVS {
            self.rejects += 1;
            return Err(MV_ENAME);
        }
        let si = match self.src(name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(MV_ENOENT);
            }
        };
        let s = match self.srcs.get(si) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(MV_ENOENT);
            }
        };
        if s.kind == K_DIR {
            self.rejects += 1;
            return Err(MV_EISDIR);
        }
        if s.immutable {
            self.rejects += 1;
            return Err(MV_EPERM);
        }
        if !s.readable {
            self.rejects += 1;
            return Err(MV_EACCES);
        }
        let hit = self.dst_find(dst_dev, dst);
        if let Some(di) = hit {
            let d = match self.dsts.get(di) {
                Some(v) => *v,
                None => {
                    self.rejects += 1;
                    return Err(MV_ENOENT);
                }
            };
            if !force {
                self.rejects += 1;
                return Err(MV_EEXIST);
            }
            if d.protected {
                self.rejects += 1;
                return Err(MV_EPERM);
            }
            if s.dev == dst_dev {
                self.dsts.remove(di);
                let mut n = [0u8; NAME_CAP];
                let len = put(&mut n, dst);
                if let Some(m) = self.srcs.get_mut(si) {
                    m.name = n;
                    m.nlen = len;
                }
                return Ok(0);
            }
            if let Some(f) = self.free.get_mut(dst_dev as usize) {
                *f = f.saturating_add(d.size);
            }
            self.dsts.remove(di);
        }
        if s.dev == dst_dev {
            let mut n = [0u8; NAME_CAP];
            let len = put(&mut n, dst);
            if let Some(m) = self.srcs.get_mut(si) {
                m.name = n;
                m.nlen = len;
            }
            return Ok(0);
        }
        let free = self.free(dst_dev);
        if s.size > free {
            self.journal.push(P_COPY);
            self.discarded += free as u32;
            self.rejects += 1;
            return Err(MV_ENOSPC);
        }
        self.journal.push(P_COPY);
        if s.corrupt {
            self.journal.push(P_VERIFY);
            self.rejects += 1;
            return Err(MV_EIO);
        }
        self.journal.push(P_VERIFY);
        self.journal.push(P_UNLINK);
        self.journal.push(P_COMMIT);
        if let Some(f) = self.free.get_mut(dst_dev as usize) {
            *f -= s.size;
        }
        self.copied += s.size as u32;
        if let Some(m) = self.srcs.get_mut(si) {
            m.present = false;
            if m.nlink > 0 {
                m.nlink -= 1;
            }
            if m.nlink == 0 {
                self.freed += 1;
            }
        }
        let mut n = [0u8; NAME_CAP];
        let len = put(&mut n, dst);
        self.dsts.push(Dst { name: n, nlen: len, dev: dst_dev, size: s.size, protected: false, committed: true });
        Ok(s.size)
    }

    /// 日志窗口与计费审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.srcs.is_empty() {
            return Err(MV_EBAL);
        }
        let mut i = 0usize;
        while i < self.journal.len() {
            let p = match self.journal.get(i) {
                Some(v) => *v,
                None => return Err(MV_ENOBS),
            };
            if p == P_COMMIT {
                let a = if i >= 3 { self.journal.get(i - 3) } else { None };
                let b = if i >= 2 { self.journal.get(i - 2) } else { None };
                let c = if i >= 1 { self.journal.get(i - 1) } else { None };
                if a != Some(&P_COPY) || b != Some(&P_VERIFY) || c != Some(&P_UNLINK) {
                    return Err(MV_EBAL);
                }
            }
            i += 1;
        }
        if self.commit_count() != self.journal.iter().filter(|x| **x == P_COMMIT).count() {
            return Err(MV_EBAL);
        }
        let mut a = 0usize;
        while a < self.dsts.len() {
            let x = match self.dsts.get(a) {
                Some(v) => *v,
                None => return Err(MV_ENOBS),
            };
            let mut b = a + 1;
            while b < self.dsts.len() {
                if let Some(y) = self.dsts.get(b) {
                    if x.dev == y.dev && x.name() == y.name() {
                        return Err(MV_EBAL);
                    }
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.srcs.len() {
            if let Some(s) = self.srcs.get(i) {
                h = fold(h, s.dev as u64);
                h = fold(h, s.size as u64);
                h = fold(h, s.nlink as u64);
                h = fold(h, s.present as u64);
                let mut k = 0usize;
                while k < s.nlen as usize {
                    if let Some(b) = s.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        i = 0;
        while i < self.dsts.len() {
            if let Some(d) = self.dsts.get(i) {
                h = fold(h, d.dev as u64);
                h = fold(h, d.size as u64);
                h = fold(h, d.committed as u64);
                let mut k = 0usize;
                while k < d.nlen as usize {
                    if let Some(b) = d.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        i = 0;
        while i < self.journal.len() {
            if let Some(p) = self.journal.get(i) {
                h = fold(h, *p as u64);
            }
            i += 1;
        }
        h = fold(h, self.copied as u64);
        h = fold(h, self.freed as u64);
        fold(h, self.rejects as u64)
    }
}

/// 起始源表。
pub fn build_srcs() -> Vec<Src> {
    let mut v = Vec::new();
    let mut a = Src::make(b"s1", 0, 100, 1);
    v.push(a);
    a = Src::make(b"s2", 0, 200, 2);
    v.push(a);
    a = Src::make(b"s3", 0, 50, 1);
    a.immutable = true;
    v.push(a);
    let mut b = Src::make(b"big", 0, 900, 1);
    v.push(b);
    b = Src::make(b"p", 0, 60, 1);
    b.readable = false;
    v.push(b);
    v.push(Src::make(b"d1", 0, 30, 1));
    v.push(Src::make(b"s4", 0, 70, 1));
    v.push(Src::make(b"s5", 0, 80, 1));
    let mut c = Src::make(b"s6", 0, 40, 1);
    c.corrupt = true;
    v.push(c);
    v.push(Src::make(b"s7", 0, 40, 1));
    let mut d = Src::make(b"dir", 0, 0, 1);
    d.kind = K_DIR;
    v.push(d);
    v
}

/// 起始目标表。
pub fn build_dsts() -> Vec<Dst> {
    let mut v = Vec::new();
    let mut n = [0u8; NAME_CAP];
    let l = put(&mut n, b"d2");
    v.push(Dst { name: n, nlen: l, dev: 1, size: 30, protected: false, committed: false });
    let mut m = [0u8; NAME_CAP];
    let k = put(&mut m, b"d3");
    v.push(Dst { name: m, nlen: k, dev: 1, size: 10, protected: true, committed: false });
    v
}

/// 源表加目标表。
pub fn build_migrator() -> Migrator {
    Migrator::new(build_srcs(), build_dsts(), [1024, 512, 200])
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub src: [u8; NAME_CAP],
    pub slen: u8,
    pub dst: [u8; NAME_CAP],
    pub dlen: u8,
    pub dev: u8,
    pub force: bool,
    pub ok: bool,
    pub expect: u16,
}

impl Case {
    pub fn src(&self) -> &[u8] {
        cut(&self.src, self.slen)
    }

    pub fn dst(&self) -> &[u8] {
        cut(&self.dst, self.dlen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub ok: bool,
    pub value: u16,
}

/// 账本。
pub struct Ledger {
    m: Migrator,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { m: build_migrator(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, src: &[u8], dev: u8, dst: &[u8], force: bool, ok: bool, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MV_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(MV_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(MV_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(MV_EORDER);
        }
        let r = self.m.migrate(src, dev, dst, force);
        let end = match r {
            Ok(v) => {
                if !ok || v != expect {
                    self.rejects += 1;
                    return Err(MV_EMISMATCH);
                }
                Ending { ok: true, value: v }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    self.rejects += 1;
                    return Err(MV_EMISMATCH);
                }
                Ending { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { src: [0; NAME_CAP], slen: 0, dst: [0; NAME_CAP], dlen: 0, dev, force, ok, expect };
        c.slen = put(&mut c.src, src);
        c.dlen = put(&mut c.dst, dst);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
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

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.ok {
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

    pub fn m(&self) -> &Migrator {
        &self.m
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(MV_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(MV_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(MV_ENOBS),
            };
            if c.ok != e.ok || c.expect != e.value {
                return Err(MV_EMISMATCH);
            }
            i += 1;
        }
        self.m.audit()
    }

    pub fn seal(&self) -> u64 {
        self.m.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MV_EFROZEN);
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
            Err(MV_EMISMATCH)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enoent = (MV_ENOENT & 0xffff) as u16;
    let eperm = (MV_EPERM & 0xffff) as u16;
    let eisdir = (MV_EISDIR & 0xffff) as u16;
    let eacces = (MV_EACCES & 0xffff) as u16;
    let enospc = (MV_ENOSPC & 0xffff) as u16;
    let eexist = (MV_EEXIST & 0xffff) as u16;
    let eio = (MV_EIO & 0xffff) as u16;
    let _ = l.register(0, b"s1", 1, b"t1", false, true, 100);
    let _ = l.register(1, b"s1", 1, b"t1", false, false, enoent);
    let _ = l.register(2, b"s2", 1, b"t2", false, true, 200);
    let _ = l.register(3, b"s3", 1, b"t3", false, false, eperm);
    let _ = l.register(4, b"dir", 1, b"t4", false, false, eisdir);
    let _ = l.register(5, b"p", 1, b"t5", false, false, eacces);
    let _ = l.register(6, b"big", 1, b"t6", false, false, enospc);
    let _ = l.register(7, b"d1", 0, b"moved", false, true, 0);
    let _ = l.register(8, b"s4", 1, b"d2", false, false, eexist);
    let _ = l.register(9, b"s5", 1, b"d3", true, false, eperm);
    let _ = l.register(10, b"s5", 1, b"d2", true, true, 80);
    let _ = l.register(11, b"s5", 1, b"t7", false, false, enoent);
    let _ = l.register(12, b"s6", 2, b"t8", false, false, eio);
    let _ = l.register(13, b"s7", 2, b"t9", false, true, 40);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 5 && l.err_count() == 9 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = build_migrator();
    let r = a.migrate(b"s1", 1, b"t1", false);
    out[1] = r == Ok(100)
        && a.journal_len() == 4
        && a.phase(0) == Some(P_COPY)
        && a.phase(1) == Some(P_VERIFY)
        && a.phase(2) == Some(P_UNLINK)
        && a.phase(3) == Some(P_COMMIT)
        && a.src(b"s1").is_none()
        && a.dst_find(1, b"t1").is_some()
        && a.copied() == 100;

    let mut b = build_migrator();
    let before = b.src(b"big").is_some();
    let r2 = b.migrate(b"big", 1, b"t6", false);
    out[2] = r2 == Err(MV_ENOSPC)
        && before
        && b.src(b"big").is_some()
        && b.dst_find(1, b"t6").is_none()
        && b.journal_len() == 1
        && b.freed() == 0;

    let mut c = build_migrator();
    let free0 = c.free(2);
    let r3 = c.migrate(b"s6", 2, b"t8", false);
    let r4 = c.migrate(b"s7", 2, b"t9", false);
    out[3] = r3 == Err(MV_EIO)
        && c.src(b"s6").is_some()
        && c.dst_find(2, b"t8").is_none()
        && c.free(2) == free0 - 40
        && r4 == Ok(40)
        && c.freed() == 1;
    out
}

/// 十三判据。
pub fn run_unx11430_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11430.mv_crossdev");
    let l = build_ledger();
    let ec = |v: u32| (v & 0xffff) as u16;
    s.add("MV1-十四格齐备", l.len() == SAMPLES && l.case(0).map(|c| c.src() == b"s1" as &[u8]) == Some(true), "static");
    s.add_dyn("MV2-五应九拒", l.ok_count() == 5 && l.err_count() == 9 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("MV3-四阶段完整", l.m().journal_len() == 19 && l.m().commit_count() == 4 && l.m().copied() == 420, "static");
    s.add("MV4-硬链接删名不释放", l.ending(2).map(|e| e.value) == Some(200) && l.m().src(b"s2").is_none() && l.m().freed() == 3, "static");
    s.add("MV5-不可变源", l.ending(3).map(|e| e.value) == Some(ec(MV_EPERM)), "static");
    s.add("MV6-目录拒绝", l.ending(4).map(|e| e.value) == Some(ec(MV_EISDIR)), "static");
    s.add("MV7-不可读", l.ending(5).map(|e| e.value) == Some(ec(MV_EACCES)), "static");
    s.add("MV8-配额不足源保留", l.ending(6).map(|e| e.value) == Some(ec(MV_ENOSPC)) && l.m().src(b"big").is_some() && l.m().dst_find(1, b"t6").is_none(), "static");
    s.add("MV9-同设备快路径", l.ending(7).map(|e| e.value) == Some(0) && l.m().src(b"d1").is_none() && l.m().src(b"moved").is_some() && l.m().journal_len() == 19, "static");
    s.add("MV10-重名拒绝与保护目标", l.ending(8).map(|e| e.value) == Some(ec(MV_EEXIST)) && l.ending(9).map(|e| e.value) == Some(ec(MV_EPERM)), "static");
    s.add("MV11-强制覆盖释放空间", l.ending(10).map(|e| e.value) == Some(80) && l.m().dst_find(1, b"t2").is_some(), "static");
    s.add("MV12-损坏清目标留源", l.ending(12).map(|e| e.value) == Some(ec(MV_EIO)) && l.m().src(b"s6").is_some() && l.m().dst_find(2, b"t8").is_none(), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(MV_EFROZEN);
    let inj = inject_four();
    s.add_dyn("MV13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_phase_pipeline() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"s1", 1, b"t1", false), Ok(100));
        assert_eq!(m.journal_len(), 4);
        assert_eq!(m.phase(3), Some(P_COMMIT));
        assert_eq!(m.free(1), 412);
    }

    #[test]
    fn no_space_keeps_source() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"big", 1, b"t6", false), Err(MV_ENOSPC));
        assert!(m.src(b"big").is_some());
        assert_eq!(m.journal_len(), 1);
    }

    #[test]
    fn hardlink_not_freed() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"s2", 1, b"t2", false), Ok(200));
        assert!(m.src(b"s2").is_none());
        assert_eq!(m.freed(), 0);
    }

    #[test]
    fn corrupt_discards_target() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"s6", 2, b"t8", false), Err(MV_EIO));
        assert!(m.src(b"s6").is_some());
        assert!(m.dst_find(2, b"t8").is_none());
        assert_eq!(m.phase(1), Some(P_VERIFY));
    }

    #[test]
    fn same_device_shortcut() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"d1", 0, b"moved", false), Ok(0));
        assert_eq!(m.journal_len(), 0);
        assert!(m.src(b"d1").is_none());
        assert_eq!(m.copied(), 0);
    }

    #[test]
    fn force_replaces_target() {
        let mut m = build_migrator();
        assert_eq!(m.migrate(b"s4", 1, b"d2", false), Err(MV_EEXIST));
        assert_eq!(m.migrate(b"s5", 1, b"d3", true), Err(MV_EPERM));
        assert_eq!(m.migrate(b"s5", 1, b"d2", true), Ok(80));
        assert!(m.dst_find(1, b"d2").is_some());
    }

    #[test]
    fn ledger_replays() {
        let l = build_ledger();
        assert_eq!(l.ok_count(), 5);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11430_checks().failed_count(), 0);
    }
}
