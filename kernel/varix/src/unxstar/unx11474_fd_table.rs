//! UNX-F11474 文件描述符表分配（0x20d70 段）
//!
//! 每个进程维护一张描述符表，`open` 选择编号最小的空闲槽位，`close` 把
//! 槽位归还，下一次 `open` 立刻复用它。因此“关闭 0 号后打开新文件会拿到
//! 0 号”不是巧合，而是分配规则。表有两道上限：进程级 `RLIMIT_NOFILE`
//! 决定 `EMFILE`，系统级已打开文件总数决定 `ENFILE`，两者检查顺序不同。
//!
//! `FD_CLOEXEC` 是槽位属性，`exec` 时整表扫描，把带标记的槽位一次性释放；
//! 关闭数量是返回值的一部分，便于调用方核对资源。关闭未打开或越界的槽位
//! 一律 `EBADF`，重复关闭不会二次扣减全局计数。
//!
//! 不变量：`global_open == used()`；槽位编号等于下标；`close` 成功后
//! 该槽位 `used == false`；`exec` 的关闭数等于执行前带 cloexec 的槽位数；
//! 任何失败路径都不改变表与计数。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d70;
pub const FD_EID: u32 = SEG_BASE + 0x01;
pub const FD_EORDER: u32 = SEG_BASE + 0x02;
pub const FD_EDUP: u32 = SEG_BASE + 0x03;
pub const FD_EFROZEN: u32 = SEG_BASE + 0x04;
pub const FD_ENOBS: u32 = SEG_BASE + 0x05;
pub const FD_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const FD_EBAL: u32 = SEG_BASE + 0x07;
pub const FD_ESEAL: u32 = SEG_BASE + 0x08;
pub const FD_EBADF: u32 = SEG_BASE + 0x09;
pub const FD_EMFILE: u32 = SEG_BASE + 0x0a;
pub const FD_ENFILE: u32 = SEG_BASE + 0x0b;
pub const FD_EINVAL: u32 = SEG_BASE + 0x0c;
pub const FD_EOVERFLOW: u32 = SEG_BASE + 0x0d;
pub const FD_EBUSY: u32 = SEG_BASE + 0x0e;

pub const MAXFD: usize = 12;
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
    pub ino: u16,
}

/// 描述符表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub slots: Vec<Slot>,
    pub limit: u8,
    pub global_open: u16,
    pub global_max: u16,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub fd: u8,
    pub count: u16,
    pub closed: u8,
}

impl Table {
    pub fn used(&self) -> u16 {
        let mut n = 0u16;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
    pub fn open(&mut self, ino: u16, cloexec: bool) -> Result<u8, u32> {
        if self.used() >= self.limit as u16 {
            return Err(FD_EMFILE);
        }
        if self.global_open >= self.global_max {
            return Err(FD_ENFILE);
        }
        let mut idx = 0usize;
        let mut found = None;
        while idx < self.slots.len() {
            if let Some(s) = self.slots.get(idx) {
                if !s.used {
                    found = Some(idx);
                    break;
                }
            }
            idx += 1;
        }
        let at = match found {
            Some(v) => v,
            None => return Err(FD_EMFILE),
        };
        if let Some(s) = self.slots.get_mut(at) {
            s.used = true;
            s.cloexec = cloexec;
            s.ino = ino;
        } else {
            return Err(FD_EOVERFLOW);
        }
        self.global_open = self.global_open.saturating_add(1);
        Ok(at as u8)
    }
    pub fn close(&mut self, fd: u8) -> Result<(), u32> {
        if (fd as usize) >= self.slots.len() {
            return Err(FD_EBADF);
        }
        let s = match self.slots.get(fd as usize) {
            Some(v) => *v,
            None => return Err(FD_EBADF),
        };
        if !s.used {
            return Err(FD_EBADF);
        }
        if let Some(m) = self.slots.get_mut(fd as usize) {
            m.used = false;
            m.cloexec = false;
            m.ino = 0;
        }
        self.global_open = self.global_open.saturating_sub(1);
        Ok(())
    }
    pub fn set_cloexec(&mut self, fd: u8, on: bool) -> Result<(), u32> {
        if (fd as usize) >= self.slots.len() {
            return Err(FD_EBADF);
        }
        let s = match self.slots.get(fd as usize) {
            Some(v) => *v,
            None => return Err(FD_EBADF),
        };
        if !s.used {
            return Err(FD_EBADF);
        }
        if let Some(m) = self.slots.get_mut(fd as usize) {
            m.cloexec = on;
        }
        Ok(())
    }
    /// exec：关闭全部 cloexec 槽位，返回关闭数。
    pub fn exec(&mut self) -> u8 {
        let mut closed = 0u8;
        let mut i = 0usize;
        while i < self.slots.len() {
            let hit = match self.slots.get(i) {
                Some(s) => s.used && s.cloexec,
                None => false,
            };
            if hit {
                if let Some(s) = self.slots.get_mut(i) {
                    s.used = false;
                    s.cloexec = false;
                    s.ino = 0;
                }
                self.global_open = self.global_open.saturating_sub(1);
                closed = closed.saturating_add(1);
            }
            i += 1;
        }
        closed
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.slots.is_empty() || self.slots.len() > MAXFD {
            return Err(FD_EBAL);
        }
        if self.used() != self.global_open {
            return Err(FD_EBAL);
        }
        if self.global_open > self.global_max {
            return Err(FD_EBAL);
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
                h = fold(h, s.ino as u64);
            }
            i += 1;
        }
        h = fold(h, self.limit as u64);
        h = fold(h, self.global_open as u64);
        fold(h, self.global_max as u64)
    }
}

/// 基础表：0/1/3 已用，限额 8，全局 3/16。
pub fn base_table() -> Table {
    let mut slots = Vec::new();
    slots.push(Slot { used: true, cloexec: false, ino: 1 });
    slots.push(Slot { used: true, cloexec: false, ino: 1 });
    slots.push(Slot { used: false, cloexec: false, ino: 0 });
    slots.push(Slot { used: true, cloexec: false, ino: 7 });
    let mut k = 4usize;
    while k < MAXFD {
        slots.push(Slot { used: false, cloexec: false, ino: 0 });
        k += 1;
    }
    Table { slots, limit: 8, global_open: 3, global_max: 16 }
}

fn snap(t: &Table, fd: u8, closed: u8) -> Out {
    Out { fd, count: t.global_open, closed }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut t = base_table();
    match id {
        0 => {
            let fd = t.open(9, false)?;
            Ok(snap(&t, fd, 0))
        }
        1 => {
            let _ = t.open(9, false)?;
            let fd = t.open(10, false)?;
            Ok(snap(&t, fd, 0))
        }
        2 => {
            let fd = t.open(9, true)?;
            let closed = t.exec();
            Ok(snap(&t, fd, closed))
        }
        3 => {
            t.close(3)?;
            let fd = t.open(9, false)?;
            Ok(snap(&t, fd, 0))
        }
        4 => t.close(99).map(|_| snap(&t, 99, 0)),
        5 => t.close(2).map(|_| snap(&t, 2, 0)),
        6 => {
            t.set_cloexec(3, true)?;
            let closed = t.exec();
            Ok(snap(&t, 3, closed))
        }
        7 => {
            t.limit = 3;
            let fd = t.open(9, false)?;
            Ok(snap(&t, fd, 0))
        }
        8 => {
            t.global_max = 3;
            let fd = t.open(9, false)?;
            Ok(snap(&t, fd, 0))
        }
        9 => {
            t.limit = 5;
            let _ = t.open(9, false)?;
            let _ = t.open(9, false)?;
            let _ = t.open(9, false)?;
            Ok(snap(&t, 0, 0))
        }
        10 => t.set_cloexec(9, true).map(|_| snap(&t, 9, 0)),
        11 => {
            let closed = t.exec();
            Ok(snap(&t, 0, closed))
        }
        12 => {
            t.close(0)?;
            let fd = t.open(9, false)?;
            Ok(snap(&t, fd, 0))
        }
        13 => {
            t.close(3)?;
            t.close(3)?;
            Ok(snap(&t, 3, 0))
        }
        _ => Err(FD_EID),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub fd: u8,
    pub count: u16,
    pub closed: u8,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub fd: u8,
    pub count: u16,
    pub closed: u8,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, fd: u8, count: u16, closed: u8, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(FD_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(FD_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(FD_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, fd, count, closed, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.fd != fd || o.count != count || o.closed != closed {
                    return Err(FD_EMISMATCH);
                }
                Outcome { ok: true, fd: o.fd, count: o.count, closed: o.closed, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(FD_EMISMATCH);
                }
                Outcome { ok: false, fd: 0, count: 0, closed: 0, code: low }
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
            return Err(FD_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(FD_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(FD_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(FD_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.fd != o.fd || v.count != o.count || v.closed != o.closed {
                        return Err(FD_EMISMATCH);
                    }
                    if v.closed > 0 && v.count + v.closed as u16 > 16 {
                        return Err(FD_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(FD_EMISMATCH);
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
            return Err(FD_EFROZEN);
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
                h = fold(h, o.fd as u64);
                h = fold(h, o.count as u64);
                h = fold(h, o.closed as u64);
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
            Err(FD_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let ebadf = (FD_EBADF & 0xffff) as u16;
    let emfile = (FD_EMFILE & 0xffff) as u16;
    let enfile = (FD_ENFILE & 0xffff) as u16;
    let _ = l.register(0, 0, true, 2, 4, 0, 0);
    let _ = l.register(1, 1, true, 4, 5, 0, 0);
    let _ = l.register(2, 2, true, 2, 3, 1, 0);
    let _ = l.register(3, 3, true, 2, 3, 0, 0);
    let _ = l.register(4, 4, false, 0, 0, 0, ebadf);
    let _ = l.register(5, 5, false, 0, 0, 0, ebadf);
    let _ = l.register(6, 6, true, 3, 2, 1, 0);
    let _ = l.register(7, 7, false, 0, 0, 0, emfile);
    let _ = l.register(8, 8, false, 0, 0, 0, enfile);
    let _ = l.register(9, 9, false, 0, 0, 0, emfile);
    let _ = l.register(10, 10, false, 0, 0, 0, ebadf);
    let _ = l.register(11, 11, true, 0, 3, 0, 0);
    let _ = l.register(12, 12, true, 0, 3, 0, 0);
    let _ = l.register(13, 13, false, 0, 0, 0, ebadf);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 7 && l.err_count() == 7 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(FD_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut t = base_table();
    let first = t.open(9, false);
    let second = t.open(9, true);
    let closed = t.close(2);
    let third = t.open(10, false);
    out[1] = first == Ok(2)
        && second == Ok(4)
        && closed.is_ok()
        && third == Ok(2)
        && t.used() == 5
        && t.global_open == 5;

    let mut e = base_table();
    let fd = e.open(9, true);
    let n = e.exec();
    out[2] = fd == Ok(2)
        && n == 1
        && e.global_open == 3
        && e.slots.get(2).map(|s| s.used) == Some(false)
        && e.slots.get(0).map(|s| s.used) == Some(true);

    let mut a = base_table();
    a.limit = 4;
    let one = a.open(9, false);
    let two = a.open(9, false);
    let mut b = base_table();
    b.global_max = 3;
    let sys = b.open(9, false);
    let mut c = base_table();
    let bad = c.close(9);
    out[3] = one == Ok(2)
        && two == Err(FD_EMFILE)
        && sys == Err(FD_ENFILE)
        && bad == Err(FD_EBADF)
        && c.set_cloexec(9, true) == Err(FD_EBADF);
    out
}

/// 十三项判据。
pub fn run_unx11474_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11474_fd_table");
    let l = build_ledger();
    s.add("FD1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "FD2-七应七拒",
        l.ok_count() == 7 && l.err_count() == 7 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "FD3-最低空闲分配",
        {
            let mut t = base_table();
            t.open(9, false) == Ok(2)
        },
        "static",
    );
    s.add(
        "FD4-连续两次分配",
        {
            let mut t = base_table();
            t.open(9, false) == Ok(2) && t.open(10, false) == Ok(4)
        },
        "static",
    );
    s.add(
        "FD5-cloexec 随 exec 关闭",
        {
            let mut t = base_table();
            let fd = t.open(9, true);
            let n = t.exec();
            fd == Ok(2) && n == 1 && t.used() == 3 && t.global_open == 3
        },
        "static",
    );
    s.add(
        "FD6-关闭后复用低位",
        {
            let mut t = base_table();
            t.close(3).is_ok() && t.open(9, false) == Ok(2) && t.used() == 3
        },
        "static",
    );
    s.add(
        "FD7-坏描述符拒绝",
        {
            let mut t = base_table();
            t.close(99) == Err(FD_EBADF)
                && t.close(2) == Err(FD_EBADF)
                && t.set_cloexec(2, true) == Err(FD_EBADF)
                && t.used() == 3
        },
        "static",
    );
    s.add(
        "FD8-进程上限 EMFILE",
        {
            let mut t = base_table();
            t.limit = 3;
            t.open(9, false) == Err(FD_EMFILE) && t.used() == 3
        },
        "static",
    );
    s.add(
        "FD9-系统上限 ENFILE",
        {
            let mut t = base_table();
            t.global_max = 3;
            t.open(9, false) == Err(FD_ENFILE) && t.global_open == 3
        },
        "static",
    );
    s.add(
        "FD10-限额随关闭释放",
        {
            let mut t = base_table();
            t.limit = 3;
            t.close(3).is_ok() && t.open(9, false) == Ok(2)
        },
        "static",
    );
    s.add(
        "FD11-exec 无标记不关闭",
        {
            let mut t = base_table();
            t.exec() == 0 && t.used() == 3 && t.global_open == 3
        },
        "static",
    );
    s.add(
        "FD12-零号槽位复用",
        {
            let mut t = base_table();
            t.close(0).is_ok()
                && t.open(9, false) == Ok(0)
                && t.slots.get(0).map(|s| s.used) == Some(true)
                && t.used() == 3
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FD_EFROZEN);
    s.add_dyn(
        "FD13-冻结重放与四路",
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
    fn lowest_free_is_used() {
        let mut t = base_table();
        assert_eq!(t.open(9, false), Ok(2));
        assert_eq!(t.open(9, false), Ok(4));
    }

    #[test]
    fn close_allows_reuse() {
        let mut t = base_table();
        assert!(t.close(3).is_ok());
        assert_eq!(t.open(9, false), Ok(2));
        assert_eq!(t.used(), 3);
    }

    #[test]
    fn cloexec_closes_on_exec() {
        let mut t = base_table();
        assert_eq!(t.open(9, true), Ok(2));
        assert_eq!(t.exec(), 1);
        assert_eq!(t.global_open, 3);
    }

    #[test]
    fn bad_descriptors_refused() {
        let mut t = base_table();
        assert_eq!(t.close(99), Err(FD_EBADF));
        assert_eq!(t.close(2), Err(FD_EBADF));
    }

    #[test]
    fn per_process_limit() {
        let mut t = base_table();
        t.limit = 3;
        assert_eq!(t.open(9, false), Err(FD_EMFILE));
    }

    #[test]
    fn system_limit() {
        let mut t = base_table();
        t.global_max = 3;
        assert_eq!(t.open(9, false), Err(FD_ENFILE));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.err_count(), 7);
        assert!(l.audit().is_ok());
        let s = run_unx11474_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
