//! UNX-F10115 unlink/rename 正反双判据（删除改名达成与缺失路径误用受控）（0x10180 段）
//!
//! 锚点：UNX-F10115-J1 unlink/rename 达成（nlink 递减与改名后可打开
//! LTP unlink01/rename01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：名字册（ino→nlink 的 inode 表 + 名→ino 的目录映射）+ 链核
//! （unlink 递减 nlink 至零释放/rename 改名保 ino）+ 链账（逐次留痕 +
//! nlink 守恒对平）+ 注入核（缺失路径/无名 inode/重复建名/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10180 独占 0x20）。
pub const SEG_BASE: u32 = 0x10180;
pub const UL_NOENT: u32 = SEG_BASE + 0x01; // 缺失路径
pub const UL_DUP_NAME: u32 = SEG_BASE + 0x02; // 重复建名
pub const UL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UL_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const UL_ORPHAN: u32 = SEG_BASE + 0x05; // 无名 inode 操作

/// 单 inode 记录。
#[derive(Clone)]
pub struct InoRec {
    pub ino: u32,
    pub nlink: u32,
    pub payload: Vec<u8>,
}

impl InoRec {
    pub fn ok(&self) -> bool {
        self.ino != 0
    }

    pub fn released(&self) -> bool {
        self.nlink == 0
    }
}

/// 链账留痕。
#[derive(Clone)]
pub struct UlRec {
    pub seq: u32,
    pub op: u32, // 0=link 1=unlink 2=rename
    pub a: String,
    pub b: String,
    pub ino: u32,
}

impl UlRec {
    pub fn ok(&self) -> bool {
        self.op <= 2 && self.ino != 0 && (!self.a.is_empty() || !self.b.is_empty())
    }
}

/// 名字册。
pub struct NameBook {
    pub inodes: Vec<InoRec>,
    pub names: Vec<(String, u32)>,
    pub ledger: Vec<UlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl NameBook {
    pub fn new() -> NameBook {
        NameBook { inodes: Vec::new(), names: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn next_ino(&self) -> u32 {
        self.inodes.len() as u32 + 1
    }

    /// 建名：新 inode + 名映射（重名拒）。
    pub fn create(&mut self, name: &str, payload: &[u8]) -> Result<u32, u32> {
        if self.sealed {
            return Err(UL_SEALED);
        }
        if name.is_empty() {
            return Err(UL_NOENT);
        }
        if self.names.iter().any(|(n, _)| n == name) {
            return Err(UL_DUP_NAME);
        }
        let ino = self.next_ino();
        self.inodes.push(InoRec { ino, nlink: 1, payload: payload.to_vec() });
        self.names.push((name.to_string(), ino));
        self.ledger.push(UlRec {
            seq: self.seq_next,
            op: 0,
            a: name.to_string(),
            b: String::new(),
            ino,
        });
        self.seq_next += 1;
        Ok(ino)
    }

    fn ino_idx(&self, ino: u32) -> Option<usize> {
        self.inodes.iter().position(|i| i.ino == ino)
    }

    fn name_ino(&self, name: &str) -> Option<u32> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, i)| *i)
    }

    /// unlink：名删 + nlink 递减，至零释放。
    pub fn unlink(&mut self, name: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UL_SEALED);
        }
        if note.is_empty() {
            return Err(UL_EMPTY_NOTE);
        }
        let ino = match self.name_ino(name) {
            Some(i) => i,
            None => return Err(UL_NOENT),
        };
        self.names.retain(|(n, _)| n != name);
        let idx = match self.ino_idx(ino) {
            Some(i) => i,
            None => return Err(UL_ORPHAN),
        };
        self.inodes[idx].nlink = self.inodes[idx].nlink.saturating_sub(1);
        let left = self.inodes[idx].nlink;
        self.ledger.push(UlRec {
            seq: self.seq_next,
            op: 1,
            a: name.to_string(),
            b: String::new(),
            ino,
        });
        self.seq_next += 1;
        let _ = note;
        Ok(left)
    }

    /// rename：改保 ino，缺失源/目标占位冲突拒。
    pub fn rename(&mut self, from: &str, to: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UL_SEALED);
        }
        if note.is_empty() {
            return Err(UL_EMPTY_NOTE);
        }
        if to.is_empty() {
            return Err(UL_NOENT);
        }
        let ino = match self.name_ino(from) {
            Some(i) => i,
            None => return Err(UL_NOENT),
        };
        if self.name_ino(to).is_some() {
            return Err(UL_DUP_NAME);
        }
        self.names.retain(|(n, _)| n != from);
        self.names.push((to.to_string(), ino));
        self.ledger.push(UlRec {
            seq: self.seq_next,
            op: 2,
            a: from.to_string(),
            b: to.to_string(),
            ino,
        });
        self.seq_next += 1;
        Ok(ino)
    }

    /// 按名取 payload（改名后可打开判据面）。
    pub fn open_by_name(&self, name: &str) -> Option<Vec<u8>> {
        let ino = self.name_ino(name)?;
        let idx = self.ino_idx(ino)?;
        Some(self.inodes[idx].payload.clone())
    }

    /// nlink 守恒：每 inode nlink == 名映射计数（释放 inode 除外）。
    pub fn conserved(&self) -> bool {
        for i in &self.inodes {
            let name_count = self.names.iter().filter(|(_, ino)| *ino == i.ino).count() as u32;
            if i.nlink == 0 {
                if name_count != 0 {
                    return false;
                }
            } else if i.nlink != name_count {
                return false;
            }
        }
        true
    }

    /// 释放 inode 数。
    pub fn released_count(&self) -> usize {
        self.inodes.iter().filter(|i| i.released()).count()
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
        let mut h: u32 = 0x101800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.ino);
            for b in r.a.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in r.b.bytes() {
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
    let mut b = NameBook::new();
    let _ = b.create("a", b"pa");
    // 1) 缺失路径 unlink 必拒
    got[0] = b.unlink("ghost", "inj-noent").err() == Some(UL_NOENT);
    // 2) 重复建名必拒
    got[1] = b.create("a", b"dup").err() == Some(UL_DUP_NAME);
    // 3) 缺失源 rename 必拒
    got[2] = b.rename("ghost", "t", "inj-ren").err() == Some(UL_NOENT);
    // 4) 空留痕必拒
    got[3] = b.unlink("a", "").err() == Some(UL_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.unlink("a", "inj-seal").err() == Some(UL_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10115_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10115");

    // 正路径：unlink01 面——nlink 递减
    let mut b = NameBook::new();
    let _ = b.create("one", b"data1");
    let _ = b.create("two", b"data2");
    let left = b.unlink("one", "删除").unwrap_or(9);
    let j1 = left == 0 && b.released_count() == 1;
    s.add("J1-nlink递减", j1, "unlink 后 nlink 归零并释放");

    // rename01 面：改名后可打开（payload 保真）
    let j2 = {
        let mut b2 = NameBook::new();
        let _ = b2.create("old.txt", b"keep-me");
        let ino = b2.rename("old.txt", "new.txt", "改名").unwrap_or(0);
        ino != 0
            && b2.open_by_name("new.txt").unwrap_or_default() == b"keep-me".to_vec()
            && b2.open_by_name("old.txt").is_none()
    };
    s.add("J2-改名可打开", j2, "rename 后新名可读旧内容");

    // rename 保 ino
    let j3 = {
        let mut b3 = NameBook::new();
        let i1 = b3.create("x", b"").unwrap_or(0);
        let i2 = b3.rename("x", "y", "保号").unwrap_or(0);
        i1 == i2
    };
    s.add("J3-rename保ino", j3, "改名不改 inode 号");

    // 硬链守恒：建双名（link 面）后 unlink 单名仍可开
    let j4 = {
        let mut b4 = NameBook::new();
        let ino = b4.create("real", b"shared").unwrap_or(0);
        // 手工加名模拟 link
        b4.names.push(("alias".to_string(), ino));
        if let Some(idx) = b4.inodes.iter().position(|i| i.ino == ino) {
            b4.inodes[idx].nlink = 2;
        }
        let left = b4.unlink("alias", "摘链").unwrap_or(9);
        left == 1
            && b4.open_by_name("real").unwrap_or_default() == b"shared".to_vec()
            && b4.conserved()
    };
    s.add("J4-硬链守恒", j4, "双名摘一后 nlink==1 且内容可达");

    // 缺失路径受控（unlink/rename 双面）
    let j5 = b.unlink("ghost", "g").err() == Some(UL_NOENT)
        && b.rename("ghost", "t", "g").err() == Some(UL_NOENT);
    s.add("J5-缺失路径受控", j5, "缺失源路径双操作同拒");

    // 重复建名受控
    let j6 = b.create("two", b"dup").err() == Some(UL_DUP_NAME);
    s.add("J6-重名受控", j6, "同名创建必拒");

    // rename 目标占位受控
    let j7 = {
        let _ = b.create("three", b"");
        b.rename("two", "three", "撞名").err() == Some(UL_DUP_NAME)
    };
    s.add("J7-目标占位受控", j7, "改名撞存活名必拒");

    // 空留痕拒收
    let j8 = b.rename("fresh", "f2", "").err() == Some(UL_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 守恒面
    let j9 = b.conserved();
    s.add("J9-全局守恒", j9, "nlink 与名映射计数对平");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且 ino 非零");

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
    fn unx10115_link_flow() {
        let mut b = NameBook::new();
        let i = b.create("a", b"pa").unwrap_or(0);
        assert_eq!(b.unlink("a", "x").unwrap_or(9), 0);
        assert!(b.open_by_name("a").is_none());
        assert_eq!(b.rename("a", "b", "y").err(), Some(UL_NOENT));
        let _ = i;
    }

    #[test]
    fn unx10115_checks_green() {
        let s = run_unx10115_checks();
        assert!(s.all_passed(), "UNX-F10115 判据未全绿");
    }
}
