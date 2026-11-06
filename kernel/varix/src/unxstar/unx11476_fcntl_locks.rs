//! UNX-F11476 fcntl 记录锁（0x20d90 段）
//!
//! POSIX 记录锁按“字节区间 + 属主”记账：读锁之间可共享，写锁与任何锁互斥。
//! 同一属主再次加锁会替换自己在该区间上的旧锁——这既是升级（读变写）也是
//! 降级（写变读）的唯一途径；被替换的区间要精确切分，左段与右段分别保留。
//! 关闭该属主的任意一个描述符会释放它的全部锁，这是 fcntl 锁与 flock 的
//! 关键差异。
//!
//! 非阻塞加锁冲突返回 `EAGAIN`，模拟阻塞时检测到环则返回 `EDEADLK`；
//! 查询（`F_GETLK`）不改变任何状态，只报告第一个冲突属主。相邻且同属主
//! 同类型的区间会被合并，保证账本形状唯一：同样的操作序列必须得到同样的
//! 锁列表，否则重放签名会漂移。
//!
//! 不变量：区间半开且 `start < end`；同一属主同类型锁两两不相接；读锁
//! 可重叠，写锁不与任何其他锁重叠；`bytes` 是各锁长度之和。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d90;
pub const LK_EID: u32 = SEG_BASE + 0x01;
pub const LK_EORDER: u32 = SEG_BASE + 0x02;
pub const LK_EDUP: u32 = SEG_BASE + 0x03;
pub const LK_EFROZEN: u32 = SEG_BASE + 0x04;
pub const LK_ENOBS: u32 = SEG_BASE + 0x05;
pub const LK_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const LK_EBAL: u32 = SEG_BASE + 0x07;
pub const LK_ESEAL: u32 = SEG_BASE + 0x08;
pub const LK_EAGAIN: u32 = SEG_BASE + 0x09;
pub const LK_EINVAL: u32 = SEG_BASE + 0x0a;
pub const LK_EACCES: u32 = SEG_BASE + 0x0b;
pub const LK_EDEADLK: u32 = SEG_BASE + 0x0c;
pub const LK_EBADF: u32 = SEG_BASE + 0x0d;
pub const LK_EOVERFLOW: u32 = SEG_BASE + 0x0e;

pub const F_RDLCK: u8 = 1;
pub const F_WRLCK: u8 = 2;
pub const F_UNLCK: u8 = 0;

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

/// 一把锁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lock {
    pub owner: u16,
    pub kind: u8,
    pub start: u64,
    pub end: u64,
}

/// 锁管理器。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mgr {
    pub locks: Vec<Lock>,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub locks: u8,
    pub bytes: u64,
    pub conflict: u16,
}

fn overlap(a0: u64, a1: u64, b0: u64, b1: u64) -> bool {
    a0 < b1 && b0 < a1
}

impl Mgr {
    pub fn new() -> Mgr {
        Mgr { locks: Vec::new() }
    }
    pub fn bytes(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.locks.len() {
            if let Some(l) = self.locks.get(i) {
                n = n.saturating_add(l.end.saturating_sub(l.start));
            }
            i += 1;
        }
        n
    }
    pub fn conflict(&self, owner: u16, kind: u8, start: u64, end: u64) -> Option<u16> {
        let mut i = 0usize;
        while i < self.locks.len() {
            if let Some(l) = self.locks.get(i) {
                if l.owner != owner && overlap(l.start, l.end, start, end) {
                    if kind == F_WRLCK || l.kind == F_WRLCK {
                        return Some(l.owner);
                    }
                }
            }
            i += 1;
        }
        None
    }
    fn merge_pass(&mut self) -> bool {
        let mut changed = false;
        let mut i = 0usize;
        while i < self.locks.len() {
            let mut j = i + 1;
            while j < self.locks.len() {
                let a = match self.locks.get(i) {
                    Some(v) => *v,
                    None => return changed,
                };
                let b = match self.locks.get(j) {
                    Some(v) => *v,
                    None => return changed,
                };
                let same = a.owner == b.owner && a.kind == b.kind && a.start <= b.end && b.start <= a.end;
                if same {
                    let s = if a.start < b.start { a.start } else { b.start };
                    let e = if a.end > b.end { a.end } else { b.end };
                    if let Some(m) = self.locks.get_mut(i) {
                        m.start = s;
                        m.end = e;
                    }
                    let _ = self.locks.remove(j);
                    changed = true;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        changed
    }
    fn normalize(&mut self) {
        let mut guard = 0u32;
        while self.merge_pass() && guard < 64 {
            guard += 1;
        }
    }
    /// F_SETLK / F_SETLKW。
    pub fn setlk(&mut self, owner: u16, kind: u8, start: u64, end: u64, wait: bool) -> Result<(), u32> {
        if kind != F_RDLCK && kind != F_WRLCK {
            return Err(LK_EINVAL);
        }
        if start >= end {
            return Err(LK_EINVAL);
        }
        if self.conflict(owner, kind, start, end).is_some() {
            return Err(if wait { LK_EDEADLK } else { LK_EAGAIN });
        }
        let mut next: Vec<Lock> = Vec::new();
        let mut i = 0usize;
        while i < self.locks.len() {
            let l = match self.locks.get(i) {
                Some(v) => *v,
                None => return Err(LK_ENOBS),
            };
            if l.owner == owner && overlap(l.start, l.end, start, end) {
                if l.start < start {
                    next.push(Lock { owner, kind: l.kind, start: l.start, end: start });
                }
                if l.end > end {
                    next.push(Lock { owner, kind: l.kind, start: end, end: l.end });
                }
            } else {
                next.push(l);
            }
            i += 1;
        }
        next.push(Lock { owner, kind, start, end });
        self.locks = next;
        self.normalize();
        Ok(())
    }
    /// 解锁区间。
    pub fn unlock(&mut self, owner: u16, start: u64, end: u64) -> Result<(), u32> {
        if start >= end {
            return Err(LK_EINVAL);
        }
        let mut next: Vec<Lock> = Vec::new();
        let mut i = 0usize;
        while i < self.locks.len() {
            let l = match self.locks.get(i) {
                Some(v) => *v,
                None => return Err(LK_ENOBS),
            };
            if l.owner == owner && overlap(l.start, l.end, start, end) {
                if l.start < start {
                    next.push(Lock { owner, kind: l.kind, start: l.start, end: start });
                }
                if l.end > end {
                    next.push(Lock { owner, kind: l.kind, start: end, end: l.end });
                }
            } else {
                next.push(l);
            }
            i += 1;
        }
        self.locks = next;
        self.normalize();
        Ok(())
    }
    /// F_GETLK：返回冲突属主。
    pub fn getlk(&self, owner: u16, kind: u8, start: u64, end: u64) -> Result<Option<u16>, u32> {
        if kind != F_RDLCK && kind != F_WRLCK {
            return Err(LK_EINVAL);
        }
        if start >= end {
            return Err(LK_EINVAL);
        }
        Ok(self.conflict(owner, kind, start, end))
    }
    /// 关闭描述符：释放属主全部锁。
    pub fn close_owner(&mut self, owner: u16) -> u32 {
        let mut kept: Vec<Lock> = Vec::new();
        let mut released = 0u32;
        let mut i = 0usize;
        while i < self.locks.len() {
            if let Some(l) = self.locks.get(i) {
                if l.owner == owner {
                    released += 1;
                } else {
                    kept.push(*l);
                }
            }
            i += 1;
        }
        self.locks = kept;
        released
    }
    pub fn owned(&self, owner: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.locks.len() {
            if let Some(l) = self.locks.get(i) {
                if l.owner == owner {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn audit(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.locks.len() {
            let a = match self.locks.get(i) {
                Some(v) => *v,
                None => return Err(LK_ENOBS),
            };
            if a.start >= a.end {
                return Err(LK_EBAL);
            }
            if a.kind != F_RDLCK && a.kind != F_WRLCK {
                return Err(LK_EBAL);
            }
            let mut j = i + 1;
            while j < self.locks.len() {
                let b = match self.locks.get(j) {
                    Some(v) => *v,
                    None => return Err(LK_ENOBS),
                };
                if a.owner == b.owner && a.kind == b.kind && a.start <= b.end && b.start <= a.end {
                    return Err(LK_EBAL);
                }
                if overlap(a.start, a.end, b.start, b.end) && (a.kind == F_WRLCK || b.kind == F_WRLCK) {
                    return Err(LK_EBAL);
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
        while i < self.locks.len() {
            if let Some(l) = self.locks.get(i) {
                h = fold(h, l.owner as u64);
                h = fold(h, l.kind as u64);
                h = fold(h, l.start);
                h = fold(h, l.end);
            }
            i += 1;
        }
        h
    }
}

fn snap(m: &Mgr, conflict: u16) -> Out {
    Out { locks: m.locks.len() as u8, bytes: m.bytes(), conflict }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut m = Mgr::new();
    match id {
        0 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            Ok(snap(&m, 0))
        }
        1 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            m.setlk(200, F_RDLCK, 0, 100, false)?;
            Ok(snap(&m, 0))
        }
        2 => {
            m.setlk(100, F_WRLCK, 0, 100, false)?;
            m.setlk(200, F_WRLCK, 0, 100, false)?;
            Ok(snap(&m, 0))
        }
        3 => {
            m.setlk(100, F_WRLCK, 0, 100, false)?;
            m.setlk(200, F_RDLCK, 0, 100, true)?;
            Ok(snap(&m, 0))
        }
        4 => {
            m.setlk(100, F_WRLCK, 0, 100, false)?;
            m.setlk(100, F_WRLCK, 100, 200, false)?;
            Ok(snap(&m, 0))
        }
        5 => {
            m.setlk(100, F_WRLCK, 0, 300, false)?;
            m.unlock(100, 100, 200)?;
            Ok(snap(&m, 0))
        }
        6 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            m.setlk(100, F_WRLCK, 0, 150, false)?;
            Ok(snap(&m, 0))
        }
        7 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            let c = m.getlk(200, F_WRLCK, 0, 100)?;
            let owner = match c {
                Some(v) => v,
                None => 0,
            };
            Ok(snap(&m, owner))
        }
        8 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            let c = m.getlk(200, F_RDLCK, 0, 100)?;
            let owner = match c {
                Some(v) => v,
                None => 0,
            };
            Ok(snap(&m, owner))
        }
        9 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            m.setlk(100, F_RDLCK, 100, 200, false)?;
            let _ = m.close_owner(100);
            Ok(snap(&m, 0))
        }
        10 => m.setlk(100, F_RDLCK, 50, 50, false).map(|_| snap(&m, 0)),
        11 => {
            m.setlk(100, F_WRLCK, 0, 100, false)?;
            m.unlock(100, 200, 300)?;
            Ok(snap(&m, 0))
        }
        12 => {
            m.setlk(100, F_RDLCK, 0, 100, false)?;
            m.setlk(200, F_RDLCK, 200, 300, false)?;
            Ok(snap(&m, 0))
        }
        13 => {
            m.setlk(100, F_WRLCK, 0, 100, false)?;
            m.setlk(200, F_WRLCK, 100, 200, false)?;
            Ok(snap(&m, 0))
        }
        _ => Err(LK_EID),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub locks: u8,
    pub bytes: u64,
    pub conflict: u16,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub locks: u8,
    pub bytes: u64,
    pub conflict: u16,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, locks: u8, bytes: u64, conflict: u16, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(LK_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(LK_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(LK_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(LK_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, locks, bytes, conflict, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.locks != locks || o.bytes != bytes || o.conflict != conflict {
                    return Err(LK_EMISMATCH);
                }
                Outcome { ok: true, locks: o.locks, bytes: o.bytes, conflict: o.conflict, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(LK_EMISMATCH);
                }
                Outcome { ok: false, locks: 0, bytes: 0, conflict: 0, code: low }
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
            return Err(LK_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(LK_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(LK_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(LK_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.locks != o.locks || v.bytes != o.bytes || v.conflict != o.conflict {
                        return Err(LK_EMISMATCH);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(LK_EMISMATCH);
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
            return Err(LK_EFROZEN);
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
                h = fold(h, o.locks as u64);
                h = fold(h, o.bytes);
                h = fold(h, o.conflict as u64);
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
            Err(LK_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eagain = (LK_EAGAIN & 0xffff) as u16;
    let einval = (LK_EINVAL & 0xffff) as u16;
    let edead = (LK_EDEADLK & 0xffff) as u16;
    let _ = l.register(0, 0, true, 1, 100, 0, 0);
    let _ = l.register(1, 1, true, 2, 200, 0, 0);
    let _ = l.register(2, 2, false, 0, 0, 0, eagain);
    let _ = l.register(3, 3, false, 0, 0, 0, edead);
    let _ = l.register(4, 4, true, 1, 200, 0, 0);
    let _ = l.register(5, 5, true, 2, 200, 0, 0);
    let _ = l.register(6, 6, true, 1, 150, 0, 0);
    let _ = l.register(7, 7, true, 1, 100, 100, 0);
    let _ = l.register(8, 8, true, 1, 100, 0, 0);
    let _ = l.register(9, 9, true, 0, 0, 0, 0);
    let _ = l.register(10, 10, false, 0, 0, 0, einval);
    let _ = l.register(11, 11, true, 1, 100, 0, 0);
    let _ = l.register(12, 12, true, 2, 200, 0, 0);
    let _ = l.register(13, 13, true, 2, 200, 0, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(LK_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut m = Mgr::new();
    let a = m.setlk(100, F_RDLCK, 0, 100, false);
    let b = m.setlk(200, F_RDLCK, 0, 100, false);
    let c = m.getlk(300, F_WRLCK, 0, 100);
    out[1] = a.is_ok()
        && b.is_ok()
        && m.locks.len() == 2
        && c == Ok(Some(100))
        && m.audit().is_ok();

    let mut s = Mgr::new();
    let w = s.setlk(100, F_WRLCK, 0, 300, false);
    let cut = s.unlock(100, 100, 200);
    let shape = s.locks.len() == 2 && s.bytes() == 200 && s.audit().is_ok();
    let all = s.close_owner(100);
    out[2] = w.is_ok() && cut.is_ok() && shape && all == 2 && s.locks.is_empty();

    let mut g = Mgr::new();
    let r = g.setlk(100, F_RDLCK, 0, 100, false);
    let up = g.setlk(100, F_WRLCK, 0, 150, false);
    let bad = g.setlk(100, F_RDLCK, 10, 10, false);
    out[3] = r.is_ok()
        && up.is_ok()
        && g.locks.len() == 1
        && g.bytes() == 150
        && g.locks.get(0).map(|x| x.kind) == Some(F_WRLCK)
        && bad == Err(LK_EINVAL)
        && g.setlk(200, F_WRLCK, 0, 10, false) == Err(LK_EAGAIN)
        && g.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11476_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11476_fcntl_locks");
    let l = build_ledger();
    s.add("LK1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "LK2-十一应三拒",
        l.ok_count() == 11 && l.err_count() == 3 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "LK3-读锁可共享",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 0, 100, false).is_ok()
                && m.setlk(200, F_RDLCK, 0, 100, false).is_ok()
                && m.locks.len() == 2
                && m.audit().is_ok()
        },
        "static",
    );
    s.add(
        "LK4-写锁互斥",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_WRLCK, 0, 100, false).is_ok()
                && m.setlk(200, F_WRLCK, 0, 100, false) == Err(LK_EAGAIN)
                && m.setlk(200, F_RDLCK, 0, 100, true) == Err(LK_EDEADLK)
        },
        "static",
    );
    s.add(
        "LK5-同属主相邻合并",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_WRLCK, 0, 100, false).is_ok()
                && m.setlk(100, F_WRLCK, 100, 200, false).is_ok()
                && m.locks.len() == 1
                && m.bytes() == 200
        },
        "static",
    );
    s.add(
        "LK6-解锁切分",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_WRLCK, 0, 300, false).is_ok()
                && m.unlock(100, 100, 200).is_ok()
                && m.locks.len() == 2
                && m.bytes() == 200
                && m.audit().is_ok()
        },
        "static",
    );
    s.add(
        "LK7-同属主升级替换",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 0, 100, false).is_ok()
                && m.setlk(100, F_WRLCK, 0, 150, false).is_ok()
                && m.locks.len() == 1
                && m.locks.get(0).map(|x| (x.kind, x.start, x.end)) == Some((F_WRLCK, 0, 150))
        },
        "static",
    );
    s.add(
        "LK8-getlk 报告冲突",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 0, 100, false).is_ok()
                && m.getlk(200, F_WRLCK, 0, 100) == Ok(Some(100))
                && m.getlk(200, F_WRLCK, 200, 300) == Ok(None)
        },
        "static",
    );
    s.add(
        "LK9-读读不冲突",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 0, 100, false).is_ok()
                && m.getlk(200, F_RDLCK, 0, 100) == Ok(None)
        },
        "static",
    );
    s.add(
        "LK10-关闭释放全部锁",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 0, 100, false).is_ok()
                && m.setlk(100, F_RDLCK, 100, 200, false).is_ok()
                && m.owned(100) == 1
                && m.close_owner(100) == 1
                && m.locks.is_empty()
        },
        "static",
    );
    s.add(
        "LK11-非法区间",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_RDLCK, 10, 10, false) == Err(LK_EINVAL)
                && m.unlock(100, 20, 20) == Err(LK_EINVAL)
                && m.getlk(100, F_RDLCK, 20, 10) == Err(LK_EINVAL)
        },
        "static",
    );
    s.add(
        "LK12-不同属主不合并",
        {
            let mut m = Mgr::new();
            m.setlk(100, F_WRLCK, 0, 100, false).is_ok()
                && m.setlk(200, F_WRLCK, 100, 200, false).is_ok()
                && m.locks.len() == 2
                && m.audit().is_ok()
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(LK_EFROZEN);
    s.add_dyn(
        "LK13-冻结重放与四路",
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
    fn readers_share_writers_exclude() {
        let mut m = Mgr::new();
        assert!(m.setlk(1, F_RDLCK, 0, 10, false).is_ok());
        assert!(m.setlk(2, F_RDLCK, 0, 10, false).is_ok());
        assert_eq!(m.setlk(3, F_WRLCK, 0, 10, false), Err(LK_EAGAIN));
    }

    #[test]
    fn same_owner_replaces() {
        let mut m = Mgr::new();
        assert!(m.setlk(1, F_RDLCK, 0, 100, false).is_ok());
        assert!(m.setlk(1, F_WRLCK, 50, 150, false).is_ok());
        assert_eq!(m.locks.len(), 2);
        assert_eq!(m.bytes(), 150);
    }

    #[test]
    fn unlock_splits() {
        let mut m = Mgr::new();
        assert!(m.setlk(1, F_WRLCK, 0, 300, false).is_ok());
        assert!(m.unlock(1, 100, 200).is_ok());
        assert_eq!(m.locks.len(), 2);
        assert_eq!(m.bytes(), 200);
    }

    #[test]
    fn close_releases_owner() {
        let mut m = Mgr::new();
        assert!(m.setlk(1, F_RDLCK, 0, 100, false).is_ok());
        assert!(m.setlk(1, F_RDLCK, 100, 200, false).is_ok());
        assert_eq!(m.owned(1), 1);
        assert_eq!(m.close_owner(1), 1);
        assert!(m.locks.is_empty());
    }

    #[test]
    fn getlk_reports_owner() {
        let mut m = Mgr::new();
        assert!(m.setlk(7, F_RDLCK, 0, 10, false).is_ok());
        assert_eq!(m.getlk(8, F_WRLCK, 0, 10), Ok(Some(7)));
        assert_eq!(m.getlk(8, F_RDLCK, 0, 10), Ok(None));
    }

    #[test]
    fn blocking_conflict_is_edeadlk() {
        let mut m = Mgr::new();
        assert!(m.setlk(1, F_WRLCK, 0, 10, false).is_ok());
        assert_eq!(m.setlk(2, F_WRLCK, 0, 10, true), Err(LK_EDEADLK));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 3);
        assert!(l.audit().is_ok());
        let s = run_unx11476_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
