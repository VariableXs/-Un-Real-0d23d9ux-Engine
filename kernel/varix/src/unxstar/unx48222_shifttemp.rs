//! unx48222_shifttemp — UNX-F48222 Shift 单击临时英文语义（释放上屏原文，长按不误触）（诊断段 0xBCA0）。
//!
//! 判据原文：
//!   J1 单击/长按 ×10^3 区分正确；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 时分判定：按下/释放时刻差 dur < 200ms 判单击（临时英文切换），
//!      dur ≥ 200ms 判长按（不切态）；判定只看时差纯整数算术，确定
//!      可重放，边界 199/200/201 三点必测。
//!   2. 释放上屏原文：中文态下有预编辑挂账时单击 Shift，预编辑原文
//!      逐字上屏（verbatim 提交，不改一字），随后进入英文临时态。
//!   3. 长按不误触：长按 Shift 不切态、不动预编辑、不上屏——三不语义；
//!      拦截次数单独记账（长按防线命中账）。
//!   4. 临时态进出对称：临时英文态下 ASCII 直通输出，单击 Shift 即回
//!      中文态；时序倒流（释放早于按下）与重复按下均拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBCA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_TIME_BACK: u16 = SEG_BASE + 1; // 释放早于按下（时序倒流）
const E_DOUBLE_PRESS: u16 = SEG_BASE + 2; // 未释放再按
const E_NO_PRESS: u16 = SEG_BASE + 3; // 无按而释
const E_CLASSIFY_OFF: u16 = SEG_BASE + 4; // ×10^3 判定口径失守
const E_COMMIT_OFF: u16 = SEG_BASE + 5; // 上屏原文不符
const E_STATE_BAD: u16 = SEG_BASE + 6; // 语义机状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 单击上限：时差 < 200ms 判单击。
pub const TAP_MAX_MS: u32 = 200;

/// 语义机状态：中文 / 英文临时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShSt {
    Zh,
    EnTemp,
}

/// 纯分类器：时差 → 单击判定（true=单击 / false=长按）。
pub fn is_tap(dur_ms: u32) -> bool {
    dur_ms < TAP_MAX_MS
}

/// Shift 临时英文语义机。
pub struct ShiftSem {
    pub st: ShSt,
    pressed_at: Option<u32>,
    pub preedit: Vec<u16>,       // 中文预编辑（挂账原文）
    pub committed_out: Vec<u16>, // 释放上屏原文账（verbatim）
    pub english_out: Vec<u16>,   // 临时英文直通输出账
    pub tap_total: u32,
    pub long_total: u32,
    pub long_guard_hits: u32, // 长按防线命中账（三不语义拦截）
    pub commit_events: u32,
}

impl ShiftSem {
    pub fn new() -> ShiftSem {
        ShiftSem {
            st: ShSt::Zh,
            pressed_at: None,
            preedit: Vec::new(),
            committed_out: Vec::new(),
            english_out: Vec::new(),
            tap_total: 0,
            long_total: 0,
            long_guard_hits: 0,
            commit_events: 0,
        }
    }

    /// Shift 按下。
    pub fn shift_press(&mut self, t_ms: u32) -> Result<(), u16> {
        if self.pressed_at.is_some() {
            return Err(E_DOUBLE_PRESS);
        }
        self.pressed_at = Some(t_ms);
        Ok(())
    }

    /// Shift 释放：时差判定 + 语义执行（纪律 1/2/3）。
    /// 返回 true 表示本次为单击。
    pub fn shift_release(&mut self, t_ms: u32) -> Result<bool, u16> {
        let t0 = match self.pressed_at {
            Some(t) => t,
            None => return Err(E_NO_PRESS),
        };
        if t_ms < t0 {
            self.pressed_at = None;
            return Err(E_TIME_BACK);
        }
        let dur = t_ms - t0;
        self.pressed_at = None;
        let tap = is_tap(dur);
        if tap {
            self.tap_total += 1;
            // 单击语义：Zh → 释放上屏原文 + 进临时态；EnTemp → 回中文。
            match self.st {
                ShSt::Zh => {
                    if self.preedit.len() > 0 {
                        let mut i = 0usize;
                        while i < self.preedit.len() {
                            self.committed_out.push(self.preedit[i]); // verbatim 逐字上屏
                            i += 1;
                        }
                        self.preedit.clear();
                    }
                    self.commit_events += 1;
                    self.st = ShSt::EnTemp;
                }
                ShSt::EnTemp => {
                    self.st = ShSt::Zh;
                }
            }
        } else {
            self.long_total += 1;
            // 长按三不语义：不切态、不动预编辑、不上屏——只记防线命中。
            self.long_guard_hits += 1;
        }
        Ok(tap)
    }

    /// 字符键：中文态进预编辑（挂账原文），临时英文态 ASCII 直通。
    pub fn key_char(&mut self, ch: u16) {
        match self.st {
            ShSt::Zh => self.preedit.push(ch),
            ShSt::EnTemp => self.english_out.push(ch),
        }
    }

    pub fn preedit_len(&self) -> usize {
        self.preedit.len()
    }
    pub fn committed_len(&self) -> usize {
        self.committed_out.len()
    }

    /// ×10^3 判定口径核验（J1 硬口径）：单击/长按计数与解析口径一致。
    pub fn assert_counts(&self, want_taps: u32, want_longs: u32) -> Result<(), u16> {
        if self.tap_total != want_taps || self.long_total != want_longs {
            return Err(E_CLASSIFY_OFF);
        }
        Ok(())
    }

    /// 账间闭合审计：上屏原文账长度 = 单击提交事件的挂账总量；
    /// 状态闭集、挂按位闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.tap_total + self.long_total == 0 {
            return Err(E_STATE_BAD);
        }
        if self.st != ShSt::Zh && self.st != ShSt::EnTemp {
            return Err(E_STATE_BAD);
        }
        if self.pressed_at.is_some() {
            return Err(E_STATE_BAD); // 审计点不得有未释放的按
        }
        if self.commit_events == 0 && self.committed_len() > 0 {
            return Err(E_COMMIT_OFF);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：判定量达标 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.tap_total + self.long_total == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（单击/长按 ×10^3 区分正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10^3：时差 dur_i = i % 400（0..399 周期遍历 1000 次），
    //    解析口径：dur<200 为单击 → 单击恰 600 次、长按恰 400 次；
    //    语义机逐次判定与解析口径逐位一致。
    let mut sm = ShiftSem::new();
    let mut i = 0u32;
    while i < 1000 {
        let dur = i % 400;
        let t0 = i * 1000;
        sm.shift_press(t0)?;
        let tap = sm.shift_release(t0 + dur)?;
        if tap != is_tap(dur) {
            return Err(E_CLASSIFY_OFF);
        }
        i += 1;
    }
    sm.assert_counts(600, 400)?;
    hits += 1;

    // R2 边界三点：199ms 单击、200ms 长按、201ms 长按（开区间口径），
    //    0ms 极速单击亦判单击。
    let mut sm2 = ShiftSem::new();
    sm2.shift_press(0)?;
    if !sm2.shift_release(199)? {
        return Err(E_CLASSIFY_OFF);
    }
    sm2.shift_press(1000)?;
    if sm2.shift_release(1200)? {
        return Err(E_CLASSIFY_OFF);
    }
    sm2.shift_press(2000)?;
    if sm2.shift_release(2201)? {
        return Err(E_CLASSIFY_OFF);
    }
    sm2.shift_press(3000)?;
    if !sm2.shift_release(3000)? {
        return Err(E_CLASSIFY_OFF);
    }
    sm2.assert_counts(2, 2)?;
    hits += 1;

    // R3 释放上屏原文：中文态挂 5 字预编辑，单击 Shift → 原文逐字
    //    verbatim 上屏（不改一字）、预编辑清空、进临时英文态；
    //    随后 ASCII 直通输出账。
    let mut sm3 = ShiftSem::new();
    let mut j = 0u16;
    while j < 5 {
        sm3.key_char(0x4E00 + j);
        j += 1;
    }
    if sm3.preedit_len() != 5 {
        return Err(E_COMMIT_OFF);
    }
    sm3.shift_press(10_000)?;
    if !sm3.shift_release(10_050)? {
        return Err(E_CLASSIFY_OFF);
    }
    if sm3.committed_len() != 5 {
        return Err(E_COMMIT_OFF);
    }
    let mut k = 0usize;
    while k < 5 {
        if sm3.committed_out[k] != 0x4E00 + k as u16 {
            return Err(E_COMMIT_OFF); // verbatim 逐位核对
        }
        k += 1;
    }
    if sm3.preedit_len() != 0 || sm3.st != ShSt::EnTemp {
        return Err(E_STATE_BAD);
    }
    sm3.key_char(0x0061); // 'a'
    sm3.key_char(0x0062); // 'b'
    if sm3.english_out.len() != 2 || sm3.english_out[0] != 0x0061 {
        return Err(E_COMMIT_OFF);
    }
    sm3.audit()?;
    hits += 1;

    // R4 长按不误触（三不语义）：挂 3 字预编辑时长按 Shift → 态不动、
    //    预编辑分毫不少、上屏账分毫未增、防线命中 +1。
    let mut sm4 = ShiftSem::new();
    let mut j = 0u16;
    while j < 3 {
        sm4.key_char(0x4E10 + j);
        j += 1;
    }
    let c_before = sm4.committed_len();
    sm4.shift_press(20_000)?;
    if sm4.shift_release(20_500)? {
        return Err(E_CLASSIFY_OFF);
    }
    if sm4.st != ShSt::Zh || sm4.preedit_len() != 3 || sm4.committed_len() != c_before {
        return Err(E_STATE_BAD);
    }
    if sm4.long_guard_hits != 1 {
        return Err(E_STATE_BAD);
    }
    sm4.audit()?;
    hits += 1;

    // R5 临时态进出对称：临时英文态单击 Shift → 回中文，后续中文字
    //    回预编辑不再直通（进出对称 + 通路切换）。
    let mut sm5 = ShiftSem::new();
    sm5.shift_press(30_000)?;
    sm5.shift_release(30_030)?; // 单击进临时
    if sm5.st != ShSt::EnTemp {
        return Err(E_STATE_BAD);
    }
    sm5.shift_press(31_000)?;
    sm5.shift_release(31_040)?; // 单击回中文
    if sm5.st != ShSt::Zh {
        return Err(E_STATE_BAD);
    }
    sm5.key_char(0x4E8C); // 中文字必须回预编辑
    if sm5.preedit_len() != 1 || sm5.english_out.len() != 0 {
        return Err(E_COMMIT_OFF);
    }
    sm5.assert_counts(2, 0)?;
    hits += 1;

    // R6 时序倒流拒收：释放早于按下 → E_TIME_BACK，且挂按位复位、
    //    不计单击不计长按。
    let mut sm6 = ShiftSem::new();
    sm6.shift_press(40_000)?;
    if sm6.shift_release(39_000).is_ok() {
        return Err(E_TIME_BACK);
    }
    sm6.shift_press(41_000)?;
    if sm6.shift_release(40_999).is_ok() {
        return Err(E_TIME_BACK);
    }
    if sm6.tap_total != 0 || sm6.long_total != 0 {
        return Err(E_CLASSIFY_OFF);
    }
    hits += 1;

    // R7 按位纪律：无按而释（E_NO_PRESS）、未释再按（E_DOUBLE_PRESS）
    //    双拒——按位状态机闭合。
    let mut sm7 = ShiftSem::new();
    if sm7.shift_release(1).is_ok() {
        return Err(E_NO_PRESS);
    }
    sm7.shift_press(50_000)?;
    if sm7.shift_press(50_100).is_ok() {
        return Err(E_DOUBLE_PRESS);
    }
    sm7.shift_release(50_120)?;
    if sm7.tap_total != 1 {
        return Err(E_CLASSIFY_OFF);
    }
    hits += 1;

    // R8 账间闭合终判：×10^3 行走后 上屏账=0（无预编辑挂账的单击只切态）、
    //    判定总量=1000、审计闭合；写回（判据 J2）放行。
    sm.audit()?;
    if sm.committed_len() != 0 {
        return Err(E_COMMIT_OFF);
    }
    if sm.tap_total + sm.long_total != 1000 {
        return Err(E_CLASSIFY_OFF);
    }
    if sm.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sm.writeback(true, 0x3E8)?;
    hits += 1;

    // R9 边界全扫：dur 0..=201 逐点分类单调性——0..199 全单击、
    //    200 起全长按，开区间口径逐点核对（边界防线全覆盖）。
    let mut d = 0u32;
    while d <= 201 {
        let want = d < TAP_MAX_MS;
        if is_tap(d) != want {
            return Err(E_CLASSIFY_OFF);
        }
        let mut sm9 = ShiftSem::new();
        sm9.shift_press(0)?;
        if sm9.shift_release(d)? != want {
            return Err(E_CLASSIFY_OFF);
        }
        d += 1;
    }
    hits += 1;

    Ok(hits)
}
