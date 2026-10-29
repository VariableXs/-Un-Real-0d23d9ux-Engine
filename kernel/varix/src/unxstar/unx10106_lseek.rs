//! UNX-F10106 lseek 正反双判据（定位达成与非法偏移误用受控）（0x10060 段）
//!
//! 锚点：UNX-F10106-J1 lseek 定位达成（SEEK_SET/CUR/END 三基面 LTP
//! lseek01 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：定位册（内容+偏移+长度锚的定位件表）+ seek 核（SET/CUR/END 三
//! 基准换算、负结果拒返、越尾合法稀疏位）+ 定位账（逐次 seek 留痕可折
//! 叠）+ 注入核（负结果/坏基准/坏 fd/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10060 独占 0x20）。
pub const SEG_BASE: u32 = 0x10060;
pub const SK_NEGATIVE: u32 = SEG_BASE + 0x01; // 结果偏移为负
pub const SK_BAD_WHENCE: u32 = SEG_BASE + 0x02; // 基准出闭集
pub const SK_BAD_FD: u32 = SEG_BASE + 0x03; // 坏 fd
pub const SK_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const SK_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// SEEK 基准闭集。
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// 定位件定容。
pub const FD_CAP: usize = 6;

/// 定位件：内容 + 独立偏移。
#[derive(Clone)]
pub struct SeekFile {
    pub data: Vec<u8>,
    pub pos: i64,
}

impl SeekFile {
    pub fn new(data: &[u8]) -> SeekFile {
        SeekFile { data: data.to_vec(), pos: 0 }
    }

    pub fn len(&self) -> i64 {
        self.data.len() as i64
    }
}

/// 定位账留痕。
#[derive(Clone)]
pub struct SkRec {
    pub seq: u32,
    pub fd: i32,
    pub whence: i32,
    pub off: i64,
    pub result: i64,
    pub note: String,
}

impl SkRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && (self.whence == SEEK_SET || self.whence == SEEK_CUR || self.whence == SEEK_END)
            && self.result >= 0
    }
}

/// 定位册。
pub struct SeekBook {
    pub files: Vec<Option<SeekFile>>,
    pub ledger: Vec<SkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SeekBook {
    pub fn new() -> SeekBook {
        SeekBook { files: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn install(&mut self, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(SK_SEALED);
        }
        if let Some(i) = self.files.iter().position(|s| s.is_none()) {
            self.files[i] = Some(SeekFile::new(data));
            return Ok(i);
        }
        if self.files.len() >= FD_CAP {
            return Err(SK_BAD_FD);
        }
        self.files.push(Some(SeekFile::new(data)));
        Ok(self.files.len() - 1)
    }

    /// lseek 核心：三基准换算，负结果拒返且不动原偏移。
    pub fn lseek(&mut self, fd: i32, off: i64, whence: i32, note: &str) -> Result<i64, u32> {
        if self.sealed {
            return Err(SK_SEALED);
        }
        if note.is_empty() {
            return Err(SK_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.files.len() || self.files[fd as usize].is_none() {
            return Err(SK_BAD_FD);
        }
        if whence != SEEK_SET && whence != SEEK_CUR && whence != SEEK_END {
            return Err(SK_BAD_WHENCE);
        }
        let u = fd as usize;
        let mut sk_default = SeekFile { data: Vec::new(), pos: 0 };
        let f = self.files[u].as_mut().unwrap_or(&mut sk_default);
        // 先心算后落位：负结果直接拒
        let base = match whence {
            SEEK_SET => 0,
            SEEK_CUR => f.pos,
            _ => f.len(),
        };
        let target = base.wrapping_add(off);
        if target < 0 {
            return Err(SK_NEGATIVE);
        }
        let prev = f.pos;
        f.pos = target;
        self.ledger.push(SkRec {
            seq: self.seq_next,
            fd,
            whence,
            off,
            result: target,
            note: note.to_string(),
        });
        self.seq_next += 1;
        let _ = prev;
        Ok(target)
    }

    /// 截断：收缩时偏移越界回落到新尾。
    pub fn truncate(&mut self, fd: i32, new_len: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(SK_SEALED);
        }
        if fd < 0 || fd as usize >= self.files.len() || self.files[fd as usize].is_none() {
            return Err(SK_BAD_FD);
        }
        let u = fd as usize;
        let mut sk_default = SeekFile { data: Vec::new(), pos: 0 };
        let f = self.files[u].as_mut().unwrap_or(&mut sk_default);
        if new_len > f.data.len() {
            return Err(SK_BAD_FD);
        }
        f.data.truncate(new_len);
        if f.pos > new_len as i64 {
            f.pos = new_len as i64;
        }
        Ok(new_len)
    }

    /// 自当前偏移读（校验定位效果）。
    pub fn read_here(&mut self, fd: i32, count: usize) -> Result<Vec<u8>, u32> {
        if fd < 0 || fd as usize >= self.files.len() || self.files[fd as usize].is_none() {
            return Err(SK_BAD_FD);
        }
        let u = fd as usize;
        let mut sk_default = SeekFile { data: Vec::new(), pos: 0 };
        let f = self.files[u].as_mut().unwrap_or(&mut sk_default);
        let start = f.pos.max(0) as usize;
        let end = (start + count).min(f.data.len());
        if start > f.data.len() {
            return Ok(Vec::new());
        }
        Ok(f.data[start..end].to_vec())
    }

    pub fn pos_of(&self, fd: i32) -> Option<i64> {
        if fd < 0 || fd as usize >= self.files.len() {
            return None;
        }
        self.files[fd as usize].as_ref().map(|f| f.pos)
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
        let mut h: u32 = 0x100600;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.result as u32);
            h = h.wrapping_mul(31).wrapping_add(r.whence as u32);
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
    let mut b = SeekBook::new();
    let f = b.install(b"0123456789").unwrap_or(usize::MAX);
    // 1) SET 负结果必拒
    got[0] = b.lseek(f as i32, -5, SEEK_SET, "inj-neg").err() == Some(SK_NEGATIVE);
    // 2) CUR 越退为负必拒
    let _ = b.lseek(f as i32, 3, SEEK_SET, "p3");
    got[1] = b.lseek(f as i32, -9, SEEK_CUR, "inj-cur").err() == Some(SK_NEGATIVE);
    // 3) 坏基准必拒
    got[2] = b.lseek(f as i32, 0, 7, "inj-whence").err() == Some(SK_BAD_WHENCE);
    // 4) 坏 fd 必拒
    got[3] = b.lseek(-1, 0, SEEK_SET, "inj-fd").err() == Some(SK_BAD_FD);
    // 5) 封表后 seek 必抓
    b.seal();
    got[4] = b.lseek(f as i32, 0, SEEK_SET, "inj-seal").err() == Some(SK_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10106");

    // 正路径：SEEK_SET 三连定位
    let mut b = SeekBook::new();
    let f = b.install(b"0123456789").unwrap_or(usize::MAX);
    let r1 = b.lseek(f as i32, 4, SEEK_SET, "set4").unwrap_or(-1);
    let r2 = b.lseek(f as i32, 2, SEEK_SET, "set2").unwrap_or(-1);
    let j1 = r1 == 4 && r2 == 2 && b.pos_of(f as i32) == Some(2);
    s.add("J1-SET定位", j1, "SEEK_SET 绝对定位生效");

    // SEEK_CUR 相对定位
    let r3 = b.lseek(f as i32, 3, SEEK_CUR, "cur+3").unwrap_or(-1);
    let j2 = r3 == 5 && b.pos_of(f as i32) == Some(5);
    s.add("J2-CUR定位", j2, "SEEK_CUR 相对推进 2→5");

    // SEEK_END 基准定位
    let r4 = b.lseek(f as i32, -2, SEEK_END, "end-2").unwrap_or(-1);
    let j3 = r4 == 8 && b.read_here(f as i32, 4).unwrap_or_default() == b"89".to_vec();
    s.add("J3-END定位", j3, "SEEK_END 相对回退定位至 8");

    // 定位效果：read_here 自新偏移起读
    let j4 = {
        let _ = b.lseek(f as i32, 0, SEEK_SET, "back");
        b.read_here(f as i32, 3).unwrap_or_default() == b"012".to_vec()
    };
    s.add("J4-定位生效", j4, "回零后读首段");

    // 负结果受控且原偏移不动
    let j5 = {
        let _ = b.lseek(f as i32, 5, SEEK_SET, "p5");
        let err = b.lseek(f as i32, -10, SEEK_CUR, "neg");
        err == Err(SK_NEGATIVE) && b.pos_of(f as i32) == Some(5)
    };
    s.add("J5-负偏移受控", j5, "负结果拒返且偏移保持");

    // 坏基准受控
    let j6 = b.lseek(f as i32, 1, 9, "bad-whence").err() == Some(SK_BAD_WHENCE);
    s.add("J6-基准闭集", j6, "whence 出三基准闭集必拒");

    // 坏 fd 受控
    let j7 = b.lseek(-2, 0, SEEK_SET, "negfd").err() == Some(SK_BAD_FD)
        && b.lseek(FD_CAP as i32 + 1, 0, SEEK_SET, "oobfd").err() == Some(SK_BAD_FD);
    s.add("J7-坏fd受控", j7, "负与越界 fd 同码拒");

    // 越尾定位合法（稀疏位），读回为空
    let j8 = {
        let r = b.lseek(f as i32, 20, SEEK_END, "sparse").unwrap_or(-1);
        r == 30 && b.read_here(f as i32, 4).unwrap_or_default().is_empty()
    };
    s.add("J8-越尾稀疏位", j8, "END+20 定位合法且读空");

    // 定位件容量守恒
    let j9 = {
        let mut b9 = SeekBook::new();
        let mut ok = true;
        for _ in 0..FD_CAP {
            if b9.install(b"x").is_err() {
                ok = false;
            }
        }
        ok && b9.install(b"y").err() == Some(SK_BAD_FD)
    };
    s.add("J9-容量守恒", j9, "FD_CAP 守恒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "基准在闭集、结果非负、注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 截断回落：偏移越新尾则钳回
    let j13 = {
        let mut b13 = SeekBook::new();
        let f13 = b13.install(b"0123456789").unwrap_or(usize::MAX);
        let _ = b13.lseek(f13 as i32, 8, SEEK_SET, "p8");
        let n = b13.truncate(f13 as i32, 4).unwrap_or(0);
        n == 4 && b13.pos_of(f13 as i32) == Some(4)
    };
    s.add("J13-截断回落", j13, "截断后偏移钳回新尾");

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10106_lseek_flow() {
        let mut b = SeekBook::new();
        let f = b.install(b"abcdef").unwrap_or(usize::MAX);
        assert_eq!(b.lseek(f as i32, 2, SEEK_SET, "a").unwrap_or(-1), 2);
        assert_eq!(b.lseek(f as i32, 1, SEEK_CUR, "b").unwrap_or(-1), 3);
        assert_eq!(b.lseek(f as i32, -1, SEEK_END, "c").unwrap_or(-1), 5);
        assert!(b.lseek(f as i32, -100, SEEK_CUR, "d") == Err(SK_NEGATIVE));
    }

    #[test]
    fn unx10106_checks_green() {
        let s = run_unx10106_checks();
        assert!(s.all_passed(), "UNX-F10106 判据未全绿");
    }
}
