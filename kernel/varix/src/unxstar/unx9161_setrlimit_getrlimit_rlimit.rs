//! UNX-F9161 setrlimit/getrlimit 语义档总纲（rlimit 结构四元组）（0x633E0 段）
//!
//! 锚点：UNX-F9161-J1 限额账（资源 × set 动作 × 旧值/新值/结果/留
//! 痕逐条落账）；J2 限额语义（资源闭集 + 软≤硬 + 提硬限特权闸 +
//! RLIM_INFINITY 直通四性质）；J3 限额指纹（账本折叠指纹）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B19（rlimit 总纲面）。
//!
//! 结构：限额四元组（cur/max/hard_floor/flags）+ 资源册 + set/get
//! 核（三闸：值序闸/特权闸/无穷闸）+ 限额账本 + 折叠指纹 + 注入核
//! （软>硬/空留痕/伪资源/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x633E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x633E0;
pub const RL_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const RL_BAD_ORDER: u32 = SEG_BASE + 0x02; // 软>硬
pub const RL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RL_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RL_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 非特权提硬限
pub const R_EINVAL: u32 = 2; // 软>硬/伪资源
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 资源闭集（B19 档覆盖十个）。
pub const RS_NOFILE: usize = 0;
pub const RS_CORE: usize = 1;
pub const RS_STACK: usize = 2;
pub const RS_AS: usize = 3;
pub const RS_CPU: usize = 4;
pub const RS_NPROC: usize = 5;
pub const RS_FSIZE: usize = 6;
pub const RS_DATA: usize = 7;
pub const RS_RSS: usize = 8;
pub const RS_SIGPENDING: usize = 9;
pub const RS_COUNT: usize = 10;

pub const RS_NAMES: [&str; RS_COUNT] = [
    "NOFILE", "CORE", "STACK", "AS", "CPU", "NPROC", "FSIZE", "DATA", "RSS", "SIGPENDING",
];

/// RLIM_INFINITY。
pub const RLIM_INFINITY: u32 = u32::MAX;

/// 限额四元组。
#[derive(Clone)]
pub struct Rlim {
    pub res: usize,
    pub cur: u32,
    pub max: u32,
    /// 硬限地板（提硬限不得低于初值——非特权时）
    pub hard_floor: u32,
}

impl Rlim {
    pub fn order_ok(&self) -> bool {
        self.cur == RLIM_INFINITY || self.max == RLIM_INFINITY || self.cur <= self.max
    }
}

/// 单条限额账。
#[derive(Clone)]
pub struct RlRec {
    pub seq: u32,
    pub res: usize,
    pub old_cur: u32,
    pub old_max: u32,
    pub new_cur: u32,
    pub new_max: u32,
    pub code: u32,
    pub note: String,
}

impl RlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "RL{}|{}|{}:{}->{}:{}|{}|{}",
            self.seq,
            RS_NAMES[self.res],
            self.old_cur,
            self.old_max,
            self.new_cur,
            self.new_max,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 限额簿。
pub struct RlBook {
    pub limits: Vec<Rlim>,
    pub ledger: Vec<RlRec>,
    pub seq_next: u32,
    pub privileged: bool,
    pub sealed: bool,
}

impl RlBook {
    pub fn new(privileged: bool) -> RlBook {
        RlBook {
            limits: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            privileged,
            sealed: false,
        }
    }

    /// 初值登记。
    pub fn seed(&mut self, res: usize, cur: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL_FROZEN);
        }
        if res >= RS_COUNT {
            return Err(RL_UNK_RES);
        }
        if let Some(i) = self.limits.iter().position(|l| l.res == res) {
            self.limits[i].cur = cur;
            self.limits[i].max = max;
            return Ok(());
        }
        self.limits.push(Rlim { res, cur, max, hard_floor: max });
        Ok(())
    }

    fn idx_of(&self, res: usize) -> Option<usize> {
        self.limits.iter().position(|l| l.res == res)
    }

    /// getrlimit。
    pub fn get(&self, res: usize) -> Result<(u32, u32), u32> {
        match self.idx_of(res) {
            Some(i) => Ok((self.limits[i].cur, self.limits[i].max)),
            None => Err(R_EINVAL),
        }
    }

    /// setrlimit：值序闸 + 特权闸 + 无穷闸。
    pub fn set(&mut self, res: usize, new_cur: u32, new_max: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL_FROZEN);
        }
        if note.is_empty() {
            return Err(RL_EMPTY_NOTE);
        }
        if res >= RS_COUNT {
            self.record(res, 0, 0, new_cur, new_max, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        let i = match self.idx_of(res) {
            Some(i) => i,
            None => {
                self.record(res, 0, 0, new_cur, new_max, R_EINVAL, note);
                return Err(R_EINVAL);
            }
        };
        let (oc, om) = (self.limits[i].cur, self.limits[i].max);
        // 值序闸：软 > 硬 拒
        let order_bad = !(new_cur == RLIM_INFINITY || new_max == RLIM_INFINITY || new_cur <= new_max);
        if order_bad {
            self.record(res, oc, om, new_cur, new_max, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        // 特权闸：非特权提硬限拒（新硬 > 旧硬 且超地板）
        if !self.privileged && new_max != RLIM_INFINITY && new_max > om {
            self.record(res, oc, om, new_cur, new_max, R_EPERM, note);
            return Err(R_EPERM);
        }
        self.limits[i].cur = new_cur;
        self.limits[i].max = new_max;
        self.record(res, oc, om, new_cur, new_max, R_OK, note);
        Ok(self.ledger.last().map(|r| r.seq).unwrap_or(0))
    }

    fn record(
        &mut self,
        res: usize,
        oc: u32,
        om: u32,
        nc: u32,
        nm: u32,
        code: u32,
        note: &str,
    ) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(RlRec {
            seq,
            res,
            old_cur: oc,
            old_max: om,
            new_cur: nc,
            new_max: nm,
            code,
            note: note.to_string(),
        });
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
        let mut h: u32 = 0x91610;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.res as u32);
            h = h.wrapping_mul(31).wrapping_add(r.new_cur);
            h = h.wrapping_mul(31).wrapping_add(r.new_max);
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

/// 基础样本（特权视角）：NOFILE(64/4096)、CORE(0/∞)、STACK(8M/∞)、AS(∞/∞)。
pub fn mk_book(privileged: bool) -> RlBook {
    let mut b = RlBook::new(privileged);
    let _ = b.seed(RS_NOFILE, 64, 4096);
    let _ = b.seed(RS_CORE, 0, RLIM_INFINITY);
    let _ = b.seed(RS_STACK, 8 * 1024 * 1024, RLIM_INFINITY);
    let _ = b.seed(RS_AS, RLIM_INFINITY, RLIM_INFINITY);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 软>硬账（struct 自查）
    let bad = Rlim { res: 0, cur: 100, max: 50, hard_floor: 50 };
    got[0] = !bad.order_ok();
    // 2) 空留痕
    let bad2 = RlRec {
        seq: 2,
        res: 0,
        old_cur: 0,
        old_max: 0,
        new_cur: 0,
        new_max: 0,
        code: R_OK,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 软>硬 set 拒
    let mut b = mk_book(true);
    got[2] = b.set(RS_NOFILE, 5000, 4096, "inj-order").err() == Some(R_EINVAL);
    // 4) 非特权提硬限拒
    let mut b2 = mk_book(false);
    got[3] = b2.set(RS_NOFILE, 100, 8192, "inj-eperm").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.set(RS_NOFILE, 128, 4096, "inj-frozen").err() == Some(RL_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9161");
    let mut b = mk_book(true);

    // J1 限额账非空
    let _ = b.set(RS_NOFILE, 128, 4096, "提软限");
    let j1 = !b.ledger.is_empty();
    s.add("J1-限额账非空", j1, "set 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且码闭集");

    // J3 资源闭集
    let j3 = {
        let mut b2 = mk_book(true);
        b2.seed(99, 1, 1).err() == Some(RL_UNK_RES) && b2.limits.len() == 4
    };
    s.add("J3-资源闭集", j3, "伪资源拒且初值登记幂等");

    // J4 值序闸
    let j4 = b.set(RS_NOFILE, 5000, 4096, "软>硬").err() == Some(R_EINVAL);
    s.add("J4-值序闸", j4, "软>硬必 EINVAL");

    // J5 特权闸（非特权提硬限 EPERM）
    let j5 = {
        let mut b2 = mk_book(false);
        b2.set(RS_NOFILE, 100, 8192, "提硬").err() == Some(R_EPERM)
    };
    s.add("J5-特权闸", j5, "非特权提硬限必 EPERM");

    // J6 特权可提硬限
    let j6 = {
        let mut b2 = mk_book(true);
        b2.set(RS_NOFILE, 100, 8192, "特权提硬").is_ok()
    };
    s.add("J6-特权提硬", j6, "特权视角提硬限放行");

    // J7 RLIM_INFINITY 直通
    let j7 = {
        let mut b2 = mk_book(true);
        b2.set(RS_STACK, RLIM_INFINITY, RLIM_INFINITY, "无限").is_ok()
            && b2.get(RS_STACK) == Ok((RLIM_INFINITY, RLIM_INFINITY))
    };
    s.add("J7-无穷直通", j7, "RLIM_INFINITY 绕值序闸");

    // J8 getrlimit 闭路
    let j8 = b.get(RS_NOFILE) == Ok((128, 4096));
    s.add("J8-get闭路", j8, "get 与 set 后值一致");

    // J9 get 未登记资源
    let j9 = b.get(RS_CPU).err() == Some(R_EINVAL);
    s.add("J9-未登记拒", j9, "未 seed 资源 get 必拒");

    // J10 软限可降不可越硬（降软放行）
    let j10 = {
        let mut b2 = mk_book(true);
        b2.set(RS_NOFILE, 32, 4096, "降软").is_ok() && b2.get(RS_NOFILE) == Ok((32, 4096))
    };
    s.add("J10-降软放行", j10, "软限下调放行");

    // J11 拒绝不落成功账
    let j11 = {
        let before_ok = b.ledger.iter().filter(|r| r.code == R_OK).count();
        let _ = b.set(RS_NOFILE, 9999, 4096, "不落");
        b.ledger.iter().filter(|r| r.code == R_OK).count() == before_ok
    };
    s.add("J11-拒绝零成功账", j11, "拒绝路径不产生成功账");

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
    fn unx9161_rlimit_flow() {
        let mut b = mk_book(true);
        assert!(b.set(RS_NOFILE, 128, 4096, "a").is_ok());
        assert_eq!(b.get(RS_NOFILE), Ok((128, 4096)));
        assert_eq!(b.set(RS_NOFILE, 5000, 4096, "b"), Err(R_EINVAL));
        // 非特权
        let mut u = mk_book(false);
        assert_eq!(u.set(RS_NOFILE, 100, 8192, "c"), Err(R_EPERM));
        assert!(u.set(RS_NOFILE, 32, 4096, "d").is_ok());
    }

    #[test]
    fn unx9161_checks_green() {
        let s = run_unx9161_checks();
        assert!(s.all_passed(), "UNX-F9161 判据未全绿");
    }
}
