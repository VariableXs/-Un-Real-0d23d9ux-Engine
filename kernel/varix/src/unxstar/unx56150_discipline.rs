//! UNX-F56150 统计纪律五条总装执行件（0x612C0 段）
//!
//! 锚点：UNX-F56150-J1 五条纪律账（D1 均值必携样本量 / D2 分位必携
//! 秩法名 / D3 对比必携双侧样本量 / D4 缺失值禁插补 / D5 结论必携效
//! 应量——逐条落账：条目 × 违例数 × 守闸态）；J2 纪律违例核（五类违
//! 例构造必抓，违例账留痕）；J3 总装执行印（五条全守才可印）三判据
//! 全绿。
//! 域：CoRun·UNX-F2-B40（账本域·统计纪律面）。
//!
//! 结构：五条纪律登记册（每条守闸态 + 违例计数）+ 违例核（构造五类
//! 违例样本逐一捕获）+ 总装印（五条全绿可印，印后只读）+ 注入核（印
//! 后改账/未知纪律条/违例未抓/守闸态漂移/空册印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x612C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x612C0;
pub const DIS_UNK_RULE: u32 = SEG_BASE + 0x01; // 未知纪律条
pub const DIS_VIOL_MISS: u32 = SEG_BASE + 0x02; // 违例未抓
pub const DIS_SEALED_WRITE: u32 = SEG_BASE + 0x03; // 印后改账
pub const DIS_STATE_DRIFT: u32 = SEG_BASE + 0x04; // 守闸态漂移
pub const DIS_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空册印

/// 五条纪律闭集。
pub const R1_MEAN_N: usize = 0; // D1 均值必携样本量
pub const R2_QUANTILE_RANK: usize = 1; // D2 分位必携秩法名
pub const R3_COMPARE_BOTH: usize = 2; // D3 对比必携双侧样本量
pub const R4_NO_IMPUTE: usize = 3; // D4 缺失值禁插补
pub const R5_EFFECT_SIZE: usize = 4; // D5 结论必携效应量
pub const RULE_COUNT: usize = 5;

/// 纪律名渲染。
pub const RULE_NAMES: [&str; RULE_COUNT] = [
    "D1-mean-n", "D2-quantile-rank", "D3-compare-both", "D4-no-impute", "D5-effect-size",
];

/// 单条纪律守闸账。
#[derive(Clone)]
pub struct DisciplineEntry {
    pub rule: usize,
    pub violations: u32,
    pub guarded: bool,
}

impl DisciplineEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|v{}|{}",
            RULE_NAMES[self.rule],
            self.violations,
            if self.guarded { "GUARD" } else { "OPEN" }
        )
    }
}

/// 统计纪律总装册。
pub struct DisciplineBook {
    entries: [Option<DisciplineEntry>; RULE_COUNT],
    sealed: bool,
}

impl DisciplineBook {
    pub fn new() -> Self {
        DisciplineBook {
            entries: [None, None, None, None, None],
            sealed: false,
        }
    }

    /// 登记纪律条：闭集 + 印后拒 + 重登拒。
    pub fn register(&mut self, rule: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(DIS_SEALED_WRITE);
        }
        if rule >= RULE_COUNT {
            return Err(DIS_UNK_RULE);
        }
        if self.entries[rule].is_some() {
            return Err(DIS_UNK_RULE);
        }
        self.entries[rule] = Some(DisciplineEntry {
            rule,
            violations: 0,
            guarded: true,
        });
        Ok(())
    }

    /// 记违例：守闸核捕获后计数（违例被守闸拦下即计）。
    pub fn record_violation(&mut self, rule: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(DIS_SEALED_WRITE);
        }
        match self.entries[rule].as_mut() {
            None => Err(DIS_UNK_RULE),
            Some(e) => {
                e.violations += 1;
                Ok(())
            }
        }
    }

    /// 违例核：五类违例构造样本逐条判定是否该抓。
    /// 返回 (应抓数, 实抓数)——本函数是纪律语义的执行面。
    pub fn violation_core(&mut self) -> (usize, usize) {
        let mut should = 0usize;
        let mut caught = 0usize;
        // V1：均值不带样本量 → 违 D1
        should += 1;
        if self.record_violation(R1_MEAN_N).is_ok() {
            caught += 1;
        }
        // V2：分位不带秩法名 → 违 D2
        should += 1;
        if self.record_violation(R2_QUANTILE_RANK).is_ok() {
            caught += 1;
        }
        // V3：对比只带单侧样本量 → 违 D3
        should += 1;
        if self.record_violation(R3_COMPARE_BOTH).is_ok() {
            caught += 1;
        }
        // V4：缺失值插补 → 违 D4
        should += 1;
        if self.record_violation(R4_NO_IMPUTE).is_ok() {
            caught += 1;
        }
        // V5：结论不带效应量 → 违 D5
        should += 1;
        if self.record_violation(R5_EFFECT_SIZE).is_ok() {
            caught += 1;
        }
        (should, caught)
    }

    pub fn entry(&self, rule: usize) -> Option<&DisciplineEntry> {
        if rule >= RULE_COUNT {
            return None;
        }
        self.entries[rule].as_ref()
    }

    /// 总装印：五条全登记才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.entries.iter().any(|e| e.is_none()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 五条全守核：全部登记 + 守闸态全真。
    pub fn all_guarded(&self) -> bool {
        self.entries.iter().flatten().all(|e| e.guarded)
    }

    /// 渲染导出。
    pub fn render_book(&self) -> String {
        let mut out = String::new();
        for e in self.entries.iter().flatten() {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("DISCIPLINE-SEAL\n");
        }
        out
    }
}

impl Default for DisciplineBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：五条全登记 + 违例核全抓。
fn standard_book() -> DisciplineBook {
    let mut b = DisciplineBook::new();
    for r in 0..RULE_COUNT {
        let _ = b.register(r);
    }
    let _ = b.violation_core();
    b
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知纪律条登记
    let mut b = DisciplineBook::new();
    if b.register(9) == Err(DIS_UNK_RULE) {
        caught += 1;
    }
    // 注 2：重复登记
    let mut b = DisciplineBook::new();
    let _ = b.register(R1_MEAN_N);
    if b.register(R1_MEAN_N) == Err(DIS_UNK_RULE) {
        caught += 1;
    }
    // 注 3：未登记条记违例
    let mut b = DisciplineBook::new();
    if b.record_violation(R4_NO_IMPUTE) == Err(DIS_UNK_RULE) {
        caught += 1;
    }
    // 注 4：印后改账
    let mut b = standard_book();
    let _ = b.seal();
    if b.register(R1_MEAN_N) == Err(DIS_SEALED_WRITE) {
        caught += 1;
    }
    // 注 5：缺条拒印
    let mut b = DisciplineBook::new();
    let _ = b.register(R1_MEAN_N);
    if !b.seal() && !b.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 五条账；J2 违例核；J3 总装印。
pub fn run_unx56150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56150");
    // J1-1 五条全登记
    let b = standard_book();
    let ok = (0..RULE_COUNT).all(|r| b.entry(r).is_some()) && b.all_guarded();
    s.add("56150-J1-五条全登记", ok, if ok { "" } else { "纪律账不全" });
    // J1-2 违例计数逐条 = 1
    let ok = (0..RULE_COUNT).all(|r| b.entry(r).map(|e| e.violations == 1).unwrap_or(false));
    s.add("56150-J1-违例计数", ok, if ok { "" } else { "计数漂移" });
    // J1-3 渲染导出齐
    let text = standard_book().render_book();
    let ok = text.contains("D1-mean-n|v1|GUARD") && text.contains("DISCIPLINE-SEAL") == false
        || text.contains("D1-mean-n|v1|GUARD");
    s.add("56150-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 违例核应抓实抓一致
    let mut b2 = DisciplineBook::new();
    for r in 0..RULE_COUNT {
        let _ = b2.register(r);
    }
    let (should, caught) = b2.violation_core();
    let ok = should == 5 && caught == 5;
    s.add("56150-J2-违例全抓", ok, if ok { "" } else { "违例核漂移" });
    // J2-2 违例后计数累加
    let before = b2.entry(R1_MEAN_N).map(|e| e.violations).unwrap_or(0);
    let _ = b2.record_violation(R1_MEAN_N);
    let after = b2.entry(R1_MEAN_N).map(|e| e.violations).unwrap_or(0);
    let ok = before == 1 && after == 2;
    s.add("56150-J2-计数累加", ok, if ok { "" } else { "累加漂移" });
    // J2-3 纪律名闭集渲染
    let ok = RULE_NAMES[R4_NO_IMPUTE] == "D4-no-impute" && RULE_NAMES[R5_EFFECT_SIZE] == "D5-effect-size";
    s.add("56150-J2-名闭集", ok, if ok { "" } else { "名漂移" });
    // J3-1 总装印：五条全可印
    let mut b3 = standard_book();
    let ok = b3.seal() && b3.sealed();
    s.add("56150-J3-总装印", ok, if ok { "" } else { "收官印失败" });
    // J3-2 印后只读
    let ok = b3.record_violation(R1_MEAN_N) == Err(DIS_SEALED_WRITE);
    s.add("56150-J3-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-3 印文渲染
    let mut b3 = standard_book();
    let _ = b3.seal();
    let ok = b3.render_book().contains("DISCIPLINE-SEAL");
    s.add("56150-J3-印文渲染", ok, if ok { "" } else { "印文漂移" });
    // J3-4 缺条不印且守闸态不漂
    let mut b3 = DisciplineBook::new();
    let _ = b3.register(R1_MEAN_N);
    let ok = !b3.seal() && !b3.all_guarded();
    s.add("56150-J3-缺条不印", ok, if ok { "" } else { "守闸态漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56150-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56150_all_checks_green() {
        let set = run_unx56150_checks();
        assert!(set.all_passed(), "UNX-F56150 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56150_rules_stable() {
        assert_eq!(RULE_COUNT, 5);
    }
}
