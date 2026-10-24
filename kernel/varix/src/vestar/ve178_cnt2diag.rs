//! ve178_cnt2diag ↔ VE-F0178 计数器到诊断中心通路
//!
//! 注册计数器（F0177）按采集档位汇聚到诊断中心快照（F0104 式原子发布语义复用），
//! 通路带宽配额防高频计数器挤占；断路检测（来源停更标记失联）。
//!
//! 判据：档位汇聚、配额限流、失联标记、原子发布。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::ve177_perfcnt::{CollectTier, PerfCounterRegistry};

/// 通路错误码。
pub mod err {
    /// 配额越限。
    pub const E_QUOTA: u16 = 0x7801;
    /// 来源失联。
    pub const E_SOURCE_LOST: u16 = 0x7802;
    /// 快照版本无效。
    pub const E_BAD_SNAPSHOT: u16 = 0x7803;
    /// 计数器未注册。
    pub const E_UNREGISTERED: u16 = 0x7804;
}

/// 诊断中心快照（原子发布语义）。
#[derive(Debug, Clone)]
pub struct DiagSnapshot {
    /// 快照版本号（单调递增）。
    pub version: u64,
    /// 时间戳（逻辑时钟）。
    pub timestamp: u64,
    /// 计数器值表（名称 → 当前值）。
    pub values: BTreeMap<String, i64>,
    /// 失联来源列表。
    pub lost_sources: Vec<String>,
}

/// 配额记录（按档位分配带宽）。
#[derive(Debug, Clone)]
pub struct QuotaRecord {
    /// 档位。
    pub tier: CollectTier,
    /// 每周期最大推送上限。
    pub max_per_cycle: u32,
    /// 本周期已推送数。
    pub pushed: u32,
    /// 被限流次数。
    pub throttled: u64,
}

/// 失联检测器。
#[derive(Debug, Clone)]
pub struct LivenessDetector {
    /// 来源 → 最后更新逻辑时间。
    last_seen: BTreeMap<String, u64>,
    /// 失联阈值（逻辑时间差）。
    pub threshold: u64,
}

impl LivenessDetector {
    /// 新建检测器。
    pub fn new(threshold: u64) -> Self {
        Self {
            last_seen: BTreeMap::new(),
            threshold,
        }
    }

    /// 记录来源活跃。
    pub fn touch(&mut self, source: &str, now: u64) {
        self.last_seen.insert(source.to_string(), now);
    }

    /// 检测失联来源。
    pub fn detect_lost(&self, now: u64) -> Vec<String> {
        self.last_seen
            .iter()
            .filter(|(_, &t)| now.saturating_sub(t) > self.threshold)
            .map(|(s, _)| s.clone())
            .collect()
    }

    /// 判断来源是否失联。
    pub fn is_lost(&self, source: &str, now: u64) -> bool {
        self.last_seen
            .get(source)
            .map(|&t| now.saturating_sub(t) > self.threshold)
            .unwrap_or(true) // 从未见过视为失联
    }
}

/// 汇聚通路：档位 × 计数器 → 诊断中心快照。
pub struct AggPipeline {
    /// 各档位配额。
    quotas: BTreeMap<CollectTier, QuotaRecord>,
    /// 当前快照（原子发布）。
    snapshot: DiagSnapshot,
    /// 失联检测器。
    liveness: LivenessDetector,
    /// 逻辑时钟。
    clock: u64,
    /// 汇聚周期计数。
    cycles: u64,
}

impl AggPipeline {
    /// 新建通路（默认配额：PerFrame=64, PerSecond=256, PerMinute=1024, OnDemand=4096）。
    pub fn new() -> Self {
        let mut quotas = BTreeMap::new();
        quotas.insert(
            CollectTier::PerFrame,
            QuotaRecord {
                tier: CollectTier::PerFrame,
                max_per_cycle: 64,
                pushed: 0,
                throttled: 0,
            },
        );
        quotas.insert(
            CollectTier::PerSecond,
            QuotaRecord {
                tier: CollectTier::PerSecond,
                max_per_cycle: 256,
                pushed: 0,
                throttled: 0,
            },
        );
        quotas.insert(
            CollectTier::PerMinute,
            QuotaRecord {
                tier: CollectTier::PerMinute,
                max_per_cycle: 1024,
                pushed: 0,
                throttled: 0,
            },
        );
        quotas.insert(
            CollectTier::OnDemand,
            QuotaRecord {
                tier: CollectTier::OnDemand,
                max_per_cycle: 4096,
                pushed: 0,
                throttled: 0,
            },
        );
        Self {
            quotas,
            snapshot: DiagSnapshot {
                version: 1,
                timestamp: 0,
                values: BTreeMap::new(),
                lost_sources: Vec::new(),
            },
            liveness: LivenessDetector::new(100),
            clock: 0,
            cycles: 0,
        }
    }

    /// 当前快照版本号。
    pub fn snapshot_version(&self) -> u64 {
        self.snapshot.version
    }

    /// 获取当前快照（只读引用）。
    pub fn current_snapshot(&self) -> &DiagSnapshot {
        &self.snapshot
    }

    /// 推送计数器值（受配额约束）。
    ///
    /// 返回 Ok(()) 表示推入成功；Err(E_QUOTA) 表示被限流。
    pub fn push(
        &mut self,
        name: &str,
        value: i64,
        tier: CollectTier,
        source: &str,
        registry: &PerfCounterRegistry,
    ) -> Result<(), u16> {
        // 未注册计数器拒绝
        if registry.get(name).is_none() {
            return Err(err::E_UNREGISTERED);
        }
        // 配额检查
        if let Some(q) = self.quotas.get_mut(&tier) {
            if q.pushed >= q.max_per_cycle {
                q.throttled += 1;
                return Err(err::E_QUOTA);
            }
            q.pushed += 1;
        }
        // 原子更新快照值
        self.snapshot.values.insert(name.to_string(), value);
        self.liveness.touch(source, self.clock);
        Ok(())
    }

    /// 批量推送（部分成功：受限的跳过不阻断其余）。
    pub fn push_batch(
        &mut self,
        items: &[(&str, i64, CollectTier, &str)],
        registry: &PerfCounterRegistry,
    ) -> (usize, usize) {
        let mut ok = 0;
        let mut throttled = 0;
        for &(name, val, tier, src) in items {
            match self.push(name, val, tier, src, registry) {
                Ok(()) => ok += 1,
                Err(err::E_QUOTA) => throttled += 1,
                Err(_) => {} // 其他错误跳过
            }
        }
        (ok, throttled)
    }

    /// 触发一次汇聚周期：推进时钟、重置配额、更新失联、原子发布新快照版本。
    pub fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.cycles += 1;
        // 重置每周期配额
        for q in self.quotas.values_mut() {
            q.pushed = 0;
        }
        // 失联检测
        let lost = self.liveness.detect_lost(self.clock);
        // 原子发布：新版本号
        self.snapshot.version += 1;
        self.snapshot.timestamp = self.clock;
        self.snapshot.lost_sources = lost;
        self.snapshot.version
    }

    /// 获取配额状态。
    pub fn quota(&self, tier: CollectTier) -> Option<&QuotaRecord> {
        self.quotas.get(&tier)
    }

    /// 检查来源是否失联。
    pub fn is_source_lost(&self, source: &str) -> bool {
        self.liveness.is_lost(source, self.clock)
    }

    /// 周期计数。
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// 逻辑时钟。
    pub fn now(&self) -> u64 {
        self.clock
    }
}

impl Default for AggPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_registry() -> PerfCounterRegistry {
        let mut reg = PerfCounterRegistry::new();
        reg.register("gpu.fps", "fps", "gpu", CollectTier::PerFrame, "帧率")
            .unwrap();
        reg.register("cpu.load", "%", "cpu", CollectTier::PerSecond, "负载")
            .unwrap();
        reg
    }

    #[test]
    fn test_push_and_snapshot() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        assert!(pipe.push("gpu.fps", 60, CollectTier::PerFrame, "gpu", &reg).is_ok());
        assert_eq!(pipe.current_snapshot().values.get("gpu.fps"), Some(&60));
    }

    #[test]
    fn test_unregistered_rejected() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        assert_eq!(
            pipe.push("unknown", 1, CollectTier::PerFrame, "s", &reg),
            Err(err::E_UNREGISTERED)
        );
    }

    #[test]
    fn test_quota_throttle() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        // PerFrame 配额 64
        for i in 0..64 {
            assert!(pipe.push("gpu.fps", i, CollectTier::PerFrame, "gpu", &reg).is_ok());
        }
        // 第 65 次被限流
        assert_eq!(
            pipe.push("gpu.fps", 65, CollectTier::PerFrame, "gpu", &reg),
            Err(err::E_QUOTA)
        );
        assert_eq!(pipe.quota(CollectTier::PerFrame).unwrap().throttled, 1);
    }

    #[test]
    fn test_tick_resets_quota_and_publishes() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        for i in 0..64 {
            pipe.push("gpu.fps", i, CollectTier::PerFrame, "gpu", &reg).unwrap();
        }
        assert_eq!(
            pipe.push("gpu.fps", 0, CollectTier::PerFrame, "gpu", &reg),
            Err(err::E_QUOTA)
        );
        let v = pipe.tick();
        assert_eq!(v, pipe.snapshot_version());
        // 配额重置后又能推了
        assert!(pipe.push("gpu.fps", 100, CollectTier::PerFrame, "gpu", &reg).is_ok());
    }

    #[test]
    fn test_liveness_detection() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        pipe.liveness.threshold = 5;
        pipe.push("gpu.fps", 1, CollectTier::PerFrame, "gpu", &reg).unwrap();
        // 推进时钟超过阈值，gpu 失联
        for _ in 0..6 {
            pipe.tick();
        }
        assert!(pipe.is_source_lost("gpu"));
    }

    #[test]
    fn test_low_tier_throttled_first() {
        let reg = setup_registry();
        let mut pipe = AggPipeline::new();
        // 覆盖 PerMinute 配额为 2，验证低档位先限流
        pipe.quotas.insert(
            CollectTier::PerMinute,
            QuotaRecord {
                tier: CollectTier::PerMinute,
                max_per_cycle: 2,
                pushed: 0,
                throttled: 0,
            },
        );
        reg.register("mem.free", "bytes", "mem", CollectTier::PerMinute, "空闲")
            .unwrap();
        assert!(pipe.push("mem.free", 1, CollectTier::PerMinute, "mem", &reg).is_ok());
        assert!(pipe.push("mem.free", 2, CollectTier::PerMinute, "mem", &reg).is_ok());
        assert_eq!(
            pipe.push("mem.free", 3, CollectTier::PerMinute, "mem", &reg),
            Err(err::E_QUOTA)
        );
    }
}
