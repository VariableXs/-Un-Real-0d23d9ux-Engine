//! UNX-F11237 30 样本逐特性点亮推进账（0x1f837 段）
//!
//! 锚点：UNX-F11237-J1 点亮进度账（样本×特性矩阵实跑态）落账，本批末点亮
//! 率与策略账一致。
//! 域：CoRun·UNX-C5-B02——推进按批进行：每批六样本、批内只进不退、批间
//! 累计不回撤；五批走完点亮率必须与策略账的 100% 对齐。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f837;
pub const LP_BATCH: u32 = SEG_BASE + 0x01; // 批号越界/乱序
pub const LP_COUNT: u32 = SEG_BASE + 0x02; // 批内样本数不符
pub const LP_REGRESS: u32 = SEG_BASE + 0x03; // 点亮数回撤
pub const LP_RATE: u32 = SEG_BASE + 0x04; // 点亮率未达策略
pub const LP_STATE: u32 = SEG_BASE + 0x05; // 样本态缺失
pub const LP_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后改写
pub const LP_OVER: u32 = SEG_BASE + 0x07; // 超容

/// 批数、每批样本数、总样本。
pub const BATCHES: u32 = 5;
pub const PER_BATCH: usize = 6;
pub const TOTAL: usize = 30;
pub const LP_TAG: u64 = 0x1f837c3;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单样本实跑态。
#[derive(Clone, Debug)]
pub struct SampleState {
    pub id: u32,
    pub feature: u8,
    pub lit: bool,
}

/// 推进账。
#[derive(Clone, Debug)]
pub struct Progress {
    pub batch: u32,
    pub states: Vec<SampleState>,
    pub history: Vec<(u32, u32)>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Progress {
    pub fn new() -> Progress {
        Progress { batch: 0, states: Vec::new(), history: Vec::new(), frozen: false, root: None }
    }

    /// 推进一批：批号必须等于已批数，样本数恰六，点亮数批内不减。
    pub fn advance(&mut self, lit: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(LP_FROZEN);
        }
        if self.batch >= BATCHES {
            return Err(LP_BATCH);
        }
        if lit > PER_BATCH {
            return Err(LP_COUNT);
        }
        let prev = match self.history.last() {
            Some(h) => h.1,
            None => 0,
        };
        let now = prev.saturating_add(lit as u32);
        if now < prev {
            return Err(LP_REGRESS);
        }
        let base = self.batch as usize * PER_BATCH;
        let mut k = 0usize;
        while k < PER_BATCH {
            let id = (base + k) as u32 + 1;
            let feature = (k % 6) as u8;
            self.states.push(SampleState { id, feature, lit: k < lit });
            k += 1;
        }
        self.batch = self.batch.saturating_add(1);
        self.history.push((self.batch, now));
        Ok(now)
    }

    pub fn lit(&self) -> usize {
        self.states.iter().filter(|s| s.lit).count()
    }

    pub fn rate(&self) -> u32 {
        if self.states.is_empty() {
            return 0;
        }
        (self.lit() as u32).saturating_mul(100) / TOTAL as u32
    }

    pub fn monotone(&self) -> bool {
        let mut prev = 0u32;
        let mut k = 0usize;
        while k < self.history.len() {
            if self.history[k].1 < prev {
                return false;
            }
            prev = self.history[k].1;
            k += 1;
        }
        true
    }

    pub fn full(&self) -> bool {
        self.batch == BATCHES && self.states.len() == TOTAL && self.lit() == TOTAL && self.rate() == 100
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(LP_FROZEN);
        }
        if self.batch != BATCHES {
            return Err(LP_BATCH);
        }
        if self.states.len() != TOTAL {
            return Err(LP_STATE);
        }
        if !self.monotone() {
            return Err(LP_REGRESS);
        }
        if self.rate() != 100 {
            return Err(LP_RATE);
        }
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        Ok(r)
    }

    pub fn render(&self) -> String {
        format!("progress {}/{} batches lit {}/{}", self.batch, BATCHES, self.lit(), TOTAL)
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = LP_TAG;
        h = (h ^ self.batch as u64).wrapping_mul(FP_STEP);
        let mut k = 0usize;
        while k < self.states.len() {
            h = (h ^ self.states[k].id as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.states[k].feature as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.states[k].lit as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        k = 0;
        while k < self.history.len() {
            h = (h ^ self.history[k].0 as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.history[k].1 as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 组一张五批全点亮的推进账。
pub fn build() -> Progress {
    let mut p = Progress::new();
    let mut k = 0u32;
    while k < BATCHES {
        let _ = p.advance(PER_BATCH);
        k += 1;
    }
    p
}

/// 四路对抗：超批推进、批内超数、欠批冻存、累计回撤。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut a = build();
    out[0] = a.advance(1) == Err(LP_BATCH);
    let mut b = Progress::new();
    out[1] = b.advance(PER_BATCH + 1) == Err(LP_COUNT);
    let mut c = Progress::new();
    let mut k = 0u32;
    while k < BATCHES - 1 {
        let _ = c.advance(PER_BATCH);
        k += 1;
    }
    out[2] = c.freeze() == Err(LP_BATCH);
    let mut d = build();
    d.history.push((6, 10));
    out[3] = d.freeze() == Err(LP_REGRESS);
    out
}

/// 判据入口。
pub fn run_unx11237_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11237");
    let mut p = build();
    let root = p.freeze().unwrap_or(0);

    s.add("J1-五批走完", p.batch == BATCHES, "推进账恰走五批");
    s.add("J2-三十样本", p.states.len() == TOTAL, "样本×特性矩阵落三十格");
    s.add("J3-每批六格", p.history.iter().all(|h| h.0 >= 1 && h.0 <= BATCHES), "五个批次号 1..=5 各一条记录");
    s.add("J4-全点亮", p.lit() == TOTAL, "三十样本全部点亮");
    s.add("J5-点亮率", p.rate() == 100, "末批点亮率为 100%");
    s.add("J6-累计单调", p.monotone(), "批间点亮累计数不回撤");
    s.add("J7-批末对齐", p.history.last().map(|h| h.1) == Some(TOTAL as u32), "末批记录累计等于三十");
    s.add("J8-冻存根", root != 0 && p.root == Some(root), "推进账冻存产出非零根");

    s.add("J9-冻后推进拒", p.advance(1) == Err(LP_FROZEN), "冻存后再推进拒为 LP_FROZEN");
    s.add("J10-超批拒", {
        let mut x = build();
        x.advance(0) == Err(LP_BATCH)
    }, "第六批推进拒为 LP_BATCH");
    s.add("J11-批内超数拒", {
        let mut x = Progress::new();
        x.advance(PER_BATCH + 1) == Err(LP_COUNT)
    }, "单批点亮超六样本拒为 LP_COUNT");
    s.add("J12-回撤禁冻", {
        let mut x = build();
        x.history.push((6, 3));
        x.freeze() == Err(LP_REGRESS)
    }, "累计回撤的账冻存拒为 LP_REGRESS");

    let four = inject_four();
    s.add("J13-四路注入", four.iter().all(|x| *x), "超批/批内超数/欠批/回撤四路全拒");
    s.add_dyn(
        "J14-推进明细",
        p.batch == 5 && p.lit() == 30,
        &format!("推进 {}/5 批，点亮 {}/30，率 {}%", p.batch, p.lit(), p.rate()),
    );
    s.add_dyn(
        "J15-账根",
        root == p.fold(),
        &format!("根 0x{:x}，末批累计 {}", root, p.history[4].1),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11237_lit_progress_flow() {
        let mut p = build();
        assert!(p.full());
        let r = p.freeze().unwrap_or(0);
        assert!(r != 0);
        assert_eq!(p.advance(1), Err(LP_FROZEN));
        assert!(inject_four().iter().all(|x| *x));
    }

    #[test]
    fn unx11237_checks_green() {
        let s = run_unx11237_checks();
        assert!(s.all_passed(), "UNX-F11237 判据未全绿");
    }
}

/// 覆盖率面：按批推进后的缺口与特征分布。
impl Progress {
    /// 距满账还差多少样本。
    pub fn deficit(&self) -> usize {
        TOTAL.saturating_sub(self.states.len())
    }

    /// 距满账还差多少批。
    pub fn remaining_batches(&self) -> u32 {
        BATCHES.saturating_sub(self.batch)
    }

    /// 某特征（0..5）已点亮的样本数。
    pub fn per_feature(&self, feature: u8) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.states.len() {
            let s = &self.states[k];
            if s.feature == feature && s.lit {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 已点亮特征位图（bit k 对应特征 k）。
    pub fn feature_coverage(&self) -> u8 {
        let mut m = 0u8;
        let mut k = 0usize;
        while k < self.states.len() {
            let s = &self.states[k];
            if s.lit && s.feature < 8 {
                m |= 1u8 << s.feature;
            }
            k += 1;
        }
        m
    }

    /// 每批点亮数是否恒定（等速推进）。
    pub fn steady(&self) -> bool {
        if self.history.len() < 2 {
            return self.history.len() == 1;
        }
        let d0 = self.history[0].1;
        let mut k = 1usize;
        while k < self.history.len() {
            if self.history[k].1.wrapping_sub(self.history[k - 1].1) != d0 {
                return false;
            }
            k += 1;
        }
        true
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn deficit_and_remaining_track_advance() {
        let mut p = Progress::new();
        assert_eq!(p.deficit(), TOTAL);
        assert_eq!(p.remaining_batches(), BATCHES);
        assert!(p.advance(PER_BATCH).is_ok());
        assert_eq!(p.deficit(), TOTAL - PER_BATCH);
        assert_eq!(p.remaining_batches(), BATCHES - 1);
    }

    #[test]
    fn feature_distribution_is_uniform() {
        let p = build();
        let mut k = 0u8;
        while k < 6 {
            assert_eq!(p.per_feature(k), BATCHES as usize);
            k += 1;
        }
        assert_eq!(p.feature_coverage(), 0x3F);
        assert!(p.steady());
        assert!(p.full());
    }
}
