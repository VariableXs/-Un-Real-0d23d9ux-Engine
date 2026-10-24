#![allow(dead_code)]
//! VE-F3955 · 无障碍 fuzz 总闸
//!
//! 全域无障碍 fuzz 统一调度：八 fuzz 源归一（S01-S08 + 全域态）。双优先
//! 配额全域断言：安全 30% + 无障碍 20%（双配额全域红线）。不变量全域：
//! 断链/等价/覆盖/密码禁/标签禁/保底六不变量全域归一——全域违例分级
//! （降级→P0）；闭环全域（未闭环→阻断）。
//! 错误路径：配额破→修正；违例降级→P0；未闭环→阻断；源缺→补。
//! 性能：调度 O(八源)；配额 O(双)断言；不变量 O(全域)；回放 O(全域夜间)。
//! 跨批对接：F2487 总闸全域；F3315/F3955 双前例全域单源；全域语料对端。
//! 无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2A40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2A40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8OCode(pub u16);

impl S8OCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const QUOTA_BREACH: Self = Self(SEG_BASE + 1);
    pub const VIOLATION_DOWNGRADED: Self = Self(SEG_BASE + 2);
    pub const LOOP_OPEN: Self = Self(SEG_BASE + 3);
    pub const SOURCE_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::QUOTA_BREACH => "配额破→修正",
            Self::VIOLATION_DOWNGRADED => "违例降级→P0",
            Self::LOOP_OPEN => "未闭环→阻断",
            Self::SOURCE_GAP => "源缺→补",
            _ => "未知",
        }
    }
}

// ── 八 fuzz 源归一 ───────────────────────────────────────────────────

/// 八源：S01-S08 八组各一源 + 全域态并入门（全域态并入 S08 源计数）。
pub const FUZZ_SOURCES: [&str; 8] = [
    "S01", "S02", "S03", "S04", "S05", "S06", "S07", "S08+全域态",
];

/// 源调度记录：源名 → 案例数。
#[derive(Debug, Clone)]
pub struct SourceQuota {
    pub cases: [u32; 8],
}

/// 双配额全域断言：安全 30% ± 容差、无障碍 20% ± 容差。
pub const SECURE_QUOTA_PPM: u32 = 3_000;
pub const A11Y_QUOTA_PPM: u32 = 2_000;
pub const QUOTA_TOL_PPM: u32 = 200;

impl SourceQuota {
    pub fn new() -> Self {
        Self { cases: [0; 8] }
    }

    pub fn add(&mut self, src: usize, n: u32) -> S8OCode {
        if src >= 8 {
            return S8OCode::SOURCE_GAP;
        }
        self.cases[src] += n;
        S8OCode::OK
    }

    pub fn total(&self) -> u32 {
        self.cases.iter().sum()
    }

    /// 双配额占比核查（万分比）：源 0 = 安全源、源 7 = 无障碍源。
    pub fn quota_check(&self, secure_ppm: u32, a11y_ppm: u32) -> [bool; 2] {
        let t = self.total();
        if t == 0 {
            return [false, false];
        }
        let secure = self.cases[0] * 10_000 / t;
        let a11y = self.cases[7] * 10_000 / t;
        [
            secure.abs_diff(secure_ppm) <= QUOTA_TOL_PPM,
            a11y.abs_diff(a11y_ppm) <= QUOTA_TOL_PPM,
        ]
    }
}

impl Default for SourceQuota {
    fn default() -> Self {
        Self::new()
    }
}

// ── 六不变量全域 ─────────────────────────────────────────────────────

/// 六不变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invariant {
    /// 断链（公告断流）。
    NoBreak,
    /// 等价（替代路径等价）。
    Equiv,
    /// 覆盖（全域覆盖）。
    Cover,
    /// 密码禁（敏感输入禁播）。
    NoPasswordSpeak,
    /// 标签禁（内部标签禁外播）。
    NoLabelLeak,
    /// 保底（降级保底可用）。
    Fallback,
}

pub const INVARIANTS: [Invariant; 6] = [
    Invariant::NoBreak,
    Invariant::Equiv,
    Invariant::Cover,
    Invariant::NoPasswordSpeak,
    Invariant::NoLabelLeak,
    Invariant::Fallback,
];

/// 违例分级：P0 禁降级 / P1 / P2。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sev {
    P0,
    P1,
    P2,
}

/// 违例记录。
#[derive(Debug, Clone)]
pub struct Violation {
    pub inv: Invariant,
    pub sev: Sev,
    /// 案例指纹（回放锚）。
    pub case_fp: u64,
    /// 是否已闭环。
    pub closed: bool,
}

/// 违例总账：分级全域执法。
#[derive(Debug, Clone)]
pub struct ViolationBook {
    pub items: Vec<Violation>,
}

impl ViolationBook {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn report(&mut self, v: Violation) {
        self.items.push(v);
    }

    /// 降级检出：P0 违例被标 P1/P2 = 降级违例（禁）。
    pub fn downgraded(&self) -> Vec<u64> {
        self.items
            .iter()
            .filter(|v| matches!(v.inv, Invariant::NoBreak | Invariant::NoPasswordSpeak) && v.sev != Sev::P0)
            .map(|v| v.case_fp)
            .collect()
    }

    /// 修正：降级违例升回 P0。
    pub fn enforce_p0(&mut self) -> usize {
        let mut n = 0;
        for v in self.items.iter_mut() {
            if matches!(v.inv, Invariant::NoBreak | Invariant::NoPasswordSpeak) && v.sev != Sev::P0 {
                v.sev = Sev::P0;
                n += 1;
            }
        }
        n
    }

    /// 未闭环检出。
    pub fn open(&self) -> Vec<u64> {
        self.items.iter().filter(|v| !v.closed).map(|v| v.case_fp).collect()
    }

    /// 阻断判定：存在未闭环 P0 → 阻断。
    pub fn blocked(&self) -> bool {
        self.items.iter().any(|v| !v.closed && v.sev == Sev::P0)
    }

    pub fn close(&mut self, fp: u64) -> S8OCode {
        match self.items.iter_mut().find(|v| v.case_fp == fp && !v.closed) {
            Some(v) => {
                v.closed = true;
                S8OCode::OK
            }
            None => S8OCode::LOOP_OPEN,
        }
    }
}

impl Default for ViolationBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 源缺→补 ──────────────────────────────────────────────────────────

/// 源覆盖核查：八源全有案例才算归一。
pub fn source_gaps(q: &SourceQuota) -> Vec<usize> {
    (0..8).filter(|i| q.cases[*i] == 0).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves209_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves209_s08fuzz");

    // 01 八源清单守恒
    {
        chk(&mut set, "C3955-01", FUZZ_SOURCES.len() == 8 && FUZZ_SOURCES[0] == "S01" && FUZZ_SOURCES[7] == "S08+全域态", "err");
    }
    // 02 双配额达标
    {
        let mut q = SourceQuota::new();
        // 安全源 0 占 30%，无障碍源 7 占 20%，其余摊 50%。
        q.add(0, 300);
        q.add(7, 200);
        for i in 1..7 {
            q.add(i, 83);
        }
        let c = q.quota_check(SECURE_QUOTA_PPM, A11Y_QUOTA_PPM);
        chk(&mut set, "C3955-02", c[0] && c[1], "err");
    }
    // 03 配额破检出（安全不足）
    {
        let mut q = SourceQuota::new();
        q.add(0, 100);
        q.add(7, 200);
        for i in 1..7 {
            q.add(i, 116);
        }
        let c = q.quota_check(SECURE_QUOTA_PPM, A11Y_QUOTA_PPM);
        chk(&mut set, "C3955-03", !c[0] && c[1], "err");
    }
    // 04 六不变量清单守恒
    {
        chk(&mut set, "C3955-04", INVARIANTS.len() == 6 && Invariant::NoBreak as usize == 0 && Invariant::Fallback as usize == 5, "err");
    }
    // 05 降级检出：断链违例被标 P1
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::NoBreak, sev: Sev::P1, case_fp: 0xA1, closed: false });
        chk(&mut set, "C3955-05", b.downgraded() == vec![0xA1], "err");
    }
    // 06 修正升回 P0
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::NoPasswordSpeak, sev: Sev::P2, case_fp: 0xB2, closed: false });
        let n = b.enforce_p0();
        chk(&mut set, "C3955-06", n == 1 && b.items[0].sev == Sev::P0 && b.downgraded().is_empty(), "err");
    }
    // 07 非红线违例分级放行
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::Cover, sev: Sev::P1, case_fp: 0xC3, closed: false });
        chk(&mut set, "C3955-07", b.downgraded().is_empty() && !b.blocked(), "err");
    }
    // 08 未闭环 P0 → 阻断
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::NoBreak, sev: Sev::P0, case_fp: 0xD4, closed: false });
        chk(&mut set, "C3955-08", b.blocked() && b.open() == vec![0xD4], "err");
    }
    // 09 闭环后解除阻断
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::NoBreak, sev: Sev::P0, case_fp: 0xD4, closed: false });
        chk(&mut set, "C3955-09", b.close(0xD4) == S8OCode::OK && !b.blocked() && b.open().is_empty(), "err");
    }
    // 10 重复闭环拒收
    {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::Equiv, sev: Sev::P1, case_fp: 0xE5, closed: false });
        b.close(0xE5);
        chk(&mut set, "C3955-10", b.close(0xE5) == S8OCode::LOOP_OPEN, "err");
    }
    // 11 源缺→补实测
    {
        let mut q = SourceQuota::new();
        q.add(0, 10);
        q.add(7, 10);
        let gaps = source_gaps(&q);
        chk(&mut set, "C3955-11", gaps.len() == 6 && !gaps.contains(&0) && !gaps.contains(&7), "err");
    }
    // 12 八源归一后零缺口
    {
        let mut q = SourceQuota::new();
        for i in 0..8 {
            q.add(i, 10);
        }
        chk(&mut set, "C3955-12", source_gaps(&q).is_empty() && q.total() == 80, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3955-13", S8OCode::SOURCE_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8OCode::OK, S8OCode::QUOTA_BREACH, S8OCode::VIOLATION_DOWNGRADED, S8OCode::LOOP_OPEN, S8OCode::SOURCE_GAP];
        chk(&mut set, "C3955-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_fuzz_all_green() {
        let s = run_ves209_checks();
        assert!(s.all_passed(), "ves209 red: {}", s.red_items_text());
    }

    #[test]
    fn test_quota_extremes() {
        let q = SourceQuota::new();
        let c = q.quota_check(SECURE_QUOTA_PPM, A11Y_QUOTA_PPM);
        assert!(!c[0] && !c[1]); // 空账 = 不达标
    }

    #[test]
    fn test_violation_full_cycle() {
        let mut b = ViolationBook::new();
        b.report(Violation { inv: Invariant::NoLabelLeak, sev: Sev::P0, case_fp: 7, closed: false });
        assert!(b.blocked());
        b.close(7);
        assert!(!b.blocked());
        assert!(b.open().is_empty());
    }
}
