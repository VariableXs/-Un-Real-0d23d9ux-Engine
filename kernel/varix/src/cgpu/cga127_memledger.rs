//! cga127_memledger — CGPU-F0127 A 域内存总账。
//!
//! 判据原文（施工书锚点 CGPU-F0127）：A 域内存总账（F0384 范式）：静态
//! （线程池结构/任务图池/分配器结构）与动态峰值（压测）分账，总红线
//! ≤16MB 常驻（CPU 渲染基础设施轻量化）。判据：分账、红线、压测归零
//! ±5%、超标清单、入证据。
//!
//! **设计定盘**：静态账（组件 → 常驻字节）与动态账（压测峰值逐点记录）
//! 分列。总红线 16MB（16 × 1024 × 1024 字节）作用于常驻面（静态合计 +
//! 动态峰值的常驻余量口径：常驻 = 静态合计 + 动态峰值中标记 persistent
//! 的部分；瞬态峰值不占常驻但计入压测归零核验）。压测归零 ±5% = 压测
//! 结束后的余量必须回到压测前基线的 ±5% 内（泄漏检测的账面化）。超标
//! 清单 = 超红线组件的显性清单（不许笼统报"超标"——必须点名）。
//!
//! 诊断码段 0x01D0~0x01DF（0x01 段第 14 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA127_VERSION: &str = "cga127-memledger-v1";

/// 诊断子区基值（0x01 段第 14 子区）。
pub const SEG_BASE: u16 = 0x01D0;

/// 常驻红线（16 MiB）。
pub const RESIDENT_REDLINE: u64 = 16 * 1024 * 1024;

/// 压测归零容差（千分比 5%）。
pub const LEAK_TOL_PERMILLE: u64 = 50;

/// 静态账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StaticRow {
    /// 组件号。
    pub comp: u32,
    /// 常驻字节。
    pub bytes: u64,
}

/// 动态峰值行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeakRow {
    /// 压测场景号。
    pub scene: u32,
    /// 峰值字节。
    pub peak_bytes: u64,
    /// 是否常驻（persistent 标记）。
    pub persistent: bool,
}

/// 内存总账。
pub struct MemLedger {
    /// 静态账。
    statics: Vec<StaticRow>,
    /// 动态账。
    peaks: Vec<PeakRow>,
    /// 压测前基线（字节）。
    baseline: Option<u64>,
    /// 压测后余量（字节）。
    residual: Option<u64>,
    /// 超标清单（缓存，核验时生成）。
    over_list: Vec<u32>,
    /// 封账。
    sealed: Option<u64>,
}

impl MemLedger {
    /// 空账。
    pub fn new() -> Self {
        MemLedger {
            statics: Vec::new(),
            peaks: Vec::new(),
            baseline: None,
            residual: None,
            over_list: Vec::new(),
            sealed: None,
        }
    }

    /// 静态记账（判据"分账"：组件重复即拒；零字节拒）。
    pub fn record_static(&mut self, comp: u32, bytes: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if bytes == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.statics.iter().any(|s| s.comp == comp) {
            return Err(SEG_BASE + 3);
        }
        self.statics.push(StaticRow { comp, bytes });
        Ok(())
    }

    /// 动态峰值记账（判据"压测"：场景重复即拒；零峰值拒）。
    pub fn record_peak(&mut self, scene: u32, peak_bytes: u64, persistent: bool) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if peak_bytes == 0 {
            return Err(SEG_BASE + 2);
        }
        if self.peaks.iter().any(|p| p.scene == scene) {
            return Err(SEG_BASE + 4);
        }
        self.peaks.push(PeakRow {
            scene,
            peak_bytes,
            persistent,
        });
        Ok(())
    }

    /// 静态合计。
    pub fn static_sum(&self) -> u64 {
        self.statics.iter().map(|s| s.bytes).sum()
    }

    /// 常驻合计（静态 + 持久动态）。
    pub fn resident_sum(&self) -> u64 {
        self.static_sum()
            + self
                .peaks
                .iter()
                .filter(|p| p.persistent)
                .map(|p| p.peak_bytes)
                .sum::<u64>()
    }

    /// 红线核验（判据"红线"：常驻 ≤ 16MiB；超线生成超标清单）。
    pub fn redline_check(&mut self) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        self.over_list.clear();
        if self.resident_sum() > RESIDENT_REDLINE {
            // 超标点名：静态逐行 + 持久动态逐行，按字节降序累积到超线为止。
            let mut cands: Vec<(u32, u64)> = self
                .statics
                .iter()
                .map(|s| (s.comp, s.bytes))
                .chain(
                    self.peaks
                        .iter()
                        .filter(|p| p.persistent)
                        .map(|p| (p.scene, p.peak_bytes)),
                )
                .collect();
            cands.sort_by(|a, b| b.1.cmp(&a.1));
            let over = self.resident_sum() - RESIDENT_REDLINE;
            let mut acc = 0u64;
            for (id, bytes) in cands {
                self.over_list.push(id);
                acc += bytes;
                if acc >= over {
                    break;
                }
            }
            return Err(SEG_BASE + 5);
        }
        Ok(())
    }

    /// 超标清单查询。
    pub fn over_item(&self, idx: usize) -> Option<u32> {
        self.over_list.get(idx).copied()
    }

    /// 压测归零核验（判据"压测归零 ±5%"：基线与余量都已登记且
    /// 偏差在容差内——泄漏检测的账面化）。
    pub fn leak_check(&mut self, baseline: u64, residual: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 1);
        }
        if baseline == 0 {
            return Err(SEG_BASE + 6);
        }
        self.baseline = Some(baseline);
        self.residual = Some(residual);
        let diff = if residual > baseline {
            residual - baseline
        } else {
            baseline - residual
        };
        if diff * 1000 > baseline * LEAK_TOL_PERMILLE {
            return Err(SEG_BASE + 7);
        }
        Ok(())
    }

    /// 行数。
    pub fn static_count(&self) -> usize {
        self.statics.len()
    }

    /// 封账（判据"入证据"：红线过 + 泄漏核验过才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 8);
        }
        if self.baseline.is_none() || self.residual.is_none() {
            return Err(SEG_BASE + 9);
        }
        self.redline_check()?;
        if self.statics.is_empty() {
            return Err(SEG_BASE + 10);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for s in &self.statics {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ s.bytes;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for MemLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 建议削减量（判据"超标清单"的处置面）：回到红线内需要削减的字节
/// 数（未超线返回 0）。
pub fn suggested_cut(resident: u64, redline: u64) -> u64 {
    resident.saturating_sub(redline)
}

/// 静态构成投影行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StaticShare {
    /// 组件号。
    pub comp: u32,
    /// 占静态合计的千分比。
    pub share_permille: u64,
}

impl MemLedger {
    /// 动态峰值最大值（判据"动态峰值"的聚合面；无动态账返回 None）。
    pub fn max_peak(&self) -> Option<u64> {
        self.peaks.iter().map(|p| p.peak_bytes).max()
    }

    /// 持久动态合计（常驻口径的动态面独立读数）。
    pub fn persistent_dynamic_sum(&self) -> u64 {
        self.peaks
            .iter()
            .filter(|p| p.persistent)
            .map(|p| p.peak_bytes)
            .sum()
    }

    /// 泄漏余量千分比（|余量-基线| / 基线；未做泄漏核验返回 None）。
    pub fn leak_delta_permille(&self) -> Option<u64> {
        let b = self.baseline?;
        let r = self.residual?;
        if b == 0 {
            return None;
        }
        let diff = if r > b { r - b } else { b - r };
        Some(diff * 1000 / b)
    }

    /// 静态构成投影（判据"分账"的占比面：逐组件占静态合计千分比，
    /// 静态合计为零时返回空表）。
    pub fn static_shares(&self) -> Vec<StaticShare> {
        let total = self.static_sum();
        if total == 0 {
            return Vec::new();
        }
        self.statics
            .iter()
            .map(|s| StaticShare {
                comp: s.comp,
                share_permille: s.bytes * 1000 / total,
            })
            .collect()
    }

    /// 超标清单长度。
    pub fn over_count(&self) -> usize {
        self.over_list.len()
    }

    /// 建议削减量（方法面包装 suggested_cut）。
    pub fn cut_advice(&self) -> u64 {
        suggested_cut(self.resident_sum(), RESIDENT_REDLINE)
    }

    /// 压测场景数。
    pub fn peak_count(&self) -> usize {
        self.peaks.len()
    }
}
