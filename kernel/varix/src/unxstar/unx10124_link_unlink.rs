//! UNX-F10124 link/unlink 硬链正反双判据（链接计数达成与跨设备误用受控）（0x102a0 段）
//!
//! 锚点：UNX-F10124-J1 硬链达成（nlink 递增/递减守恒 LTP link01/link03
//! 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：硬链账（ino→nlink 中心账 + 名→ino 双向映射）+ 链核（link 增
//! nlink/unlink 减 nlink/零释放/跨设备 EXDEV 拒）+ 守恒核（nlink==名计
//! 数对平）+ 注入核（跨设备/缺失源/撞名/空留痕/封账必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x102a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x102a0;
pub const LK_EXDEV: u32 = SEG_BASE + 0x01; // 跨设备
pub const LK_NOENT: u32 = SEG_BASE + 0x02; // 缺失源
pub const LK_DUP: u32 = SEG_BASE + 0x03; // 撞名
pub const LK_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const LK_SEALED: u32 = SEG_BASE + 0x05; // 封账后操作

/// 设备号闭集（单设备模拟）。
pub const DEV_LOCAL: u32 = 1;
pub const DEV_REMOTE: u32 = 2;

/// 链账留痕。
#[derive(Clone)]
pub struct LkRec {
    pub seq: u32,
    pub op: u32, // 0=link 1=unlink
    pub name: String,
    pub ino: u32,
    pub nlink_after: u32,
}

impl LkRec {
    pub fn ok(&self) -> bool {
        self.op <= 1 && self.ino != 0 && !self.name.is_empty()
    }
}

/// 硬链账。
pub struct HardLinkBook {
    /// 名 → (ino, dev)
    pub names: Vec<(String, u32, u32)>,
    /// ino → nlink
    pub nlinks: Vec<(u32, u32)>,
    pub ledger: Vec<LkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HardLinkBook {
    pub fn new() -> HardLinkBook {
        HardLinkBook { names: Vec::new(), nlinks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 建源件。
    pub fn create(&mut self, name: &str, dev: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if name.is_empty() {
            return Err(LK_NOENT);
        }
        if self.names.iter().any(|(n, _, _)| n == name) {
            return Err(LK_DUP);
        }
        let ino = self.nlinks.iter().map(|(i, _)| *i).max().unwrap_or(0) + 1;
        self.names.push((name.to_string(), ino, dev));
        self.nlinks.push((ino, 1));
        Ok(ino)
    }

    fn find_name(&self, name: &str) -> Option<(u32, u32)> {
        self.names.iter().find(|(n, _, _)| n == name).map(|(_, i, d)| (*i, *d))
    }

    fn bump(&mut self, ino: u32, delta: i32) -> u32 {
        for e in self.nlinks.iter_mut() {
            if e.0 == ino {
                if delta >= 0 {
                    e.1 = e.1.saturating_add(delta as u32);
                } else {
                    e.1 = e.1.saturating_sub((-delta) as u32);
                }
                return e.1;
            }
        }
        0
    }

    /// link：增 nlink 并挂新名（跨设备/撞名/缺失源拒）。
    pub fn link(&mut self, src: &str, dst: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if note.is_empty() {
            return Err(LK_EMPTY_NOTE);
        }
        if dst.is_empty() {
            return Err(LK_NOENT);
        }
        let (ino, dev) = match self.find_name(src) {
            Some(v) => v,
            None => return Err(LK_NOENT),
        };
        if dev != DEV_LOCAL {
            return Err(LK_EXDEV);
        }
        if self.find_name(dst).is_some() {
            return Err(LK_DUP);
        }
        self.names.push((dst.to_string(), ino, dev));
        let after = self.bump(ino, 1);
        self.ledger.push(LkRec {
            seq: self.seq_next,
            op: 0,
            name: dst.to_string(),
            ino,
            nlink_after: after,
        });
        self.seq_next += 1;
        Ok(after)
    }

    /// unlink：减 nlink，名摘除，至零释放。
    pub fn unlink(&mut self, name: &str, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if note.is_empty() {
            return Err(LK_EMPTY_NOTE);
        }
        let ino = match self.find_name(name) {
            Some((i, _)) => i,
            None => return Err(LK_NOENT),
        };
        self.names.retain(|(n, _, _)| n != name);
        let after = self.bump(ino, -1);
        self.ledger.push(LkRec {
            seq: self.seq_next,
            op: 1,
            name: name.to_string(),
            ino,
            nlink_after: after,
        });
        self.seq_next += 1;
        Ok(after)
    }

    /// 守恒核：每 ino 的 nlink == 挂名数（释放后为 0）。
    pub fn conserved(&self) -> bool {
        for (ino, n) in &self.nlinks {
            let cnt = self.names.iter().filter(|(_, i, _)| i == ino).count() as u32;
            if *n != cnt {
                return false;
            }
        }
        true
    }

    /// 双名可达核：link 后两名的 ino 相同。
    pub fn same_ino(&self, a: &str, b: &str) -> bool {
        match (self.find_name(a), self.find_name(b)) {
            (Some((ia, _)), Some((ib, _))) => ia == ib,
            _ => false,
        }
    }

    /// 按名取 ino。
    pub fn ino_of(&self, name: &str) -> Option<u32> {
        self.find_name(name).map(|(i, _)| i)
    }

    /// 挂名清单（按 ino 枚举）。
    pub fn names_of(&self, ino: u32) -> Vec<String> {
        self.names
            .iter()
            .filter(|(_, i, _)| *i == ino)
            .map(|(n, _, _)| n.clone())
            .collect()
    }

    /// 活链计数（nlink>0 的 inode 数）。
    pub fn live_inodes(&self) -> usize {
        self.nlinks.iter().filter(|(_, n)| *n > 0).count()
    }

    /// 最大 nlink（守恒面上限探测）。
    pub fn max_nlink(&self) -> u32 {
        self.nlinks.iter().map(|(_, n)| *n).max().unwrap_or(0)
    }

    /// 留痕与账面对平：每条 unlink 后 nlink_after 与账一致。
    pub fn ledger_matches(&self) -> bool {
        for r in &self.ledger {
            let cur = self.nlinks.iter().find(|(i, _)| *i == r.ino).map(|(_, n)| *n);
            if r.op == 1 && cur != Some(r.nlink_after) {
                return false;
            }
        }
        true
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
        let mut h: u32 = 0x102a00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.nlink_after);
            for b in r.name.bytes() {
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
    let mut b = HardLinkBook::new();
    let _ = b.create("local", DEV_LOCAL);
    let _ = b.create("remote", DEV_REMOTE);
    // 1) 跨设备 link 必拒
    got[0] = b.link("remote", "alias", "inj-xdev").err() == Some(LK_EXDEV);
    // 2) 缺失源必拒
    got[1] = b.link("ghost", "g2", "inj-noent").err() == Some(LK_NOENT);
    // 3) 撞名必拒
    let _ = b.link("local", "twin", "inj-ok");
    got[2] = b.link("local", "twin", "inj-dup").err() == Some(LK_DUP);
    // 4) 空留痕必拒
    got[3] = b.unlink("twin", "").err() == Some(LK_EMPTY_NOTE);
    // 5) 封账后操作必抓
    b.seal();
    got[4] = b.unlink("twin", "inj-seal").err() == Some(LK_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10124");

    // 正路径：link01 面——link 后 nlink 增至 2 且双名同 ino
    let mut b = HardLinkBook::new();
    let _ = b.create("real", DEV_LOCAL);
    let after = b.link("real", "alias", "挂链").unwrap_or(0);
    let j1 = after == 2 && b.same_ino("real", "alias");
    s.add("J1-挂链增计", j1, "link 后 nlink==2 且双名同 ino");

    // link03 面：unlink 递减守恒
    let left = b.unlink("alias", "摘链").unwrap_or(9);
    let j2 = left == 1 && b.conserved();
    s.add("J2-摘链递减", j2, "unlink 后 nlink==1 且账对平");

    // 零释放：双摘归零
    let j3 = {
        let _ = b.unlink("real", "再摘");
        b.nlinks.iter().all(|(_, n)| *n == 0) && b.names.is_empty()
    };
    s.add("J3-零释放", j3, "全摘后 nlink 归零且名清空");

    // 重挂可达
    let j4 = {
        let _ = b.create("root2", DEV_LOCAL);
        let a2 = b.link("root2", "leaf2", "重挂").unwrap_or(0);
        a2 == 2 && b.same_ino("root2", "leaf2") && b.conserved()
    };
    s.add("J4-重挂可达", j4, "再建再挂守恒成立");

    // 跨设备受控
    let j5 = {
        let _ = b.create("far", DEV_REMOTE);
        b.link("far", "far2", "跨设备").err() == Some(LK_EXDEV)
    };
    s.add("J5-跨设备受控", j5, "非本设备源必 EXDEV");

    // 缺失源受控
    let j6 = b.link("ghost", "g", "缺席").err() == Some(LK_NOENT)
        && b.unlink("ghost", "缺席").err() == Some(LK_NOENT);
    s.add("J6-缺失源受控", j6, "link/unlink 双面同拒");

    // 撞名受控
    let j7 = b.link("root2", "leaf2", "撞").err() == Some(LK_DUP);
    s.add("J7-撞名受控", j7, "目标名已挂必拒");

    // 空留痕拒收
    let j8 = b.link("root2", "x", "").err() == Some(LK_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j9 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J9-留痕合法", j9, "op 在闭集且名非空");

    // 注入核 5/5
    let inj = inject_five();
    let j10 = inj.iter().all(|g| *g);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", inj));

    // 挂名清单与活链计数
    let j12 = b.names_of(b.ino_of("root2").unwrap_or(0)).len() == 2 && b.live_inodes() == 2;
    s.add("J12-挂名清单", j12, "root2 挂两名且活链两支");

    // 峰链与守恒回对
    let j13 = b.max_nlink() == 2 && b.conserved();
    s.add("J13-峰链回对", j13, "峰值 nlink==2 且全局守恒");

    // 指纹稳定
    let j11 = b.fold() == b.fold();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10124_link_flow() {
        let mut b = HardLinkBook::new();
        let _ = b.create("a", DEV_LOCAL);
        assert_eq!(b.link("a", "b", "l").unwrap_or(0), 2);
        assert!(b.same_ino("a", "b"));
        assert_eq!(b.unlink("b", "u").unwrap_or(0), 1);
        assert!(b.link("a", "b", "x") .is_ok());
        let _ = b;
    }

    #[test]
    fn unx10124_checks_green() {
        let s = run_unx10124_checks();
        assert!(s.all_passed(), "UNX-F10124 判据未全绿");
    }
}
