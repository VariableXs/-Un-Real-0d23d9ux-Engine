//! UNX-F9183 setgid/setregid/setresgid 语义档（0x636A0 段）
//!
//! 锚点：UNX-F9183-J1 gid 翻转账（setgid/setregid/setresgid 三门逐
//! 条落账：rgid/egid/sgid 旧三元 → 新三元、结果、留痕）；J2 翻转语
//! 义（三门规则对齐 uid 档 + 特权任意 + 三值闸 + KEEP 保持四性质）；
//! J3 gid 翻转指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（gid 三门面）。
//!
//! 结构：gid 三元组册 + 三门翻转核 + 翻转账本 + 折叠指纹 + 注入核
//! （错码/空留痕/越权翻转/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x636A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x636A0;
pub const GI_UNK_MOVE: u32 = SEG_BASE + 0x01; // 越权翻转
pub const GI_BAD_ARG: u32 = SEG_BASE + 0x02; // 非法参数
pub const GI_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const GI_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const GI_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1;
pub const R_COUNT: usize = 2;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM"];

/// gid 三元组。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Gids {
    pub rgid: u32,
    pub egid: u32,
    pub sgid: u32,
}

impl Gids {
    pub fn is_priv(&self) -> bool {
        self.egid == 0
    }

    pub fn target_ok(&self, v: u32) -> bool {
        v == self.rgid || v == self.egid || v == self.sgid
    }
}

/// KEEP 哨兵。
pub const KEEP: u32 = u32::MAX;

/// 单条账。
#[derive(Clone)]
pub struct GiRec {
    pub seq: u32,
    pub op: usize, // 0=setgid 1=setregid 2=setresgid
    pub old: Gids,
    pub new: Gids,
    pub code: u32,
    pub note: String,
}

impl GiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < 3 && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "GI{}|op{}|{:?}->{:?}|{}|{}",
            self.seq,
            self.op,
            (self.old.rgid, self.old.egid, self.old.sgid),
            (self.new.rgid, self.new.egid, self.new.sgid),
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// gid 簿。
pub struct GiBook {
    pub gids: Gids,
    pub ledger: Vec<GiRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl GiBook {
    pub fn new(r: u32, e: u32, s: u32) -> GiBook {
        GiBook {
            gids: Gids { rgid: r, egid: e, sgid: s },
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// setgid：特权 → 全翻；非特权 → 仅 egid=gid（gid∈三值）。
    pub fn setgid(&mut self, gid: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GI_FROZEN);
        }
        if note.is_empty() {
            return Err(GI_EMPTY_NOTE);
        }
        let old = self.gids;
        if old.is_priv() {
            self.gids = Gids { rgid: gid, egid: gid, sgid: gid };
            self.record(0, old, self.gids, R_OK, note);
            return Ok(());
        }
        if !old.target_ok(gid) {
            self.record(0, old, old, R_EPERM, note);
            return Err(R_EPERM);
        }
        self.gids.egid = gid;
        self.record(0, old, self.gids, R_OK, note);
        Ok(())
    }

    /// setregid：KEEP 保持；特权任意；非特权三值闸。
    pub fn setregid(&mut self, r: u32, e: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GI_FROZEN);
        }
        if note.is_empty() {
            return Err(GI_EMPTY_NOTE);
        }
        let old = self.gids;
        if !old.is_priv() {
            let ok_r = r == KEEP || old.target_ok(r);
            let ok_e = e == KEEP || old.target_ok(e);
            if !ok_r || !ok_e {
                self.record(1, old, old, R_EPERM, note);
                return Err(R_EPERM);
            }
        }
        let nr = if r == KEEP { old.rgid } else { r };
        let ne = if e == KEEP { old.egid } else { e };
        self.gids = Gids { rgid: nr, egid: ne, sgid: old.sgid };
        self.record(1, old, self.gids, R_OK, note);
        Ok(())
    }

    /// setresgid：KEEP 保持；特权任意；非特权三值闸。
    pub fn setresgid(&mut self, r: u32, e: u32, s: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(GI_FROZEN);
        }
        if note.is_empty() {
            return Err(GI_EMPTY_NOTE);
        }
        let old = self.gids;
        if !old.is_priv() {
            let ok_r = r == KEEP || old.target_ok(r);
            let ok_e = e == KEEP || old.target_ok(e);
            let ok_s = s == KEEP || old.target_ok(s);
            if !ok_r || !ok_e || !ok_s {
                self.record(2, old, old, R_EPERM, note);
                return Err(R_EPERM);
            }
        }
        let nr = if r == KEEP { old.rgid } else { r };
        let ne = if e == KEEP { old.egid } else { e };
        let ns = if s == KEEP { old.sgid } else { s };
        self.gids = Gids { rgid: nr, egid: ne, sgid: ns };
        self.record(2, old, self.gids, R_OK, note);
        Ok(())
    }

    fn record(&mut self, op: usize, old: Gids, new: Gids, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(GiRec { seq, op, old, new, code, note: note.to_string() });
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
        let mut h: u32 = 0x91830;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.new.rgid);
            h = h.wrapping_mul(31).wrapping_add(r.new.egid);
            h = h.wrapping_mul(31).wrapping_add(r.new.sgid);
            h = h.wrapping_mul(31).wrapping_add(r.code);
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
    // 1) 错码账
    let bad = GiRec {
        seq: 1,
        op: 0,
        old: Gids { rgid: 0, egid: 0, sgid: 0 },
        new: Gids { rgid: 0, egid: 0, sgid: 0 },
        code: 9,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = GiRec {
        seq: 2,
        op: 0,
        old: Gids { rgid: 0, egid: 0, sgid: 0 },
        new: Gids { rgid: 0, egid: 0, sgid: 0 },
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 非特权 setgid 外来
    let mut b = GiBook::new(1000, 1000, 1000);
    got[2] = b.setgid(7, "inj-foreign").err() == Some(R_EPERM);
    // 4) 非特权 setresgid 外来
    got[3] = b.setresgid(7, KEEP, KEEP, "inj-res").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.setgid(1000, "inj-frozen").err() == Some(GI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9183");
    let mut b = GiBook::new(1000, 1000, 1000);

    // J1 翻转账非空（三门齐）
    let j1 = {
        let _ = b.setgid(1000, "setgid 门");
        let _ = b.setregid(KEEP, 1000, "setregid 门");
        let _ = b.setresgid(KEEP, KEEP, KEEP, "setresgid 门");
        b.ledger.iter().filter(|r| r.op == 0).count() >= 1
            && b.ledger.iter().filter(|r| r.op == 1).count() >= 1
            && b.ledger.iter().filter(|r| r.op == 2).count() >= 1
    };
    s.add("J1-三门账齐", j1, "setgid/setregid/setresgid 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/码闭集");

    // J3 特权 setgid 全翻
    let j3 = {
        let mut p = GiBook::new(0, 0, 0);
        let _ = p.setgid(1001, "降权");
        p.gids == Gids { rgid: 1001, egid: 1001, sgid: 1001 }
    };
    s.add("J3-特权全翻", j3, "root setgid 三值齐变");

    // J4 非特权三值闸
    let j4 = b.setgid(7, "外来").err() == Some(R_EPERM);
    s.add("J4-三值闸", j4, "非特权外来 gid 必 EPERM");

    // J5 setregid KEEP 保持
    let j5 = {
        let mut u = GiBook::new(1000, 1001, 1001);
        let _ = u.setregid(KEEP, 1000, "只改e");
        u.gids.rgid == 1000 && u.gids.egid == 1000
    };
    s.add("J5-KEEP保持", j5, "setregid(-1,x) rgid 不动");

    // J6 setresgid 三位任意（特权）
    let j6 = {
        let mut p = GiBook::new(0, 0, 0);
        p.setresgid(42, 43, 44, "任意").is_ok()
            && p.gids.rgid == 42
            && p.gids.egid == 43
            && p.gids.sgid == 44
    };
    s.add("J6-res三位任意", j6, "root 三位任意设");

    // J7 非特权 setresgid 外来拒
    let j7 = b.setresgid(7, KEEP, KEEP, "外来").err() == Some(R_EPERM);
    s.add("J7-res外来拒", j7, "setresgid 外来值必 EPERM");

    // J8 互换放行（r↔e）
    let j8 = {
        let mut u = GiBook::new(1000, 1001, 1001);
        u.setregid(1001, 1000, "互换").is_ok()
    };
    s.add("J8-互换放行", j8, "非特权 r/e 互换合法");

    // J9 同值放行
    let j9 = {
        let mut u = GiBook::new(1000, 1000, 1000);
        u.setgid(1000, "同值").is_ok()
    };
    s.add("J9-同值放行", j9, "gid∈{r,e,s} 放行");

    // J10 三门操作序列一致性
    let j10 = {
        let mut u = GiBook::new(1000, 1001, 1001);
        let a = u.setgid(1000, "a").is_ok();
        let bb = u.setregid(1001, KEEP, "b").is_ok();
        let c = u.setresgid(KEEP, 1000, 1001, "c").is_ok();
        a && bb && c
    };
    s.add("J10-三门序列", j10, "三门连续操作全通");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.setgid(1000, "").err() == Some(GI_EMPTY_NOTE);
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
    fn unx9183_gid_flow() {
        // 特权全翻
        let mut p = GiBook::new(0, 0, 0);
        assert!(p.setgid(1001, "a").is_ok());
        assert_eq!(p.gids, Gids { rgid: 1001, egid: 1001, sgid: 1001 });
        assert!(p.setresgid(42, 43, 44, "b").is_ok());
        // 非特权
        let mut u = GiBook::new(1000, 1000, 1000);
        assert!(u.setgid(1000, "c").is_ok());
        assert_eq!(u.setgid(7, "d"), Err(R_EPERM));
        assert!(u.setregid(KEEP, 1000, "e").is_ok());
        assert_eq!(u.setresgid(7, KEEP, KEEP, "f"), Err(R_EPERM));
    }

    #[test]
    fn unx9183_checks_green() {
        let s = run_unx9183_checks();
        assert!(s.all_passed(), "UNX-F9183 判据未全绿");
    }
}
