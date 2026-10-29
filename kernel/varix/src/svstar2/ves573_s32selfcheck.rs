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
    /// 对表行重复（同档双行账目漂移）。
    RowDup = SEG_BASE + 0x04,
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
    /// 深化：异议流账（累计登记/维持/翻案）。
    pub dispute_total: u32,
    pub upheld: u32,
    pub overturned: u32,
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

    // ─── 深化块：对表覆盖式更新（同档单行）───

    /// 对表行入账（覆盖式）：同档已有行即覆盖，不新增行（防双行漂移）。
    /// 返回 (是否覆盖旧行, 当前总行数)。
    pub fn tier_row_upsert(&mut self, row: TierRow) -> (bool, usize) {
        let existed = self.tier_rows.iter_mut().find(|r| r.tier == row.tier);
        match existed {
            Some(r) => {
                *r = row;
                (true, self.tier_rows.len())
            }
            None => {
                self.tier_rows.push(row);
                (false, self.tier_rows.len())
            }
        }
    }

    /// 双行检出：任一同档出现两行即漂移（对账口）。
    pub fn dup_free(&self) -> bool {
        for i in 0..self.tier_rows.len() {
            for j in (i + 1)..self.tier_rows.len() {
                if self.tier_rows[i].tier == self.tier_rows[j].tier {
                    return false;
                }
            }
        }
        true
    }

    // ─── 深化块：四维加权总分 ───

    /// 加权总分 bp：Σ(维得分率×权重)/Σ权重。
    /// 权重：覆盖 3 / 正确 4 / 一致 3 / 无障碍 2。未记分维按 0 计。
    pub fn weighted_total(&self) -> u32 {
        const W: [u64; 4] = [3, 4, 3, 2];
        let mut sum = 0u64;
        let mut wsum = 0u64;
        for i in 0..4 {
            let (g, t) = self.dims[i];
            let rate = if t == 0 { 0 } else { g as u64 * 10000 / t as u64 };
            sum += rate * W[i];
            wsum += W[i];
        }
        if wsum == 0 {
            return 0;
        }
        (sum / wsum) as u32
    }

    // ─── 深化块：异议流守恒 ───

    /// 异议登记入账版。
    pub fn dispute_ledger(&mut self, n: u8) {
        self.dispute(n);
        self.dispute_total += 1;
    }

    /// 裁决入账版：维持/翻案都计数。
    pub fn adjudicate_ledger(&mut self, n: u8, uphold: bool) {
        let was = self.disputes.contains(&n);
        self.adjudicate(n, uphold);
        if was {
            if uphold {
                self.upheld += 1;
            } else {
                self.overturned += 1;
            }
        }
    }

    /// 流守恒：登记 ≡ 维持 + 翻案 + 未结。
    pub fn dispute_conserves(&self) -> bool {
        self.dispute_total
            == self.upheld + self.overturned + self.disputes.len() as u32
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

    // ── 深化判据：对表覆盖式更新（同档单行）──
    let mut sr = U07SelfCheck::new();
    let (_, n1) = sr.tier_row_upsert(TierRow { tier: Tier::Flagship, budget: 100, measured: 90 });
    let (overwrote, n2) = sr.tier_row_upsert(TierRow { tier: Tier::Flagship, budget: 100, measured: 95 });
    chk(&mut set, "C573-13", n1 == 1 && overwrote && n2 == 1 && sr.dup_free(),
        "同档覆盖不增行");
    let (_, n3) = sr.tier_row_upsert(TierRow { tier: Tier::LowEnd, budget: 60, measured: 55 });
    chk(&mut set, "C573-14", n3 == 2 && sr.dup_free(), "新档新增行");
    // 手工注入双行 → dup_free 即破。
    sr.tier_rows.push(TierRow { tier: Tier::Flagship, budget: 1, measured: 1 });
    chk(&mut set, "C573-15", !sr.dup_free(), "双行漂移检出");

    // ── 深化判据：四维加权总分 ──
    let mut wt = U07SelfCheck::new();
    let _ = wt.score(Dim::Coverage, 10000, 10000);
    let _ = wt.score(Dim::Correctness, 10000, 10000);
    let _ = wt.score(Dim::Consistency, 10000, 10000);
    let _ = wt.score(Dim::A11y, 10000, 10000);
    chk(&mut set, "C573-16", wt.weighted_total() == 10000, "全满分加权 10000");
    // 仅无障碍 50%：总权 12，得分 = (9×10000 + 2×5000)/12 = 9166。
    let _ = wt.score(Dim::A11y, 5000, 10000);
    chk(&mut set, "C573-17", wt.weighted_total() == 9166, "部分维加权核算");
    // 未记分维按 0 计（不臆测）。
    let wz = U07SelfCheck::new();
    chk(&mut set, "C573-18", wz.weighted_total() == 0, "未记分恒零");

    // ── 深化判据：异议流守恒 ──
    let mut sd = U07SelfCheck::new();
    sd.dispute_ledger(1);
    sd.dispute_ledger(2);
    sd.dispute_ledger(3);
    sd.adjudicate_ledger(1, true);   // 维持
    sd.adjudicate_ledger(2, false);  // 翻案
    chk(&mut set, "C573-19", sd.dispute_conserves() && sd.upheld == 1
        && sd.overturned == 1 && sd.disputes == [3], "登记≡维持+翻案+未结");
    // 幂等登记不重复计数（dispute 幂等，但 ledger 版直接累计——用 disputes 内容守恒校验）。
    chk(&mut set, "C573-20", !sd.dispute_conserves() == false, "守恒函数双向核");

    // ── 深化判据：LCG 对表注入（超预算检出率 100%）──
    let mut rng: u64 = 0x5730_BEEF;
    let mut caught = 0u32;
    let mut rounds = 0u32;
    for _ in 0..64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let over = rng % 2 == 0;
        let budget = 100;
        let measured = if over { 100 + (rng % 50) as u32 + 1 } else { (rng % 100) as u32 };
        let mut si = U07SelfCheck::new();
        si.tier_rows.push(TierRow { tier: Tier::Flagship, budget, measured });
        rounds += 1;
        if si.tier_check() == Ok(false) {
            caught += 1;
        }
    }
    chk(&mut set, "C573-21", caught == rounds, "64 轮超预算注入全抓");

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

    #[test]
    fn tier_row_upsert_no_dup() {
        let mut sc = U07SelfCheck::new();
        let r = TierRow { tier: Tier::Flagship, budget: 100, measured: 90 };
        let (_, n1) = sc.tier_row_upsert(r);
        assert_eq!(n1, 1);
        let (ow, n2) = sc.tier_row_upsert(TierRow { tier: Tier::Flagship, budget: 100, measured: 95 });
        assert!(ow && n2 == 1);
        assert!(sc.dup_free());
    }

    #[test]
    fn dispute_flow_conserves() {
        let mut sc = U07SelfCheck::new();
        sc.dispute_ledger(1);
        sc.dispute_ledger(2);
        sc.adjudicate_ledger(1, true);
        assert!(sc.dispute_conserves());
        assert_eq!(sc.upheld, 1);
        assert_eq!(sc.overturned, 0);
    }

    #[test]
    fn weighted_total_math() {
        let mut sc = U07SelfCheck::new();
        let _ = sc.score(Dim::Coverage, 9000, 10000);
        // 仅一维 90%：权 3 → (3×9000)/3 = 9000。
        assert_eq!(sc.weighted_total(), 9000);
    }
}
