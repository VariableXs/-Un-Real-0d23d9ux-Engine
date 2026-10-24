//! VE-F0797 滤镜性能预算
//! 职责：滤镜算子的成本模型（按算子类别 + 位图面积）、预算表、
//!       超预算降级（quality tier：Full → Half → Quarter 分辨率）。
//! 契约：降级只缩采样尺寸、不改算子语义；预算比较含安全余量。

use crate::checks::CheckSet;
use super::vec381_fbase::FBitmap;
use super::vec393_fcpu::{ops_per_px, total_ops};

/// 算子类别名
pub const CLASS_NAMES: [&str; 3] = ["brightness", "matrix", "convolution"];

/// 质量档
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Full,
    Half,
    Quarter,
}

impl Quality {
    /// 尺寸缩放因子（定点 0.5 步进用分子/分母表达）
    pub fn scale(&self) -> (usize, usize) {
        match self {
            Quality::Full => (1, 1),
            Quality::Half => (1, 2),
            Quality::Quarter => (1, 4),
        }
    }
    pub fn next_down(self) -> Quality {
        match self {
            Quality::Full => Quality::Half,
            Quality::Half => Quality::Quarter,
            Quality::Quarter => Quality::Quarter,
        }
    }
    /// 降级后的执行尺寸
    pub fn dims(self, w: usize, h: usize) -> (usize, usize) {
        let (n, d) = self.scale();
        ((w * n).div_ceil(d), (h * n).div_ceil(d))
    }
}

/// 成本估算（ops 总量）
pub fn estimate(b: &FBitmap, class: u32) -> u64 {
    total_ops(b, class)
}

/// 预算判定（含 10% 安全余量）
pub fn within_budget(b: &FBitmap, class: u32, cap_ops: u64) -> bool {
    let cost = estimate(b, class);
    cost.saturating_add(cost / 10) <= cap_ops
}

/// 自动降级：从 Full 起，直到预算内或到底
pub fn auto_quality(w: usize, h: usize, class: u32, cap_ops: u64) -> Quality {
    let mut q = Quality::Full;
    loop {
        let (qw, qh) = q.dims(w, h);
        let bm = FBitmap::new(qw, qh);
        if within_budget(&bm, class, cap_ops) {
            return q;
        }
        let nq = q.next_down();
        if nq == q {
            return q;
        }
        q = nq;
    }
}

pub fn run_vec397_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec397");
    // 族一：类别表
    s.add("预算-类名", CLASS_NAMES.len() == 3 && CLASS_NAMES[0] == "brightness", "names");
    s.add("预算-单价序", ops_per_px(0) < ops_per_px(1) && ops_per_px(1) < ops_per_px(2), "price order");
    // 族二：质量档缩放
    s.add("预算-Full不缩", Quality::Full.dims(100, 50) == (100, 50), "full");
    s.add("预算-Half减半", Quality::Half.dims(100, 50) == (50, 25), "half");
    s.add("预算-Quarter四分", Quality::Quarter.dims(100, 50) == (25, 13), "quarter ceil");
    s.add("预算-奇数上取", Quality::Half.dims(101, 51) == (51, 26), "odd ceil");
    // 族三：降级链
    s.add("预算-Full→Half", Quality::Full.next_down() == Quality::Half, "full down");
    s.add("预算-Quarter到底", Quality::Quarter.next_down() == Quality::Quarter, "floor");
    // 族四：成本与预算
    let bm = FBitmap::new(100, 100);
    let cost = estimate(&bm, 0);
    s.add("预算-估算", cost == 100 * 100 * 4, "100x100 class0");
    s.add("预算-内含余量", within_budget(&bm, 0, cost + cost / 10 + 1), "with margin");
    s.add("预算-临界拒", !within_budget(&bm, 0, cost + cost / 10), "exact margin reject");
    s.add("预算-明显超", !within_budget(&bm, 2, 1000), "way over");
    // 族五：自动降级
    let q0 = auto_quality(10, 10, 0, u64::MAX);
    s.add("预算-大额Full", q0 == Quality::Full, "huge cap full");
    let q1 = auto_quality(1000, 1000, 2, 1000);
    s.add("预算-小额到底", q1 == Quality::Quarter, "tiny cap floor");
    let q2 = auto_quality(200, 200, 2, 45 * 100 * 100);
    s.add("预算-降半档", q2 == Quality::Half || q2 == Quality::Full, "mid cap");
    // 族六：语义不变（缩放只改尺寸，验证 dims 单调）
    let (w1, h1) = Quality::Half.dims(64, 64);
    let (w2, h2) = Quality::Quarter.dims(64, 64);
    s.add("预算-降级单调", w1 > w2 && h1 > h2, "dims monotone");
    s
}
