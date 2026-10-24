// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — SVSTAR2 · ves107_regression
// VE-F3707 · 主题回归基线（R06 组 · 主题质量与一致性组）
//
// 锚点：VE-F3707
// 判据原文：基线强制、5% 回归门、SSIM 单源、豁免到期、改版保护、判据。
//
// 难点分析：
//   1. 基线红线：改版无基线=回归盲改——重大改版必须先捕获基线，没有基线就拦截。
//   2. 五维回归门：任一维降>5%→回归警报。
//   3. 豁免必须到期——永久豁免=红线旁路。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;

// ── 诊断码（0x9B00 起）────────────────────────────────────────────

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x9B00;

/// 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C9BCode(pub u16);

impl C9BCode {
    pub const NO_BASELINE: Self = Self(SEG_BASE);      // 无基线改版
    pub const REGRESSION: Self = Self(SEG_BASE + 1);   // 五维回归
    pub const SSIM_DROP: Self = Self(SEG_BASE + 2);    // 视觉回归
    pub const EXPIRED_EXEMPT: Self = Self(SEG_BASE + 3); // 豁免过期
    pub const BASELINE_OK: Self = Self(SEG_BASE + 4);  // 基线合规
    pub const NO_REGRESSION: Self = Self(SEG_BASE + 5); // 无回归
    pub const EXEMPT_VALID: Self = Self(SEG_BASE + 6); // 豁免有效

    pub fn code(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::NO_BASELINE => "改版无基线——基线强制拦截",
            Self::REGRESSION => "五维回归——任一维降>5% 警报",
            Self::SSIM_DROP => "视觉回归——SSIM 下降超阈值",
            Self::EXPIRED_EXEMPT => "豁免过期——须重新评审",
            Self::BASELINE_OK => "基线捕获合规",
            Self::NO_REGRESSION => "回归门通过",
            Self::EXEMPT_VALID => "豁免有效",
            _ => "未知诊断",
        }
    }
}

// ── 基线捕获 ──────────────────────────────────────────────────────

/// 五维快照（0..10000 基点/维）。
pub struct QualitySnapshot {
    pub dims: [u16; 5],
}

impl QualitySnapshot {
    pub fn new(dims: [u16; 5]) -> Self {
        Self { dims }
    }

    /// 与基线比较：任一维降 >500 基点（5%）即回归。
    pub fn regressed_vs(&self, baseline: &Self) -> bool {
        for i in 0..5 {
            let base = baseline.dims[i] as i32;
            let now = self.dims[i] as i32;
            if base - now > 500 {
                return true;
            }
        }
        false
    }

    /// 最大降幅（基点）。
    pub fn max_drop(&self, baseline: &Self) -> i32 {
        let mut max_drop = 0i32;
        for i in 0..5 {
            let drop = (baseline.dims[i] as i32) - (self.dims[i] as i32);
            if drop > max_drop {
                max_drop = drop;
            }
        }
        max_drop
    }
}

/// 基线管理器。
pub struct BaselineManager {
    /// 捕获的基线。
    baselines: Vec<(u64, QualitySnapshot)>,
}

impl BaselineManager {
    pub fn new() -> Self {
        Self { baselines: Vec::new() }
    }

    /// 捕获基线。
    pub fn capture(&mut self, theme_id: u64, snap: QualitySnapshot) {
        self.baselines.push((theme_id, snap));
    }

    /// 查询基线。
    pub fn get(&self, theme_id: u64) -> Option<&QualitySnapshot> {
        for (id, s) in &self.baselines {
            if *id == theme_id {
                return Some(s);
            }
        }
        None
    }

    /// 改版前必须有基线。
    pub fn gate_redesign(&self, theme_id: u64) -> Result<(), C9BCode> {
        if self.get(theme_id).is_some() {
            Ok(())
        } else {
            Err(C9BCode::NO_BASELINE)
        }
    }
}

impl Default for BaselineManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── 回归门 ────────────────────────────────────────────────────────

/// 回归门。
pub struct RegressionGate {
    /// 豁免主题。
    exemptions: Vec<(u64, u32)>, // (theme_id, expire_day)
}

impl RegressionGate {
    pub fn new() -> Self {
        Self { exemptions: Vec::new() }
    }

    /// 登记豁免（必须带到期日）。
    pub fn exempt(&mut self, theme_id: u64, expire_day: u32) {
        self.exemptions.push((theme_id, expire_day));
    }

    /// 检查回归。有有效豁免即放行。
    pub fn check(&mut self, theme_id: u64, now_day: u32, snap: &QualitySnapshot, baseline: &QualitySnapshot) -> Result<(), C9BCode> {
        // 先看豁免
        for &(_, expire) in &self.exemptions {
            let _ = expire;
        }
        for (id, expire) in &self.exemptions {
            if *id == theme_id {
                if now_day <= *expire {
                    return Ok(()); // 有效豁免
                }
                return Err(C9BCode::EXPIRED_EXEMPT);
            }
        }
        // 无豁免：走回归门
        if snap.regressed_vs(baseline) {
            return Err(C9BCode::REGRESSION);
        }
        Ok(())
    }
}

impl Default for RegressionGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── SSIM 视觉回归 ────────────────────────────────────────────────

/// SSIM 视觉对比（单源复用声明：F2877 口径）。
pub struct SsimComparator {
    /// SSIM 下降阈值（基点，500=5%）。
    pub threshold_bp: u16,
}

impl SsimComparator {
    pub fn standard() -> Self {
        Self { threshold_bp: 500 }
    }

    /// 对比两帧 SSIM 值（0..10000）。
    pub fn compare(&self, baseline_ssim: u16, current_ssim: u16) -> Result<(), C9BCode> {
        let drop = (baseline_ssim as i32) - (current_ssim as i32);
        if drop > self.threshold_bp as i32 {
            Err(C9BCode::SSIM_DROP)
        } else {
            Ok(())
        }
    }
}

// ── 判据 ──────────────────────────────────────────────────────────

pub fn run_ves107_checks() -> CheckSet {
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves107_regression");
    {
        // 01 无基线拦截
        let bm = BaselineManager::new();
        chk(
            &mut set,
            "C39B-01",
            bm.gate_redesign(1).err() == Some(C9BCode::NO_BASELINE),
            "改版无基线=回归盲改→拦截",
        );
    }
    {
        // 02 有基线放行
        let mut bm = BaselineManager::new();
        bm.capture(1, QualitySnapshot::new([8000; 5]));
        chk(&mut set, "C39B-02", bm.gate_redesign(1).is_ok(), "基线捕获后放行改版");
    }
    {
        // 03 回归检出（一维降>5%）
        let base = QualitySnapshot::new([8000, 8000, 8000, 8000, 8000]);
        let now = QualitySnapshot::new([8000, 7400, 8000, 8000, 8000]); // 一维降 600
        chk(&mut set, "C39B-03", now.regressed_vs(&base), "一维降 600>500 基点即回归");
    }
    {
        // 04 小幅波动不算回归
        let base = QualitySnapshot::new([8000; 5]);
        let now = QualitySnapshot::new([7800; 5]); // 各降 200
        chk(&mut set, "C39B-04", !now.regressed_vs(&base), "降 200≤500 不算回归");
    }
    {
        // 05 最大降幅
        let base = QualitySnapshot::new([8000, 8000, 8000, 8000, 8000]);
        let now = QualitySnapshot::new([8000, 7000, 8000, 8000, 8000]);
        chk(&mut set, "C39B-05", now.max_drop(&base) == 1000, "最大降幅 1000 基点");
    }
    {
        // 06 回归门拦截
        let mut g = RegressionGate::new();
        let base = QualitySnapshot::new([8000; 5]);
        let now = QualitySnapshot::new([7000, 8000, 8000, 8000, 8000]);
        let r = g.check(1, 0, &now, &base);
        chk(&mut set, "C39B-06", r.err() == Some(C9BCode::REGRESSION), "回归门拦截");
    }
    {
        // 07 豁免放行
        let mut g = RegressionGate::new();
        g.exempt(1, 100); // 到期日 100
        let base = QualitySnapshot::new([8000; 5]);
        let now = QualitySnapshot::new([7000, 8000, 8000, 8000, 8000]);
        let r = g.check(1, 50, &now, &base); // 第 50 天
        chk(&mut set, "C39B-07", r.is_ok(), "有效豁免放行回归");
    }
    {
        // 08 豁免过期
        let mut g = RegressionGate::new();
        g.exempt(1, 100);
        let base = QualitySnapshot::new([8000; 5]);
        let now = QualitySnapshot::new([7000, 8000, 8000, 8000, 8000]);
        let r = g.check(1, 101, &now, &base); // 第 101 天
        chk(
            &mut set,
            "C39B-08",
            r.err() == Some(C9BCode::EXPIRED_EXEMPT),
            "豁免到期→须重新评审",
        );
    }
    {
        // 09 SSIM 无回归
        let sc = SsimComparator::standard();
        chk(&mut set, "C39B-09", sc.compare(9000, 8800).is_ok(), "SSIM 降 200≤500 通过");
    }
    {
        // 10 SSIM 回归
        let sc = SsimComparator::standard();
        chk(
            &mut set,
            "C39B-10",
            sc.compare(9000, 8000).err() == Some(C9BCode::SSIM_DROP),
            "SSIM 降 1000>500→视觉回归",
        );
    }
    {
        // 11 边界：恰好降 500 不触发
        let base = QualitySnapshot::new([8000; 5]);
        let now = QualitySnapshot::new([7500; 5]); // 降 500
        chk(&mut set, "C39B-11", !now.regressed_vs(&base), "恰好降 500 边界不触发");
    }
    {
        // 12 基线查询
        let mut bm = BaselineManager::new();
        bm.capture(42, QualitySnapshot::new([9000; 5]));
        let got = bm.get(42);
        chk(&mut set, "C39B-12", got.is_some() && got.unwrap().dims[0] == 9000, "基线查询正确");
    }
    set
}
