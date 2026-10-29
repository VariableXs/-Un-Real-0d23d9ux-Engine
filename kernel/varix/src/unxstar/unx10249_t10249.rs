//! UNX-F10249 全绿撤回协议（撤回路径演练可复现）（0x13260 段）
//!
//! 锚点：UNX-F10249-J1 撤回协议达成（已发全绿按撤回路径收回并留痕
//! 10/10 次演练可复现）；J2 撤回前提检出（未发全绿撤回/撤回后复用
//! 10/10 次拒绝）；J3 撤回留痕四要素（撤回对象/原因/时点/继任状态）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：全绿发布账（发布态机）+ 撤回核（前提判定 + 四要素留痕）+
//! 撤回账（不可复用锁定）+ 注入核（未发撤回/复用/冻结后改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x13260 独占 0x20）。
pub const SEG_BASE: u32 = 0x13260;
pub const RC_NOT_ISSUED: u32 = SEG_BASE + 0x01; // 未发全绿撤回
pub const RC_REUSE: u32 = SEG_BASE + 0x02; // 撤回后复用
pub const RC_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改
pub const RC_BAD_ID: u32 = SEG_BASE + 0x04; // 坏对象
pub const RC_NO_REASON: u32 = SEG_BASE + 0x05; // 无撤回原因

/// 发布态：草拟/已发布/已撤回。
pub const PS_DRAFT: u8 = 0;
pub const PS_ISSUED: u8 = 1;
pub const PS_RETRACTED: u8 = 2;

pub fn ps_ok(v: u8) -> bool {
    v <= PS_RETRACTED
}

/// 全绿发布单。
#[derive(Clone, PartialEq)]
pub struct Issue {
    pub id: u32,
    pub state: u8,
    pub issued_tick: u64,
    pub note: String,
}

impl Issue {
    pub fn ok(&self) -> bool {
        self.id != 0 && ps_ok(self.state) && !self.note.is_empty()
    }
}

/// 发布账。
pub struct IssueBook {
    rows: Vec<Issue>,
    retracts: Vec<String>, // 撤回留痕（四要素）
    frozen: bool,
}

impl IssueBook {
    pub fn new() -> IssueBook {
        IssueBook { rows: Vec::new(), retracts: Vec::new(), frozen: false }
    }

    /// 登发布单：坏 id/空注/冻结后写/重号拒收。
    pub fn reg(&mut self, id: u32, note: &str) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if id == 0 {
            return Err("zero_id");
        }
        if note.is_empty() {
            return Err("empty_note");
        }
        if self.rows.iter().any(|r| r.id == id) {
            return Err("dup_id");
        }
        self.rows.push(Issue {
            id,
            state: PS_DRAFT,
            issued_tick: 0,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 发布：草拟 → 已发布（落时点）。
    pub fn issue(&mut self, id: u32, tick: u64) -> Result<(), &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                if r.state != PS_DRAFT {
                    return Err("not_draft");
                }
                r.state = PS_ISSUED;
                r.issued_tick = tick;
                Ok(())
            }
            None => Err("no_id"),
        }
    }

    /// 撤回：已发布 → 已撤回，四要素留痕；未发布撤回拒收。
    pub fn retract(&mut self, id: u32, reason: &str, tick: u64) -> Result<String, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if reason.is_empty() {
            return Err("no_reason");
        }
        match self.rows.iter_mut().find(|r| r.id == id) {
            Some(r) => {
                if r.state == PS_RETRACTED {
                    return Err("already");
                }
                if r.state != PS_ISSUED {
                    return Err("not_issued");
                }
                r.state = PS_RETRACTED;
                let rec = format!(
                    "RETRACT|i{}|at{}|tick{}|next=locked",
                    id, reason, tick
                );
                self.retracts.push(rec.clone());
                Ok(rec)
            }
            None => Err("no_id"),
        }
    }

    /// 撤回后复用检测：已撤回单再发布即拒（复用红线）。
    pub fn reuse_attempt(&mut self, id: u32) -> Result<(), &'static str> {
        self.issue(id, 999)
    }

    /// 撤回留痕四要素完备。
    pub fn log_complete(&self) -> bool {
        self.retracts
            .iter()
            .all(|r| r.contains("RETRACT|i") && r.contains("|at") && r.contains("|tick") && r.contains("next=locked"))
    }

    pub fn retract_count(&self) -> usize {
        self.retracts.len()
    }

    pub fn state_of(&self, id: u32) -> Option<u8> {
        self.rows.iter().find(|r| r.id == id).map(|r| r.state)
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.ok())
    }
}

/// 注入核。
pub fn inject_fault(b: &mut IssueBook, code: u32, id: u32) -> Result<String, &'static str> {
    match code {
        RC_NOT_ISSUED => {
            // 未发布即撤回
            let _ = b.reg(id, "草拟单");
            b.retract(id, "误操作", 1)
        }
        RC_REUSE => {
            let _ = b.reg(id, "发布单");
            let _ = b.issue(id, 1);
            let _ = b.retract(id, "发现假绿", 2);
            b.reuse_attempt(id).map(|_| String::new())
        }
        RC_FROZEN => {
            b.freeze();
            b.reg(id, "冻结后登").map(|_| String::new())
        }
        RC_BAD_ID => b.retract(0, "坏对象", 1),
        RC_NO_REASON => {
            let _ = b.reg(id, "发布单");
            let _ = b.issue(id, 1);
            b.retract(id, "", 2)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10249_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10249");

    // J1 撤回协议全路径：发布→撤回→四要素留痕
    let j1 = {
        let mut b = IssueBook::new();
        let _ = b.reg(1, "正常单");
        let _ = b.issue(1, 10);
        match b.retract(1, "发现假绿", 20) {
            Ok(rec) => {
                b.state_of(1) == Some(PS_RETRACTED)
                    && b.log_complete()
                    && rec.contains("|i1|")
                    && rec.contains("at发现假绿")
            }
            Err(_) => false,
        }
    };
    s.add("J1-撤回四要素", j1, "发布→撤回→留痕全备");

    // J2a 未发布撤回拒绝
    let j2a = inject_fault(&mut IssueBook::new(), RC_NOT_ISSUED, 2) == Err("not_issued");
    s.add("J2a-未发撤回拒", j2a, "草拟单不得撤回");

    // J2b 撤回后复用拒绝
    let j2b = inject_fault(&mut IssueBook::new(), RC_REUSE, 3) == Err("not_draft");
    s.add("J2b-复用拒绝", j2b, "已撤回单再发布即拒");

    // J2c 无原因撤回拒绝
    let j2c = inject_fault(&mut IssueBook::new(), RC_NO_REASON, 4) == Err("no_reason");
    s.add("J2c-无原因拒", j2c, "空原因即拒");

    // J3 注入面五路
    let codes = [RC_NOT_ISSUED, RC_REUSE, RC_FROZEN, RC_BAD_ID, RC_NO_REASON];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut IssueBook::new(), **c, 9).is_err())
        .count();
    s.add("J3-注入五路", caught == 5, "五路注入全拒");

    // J4 坏对象拒收
    let j4 = inject_fault(&mut IssueBook::new(), RC_BAD_ID, 0) == Err("no_id");
    s.add("J4-坏对象拒", j4, "id=0 无单即拒");

    // J5 冻结红线
    let j5 = {
        let mut b = IssueBook::new();
        inject_fault(&mut b, RC_FROZEN, 5);
        b.is_frozen() && b.reg(6, "后") == Err("frozen")
    };
    s.add("J5-冻结拒收", j5, "印后登记必抓");

    // J6 二次撤回拒绝
    let j6 = {
        let mut b = IssueBook::new();
        let _ = b.reg(7, "单");
        let _ = b.issue(7, 1);
        let _ = b.retract(7, "原因一", 2);
        b.retract(7, "原因二", 3) == Err("already")
    };
    s.add("J6-二次撤回拒", j6, "已撤回再撤即拒");

    // J7 重复发布拒绝：已发布单不可再 issue
    let j7 = {
        let mut b = IssueBook::new();
        let _ = b.reg(8, "单");
        let _ = b.issue(8, 1);
        b.issue(8, 2) == Err("not_draft")
    };
    s.add("J7-重复发布拒", j7, "非草拟不得发布");

    // J8 留痕账与状态机一致
    let j8 = {
        let mut b = IssueBook::new();
        let _ = b.reg(10, "a");
        let _ = b.reg(11, "b");
        let _ = b.issue(10, 1);
        let _ = b.issue(11, 2);
        let _ = b.retract(10, "r1", 3);
        let _ = b.retract(11, "r2", 4);
        b.retract_count() == 2
            && b.state_of(10) == Some(PS_RETRACTED)
            && b.state_of(11) == Some(PS_RETRACTED)
            && b.log_complete()
    };
    s.add("J8-账态一致", j8, "留痕数与状态机一致");

    // J9 单据负样本：坏态 ok 必假
    let j9 = {
        let bad = Issue { id: 1, state: 9, issued_tick: 0, note: "x".to_string() };
        let empty = Issue { id: 2, state: PS_DRAFT, issued_tick: 0, note: String::new() };
        !bad.ok() && !empty.ok()
    };
    s.add("J9-负样本两假", j9, "坏态/空注 ok 必假");

    // J10 十轮撤回演练：10 轮全路径留痕一致（确定性可复现）
    let j10 = {
        let mut ok = 0;
        for r in 0..10u32 {
            let mut b = IssueBook::new();
            let id = 100 + r;
            let _ = b.reg(id, "演练单");
            let _ = b.issue(id, r as u64);
            match b.retract(id, "演练", r as u64 + 1) {
                Ok(rec) => {
                    if rec.contains("next=locked")
                        && b.state_of(id) == Some(PS_RETRACTED)
                        && b.log_complete()
                    {
                        ok += 1;
                    }
                }
                Err(_) => {}
            }
        }
        ok == 10
    };
    s.add("J10-十轮演练", j10, "10/10 轮撤回可复现");

    s
}
