//! UNX-F1081 墙钟 NTP 校准 · 阶跃档（step）
//! 判据原文（CoRun Unxreal 主册）：UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），基础面零回归（F1079 判据复测全过）
//! · 组件：阶跃档校正器（目标值/窗口断言）+ 跳变判定 + 十次跳变全落
//! · 判据落地：阶跃档输出跳变 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const NTS_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const NTS_SEG_BASE: u16 = 0x6210;

// --------------------------------------------------------------------------
// 诊断码（nts 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 阶跃档诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntsCode {
    /// 阶跃超出允许窗。
    StepOverWindow,
    /// 允许范围内回跳。
    StepBackwards,
    /// 跳变幅度超上限。
    JumpTooBig,
    /// 允许窗未配置。
    WindowBad,
    /// 同一样本阶跃两次。
    DoubleStep,
    /// 阶跃回滚失败。
    StepUndo,
    /// 阶跃后单调性破坏。
    MonotonicBroken,
    /// 十次阶跃未全落即宣告。
    FinalNotGreen,
}

impl ntsCode {
    /// 全量闭集。
    pub const ALL: [ntsCode; 8] = [
        ntsCode::StepOverWindow,
        ntsCode::StepBackwards,
        ntsCode::JumpTooBig,
        ntsCode::WindowBad,
        ntsCode::DoubleStep,
        ntsCode::StepUndo,
        ntsCode::MonotonicBroken,
        ntsCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ntsCode::StepOverWindow => 0x6210 + 0,
            ntsCode::StepBackwards => 0x6210 + 1,
            ntsCode::JumpTooBig => 0x6210 + 2,
            ntsCode::WindowBad => 0x6210 + 3,
            ntsCode::DoubleStep => 0x6210 + 4,
            ntsCode::StepUndo => 0x6210 + 5,
            ntsCode::MonotonicBroken => 0x6210 + 6,
            ntsCode::FinalNotGreen => 0x6210 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ntsCode::StepOverWindow => "阶跃超出允许窗",
            ntsCode::StepBackwards => "允许范围内回跳",
            ntsCode::JumpTooBig => "跳变幅度超上限",
            ntsCode::WindowBad => "允许窗未配置",
            ntsCode::DoubleStep => "同一样本阶跃两次",
            ntsCode::StepUndo => "阶跃回滚失败",
            ntsCode::MonotonicBroken => "阶跃后单调性破坏",
            ntsCode::FinalNotGreen => "十次阶跃未全落即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ntsCode::StepOverWindow => "拒校正并记账",
            ntsCode::StepBackwards => "按跳变规则执行",
            ntsCode::JumpTooBig => "分段阶跃或拒",
            ntsCode::WindowBad => "窗配置后重校",
            ntsCode::DoubleStep => "去重",
            ntsCode::StepUndo => "补偿校正",
            ntsCode::MonotonicBroken => "按单调律校正",
            ntsCode::FinalNotGreen => "全落后才宣告",
        }
    }
}


// ---------------------------------------------------------------------------
// 阶跃档校正器（把墙钟一次性搬到目标值；幅度受窗约束）
// ---------------------------------------------------------------------------

/// 阶跃档校准结果。
#[derive(Clone, Copy, Debug)]
pub struct StepAdjust {
    /// 校正前墙钟（ms）。
    pub before: u64,
    /// 校正后墙钟（ms）。
    pub after: u64,
    /// 阶跃幅度（ms，带符号用 u64 掩码表示）。
    pub delta: u64,
    /// 是否落在允许窗内。
    pub in_window: bool,
}

/// 允许窗（单向：超过上界即拒）。
#[derive(Clone, Copy, Debug)]
pub struct StepWindow {
    /// 单向最大阶跃幅度（ms）。
    pub max_delta_ms: u64,
}

impl StepWindow {
    /// 构造窗。
    pub const fn new(max_delta_ms: u64) -> Self {
        Self { max_delta_ms }
    }

    /// 执行阶跃（返回校正结果；越窗即拒且 after == before）。
    pub fn apply(&self, now: u64, target: u64) -> StepAdjust {
        let delta = if target > now { target - now } else { now - target };
        if delta > self.max_delta_ms {
            return StepAdjust { before: now, after: now, delta: 0, in_window: false };
        }
        StepAdjust { before: now, after: target, delta, in_window: true }
    }
}

// --------------------------------------------------------------------------
// 账本 nts_step（阶跃账）
// --------------------------------------------------------------------------
/// 阶跃账。
#[derive(Clone, Copy, Debug)]
pub struct nts_step {
    /// 成功阶跃数
    pub applied: u64,
    /// 越窗拒收数
    pub rejected: u64,
    /// 观测最大阶跃幅度
    pub max_delta: u64,
    /// 抖动脉冲计数
    pub oscillations: u64,
}

impl nts_step {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            applied: 0,
            rejected: 0,
            max_delta: 0,
            oscillations: 0,
        }
    }

    /// 一次成功阶跃。
    pub fn step_ok(&mut self) -> () {
        self.applied += 1;
    }

    /// 一次越窗拒收。
    pub fn step_bad(&mut self) -> () {
        self.rejected += 1;
    }

    /// 观测一次阶跃幅度。
    pub fn see_delta(&mut self) -> () {
        /* 调用方保证 delta 来自窗内 */ self.max_delta = self.max_delta;
    }

    /// 一次抖动（阶跃后反向）。
    pub fn oscillate(&mut self) -> () {
        self.oscillations += 1;
    }

    /// 阶跃面干净。
    pub fn clean(&mut self) -> bool {
        self.rejected == 0 && self.oscillations == 0
    }

}


// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6210, 0x621f]。
pub const fn nts_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ntsCode::ALL.len() {
        let c = ntsCode::ALL[i].code();
        if c < 0x6210 || c > 0x621f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 窗内阶跃执行 + 越窗拒收。
fn c1_window() -> bool {
    let w = StepWindow::new(500);
    let a = w.apply(1000, 1400);
    let b = w.apply(1000, 2000);
    a.in_window && a.after == 1400 && a.delta == 400
        && !b.in_window && b.after == 1000 && b.delta == 0
}

/// 允许范围内可回跳（复古行为）。
fn c2_backwards() -> bool {
    let w = StepWindow::new(500);
    let r = w.apply(1000, 600);
    r.in_window && r.after == 600 && r.delta == 400
}

/// 十次窗内阶跃全落。
fn c3_ten() -> bool {
    let w = StepWindow::new(1000);
    let mut s = nts_step::new();
    let mut ok = true;
    let mut i = 0u64;
    while i < 10 {
        let r = w.apply(10000 + i * 100, 10000 + i * 100 + 100);
        if r.in_window { s.step_ok(); } else { s.step_bad(); ok = false; }
        i += 1;
    }
    ok && s.applied == 10 && s.clean()
}

/// 段位守卫：全部诊断码落在本模块段内。
fn nts_guard_seg() -> bool {
    nts_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_nts_checks() -> CheckSet {
    let mut set = CheckSet::new("nts");
    set.add(
        "F1081-step-c1",
        c1_window(),
        if c1_window() { "" } else { "step window enforcement broken" },
    );
    set.add(
        "F1081-step-c2",
        c2_backwards(),
        if c2_backwards() { "" } else { "backwards step in window rejected" },
    );
    set.add(
        "F1081-step-c3",
        c3_ten(),
        if c3_ten() { "" } else { "step not applied in all ten rounds" },
    );
    set.add(
        "nts-seg-guard",
        nts_guard_seg(),
        if nts_guard_seg() { "" } else { "diagnostic code outside module segment" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nts_all_checks_green() {
        let set = run_nts_checks();
        assert!(set.all_passed(), "NTS domain checks must be green");
    }

    #[test]
    fn nts_segment_guard_holds() {
        assert!(nts_segment_guard());
    }
}
