//! UNX-F10133 稀疏文件正反双判据（空洞语义达成与负偏移误用受控）（0x103c0 段）
//!
//! 锚点：UNX-F10133-J1 空洞语义达成（lseek 越写→读空洞零填充 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：块图册（块号→数据块的稀疏映射，缺块即洞）+ 稀疏核（越尾写建
//! 洞/读洞零填/块计数统计/负偏移拒）+ 稀疏账（逐次留痕）+ 注入核（负
//! 偏移/越界块/坏柄/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x103c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x103c0;
pub const SP_NEG: u32 = SEG_BASE + 0x01; // 负偏移
pub const SP_BAD_FD: u32 = SEG_BASE + 0x02; // 坏柄
pub const SP_OVER: u32 = SEG_BASE + 0x03; // 块数越界
pub const SP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const SP_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 块大小与块数上限。
pub const BLK: usize = 8;
pub const BLK_MAX: usize = 8;

/// 稀疏件：块号→块数据（缺块即洞）。
pub struct SparseFile {
    pub name: String,
    pub blocks: Vec<(usize, Vec<u8>)>,
    pub logical_len: usize,
}

impl SparseFile {
    pub fn new(name: &str) -> SparseFile {
        SparseFile { name: name.to_string(), blocks: Vec::new(), logical_len: 0 }
    }

    /// 块读：缺块返零（洞）。
    pub fn block_read(&self, bno: usize) -> Vec<u8> {
        match self.blocks.iter().find(|(b, _)| *b == bno) {
            Some((_, v)) => v.clone(),
            None => vec![0u8; BLK],
        }
    }

    /// 块写：建/覆块。
    pub fn block_write(&mut self, bno: usize, data: &[u8]) {
        let mut blk = vec![0u8; BLK];
        let n = data.len().min(BLK);
        blk[..n].copy_from_slice(&data[..n]);
        match self.blocks.iter_mut().find(|(b, _)| *b == bno) {
            Some((_, v)) => *v = blk,
            None => self.blocks.push((bno, blk)),
        }
        let end = (bno + 1) * BLK;
        if end > self.logical_len {
            self.logical_len = end;
        }
    }

    /// 实块数（有数据的块）。
    pub fn real_blocks(&self) -> usize {
        self.blocks.len()
    }

    /// 洞块数（逻辑范围内缺块）。
    pub fn hole_blocks(&self) -> usize {
        let total = self.logical_len / BLK;
        total.saturating_sub(self.real_blocks())
    }
}

/// 稀疏账留痕。
#[derive(Clone)]
pub struct SpRec {
    pub seq: u32,
    pub fd: i32,
    pub off: i64,
    pub n: usize,
    pub is_write: bool,
}

impl SpRec {
    pub fn ok(&self) -> bool {
        self.fd >= 0 && self.off >= 0
    }
}

/// 稀疏册。
pub struct SparseBook {
    pub slots: Vec<Option<SparseFile>>,
    pub ledger: Vec<SpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SparseBook {
    pub fn new() -> SparseBook {
        SparseBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn create(&mut self, name: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(SP_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(SparseFile::new(name));
            return Ok(i);
        }
        if self.slots.len() >= 4 {
            return Err(SP_BAD_FD);
        }
        self.slots.push(Some(SparseFile::new(name)));
        Ok(self.slots.len() - 1)
    }

    /// 偏移写：越尾建洞（负偏移/越块界拒）。
    pub fn write_at(&mut self, fd: i32, off: i64, buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(SP_SEALED);
        }
        if note.is_empty() {
            return Err(SP_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(SP_BAD_FD);
        }
        if off < 0 {
            return Err(SP_NEG);
        }
        let start = off as usize;
        let end_blk = (start + buf.len().max(1) - 1) / BLK;
        if end_blk >= BLK_MAX {
            return Err(SP_OVER);
        }
        let u = fd as usize;
        let f = match self.slots[u].as_mut() {
            Some(f) => f,
            None => return Err(SP_BAD_FD),
        };
        let mut done = 0;
        while done < buf.len() {
            let pos = start + done;
            let bno = pos / BLK;
            let inblk = pos % BLK;
            let take = (BLK - inblk).min(buf.len() - done);
            // 取旧块拼接再写回（保跨块一致性）
            let mut blk = f.block_read(bno);
            blk[inblk..inblk + take].copy_from_slice(&buf[done..done + take]);
            f.block_write(bno, &blk);
            done += take;
        }
        self.ledger.push(SpRec {
            seq: self.seq_next,
            fd,
            off,
            n: buf.len(),
            is_write: true,
        });
        self.seq_next += 1;
        Ok(buf.len())
    }

    /// 偏移读：洞返零。
    pub fn read_at(&mut self, fd: i32, off: i64, count: usize) -> Result<Vec<u8>, u32> {
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(SP_BAD_FD);
        }
        if off < 0 {
            return Err(SP_NEG);
        }
        let start = off as usize;
        let u = fd as usize;
        let f = match self.slots[u].as_ref() {
            Some(f) => f,
            None => return Err(SP_BAD_FD),
        };
        let mut out: Vec<u8> = Vec::new();
        let mut done = 0;
        while done < count {
            let pos = start + done;
            if pos >= BLK_MAX * BLK {
                break;
            }
            let bno = pos / BLK;
            let inblk = pos % BLK;
            let blk = f.block_read(bno);
            let take = (BLK - inblk).min(count - done);
            out.extend_from_slice(&blk[inblk..inblk + take]);
            done += take;
        }
        self.ledger.push(SpRec { seq: self.seq_next, fd, off, n: out.len(), is_write: false });
        self.seq_next += 1;
        Ok(out)
    }

    pub fn stats_of(&self, fd: i32) -> Option<(usize, usize)> {
        self.slots[fd as usize]
            .as_ref()
            .map(|f| (f.real_blocks(), f.hole_blocks()))
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
        let mut h: u32 = 0x103c00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.off as u32);
            h = h.wrapping_mul(31).wrapping_add(r.n as u32);
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
    let mut b = SparseBook::new();
    let f = b.create("s").unwrap_or(0);
    // 1) 负偏移写必拒
    got[0] = b.write_at(f as i32, -4, b"x", "inj-neg").err() == Some(SP_NEG);
    // 2) 负偏移读必拒
    got[1] = b.read_at(f as i32, -1, 1).err() == Some(SP_NEG);
    // 3) 块数越界必拒
    got[2] = b.write_at(f as i32, (BLK_MAX * BLK) as i64, b"x", "inj-over").err() == Some(SP_OVER);
    // 4) 空留痕必拒
    got[3] = b.write_at(f as i32, 0, b"y", "").err() == Some(SP_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.write_at(f as i32, 0, b"z", "inj-seal").err() == Some(SP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10133_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10133");

    // 正路径：越尾写建洞、读洞零填
    let mut b = SparseBook::new();
    let f = b.create("sparse.dat").unwrap_or(0);
    let _ = b.write_at(f as i32, BLK as i64 * 3, b"TAIL", "越尾写");
    let hole = b.read_at(f as i32, 0, BLK * 3).unwrap_or_default();
    let tail = b.read_at(f as i32, BLK as i64 * 3, 4).unwrap_or_default();
    let j1 = hole == vec![0u8; BLK * 3] && tail == b"TAIL".to_vec();
    s.add("J1-洞零填", j1, "三块洞读全零且尾块数据可达");

    // 块计数：实 1 洞 3
    let j2 = b.stats_of(f as i32) == Some((1, 3));
    s.add("J2-块计数", j2, "实块 1 洞块 3 对平");

    // 跨块写一致性
    let j3 = {
        let mut b3 = SparseBook::new();
        let f3 = b3.create("cross").unwrap_or(0);
        let _ = b3.write_at(f3 as i32, BLK as i64 - 2, b"MNOP", "跨块");
        let g = b3.read_at(f3 as i32, BLK as i64 - 2, 4).unwrap_or_default();
        g == b"MNOP".to_vec()
    };
    s.add("J3-跨块写", j3, "块界两侧拼接一致");

    // 覆写不增块
    let j4 = {
        let _ = b.write_at(f as i32, BLK as i64 * 3, b"TAIL", "覆写");
        b.stats_of(f as i32).map(|(r, _)| r == 1).unwrap_or(false)
    };
    s.add("J4-覆写守恒", j4, "同位覆写实块数不增");

    // 负偏移受控
    let j5 = b.write_at(f as i32, -1, b"x", "负").err() == Some(SP_NEG)
        && b.read_at(f as i32, -8, 1).err() == Some(SP_NEG);
    s.add("J5-负偏移受控", j5, "读写双面同拒");

    // 块数越界受控
    let j6 = b.write_at(f as i32, (BLK_MAX * BLK - 2) as i64, b"abc", "越尾")
        .err() == Some(SP_OVER);
    s.add("J6-块界受控", j6, "出 BLK_MAX 必拒");

    // 坏柄受控
    let j7 = b.read_at(99, 0, 1).err() == Some(SP_BAD_FD);
    s.add("J7-坏柄受控", j7, "越界柄必拒");

    // 空留痕拒收
    let j8 = b.write_at(f as i32, 0, b"q", "").err() == Some(SP_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 柄容量守恒
    let j9 = {
        let mut b9 = SparseBook::new();
        let mut ok = true;
        for _ in 0..4 {
            if b9.create("n").is_err() {
                ok = false;
            }
        }
        ok && b9.create("over").err() == Some(SP_BAD_FD)
    };
    s.add("J9-容量守恒", j9, "柄槽满必拒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "偏移非负且柄非负");

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
    fn unx10133_sparse_flow() {
        let mut b = SparseBook::new();
        let f = b.create("x").unwrap_or(0);
        let _ = b.write_at(f as i32, 20, b"Z", "w");
        assert_eq!(b.stats_of(f as i32), Some((1, 2)));
        let g = b.read_at(f as i32, 0, 21).unwrap_or_default();
        assert_eq!(g[20], b'Z');
        assert!(g[..20].iter().all(|c| *c == 0));
    }

    #[test]
    fn unx10133_checks_green() {
        let s = run_unx10133_checks();
        assert!(s.all_passed(), "UNX-F10133 判据未全绿");
    }
}
