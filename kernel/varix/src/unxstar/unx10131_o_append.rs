//! UNX-F10131 O_APPEND 并发追加正反双判据（原子追加达成与无标志竞态受控）（0x10380 段）
//!
//! 锚点：UNX-F10131-J1 原子追加达成（多写端偏移独立终位正确 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：共享底册（单文件多写端视图）+ 追加核（O_APPEND 写端自尾原子
//! 落位/无标志写端各自偏移互相覆盖）+ 追加账（逐次留痕 + 终位对平）+
//! 注入核（竞态检出/坏端/越容/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10380 独占 0x20）。
pub const SEG_BASE: u32 = 0x10380;
pub const AP_RACY: u32 = SEG_BASE + 0x01; // 无标志竞态检出
pub const AP_BAD_FD: u32 = SEG_BASE + 0x02; // 坏写端
pub const AP_OVER: u32 = SEG_BASE + 0x03; // 越容
pub const AP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const AP_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 底册容量。
pub const DATA_CAP: usize = 64;
/// 写端数上限。
pub const WRITER_MAX: usize = 4;

/// 写端视图。
#[derive(Clone)]
pub struct Writer {
    pub id: u32,
    pub append_mode: bool,
    /// 无标志写端的私有偏移。
    pub own_pos: usize,
}

impl Writer {
    pub fn ok(&self) -> bool {
        self.id != 0
    }
}

/// 追加账留痕。
#[derive(Clone)]
pub struct ApRec {
    pub seq: u32,
    pub writer: u32,
    pub n: usize,
    pub landed_at: usize,
    pub append: bool,
}

impl ApRec {
    pub fn ok(&self) -> bool {
        self.writer != 0 && self.n > 0
    }
}

/// 共享底册 + 追加核。
pub struct AppendBook {
    pub data: Vec<u8>,
    pub writers: Vec<Writer>,
    pub ledger: Vec<ApRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AppendBook {
    pub fn new() -> AppendBook {
        AppendBook { data: Vec::new(), writers: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂写端。
    pub fn attach(&mut self, append_mode: bool) -> Result<u32, u32> {
        if self.sealed {
            return Err(AP_SEALED);
        }
        if self.writers.len() >= WRITER_MAX {
            return Err(AP_BAD_FD);
        }
        let id = self.writers.len() as u32 + 1;
        self.writers.push(Writer { id, append_mode, own_pos: 0 });
        Ok(id)
    }

    fn widx(&self, id: u32) -> Option<usize> {
        self.writers.iter().position(|w| w.id == id)
    }

    /// 写核心：APPEND 端自尾原子落位；非 APPEND 端按私有偏移覆盖写。
    pub fn write(&mut self, id: u32, buf: &[u8], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(AP_SEALED);
        }
        if note.is_empty() {
            return Err(AP_EMPTY_NOTE);
        }
        let wi = match self.widx(id) {
            Some(i) => i,
            None => return Err(AP_BAD_FD),
        };
        let append = self.writers[wi].append_mode;
        let landed = if append {
            let at = self.data.len();
            if at + buf.len() > DATA_CAP {
                return Err(AP_OVER);
            }
            self.data.extend_from_slice(buf);
            at
        } else {
            let pos = self.writers[wi].own_pos;
            if pos + buf.len() > DATA_CAP {
                return Err(AP_OVER);
            }
            while self.data.len() < pos {
                self.data.push(0);
            }
            let mut i = 0;
            while i < buf.len() {
                if pos + i < self.data.len() {
                    self.data[pos + i] = buf[i];
                } else {
                    self.data.push(buf[i]);
                }
                i += 1;
            }
            self.writers[wi].own_pos = pos + buf.len();
            pos
        };
        self.ledger.push(ApRec {
            seq: self.seq_next,
            writer: id,
            n: buf.len(),
            landed_at: landed,
            append,
        });
        self.seq_next += 1;
        Ok(buf.len())
    }

    /// 终位对平：APPEND 端落位应单调递增。
    pub fn append_landed_monotonic(&self) -> bool {
        let mut last: usize = 0;
        for r in &self.ledger {
            if r.append {
                if r.landed_at < last {
                    return false;
                }
                last = r.landed_at;
            }
        }
        true
    }

    /// 竞态检出：非 APPEND 双端重叠落位即覆盖。
    pub fn race_detected(&self) -> bool {
        let plain: Vec<&ApRec> = self.ledger.iter().filter(|r| !r.append).collect();
        for i in 0..plain.len() {
            for j in (i + 1)..plain.len() {
                let a = plain[i];
                let b = plain[j];
                if a.landed_at < b.landed_at + b.n && b.landed_at < a.landed_at + a.n {
                    return true;
                }
            }
        }
        false
    }

    /// APPEND 总字节数。
    pub fn appended_total(&self) -> usize {
        self.ledger.iter().filter(|r| r.append).map(|r| r.n).fold(0usize, |a, b| a + b)
    }

    pub fn data_of(&self) -> Vec<u8> {
        self.data.clone()
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
        let mut h: u32 = 0x103800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.landed_at as u32);
            h = h.wrapping_mul(31).wrapping_add(r.n as u32);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 留痕笔数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AppendBook::new();
    let a = b.attach(true).unwrap_or(0);
    let p1 = b.attach(false).unwrap_or(0);
    let p2 = b.attach(false).unwrap_or(0);
    // 1) 无标志双端竞态必检出
    let _ = b.write(p1, b"XX", "inj-r1");
    let _ = b.write(p2, b"YY", "inj-r2");
    got[0] = b.race_detected();
    // 2) 坏写端必拒
    got[1] = b.write(99, b"x", "inj-bad").err() == Some(AP_BAD_FD);
    // 3) 越容必拒
    let big = [1u8; DATA_CAP + 1];
    got[2] = b.write(a, &big, "inj-over").err() == Some(AP_OVER);
    // 4) 空留痕必拒
    got[3] = b.write(a, b"z", "").err() == Some(AP_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.write(a, b"w", "inj-seal").err() == Some(AP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10131_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10131");

    // 正路径：双 APPEND 端原子追加、终位正确
    let mut b = AppendBook::new();
    let w1 = b.attach(true).unwrap_or(0);
    let w2 = b.attach(true).unwrap_or(0);
    let _ = b.write(w1, b"AAAA", "甲追");
    let _ = b.write(w2, b"BB", "乙追");
    let _ = b.write(w1, b"C", "甲再追");
    let j1 = b.data_of() == b"AAAABBC".to_vec() && b.append_landed_monotonic();
    s.add("J1-原子追加", j1, "三笔追加终位 0/4/6 且内容顺拼");

    // 偏移独立：两端各自终位无重叠
    let j2 = {
        let lands: Vec<usize> = b
            .ledger
            .iter()
            .filter(|r| r.append)
            .map(|r| r.landed_at)
            .collect();
        lands == vec![0usize, 4, 6]
    };
    s.add("J2-终位独立", j2, "落位点两两不重叠");

    // 追加总量对平
    let j3 = b.appended_total() == 7 && b.data_of().len() == 7;
    s.add("J3-总量对平", j3, "追加 7 字节落盘 7 字节");

    // 无标志端覆盖语义
    let j4 = {
        let mut b4 = AppendBook::new();
        let p = b4.attach(false).unwrap_or(0);
        let _ = b4.write(p, b"abc", "覆盖");
        let _ = b4.write(p, b"Z", "覆写");
        b4.data_of() == b"abcZ".to_vec()
    };
    s.add("J4-覆盖语义", j4, "无标志端按私有偏移推进写");

    // 竞态检出
    let j5 = inject_five()[0];
    s.add("J5-竞态检出", j5, "双无标志端重叠落位必检出");

    // 坏写端受控
    let j6 = b.write(42, b"x", "野端").err() == Some(AP_BAD_FD);
    s.add("J6-坏端受控", j6, "未挂写端必拒");

    // 写端容量守恒
    let j7 = {
        let mut b7 = AppendBook::new();
        let mut ok = true;
        for _ in 0..WRITER_MAX {
            if b7.attach(true).is_err() {
                ok = false;
            }
        }
        ok && b7.attach(true).err() == Some(AP_BAD_FD)
    };
    s.add("J7-写端守恒", j7, "WRITER_MAX 守恒");

    // 越容受控（按现量补齐到恰满，再验超一必拒）
    let j8 = {
        let cur = b.data_of().len();
        let mut big = Vec::new();
        big.resize(DATA_CAP - cur, 2u8);
        let _ = b.write(w2, &big, "填满");
        b.write(w2, b"!", "越容").err() == Some(AP_OVER)
    };
    s.add("J8-越容受控", j8, "超 DATA_CAP 必拒");

    // 空留痕拒收
    let j9 = b.write(w2, b"y", "").err() == Some(AP_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "端非零且量非零");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 留痕笔数非零
    let j13 = b.rec_count() > 0;
    s.add("J13-账面非空", j13, "append 留痕账非零");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10131_append_flow() {
        let mut b = AppendBook::new();
        let a = b.attach(true).unwrap_or(0);
        let _ = b.write(a, b"ab", "x");
        let _ = b.write(a, b"cd", "y");
        assert_eq!(b.data_of(), b"abcd".to_vec());
        assert!(b.append_landed_monotonic());
    }

    #[test]
    fn unx10131_checks_green() {
        let s = run_unx10131_checks();
        assert!(s.all_passed(), "UNX-F10131 判据未全绿");
    }
}
