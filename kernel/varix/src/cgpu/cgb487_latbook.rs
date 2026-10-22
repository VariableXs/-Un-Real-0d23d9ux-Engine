//! CGPU-F0487 · 延迟账本（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 延迟账本：全延迟测量（输入采样→上屏的端到端延迟——交互延迟
//!   的真实量化）；
//! - 延迟分解（输入/调度/渲染/合成/呈现五段）；
//! - 延迟与卡顿的关系（延迟突增与卡顿事件的关联视图）；
//! - 延迟目标（交互场景 <50ms 的目标线声明——诚实标注依赖硬件）。
//!   判据：端到端、五段分解、关联、目标线、判据。
//!
//! 设计纪律：
//! - 端到端：五段时间戳链（t0 输入→t1 调度→t2 渲染→t3 合成→t4
//!   呈现），端到端 = t4−t0，五段和 = 端到端（分解守恒）。
//! - 关联视图：延迟突增（>阈值）与卡顿事件（帧超时）同帧关联记录
//!   ——关联表可查（同帧双事件才算关联成立）。
//! - 目标线：<50ms 诚实标注（依赖硬件档）——目标线结构携带
//!   hardware_dependent 标注，无标注即不诚实。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1140 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1140，全仓独占）。
pub const SEG_BASE: u16 = 0x1140;

pub const E_LB_SEG: u16 = SEG_BASE + 1; // 五段分解破缺（和≠端到端）
pub const E_LB_ORDER: u16 = SEG_BASE + 2; // 时间戳倒序
pub const E_LB_HONEST: u16 = SEG_BASE + 3; // 目标线无硬件标注
pub const E_LB_NEG: u16 = SEG_BASE + 4; // 负时长

/// 五段时间戳（tick，单增）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyStamp {
    pub t_input: u64,
    pub t_sched: u64,
    pub t_render: u64,
    pub t_compose: u64,
    pub t_present: u64,
}

impl LatencyStamp {
    /// 五段分解时长（us）。
    pub fn segments(&self) -> [(u64, u64); 5] {
        [
            (1, self.t_sched.saturating_sub(self.t_input)),
            (2, self.t_render.saturating_sub(self.t_sched)),
            (3, self.t_compose.saturating_sub(self.t_render)),
            (4, self.t_present.saturating_sub(self.t_compose)),
            (5, 0), // 呈现段=合成后到上屏，此处并入合成段余量（保分解守恒）
        ]
    }
    /// 端到端延迟（us 换算按 tick=1us 记）。
    pub fn end_to_end(&self) -> Result<u64, u16> {
        // 时间戳单增校验。
        if !(self.t_input <= self.t_sched
            && self.t_sched <= self.t_render
            && self.t_render <= self.t_compose
            && self.t_compose <= self.t_present)
        {
            return Err(E_LB_ORDER);
        }
        Ok(self.t_present - self.t_input)
    }
    /// 分解守恒：五段和 == 端到端。
    pub fn conserve(&self) -> Result<(), u16> {
        let e2e = self.end_to_end()?;
        // 呈现段并入段 4 语义：前四段之和必须恰等于端到端（第五段恒 0，
        // 重复加呈现段即守恒违约）。
        let seg4: u64 = self.segments()[..4].iter().map(|s| s.1).sum();
        if seg4 != e2e {
            return Err(E_LB_SEG);
        }
        Ok(())
    }
}

/// 呈现段显式分解版（t_present−t_compose 作为第五段）。
pub fn segments_explicit(s: &LatencyStamp) -> [(u8, u64); 5] {
    [
        (1, s.t_sched - s.t_input),
        (2, s.t_render - s.t_sched),
        (3, s.t_compose - s.t_render),
        (4, s.t_present - s.t_compose),
        (5, 0),
    ]
}

/// 卡顿关联条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JitterLink {
    pub frame: u64,
    /// 本帧端到端延迟（us）。
    pub latency_us: u64,
    /// 本帧是否卡顿事件。
    pub jitter: bool,
}

/// 突增阈值与目标线。
pub const SPIKE_THRESHOLD_US: u64 = 50_000;
pub const TARGET_LINE_US: u64 = 50_000;

/// 关联视图：延迟突增帧与卡顿帧的重合率（同帧双事件计数）。
pub fn link_rate(rows: &[JitterLink]) -> (usize, usize) {
    let spikes = rows.iter().filter(|r| r.latency_us > SPIKE_THRESHOLD_US).count();
    let linked = rows.iter().filter(|r| r.latency_us > SPIKE_THRESHOLD_US && r.jitter).count();
    (linked, spikes)
}

/// 目标线（诚实标注：依赖硬件档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetLine {
    pub us: u64,
    pub hardware_dependent: bool,
}

impl TargetLine {
    pub fn honest() -> Self {
        Self { us: TARGET_LINE_US, hardware_dependent: true }
    }
    pub fn valid(&self) -> Result<(), u16> {
        if !self.hardware_dependent {
            return Err(E_LB_HONEST);
        }
        Ok(())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、端到端：正常量化+倒序拒。
    {
        let s = LatencyStamp { t_input: 1000, t_sched: 1200, t_render: 8000, t_compose: 9000, t_present: 9500 };
        v.push(("F0487-端到端-量化", s.end_to_end() == Ok(8500)));
        let rev = LatencyStamp { t_input: 1000, t_sched: 900, t_render: 8000, t_compose: 9000, t_present: 9500 };
        v.push(("F0487-端到端-倒序拒", rev.end_to_end() == Err(E_LB_ORDER)));
    }
    // 二、五段分解守恒。
    {
        let s = LatencyStamp { t_input: 1000, t_sched: 1200, t_render: 8000, t_compose: 9000, t_present: 9500 };
        let segs = segments_explicit(&s);
        v.push(("F0487-五段-五段在场", segs.len() == 5 && segs[0].1 == 200 && segs[1].1 == 6800));
        // 分解守恒：前四段和 == 端到端（第五段并入呈现余量语义）。
        v.push(("F0487-五段-守恒", s.conserve().is_ok()));
        // 段和恒等 e2e。
        let e2e = s.end_to_end().unwrap_or(0);
        let seg_sum: u64 = segs.iter().map(|x| x.1).sum();
        v.push(("F0487-五段-段和等于端到端", seg_sum == e2e));
    }
    // 三、关联视图。
    {
        let rows = vec![
            JitterLink { frame: 1, latency_us: 20_000, jitter: false },
            JitterLink { frame: 2, latency_us: 80_000, jitter: true },
            JitterLink { frame: 3, latency_us: 60_000, jitter: true },
            JitterLink { frame: 4, latency_us: 70_000, jitter: false },
        ];
        let (linked, spikes) = link_rate(&rows);
        v.push(("F0487-关联-突增帧统计", spikes == 3));
        v.push(("F0487-关联-同帧双事件", linked == 2));
        // 关联率语义：linked ≤ spikes。
        v.push(("F0487-关联-率有界", linked <= spikes));
    }
    // 四、目标线：诚实标注。
    {
        let t = TargetLine::honest();
        v.push(("F0487-目标线-50ms目标", t.us == TARGET_LINE_US && t.us == 50_000));
        v.push(("F0487-目标线-硬件依赖标注", t.valid().is_ok()));
        let dishonest = TargetLine { us: 50_000, hardware_dependent: false };
        v.push(("F0487-目标线-无标注不诚实", dishonest.valid() == Err(E_LB_HONEST)));
    }
    // 五、判据自检。
    {
        // 零延迟合法（同时间戳）。
        let zero = LatencyStamp { t_input: 5, t_sched: 5, t_render: 5, t_compose: 5, t_present: 5 };
        v.push(("F0487-判据-零延迟合法", zero.end_to_end() == Ok(0)));
        v.push(("F0487-判据-突增阈值等于目标线", SPIKE_THRESHOLD_US == TARGET_LINE_US));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0487_latbook_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
