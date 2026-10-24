//! CGPU-F0422 · 季节性模式（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 季节模式：时段性画像（工作/夜间/周末负载差异的学习）；模式应用
//!   （预测器 F0372 的先验输入——按时段选画像）；模式失效（作息变化→
//!   模式过期回退）；模式可见（用户可查自己的负载作息画像——透明）。
//! - 判据：学习、先验应用、失效回退、可见、判据。
//!
//! 设计纪律：
//! - 时段轴：工作时段/夜间/周末三段（段号 0/1/2）；学习 = 各段独立
//!   EWMA 累积负载画像（与 C04 同构的权重向量），段间互不串账。
//! - 先验应用：预测器取先验 = 当前时段的段画像；段画像有置信计数，
//!   计数不足 → 回退到全时段平均（不装懂）。
//! - 失效：段画像与近期实测偏差持续超带 → 模式过期（标记 stale）→
//!   回退全时段平均并重新学习。
//! - 可见：用户可查（返回段画像 + 学习进度 + 是否 stale——透明）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D90 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D90，全仓独占）。
pub const SEG_BASE: u16 = 0x0D90;

pub const E_SEA_SEG: u16 = SEG_BASE + 1; // 时段非法
pub const E_SEA_WARMUP: u16 = SEG_BASE + 2; // 学习不足（置信未达先验门槛）
pub const E_SEA_STALE: u16 = SEG_BASE + 3; // 模式已过期仍被引用
pub const E_SEA_BAND: u16 = SEG_BASE + 4; // 失效带非法
pub const E_SEA_VEC: u16 = SEG_BASE + 5; // 权重向量维不一致
pub const E_SEA_SUM: u16 = SEG_BASE + 6; // 权重和越界（≠1000）

/// 时段三段。
pub const SEG_WORK: u8 = 0; // 工作时段
pub const SEG_NIGHT: u8 = 1; // 夜间
pub const SEG_WEEKEND: u8 = 2; // 周末
pub const SEG_COUNT: usize = 3;

/// 先验门槛：段画像置信计数 ≥ 此值才可直接用（否则回退均值）。
pub const PRIOR_MIN_CONF: u32 = 8;
/// 失效带（画像与实测偏差千分位）。
pub const STALE_BAND_PM: u32 = 400;

pub fn seg_name(s: u8) -> &'static str {
    match s {
        SEG_WORK => "工作时段",
        SEG_NIGHT => "夜间",
        _ => "周末",
    }
}

/// 段画像（EWMA 学习态）。
#[derive(Clone, Debug)]
pub struct SegProfile {
    /// EWMA 权重向量（千分位，和恒 1000）。
    pub weights: Vec<u32>,
    /// 置信计数（学习样本数）。
    pub conf: u32,
    /// 过期标记。
    pub stale: bool,
}

/// 季节模式引擎（单账户）。
#[derive(Clone, Debug)]
pub struct SeasonEngine {
    pub segs: [SegProfile; SEG_COUNT],
    /// 向量维数（全部段一致）。
    pub dims: usize,
    /// EWMA 平滑系数（千分位：new = old×(1-α) + obs×α）。
    pub alpha_pm: u32,
}

impl SeasonEngine {
    /// 初始化：三段同一起点画像（均匀分布）。
    pub fn new(dims: usize) -> Self {
        let unit = 1000 / dims.max(1) as u32;
        let mut w = vec![unit; dims];
        // 余数给首维（和恒 1000）。
        let rem = 1000 - unit * dims as u32;
        w[0] += rem;
        SeasonEngine {
            segs: [
                SegProfile { weights: w.clone(), conf: 0, stale: false },
                SegProfile { weights: w.clone(), conf: 0, stale: false },
                SegProfile { weights: w, conf: 0, stale: false },
            ],
            dims,
            alpha_pm: 200,
        }
    }

    /// 学习：观察一窗（时段 + 归一化权重向量）→ 对应段 EWMA 更新。
    pub fn learn(&mut self, seg: u8, obs: &[u32]) -> Result<(), u16> {
        if seg as usize >= SEG_COUNT {
            return Err(E_SEA_SEG);
        }
        if obs.len() != self.dims {
            return Err(E_SEA_VEC);
        }
        let sum: u32 = obs.iter().sum();
        if sum != 1000 {
            return Err(E_SEA_SUM);
        }
        let a = self.alpha_pm as u64;
        let p = &mut self.segs[seg as usize];
        for i in 0..self.dims {
            let old = p.weights[i] as u64;
            let nw = old * (1000 - a) / 1000 + obs[i] as u64 * a / 1000;
            p.weights[i] = nw as u32;
        }
        // 和微调（舍入余数回填首维）。
        let s: u32 = p.weights.iter().sum();
        let diff = 1000i64 - s as i64;
        if diff != 0 && !p.weights.is_empty() {
            let v = p.weights[0] as i64 + diff;
            if v >= 0 {
                p.weights[0] = v as u32;
            }
        }
        p.conf += 1;
        Ok(())
    }

    /// 先验应用：按时段取画像（预测器 F0372 的输入口）。
    /// 置信不足或已过期 → 回退全时段平均（Ok(None) 显性标注回退）。
    pub fn prior(&self, seg: u8) -> Result<(Vec<u32>, bool), u16> {
        if seg as usize >= SEG_COUNT {
            return Err(E_SEA_SEG);
        }
        let p = &self.segs[seg as usize];
        if p.stale {
            return Err(E_SEA_STALE);
        }
        if p.conf < PRIOR_MIN_CONF {
            return Ok((self.average_profile(), true)); // 回退
        }
        Ok((p.weights.clone(), false))
    }

    /// 全时段平均画像（回退目标）。
    pub fn average_profile(&self) -> Vec<u32> {
        let mut v = vec![0u32; self.dims];
        for p in &self.segs {
            for i in 0..self.dims {
                v[i] += p.weights[i];
            }
        }
        // 均值 + 舍入余数回填。
        for i in 0..self.dims {
            v[i] /= SEG_COUNT as u32;
        }
        let s: u32 = v.iter().sum();
        let diff = 1000i64 - s as i64;
        if diff != 0 && !v.is_empty() {
            let nv = v[0] as i64 + diff;
            if nv >= 0 {
                v[0] = nv as u32;
            }
        }
        v
    }

    /// 失效检测：段画像 vs 近期实测（归一化权重）偏差超带 → 标 stale。
    pub fn check_stale(&mut self, seg: u8, recent: &[u32]) -> Result<bool, u16> {
        if seg as usize >= SEG_COUNT {
            return Err(E_SEA_SEG);
        }
        if recent.len() != self.dims {
            return Err(E_SEA_VEC);
        }
        let p = &self.segs[seg as usize];
        // 偏差 = 各维绝对差之和（L1，千分位）。
        let mut dev = 0i64;
        for i in 0..self.dims {
            dev += (p.weights[i] as i64 - recent[i] as i64).abs();
        }
        // dev 已是千分位权重差的 L1 和——取平均即平均偏差（勿再 ×1000）。
        let dev_pm = if self.dims > 0 { dev / self.dims as i64 } else { 0 };
        let stale_now = dev_pm > STALE_BAND_PM as i64;
        if stale_now {
            self.segs[seg as usize].stale = true;
        }
        Ok(stale_now)
    }

    /// 过期回退重学习：清 stale、置信清零（从头累积）。
    pub fn reset_seg(&mut self, seg: u8) -> Result<(), u16> {
        if seg as usize >= SEG_COUNT {
            return Err(E_SEA_SEG);
        }
        let base = self.average_profile();
        let p = &mut self.segs[seg as usize];
        p.weights = base;
        p.conf = 0;
        p.stale = false;
        Ok(())
    }

    /// 模式可见：返回（段名/画像/置信/是否 stale）——用户透明口。
    pub fn visible(&self, seg: u8) -> Result<(&'static str, Vec<u32>, u32, bool), u16> {
        if seg as usize >= SEG_COUNT {
            return Err(E_SEA_SEG);
        }
        let p = &self.segs[seg as usize];
        Ok((seg_name(seg), p.weights.clone(), p.conf, p.stale))
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn eng() -> SeasonEngine {
        SeasonEngine::new(2)
    }

    #[test]
    fn a01_学习_三段独立EWMA互不串账() {
        let mut e = eng();
        // 工作段连续学习重 CPU（800/200）。
        for _ in 0..10 {
            e.learn(SEG_WORK, &[800, 200]).ok();
        }
        // 夜间段学轻负载（200/800）。
        for _ in 0..10 {
            e.learn(SEG_NIGHT, &[200, 800]).ok();
        }
        // 段间互不串账。
        assert!(e.segs[SEG_WORK as usize].weights[0] > 700);
        assert!(e.segs[SEG_NIGHT as usize].weights[0] < 300);
        assert!(e.segs[SEG_WEEKEND as usize].conf == 0);
        // 权重和恒 1000。
        for p in &e.segs {
            assert_eq!(p.weights.iter().sum::<u32>(), 1000);
        }
        // 向量维不一致拒。
        assert_eq!(e.learn(SEG_WORK, &[1000]), Err(E_SEA_VEC));
        // 和不为 1000 拒。
        assert_eq!(e.learn(SEG_WORK, &[600, 300]), Err(E_SEA_SUM));
    }

    #[test]
    fn a02_先验应用_置信不足回退均值() {
        let mut e = eng();
        // 置信 0 < 门槛 ⇒ 回退（Ok(true) 显性标注）。
        assert_eq!(e.prior(SEG_WORK), Ok((e.average_profile(), true)));
        // 学够门槛后直接用段画像。
        for _ in 0..PRIOR_MIN_CONF {
            e.learn(SEG_WORK, &[900, 100]).ok();
        }
        let (w, fell_back) = e.prior(SEG_WORK).unwrap_or((Vec::new(), false));
        assert!(!fell_back);
        assert!(w[0] > 800);
        // 未学的段仍回退。
        assert_eq!(e.prior(SEG_WEEKEND).map(|(_, f)| f), Ok(true));
    }

    #[test]
    fn a03_失效回退_偏差超带标stale重学习() {
        let mut e = eng();
        for _ in 0..10 {
            e.learn(SEG_WORK, &[900, 100]).ok();
        }
        // 作息变化：近期实测变 100/900（L1 偏差 = (800+800)/2 = 800 > 400 带）。
        assert_eq!(e.check_stale(SEG_WORK, &[100, 900]), Ok(true));
        // stale 后先验引用拒绝（不许装懂）。
        assert_eq!(e.prior(SEG_WORK), Err(E_SEA_STALE));
        // 重置重学习。
        e.reset_seg(SEG_WORK).ok();
        assert_eq!(e.segs[SEG_WORK as usize].stale, false);
        assert_eq!(e.segs[SEG_WORK as usize].conf, 0);
        // 偏差带内不标 stale。
        let mut e2 = eng();
        for _ in 0..10 {
            e2.learn(SEG_WORK, &[600, 400]).ok();
        }
        assert_eq!(e2.check_stale(SEG_WORK, &[550, 450]), Ok(false));
    }

    #[test]
    fn a04_可见_用户透明口() {
        let mut e = eng();
        for _ in 0..5 {
            e.learn(SEG_NIGHT, &[300, 700]).ok();
        }
        let (name, w, conf, stale) = e.visible(SEG_NIGHT).unwrap_or(("", Vec::new(), 0, false));
        assert_eq!(name, "夜间");
        assert_eq!(conf, 5);
        assert!(!stale);
        assert_eq!(w.len(), 2);
        // 段非法。
        assert_eq!(e.visible(9), Err(E_SEA_SEG));
        assert_eq!(e.check_stale(9, &[500, 500]), Err(E_SEA_SEG));
        assert_eq!(e.reset_seg(9), Err(E_SEA_SEG));
    }

    #[test]
    fn a05_均匀起点_和守恒与余数回填() {
        // 3 维均匀：333/333/333 +1 = 334/333/333（余数给首维）。
        let e = SeasonEngine::new(3);
        assert_eq!(e.segs[0].weights, vec![334, 333, 333]);
        for p in &e.segs {
            assert_eq!(p.weights.iter().sum::<u32>(), 1000);
        }
        // 回退画像同样守恒。
        let e2 = SeasonEngine::new(7);
        let avg = e2.average_profile();
        assert_eq!(avg.iter().sum::<u32>(), 1000);
    }
}
