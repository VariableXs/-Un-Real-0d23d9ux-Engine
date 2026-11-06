//! UNX-F11475 dup 与 dup2 共享描述（0x20d80 段）
//!
//! `dup` 复制的是“打开文件描述”而不是文件本身：新旧描述符共享偏移、
//! 状态标志与引用计数，任何一端 `read`/`write` 推进的偏移另一端立即可见。
//! `dup` 总是选择编号最小的空闲槽位，并把新槽位的 `FD_CLOEXEC` 清掉；
//! `dup2` 则先原子地关闭目标槽位，再把目标指向源描述，自复制（old==new）
//! 是空操作，连 cloexec 都不改。
//!
//! 引用计数是唯一的生命周期真相：槽位关闭只递减计数，最后一个引用消失
//! 时描述才被释放；`dup2` 覆盖目标时同样递减旧描述的计数。表满时 `dup`
//! 返回 `EMFILE`，越界目标返回 `EBADF`，源无效时任何目标都不动。
//!
//! 不变量：每个使用中的槽位都指向一个 `refs >= 1` 的描述；`refs` 总和
//! 等于使用中的槽位数；共享描述的所有槽位偏移一致；失败路径不改变表。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d80;
pub const DP_EID: u32 = SEG_BASE + 0x01;
pub const DP_EORDER: u32 = SEG_BASE + 0x02;
pub const DP_EDUP: u32 = SEG_BASE + 0x03;
pub const DP_EFROZEN: u32 = SEG_BASE + 0x04;
pub const DP_ENOBS: u32 = SEG_BASE + 0x05;
pub const DP_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const DP_EBAL: u32 = SEG_BASE + 0x07;
pub const DP_ESEAL: u32 = SEG_BASE + 0x08;
pub const DP_EBADF: u32 = SEG_BASE + 0x09;
pub const DP_EMFILE: u32 = SEG_BASE + 0x0a;
pub const DP_EINVAL: u32 = SEG_BASE + 0x0b;
pub const DP_EOVERFLOW: u32 = SEG_BASE + 0x0c;
pub const DP_EBUSY: u32 = SEG_BASE + 0x0d;
pub const DP_ENOSPC: u32 = SEG_BASE + 0x0e;

pub const MAXFD: usize = 8;
pub const NODESC: u8 = 0xff;
pub const SAMPLES: usize = 14;

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

/// 槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub used: bool,
    pub cloexec: bool,
    pub desc: u8,
}

/// 打开文件描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desc {
    pub used: bool,
    pub ino: u16,
    pub offset: u64,
    pub refs: u8,
    pub append: bool,
}

/// 表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub slots: Vec<Slot>,
    pub descs: Vec<Desc>,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub newfd: u8,
    pub refs: u8,
    pub offset: u64,
}

impl Table {
    fn desc_of(&self, fd: u8) -> Option<usize> {
        let s = self.slots.get(fd as usize)?;
        if !s.used {
            return None;
        }
        let d = s.desc as usize;
        match self.descs.get(d) {
            Some(x) => {
                if x.used {
                    Some(d)
                } else {
                    None
                }
            }
            None => None,
        }
    }
    fn lowest_free(&self) -> Option<u8> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if !s.used {
                    return Some(i as u8);
                }
            }
            i += 1;
        }
        None
    }
    fn release_desc(&mut self, d: usize) {
        if let Some(x) = self.descs.get_mut(d) {
            x.refs = x.refs.saturating_sub(1);
            if x.refs == 0 {
                x.used = false;
                x.offset = 0;
                x.ino = 0;
            }
        }
    }
    pub fn dup(&mut self, old: u8) -> Result<u8, u32> {
        let d = match self.desc_of(old) {
            Some(v) => v,
            None => return Err(DP_EBADF),
        };
        let new = match self.lowest_free() {
            Some(v) => v,
            None => return Err(DP_EMFILE),
        };
        if let Some(dd) = self.descs.get_mut(d) {
            dd.refs = dd.refs.saturating_add(1);
        }
        if let Some(s) = self.slots.get_mut(new as usize) {
            s.used = true;
            s.cloexec = false;
            s.desc = d as u8;
        }
        Ok(new)
    }
    pub fn dup2(&mut self, old: u8, new: u8) -> Result<u8, u32> {
        let d = match self.desc_of(old) {
            Some(v) => v,
            None => return Err(DP_EBADF),
        };
        if (new as usize) >= self.slots.len() {
            return Err(DP_EBADF);
        }
        if old == new {
            return Ok(new);
        }
        let old_target = match self.slots.get(new as usize) {
            Some(s) => *s,
            None => return Err(DP_EBADF),
        };
        if old_target.used {
            self.release_desc(old_target.desc as usize);
        }
        if let Some(dd) = self.descs.get_mut(d) {
            dd.refs = dd.refs.saturating_add(1);
        }
        if let Some(s) = self.slots.get_mut(new as usize) {
            s.used = true;
            s.cloexec = false;
            s.desc = d as u8;
        }
        Ok(new)
    }
    pub fn close(&mut self, fd: u8) -> Result<(), u32> {
        let d = match self.desc_of(fd) {
            Some(v) => v,
            None => return Err(DP_EBADF),
        };
        self.release_desc(d);
        if let Some(s) = self.slots.get_mut(fd as usize) {
            s.used = false;
            s.cloexec = false;
            s.desc = NODESC;
        }
        Ok(())
    }
    pub fn set_cloexec(&mut self, fd: u8, on: bool) -> Result<(), u32> {
        if self.desc_of(fd).is_none() {
            return Err(DP_EBADF);
        }
        if let Some(s) = self.slots.get_mut(fd as usize) {
            s.cloexec = on;
        }
        Ok(())
    }
    pub fn write(&mut self, fd: u8, n: u64) -> Result<u64, u32> {
        let d = match self.desc_of(fd) {
            Some(v) => v,
            None => return Err(DP_EBADF),
        };
        let mut off = 0u64;
        if let Some(x) = self.descs.get_mut(d) {
            x.offset = x.offset.saturating_add(n);
            off = x.offset;
        }
        Ok(off)
    }
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u32;
        let mut used_slots = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            let s = match self.slots.get(i) {
                Some(v) => *v,
                None => return Err(DP_EBAL),
            };
            if s.used {
                used_slots += 1;
                match self.descs.get(s.desc as usize) {
                    Some(d) => {
                        if !d.used || d.refs == 0 {
                            return Err(DP_EBAL);
                        }
                    }
                    None => return Err(DP_EBAL),
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.descs.len() {
            let d = match self.descs.get(k) {
                Some(v) => *v,
                None => return Err(DP_EBAL),
            };
            if d.used {
                if d.refs == 0 {
                    return Err(DP_EBAL);
                }
                sum += d.refs as u32;
            } else if d.refs != 0 {
                return Err(DP_EBAL);
            }
            k += 1;
        }
        if sum != used_slots {
            return Err(DP_EBAL);
        }
        Ok(())
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                h = fold(h, u64::from(s.used));
                h = fold(h, u64::from(s.cloexec));
                h = fold(h, s.desc as u64);
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.descs.len() {
            if let Some(d) = self.descs.get(k) {
                h = fold(h, u64::from(d.used));
                h = fold(h, d.ino as u64);
                h = fold(h, d.offset);
                h = fold(h, d.refs as u64);
            }
            k += 1;
        }
        h
    }
}

/// 基础表：fd0->desc0, fd1->desc1, fd3->desc2。
pub fn base_table() -> Table {
    let slots = alloc::vec![
        Slot { used: true, cloexec: false, desc: 0 },
        Slot { used: true, cloexec: false, desc: 1 },
        Slot { used: false, cloexec: false, desc: NODESC },
        Slot { used: true, cloexec: false, desc: 2 },
        Slot { used: false, cloexec: false, desc: NODESC },
        Slot { used: false, cloexec: false, desc: NODESC },
        Slot { used: false, cloexec: false, desc: NODESC },
        Slot { used: false, cloexec: false, desc: NODESC },
    ];
    let descs = alloc::vec![
        Desc { used: true, ino: 1, offset: 0, refs: 1, append: false },
        Desc { used: true, ino: 1, offset: 10, refs: 1, append: false },
        Desc { used: true, ino: 7, offset: 100, refs: 1, append: false },
    ];
    Table { slots, descs }
}

fn snap(t: &Table, fd: u8) -> Out {
    match t.desc_of(fd) {
        Some(d) => match t.descs.get(d) {
            Some(x) => Out { newfd: fd, refs: x.refs, offset: x.offset },
            None => Out { newfd: fd, refs: 0, offset: 0 },
        },
        None => Out { newfd: fd, refs: 0, offset: 0 },
    }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut t = base_table();
    match id {
        0 => {
            let fd = t.dup(0)?;
            Ok(snap(&t, fd))
        }
        1 => {
            let fd = t.dup(3)?;
            Ok(snap(&t, fd))
        }
        2 => t.dup(2).map(|fd| snap(&t, fd)),
        3 => {
            while t.lowest_free().is_some() {
                let _ = t.dup(0);
            }
            t.dup(0).map(|fd| snap(&t, fd))
        }
        4 => {
            let fd = t.dup2(0, 2)?;
            Ok(snap(&t, fd))
        }
        5 => {
            let fd = t.dup2(0, 0)?;
            Ok(snap(&t, fd))
        }
        6 => {
            let fd = t.dup2(0, 1)?;
            Ok(snap(&t, fd))
        }
        7 => t.dup2(5, 2).map(|fd| snap(&t, fd)),
        8 => t.dup2(0, 9).map(|fd| snap(&t, fd)),
        9 => {
            t.set_cloexec(3, true)?;
            let fd = t.dup2(3, 3)?;
            Ok(snap(&t, fd))
        }
        10 => {
            t.set_cloexec(0, true)?;
            let fd = t.dup(0)?;
            Ok(snap(&t, fd))
        }
        11 => {
            let fd = t.dup(0)?;
            let _ = t.write(fd, 100)?;
            Ok(snap(&t, 0))
        }
        12 => {
            let fd = t.dup(0)?;
            t.close(fd)?;
            let _ = t.write(0, 20)?;
            Ok(snap(&t, 0))
        }
        13 => {
            let fd = t.dup2(0, 1)?;
            Ok(snap(&t, fd))
        }
        _ => Err(DP_EID),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub newfd: u8,
    pub refs: u8,
    pub offset: u64,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub newfd: u8,
    pub refs: u8,
    pub offset: u64,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, newfd: u8, refs: u8, offset: u64, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(DP_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(DP_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, newfd, refs, offset, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.newfd != newfd || o.refs != refs || o.offset != offset {
                    return Err(DP_EMISMATCH);
                }
                Outcome { ok: true, newfd: o.newfd, refs: o.refs, offset: o.offset, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(DP_EMISMATCH);
                }
                Outcome { ok: false, newfd: 0, refs: 0, offset: 0, code: low }
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
            return Err(DP_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(DP_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(DP_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(DP_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.newfd != o.newfd || v.refs != o.refs || v.offset != o.offset {
                        return Err(DP_EMISMATCH);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(DP_EMISMATCH);
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
            return Err(DP_EFROZEN);
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
                h = fold(h, o.newfd as u64);
                h = fold(h, o.refs as u64);
                h = fold(h, o.offset);
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
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() && fresh.err_count() == self.err_count() {
            Ok(fresh.seal())
        } else {
            Err(DP_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let ebadf = (DP_EBADF & 0xffff) as u16;
    let emfile = (DP_EMFILE & 0xffff) as u16;
    let _ = l.register(0, 0, true, 2, 2, 0, 0);
    let _ = l.register(1, 1, true, 2, 2, 100, 0);
    let _ = l.register(2, 2, false, 0, 0, 0, ebadf);
    let _ = l.register(3, 3, false, 0, 0, 0, emfile);
    let _ = l.register(4, 4, true, 2, 2, 0, 0);
    let _ = l.register(5, 5, true, 0, 1, 0, 0);
    let _ = l.register(6, 6, true, 1, 2, 0, 0);
    let _ = l.register(7, 7, false, 0, 0, 0, ebadf);
    let _ = l.register(8, 8, false, 0, 0, 0, ebadf);
    let _ = l.register(9, 9, true, 3, 1, 100, 0);
    let _ = l.register(10, 10, true, 2, 2, 0, 0);
    let _ = l.register(11, 11, true, 0, 2, 100, 0);
    let _ = l.register(12, 12, true, 0, 1, 20, 0);
    let _ = l.register(13, 13, true, 1, 2, 0, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(DP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut t = base_table();
    let a = t.dup(0);
    let b = match a {
        Ok(fd) => t.dup(fd),
        Err(_) => Err(DP_EBADF),
    };
    out[1] = a == Ok(2)
        && b == Ok(4)
        && t.desc_of(0) == Some(0)
        && t.desc_of(4) == Some(0)
        && t.descs.get(0).map(|d| d.refs) == Some(3)
        && t.audit().is_ok();

    let mut u = base_table();
    let target = u.dup2(0, 1);
    let same = u.dup2(0, 0);
    out[2] = target == Ok(1)
        && same == Ok(0)
        && u.slots.get(1).map(|s| s.desc) == Some(0)
        && u.descs.get(1).map(|d| d.used) == Some(false)
        && u.descs.get(0).map(|d| d.refs) == Some(2)
        && u.audit().is_ok();

    let mut c = base_table();
    let _ = c.set_cloexec(0, true);
    let d = c.dup(0);
    let w = match d {
        Ok(fd) => c.write(fd, 100),
        Err(_) => Err(DP_EBADF),
    };
    let r = c.dup2(9, 2);
    out[3] = d == Ok(2)
        && c.slots.get(2).map(|s| s.cloexec) == Some(false)
        && c.slots.get(0).map(|s| s.cloexec) == Some(true)
        && w == Ok(100)
        && r == Err(DP_EBADF)
        && c.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11475_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11475_dup_dup2");
    let l = build_ledger();
    s.add("DP1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "DP2-十应四拒",
        l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "DP3-dup 取最低空闲",
        {
            let mut t = base_table();
            t.dup(0) == Ok(2)
        },
        "static",
    );
    s.add(
        "DP4-dup 共享描述",
        {
            let mut t = base_table();
            let fd = t.dup(0);
            fd == Ok(2)
                && t.desc_of(2) == Some(0)
                && t.descs.get(0).map(|d| d.refs) == Some(2)
                && t.audit().is_ok()
        },
        "static",
    );
    s.add(
        "DP5-dup 无效源",
        {
            let mut t = base_table();
            t.dup(2) == Err(DP_EBADF) && t.audit().is_ok()
        },
        "static",
    );
    s.add(
        "DP6-表满拒绝",
        {
            let mut t = base_table();
            while t.lowest_free().is_some() {
                let _ = t.dup(0);
            }
            t.lowest_free().is_none() && t.dup(0) == Err(DP_EMFILE) && t.audit().is_ok()
        },
        "static",
    );
    s.add(
        "DP7-dup2 覆盖递减旧描述",
        {
            let mut t = base_table();
            t.dup2(0, 1) == Ok(1)
                && t.descs.get(1).map(|d| d.used) == Some(false)
                && t.descs.get(0).map(|d| d.refs) == Some(2)
                && t.audit().is_ok()
        },
        "static",
    );
    s.add(
        "DP8-dup2 自复制空操作",
        {
            let mut t = base_table();
            let before = t.seal();
            t.dup2(0, 0) == Ok(0) && t.seal() == before
        },
        "static",
    );
    s.add(
        "DP9-dup2 越界与无效",
        {
            let mut t = base_table();
            t.dup2(5, 2) == Err(DP_EBADF)
                && t.dup2(0, 9) == Err(DP_EBADF)
                && t.audit().is_ok()
        },
        "static",
    );
    s.add(
        "DP10-dup 不继承 cloexec",
        {
            let mut t = base_table();
            t.set_cloexec(0, true).is_ok()
                && t.dup(0) == Ok(2)
                && t.slots.get(2).map(|s| s.cloexec) == Some(false)
                && t.slots.get(0).map(|s| s.cloexec) == Some(true)
        },
        "static",
    );
    s.add(
        "DP11-共享偏移",
        {
            let mut t = base_table();
            let fd = t.dup(0);
            let off = match fd {
                Ok(f) => t.write(f, 100),
                Err(_) => Err(DP_EBADF),
            };
            off == Ok(100) && t.descs.get(0).map(|d| d.offset) == Some(100)
        },
        "static",
    );
    s.add(
        "DP12-关闭一支不影响另一支",
        {
            let mut t = base_table();
            let fd = t.dup(0);
            let closed = match fd {
                Ok(f) => t.close(f),
                Err(_) => Err(DP_EBADF),
            };
            closed.is_ok()
                && t.descs.get(0).map(|d| (d.refs, d.used)) == Some((1, true))
                && t.write(0, 20) == Ok(20)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DP_EFROZEN);
    s.add_dyn(
        "DP13-冻结重放与四路",
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
    fn dup_shares_description() {
        let mut t = base_table();
        let fd = t.dup(0);
        assert_eq!(fd, Ok(2));
        assert_eq!(t.desc_of(0), t.desc_of(2));
        assert_eq!(t.descs.get(0).map(|d| d.refs), Some(2));
    }

    #[test]
    fn dup2_replaces_target() {
        let mut t = base_table();
        assert_eq!(t.dup2(0, 1), Ok(1));
        assert_eq!(t.descs.get(1).map(|d| d.used), Some(false));
        assert_eq!(t.descs.get(0).map(|d| d.refs), Some(2));
    }

    #[test]
    fn dup2_self_is_noop() {
        let mut t = base_table();
        let before = t.seal();
        assert_eq!(t.dup2(0, 0), Ok(0));
        assert_eq!(t.seal(), before);
    }

    #[test]
    fn bad_sources_refused() {
        let mut t = base_table();
        assert_eq!(t.dup(2), Err(DP_EBADF));
        assert_eq!(t.dup2(2, 0), Err(DP_EBADF));
    }

    #[test]
    fn dup_clears_cloexec() {
        let mut t = base_table();
        let _ = t.set_cloexec(0, true);
        assert_eq!(t.dup(0), Ok(2));
        assert_eq!(t.slots.get(2).map(|s| s.cloexec), Some(false));
    }

    #[test]
    fn offsets_are_shared() {
        let mut t = base_table();
        let fd = match t.dup(0) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.write(fd, 100), Ok(100));
        assert_eq!(t.descs.get(0).map(|d| d.offset), Some(100));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        let s = run_unx11475_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
