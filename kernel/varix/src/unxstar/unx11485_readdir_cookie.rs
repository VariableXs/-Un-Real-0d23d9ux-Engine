//! UNX-F11485 目录游标与 readdir cookie（0x20e20 段）
//!
//! 目录迭代使用 cookie 而不是内存下标语义：`telldir` 返回的 cookie 指向
//! 某个目录槽位，`seekdir` 把游标放回该槽位，下一次 `readdir` 从那里
//! 继续。删除目录项只把槽位标记为墓碑，不移动其他槽位，因此后续条目的
//! cookie 保持稳定；这正是应用在遍历过程中删除文件时不会重读或跳过
//! 大量条目的原因。
//!
//! 墓碑会被扫描逻辑跳过，但跳到墓碑 cookie 仍然合法：`readdir` 从该槽
//! 开始向后找第一个仍在使用的位置。新目录项优先复用最低的空槽，所以
//! 一个刚被删除的名字可能在同一 cookie 上被新名字替代，而其他条目的
//! cookie 不变。cookie 等于目录长度表示 EOF；超过长度或负值等价于
//! 非法 cookie，返回 `EINVAL`。槽位全满且没有墓碑时新增返回 `ENOSPC`。
//!
//! 不变量：每个使用中的槽位名字唯一；墓碑保留 `ino` 但不再被 `lookup`
//! 命中；`next = 槽位 + 1` 且不超过长度；EOF 结果不携带节点信息；目录项
//! 的类型与 inode 编号对同一条目恒定。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e20;
pub const RD_EID: u32 = SEG_BASE + 0x01;
pub const RD_EORDER: u32 = SEG_BASE + 0x02;
pub const RD_EDUP: u32 = SEG_BASE + 0x03;
pub const RD_EFROZEN: u32 = SEG_BASE + 0x04;
pub const RD_ENOBS: u32 = SEG_BASE + 0x05;
pub const RD_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const RD_EBAL: u32 = SEG_BASE + 0x07;
pub const RD_ESEAL: u32 = SEG_BASE + 0x08;
pub const RD_ENOENT: u32 = SEG_BASE + 0x09;
pub const RD_ENOSPC: u32 = SEG_BASE + 0x0a;
pub const RD_EINVAL: u32 = SEG_BASE + 0x0b;
pub const RD_EEXIST: u32 = SEG_BASE + 0x0c;

pub const SAMPLES: usize = 15;
pub const NAME_CAP: usize = 8;
pub const SLOT_CAP: usize = 8;
pub const DT_FILE: u8 = 1;
pub const DT_DIR: u8 = 2;

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

/// 目录槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub ino: u32,
    pub dtype: u8,
    pub used: bool,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

/// 一次迭代结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub eof: bool,
    pub ino: u32,
    pub dtype: u8,
    pub ch: u8,
    pub next: u32,
}

/// 目录。
pub struct Dir {
    pub slots: Vec<Slot>,
    pub next_ino: u32,
}

fn named(s: &Slot, name: &[u8]) -> bool {
    s.used
        && (s.nlen as usize) == name.len()
        && s.name.get(0..name.len()).map(|v| v == name).unwrap_or(false)
}

impl Dir {
    /// 五个条目：a、b、c(目录)、d、e。
    pub fn base() -> Dir {
        let mut slots = Vec::new();
        let items: [(&[u8], u32, u8); 5] = [
            (b"a", 2, DT_FILE),
            (b"b", 3, DT_FILE),
            (b"c", 4, DT_DIR),
            (b"d", 5, DT_FILE),
            (b"e", 6, DT_FILE),
        ];
        let mut i = 0usize;
        while i < items.len() {
            if let Some(&(name, ino, dtype)) = items.get(i) {
                let mut buf = [0u8; NAME_CAP];
                let mut k = 0usize;
                while k < name.len() {
                    if let Some(&b) = name.get(k) {
                        buf[k] = b;
                    }
                    k += 1;
                }
                slots.push(Slot { ino, dtype, used: true, name: buf, nlen: name.len() as u8 });
            }
            i += 1;
        }
        Dir { slots, next_ino: 7 }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, i: usize) -> Option<Slot> {
        self.slots.get(i).copied()
    }

    /// 从 cookie 开始找下一个仍在使用的位置。
    pub fn readdir(&self, cookie: u32) -> Result<Probe, u32> {
        if cookie > (self.slots.len() as u32) {
            return Err(RD_EINVAL);
        }
        let mut i = cookie as usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.used {
                    let ch = s.name.first().copied().unwrap_or(0);
                    return Ok(Probe {
                        eof: false,
                        ino: s.ino,
                        dtype: s.dtype,
                        ch,
                        next: (i as u32) + 1,
                    });
                }
            }
            i += 1;
        }
        Ok(Probe { eof: true, ino: 0, dtype: 0, ch: 0, next: self.slots.len() as u32 })
    }

    pub fn seekdir(&self, cookie: u32) -> Result<(), u32> {
        if cookie > (self.slots.len() as u32) {
            return Err(RD_EINVAL);
        }
        Ok(())
    }

    /// 查找仍在使用的名字。
    pub fn lookup(&self, name: &[u8]) -> Option<&Slot> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if named(s, name) {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 删除名字，留下墓碑。
    pub fn unlink(&mut self, name: &[u8]) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.slots.len() {
            let hit = match self.slots.get(i) {
                Some(s) => named(s, name),
                None => false,
            };
            if hit {
                let ino = match self.slots.get(i) {
                    Some(s) => s.ino,
                    None => 0,
                };
                if let Some(s) = self.slots.get_mut(i) {
                    s.used = false;
                }
                return Ok(ino);
            }
            i += 1;
        }
        Err(RD_ENOENT)
    }

    /// 新增目录项，优先复用最低墓碑。
    pub fn add(&mut self, name: &[u8]) -> Result<u32, u32> {
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(RD_EINVAL);
        }
        if self.lookup(name).is_some() {
            return Err(RD_EEXIST);
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        let mut buf = [0u8; NAME_CAP];
        let mut k = 0usize;
        while k < name.len() {
            if let Some(&b) = name.get(k) {
                buf[k] = b;
            }
            k += 1;
        }
        let mut free = None;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if !s.used {
                    free = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match free {
            Some(idx) => {
                if let Some(s) = self.slots.get_mut(idx) {
                    s.ino = ino;
                    s.dtype = DT_FILE;
                    s.used = true;
                    s.name = buf;
                    s.nlen = name.len() as u8;
                }
                Ok(idx as u32)
            }
            None => {
                if self.slots.len() >= SLOT_CAP {
                    return Err(RD_ENOSPC);
                }
                let idx = self.slots.len() as u32;
                self.slots.push(Slot {
                    ino,
                    dtype: DT_FILE,
                    used: true,
                    name: buf,
                    nlen: name.len() as u8,
                });
                Ok(idx)
            }
        }
    }
}

/// 执行场景。
pub fn run_scen(id: u8) -> Result<Probe, u32> {
    let mut d = Dir::base();
    match id {
        0 => d.readdir(0),
        1 => d.readdir(1),
        2 => d.readdir(4),
        3 => d.readdir(5),
        4 => {
            d.unlink(b"b")?;
            d.readdir(1)
        }
        5 => {
            d.unlink(b"b")?;
            d.readdir(2)
        }
        6 => {
            d.unlink(b"b")?;
            d.unlink(b"d")?;
            d.readdir(1)
        }
        7 => {
            d.unlink(b"b")?;
            d.add(b"z")?;
            d.readdir(1)
        }
        8 => {
            d.add(b"f")?;
            d.add(b"g")?;
            d.add(b"h")?;
            d.add(b"i")?;
            d.readdir(0)
        }
        9 => d.readdir(6),
        10 => {
            d.unlink(b"b")?;
            d.readdir(0)
        }
        11 => {
            d.unlink(b"b")?;
            match d.lookup(b"b") {
                Some(s) => Ok(Probe { eof: false, ino: s.ino, dtype: s.dtype, ch: 0, next: 0 }),
                None => Err(RD_ENOENT),
            }
        }
        12 => {
            d.unlink(b"b")?;
            d.unlink(b"b")?;
            Ok(Probe { eof: true, ino: 0, dtype: 0, ch: 0, next: d.len() as u32 })
        }
        13 => {
            d.unlink(b"b")?;
            d.readdir(4)
        }
        14 => {
            d.add(b"f")?;
            d.readdir(5)
        }
        _ => Err(RD_EID),
    }
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub eof: bool,
    pub ino: u32,
    pub dtype: u8,
    pub ch: u8,
    pub next: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub eof: bool,
    pub ino: u32,
    pub dtype: u8,
    pub ch: u8,
    pub next: u32,
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
        eof: bool,
        ino: u32,
        dtype: u8,
        ch: u8,
        next: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RD_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(RD_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(RD_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(RD_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, eof, ino, dtype, ch, next, code };
        let out = match run_scen(scen) {
            Ok(v) => {
                if !ok || v.eof != eof || v.ino != ino || v.dtype != dtype || v.ch != ch || v.next != next {
                    return Err(RD_EMISMATCH);
                }
                Outcome { ok: true, eof: v.eof, ino: v.ino, dtype: v.dtype, ch: v.ch, next: v.next, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(RD_EMISMATCH);
                }
                Outcome { ok: false, eof: false, ino: 0, dtype: 0, ch: 0, next: 0, code: low }
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
            return Err(RD_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(RD_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(RD_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(RD_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok
                        || v.eof != o.eof
                        || v.ino != o.ino
                        || v.dtype != o.dtype
                        || v.ch != o.ch
                        || v.next != o.next
                    {
                        return Err(RD_EMISMATCH);
                    }
                    if v.next > (SLOT_CAP as u32) {
                        return Err(RD_EBAL);
                    }
                    if v.eof && v.dtype != 0 {
                        return Err(RD_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(RD_EMISMATCH);
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
            return Err(RD_EFROZEN);
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
                h = fold(h, u64::from(o.eof));
                h = fold(h, o.ino as u64);
                h = fold(h, o.dtype as u64);
                h = fold(h, o.ch as u64);
                h = fold(h, o.next as u64);
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
            Err(RD_ESEAL)
        }
    }
}

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enospc = (RD_ENOSPC & 0xffff) as u16;
    let einval = (RD_EINVAL & 0xffff) as u16;
    let enoent = (RD_ENOENT & 0xffff) as u16;
    let _ = l.register(0, 0, true, false, 2, DT_FILE, b'a', 1, 0);
    let _ = l.register(1, 1, true, false, 3, DT_FILE, b'b', 2, 0);
    let _ = l.register(2, 2, true, false, 6, DT_FILE, b'e', 5, 0);
    let _ = l.register(3, 3, true, true, 0, 0, 0, 5, 0);
    let _ = l.register(4, 4, true, false, 4, DT_DIR, b'c', 3, 0);
    let _ = l.register(5, 5, true, false, 4, DT_DIR, b'c', 3, 0);
    let _ = l.register(6, 6, true, false, 4, DT_DIR, b'c', 3, 0);
    let _ = l.register(7, 7, true, false, 7, DT_FILE, b'z', 2, 0);
    let _ = l.register(8, 8, false, false, 0, 0, 0, 0, enospc);
    let _ = l.register(9, 9, false, false, 0, 0, 0, 0, einval);
    let _ = l.register(10, 10, true, false, 2, DT_FILE, b'a', 1, 0);
    let _ = l.register(11, 11, false, false, 0, 0, 0, 0, enoent);
    let _ = l.register(12, 12, false, false, 0, 0, 0, 0, enoent);
    let _ = l.register(13, 13, true, false, 6, DT_FILE, b'e', 5, 0);
    let _ = l.register(14, 14, true, false, 7, DT_FILE, b'f', 6, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 11 && l.err_count() == 4 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(RD_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let d = Dir::base();
    out[1] = d.readdir(0) == Ok(Probe { eof: false, ino: 2, dtype: DT_FILE, ch: b'a', next: 1 })
        && d.readdir(1) == Ok(Probe { eof: false, ino: 3, dtype: DT_FILE, ch: b'b', next: 2 })
        && d.readdir(2) == Ok(Probe { eof: false, ino: 4, dtype: DT_DIR, ch: b'c', next: 3 });

    let mut t = Dir::base();
    let _ = t.unlink(b"b");
    let _ = t.unlink(b"d");
    let after = t.readdir(1);
    out[2] = after == Ok(Probe { eof: false, ino: 4, dtype: DT_DIR, ch: b'c', next: 3 })
        && t.readdir(4).map(|p| (p.ino, p.next)) == Ok((6, 5))
        && t.seekdir(3).is_ok();

    let mut r = Dir::base();
    let _ = r.unlink(b"b");
    let slot = r.add(b"z");
    let reused = r.readdir(1);
    let gone = r.lookup(b"b").is_none();
    let bad = r.readdir(9);
    let _ = r.add(b"f");
    let _ = r.add(b"g");
    let _ = r.add(b"h");
    let full = r.add(b"i");
    out[3] = slot == Ok(1)
        && reused == Ok(Probe { eof: false, ino: 7, dtype: DT_FILE, ch: b'z', next: 2 })
        && gone
        && bad == Err(RD_EINVAL)
        && full == Err(RD_ENOSPC);
    out
}

/// 十三项判据。
pub fn run_unx11485_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11485_readdir_cookie");
    let l = build_ledger();
    s.add(
        "RD1-十五行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "RD2-十一应四拒",
        l.ok_count() == 11 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "RD3-首条读取",
        run_scen(0).map(|p| (p.ino, p.ch, p.next)) == Ok((2, b'a', 1)),
        "static",
    );
    s.add(
        "RD4-游标前进",
        run_scen(1).map(|p| (p.ino, p.next)) == Ok((3, 2)),
        "static",
    );
    s.add(
        "RD5-尾部条目",
        run_scen(2).map(|p| (p.ino, p.ch, p.next)) == Ok((6, b'e', 5)),
        "static",
    );
    s.add(
        "RD6-末尾 EOF",
        run_scen(3).map(|p| (p.eof, p.next)) == Ok((true, 5)),
        "static",
    );
    s.add(
        "RD7-墓碑跳过",
        run_scen(4).map(|p| (p.ch, p.ino, p.next)) == Ok((b'c', 4, 3)),
        "static",
    );
    s.add(
        "RD8-跳到墓碑继续",
        run_scen(5).map(|p| (p.ch, p.next)) == Ok((b'c', 3)),
        "static",
    );
    s.add(
        "RD9-连续墓碑扫描",
        run_scen(6).map(|p| (p.ch, p.next)) == Ok((b'c', 3)),
        "static",
    );
    s.add(
        "RD10-槽位复用与 cookie 稳定",
        run_scen(7).map(|p| (p.ino, p.ch, p.next)) == Ok((7, b'z', 2))
            && run_scen(13).map(|p| (p.ch, p.next)) == Ok((b'e', 5)),
        "static",
    );
    s.add(
        "RD11-容量与非法游标",
        run_scen(8) == Err(RD_ENOSPC) && run_scen(9) == Err(RD_EINVAL),
        "static",
    );
    s.add(
        "RD12-查找与重复删除",
        run_scen(11) == Err(RD_ENOENT)
            && run_scen(12) == Err(RD_ENOENT)
            && run_scen(10).map(|p| (p.ch, p.next)) == Ok((b'a', 1)),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RD_EFROZEN);
    s.add_dyn(
        "RD13-冻结重放与四路",
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
    fn iteration_follows_cookies() {
        let d = Dir::base();
        assert_eq!(d.readdir(0).map(|p| (p.ino, p.next)), Ok((2, 1)));
        assert_eq!(d.readdir(1).map(|p| (p.ino, p.next)), Ok((3, 2)));
    }

    #[test]
    fn eof_at_length() {
        let d = Dir::base();
        assert_eq!(d.readdir(5).map(|p| (p.eof, p.next)), Ok((true, 5)));
        assert_eq!(d.readdir(6), Err(RD_EINVAL));
    }

    #[test]
    fn tombstones_are_skipped() {
        let mut d = Dir::base();
        assert_eq!(d.unlink(b"b"), Ok(3));
        assert_eq!(d.readdir(1).map(|p| (p.ch, p.next)), Ok((b'c', 3)));
        assert!(d.lookup(b"b").is_none());
    }

    #[test]
    fn slot_reuse_keeps_other_cookies() {
        let mut d = Dir::base();
        let _ = d.unlink(b"b");
        assert_eq!(d.add(b"z"), Ok(1));
        assert_eq!(d.readdir(1).map(|p| (p.ch, p.ino)), Ok((b'z', 7)));
        assert_eq!(d.readdir(4).map(|p| p.ch), Ok(b'e'));
    }

    #[test]
    fn capacity_is_bounded() {
        let mut d = Dir::base();
        assert!(d.add(b"f").is_ok());
        assert!(d.add(b"g").is_ok());
        assert!(d.add(b"h").is_ok());
        assert_eq!(d.add(b"i"), Err(RD_ENOSPC));
    }

    #[test]
    fn repeated_unlink_fails() {
        let mut d = Dir::base();
        assert_eq!(d.unlink(b"b"), Ok(3));
        assert_eq!(d.unlink(b"b"), Err(RD_ENOENT));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        let s = run_unx11485_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
