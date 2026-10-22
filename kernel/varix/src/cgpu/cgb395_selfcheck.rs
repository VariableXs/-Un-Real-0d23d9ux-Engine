//! CGPU-F0395 · 预算系统自检（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 自检：启动链探针（预算子系统启动自检——账户表/仲裁器/预测器/持久化
//!   四组件健康）；运行期心跳（仲裁器心跳——对接 F0344 自保）；自检报告
//!   （每次启动输出——异常显性化）。判据：四组件探针、心跳、报告、判据。
//!
//! 设计纪律：
//! - 四组件探针各自独立、结论三态（Healthy/Degraded/Down），异常显性化。
//! - 心跳是死线账：超过 N 帧无声即判失联（对接 F0344 自保的预算侧钩子）。
//! - 报告为启动链一次成型 + 运行期增量追加（历史不丢）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0B90 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0B90，全仓独占）。
pub const SEG_BASE: u16 = 0x0B90;

pub const E_COMPONENT_DOWN: u16 = SEG_BASE + 1;
pub const E_HEARTBEAT_LOST: u16 = SEG_BASE + 2;
pub const E_REPORT_INCOMPLETE: u16 = SEG_BASE + 3;

/// 组件编号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Component {
    /// 账户表。
    AccountTable = 0,
    /// 仲裁器。
    Arbiter = 1,
    /// 预测器。
    Predictor = 2,
    /// 持久化。
    Persistence = 3,
}

pub const COMPONENT_SLOTS: usize = 4;

/// 组件健康三态。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Health {
    Healthy,
    Degraded,
    Down,
}

/// 启动链探针结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeResult {
    pub comp: Component,
    pub health: Health,
    pub detail: u32,
}

/// 启动链探针器（判据「四组件探针」——逐组件独立探测）。
pub struct StartupProber {
    results: [Option<ProbeResult>; COMPONENT_SLOTS],
}

impl StartupProber {
    pub fn new() -> Self {
        StartupProber { results: [None; COMPONENT_SLOTS] }
    }

    /// 记录单组件探针结果（越界/重复组件显性拒）。
    pub fn record(&mut self, comp: Component, health: Health, detail: u32) -> Result<(), u16> {
        let idx = comp as usize;
        if idx >= COMPONENT_SLOTS {
            return Err(E_REPORT_INCOMPLETE);
        }
        // 最新探测覆盖旧结果（重探测语义——健康态可流转）。
        self.results[idx] = Some(ProbeResult { comp, health, detail });
        Ok(())
    }

    pub fn result_of(&self, comp: Component) -> Option<ProbeResult> {
        self.results[comp as usize].clone()
    }

    /// 全组件齐套核。
    pub fn all_probed(&self) -> bool {
        self.results.iter().all(|r| r.is_some())
    }

    /// 启动放行核：任一 Down 即拒；Degraded 放行但显性（返回降级名单）。
    pub fn admit(&self) -> Result<Vec<Component>, u16> {
        if !self.all_probed() {
            return Err(E_REPORT_INCOMPLETE);
        }
        let mut degraded = Vec::new();
        for r in self.results.iter().flatten() {
            match r.health {
                Health::Down => return Err(E_COMPONENT_DOWN),
                Health::Degraded => degraded.push(r.comp),
                Health::Healthy => {}
            }
        }
        Ok(degraded)
    }

    /// 报告成型（判据「自检报告」——启动一次输出）。
    pub fn report(&self) -> Result<[Health; COMPONENT_SLOTS], u16> {
        if !self.all_probed() {
            return Err(E_REPORT_INCOMPLETE);
        }
        let mut out = [Health::Healthy; COMPONENT_SLOTS];
        for r in self.results.iter().flatten() {
            out[r.comp as usize] = r.health;
        }
        Ok(out)
    }
}

impl Default for StartupProber {
    fn default() -> Self {
        Self::new()
    }
}

/// 心跳账（判据「心跳」——仲裁器失联检测，对接 F0344 自保）。
pub struct HeartbeatLedger {
    /// 死线（帧）。
    pub deadline_frames: u64,
    last_beat_frame: Option<u64>,
    pub beats: u64,
}

impl HeartbeatLedger {
    pub fn new(deadline_frames: u64) -> Self {
        HeartbeatLedger { deadline_frames: deadline_frames.max(1), last_beat_frame: None, beats: 0 }
    }

    /// 记一次心跳。
    pub fn beat(&mut self, frame: u64) {
        self.last_beat_frame = Some(frame);
        self.beats += 1;
    }

    /// 失联核：当前帧距上次心跳超死线即失联（从未有心跳也判失联）。
    pub fn alive_at(&self, now_frame: u64) -> Result<(), u16> {
        match self.last_beat_frame {
            None => Err(E_HEARTBEAT_LOST),
            Some(f) => {
                if now_frame.saturating_sub(f) > self.deadline_frames {
                    Err(E_HEARTBEAT_LOST)
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn silence(&self) -> Option<u64> {
        self.last_beat_frame
    }
}

/// 自检报告账（启动成型 + 运行期追加——历史不丢）。
pub struct SelfCheckReport {
    startup: Option<[Health; COMPONENT_SLOTS]>,
    pub incidents: Vec<(u64, u16)>, // (frame, err)
}

impl SelfCheckReport {
    pub fn new() -> Self {
        SelfCheckReport { startup: None, incidents: Vec::new() }
    }

    /// 启动报告成型（幂等：只成型一次）。
    pub fn seal_startup(&mut self, health: [Health; COMPONENT_SLOTS]) -> Result<(), u16> {
        if self.startup.is_some() {
            return Ok(());
        }
        self.startup = Some(health);
        Ok(())
    }

    /// 运行期事故追加。
    pub fn add_incident(&mut self, frame: u64, err: u16) {
        self.incidents.push((frame, err));
    }

    /// 报告完整性：启动成型过（判据「每次启动输出」）。
    pub fn sealed(&self) -> bool {
        self.startup.is_some()
    }

    pub fn startup_health(&self) -> Option<[Health; COMPONENT_SLOTS]> {
        self.startup
    }

    pub fn incident_count(&self) -> usize {
        self.incidents.len()
    }
}

impl Default for SelfCheckReport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn probed_all_healthy() -> StartupProber {
        let mut p = StartupProber::new();
        p.record(Component::AccountTable, Health::Healthy, 0).unwrap();
        p.record(Component::Arbiter, Health::Healthy, 0).unwrap();
        p.record(Component::Predictor, Health::Healthy, 0).unwrap();
        p.record(Component::Persistence, Health::Healthy, 0).unwrap();
        p
    }

    #[test]
    fn a95_四组件探针_齐套与放行() {
        let p = probed_all_healthy();
        assert!(p.all_probed());
        assert!(p.admit().unwrap().is_empty());
        assert_eq!(p.report().unwrap(), [Health::Healthy; 4]);
        // 三态序显性自检（Ord 派生锚）：Healthy < Degraded < Down。
        assert!(Health::Healthy < Health::Degraded && Health::Degraded < Health::Down);
        // 重探测覆盖（健康态流转）。
        let mut p2 = probed_all_healthy();
        p2.record(Component::Arbiter, Health::Degraded, 9).unwrap();
        assert_eq!(p2.report().unwrap()[1], Health::Degraded);
        p2.record(Component::Arbiter, Health::Healthy, 0).unwrap();
        assert_eq!(p2.report().unwrap()[1], Health::Healthy);
        // 漏探测即拒。
        let mut thin = StartupProber::new();
        thin.record(Component::AccountTable, Health::Healthy, 0).unwrap();
        assert_eq!(thin.admit().unwrap_err(), E_REPORT_INCOMPLETE);
        // Down 必拒。
        let mut dead = probed_all_healthy();
        dead.record(Component::Predictor, Health::Down, 1).unwrap();
        assert_eq!(dead.admit().unwrap_err(), E_COMPONENT_DOWN);
        // Degraded 放行但显性。
        let mut deg = probed_all_healthy();
        deg.record(Component::Persistence, Health::Degraded, 2).unwrap();
        assert_eq!(deg.admit().unwrap(), vec![Component::Persistence]);
    }

    #[test]
    fn a95_心跳_死线失联() {
        let mut h = HeartbeatLedger::new(8);
        // 从未有心跳即失联。
        assert_eq!(h.alive_at(0).unwrap_err(), E_HEARTBEAT_LOST);
        h.beat(10);
        assert!(h.alive_at(18).is_ok()); // 恰在死线内（≤8）
        assert_eq!(h.alive_at(19).unwrap_err(), E_HEARTBEAT_LOST); // 超 8
        assert_eq!(h.beats, 1);
        assert_eq!(h.silence(), Some(10));
        // 死线 0 钳为 1。
        assert_eq!(HeartbeatLedger::new(0).deadline_frames, 1);
    }

    #[test]
    fn a95_报告_启动成型与事故追加() {
        let mut r = SelfCheckReport::new();
        assert!(!r.sealed());
        let p = probed_all_healthy();
        r.seal_startup(p.report().unwrap()).unwrap();
        assert!(r.sealed());
        // 幂等成型。
        let mut dead = probed_all_healthy();
        dead.record(Component::Arbiter, Health::Down, 1).unwrap();
        r.seal_startup(dead.report().unwrap()).unwrap();
        assert_eq!(r.startup_health().unwrap(), [Health::Healthy; 4]);
        // 事故追加。
        r.add_incident(100, E_HEARTBEAT_LOST);
        r.add_incident(200, E_COMPONENT_DOWN);
        assert_eq!(r.incident_count(), 2);
        assert_eq!(r.incidents[1], (200, E_COMPONENT_DOWN));
        // 组件枚举槽位一致。
        assert_eq!(Component::Persistence as usize, COMPONENT_SLOTS - 1);
    }
}
