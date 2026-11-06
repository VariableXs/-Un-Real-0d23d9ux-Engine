//! UNX-F11441 cp 模式继承（0x20b50 段）
//!
//! cp 的模式语义由三件事共同决定：源模式、目标是否存在、以及 -p/-n 选项。模型把
//! 每个条目拆成身份字段（名字、ino、类型）与内容字段（模式、属主、大小），复制
//! 时按策略逐字段决定是“盖写”还是“保留”。新建目标使用源模式与 umask 取反后的与，
//! 并总是清掉 setuid/setgid；已存在目标在不带 -p 时保留自己的模式与属主，只替换
//! 大小；带 -p 时才把源模式与属主搬到目标上。
//!
//! 特权位还有一条独立规则：即使带 -p，调用者不是 root 也不能保留 setuid/setgid，
//! 否则普通用户可以通过复制自己的文件制造提权跳板。root 的 -p 复制是唯一能保留
//! 6000 位的路径。组属主同理：只有 root 的 -p 复制才把 gid 设为源 gid，否则一律
//! 使用调用者身份。模型用源文件 6755 验证三种组合：默认得 0755、root -p 得 6755、
//! 非 root -p 得 0755。
//!
//! 目标形态决定错误而不是模式：源是目录返回 EISDIR（本模型没有递归选项），目标
//! 是目录返回 EISDIR，目标只读返回 EROFS，源不可读返回 EACCES，源缺失返回 ENOENT，
//! 源与目标同名（同一 ino）返回 EINVAL，-n 且目标存在返回 EEXIST。所有失败都不
//! 改变目标条目，模型先完成全部判定再写入。
//!
//! 审计要求：新建条目的模式、属主、大小等于策略函数的结果；带 -p 的盖写同样要
//! 满足策略；不带 -p 的盖写只检查大小；任何非 root 复制都不得留下 6000 位；名字
//! 唯一；ino 唯一。十六格账本八成八败，覆盖默认、盖写、保留、丢弃特权位、拒绝
//! 覆盖、目录、只读、自复制与缺失源。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b50;
/// 编号非法。
pub const CP_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const CP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const CP_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const CP_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const CP_EBAL: u32 = SEG_BASE + 0x07;
/// 源缺失。
pub const CP_ENOENT: u32 = SEG_BASE + 0x08;
/// 目标已存在。
pub const CP_EEXIST: u32 = SEG_BASE + 0x09;
/// 目录不递归。
pub const CP_EISDIR: u32 = SEG_BASE + 0x0a;
/// 自复制。
pub const CP_EINVAL: u32 = SEG_BASE + 0x0b;
/// 源不可读。
pub const CP_EACCES: u32 = SEG_BASE + 0x0c;
/// 目标只读。
pub const CP_EROFS: u32 = SEG_BASE + 0x0d;
/// 票据不一致。
pub const CP_ESEAL: u32 = SEG_BASE + 0x0e;

/// 选项：保留模式与属主。
pub const F_PRESERVE: u8 = 1;
/// 选项：不覆盖。
pub const F_NOCLOBBER: u8 = 2;

/// 普通文件。
pub const K_FILE: u8 = 1;
/// 目录。
pub const K_DIR: u8 = 2;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 样本格数。
pub const SAMPLES: usize = 16;
/// 特权位。
pub const SPECIAL: u16 = 0o6000;

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

/// 策略：目标模式。
pub fn want_perm(src_perm: u16, preserved: bool, root: bool, umask: u16) -> u16 {
    if preserved {
        if root {
            src_perm
        } else {
            src_perm & !SPECIAL
        }
    } else {
        src_perm & !(umask & 0o777) & !SPECIAL
    }
}

/// 策略：目标 uid。
pub fn want_uid(src_uid: u16, caller: u16, preserved: bool, root: bool) -> u16 {
    if preserved && root {
        src_uid
    } else {
        caller
    }
}

/// 条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub perm: u16,
    pub uid: u16,
    pub gid: u16,
    pub size: u32,
    pub ino: u32,
    pub readable: bool,
    pub ro: bool,
    pub copied: bool,
    pub overwritten: bool,
    pub preserved: bool,
    pub root: bool,
    pub caller: u16,
    pub src_perm: u16,
    pub src_uid: u16,
    pub src_gid: u16,
    pub src_size: u32,
}

impl Ent {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 复制结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub perm: u16,
    pub uid: u16,
    pub size: u32,
}

/// 文件系统。
pub struct Fs {
    ents: Vec<Ent>,
    umask: u16,
    copies: u32,
    rejects: u32,
}

impl Fs {
    pub fn new() -> Fs {
        let mut v = Vec::new();
        v.push(src(b"f", K_FILE, 0o6755, 0, 100, 1, true, false));
        v.push(src(b"g", K_FILE, 0o644, 7, 50, 2, true, false));
        v.push(src(b"d", K_DIR, 0o755, 0, 0, 3, true, false));
        v.push(src(b"unread", K_FILE, 0o644, 0, 10, 4, false, false));
        v.push(src(b"ro2", K_FILE, 0o644, 0, 10, 5, true, true));
        Fs { ents: v, umask: 0o022, copies: 0, rejects: 0 }
    }

    pub fn umask(&self) -> u16 {
        self.umask
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn ent(&self, name: &[u8]) -> Option<Ent> {
        match self.find(name) {
            Some(i) => match self.ents.get(i) {
                Some(v) => Some(*v),
                None => None,
            },
            None => None,
        }
    }

    pub fn copies(&self) -> u32 {
        self.copies
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn len(&self) -> usize {
        self.ents.len()
    }

    /// 复制 src 到 dst。
    pub fn copy(&mut self, caller: u16, src_name: &[u8], dst_name: &[u8], flags: u8) -> Result<Out, u32> {
        let si = match self.find(src_name) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(CP_ENOENT);
            }
        };
        let s = match self.ents.get(si) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(CP_ENOENT);
            }
        };
        if s.kind == K_DIR {
            self.rejects += 1;
            return Err(CP_EISDIR);
        }
        if !s.readable {
            self.rejects += 1;
            return Err(CP_EACCES);
        }
        if src_name == dst_name {
            self.rejects += 1;
            return Err(CP_EINVAL);
        }
        let preserved = flags & F_PRESERVE != 0;
        let root = caller == 0;
        let wp = want_perm(s.perm, preserved, root, self.umask);
        let wu = want_uid(s.uid, caller, preserved, root);
        let di = self.find(dst_name);
        if let Some(k) = di {
            let d = match self.ents.get(k) {
                Some(v) => *v,
                None => {
                    self.rejects += 1;
                    return Err(CP_ENOBS);
                }
            };
            if flags & F_NOCLOBBER != 0 {
                self.rejects += 1;
                return Err(CP_EEXIST);
            }
            if d.kind == K_DIR {
                self.rejects += 1;
                return Err(CP_EISDIR);
            }
            if d.ro {
                self.rejects += 1;
                return Err(CP_EROFS);
            }
            if let Some(e) = self.ents.get_mut(k) {
                e.size = s.size;
                e.copied = true;
                e.overwritten = true;
                e.preserved = preserved;
                e.root = root;
                e.caller = caller;
                e.src_perm = s.perm;
                e.src_uid = s.uid;
                e.src_gid = s.gid;
                e.src_size = s.size;
                if preserved {
                    e.perm = wp;
                    e.uid = wu;
                    e.gid = if root { s.gid } else { caller };
                }
            }
            self.copies += 1;
            return Ok(Out { perm: self.ents.get(k).map(|e| e.perm).unwrap_or(0), uid: self.ents.get(k).map(|e| e.uid).unwrap_or(0), size: s.size });
        }
        let mut e = Ent {
            name: [0; NAME_CAP],
            nlen: 0,
            kind: K_FILE,
            perm: wp,
            uid: wu,
            gid: if preserved && root { s.gid } else { caller },
            size: s.size,
            ino: 100 + self.copies,
            readable: true,
            ro: false,
            copied: true,
            overwritten: false,
            preserved,
            root,
            caller,
            src_perm: s.perm,
            src_uid: s.uid,
            src_gid: s.gid,
            src_size: s.size,
        };
        e.nlen = put(&mut e.name, dst_name);
        self.ents.push(e);
        self.copies += 1;
        Ok(Out { perm: wp, uid: wu, size: s.size })
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.ents.is_empty() || self.copies as usize > self.ents.len() {
            return Err(CP_EBAL);
        }
        let mut i = 0usize;
        while i < self.ents.len() {
            let e = match self.ents.get(i) {
                Some(v) => *v,
                None => return Err(CP_ENOBS),
            };
            if e.nlen == 0 || e.perm > 0o7777 || (e.kind != K_FILE && e.kind != K_DIR) {
                return Err(CP_EBAL);
            }
            if e.copied && !e.root && (e.perm & SPECIAL) != 0 {
                return Err(CP_EBAL);
            }
            if e.copied && (!e.overwritten || e.preserved) {
                if e.perm != want_perm(e.src_perm, e.preserved, e.root, self.umask)
                    || e.uid != want_uid(e.src_uid, e.caller, e.preserved, e.root)
                    || e.size != e.src_size
                {
                    return Err(CP_EBAL);
                }
            } else if e.copied && e.size != e.src_size {
                return Err(CP_EBAL);
            }
            let mut k = i + 1;
            while k < self.ents.len() {
                if let Some(o) = self.ents.get(k) {
                    if e.name() == o.name() || (e.ino == o.ino && e.copied && o.copied) {
                        return Err(CP_EBAL);
                    }
                }
                k += 1;
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = fold(FNV_OFF, self.umask as u64);
        let mut i = 0usize;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                h = fold(h, e.perm as u64);
                h = fold(h, e.uid as u64);
                h = fold(h, e.gid as u64);
                h = fold(h, e.size as u64);
                h = fold(h, e.ino as u64);
                h = fold(h, e.copied as u64);
                h = fold(h, e.overwritten as u64);
                h = fold(h, e.preserved as u64);
                let mut k = 0usize;
                while k < e.nlen as usize {
                    if let Some(b) = e.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.copies as u64);
        fold(h, self.rejects as u64)
    }
}

fn src(name: &[u8], kind: u8, perm: u16, uid: u16, size: u32, ino: u32, readable: bool, ro: bool) -> Ent {
    let mut e = Ent {
        name: [0; NAME_CAP],
        nlen: 0,
        kind,
        perm,
        uid,
        gid: uid,
        size,
        ino,
        readable,
        ro,
        copied: false,
        overwritten: false,
        preserved: false,
        root: false,
        caller: 0,
        src_perm: perm,
        src_uid: uid,
        src_gid: uid,
        src_size: size,
    };
    e.nlen = put(&mut e.name, name);
    e
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub src: [u8; NAME_CAP],
    pub slen: u8,
    pub dst: [u8; NAME_CAP],
    pub dlen: u8,
    pub flags: u8,
    pub caller: u16,
    pub ok: bool,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub out: Out,
    pub code: u16,
}

fn zero_out() -> Out {
    Out { perm: 0, uid: 0, size: 0 }
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

    pub fn register(&mut self, slot: u8, caller: u16, src_name: &[u8], dst_name: &[u8], flags: u8, expect: Out, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(CP_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(CP_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(CP_EORDER);
        }
        let r = self.fs.copy(caller, src_name, dst_name, flags);
        let end = match r {
            Ok(o) => {
                if !ok || o != expect {
                    self.rejects += 1;
                    return Err(CP_EMISMATCH);
                }
                Ending { out: o, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(CP_EMISMATCH);
                }
                Ending { out: zero_out(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { src: [0; NAME_CAP], slen: 0, dst: [0; NAME_CAP], dlen: 0, flags, caller, ok };
        c.slen = put(&mut c.src, src_name);
        c.dlen = put(&mut c.dst, dst_name);
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
            return Err(CP_EBAL);
        }
        self.fs.audit()
    }

    pub fn seal(&self) -> u64 {
        self.fs.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CP_EFROZEN);
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
            Err(CP_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn o(perm: u16, uid: u16, size: u32) -> Out {
    Out { perm, uid, size }
}

/// 十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, 1000, b"f", b"n1", 0, o(0o755, 1000, 100), true);
    let _ = l.register(1, 1000, b"f", b"n1", 0, o(0o755, 1000, 100), true);
    let _ = l.register(2, 0, b"f", b"n2", F_PRESERVE, o(0o6755, 0, 100), true);
    let _ = l.register(3, 1000, b"f", b"n3", F_PRESERVE, o(0o755, 1000, 100), true);
    let _ = l.register(4, 1000, b"g", b"n4", 0, o(0o644, 1000, 50), true);
    let _ = l.register(5, 1000, b"g", b"n5", F_NOCLOBBER, o(0o644, 1000, 50), true);
    let _ = l.register(6, 1000, b"g", b"n5", F_NOCLOBBER, zero_out(), false);
    let _ = l.register(7, 1000, b"unread", b"n6", 0, zero_out(), false);
    let _ = l.register(8, 1000, b"nope", b"n7", 0, zero_out(), false);
    let _ = l.register(9, 1000, b"g", b"d", 0, zero_out(), false);
    let _ = l.register(10, 1000, b"d", b"n8", 0, zero_out(), false);
    let _ = l.register(11, 1000, b"g", b"g", 0, zero_out(), false);
    let _ = l.register(12, 1000, b"g", b"ro2", 0, zero_out(), false);
    let _ = l.register(13, 1000, b"n1", b"n9", 0, o(0o755, 1000, 100), true);
    let _ = l.register(14, 1000, b"n9", b"n9", 0, zero_out(), false);
    let _ = l.register(15, 0, b"g", b"n10", F_PRESERVE, o(0o644, 7, 50), true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 8 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Fs::new();
    out[1] = a.copy(1000, b"f", b"x", 0) == Ok(o(0o755, 1000, 100))
        && a.copy(0, b"f", b"y", F_PRESERVE) == Ok(o(0o6755, 0, 100))
        && a.copy(7, b"f", b"z", F_PRESERVE) == Ok(o(0o755, 7, 100));

    let mut b = Fs::new();
    let first = b.copy(1000, b"g", b"w", 0);
    let again = b.copy(1000, b"f", b"w", 0);
    out[2] = first == Ok(o(0o644, 1000, 50)) && again == Ok(o(0o644, 1000, 100))
        && b.ent(b"w").map(|e| (e.perm, e.overwritten, e.size)) == Some((0o644, true, 100));

    let mut c = Fs::new();
    let n = c.copy(1000, b"g", b"w", F_NOCLOBBER);
    let n2 = c.copy(1000, b"g", b"w", F_NOCLOBBER);
    let selfc = c.copy(1000, b"g", b"g", 0);
    out[3] = n == Ok(o(0o644, 1000, 50)) && n2 == Err(CP_EEXIST) && selfc == Err(CP_EINVAL);
    out
}

/// 十三判据。
pub fn run_unx11441_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11441.cpmodes");
    let l = build_ledger();
    s.add("CP1-十六格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("CP2-八成八拒", l.ok_count() == 8 && l.err_count() == 8 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("CP3-默认清特权", l.ending(0).map(|e| e.out) == Some(o(0o755, 1000, 100)), "static");
    s.add("CP4-盖写保留模式", l.ending(1).map(|e| e.out) == Some(o(0o755, 1000, 100)) && l.fs().ent(b"n1").map(|e| e.overwritten) == Some(true), "static");
    s.add("CP5-root保留", l.ending(2).map(|e| e.out) == Some(o(0o6755, 0, 100)), "static");
    s.add("CP6-非root丢弃", l.ending(3).map(|e| e.out) == Some(o(0o755, 1000, 100)), "static");
    s.add("CP7-普通默认", l.ending(4).map(|e| e.out) == Some(o(0o644, 1000, 50)), "static");
    s.add("CP8-不覆盖", l.ending(5).map(|e| e.out) == Some(o(0o644, 1000, 50)) && l.ending(6).map(|e| e.code) == Some((CP_EEXIST & 0xffff) as u16), "static");
    s.add("CP9-不可读与缺失", l.ending(7).map(|e| e.code) == Some((CP_EACCES & 0xffff) as u16) && l.ending(8).map(|e| e.code) == Some((CP_ENOENT & 0xffff) as u16), "static");
    s.add("CP10-目录双向", l.ending(9).map(|e| e.code) == Some((CP_EISDIR & 0xffff) as u16) && l.ending(10).map(|e| e.code) == Some((CP_EISDIR & 0xffff) as u16), "static");
    s.add("CP11-自复制与只读", l.ending(11).map(|e| e.code) == Some((CP_EINVAL & 0xffff) as u16) && l.ending(12).map(|e| e.code) == Some((CP_EROFS & 0xffff) as u16) && l.ending(14).map(|e| e.code) == Some((CP_EINVAL & 0xffff) as u16), "static");
    s.add("CP12-复制品再复制", l.ending(13).map(|e| e.out) == Some(o(0o755, 1000, 100)) && l.ending(15).map(|e| e.out) == Some(o(0o644, 7, 50)) && l.fs().len() == 12 && l.fs().copies() == 8 && l.fs().ent(b"n1").map(|e| e.ino) == Some(100), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CP_EFROZEN);
    let inj = inject_four();
    s.add_dyn("CP13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_clears_special() {
        let mut f = Fs::new();
        assert_eq!(f.copy(1000, b"f", b"x", 0), Ok(o(0o755, 1000, 100)));
    }

    #[test]
    fn preserve_matrix() {
        let mut f = Fs::new();
        assert_eq!(f.copy(0, b"f", b"a", F_PRESERVE), Ok(o(0o6755, 0, 100)));
        assert_eq!(f.copy(5, b"f", b"b", F_PRESERVE), Ok(o(0o755, 5, 100)));
    }

    #[test]
    fn overwrite_keeps_mode() {
        let mut f = Fs::new();
        assert_eq!(f.copy(1000, b"g", b"x", 0), Ok(o(0o644, 1000, 50)));
        assert_eq!(f.copy(1000, b"f", b"x", 0), Ok(o(0o644, 1000, 100)));
        assert_eq!(f.ent(b"x").map(|e| (e.perm, e.size)), Some((0o644, 100)));
    }

    #[test]
    fn noclobber() {
        let mut f = Fs::new();
        assert_eq!(f.copy(1000, b"g", b"x", F_NOCLOBBER).map(|o| o.size), Ok(50));
        assert_eq!(f.copy(1000, b"g", b"x", F_NOCLOBBER), Err(CP_EEXIST));
    }

    #[test]
    fn error_matrix() {
        let mut f = Fs::new();
        assert_eq!(f.copy(1000, b"nope", b"x", 0), Err(CP_ENOENT));
        assert_eq!(f.copy(1000, b"unread", b"x", 0), Err(CP_EACCES));
        assert_eq!(f.copy(1000, b"d", b"x", 0), Err(CP_EISDIR));
        assert_eq!(f.copy(1000, b"g", b"d", 0), Err(CP_EISDIR));
        assert_eq!(f.copy(1000, b"g", b"ro2", 0), Err(CP_EROFS));
    }

    #[test]
    fn policy_function() {
        assert_eq!(want_perm(0o6755, false, true, 0o022), 0o755);
        assert_eq!(want_perm(0o6755, true, true, 0o022), 0o6755);
        assert_eq!(want_perm(0o6755, true, false, 0o022), 0o755);
        assert_eq!(want_perm(0o777, false, true, 0o077), 0o700);
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11441_checks().failed_count(), 0);
    }
}
