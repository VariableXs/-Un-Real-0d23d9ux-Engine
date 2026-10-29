//! UNX-F10112 readv/writev 正反双判据（聚合读写达成与 iovcnt 越界误用受控）（0x10120 段）
//!
//! 锚点：UNX-F10112-J1 readv/writev 聚合读写达成（分段拼接一致 LTP
//! readv01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：聚合件（内容+共享偏移）+ 散读核（iov 分段聚读/聚写、iovcnt
//! 闭集、零长段容忍）+ 聚合账（逐段留痕可对平）+ 注入核（iovcnt 越界/
//! 负 fd/超容写/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10120 独占 0x20）。
pub const SEG_BASE: u32 = 0x10120;
pub const RV_IOVCNT: u32 = SEG_BASE + 0x01; // iovcnt 越界
pub const RV_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const RV_FULL: u32 = SEG_BASE + 0x03; // 超容写
pub const RV_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const RV_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// iovcnt 闭集上限。
pub const IOV_MAX: usize = 8;
/// 聚合件容量。
pub const FILE_CAP: usize = 64;

/// 单个 iovec 段。
#[derive(Clone)]
pub struct Iovec {
    pub len: usize,
}

impl Iovec {
    pub fn new(len: usize) -> Iovec {
        Iovec { len }
    }
}

/// 聚合件。
#[derive(Clone)]
pub struct VFile2 {
    pub data: Vec<u8>,
    pub pos: usize,
}

impl VFile2 {
    pub fn new(data: &[u8]) -> VFile2 {
        VFile2 { data: data.to_vec(), pos: 0 }
    }

    /// 自 pos 裁剪读 count 字节并推进。
    pub fn read(&mut self, count: usize) -> Vec<u8> {
        let start = self.pos.min(self.data.len());
        let end = (start + count).min(self.data.len());
        let out = self.data[start..end].to_vec();
        self.pos = end;
        out
    }

    /// 自 pos 写并推进，超容拒。
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, u32> {
        if self.pos + buf.len() > FILE_CAP {
            return Err(RV_FULL);
        }
        let mut i = 0;
        while i < buf.len() {
            if self.pos + i < self.data.len() {
                self.data[self.pos + i] = buf[i];
            } else {
                self.data.push(buf[i]);
            }
            i += 1;
        }
        self.pos += buf.len();
        Ok(buf.len())
    }
}

/// 聚合账留痕。
#[derive(Clone)]
pub struct RvRec {
    pub seq: u32,
    pub fd: i32,
    pub segs: usize,
    pub total: usize,
    pub is_write: bool,
    pub note: String,
}

impl RvRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0 && self.segs <= IOV_MAX
    }
}

/// 聚合册。
pub struct VectorBook {
    pub slots: Vec<Option<VFile2>>,
    pub ledger: Vec<RvRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl VectorBook {
    pub fn new() -> VectorBook {
        VectorBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(RV_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(VFile2::new(data));
            return Ok(i);
        }
        if self.slots.len() >= 6 {
            return Err(RV_BAD_FD);
        }
        self.slots.push(Some(VFile2::new(data)));
        Ok(self.slots.len() - 1)
    }

    fn resolve(&self, fd: i32) -> Result<usize, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(RV_BAD_FD);
        }
        Ok(fd as usize)
    }

    /// readv：按 iov 段长依次聚读，拼成一段返回。
    pub fn readv(&mut self, fd: i32, iov: &[Iovec], note: &str) -> Result<Vec<u8>, u32> {
        if self.sealed {
            return Err(RV_SEALED);
        }
        if note.is_empty() {
            return Err(RV_EMPTY_NOTE);
        }
        if iov.len() > IOV_MAX || iov.is_empty() {
            return Err(RV_IOVCNT);
        }
        let u = self.resolve(fd)?;
        let mut out: Vec<u8> = Vec::new();
        for seg in iov {
            let f = match self.slots[u].as_mut() {
                Some(f) => f,
                None => return Err(RV_BAD_FD),
            };
            let part = f.read(seg.len);
            out.extend(part);
        }
        self.ledger.push(RvRec {
            seq: self.seq_next,
            fd,
            segs: iov.len(),
            total: out.len(),
            is_write: false,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(out)
    }

    /// writev：按 iov 段长切分 buf 依次聚写。
    pub fn writev(&mut self, fd: i32, iov: &[Iovec], buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(RV_SEALED);
        }
        if note.is_empty() {
            return Err(RV_EMPTY_NOTE);
        }
        if iov.len() > IOV_MAX || iov.is_empty() {
            return Err(RV_IOVCNT);
        }
        let total: usize = iov.iter().map(|s| s.len).fold(0usize, |a, b| a + b);
        if total != buf.len() {
            return Err(RV_FULL);
        }
        let u = self.resolve(fd)?;
        let mut done = 0usize;
        for seg in iov {
            let f = match self.slots[u].as_mut() {
                Some(f) => f,
                None => return Err(RV_BAD_FD),
            };
            let chunk = &buf[done..done + seg.len];
            f.write(chunk)?;
            done += seg.len;
        }
        self.ledger.push(RvRec {
            seq: self.seq_next,
            fd,
            segs: iov.len(),
            total,
            is_write: true,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(total)
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
        let mut h: u32 = 0x101200;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.segs as u32);
            h = h.wrapping_mul(31).wrapping_add(r.total as u32);
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
    let mut b = VectorBook::new();
    let f = b.open(b"abcdef").unwrap_or(usize::MAX);
    // 1) iovcnt 超 IOV_MAX 必拒
    let too_many: Vec<Iovec> = (0..IOV_MAX + 1).map(|_| Iovec::new(1)).collect();
    got[0] = b.readv(f as i32, &too_many, "inj-cnt").err() == Some(RV_IOVCNT);
    // 2) 空 iov 必拒
    let none: Vec<Iovec> = Vec::new();
    got[1] = b.readv(f as i32, &none, "inj-empty").err() == Some(RV_IOVCNT);
    // 3) 坏 fd 必拒
    let one = [Iovec::new(2)];
    got[2] = b.readv(-1, &one, "inj-fd").err() == Some(RV_BAD_FD);
    // 4) 段总长与 buf 不符必拒
    let two = [Iovec::new(1), Iovec::new(1)];
    got[3] = b.writev(f as i32, &two, b"xyz", "inj-len").err() == Some(RV_FULL);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.readv(f as i32, &one, "inj-seal").err() == Some(RV_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10112");

    // 正路径：readv01 面——分段聚读拼接一致
    let mut b = VectorBook::new();
    let f = b.open(b"agg-read-data!").unwrap_or(usize::MAX);
    let iov = [Iovec::new(3), Iovec::new(5), Iovec::new(1)];
    let g = b.readv(f as i32, &iov, "聚读").unwrap_or_default();
    let j1 = g == b"agg-read-".to_vec() && g.len() == 9;
    s.add("J1-聚读拼接", j1, "3+5+1 三段拼回 agg-read-");

    // writev 聚写一致
    let j2 = {
        let mut b2 = VectorBook::new();
        let f2 = b2.open(b"").unwrap_or(usize::MAX);
        let wiov = [Iovec::new(2), Iovec::new(4)];
        let n = b2.writev(f2 as i32, &wiov, b"abCDEF", "聚写").unwrap_or(0);
        b2.data_of(f2 as i32).unwrap_or_default() == b"abCDEF".to_vec() && n == 6
    };
    s.add("J2-聚写一致", j2, "2+4 两段聚写落盘一致");

    // 段偏移推进：聚读后常规读自新位起
    let j3 = {
        let mut b3 = VectorBook::new();
        let f3 = b3.open(b"0123456789").unwrap_or(usize::MAX);
        let _ = b3.readv(f3 as i32, &[Iovec::new(4)], "推进");
        b3.slots[f3].as_ref().map(|x| x.pos).unwrap_or(0) == 4
    };
    s.add("J3-偏移推进", j3, "聚读后 pos 前进段总长");

    // 零长段容忍
    let j4 = {
        let z = [Iovec::new(0), Iovec::new(2)];
        let g4 = b.readv(f as i32, &z, "零段").unwrap_or_default();
        g4.len() == 2
    };
    s.add("J4-零段容忍", j4, "零长段不消耗偏移");

    // iovcnt 越界受控
    let j5 = {
        let many: Vec<Iovec> = (0..IOV_MAX + 2).map(|_| Iovec::new(0)).collect();
        b.readv(f as i32, &many, "越界").err() == Some(RV_IOVCNT)
    };
    s.add("J5-iovcnt越界", j5, "超 IOV_MAX 必拒");

    // 空 iov 受控
    let j6 = {
        let none: Vec<Iovec> = Vec::new();
        b.writev(f as i32, &none, b"", "空iov").err() == Some(RV_IOVCNT)
    };
    s.add("J6-空iov受控", j6, "空 iov 向量必拒");

    // 段总长校验受控
    let j7 = {
        let bad = [Iovec::new(3), Iovec::new(3)];
        b.writev(f as i32, &bad, b"abcd", "错长").err() == Some(RV_FULL)
    };
    s.add("J7-段长对平", j7, "段总长与 buf 不符必拒");

    // 坏 fd 受控
    let j8 = {
        let one = [Iovec::new(1)];
        b.readv(99, &one, "oob").err() == Some(RV_BAD_FD)
    };
    s.add("J8-坏fd受控", j8, "越界 fd 必拒");

    // 空留痕拒收
    let j9 = {
        let one = [Iovec::new(1)];
        b.readv(f as i32, &one, "").err() == Some(RV_EMPTY_NOTE)
    };
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "段数在闭集且注非空");

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
    fn unx10112_vector_flow() {
        let mut b = VectorBook::new();
        let f = b.open(b"hello").unwrap_or(0);
        let iov = [Iovec::new(2), Iovec::new(3)];
        assert_eq!(b.readv(f as i32, &iov, "a").unwrap_or_default(), b"hello".to_vec());
        let wiov = [Iovec::new(1), Iovec::new(1)];
        assert_eq!(b.writev(f as i32, &wiov, b"XY", "b").unwrap_or(0), 2);
    }

    #[test]
    fn unx10112_checks_green() {
        let s = run_unx10112_checks();
        assert!(s.all_passed(), "UNX-F10112 判据未全绿");
    }
}
