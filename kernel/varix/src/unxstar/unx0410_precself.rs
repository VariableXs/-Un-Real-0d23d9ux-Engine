//! unx0410_precself —— UNX-F0410 计时精度自检（回读抖动限）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0410 · 计时精度自检 · 目标 220 行
//! 判据原文：UNX-F0410-J1 计时精度自检：连续 N 次回读时钟，
//!           相邻两次差值（抖动）逐个落限 [0, jitter_cap]；
//!           负差值 = 回退（单调违例）立案；超上限抖动立案
//!           （计数器可能被并发改写）；全限内 → 精度自检绿；
//!           抖动统计（最大/平均/样本数）可导出；限值零 = 每
//!           次回读必须严格递增（最强限）。
//! 组件：回读序列（时点数组）+ 抖动分析（差分 + 限值核）+
//!       立案账（回退案/超抖案带样本号）+ 统计导出（三数）+
//!       自检闸。
//! 职责定位：精度自检是「时钟的体检心电图」——连续回读的差分
//!   序列就是心率：健康时钟的心率平稳且只进不退；回退说明
//!   有人动了计数器或读到了撕裂值；超抖说明时钟源可能被并发
//!   改频。限值零是最强判定：要求每次回读严格递增，用来验收
//!   计数器读法的原子性。
//! 性能逐项分解：差分 O(样本数)；立案 O(1)/样本；统计 O(样本数)。
//! 诊断码段：0x21000（0x21001~0x21009 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PS_VERSION: &str = "unx0410_precself-v1";

/// 诊断段基址（0x21000 段 · 本模块独占）。
pub const PS_SEG_BASE: u32 = 0x21000;
pub const XC_REGRESS: u32 = 0x21001; // 回退（单调违例）
pub const XC_JITTER_HI: u32 = 0x21002; // 超限抖动
pub const XC_FEW_SAMPLES: u32 = 0x21003; // 样本不足（<2）
pub const XC_CAP_OOR: u32 = 0x21004; // 限值越域（>1e6）
pub const XC_STAT_LOST: u32 = 0x21005; // 统计导出核破
pub const XC_REPLAY: u32 = 0x21006; // 分析重放不一致
pub const XC_MAGIC: u32 = 0x21007; // 序列魔数破
pub const XC_ZERO_CAP_NEG: u32 = 0x21008; // 强限下允许相等的语义错
pub const XC_CASE_LOST: u32 = 0x21009; // 立案丢失

/// 抖动分析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JitterStat {
    /// 样本数。
    pub samples: u32,
    /// 差分数。
    pub diffs: u32,
    /// 最大抖动。
    pub max_jitter: u64,
    /// 平均抖动（整数近似）。
    pub avg_jitter: u64,
    /// 立案数（回退 + 超抖）。
    pub cases: u32,
}

/// 抖动分析器。
pub struct PrecAnalyzer {
    seq: Vec<u64>,
    cap: u64,
    cases: Vec<(u32, u8)>, // (样本号, 1=回退 2=超抖)
}

/// 限值上限。
pub const CAP_MAX: u64 = 1_000_000;

impl PrecAnalyzer {
    /// 建：抖动限值。
    pub fn new(cap: u64) -> Result<PrecAnalyzer, u32> {
        if cap > CAP_MAX {
            return Err(XC_CAP_OOR);
        }
        Ok(PrecAnalyzer { seq: Vec::new(), cap, cases: Vec::new() })
    }

    /// 追加回读样本。
    pub fn feed(&mut self, t: u64) {
        self.seq.push(t);
    }

    /// 差分 + 限值核：逐差分判定；结果含立案。
    pub fn analyze(&mut self) -> Result<JitterStat, u32> {
        self.cases.clear();
        let n = self.seq.len();
        if n < 2 {
            return Err(XC_FEW_SAMPLES);
        }
        let mut diffs: Vec<u64> = Vec::new();
        for i in 1..n {
            let d_prev = self.seq[i - 1];
            let d_cur = self.seq[i];
            if d_cur < d_prev {
                self.cases.push((i as u32, 1));
                diffs.push(0); // 回退差分记 0（账目保持长度）
            } else {
                let d = d_cur - d_prev;
                // 超抖判定：常规限 d > cap；强限（cap=0）下 d == 0 也立案
                // （强限要求严格递增）。
                let over = if self.cap == 0 { d == 0 } else { d > self.cap };
                if over {
                    self.cases.push((i as u32, 2));
                }
                diffs.push(d);
            }
        }
        let max_j = diffs.iter().copied().max().unwrap_or(0);
        let sum: u64 = diffs.iter().sum();
        let avg = if !diffs.is_empty() { sum / diffs.len() as u64 } else { 0 };
        Ok(JitterStat {
            samples: n as u32,
            diffs: diffs.len() as u32,
            max_jitter: max_j,
            avg_jitter: avg,
            cases: self.cases.len() as u32,
        })
    }

    /// 自检闸：无立案绿。
    pub fn gate(&mut self) -> Result<JitterStat, u32> {
        let s = self.analyze()?;
        if s.cases != 0 {
            return Err(XC_JITTER_HI);
        }
        Ok(s)
    }

    /// 立案表读取。
    pub fn cases(&self) -> &[(u32, u8)] {
        &self.cases
    }

    /// 样本数。
    pub fn sample_count(&self) -> usize {
        self.seq.len()
    }

    /// 统计导出核：max ≥ avg（恒等式）。
    pub fn stat_ok(s: &JitterStat) -> Result<(), u32> {
        if s.max_jitter < s.avg_jitter {
            return Err(XC_STAT_LOST);
        }
        if s.diffs != s.samples - 1 {
            return Err(XC_STAT_LOST);
        }
        Ok(())
    }

    /// 重放核：同序列同统计（确定性）。
    pub fn replay_eq(seq: &[u64], cap: u64) -> Result<(), u32> {
        let mut a = match PrecAnalyzer::new(cap) {
            Ok(x) => x,
            Err(_) => return Err(XC_CAP_OOR),
        };
        let mut b = match PrecAnalyzer::new(cap) {
            Ok(x) => x,
            Err(_) => return Err(XC_CAP_OOR),
        };
        for &t in seq.iter() {
            a.feed(t);
            b.feed(t);
        }
        let sa = a.analyze()?;
        let sb = b.analyze()?;
        if sa != sb {
            return Err(XC_REPLAY);
        }
        Ok(())
    }

    /// 魔数核（限值域内）。
    pub fn magic_ok(&self) -> Result<(), u32> {
        if self.cap > CAP_MAX {
            return Err(XC_MAGIC);
        }
        Ok(())
    }
}

pub fn run_unx0410_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0410_precself");
    // 族一：健康序列（步进 10，限 20）。
    let mut a = match PrecAnalyzer::new(20) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建分析器", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 10, 20, 30, 40, 50].iter() {
        a.feed(*t);
    }
    cs.add("样本 6", a.sample_count() == 6, "五差分");
    let s1 = a.analyze();
    cs.add("分析成", s1.is_ok(), "差分完整");
    let s1 = match s1 {
        Ok(s) => s,
        Err(_) => {
            cs.add("分析失败", false, "破");
            return cs;
        }
    };
    cs.add("差分数 5", s1.diffs == 5, "n-1");
    cs.add("最大抖 10", s1.max_jitter == 10, "步进 10");
    cs.add("平均抖 10", s1.avg_jitter == 10, "均匀步进");
    cs.add("立案 0", s1.cases == 0, "全限内");
    cs.add("统计核过", PrecAnalyzer::stat_ok(&s1).is_ok(), "max ≥ avg 且差分数合");
    cs.add("自检闸绿", a.gate().is_ok(), "无案");
    // 族二：回退检出。
    let mut r = match PrecAnalyzer::new(20) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建2", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 10, 5, 20].iter() {
        r.feed(*t);
    }
    cs.add("回退检出", r.analyze() == Ok(JitterStat { samples: 4, diffs: 3, max_jitter: 15, avg_jitter: 8, cases: 1 }), "样本 2 回退");
    cs.add("立案定位", r.cases() == &[(2u32, 1u8)], "样本号 2 · 回退");
    // 族三：超抖检出。
    let mut j = match PrecAnalyzer::new(5) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建3", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 3, 100, 104].iter() {
        j.feed(*t);
    }
    cs.add("超抖检出",
        j.analyze() == Ok(JitterStat { samples: 4, diffs: 3, max_jitter: 97, avg_jitter: 34, cases: 1 }),
        "差 100 > 5");
    cs.add("超抖定位", j.cases() == &[(2u32, 2u8)], "样本号 2 · 超抖");
    // 限值零（最强限：严格递增）。
    let mut z = match PrecAnalyzer::new(0) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建4", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 1, 2, 3].iter() {
        z.feed(*t);
    }
    cs.add("强限过", z.gate().is_ok(), "严格递增");
    let mut z2 = match PrecAnalyzer::new(0) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建5", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 1, 1, 2].iter() {
        z2.feed(*t);
    }
    cs.add("强限相等也超抖",
        z2.analyze() == Ok(JitterStat { samples: 4, diffs: 3, max_jitter: 1, avg_jitter: 0, cases: 1 }),
        "差 0 > 限 0");
    // 族四：样本不足 / 参数拒绝。
    let mut f = match PrecAnalyzer::new(10) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建6", false, "拒");
            return cs;
        }
    };
    f.feed(1);
    cs.add("单样本拒", f.analyze() == Err(XC_FEW_SAMPLES), "n=1");
    cs.add("零样本拒", match PrecAnalyzer::new(10) {
        Ok(mut x) => x.analyze() == Err(XC_FEW_SAMPLES),
        Err(_) => false,
    }, "n=0");
    cs.add("限值超域拒", matches!(PrecAnalyzer::new(CAP_MAX + 1), Err(XC_CAP_OOR)), ">1e6");
    cs.add("限值上限过", PrecAnalyzer::new(CAP_MAX).is_ok(), "=1e6");
    // 族五：重放确定性。
    let seq: [u64; 5] = [0, 7, 15, 21, 40];
    cs.add("重放一致", PrecAnalyzer::replay_eq(&seq, 20).is_ok(), "统计全等");
    // 族六：立案丢失核（有案时 gate 必红）。
    let mut g = match PrecAnalyzer::new(1) {
        Ok(x) => x,
        Err(_) => {
            cs.add("建7", false, "拒");
            return cs;
        }
    };
    for t in [0u64, 1, 9].iter() {
        g.feed(*t);
    }
    cs.add("有案闸红", g.gate() == Err(XC_JITTER_HI), "差 8 > 1");
    cs.add("魔数核", g.magic_ok().is_ok(), "限值域内");
    cs
}
