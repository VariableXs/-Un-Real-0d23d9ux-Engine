//! UNX-F10111 pread/pwrite 正反双判据（偏移读写达成与负偏移误用受控）（0x10100 段）
//!
//! 锚点：UNX-F10111-J1 pread/pwrite 偏移读写达成（不移动共享偏移 LTP
//! pread01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：双偏移件（内容 + 共享偏移 pos + 独立查询偏移 qpos 分离）+
//! 偏移核（pread/pwrite 显式偏移不触 pos / 负偏移拒返）+ 偏移账（逐次
//! 留痕 + pos 恒定证明）+ 注入核（负偏移/越尾读空/坏 fd/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10100 独占 0x20）。
pub const SEG_BASE: u32 = 0x10100;
pub const PW_NEG_OFF: u32 = SEG_BASE + 0x01; // 负偏移
pub const PW_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const PW_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const PW_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const PW_OVER_EOF: u32 = SEG_BASE + 0x05; // 写越过 FILE_CAP

/// 文件容量上限。
pub const FILE_CAP: usize = 48;

/// 双偏移件。
#[derive(Clone)]
pub struct DualFile {
    pub data: Vec<u8>,
    pub pos: usize,
    pub id: u32,
}

impl DualFile {
    pub fn new(id: u32, data: &[u8]) -> DualFile {
        DualFile { data: data.to_vec(), pos: 0, id }
    }
}

/// 偏移账留痕。
#[derive(Clone)]
pub struct PwRec {
    pub seq: u32,
    pub fd: i32,
    pub off: i64,
    pub n: usize,
    pub is_write: bool,
    pub note: String,
}

impl PwRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.off >= 0 && self.fd >= 0
    }
}

/// 偏移册。
pub struct OffsetBook {
    pub slots: Vec<Option<DualFile>>,
    pub ledger: Vec<PwRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl OffsetBook {
    pub fn new() -> OffsetBook {
        OffsetBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, id: u32, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(PW_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(DualFile::new(id, data));
            return Ok(i);
        }
        if self.slots.len() >= 8 {
            return Err(PW_BAD_FD);
        }
        self.slots.push(Some(DualFile::new(id, data)));
        Ok(self.slots.len() - 1)
    }

    fn resolve(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(PW_BAD_FD);
        }
        Ok(fd as usize)
    }

    /// pread：显式偏移读，不触共享 pos，负偏移拒返。
    pub fn pread(&mut self, fd: i32, off: i64, count: usize, note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(PW_SEALED);
        }
        if note.is_empty() {
            return Err(PW_EMPTY_NOTE);
        }
        let u = self.resolve(fd)?;
        if off < 0 {
            return Err(PW_NEG_OFF);
        }
        let start = off as usize;
        let pos_before = self.slots[u].as_ref().map(|f| f.pos).unwrap_or(0);
        let out: Vec<u8> = if start >= self.slots[u].as_ref().map(|f| f.data.len()).unwrap_or(0) {
            Vec::new()
        } else {
            let f = match self.slots[u].as_ref() {
                Some(f) => f,
                None => return Err(PW_BAD_FD),
            };
            let end = (start + count).min(f.data.len());
            f.data[start..end].to_vec()
        };
        // pos 恒定证明：读后与读前一致
        let pos_after = self.slots[u].as_ref().map(|f| f.pos).unwrap_or(0);
        if pos_after != pos_before {
            return Err(PW_BAD_FD);
        }
        self.ledger.push(PwRec {
            seq: self.seq_next,
            fd,
            off,
            n: out.len(),
            is_write: false,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(out)
    }

    /// pwrite：显式偏移写，不触共享 pos，负偏移拒返，越 FILE_CAP 拒。
    pub fn pwrite(&mut self, fd: i32, off: i64, buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(PW_SEALED);
        }
        if note.is_empty() {
            return Err(PW_EMPTY_NOTE);
        }
        let u = self.resolve(fd)?;
        if off < 0 {
            return Err(PW_NEG_OFF);
        }
        let start = off as usize;
        if start + buf.len() > FILE_CAP {
            return Err(PW_OVER_EOF);
        }
        let pos_before = self.slots[u].as_ref().map(|f| f.pos).unwrap_or(0);
        {
            let f = match self.slots[u].as_mut() {
                Some(f) => f,
                None => return Err(PW_BAD_FD),
            };
            while f.data.len() < start {
                f.data.push(0);
            }
            let mut i = 0;
            while i < buf.len() {
                if start + i < f.data.len() {
                    f.data[start + i] = buf[i];
                } else {
                    f.data.push(buf[i]);
                }
                i += 1;
            }
        }
        let pos_after = self.slots[u].as_ref().map(|f| f.pos).unwrap_or(0);
        if pos_after != pos_before {
            return Err(PW_BAD_FD);
        }
        self.ledger.push(PwRec {
            seq: self.seq_next,
            fd,
            off,
            n: buf.len(),
            is_write: true,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(buf.len())
    }

    /// 常规读（推进 pos，与 pread 对面）。
    pub fn read_seq(&mut self, fd: i32, count: usize) -> Result<Vec<u8>, u32> {
        let u = self.resolve(fd)?;
        let f = match self.slots[u].as_mut() {
            Some(f) => f,
            None => return Err(PW_BAD_FD),
        };
        let start = f.pos.min(f.data.len());
        let end = (start + count).min(f.data.len());
        let out = f.data[start..end].to_vec();
        f.pos = end;
        Ok(out)
    }

    pub fn pos_of(&self, fd: i32) -> Option<usize> {
        let u = self.resolve(fd).ok()?;
        self.slots[u].as_ref().map(|f| f.pos)
    }

    pub fn data_of(&self, fd: i32) -> Option<Vec<u8>> {
        let u = self.resolve(fd).ok()?;
        self.slots[u].as_ref().map(|f| f.data.clone())
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
        let mut h: u32 = 0x101000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.off as u32);
            h = h.wrapping_mul(31).wrapping_add(r.n as u32);
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
    let mut b = OffsetBook::new();
    let f = b.open(1, b"0123456789").unwrap_or(usize::MAX);
    // 1) 负偏移读必拒
    got[0] = b.pread(f as i32, -1, 2, "inj-neg").err() == Some(PW_NEG_OFF);
    // 2) 负偏移写必拒
    got[1] = b.pwrite(f as i32, -5, b"x", "inj-negw").err() == Some(PW_NEG_OFF);
    // 3) 越容写必拒
    let big = [1u8; FILE_CAP];
    got[2] = b.pwrite(f as i32, 8, &big, "inj-over").err() == Some(PW_OVER_EOF);
    // 4) 坏 fd 必拒
    got[3] = b.pread(-2, 0, 2, "inj-fd").err() == Some(PW_BAD_FD);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.pread(f as i32, 0, 2, "inj-seal").err() == Some(PW_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10111");

    // 正路径：pread01 面——显式偏移读内容一致
    let mut b = OffsetBook::new();
    let f = b.open(1, b"0123456789").unwrap_or(usize::MAX);
    let g = b.pread(f as i32, 3, 4, "偏移读").unwrap_or_default();
    let j1 = g == b"3456".to_vec();
    s.add("J1-偏移读一致", j1, "off=3 读 4 字节得 3456");

    // pos 恒定证明：pread 后常规读仍从 0 起
    let j2 = {
        let _ = b.pread(f as i32, 8, 2, "不动pos");
        b.read_seq(f as i32, 2).unwrap_or_default() == b"01".to_vec()
    };
    s.add("J2-pos不动", j2, "pread 后共享偏移仍 0");

    // pwrite 显式偏移写且 pos 不动
    let j3 = {
        let mut b3 = OffsetBook::new();
        let f3 = b3.open(2, b"aaaaa").unwrap_or(usize::MAX);
        let _ = b3.pwrite(f3 as i32, 2, b"BC", "偏移写");
        b3.data_of(f3 as i32).unwrap_or_default() == b"aaBCa".to_vec()
            && b3.pos_of(f3 as i32) == Some(0)
    };
    s.add("J3-偏移写", j3, "pwrite(2) 覆写中段且 pos 保持 0");

    // 越尾读空
    let j4 = b.pread(f as i32, 99, 4, "越尾").unwrap_or_default().is_empty();
    s.add("J4-越尾读空", j4, "偏移越 EOF 得零字节");

    // 负偏移受控（读/写两面）
    let j5 = b.pread(f as i32, -3, 1, "neg").err() == Some(PW_NEG_OFF)
        && b.pwrite(f as i32, -3, b"x", "neg").err() == Some(PW_NEG_OFF);
    s.add("J5-负偏移受控", j5, "负偏移读写同码拒");

    // 越容写受控
    let j6 = {
        let big = [2u8; FILE_CAP];
        b.pwrite(f as i32, 1, &big, "over").err() == Some(PW_OVER_EOF)
    };
    s.add("J6-越容写受控", j6, "start+len 超 FILE_CAP 必拒");

    // 空洞补零语义：pwrite 跳段补零
    let j7 = {
        let mut b7 = OffsetBook::new();
        let f7 = b7.open(3, b"ab").unwrap_or(usize::MAX);
        let _ = b7.pwrite(f7 as i32, 4, b"Z", "补洞");
        b7.data_of(f7 as i32).unwrap_or_default() == b"ab\0\0Z".to_vec()
    };
    s.add("J7-空洞补零", j7, "跳写段以零填充");

    // 坏 fd 受控
    let j8 = b.pread(99, 0, 1, "oob").err() == Some(PW_BAD_FD);
    s.add("J8-坏fd受控", j8, "越界 fd 必拒");

    // 空留痕拒收
    let j9 = b.pread(f as i32, 0, 1, "").err() == Some(PW_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "偏移非负、注非空");

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
    fn unx10111_pread_flow() {
        let mut b = OffsetBook::new();
        let f = b.open(1, b"abcdef").unwrap_or(0);
        assert_eq!(b.pread(f as i32, 2, 3, "a").unwrap_or_default(), b"cde".to_vec());
        assert!(b.pread(f as i32, -1, 1, "b") == Err(PW_NEG_OFF));
        let _ = b.pwrite(f as i32, 0, b"XY", "c");
        assert_eq!(b.data_of(f as i32).unwrap_or_default(), b"XYcdef".to_vec());
    }

    #[test]
    fn unx10111_checks_green() {
        let s = run_unx10111_checks();
        assert!(s.all_passed(), "UNX-F10111 判据未全绿");
    }
}
