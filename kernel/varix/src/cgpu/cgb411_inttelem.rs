//! CGPU-F0411 · 集成遥测（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 遥测：各集成点联动频率/故障率/降级时长/对账差异次数——四指标进
//!   B08 总线；按域分列可查。
//! - 判据：四指标、分列、判据。
//!
//! 设计纪律：
//! - 四指标账（每集成点一列）：联动次数（频率分子）、故障次数（率
//!   分子）、降级累计帧（时长）、对账差异次数；观测帧数为公共分母。
//! - B08 总线出口：四指标打包成总线帧（域号+四元组+观测窗），帧格式
//!   固化可复算（总线消费侧同构解析）。
//! - 按域分列：六点各一列，互不串账；列查询按点号直达。
//! - 率口径：故障率 = 故障×1000/联动；降级时长占比 = 降级帧×1000/观测帧。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CE0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CE0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CE0;

pub const E_TLM_POINT: u16 = SEG_BASE + 1; // 集成点非法
pub const E_TLM_WINDOW: u16 = SEG_BASE + 2; // 观测窗非法（0 或降级帧超窗）
pub const E_TLM_NEG: u16 = SEG_BASE + 3; // 指标回退（计数只增）
pub const E_TLM_BUS: u16 = SEG_BASE + 4; // 总线帧格式错

/// 六集成点（矩阵坐标同源）。
pub const TL_POINTS: usize = 6;

/// 单点四指标账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TlmRow {
    /// 联动次数。
    pub links: u64,
    /// 故障次数。
    pub faults: u64,
    /// 降级累计帧。
    pub degraded_frames: u64,
    /// 对账差异次数。
    pub recon_diffs: u64,
}

impl TlmRow {
    /// 联动频率：联动 ×1000 / 观测帧。
    pub fn link_rate_pm(&self, window: u64) -> u64 {
        if window == 0 {
            return 0;
        }
        self.links * 1000 / window
    }

    /// 故障率：故障 ×1000 / 联动（联动为 0 时尚无故障可言 → 0）。
    pub fn fault_rate_pm(&self) -> u64 {
        if self.links == 0 {
            return 0;
        }
        self.faults * 1000 / self.links
    }

    /// 降级时长占比：降级帧 ×1000 / 观测帧。
    pub fn degrade_ratio_pm(&self, window: u64) -> u64 {
        if window == 0 {
            return 0;
        }
        self.degraded_frames * 1000 / window
    }
}

/// B08 总线帧（域号 + 四元组 + 观测窗）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusFrame {
    pub point: u8,
    pub links: u64,
    pub faults: u64,
    pub degraded_frames: u64,
    pub recon_diffs: u64,
    pub window: u64,
}

/// 集成遥测总账。
#[derive(Clone, Debug)]
pub struct IntTlemetry {
    pub rows: [TlmRow; TL_POINTS],
    /// 观测窗（帧）。
    pub window: u64,
    /// 总线出口历史。
    pub bus: Vec<BusFrame>,
}

impl IntTlemetry {
    pub fn new(window: u64) -> Self {
        IntTlemetry {
            rows: [TlmRow::default(); TL_POINTS],
            window,
            bus: Vec::new(),
        }
    }

    /// 指标递增（计数只增防线：不允许回退）。
    pub fn bump(&mut self, point: u8, field: u8, n: u64) -> Result<(), u16> {
        if point as usize >= TL_POINTS {
            return Err(E_TLM_POINT);
        }
        if self.window == 0 {
            return Err(E_TLM_WINDOW);
        }
        let r = &mut self.rows[point as usize];
        let cur = match field {
            0 => &mut r.links,
            1 => &mut r.faults,
            2 => &mut r.degraded_frames,
            _ => &mut r.recon_diffs,
        };
        // 计数只增防线：溢出即拒（不回绕）。
        let nv = match cur.checked_add(n) {
            Some(v) => v,
            None => return Err(E_TLM_NEG),
        };
        if n > 0 && nv < *cur {
            return Err(E_TLM_NEG);
        }
        *cur = nv;
        // 降级帧不得超观测窗。
        if field == 2 && r.degraded_frames > self.window {
            return Err(E_TLM_WINDOW);
        }
        Ok(())
    }

    /// 观测窗推进（新窗累计照旧——四指标为窗内滚动累计，此处只校验窗合法）。
    pub fn rewindow(&mut self, window: u64) -> Result<(), u16> {
        if window == 0 {
            return Err(E_TLM_WINDOW);
        }
        if self.rows.iter().any(|r| r.degraded_frames > window) {
            return Err(E_TLM_WINDOW);
        }
        self.window = window;
        Ok(())
    }

    /// B08 总线出帧：六点各一帧，格式固化。
    pub fn emit_bus(&mut self) -> Result<usize, u16> {
        for (p, r) in self.rows.iter().enumerate() {
            let f = BusFrame {
                point: p as u8,
                links: r.links,
                faults: r.faults,
                degraded_frames: r.degraded_frames,
                recon_diffs: r.recon_diffs,
                window: self.window,
            };
            // 帧自检：域号合法 + 降级帧 ≤ 窗。
            if f.point as usize >= TL_POINTS || f.degraded_frames > f.window {
                return Err(E_TLM_BUS);
            }
            self.bus.push(f);
        }
        Ok(self.bus.len())
    }

    /// 按域分列查询：四元组直达。
    pub fn row_of(&self, point: u8) -> Result<TlmRow, u16> {
        if point as usize >= TL_POINTS {
            return Err(E_TLM_POINT);
        }
        Ok(self.rows[point as usize])
    }

    /// 全线健康：故障率 ≤ 带且降级占比 ≤ 带（千分位阈值）。
    pub fn healthy(&self, fault_band_pm: u64, degrade_band_pm: u64) -> bool {
        self.rows.iter().enumerate().all(|(_, r)| {
            r.fault_rate_pm() <= fault_band_pm
                && r.degrade_ratio_pm(self.window) <= degrade_band_pm
        })
    }

    /// 总线帧解析同构（消费侧对账：帧字段回填行应与账一致）。
    pub fn bus_roundtrip_ok(&self) -> bool {
        self.bus.iter().all(|f| {
            let r = self.rows[f.point as usize];
            r.links == f.links
                && r.faults == f.faults
                && r.degraded_frames == f.degraded_frames
                && r.recon_diffs == f.recon_diffs
                && f.window == self.window
        })
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgpu::cgb409_regmatrix::POINT_D;

    #[test]
    fn a01_四指标_计数与口径() {
        let mut t = IntTlemetry::new(1_000);
        t.bump(POINT_D, 0, 120).ok(); // 联动 120
        t.bump(POINT_D, 1, 3).ok(); // 故障 3
        t.bump(POINT_D, 2, 50).ok(); // 降级 50 帧
        t.bump(POINT_D, 3, 2).ok(); // 对账差异 2
        let r = t.row_of(POINT_D).ok();
        assert_eq!(r.map(|r| r.links), Some(120));
        assert_eq!(r.map(|r| r.fault_rate_pm()), Some(25)); // 3×1000/120
        assert_eq!(r.map(|r| r.link_rate_pm(1_000)), Some(120));
        assert_eq!(r.map(|r| r.degrade_ratio_pm(1_000)), Some(50));
        // 点越界拒。
        assert_eq!(t.bump(9, 0, 1), Err(E_TLM_POINT));
    }

    #[test]
    fn a02_分列_六点互不串账() {
        let mut t = IntTlemetry::new(1_000);
        t.bump(0, 0, 10).ok();
        t.bump(3, 0, 99).ok();
        t.bump(5, 1, 1).ok();
        // 各列独立。
        assert_eq!(t.row_of(0).ok().map(|r| r.links), Some(10));
        assert_eq!(t.row_of(3).ok().map(|r| r.links), Some(99));
        assert_eq!(t.row_of(5).ok().map(|r| r.links), Some(0));
        assert_eq!(t.row_of(5).ok().map(|r| r.faults), Some(1));
        assert_eq!(t.row_of(1).ok().map(|r| r.links), Some(0));
    }

    #[test]
    fn a03_B08总线_出帧与同构回读() {
        let mut t = IntTlemetry::new(2_000);
        t.bump(0, 0, 200).ok();
        t.bump(2, 2, 100).ok();
        assert_eq!(t.emit_bus(), Ok(6));
        assert_eq!(t.bus.len(), 6);
        assert!(t.bus_roundtrip_ok());
        // 帧序按点号排列。
        for (i, f) in t.bus.iter().enumerate() {
            assert_eq!(f.point as usize, i);
        }
        // 帧字段固化：点 2 的降级帧 = 100。
        assert_eq!(t.bus[2].degraded_frames, 100);
        assert_eq!(t.bus[2].window, 2_000);
    }

    #[test]
    fn a04_健康线_故障带与降级带() {
        let mut t = IntTlemetry::new(1_000);
        // 点 0：联动 100 故障 1（10‰）降级 20（20‰）。
        t.bump(0, 0, 100).ok();
        t.bump(0, 1, 1).ok();
        t.bump(0, 2, 20).ok();
        // 点 1：联动 100 故障 0 降级 10。
        t.bump(1, 0, 100).ok();
        t.bump(1, 2, 10).ok();
        assert!(t.healthy(20, 30)); // 带内
        assert!(!t.healthy(5, 30)); // 故障率超带
        assert!(!t.healthy(20, 10)); // 降级占比超带
        // 降级帧超窗拒绝。
        assert_eq!(t.bump(2, 2, 2_000), Err(E_TLM_WINDOW));
        // 零窗非法。
        let mut z = IntTlemetry::new(1_000);
        z.window = 0;
        assert_eq!(z.bump(0, 0, 1), Err(E_TLM_WINDOW));
        assert_eq!(z.rewindow(0), Err(E_TLM_WINDOW));
    }

    #[test]
    fn a05_窗重设_降级帧不得超新窗() {
        let mut t = IntTlemetry::new(100);
        t.bump(1, 2, 80).ok();
        // 新窗 90 ≥ 80 放行。
        assert_eq!(t.rewindow(90), Ok(()));
        // 新窗 50 < 80 拒。
        assert_eq!(t.rewindow(50), Err(E_TLM_WINDOW));
        assert_eq!(t.window, 90);
    }
}
