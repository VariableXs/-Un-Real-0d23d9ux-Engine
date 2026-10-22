//! CGPU-F0383 · 预测文档与基准（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 文档基准：文档（预测架构白皮书/调参指南）；基准（预测命中率基准
//!   负载谱/预测器开销实测）——入册回归。
//! - 判据：两文档、两基准、判据。
//!
//! 设计纪律：
//! - 文档以「章节清单 + 完整性断言」入册：缺章节=入册失败。
//! - 基准可执行：命中率达阈值、开销不越 0.5% 线——数字漂移即回归红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0AB0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0AB0，全仓独占）。
pub const SEG_BASE: u16 = 0x0AB0;

pub const E_DOC_INCOMPLETE: u16 = SEG_BASE + 1;
pub const E_BENCH_MISS: u16 = SEG_BASE + 2;
pub const E_BENCH_OVERHEAD: u16 = SEG_BASE + 3;
pub const E_REGRESSION: u16 = SEG_BASE + 4;

/// 命中率达标线（千分位）。
pub const HIT_RATE_TARGET_PM: u32 = 700;
/// 开销红线（千分位——与 F0379 预算一致）。
pub const OVERHEAD_LIMIT_PM: u32 = 5;

/// 章节条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    pub title: &'static str,
    pub min_lines: u32,
}

/// 文档（章节清单 + 行数账）。
pub struct DocBook {
    pub name: &'static str,
    required: Vec<Section>,
    pub actual_lines: Vec<(&'static str, u32)>,
}

impl DocBook {
    pub fn new(name: &'static str, required: Vec<Section>) -> Self {
        DocBook { name, required, actual_lines: Vec::new() }
    }

    /// 预测架构白皮书（两文档之一）的冻结章节表。
    pub fn whitepaper() -> Self {
        DocBook::new(
            "预测架构白皮书",
            vec![
                Section { title: "两段架构", min_lines: 40 },
                Section { title: "画像层", min_lines: 30 },
                Section { title: "预测层", min_lines: 40 },
                Section { title: "预留层", min_lines: 30 },
                Section { title: "降级与兜底", min_lines: 25 },
                Section { title: "诚实边界", min_lines: 20 },
            ],
        )
    }

    /// 调参指南（两文档之二）的冻结章节表。
    pub fn tuning_guide() -> Self {
        DocBook::new(
            "调参指南",
            vec![
                Section { title: "EWMA 系数", min_lines: 25 },
                Section { title: "区间宽度", min_lines: 25 },
                Section { title: "降频因子", min_lines: 15 },
                Section { title: "阈值速查表", min_lines: 20 },
            ],
        )
    }

    pub fn write(&mut self, title: &'static str, lines: u32) -> Result<(), u16> {
        if !self.required.iter().any(|s| s.title == title) {
            return Err(E_DOC_INCOMPLETE);
        }
        if let Some(e) = self.actual_lines.iter_mut().find(|(t, _)| *t == title) {
            e.1 = lines;
        } else {
            self.actual_lines.push((title, lines));
        }
        Ok(())
    }

    /// 入册核：必填章节齐全且行数达标（判据「两文档」的完整性断言）。
    pub fn complete(&self) -> Result<(), u16> {
        for s in &self.required {
            match self.actual_lines.iter().find(|(t, _)| *t == s.title) {
                None => return Err(E_DOC_INCOMPLETE),
                Some((_, n)) if *n < s.min_lines => return Err(E_DOC_INCOMPLETE),
                Some(_) => {}
            }
        }
        Ok(())
    }
}

/// 基准负载谱条目（预测命中率基准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchLoad {
    pub scene: u16,
    pub shape: u8, // 0 稳态 1 周期 2 突变 3 混合
    pub frames: u32,
    pub hits: u32,
}

/// 基准结果册（入册回归的物理载体）。
pub struct BenchLedger {
    loads: Vec<BenchLoad>,
    /// 开销实测（千分位）。
    pub overhead_pm: Option<u32>,
}

impl BenchLedger {
    pub fn new() -> Self {
        BenchLedger { loads: Vec::new(), overhead_pm: None }
    }

    pub fn record_load(&mut self, l: BenchLoad) -> Result<(), u16> {
        if l.frames == 0 || l.hits > l.frames || l.shape > 3 {
            return Err(E_BENCH_MISS);
        }
        self.loads.push(l);
        Ok(())
    }

    pub fn record_overhead(&mut self, pm: u32) {
        self.overhead_pm = Some(pm);
    }

    /// 总命中率（千分位）。
    pub fn total_hit_rate_pm(&self) -> Option<u32> {
        let f: u64 = self.loads.iter().map(|l| l.frames as u64).sum();
        let h: u64 = self.loads.iter().map(|l| l.hits as u64).sum();
        if f == 0 {
            None
        } else {
            Some(((h * 1000) / f) as u32)
        }
    }

    /// 入册回归（判据「两基准」）：命中率达标 + 开销不越线。
    pub fn regression_ok(&self) -> Result<(), u16> {
        let rate = self.total_hit_rate_pm().ok_or(E_BENCH_MISS)?;
        if rate < HIT_RATE_TARGET_PM {
            return Err(E_REGRESSION);
        }
        let oh = self.overhead_pm.ok_or(E_BENCH_MISS)?;
        if oh > OVERHEAD_LIMIT_PM {
            return Err(E_BENCH_OVERHEAD);
        }
        // 每谱型至少一条负载。
        for shape in 0..=3u8 {
            if !self.loads.iter().any(|l| l.shape == shape) {
                return Err(E_BENCH_MISS);
            }
        }
        Ok(())
    }

    pub fn loads(&self) -> &[BenchLoad] {
        &self.loads
    }
}

impl Default for BenchLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 入册总核：两文档全过 + 两基准全过（判据总入口）。
pub fn registry_gate(white: &DocBook, guide: &DocBook, bench: &BenchLedger) -> Result<(), u16> {
    white.complete()?;
    guide.complete()?;
    bench.regression_ok()
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn filled_white() -> DocBook {
        let mut d = DocBook::whitepaper();
        d.write("两段架构", 55).unwrap();
        d.write("画像层", 40).unwrap();
        d.write("预测层", 60).unwrap();
        d.write("预留层", 35).unwrap();
        d.write("降级与兜底", 30).unwrap();
        d.write("诚实边界", 22).unwrap();
        d
    }

    #[test]
    fn a83_两文档_完整性与缺章必拒() {
        let w = filled_white();
        assert!(w.complete().is_ok());
        assert_eq!(w.name, "预测架构白皮书");
        // 缺一章即拒。
        let mut thin = DocBook::whitepaper();
        thin.write("两段架构", 50).unwrap();
        assert_eq!(thin.complete().unwrap_err(), E_DOC_INCOMPLETE);
        // 行数不达标即拒。
        let mut short = filled_white();
        short.write("预测层", 10).unwrap();
        assert_eq!(short.complete().unwrap_err(), E_DOC_INCOMPLETE);
        // 未知章节拒收。
        let mut g = DocBook::tuning_guide();
        assert_eq!(g.write("鬼章节", 100).unwrap_err(), E_DOC_INCOMPLETE);
        // 调参指南全填。
        g.write("EWMA 系数", 30).unwrap();
        g.write("区间宽度", 28).unwrap();
        g.write("降频因子", 16).unwrap();
        g.write("阈值速查表", 25).unwrap();
        assert!(g.complete().is_ok());
    }

    #[test]
    fn a83_命中率基准_四谱型达标() {
        let mut b = BenchLedger::new();
        let rows = [(0u8, 900u32, 1000u32), (1, 760, 1000), (2, 705, 1000), (3, 720, 1000)];
        for (shape, hits, frames) in rows {
            assert!(b.record_load(BenchLoad { scene: 1, shape, frames, hits }).is_ok());
        }
        // 总命中率 = (900+760+705+720)/4000 = 771‰ ≥ 700‰。
        assert_eq!(b.total_hit_rate_pm(), Some(771));
        b.record_overhead(3);
        assert!(b.regression_ok().is_ok());
        // 缺谱型必拒。
        let mut thin = BenchLedger::new();
        assert!(thin.record_load(BenchLoad { scene: 1, shape: 0, frames: 100, hits: 95 }).is_ok());
        assert_eq!(thin.regression_ok().unwrap_err(), E_BENCH_MISS);
        // 非法行拒收。
        assert_eq!(b.record_load(BenchLoad { scene: 1, shape: 9, frames: 10, hits: 1 }).unwrap_err(), E_BENCH_MISS);
        assert_eq!(b.record_load(BenchLoad { scene: 1, shape: 0, frames: 10, hits: 11 }).unwrap_err(), E_BENCH_MISS);
    }

    #[test]
    fn a83_开销红线与入册总核() {
        let mut b = BenchLedger::new();
        for shape in 0..=3u8 {
            b.record_load(BenchLoad { scene: 2, shape, frames: 500, hits: 400 }).unwrap();
        }
        b.record_overhead(9); // 超 5‰ 红线
        assert_eq!(b.regression_ok().unwrap_err(), E_BENCH_OVERHEAD);
        b.record_overhead(4);
        assert!(b.regression_ok().is_ok());
        // 命中率不达标。
        let mut low = BenchLedger::new();
        for shape in 0..=3u8 {
            low.record_load(BenchLoad { scene: 2, shape, frames: 500, hits: 300 }).unwrap();
        }
        low.record_overhead(2);
        assert_eq!(low.regression_ok().unwrap_err(), E_REGRESSION);
        // 总核：文档缺一即拒。
        let g = DocBook::tuning_guide();
        assert_eq!(registry_gate(&filled_white(), &g, &b).unwrap_err(), E_DOC_INCOMPLETE);
        // 总核全过。
        let mut g2 = DocBook::tuning_guide();
        g2.write("EWMA 系数", 25).unwrap();
        g2.write("区间宽度", 25).unwrap();
        g2.write("降频因子", 15).unwrap();
        g2.write("阈值速查表", 20).unwrap();
        assert!(registry_gate(&filled_white(), &g2, &b).is_ok());
        // 常量与 F0379 预算一致。
        assert_eq!(OVERHEAD_LIMIT_PM, 5);
        assert_eq!(HIT_RATE_TARGET_PM, 700);
    }
}
