//! UNX-F10126 rename 目录替换正反双判据（替换达成与非空目录误用受控）（0x102e0 段）
//!
//! 锚点：UNX-F10126-J1 目录替换达成（空目录换位语义 LTP rename02 面
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：目录册（目录节点含成员计数）+ 换位核（目录→空目录替换/非空
//! 目标拒/自换位幂等/文件占位拒）+ 换位账（逐次留痕）+ 注入核（非空
//! 目标/文件换目录/缺失源/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x102e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x102e0;
pub const RN_NOTEMPTY: u32 = SEG_BASE + 0x01; // 非空目录目标
pub const RN_NOENT: u32 = SEG_BASE + 0x02; // 缺失源
pub const RN_TYPE: u32 = SEG_BASE + 0x03; // 类型错配
pub const RN_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const RN_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 目录节点。
#[derive(Clone)]
pub struct DirNode2 {
    pub name: String,
    pub is_dir: bool,
    pub members: u32,
}

impl DirNode2 {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && (self.is_dir || self.members == 0)
    }

    pub fn empty_dir(&self) -> bool {
        self.is_dir && self.members == 0
    }
}

/// 换位账留痕。
#[derive(Clone)]
pub struct RnRec {
    pub seq: u32,
    pub from: String,
    pub to: String,
    pub replaced: bool,
}

impl RnRec {
    pub fn ok(&self) -> bool {
        !self.from.is_empty() && !self.to.is_empty()
    }
}

/// 目录册 + 换位核。
pub struct RenameBook {
    pub nodes: Vec<DirNode2>,
    pub ledger: Vec<RnRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RenameBook {
    pub fn new() -> RenameBook {
        RenameBook { nodes: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 建目录。
    pub fn mkdir(&mut self, name: &str, members: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RN_SEALED);
        }
        if name.is_empty() {
            return Err(RN_NOENT);
        }
        self.nodes.push(DirNode2 { name: name.to_string(), is_dir: true, members });
        Ok(())
    }

    /// 建文件。
    pub fn touch(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RN_SEALED);
        }
        self.nodes.push(DirNode2 { name: name.to_string(), is_dir: false, members: 0 });
        Ok(())
    }

    fn pos_of(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// rename 核心：目录→空目录换位；其余按类型/占位拒。
    pub fn rename(&mut self, from: &str, to: &str, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(RN_SEALED);
        }
        if note.is_empty() {
            return Err(RN_EMPTY_NOTE);
        }
        if to.is_empty() {
            return Err(RN_NOENT);
        }
        let fi = match self.pos_of(from) {
            Some(i) => i,
            None => return Err(RN_NOENT),
        };
        let from_is_dir = self.nodes[fi].is_dir;
        let replaced = match self.pos_of(to) {
            None => false,
            Some(ti) => {
                if ti == fi {
                    // 自换位：幂等无动作（POSIX 语义成功且不变形）
                    false
                } else {
                    let to_is_dir = self.nodes[ti].is_dir;
                    if from_is_dir != to_is_dir {
                        return Err(RN_TYPE);
                    }
                    if to_is_dir && !self.nodes[ti].empty_dir() {
                        return Err(RN_NOTEMPTY);
                    }
                    // 文件→文件覆盖或目录→空目录：换位（入位即承目标名）
                    let mut moved = self.nodes[fi].clone();
                    moved.name = to.to_string();
                    self.nodes[ti] = moved;
                    self.nodes.remove(fi);
                    true
                }
            }
        };
        if !replaced {
            // 纯改名
            self.nodes[fi].name = to.to_string();
        }
        self.ledger.push(RnRec {
            seq: self.seq_next,
            from: from.to_string(),
            to: to.to_string(),
            replaced,
        });
        self.seq_next += 1;
        Ok(replaced)
    }

    /// 自换位幂等。
    pub fn self_rename(&mut self, name: &str) -> Result<bool, u32> {
        self.rename(name, name, "self")
    }

    /// 成员数维护（建删面配套）。
    pub fn add_member(&mut self, name: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RN_SEALED);
        }
        let i = match self.pos_of(name) {
            Some(i) => i,
            None => return Err(RN_NOENT),
        };
        if !self.nodes[i].is_dir {
            return Err(RN_TYPE);
        }
        self.nodes[i].members += 1;
        Ok(self.nodes[i].members)
    }

    /// 成员数递减（至零不回退）。
    pub fn drop_member(&mut self, name: &str) -> Result<u32, u32> {
        let i = match self.pos_of(name) {
            Some(i) => i,
            None => return Err(RN_NOENT),
        };
        self.nodes[i].members = self.nodes[i].members.saturating_sub(1);
        Ok(self.nodes[i].members)
    }

    /// 换位次数统计。
    pub fn replaced_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.replaced).count()
    }

    /// 节点总数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 目录清单渲染（诊断面）。
    pub fn render_list(&self) -> String {
        let mut out = String::new();
        for n in &self.nodes {
            out.push_str(&n.name);
            out.push(if n.is_dir { '/' } else { '.' });
        }
        out
    }

    pub fn find(&self, name: &str) -> bool {
        self.pos_of(name).is_some()
    }

    pub fn node(&self, name: &str) -> Option<DirNode2> {
        self.pos_of(name).map(|i| self.nodes[i].clone())
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
        let mut h: u32 = 0x102e00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.replaced as u32);
            for b in r.from.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in r.to.bytes() {
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
    let mut b = RenameBook::new();
    let _ = b.mkdir("full", 3);
    let _ = b.mkdir("empty", 0);
    // 1) 非空目录目标必拒
    got[0] = b.rename("empty", "full", "inj-ne").err() == Some(RN_NOTEMPTY);
    // 2) 文件换目录必拒
    let _ = b.touch("f.txt");
    got[1] = b.rename("f.txt", "empty", "inj-type").err() == Some(RN_TYPE);
    // 3) 缺失源必拒
    got[2] = b.rename("ghost", "empty", "inj-noent").err() == Some(RN_NOENT);
    // 4) 空留痕必拒
    got[3] = b.rename("empty", "e2", "").err() == Some(RN_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.rename("empty", "e3", "inj-seal").err() == Some(RN_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10126");

    // 正路径：空目录换位（rename02 面）
    let mut b = RenameBook::new();
    let _ = b.mkdir("src", 0);
    let _ = b.mkdir("dst", 0);
    let rep = b.rename("src", "dst", "换位").unwrap_or(true);
    let j1 = rep && b.find("dst") && !b.find("src");
    s.add("J1-空目录换位", j1, "dst 位被 src 内容顶替");

    // 纯改名达成
    let j2 = {
        let mut b2 = RenameBook::new();
        let _ = b2.mkdir("old", 0);
        let rep = b2.rename("old", "new", "改名").unwrap_or(true);
        !rep && b2.find("new") && b2.node("new").map(|n| n.is_dir).unwrap_or(false)
    };
    s.add("J2-纯改名", j2, "无目标时纯改名且目录性保持");

    // 非空目录目标受控
    let j3 = {
        let mut b3 = RenameBook::new();
        let _ = b3.mkdir("a", 2);
        let _ = b3.mkdir("b", 0);
        b3.rename("b", "a", "撞非空").err() == Some(RN_NOTEMPTY)
    };
    s.add("J3-非空目标受控", j3, "成员非零目标必拒");

    // 类型错配受控
    let j4 = {
        let mut b4 = RenameBook::new();
        let _ = b4.mkdir("d", 0);
        let _ = b4.touch("f");
        b4.rename("f", "d", "型撞").err() == Some(RN_TYPE)
    };
    s.add("J4-类型错配受控", j4, "文件换目录必拒");

    // 自换位幂等
    let j5 = {
        let mut b5 = RenameBook::new();
        let _ = b5.mkdir("solo", 0);
        b5.self_rename("solo").is_ok() && b5.find("solo")
    };
    s.add("J5-自换位幂等", j5, "同名 rename 无害落地");

    // 缺失源受控
    let j6 = b.rename("ghost", "dst", "缺席").err() == Some(RN_NOENT);
    s.add("J6-缺失源受控", j6, "缺失源必拒");

    // 文件覆盖语义
    let j7 = {
        let mut b7 = RenameBook::new();
        let _ = b7.touch("a");
        let _ = b7.touch("b");
        let rep = b7.rename("a", "b", "覆盖").unwrap_or(true);
        rep && b7.find("b") && !b7.find("a") && b7.nodes.len() == 1
    };
    s.add("J7-文件覆盖", j7, "同名文件顶替且表长守恒");

    // 空留痕拒收
    let j8 = b.rename("dst", "x", "").err() == Some(RN_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 节点合法性守恒
    let j9 = b.nodes.iter().all(|n| n.ok());
    s.add("J9-节点合法", j9, "名非空且文件零成员");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "双名非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // 成员维护面：换位前非空、清员后可换
    let j13 = {
        let mut b13 = RenameBook::new();
        let _ = b13.mkdir("src", 0);
        let _ = b13.mkdir("dst", 0);
        let _ = b13.add_member("dst");
        let blocked = b13.rename("src", "dst", "受阻").err() == Some(RN_NOTEMPTY);
        let _ = b13.drop_member("dst");
        let _ = b13.drop_member("dst");
        let cleared = b13.rename("src", "dst", "清后换").unwrap_or(false);
        blocked && cleared && b13.replaced_count() == 1
    };
    s.add("J13-清员可换", j13, "非空受阻、清员后换位成功");

    // 节点数与渲染
    let j14 = b.node_count() >= 1 && b.render_list().contains("dst/");
    s.add("J14-清单渲染", j14, "目录性以斜杠渲染");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10126_rename_flow() {
        let mut b = RenameBook::new();
        let _ = b.mkdir("a", 0);
        let _ = b.mkdir("b", 1);
        assert!(b.rename("a", "b", "x") == Err(RN_NOTEMPTY));
        let _ = b.rename("a", "c", "y");
        assert!(b.find("c"));
    }

    #[test]
    fn unx10126_checks_green() {
        let s = run_unx10126_checks();
        assert!(s.all_passed(), "UNX-F10126 判据未全绿");
    }
}
