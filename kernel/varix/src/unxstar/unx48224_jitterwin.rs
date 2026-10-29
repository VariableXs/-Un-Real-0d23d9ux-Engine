//! unx48224_jitterwin — UNX-F48224 切换抖动合并窗口（默认 40ms，20ms 内双击合并；实测校准后冻结）（诊断段 0xBCE0）。
//!
//! 判据原文：
//!   J1 合并 ×10^3 误切为零；校准后冻结留痕；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双窗语义：合并窗（merge，窗内重按视作抖动不切态，默认 20ms）与
//!      冷却窗（cool，窗内禁止再次切换，默认 40ms）两个常数分开记账；
//!      窗宽单位毫秒，全整数比较，无浮点。
//!   2. 判定规则：新按下时刻与上一有效按下时刻差 < merge 窗 → 合并
//!      （落抖动账，态不变）；差 ≥ merge 且处于冷却期内 → 拒切（落
//!      拒切账，态不变，防连击误切）；差 ≥ merge 且出冷却 → 正常切换。
//!   3. 校准冻结：校准入口以实测样本决定窗宽，一经 freeze 即冻结——
//!      冻结后任何再校准/改宽请求一律拒收并留痕（冻结账：窗宽、样本
//!      数、冻结时刻 tick 三元组），改宽必先解冻（本卡不解冻，冻结即终态）。
//!   4. 误切为零口径（J1 ×10^3）：注入 1000 组双击抖动（间隔 < merge 窗），
//!      每组合并后状态机必须与注入前逐位一致（态不变、账只进抖动页），
//!      误切计数恒 0。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBCE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MISSWITCH: u16 = SEG_BASE + 1;  // 抖动被误切
const E_FROZEN_EDIT: u16 = SEG_BASE + 2;// 冻结后改窗被拒（预期拒收码）
const E_BAD_WINDOW: u16 = SEG_BASE + 3; // 非法窗宽（0 或 merge>cool）
const E_TIME_BACK: u16 = SEG_BASE + 4;  // 时刻回退
const E_STATE_BAD: u16 = SEG_BASE + 5;  // 状态机非法迁移
const E_JITTER_MISS: u16 = SEG_BASE + 6;// ×10^3 合并样本不足
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 缺省窗宽（ms）：合并窗 20、冷却窗 40。
pub const DEF_MERGE_MS: u64 = 20;
pub const DEF_COOL_MS: u64 = 40;

/// 切换状态机三态（中英切换域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwState {
    Chinese,
    English,
    TempEnglish,
}

/// 一条合并账。
#[derive(Clone, Copy, Debug)]
pub struct MergeEntry {
    pub press_tick: u64,     // 被合并按下的时刻
    pub gap_ms: u64,         // 与上一有效按下的间隔
    pub state_kept: SwState, // 合并时保持的态
}

/// 一条拒切账（冷却期内）。
#[derive(Clone, Copy, Debug)]
pub struct RejectEntry {
    pub press_tick: u64,
    pub gap_ms: u64,
}

/// 冻结留痕：窗宽 + 校准样本数 + 冻结时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FreezeMark {
    pub merge_ms: u64,
    pub cool_ms: u64,
    pub samples: u32,
    pub freeze_tick: u64,
}

/// 抖动合并窗口枢纽。
pub struct JitterWin {
    pub state: SwState,
    pub merge_ms: u64,
    pub cool_ms: u64,
    pub frozen: bool,
    pub freeze_mark: Option<FreezeMark>,
    last_press: u64, // 上一有效按下时刻
    last_switch: u64,// 上一成功切换时刻（冷却基准）
    merges: Vec<MergeEntry>,
    rejects: Vec<RejectEntry>,
    pub merge_total: u32,
    pub reject_total: u32,
    pub switch_total: u32,
    pub misswitch_total: u32,
}

impl JitterWin {
    pub fn new() -> JitterWin {
        JitterWin {
            state: SwState::Chinese,
            merge_ms: DEF_MERGE_MS,
            cool_ms: DEF_COOL_MS,
            frozen: false,
            freeze_mark: None,
            last_press: 0,
            last_switch: 0,
            merges: Vec::new(),
            rejects: Vec::new(),
            merge_total: 0,
            reject_total: 0,
            switch_total: 0,
            misswitch_total: 0,
        }
    }

    fn valid_windows(merge: u64, cool: u64) -> bool {
        merge > 0 && cool > 0 && merge <= cool
    }

    /// 校准入口：实测样本定窗宽；冻结后拒收（留痕拒收由调用方挂账）。
    pub fn calibrate(&mut self, merge: u64, cool: u64, samples: u32, tick: u64) -> Result<FreezeMark, u16> {
        if self.frozen {
            return Err(E_FROZEN_EDIT);
        }
        if !Self::valid_windows(merge, cool) {
            return Err(E_BAD_WINDOW);
        }
        if samples == 0 {
            return Err(E_BAD_WINDOW);
        }
        self.merge_ms = merge;
        self.cool_ms = cool;
        let mark = FreezeMark { merge_ms: merge, cool_ms: cool, samples, freeze_tick: tick };
        self.freeze_mark = Some(mark);
        Ok(mark)
    }

    /// 冻结：一次定终身（本卡冻结即终态，无解冻入口）。
    pub fn freeze(&mut self) -> Result<FreezeMark, u16> {
        match self.freeze_mark {
            Some(m) => {
                self.frozen = true;
                Ok(m)
            }
            None => Err(E_STATE_BAD), // 未校准不得冻结
        }
    }

    /// 按下事件主径（纪律 2）：合并 / 拒切 / 正常切换三分支。
    pub fn press(&mut self, tick: u64) -> Result<PressOutcome, u16> {
        if self.last_press != 0 && tick < self.last_press {
            return Err(E_TIME_BACK);
        }
        let gap = if self.last_press == 0 { u64::MAX } else { tick - self.last_press };
        self.last_press = tick;
        if gap < self.merge_ms {
            // 抖动合并：态不变，误切计数不动（判定后复核）。
            let kept = self.state;
            self.merges.push(MergeEntry { press_tick: tick, gap_ms: gap, state_kept: kept });
            self.merge_total += 1;
            // 误切复核：合并后态必须与合并前一致（合并不改态是规则内建）。
            if self.state != kept {
                self.misswitch_total += 1;
                return Err(E_MISSWITCH);
            }
            return Ok(PressOutcome::Merged);
        }
        if self.last_switch != 0 && tick - self.last_switch < self.cool_ms && self.switch_total > 0 {
            // 冷却期内：拒切，态不变。
            self.rejects.push(RejectEntry { press_tick: tick, gap_ms: gap });
            self.reject_total += 1;
            return Ok(PressOutcome::RejectedCool);
        }
        // 正常切换：三态轮转 Chinese → English → TempEnglish → Chinese。
        self.state = match self.state {
            SwState::Chinese => SwState::English,
            SwState::English => SwState::TempEnglish,
            SwState::TempEnglish => SwState::Chinese,
        };
        self.last_switch = tick;
        self.switch_total += 1;
        Ok(PressOutcome::Switched)
    }

    pub fn merges_len(&self) -> usize {
        self.merges.len()
    }
    pub fn rejects_len(&self) -> usize {
        self.rejects.len()
    }

    /// ×N 合并覆盖核验（J1 硬口径）。
    pub fn merge_coverage(&self, want: u32) -> Result<(), u16> {
        if self.merge_total < want {
            return Err(E_JITTER_MISS);
        }
        Ok(())
    }

    /// 误切为零终判。
    pub fn assert_zero_misswitch(&self) -> Result<(), u16> {
        if self.misswitch_total != 0 {
            return Err(E_MISSWITCH);
        }
        Ok(())
    }

    /// 账目守恒：合并+拒切+切换 = 总按下数（由调用方传入总按下数）。
    pub fn audit(&self, presses: u32) -> Result<(), u16> {
        if self.merge_total + self.reject_total + self.switch_total != presses {
            return Err(E_STATE_BAD);
        }
        if self.merges.len() as u32 != self.merge_total
            || self.rejects.len() as u32 != self.reject_total
        {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 按下事件的三种结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressOutcome {
    Merged,
    RejectedCool,
    Switched,
}

/// 自例程判据：逐条落实 J1（合并 ×10^3 误切为零；校准后冻结留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 抖动合并 ×10^3：1000 组双击（间隔 5..19ms 交替），全合并、
    //    态零变化、误切为零；组间抬隔 ≥冷却窗，穿插 100 次真切换。
    let mut jw = JitterWin::new();
    jw.calibrate(DEF_MERGE_MS, DEF_COOL_MS, 1000, 0)?;
    jw.freeze()?;
    let mut tick = 100u64;
    let mut presses = 0u32;
    let mut i = 0u32;
    while i < 1000 {
        let gap = if i % 2 == 0 { 5u64 } else { 19u64 };
        // 双击第一击：距上次有效按下 > 冷却窗 → 正常切换。
        tick += DEF_COOL_MS + 1;
        let out1 = jw.press(tick)?;
        presses += 1;
        if out1 != PressOutcome::Switched {
            return Err(E_STATE_BAD);
        }
        // 双击第二击：间隔 < 20ms → 必合并，态不变。
        let before = jw.state;
        tick += gap;
        let out2 = jw.press(tick)?;
        presses += 1;
        if out2 != PressOutcome::Merged {
            return Err(E_MISSWITCH);
        }
        if jw.state != before {
            return Err(E_MISSWITCH);
        }
        i += 1;
    }
    jw.merge_coverage(1000)?;
    jw.assert_zero_misswitch()?;
    jw.audit(presses)?;
    if jw.switch_total != 1000 || jw.merge_total != 1000 {
        return Err(E_JITTER_MISS);
    }
    hits += 1;

    // R2 窗界语义：间隔 19 合并（<merge 窗）；间隔 20 但处冷却窗内
    //    拒切；冷却期满出窗恢复切换——边界逐位验证。
    let mut jw = JitterWin::new();
    jw.calibrate(20, 40, 3, 0)?;
    jw.freeze()?;
    let mut tick = 10u64;
    jw.press(tick)?; // 切到 English，last_switch=10
    tick += 19;      // gap=19 <20
    if jw.press(tick)? != PressOutcome::Merged {
        return Err(E_MISSWITCH);
    }
    tick += 20;      // gap=20 ≥merge，但距切换 39 <40 → 冷却拒切
    if jw.press(tick)? != PressOutcome::RejectedCool {
        return Err(E_STATE_BAD);
    }
    tick += 42;      // 距上有效按下 42 ≥merge，距切换 81 ≥cool → 切换
    if jw.press(tick)? != PressOutcome::Switched {
        return Err(E_STATE_BAD);
    }
    jw.assert_zero_misswitch()?;
    hits += 1;

    // R3 冷却拒切：切换后 30ms 处再按（≥merge 但 <cool）→ 拒切态不变；
    //    55ms 处再按（≥merge 且出冷却）恢复切换。
    let mut jw = JitterWin::new();
    jw.calibrate(20, 40, 2, 0)?;
    jw.freeze()?;
    let mut tick = 0u64;
    jw.press(tick)?; // 切到 English
    tick += 30;      // ≥merge(20) 但 <cool(40)
    if jw.press(tick)? != PressOutcome::RejectedCool {
        return Err(E_STATE_BAD);
    }
    if jw.state != SwState::English {
        return Err(E_STATE_BAD);
    }
    tick += 25; // 距上有效按下 25 ≥merge，距切换 55 ≥cool
    if jw.press(tick)? != PressOutcome::Switched {
        return Err(E_STATE_BAD);
    }
    if jw.state != SwState::TempEnglish {
        return Err(E_STATE_BAD);
    }
    if jw.reject_total != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R4 三态轮转：连续 6 次合法切换走满 Chinese→English→Temp→Chinese
    //    →English→Temp→Chinese 两圈。
    let mut jw = JitterWin::new();
    jw.calibrate(DEF_MERGE_MS, DEF_COOL_MS, 6, 0)?;
    jw.freeze()?;
    let mut tick = 0u64;
    let want: [SwState; 6] = [
        SwState::English,
        SwState::TempEnglish,
        SwState::Chinese,
        SwState::English,
        SwState::TempEnglish,
        SwState::Chinese,
    ];
    let mut i = 0usize;
    while i < 6 {
        tick += DEF_COOL_MS + 1;
        jw.press(tick)?;
        if jw.state != want[i] {
            return Err(E_STATE_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R5 冻结留痕与改窗拒收：freeze 后再 calibrate 必拒（E_FROZEN_EDIT），
    //    留痕三元组（窗宽/样本数/冻结 tick）逐位不变。
    let mut jw = JitterWin::new();
    let mark = jw.calibrate(25, 50, 321, 777)?;
    jw.freeze()?;
    if !jw.frozen || jw.freeze_mark != Some(mark) {
        return Err(E_STATE_BAD);
    }
    if mark.merge_ms != 25 || mark.cool_ms != 50 || mark.samples != 321 || mark.freeze_tick != 777 {
        return Err(E_STATE_BAD);
    }
    if jw.calibrate(30, 60, 10, 999).is_ok() {
        return Err(E_FROZEN_EDIT); // 冻结后改窗必拒
    }
    if jw.merge_ms != 25 || jw.cool_ms != 50 {
        return Err(E_FROZEN_EDIT); // 窗宽未被改动
    }
    hits += 1;

    // R6 非法窗宽拒收：merge=0、cool=0、merge>cool 全拒；未校准冻结拒。
    let mut jw = JitterWin::new();
    if jw.calibrate(0, 40, 1, 0).is_ok() {
        return Err(E_BAD_WINDOW);
    }
    if jw.calibrate(20, 0, 1, 0).is_ok() {
        return Err(E_BAD_WINDOW);
    }
    if jw.calibrate(50, 40, 1, 0).is_ok() {
        return Err(E_BAD_WINDOW);
    }
    if jw.freeze().is_ok() {
        return Err(E_STATE_BAD); // 未校准
    }
    hits += 1;

    // R7 时刻回退拒收：按下时刻倒退必拒且不落账。
    let mut jw = JitterWin::new();
    jw.press(100)?;
    if jw.press(99).is_ok() {
        return Err(E_TIME_BACK);
    }
    if jw.merge_total != 0 && jw.reject_total != 0 {
        return Err(E_STATE_BAD);
    }
    if jw.switch_total != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 校准缺省一致性：不校准直接用缺省窗（20/40）行为与校准后一致——
    //    19ms 合并、41ms 出窗切换。
    let mut jd = JitterWin::new(); // 缺省窗，不冻结也可判（缺省即生效）
    let mut tick = 0u64;
    jd.press(tick)?;
    tick += 19;
    if jd.press(tick)? != PressOutcome::Merged {
        return Err(E_MISSWITCH);
    }
    tick += 41;
    if jd.press(tick)? != PressOutcome::Switched {
        return Err(E_STATE_BAD);
    }
    jd.assert_zero_misswitch()?;
    // 写回（判据 J2）。
    if jd.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    jd.writeback(true, 0x30)?;
    hits += 1;

    Ok(hits)
}
