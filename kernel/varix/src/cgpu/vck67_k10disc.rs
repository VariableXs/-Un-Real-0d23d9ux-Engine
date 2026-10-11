//! CGPU-F1748 · K 域三纪律终审
//!
//! 判据锚点（施工书原文摘录）：
//! 三纪律终审：①呈现不阻塞（异构调度/遥测开销总账（0.1ms+0.1ms+0.05ms——回读
//! ——帧内纪律终审；②采样不耗样本（异构采样开销/自适应降级——采样审计终审；③
//! 数据不造假（估算/典型值/两态标注/支持矩阵诚实/基准诚实——不造假全域抽查 30
//! 路径——抽查报告；纪律形式化（断言 CI 状态——固化确认；违例历史（零未决声明；
//! 测试（开销回读/采样审计/不造假抽查/断言/违例五组）。判据：开销回读、采样审计、
//! 不造假 30 路径、固化、零违例、五组、判据。
//!
//! 诊断码段 0xBF01~0xBF05（独占）：
//!   0xBF01 开销失约（三开销账任一超恰值）
//!   0xBF02 采样失审（采样开销超/降级断档）
//!   0xBF03 不造假失查（30 路径抽查缺/违例隐性）
//!   0xBF04 固化/违例失账（断言未固化/违例未决）
//!   0xBF05 段越界（判码落 0xBF00..0xC000 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const K10_DISC_VERSION: &str = "K67-k10disc-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xBF01~0xBF05）
// ---------------------------------------------------------------------------

pub const XC_TD_OVERHEAD_OVER: u32 = 0xBF01;
pub const XC_TD_SAMPLE_AUDIT: u32 = 0xBF02;
pub const XC_TD_HONESTY_GAP: u32 = 0xBF03;
pub const XC_TD_FREEZE_LOST: u32 = 0xBF04;
pub const XC_TD_SEG_OOR: u32 = 0xBF05;

// ---------------------------------------------------------------------------
// 一、呈现不阻塞（三开销恰值回读）
// ---------------------------------------------------------------------------

/// 三开销恰值（µs）：异构调度 100 / 遥测 100 / 面板 50。
pub const OVERHEAD_BUDGET_US: [(&str, u32); 3] =
    [("het_sched", 100), ("telemetry", 100), ("panel", 50)];

pub fn overhead_readback(measured: [u32; 3]) -> Result<(), u32> {
    let mut i = 0;
    while i < 3 {
        let (name, budget) = OVERHEAD_BUDGET_US[i];
        if name.is_empty() || measured[i] == 0 || measured[i] > budget {
            return Err(XC_TD_OVERHEAD_OVER);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、采样不耗样本（采样审计终审）
// ---------------------------------------------------------------------------

/// 采样审计账：采样开销占比千分比 ≤ 预算 + 降级四档连续。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleAudit {
    /// 采样开销占比（墙钟千分比）。
    pub overhead_permille: u32,
    /// 预算（≤50‰）。
    pub budget_permille: u32,
    /// 降级四档连续（0/1/2/3 无跳档）。
    pub degrade_ladder: [bool; 4],
}

impl SampleAudit {
    pub fn sound(&self) -> Result<(), u32> {
        if self.overhead_permille > self.budget_permille {
            return Err(XC_TD_SAMPLE_AUDIT);
        }
        for on in self.degrade_ladder.iter() {
            if !on {
                return Err(XC_TD_SAMPLE_AUDIT);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、数据不造假（全域抽查 30 路径）
// ---------------------------------------------------------------------------

/// 抽查路径账：30 路径全查（估算/典型值/两态/矩阵诚实/基准诚实五类×6）。
pub const HONESTY_PATHS: u16 = 30;

/// 抽查报告：路径数恰值 + 违例数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HonestyReport {
    pub paths_checked: u16,
    pub violations: u16,
}

impl HonestyReport {
    pub fn sound(&self) -> Result<(), u32> {
        if self.paths_checked != HONESTY_PATHS {
            return Err(XC_TD_HONESTY_GAP);
        }
        // 违例必须为 0（有违例须先整改再收口）。
        if self.violations != 0 {
            return Err(XC_TD_HONESTY_GAP);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、纪律形式化（断言 CI 固化）与违例历史（零未决）
// ---------------------------------------------------------------------------

/// 固化账：五组断言 CI 状态全绿。
pub const ASSERTION_CIS: [&str; 5] =
    ["overhead_ci", "sample_ci", "honesty_ci", "freeze_ci", "violation_ci"];

pub fn freeze_audit(states: &[bool; 5]) -> Result<(), u32> {
    for ok in states.iter() {
        if !ok {
            return Err(XC_TD_FREEZE_LOST);
        }
    }
    Ok(())
}

/// 违例历史：总发现数/已决数/未决数——未决必零。
pub fn violation_history(found: u16, resolved: u16, open: u16) -> Result<(), u32> {
    if open != 0 {
        return Err(XC_TD_FREEZE_LOST);
    }
    if resolved > found {
        return Err(XC_TD_FREEZE_LOST);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（五组：开销回读/采样审计/不造假抽查/断言/违例）
// ---------------------------------------------------------------------------

/// 三纪律终审引擎。
pub struct K10DiscEngine;

impl K10DiscEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：三开销恰值回读 + 反例（超预算拒/零值拒）。
        overhead_readback([100, 100, 50])?;
        if overhead_readback([101, 100, 50]).is_ok() {
            return Err(XC_TD_OVERHEAD_OVER);
        }
        if overhead_readback([0, 100, 50]).is_ok() {
            return Err(XC_TD_OVERHEAD_OVER);
        }
        // 组二：采样审计 + 反例（超预算/降级断档拒）。
        SampleAudit { overhead_permille: 50, budget_permille: 50, degrade_ladder: [true, true, true, true] }
            .sound()?;
        if (SampleAudit { overhead_permille: 51, budget_permille: 50, degrade_ladder: [true, true, true, true] })
            .sound()
            .is_ok()
        {
            return Err(XC_TD_SAMPLE_AUDIT);
        }
        if (SampleAudit { overhead_permille: 40, budget_permille: 50, degrade_ladder: [true, true, false, true] })
            .sound()
            .is_ok()
        {
            return Err(XC_TD_SAMPLE_AUDIT);
        }
        // 组三：不造假 30 路径抽查 + 反例（路径缺/违例拒）。
        HonestyReport { paths_checked: 30, violations: 0 }.sound()?;
        if (HonestyReport { paths_checked: 29, violations: 0 }).sound().is_ok() {
            return Err(XC_TD_HONESTY_GAP);
        }
        if (HonestyReport { paths_checked: 30, violations: 1 }).sound().is_ok() {
            return Err(XC_TD_HONESTY_GAP);
        }
        // 组四：断言固化 + 反例。
        freeze_audit(&[true, true, true, true, true])?;
        if freeze_audit(&[true, true, true, false, true]).is_ok() {
            return Err(XC_TD_FREEZE_LOST);
        }
        // 组五：违例历史零未决 + 反例。
        violation_history(12, 12, 0)?;
        if violation_history(12, 11, 1).is_ok() {
            return Err(XC_TD_FREEZE_LOST);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xBF00..0xC000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_TD_OVERHEAD_OVER,
            XC_TD_SAMPLE_AUDIT,
            XC_TD_HONESTY_GAP,
            XC_TD_FREEZE_LOST,
            XC_TD_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xBF00 || codes[i] >= 0xC000 {
                return Err(XC_TD_OVERHEAD_OVER);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_TD_OVERHEAD_OVER);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
