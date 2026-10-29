//! UNX-F48568 —— I·候选窗与焦点丢失联签（失焦即收，焦点还回触发元素数据位）
//! 判据原文：UNX-F48568-J1 候选窗展开态失焦立即收回（retract 置位 + 收回
//!           事件含失焦时刻组合态）；焦点还回时触发元素数据位记回焦来源
//!           元素 id；重复失焦幂等；组合中途失焦同样收回且组合态不外泄；
//!           LCG 随机焦点序列 ×256 状态自洽；收回账封闭；重演一致；守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF6A0; // I29 焦点丢失联签段

const E_RETRACT_MISS: u16 = SEG_BASE + 1; // 失焦未收回
const E_BIT_MISS: u16 = SEG_BASE + 2; // 还焦数据位缺失
const E_DOUBLE_RETRACT: u16 = SEG_BASE + 3; // 重复失焦未幂等
const E_STATE_BAD: u16 = SEG_BASE + 4; // 焦点态失准
const E_PREEDIT_LEAK: u16 = SEG_BASE + 5; // 组合态外泄
const E_KIND_BAD: u16 = SEG_BASE + 6; // 收回账形态越界
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

/// 焦点态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FocusState {
    Focused = 0,
    Lost = 1,
}

/// 候选窗展开态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CandState {
    Expanded = 0,
    Retracted = 1,
}

/// 收回事件。
pub struct RetractEvent {
    pub seq: u64,
    pub cand_state: CandState, // 失焦时刻组合态（展开/收起）
    pub had_preedit: bool,     // 失焦时是否有未上屏组合串
}

/// 还焦记录。
pub struct RefocusBit {
    pub seq: u64,
    pub element: u32, // 回焦来源元素 id
}

/// 焦点联签器。
pub struct FocusSentinel {
    pub focus: FocusState,
    pub cand: CandState,
    pub preedit_len: usize,
    pub retracts: Vec<RetractEvent>,
    pub refocus: Vec<RefocusBit>,
    pub seq: u64,
    pub label: String,
}

impl FocusSentinel {
    pub fn new() -> Self {
        FocusSentinel {
            focus: FocusState::Focused,
            cand: CandState::Expanded,
            preedit_len: 0,
            retracts: Vec::new(),
            refocus: Vec::new(),
            seq: 0,
            label: "focus-sentinel".to_string(),
        }
    }

    /// 失焦：收回候选窗、清组合态并记事件；已 Lost 幂等拒。
    pub fn on_focus_lost(&mut self) -> Result<(), u16> {
        self.seq += 1;
        if self.focus == FocusState::Lost {
            return Err(E_DOUBLE_RETRACT);
        }
        let ev = RetractEvent {
            seq: self.seq,
            cand_state: self.cand,
            had_preedit: self.preedit_len > 0,
        };
        self.retracts.push(ev);
        self.cand = CandState::Retracted;
        // 组合态就地清空不外泄
        self.preedit_len = 0;
        self.focus = FocusState::Lost;
        Ok(())
    }

    /// 还焦：焦点恢复 + 触发元素数据位登记。
    pub fn on_refocus(&mut self, element: u32) -> Result<(), u16> {
        self.seq += 1;
        if self.focus == FocusState::Focused {
            return Err(E_STATE_BAD);
        }
        self.refocus.push(RefocusBit { seq: self.seq, element });
        self.focus = FocusState::Focused;
        self.cand = CandState::Expanded;
        Ok(())
    }

    /// 展开态登记（模拟输入活动）。
    pub fn set_preedit(&mut self, n: usize) {
        if self.focus == FocusState::Focused && self.cand == CandState::Expanded {
            self.preedit_len = n;
        }
    }

    /// 收回账封闭：事件态合法、收回后必 Retracted/Lost。
    pub fn audit_retracts(&self) -> Result<(), u16> {
        if self.focus == FocusState::Lost && self.cand != CandState::Retracted {
            return Err(E_RETRACT_MISS);
        }
        let mut i = 0usize;
        while i < self.retracts.len() {
            if self.retracts[i].cand_state as u8 > 1 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 还焦数据位：条目与还焦次数一致且元素域合法。
    pub fn audit_refocus_bits(&self, expect_len: usize) -> Result<(), u16> {
        if self.refocus.len() != expect_len {
            return Err(E_BIT_MISS);
        }
        let mut i = 0usize;
        while i < self.refocus.len() {
            if self.refocus[i].element == 0 {
                return Err(E_BIT_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.focus as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.cand as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.retracts.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.refocus.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.retracts.len() {
            h ^= self.retracts[i].cand_state as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.retracts[i].had_preedit as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 主巡检：焦点丢失联签八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 失焦即收：展开态失焦 → Retracted + 事件记录
    let mut s = FocusSentinel::new();
    match s.on_focus_lost() {
        Ok(()) => {}
        _ => return Err(E_RETRACT_MISS),
    }
    if s.cand != CandState::Retracted || s.focus != FocusState::Lost {
        return Err(E_RETRACT_MISS);
    }
    if s.retracts.len() != 1 || s.retracts[0].cand_state != CandState::Expanded {
        return Err(E_RETRACT_MISS);
    }
    hits += 1;

    // R2 焦点还回触发元素数据位
    match s.on_refocus(7) {
        Ok(()) => {}
        _ => return Err(E_BIT_MISS),
    }
    if s.focus != FocusState::Focused || s.cand != CandState::Expanded {
        return Err(E_STATE_BAD);
    }
    s.audit_refocus_bits(1)?;
    if s.refocus[0].element != 7 {
        return Err(E_BIT_MISS);
    }
    hits += 1;

    // R3 重复失焦幂等拒
    let _ = s.on_focus_lost()?;
    match s.on_focus_lost() {
        Err(E_DOUBLE_RETRACT) => {}
        _ => return Err(E_DOUBLE_RETRACT),
    }
    if s.retracts.len() != 2 {
        return Err(E_DOUBLE_RETRACT);
    }
    hits += 1;

    // R4 组合中途失焦：收回且组合清空不外泄
    let mut s2 = FocusSentinel::new();
    s2.set_preedit(9);
    if s2.preedit_len != 9 {
        return Err(E_PREEDIT_LEAK);
    }
    let _ = s2.on_focus_lost()?;
    if s2.preedit_len != 0 {
        return Err(E_PREEDIT_LEAK);
    }
    if s2.retracts[0].had_preedit != true {
        return Err(E_PREEDIT_LEAK);
    }
    s2.audit_retracts()?;
    hits += 1;

    // R5 Lost 态下 set_preedit 不生效
    s2.set_preedit(5);
    if s2.preedit_len != 0 {
        return Err(E_PREEDIT_LEAK);
    }
    hits += 1;

    // R6 收回-还焦循环 ×3 数据位递增
    let mut s3 = FocusSentinel::new();
    let mut el = 1u32;
    let mut round = 0usize;
    while round < 3 {
        let _ = s3.on_focus_lost()?;
        let _ = s3.on_refocus(el)?;
        el += 2;
        round += 1;
    }
    s3.audit_refocus_bits(3)?;
    if s3.refocus[0].element != 1 || s3.refocus[1].element != 3 || s3.refocus[2].element != 5 {
        return Err(E_BIT_MISS);
    }
    s3.audit_retracts()?;
    hits += 1;

    // R7 LCG 随机焦点序列 ×256：状态自洽零崩
    let mut rng = Lcg::new(0x48568);
    let mut s4 = FocusSentinel::new();
    let mut k = 0u32;
    let mut n_retract = 0u32;
    let mut n_refocus = 0u32;
    while k < 256 {
        let act = rng.below(4);
        if act == 0 {
            if s4.focus == FocusState::Focused {
                let _ = s4.on_focus_lost()?;
                n_retract += 1;
            } else {
                match s4.on_focus_lost() {
                    Err(E_DOUBLE_RETRACT) => {}
                    _ => return Err(E_DOUBLE_RETRACT),
                }
            }
        } else if act == 1 {
            if s4.focus == FocusState::Lost {
                let elid = 1 + rng.below(99) as u32;
                let _ = s4.on_refocus(elid)?;
                n_refocus += 1;
            } else {
                match s4.on_refocus(1) {
                    Err(E_STATE_BAD) => {}
                    _ => return Err(E_STATE_BAD),
                }
            }
        } else if act == 2 {
            s4.set_preedit((rng.below(16)) as usize);
        }
        // 不变式：Lost 必 Retracted；Focused 时 retract/refocus 账与转换数一致
        if s4.focus == FocusState::Lost && s4.cand != CandState::Retracted {
            return Err(E_STATE_BAD);
        }
        k += 1;
    }
    s4.audit_retracts()?;
    if n_retract == 0 || n_refocus == 0 {
        return Err(E_BAL_MISS);
    }
    if s4.retracts.len() as u32 != n_retract || s4.refocus.len() as u32 != n_refocus {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = s4.fingerprint();
    let mut rng2 = Lcg::new(0x48568);
    let mut s5 = FocusSentinel::new();
    let mut k = 0u32;
    while k < 256 {
        let act = rng2.below(4);
        if act == 0 {
            let _ = s5.on_focus_lost();
        } else if act == 1 {
            if s5.focus == FocusState::Lost {
                let elid = 1 + rng2.below(99) as u32;
                let _ = s5.on_refocus(elid);
            } else {
                let _ = s5.on_refocus(1);
            }
        } else if act == 2 {
            s5.set_preedit((rng2.below(16)) as usize);
        }
        k += 1;
    }
    if s5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
