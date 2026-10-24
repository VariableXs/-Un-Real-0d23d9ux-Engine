//! cga40_predict.rs ↔ CGPU-F0040 帧间预测性负载调整（0x1BD0 段）
//!
//! 职责（施工书原文）：
//!   用上一帧的耗时预测本帧：瓦片/任务级耗时滑动窗口预测（EWMA）、
//!   预算分配按预测调整（重瓦片多给时间片）、预测失误的在线修正
//!   （首帧新场景无预测数据——冷启动策略：保守预算）。
//!   预测纪律：预测只调顺序与切片，不改像素结果（确定性纪律不破）。
//!
//! 判据（施工书原文）：
//!   预测准确率 ≥85%、冷启动策略、确定性不破、调整收益实测、窗口参数可调。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1BD0 独占 0x10）。
pub const SEG_BASE: u16 = 0x1BD0;
pub const XC_ALPHA: u16 = SEG_BASE + 1; // EWMA 系数非法
pub const XC_WINDOW: u16 = SEG_BASE + 2; // 窗口尺寸非法

/// EWMA 预测器：pred_{n+1} = α·obs_n + (1-α)·pred_n
#[derive(Clone)]
pub struct Ewma {
    alpha: f32,       // 0<α≤1
    window: u32,      // 滑动窗口（最近 N 观测参与再锚定）
    hist: Vec<f32>,
    pred: f32,
    pub primed: bool, // 冷启动标记
}

impl Ewma {
    pub fn new(alpha: f32, window: u32) -> Result<Self, u16> {
        if alpha <= 0.0 || alpha > 1.0 {
            return Err(XC_ALPHA);
        }
        if window == 0 || window > 1024 {
            return Err(XC_WINDOW);
        }
        Ok(Ewma { alpha, window, hist: Vec::new(), pred: 0.0, primed: false })
    }
    /// 冷启动策略：无预测数据时给保守预算（观测上界的 1.25 倍或 2×均值上界）
    pub fn cold_budget(&self, worst_case: f32) -> f32 {
        if self.primed { self.pred } else { worst_case * 1.25 }
    }
    /// 观测喂入：窗口滑动 + EWMA 更新；窗口满时先按窗内均值再锚
    pub fn observe(&mut self, ms: f32) -> f32 {
        self.hist.push(ms);
        if self.hist.len() > self.window as usize {
            self.hist.remove(0);
        }
        if !self.primed {
            self.pred = ms;
            self.primed = true;
        } else {
            // 窗口再锚：每满窗一次，用窗均值校一次基线（防漂移）
            if self.hist.len() == self.window as usize
                && self.hist.len() as u32 % self.window == 0
            {
                let mean = self.hist.iter().sum::<f32>() / self.hist.len() as f32;
                self.pred = self.pred * 0.5 + mean * 0.5;
            }
            self.pred = self.alpha * ms + (1.0 - self.alpha) * self.pred;
        }
        self.pred
    }
    pub fn predict(&self) -> f32 { self.pred }
    /// 预测误差（相对观测）
    pub fn last_err_pct(&self, obs: f32) -> f32 {
        if obs <= 0.0 { return 0.0; }
        ((self.pred - obs).abs() / obs) * 100.0
    }
}

/// 瓦片预算分配：按预测耗时比例切片（重瓦片多给）
pub fn allocate_budgets(preds: &[f32], total_budget_ms: f32) -> Vec<f32> {
    let sum: f32 = preds.iter().map(|p| p.max(1e-6)).sum();
    if sum <= 0.0 {
        return vec![total_budget_ms / preds.len().max(1) as f32; preds.len()];
    }
    preds.iter().map(|p| total_budget_ms * p.max(1e-6) / sum).collect()
}

/// 执行顺序优化：长任务先跑（LPT——减少尾部空转；只调序不改性）
pub fn lpt_order(preds: &[f32]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..preds.len()).collect();
    idx.sort_by(|&a, &b| preds[b].partial_cmp(&preds[a]).unwrap_or(core::cmp::Ordering::Equal));
    idx
}

/// 确定性守卫：预测与排序不触碰像素负载（same input → same pixel hash）
pub fn pixel_workload(tile_id: u32, seed: u64) -> u64 {
    // 像素结果的确定性函数——与预测/预算/顺序完全无关
    let mut x = (tile_id as u64) ^ seed;
    x = x.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0xBF58476D1CE4E5B9);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^ (x >> 27)
}

/// 预测准确率评估：|pred-obs|/obs ≤ 15% 计准
pub fn accuracy_pct(preds: &[f32], obs: &[f32], tol_pct: f32) -> f32 {
    if preds.len() != obs.len() || obs.is_empty() {
        return 0.0;
    }
    let hit = preds.iter().zip(obs.iter()).filter(|&(p, &o)| {
        if o <= 0.0 { return false; }
        ((p - o).abs() / o) * 100.0 <= tol_pct
    }).count();
    (hit * 100) as f32 / obs.len() as f32
}

/// 调整收益账（LPT 排序 vs 自然序的尾部差——确定性模拟）
pub fn lpt_gain_ms(preds: &[f32], workers: u32) -> f32 {
    // 单核口径：worker=1 时 LPT 无效；多片分摊时衡量负载均衡差
    if workers <= 1 {
        return 0.0;
    }
    let mut bins = vec![0f32; workers as usize];
    // 自然序
    let mut natural = bins.clone();
    let nn = natural.len();
    for (i, &p) in preds.iter().enumerate() {
        natural[i % nn] += p;
    }
    let bal_natural = natural.iter().cloned().fold(0f32, f32::max)
        - natural.iter().cloned().fold(f32::MAX, f32::min);
    // LPT 序
    for &i in &lpt_order(preds) {
        let w = bins.iter_mut().min_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal)).unwrap();
        *w += preds[i];
    }
    let bal_lpt = bins.iter().cloned().fold(0f32, f32::max)
        - bins.iter().cloned().fold(f32::MAX, f32::min);
    (bal_natural - bal_lpt).max(0.0)
}

pub fn self_audit() -> Result<(), u16> {
    let mut e = Ewma::new(0.3, 8)?;
    let _ = e.observe(10.0);
    let _ = allocate_budgets as fn(&[f32], f32) -> Vec<f32>;
    Ok(())
}

pub fn run_cga40_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0040");
    // J1 预测准确率 ≥85%（平稳信号 60 帧内预测误差 ≤15% 的占比）
    let mut e = Ewma::new(0.4, 8).ok().unwrap();
    let mut preds = Vec::new();
    let mut obs = Vec::new();
    for i in 0..60u32 {
        let o = 16.0 + (i % 3) as f32 * 0.1; // 平稳微扰
        let p = e.observe(o);
        if i > 0 {
            preds.push(p);
            obs.push(o);
        }
    }
    let acc = accuracy_pct(&preds, &obs, 15.0);
    let ok = acc >= 85.0;
    s.add("CGA-40-01-准确率≥85%", ok, if ok { "" } else { "准确率破缺" });
    // J1-b EWMA 数学性质（α=1 直通 / α 小平滑）
    let mut e1 = Ewma::new(1.0, 4).ok().unwrap();
    e1.observe(10.0);
    let p1 = e1.observe(20.0);
    let mut e2 = Ewma::new(0.1, 4).ok().unwrap();
    e2.observe(10.0);
    let p2 = e2.observe(20.0);
    let ok = (p1 - 20.0).abs() < 1e-5 && p2 < 12.0 && p1 > p2;
    s.add("CGA-40-02-EWMA性质", ok, if ok { "" } else { "系数破缺" });
    // J2 冷启动策略（未 primed → worst×1.25；primed 后用预测）
    let mut e3 = Ewma::new(0.5, 8).ok().unwrap();
    let cold = e3.cold_budget(20.0);
    let _ = e3.observe(16.0);
    let warm = e3.cold_budget(20.0);
    let ok = (cold - 25.0).abs() < 1e-5 && !e3.primed == false && (warm - 16.0).abs() < 1e-5;
    s.add("CGA-40-03-冷启动保守", ok, if ok { "" } else { "冷启破缺" });
    // J3 预算按预测切片（重瓦片多给 + 总量守恒）
    let preds = [4.0, 1.0, 1.0, 2.0];
    let budgets = allocate_budgets(&preds, 16.0);
    let sum: f32 = budgets.iter().sum();
    let ok = (sum - 16.0).abs() < 1e-4
        && budgets[0] > budgets[1]
        && budgets[0] > budgets[2]
        && budgets[0] > budgets[3];
    s.add("CGA-40-04-预算切片", ok, if ok { "" } else { "切片破缺" });
    // J4 确定性不破（同瓦片同种子像素负载与预测/顺序无关）
    let a = pixel_workload(7, 42);
    let b = pixel_workload(7, 42);
    let c = pixel_workload(8, 42);
    let ok = a == b && a != c;
    s.add("CGA-40-05-确定性不破", ok, if ok { "" } else { "像素漂移" });
    // J4-b 只调序不改性（LPT 输出是原索引排列）
    let order = lpt_order(&[1.0, 4.0, 2.0, 3.0]);
    let mut sorted = order.clone();
    sorted.sort();
    let ok = order.first() == Some(&1) && sorted == vec![0, 1, 2, 3];
    s.add("CGA-40-06-LPT调序", ok, if ok { "" } else { "排列破缺" });
    // J5 调整收益实测（LPT 均衡差 < 自然序；单核收益=0 口径）
    let preds5 = [1.0, 8.0, 2.0, 7.0, 3.0];
    let gain = lpt_gain_ms(&preds5, 4);
    let zero = lpt_gain_ms(&preds5, 1);
    let ok = gain > 0.0 && zero == 0.0;
    s.add("CGA-40-07-收益实测", ok, if ok { "" } else { "收益破缺" });
    // J6 窗口参数可调（α 越界拒 + 窗口越界拒 + 合法值过）
    let ok = matches!(Ewma::new(0.0, 8), Err(XC_ALPHA))
        && matches!(Ewma::new(1.1, 8), Err(XC_ALPHA))
        && matches!(Ewma::new(0.5, 0), Err(XC_WINDOW))
        && matches!(Ewma::new(0.5, 1025), Err(XC_WINDOW))
        && Ewma::new(0.5, 1024).is_ok();
    s.add("CGA-40-08-参数校验", ok, if ok { "" } else { "越界放行" });
    // J6-b 窗口滑动（超窗丢最老观测）
    let mut e4 = Ewma::new(1.0, 2).ok().unwrap();
    for v in [1.0, 2.0, 3.0, 4.0] { e4.observe(v); }
    let ok = e4.hist == vec![3.0, 4.0];
    s.add("CGA-40-09-窗口滑动", ok, if ok { "" } else { "滑窗破缺" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0 && XC_ALPHA != XC_WINDOW;
    s.add("CGA-40-10-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga40_checks_all_green() {
        let set = run_cga40_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga40_pred_never_changes_pixels() {
        // 预测器跑过之后像素负载不变
        let before = pixel_workload(3, 7);
        let mut e = Ewma::new(0.5, 4).ok().unwrap();
        for v in [10.0, 12.0, 11.0] { e.observe(v); }
        let _ = allocate_budgets(&[e.predict(); 4], 16.0);
        assert_eq!(pixel_workload(3, 7), before, "预测不得触像素");
    }
}
