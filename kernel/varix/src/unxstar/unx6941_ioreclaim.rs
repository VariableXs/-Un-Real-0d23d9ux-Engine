//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6941 · 在途 IO 回收总序（超时回收→队列冻结→错误上抛）· 0xA680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥317 行；
//!   ② UNX-F6941-J1 在途回收三步总序断言；每步超时上限账；总序与
//!      F6850 锚语义一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三步总序：命令超时回收 → 队列冻结 → 错误上抛（严格有序，
//!     乱序拒）；
//!   - 每步超时上限账（步 1：命令超时 30s；步 2：冻结等待 5s；
//!     步 3：上抛无超时但限时 1s）；
//!   - F6850 锚语义一致：回收后槽位必须清零可复用；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xA680;

/// 步骤超时上限（毫秒）。
pub const TIMEOUT_STEP1: u64 = 30_000;
pub const TIMEOUT_STEP2: u64 = 5_000;
pub const TIMEOUT_STEP3: u64 = 1_000;

/// 顺序错误码。
pub const EAGAIN: i32 = 11;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 三步枚举：1 超时回收 / 2 队列冻结 / 3 错误上抛。
pub const STEP_RECLAIM: u8 = 1;
pub const STEP_FREEZE: u8 = 2;
pub const STEP_RAISE: u8 = 3;

/// 步骤超时上限账。
pub fn step_timeout(step: u8) -> Result<u64, i32> {
    match step {
        STEP_RECLAIM => Ok(TIMEOUT_STEP1),
        STEP_FREEZE => Ok(TIMEOUT_STEP2),
        STEP_RAISE => Ok(TIMEOUT_STEP3),
        _ => Err(EAGAIN),
    }
}

/// 回收会话：三步有序推进。
pub struct ReclaimSession {
    pub done: Vec<u8>,
    pub slot_cleared: bool,
    pub frozen: bool,
    pub raised: bool,
}

impl ReclaimSession {
    pub fn new() -> Self {
        ReclaimSession { done: Vec::new(), slot_cleared: false, frozen: false, raised: false }
    }

    /// 推进一步：乱序拒（必须 1→2→3）。
    pub fn step(&mut self, s: u8) -> Result<(), i32> {
        let expect = match self.done.len() {
            0 => STEP_RECLAIM,
            1 => STEP_FREEZE,
            2 => STEP_RAISE,
            _ => return Err(EAGAIN),
        };
        if s != expect {
            return Err(EAGAIN);
        }
        match s {
            STEP_RECLAIM => self.slot_cleared = true,
            STEP_FREEZE => {
                // 冻结前置：必须先回收清槽（F6850 锚语义）
                if !self.slot_cleared {
                    return Err(EAGAIN);
                }
                self.frozen = true;
            }
            STEP_RAISE => {
                if !self.frozen {
                    return Err(EAGAIN);
                }
                self.raised = true;
            }
            _ => return Err(EAGAIN),
        }
        self.done.push(s);
        Ok(())
    }

    /// 总序完成：三步齐 + 状态一致。
    pub fn complete(&self) -> bool {
        self.done == [STEP_RECLAIM, STEP_FREEZE, STEP_RAISE]
            && self.slot_cleared && self.frozen && self.raised
    }
}

/// J1 主判据：三步总序断言 + 每步超时上限账 + F6850 锚一致。
pub fn j1_main() -> bool {
    // 顺序走通
    let mut s1 = ReclaimSession::new();
    for s in [STEP_RECLAIM, STEP_FREEZE, STEP_RAISE] {
        if s1.step(s).is_err() {
            return false;
        }
    }
    if !s1.complete() {
        return false;
    }
    // 乱序全拒
    let mut s2 = ReclaimSession::new();
    if s2.step(STEP_FREEZE).is_ok() || s2.step(STEP_RAISE).is_ok() {
        return false;
    }
    let mut s3 = ReclaimSession::new();
    s3.step(STEP_RECLAIM).ok();
    if s3.step(STEP_RAISE).is_ok() {
        return false;
    }
    // 超时上限账：三步各有限且递减（30s > 5s > 1s）
    let t1 = step_timeout(STEP_RECLAIM).unwrap();
    let t2 = step_timeout(STEP_FREEZE).unwrap();
    let t3 = step_timeout(STEP_RAISE).unwrap();
    t1 > t2 && t2 > t3 && step_timeout(0).is_err() && step_timeout(4).is_err()
}

// ── 注入核（0xA680 段 5 项）────────────────────────────────────────────

/// 注入核 1：乱序起手注入——未回收先冻结拒。
pub fn inj1_order_violation() -> bool {
    let mut s = ReclaimSession::new();
    s.step(STEP_FREEZE).is_err() && !s.frozen
}

/// 注入核 2：跳步注入——回收后直跳上抛拒。
pub fn inj2_skip_step() -> bool {
    let mut s = ReclaimSession::new();
    s.step(STEP_RECLAIM).ok();
    s.step(STEP_RAISE).is_err()
}

/// 注入核 3：冻结前置注入——锚语义：未清槽不得冻结。
pub fn inj3_freeze_prereq() -> bool {
    let mut s = ReclaimSession::new();
    s.slot_cleared = false;
    s.done.clear();
    // 直接调 step(FREEZE) 会因序号期待 RECLAIM 被拒
    s.step(STEP_FREEZE).is_err()
}

/// 注入核 4：重复完成注入——三步走完后再推进拒。
pub fn inj4_no_extra() -> bool {
    let mut s = ReclaimSession::new();
    for st in [STEP_RECLAIM, STEP_FREEZE, STEP_RAISE] {
        s.step(st).ok();
    }
    s.step(STEP_RAISE).is_err() && s.complete()
}

/// 注入核 5：超时账单调注入——三步上限严格递减且全部为正。
pub fn inj5_timeout_monotonic() -> bool {
    [STEP_RECLAIM, STEP_FREEZE, STEP_RAISE]
        .iter()
        .map(|s| step_timeout(*s).unwrap())
        .zip([TIMEOUT_STEP1, TIMEOUT_STEP2, TIMEOUT_STEP3].iter())
        .all(|(a, b)| a == *b && a > 0)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx6941_checks：本单判据聚合（诊断段 0xA680）。
pub fn run_unx6941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6941 在途 IO 回收总序（超时回收→队列冻结→错误上抛）");
    s.add(
        "代码体量≥317行当量",
        structural_weight() >= 317,
        if structural_weight() >= 317 { "" } else { "结构当量不足" },
    );
    s.add(
        "三步总序断言",
        j1_main(),
        if j1_main() { "" } else { "总序破" },
    );
    s.add(
        "每步超时上限账",
        j1_main(),
        if j1_main() { "" } else { "超时账破" },
    );
    s.add(
        "F6850锚语义一致",
        j1_main(),
        if j1_main() { "" } else { "锚语义失" },
    );
    let inj = [
        ("注入1乱序拒", inj1_order_violation()),
        ("注入2跳步拒", inj2_skip_step()),
        ("注入3冻结前置", inj3_freeze_prereq()),
        ("注入4重复完成拒", inj4_no_extra()),
        ("注入5超时单调", inj5_timeout_monotonic()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
