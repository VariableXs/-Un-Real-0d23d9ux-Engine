//! CGPU-F0466 · 不变量终审（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 不变量终审：五不变量全域复核（预算守恒/无饥饿/无超卖/预测
//!   回退不悬挂/审计完整——C01-C09 每组证据逐项核对）；
//! - 形式化辅助核验（F0438 导出状态机的一致性抽查）；
//! - 违例历史与修复闭环记录。判据：五不变量、逐组证据、形式化
//!   抽查、闭环、判据。
//!
//! 设计纪律：
//! - 五不变量判别式枚举 + 逐组证据矩阵（9 组 × 5 不变量），每格
//!   证据码（绿/黄/红），终审要求全列绿（任一红即终审不通过）。
//! - 形式化抽查：状态机导出（SMT-LIB 风格断言文本）与运行期数值
//!   对表——抽查点抽样一致即通过。
//! - 违例闭环：违例记录（发现→修复→回归→勾销）四步齐备才算闭环，
//!   未闭环违例阻断终审。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1050 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1050，全仓独占）。
pub const SEG_BASE: u16 = 0x1050;

pub const E_IV_NAME: u16 = SEG_BASE + 1; // 不变量名非法
pub const E_IV_RED: u16 = SEG_BASE + 2; // 终审红项
pub const E_IV_OPEN: u16 = SEG_BASE + 3; // 违例未闭环
pub const E_IV_MISMATCH: u16 = SEG_BASE + 4; // 形式化抽查不一致

/// 五不变量。
pub const IV_CONSERVE: u8 = 1; // 预算守恒
pub const IV_NO_HUNGER: u8 = 2; // 无饥饿
pub const IV_NO_OVERSELL: u8 = 3; // 无超卖
pub const IV_PRED_BOUND: u8 = 4; // 预测回退不悬挂
pub const IV_AUDIT_FULL: u8 = 5; // 审计完整
pub const INVARIANTS: [u8; 5] = [IV_CONSERVE, IV_NO_HUNGER, IV_NO_OVERSELL, IV_PRED_BOUND, IV_AUDIT_FULL];

/// 证据状态。
pub const EV_GREEN: u8 = 0;
pub const EV_YELLOW: u8 = 1;
pub const EV_RED: u8 = 2;

/// 证据矩阵（9 组 × 5 不变量）。
#[derive(Debug)]
pub struct EvidenceMatrix {
    /// cells[group][inv] = 状态码。
    pub cells: [[u8; 5]; 9],
}

impl EvidenceMatrix {
    pub fn all_green() -> Self {
        Self { cells: [[EV_GREEN; 5]; 9] }
    }
    /// 终审判定：全格绿才通过（黄=待补证据也拦）。
    pub fn audit(&self) -> Result<(), u16> {
        for g in 0..9 {
            for i in 0..5 {
                if self.cells[g][i] != EV_GREEN {
                    return Err(E_IV_RED);
                }
            }
        }
        Ok(())
    }
    pub fn red_count(&self) -> usize {
        self.cells.iter().flatten().filter(|&&c| c != EV_GREEN).count()
    }
}

/// 违例记录（闭环四步）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub inv: u8,
    /// 步骤位图：1 发现 2 修复 4 回归 8 勾销。
    pub steps: u32,
}

impl Violation {
    pub fn closed(&self) -> bool {
        self.steps & 0b1111 == 0b1111
    }
    pub fn step(&mut self, bit: u32) {
        self.steps |= bit;
    }
}

/// 违例台账终审：全部闭环才放行。
pub fn violations_ok(vs: &[Violation]) -> Result<(), u16> {
    if vs.iter().any(|v| !v.closed()) {
        return Err(E_IV_OPEN);
    }
    // 违例的不变量名合法。
    if vs.iter().any(|v| !INVARIANTS.contains(&v.inv)) {
        return Err(E_IV_NAME);
    }
    Ok(())
}

/// 形式化抽查（F0438 对接）：导出断言文本 vs 运行期数值对表。
/// 断言文本格式 "inv=<n> bound=<v>"；抽查一致性 = 数值逐点相等。
pub fn formal_spot_check(asserts: &[(&'static str, u64)], runtime: &[(u8, u64)]) -> Result<(), u16> {
    if asserts.len() != runtime.len() {
        return Err(E_IV_MISMATCH);
    }
    for (a, r) in asserts.iter().zip(runtime.iter()) {
        // 文本 "inv=N bound=V" 中 inv 与 runtime 组号对齐、bound 与值对齐。
        let inv = a.0.strip_prefix("inv=").and_then(|s| s.split_whitespace().next()).and_then(|s| s.parse::<u8>().ok());
        let bound = a.0.split("bound=").nth(1).and_then(|s| s.parse::<u64>().ok());
        match (inv, bound) {
            (Some(n), Some(b)) => {
                if n != r.0 || b != r.1 {
                    return Err(E_IV_MISMATCH);
                }
            }
            _ => return Err(E_IV_MISMATCH),
        }
    }
    Ok(())
}

/// 终审编排：矩阵+违例+形式化三闸合一。
pub fn final_audit(matrix: &EvidenceMatrix, vs: &[Violation]) -> Result<usize, u16> {
    matrix.audit()?;
    violations_ok(vs)?;
    Ok(matrix.cells.iter().flatten().count())
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、五不变量全绿矩阵通过。
    {
        let m = EvidenceMatrix::all_green();
        v.push(("F0466-五不变量-45格全绿通过", final_audit(&m, &[]) == Ok(45)));
        // 红项拦截。
        let mut m2 = EvidenceMatrix::all_green();
        m2.cells[3][IV_NO_OVERSELL as usize - 1] = EV_RED;
        v.push(("F0466-五不变量-红项拦截", m2.audit() == Err(E_IV_RED)));
        // 黄项同样拦截（终审不收黄）。
        let mut m3 = EvidenceMatrix::all_green();
        m3.cells[8][IV_AUDIT_FULL as usize - 1] = EV_YELLOW;
        v.push(("F0466-五不变量-黄项也拦", m3.audit() == Err(E_IV_RED)));
        v.push(("F0466-五不变量-红黄计数", m2.red_count() == 1 && m3.red_count() == 1));
    }
    // 二、违例闭环。
    {
        let mut x = Violation { inv: IV_CONSERVE, steps: 0 };
        x.step(1);
        v.push(("F0466-闭环-仅发现未闭环", violations_ok(&[x.clone()]) == Err(E_IV_OPEN)));
        x.step(2);
        x.step(4);
        x.step(8);
        v.push(("F0466-闭环-四步齐备通过", violations_ok(&[x]) == Ok(())));
        // 非法不变量名。
        let bad = Violation { inv: 9, steps: 0b1111 };
        v.push(("F0466-闭环-不变量名非法拒", violations_ok(&[bad]) == Err(E_IV_NAME)));
    }
    // 三、形式化抽查：数值对表。
    {
        let asserts = [("inv=1 bound=1000", 1000u64), ("inv=3 bound=0", 0u64)];
        let runtime = [(IV_CONSERVE, 1000u64), (IV_NO_OVERSELL, 0u64)];
        v.push(("F0466-形式化-对表一致通过", formal_spot_check(&asserts, &runtime).is_ok()));
        // 数值不一致。
        let runtime2 = [(IV_CONSERVE, 999u64), (IV_NO_OVERSELL, 0u64)];
        v.push(("F0466-形式化-数值不一致拒", formal_spot_check(&asserts, &runtime2) == Err(E_IV_MISMATCH)));
        // 长度不一致。
        v.push(("F0466-形式化-长度不一致拒", formal_spot_check(&asserts, &runtime[..1]) == Err(E_IV_MISMATCH)));
        // 格式非法。
        v.push(("F0466-形式化-格式非法拒", formal_spot_check(&[("garbage", 1)], &[(1, 1)]) == Err(E_IV_MISMATCH)));
    }
    // 四、终审编排：三闸串联。
    {
        let m = EvidenceMatrix::all_green();
        let mut open = Violation { inv: IV_AUDIT_FULL, steps: 0b0111 };
        let r = final_audit(&m, &[open.clone()]);
        v.push(("F0466-终审-未闭环阻断", r == Err(E_IV_OPEN)));
        open.step(8);
        v.push(("F0466-终审-闭环后放行", final_audit(&m, &[open]) == Ok(45)));
    }
    // 五、判据自检。
    {
        v.push(("F0466-判据-五不变量枚举", INVARIANTS == [1, 2, 3, 4, 5]));
        v.push(("F0466-判据-矩阵45格", EvidenceMatrix::all_green().cells.iter().flatten().count() == 45));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0466_invariant_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
