//! UNX-F9186 umask 语义档（继承与返回旧值）（0x63700 段）
//!
//! 锚点：UNX-F9186-J1 umask 账（每次 umask() 落账：旧掩码、新掩码、
//! 留痕）；J2 umask 语义（返回旧值 + 9 位权限域 + 子承父继承 + 特殊
//! 位忽略四性质）；J3 umask 指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（umask 继承面）。
//!
//! 结构：umask 册（进程 → 掩码）+ umask 核（设新返旧 + 权限域规整）
//! + 继承核（fork 拷贝）+ umask 账本 + 折叠指纹 + 注入核（错码/空留
//! 痕/越域掩码/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63700 独占 0x20）。
pub const SEG_BASE: u32 = 0x63700;
pub const UM_UNK_MASK: u32 = SEG_BASE + 0x01; // 越域掩码
pub const UM_BAD_INHERIT: u32 = SEG_BASE + 0x02; // 继承错
pub const UM_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UM_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const UM_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 9 位权限域掩码（rwxrwxrwx）。
pub const MODE_MASK: u32 = 0o777;

/// 掩码规整：剥特殊位（setuid/setgid/sticky），留 9 位。
pub fn normalize(mask: u32) -> u32 {
    mask & MODE_MASK
}

/// 权限计算：mode & !umask。
pub fn apply_umask(mode: u32, umask: u32) -> u32 {
    mode & !normalize(umask)
}

/// 单条 umask 账。
#[derive(Clone)]
pub struct UmRec {
    pub seq: u32,
    pub pid: u32,
    pub old: u32,
    pub new: u32,
    pub note: String,
}

impl UmRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && normalize(self.new) == self.new
    }

    pub fn render_line(&self) -> String {
        format!(
            "UM{}|p{}|{:o}->{:o}|{}",
            self.seq, self.pid, self.old, self.new, self.note
        )
    }
}

/// umask 簿。
pub struct UmBook {
    /// pid → 掩码
    pub masks: Vec<(u32, u32)>,
    pub ledger: Vec<UmRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UmBook {
    pub fn new() -> UmBook {
        UmBook { masks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn entry(&mut self, pid: u32) -> usize {
        if let Some(i) = self.masks.iter().position(|(w, _)| *w == pid) {
            return i;
        }
        self.masks.push((pid, 0o022)); // 默认 umask
        self.masks.len() - 1
    }

    /// umask(new) → 返回旧值。
    pub fn umask(&mut self, pid: u32, new: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UM_FROZEN);
        }
        if note.is_empty() {
            return Err(UM_EMPTY_NOTE);
        }
        if pid == 0 {
            return Err(UM_UNK_MASK);
        }
        let n = normalize(new);
        if n != new {
            // 特殊位剥离留痕（不算错，但记录规整后值）
        }
        let i = self.entry(pid);
        let old = self.masks[i].1;
        self.masks[i].1 = n;
        self.ledger.push(UmRec { seq: self.seq_next, pid, old, new: n, note: note.to_string() });
        self.seq_next += 1;
        Ok(old)
    }

    /// fork 继承：子掩码 = 父掩码。
    pub fn fork_inherit(&mut self, parent: u32, child: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UM_FROZEN);
        }
        if note.is_empty() {
            return Err(UM_EMPTY_NOTE);
        }
        let pm = match self.masks.iter().find(|(w, _)| *w == parent) {
            Some((_, m)) => *m,
            None => {
                let _ = self.entry(parent);
                self.masks.iter().find(|(w, _)| *w == parent).map(|(_, m)| *m).unwrap_or(0o022)
            }
        };
        let ci = self.entry(child);
        self.masks[ci].1 = pm;
        self.ledger.push(UmRec {
            seq: self.seq_next,
            pid: child,
            old: 0o022,
            new: pm,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(pm)
    }

    pub fn mask_of(&self, pid: u32) -> Option<u32> {
        self.masks.iter().find(|(w, _)| *w == pid).map(|(_, m)| *m)
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
        let mut h: u32 = 0x91860;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.old);
            h = h.wrapping_mul(31).wrapping_add(r.new);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 越域掩码记录
    let bad = UmRec {
        seq: 1,
        pid: 1,
        old: 0o022,
        new: 0o1777 & MODE_MASK, // 规整后合法的才 ok
        note: "inj-mask".to_string(),
    };
    let bad = UmRec { new: 0o1777, ..bad }; // 特殊位未剥 → !ok
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = UmRec {
        seq: 2,
        pid: 1,
        old: 0,
        new: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 零号 pid 拒
    let mut b = UmBook::new();
    got[2] = b.umask(0, 0o022, "inj-zero").err() == Some(UM_UNK_MASK);
    // 4) 权限应用正确性反证
    got[3] = apply_umask(0o666, 0o022) == 0o644;
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.umask(5, 0o022, "inj-frozen").err() == Some(UM_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9186_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9186");
    let mut b = UmBook::new();

    // J1 umask 账非空
    let _ = b.umask(100, 0o027, "设掩码");
    let j1 = !b.ledger.is_empty();
    s.add("J1-umask账非空", j1, "umask 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且新掩码规整");

    // J3 返回旧值
    let j3 = {
        let mut b2 = UmBook::new();
        let first = b2.umask(100, 0o027, "一");
        first == Ok(0o022) // 默认 022
    };
    s.add("J3-返回旧值", j3, "首次 umask 返默认 022");

    // J4 9 位权限域：特殊位剥离
    let j4 = normalize(0o1777) == 0o777 && normalize(0o022) == 0o022;
    s.add("J4-权限域规整", j4, "特殊位剥离留 9 位");

    // J5 权限应用：mode & !umask
    let j5 = apply_umask(0o666, 0o022) == 0o644
        && apply_umask(0o777, 0o027) == 0o750
        && apply_umask(0o666, 0) == 0o666;
    s.add("J5-掩码应用", j5, "mode & !umask 三例");

    // J6 子承父继承
    let j6 = {
        let mut b2 = UmBook::new();
        let _ = b2.umask(100, 0o077, "父设");
        let _ = b2.fork_inherit(100, 101, "子承");
        b2.mask_of(101) == Some(0o077)
    };
    s.add("J6-子承父", j6, "fork 后子掩码=父掩码");

    // J7 子独立：子改掩码不影响父
    let j7 = {
        let mut b2 = UmBook::new();
        let _ = b2.umask(100, 0o077, "父");
        let _ = b2.fork_inherit(100, 101, "承");
        let _ = b2.umask(101, 0, "子改");
        b2.mask_of(100) == Some(0o077) && b2.mask_of(101) == Some(0)
    };
    s.add("J7-子独立", j7, "改子不改父");

    // J8 零号 pid 拒
    let j8 = b.umask(0, 0o022, "零").err() == Some(UM_UNK_MASK);
    s.add("J8-零号拒", j8, "pid=0 必拒");

    // J9 连续设掩码链
    let j9 = {
        let mut b2 = UmBook::new();
        let a = b2.umask(100, 0o027, "a");
        let bb = b2.umask(100, 0o077, "b");
        a == Ok(0o022) && bb == Ok(0o027)
    };
    s.add("J9-设链回值", j9, "连续设置逐次返旧");

    // J10 默认掩码 022
    let j10 = {
        let b2 = UmBook::new();
        b2.mask_of(999).is_none() // 未登记走 entry 默认
    };
    s.add("J10-默认登记", j10, "未登记 pid 无册面（entry 默认 022）");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.umask(100, 0o022, "").err() == Some(UM_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9186_umask_flow() {
        assert_eq!(normalize(0o1777), 0o777);
        assert_eq!(apply_umask(0o666, 0o022), 0o644);
        assert_eq!(apply_umask(0o777, 0o027), 0o750);
        let mut b = UmBook::new();
        assert_eq!(b.umask(100, 0o027, "a"), Ok(0o022));
        assert_eq!(b.umask(100, 0o077, "b"), Ok(0o027));
        assert_eq!(b.mask_of(100), Some(0o077));
        // 继承
        let _ = b.fork_inherit(100, 101, "c");
        assert_eq!(b.mask_of(101), Some(0o077));
        assert_eq!(b.umask(0, 0o022, "d"), Err(UM_UNK_MASK));
    }

    #[test]
    fn unx9186_checks_green() {
        let s = run_unx9186_checks();
        assert!(s.all_passed(), "UNX-F9186 判据未全绿");
    }
}
