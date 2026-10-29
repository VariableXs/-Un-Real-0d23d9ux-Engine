//! UNX-F9185 getgroups/setgroups 语义档（补充组集）（0x636E0 段）
//!
//! 锚点：UNX-F9185-J1 组集账（get/set 每次落账：动作、组集快照、
//! 结果、留痕）；J2 组集语义（setgroups 特权专属 + 组集去重 + 容量
//! 闸 + get 闭路四性质）；J3 组集指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（补充组集面）。
//!
//! 结构：组集册（gset）+ 组集核（get/set 双门 + 去重）+ 组集账本 +
//! 折叠指纹 + 注入核（错码/空留痕/越权 set/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x636E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x636E0;
pub const GG_UNK_MOVE: u32 = SEG_BASE + 0x01; // 越权
pub const GG_OVERFLOW: u32 = SEG_BASE + 0x02; // 容量溢出
pub const GG_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const GG_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const GG_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 组容量上限（样本小上限）。
pub const NGROUPS_MAX: usize = 32;

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1;
pub const R_EINVAL: u32 = 2;
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 单条组集账。
#[derive(Clone)]
pub struct GgRec {
    pub seq: u32,
    pub op: usize, // 0=get 1=set
    pub size: usize,
    pub code: u32,
    pub note: String,
}

impl GgRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < 2 && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "GG{}|{}|n{}|{}|{}",
            self.seq,
            if self.op == 0 { "get" } else { "set" },
            self.size,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 组集簿。
pub struct GgBook {
    pub gset: Vec<u32>,
    pub privileged: bool,
    pub ledger: Vec<GgRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl GgBook {
    pub fn new(privileged: bool) -> GgBook {
        GgBook {
            gset: vec![100],
            privileged,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// getgroups：快照（容量闸：size < 组数 → EINVAL）。
    pub fn getgroups(&mut self, size: usize, note: &str) -> Result<Vec<u32>, u32> {
        if self.sealed {
            return Err(GG_FROZEN);
        }
        if note.is_empty() {
            return Err(GG_EMPTY_NOTE);
        }
        if size < self.gset.len() {
            self.record(0, self.gset.len(), R_EINVAL, note);
            return Err(R_EINVAL);
        }
        self.record(0, self.gset.len(), R_OK, note);
        Ok(self.gset.clone())
    }

    /// setgroups：特权专属 + 去重 + 容量闸。
    pub fn setgroups(&mut self, list: &[u32], note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(GG_FROZEN);
        }
        if note.is_empty() {
            return Err(GG_EMPTY_NOTE);
        }
        if !self.privileged {
            self.record(1, self.gset.len(), R_EPERM, note);
            return Err(R_EPERM);
        }
        // 去重
        let mut dedup: Vec<u32> = Vec::new();
        for g in list {
            if !dedup.contains(g) {
                dedup.push(*g);
            }
        }
        if dedup.len() > NGROUPS_MAX {
            self.record(1, dedup.len(), R_EINVAL, note);
            return Err(R_EINVAL);
        }
        self.gset = dedup.clone();
        self.record(1, self.gset.len(), R_OK, note);
        Ok(self.gset.len())
    }

    /// 组闭路：get 返回与册面一致。
    pub fn closed(&mut self, note: &str) -> bool {
        match self.getgroups(NGROUPS_MAX, note) {
            Ok(v) => v == self.gset,
            Err(_) => false,
        }
    }

    fn record(&mut self, op: usize, size: usize, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(GgRec { seq, op, size, code, note: note.to_string() });
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
        let mut h: u32 = 0x91850;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.size as u32);
            h = h.wrapping_mul(31).wrapping_add(r.code);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        for g in &self.gset {
            h = h.wrapping_mul(31).wrapping_add(*g);
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
    // 1) 错码账
    let bad = GgRec { seq: 1, op: 0, size: 1, code: 9, note: "inj-code".to_string() };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = GgRec { seq: 2, op: 0, size: 0, code: 0, note: String::new() };
    got[1] = !bad2.ok();
    // 3) 越权 set
    let mut b = GgBook::new(false);
    got[2] = b.setgroups(&[7], "inj-eperm").err() == Some(R_EPERM);
    // 4) 容量溢出
    let mut p = GgBook::new(true);
    let big: Vec<u32> = (0..(NGROUPS_MAX + 1) as u32).map(|i| 500 + i).collect();
    got[3] = p.setgroups(&big, "inj-overflow").err() == Some(R_EINVAL);
    // 5) 印后登记必抓
    p.seal();
    got[4] = p.setgroups(&[7], "inj-frozen").err() == Some(GG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9185_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9185");
    let mut b = GgBook::new(true);

    // J1 组集账非空
    let _ = b.getgroups(NGROUPS_MAX, "get 快照");
    let j1 = !b.ledger.is_empty();
    s.add("J1-组集账非空", j1, "get/set 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/码闭集");

    // J3 setgroups 特权专属
    let j3 = {
        let mut u = GgBook::new(false);
        u.setgroups(&[7, 8], "越权").err() == Some(R_EPERM)
    };
    s.add("J3-set特权专属", j3, "非特权 setgroups 必 EPERM");

    // J4 特权 set 放行
    let j4 = {
        let mut p = GgBook::new(true);
        p.setgroups(&[7, 8, 9], "设") == Ok(3) && p.gset == vec![7, 8, 9]
    };
    s.add("J4-set放行", j4, "特权设置生效");

    // J5 去重
    let j5 = {
        let mut p = GgBook::new(true);
        p.setgroups(&[7, 7, 8], "重") == Ok(2) && p.gset == vec![7, 8]
    };
    s.add("J5-去重", j5, "重复 gid 单次登记");

    // J6 容量闸
    let j6 = {
        let mut p = GgBook::new(true);
        let big: Vec<u32> = (0..(NGROUPS_MAX + 1) as u32).map(|i| 500 + i).collect();
        p.setgroups(&big, "溢出").err() == Some(R_EINVAL)
    };
    s.add("J6-容量闸", j6, "超 NGROUPS_MAX 必 EINVAL");

    // J7 get 闭路
    let j7 = {
        let mut p = GgBook::new(true);
        let _ = p.setgroups(&[7, 8], "设");
        p.closed("闭路")
    };
    s.add("J7-get闭路", j7, "get 与册面一致");

    // J8 get 容量不足拒
    let j8 = {
        let mut p = GgBook::new(true);
        let _ = p.setgroups(&[7, 8, 9], "设3");
        p.getgroups(2, "小buf").err() == Some(R_EINVAL)
    };
    s.add("J8-get容量拒", j8, "size<组数必 EINVAL");

    // J9 get 零容量探测
    let j9 = {
        let mut p = GgBook::new(true);
        p.getgroups(0, "探测").err() == Some(R_EINVAL)
    };
    s.add("J9-零容量拒", j9, "size=0 且非空组必 EINVAL");

    // J10 空组集合法
    let j10 = {
        let mut p = GgBook::new(true);
        p.setgroups(&[], "清空") == Ok(0) && p.gset.is_empty()
    };
    s.add("J10-空集合法", j10, "空组集可设");

    // J11 非特权 get 放行
    let j11 = {
        let mut u = GgBook::new(false);
        u.getgroups(NGROUPS_MAX, "查").is_ok()
    };
    s.add("J11-get无特权", j11, "getgroups 无特权限制");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx9185_groups_flow() {
        // 特权
        let mut p = GgBook::new(true);
        assert_eq!(p.setgroups(&[7, 7, 8], "a"), Ok(2));
        assert_eq!(p.gset, vec![7, 8]);
        let got = p.getgroups(NGROUPS_MAX, "b").unwrap();
        assert_eq!(got, vec![7, 8]);
        assert_eq!(p.getgroups(1, "c"), Err(R_EINVAL));
        // 非特权
        let mut u = GgBook::new(false);
        assert_eq!(u.setgroups(&[7], "d"), Err(R_EPERM));
        assert!(u.getgroups(NGROUPS_MAX, "e").is_ok());
    }

    #[test]
    fn unx9185_checks_green() {
        let s = run_unx9185_checks();
        assert!(s.all_passed(), "UNX-F9185 判据未全绿");
    }
}
