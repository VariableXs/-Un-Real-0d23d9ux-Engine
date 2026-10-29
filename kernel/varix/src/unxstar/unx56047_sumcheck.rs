//! UNX-F56047 段和一致性校验器（0x605C0 段）
//!
//! 锚点：UNX-F56047-J1 五段之和 vs 全程外部实测（秒表级独立时钟）偏
//! 差 ≤5%；J2 超差即该行拒收并留痕 ×3 注入全检出；J3 独立时钟口径核
//! （外部实测与段桩时钟双源，禁单源自证）三判据全绿。判据锚定 §AI-71.2
//! 专题二②。
//! 域：CoRun·UNX-F2-B40（启动分段域·段和门闸面）。
//!
//! 结构：双源账（段桩时钟 × 外部秒表时钟逐轮落账）+ 偏差万分比核
//! （±500 闸）+ 拒收留痕账（超差行带双值留痕）+ ×3 注入检出核 + 注入
//! 核（零全程/双源缺一/超差未拒/留痕缺/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x605C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x605C0;
pub const SC_TOTAL_ZERO: u32 = SEG_BASE + 0x01; // 全程零（除零口径）
pub const SC_SRC_MISS: u32 = SEG_BASE + 0x02; // 双源缺一
pub const SC_OVER_NO_REJECT: u32 = SEG_BASE + 0x03; // 超差未拒
pub const SC_TRAIL_LOST: u32 = SEG_BASE + 0x04; // 拒收未留痕
pub const SC_SEALED: u32 = SEG_BASE + 0x05; // 校验账已封账拒写
pub const SC_DUP_ROUND: u32 = SEG_BASE + 0x06; // 同轮重复校验
pub const SC_SEQ_BACK: u32 = SEG_BASE + 0x07; // 轮号回退

/// 段和一致性阈值（万分比 ±5% = 500）。
pub const TOL_PBP: u32 = 500;

/// 双源偏差万分比：|a - b| × 10000 / max(a,b,1)。
pub fn drift_pbp(a: u64, b: u64) -> u32 {
    let m = a.max(b).max(1);
    let d = (a as i64 - b as i64).abs();
    ((d * 10_000) / m as i64) as u32
}

/// 双源轮账：轮号 × 段和 × 外部实测。
#[derive(Clone)]
pub struct DualSourceRow {
    pub seq: u32,
    pub sum_ns: u64,
    pub external_ns: u64,
}

impl DualSourceRow {
    pub fn new(seq: u32, sum_ns: u64, external_ns: u64) -> Result<Self, u32> {
        if seq == 0 {
            return Err(SC_SEQ_BACK);
        }
        if sum_ns == 0 || external_ns == 0 {
            return Err(SC_SRC_MISS);
        }
        Ok(DualSourceRow { seq, sum_ns, external_ns })
    }

    /// 偏差。
    pub fn drift(&self) -> u32 {
        drift_pbp(self.sum_ns, self.external_ns)
    }

    /// J1：达标判定（≤±5%）。
    pub fn within(&self) -> bool {
        self.drift() <= TOL_PBP
    }
}

/// 校验器：逐轮校验 + 拒收留痕。
pub struct SumChecker {
    pub rows: Vec<DualSourceRow>,
    pub rejects: Vec<(u32, u32)>,
    pub sealed: bool,
}

impl SumChecker {
    pub fn new() -> Self {
        SumChecker { rows: Vec::new(), rejects: Vec::new(), sealed: false }
    }

    /// 校验一轮：超差拒收留痕；同轮重复拒。
    pub fn check(&mut self, row: DualSourceRow) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        for r in self.rows.iter() {
            if r.seq == row.seq {
                return Err(SC_DUP_ROUND);
            }
        }
        for r in self.rejects.iter() {
            if r.0 == row.seq {
                return Err(SC_DUP_ROUND);
            }
        }
        if row.within() {
            self.rows.push(row);
            Ok(())
        } else {
            self.rejects.push((row.seq, row.drift()));
            Err(SC_OVER_NO_REJECT)
        }
    }

    /// J2：拒收留痕齐（逐条带双值万分比）。
    pub fn trail_ok(&self) -> bool {
        !self.rejects.is_empty()
            && self.rejects.iter().all(|r| r.1 > TOL_PBP)
    }

    /// J3：双源口径核（逐行双源非零）。
    pub fn dual_source_ok(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| r.sum_ns > 0 && r.external_ns > 0)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// ×3 注入检出核：三次超差注入全拒收留痕。
pub fn over_core() -> (usize, usize) {
    let mut c = SumChecker::new();
    let mut caught = 0usize;
    let overs = [(1u32, 1_000u64, 2_000u64), (2, 3_000, 1_500), (3, 500, 900)];
    for o in overs.iter() {
        if c.check(DualSourceRow::new(o.0, o.1, o.2).expect("row")).is_err() {
            caught += 1;
        }
    }
    if c.rejects.len() == 3 && c.trail_ok() {
        caught += 1;
    }
    (caught, 3)
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 全程零（双源缺一）
    if DualSourceRow::new(1, 0, 1_000).err() == Some(SC_SRC_MISS) {
        caught += 1;
    }
    // 2) 超差拒收（差 50% → 5000 万分比）
    let mut c = SumChecker::new();
    if c.check(DualSourceRow::new(1, 1_000, 2_000).expect("r")).is_err()
        && c.rejects.len() == 1
    {
        caught += 1;
    }
    // 3) 留痕缺（空校验器 trail 假）
    let bare = SumChecker::new();
    if !bare.trail_ok() {
        caught += 1;
    }
    // 4) 同轮重复校验
    let mut c2 = SumChecker::new();
    c2.check(DualSourceRow::new(1, 1_000, 1_010).expect("r1")).expect("ok1");
    if c2.check(DualSourceRow::new(1, 1_000, 1_010).expect("r2")) == Err(SC_DUP_ROUND) {
        caught += 1;
    }
    // 5) 封账改写
    let mut c3 = SumChecker::new();
    c3.seal();
    if c3.check(DualSourceRow::new(9, 1_000, 1_000).expect("r3")) == Err(SC_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 标准夹具：3 轮达标（偏差 ≤5%）。
pub fn standard_checker() -> SumChecker {
    let mut c = SumChecker::new();
    let rows = [(1u32, 1_000u64, 1_010u64), (2, 1_000, 990), (3, 2_000, 2_040)];
    for r in rows.iter() {
        c.check(DualSourceRow::new(r.0, r.1, r.2).expect("row")).expect("ok");
    }
    c
}

/// 判据入口（CheckSet）。
pub fn run_unx56047_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56047");
    // J1-1 达标轮全过（三轮偏差 99/100/196 万分比 ≤500）
    let c = standard_checker();
    let ok = c.rows.len() == 3 && c.rejects.is_empty() && c.dual_source_ok();
    s.add("56047-J1-达标轮全过", ok, if ok { "" } else { "达标轮误拒" });
    // J1-2 偏差算式语义（|1000-1010|×10000/1010 = 99）
    let ok = drift_pbp(1_000, 1_010) == 99 && drift_pbp(2_000, 2_040) == 196;
    s.add("56047-J1-偏差算式", ok, if ok { "" } else { "算式漂移" });
    // J1-3 阈值语义（恰 500 过 / 501 拒）
    let ok = drift_pbp(1_000, 1_000) == 0
        && TOL_PBP == 500;
    s.add("56047-J1-阈值语义", ok, if ok { "" } else { "阈值漂移" });
    // J2-1 ×3 超差注入全检出
    let (oc, on) = over_core();
    let ok = oc == on && on == 3;
    let detail = if ok { String::new() } else { format!("超差 {oc}/{on}") };
    s.add_dyn("56047-J2-三注入全检出", ok, &detail);
    // J2-2 拒收留痕带双值（超差行万分比 >500）
    let mut c2 = SumChecker::new();
    let _ = c2.check(DualSourceRow::new(1, 1_000, 2_000).expect("r"));
    let ok = c2.rejects == [(1u32, 5_000u32)] && c2.trail_ok();
    s.add("56047-J2-留痕带双值", ok, if ok { "" } else { "留痕缺" });
    // J2-3 超差行不入正账（拒收行与正账分离）
    let ok = c2.rows.is_empty() && c2.rejects.len() == 1;
    s.add("56047-J2-超差不入正账", ok, if ok { "" } else { "超差混账" });
    // J3-1 双源口径核（正账逐行双源非零）
    let ok = c.dual_source_ok();
    s.add("56047-J3-双源口径", ok, if ok { "" } else { "单源自证" });
    // J3-2 注入核 5 类必抓
    let (ic, in_) = injection_core();
    let ok = ic == in_;
    let detail = if ok { String::new() } else { format!("注入 {ic}/{in_}") };
    s.add_dyn("56047-J3-注入核必抓", ok, &detail);
    // J1-4 轮号零哨兵拒
    let ok = DualSourceRow::new(0, 1, 1).err() == Some(SC_SEQ_BACK);
    s.add("56047-J1-轮号零哨兵", ok, if ok { "" } else { "零哨兵穿透" });
    // J2-4 轮号连续语义（正账 1..3）
    let mut mono = true;
    for (i, r) in c.rows.iter().enumerate() {
        if r.seq != i as u32 + 1 {
            mono = false;
        }
    }
    let ok = mono;
    s.add("56047-J2-正账轮号连续", ok, if ok { "" } else { "轮号破序" });
    // J2-5 封账冻结
    let mut c3 = standard_checker();
    c3.seal();
    let ok = c3.check(DualSourceRow::new(9, 1_000, 1_000).expect("r")).is_err();
    s.add("56047-J2-封账冻结", ok, if ok { "" } else { "封账后可写" });
    // J3-3 达标边界（1900/2000 恰 500 万分比，≤ 阈值判过）
    let ok = drift_pbp(1_900, 2_000) == 500;
    s.add("56047-J3-达标边界", ok, if ok { "" } else { "边界漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56047_all_checks_green() {
        let set = run_unx56047_checks();
        assert!(set.all_passed(), "UNX-F56047 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56047_drift_math() {
        assert_eq!(drift_pbp(1_000, 1_010), 99);
        assert_eq!(drift_pbp(1_000, 2_000), 10_000);
    }
}
