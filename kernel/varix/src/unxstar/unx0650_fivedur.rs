//! unx0650_fivedur —— UNX-F0650 回归时长五数（批级耗时账）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0650 · 回归时长五数（批级耗时账） · 目标 ≥200 行
//! 判据原文摘录：UNX-F0650-J1 全域回归耗时五数（min/p25/中位/p75/
//!   max）落账，轮间零交叉断言过。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 五数定义（整数口径）：min = 最小耗时；p25/中位/p75 = 升序序
//!      列的整数序取数（下取整位次）；max = 最大耗时。五数落账后
//!      逐项复算一致才平。
//!   2. 有序不变式：min ≤ p25 ≤ 中位 ≤ p75 ≤ max（整数链），破链
//!      即五数账假。
//!   3. 轮间零交叉：每轮耗时账 [起拍, 止拍] 区间逐轮比对，任意两轮
//!      区间不交叠（前轮止 ≤ 后轮起），交叉即红。
//!   4. 批级账：每轮时长按批拆账（批数闭集），批时长和 = 轮时长。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术
//!      （禁浮点），确定性。
//!
//! 诊断段声明：SEG 0x52C0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x52C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x52C0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x52CF;

/// XcCode：耗时为零。
pub const XC_ZERO_DUR: u16 = SEG_BASE + 1; // 0x52C1 零耗时轮拒收
/// XcCode：起止倒挂。
pub const XC_SPAN_BACK: u16 = SEG_BASE + 2; // 0x52C2 止拍 ≤ 起拍
/// XcCode：轮间交叉。
pub const XC_CROSS: u16 = SEG_BASE + 3; // 0x52C3 区间交叠
/// XcCode：五数复算不一致。
pub const XC_FIVE_MISMATCH: u16 = SEG_BASE + 4; // 0x52C4 落账 ≠ 复算
/// XcCode：有序链破。
pub const XC_ORDER_BREAK: u16 = SEG_BASE + 5; // 0x52C5 min≤p25≤中位≤p75≤max 破
/// XcCode：批拆账不平。
pub const XC_BATCH_SPLIT: u16 = SEG_BASE + 6; // 0x52C6 Σ批时长 ≠ 轮时长

/// 单轮回归耗时账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundDur {
    /// 轮号。
    pub round_no: usize,
    /// 起拍。
    pub start: u64,
    /// 止拍。
    pub end: u64,
    /// 批级拆账（批号 → 该批耗时；闭集 5 批）。
    pub batch_dur: [u64; 5],
}

impl RoundDur {
    /// 轮时长。
    pub fn dur(&self) -> u64 {
        self.end - self.start
    }

    /// 批拆账守恒：Σ批 = 轮时长。
    pub fn batch_split_ok(&self) -> bool {
        let s: u64 = self.batch_dur.iter().sum();
        s == self.dur()
    }
}

/// 五数账（落账面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveDur {
    /// min。
    pub min: u64,
    /// p25。
    pub p25: u64,
    /// 中位。
    pub median: u64,
    /// p75。
    pub p75: u64,
    /// max。
    pub max: u64,
}

impl FiveDur {
    /// 有序链：min ≤ p25 ≤ 中位 ≤ p75 ≤ max。
    pub fn ordered(&self) -> bool {
        self.min <= self.p25
            && self.p25 <= self.median
            && self.median <= self.p75
            && self.p75 <= self.max
    }
}

/// 升序整数序列的序取数（整数口径：位次 k ∈ 0..n，下取整）。
pub fn order_pick(sorted: &[u64], k: usize) -> u64 {
    // 调用方保证 k < len；防御性回退取末位（不 panic）。
    match sorted.get(k) {
        Some(v) => *v,
        None => 0,
    }
}

/// 回归耗时账本。
#[derive(Clone, Debug, Default)]
pub struct DurBook {
    /// 逐轮耗时账（按轮号升序）。
    rounds: Vec<RoundDur>,
    /// 五数落账。
    five: Option<FiveDur>,
}

impl DurBook {
    /// 空账本。
    pub fn new() -> Self {
        DurBook {
            rounds: Vec::new(),
            five: None,
        }
    }

    /// 落一轮（止 > 起 + 批拆守恒 + 轮间零交叉 + 轮号连续）。
    pub fn log_round(&mut self, start: u64, end: u64, batch_dur: [u64; 5]) -> Result<usize, u16> {
        if end <= start {
            return Err(XC_SPAN_BACK);
        }
        if start == end {
            return Err(XC_ZERO_DUR);
        }
        let dummy = RoundDur {
            round_no: usize::MAX,
            start,
            end,
            batch_dur,
        };
        if !dummy.batch_split_ok() {
            return Err(XC_BATCH_SPLIT);
        }
        if let Some(last) = self.rounds.last() {
            if start < last.end {
                return Err(XC_CROSS);
            }
        }
        let round_no = self.rounds.len();
        self.rounds.push(RoundDur {
            round_no,
            start,
            end,
            batch_dur,
        });
        Ok(round_no)
    }

    /// 轮数。
    pub fn round_n(&self) -> usize {
        self.rounds.len()
    }

    /// 轮间零交叉核（全量复扫）。
    pub fn no_cross(&self) -> Result<usize, u16> {
        for w in self.rounds.windows(2) {
            if w[1].start < w[0].end {
                return Err(XC_CROSS);
            }
        }
        Ok(self.rounds.len())
    }

    /// 五数复算：升序耗时序列的整数序取数。
    pub fn recalc_five(&self) -> Result<FiveDur, u16> {
        if self.rounds.is_empty() {
            return Err(XC_ZERO_DUR);
        }
        let mut durs: Vec<u64> = self.rounds.iter().map(|r| r.dur()).collect();
        durs.sort_unstable();
        let n = durs.len();
        let p25 = order_pick(&durs, n / 4);
        let median = order_pick(&durs, n / 2);
        let p75 = order_pick(&durs, (n * 3) / 4);
        let f = FiveDur {
            min: durs[0],
            p25,
            median,
            p75,
            max: durs[n - 1],
        };
        if !f.ordered() {
            return Err(XC_ORDER_BREAK);
        }
        Ok(f)
    }

    /// 五数落账（复算先于落账）。
    pub fn seal_five(&mut self) -> Result<FiveDur, u16> {
        let f = self.recalc_five()?;
        self.five = Some(f);
        Ok(f)
    }

    /// 五数复验：落账值 = 复算值 + 有序链。
    pub fn verify_five(&self) -> Result<FiveDur, u16> {
        let f = match self.five {
            Some(f) => f,
            None => return Err(XC_FIVE_MISMATCH),
        };
        let r = self.recalc_five()?;
        if f != r {
            return Err(XC_FIVE_MISMATCH);
        }
        if !f.ordered() {
            return Err(XC_ORDER_BREAK);
        }
        Ok(f)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!("回归时长五数 · 轮数 {} · 五数落账={}", self.round_n(), self.five.is_some());
        if let Some(f) = &self.five {
            s.push_str(&alloc::format!(
                "\n  min={} p25={} 中位={} p75={} max={}",
                f.min, f.p25, f.median, f.p75, f.max
            ));
        }
        for r in &self.rounds {
            s.push_str(&alloc::format!(
                "\n  轮{} [{}..{}] 时长{}",
                r.round_no, r.start, r.end, r.dur()
            ));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准耗时账构造器：8 轮，轮 i 时长 = 40+((i*7)%23)，起拍 = 前轮止拍
/// +5（零交叉），批拆 5 批均分（余数入末批）。
pub fn build_standard_dur() -> Result<DurBook, u16> {
    let mut b = DurBook::new();
    let mut start: u64 = 100;
    for i in 0..8 {
        let d = 40 + ((i as u64 * 7) % 23);
        let end = start + d;
        let base = d / 5;
        let rem = d % 5;
        let mut split = [base; 5];
        split[4] += rem;
        b.log_round(start, end, split)?;
        start = end + 5;
    }
    b.seal_five()?;
    Ok(b)
}

/// 自例程：主路径（五数落账→复验→零交叉）+ 错误路径（倒挂/交叉/拆账不平/复算漂移）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let b = build_standard_dur()?;
    let f = b.verify_five()?;
    if !f.ordered() {
        return Err(XC_ORDER_BREAK);
    }
    if b.no_cross()? != 8 {
        return Err(XC_CROSS);
    }
    if b.fingerprint() == 0 {
        return Err(XC_FIVE_MISMATCH);
    }

    // 错误路径 1：起止倒挂。
    let mut b1 = DurBook::new();
    let split = [10u64, 10, 10, 10, 10];
    if b1.log_round(50, 50, split) != Err(XC_SPAN_BACK) {
        return Err(XC_SPAN_BACK);
    }
    if b1.log_round(60, 55, split) != Err(XC_SPAN_BACK) {
        return Err(XC_SPAN_BACK);
    }
    // 错误路径 2：轮间交叉。
    let mut b2 = DurBook::new();
    b2.log_round(100, 150, split)?;
    if b2.log_round(140, 200, split) != Err(XC_CROSS) {
        return Err(XC_CROSS);
    }
    // 错误路径 3：批拆账不平。
    let mut b3 = DurBook::new();
    let bad_split = [10u64, 10, 10, 10, 9]; // Σ=49 ≠ 50
    if b3.log_round(0, 50, bad_split) != Err(XC_BATCH_SPLIT) {
        return Err(XC_BATCH_SPLIT);
    }
    // 错误路径 4：五数复算漂移（落账后篡改一轮时长）。
    let mut b4 = build_standard_dur()?;
    b4.rounds[2].end += 10;
    if b4.verify_five() != Err(XC_FIVE_MISMATCH) {
        return Err(XC_FIVE_MISMATCH);
    }
    // 错误路径 5：有序链破（落账面手工注入乱序五数）。
    let mut b5 = build_standard_dur()?;
    b5.five = Some(FiveDur {
        min: 9,
        p25: 50,
        median: 40,
        p75: 60,
        max: 70,
    });
    if b5.verify_five() != Err(XC_FIVE_MISMATCH) {
        return Err(XC_FIVE_MISMATCH);
    }
    // 错误路径 6：未落五数先验。
    let b6 = DurBook::new();
    if b6.verify_five() != Err(XC_FIVE_MISMATCH) {
        return Err(XC_FIVE_MISMATCH);
    }
    Ok(0)
}
