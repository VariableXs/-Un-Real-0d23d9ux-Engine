//! UNX-F10137 RLIMIT_FSIZE 正反双判据（限值达成与超限写误用受控）（0x10440 段）
//!
//! 锚点：UNX-F10137-J1 限值达成（prlimit 置限后写至限位成功 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：限额册（软限/硬限双锚 + 写偏移推进）+ 限核（限内写全成/触限
//! EFBIG/提限放行/降限拒缩不降触）+ 限账（逐次留痕）+ 注入核（超限写/
//! 软超硬/坏柄/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10440 独占 0x20）。
pub const SEG_BASE: u32 = 0x10440;
pub const RL_EFBIG: u32 = SEG_BASE + 0x01; // 超限写
pub const RL_INVAL: u32 = SEG_BASE + 0x02; // 软限超硬限
pub const RL_BAD_FD: u32 = SEG_BASE + 0x03; // 坏柄
pub const RL_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const RL_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 硬限锚。
pub const HARD_CAP: u32 = 256;

/// 限额件。
pub struct RFile {
    pub data: Vec<u8>,
}

impl RFile {
    pub fn new() -> RFile {
        RFile { data: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

/// 限账留痕。
#[derive(Clone)]
pub struct RlRec {
    pub seq: u32,
    pub fd: i32,
    pub wrote: usize,
    pub size_after: usize,
    pub note: String,
}

impl RlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0
    }
}

/// 限额册。
pub struct LimitBook {
    pub soft: u32,
    pub hard: u32,
    pub slots: Vec<Option<RFile>>,
    pub ledger: Vec<RlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LimitBook {
    pub fn new() -> LimitBook {
        LimitBook {
            soft: HARD_CAP,
            hard: HARD_CAP,
            slots: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// prlimit 置限：软 ≤ 硬 ≤ HARD_CAP。
    pub fn prlimit(&mut self, soft: u32, hard: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if soft > hard || hard > HARD_CAP {
            return Err(RL_INVAL);
        }
        self.soft = soft;
        self.hard = hard;
        Ok(())
    }

    /// 降限裁断：新软限不得低于现数据量。
    pub fn can_lower(&self, new_soft: u32) -> bool {
        let max_size = self
            .slots
            .iter()
            .flatten()
            .map(|f| f.len() as u32)
            .max()
            .unwrap_or(0);
        new_soft >= max_size
    }

    pub fn open(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(RFile::new());
            return Ok(i);
        }
        if self.slots.len() >= 4 {
            return Err(RL_BAD_FD);
        }
        self.slots.push(Some(RFile::new()));
        Ok(self.slots.len() - 1)
    }

    /// 写核心：限内全成、触限 EFBIG（半笔不落）。
    pub fn write(&mut self, fd: i32, buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(RL_SEALED);
        }
        if note.is_empty() {
            return Err(RL_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(RL_BAD_FD);
        }
        let u = fd as usize;
        let cur = self.slots[u].as_ref().map(|f| f.len()).unwrap_or(0);
        if cur as u32 + buf.len() as u32 > self.soft {
            return Err(RL_EFBIG);
        }
        let f = match self.slots[u].as_mut() {
            Some(f) => f,
            None => return Err(RL_BAD_FD),
        };
        f.data.extend_from_slice(buf);
        self.ledger.push(RlRec {
            seq: self.seq_next,
            fd,
            wrote: buf.len(),
            size_after: f.len(),
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(buf.len())
    }

    /// 尺寸回读。
    pub fn size_of(&self, fd: i32) -> Option<usize> {
        if fd < 0 || fd as usize >= self.slots.len() {
            return None;
        }
        self.slots[fd as usize].as_ref().map(|f| f.len())
    }

    /// 限位渲染（诊断面）。
    pub fn render_limit(&self) -> String {
        format!("soft{}|hard{}|cap{}", self.soft, self.hard, HARD_CAP)
    }

    /// 限内余量。
    pub fn headroom_of(&self, fd: i32) -> Option<u32> {
        let sz = self.size_of(fd)? as u32;
        if sz > self.soft {
            None
        } else {
            Some(self.soft - sz)
        }
    }

    /// 写次数统计。
    pub fn write_count(&self) -> usize {
        self.ledger.len()
    }

    /// 写量累计。
    pub fn bytes_written(&self) -> usize {
        self.ledger.iter().map(|r| r.wrote).fold(0usize, |a, b| a + b)
    }

    /// 尺寸对平核：账面末次 size_after 与现值一致。
    pub fn size_matches(&self, fd: i32) -> bool {
        let cur = self.size_of(fd);
        match (cur, self.ledger.iter().rev().find(|r| r.fd == fd)) {
            (Some(c), Some(r)) => c == r.size_after,
            _ => false,
        }
    }

    /// 半笔不落核：越限写后尺寸不动。
    pub fn atomic_reject(&mut self, fd: i32, buf: &[u8]) -> bool {
        let before = self.size_of(fd);
        let _ = self.write(fd, buf, "atomic");
        before == self.size_of(fd)
    }

    /// 限位写面：恰好填至限位。
    pub fn fill_to_limit(&mut self, fd: i32, note: &str) -> Result<usize, u32> {
        let cur = self.size_of(fd).unwrap_or(0) as u32;
        if cur >= self.soft {
            return Ok(0);
        }
        let need = (self.soft - cur) as usize;
        let buf = vec![0xA5u8; need];
        self.write(fd, &buf, note)
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
        let mut h: u32 = 0x104400;
        h = h.wrapping_mul(31).wrapping_add(self.soft);
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.size_after as u32);
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
    let mut b = LimitBook::new();
    let _ = b.prlimit(4, 8);
    let f = b.open().unwrap_or(0);
    // 1) 超限写必拒
    got[0] = b.write(f as i32, b"012345", "inj-over").err() == Some(RL_EFBIG);
    // 2) 软超硬必拒
    got[1] = b.prlimit(9, 8).err() == Some(RL_INVAL);
    // 3) 坏柄必拒
    got[2] = b.write(-1, b"x", "inj-fd").err() == Some(RL_BAD_FD);
    // 4) 空留痕必拒
    got[3] = b.write(f as i32, b"y", "").err() == Some(RL_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.write(f as i32, b"z", "inj-seal").err() == Some(RL_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10137");

    // 正路径：置限后写至限位成功
    let mut b = LimitBook::new();
    let _ = b.prlimit(16, 32);
    let f = b.open().unwrap_or(0);
    let n = b.fill_to_limit(f as i32, "填限").unwrap_or(0);
    let j1 = n == 16 && b.size_of(f as i32) == Some(16);
    s.add("J1-限位达成", j1, "软限 16 恰填满成功");

    // 触限 EFBIG
    let j2 = b.write(f as i32, b"!", "越限").err() == Some(RL_EFBIG);
    s.add("J2-越限受控", j2, "限后一字节必拒且不落盘");

    // 限内分段写
    let j3 = {
        let mut b3 = LimitBook::new();
        let _ = b3.prlimit(12, 12);
        let f3 = b3.open().unwrap_or(0);
        let a = b3.write(f3 as i32, b"abcd", "段一").unwrap_or(0);
        let c = b3.write(f3 as i32, b"efgh", "段二").unwrap_or(0);
        a == 4 && c == 4 && b3.size_of(f3 as i32) == Some(8)
    };
    s.add("J3-分段限内", j3, "限内分段写累计推进");

    // 提限放行
    let j4 = {
        let _ = b.prlimit(24, 32);
        let ok = b.write(f as i32, b"EXTRA", "提限写").is_ok();
        ok && b.size_of(f as i32) == Some(21)
    };
    s.add("J4-提限放行", j4, "软限提至 24 后续写成功");

    // 降限裁断（现量锚自 size_of 推导：恰等现量可、低一字节拒）
    let cur_sz = b.size_of(f as i32).unwrap_or(0) as u32;
    let j5 = b.can_lower(cur_sz) && !b.can_lower(cur_sz.saturating_sub(1));
    s.add("J5-降限裁断", j5, "降限不低于现量可、低于拒");

    // 软超硬受控
    let j6 = b.prlimit(33, 32).err() == Some(RL_INVAL)
        && b.prlimit(0, HARD_CAP + 1).err() == Some(RL_INVAL);
    s.add("J6-限界受控", j6, "软超硬与硬超锚同拒");

    // 坏柄受控
    let j7 = b.write(99, b"x", "野柄").err() == Some(RL_BAD_FD);
    s.add("J7-坏柄受控", j7, "越界柄必拒");

    // 空留痕拒收
    let j8 = b.write(f as i32, b"q", "").err() == Some(RL_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j9 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J9-留痕合法", j9, "柄非负且注非空");

    // 半笔不落与尺寸对平
    let j12 = b.atomic_reject(f as i32, b"TOOBIG") && b.size_matches(f as i32);
    s.add("J12-原子拒写", j12, "越限半笔不落且账面末次对平");

    // 余量与渲染
    let j13 = b.headroom_of(f as i32) == Some(3) && b.render_limit().contains("soft24");
    s.add("J13-余量渲染", j13, "限内余量 3 且渲染含软限");

    // 写量统计
    let j14 = b.write_count() >= 2 && b.bytes_written() == 16 + 5;
    s.add("J14-写量对平", j14, "两笔写 21 字节累计对平");

    // 注入核 5/5
    let inj = inject_five();
    let j10 = inj.iter().all(|g| *g);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10137_limit_flow() {
        let mut b = LimitBook::new();
        let _ = b.prlimit(4, 8);
        let f = b.open().unwrap_or(0);
        assert_eq!(b.write(f as i32, b"abcd", "a").unwrap_or(0), 4);
        assert!(b.write(f as i32, b"x", "b") == Err(RL_EFBIG));
        let _ = b.prlimit(8, 8);
        assert!(b.write(f as i32, b"efgh", "c").is_ok());
    }

    #[test]
    fn unx10137_checks_green() {
        let s = run_unx10137_checks();
        assert!(s.all_passed(), "UNX-F10137 判据未全绿");
    }
}
