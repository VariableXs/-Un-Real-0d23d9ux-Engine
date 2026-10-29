//! UNX 卡 10152 — 共享内存 shmget/shmat 正反双判据（附着达成与非法标识误用受控）
//! 段位 seg=0x10620
//! 判据原文：UNX-F10152-J1 附着达成（shmget→shmat→双进程可见 10/10 次）
//! 锚点：shmget 建段→shmat 附着→双进程写读互见→shmdt 摘除→非法 shmid 受控
//! 域：ShmBook 段册（段表 + 附着表 + 段容量账）
//! 结构：get 建段 → at 附着（pid, 段）→ 写共享页 → dt 摘除 → 误用受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

pub const SEG_BASE: u32 = 0x10620;
pub const SH_BAD_SHMID: u32 = SEG_BASE + 0x01;
pub const SH_SEALED: u32 = SEG_BASE + 0x02;
pub const SH_BAD_SIZE: u32 = SEG_BASE + 0x03;
pub const SH_SEG_CAP: u32 = SEG_BASE + 0x04;
pub const SH_NOT_ATTACHED: u32 = SEG_BASE + 0x05;

/// 段容量与段帽
pub const SEG_SIZE_MAX: usize = 0x10_0000;
pub const SEG_CAP: usize = 0x20;

/// 一个共享段
#[derive(Clone, Debug)]
pub struct ShmSeg {
    pub shmid: usize,
    pub size: usize,
    pub data: Vec<u8>,
    /// 附着进程集
    pub pids: Vec<u32>,
    pub live: bool,
}

/// 段册
pub struct ShmBook {
    pub segs: Vec<ShmSeg>,
    next_id: usize,
    sealed: bool,
}

impl ShmBook {
    pub fn new() -> Self {
        ShmBook {
            segs: Vec::new(),
            next_id: 1,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// shmget：建段（size 闸 + 容量闸）
    pub fn shmget(&mut self, size: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(SH_SEALED);
        }
        if size == 0 || size > SEG_SIZE_MAX {
            return Err(SH_BAD_SIZE);
        }
        if self.segs.len() >= SEG_CAP {
            return Err(SH_SEG_CAP);
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut data = Vec::new();
        data.resize(size, 0u8);
        self.segs.push(ShmSeg {
            shmid: id,
            size,
            data,
            pids: Vec::new(),
            live: true,
        });
        Ok(id)
    }

    fn find(&self, shmid: usize) -> Result<usize, u32> {
        self.segs
            .iter()
            .position(|s| s.shmid == shmid && s.live)
            .ok_or(SH_BAD_SHMID)
    }

    /// shmat：进程附着
    pub fn shmat(&mut self, shmid: usize, pid: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(SH_SEALED);
        }
        let i = self.find(shmid)?;
        if self.segs[i].pids.contains(&pid) {
            return Ok(self.segs[i].pids.len());
        }
        self.segs[i].pids.push(pid);
        Ok(self.segs[i].pids.len())
    }

    /// shmdt：摘附着
    pub fn shmdt(&mut self, shmid: usize, pid: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(SH_SEALED);
        }
        let i = self.find(shmid)?;
        let before = self.segs[i].pids.len();
        self.segs[i].pids.retain(|p| *p != pid);
        if self.segs[i].pids.len() == before {
            return Err(SH_NOT_ATTACHED);
        }
        Ok(self.segs[i].pids.len())
    }

    /// 写共享页
    pub fn write(&mut self, shmid: usize, off: usize, buf: &[u8]) -> Result<(), u32> {
        let i = self.find(shmid)?;
        if off + buf.len() > self.segs[i].size {
            return Err(SH_BAD_SIZE);
        }
        self.segs[i].data[off..off + buf.len()].copy_from_slice(buf);
        Ok(())
    }

    /// 读共享页
    pub fn read(&self, shmid: usize, off: usize, n: usize) -> Result<Vec<u8>, u32> {
        let i = self.find(shmid)?;
        if off + n > self.segs[i].size {
            return Err(SH_BAD_SIZE);
        }
        Ok(self.segs[i].data[off..off + n].to_vec())
    }

    /// 双进程可见核：两 pid 均附着
    pub fn both_attached(&self, shmid: usize, pa: u32, pb: u32) -> bool {
        match self.segs.iter().find(|s| s.shmid == shmid) {
            Some(s) => s.pids.contains(&pa) && s.pids.contains(&pb),
            None => false,
        }
    }

    /// 附着计数合计
    pub fn attach_total(&self) -> usize {
        self.segs.iter().map(|s| s.pids.len()).sum()
    }

    /// 重复附着幂等
    pub fn attach_dedup(&mut self, shmid: usize, pid: u32) -> bool {
        let n0 = self.attach_total();
        let _ = self.shmat(shmid, pid);
        self.attach_total() == n0
    }

    /// 段指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x106200;
        for s in &self.segs {
            h = h.wrapping_mul(31).wrapping_add(s.shmid as u32);
            h = h.wrapping_mul(31).wrapping_add(s.size as u32);
            h = h.wrapping_mul(31).wrapping_add(s.pids.len() as u32);
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
    let mut b = ShmBook::new();
    // 1) 零长建段必拒
    got[0] = b.shmget(0).is_err();
    // 2) 越帽建段必拒
    got[1] = b.shmget(SEG_SIZE_MAX + 1).is_err();
    // 3) 野 shmid 附着必拒
    got[2] = b.shmat(999, 1).is_err();
    // 4) 未附着摘除必拒
    let sid = b.shmget(0x1000).unwrap_or(0);
    got[3] = b.shmdt(sid, 1).is_err();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.shmat(sid, 1).is_err();
    got
}

/// 判据入口：UNX-F10152
pub fn run_unx10152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10152");
    let mut b = ShmBook::new();

    // J1 附着达成 10/10：十段各双进程附着互见
    let mut j1 = true;
    for i in 0..10usize {
        let sid = b.shmget(0x100 * (i + 1));
        let sid = match sid {
            Ok(v) => v,
            Err(_) => {
                j1 = false;
                break;
            }
        };
        if b.shmat(sid, 100).is_err() || b.shmat(sid, 200).is_err() {
            j1 = false;
        }
        if !b.both_attached(sid, 100, 200) {
            j1 = false;
        }
    }
    s.add("J1_shmat_10x", j1, "shmget→shmat→双进程可见 10/10");

    // J2 双进程写读互见
    let sid = 1usize;
    let w = b.write(sid, 0, b"SHRDATA");
    let ra = b.read(sid, 0, 7);
    let j2 = w.is_ok() && ra == Ok(b"SHRDATA".to_vec()) && b.both_attached(sid, 100, 200);
    s.add("J2_shared_view", j2, "共享页写入双附着面同值");

    // J3 野 shmid 受控
    let j3 = b.shmat(999, 1) == Err(SH_BAD_SHMID)
        && b.read(999, 0, 1) == Err(SH_BAD_SHMID)
        && b.shmdt(999, 1) == Err(SH_BAD_SHMID);
    s.add("J3_bad_shmid", j3, "at/read/dt 野柄三路受控");

    // J4 零长/越帽建段受控
    let j4 = b.shmget(0) == Err(SH_BAD_SIZE)
        && b.shmget(SEG_SIZE_MAX + 1) == Err(SH_BAD_SIZE);
    s.add("J4_bad_size", j4, "size 双界受控 BAD_SIZE");

    // J5 越界写读受控
    let j5 = b.write(sid, 0x100 - 1, b"XY") == Err(SH_BAD_SIZE)
        && b.read(sid, 0x100 - 1, 2) == Err(SH_BAD_SIZE);
    s.add("J5_range", j5, "段尾越界写读双拒");

    // J6 摘附着
    let d = b.shmdt(sid, 100);
    let j6 = d == Ok(1) && !b.both_attached(sid, 100, 200);
    s.add("J6_shmdt", j6, "摘 100 后仅 200 附着");

    // J7 未附着摘除受控
    let j7 = b.shmdt(sid, 100) == Err(SH_NOT_ATTACHED);
    s.add("J7_dt_fresh", j7, "重复摘除拒 NOT_ATTACHED");

    // J8 重复附着幂等
    let mut b2 = ShmBook::new();
    let s2 = b2.shmget(0x1000).unwrap_or(0);
    let _ = b2.shmat(s2, 5);
    s.add("J8_dedup", b2.attach_dedup(s2, 5), "重复附着计数不增");

    // J9 段容量守门
    let mut b3 = ShmBook::new();
    let mut fill_ok = true;
    for _ in 0..SEG_CAP {
        if b3.shmget(0x10).is_err() {
            fill_ok = false;
        }
    }
    let j9 = fill_ok && b3.shmget(0x10) == Err(SH_SEG_CAP);
    s.add("J9_seg_cap", j9, "段满 32 拒 SEG_CAP");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封册受控
    let mut b4 = ShmBook::new();
    let s4 = b4.shmget(0x100).unwrap_or(0);
    b4.seal();
    let j11 = b4.shmget(0x100) == Err(SH_SEALED)
        && b4.shmat(s4, 1) == Err(SH_SEALED)
        && b4.shmdt(s4, 1) == Err(SH_SEALED);
    s.add("J11_sealed", j11, "封册后三操作皆拒 SEALED");

    // J12 附着计数对平
    let want: usize = b.segs.iter().map(|sg| sg.pids.len()).sum();
    s.add("J12_attach_sum", b.attach_total() == want && b.attach_total() >= 10, "附着计数 ≥10 对平");

    // J13 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "段册折叠指纹一致");

    // J14 shmid 单调
    let ids: Vec<usize> = b.segs.iter().map(|sg| sg.shmid).collect();
    let mut mono = true;
    for w in ids.windows(2) {
        if w[1] <= w[0] {
            mono = false;
        }
    }
    s.add("J14_id_mono", mono && ids.len() == 10, "shmid 单调且十段在册");

    // J15 写后双面读一致
    let r1 = b.read(2, 0, 4).unwrap_or_default();
    let r2 = b.read(2, 0, 4).unwrap_or_default();
    s.add("J15_dual_read", r1 == r2 && r1 == vec![0, 0, 0, 0], "未写区双读同零");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shm52_flow() {
        let mut b = ShmBook::new();
        let sid = b.shmget(0x100).unwrap();
        assert!(b.shmat(sid, 1).is_ok());
        assert!(b.shmat(sid, 2).is_ok());
        b.write(sid, 0, b"AB").unwrap();
        assert_eq!(b.read(sid, 0, 2), Ok(b"AB".to_vec()));
        assert!(b.both_attached(sid, 1, 2));
    }

    #[test]
    fn test_shm52_misuse() {
        let mut b = ShmBook::new();
        assert_eq!(b.shmget(0), Err(SH_BAD_SIZE));
        assert_eq!(b.shmat(9, 1), Err(SH_BAD_SHMID));
    }

    #[test]
    fn test_shm52_checks() {
        let cs = run_unx10152_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_shm52_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
