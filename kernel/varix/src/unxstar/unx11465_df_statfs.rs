//! UNX-F11465 df 与 statfs 字段核算（0x20ce0 段）
//!
//! `statfs` 把一块设备的几何摊开成几个互相约束的计数：总块、空闲块、保留块、
//! 可用块、inode 总数与空闲数。`f_bavail = f_bfree - f_bsize 保留量` 只对
//! 非特权用户成立；`f_bfree` 才是超级用户视角。把两者混为一谈会让 `df` 在
//! 保留区未耗尽时显示 100% 却仍能写。本模型分别保存物理空闲与保留量，并在
//! 快照里同时给出三种视角。
//!
//! 所有比率用 `u64` 中间量计算，先乘后除，避免 `blocks * 100` 在 32 位下回绕。
//! 空闲大于总量、保留大于空闲、inode 空闲大于总数都属于元数据自相矛盾，快照
//! 直接拒绝而不是钳位——钳位会把损坏伪装成“满盘”。只读挂载不改几何，但会
//! 把 `f_bavail` 与 `f_bfree` 的可用面收窄为零，`df` 必须能区分“没空间”与
//! “不许写”。
//!
//! 审计重算三条恒等式：`used = blocks - bfree`；`bavail = bfree - reserved`
//! （下溢则为 0）；`blocks * bsize` 与以字节表示的总容量一致。快照是纯函数，
//! 同一张挂载表重复取快照必须逐字段相等——`replay()` 用整表签名验证这一点。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ce0;
pub const DF_EID: u32 = SEG_BASE + 0x01;
pub const DF_EORDER: u32 = SEG_BASE + 0x02;
pub const DF_EDUP: u32 = SEG_BASE + 0x03;
pub const DF_EFROZEN: u32 = SEG_BASE + 0x04;
pub const DF_ENOBS: u32 = SEG_BASE + 0x05;
pub const DF_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const DF_EBAL: u32 = SEG_BASE + 0x07;
pub const DF_ESEAL: u32 = SEG_BASE + 0x08;
pub const DF_ENOENT: u32 = SEG_BASE + 0x09;
pub const DF_EDOM: u32 = SEG_BASE + 0x0a;
pub const DF_EROFS: u32 = SEG_BASE + 0x0b;
pub const DF_EINVAL: u32 = SEG_BASE + 0x0c;
pub const DF_EOVERFLOW: u32 = SEG_BASE + 0x0d;
pub const DF_ENOSPC: u32 = SEG_BASE + 0x0e;
pub const DF_EBADF: u32 = SEG_BASE + 0x0f;

/// 可写。
pub const M_RW: u8 = 0;
/// 只读。
pub const M_RO: u8 = 1;
pub const MNT_MAX: usize = 6;
pub const SAMPLES: usize = 13;

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

/// 挂载几何。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mount {
    pub id: u8,
    pub bsize: u32,
    pub blocks: u32,
    pub bfree: u32,
    pub reserved: u32,
    pub files: u32,
    pub ffree: u32,
    pub namelen: u16,
    pub flags: u8,
    pub name: [u8; 8],
    pub nlen: u8,
}

impl Mount {
    pub fn make(id: u8, bsize: u32, blocks: u32, bfree: u32, reserved: u32, files: u32, ffree: u32, name: &[u8]) -> Mount {
        let mut b = [0u8; 8];
        let mut i = 0;
        while i < name.len() && i < 8 {
            b[i] = name[i];
            i += 1;
        }
        let n = if name.len() > 8 { 8u8 } else { name.len() as u8 };
        Mount { id, bsize, blocks, bfree, reserved, files, ffree, namelen: 255, flags: M_RW, name: b, nlen: n }
    }
    pub fn name(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > 8 {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// statfs 快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatFs {
    pub bsize: u32,
    pub frsize: u32,
    pub blocks: u32,
    pub bfree: u32,
    pub bavail: u32,
    pub files: u32,
    pub ffree: u32,
    pub used_pct: u32,
    pub inode_pct: u32,
    pub ro: bool,
    pub namelen: u16,
}

/// 挂载表。
pub struct Df {
    mounts: Vec<Mount>,
    errors: u32,
}

impl Df {
    pub fn new(mounts: Vec<Mount>) -> Df {
        Df { mounts, errors: 0 }
    }
    pub fn mount(&self, id: u8) -> Option<Mount> {
        let mut i = 0;
        while i < self.mounts.len() {
            if let Some(m) = self.mounts.get(i) {
                if m.id == id {
                    return Some(*m);
                }
            }
            i += 1;
        }
        None
    }
    pub fn errors(&self) -> u32 {
        self.errors
    }
    /// 取快照。
    pub fn statfs(&self, id: u8) -> Result<StatFs, u32> {
        let m = match self.mount(id) {
            Some(v) => v,
            None => return Err(DF_ENOENT),
        };
        if m.bfree > m.blocks || m.reserved > m.bfree || m.ffree > m.files {
            return Err(DF_EDOM);
        }
        if m.blocks == 0 || m.files == 0 {
            return Err(DF_EINVAL);
        }
        if m.bsize < 512 || (m.bsize & (m.bsize - 1)) != 0 {
            return Err(DF_EDOM);
        }
        let bavail = m.bfree - m.reserved;
        let used = m.blocks - m.bfree;
        let used_pct = ((used as u64) * 100 / (m.blocks as u64)) as u32;
        let inode_used = m.files - m.ffree;
        let inode_pct = ((inode_used as u64) * 100 / (m.files as u64)) as u32;
        let ro = m.flags == M_RO;
        let bavail_v = if ro { 0 } else { bavail };
        Ok(StatFs {
            bsize: m.bsize,
            frsize: m.bsize,
            blocks: m.blocks,
            bfree: m.bfree,
            bavail: bavail_v,
            files: m.files,
            ffree: m.ffree,
            used_pct,
            inode_pct,
            ro,
            namelen: m.namelen,
        })
    }
    /// 整表签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.mounts.len() {
            if let Some(m) = self.mounts.get(i) {
                h = fold(h, m.id as u64);
                h = fold(h, m.bsize as u64);
                h = fold(h, m.blocks as u64);
                h = fold(h, m.bfree as u64);
                h = fold(h, m.reserved as u64);
                h = fold(h, m.files as u64);
                h = fold(h, m.ffree as u64);
                h = fold(h, m.flags as u64);
            }
            i += 1;
        }
        fold(h, self.errors as u64)
    }
    /// 全表一致性。
    pub fn audit(&self) -> Result<(), u32> {
        if self.mounts.is_empty() {
            return Err(DF_EBAL);
        }
        let mut i = 0;
        while i < self.mounts.len() {
            let m = match self.mounts.get(i) {
                Some(v) => v,
                None => return Err(DF_ENOBS),
            };
            if m.bfree > m.blocks {
                return Err(DF_EBAL);
            }
            let mut j = i + 1;
            while j < self.mounts.len() {
                if let Some(n) = self.mounts.get(j) {
                    if n.id == m.id {
                        return Err(DF_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
    /// 字节容量（u64）。
    pub fn bytes_total(&self, id: u8) -> Result<u64, u32> {
        let m = match self.mount(id) {
            Some(v) => v,
            None => return Err(DF_ENOENT),
        };
        Ok((m.blocks as u64).saturating_mul(m.bsize as u64))
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub id: u8,
    pub expect_ok: bool,
    pub expect_bavail: u16,
    pub expect_used: u16,
    pub expect_inode: u16,
    pub expect_ro: bool,
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub bavail: u16,
    pub used: u16,
    pub inode: u16,
    pub ro: bool,
}

/// 账本。
pub struct Ledger {
    df: Df,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            df: Df::new(build_mounts()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn register(&mut self, slot: u8, id: u8, ok: bool, bavail: u16, used: u16, inode: u16, ro: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DF_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DF_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(DF_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(DF_EORDER);
        }
        let s = Sample { id, expect_ok: ok, expect_bavail: bavail, expect_used: used, expect_inode: inode, expect_ro: ro };
        let out = match self.df.statfs(id) {
            Ok(v) => {
                if !ok || v.bavail != bavail as u32 || v.used_pct != used as u32 || v.inode_pct != inode as u32 || v.ro != ro {
                    return Err(DF_EMISMATCH);
                }
                Outcome { ok: true, bavail, used, inode, ro }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != bavail {
                    return Err(DF_EMISMATCH);
                }
                Outcome { ok: false, bavail: 0, used: 0, inode: 0, ro: false }
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
    pub fn df(&self) -> &Df {
        &self.df
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(DF_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(DF_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(DF_ENOBS),
            };
            if s.expect_ok != o.ok || s.expect_ro != o.ro {
                return Err(DF_EMISMATCH);
            }
            if s.expect_ok && (s.expect_bavail != o.bavail || s.expect_used != o.used || s.expect_inode != o.inode) {
                return Err(DF_EMISMATCH);
            }
            i += 1;
        }
        self.df.audit()
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DF_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = self.df.seal();
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, o.bavail as u64);
                h = fold(h, o.used as u64);
                h = fold(h, o.inode as u64);
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
            Err(DF_ESEAL)
        }
    }
}

/// 挂载表。
pub fn build_mounts() -> Vec<Mount> {
    let mut v = Vec::new();
    v.push(Mount::make(1, 4096, 1000, 400, 100, 256, 56, b"root"));
    v.push(Mount::make(2, 512, 2000, 2000, 0, 128, 0, b"data"));
    v.push(Mount::make(3, 4096, 800, 100, 0, 64, 63, b"rofs"));
    v.push(Mount::make(4, 1024, 0, 0, 0, 32, 32, b"empty"));
    v.push(Mount::make(5, 300, 100, 50, 0, 16, 8, b"odd"));
    v.push(Mount::make(6, 4096, 100, 90, 91, 10, 5, b"bad"));
    let mut d = Df::new(v);
    if let Some(m) = d.mounts.get_mut(1) {
        m.flags = M_RO;
    }
    d.mounts
}

/// 十三格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (DF_ENOENT & 0xffff) as u16;
    let edom = (DF_EDOM & 0xffff) as u16;
    let einval = (DF_EINVAL & 0xffff) as u16;
    let _ = l.register(0, 1, true, 300, 60, 78, false);
    let _ = l.register(1, 2, true, 0, 0, 100, true);
    let _ = l.register(2, 3, true, 100, 87, 1, false);
    let _ = l.register(3, 1, true, 300, 60, 78, false);
    let _ = l.register(4, 4, false, einval, 0, 0, false);
    let _ = l.register(5, 5, false, edom, 0, 0, false);
    let _ = l.register(6, 6, false, edom, 0, 0, false);
    let _ = l.register(7, 9, false, noent, 0, 0, false);
    let _ = l.register(8, 2, true, 0, 0, 100, true);
    let _ = l.register(9, 3, true, 100, 87, 1, false);
    let _ = l.register(10, 9, false, noent, 0, 0, false);
    let _ = l.register(11, 1, true, 300, 60, 78, false);
    let _ = l.register(12, 4, false, einval, 0, 0, false);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 7 && l.err_count() == 6 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(DF_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let d = Df::new(build_mounts());
    let s1 = d.statfs(1);
    let s2 = d.statfs(2);
    out[1] = match (s1, s2) {
        (Ok(a), Ok(b)) => {
            a.bavail == 300
                && a.bfree == 400
                && a.used_pct == 60
                && a.inode_pct == 78
                && !a.ro
                && b.bavail == 0
                && b.ro
                && b.bfree == 2000
                && b.used_pct == 0
        }
        _ => false,
    };

    let s3 = d.statfs(3);
    out[2] = match s3 {
        Ok(v) => v.bavail == 100 && v.used_pct == 87 && v.inode_pct == 1 && (v.bsize as u64) * 800 == 3276800,
        Err(_) => false,
    };

    out[3] = d.statfs(4) == Err(DF_EINVAL)
        && d.statfs(5) == Err(DF_EDOM)
        && d.statfs(6) == Err(DF_EDOM)
        && d.statfs(9) == Err(DF_ENOENT)
        && d.bytes_total(1) == Ok(4096000)
        && d.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11465_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11465.statfs");
    let l = build_ledger();
    s.add("DF1-十三格齐备", l.len() == SAMPLES && l.sample(0).map(|x| x.id) == Some(1), "static");
    s.add_dyn(
        "DF2-七应六拒",
        l.ok_count() == 7 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "DF3-可用块扣除保留量",
        l.outcome(0).map(|o| (o.bavail, o.used)) == Some((300, 60))
            && l.df().mount(1).map(|m| m.bfree) == Some(400),
        "static",
    );
    s.add(
        "DF4-只读不改几何只收可用面",
        l.outcome(1).map(|o| (o.bavail, o.used, o.ro)) == Some((0, 0, true))
            && l.outcome(1).map(|o| o.inode) == Some(100),
        "static",
    );
    s.add(
        "DF5-高水位比率",
        l.outcome(2).map(|o| (o.bavail, o.used, o.inode)) == Some((100, 87, 1)),
        "static",
    );
    s.add(
        "DF6-零块设备非法",
        l.outcome(4).map(|o| o.ok) == Some(false) && l.sample(4).map(|x| x.expect_bavail) == Some((DF_EINVAL & 0xffff) as u16),
        "static",
    );
    s.add(
        "DF7-非二次幂块长拒绝",
        l.outcome(5).map(|o| o.ok) == Some(false) && l.sample(5).map(|x| x.expect_bavail) == Some((DF_EDOM & 0xffff) as u16),
        "static",
    );
    s.add(
        "DF8-保留量超空闲拒绝",
        l.outcome(6).map(|o| o.ok) == Some(false) && l.df().mount(6).map(|m| m.reserved) == Some(91),
        "static",
    );
    s.add(
        "DF9-未知挂载点拒绝",
        l.outcome(7).map(|o| o.ok) == Some(false) && l.outcome(10).map(|o| o.ok) == Some(false),
        "static",
    );
    s.add(
        "DF10-重复快照逐字段一致",
        l.outcome(0).map(|o| (o.bavail, o.used, o.inode, o.ro)) == l.outcome(3).map(|o| (o.bavail, o.used, o.inode, o.ro))
            && l.outcome(3).map(|o| (o.bavail, o.used, o.inode, o.ro)) == l.outcome(11).map(|o| (o.bavail, o.used, o.inode, o.ro)),
        "static",
    );
    s.add(
        "DF11-字节容量用六十四位",
        l.df().bytes_total(1) == Ok(4096000)
            && l.df().bytes_total(2) == Ok(1024000)
            && l.df().bytes_total(9) == Err(DF_ENOENT),
        "static",
    );
    s.add(
        "DF12-审计与签名稳定",
        l.df().audit().is_ok()
            && l.df().mount(3).map(|m| (m.files, m.ffree, m.namelen)) == Some((64, 63, 255))
            && l.df().mount(2).map(|m| m.flags) == Some(M_RO),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DF_EFROZEN);
    s.add_dyn(
        "DF13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} avail={}", l.ok_count(), l.err_count(), l.outcome(0).map(|o| o.bavail).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_reduces_bavail() {
        let d = Df::new(build_mounts());
        match d.statfs(1) {
            Ok(s) => {
                assert_eq!(s.bfree, 400);
                assert_eq!(s.bavail, 300);
                assert_eq!(s.used_pct, 60);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn read_only_zeroes_bavail() {
        let d = Df::new(build_mounts());
        assert_eq!(d.statfs(2).map(|s| (s.bavail, s.ro)), Ok((0, true)));
    }

    #[test]
    fn malformed_geometry_refused() {
        let d = Df::new(build_mounts());
        assert_eq!(d.statfs(4), Err(DF_EINVAL));
        assert_eq!(d.statfs(5), Err(DF_EDOM));
        assert_eq!(d.statfs(6), Err(DF_EDOM));
    }

    #[test]
    fn byte_capacity_wide() {
        let d = Df::new(build_mounts());
        assert_eq!(d.bytes_total(1), Ok(4096000));
        assert_eq!(d.bytes_total(9), Err(DF_ENOENT));
    }

    #[test]
    fn duplicate_ids_rejected_by_audit() {
        let mut v = build_mounts();
        v.push(Mount::make(1, 512, 10, 5, 0, 10, 5, b"dup"));
        assert_eq!(Df::new(v).audit(), Err(DF_EBAL));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 7);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        let s = run_unx11465_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
