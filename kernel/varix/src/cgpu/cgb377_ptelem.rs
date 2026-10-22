//! CGPU-F0377 · 与 P 域自适应遥测联动预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - P 域预留：长期统计对接（F2401+ 自适应遥测的长期画像——本域画像层的
//!   长期版接口预留）；场景画像共享（P 域场景画像→本域先验输入）；
//!   接口冻结预留（跨域接口先冻结后实现）。
//! - 判据：接口预留、共享、冻结、判据。
//!
//! 设计纪律：
//! - 一期纯接口预留：跨域数据面不假装可用（诚实标注），通道握手先冻结。
//! - 画像共享单向：P 域 → 本域（先验输入），本域不回写 P 域内部态。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0A20 独占。

use alloc::vec::Vec;
use alloc::string::String;

/// 诊断段基址（0x0A20，全仓独占）。
pub const SEG_BASE: u16 = 0x0A20;

pub const E_CHANNEL_UNKNOWN: u16 = SEG_BASE + 1;
pub const E_NOT_NEGOTIATED: u16 = SEG_BASE + 2;
pub const E_PAYLOAD_MISMATCH: u16 = SEG_BASE + 3;
pub const E_DIRECTION_VIOLATION: u16 = SEG_BASE + 4;
pub const E_FROZEN_MISMATCH: u16 = SEG_BASE + 5;
pub const E_STALE_PROFILE: u16 = SEG_BASE + 6;

/// 接口协议版本（先冻结后实现：版本不匹配即拒）。
pub const IFACE_VERSION: u32 = 1;

/// 通道方向：预留期单向（P→C），双向留待二期。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// P 域 → 本域（先验输入，一期唯一合法方向）。
    PToC,
    /// 本域 → P 域（一期预留，收到即拒——诚实标注不假装可用）。
    CToP,
}

/// 跨域通道描述（冻结契约的最小单位）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChannelSpec {
    pub name: String,
    pub version: u32,
    pub direction: Direction,
    /// 载荷 schema 指纹（先冻结后实现——实现方必须逐字节对齐）。
    pub schema_fingerprint: u64,
}

impl ChannelSpec {
    pub fn new(name: &str, direction: Direction, schema_fingerprint: u64) -> Self {
        ChannelSpec { name: String::from(name), version: IFACE_VERSION, direction, schema_fingerprint }
    }

    pub fn is_frozen_v1(&self) -> bool {
        self.version == IFACE_VERSION
    }
}

/// 长期画像载荷（P 域 F2401+ 自适应遥测的长期版——本域画像层的先验输入）。
/// 一期只定义形状与钳制边界，不消费真实数据（预留接口）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermProfile {
    /// 观测窗口数（P 域聚合粒度）。
    pub windows: u32,
    /// 负载均值（0~10_000 千分位钳制）。
    pub load_mean_pm: u32,
    /// 负载峰值千分位。
    pub load_peak_pm: u32,
    /// 周期性标志（P 域检测到的稳定周期）。
    pub periodic: bool,
    /// 场景代号（P 域场景画像共享）。
    pub scene_code: u16,
}

impl LongTermProfile {
    /// 逐字段钳制（外部数据不可信——先钳后用）。
    pub fn clamp_inplace(&mut self) {
        self.load_mean_pm = self.load_mean_pm.min(10_000);
        self.load_peak_pm = self.load_peak_pm.min(10_000);
    }

    /// 结构自洽：峰值不小于均值（违反即结构错误）。
    pub fn struct_ok(&self) -> bool {
        self.windows > 0 && self.load_peak_pm >= self.load_mean_pm
    }
}

/// 跨域总线：通道注册表 + 先验消费记录。
/// 冻结语义：注册后的通道 spec 不可变（改动需新版本通道）。
pub struct CrossDomainBus {
    channels: Vec<ChannelSpec>,
    /// 每通道收到的先验画像条数（遥测可见）。
    prior_counts: Vec<(String, u64)>,
}

impl CrossDomainBus {
    pub fn new() -> Self {
        CrossDomainBus { channels: Vec::new(), prior_counts: Vec::new() }
    }

    /// 注册通道（冻结动作）。同名单向注册一次；重复注册返回冻结冲突码。
    pub fn register(&mut self, spec: ChannelSpec) -> Result<(), u16> {
        if !spec.is_frozen_v1() {
            return Err(E_FROZEN_MISMATCH);
        }
        if self.channels.iter().any(|c| c.name == spec.name) {
            return Err(SEG_BASE + 7);
        }
        self.prior_counts.push((spec.name.clone(), 0));
        self.channels.push(spec);
        Ok(())
    }

    fn channel_index(&self, name: &str) -> Result<usize, u16> {
        self.channels.iter().position(|c| c.name == name).ok_or(E_CHANNEL_UNKNOWN)
    }

    /// 接收 P 域先验画像（一期唯一数据入口）。
    /// 判据「共享」：通道必须存在、方向必须 PToC、载荷必须过钳制且自洽。
    pub fn receive_prior(&mut self, channel: &str, mut profile: LongTermProfile) -> Result<(), u16> {
        let idx = self.channel_index(channel)?;
        let spec = &self.channels[idx];
        if spec.direction != Direction::PToC {
            return Err(E_DIRECTION_VIOLATION);
        }
        profile.clamp_inplace();
        if !profile.struct_ok() {
            return Err(E_PAYLOAD_MISMATCH);
        }
        let cnt = &mut self.prior_counts[idx].1;
        *cnt += 1;
        Ok(())
    }

    /// 本域向 P 域回写（一期预留接口——诚实标注：必拒）。
    pub fn send_to_p(&self, channel: &str) -> Result<(), u16> {
        let idx = self.channel_index(channel)?;
        if self.channels[idx].direction == Direction::PToC {
            // 一期：PToC 通道不允许反向发送（预留接口，二期解冻）。
            return Err(E_DIRECTION_VIOLATION);
        }
        Err(E_NOT_NEGOTIATED)
    }

    /// 通道握手探测：能力探测语义——通道在册且冻结 v1 才算可协商。
    pub fn negotiated(&self, channel: &str) -> bool {
        match self.channel_index(channel) {
            Ok(i) => self.channels[i].is_frozen_v1(),
            Err(_) => false,
        }
    }

    pub fn prior_count(&self, channel: &str) -> u64 {
        self.channel_index(channel).map(|i| self.prior_counts[i].1).unwrap_or(0)
    }

    /// 先验新鲜度：窗口数低于下限视为陈旧（消费方应回退纯统计路径）。
    pub fn prior_fresh(&self, profile: &LongTermProfile, min_windows: u32) -> Result<(), u16> {
        if profile.windows < min_windows {
            return Err(E_STALE_PROFILE);
        }
        Ok(())
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

impl Default for CrossDomainBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 本域画像层长期版接口（预留形状——F2401+ 对齐位）。
/// 一期只聚合自身采样计数；跨域合流在 P 域通道解冻后启用。
pub struct LocalLongTermAgg {
    samples: u64,
    scene_hist: Vec<(u16, u64)>,
}

impl LocalLongTermAgg {
    pub fn new() -> Self {
        LocalLongTermAgg { samples: 0, scene_hist: Vec::new() }
    }

    pub fn observe(&mut self, scene_code: u16) {
        self.samples += 1;
        if let Some(e) = self.scene_hist.iter_mut().find(|(c, _)| *c == scene_code) {
            e.1 += 1;
        } else {
            self.scene_hist.push((scene_code, 1));
        }
    }

    /// 众数场景（平票取先到——确定性）。
    pub fn dominant_scene(&self) -> Option<u16> {
        let mut best: Option<(u16, u64)> = None;
        for &(c, n) in &self.scene_hist {
            match best {
                Some((_, bn)) if bn >= n => {}
                _ => best = Some((c, n)),
            }
        }
        best.map(|(c, _)| c)
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }
}

impl Default for LocalLongTermAgg {
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

    #[test]
    #[allow(unused_must_use)]
    fn a77_接口预留_注册与握手() {
        let mut bus = CrossDomainBus::new();
        let spec = ChannelSpec::new("p2c_profile", Direction::PToC, 0xA11CE);
        assert!(bus.register(spec).is_ok());
        // 冻结冲突：同名重复注册。
        assert_eq!(bus.register(ChannelSpec::new("p2c_profile", Direction::PToC, 1)).unwrap_err(), SEG_BASE + 7);
        // 版本不匹配即拒（先冻结后实现）。
        let mut bad = ChannelSpec::new("p2c_x", Direction::PToC, 2);
        bad.version = 2;
        assert_eq!(bus.register(bad).unwrap_err(), E_FROZEN_MISMATCH);
        assert!(bus.negotiated("p2c_profile"));
        assert!(!bus.negotiated("nope"));
    }

    #[test]
    fn a77_共享_钳制与结构校验() {
        let mut bus = CrossDomainBus::new();
        bus.register(ChannelSpec::new("p2c_profile", Direction::PToC, 1)).unwrap();
        let p = LongTermProfile { windows: 90, load_mean_pm: 4_200, load_peak_pm: 9_100, periodic: true, scene_code: 3 };
        assert!(bus.receive_prior("p2c_profile", p).is_ok());
        assert_eq!(bus.prior_count("p2c_profile"), 1);
        // 超限钳制无害化。
        let mut q = LongTermProfile { windows: 10, load_mean_pm: 99_999, load_peak_pm: 88_888, periodic: false, scene_code: 0 };
        q.clamp_inplace();
        assert_eq!(q.load_mean_pm, 10_000);
        assert_eq!(q.load_peak_pm, 10_000);
        // 结构违例：均值 > 峰值。
        let bad = LongTermProfile { windows: 5, load_mean_pm: 6_000, load_peak_pm: 5_000, periodic: false, scene_code: 0 };
        assert_eq!(bus.receive_prior("p2c_profile", bad).unwrap_err(), E_PAYLOAD_MISMATCH);
    }

    #[test]
    #[allow(unused_must_use)]
    fn a77_诚实标注_反向通道一期必拒() {
        let mut bus = CrossDomainBus::new();
        bus.register(ChannelSpec::new("p2c_profile", Direction::PToC, 1)).unwrap();
        // PToC 通道反向发送：方向违例。
        assert_eq!(bus.send_to_p("p2c_profile").unwrap_err(), E_DIRECTION_VIOLATION);
        // CToP 通道（预留方向）：未协商必拒。
        bus.register(ChannelSpec::new("c2p_report", Direction::CToP, 1)).unwrap();
        assert_eq!(bus.send_to_p("c2p_report").unwrap_err(), E_NOT_NEGOTIATED);
        // 未知通道。
        assert_eq!(bus.send_to_p("ghost").unwrap_err(), E_CHANNEL_UNKNOWN);
    }

    #[test]
    fn a77_新鲜度与本地聚合() {
        let bus = CrossDomainBus::new();
        let stale = LongTermProfile { windows: 2, load_mean_pm: 100, load_peak_pm: 200, periodic: false, scene_code: 1 };
        assert_eq!(bus.prior_fresh(&stale, 30).unwrap_err(), E_STALE_PROFILE);
        let fresh = LongTermProfile { windows: 64, load_mean_pm: 100, load_peak_pm: 200, periodic: false, scene_code: 1 };
        assert!(bus.prior_fresh(&fresh, 30).is_ok());

        let mut agg = LocalLongTermAgg::new();
        for _ in 0..5 { agg.observe(7); }
        for _ in 0..3 { agg.observe(2); }
        assert_eq!(agg.samples(), 8);
        assert_eq!(agg.dominant_scene(), Some(7));
        assert_eq!(LocalLongTermAgg::default().dominant_scene(), None);
    }
}
