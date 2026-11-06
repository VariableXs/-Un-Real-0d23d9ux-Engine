//! UNX-F11466 inode 分配与世代号（0x20cf0 段）
//!
//! inode 分配器要同时给出三件事：一个空闲槽位、该槽位的世代号、以及“下一个
//! 该从哪找”的游标。最低空闲（lowest-free）保证可预测性，游标分配（hint）
//! 保证频繁 free/alloc 不在低位反复摩擦。两种策略共用同一张位图，因此
//! “谁在用”永远只有一个真相来源，审计只需数 1 的个数。
//!
//! 释放不是清零：槽位标记为空闲，世代号加一。旧的文件句柄带着 `(ino, gen)`
//! 二元组来访时，世代不匹配就会被识别为陈旧引用。重复释放必须拒绝，否则
//! 世代会被同一次逻辑删除推进两次，让一个仍然合法的句柄提前失效。保留
//! （reserve）用于 mkfs 之后预置根目录等固定 inode：它要求槽位空闲，成功后
//! 不推进世代，因为这不是一次“重用”。
//!
//! 不变量：`used` 数与位图 1 的个数相等；每个槽位的世代号只增不减（按 u16
//! 回绕）；游标始终落在 `[0, CAP]` 内。审计逐槽重算，满盘时 `alloc` 返回
//! `ENOSPC` 且位图不变。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20cf0;
pub const IN_EID: u32 = SEG_BASE + 0x01;
pub const IN_EORDER: u32 = SEG_BASE + 0x02;
pub const IN_EDUP: u32 = SEG_BASE + 0x03;
pub const IN_EFROZEN: u32 = SEG_BASE + 0x04;
pub const IN_ENOBS: u32 = SEG_BASE + 0x05;
pub const IN_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const IN_EBAL: u32 = SEG_BASE + 0x07;
pub const IN_ESEAL: u32 = SEG_BASE + 0x08;
pub const IN_ENOSPC: u32 = SEG_BASE + 0x09;
pub const IN_EEXIST: u32 = SEG_BASE + 0x0a;
pub const IN_EINVAL: u32 = SEG_BASE + 0x0b;
pub const IN_ERANGE: u32 = SEG_BASE + 0x0c;
pub const IN_EBADF: u32 = SEG_BASE + 0x0d;
pub const IN_EOVERFLOW: u32 = SEG_BASE + 0x0e;
pub const IN_ESTALE: u32 = SEG_BASE + 0x0f;

pub const K_FREE: u8 = 0;
pub const K_REG: u8 = 1;
pub const K_DIR: u8 = 2;
pub const CAP: usize = 10;
pub const SAMPLES: usize = 13;

pub const OP_ALLOC: u8 = 1;
pub const OP_FREE: u8 = 2;
pub const OP_RESERVE: u8 = 3;
pub const OP_HINT: u8 = 4;

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
    pub ino: u16,
    pub used: bool,
    pub gen: u16,
    pub kind: u8,
}

/// 分配器。
pub struct Table {
    slots: Vec<Slot>,
    hint: u16,
    errors: u32,
}

impl Table {
    pub fn new(slots: Vec<Slot>) -> Table {
        Table { slots, hint: 0, errors: 0 }
    }
    pub fn slot(&self, ino: u16) -> Option<Slot> {
        if (ino as usize) >= self.slots.len() {
            return None;
        }
        self.slots.get(ino as usize).copied()
    }
    pub fn errors(&self) -> u32 {
        self.errors
    }
    pub fn used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
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
    pub fn hint(&self) -> u16 {
        self.hint
    }
    fn scan_low(&self) -> Option<u16> {
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if !s.used {
                    return Some(s.ino);
                }
            }
            i += 1;
        }
        None
    }
    fn scan_hint(&self) -> Option<u16> {
        if self.slots.is_empty() {
            return None;
        }
        let mut k = 0usize;
        let start = self.hint as usize % self.slots.len();
        while k < self.slots.len() {
            let idx = (start + k) % self.slots.len();
            if let Some(s) = self.slots.get(idx) {
                if !s.used {
                    return Some(s.ino);
                }
            }
            k += 1;
        }
        None
    }
    fn place(&mut self, ino: u16, kind: u8) -> (u16, u16) {
        let gen = match self.slots.get(ino as usize) {
            Some(s) => s.gen,
            None => 0,
        };
        if let Some(s) = self.slots.get_mut(ino as usize) {
            s.used = true;
            s.kind = kind;
        }
        self.hint = ino.saturating_add(1);
        (ino, gen)
    }
    /// 最低空闲分配。
    pub fn alloc(&mut self, kind: u8) -> Result<(u16, u16), u32> {
        let ino = match self.scan_low() {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(IN_ENOSPC);
            }
        };
        Ok(self.place(ino, kind))
    }
    /// 游标分配。
    pub fn alloc_hint(&mut self, kind: u8) -> Result<(u16, u16), u32> {
        let ino = match self.scan_hint() {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(IN_ENOSPC);
            }
        };
        Ok(self.place(ino, kind))
    }
    /// 释放。
    pub fn free(&mut self, ino: u16) -> Result<u16, u32> {
        if (ino as usize) >= self.slots.len() {
            self.errors += 1;
            return Err(IN_ERANGE);
        }
        let s = match self.slots.get(ino as usize) {
            Some(v) => *v,
            None => {
                self.errors += 1;
                return Err(IN_ERANGE);
            }
        };
        if !s.used {
            self.errors += 1;
            return Err(IN_EINVAL);
        }
        if let Some(m) = self.slots.get_mut(ino as usize) {
            m.used = false;
            m.kind = K_FREE;
            m.gen = m.gen.wrapping_add(1);
        }
        Ok(ino)
    }
    /// 预留固定 inode。
    pub fn reserve(&mut self, ino: u16, kind: u8) -> Result<u16, u32> {
        if (ino as usize) >= self.slots.len() {
            self.errors += 1;
            return Err(IN_ERANGE);
        }
        let s = match self.slots.get(ino as usize) {
            Some(v) => *v,
            None => {
                self.errors += 1;
                return Err(IN_ERANGE);
            }
        };
        if s.used {
            self.errors += 1;
            return Err(IN_EEXIST);
        }
        if let Some(m) = self.slots.get_mut(ino as usize) {
            m.used = true;
            m.kind = kind;
        }
        Ok(s.gen)
    }
    /// 世代校验。
    pub fn check_gen(&self, ino: u16, gen: u16) -> Result<u16, u32> {
        match self.slot(ino) {
            Some(s) => {
                if s.used && s.gen == gen {
                    Ok(ino)
                } else {
                    Err(IN_ESTALE)
                }
            }
            None => Err(IN_EID),
        }
    }
    /// 审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.slots.is_empty() {
            return Err(IN_EBAL);
        }
        let mut i = 0;
        while i < self.slots.len() {
            let s = match self.slots.get(i) {
                Some(v) => v,
                None => return Err(IN_ENOBS),
            };
            if (s.ino as usize) != i {
                return Err(IN_EBAL);
            }
            if s.used && s.kind == K_FREE {
                return Err(IN_EBAL);
            }
            if !s.used && s.kind != K_FREE {
                return Err(IN_EBAL);
            }
            i += 1;
        }
        Ok(())
    }
    /// 签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                h = fold(h, s.ino as u64);
                h = fold(h, u64::from(s.used));
                h = fold(h, s.gen as u64);
                h = fold(h, s.kind as u64);
            }
            i += 1;
        }
        h = fold(h, self.hint as u64);
        fold(h, self.errors as u64)
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub op: u8,
    pub arg: u16,
    pub kind: u8,
    pub expect_ok: bool,
    pub expect_ino: u16,
    pub expect_gen: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub ino: u16,
    pub gen: u16,
}

/// 账本。
pub struct Ledger {
    t: Table,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            t: Table::new(build_slots()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, op: u8, arg: u16, kind: u8, ok: bool, ino: u16, gen: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(IN_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(IN_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(IN_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(IN_EORDER);
        }
        let s = Sample { op, arg, kind, expect_ok: ok, expect_ino: ino, expect_gen: gen };
        let r = match op {
            OP_ALLOC => self.t.alloc(kind),
            OP_HINT => self.t.alloc_hint(kind),
            OP_FREE => self.t.free(arg).map(|i| (i, 0)),
            OP_RESERVE => self.t.reserve(arg, kind).map(|g| (arg, g)),
            _ => Err(IN_EBADF),
        };
        let out = match r {
            Ok((i, g)) => {
                if !ok || i != ino || g != gen {
                    return Err(IN_EMISMATCH);
                }
                Outcome { ok: true, ino: i, gen: g }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != ino {
                    return Err(IN_EMISMATCH);
                }
                Outcome { ok: false, ino: 0, gen: 0 }
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
    pub fn t(&self) -> &Table {
        &self.t
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(IN_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(IN_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(IN_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(IN_EMISMATCH);
            }
            if s.expect_ok && (s.expect_ino != o.ino || s.expect_gen != o.gen) {
                return Err(IN_EMISMATCH);
            }
            i += 1;
        }
        self.t.audit()
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(IN_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = self.t.seal();
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, o.ino as u64);
                h = fold(h, o.gen as u64);
            }
            i += 1;
        }
        h
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() {
            Ok(fresh.seal())
        } else {
            Err(IN_ESEAL)
        }
    }
}

/// 初始槽位。
pub fn build_slots() -> Vec<Slot> {
    let mut v = Vec::new();
    v.push(Slot { ino: 0, used: true, gen: 1, kind: K_DIR });
    v.push(Slot { ino: 1, used: true, gen: 1, kind: K_DIR });
    v.push(Slot { ino: 2, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 3, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 4, used: true, gen: 2, kind: K_REG });
    v.push(Slot { ino: 5, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 6, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 7, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 8, used: false, gen: 0, kind: K_FREE });
    v.push(Slot { ino: 9, used: true, gen: 0, kind: K_REG });
    v
}

/// 十三格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enospc = (IN_ENOSPC & 0xffff) as u16;
    let eexist = (IN_EEXIST & 0xffff) as u16;
    let einval = (IN_EINVAL & 0xffff) as u16;
    let eid = (IN_ERANGE & 0xffff) as u16;
    let _ = l.register(0, OP_ALLOC, 0, K_REG, true, 2, 0);
    let _ = l.register(1, OP_ALLOC, 0, K_REG, true, 3, 0);
    let _ = l.register(2, OP_ALLOC, 0, K_REG, true, 5, 0);
    let _ = l.register(3, OP_FREE, 2, K_FREE, true, 2, 0);
    let _ = l.register(4, OP_ALLOC, 0, K_REG, true, 2, 1);
    let _ = l.register(5, OP_RESERVE, 6, K_DIR, true, 6, 0);
    let _ = l.register(6, OP_RESERVE, 6, K_DIR, false, eexist, 0);
    let _ = l.register(7, OP_RESERVE, 10, K_DIR, false, eid, 0);
    let _ = l.register(8, OP_FREE, 6, K_FREE, true, 6, 0);
    let _ = l.register(9, OP_FREE, 6, K_FREE, false, einval, 0);
    let _ = l.register(10, OP_HINT, 0, K_REG, true, 6, 1);
    let _ = l.register(11, OP_FREE, 2, K_FREE, true, 2, 0);
    let _ = l.register(12, OP_ALLOC, 0, K_DIR, true, 2, 2);
    let _ = enospc;
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 10 && l.err_count() == 3 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(IN_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut t = Table::new(build_slots());
    let a = t.alloc(K_REG);
    let b = t.alloc(K_REG);
    let c = t.alloc(K_REG);
    let d = t.free(2);
    let e = t.alloc(K_REG);
    out[1] = a == Ok((2, 0))
        && b == Ok((3, 0))
        && c == Ok((5, 0))
        && d == Ok(2)
        && e == Ok((2, 1))
        && t.slot(2).map(|s| s.gen) == Some(1)
        && t.used() == 7;

    let mut f = Table::new(build_slots());
    let mut n = 0;
    while n < 6 {
        let _ = f.alloc(K_REG);
        n += 1;
    }
    let full = f.alloc(K_REG);
    let reusable = f.free(6);
    let again = f.alloc(K_REG);
    out[2] = full == Err(IN_ENOSPC) && reusable == Ok(6) && again == Ok((6, 1)) && f.used() == 10;

    let mut g = Table::new(build_slots());
    out[3] = g.reserve(9, K_DIR) == Err(IN_EEXIST)
        && g.reserve(10, K_DIR) == Err(IN_ERANGE)
        && g.free(2) == Err(IN_EINVAL)
        && g.free(99) == Err(IN_ERANGE)
        && g.check_gen(0, 1) == Ok(0)
        && g.check_gen(0, 0) == Err(IN_ESTALE)
        && g.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11466_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11466.inode_alloc");
    let l = build_ledger();
    s.add("IN1-十三格齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.op) == Some(OP_ALLOC), "static");
    s.add_dyn(
        "IN2-十应三拒",
        l.ok_count() == 10 && l.err_count() == 3 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "IN3-最低空闲顺序",
        l.outcome(0).map(|o| o.ino) == Some(2)
            && l.outcome(1).map(|o| o.ino) == Some(3)
            && l.outcome(2).map(|o| o.ino) == Some(5),
        "static",
    );
    s.add(
        "IN4-释放推进世代",
        l.sample(3).map(|x| x.arg) == Some(2)
            && l.outcome(4).map(|o| (o.ino, o.gen)) == Some((2, 1))
            && l.outcome(12).map(|o| (o.ino, o.gen)) == Some((2, 2))
            && l.t().slot(2).map(|s| (s.used, s.gen)) == Some((true, 2)),
        "static",
    );
    s.add(
        "IN5-重复释放拒绝",
        l.outcome(9).map(|o| o.ok) == Some(false)
            && l.sample(9).map(|x| x.expect_ino) == Some((IN_EINVAL & 0xffff) as u16)
            && l.t().slot(6).map(|s| s.gen) == Some(1),
        "static",
    );
    s.add(
        "IN6-预留成功不推世代",
        l.outcome(5).map(|o| (o.ino, o.gen)) == Some((6, 0))
            && {
                let mut fresh = Table::new(build_slots());
                fresh.reserve(6, K_DIR) == Ok(0)
                    && fresh.slot(6).map(|s| (s.used, s.gen, s.kind)) == Some((true, 0, K_DIR))
            },
        "static",
    );
    s.add(
        "IN7-预留已用拒绝",
        l.outcome(6).map(|o| o.ok) == Some(false)
            && l.sample(6).map(|x| x.expect_ino) == Some((IN_EEXIST & 0xffff) as u16),
        "static",
    );
    s.add(
        "IN8-越界预留与释放",
        l.outcome(7).map(|o| o.ok) == Some(false)
            && Table::new(build_slots()).free(10) == Err(IN_ERANGE)
            && Table::new(build_slots()).reserve(10, K_REG) == Err(IN_ERANGE),
        "static",
    );
    s.add(
        "IN9-游标绕回分配",
        l.outcome(10).map(|o| (o.ino, o.gen)) == Some((6, 1))
            && {
                let mut fresh = Table::new(build_slots());
                fresh.alloc(K_REG) == Ok((2, 0))
                    && fresh.alloc(K_REG) == Ok((3, 0))
                    && fresh.alloc(K_REG) == Ok((5, 0))
                    && fresh.free(2) == Ok(2)
                    && fresh.alloc_hint(K_REG) == Ok((6, 0))
                    && fresh.hint() == 7
            },
        "static",
    );
    s.add(
        "IN10-二次复用世代累加",
        l.outcome(12).map(|o| (o.ino, o.gen)) == Some((2, 2))
            && l.t().slot(2).map(|s| s.kind) == Some(K_DIR),
        "static",
    );
    s.add(
        "IN11-位图计数一致",
        l.t().used() == 8 && l.t().used() == {
            let mut n = 0;
            let mut i = 0;
            while i < CAP {
                if l.t().slot(i as u16).map(|s| s.used) == Some(true) {
                    n += 1;
                }
                i += 1;
            }
            n
        },
        "static",
    );
    s.add(
        "IN12-世代句柄校验",
        l.t().check_gen(0, 1) == Ok(0)
            && l.t().check_gen(1, 1) == Ok(1)
            && l.t().check_gen(2, 2) == Ok(2)
            && l.t().check_gen(2, 1) == Err(IN_ESTALE)
            && l.t().check_gen(7, 0) == Err(IN_ESTALE),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(IN_EFROZEN);
    s.add_dyn(
        "IN13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} used={} hint={}", l.ok_count(), l.err_count(), l.t().used(), l.t().hint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_free_walks_up() {
        let mut t = Table::new(build_slots());
        assert_eq!(t.alloc(K_REG), Ok((2, 0)));
        assert_eq!(t.alloc(K_REG), Ok((3, 0)));
        assert_eq!(t.alloc(K_REG), Ok((5, 0)));
    }

    #[test]
    fn free_bumps_generation() {
        let mut t = Table::new(build_slots());
        assert_eq!(t.alloc(K_REG), Ok((2, 0)));
        assert_eq!(t.free(2), Ok(2));
        assert_eq!(t.slot(2).map(|s| s.gen), Some(1));
        assert_eq!(t.free(2), Err(IN_EINVAL));
        assert_eq!(t.alloc(K_REG), Ok((2, 1)));
    }

    #[test]
    fn full_table_refuses() {
        let mut t = Table::new(build_slots());
        let mut i = 0;
        while i < 6 {
            assert!(t.alloc(K_REG).is_ok());
            i += 1;
        }
        assert_eq!(t.alloc(K_REG), Err(IN_ENOSPC));
        assert_eq!(t.used(), 10);
    }

    #[test]
    fn reserve_semantics() {
        let mut t = Table::new(build_slots());
        assert_eq!(t.reserve(6, K_DIR), Ok(0));
        assert_eq!(t.reserve(6, K_DIR), Err(IN_EEXIST));
        assert_eq!(t.reserve(99, K_DIR), Err(IN_ERANGE));
        assert_eq!(t.free(6), Ok(6));
    }

    #[test]
    fn stale_generation_detected() {
        let mut t = Table::new(build_slots());
        assert_eq!(t.check_gen(0, 1), Ok(0));
        assert_eq!(t.check_gen(0, 2), Err(IN_ESTALE));
        assert_eq!(t.check_gen(7, 0), Err(IN_ESTALE));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 3);
        assert!(l.audit().is_ok());
        let s = run_unx11466_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
