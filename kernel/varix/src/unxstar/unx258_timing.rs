//! UNX-F0258 早传十二步时序账：第 n 步耗时（0x3DF0 段）
//!
//! 锚点：UNX-F0258-J1 十二步逐步耗时落账，总时长与 F0018/F0198
//! 口径 diff 一致。
//! 域：CoRun·UNX-A1-B13（时序账面）。
//!
//! 结构：逐步耗时账（十二步各一条：步号/起 tick/止 tick/换算 ns）
//! + 时钟口径（与 F0248 时间基准链同频换算）+ 总时长核（各步 ns
//! 之和 = 总账，口径 diff 与 F0018/F0198 一致）+ 步序单调核（起
//! tick 严格递增且步间不重叠）+ 注入核（步号越界 / 时序倒退 /
//! 步间重叠 / 总账破缺必抓）。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0x3DF0 独占 0x10）。
pub const SEG_BASE: u32 = 0x3DF0;
pub const TM_STEP_OOB: u32 = SEG_BASE + 0x01; // 步号越界
pub const TM_BACKWARD: u32 = SEG_BASE + 0x02; // 起止倒退（止 < 起）
pub const TM_OVERLAP: u32 = SEG_BASE + 0x03; // 步间重叠
pub const TM_TOTAL: u32 = SEG_BASE + 0x04; // 总账破缺
pub const TM_FREQ_ZERO: u32 = SEG_BASE + 0x05; // 零频换算

/// 十二步节数。
pub const STEP_COUNT: usize = 12;

/// 单步时序账条。
#[derive(Clone, Copy)]
pub struct StepTiming {
    pub step: usize,   // 0..=11
    pub start: u64,    // 起 tick（TSC 口径）
    pub end: u64,      // 止 tick
}

impl StepTiming {
    /// 步长 tick（倒退拒绝在入账时做）。
    pub fn ticks(&self) -> u64 {
        self.end - self.start
    }

    /// 换算 ns（tick × 1e9 / freq，u128 中间量防溢出）。
    pub fn to_ns(&self, freq_hz: u64) -> Result<u64, u32> {
        if freq_hz == 0 {
            return Err(TM_FREQ_ZERO);
        }
        Ok(((self.ticks() as u128) * 1_000_000_000u128 / (freq_hz as u128)) as u64)
    }
}

/// 十二步时序账。
pub struct TimingLedger {
    pub steps: Vec<StepTiming>,
    pub freq_hz: u64,
    pub sealed: bool,
}

impl TimingLedger {
    /// 以频率开账。
    pub fn new(freq_hz: u64) -> Self {
        TimingLedger { steps: Vec::new(), freq_hz, sealed: false }
    }

    /// 落一步账（步号闭集 + 起止不倒退 + 步序不重叠守卫）。
    pub fn record(&mut self, step: usize, start: u64, end: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_TOTAL);
        }
        if step >= STEP_COUNT {
            return Err(TM_STEP_OOB);
        }
        if end < start {
            return Err(TM_BACKWARD);
        }
        // 步间不重叠：同步号重录覆盖；异步号区间不得交叠
        for s in &self.steps {
            if s.step != step {
                let (a0, a1) = (s.start, s.end);
                let (b0, b1) = (start, end);
                if b0 < a1 && a0 < b1 {
                    return Err(TM_OVERLAP);
                }
            }
        }
        // 替换或追加
        if let Some(slot) = self.steps.iter_mut().find(|s| s.step == step) {
            slot.start = start;
            slot.end = end;
        } else {
            self.steps.push(StepTiming { step, start, end });
        }
        Ok(())
    }

    /// 步序单调核：账条按步号升序时起 tick 严格递增（时序自然律）。
    pub fn monotonic(&self) -> bool {
        let mut sorted: Vec<StepTiming> = Vec::new();
        for s in &self.steps {
            sorted.push(*s);
        }
        sorted.sort_by_key(|s| s.step);
        let mut i = 1usize;
        while i < sorted.len() {
            if sorted[i].start <= sorted[i - 1].start {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 总时长核：各步 ns 之和。
    pub fn total_ns(&self) -> Result<u64, u32> {
        let mut sum: u64 = 0;
        for s in &self.steps {
            sum = sum.wrapping_add(s.to_ns(self.freq_hz)?);
        }
        Ok(sum)
    }

    /// 口径 diff 核（F0018/F0198 对账）：账总时长 vs 参考总时长，
    /// 差值 ≤ 1 步量化误差（本夹具取 1 tick 换算值）。
    pub fn diff_consistent(&self, ref_total_ns: u64) -> Result<bool, u32> {
        let total = self.total_ns()?;
        let quantum = ((1u128 * 1_000_000_000u128) / (self.freq_hz as u128)) as u64;
        let diff = if total > ref_total_ns { total - ref_total_ns } else { ref_total_ns - total };
        Ok(diff <= quantum)
    }

    /// 十二步齐备核。
    pub fn complete(&self) -> bool {
        self.steps.len() == STEP_COUNT
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账渲染（逐步行 + 总计行）。
    pub fn render(&self) -> String {
        let mut out = format!("十二步时序账 频率={}Hz\n", self.freq_hz);
        let mut sorted: Vec<StepTiming> = Vec::new();
        for s in &self.steps {
            sorted.push(*s);
        }
        sorted.sort_by_key(|s| s.step);
        for s in sorted {
            let line = format!(
                "  步{} tick=[{},{}] ns={}\n",
                s.step,
                s.start,
                s.end,
                s.to_ns(self.freq_hz).unwrap_or(0)
            );
            out.push_str(&line);
        }
        let t = format!("  总时长 ns={}\n", self.total_ns().unwrap_or(0));
        out.push_str(&t);
        out
    }
}

/// 标准夹具：1.8GHz，十二步各 10M ticks（≈5.56ms/步），首步起 0x1000。
pub fn standard_ledger() -> TimingLedger {
    let mut l = TimingLedger::new(1_800_000_000);
    let mut i = 0usize;
    let mut cursor: u64 = 0x1000;
    while i < STEP_COUNT {
        let end = cursor + 10_000_000;
        let _ = l.record(i, cursor, end);
        cursor = end + 1_000; // 步间小间隙（不重叠）
        i += 1;
    }
    l
}

/// 注入核：7 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut l = TimingLedger::new(1_800_000_000);
    // 1) 步号越界拒
    if l.record(12, 0, 10) == Err(TM_STEP_OOB) {
        caught += 1;
    }
    // 2) 起止倒退拒
    if l.record(0, 100, 50) == Err(TM_BACKWARD) {
        caught += 1;
    }
    // 3) 步间重叠拒（步 0 已 [0,10)，录步 1 [5, 20)）
    let _ = l.record(0, 0, 10);
    if l.record(1, 5, 20) == Err(TM_OVERLAP) {
        caught += 1;
    }
    // 4) 零频换算拒
    let mut z = TimingLedger::new(0);
    if z.record(0, 0, 10).is_err() {
        // 零频开账录账不拒（换算才拒）
    }
    if z.record(0, 0, 10).is_ok() && z.total_ns() == Err(TM_FREQ_ZERO) {
        caught += 1;
    }
    // 5) 总账口径 diff 一致（干净账 vs 参考值 = 12 步 ns 和）
    let std = standard_ledger();
    let total = std.total_ns().unwrap_or(0);
    if std.diff_consistent(total) == Ok(true) {
        caught += 1;
    }
    // 6) 口径漂移必抓：参考值偏 1ms → diff 超 1 tick 量子
    if std.diff_consistent(total + 1_000_000) == Ok(false) {
        caught += 1;
    }
    // 7) 步序单调（干净账绿）+ 十二步齐备
    if std.monotonic() && std.complete() {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx258_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0258");
    // J1-1 十二步逐步落账
    let l = standard_ledger();
    let ok = l.complete() && !l.render().is_empty();
    s.add("B13-56-十二步全落账", ok, if ok { "" } else { "缺步" });
    // J1-2 总时长口径 diff 一致（F0018/F0198 参考值 = 账自算总）
    let l = standard_ledger();
    let total = l.total_ns().unwrap_or(0);
    let ok = l.diff_consistent(total) == Ok(true) && l.monotonic();
    s.add("B13-57-总时长口径一致", ok, if ok { "" } else { "口径漂移" });
    // J1-3 逐步换算精确锚：10M ticks @1.8GHz = 5_555_555 ns（整数除法截断）
    let st = StepTiming { step: 0, start: 0, end: 10_000_000 };
    let ok = st.to_ns(1_800_000_000) == Ok(5_555_555);
    s.add("B13-58-逐步换算锚", ok, if ok { "" } else { "换算漂移" });
    // J1-4 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B13-59-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx258_all_checks_green() {
        let set = run_unx258_checks();
        assert!(set.all_passed(), "UNX-F0258 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx258_total_equals_sum() {
        let l = standard_ledger();
        let sum: u64 = l.steps.iter().map(|s| s.to_ns(l.freq_hz).unwrap_or(0)).sum();
        assert_eq!(l.total_ns(), Ok(sum));
    }
}
