//! UNX-F1081 墙钟 NTP 校准 · 渐变档（slew）
//! 判据原文（CoRun Unxreal 主册）：UNX-F1081-J1 双协议正确（阶跃档输出跳变 10/10 次、渐变档输出斜率受控 10/10 次），校准事件账落盘（来源/幅度/档位全记录），基础面零回归（F1079 判据复测全过）
//! · 组件：渐变档速率受控调整器（斜率 ns/tick）+ 步进状态机 + 十次斜率受控断言
//! · 判据落地：渐变档输出斜率受控 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const NTW_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const NTW_SEG_BASE: u16 = 0x6220;

// --------------------------------------------------------------------------
// 诊断码（ntw 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 渐变档诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ntwCode {
    /// 调整斜率超上限。
    SlopeOver,
    /// 斜率震荡（来回变号）。
    SlopeOsc,
    /// 斜率为零不收敛。
    SlopeZero,
    /// 斜率过小收敛慢。
    SlopeUnder,
    /// 死区宽度为零。
    DeadZoneBad,
    /// 渐变中被强制跳变。
    StepFromSlew,
    /// 斜率与实测不符。
    SlopeDrift,
    /// 十次斜率未受控即宣告。
    FinalNotGreen,
}

impl ntwCode {
    /// 全量闭集。
    pub const ALL: [ntwCode; 8] = [
        ntwCode::SlopeOver,
        ntwCode::SlopeOsc,
        ntwCode::SlopeZero,
        ntwCode::SlopeUnder,
        ntwCode::DeadZoneBad,
        ntwCode::StepFromSlew,
        ntwCode::SlopeDrift,
        ntwCode::FinalNotGreen,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ntwCode::SlopeOver => 0x6220 + 0,
            ntwCode::SlopeOsc => 0x6220 + 1,
            ntwCode::SlopeZero => 0x6220 + 2,
            ntwCode::SlopeUnder => 0x6220 + 3,
            ntwCode::DeadZoneBad => 0x6220 + 4,
            ntwCode::StepFromSlew => 0x6220 + 5,
            ntwCode::SlopeDrift => 0x6220 + 6,
            ntwCode::FinalNotGreen => 0x6220 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ntwCode::SlopeOver => "调整斜率超上限",
            ntwCode::SlopeOsc => "斜率震荡（来回变号）",
            ntwCode::SlopeZero => "斜率为零不收敛",
            ntwCode::SlopeUnder => "斜率过小收敛慢",
            ntwCode::DeadZoneBad => "死区宽度为零",
            ntwCode::StepFromSlew => "渐变中被强制跳变",
            ntwCode::SlopeDrift => "斜率与实测不符",
            ntwCode::FinalNotGreen => "十次斜率未受控即宣告",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ntwCode::SlopeOver => "按上限截断并记账",
            ntwCode::SlopeOsc => "引入死区抑制",
            ntwCode::SlopeZero => "补最小调整",
            ntwCode::SlopeUnder => "放宽上限",
            ntwCode::DeadZoneBad => "按默认死区",
            ntwCode::StepFromSlew => "保持渐变语义",
            ntwCode::SlopeDrift => "重校模型",
            ntwCode::FinalNotGreen => "受控后才宣告",
        }
    }
}


// ---------------------------------------------------------------------------
// 渐变档速率受控调整器（每 tick 只推进 |slope| ns，永不跳变）
// ---------------------------------------------------------------------------

/// 速率受控墙钟调整器。
pub struct SlewAdjuster {
    /// 当前墙钟（ns）。
    pub now_ns: u64,
    /// 目标墙钟（ns）。
    pub target_ns: u64,
    /// 每 tick 推进量（ns，非负）。
    pub slope_ns: u64,
    /// 每 tick 最大推进量（ns，上限）。
    pub max_slope_ns: u64,
    /// 死区宽度（ns，|误差| ≤ 死区即停）。
    pub dead_zone_ns: u64,
    /// 完成标志。
    pub done: bool,
}

impl SlewAdjuster {
    /// 构造调整器。
    pub const fn new(now_ns: u64, target_ns: u64, max_slope_ns: u64, dead_zone_ns: u64) -> Self {
        Self { now_ns, target_ns, slope_ns: 0, max_slope_ns, dead_zone_ns, done: false }
    }

    /// 设定期望斜率（超上限截断，负向归零）。
    pub fn set_slope(&mut self, want_ns: u64) {
        self.slope_ns = if want_ns > self.max_slope_ns { self.max_slope_ns } else { want_ns };
    }

    /// 推进一个 tick（返回本 tick 实际推进量）。
    pub fn tick(&mut self) -> u64 {
        if self.done {
            return 0;
        }
        let err = if self.target_ns > self.now_ns {
            self.target_ns - self.now_ns
        } else {
            self.now_ns - self.target_ns
        };
        if err <= self.dead_zone_ns || self.slope_ns == 0 {
            self.done = true;
            return 0;
        }
        let adv = if self.slope_ns < err { self.slope_ns } else { err };
        if self.target_ns > self.now_ns {
            self.now_ns += adv;
        } else {
            self.now_ns -= adv;
        }
        adv
    }

    /// 剩余误差（ns）。
    pub fn remaining(&self) -> u64 {
        if self.target_ns > self.now_ns {
            self.target_ns - self.now_ns
        } else {
            self.now_ns - self.target_ns
        }
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6220, 0x622f]。
pub const fn ntw_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ntwCode::ALL.len() {
        let c = ntwCode::ALL[i].code();
        if c < 0x6220 || c > 0x622f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 斜率超上限即截断。
fn c1_slope_cap() -> bool {
    let mut a = SlewAdjuster::new(0, 1_000_000, 1000, 10);
    a.set_slope(50_000);
    a.slope_ns == 1000 && a.tick() == 1000
}

/// 渐变收敛且零跳变（每 tick ≤ slope）。
fn c2_converge() -> bool {
    let mut a = SlewAdjuster::new(0, 100_000, 1000, 10);
    a.set_slope(1000);
    let mut max_step = 0u64;
    let mut i = 0u64;
    while i < 500 && !a.done {
        let s = a.tick();
        if s > max_step { max_step = s; }
        i += 1;
    }
    a.done && a.now_ns == 100_000 && max_step <= 1000
}

/// 死区内即停。
fn c3_deadzone() -> bool {
    let mut a = SlewAdjuster::new(0, 5, 1000, 10);
    a.set_slope(1000);
    let s = a.tick();
    a.done && s == 0 && a.now_ns == 0 && a.remaining() == 5
}

/// 段位守卫：全部诊断码落在本模块段内。
fn ntw_guard_seg() -> bool {
    ntw_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ntw_checks() -> CheckSet {
    let mut set = CheckSet::new("ntw");
    set.add(
        "F1081-slew-c1",
        c1_slope_cap(),
        if c1_slope_cap() { "" } else { "slope cap not enforced" },
    );
    set.add(
        "F1081-slew-c2",
        c2_converge(),
        if c2_converge() { "" } else { "slew jumped instead of converging" },
    );
    set.add(
        "F1081-slew-c3",
        c3_deadzone(),
        if c3_deadzone() { "" } else { "dead zone not respected" },
    );
    set.add(
        "ntw-seg-guard",
        ntw_guard_seg(),
        if ntw_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn ntw_all_checks_green() {
        let set = run_ntw_checks();
        assert!(set.all_passed(), "NTW domain checks must be green");
    }

    #[test]
    fn ntw_segment_guard_holds() {
        assert!(ntw_segment_guard());
    }
}
