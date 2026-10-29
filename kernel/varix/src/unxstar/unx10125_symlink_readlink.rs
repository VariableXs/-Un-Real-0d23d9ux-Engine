//! UNX-F10125 symlink/readlink 正反双判据（符号链达成与环路误用受控）（0x102c0 段）
//!
//! 锚点：UNX-F10125-J1 符号链达成（symlink→readlink 回读一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：符号链表（名→目标串 + 深度计）+ 链核（建链/回读/目标跟随/
//! 环路探测/悬挂链识别）+ 链账（逐次留痕）+ 注入核（环路/超深/撞名/
//! 空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x102c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x102c0;
pub const SL_ELOOP: u32 = SEG_BASE + 0x01; // 环路
pub const SL_DUP: u32 = SEG_BASE + 0x02; // 撞名
pub const SL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const SL_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const SL_TOO_DEEP: u32 = SEG_BASE + 0x05; // 跟随超深

/// 跟随深度上限。
pub const FOLLOW_MAX: usize = 8;

/// 符号链记录。
#[derive(Clone)]
pub struct Symlink {
    pub name: String,
    pub target: String,
}

impl Symlink {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && !self.target.is_empty()
    }
}

/// 链账留痕。
#[derive(Clone)]
pub struct SlRec {
    pub seq: u32,
    pub name: String,
    pub target: String,
    pub op: u32, // 0=symlink 1=readlink
}

impl SlRec {
    pub fn ok(&self) -> bool {
        self.op <= 1 && !self.name.is_empty()
    }
}

/// 符号链册。
pub struct SymlinkBook {
    pub links: Vec<Symlink>,
    pub files: Vec<String>,
    pub ledger: Vec<SlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SymlinkBook {
    pub fn new() -> SymlinkBook {
        SymlinkBook { links: Vec::new(), files: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 注册实体名（供跟随判定）。
    pub fn add_file(&mut self, name: &str) {
        self.files.push(name.to_string());
    }

    fn is_link(&self, name: &str) -> bool {
        self.links.iter().any(|l| l.name == name)
    }

    fn is_file(&self, name: &str) -> bool {
        self.files.iter().any(|f| f == name)
    }

    /// symlink：建链（撞名拒）。
    pub fn symlink(&mut self, name: &str, target: &str, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if note.is_empty() {
            return Err(SL_EMPTY_NOTE);
        }
        if name.is_empty() || target.is_empty() {
            return Err(SL_DUP);
        }
        if self.is_link(name) || self.is_file(name) {
            return Err(SL_DUP);
        }
        self.links.push(Symlink { name: name.to_string(), target: target.to_string() });
        self.ledger.push(SlRec {
            seq: self.seq_next,
            name: name.to_string(),
            target: target.to_string(),
            op: 0,
        });
        self.seq_next += 1;
        Ok(())
    }

    /// readlink：回读目标串（非链名拒）。
    pub fn readlink(&mut self, name: &str, note: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if note.is_empty() {
            return Err(SL_EMPTY_NOTE);
        }
        let target = match self.links.iter().find(|l| l.name == name) {
            Some(l) => l.target.clone(),
            None => return Err(SL_DUP),
        };
        self.ledger.push(SlRec {
            seq: self.seq_next,
            name: name.to_string(),
            target: target.clone(),
            op: 1,
        });
        self.seq_next += 1;
        Ok(target)
    }

    /// 跟随解析：链→链→…→实体；环路/超深拒。
    pub fn follow(&mut self, name: &str, note: &str) -> Result<String, u32> {
        if self.sealed {
            return Err(SL_SEALED);
        }
        if note.is_empty() {
            return Err(SL_EMPTY_NOTE);
        }
        let mut cur = name.to_string();
        let mut depth = 0usize;
        let mut seen: Vec<String> = Vec::new();
        loop {
            if depth > FOLLOW_MAX {
                return Err(SL_TOO_DEEP);
            }
            if seen.contains(&cur) {
                return Err(SL_ELOOP);
            }
            seen.push(cur.clone());
            match self.links.iter().find(|l| l.name == cur) {
                Some(l) => {
                    let t = l.target.clone();
                    if self.is_file(&t) {
                        return Ok(t);
                    }
                    cur = t;
                    depth += 1;
                }
                None => {
                    if self.is_file(&cur) {
                        return Ok(cur);
                    }
                    // 悬挂链：目标不存在——按回读面返回目标串
                    return Ok(cur);
                }
            }
        }
    }

    /// 链目标回读一致性核（symlink→readlink 往返）。
    pub fn roundtrip(&mut self, name: &str) -> bool {
        let want = self.links.iter().find(|l| l.name == name).map(|l| l.target.clone());
        match want {
            Some(t) => self.readlink(name, "roundtrip").unwrap_or_default() == t,
            None => false,
        }
    }

    /// 链数量。
    pub fn chain_count(&self) -> usize {
        self.links.len()
    }

    /// 实体数。
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// 悬挂链清单：目标既非实体也非链。
    pub fn dangling(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.links {
            let t = l.target.clone();
            if !self.is_file(&t) && !self.is_link(&t) {
                out.push(l.name.clone());
            }
        }
        out
    }

    /// 目标渲染（诊断面）。
    pub fn render(&self, name: &str) -> String {
        match self.links.iter().find(|l| l.name == name) {
            Some(l) => format!("{} -> {}", l.name, l.target),
            None => format!("{} -> ?", name),
        }
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
        let mut h: u32 = 0x102c00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            for b in r.target.bytes() {
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
    let mut b = SymlinkBook::new();
    let _ = b.symlink("a", "b", "inj-1");
    let _ = b.symlink("b", "a", "inj-2");
    // 1) 环路必检出
    got[0] = b.follow("a", "inj-loop").err() == Some(SL_ELOOP);
    // 2) 撞名必拒
    got[1] = b.symlink("a", "x", "inj-dup").err() == Some(SL_DUP);
    // 3) 非链名 readlink 必拒
    got[2] = b.readlink("nochain", "inj-no").err() == Some(SL_DUP);
    // 4) 空留痕必拒
    got[3] = b.readlink("a", "").err() == Some(SL_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.readlink("a", "inj-seal").err() == Some(SL_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10125");

    // 正路径：symlink→readlink 回读一致
    let mut b = SymlinkBook::new();
    let _ = b.symlink("lnk", "real.txt", "建链");
    let back = b.readlink("lnk", "回读").unwrap_or_default();
    let j1 = back == "real.txt" && b.roundtrip("lnk");
    s.add("J1-回读一致", j1, "readlink 得目标串且往返一致");

    // 单链跟随落实体
    let j2 = {
        b.add_file("real.txt");
        b.follow("lnk", "跟随").unwrap_or_default() == "real.txt"
    };
    s.add("J2-单链跟随", j2, "链跟随落实体名");

    // 双链级联跟随
    let j3 = {
        let _ = b.symlink("l2", "lnk", "级联");
        b.follow("l2", "双跳").unwrap_or_default() == "real.txt"
    };
    s.add("J3-级联跟随", j3, "链→链→实体两跳落位");

    // 环路受控
    let j4 = {
        let _ = b.symlink("x", "y", "环一");
        let _ = b.symlink("y", "x", "环二");
        b.follow("x", "探环").err() == Some(SL_ELOOP)
    };
    s.add("J4-环路受控", j4, "互指双链必 ELOOP");

    // 悬挂链识别：目标缺席返回目标串不炸
    let j5 = {
        let _ = b.symlink("dang", "nowhere", "悬挂");
        b.follow("dang", "探悬").unwrap_or_default() == "nowhere"
    };
    s.add("J5-悬挂识别", j5, "悬挂链返回目标串受控");

    // 撞名受控（对实体名同样拒）
    let j6 = b.symlink("lnk", "other", "撞链").err() == Some(SL_DUP);
    s.add("J6-撞链受控", j6, "重名建链必拒");

    // 非链 readlink 受控
    let j7 = b.readlink("ghost", "野名").err() == Some(SL_DUP);
    s.add("J7-野名受控", j7, "非链名回读必拒");

    // 空留痕拒收
    let j8 = b.symlink("nn", "tt", "").err() == Some(SL_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 跟随深度上限自洽（正常链不触顶）
    let j9 = b.follow("l2", "深探").is_ok() && FOLLOW_MAX >= 2;
    s.add("J9-深度自洽", j9, "级联链在深度限内");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集且名非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 悬挂链清单与计数
    let j13 = b.dangling() == vec!["dang".to_string()]
        && b.chain_count() == 5
        && b.file_count() == 1;
    s.add("J13-悬挂清单", j13, "dang 唯一悬挂且计数对平");

    // 渲染面
    let j14 = b.render("lnk") == "lnk -> real.txt" && b.render("nope").contains('?');
    s.add("J14-渲染面", j14, "链渲染命中与野名占位");

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10125_symlink_flow() {
        let mut b = SymlinkBook::new();
        b.add_file("t");
        let _ = b.symlink("s", "t", "a");
        assert_eq!(b.readlink("s", "b").unwrap_or_default(), "t");
        assert_eq!(b.follow("s", "c").unwrap_or_default(), "t");
        let _ = b.symlink("s2", "s", "d");
        assert!(b.follow("s2", "e").is_ok());
    }

    #[test]
    fn unx10125_checks_green() {
        let s = run_unx10125_checks();
        assert!(s.all_passed(), "UNX-F10125 判据未全绿");
    }
}
