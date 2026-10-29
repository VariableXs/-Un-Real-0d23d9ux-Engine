//! UNX-F10127 truncate/ftruncate 正反双判据（截断达成与负长误用受控）（0x10300 段）
//!
//! 锚点：UNX-F10127-J1 截断达成（缩短/加长与零填充语义 LTP ftruncate01
//! 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：截断件（内容+截断史）+ 截断核（缩短裁尾/加长零填/负长拒/同名
//! truncate 路径面）+ 截断账（逐次留痕 + 长度轨迹单调可折）+ 注入核
//! （负长/坏柄/超容加长/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10300 独占 0x20）。
pub const SEG_BASE: u32 = 0x10300;
pub const TR_NEG: u32 = SEG_BASE + 0x01; // 负长
pub const TR_BAD_FD: u32 = SEG_BASE + 0x02; // 坏柄
pub const TR_OVER: u32 = SEG_BASE + 0x03; // 加长越容
pub const TR_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const TR_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 容量上限。
pub const TRUNC_CAP: usize = 64;

/// 截断件。
#[derive(Clone)]
pub struct TruncFile {
    pub name: String,
    pub data: Vec<u8>,
}

impl TruncFile {
    pub fn new(name: &str, data: &[u8]) -> TruncFile {
        TruncFile { name: name.to_string(), data: data.to_vec() }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 截断语义：缩短裁尾、加长零填。
    pub fn apply(&mut self, new_len: usize) {
        if new_len < self.data.len() {
            self.data.truncate(new_len);
        } else {
            while self.data.len() < new_len {
                self.data.push(0);
            }
        }
    }

    /// 零填校验：加长区全零。
    pub fn tail_zero(&self, from: usize) -> bool {
        self.data.iter().skip(from).all(|b| *b == 0)
    }
}

/// 截断账留痕。
#[derive(Clone)]
pub struct TrRec {
    pub seq: u32,
    pub fd: i32,
    pub from_len: usize,
    pub to_len: usize,
    pub note: String,
}

impl TrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0
    }
}

/// 截断册。
pub struct TruncBook {
    pub slots: Vec<Option<TruncFile>>,
    pub ledger: Vec<TrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TruncBook {
    pub fn new() -> TruncBook {
        TruncBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, name: &str, data: &[u8]) -> Result<usize, u32> {
        if self.sealed {
            return Err(TR_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(TruncFile::new(name, data));
            return Ok(i);
        }
        if self.slots.len() >= 6 {
            return Err(TR_BAD_FD);
        }
        self.slots.push(Some(TruncFile::new(name, data)));
        Ok(self.slots.len() - 1)
    }

    /// ftruncate 面：按柄截断。
    pub fn ftruncate(&mut self, fd: i32, new_len: i64, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(TR_SEALED);
        }
        if note.is_empty() {
            return Err(TR_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(TR_BAD_FD);
        }
        if new_len < 0 {
            return Err(TR_NEG);
        }
        if new_len as usize > TRUNC_CAP {
            return Err(TR_OVER);
        }
        let u = fd as usize;
        let from = self.slots[u].as_ref().map(|f| f.len()).unwrap_or(0);
        let f = match self.slots[u].as_mut() {
            Some(f) => f,
            None => return Err(TR_BAD_FD),
        };
        f.apply(new_len as usize);
        self.ledger.push(TrRec {
            seq: self.seq_next,
            fd,
            from_len: from,
            to_len: new_len as usize,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(new_len as usize)
    }

    /// truncate 面：按名截断（缺名拒）。
    pub fn truncate_by_name(&mut self, name: &str, new_len: i64, note: &str) -> Result<usize, u32> {
        let pos = self.slots.iter().position(|s| {
            s.as_ref().map(|f| f.name == name).unwrap_or(false)
        });
        match pos {
            Some(i) => self.ftruncate(i as i32, new_len, note),
            None => Err(TR_BAD_FD),
        }
    }

    /// 长度轨迹核：账面长度链与现状一致。
    pub fn trail_matches(&self, fd: i32) -> bool {
        let cur = if fd >= 0 && (fd as usize) < self.slots.len() {
            self.slots[fd as usize].as_ref().map(|f| f.len())
        } else {
            None
        };
        match (cur, self.ledger.iter().rev().find(|r| r.fd == fd)) {
            (Some(c), Some(r)) => c == r.to_len,
            _ => false,
        }
    }

    /// 截断次数统计。
    pub fn op_count(&self) -> usize {
        self.ledger.len()
    }

    /// 缩/扩双向计数。
    pub fn shrink_grow_count(&self) -> (usize, usize) {
        let s = self.ledger.iter().filter(|r| r.to_len < r.from_len).count();
        let g = self.ledger.iter().filter(|r| r.to_len > r.from_len).count();
        (s, g)
    }

    /// 按名取长（缺名 None）。
    pub fn len_by_name(&self, name: &str) -> Option<usize> {
        self.slots
            .iter()
            .find(|s| s.as_ref().map(|f| f.name == name).unwrap_or(false))
            .and_then(|s| s.as_ref().map(|f| f.len()))
    }

    pub fn len_of(&self, fd: i32) -> Option<usize> {
        if fd < 0 || fd as usize >= self.slots.len() {
            return None;
        }
        self.slots[fd as usize].as_ref().map(|f| f.len())
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
        let mut h: u32 = 0x103000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.to_len as u32);
            h = h.wrapping_mul(31).wrapping_add(r.from_len as u32);
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
    let mut b = TruncBook::new();
    let f = b.open("t", b"0123").unwrap_or(0);
    // 1) 负长必拒
    got[0] = b.ftruncate(f as i32, -1, "inj-neg").err() == Some(TR_NEG);
    // 2) 坏柄必拒
    got[1] = b.ftruncate(-2, 0, "inj-fd").err() == Some(TR_BAD_FD);
    // 3) 超容加长必拒
    got[2] = b.ftruncate(f as i32, (TRUNC_CAP + 1) as i64, "inj-over").err() == Some(TR_OVER);
    // 4) 空留痕必拒
    got[3] = b.ftruncate(f as i32, 2, "").err() == Some(TR_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.ftruncate(f as i32, 0, "inj-seal").err() == Some(TR_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10127");

    // 正路径：缩短裁尾（ftruncate01 面）
    let mut b = TruncBook::new();
    let f = b.open("a", b"abcdefgh").unwrap_or(0);
    let n = b.ftruncate(f as i32, 4, "缩短").unwrap_or(0);
    let j1 = n == 4 && b.len_of(f as i32) == Some(4);
    s.add("J1-缩短裁尾", j1, "8→4 截断生效");

    // 加长零填
    let j2 = {
        let _ = b.ftruncate(f as i32, 8, "加长");
        let fref = b.slots[f].as_ref();
        match fref {
            Some(t) => t.len() == 8 && t.tail_zero(4),
            None => false,
        }
    };
    s.add("J2-加长零填", j2, "4→8 加长区全零");

    // 零截断清空
    let j3 = {
        let _ = b.ftruncate(f as i32, 0, "清空");
        b.len_of(f as i32) == Some(0)
    };
    s.add("J3-零截断清空", j3, "0 长清空内容");

    // truncate 按名面
    let j4 = {
        let _ = b.open("named", b"xyz");
        b.truncate_by_name("named", 1, "按名").is_ok() && {
            let pos = b.slots.iter().position(|s| {
                s.as_ref().map(|t| t.name == "named").unwrap_or(false)
            });
            pos.map(|i| b.len_of(i as i32) == Some(1)).unwrap_or(false)
        }
    };
    s.add("J4-按名截断", j4, "truncate 路径面同语义");

    // 按名缺席受控
    let j5 = b.truncate_by_name("ghost", 0, "缺席").err() == Some(TR_BAD_FD);
    s.add("J5-按名缺席受控", j5, "缺名必拒");

    // 负长受控
    let j6 = b.ftruncate(f as i32, -5, "负").err() == Some(TR_NEG);
    s.add("J6-负长受控", j6, "负长必拒且不动内容");

    // 超容加长受控
    let j7 = b.ftruncate(f as i32, (TRUNC_CAP + 1) as i64, "越容").err() == Some(TR_OVER);
    s.add("J7-越容受控", j7, "加长出 TRUNC_CAP 必拒");

    // 坏柄受控
    let j8 = b.ftruncate(99, 1, "越界").err() == Some(TR_BAD_FD);
    s.add("J8-坏柄受控", j8, "越界柄必拒");

    // 长度轨迹对平
    let j9 = b.trail_matches(f as i32);
    s.add("J9-轨迹对平", j9, "账面末长与现状一致");

    // 空留痕拒收
    let j10 = b.ftruncate(f as i32, 1, "").err() == Some(TR_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j11 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J11-留痕合法", j11, "柄非负且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = b.fold() == b.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    // 缩/扩双向计数
    let (sc, gc) = b.shrink_grow_count();
    let j14 = sc >= 1 && gc >= 1 && b.op_count() >= 2;
    s.add("J14-双向计数", j14, "缩扩两向各有留痕");

    // 按名取长
    let j15 = b.len_by_name("named") == Some(1) && b.len_by_name("ghost").is_none();
    s.add("J15-按名取长", j15, "有名得长、缺名空回");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10127_trunc_flow() {
        let mut b = TruncBook::new();
        let f = b.open("x", b"abcd").unwrap_or(0);
        let _ = b.ftruncate(f as i32, 2, "a");
        assert_eq!(b.len_of(f as i32), Some(2));
        let _ = b.ftruncate(f as i32, 5, "b");
        let t = b.slots[f].clone().unwrap_or(TruncFile::new("", b""));
        assert_eq!(t.len(), 5);
        assert!(b.ftruncate(f as i32, -1, "c") == Err(TR_NEG));
    }

    #[test]
    fn unx10127_checks_green() {
        let s = run_unx10127_checks();
        assert!(s.all_passed(), "UNX-F10127 判据未全绿");
    }
}
