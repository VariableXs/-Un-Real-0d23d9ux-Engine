//! UNX-F11482 open 标志组合与校验（0x20df0 段）
//!
//! `open(2)` 的 flags 是一组按位组合，访问模式只占最低两位。取值 3 不是
//! 合法模式；`O_EXCL` 离开 `O_CREAT` 没有意义；`O_TRUNC` 要求写权限，
//! 只读加截断按本模块的策略直接拒绝。这些组合错误必须在查目录项之前
//! 判定，否则不存在的文件名会掩盖标志本身的错误。
//!
//! 目录项有文件、目录、符号链接三种形态。符号链接默认被跟随，跟随过程
//! 设有层数上限，自指链接由此得到 `ELOOP`，悬空链接得到 `ENOENT`；
//! `O_NOFOLLOW` 让链接本身成为错误而不是目标。目录只能以只读方式打开，
//! 写打开目录是 `EISDIR`，`O_DIRECTORY` 命中非目录是 `ENOTDIR`。
//! `O_CREAT|O_EXCL` 在文件已存在时返回 `EEXIST`，包括已存在的链接。
//!
//! 新建文件的大小为零，权限取 `0o666 & !umask`；`O_TRUNC` 只对已有普通
//! 文件把大小清零。`O_APPEND` 与 `O_CLOEXEC` 只是记录在打开描述上的
//! 布尔位，不改变目录项。不变量：返回的 `ino` 必须仍可在表中查到，目录
//! 的 `size` 恒为零，只有 `O_TRUNC` 会把普通文件的大小变成零。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20df0;
pub const OP_EID: u32 = SEG_BASE + 0x01;
pub const OP_EORDER: u32 = SEG_BASE + 0x02;
pub const OP_EDUP: u32 = SEG_BASE + 0x03;
pub const OP_EFROZEN: u32 = SEG_BASE + 0x04;
pub const OP_ENOBS: u32 = SEG_BASE + 0x05;
pub const OP_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const OP_EBAL: u32 = SEG_BASE + 0x07;
pub const OP_ESEAL: u32 = SEG_BASE + 0x08;
pub const OP_EINVAL: u32 = SEG_BASE + 0x09;
pub const OP_ENOENT: u32 = SEG_BASE + 0x0a;
pub const OP_EISDIR: u32 = SEG_BASE + 0x0b;
pub const OP_ENOTDIR: u32 = SEG_BASE + 0x0c;
pub const OP_ELOOP: u32 = SEG_BASE + 0x0d;
pub const OP_EEXIST: u32 = SEG_BASE + 0x0e;

pub const SAMPLES: usize = 15;
pub const NAME_CAP: usize = 8;
pub const FOLLOW_MAX: u32 = 8;

pub const OF_ACCMODE: u32 = 0o3;
pub const OF_RDONLY: u32 = 0o0;
pub const OF_WRONLY: u32 = 0o1;
pub const OF_RDWR: u32 = 0o2;
pub const OF_CREAT: u32 = 0o100;
pub const OF_EXCL: u32 = 0o200;
pub const OF_TRUNC: u32 = 0o1000;
pub const OF_APPEND: u32 = 0o2000;
pub const OF_DIRECTORY: u32 = 0o200000;
pub const OF_NOFOLLOW: u32 = 0o400000;
pub const OF_CLOEXEC: u32 = 0o2000000;

pub const K_FILE: u8 = 0;
pub const K_DIR: u8 = 1;
pub const K_LINK: u8 = 2;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
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

/// 目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ino: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub size: u32,
    pub mode: u16,
    pub target: [u8; NAME_CAP],
    pub tlen: u8,
}

/// 打开结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub ino: u32,
    pub acc: u8,
    pub append: bool,
    pub cloexec: bool,
    pub size: u32,
}

/// 目录表。
pub struct Table {
    pub entries: Vec<Entry>,
    pub umask: u16,
}

fn namebuf(name: &[u8]) -> ([u8; NAME_CAP], u8) {
    let mut buf = [0u8; NAME_CAP];
    let n = if name.len() > NAME_CAP { NAME_CAP } else { name.len() };
    let mut i = 0usize;
    while i < n {
        if let Some(&b) = name.get(i) {
            buf[i] = b;
        }
        i += 1;
    }
    (buf, n as u8)
}

fn entry(ino: u32, name: &[u8], kind: u8, size: u32, mode: u16) -> Entry {
    let (nbuf, nlen) = namebuf(name);
    Entry {
        ino,
        name: nbuf,
        nlen,
        kind,
        size,
        mode,
        target: [0u8; NAME_CAP],
        tlen: 0,
    }
}

fn link(ino: u32, name: &[u8], target: &[u8], mode: u16) -> Entry {
    let mut e = entry(ino, name, K_LINK, 0, mode);
    let (tbuf, tlen) = namebuf(target);
    e.target = tbuf;
    e.tlen = tlen;
    e
}

fn same(e: &Entry, name: &[u8]) -> bool {
    (e.nlen as usize) == name.len()
        && e.name.get(0..name.len()).map(|s| s == name).unwrap_or(false)
}

impl Table {
    /// 基准目录表。
    pub fn base() -> Table {
        let mut entries = Vec::new();
        entries.push(entry(0, b".", K_DIR, 0, 0o755));
        entries.push(entry(1, b"file", K_FILE, 100, 0o644));
        entries.push(entry(2, b"dir", K_DIR, 0, 0o755));
        entries.push(link(3, b"link", b"file", 0o777));
        entries.push(link(4, b"loop", b"loop", 0o777));
        entries.push(link(5, b"dangle", b"ghost", 0o777));
        entries.push(entry(6, b"ro", K_FILE, 50, 0o444));
        entries.push(entry(7, b"cfg", K_FILE, 0, 0o600));
        Table { entries, umask: 0o022 }
    }

    pub fn lookup(&self, name: &[u8]) -> Option<&Entry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if same(e, name) {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    pub fn entry(&self, ino: u32) -> Option<&Entry> {
        self.entries.get(ino as usize)
    }

    fn index_of(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if same(e, name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 打开文件。
    pub fn open(&mut self, name: &[u8], flags: u32) -> Result<OpenFile, u32> {
        let acc = ((flags & OF_ACCMODE) & 0xff) as u8;
        if acc > 2 {
            return Err(OP_EINVAL);
        }
        let creat = (flags & OF_CREAT) != 0;
        let excl = (flags & OF_EXCL) != 0;
        let trunc = (flags & OF_TRUNC) != 0;
        let dirf = (flags & OF_DIRECTORY) != 0;
        let nofol = (flags & OF_NOFOLLOW) != 0;
        let append = (flags & OF_APPEND) != 0;
        let cloexec = (flags & OF_CLOEXEC) != 0;
        if excl && !creat {
            return Err(OP_EINVAL);
        }
        if trunc && acc == 0 {
            return Err(OP_EINVAL);
        }
        if name.is_empty() {
            return Err(OP_EINVAL);
        }
        let found = self.index_of(name);
        if creat && excl && found.is_some() {
            return Err(OP_EEXIST);
        }
        let mut idx = match found {
            Some(v) => v,
            None => {
                if !creat {
                    return Err(OP_ENOENT);
                }
                let ino = self.entries.len() as u32;
                let mode = 0o666u16 & !self.umask;
                let (nbuf, nlen) = namebuf(name);
                self.entries.push(Entry {
                    ino,
                    name: nbuf,
                    nlen,
                    kind: K_FILE,
                    size: 0,
                    mode,
                    target: [0u8; NAME_CAP],
                    tlen: 0,
                });
                ino as usize
            }
        };
        let mut guard = 0u32;
        loop {
            let kind = match self.entries.get(idx) {
                Some(e) => e.kind,
                None => return Err(OP_ENOENT),
            };
            if kind != K_LINK {
                break;
            }
            if nofol {
                return Err(OP_ELOOP);
            }
            guard += 1;
            if guard > FOLLOW_MAX {
                return Err(OP_ELOOP);
            }
            let target = match self.entries.get(idx) {
                Some(e) => e.target,
                None => return Err(OP_ENOENT),
            };
            let tlen = match self.entries.get(idx) {
                Some(e) => e.tlen as usize,
                None => return Err(OP_ENOENT),
            };
            let slice = match target.get(0..tlen) {
                Some(s) => s,
                None => return Err(OP_ENOENT),
            };
            match self.index_of(slice) {
                Some(t) => idx = t,
                None => return Err(OP_ENOENT),
            }
        }
        let (kind, size) = match self.entries.get(idx) {
            Some(e) => (e.kind, e.size),
            None => return Err(OP_ENOENT),
        };
        if kind == K_DIR {
            if acc != 0 {
                return Err(OP_EISDIR);
            }
            if trunc {
                return Err(OP_EISDIR);
            }
        }
        if dirf && kind != K_DIR {
            return Err(OP_ENOTDIR);
        }
        if trunc && kind == K_FILE {
            if let Some(e) = self.entries.get_mut(idx) {
                e.size = 0;
            }
        }
        let final_size = match self.entries.get(idx) {
            Some(e) => e.size,
            None => size,
        };
        Ok(OpenFile {
            ino: idx as u32,
            acc,
            append,
            cloexec,
            size: final_size,
        })
    }
}

/// 场景结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Res {
    pub ino: i32,
    pub acc: u8,
    pub append: bool,
    pub cloexec: bool,
    pub size: u32,
}

/// 执行场景。
pub fn run_scen(id: u8) -> Result<Res, u32> {
    let mut t = Table::base();
    let (name, flags): (&[u8], u32) = match id {
        0 => (b"file", OF_RDONLY),
        1 => (b"file", OF_RDWR | OF_APPEND | OF_CLOEXEC),
        2 => (b"dir", OF_RDONLY),
        3 => (b"dir", OF_WRONLY),
        4 => (b"dir", OF_RDONLY | OF_DIRECTORY),
        5 => (b"file", OF_RDONLY | OF_DIRECTORY),
        6 => (b"link", OF_RDONLY),
        7 => (b"link", OF_RDONLY | OF_NOFOLLOW),
        8 => (b"loop", OF_RDONLY),
        9 => (b"dangle", OF_RDONLY),
        10 => (b"new", OF_WRONLY | OF_CREAT | OF_EXCL),
        11 => (b"file", OF_WRONLY | OF_CREAT | OF_EXCL),
        12 => (b"file", OF_WRONLY | OF_TRUNC),
        13 => (b"file", OF_RDONLY | OF_TRUNC),
        14 => (b"file", 0o3),
        _ => return Err(OP_EID),
    };
    match t.open(name, flags) {
        Ok(f) => Ok(Res {
            ino: f.ino as i32,
            acc: f.acc,
            append: f.append,
            cloexec: f.cloexec,
            size: f.size,
        }),
        Err(e) => Err(e),
    }
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub ino: i32,
    pub acc: u8,
    pub append: bool,
    pub cloexec: bool,
    pub size: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub ino: i32,
    pub acc: u8,
    pub append: bool,
    pub cloexec: bool,
    pub size: u32,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    pub fn register(
        &mut self,
        slot: u8,
        scen: u8,
        ok: bool,
        ino: i32,
        acc: u8,
        append: bool,
        cloexec: bool,
        size: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(OP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(OP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(OP_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(OP_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, ino, acc, append, cloexec, size, code };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok
                    || v.ino != ino
                    || v.acc != acc
                    || v.append != append
                    || v.cloexec != cloexec
                    || v.size != size
                {
                    return Err(OP_EMISMATCH);
                }
                Outcome {
                    ok: true,
                    ino: v.ino,
                    acc: v.acc,
                    append: v.append,
                    cloexec: v.cloexec,
                    size: v.size,
                    code: 0,
                }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(OP_EMISMATCH);
                }
                Outcome {
                    ok: false,
                    ino: -1,
                    acc: 0,
                    append: false,
                    cloexec: false,
                    size: 0,
                    code: low,
                }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }

    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(OP_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(OP_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(OP_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(OP_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.ino != o.ino
                        || v.acc != o.acc
                        || v.append != o.append
                        || v.cloexec != o.cloexec
                        || v.size != o.size
                    {
                        return Err(OP_EMISMATCH);
                    }
                    if v.ino < 0 || v.acc > 2 {
                        return Err(OP_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(OP_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(OP_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.ino as u64);
                h = fold(h, o.acc as u64);
                h = fold(h, u64::from(o.append));
                h = fold(h, u64::from(o.cloexec));
                h = fold(h, o.size as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal()
            && fresh.ok_count() == self.ok_count()
            && fresh.err_count() == self.err_count()
        {
            Ok(fresh.seal())
        } else {
            Err(OP_ESEAL)
        }
    }
}

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let einval = (OP_EINVAL & 0xffff) as u16;
    let enoent = (OP_ENOENT & 0xffff) as u16;
    let eisdir = (OP_EISDIR & 0xffff) as u16;
    let enotdir = (OP_ENOTDIR & 0xffff) as u16;
    let eloop = (OP_ELOOP & 0xffff) as u16;
    let eexist = (OP_EEXIST & 0xffff) as u16;
    let _ = l.register(0, 0, true, 1, 0, false, false, 100, 0);
    let _ = l.register(1, 1, true, 1, 2, true, true, 100, 0);
    let _ = l.register(2, 2, true, 2, 0, false, false, 0, 0);
    let _ = l.register(3, 3, false, -1, 0, false, false, 0, eisdir);
    let _ = l.register(4, 4, true, 2, 0, false, false, 0, 0);
    let _ = l.register(5, 5, false, -1, 0, false, false, 0, enotdir);
    let _ = l.register(6, 6, true, 1, 0, false, false, 100, 0);
    let _ = l.register(7, 7, false, -1, 0, false, false, 0, eloop);
    let _ = l.register(8, 8, false, -1, 0, false, false, 0, eloop);
    let _ = l.register(9, 9, false, -1, 0, false, false, 0, enoent);
    let _ = l.register(10, 10, true, 8, 1, false, false, 0, 0);
    let _ = l.register(11, 11, false, -1, 0, false, false, 0, eexist);
    let _ = l.register(12, 12, true, 1, 1, false, false, 0, 0);
    let _ = l.register(13, 13, false, -1, 0, false, false, 0, einval);
    let _ = l.register(14, 14, false, -1, 0, false, false, 0, einval);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 7 && l.err_count() == 8 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(OP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut t = Table::base();
    let rw = t.open(b"file", OF_RDWR | OF_APPEND | OF_CLOEXEC);
    let bad_acc = t.open(b"file", 3);
    let bad_excl = t.open(b"file", OF_EXCL);
    out[1] = rw.map(|f| (f.ino, f.acc, f.append, f.cloexec)) == Ok((1, 2, true, true))
        && bad_acc == Err(OP_EINVAL)
        && bad_excl == Err(OP_EINVAL);

    let mut t = Table::base();
    let follow = t.open(b"link", OF_RDONLY);
    let nofollow = t.open(b"link", OF_RDONLY | OF_NOFOLLOW);
    let cyc = t.open(b"loop", OF_RDONLY);
    let dangling = t.open(b"dangle", OF_RDONLY);
    out[2] = follow.map(|f| (f.ino, f.size)) == Ok((1, 100))
        && nofollow == Err(OP_ELOOP)
        && cyc == Err(OP_ELOOP)
        && dangling == Err(OP_ENOENT);

    let mut t = Table::base();
    let fresh = t.open(b"new", OF_WRONLY | OF_CREAT | OF_EXCL);
    let mode = t.lookup(b"new").map(|e| (e.mode, e.size));
    let again = t.open(b"file", OF_WRONLY | OF_CREAT | OF_EXCL);
    let tr = t.open(b"file", OF_WRONLY | OF_TRUNC);
    let ro_tr = t.open(b"file", OF_RDONLY | OF_TRUNC);
    out[3] = fresh.map(|f| (f.ino, f.size)) == Ok((8, 0))
        && mode == Some((0o644, 0))
        && again == Err(OP_EEXIST)
        && tr.map(|f| f.size) == Ok(0)
        && ro_tr == Err(OP_EINVAL);
    out
}

/// 十三项判据。
pub fn run_unx11482_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11482_open_flags");
    let l = build_ledger();
    s.add(
        "OP1-十五行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "OP2-七应八拒",
        l.ok_count() == 7 && l.err_count() == 8 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "OP3-访问模式记录",
        run_scen(0).map(|r| (r.acc, r.size)) == Ok((0, 100)),
        "static",
    );
    s.add(
        "OP4-追加与关闭执行位",
        run_scen(1).map(|r| (r.acc, r.append, r.cloexec)) == Ok((2, true, true)),
        "static",
    );
    s.add(
        "OP5-目录只读与写拒绝",
        run_scen(2).map(|r| r.ino) == Ok(2) && run_scen(3) == Err(OP_EISDIR),
        "static",
    );
    s.add(
        "OP6-O_DIRECTORY 校验",
        run_scen(4).map(|r| r.ino) == Ok(2) && run_scen(5) == Err(OP_ENOTDIR),
        "static",
    );
    s.add(
        "OP7-跟随符号链接",
        run_scen(6).map(|r| (r.ino, r.size)) == Ok((1, 100)),
        "static",
    );
    s.add(
        "OP8-不跟随链接",
        run_scen(7) == Err(OP_ELOOP),
        "static",
    );
    s.add(
        "OP9-循环与悬空链接",
        run_scen(8) == Err(OP_ELOOP) && run_scen(9) == Err(OP_ENOENT),
        "static",
    );
    s.add(
        "OP10-创建新文件",
        {
            let mut t = Table::base();
            let f = t.open(b"new", OF_WRONLY | OF_CREAT | OF_EXCL);
            f.map(|v| (v.ino, v.size)) == Ok((8, 0))
                && t.lookup(b"new").map(|e| e.mode) == Some(0o644)
        },
        "static",
    );
    s.add(
        "OP11-独占创建冲突",
        run_scen(11) == Err(OP_EEXIST),
        "static",
    );
    s.add(
        "OP12-截断与非法组合",
        {
            let mut t = Table::base();
            let tr = t.open(b"file", OF_WRONLY | OF_TRUNC);
            let ro = t.open(b"file", OF_RDONLY | OF_TRUNC);
            let ex = t.open(b"file", OF_EXCL);
            let acc = t.open(b"file", 3);
            tr.map(|f| f.size) == Ok(0)
                && ro == Err(OP_EINVAL)
                && ex == Err(OP_EINVAL)
                && acc == Err(OP_EINVAL)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(OP_EFROZEN);
    s.add_dyn(
        "OP13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_cloexec_are_recorded() {
        let mut t = Table::base();
        let f = t.open(b"file", OF_RDWR | OF_APPEND | OF_CLOEXEC);
        assert_eq!(f.map(|v| (v.acc, v.append, v.cloexec)), Ok((2, true, true)));
    }

    #[test]
    fn directory_cannot_be_opened_for_write() {
        let mut t = Table::base();
        assert_eq!(t.open(b"dir", OF_WRONLY), Err(OP_EISDIR));
        assert!(t.open(b"dir", OF_RDONLY | OF_DIRECTORY).is_ok());
    }

    #[test]
    fn symlinks_follow_or_fail() {
        let mut t = Table::base();
        assert_eq!(t.open(b"link", OF_RDONLY).map(|f| f.ino), Ok(1));
        assert_eq!(t.open(b"link", OF_RDONLY | OF_NOFOLLOW), Err(OP_ELOOP));
        assert_eq!(t.open(b"loop", OF_RDONLY), Err(OP_ELOOP));
        assert_eq!(t.open(b"dangle", OF_RDONLY), Err(OP_ENOENT));
    }

    #[test]
    fn create_applies_umask() {
        let mut t = Table::base();
        let f = t.open(b"new", OF_WRONLY | OF_CREAT | OF_EXCL);
        assert_eq!(f.map(|v| v.ino), Ok(8));
        assert_eq!(t.lookup(b"new").map(|e| e.mode), Some(0o644));
    }

    #[test]
    fn truncate_needs_write_access() {
        let mut t = Table::base();
        assert_eq!(t.open(b"file", OF_WRONLY | OF_TRUNC).map(|f| f.size), Ok(0));
        assert_eq!(t.open(b"file", OF_RDONLY | OF_TRUNC), Err(OP_EINVAL));
    }

    #[test]
    fn invalid_combinations_rejected() {
        let mut t = Table::base();
        assert_eq!(t.open(b"file", 3), Err(OP_EINVAL));
        assert_eq!(t.open(b"file", OF_EXCL), Err(OP_EINVAL));
        assert_eq!(t.open(b"file", OF_WRONLY | OF_CREAT | OF_EXCL), Err(OP_EEXIST));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.err_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11482_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
