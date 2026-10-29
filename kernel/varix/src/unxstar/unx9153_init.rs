//! UNX-F9153 孤儿进程与 init 收养协议（0x632E0 段）
//!
//! 锚点：UNX-F9153-J1 收养账（每次父亡收养落账：孤儿 pid、旧父、新
//! 父（恒 1）、僵尸转交数、留痕）；J2 收养语义（父亡触发 + 存活子全
//! 收 + 僵尸子转交 wait 权 + 收养幂等四性质）；J3 收养指纹（账本折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（init 收养面）。
//!
//! 结构：族树簿（pid/ppid/alive/zombie）+ 收养核（BFS 全收）+ 收养
//! 账本 + wait 权转交簿 + 折叠指纹 + 注入核（错码/空留痕/伪收养/未
//! 知 pid/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x632E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x632E0;
pub const AD_UNK_PID: u32 = SEG_BASE + 0x01; // 未知 pid
pub const AD_BAD_ADOPT: u32 = SEG_BASE + 0x02; // 伪收养
pub const AD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const AD_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const AD_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 收养目标。
pub const INIT_PID: u32 = 1;

/// 单条收养账。
#[derive(Clone)]
pub struct Adopt {
    pub seq: u32,
    pub dead: u32,
    pub orphans: Vec<u32>,
    pub zombies: Vec<u32>,
    pub note: String,
}

impl Adopt {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.dead != 0
    }

    pub fn render_line(&self) -> String {
        format!(
            "AD{}|dead{}|orph{:?}|zom{:?}|{}",
            self.seq, self.dead, self.orphans, self.zombies, self.note
        )
    }
}

/// 族树条目。
#[derive(Clone)]
pub struct Kin {
    pub pid: u32,
    pub ppid: u32,
    pub alive: bool,
    pub zombie: bool,
    /// wait 权属（收养后转 init）
    pub waiter: u32,
}

/// 收养簿。
pub struct AdoptBook {
    pub kin: Vec<Kin>,
    pub ledger: Vec<Adopt>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AdoptBook {
    pub fn new() -> AdoptBook {
        AdoptBook { kin: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn spawn(&mut self, pid: u32, ppid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AD_FROZEN);
        }
        if pid == 0 || self.kin.iter().any(|k| k.pid == pid) {
            return Err(AD_UNK_PID);
        }
        let waiter = if ppid == 0 { pid } else { ppid };
        self.kin.push(Kin { pid, ppid, alive: true, zombie: false, waiter });
        Ok(())
    }

    /// 子化僵尸：父可 wait 的已亡子。
    pub fn make_zombie(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AD_FROZEN);
        }
        match self.kin.iter_mut().find(|k| k.pid == pid) {
            Some(k) => {
                k.alive = false;
                k.zombie = true;
                Ok(())
            }
            None => Err(AD_UNK_PID),
        }
    }

    pub fn find(&self, pid: u32) -> Option<usize> {
        self.kin.iter().position(|k| k.pid == pid)
    }

    /// 收养核：父亡（非 init），其全部存活子与僵尸子转投 init。
    /// 返回 (存活孤儿数, 僵尸转交数)。
    pub fn die_and_adopt(&mut self, pid: u32, note: &str) -> Result<(usize, usize), u32> {
        if self.sealed {
            return Err(AD_FROZEN);
        }
        if note.is_empty() {
            return Err(AD_EMPTY_NOTE);
        }
        if pid == INIT_PID {
            return Err(AD_BAD_ADOPT); // init 不亡
        }
        let i = match self.find(pid) {
            Some(i) => i,
            None => return Err(AD_UNK_PID),
        };
        let already_dead = !self.kin[i].alive;
        self.kin[i].alive = false;
        // 幂等闸：已亡者再触发零收养（防僵尸重复收集）
        if already_dead {
            self.ledger.push(Adopt {
                seq: self.seq_next,
                dead: pid,
                orphans: Vec::new(),
                zombies: Vec::new(),
                note: note.to_string(),
            });
            self.seq_next += 1;
            return Ok((0, 0));
        }
        // 孤儿收集（先收集后写，避免借用冲突）
        let mut orphans: Vec<u32> = Vec::new();
        let mut zombies: Vec<u32> = Vec::new();
        let n = self.kin.len();
        for k in 0..n {
            if k == i || self.kin[k].ppid != pid {
                continue;
            }
            if self.kin[k].zombie {
                zombies.push(self.kin[k].pid);
            } else if self.kin[k].alive {
                orphans.push(self.kin[k].pid);
            }
        }
        for &o in &orphans {
            let oi = self.find(o).unwrap();
            self.kin[oi].ppid = INIT_PID;
            self.kin[oi].waiter = INIT_PID;
        }
        for &z in &zombies {
            let zi = self.find(z).unwrap();
            self.kin[zi].waiter = INIT_PID;
        }
        self.ledger.push(Adopt {
            seq: self.seq_next,
            dead: pid,
            orphans: orphans.clone(),
            zombies: zombies.clone(),
            note: note.to_string(),
        });
        let seq = self.seq_next;
        self.seq_next += 1;
        let _ = seq;
        Ok((orphans.len(), zombies.len()))
    }

    /// wait 权查询：pid 的可收尸人。
    pub fn waiter_of(&self, pid: u32) -> Option<u32> {
        self.kin.iter().find(|k| k.pid == pid).map(|k| k.waiter)
    }

    /// 收养幂等：二次触发零收养（父已标亡但子 ppid 已改投）。
    pub fn adopt_idempotent(&mut self, pid: u32, note: &str) -> Result<(usize, usize), u32> {
        self.die_and_adopt(pid, note)
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
        let mut h: u32 = 0x91530;
        for a in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(a.seq);
            h = h.wrapping_mul(31).wrapping_add(a.dead);
            for o in &a.orphans {
                h = h.wrapping_mul(31).wrapping_add(*o);
            }
            for z in &a.zombies {
                h = h.wrapping_mul(31).wrapping_add(z.wrapping_add(0x77));
            }
            for b in a.note.bytes() {
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

/// 基础样本：init(1) → A(5) → {B(6), C(7), Z(8·僵尸)}；B → D(9)。
pub fn mk_book() -> AdoptBook {
    let mut b = AdoptBook::new();
    let _ = b.spawn(1, 0);
    let _ = b.spawn(5, 1);
    let _ = b.spawn(6, 5);
    let _ = b.spawn(7, 5);
    let _ = b.spawn(8, 5);
    let _ = b.spawn(9, 6);
    let _ = b.make_zombie(8);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码账（dead=0）
    let bad = Adopt {
        seq: 1,
        dead: 0,
        orphans: Vec::new(),
        zombies: Vec::new(),
        note: "inj-dead".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Adopt {
        seq: 2,
        dead: 5,
        orphans: Vec::new(),
        zombies: Vec::new(),
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪收养：init 亡被拒
    let mut b = mk_book();
    got[2] = b.die_and_adopt(1, "inj-init").err() == Some(AD_BAD_ADOPT);
    // 4) 未知 pid
    got[3] = b.die_and_adopt(0xDEAD, "inj-unk").err() == Some(AD_UNK_PID);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.die_and_adopt(5, "inj-frozen").err() == Some(AD_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9153");
    let mut b = mk_book();

    // J1 收养账非空
    let _ = b.die_and_adopt(5, "A 亡收养");
    let j1 = !b.ledger.is_empty();
    s.add("J1-收养账非空", j1, "父亡收养落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|a| a.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 dead 非零");

    // J3 全收：A 亡后 B、C 投 init
    let j3 = {
        let mut b2 = mk_book();
        let _ = b2.die_and_adopt(5, "全收");
        b2.waiter_of(6) == Some(INIT_PID) && b2.waiter_of(7) == Some(INIT_PID)
    };
    s.add("J3-存活全收", j3, "存活子全部转投 init");

    // J4 僵尸转交：Z(8) wait 权转 init
    let j4 = {
        let mut b2 = mk_book();
        let _ = b2.die_and_adopt(5, "转交");
        b2.waiter_of(8) == Some(INIT_PID)
    };
    s.add("J4-僵尸转交", j4, "僵尸子 wait 权转 init");

    // J5 隔代不收：D(9) 父是 B(6) 不因 A 亡而收
    let j5 = {
        let mut b2 = mk_book();
        let (o, z) = b2.die_and_adopt(5, "隔代").unwrap_or((9, 9));
        o == 2 && z == 1 && b2.waiter_of(9) == Some(6)
    };
    s.add("J5-隔代不收", j5, "孙辈不越代收养");

    // J6 收养后 ppid 改写
    let j6 = {
        let mut b2 = mk_book();
        let _ = b2.die_and_adopt(5, "ppid");
        b2.kin[b2.find(6).unwrap()].ppid == INIT_PID
    };
    s.add("J6-ppid改写", j6, "孤儿 ppid 物理改投 1");

    // J7 收养幂等：二次触发零收
    let j7 = {
        let mut b2 = mk_book();
        let _ = b2.die_and_adopt(5, "一次");
        let second = b2.die_and_adopt(5, "二次").unwrap_or((9, 9));
        second == (0, 0)
    };
    s.add("J7-收养幂等", j7, "二次触发零收养");

    // J8 init 不亡
    let j8 = b.die_and_adopt(1, "init亡").err() == Some(AD_BAD_ADOPT);
    s.add("J8-init不亡", j8, "init 收养触发必拒");

    // J9 未知 pid
    let j9 = b.die_and_adopt(0xBEEF, "未知").err() == Some(AD_UNK_PID);
    s.add("J9-未知pid", j9, "未知 pid 收养必拒");

    // J10 连锁收养：B 亡后 D 投 init（B 已先被 A 亡改投？否——B 死时 D 才收）
    let j10 = {
        let mut b2 = mk_book();
        let _ = b2.die_and_adopt(5, "A 亡");
        let r = b2.die_and_adopt(6, "B 亡");
        r == Ok((1, 0)) && b2.waiter_of(9) == Some(INIT_PID)
    };
    s.add("J10-连锁收养", j10, "子亡后孙辈继续转投 init");

    // J11 空留痕拒
    let j11 = b.die_and_adopt(7, "").err() == Some(AD_EMPTY_NOTE);
    s.add("J11-空留痕拒", j11, "空留痕必拒");

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
    fn unx9153_adopt_flow() {
        let mut b = mk_book();
        assert_eq!(b.die_and_adopt(5, "a"), Ok((2, 1)));
        assert_eq!(b.waiter_of(6), Some(1));
        assert_eq!(b.waiter_of(8), Some(1));
        // 幂等
        assert_eq!(b.die_and_adopt(5, "b"), Ok((0, 0)));
        // 连锁
        assert_eq!(b.die_and_adopt(6, "c"), Ok((1, 0)));
        assert_eq!(b.waiter_of(9), Some(1));
    }

    #[test]
    fn unx9153_checks_green() {
        let s = run_unx9153_checks();
        assert!(s.all_passed(), "UNX-F9153 判据未全绿");
    }
}
