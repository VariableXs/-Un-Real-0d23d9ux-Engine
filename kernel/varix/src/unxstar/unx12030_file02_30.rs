//! UNX-F12030 STATUS_FILE_* 文件错误族预告（D2/B1 联签）（0x195E0 段）
//!
//! 判据原文：文件操作四拒面——对目录做文件读写（FILE_IS_A_DIRECTORY
//! 0xC00000BA）、读越 EOF（STATUS_END_OF_FILE 0xC0000011）、对已关句柄
//! 操作（FILE_CLOSED 0xC0000128）、对文件做目录操作（NOT_A_DIRECTORY
//! 0xC0000103）；拒面逐笔入账对平。
//!
//! 结构：文件册（FileBook：句柄 × 态 × 位置 × 尺寸 × 目录位）+ 读/写双核
//! （五码守卫）+ 拒账（FlRec 分面留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12030_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x195E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x195E0;
pub const FL_IS_DIR: u32 = SEG_BASE + 0x01; // 目录做文件操作
pub const FL_EOF: u32 = SEG_BASE + 0x02; // 读越 EOF
pub const FL_CLOSED: u32 = SEG_BASE + 0x03; // 已关句柄操作
pub const FL_NOT_DIR: u32 = SEG_BASE + 0x04; // 文件做目录操作
pub const FL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 语义钉值。
pub const STATUS_END_OF_FILE: u32 = 0xC000_0011;

/// 文件条目。
#[derive(Clone)]
pub struct FlEntry {
    pub handle: u32,
    pub is_dir: bool,
    pub open: bool,
    pub pos: u64,
    pub size: u64,
}

/// 一笔拒账。
#[derive(Clone)]
pub struct FlRec {
    pub seq: u32,
    pub handle: u32,
    /// 拒面：1=目录 2=EOF 3=已关 4=非目录。
    pub face: u8,
    pub note: String,
}

impl FlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 文件册模型。
pub struct FileBook {
    pub entries: Vec<FlEntry>,
    pub ledger: Vec<FlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FileBook {
    pub fn new() -> FileBook {
        FileBook { entries: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn pos(&self, h: u32) -> Option<usize> {
        self.entries.iter().position(|e| e.handle == h)
    }

    fn rec(&mut self, h: u32, face: u8, note: &str) {
        self.ledger.push(FlRec { seq: self.seq_next, handle: h, face, note: note.to_string() });
        self.seq_next += 1;
    }

    /// 装册。
    pub fn enlist(&mut self, h: u32, is_dir: bool, size: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FL_FROZEN);
        }
        if note.is_empty() || h == 0 || self.pos(h).is_some() {
            return Err(FL_CLOSED);
        }
        self.entries.push(FlEntry { handle: h, is_dir, open: true, pos: 0, size });
        Ok(())
    }

    /// 读核：已关 → 目录 → 越EOF 三面。
    pub fn read(&mut self, h: u32, nbytes: u64, note: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(FL_FROZEN);
        }
        if note.is_empty() {
            return Err(FL_CLOSED);
        }
        let i = match self.pos(h) {
            Some(i) => i,
            None => {
                self.rec(h, 3, note);
                return Err(FL_CLOSED);
            }
        };
        if !self.entries[i].open {
            self.rec(h, 3, note);
            return Err(FL_CLOSED);
        }
        if self.entries[i].is_dir {
            self.rec(h, 1, note);
            return Err(FL_IS_DIR);
        }
        // 越读：位置+n 超尺寸判 EOF（相加防回绕：n > size-pos 才越）。
        let e = &self.entries[i];
        if nbytes > e.size - e.pos {
            self.rec(h, 2, note);
            return Err(FL_EOF);
        }
        self.entries[i].pos += nbytes;
        Ok(self.entries[i].pos)
    }

    /// 目录枚举核：仅目录可枚举。
    pub fn enumerate(&mut self, h: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FL_FROZEN);
        }
        if note.is_empty() {
            return Err(FL_CLOSED);
        }
        let i = match self.pos(h) {
            Some(i) => i,
            None => {
                self.rec(h, 3, note);
                return Err(FL_CLOSED);
            }
        };
        if !self.entries[i].open {
            self.rec(h, 3, note);
            return Err(FL_CLOSED);
        }
        if !self.entries[i].is_dir {
            self.rec(h, 4, note);
            return Err(FL_NOT_DIR);
        }
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self, h: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FL_FROZEN);
        }
        let i = match self.pos(h) {
            Some(i) => i,
            None => return Err(FL_CLOSED),
        };
        if !self.entries[i].open {
            return Err(FL_CLOSED);
        }
        self.entries[i].open = false;
        Ok(())
    }

    /// 拒面清点。
    pub fn count_face(&self, face: u8) -> usize {
        self.ledger.iter().filter(|r| r.face == face).count()
    }

    /// 对平面：四拒面合计==账总数。
    pub fn faces_tally(&self) -> bool {
        self.count_face(1) + self.count_face(2) + self.count_face(3) + self.count_face(4)
            == self.ledger.len()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.handle);
            h = h.wrapping_mul(211).wrapping_add(e.open as u32);
            h = h.wrapping_mul(211).wrapping_add((e.pos & 0xFFFF) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = FileBook::new();
    let _ = b.enlist(0x11, true, 0, "dir");
    let _ = b.enlist(0x22, false, 0x100, "file");
    // 1) 目录做读 → FL_IS_DIR。
    got[0] = b.read(0x11, 8, "dir-rd").err() == Some(FL_IS_DIR);
    // 2) 读越 EOF → FL_EOF。
    got[1] = b.read(0x22, 0x101, "eof").err() == Some(FL_EOF);
    // 3) 已关句柄读 → FL_CLOSED。
    let _ = b.close(0x22);
    got[2] = b.read(0x22, 8, "closed").err() == Some(FL_CLOSED);
    // 4) 文件做目录枚举 → FL_NOT_DIR。
    let _ = b.enlist(0x33, false, 0x10, "f2");
    got[3] = b.enumerate(0x33, "enum").err() == Some(FL_NOT_DIR);
    // 5) 印账后动作 → FL_FROZEN。
    b.seal();
    got[4] = b.read(0x11, 0, "post").err() == Some(FL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12030");

    // J1 正路径：读推进位置。
    let j1 = {
        let mut b = FileBook::new();
        let _ = b.enlist(1, false, 0x100, "f");
        b.read(1, 0x40, "r1") == Ok(0x40) && b.read(1, 0x40, "r2") == Ok(0x80)
    };
    s.add("J1-读推进", j1, "0x40+0x40=0x80");

    // J2 目录读拒。
    let j2 = {
        let mut b = FileBook::new();
        let _ = b.enlist(2, true, 0, "d");
        b.read(2, 8, "rd").err() == Some(FL_IS_DIR)
    };
    s.add("J2-目录读拒", j2, "FILE_IS_A_DIRECTORY");

    // J3 越EOF拒：恰差一字节与整越双面。
    let j3 = {
        let mut b = FileBook::new();
        let _ = b.enlist(3, false, 0x10, "f");
        let one = b.read(3, 0x11, "a").err() == Some(FL_EOF);
        let mut b2 = FileBook::new();
        let _ = b2.enlist(4, false, 0x10, "f");
        let far = b2.read(4, 0x100, "b").err() == Some(FL_EOF);
        one && far
    };
    s.add("J3-越EOF拒", j3, "差 1 字节亦拒");

    // J4 边界收：恰读满不越。
    let j4 = {
        let mut b = FileBook::new();
        let _ = b.enlist(5, false, 0x10, "f");
        b.read(5, 0x10, "full") == Ok(0x10)
    };
    s.add("J4-恰满边界", j4, "读满合法");

    // J5 已关句柄拒。
    let j5 = {
        let mut b = FileBook::new();
        let _ = b.enlist(6, false, 8, "f");
        let _ = b.close(6);
        b.read(6, 1, "x").err() == Some(FL_CLOSED)
    };
    s.add("J5-已关拒", j5, "FILE_CLOSED");

    // J6 幽灵句柄拒。
    let j6 = {
        let mut b = FileBook::new();
        b.read(0xDEAD, 1, "g").err() == Some(FL_CLOSED)
    };
    s.add("J6-幽灵拒", j6, "未装册句柄同拒面");

    // J7 文件做目录枚举拒。
    let j7 = {
        let mut b = FileBook::new();
        let _ = b.enlist(7, false, 8, "f");
        b.enumerate(7, "e").err() == Some(FL_NOT_DIR)
    };
    s.add("J7-非目录拒", j7, "NOT_A_DIRECTORY");

    // J8 目录枚举放行。
    let j8 = {
        let mut b = FileBook::new();
        let _ = b.enlist(8, true, 0, "d");
        b.enumerate(8, "e").is_ok()
    };
    s.add("J8-目录枚举过", j8, "真目录可枚举");

    // J9 拒面对平。
    let j9 = {
        let mut b = FileBook::new();
        let _ = b.enlist(9, true, 0, "d");
        let _ = b.enlist(10, false, 8, "f");
        let _ = b.read(9, 1, "a");
        let _ = b.read(10, 9, "b");
        let _ = b.enumerate(10, "c");
        let _ = b.close(10);
        let _ = b.read(10, 1, "e");
        b.count_face(1) == 1 && b.count_face(2) == 1 && b.count_face(3) == 1
            && b.count_face(4) == 1 && b.faces_tally()
    };
    s.add("J9-四面对平", j9, "各一脚");

    // J10 印账冻结。
    let j10 = {
        let mut b = FileBook::new();
        let _ = b.enlist(11, false, 8, "f");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.read(11, 1, "post").err() == Some(FL_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：文件读面。
    let mut b = FileBook::new();
    let _ = b.enlist(0x40, false, 0x1000, "sum-f");
    let _ = b.read(0x40, 0x200, "sum-1");
    s.add("样例位置核对", b.read(0x40, 0x200, "sum-2") == Ok(0x400), "0x400 对账");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12030_file_flow() {
        let mut b = FileBook::new();
        assert!(b.enlist(1, false, 0x20, "f").is_ok());
        assert_eq!(b.read(1, 0x21, "a"), Err(FL_EOF));
        assert_eq!(b.read(1, 0x20, "b"), Ok(0x20));
        assert!(b.close(1).is_ok());
        assert_eq!(b.read(1, 1, "c"), Err(FL_CLOSED));
    }

    #[test]
    fn unx12030_checks_green() {
        let s = run_unx12030_checks();
        assert!(s.all_passed(), "UNX-F12030 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12030_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
