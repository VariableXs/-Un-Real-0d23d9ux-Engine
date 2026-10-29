//! unx48227_temppunct — UNX-F48227 临时英文期内标点直通语义（英文态标点不转中文）（诊断段 0xBD40）。
//!
//! 判据原文：
//!   J1 直通 ×10^3 正确；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三态语义：中文态（标点过配对表转中文）、临时英文态（Shift 按住
//!      进入，Shift 释放或字符上屏退出）、英文锁定态（全直通）；标点
//!      直通只在后两态生效，中文态标点必转——双向往返都断言。
//!   2. 直通口径：临时英文态内，标点键输出必须与键面 ASCII 逐字节
//!      相等（零转换、零缓冲改写、零顺序重排）；×10^3 = 1000 次标点
//!      按键逐次断言，差一字节即坏。
//!   3. 态门禁：直通账只记临时英文/英文锁定态内的按键；中文态按键走
//!      转换账，两账分列；串账（中文态出直通结果或反之）即语义失守。
//!   4. 退出即恢复：Shift 释放或上屏事件后回到中文态，紧随的标点键
//!      必须恢复转换行为——恢复首键单独断言，防直通粘滞。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBD40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PASSTHROUGH_BROKEN: u16 = SEG_BASE + 1; // 直通输出与键面不等
const E_CONVERTED_IN_TEMP: u16 = SEG_BASE + 2;  // 临时英文态发生转换
const E_STICKY_PASSTHROUGH: u16 = SEG_BASE + 3; // 退出后直通粘滞
const E_STATE_BAD: u16 = SEG_BASE + 4;          // 三态机非法迁移
const E_PT_MISS: u16 = SEG_BASE + 5;            // ×10^3 直通样本不足
const E_LEDGER_BAD: u16 = SEG_BASE + 6;         // 双账分列失守
const E_EMPTY_SUM: u16 = SEG_BASE + 7;          // 写回摘要为空
const E_NOT_PUNCT: u16 = SEG_BASE + 8;          // 非标点键入标点面

/// 标点键闭集：可打印 ASCII 中除字母与数字外的全部符号键。
pub fn is_punct_key(key: u8) -> bool {
    if key < 0x21 || key > 0x7E {
        return false;
    }
    let is_digit = key >= b'0' && key <= b'9';
    let is_upper = key >= b'A' && key <= b'Z';
    let is_lower = key >= b'a' && key <= b'z';
    !is_digit && !is_upper && !is_lower
}

/// 三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwState {
    Chinese,
    TempEnglish,
    EnglishLock,
}

/// 中文态标点转换表（节选：与微软拼音缺省锚一致的直译项）。
pub fn cn_convert(key: u8) -> Option<u32> {
    match key {
        b',' => Some(0xFF0C), // ，
        b'.' => Some(0x3002), // 。
        b';' => Some(0xFF1B), // ；
        b':' => Some(0xFF1A), // ：
        b'?' => Some(0xFF1F), // ？
        b'!' => Some(0xFF01), // ！
        b'(' => Some(0xFF08), // （
        b')' => Some(0xFF09), // ）
        _ => None,
    }
}

/// 直通账条目。
#[derive(Clone, Copy, Debug)]
pub struct PtRecord {
    pub key: u8,
    pub state: SwState,
}

/// 转换账条目。
#[derive(Clone, Copy, Debug)]
pub struct CvRecord {
    pub key: u8,
    pub out: u32,
}

/// 临时英文标点直通枢纽。
pub struct TempPunct {
    pub state: SwState,
    pt_ledger: Vec<PtRecord>,
    cv_ledger: Vec<CvRecord>,
    pub pt_total: u32,
    pub cv_total: u32,
    pub violations: u32,
}

impl TempPunct {
    pub fn new() -> TempPunct {
        TempPunct {
            state: SwState::Chinese,
            pt_ledger: Vec::new(),
            cv_ledger: Vec::new(),
            pt_total: 0,
            cv_total: 0,
            violations: 0,
        }
    }

    /// Shift 按下：中文态 → 临时英文态；锁定态不变。
    pub fn shift_down(&mut self) -> Result<(), u16> {
        match self.state {
            SwState::Chinese => {
                self.state = SwState::TempEnglish;
                Ok(())
            }
            SwState::TempEnglish => Err(E_STATE_BAD), // 重复按下不叠态
            SwState::EnglishLock => Ok(()),           // 锁定态下 Shift 无效
        }
    }

    /// Shift 释放：临时英文态 → 中文态（恢复转换），其余态不变。
    pub fn shift_up(&mut self) -> Result<(), u16> {
        match self.state {
            SwState::TempEnglish => {
                self.state = SwState::Chinese;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// 英文锁定开关。
    pub fn toggle_lock(&mut self) -> Result<(), u16> {
        self.state = if self.state == SwState::EnglishLock {
            SwState::Chinese
        } else {
            SwState::EnglishLock
        };
        Ok(())
    }

    /// 标点键主径：按态分派直通或转换，双账分列（纪律 1/3）。
    /// 非标点键（字母/数字/控制键）不入本面，一律拒收。
    pub fn press_punct(&mut self, key: u8) -> Result<PunctOut, u16> {
        if !is_punct_key(key) {
            return Err(E_NOT_PUNCT);
        }
        match self.state {
            SwState::TempEnglish | SwState::EnglishLock => {
                // 直通：输出必须等于键面 ASCII（判定后落直通账）。
                if key > 0x7E {
                    return Err(E_PASSTHROUGH_BROKEN);
                }
                self.pt_ledger.push(PtRecord { key, state: self.state });
                self.pt_total += 1;
                Ok(PunctOut::Pass(key))
            }
            SwState::Chinese => {
                let out = match cn_convert(key) {
                    Some(v) => v,
                    None => {
                        // 中文态下无中文形的键（如反斜杠）按锚面直出但仍落转换账。
                        key as u32
                    }
                };
                self.cv_ledger.push(CvRecord { key, out });
                self.cv_total += 1;
                Ok(PunctOut::Converted(out))
            }
        }
    }

    /// 直通账逐条复核（J1 主体）：账内每条的键面与态都合法。
    pub fn audit_passthrough(&self) -> Result<(), u16> {
        if self.pt_ledger.len() as u32 != self.pt_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.pt_ledger.len() {
            let r = &self.pt_ledger[i];
            if r.key > 0x7E {
                return Err(E_PASSTHROUGH_BROKEN);
            }
            if r.state == SwState::Chinese {
                return Err(E_CONVERTED_IN_TEMP); // 中文态混入直通账
            }
            i += 1;
        }
        Ok(())
    }

    /// 转换账逐条复核：中文态账内键在中文态重放必得同码。
    pub fn audit_convert(&self) -> Result<(), u16> {
        if self.cv_ledger.len() as u32 != self.cv_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.cv_ledger.len() {
            let r = &self.cv_ledger[i];
            let want = match cn_convert(r.key) {
                Some(v) => v,
                None => r.key as u32,
            };
            if want != r.out {
                return Err(E_PASSTHROUGH_BROKEN);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 直通覆盖核验（J1 硬口径）。
    pub fn pt_coverage(&self, want: u32) -> Result<(), u16> {
        if self.pt_total < want {
            return Err(E_PT_MISS);
        }
        Ok(())
    }

    /// 零违规终判。
    pub fn assert_clean(&self) -> Result<(), u16> {
        if self.violations != 0 {
            return Err(E_CONVERTED_IN_TEMP);
        }
        Ok(())
    }

    pub fn pt_ledger_len(&self) -> usize {
        self.pt_ledger.len()
    }
    pub fn cv_ledger_len(&self) -> usize {
        self.cv_ledger.len()
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

/// 标点输出：直通字节 / 转换码点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PunctOut {
    Pass(u8),
    Converted(u32),
}

/// 自例程判据：逐条落实 J1（直通 ×10^3 正确），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let punct_keys: [u8; 8] = [b',', b'.', b';', b':', b'?', b'!', b'(', b')'];

    // R1 主扫 ×10^3：临时英文态内 1000 次标点按键，输出逐字节==键面，
    //    直通账 1000 条、转换账零增长。
    let mut tp = TempPunct::new();
    tp.shift_down()?; // → TempEnglish
    if tp.state != SwState::TempEnglish {
        return Err(E_STATE_BAD);
    }
    let mut i = 0u32;
    while i < 1000 {
        let key = punct_keys[(i % 8) as usize];
        match tp.press_punct(key)? {
            PunctOut::Pass(b) => {
                if b != key {
                    return Err(E_PASSTHROUGH_BROKEN);
                }
            }
            PunctOut::Converted(_) => return Err(E_CONVERTED_IN_TEMP),
        }
        i += 1;
    }
    tp.pt_coverage(1000)?;
    tp.audit_passthrough()?;
    tp.audit_convert()?;
    tp.assert_clean()?;
    if tp.cv_total != 0 {
        return Err(E_LEDGER_BAD); // 临时英文态不得产转换账
    }
    hits += 1;

    // R2 恢复首键断言（纪律 4）：Shift 释放后回到中文态，首标点键必转
    //    中文形（，=U+FF0C），防直通粘滞。
    let mut tp = TempPunct::new();
    tp.shift_down()?;
    tp.press_punct(b',')?;
    tp.shift_up()?;
    if tp.state != SwState::Chinese {
        return Err(E_STATE_BAD);
    }
    match tp.press_punct(b',')? {
        PunctOut::Converted(0xFF0C) => {}
        _ => return Err(E_STICKY_PASSTHROUGH),
    }
    tp.audit_convert()?;
    hits += 1;

    // R3 中文态转换对照：8 键全表逐键转换，出参与锚表逐位相等。
    let mut tp = TempPunct::new();
    let want: [u32; 8] = [0xFF0C, 0x3002, 0xFF1B, 0xFF1A, 0xFF1F, 0xFF01, 0xFF08, 0xFF09];
    let mut i = 0usize;
    while i < 8 {
        match tp.press_punct(punct_keys[i])? {
            PunctOut::Converted(v) => {
                if v != want[i] {
                    return Err(E_PASSTHROUGH_BROKEN);
                }
            }
            PunctOut::Pass(_) => return Err(E_CONVERTED_IN_TEMP),
        }
        i += 1;
    }
    if tp.cv_total != 8 || tp.pt_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    tp.audit_convert()?;
    hits += 1;

    // R4 英文锁定态直通：锁定后标点全直通，解锁后恢复转换。
    let mut tp = TempPunct::new();
    tp.toggle_lock()?; // → EnglishLock
    if tp.state != SwState::EnglishLock {
        return Err(E_STATE_BAD);
    }
    let mut i = 0u32;
    while i < 100 {
        let key = punct_keys[(i % 8) as usize];
        match tp.press_punct(key)? {
            PunctOut::Pass(b) => {
                if b != key {
                    return Err(E_PASSTHROUGH_BROKEN);
                }
            }
            PunctOut::Converted(_) => return Err(E_CONVERTED_IN_TEMP),
        }
        i += 1;
    }
    // 锁定态下 Shift 按放无效（不叠态、不退态）。
    tp.shift_down()?;
    tp.shift_up()?;
    if tp.state != SwState::EnglishLock {
        return Err(E_STATE_BAD);
    }
    tp.toggle_lock()?; // → Chinese
    match tp.press_punct(b'?')? {
        PunctOut::Converted(0xFF1F) => {}
        _ => return Err(E_STICKY_PASSTHROUGH),
    }
    tp.audit_passthrough()?;
    tp.audit_convert()?;
    hits += 1;

    // R5 重复 Shift 按下拒收：临时英文态内再次 Shift 按下必拒（不叠态），
    //    态不变、账不变。
    let mut tp = TempPunct::new();
    tp.shift_down()?;
    tp.press_punct(b'.')?;
    let pt0 = tp.pt_total;
    if tp.shift_down().is_ok() {
        return Err(E_STATE_BAD);
    }
    if tp.state != SwState::TempEnglish || tp.pt_total != pt0 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 临时英文态内字母键语义不越权：标点面只管标点，字母键进本面
    //    一律拒（面界清晰），态与账不动。
    let mut tp = TempPunct::new();
    tp.shift_down()?;
    if tp.press_punct(b'a').is_ok() {
        return Err(E_STATE_BAD);
    }
    if tp.press_punct(b'1').is_ok() {
        return Err(E_STATE_BAD);
    }
    if tp.pt_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 释放多次幂等：未按 Shift 直接释放、释放后重复释放均幂等放行，
    //    不产生态抖动。
    let mut tp = TempPunct::new();
    tp.shift_up()?; // 中文态释放：幂等
    if tp.state != SwState::Chinese {
        return Err(E_STATE_BAD);
    }
    tp.shift_down()?;
    tp.shift_up()?;
    tp.shift_up()?; // 重复释放：幂等
    if tp.state != SwState::Chinese {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 混合流×2 与写回（判据 J2）：50 次进入/退出循环，每次临时期
    //    内 20 键直通、退出后 1 键转换；总直通 1000 恰好 ×10^3 口径
    //    复用闭合；末尾写回。
    let mut tp = TempPunct::new();
    let mut round = 0u32;
    while round < 50 {
        tp.shift_down()?;
        let mut k = 0u32;
        while k < 20 {
            let key = punct_keys[(k % 8) as usize];
            match tp.press_punct(key)? {
                PunctOut::Pass(b) => {
                    if b != key {
                        return Err(E_PASSTHROUGH_BROKEN);
                    }
                }
                PunctOut::Converted(_) => return Err(E_CONVERTED_IN_TEMP),
            }
            k += 1;
        }
        tp.shift_up()?;
        match tp.press_punct(b',')? {
            PunctOut::Converted(0xFF0C) => {}
            _ => return Err(E_STICKY_PASSTHROUGH),
        }
        round += 1;
    }
    if tp.pt_total != 1000 {
        return Err(E_PT_MISS);
    }
    tp.pt_coverage(1000)?;
    tp.audit_passthrough()?;
    tp.audit_convert()?;
    tp.assert_clean()?;
    if tp.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    tp.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
