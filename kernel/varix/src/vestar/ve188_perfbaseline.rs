//! ve188_perfbaseline ↔ VE-F0188 域性能基线总账
//!
//! A 域全部性能相关条目的基线汇总登记：呈现延迟、提交开销、内存带宽、软渲帧率分档
//! （按硬件路径×分辨率×档位诚实标注），样本量与采集环境如实记录；基线版本化，
//! 升级重测后更新并标注变化原因。
//!
//! 判据：分档诚实、样本如实、版本化、实测修正。

use alloc::format;
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 硬件路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HwPath {
    /// 硬件加速 GPU 路径。
    GpuAccel,
    /// 软件渲染路径。
    SoftRender,
    /// 混合路径。
    Hybrid,
}

/// 性能条目基线。
#[derive(Debug, Clone)]
pub struct PerfBaseline {
    /// 条目名称。
    pub name: String,
    /// 硬件路径。
    pub hw_path: HwPath,
    /// 分辨率标识（如 "1920x1080"）。
    pub resolution: String,
    /// 档位标识（如 "high"/"medium"/"low"）。
    pub profile: String,
    /// P50 值。
    pub p50: f64,
    /// P95 值。
    pub p95: f64,
    /// P99 值。
    pub p99: f64,
    /// 样本量。
    pub sample_count: u64,
    /// 采集环境描述。
    pub environment: String,
    /// 采集时间戳。
    pub timestamp: u64,
    /// 是否为实验档（样本不足不出版结论）。
    pub experimental: bool,
}

/// 性能基线总账。
pub struct PerfBaselineLedger {
    /// 条目表。
    entries: BTreeMap<String, PerfBaseline>,
    /// 版本记录（版本号 → 变更原因）。
    version_log: Vec<(u64, String)>,
    /// 当前版本。
    version: u64,
    /// 最小样本量阈值（低于此标注 experimental）。
    pub min_samples: u64,
}

impl PerfBaselineLedger {
    /// 新建总账。
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            version_log: vec![(1, "初始版本".to_string())],
            version: 1,
            min_samples: 30,
        }
    }

    /// 当前版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 登记基线条目（样本不足自动标注 experimental）。
    pub fn register(
        &mut self,
        name: &str,
        hw_path: HwPath,
        resolution: &str,
        profile: &str,
        p50: f64,
        p95: f64,
        p99: f64,
        sample_count: u64,
        environment: &str,
    ) -> u64 {
        let experimental = sample_count < self.min_samples;
        let entry = PerfBaseline {
            name: name.to_string(),
            hw_path,
            resolution: resolution.to_string(),
            profile: profile.to_string(),
            p50,
            p95,
            p99,
            sample_count,
            environment: environment.to_string(),
            timestamp: self.version,
            experimental,
        };
        self.entries.insert(name.to_string(), entry);
        self.version += 1;
        self.version - 1
    }

    /// 实测修正：用新数据更新条目并标注变化原因。
    pub fn update_with_reason(
        &mut self,
        name: &str,
        p50: f64,
        p95: f64,
        p99: f64,
        sample_count: u64,
        reason: &str,
    ) -> Result<(), u16> {
        let entry = self.entries.get_mut(name).ok_or(0x8101u16)?;
        entry.p50 = p50;
        entry.p95 = p95;
        entry.p99 = p99;
        entry.sample_count = sample_count;
        entry.experimental = sample_count < self.min_samples;
        entry.timestamp = self.version;
        self.version += 1;
        self.version_log.push((self.version, reason.to_string()));
        Ok(())
    }

    /// 查询条目。
    pub fn get(&self, name: &str) -> Option<&PerfBaseline> {
        self.entries.get(name)
    }

    /// 按硬件路径过滤。
    pub fn filter_by_path(&self, path: HwPath) -> Vec<&PerfBaseline> {
        self.entries.values().filter(|e| e.hw_path == path).collect()
    }

    /// 按档位过滤。
    pub fn filter_by_profile(&self, profile: &str) -> Vec<&PerfBaseline> {
        self.entries
            .values()
            .filter(|e| e.profile == profile)
            .collect()
    }

    /// 列出所有条目。
    pub fn all(&self) -> Vec<&PerfBaseline> {
        self.entries.values().collect()
    }

    /// 版本日志。
    pub fn version_log(&self) -> &[(u64, String)] {
        &self.version_log
    }

    /// 检查诚实性：所有非 experimental 条目样本量必须达标。
    pub fn verify_honesty(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|e| !e.experimental && e.sample_count < self.min_samples)
            .map(|e| alloc::format!("条目[{}]非实验档但样本量不足", e.name))
            .collect()
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for PerfBaselineLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_query() {
        let mut l = PerfBaselineLedger::new();
        l.register("present_latency", HwPath::GpuAccel, "1920x1080", "high", 8.5, 12.0, 15.0, 100, "RTX3060");
        assert_eq!(l.len(), 1);
        let e = l.get("present_latency").unwrap();
        assert_eq!(e.p50, 8.5);
        assert!(!e.experimental);
    }

    #[test]
    fn test_insufficient_samples_experimental() {
        let mut l = PerfBaselineLedger::new();
        l.register("x", HwPath::SoftRender, "1920x1080", "low", 16.0, 20.0, 25.0, 5, "i5");
        assert!(l.get("x").unwrap().experimental);
    }

    #[test]
    fn test_update_with_reason() {
        let mut l = PerfBaselineLedger::new();
        l.register("x", HwPath::GpuAccel, "1920x1080", "high", 8.0, 12.0, 15.0, 100, "env");
        l.update_with_reason("x", 7.5, 11.0, 14.0, 100, "驱动升级重测").unwrap();
        assert_eq!(l.get("x").unwrap().p50, 7.5);
        assert!(l.version_log().len() >= 2);
    }

    #[test]
    fn test_filter_by_path() {
        let mut l = PerfBaselineLedger::new();
        l.register("a", HwPath::GpuAccel, "1920x1080", "high", 1.0, 2.0, 3.0, 100, "e");
        l.register("b", HwPath::SoftRender, "1920x1080", "high", 1.0, 2.0, 3.0, 100, "e");
        assert_eq!(l.filter_by_path(HwPath::GpuAccel).len(), 1);
        assert_eq!(l.filter_by_path(HwPath::SoftRender).len(), 1);
    }

    #[test]
    fn test_verify_honesty() {
        let mut l = PerfBaselineLedger::new();
        l.register("x", HwPath::GpuAccel, "1920x1080", "high", 1.0, 2.0, 3.0, 100, "e");
        assert_eq!(l.verify_honesty().len(), 0);
    }
}
