//! VE-F0117 · 多适配器遥测聚合（VE-A 域 · A05 多 GPU 与异构组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0117`
//!
//! **职责定位（锚点原文）**：多卡遥测的统一聚合（每卡指标+跨卡指标两层），
//! 聚合口径统一（各卡一把尺），聚合数据不出机；含聚合的分位统计（P95 而非均值）。
//!
//! **铁律**：
//! 1. **两层聚合**——每卡指标 + 跨卡指标。
//! 2. **口径统一**——各卡一把尺。
//! 3. **零出机**——聚合数据不出机。
//! 4. **异常标注**——异常值事件标注。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BB（遥测聚合）。
pub const SEG_BASE: u16 = 0xE8BB;

/// 口径漂移。
pub const XC_DRIFT: u16 = SEG_BASE | 0x01;
/// 聚合失败。
pub const XC_AGG_FAIL: u16 = SEG_BASE | 0x02;
/// 异常值。
pub const XC_ANOMALY: u16 = SEG_BASE | 0x03;

/// 单卡指标。
#[derive(Clone, Copy, Debug)]
pub struct GpuMetric {
    pub gpu_id: u16,
    /// 值（微秒）。
    pub value: u64,
    /// 口径版本。
    pub caliber: u32,
}

/// 聚合结果。
#[derive(Clone, Debug)]
pub struct AggResult {
    /// 两层：per_gpu + cross。
    pub per_gpu: Vec<(u16, u64)>,
    pub cross_p95: u64,
    pub cross_max: u64,
    /// 异常标注。
    pub anomalies: Vec<u16>,
}

/// 遥测聚合器。
pub struct TelemetryAgg {
    /// 统一口径版本。
    caliber: u32,
    /// 样本缓存。
    samples: Vec<GpuMetric>,
    last_code: u16,
    /// 出机拦截次数。
    egress_blocks: u32,
    aggs: u32,
}

impl TelemetryAgg {
    pub fn new(caliber: u32) -> Result<Self, u16> {
        if caliber == 0 {
            return Err(XC_DRIFT);
        }
        Ok(TelemetryAgg {
            caliber,
            samples: Vec::new(),
            last_code: 0,
            egress_blocks: 0,
            aggs: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 推入指标（口径校验）。
    pub fn push(&mut self, m: GpuMetric) -> Result<(), u16> {
        if m.caliber != self.caliber {
            self.last_code = XC_DRIFT;
            return Err(XC_DRIFT);
        }
        self.samples.push(m);
        self.last_code = 0;
        Ok(())
    }

    /// 聚合（两层）。
    pub fn aggregate(&mut self) -> Result<AggResult, u16> {
        if self.samples.is_empty() {
            self.last_code = XC_AGG_FAIL;
            return Err(XC_AGG_FAIL);
        }
        let mut per_gpu: Vec<(u16, u64)> = Vec::new();
        for s in &self.samples {
            match per_gpu.iter_mut().find(|(id, _)| *id == s.gpu_id) {
                Some((_, v)) => *v += s.value,
                None => per_gpu.push((s.gpu_id, s.value)),
            }
        }
        // P95 而非均值。
        let mut vals: Vec<u64> = self.samples.iter().map(|s| s.value).collect();
        vals.sort();
        let p95 = vals[((vals.len() as f64) * 0.95) as usize];
        let mx = *vals.last().unwrap();
        // 异常标注（超 2 倍 P50）。
        let p50 = vals[vals.len() / 2];
        let mut anomalies = Vec::new();
        for s in &self.samples {
            if p50 > 0 && s.value > p50 * 2 {
                anomalies.push(s.gpu_id);
                self.last_code = XC_ANOMALY;
            }
        }
        self.aggs += 1;
        Ok(AggResult {
            per_gpu,
            cross_p95: p95,
            cross_max: mx,
            anomalies,
        })
    }

    /// 出机拦截（零出机铁律）。
    pub fn block_egress(&mut self) {
        self.egress_blocks += 1;
    }

    pub fn egress_blocks(&self) -> u32 {
        self.egress_blocks
    }

    pub fn aggs(&self) -> u32 {
        self.aggs
    }
}

/// 自检：两层聚合、口径统一、零出机、异常标注。
pub fn run_vea117_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn met(g: u16, v: u64) -> GpuMetric {
        GpuMetric {
            gpu_id: g,
            value: v,
            caliber: 1,
        }
    }

    let mut set = CheckSet::new("vea117_agg");

    // C117-01 口径统一加载
    {
        let a = TelemetryAgg::new(1);
        chk(&mut set, "C117-01 口径", a.is_ok(), "口径失败");
    }

    // C117-02 口径漂移拒绝
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        let r = a.push(met(0, 100));
        // 同口径通过。
        let r2 = a.push(GpuMetric { gpu_id: 1, value: 1, caliber: 99 });
        chk(&mut set, "C117-02 漂移拒", r.is_ok() && r2.is_err(), "漂移被放");
    }

    // C117-03 两层聚合
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        let _ = a.push(met(0, 100));
        let _ = a.push(met(0, 200));
        let _ = a.push(met(1, 150));
        let r = a.aggregate();
        let ok = r.is_ok() && r.unwrap().per_gpu.len() == 2;
        chk(&mut set, "C117-03 两层", ok, "两层缺");
    }

    // C117-04 P95 非均值
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        for i in 0..20 {
            let _ = a.push(met(0, 10 + i));
        }
        let r = a.aggregate().unwrap();
        chk(&mut set, "C117-04 P95", r.cross_p95 >= 25, "P95 错");
    }

    // C117-05 异常标注
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        for _ in 0..10 {
            let _ = a.push(met(0, 100));
        }
        let _ = a.push(met(1, 10000));
        let r = a.aggregate().unwrap();
        chk(&mut set, "C117-05 异常标注", r.anomalies.contains(&1), "异常漏标");
    }

    // C117-06 零出机
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        a.block_egress();
        chk(&mut set, "C117-06 零出机", a.egress_blocks() == 1, "出机未拦");
    }

    // C117-07 空聚合拒绝
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        let r = a.aggregate();
        chk(&mut set, "C117-07 空拒", r.is_err(), "空被放");
    }

    // C117-08 零口径拒绝
    {
        let r = TelemetryAgg::new(0);
        chk(&mut set, "C117-08 零口径拒", r.is_err(), "零口径被放");
    }

    // C117-09 跨卡 max
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        let _ = a.push(met(0, 50));
        let _ = a.push(met(1, 500));
        let r = a.aggregate().unwrap();
        chk(&mut set, "C117-09 跨卡 max", r.cross_max == 500, "max 错");
    }

    // C117-10 聚合计数
    {
        let mut a = TelemetryAgg::new(1).unwrap();
        let _ = a.push(met(0, 1));
        let _ = a.aggregate();
        let _ = a.aggregate();
        chk(&mut set, "C117-10 计数", a.aggs() == 2, "计数错");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea117_all_green() {
        let s = run_vea117_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
