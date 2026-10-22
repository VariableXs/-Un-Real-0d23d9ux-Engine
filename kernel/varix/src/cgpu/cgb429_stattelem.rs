//! CGPU-F0429 · 统计遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：聚合耗时/存储水位/查询延迟 P95/导出次数——四指标进 B08 总线
//!   （统计系统自身可观测）。判据：四指标、进总线、判据。
//!
//! 设计纪律：
//! - 统计系统是"观测别人的系统"，本模块让它也被观测（自指闭环）：
//!   四指标分别覆盖统计管线的四个阶段（聚合→存储→查询→导出），
//!   任一阶段劣化都能从总线帧上读出——不新增第五指标（诚实覆盖面）。
//! - 聚合耗时：微秒累计（只增计数器），配合 rollup 次数可出均摊。
//! - 存储水位：已用字节 / 配额字节，越顶（>100%）直接拒绝继续落盘。
//! - 查询延迟 P95：滚动样本窗（容量钳制），分位数 = ceil(0.95·n) 位
//!   （升序第 k 小，k 从 1 起）——单尖峰不冒充 P95。
//! - 进总线：B08 同源帧格式（魔数+序号+四槽+校验），帧序只增，
//!   往返可读回（发射即自证）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0E00 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0E00，全仓独占）。
pub const SEG_BASE: u16 = 0x0E00;

pub const E_STL_POINT: u16 = SEG_BASE + 1; // 指标点非法
pub const E_STL_WINDOW: u16 = SEG_BASE + 2; // 样本窗非法（0 或超钳制）
pub const E_STL_NEG: u16 = SEG_BASE + 3; // 计数回退（只增被违反）
pub const E_STL_BUS: u16 = SEG_BASE + 4; // 总线帧校验错
pub const E_STL_WATER: u16 = SEG_BASE + 5; // 水位越顶（存储超配额）

/// 指标点：聚合耗时 / 存储水位 / 查询延迟 P95 / 导出次数。
pub const STL_AGG: usize = 0;
pub const STL_WATER: usize = 1;
pub const STL_QP95: usize = 2;
pub const STL_EXPORT: usize = 3;
pub const STL_POINTS: usize = 4;

pub fn stl_point_name(p: usize) -> &'static str {
    match p {
        STL_AGG => "聚合耗时",
        STL_WATER => "存储水位",
        STL_QP95 => "查询P95",
        _ => "导出次数",
    }
}

/// 样本窗容量钳制（防止无界增长——遥测自身不能成为内存泄漏源）。
pub const STL_SAMPLE_CAP: usize = 64;

/// B08 同源帧魔数（统计自观测段——段号 0x0E00 进魔数低位）。
pub const STL_MAGIC: u32 = 0xB08E_0E00;

/// 统计自观测总线帧（B08 同源：魔数+序号+四槽+校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StlFrame {
    /// 魔数（格式自证）。
    pub magic: u32,
    /// 帧序号（只增）。
    pub seq: u64,
    /// 四指标槽：[聚合耗时us累计, 水位‰, P95us, 导出次数]。
    pub metrics: [u64; STL_POINTS],
    /// 校验字（槽和的低 32 位——同源算法）。
    pub check: u32,
}

impl StlFrame {
    fn compute_check(metrics: &[u64; STL_POINTS]) -> u32 {
        let mut acc: u64 = 0;
        for v in metrics.iter() {
            acc = acc.wrapping_add(*v);
        }
        acc as u32
    }

    /// 帧自校验：魔数 + 校验字双验证。
    pub fn self_ok(&self) -> bool {
        self.magic == STL_MAGIC && self.check == Self::compute_check(&self.metrics)
    }
}

/// 统计自观测遥测器。
#[derive(Debug)]
pub struct StTelemetry {
    /// 聚合耗时累计（微秒，只增）。
    agg_us: u64,
    /// rollup 次数（配合均摊）。
    rollups: u64,
    /// 存储已用字节 / 配额字节。
    used_bytes: u64,
    quota_bytes: u64,
    /// 查询延迟滚动样本（微秒）。
    lat: Vec<u64>,
    /// 导出次数（只增）。
    exports: u64,
    /// 样本窗容量。
    cap: usize,
    /// 已发射帧序号（只增）。
    seq: u64,
}

impl Default for StTelemetry {
    fn default() -> Self {
        // 公称容量必然合法（new 的拒绝条件只针对 0 与超硬钳）。
        Self {
            agg_us: 0,
            rollups: 0,
            used_bytes: 0,
            quota_bytes: 0,
            lat: Vec::new(),
            exports: 0,
            cap: STL_SAMPLE_CAP,
            seq: 0,
        }
    }
}

impl StTelemetry {
    /// 构造：样本窗容量须非零且不超硬钳（2× 公称容量）。
    pub fn new(cap: usize) -> Result<Self, u16> {
        if cap == 0 || cap > STL_SAMPLE_CAP * 2 {
            return Err(E_STL_WINDOW);
        }
        Ok(Self {
            agg_us: 0,
            rollups: 0,
            used_bytes: 0,
            quota_bytes: 0,
            lat: Vec::new(),
            exports: 0,
            cap,
            seq: 0,
        })
    }

    /// 记一次聚合耗时（微秒）——累计只增。
    pub fn note_agg(&mut self, us: u64) {
        self.agg_us = self.agg_us.saturating_add(us);
        self.rollups = self.rollups.saturating_add(1);
    }

    /// 记一次查询延迟样本（微秒）——滚动窗，满则挤掉最旧。
    pub fn note_query(&mut self, us: u64) {
        if self.lat.len() >= self.cap {
            self.lat.remove(0);
        }
        self.lat.push(us);
    }

    /// 记一次导出——计数只增。
    pub fn note_export(&mut self) {
        self.exports = self.exports.saturating_add(1);
    }

    /// 更新存储水位（字节）。允许回撤（删除/过期释放是真实行为），
    /// 但配额缩减导致已用超顶时拒——水位语义：used ≤ quota 才健康。
    pub fn set_water(&mut self, used: u64, quota: u64) -> Result<(), u16> {
        if quota > 0 && used > quota {
            return Err(E_STL_WATER);
        }
        self.used_bytes = used;
        self.quota_bytes = quota;
        Ok(())
    }

    /// 聚合均摊耗时（us/次，向下取整；无聚合记 0）。
    pub fn agg_avg_us(&self) -> u64 {
        if self.rollups == 0 {
            0
        } else {
            self.agg_us / self.rollups
        }
    }

    /// 查询延迟 P95（微秒）：升序第 ceil(0.95·n) 小；空窗记 0。
    pub fn p95_us(&self) -> u64 {
        let n = self.lat.len();
        if n == 0 {
            return 0;
        }
        let mut s = self.lat.clone();
        s.sort_unstable();
        // k = ceil(95n/100)，k 从 1 起；等价于 (95n + 99)/100 的整式。
        let k = (95 * n + 99) / 100;
        s[k - 1]
    }

    /// 存储水位（千分比，0~1000；无配额记 0——未启用不虚报）。
    pub fn water_pm(&self) -> u64 {
        if self.quota_bytes == 0 {
            0
        } else {
            self.used_bytes.saturating_mul(1000) / self.quota_bytes
        }
    }

    /// 发射 B08 总线帧：四指标打包+校验+序号推进。
    pub fn emit_bus(&mut self) -> StlFrame {
        self.seq = self.seq.saturating_add(1);
        let metrics = [
            self.agg_us,
            self.water_pm(),
            self.p95_us(),
            self.exports,
        ];
        StlFrame {
            magic: STL_MAGIC,
            seq: self.seq,
            check: StlFrame::compute_check(&metrics),
            metrics,
        }
    }

    /// 发射历史帧数。
    pub fn frames_emitted(&self) -> u64 {
        self.seq
    }

    /// 计数只增防线：任何槽的回退尝试都被拒（外部对账用）。
    pub fn check_monotonic(&self, prev: &[u64; STL_POINTS], now: &[u64; STL_POINTS]) -> Result<(), u16> {
        for i in 0..STL_POINTS {
            // 水位槽允许回撤（释放是真实行为），其余槽只增。
            if i != STL_WATER && now[i] < prev[i] {
                return Err(E_STL_NEG);
            }
        }
        Ok(())
    }
}

// ==========================================================================
// 判据（施工书：四指标、进总线、判据）
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_四指标覆盖统计管线四阶段() {
        let mut t = StTelemetry::new(STL_SAMPLE_CAP).unwrap_or_else(|_| StTelemetry::default());
        // 聚合耗时：累计与均摊。
        t.note_agg(100);
        t.note_agg(200);
        assert_eq!(t.agg_avg_us(), 150);
        // 查询延迟与导出。
        t.note_query(1_000);
        t.note_export();
        // 水位：半满。
        t.set_water(500, 1_000).ok();
        // 四点名称表覆盖（指标面完整自证）。
        for p in 0..STL_POINTS {
            assert!(!stl_point_name(p).is_empty());
        }
        // 帧内四槽各就各位。
        let f = t.emit_bus();
        assert_eq!(f.metrics[STL_AGG], 300);
        assert_eq!(f.metrics[STL_WATER], 500);
        assert_eq!(f.metrics[STL_EXPORT], 1);
    }

    #[test]
    fn a02_P95分位数_单尖峰不冒充() {
        let mut t = StTelemetry::default();
        // 空窗 P95 = 0（不虚报）。
        assert_eq!(t.p95_us(), 0);
        // 20 个样本：19 个 100us + 1 个 10_000us 尖峰。
        // ceil(0.95·20)=19 → 升序第 19 位仍是 100（尖峰在第 20 位）。
        for _ in 0..19 {
            t.note_query(100);
        }
        t.note_query(10_000);
        assert_eq!(t.p95_us(), 100);
        // 再加一个 100 → n=21, ceil(0.95·21)=20 → 升序第 20 位 = 100。
        t.note_query(100);
        assert_eq!(t.p95_us(), 100);
        // 全部慢查询：n=20 全 500 → P95 = 500。
        let mut t2 = StTelemetry::default();
        for _ in 0..20 {
            t2.note_query(500);
        }
        assert_eq!(t2.p95_us(), 500);
    }

    #[test]
    fn a03_滚动窗与水位越顶() {
        let mut t = StTelemetry::new(4).unwrap_or_else(|_| StTelemetry::default());
        for v in 1..=6u64 {
            t.note_query(v);
        }
        // 容量 4：只留最后 4 个样本 [3,4,5,6]。
        assert_eq!(t.p95_us(), 6);
        // 水位：合法更新。
        assert_eq!(t.set_water(800, 1_000), Ok(()));
        assert_eq!(t.water_pm(), 800);
        // 越顶拒：used > quota。
        assert_eq!(t.set_water(1_001, 1_000), Err(E_STL_WATER));
        // 原值未被污染。
        assert_eq!(t.water_pm(), 800);
        // 非法窗容量：0 拒、超硬钳拒。
        assert_eq!(StTelemetry::new(0).unwrap_err(), E_STL_WINDOW);
        assert_eq!(StTelemetry::new(STL_SAMPLE_CAP * 2 + 1).unwrap_err(), E_STL_WINDOW);
    }

    #[test]
    fn a04_进总线_帧自校验与序号只增() {
        let mut t = StTelemetry::default();
        t.note_agg(50);
        t.note_export();
        let f1 = t.emit_bus();
        let f2 = t.emit_bus();
        // 魔数 + 校验自证。
        assert!(f1.self_ok() && f2.self_ok());
        assert_eq!(f1.magic, STL_MAGIC);
        // 序号只增。
        assert_eq!(f2.seq, f1.seq + 1);
        assert_eq!(t.frames_emitted(), 2);
        // 篡改任一槽 → 自校验红。
        let mut bad = f1;
        bad.metrics[STL_EXPORT] = bad.metrics[STL_EXPORT].wrapping_add(7);
        assert!(!bad.self_ok());
        // 篡改魔数 → 自校验红。
        let mut bad2 = f1;
        bad2.magic = 0xDEAD_BEEF;
        assert!(!bad2.self_ok());
        // 指标变更反映到下一帧。
        t.note_export();
        let f3 = t.emit_bus();
        assert!(f3.metrics[STL_EXPORT] > f2.metrics[STL_EXPORT]);
    }

    #[test]
    fn a05_只增防线_水位豁免() {
        let mut t = StTelemetry::default();
        t.note_agg(10);
        t.note_export();
        t.note_query(5);
        let prev = t.emit_bus().metrics;
        // 合法推进：各槽不减。
        t.note_agg(10);
        t.note_export();
        t.note_query(6);
        let now = t.emit_bus().metrics;
        assert_eq!(t.check_monotonic(&prev, &now), Ok(()));
        // 回退拒：聚合耗时减少。
        let mut regressed = now;
        regressed[STL_AGG] = prev[STL_AGG] - 1;
        assert_eq!(t.check_monotonic(&prev, &regressed), Err(E_STL_NEG));
        // 导出回退拒。
        let mut reg2 = now;
        reg2[STL_EXPORT] = 0;
        assert_eq!(t.check_monotonic(&prev, &reg2), Err(E_STL_NEG));
        // 水位回撤放行（释放是真实行为）。
        let mut released = now;
        released[STL_WATER] = 0;
        assert_eq!(t.check_monotonic(&prev, &released), Ok(()));
    }
}
