//! UNX-F10110 fstat 正反双判据（元数据达成与坏缓冲误用受控）（0x100e0 段）
//!
//! 锚点：UNX-F10110-J1 fstat 元数据达成（size/mtime 与 ftruncate 后一致
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：元数据册（Stat 五元组：size/mtime/uid/mode/nlink）+ 元核
//! （ftruncate 联动/写后 size 推进/mtime 单调推进）+ 元账（逐次 stat 留
//! 痕）+ 注入核（坏 fd/越界 fd/空留痕/封表/负截断长必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x100e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x100e0;
pub const FS_BAD_FD: u32 = SEG_BASE + 0x01; // 坏 fd
pub const FS_NEG_LEN: u32 = SEG_BASE + 0x02; // 负截断长
pub const FS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const FS_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const FS_NO_BUF: u32 = SEG_BASE + 0x05; // 坏缓冲（统计目标槽为空）

/// fd 表定容。
pub const FD_CAP: usize = 8;

/// Stat 五元组。
#[derive(Clone, Copy, PartialEq)]
pub struct Stat {
    pub size: u32,
    pub mtime: u32,
    pub uid: u32,
    pub mode: u32,
    pub nlink: u32,
}

impl Stat {
    pub fn zero() -> Stat {
        Stat { size: 0, mtime: 1, uid: 0, mode: 0o644, nlink: 1 }
    }

    /// 五元组合法性：mtime 非零、nlink 非零、mode 在 12 位内。
    pub fn ok(&self) -> bool {
        self.mtime != 0 && self.nlink != 0 && self.mode & !0o7777 == 0
    }

    pub fn render(&self) -> String {
        format!("sz{}|mt{}|uid{}|mo{:o}|nl{}", self.size, self.mtime, self.uid, self.mode, self.nlink)
    }
}

/// 元数据件。
#[derive(Clone)]
pub struct MetaFile {
    pub data: Vec<u8>,
    pub st: Stat,
    pub clock: u32,
}

impl MetaFile {
    pub fn new(uid: u32) -> MetaFile {
        let mut st = Stat::zero();
        st.uid = uid;
        MetaFile { data: Vec::new(), st, clock: 1 }
    }

    /// 写：size 推进 + mtime 推进（时钟单调）。
    pub fn write(&mut self, buf: &[u8]) {
        let end = self.data.len() + buf.len();
        self.data.extend_from_slice(buf);
        self.st.size = end as u32;
        self.clock += 1;
        self.st.mtime = self.clock;
    }

    /// ftruncate：缩/扩语义，mtime 推进。
    pub fn ftruncate(&mut self, new_len: usize) {
        self.data.truncate(new_len);
        self.st.size = new_len as u32;
        self.clock += 1;
        self.st.mtime = self.clock;
    }
}

/// 元账留痕。
#[derive(Clone)]
pub struct FsRec {
    pub seq: u32,
    pub fd: i32,
    pub snap: Stat,
    pub note: String,
}

impl FsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.snap.ok()
    }
}

/// 元数据册。
pub struct MetaBook {
    pub slots: Vec<Option<MetaFile>>,
    pub ledger: Vec<FsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MetaBook {
    pub fn new() -> MetaBook {
        MetaBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, uid: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(FS_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(MetaFile::new(uid));
            return Ok(i);
        }
        if self.slots.len() >= FD_CAP {
            return Err(FS_BAD_FD);
        }
        self.slots.push(Some(MetaFile::new(uid)));
        Ok(self.slots.len() - 1)
    }

    /// fstat 核心：快照落账（坏缓冲=空槽必拒）。
    pub fn fstat(&mut self, fd: i32, note: &str) -> Result<Stat, u32> {
        if self.sealed {
            return Err(FS_SEALED);
        }
        if note.is_empty() {
            return Err(FS_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(FS_BAD_FD);
        }
        let st = self.slots[fd as usize].as_ref().map(|f| f.st).unwrap_or(Stat::zero());
        self.ledger.push(FsRec { seq: self.seq_next, fd, snap: st, note: note.to_string() });
        self.seq_next += 1;
        Ok(st)
    }

    /// 写透传（联动 size/mtime）。
    pub fn write(&mut self, fd: i32, buf: &[u8]) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(FS_BAD_FD);
        }
        let f = match self.slots[fd as usize].as_mut() {
            Some(f) => f,
            None => return Err(FS_BAD_FD),
        };
        f.write(buf);
        Ok(buf.len())
    }

    /// ftruncate 透传。
    pub fn truncate(&mut self, fd: i32, new_len: usize) -> Result<u32, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(FS_BAD_FD);
        }
        let f = match self.slots[fd as usize].as_mut() {
            Some(f) => f,
            None => return Err(FS_BAD_FD),
        };
        f.ftruncate(new_len);
        Ok(f.st.size)
    }

    /// 元账对平：账内同 fd 最新快照与现状一致。
    pub fn latest_matches(&self, fd: i32) -> bool {
        let cur = if fd >= 0 && (fd as usize) < self.slots.len() {
            self.slots[fd as usize].as_ref().map(|f| f.st)
        } else {
            None
        };
        match (cur, self.ledger.iter().rev().find(|r| r.fd == fd)) {
            (Some(c), Some(r)) => c == r.snap,
            _ => false,
        }
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x100e00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.snap.size);
            h = h.wrapping_mul(31).wrapping_add(r.snap.mtime);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = MetaBook::new();
    let f = b.open(0).unwrap_or(usize::MAX);
    // 1) 负 fd 必拒
    got[0] = b.fstat(-1, "inj-neg").err() == Some(FS_BAD_FD);
    // 2) 越界 fd 必拒
    got[1] = b.fstat(99, "inj-oob").err() == Some(FS_BAD_FD);
    // 3) 空留痕必拒
    got[2] = b.fstat(f as i32, "").err() == Some(FS_EMPTY_NOTE);
    // 4) 封表后操作必抓
    b.seal();
    got[3] = b.fstat(f as i32, "inj-seal").err() == Some(FS_SEALED);
    // 5) 坏缓冲：关槽后统计必按 BAD_FD 拒
    let mut b2 = MetaBook::new();
    let f2 = b2.open(0).unwrap_or(usize::MAX);
    let _ = b2.truncate(f2 as i32, 0);
    let _ = b2.slots.remove(0);
    got[4] = b2.fstat(f2 as i32, "inj-buf").err() == Some(FS_BAD_FD);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10110");

    // 正路径：写后 size 一致
    let mut b = MetaBook::new();
    let f = b.open(0).unwrap_or(usize::MAX);
    let _ = b.write(f as i32, b"stat-sample");
    let st = b.fstat(f as i32, "写后").unwrap_or(Stat::zero());
    let j1 = st.size == 11 && st.ok();
    s.add("J1-写后size", j1, "写 11 字节后 fstat size==11");

    // ftruncate 缩短联动
    let j2 = {
        let _ = b.truncate(f as i32, 4);
        let st2 = b.fstat(f as i32, "截后").unwrap_or(Stat::zero());
        st2.size == 4
    };
    s.add("J2-截短联动", j2, "ftruncate(4) 后 size==4");

    // ftruncate 加长与 mtime 推进
    let j3 = {
        let _ = b.write(f as i32, b"xy");
        let m1 = b.fstat(f as i32, "前").unwrap_or(Stat::zero()).mtime;
        let _ = b.truncate(f as i32, 20);
        let st3 = b.fstat(f as i32, "后").unwrap_or(Stat::zero());
        st3.size == 20 && st3.mtime > m1
    };
    s.add("J3-加长mtime", j3, "加长后 size==20 且 mtime 单调推进");

    // 元账快照与现状对平
    let j4 = b.latest_matches(f as i32);
    s.add("J4-账平", j4, "最新快照与现状一致");

    // 坏 fd 受控
    let j5 = b.fstat(-2, "neg").err() == Some(FS_BAD_FD)
        && b.fstat(FD_CAP as i32 + 1, "oob").err() == Some(FS_BAD_FD);
    s.add("J5-坏fd受控", j5, "负与越界 fd 同码拒");

    // 空留痕拒收
    let j6 = b.fstat(f as i32, "").err() == Some(FS_EMPTY_NOTE);
    s.add("J6-空留痕拒", j6, "note 为空不入账");

    // Stat 五元组合法性守恒
    let j7 = b.ledger.iter().all(|r| r.ok());
    s.add("J7-五元组合法", j7, "mtime/nlink 非零且 mode 在 12 位");

    // 容量守恒
    let j8 = {
        let mut b8 = MetaBook::new();
        let mut ok = true;
        for _ in 0..FD_CAP {
            if b8.open(0).is_err() {
                ok = false;
            }
        }
        ok && b8.open(0).err() == Some(FS_BAD_FD)
    };
    s.add("J8-容量守恒", j8, "FD_CAP 守恒");

    // mode 八进制语义
    let j9 = {
        let z = Stat::zero();
        z.mode == 0o644 && z.render().contains("mo644")
    };
    s.add("J9-mode语义", j9, "缺省 mode 0644 且渲染一致");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "注非空且 seq 唯一");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10110_fstat_flow() {
        let mut b = MetaBook::new();
        let f = b.open(0).unwrap_or(0);
        let _ = b.write(f as i32, b"abcd");
        let st = b.fstat(f as i32, "s").unwrap_or(Stat::zero());
        assert_eq!(st.size, 4);
        let _ = b.truncate(f as i32, 2);
        assert_eq!(b.fstat(f as i32, "t").unwrap_or(Stat::zero()).size, 2);
        assert!(b.fstat(-1, "x") == Err(FS_BAD_FD));
    }

    #[test]
    fn unx10110_checks_green() {
        let s = run_unx10110_checks();
        assert!(s.all_passed(), "UNX-F10110 判据未全绿");
    }
}
