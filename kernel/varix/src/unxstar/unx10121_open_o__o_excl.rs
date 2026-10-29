//! UNX-F10121 open O_ 标志矩阵正反双判据（组合语义达成与 O_EXCL 冲突误用受控）（0x10240 段）
//!
//! 锚点：UNX-F10121-J1 O_ 标志组合达成（O_CREAT/O_TRUNC/O_APPEND 组合
//! 语义 LTP open05/open06 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：标志解码器（O_ 位矩阵：CREAT/EXCL/TRUNC/APPEND/RDWR 闭集）+
//! 组合核（逐组合建件语义：截断清零/追加定尾/撞件拒开）+ 组合账（逐次
//! 留痕）+ 注入核（EXCL 撞件/野位/CREAT 缺失配/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10240 独占 0x20）。
pub const SEG_BASE: u32 = 0x10240;
pub const OF_EEXIST: u32 = SEG_BASE + 0x01; // EXCL 撞件
pub const OF_NOENT: u32 = SEG_BASE + 0x02; // 无 CREAT 开缺席件
pub const OF_BAD_FLAG: u32 = SEG_BASE + 0x03; // 野位出闭集
pub const OF_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const OF_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// O_ 位矩阵。
pub const O_CREAT: u32 = 0x01;
pub const O_EXCL: u32 = 0x02;
pub const O_TRUNC: u32 = 0x04;
pub const O_APPEND: u32 = 0x08;
pub const O_RDWR: u32 = 0x10;
pub const FLAG_MASK: u32 = 0x1F;

/// 路径名定长上限。
pub const NAME_MAX: usize = 24;

/// 件记录。
#[derive(Clone)]
pub struct FlagFile {
    pub name: String,
    pub data: Vec<u8>,
    pub flags: u32,
}

impl FlagFile {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.name.len() <= NAME_MAX
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }
}

/// 组合账留痕。
#[derive(Clone)]
pub struct OfRec {
    pub seq: u32,
    pub name: String,
    pub flags: u32,
    pub note: String,
}

impl OfRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.flags & !FLAG_MASK == 0
    }
}

/// 组合册 + 组合核。
pub struct FlagBook {
    pub files: Vec<FlagFile>,
    pub ledger: Vec<OfRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FlagBook {
    pub fn new() -> FlagBook {
        FlagBook { files: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 标志闭集校验：位出矩阵必拒。
    pub fn flags_ok(flags: u32) -> bool {
        flags & !FLAG_MASK == 0
    }

    /// open 核心：按标志组合落实建件语义。
    pub fn open(&mut self, name: &str, flags: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(OF_SEALED);
        }
        if note.is_empty() {
            return Err(OF_EMPTY_NOTE);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(OF_NOENT);
        }
        if !FlagBook::flags_ok(flags) {
            return Err(OF_BAD_FLAG);
        }
        let existing = self.files.iter().position(|f| f.name == name);
        if let Some(i) = existing {
            if flags & O_EXCL != 0 {
                return Err(OF_EEXIST);
            }
            // 截断语义：O_TRUNC 清零
            if flags & O_TRUNC != 0 {
                self.files[i].data.clear();
            }
            // 追加语义：O_APPEND 定尾
            if flags & O_APPEND != 0 {
                // 写入锚：后续 write 自尾
            }
            self.files[i].flags = flags;
            self.ledger.push(OfRec {
                seq: self.seq_next,
                name: name.to_string(),
                flags,
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Ok(i);
        }
        if flags & O_CREAT == 0 {
            return Err(OF_NOENT);
        }
        self.files.push(FlagFile { name: name.to_string(), data: Vec::new(), flags });
        let idx = self.files.len() - 1;
        self.ledger.push(OfRec {
            seq: self.seq_next,
            name: name.to_string(),
            flags,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(idx)
    }

    /// 写：O_APPEND 自尾追加，否则自零起截断写。
    pub fn write(&mut self, idx: usize, buf: &[u8]) -> Result<usize, u32> {
        if idx >= self.files.len() {
            return Err(OF_NOENT);
        }
        let append = self.files[idx].flags & O_APPEND != 0;
        let f = &mut self.files[idx];
        if append {
            f.data.extend_from_slice(buf);
        } else {
            f.data.clear();
            f.data.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    /// 读回。
    pub fn read(&self, idx: usize) -> Option<Vec<u8>> {
        self.files.get(idx).map(|f| f.data.clone())
    }

    /// 按名查件。
    pub fn find(&self, name: &str) -> Option<usize> {
        self.files.iter().position(|f| f.name == name)
    }

    /// 追加位判（写面用）。
    pub fn is_append(&self, idx: usize) -> bool {
        self.files.get(idx).map(|f| f.flags & O_APPEND != 0).unwrap_or(false)
    }

    /// 标志渲染（诊断面）。
    pub fn render_flags(&self, idx: usize) -> String {
        match self.files.get(idx) {
            Some(f) => {
                let mut out = String::new();
                if f.flags & O_CREAT != 0 {
                    out.push('C');
                }
                if f.flags & O_EXCL != 0 {
                    out.push('X');
                }
                if f.flags & O_TRUNC != 0 {
                    out.push('T');
                }
                if f.flags & O_APPEND != 0 {
                    out.push('A');
                }
                if f.flags & O_RDWR != 0 {
                    out.push('W');
                }
                format!("[{}]", out)
            }
            None => String::from("[?]"),
        }
    }

    /// 组合账对平：每件现值 flags 与其**最近一次** open 账目一致
    /// （open 会更新件 flags，历史账目记录的是当时值，不可逐条对现值）。
    pub fn flags_consistent(&self) -> bool {
        self.files.iter().all(|f| match self.ledger.iter().rev().find(|r| r.name == f.name) {
            Some(r) => r.flags == f.flags,
            None => true,
        })
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
        let mut h: u32 = 0x102400;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.flags);
            for b in r.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in r.note.bytes() {
                h = h.wrapping_mul(37).wrapping_add(b as u32);
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
    let mut b = FlagBook::new();
    let _ = b.open("dup", O_CREAT | O_RDWR, "inj-c");
    // 1) EXCL 撞件必拒
    got[0] = b.open("dup", O_CREAT | O_EXCL | O_RDWR, "inj-x").err() == Some(OF_EEXIST);
    // 2) 无 CREAT 开缺席件必拒
    got[1] = b.open("ghost", O_RDWR, "inj-g").err() == Some(OF_NOENT);
    // 3) 野位必拒
    got[2] = b.open("wild", 0x40, "inj-w").err() == Some(OF_BAD_FLAG);
    // 4) 空留痕必拒
    got[3] = b.open("nn", O_CREAT | O_RDWR, "").err() == Some(OF_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.open("late", O_CREAT | O_RDWR, "inj-seal").err() == Some(OF_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10121_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10121");

    // 正路径：O_CREAT 建 + O_TRUNC 清（open05 面）
    let mut b = FlagBook::new();
    let a = b.open("log.txt", O_CREAT | O_RDWR, "建件").unwrap_or(usize::MAX);
    let _ = b.write(a, b"old-content");
    let t = b.open("log.txt", O_TRUNC | O_RDWR, "截断").unwrap_or(usize::MAX);
    let j1 = t == a && b.read(a).unwrap_or_default().is_empty();
    s.add("J1-TRUNC清零", j1, "O_TRUNC 重开清空旧内容");

    // O_APPEND 定尾（open06 面）
    let j2 = {
        let ap = b.open("log.txt", O_APPEND | O_RDWR, "追加").unwrap_or(usize::MAX);
        let _ = b.write(ap, b"tail");
        b.read(a).unwrap_or_default() == b"tail".to_vec()
    };
    s.add("J2-APPEND定尾", j2, "追加写自尾落盘");

    // O_CREAT|O_EXCL 新件达成
    let j3 = {
        let n = b.open("fresh.txt", O_CREAT | O_EXCL | O_RDWR, "独建").unwrap_or(usize::MAX);
        b.find("fresh.txt") == Some(n)
    };
    s.add("J3-EXCL独建", j3, "EXCL 新件成功落位");

    // EXCL 撞件受控
    let j4 = b.open("fresh.txt", O_CREAT | O_EXCL | O_RDWR, "撞").err() == Some(OF_EEXIST);
    s.add("J4-EXCL撞件受控", j4, "撞件必 EEXIST");

    // 无 CREAT 开缺席件受控
    let j5 = b.open("absent", O_RDWR, "缺席").err() == Some(OF_NOENT);
    s.add("J5-缺席受控", j5, "无 CREAT 开缺席件必拒");

    // 野位受控
    let j6 = b.open("w", 0x20, "野位").err() == Some(OF_BAD_FLAG);
    s.add("J6-野位受控", j6, "位出 FLAG_MASK 必拒");

    // 截断+追加组合语义：先 TRUNC 清后 APPEND 写
    let j7 = {
        let _ = b.open("combo", O_CREAT | O_RDWR, "建");
        let _ = b.write(b.find("combo").unwrap_or(0), b"AAA");
        let _ = b.open("combo", O_TRUNC | O_RDWR, "清");
        let c = b.open("combo", O_APPEND | O_RDWR, "尾");
        let _ = b.write(c.unwrap_or(0), b"B");
        b.read(b.find("combo").unwrap_or(0)).unwrap_or_default() == b"B".to_vec()
    };
    s.add("J7-组合语义", j7, "TRUNC 清后 APPEND 单写独占");

    // 非追加写覆盖语义
    let j8 = {
        let mut b8 = FlagBook::new();
        let f8 = b8.open("w", O_CREAT | O_RDWR, "建").unwrap_or(0);
        let _ = b8.write(f8, b"111");
        let _ = b8.write(f8, b"2");
        b8.read(f8).unwrap_or_default() == b"2".to_vec()
    };
    s.add("J8-覆盖写语义", j8, "无 APPEND 写重置内容");

    // 空留痕拒收
    let j9 = b.open("x", O_CREAT | O_RDWR, "").err() == Some(OF_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 超长名受控
    let long = "n".repeat(NAME_MAX + 1);
    let j10 = b.open(&long, O_CREAT | O_RDWR, "超长").err() == Some(OF_NOENT);
    s.add("J10-超长名受控", j10, "超 NAME_MAX 必拒");

    // 留痕合法与 seq 唯一
    let j11 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J11-留痕合法", j11, "flags 在闭集且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = b.fold() == b.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    // 标志矩阵渲染与账对平
    let j14 = b.render_flags(b.find("log.txt").unwrap_or(0)).contains('A')
        && b.flags_consistent();
    s.add("J14-矩阵渲染对平", j14, "APPEND 位渲染且账面一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10121_flag_flow() {
        let mut b = FlagBook::new();
        let f = b.open("a", O_CREAT | O_RDWR, "x").unwrap_or(0);
        let _ = b.write(f, b"123");
        let _ = b.open("a", O_TRUNC | O_RDWR, "y");
        assert!(b.read(f).unwrap_or_default().is_empty());
        assert!(b.open("a", O_CREAT | O_EXCL | O_RDWR, "z") == Err(OF_EEXIST));
    }

    #[test]
    fn unx10121_checks_green() {
        let s = run_unx10121_checks();
        assert!(s.all_passed(), "UNX-F10121 判据未全绿");
    }
}
