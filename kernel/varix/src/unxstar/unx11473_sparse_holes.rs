//! UNX-F11473 稀疏文件与洞（0x20d60 段）
//!
//! 稀疏文件把“长度”和“实际占用”拆开：`truncate` 增长或 `lseek` 越过末尾
//! 只扩展逻辑大小，不分配任何块，读取这些区域得到零；只有真正写入覆盖
//! 到的块才被标记为已分配。`fallocate(PUNCH_HOLE)` 反过来释放整块，长度
//! 不变，之后再读又变成洞。
//!
//! 本模块用一张 16 块的位图描述分配状态，所有区间运算都按半开区间处理。
//! 写入跨块时需要把首尾块都标记；打洞只清除“完全落在区间内”的块，部分
//! 覆盖的块保留分配状态（语义上该块被零填充）。读取返回实际可读字节与
//! 其中来自洞的字节数，越过 EOF 的部分直接截短，不报错。
//!
//! 不变量：`size <= MAX_FILE`；位图中为 1 的块数等于 `alloc`；`read` 的
//! `total + 越界丢弃 == 请求长度`；增长与打洞都不改变 `size` 之外的元数据。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d60;
pub const SP_EID: u32 = SEG_BASE + 0x01;
pub const SP_EORDER: u32 = SEG_BASE + 0x02;
pub const SP_EDUP: u32 = SEG_BASE + 0x03;
pub const SP_EFROZEN: u32 = SEG_BASE + 0x04;
pub const SP_ENOBS: u32 = SEG_BASE + 0x05;
pub const SP_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const SP_EBAL: u32 = SEG_BASE + 0x07;
pub const SP_ESEAL: u32 = SEG_BASE + 0x08;
pub const SP_EINVAL: u32 = SEG_BASE + 0x09;
pub const SP_EFBIG: u32 = SEG_BASE + 0x0a;
pub const SP_EOVERFLOW: u32 = SEG_BASE + 0x0b;
pub const SP_ENOSPC: u32 = SEG_BASE + 0x0c;
pub const SP_EBADF: u32 = SEG_BASE + 0x0d;
pub const SP_EIO: u32 = SEG_BASE + 0x0e;

pub const BSIZE: u64 = 512;
pub const NBLK: usize = 16;
pub const MAX_FILE: u64 = BSIZE * NBLK as u64;
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

/// 稀疏 inode。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub size: u64,
    pub map: [bool; NBLK],
}

/// 操作结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub size: u64,
    pub alloc: u32,
    pub total: u32,
    pub holes: u32,
}

impl Inode {
    pub fn alloc(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < NBLK {
            if self.map.get(i) == Some(&true) {
                n += 1;
            }
            i += 1;
        }
        n
    }
    pub fn reset(&mut self) {
        self.size = 0;
        let mut i = 0usize;
        while i < NBLK {
            if let Some(m) = self.map.get_mut(i) {
                *m = false;
            }
            i += 1;
        }
    }
}

/// 基础 inode：长度 4096，块 0、2、8 已分配。
pub fn base_inode() -> Inode {
    let mut ino = Inode { size: 4096, map: [false; NBLK] };
    if let Some(m) = ino.map.get_mut(0) {
        *m = true;
    }
    if let Some(m) = ino.map.get_mut(2) {
        *m = true;
    }
    if let Some(m) = ino.map.get_mut(8) {
        *m = true;
    }
    ino
}

fn mark(ino: &mut Inode, first: u64, last: u64) {
    let mut k = first;
    while k <= last && k < NBLK as u64 {
        if let Some(m) = ino.map.get_mut(k as usize) {
            *m = true;
        }
        k += 1;
    }
}

fn clear(ino: &mut Inode, first: u64, last: u64) {
    let mut k = first;
    while k <= last && k < NBLK as u64 {
        if let Some(m) = ino.map.get_mut(k as usize) {
            *m = false;
        }
        k += 1;
    }
}

/// 写入。返回新快照。
pub fn write(ino: &mut Inode, off: i64, len: u64) -> Result<Out, u32> {
    if off < 0 {
        return Err(SP_EINVAL);
    }
    let o = off as u64;
    let end = match o.checked_add(len) {
        Some(v) => v,
        None => return Err(SP_EOVERFLOW),
    };
    if end > MAX_FILE {
        return Err(SP_EFBIG);
    }
    if len > 0 {
        let first = o / BSIZE;
        let last = (end - 1) / BSIZE;
        mark(ino, first, last);
        if end > ino.size {
            ino.size = end;
        }
    }
    Ok(Out { size: ino.size, alloc: ino.alloc(), total: 0, holes: 0 })
}

/// 截断：增长不分配，缩短回收整块。
pub fn truncate(ino: &mut Inode, new: i64) -> Result<Out, u32> {
    if new < 0 {
        return Err(SP_EINVAL);
    }
    let want = new as u64;
    if want > MAX_FILE {
        return Err(SP_EFBIG);
    }
    if want < ino.size {
        let first = (want + BSIZE - 1) / BSIZE;
        if first < NBLK as u64 {
            clear(ino, first, NBLK as u64 - 1);
        }
    }
    ino.size = want;
    Ok(Out { size: ino.size, alloc: ino.alloc(), total: 0, holes: 0 })
}

/// 打洞：只清除完全被覆盖的块。
pub fn punch(ino: &mut Inode, off: i64, len: u64) -> Result<Out, u32> {
    if off < 0 {
        return Err(SP_EINVAL);
    }
    let o = off as u64;
    let end = match o.checked_add(len) {
        Some(v) => v,
        None => return Err(SP_EINVAL),
    };
    if end > MAX_FILE {
        return Err(SP_EINVAL);
    }
    if len > 0 {
        let first = (o + BSIZE - 1) / BSIZE;
        let last_full = end / BSIZE;
        if first < last_full && first < NBLK as u64 {
            let last = if last_full == 0 { 0 } else { last_full - 1 };
            clear(ino, first, last);
        }
    }
    Ok(Out { size: ino.size, alloc: ino.alloc(), total: 0, holes: 0 })
}

/// 读取：返回可读字节与其中洞字节。
pub fn read(ino: &Inode, off: i64, len: u64) -> Result<Out, u32> {
    if off < 0 {
        return Err(SP_EINVAL);
    }
    let o = off as u64;
    if o >= ino.size || len == 0 {
        return Ok(Out { size: ino.size, alloc: ino.alloc(), total: 0, holes: 0 });
    }
    let end = match o.checked_add(len) {
        Some(v) => {
            if v > ino.size {
                ino.size
            } else {
                v
            }
        }
        None => ino.size,
    };
    let first = o / BSIZE;
    let last = (end - 1) / BSIZE;
    let mut total = 0u32;
    let mut holes = 0u32;
    let mut k = first;
    while k <= last && k < NBLK as u64 {
        let bstart = k * BSIZE;
        let bend = bstart + BSIZE;
        let lo = if o > bstart { o } else { bstart };
        let hi = if end < bend { end } else { bend };
        let span = hi.saturating_sub(lo);
        total = total.saturating_add(span as u32);
        if ino.map.get(k as usize) != Some(&true) {
            holes = holes.saturating_add(span as u32);
        }
        k += 1;
    }
    Ok(Out { size: ino.size, alloc: ino.alloc(), total, holes })
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut ino = base_inode();
    match id {
        0 => write(&mut ino, 0, 1024),
        1 => write(&mut ino, 4096, 512),
        2 => write(&mut ino, 5000, 120),
        3 => truncate(&mut ino, 8192),
        4 => punch(&mut ino, 0, 1024),
        5 => punch(&mut ino, 512, 512),
        6 => truncate(&mut ino, 1024),
        7 => read(&ino, 0, 1536),
        8 => read(&ino, 4000, 1000),
        9 => read(&ino, 4096, 512),
        10 => write(&mut ino, -1, 10),
        11 => write(&mut ino, 8192, 1),
        12 => punch(&mut ino, 4000, 5000),
        13 => truncate(&mut ino, -5),
        _ => Err(SP_EBADF),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub size: u64,
    pub alloc: u32,
    pub total: u32,
    pub holes: u32,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub size: u64,
    pub alloc: u32,
    pub total: u32,
    pub holes: u32,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, size: u64, alloc: u32, total: u32, holes: u32, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SP_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SP_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(SP_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(SP_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, size, alloc, total, holes, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.size != size || o.alloc != alloc || o.total != total || o.holes != holes {
                    return Err(SP_EMISMATCH);
                }
                Outcome { ok: true, size: o.size, alloc: o.alloc, total: o.total, holes: o.holes, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(SP_EMISMATCH);
                }
                Outcome { ok: false, size: 0, alloc: 0, total: 0, holes: 0, code: low }
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
            return Err(SP_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(SP_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(SP_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(SP_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.size != o.size || v.alloc != o.alloc || v.total != o.total || v.holes != o.holes {
                        return Err(SP_EMISMATCH);
                    }
                    if v.alloc > NBLK as u32 || v.holes > v.total || v.size > MAX_FILE {
                        return Err(SP_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(SP_EMISMATCH);
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
            return Err(SP_EFROZEN);
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
                h = fold(h, o.size);
                h = fold(h, o.alloc as u64);
                h = fold(h, o.total as u64);
                h = fold(h, o.holes as u64);
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
            Err(SP_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let einval = (SP_EINVAL & 0xffff) as u16;
    let efbig = (SP_EFBIG & 0xffff) as u16;
    let _ = l.register(0, 0, true, 4096, 4, 0, 0, 0);
    let _ = l.register(1, 1, true, 4608, 3, 0, 0, 0);
    let _ = l.register(2, 2, true, 5120, 4, 0, 0, 0);
    let _ = l.register(3, 3, true, 8192, 3, 0, 0, 0);
    let _ = l.register(4, 4, true, 4096, 2, 0, 0, 0);
    let _ = l.register(5, 5, true, 4096, 3, 0, 0, 0);
    let _ = l.register(6, 6, true, 1024, 1, 0, 0, 0);
    let _ = l.register(7, 7, true, 4096, 3, 1536, 512, 0);
    let _ = l.register(8, 8, true, 4096, 3, 96, 96, 0);
    let _ = l.register(9, 9, true, 4096, 3, 0, 0, 0);
    let _ = l.register(10, 10, false, 0, 0, 0, 0, einval);
    let _ = l.register(11, 11, false, 0, 0, 0, 0, efbig);
    let _ = l.register(12, 12, false, 0, 0, 0, 0, einval);
    let _ = l.register(13, 13, false, 0, 0, 0, 0, einval);
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
    let twice = l.freeze() == Err(SP_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut a = Inode { size: 0, map: [false; NBLK] };
    let w = write(&mut a, 500, 100);
    out[1] = w.map(|o| (o.size, o.alloc)) == Ok((600, 2))
        && a.map.get(0) == Some(&true)
        && a.map.get(1) == Some(&true)
        && a.alloc() == 2;

    let mut b = Inode { size: 0, map: [false; NBLK] };
    let grew = truncate(&mut b, 4096);
    let r = read(&b, 0, 4096);
    out[2] = grew.map(|o| o.alloc) == Ok(0)
        && r.map(|o| (o.total, o.holes)) == Ok((4096, 4096))
        && b.size == 4096;

    let mut c = Inode { size: 0, map: [false; NBLK] };
    let _ = write(&mut c, 0, 2048);
    let p = punch(&mut c, 512, 512);
    let after = read(&c, 0, 1024);
    out[3] = p.map(|o| o.alloc) == Ok(3)
        && after.map(|o| (o.total, o.holes)) == Ok((1024, 512))
        && write(&mut c, -1, 1) == Err(SP_EINVAL)
        && write(&mut c, MAX_FILE as i64, 1) == Err(SP_EFBIG);
    out
}

/// 十三项判据。
pub fn run_unx11473_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11473_sparse_holes");
    let l = build_ledger();
    s.add("SP1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "SP2-十应四拒",
        l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SP3-写入标记块",
        {
            let mut i = Inode { size: 0, map: [false; NBLK] };
            write(&mut i, 0, 1024).map(|o| (o.size, o.alloc)) == Ok((1024, 2))
                && i.alloc() == 2
        },
        "static",
    );
    s.add(
        "SP4-跨块写入",
        {
            let mut i = Inode { size: 0, map: [false; NBLK] };
            write(&mut i, 500, 100).map(|o| (o.size, o.alloc)) == Ok((600, 2))
        },
        "static",
    );
    s.add(
        "SP5-增长不分配",
        {
            let mut i = base_inode();
            truncate(&mut i, 8192).map(|o| (o.size, o.alloc)) == Ok((8192, 3))
        },
        "static",
    );
    s.add(
        "SP6-打洞清整块",
        {
            let mut i = base_inode();
            punch(&mut i, 0, 1024).map(|o| o.alloc) == Ok(2)
                && i.map.get(0) == Some(&false)
                && i.map.get(2) == Some(&true)
        },
        "static",
    );
    s.add(
        "SP7-缩短回收尾部",
        {
            let mut i = base_inode();
            truncate(&mut i, 1024).map(|o| (o.size, o.alloc)) == Ok((1024, 1))
        },
        "static",
    );
    s.add(
        "SP8-读取洞计数",
        {
            let i = base_inode();
            read(&i, 0, 1536).map(|o| (o.total, o.holes)) == Ok((1536, 512))
                && read(&i, 4000, 1000).map(|o| (o.total, o.holes)) == Ok((96, 96))
        },
        "static",
    );
    s.add(
        "SP9-越过 EOF 短读",
        {
            let i = base_inode();
            read(&i, 4096, 512).map(|o| o.total) == Ok(0)
                && read(&i, 0, 10000).map(|o| o.total) == Ok(4096)
        },
        "static",
    );
    s.add(
        "SP10-负偏移与超限",
        {
            let mut i = base_inode();
            write(&mut i, -1, 1) == Err(SP_EINVAL)
                && write(&mut i, MAX_FILE as i64, 1) == Err(SP_EFBIG)
                && punch(&mut i, 4000, 5000) == Err(SP_EINVAL)
                && truncate(&mut i, -1) == Err(SP_EINVAL)
        },
        "static",
    );
    s.add(
        "SP11-部分块写入不新增",
        {
            let mut i = base_inode();
            let before = i.alloc();
            write(&mut i, 100, 100).map(|o| (o.alloc, o.size)) == Ok((before, 4096))
        },
        "static",
    );
    s.add(
        "SP12-打洞保留部分块",
        {
            let mut i = base_inode();
            punch(&mut i, 100, 900).map(|o| o.alloc) == Ok(3)
                && i.map.get(0) == Some(&true)
                && i.map.get(1) == Some(&false)
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(SP_EFROZEN);
    s.add_dyn(
        "SP13-冻结重放与四路",
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
    fn write_allocates_blocks() {
        let mut i = Inode { size: 0, map: [false; NBLK] };
        assert_eq!(write(&mut i, 0, 1024).map(|o| o.alloc), Ok(2));
        assert_eq!(i.size, 1024);
    }

    #[test]
    fn grow_does_not_allocate() {
        let mut i = base_inode();
        assert_eq!(truncate(&mut i, 8192).map(|o| o.alloc), Ok(3));
    }

    #[test]
    fn punch_clears_full_blocks() {
        let mut i = base_inode();
        assert_eq!(punch(&mut i, 0, 1024).map(|o| o.alloc), Ok(2));
    }

    #[test]
    fn read_counts_holes() {
        let i = base_inode();
        assert_eq!(read(&i, 0, 1536).map(|o| (o.total, o.holes)), Ok((1536, 512)));
    }

    #[test]
    fn read_stops_at_eof() {
        let i = base_inode();
        assert_eq!(read(&i, 4000, 1000).map(|o| o.total), Ok(96));
        assert_eq!(read(&i, 4096, 1000).map(|o| o.total), Ok(0));
    }

    #[test]
    fn limits_are_enforced() {
        let mut i = base_inode();
        assert_eq!(write(&mut i, -1, 1), Err(SP_EINVAL));
        assert_eq!(write(&mut i, MAX_FILE as i64, 1), Err(SP_EFBIG));
        assert_eq!(punch(&mut i, 4000, 5000), Err(SP_EINVAL));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        let s = run_unx11473_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
