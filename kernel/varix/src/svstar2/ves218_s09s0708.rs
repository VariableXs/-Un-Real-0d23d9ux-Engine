//! VE-F3964 · S07-S08 组自查核验
//!
//! 两组逐项核验：S07 无障碍性能五基准复测（渲染增量/色标切换/快照生成/
//! 朗读排队/工具开销五档阈值）；S08 测试工具五标准复审（快/稳/可 diff/
//! 可集成/可扩展）+ 审查器误报率复测——证据引用制执行。
//! 错误路径：基准越阈→立案+预算重议；工具不达标准→缺陷立案；误报超标
//! →规则回修。
//! 性能：复测 O(基准)；审 O(标准)；误报 O(样本)。
//! 跨批对接：S07-S08 档案单源；F3965 上游；F3967-F3968 资产归集。
//! 无障碍与隐私：工具自查含读屏仿真隐私沙箱复述；录证脱敏复核。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x30C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x30C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9DCode(pub u16);

impl S9DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const BENCH_OVER: Self = Self(SEG_BASE + 1);
    pub const TOOL_DEFECT: Self = Self(SEG_BASE + 2);
    pub const FALSE_POSITIVE: Self = Self(SEG_BASE + 3);
    pub const SANDBOX_GAP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::BENCH_OVER => "基准越阈→立案+预算重议",
            Self::TOOL_DEFECT => "工具不达标准→缺陷立案",
            Self::FALSE_POSITIVE => "误报超标→规则回修",
            Self::SANDBOX_GAP => "沙箱缺→补",
            _ => "未知",
        }
    }
}

// ── S07 五基准复测 ───────────────────────────────────────────────────

/// 五基准名。
pub const BENCH_NAMES: [&str; 5] = ["渲染增量", "色标切换", "快照生成", "朗读排队", "工具开销"];

/// 五档阈值（毫秒，万分比整型口径直接用 ms）。
pub const BENCH_THRESHOLDS_MS: [u32; 5] = [2, 50, 100, 80, 5];

/// 基准复测行。
#[derive(Debug, Clone)]
pub struct BenchRow {
    pub name: &'static str,
    /// 实测毫秒。
    pub measured_ms: u32,
}

/// S07 复测器。
#[derive(Debug, Clone)]
pub struct S07Retest {
    pub rows: Vec<BenchRow>,
}

impl S07Retest {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn submit(&mut self, name: &'static str, measured_ms: u32) -> S9DCode {
        let idx = match BENCH_NAMES.iter().position(|n| *n == name) {
            Some(i) => i,
            None => return S9DCode::BENCH_OVER,
        };
        self.rows.push(BenchRow { name, measured_ms });
        if measured_ms > BENCH_THRESHOLDS_MS[idx] {
            return S9DCode::BENCH_OVER;
        }
        S9DCode::OK
    }

    /// 越阈清单（立案+预算重议）。
    pub fn overs(&self) -> Vec<&'static str> {
        self.rows
            .iter()
            .filter(|r| {
                let idx = BENCH_NAMES.iter().position(|n| *n == r.name).unwrap_or(0);
                r.measured_ms > BENCH_THRESHOLDS_MS[idx]
            })
            .map(|r| r.name)
            .collect()
    }

    pub fn all_green(&self) -> bool {
        self.rows.len() == 5 && self.overs().is_empty()
    }

    /// 预算重议幅度：最大超阈倍率（万分比）。
    pub fn max_over_ppm(&self) -> u32 {
        self.rows
            .iter()
            .map(|r| {
                let idx = BENCH_NAMES.iter().position(|n| *n == r.name).unwrap_or(0);
                let t = BENCH_THRESHOLDS_MS[idx].max(1);
                r.measured_ms.saturating_sub(BENCH_THRESHOLDS_MS[idx]) * 10_000 / t
            })
            .max()
            .unwrap_or(0)
    }
}

impl Default for S07Retest {
    fn default() -> Self {
        Self::new()
    }
}

// ── S08 五标准复审 + 误报率 ──────────────────────────────────────────

/// 五标准（同 F3941 单源复用）。
pub const TOOL_STANDARDS: [&str; 5] = ["快", "稳", "可diff", "可集成", "可扩展"];

/// 工具复审行。
#[derive(Debug, Clone)]
pub struct ToolRow {
    pub tool: &'static str,
    pub pass: [bool; 5],
    /// 隐私沙箱合规（读屏仿真类必须 true）。
    pub sandbox_ok: bool,
    /// 是否读屏仿真类（沙箱强制档）。
    pub needs_sandbox: bool,
}

impl ToolRow {
    /// 缺陷立案：任一标准不过。
    pub fn defect(&self) -> Option<&'static str> {
        for (i, s) in TOOL_STANDARDS.iter().enumerate() {
            if !self.pass[i] {
                return Some(s);
            }
        }
        None
    }

    /// 沙箱缺检出。
    pub fn sandbox_gap(&self) -> bool {
        self.needs_sandbox && !self.sandbox_ok
    }
}

/// 审查器误报率复测：样本级统计（万分比）。
#[derive(Debug, Clone)]
pub struct FalsePositive {
    /// 总样本。
    pub samples: u32,
    /// 误报数。
    pub false_alarms: u32,
    /// 容许上限（万分比）。
    pub limit_ppm: u32,
}

pub const FP_LIMIT_PPM: u32 = 500;

impl FalsePositive {
    pub fn rate_ppm(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        self.false_alarms * 10_000 / self.samples
    }

    /// 误报超标判定。
    pub fn over(&self) -> bool {
        self.rate_ppm() > FP_LIMIT_PPM
    }

    /// 规则回修幅度：需削减的误报数。
    pub fn to_fix(&self) -> u32 {
        if !self.over() {
            return 0;
        }
        let allow = self.limit_ppm * self.samples / 10_000;
        self.false_alarms.saturating_sub(allow)
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves218_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves218_s09s0708");

    // 01 五基准清单守恒
    {
        chk(&mut set, "C3964-01", BENCH_NAMES.len() == 5 && BENCH_NAMES[0] == "渲染增量" && BENCH_NAMES[4] == "工具开销" && BENCH_THRESHOLDS_MS.iter().all(|t| *t > 0), "err");
    }
    // 02 五基准全绿
    {
        let mut r = S07Retest::new();
        for (i, n) in BENCH_NAMES.iter().enumerate() {
            r.submit(n, BENCH_THRESHOLDS_MS[i]);
        }
        chk(&mut set, "C3964-02", r.all_green() && r.overs().is_empty() && r.max_over_ppm() == 0, "err");
    }
    // 03 越阈检出→立案+重议幅度
    {
        let mut r = S07Retest::new();
        r.submit("渲染增量", 3); // 阈 2 → 超 5000ppm
        r.submit("色标切换", 40); // 阈 50 → 过
        chk(&mut set, "C3964-03", r.overs() == vec!["渲染增量"] && r.max_over_ppm() == 5_000 && !r.all_green(), "err");
    }
    // 04 边界恰达阈值放行
    {
        let mut r = S07Retest::new();
        r.submit("快照生成", 100);
        chk(&mut set, "C3964-04", r.overs().is_empty(), "err");
    }
    // 05 未知基准名拒收
    {
        let mut r = S07Retest::new();
        chk(&mut set, "C3964-05", r.submit("幻基准", 1) == S9DCode::BENCH_OVER, "err");
    }
    // 06 五标准复审全过
    {
        let t = ToolRow { tool: "调度器", pass: [true; 5], sandbox_ok: true, needs_sandbox: false };
        chk(&mut set, "C3964-06", t.defect().is_none() && !t.sandbox_gap(), "err");
    }
    // 07 工具不达标准→缺陷立案
    {
        let t = ToolRow { tool: "仿真器", pass: [true, true, false, true, true], sandbox_ok: true, needs_sandbox: true };
        chk(&mut set, "C3964-07", t.defect() == Some("可diff"), "err");
    }
    // 08 沙箱缺检出（读屏仿真类强制）
    {
        let t = ToolRow { tool: "仿真器", pass: [true; 5], sandbox_ok: false, needs_sandbox: true };
        chk(&mut set, "C3964-08", t.sandbox_gap() && t.defect().is_none(), "err");
    }
    // 09 五标准清单守恒（与 F3941 单源同构）
    {
        chk(&mut set, "C3964-09", TOOL_STANDARDS.len() == 5 && TOOL_STANDARDS[0] == "快" && TOOL_STANDARDS[4] == "可扩展", "err");
    }
    // 10 误报率在限内
    {
        let fp = FalsePositive { samples: 10_000, false_alarms: 30, limit_ppm: FP_LIMIT_PPM };
        chk(&mut set, "C3964-10", fp.rate_ppm() == 30 && !fp.over() && fp.to_fix() == 0, "err");
    }
    // 11 误报超标→规则回修幅度
    {
        let fp = FalsePositive { samples: 10_000, false_alarms: 120, limit_ppm: FP_LIMIT_PPM };
        chk(&mut set, "C3964-11", fp.over() && fp.to_fix() == 70, "err");
    }
    // 12 空样本不误判
    {
        let fp = FalsePositive { samples: 0, false_alarms: 5, limit_ppm: FP_LIMIT_PPM };
        chk(&mut set, "C3964-12", fp.rate_ppm() == 0 && !fp.over(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3964-13", S9DCode::SANDBOX_GAP.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9DCode::OK, S9DCode::BENCH_OVER, S9DCode::TOOL_DEFECT, S9DCode::FALSE_POSITIVE, S9DCode::SANDBOX_GAP];
        chk(&mut set, "C3964-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_s0708_all_green() {
        let s = run_ves218_checks();
        assert!(s.all_passed(), "ves218 red: {}", s.red_items_text());
    }

    #[test]
    fn test_bench_all_over() {
        let mut r = S07Retest::new();
        for n in BENCH_NAMES.iter() {
            r.submit(n, u32::MAX / 2).ok();
        }
        assert_eq!(r.overs().len(), 5);
        assert!(r.max_over_ppm() > 0);
    }

    #[test]
    fn test_fp_boundary() {
        let fp = FalsePositive { samples: 10_000, false_alarms: 500, limit_ppm: FP_LIMIT_PPM };
        assert_eq!(fp.rate_ppm(), 500);
        assert!(!fp.over()); // 恰在限内不超
        let fp2 = FalsePositive { samples: 10_000, false_alarms: 501, limit_ppm: FP_LIMIT_PPM };
        assert!(fp2.over());
    }
}
