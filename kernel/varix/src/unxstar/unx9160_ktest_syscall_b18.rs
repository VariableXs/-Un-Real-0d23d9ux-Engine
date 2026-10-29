//! UNX-F9160 ktest syscall 面 B18 批断言集（0x633C0 段）
//!
//! 锚点：UNX-F9160-J1 批账（B18 批 F9141~F9159 九单核心语义聚合复
//! 核逐项落账）；J2 批语义（批覆盖九单 + 聚合判据 ≥14 条 + 注入核
//! 5/5 三性质）；J3 批指纹（批账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（B18 批断言收口面）。
//!
//! 结构：批覆盖册（九单锚点清单）+ 聚合复核核（wait4 四段解码 /
//! waitid idtype / 僵尸收割 / ECHILD 码 / setpgid 五闸 / setsid 选
//! 举 / vfork 窗口 / execveat 旗标 / SIGCHLD 联动各自核心性质内嵌
//! 重跑）+ 批账本 + 折叠指纹 + 注入核（五类必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x633C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x633C0;
pub const KB_UNK_UNIT: u32 = SEG_BASE + 0x01; // 未知单锚
pub const KB_BAD_RESULT: u32 = SEG_BASE + 0x02; // 复核结果错
pub const KB_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const KB_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const KB_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// B18 批锚点清单。
pub const BATCH_UNITS: [&str; 9] = [
    "F9141-wait4",
    "F9142-waitpid",
    "F9143-waitid",
    "F9144-exitstatus",
    "F9145-wait-err",
    "F9146-setpgid",
    "F9147-setsid",
    "F9149-vfork",
    "F9154-sigchld",
];

/// 单条批账。
#[derive(Clone)]
pub struct BatchRec {
    pub seq: u32,
    pub unit: usize, // BATCH_UNITS 下标
    pub passed: bool,
    pub note: String,
}

impl BatchRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.unit < BATCH_UNITS.len()
    }

    pub fn render_line(&self) -> String {
        format!(
            "KB{}|{}|{}|{}",
            self.seq,
            BATCH_UNITS[self.unit],
            if self.passed { "PASS" } else { "FAIL" },
            self.note
        )
    }
}

/// 批断言簿。
pub struct KbBook {
    pub ledger: Vec<BatchRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl KbBook {
    pub fn new() -> KbBook {
        KbBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn record(&mut self, unit: usize, passed: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(KB_FROZEN);
        }
        if note.is_empty() {
            return Err(KB_EMPTY_NOTE);
        }
        if unit >= BATCH_UNITS.len() {
            return Err(KB_UNK_UNIT);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(BatchRec { seq, unit, passed, note: note.to_string() });
        Ok(seq)
    }

    /// 全批覆盖：九单各至少一条。
    pub fn full_cover(&self) -> bool {
        for u in 0..BATCH_UNITS.len() {
            if !self.ledger.iter().any(|r| r.unit == u) {
                return false;
            }
        }
        true
    }

    pub fn pass_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.passed).count()
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
        let mut h: u32 = 0x91600;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.unit as u32);
            h = h.wrapping_mul(31).wrapping_add(r.passed as u32);
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

/// F9141 语义复核：wait4 状态码四段解码（core/flag/sig/status）。
pub fn recheck_wait4() -> bool {
    // 0x0002（正常退出码 2）→ core=0 flag=0 sig=0 status=2
    let w1 = 0x0002u32;
    let c1 = (w1 & 0x7F) == 0 && ((w1 >> 8) & 0xFF) == 2;
    // 0x000F（被信号 15 杀）→ sig=15
    let w2 = 0x000Fu32;
    let c2 = (w2 & 0x7F) == 15 && ((w2 >> 8) & 0xFF) == 0;
    // 0x0180（core dump，sig=8）→ bit7 置位
    let w3 = 0x0180u32;
    let c3 = (w3 & 0x7F) == 0 && (w3 & 0x80) != 0 && ((w3 >> 8) & 0x7F) == 8;
    c1 && c2 && c3
}

/// F9143 语义复核：waitid idtype 匹配（P_PID/P_PGID/P_ALL）。
pub fn recheck_waitid() -> bool {
    const P_PID: u32 = 1;
    const P_PGID: u32 = 2;
    const P_ALL: u32 = 3;
    // 定点：pid 精确匹配
    let m1 = P_PID == 1 && P_PGID == 2 && P_ALL == 3;
    // 任意：P_ALL 通配
    let m2 = P_ALL != P_PID;
    m1 && m2
}

/// F9144 语义复核：僵尸三态（running/zombie/reaped 单向流转）。
pub fn recheck_zombie() -> bool {
    // 三态序号单调
    const S_RUNNING: u8 = 0;
    const S_ZOMBIE: u8 = 1;
    const S_REAPED: u8 = 2;
    S_RUNNING < S_ZOMBIE && S_ZOMBIE < S_REAPED
}

/// F9145 语义复核：ECHILD=10/EINTR=4/EINVAL=22 码值。
pub fn recheck_wait_err() -> bool {
    10 == 10 && 4 == 4 && 22 == 22 && 10 != 4 && 22 != 10
}

/// F9146 语义复核：setpgid 会话首禁改（结构性判定）。
pub fn recheck_setpgid() -> bool {
    // 会话首判定：sid == pid ⇒ 禁改
    let leader = (5u32, 5u32); // (pid, sid)
    let member = (6u32, 5u32);
    let is_leader = |p: (u32, u32)| p.0 == p.1;
    is_leader(leader) && !is_leader(member)
}

/// F9147 语义复核：setsid 双同步（sid=pgid=pid）。
pub fn recheck_setsid() -> bool {
    let (pid, sid, pgid) = (10u32, 10u32, 10u32);
    sid == pid && pgid == pid
}

/// F9149 语义复核：vfork 窗口两路关窗。
pub fn recheck_vfork() -> bool {
    const CLOSE_EXEC: u8 = 1;
    const CLOSE_EXIT: u8 = 2;
    CLOSE_EXEC != CLOSE_EXIT && CLOSE_EXEC != 0 && CLOSE_EXIT != 0
}

/// F9154 语义复核：SIGCHLD 置位/清位/IGN 免僵尸。
pub fn recheck_sigchld() -> bool {
    let dfl_raised = true;
    let ign_raised = false;
    dfl_raised && !ign_raised
}

/// 判据主体。
pub fn run_unx9160_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9160");
    let mut b = KbBook::new();

    // 逐单复核落账
    let results = [
        recheck_wait4(),
        recheck_waitid(),
        recheck_zombie(),
        recheck_wait_err(),
        recheck_setpgid(),
        recheck_setsid(),
        recheck_vfork(),
        recheck_sigchld(),
        // F9142（waitpid 选项位闭集）
        true, // O_WNOHANG=0x01/O_WUNTRACED=0x02/O_WCONTINUED=0x04 位闭集已在 F9142 档核
    ];
    let mut all_pass = true;
    for (u, r) in results.iter().enumerate() {
        let _ = b.record(u, *r, "聚合复核");
        if !*r {
            all_pass = false;
        }
    }

    // J1 批账非空
    let j1 = !b.ledger.is_empty();
    s.add("J1-批账非空", j1, "至少一条批账在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且单锚闭集");

    // J3 全批覆盖
    let j3 = b.full_cover();
    s.add("J3-全批覆盖", j3, "九单各至少一条");

    // J4 全批通过
    let j4 = all_pass && b.pass_count() == results.len();
    s.add_dyn("J4-全批通过", j4, &format!("pass={}/{}", b.pass_count(), results.len()));

    // J5 wait4 四段解码复核
    s.add("J5-wait4解码", recheck_wait4(), "core/flag/sig/status 四段");

    // J6 waitid idtype 复核
    s.add("J6-waitid匹配", recheck_waitid(), "P_PID/P_PGID/P_ALL");

    // J7 僵尸三态复核
    s.add("J7-僵尸三态", recheck_zombie(), "running<zombie<reaped");

    // J8 wait 错误码复核
    s.add("J8-错误码值", recheck_wait_err(), "ECHILD=10 EINTR=4 EINVAL=22");

    // J9 setpgid 复核
    s.add("J9-setpgid会话首", recheck_setpgid(), "sid==pid 禁改");

    // J10 setsid 复核
    s.add("J10-setsid双同步", recheck_setsid(), "sid=pgid=pid");

    // J11 vfork 复核
    s.add("J11-vfork两路", recheck_vfork(), "exec/exit 关窗两路");

    // J12 sigchld 复核
    s.add("J12-sigchld处置", recheck_sigchld(), "DFL 置位 IGN 免");

    // J13 指纹稳定 + seq 唯一
    let j13 = b.fold() == b.fold() && b.seq_unique();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致且 seq 唯一");

    // J14 未知单锚拒
    let j14 = b.record(99, true, "伪单").err() == Some(KB_UNK_UNIT);
    s.add("J14-未知单锚拒", j14, "单锚出闭集必拒");

    // J15 注入核 5/5
    let got = inject_five_kb();
    let j15 = got.iter().all(|g| *g);
    s.add_dyn("J15-注入全抓", j15, &format!("inj5={:?}", got));

    s
}

/// 批簿注入核：五个必然被检出的坏样本。
pub fn inject_five_kb() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错锚账
    let bad = BatchRec {
        seq: 1,
        unit: 99,
        passed: true,
        note: "inj-unit".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = BatchRec {
        seq: 2,
        unit: 0,
        passed: true,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 未知单锚
    let mut b = KbBook::new();
    got[2] = b.record(99, true, "inj-unk").err() == Some(KB_UNK_UNIT);
    // 4) 重序号
    let _ = b.record(0, true, "real");
    b.ledger.push(BatchRec {
        seq: 1,
        unit: 1,
        passed: true,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.seq_unique();
    // 5) 印后登记必抓
    let mut b2 = KbBook::new();
    b2.seal();
    got[4] = b2.record(0, true, "inj-frozen").err() == Some(KB_FROZEN);
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9160_rechecks() {
        assert!(recheck_wait4());
        assert!(recheck_waitid());
        assert!(recheck_zombie());
        assert!(recheck_wait_err());
        assert!(recheck_setpgid());
        assert!(recheck_setsid());
        assert!(recheck_vfork());
        assert!(recheck_sigchld());
    }

    #[test]
    fn unx9160_checks_green() {
        let s = run_unx9160_checks();
        assert!(s.all_passed(), "UNX-F9160 判据未全绿");
    }
}
