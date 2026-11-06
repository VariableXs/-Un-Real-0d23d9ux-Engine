//! UNX-F11472 truncate 与 ftruncate 语义（0x20d50 段）
//!
//! 截断文件只改长度，不改文件偏移：`ftruncate` 之后当前 `lseek` 位置原样
//! 保留，即使它已经越过新的文件末尾，下一次写入仍然落在那个偏移并把
//! 中间的洞补零。增长时新区域逻辑上为零，块数按 `ceil(size/bsize)` 计算；
//! 缩短到块边界以内会释放整块，落在部分块内则保留该块。
//!
//! 拒绝条件有明确优先级：坏描述符、目录、只读文件系统、不可变文件、
//! 只读描述符、负长度、超过上限。长度为 0 合法；长度不变时是空操作，
//! 不更新 mtime/ctime，这一点常被用来实现“探测”而不是“修改”。
//!
//! 不变量：`blocks == ceil(size/bsize)`；`size` 永不超过 `MAX_SIZE`；
//! 成功修改长度必然把时间戳推进一格；失败路径不改变 inode 或偏移。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20d50;
pub const TR_EID: u32 = SEG_BASE + 0x01;
pub const TR_EORDER: u32 = SEG_BASE + 0x02;
pub const TR_EDUP: u32 = SEG_BASE + 0x03;
pub const TR_EFROZEN: u32 = SEG_BASE + 0x04;
pub const TR_ENOBS: u32 = SEG_BASE + 0x05;
pub const TR_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const TR_EBAL: u32 = SEG_BASE + 0x07;
pub const TR_ESEAL: u32 = SEG_BASE + 0x08;
pub const TR_EACCES: u32 = SEG_BASE + 0x09;
pub const TR_EPERM: u32 = SEG_BASE + 0x0a;
pub const TR_EROFS: u32 = SEG_BASE + 0x0b;
pub const TR_EISDIR: u32 = SEG_BASE + 0x0c;
pub const TR_EINVAL: u32 = SEG_BASE + 0x0d;
pub const TR_EFBIG: u32 = SEG_BASE + 0x0e;
pub const TR_EBADF: u32 = SEG_BASE + 0x0f;

pub const BSIZE: u32 = 512;
pub const MAX_SIZE: u64 = 1u64 << 40;
pub const SAMPLES: usize = 14;

pub const FL_IMMUTABLE: u8 = 1;
pub const FL_ROFS: u8 = 2;
pub const FL_APPEND: u8 = 4;

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

/// 文件描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fd {
    pub open: bool,
    pub writable: bool,
    pub pos: u64,
}

/// inode 的长度状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub size: u64,
    pub blocks: u64,
    pub bsize: u32,
    pub flags: u8,
    pub is_dir: bool,
    pub mtime: u32,
}

/// 结果快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Out {
    pub size: u64,
    pub blocks: u64,
    pub pos: u64,
    pub mtime: u32,
}

/// 块数计算。
pub fn block_count(size: u64, bsize: u32) -> u64 {
    if size == 0 || bsize == 0 {
        0
    } else {
        size.saturating_add(bsize as u64 - 1) / bsize as u64
    }
}

/// ftruncate 语义。
pub fn ftruncate(fd: &Fd, ino: &mut Inode, new: i64) -> Result<Out, u32> {
    if !fd.open {
        return Err(TR_EBADF);
    }
    if ino.is_dir {
        return Err(TR_EISDIR);
    }
    if ino.flags & FL_ROFS != 0 {
        return Err(TR_EROFS);
    }
    if ino.flags & FL_IMMUTABLE != 0 {
        return Err(TR_EPERM);
    }
    if !fd.writable {
        return Err(TR_EACCES);
    }
    if new < 0 {
        return Err(TR_EINVAL);
    }
    let want = new as u64;
    if want > MAX_SIZE {
        return Err(TR_EFBIG);
    }
    if want == ino.size {
        return Ok(Out { size: ino.size, blocks: ino.blocks, pos: fd.pos, mtime: ino.mtime });
    }
    ino.size = want;
    ino.blocks = block_count(want, ino.bsize);
    ino.mtime = ino.mtime.wrapping_add(1);
    Ok(Out { size: ino.size, blocks: ino.blocks, pos: fd.pos, mtime: ino.mtime })
}

/// 基础 inode。
pub fn base_inode() -> Inode {
    Inode { size: 5000, blocks: block_count(5000, BSIZE), bsize: BSIZE, flags: 0, is_dir: false, mtime: 10 }
}

/// 基础描述符。
pub fn base_fd() -> Fd {
    Fd { open: true, writable: true, pos: 3000 }
}

/// 场景执行。
pub fn run_scen(id: u8) -> Result<Out, u32> {
    let mut ino = base_inode();
    let mut fd = base_fd();
    match id {
        0 => ftruncate(&fd, &mut ino, 8000),
        1 => ftruncate(&fd, &mut ino, 1000),
        2 => ftruncate(&fd, &mut ino, 0),
        3 => ftruncate(&fd, &mut ino, 5120),
        4 => ftruncate(&fd, &mut ino, 5000),
        5 => {
            fd.writable = false;
            ftruncate(&fd, &mut ino, 100)
        }
        6 => {
            ino.flags = FL_IMMUTABLE;
            ftruncate(&fd, &mut ino, 100)
        }
        7 => {
            ino.flags = FL_ROFS;
            ftruncate(&fd, &mut ino, 100)
        }
        8 => {
            ino.is_dir = true;
            ftruncate(&fd, &mut ino, 100)
        }
        9 => ftruncate(&fd, &mut ino, -1),
        10 => ftruncate(&fd, &mut ino, (MAX_SIZE + 1) as i64),
        11 => {
            fd.open = false;
            ftruncate(&fd, &mut ino, 100)
        }
        12 => ftruncate(&fd, &mut ino, 100),
        13 => {
            ino.size = 0;
            ino.blocks = 0;
            ftruncate(&fd, &mut ino, 3)
        }
        _ => Err(TR_EID),
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub size: u64,
    pub blocks: u64,
    pub pos: u64,
    pub mtime: u32,
    pub code: u16,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub size: u64,
    pub blocks: u64,
    pub pos: u64,
    pub mtime: u32,
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
    pub fn register(&mut self, slot: u8, scen: u8, ok: bool, size: u64, blocks: u64, pos: u64, mtime: u32, code: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TR_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(TR_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(TR_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(TR_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, size, blocks, pos, mtime, code };
        let out = match run_scen(scen) {
            Ok(o) => {
                if !ok || o.size != size || o.blocks != blocks || o.pos != pos || o.mtime != mtime {
                    return Err(TR_EMISMATCH);
                }
                Outcome { ok: true, size: o.size, blocks: o.blocks, pos: o.pos, mtime: o.mtime, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(TR_EMISMATCH);
                }
                Outcome { ok: false, size: 0, blocks: 0, pos: 0, mtime: 0, code: low }
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
            return Err(TR_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(TR_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(TR_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(TR_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok(v) => {
                    if !o.ok || v.size != o.size || v.blocks != o.blocks || v.pos != o.pos || v.mtime != o.mtime {
                        return Err(TR_EMISMATCH);
                    }
                    if v.blocks != block_count(v.size, BSIZE) || v.size > MAX_SIZE {
                        return Err(TR_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(TR_EMISMATCH);
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
            return Err(TR_EFROZEN);
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
                h = fold(h, o.blocks);
                h = fold(h, o.pos);
                h = fold(h, o.mtime as u64);
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
            Err(TR_ESEAL)
        }
    }
}

/// 十四行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eacces = (TR_EACCES & 0xffff) as u16;
    let eperm = (TR_EPERM & 0xffff) as u16;
    let erofs = (TR_EROFS & 0xffff) as u16;
    let eisdir = (TR_EISDIR & 0xffff) as u16;
    let einval = (TR_EINVAL & 0xffff) as u16;
    let efbig = (TR_EFBIG & 0xffff) as u16;
    let ebadf = (TR_EBADF & 0xffff) as u16;
    let _ = l.register(0, 0, true, 8000, 16, 3000, 11, 0);
    let _ = l.register(1, 1, true, 1000, 2, 3000, 11, 0);
    let _ = l.register(2, 2, true, 0, 0, 3000, 11, 0);
    let _ = l.register(3, 3, true, 5120, 10, 3000, 11, 0);
    let _ = l.register(4, 4, true, 5000, 10, 3000, 10, 0);
    let _ = l.register(5, 5, false, 0, 0, 0, 0, eacces);
    let _ = l.register(6, 6, false, 0, 0, 0, 0, eperm);
    let _ = l.register(7, 7, false, 0, 0, 0, 0, erofs);
    let _ = l.register(8, 8, false, 0, 0, 0, 0, eisdir);
    let _ = l.register(9, 9, false, 0, 0, 0, 0, einval);
    let _ = l.register(10, 10, false, 0, 0, 0, 0, efbig);
    let _ = l.register(11, 11, false, 0, 0, 0, 0, ebadf);
    let _ = l.register(12, 12, true, 100, 1, 3000, 11, 0);
    let _ = l.register(13, 13, true, 3, 1, 3000, 11, 0);
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
    let twice = l.freeze() == Err(TR_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let mut ino = Inode { size: 0, blocks: 0, bsize: BSIZE, flags: 0, is_dir: false, mtime: 10 };
    let fd = Fd { open: true, writable: true, pos: 10 };
    let grow = ftruncate(&fd, &mut ino, 1024);
    out[1] = grow.map(|o| (o.size, o.blocks, o.pos, o.mtime)) == Ok((1024, 2, 10, 11))
        && ino.size == 1024
        && ino.blocks == 2
        && fd.pos == 10;

    let mut mid = base_inode();
    let shrink = ftruncate(&base_fd(), &mut mid, 513);
    out[2] = shrink.map(|o| (o.size, o.blocks)) == Ok((513, 2))
        && block_count(513, BSIZE) == 2
        && block_count(512, BSIZE) == 1
        && block_count(0, BSIZE) == 0;

    let mut im = base_inode();
    im.flags = FL_IMMUTABLE;
    let mut ro = base_inode();
    ro.flags = FL_ROFS;
    let read_only = Fd { open: true, writable: false, pos: 0 };
    out[3] = ftruncate(&base_fd(), &mut im, 1) == Err(TR_EPERM)
        && ftruncate(&base_fd(), &mut ro, 1) == Err(TR_EROFS)
        && ftruncate(&read_only, &mut base_inode(), 1) == Err(TR_EACCES)
        && ftruncate(&base_fd(), &mut base_inode(), -1) == Err(TR_EINVAL)
        && ftruncate(&base_fd(), &mut base_inode(), (MAX_SIZE + 1) as i64) == Err(TR_EFBIG);
    out
}

/// 十三项判据。
pub fn run_unx11472_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11472_truncate");
    let l = build_ledger();
    s.add("TR1-十四行齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0), "static");
    s.add_dyn(
        "TR2-七应七拒",
        l.ok_count() == 7 && l.err_count() == 7 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "TR3-增长按块取整",
        {
            let mut i = Inode { size: 0, blocks: 0, bsize: BSIZE, flags: 0, is_dir: false, mtime: 1 };
            ftruncate(&base_fd(), &mut i, 8000).map(|o| (o.size, o.blocks)) == Ok((8000, 16))
        },
        "static",
    );
    s.add(
        "TR4-缩短释放整块",
        {
            let mut i = base_inode();
            ftruncate(&base_fd(), &mut i, 1000).map(|o| (o.size, o.blocks)) == Ok((1000, 2))
        },
        "static",
    );
    s.add(
        "TR5-零长度合法",
        {
            let mut i = base_inode();
            ftruncate(&base_fd(), &mut i, 0).map(|o| (o.size, o.blocks)) == Ok((0, 0))
        },
        "static",
    );
    s.add(
        "TR6-边界不增块",
        {
            let mut i = base_inode();
            ftruncate(&base_fd(), &mut i, 5120).map(|o| (o.blocks, o.mtime)) == Ok((10, 11))
                && block_count(5120, BSIZE) == 10
        },
        "static",
    );
    s.add(
        "TR7-同长度不更新时间",
        {
            let mut i = base_inode();
            ftruncate(&base_fd(), &mut i, 5000).map(|o| (o.size, o.mtime)) == Ok((5000, 10))
        },
        "static",
    );
    s.add(
        "TR8-只读描述符与坏描述符",
        {
            let mut i = base_inode();
            let ro = Fd { open: true, writable: false, pos: 0 };
            let closed = Fd { open: false, writable: true, pos: 0 };
            ftruncate(&ro, &mut i, 10) == Err(TR_EACCES)
                && ftruncate(&closed, &mut i, 10) == Err(TR_EBADF)
        },
        "static",
    );
    s.add(
        "TR9-不可变与只读文件系统",
        {
            let mut a = base_inode();
            a.flags = FL_IMMUTABLE;
            let mut b = base_inode();
            b.flags = FL_ROFS;
            ftruncate(&base_fd(), &mut a, 10) == Err(TR_EPERM)
                && ftruncate(&base_fd(), &mut b, 10) == Err(TR_EROFS)
        },
        "static",
    );
    s.add(
        "TR10-目录与负长度",
        {
            let mut a = base_inode();
            a.is_dir = true;
            ftruncate(&base_fd(), &mut a, 10) == Err(TR_EISDIR)
                && ftruncate(&base_fd(), &mut base_inode(), -5) == Err(TR_EINVAL)
        },
        "static",
    );
    s.add(
        "TR11-长度上限",
        {
            let mut a = Inode { size: 0, blocks: 0, bsize: BSIZE, flags: 0, is_dir: false, mtime: 1 };
            ftruncate(&base_fd(), &mut a, MAX_SIZE as i64).is_ok()
                && ftruncate(&base_fd(), &mut a, (MAX_SIZE + 1) as i64) == Err(TR_EFBIG)
                && a.size == MAX_SIZE
        },
        "static",
    );
    s.add(
        "TR12-偏移保持",
        {
            let mut i = base_inode();
            let out = ftruncate(&base_fd(), &mut i, 100);
            out.map(|o| (o.size, o.pos)) == Ok((100, 3000)) && base_fd().pos == 3000
        },
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TR_EFROZEN);
    s.add_dyn(
        "TR13-冻结重放与四路",
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
    fn blocks_round_up() {
        assert_eq!(block_count(0, BSIZE), 0);
        assert_eq!(block_count(1, BSIZE), 1);
        assert_eq!(block_count(512, BSIZE), 1);
        assert_eq!(block_count(513, BSIZE), 2);
    }

    #[test]
    fn grow_and_shrink() {
        let mut i = Inode { size: 0, blocks: 0, bsize: BSIZE, flags: 0, is_dir: false, mtime: 1 };
        assert_eq!(ftruncate(&base_fd(), &mut i, 1024).map(|o| o.blocks), Ok(2));
        assert_eq!(ftruncate(&base_fd(), &mut i, 0).map(|o| o.blocks), Ok(0));
    }

    #[test]
    fn same_size_is_noop() {
        let mut i = base_inode();
        assert_eq!(ftruncate(&base_fd(), &mut i, 5000).map(|o| o.mtime), Ok(10));
    }

    #[test]
    fn read_only_fd_refused() {
        let mut i = base_inode();
        let fd = Fd { open: true, writable: false, pos: 0 };
        assert_eq!(ftruncate(&fd, &mut i, 10), Err(TR_EACCES));
    }

    #[test]
    fn flags_are_checked_first() {
        let mut i = base_inode();
        i.flags = FL_IMMUTABLE;
        assert_eq!(ftruncate(&base_fd(), &mut i, 10), Err(TR_EPERM));
        i.flags = FL_ROFS;
        assert_eq!(ftruncate(&base_fd(), &mut i, 10), Err(TR_EROFS));
    }

    #[test]
    fn offset_survives_truncation() {
        let mut i = base_inode();
        assert_eq!(ftruncate(&base_fd(), &mut i, 100).map(|o| o.pos), Ok(3000));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.err_count(), 7);
        assert!(l.audit().is_ok());
        let s = run_unx11472_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
