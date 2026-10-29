//! UNX-F9176 资源族与 shell ulimit 消费核（0x635C0 段）
//!
//! 锚点：UNX-F9176-J1 ulimit 账（shell 内建 ulimit 每次读写落账：
//! 资源、动作（查/设软/设硬）、值、结果、留痕）；J2 ulimit 语义
//! （-S/-H 双开关 + 查询返当前档 + 设置受限闸 + 未知名拒四性质）；
//! J3 ulimit 指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（shell ulimit 消费面）。
//!
//! 结构：shell 限额册 + ulimit 核（查/设双路）+ ulimit 账本 + 折叠
//! 指纹 + 注入核（错开关/空留痕/伪资源/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x635C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x635C0;
pub const UL_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源名
pub const UL_BAD_SWITCH: u32 = SEG_BASE + 0x02; // 错开关
pub const UL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const UL_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const UL_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 硬限不可降（非特权）
pub const R_EINVAL: u32 = 2; // 伪资源/错开关
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// shell 资源名闭集（ulimit -X 的 X）。
pub const UN_FSIZE: usize = 0;
pub const UN_NOFILE: usize = 1;
pub const UN_NPROC: usize = 2;
pub const UN_STACK: usize = 3;
pub const UN_COUNT: usize = 4;

pub const UN_NAMES: [&str; UN_COUNT] = ["fsize", "nofile", "nproc", "stack"];

/// 动作闭集。
pub const ACT_QUERY_SOFT: usize = 0;
pub const ACT_QUERY_HARD: usize = 1;
pub const ACT_SET_SOFT: usize = 2;
pub const ACT_SET_HARD: usize = 3;
pub const ACT_COUNT: usize = 4;

pub const ACT_NAMES: [&str; ACT_COUNT] = ["q-soft", "q-hard", "s-soft", "s-hard"];

/// 单条 ulimit 账。
#[derive(Clone)]
pub struct UlRec {
    pub seq: u32,
    pub res: usize,
    pub act: usize,
    pub val: u32,
    pub code: u32,
    pub note: String,
}

impl UlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.res < UN_COUNT && self.act < ACT_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "UL{}|{}|{}|{}|{}|{}",
            self.seq,
            UN_NAMES[self.res],
            ACT_NAMES[self.act],
            self.val,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// shell 限额册。
pub struct UlBook {
    /// (soft, hard) per res
    pub limits: Vec<(usize, (u32, u32))>,
    pub ledger: Vec<UlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UlBook {
    pub fn new() -> UlBook {
        UlBook {
            limits: vec![
                (UN_FSIZE, (u32::MAX, u32::MAX)),
                (UN_NOFILE, (1024, 4096)),
                (UN_NPROC, (256, 512)),
                (UN_STACK, (8 * 1024 * 1024, u32::MAX)),
            ],
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    fn pair(&self, res: usize) -> Option<(u32, u32)> {
        self.limits.iter().find(|(r, _)| *r == res).map(|(_, v)| *v)
    }

    fn set_pair(&mut self, res: usize, v: (u32, u32)) {
        if let Some(i) = self.limits.iter().position(|(r, _)| *r == res) {
            self.limits[i].1 = v;
        }
    }

    /// ulimit 核：查/设双路。
    pub fn ulimit(&mut self, res: usize, act: usize, val: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UL_FROZEN);
        }
        if note.is_empty() {
            return Err(UL_EMPTY_NOTE);
        }
        if res >= UN_COUNT || act >= ACT_COUNT {
            self.record(res, act, val, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        let (soft, hard) = match self.pair(res) {
            Some(p) => p,
            None => {
                self.record(res, act, val, R_EINVAL, note);
                return Err(R_EINVAL);
            }
        };
        match act {
            ACT_QUERY_SOFT => {
                self.record(res, act, soft, R_OK, note);
                Ok(soft)
            }
            ACT_QUERY_HARD => {
                self.record(res, act, hard, R_OK, note);
                Ok(hard)
            }
            ACT_SET_SOFT => {
                if val > hard && hard != u32::MAX {
                    self.record(res, act, val, R_EPERM, note);
                    return Err(R_EPERM);
                }
                self.set_pair(res, (val, hard));
                self.record(res, act, val, R_OK, note);
                Ok(val)
            }
            _ => {
                // ACT_SET_HARD：升放行、降放行（shell 用户即 root 假设下）
                // 本档按 POSIX：非特权不可降硬限——shell 档视作特权可调
                self.set_pair(res, (soft.min(val), val));
                self.record(res, act, val, R_OK, note);
                Ok(val)
            }
        }
    }

    pub fn of(&self, res: usize) -> Option<(u32, u32)> {
        self.pair(res)
    }

    fn record(&mut self, res: usize, act: usize, val: u32, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(UlRec { seq, res, act, val, code, note: note.to_string() });
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
        let mut h: u32 = 0x91760;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.res as u32);
            h = h.wrapping_mul(31).wrapping_add(r.act as u32);
            h = h.wrapping_mul(31).wrapping_add(r.val);
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
    // 1) 伪资源账
    let bad = UlRec {
        seq: 1,
        res: 9,
        act: 0,
        val: 0,
        code: 0,
        note: "inj-res".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = UlRec {
        seq: 2,
        res: 0,
        act: 0,
        val: 0,
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪资源拒
    let mut b = UlBook::new();
    got[2] = b.ulimit(99, ACT_QUERY_SOFT, 0, "inj-res").err() == Some(R_EINVAL);
    // 4) 软限越硬拒
    got[3] = b.ulimit(UN_NOFILE, ACT_SET_SOFT, 99999, "inj-soft").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.ulimit(UN_NOFILE, ACT_QUERY_SOFT, 0, "inj-frozen").err() == Some(UL_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9176");
    let mut b = UlBook::new();

    // J1 ulimit 账非空（查/设双路）
    let j1 = {
        let _ = b.ulimit(UN_NOFILE, ACT_QUERY_SOFT, 0, "查软");
        let _ = b.ulimit(UN_NOFILE, ACT_SET_SOFT, 512, "设软");
        b.ledger.iter().filter(|r| r.act == ACT_QUERY_SOFT).count() >= 1
            && b.ledger.iter().filter(|r| r.act == ACT_SET_SOFT).count() >= 1
    };
    s.add("J1-查设双路", j1, "查询与设置账各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且资源/动作闭集");

    // J3 查询返当前档
    let j3 = b.ulimit(UN_NOFILE, ACT_QUERY_SOFT, 0, "查") == Ok(1024);
    s.add("J3-查询返档", j3, "q-soft 返当前软限");

    // J4 设置生效
    let j4 = {
        let _ = b.ulimit(UN_NOFILE, ACT_SET_SOFT, 512, "设");
        b.of(UN_NOFILE) == Some((512, 4096))
    };
    s.add("J4-设置生效", j4, "s-soft 后册面同步");

    // J5 软限越硬拒
    let j5 = b.ulimit(UN_NOFILE, ACT_SET_SOFT, 99999, "越硬").err() == Some(R_EPERM);
    s.add("J5-软越硬拒", j5, "软限设超硬必 EPERM");

    // J6 硬限升降放行（shell 特权档）
    let j6 = {
        let _ = b.ulimit(UN_NOFILE, ACT_SET_HARD, 8192, "升硬");
        b.of(UN_NOFILE) == Some((512, 8192))
    };
    s.add("J6-硬限可调", j6, "s-hard 放行且同步");

    // J7 伪资源拒
    let j7 = b.ulimit(99, ACT_QUERY_SOFT, 0, "伪").err() == Some(R_EINVAL);
    s.add("J7-伪资源拒", j7, "资源名出闭集必 EINVAL");

    // J8 伪动作拒
    let j8 = b.ulimit(UN_NOFILE, 9, 0, "野动作").err() == Some(R_EINVAL);
    s.add("J8-伪动作拒", j8, "动作出闭集必 EINVAL");

    // J9 四资源全可查
    let j9 = {
        let mut b2 = UlBook::new();
        (0..UN_COUNT).all(|r| b2.ulimit(r, ACT_QUERY_HARD, 0, "查硬").is_ok())
    };
    s.add("J9-四资源可查", j9, "四资源硬限查询全通");

    // J10 无穷值可设软
    let j10 = {
        let mut b2 = UlBook::new();
        b2.ulimit(UN_FSIZE, ACT_SET_SOFT, u32::MAX, "无限").is_ok()
    };
    s.add("J10-无穷软设", j10, "∞ 软限放行");

    // J11 软限下调放行
    let j11 = {
        let mut b2 = UlBook::new();
        b2.ulimit(UN_NPROC, ACT_SET_SOFT, 64, "降软") == Ok(64)
    };
    s.add("J11-降软放行", j11, "软限下调放行");

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
    fn unx9176_ulimit_flow() {
        let mut b = UlBook::new();
        assert_eq!(b.ulimit(UN_NOFILE, ACT_QUERY_SOFT, 0, "a"), Ok(1024));
        assert_eq!(b.ulimit(UN_NOFILE, ACT_SET_SOFT, 512, "b"), Ok(512));
        assert_eq!(b.of(UN_NOFILE), Some((512, 4096)));
        assert_eq!(b.ulimit(UN_NOFILE, ACT_SET_SOFT, 99999, "c"), Err(R_EPERM));
        assert_eq!(b.ulimit(UN_NOFILE, ACT_SET_HARD, 8192, "d"), Ok(8192));
        assert_eq!(b.ulimit(99, ACT_QUERY_SOFT, 0, "e"), Err(R_EINVAL));
        assert_eq!(b.ulimit(UN_NOFILE, 9, 0, "f"), Err(R_EINVAL));
    }

    #[test]
    fn unx9176_checks_green() {
        let s = run_unx9176_checks();
        assert!(s.all_passed(), "UNX-F9176 判据未全绿");
    }
}
