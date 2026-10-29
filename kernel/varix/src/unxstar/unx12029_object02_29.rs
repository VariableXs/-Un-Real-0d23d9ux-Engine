//! UNX-F12029 STATUS_OBJECT_* 对象管理错误族（D3 联签预告）（0x195C0 段）
//!
//! 判据原文：对象名空间四拒面——重名创建（OBJECT_NAME_COLLISION
//! 0xC0000035）、查无名（OBJECT_NAME_NOT_FOUND 0xC0000034）、路径语法
//! 坏（OBJECT_PATH_SYNTAX_BAD 0xC000003B，须以 '\' 起头且无空构件）、
//! 类型错配（OBJECT_TYPE_MISMATCH 0xC0000024）；四拒面逐笔入账对平。
//!
//! 结构：名空间册（NsBook：路径 × 类型）+ 创建/查删双核（五码守卫）+
//! 拒账（ObRec 分面留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12029_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x195C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x195C0;
pub const OB_BAD_SYNTAX: u32 = SEG_BASE + 0x01; // 路径语法坏
pub const OB_COLLISION: u32 = SEG_BASE + 0x02; // 重名创建
pub const OB_NOT_FOUND: u32 = SEG_BASE + 0x03; // 查无名
pub const OB_TYPE_MISMATCH: u32 = SEG_BASE + 0x04; // 类型错配
pub const OB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 语义钉值。
pub const OBJECT_NAME_COLLISION: u32 = 0xC000_0035;
pub const OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;

/// 对象类型闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObType {
    Directory = 1,
    File = 2,
    Event = 3,
    SymbolicLink = 4,
}

/// 一笔拒账。
#[derive(Clone)]
pub struct ObRec {
    pub seq: u32,
    pub path: String,
    /// 拒面：1=语法 2=重名 3=查无 4=类型。
    pub face: u8,
    pub note: String,
}

impl ObRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 名空间册。
pub struct NsBook {
    pub entries: Vec<(String, ObType)>,
    pub ledger: Vec<ObRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl NsBook {
    pub fn new() -> NsBook {
        NsBook { entries: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn pos(&self, path: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.0 == path)
    }

    fn rec(&mut self, path: &str, face: u8, note: &str) {
        self.ledger.push(ObRec {
            seq: self.seq_next,
            path: path.to_string(),
            face,
            note: note.to_string(),
        });
        self.seq_next += 1;
    }

    /// 路径语法核：'\' 起头、无空构件、非空。
    pub fn syntax_ok(path: &str) -> bool {
        if !path.starts_with('\\') || path.len() < 2 {
            return false;
        }
        // 空构件：连续 '\' 或尾 '\'。
        if path.contains("\\\\") || path.ends_with('\\') {
            return false;
        }
        true
    }

    /// 创建核：语法 → 重名 → 落册。
    pub fn create(&mut self, path: &str, t: ObType, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(OB_FROZEN);
        }
        if note.is_empty() {
            return Err(OB_BAD_SYNTAX);
        }
        if !Self::syntax_ok(path) {
            self.rec(path, 1, note);
            return Err(OB_BAD_SYNTAX);
        }
        if self.pos(path).is_some() {
            self.rec(path, 2, note);
            return Err(OB_COLLISION);
        }
        self.entries.push((path.to_string(), t));
        Ok(())
    }

    /// 查询核：语法 → 查无 → 类型核对。
    pub fn lookup(&mut self, path: &str, want: ObType, note: &str) -> Result<ObType, u32> {
        if self.sealed {
            return Err(OB_FROZEN);
        }
        if note.is_empty() {
            return Err(OB_BAD_SYNTAX);
        }
        if !Self::syntax_ok(path) {
            self.rec(path, 1, note);
            return Err(OB_BAD_SYNTAX);
        }
        let got = match self.pos(path) {
            Some(i) => self.entries[i].1,
            None => {
                self.rec(path, 3, note);
                return Err(OB_NOT_FOUND);
            }
        };
        if got != want {
            self.rec(path, 4, note);
            return Err(OB_TYPE_MISMATCH);
        }
        Ok(got)
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
            for by in e.0.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(211).wrapping_add(e.1 as u32);
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
    let mut b = NsBook::new();
    let _ = b.create("\\FileSystem\\Demo", ObType::File, "ok");
    // 1) 路径语法坏（无 '\' 起头）→ OB_BAD_SYNTAX。
    got[0] = b.create("NoSlash", ObType::File, "s").err() == Some(OB_BAD_SYNTAX);
    // 2) 重名创建 → OB_COLLISION。
    got[1] =
        b.create("\\FileSystem\\Demo", ObType::File, "dup").err() == Some(OB_COLLISION);
    // 3) 查无名 → OB_NOT_FOUND。
    got[2] = b.lookup("\\FileSystem\\Ghost", ObType::File, "g").err() == Some(OB_NOT_FOUND);
    // 4) 类型错配 → OB_TYPE_MISMATCH。
    got[3] =
        b.lookup("\\FileSystem\\Demo", ObType::Event, "t").err() == Some(OB_TYPE_MISMATCH);
    // 5) 印账后动作 → OB_FROZEN。
    b.seal();
    got[4] = b.create("\\X", ObType::File, "post").err() == Some(OB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12029");

    // J1 正路径：创建→查回类型一致。
    let j1 = {
        let mut b = NsBook::new();
        let _ = b.create("\\Kernel\\Objects\\Demo", ObType::Event, "c");
        b.lookup("\\Kernel\\Objects\\Demo", ObType::Event, "l") == Ok(ObType::Event)
    };
    s.add("J1-创建查回", j1, "类型一致");

    // J2 语法核：合法三例。
    let j2 = NsBook::syntax_ok("\\X")
        && NsBook::syntax_ok("\\A\\B")
        && NsBook::syntax_ok("\\Kernel\\Objects\\Demo");
    s.add("J2-语法合法面", j2, "三合法路径");

    // J3 语法坏三面：无斜杠/连续斜杠/尾斜杠。
    let j3 = !NsBook::syntax_ok("NoSlash")
        && !NsBook::syntax_ok("\\A\\\\B")
        && !NsBook::syntax_ok("\\A\\");
    s.add("J3-语法坏面", j3, "三坏路径全拒");

    // J4 重名创建拒。
    let j4 = {
        let mut b = NsBook::new();
        let _ = b.create("\\X", ObType::File, "a");
        b.create("\\X", ObType::Event, "b").err() == Some(OB_COLLISION)
    };
    s.add("J4-重名拒", j4, "名字唯一");

    // J5 查无名拒。
    let j5 = {
        let mut b = NsBook::new();
        b.lookup("\\Ghost", ObType::File, "g").err() == Some(OB_NOT_FOUND)
    };
    s.add("J5-查无拒", j5, "无名可查");

    // J6 类型错配拒。
    let j6 = {
        let mut b = NsBook::new();
        let _ = b.create("\\Obj", ObType::Directory, "c");
        b.lookup("\\Obj", ObType::File, "l").err() == Some(OB_TYPE_MISMATCH)
    };
    s.add("J6-类型错配拒", j6, "类型面精确");

    // J7 语义钉值核对。
    let j7 = OBJECT_NAME_COLLISION == 0xC000_0035 && OBJECT_NAME_NOT_FOUND == 0xC000_0034;
    s.add("J7-钉值核对", j7, "0x35/0x34");

    // J8 四拒面对平。
    let j8 = {
        let mut b = NsBook::new();
        let _ = b.create("\\Demo", ObType::File, "c");
        let _ = b.create("bad", ObType::File, "s1");
        let _ = b.create("\\Demo", ObType::File, "s2");
        let _ = b.lookup("\\Ghost", ObType::File, "s3");
        let _ = b.lookup("\\Demo", ObType::Event, "s4");
        b.count_face(1) == 1 && b.count_face(2) == 1 && b.count_face(3) == 1
            && b.count_face(4) == 1 && b.faces_tally()
    };
    s.add("J8-四面对平", j8, "各一脚");

    // J9 账目自洽。
    let j9 = {
        let mut b = NsBook::new();
        let _ = b.create("\\A", ObType::File, "a");
        let _ = b.lookup("\\B", ObType::File, "b");
        b.ledger_ok() && b.ledger.len() == 1
    };
    s.add("J9-账目自洽", j9, "查无入账过核");

    // J10 印账冻结。
    let j10 = {
        let mut b = NsBook::new();
        let _ = b.create("\\A", ObType::File, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2
            && b.create("\\B", ObType::File, "post").err() == Some(OB_FROZEN)
            && b.lookup("\\A", ObType::File, "post").err() == Some(OB_FROZEN)
    };
    s.add("J10-指纹封账", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：目录树。
    let mut b = NsBook::new();
    let _ = b.create("\\Root", ObType::Directory, "sum-r");
    let _ = b.create("\\Root\\File", ObType::File, "sum-f");
    s.add("样例双对象", b.entries.len() == 2, "目录+文件");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12029_ns_flow() {
        let mut b = NsBook::new();
        assert!(b.create("\\A", ObType::File, "a").is_ok());
        assert_eq!(b.create("\\A", ObType::File, "b"), Err(OB_COLLISION));
        assert_eq!(b.lookup("\\A", ObType::Event, "c"), Err(OB_TYPE_MISMATCH));
        assert_eq!(b.lookup("\\Z", ObType::File, "d"), Err(OB_NOT_FOUND));
        assert_eq!(b.create("bad", ObType::File, "e"), Err(OB_BAD_SYNTAX));
    }

    #[test]
    fn unx12029_checks_green() {
        let s = run_unx12029_checks();
        assert!(s.all_passed(), "UNX-F12029 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12029_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
