//! VE-F4339 · U07 预备自查（U07 性能与预算组）
//! 职责：U07 收口前自查——四维自查（覆盖/正确/一致/无障碍同前组口径）
//! + 本组特查：预算三档与实测一致 + 降级梯度可回——逐档实测对表 + 回修
//! 重扫 + 特查异议复核 + 指针悬空修正。
//! 段位：0x3AE0（续表 idx573）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3AE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsCode {
    /// 自查不达标（回修重扫）。
    NotReady = SEG_BASE,
    /// 特查异议（复核）。
    Dispute = SEG_BASE + 0x01,
    /// 档位越界。
    TierRange = SEG_BASE + 0x02,
    /// 指针悬空（修正）。
    DanglingPtr = SEG_BASE + 0x03,
}

/// 四维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim {
    Coverage,
    Correctness,
    Consistency,
    A11y,
}

/// 三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Flagship,
    Mainstream,
    LowEnd,
}

/// 档位实测对表行：(档, 预算, 实测)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TierRow {
    pub tier: Tier,
    pub budget: u32,
    pub measured: u32,
}

/// 特查记录：降级梯度可回（档 i 可回到档 i-1）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradRecord {
    pub from: u8,
    pub to: u8,
    pub ok: bool,
}

/// 自查台。
#[derive(Default)]
pub struct U07SelfCheck {
    pub dims: [(u32, u32); 4],
    pub tier_rows: Vec<TierRow>,
    pub grads: Vec<GradRecord>,
    pub disputes: Vec<u8>,
    pub rescans: u32,
    pub ptr_fixes: u32,
}

/// 四维通过线。
pub const DIM_PASS_BP: u32 = 9000;

impl U07SelfCheck {
    pub fn new() -> Self {
        U07SelfCheck { dims: [(0, 10000); 4], ..Default::default() }
    }

    /// 四维记分（超满分钳制）。
    pub fn score(&mut self, d: Dim, got: u32, total: u32) -> Result<(), UsCode> {
        let i = match d {
            Dim::Coverage => 0,
            Dim::Correctness => 1,
            Dim::Consistency => 2,
            Dim::A11y => 3,
        };
        if total == 0 {
            return Err(UsCode::NotReady);
        }
        self.dims[i] = (got.min(total), total);
        Ok(())
    }

    pub fn dims_ready(&self) -> bool {
        self.dims.iter().all(|(g, t)| *t > 0 && (*g as u64 * 10000 / *t as u64) >= DIM_PASS_BP as u64)
    }

    /// 特查一：预算三档与实测一致（实测 ≤ 预算逐档过表；缺档 = 不达标）。
    pub fn tier_check(&self) -> Result<bool, UsCode> {
        for t in [Tier::Flagship, Tier::Mainstream, Tier::LowEnd] {
            let r = self.tier_rows.iter().find(|r| r.tier == t).ok_or(UsCode::NotReady)?;
            if r.measured > r.budget {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 特查二：降级梯度可回（相邻档记录全过且覆盖 0↔1↔2 桥）。
    pub fn grad_check(&self) -> Result<bool, UsCode> {
        for pair in [(0u8, 1u8), (1, 2)] {
            let g = self
                .grads
                .iter()
                .find(|g| (g.from, g.to) == pair)
                .ok_or(UsCode::NotReady)?;
            if !g.ok {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 特查异议登记（幂等计数）。
    pub fn dispute(&mut self, n: u8) {
        if !self.disputes.contains(&n) {
            self.disputes.push(n);
        }
    }

    /// 复核裁决：维持清异 / 翻案重扫。
    pub fn adjudicate(&mut self, n: u8, uphold: bool) {
        if uphold {
            self.disputes.retain(|x| *x != n);
        } else {
            self.rescans += 1;
        }
    }

    /// 指针悬空修正（计数留痕）。
    pub fn fix_dangling(&mut self) {
        self.ptr_fixes += 1;
    }

    /// 就绪总闸：四维达标 + 三档对表过 + 梯度可回 + 无未结异议。
    pub fn ready(&self) -> bool {
        self.dims_ready()
            && self.tier_check() == Ok(true)
            && self.grad_check() == Ok(true)
            && self.disputes.is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves573_checks() -> CheckSet {
    let mut set = CheckSet::new("ves573");
    let mut sc = U07SelfCheck::new();

    // C573-01 四维记分达标。
    let _ = sc.score(Dim::Coverage, 9200, 10000);
    let _ = sc.score(Dim::Correctness, 9600, 10000);
    let _ = sc.score(Dim::Consistency, 9000, 10000);
    let _ = sc.score(Dim::A11y, 9900, 10000);
    chk(&mut set, "C573-01", sc.dims_ready(), "四维全达标");

    // C573-02 维度不足检出 + 零满分拒。
    let _ = sc.score(Dim::A11y, 8000, 10000);
    chk(&mut set, "C573-02", !sc.dims_ready() && sc.score(Dim::A11y, 1, 0).is_err(), "不足维+零满分拒");

    // C573-03 特查一：三档对表缺档 = 不达标。
    chk(&mut set, "C573-03", sc.tier_check() == Err(UsCode::NotReady), "缺档不达标");

    // C573-04 三档实测全过（预算一致）。
    sc.tier_rows.push(TierRow { tier: Tier::Flagship, budget: 100, measured: 90 });
    sc.tier_rows.push(TierRow { tier: Tier::Mainstream, budget: 200, measured: 180 });
    sc.tier_rows.push(TierRow { tier: Tier::LowEnd, budget: 60, measured: 55 });
    chk(&mut set, "C573-04", sc.tier_check() == Ok(true), "三档实测过表");

    // C573-05 超预算档检出。
    sc.tier_rows[2].measured = 70;
    chk(&mut set, "C573-05", sc.tier_check() == Ok(false), "超预算检出");

    // C573-06 特查二：梯度桥缺记录 = 不达标。
    chk(&mut set, "C573-06", sc.grad_check() == Err(UsCode::NotReady), "梯度桥缺");

    // C573-07 梯度可回全过。
    sc.grads.push(GradRecord { from: 0, to: 1, ok: true });
    sc.grads.push(GradRecord { from: 1, to: 2, ok: true });
    chk(&mut set, "C573-07", sc.grad_check() == Ok(true), "梯度可回全过");

    // C573-08 梯度断桥检出。
    sc.grads[1].ok = false;
    chk(&mut set, "C573-08", sc.grad_check() == Ok(false), "断桥检出");
    sc.grads[1].ok = true;

    // C573-09 特查异议 + 复核维持清异。
    sc.dispute(3);
    sc.dispute(3); // 幂等
    chk(&mut set, "C573-09", sc.disputes == [3], "异议幂等登记");
    sc.adjudicate(3, true);
    chk(&mut set, "C573-09b", sc.disputes.is_empty(), "维持清异");

    // C573-10 翻案重扫计数。
    sc.dispute(5);
    sc.adjudicate(5, false);
    chk(&mut set, "C573-10", sc.rescans == 1 && sc.disputes == [5], "翻案重扫");

    // C573-11 就绪总闸（翻案异议未清不放行）。
    chk(&mut set, "C573-11", !sc.ready(), "未清不放行");

    // C573-12 指针悬空修正 + 恢复就绪（超满分钳制不虚报）。
    let _ = sc.score(Dim::A11y, 9900, 10000);
    let _ = sc.score(Dim::A11y, 20000, 10000);
    sc.fix_dangling();
    sc.disputes.clear();
    chk(&mut set, "C573-12", sc.dims[3].0 == 10000 && sc.ready() && sc.ptr_fixes == 1, "修正+钳制+就绪");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_table_gate() {
        let mut sc = U07SelfCheck::new();
        assert_eq!(sc.tier_check(), Err(UsCode::NotReady));
        for (t, b, m) in [(Tier::Flagship, 100, 100), (Tier::Mainstream, 200, 200), (Tier::LowEnd, 60, 60)] {
            sc.tier_rows.push(TierRow { tier: t, budget: b, measured: m });
        }
        assert_eq!(sc.tier_check(), Ok(true)); // 边界达标
        sc.tier_rows[0].measured = 101;
        assert_eq!(sc.tier_check(), Ok(false));
    }

    #[test]
    fn grad_bridge() {
        let mut sc = U07SelfCheck::new();
        assert_eq!(sc.grad_check(), Err(UsCode::NotReady));
        sc.grads.push(GradRecord { from: 0, to: 1, ok: true });
        assert_eq!(sc.grad_check(), Err(UsCode::NotReady)); // 还缺 1↔2
        sc.grads.push(GradRecord { from: 1, to: 2, ok: true });
        assert_eq!(sc.grad_check(), Ok(true));
    }
}
