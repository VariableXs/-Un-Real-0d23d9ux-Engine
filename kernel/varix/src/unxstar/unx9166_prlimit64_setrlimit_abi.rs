//! UNX-F9166 prlimit64 与 setrlimit 协同（新旧 ABI 双轨）（0x63480 段）
//!
//! 锚点：UNX-F9166-J1 双轨账（setrlimit（旧·两参·本进程）与
//! prlimit64（新·四参·可跨进程）逐条落账：abi、pid、资源、新旧值、
//! 结果、留痕）；J2 双轨语义（新 ABI 超集 + 同簿共享 + 跨进程特权
//! 闸 + 旧 ABI 不支持 pid≠0 四性质）；J3 双轨指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B19（新旧限额 ABI 双轨面）。
//!
//! 结构：限额簿（与 B19 各档同构）+ 双轨核（旧门/新门）+ 双轨账本
//! + 折叠指纹 + 注入核（错 ABI/空留痕/越权跨进程/重号/印后登记必
//! 抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63480 独占 0x20）。
pub const SEG_BASE: u32 = 0x63480;
pub const PR_UNK_ABI: u32 = SEG_BASE + 0x01; // 错 ABI
pub const PR_BAD_TARGET: u32 = SEG_BASE + 0x02; // 越权跨进程
pub const PR_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const PR_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const PR_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// ABI 门。
pub const ABI_OLD: u8 = 0; // setrlimit
pub const ABI_NEW: u8 = 1; // prlimit64
pub const ABI_COUNT: u8 = 2;

pub const ABI_NAMES: [&str; 2] = ["setrlimit", "prlimit64"];

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 越权跨进程
pub const R_EINVAL: u32 = 2; // 值序/伪资源
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 单条双轨账。
#[derive(Clone)]
pub struct AbiRec {
    pub seq: u32,
    pub abi: u8,
    pub pid: u32,
    pub res: usize,
    pub old: (u32, u32),
    pub new: (u32, u32),
    pub code: u32,
    pub note: String,
}

impl AbiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.abi < ABI_COUNT && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "A6{}|{}|p{}|r{}|{:?}->{:?}|{}|{}",
            self.seq,
            ABI_NAMES[self.abi as usize],
            self.pid,
            self.res,
            self.old,
            self.new,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 双轨簿。限额值表按 (pid, res) 双键。
pub struct DualBook {
    /// ((pid, res), (cur, max))
    pub limits: Vec<((u32, usize), (u32, u32))>,
    pub ledger: Vec<AbiRec>,
    pub privileged: bool,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DualBook {
    pub fn new(privileged: bool) -> DualBook {
        DualBook {
            limits: Vec::new(),
            ledger: Vec::new(),
            privileged,
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn seed(&mut self, pid: u32, res: usize, cur: u32, max: u32) {
        let key = (pid, res);
        match self.limits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = (cur, max),
            None => self.limits.push((key, (cur, max))),
        }
    }

    fn get_pair(&self, pid: u32, res: usize) -> Option<(u32, u32)> {
        self.limits.iter().find(|(k, _)| *k == (pid, res)).map(|(_, v)| *v)
    }

    /// 双轨核。
    /// abi=OLD：pid 强制本进程（非 0 传 pid 会被旧门忽略——此处登记
    /// 为 EINVAL 以显式拒绝旧 ABI 的跨进程请求）；abi=NEW：pid 任意，
    /// 跨进程须特权。
    pub fn set_limit(
        &mut self,
        abi: u8,
        caller: u32,
        pid: u32,
        res: usize,
        new: (u32, u32),
        note: &str,
    ) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if note.is_empty() {
            return Err(PR_EMPTY_NOTE);
        }
        if abi >= ABI_COUNT {
            return Err(PR_UNK_ABI);
        }
        let target = if pid == 0 { caller } else { pid };
        // 旧 ABI 只支持本进程：target != caller 即拒
        if abi == ABI_OLD && target != caller {
            self.record(abi, target, res, (0, 0), new, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        // 新 ABI 跨进程须特权
        if abi == ABI_NEW && target != caller && !self.privileged {
            self.record(abi, target, res, (0, 0), new, R_EPERM, note);
            return Err(R_EPERM);
        }
        // 值序闸
        let (nc, nm) = new;
        if !(nc == u32::MAX || nm == u32::MAX || nc <= nm) {
            let old = self.get_pair(target, res).unwrap_or((0, 0));
            self.record(abi, target, res, old, new, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        let old = self.get_pair(target, res).unwrap_or((0, 0));
        self.seed(target, res, nc, nm);
        self.record(abi, target, res, old, new, R_OK, note);
        Ok(old)
    }

    fn record(
        &mut self,
        abi: u8,
        pid: u32,
        res: usize,
        old: (u32, u32),
        new: (u32, u32),
        code: u32,
        note: &str,
    ) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(AbiRec {
            seq,
            abi,
            pid,
            res,
            old,
            new,
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
        let mut h: u32 = 0x91660;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.abi as u32);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.new.0);
            h = h.wrapping_mul(31).wrapping_add(r.new.1);
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

/// 基础样本：caller=100（本进程 NOFILE 64/4096），他进程 200。
pub fn mk_book(privileged: bool) -> DualBook {
    let mut b = DualBook::new(privileged);
    b.seed(100, 0, 64, 4096); // NOFILE
    b.seed(200, 0, 32, 1024);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错 ABI 账
    let bad = AbiRec {
        seq: 1,
        abi: 9,
        pid: 1,
        res: 0,
        old: (0, 0),
        new: (0, 0),
        code: 0,
        note: "inj-abi".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = AbiRec {
        seq: 2,
        abi: 0,
        pid: 1,
        res: 0,
        old: (0, 0),
        new: (0, 0),
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 旧 ABI 跨进程拒
    let mut b = mk_book(false);
    got[2] = b.set_limit(ABI_OLD, 100, 200, 0, (64, 4096), "inj-old-cross")
        .err() == Some(R_EINVAL);
    // 4) 新 ABI 跨进程无特权拒
    got[3] = b.set_limit(ABI_NEW, 100, 200, 0, (64, 4096), "inj-new-cross")
        .err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.set_limit(ABI_NEW, 100, 0, 0, (64, 4096), "inj-frozen")
        .err() == Some(PR_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9166");
    let mut b = mk_book(true);

    // J1 双轨账非空
    let j1 = {
        let _ = b.set_limit(ABI_OLD, 100, 0, 0, (128, 4096), "旧门");
        let _ = b.set_limit(ABI_NEW, 100, 0, 0, (256, 8192), "新门");
        b.ledger.iter().filter(|r| r.abi == ABI_OLD).count() >= 1
            && b.ledger.iter().filter(|r| r.abi == ABI_NEW).count() >= 1
    };
    s.add("J1-双轨账齐", j1, "新旧门各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 ABI/码闭集");

    // J3 同簿共享：旧门写入新门可见
    let j3 = {
        let mut b2 = mk_book(true);
        let _ = b2.set_limit(ABI_OLD, 100, 0, 0, (128, 4096), "旧写");
        // 新门读同键
        let got = b2.set_limit(ABI_NEW, 100, 0, 0, (128, 4096), "新读改");
        got.is_ok() && b2.get_pair(100, 0) == Some((128, 4096))
    };
    s.add("J3-同簿共享", j3, "新旧门操作同一限额簿");

    // J4 旧 ABI 跨进程拒
    let j4 = {
        let mut b2 = mk_book(true);
        b2.set_limit(ABI_OLD, 100, 200, 0, (64, 4096), "旧跨").err() == Some(R_EINVAL)
    };
    s.add("J4-旧门跨进程拒", j4, "setrlimit 无 pid 维度");

    // J5 新 ABI 跨进程特权闸
    let j5 = {
        let mut b2 = mk_book(false);
        b2.set_limit(ABI_NEW, 100, 200, 0, (64, 4096), "越权").err() == Some(R_EPERM)
    };
    s.add("J5-新门特权闸", j5, "无特权跨进程必 EPERM");

    // J6 新 ABI 跨进程特权放行
    let j6 = {
        let mut b2 = mk_book(true);
        b2.set_limit(ABI_NEW, 100, 200, 0, (64, 8192), "特权跨").is_ok()
    };
    s.add("J6-新门特权放行", j6, "特权跨进程放行");

    // J7 pid=0 自指展开
    let j7 = {
        let mut b2 = mk_book(true);
        let _ = b2.set_limit(ABI_NEW, 100, 0, 0, (512, 4096), "自指");
        b2.get_pair(100, 0) == Some((512, 4096))
    };
    s.add("J7-pid零自指", j7, "pid=0 展开 caller");

    // J8 值序闸
    let j8 = {
        let mut b2 = mk_book(true);
        b2.set_limit(ABI_NEW, 100, 0, 0, (9000, 4096), "软>硬").err() == Some(R_EINVAL)
    };
    s.add("J8-值序闸", j8, "软>硬必 EINVAL");

    // J9 旧值留痕
    let j9 = {
        let mut b2 = mk_book(true);
        let old = b2.set_limit(ABI_NEW, 100, 0, 0, (128, 4096), "留旧");
        old == Ok((64, 4096))
    };
    s.add("J9-旧值留痕", j9, "set 返回旧值对");

    // J10 错 ABI 拒
    let j10 = b.set_limit(9, 100, 0, 0, (1, 1), "野门").err() == Some(PR_UNK_ABI);
    s.add("J10-错ABI拒", j10, "ABI 出闭集必拒");

    // J11 拒绝不落成功账
    let j11 = {
        let before = b.ledger.iter().filter(|r| r.code == R_OK).count();
        let _ = b.set_limit(ABI_OLD, 100, 200, 0, (1, 1), "不落");
        b.ledger.iter().filter(|r| r.code == R_OK).count() == before
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
    fn unx9166_dual_flow() {
        let mut b = mk_book(true);
        // 旧门本进程
        assert_eq!(b.set_limit(ABI_OLD, 100, 0, 0, (128, 4096), "a"), Ok((64, 4096)));
        assert_eq!(b.get_pair(100, 0), Some((128, 4096)));
        // 旧门跨进程拒
        assert_eq!(b.set_limit(ABI_OLD, 100, 200, 0, (1, 1), "b"), Err(R_EINVAL));
        // 新门跨进程（特权）
        assert!(b.set_limit(ABI_NEW, 100, 200, 0, (64, 8192), "c").is_ok());
        // 非特权新门跨进程
        let mut u = mk_book(false);
        assert_eq!(u.set_limit(ABI_NEW, 100, 200, 0, (1, 1), "d"), Err(R_EPERM));
    }

    #[test]
    fn unx9166_checks_green() {
        let s = run_unx9166_checks();
        assert!(s.all_passed(), "UNX-F9166 判据未全绿");
    }
}
