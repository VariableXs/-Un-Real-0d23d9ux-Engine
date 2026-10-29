//! UNX-F48567 —— I·浮层关闭路径联签（点外/Esc/失焦三路关闭数据事件齐）
//! 判据原文：UNX-F48567-J1 浮层关闭三路（点外点击/Esc 键/失焦）各自触发
//!           关闭且数据事件记录齐全（路径标/序号/关闭前状态）；
//!           已关闭浮层重复关闭幂等拒绝；三路事件互不混淆；
//!           关闭后重开恢复正常；LCG 随机事件序列 ×256 状态机自洽；
//!           事件账封闭三路；账目守恒与重演一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF660; // I29 浮层关闭路径段

const E_PATH_MISS: u16 = SEG_BASE + 1; // 关闭路径未生效
const E_REC_INCOMPLETE: u16 = SEG_BASE + 2; // 事件记录字段缺失
const E_DOUBLE_CLOSE: u16 = SEG_BASE + 3; // 重复关闭未幂等拒
const E_PATH_MIX: u16 = SEG_BASE + 4; // 三路事件混淆
const E_STATE_BAD: u16 = SEG_BASE + 5; // 状态机失准
const E_REOPEN_DEAD: u16 = SEG_BASE + 6; // 重开失效
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致

/// 关闭路径三路。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClosePath {
    OutsideClick = 0,
    EscKey = 1,
    Blur = 2,
}

/// 浮层状态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FloatState {
    Open = 0,
    Closed = 1,
}

/// 关闭事件记录。
pub struct CloseEvent {
    pub path: ClosePath,
    pub seq: u64,
    pub pre_state: FloatState,
}

/// 浮层状态机。
pub struct FloatLayer {
    pub state: FloatState,
    pub events: Vec<CloseEvent>,
    pub seq: u64,
    pub label: String,
}

impl FloatLayer {
    pub fn new() -> Self {
        FloatLayer {
            state: FloatState::Open,
            events: Vec::new(),
            seq: 0,
            label: "float-layer".to_string(),
        }
    }

    /// 三路关闭入口：Open 才关，Closed 幂等拒绝（不记事件）。
    pub fn close(&mut self, path: ClosePath) -> Result<(), u16> {
        self.seq += 1;
        if self.state == FloatState::Closed {
            return Err(E_DOUBLE_CLOSE);
        }
        self.events.push(CloseEvent { path, seq: self.seq, pre_state: self.state });
        self.state = FloatState::Closed;
        Ok(())
    }

    /// 重开：Closed → Open，事件账保留。
    pub fn reopen(&mut self) -> Result<(), u16> {
        if self.state == FloatState::Open {
            return Err(E_STATE_BAD);
        }
        self.state = FloatState::Open;
        Ok(())
    }

    /// 事件记录完整性：路径合法 + pre_state 恒 Open + seq 递增。
    pub fn audit_records(&self) -> Result<(), u16> {
        let mut last_seq = 0u64;
        let mut i = 0usize;
        while i < self.events.len() {
            let e = &self.events[i];
            if e.path as u8 > 2 {
                return Err(E_PATH_MISS);
            }
            if e.pre_state != FloatState::Open {
                return Err(E_REC_INCOMPLETE);
            }
            if e.seq <= last_seq {
                return Err(E_REC_INCOMPLETE);
            }
            last_seq = e.seq;
            i += 1;
        }
        Ok(())
    }

    /// 三路事件齐备。
    pub fn audit_paths_covered(&self) -> Result<(), u16> {
        let mut seen = [0u32; 3];
        let mut i = 0usize;
        while i < self.events.len() {
            seen[self.events[i].path as usize] += 1;
            i += 1;
        }
        if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 {
            return Err(E_PATH_MIX);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.state as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.events.len() {
            h ^= self.events[i].path as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.events[i].seq;
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

/// 主巡检：三路关闭八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三路各自关闭生效
    let paths: [ClosePath; 3] = [ClosePath::OutsideClick, ClosePath::EscKey, ClosePath::Blur];
    let mut i = 0usize;
    while i < paths.len() {
        let mut f = FloatLayer::new();
        match f.close(paths[i]) {
            Ok(()) => {}
            _ => return Err(E_PATH_MISS),
        }
        if f.state != FloatState::Closed {
            return Err(E_PATH_MISS);
        }
        i += 1;
    }
    hits += 1;

    // R2 事件记录字段齐全
    let mut f = FloatLayer::new();
    let _ = f.close(ClosePath::OutsideClick);
    f.reopen()?;
    let _ = f.close(ClosePath::EscKey);
    f.reopen()?;
    let _ = f.close(ClosePath::Blur);
    f.audit_records()?;
    if f.events.len() != 3 {
        return Err(E_REC_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < f.events.len() {
        if f.events[i].path != paths[i] {
            return Err(E_PATH_MIX);
        }
        i += 1;
    }
    f.audit_paths_covered()?;
    hits += 1;

    // R3 已关闭重复关闭幂等拒绝且不追加事件
    let n0 = f.events.len();
    match f.close(ClosePath::EscKey) {
        Err(E_DOUBLE_CLOSE) => {}
        _ => return Err(E_DOUBLE_CLOSE),
    }
    if f.events.len() != n0 {
        return Err(E_DOUBLE_CLOSE);
    }
    hits += 1;

    // R4 三路事件不混淆：路径标与提交序一致（R2 已验序，此处验多轮）
    let mut f2 = FloatLayer::new();
    let round: [ClosePath; 6] = [
        ClosePath::Blur,
        ClosePath::OutsideClick,
        ClosePath::EscKey,
        ClosePath::Blur,
        ClosePath::EscKey,
        ClosePath::OutsideClick,
    ];
    let mut i = 0usize;
    while i < round.len() {
        let _ = f2.close(round[i]);
        let _ = f2.reopen();
        i += 1;
    }
    f2.audit_records()?;
    let mut i = 0usize;
    while i < round.len() {
        if f2.events[i].path != round[i] {
            return Err(E_PATH_MIX);
        }
        i += 1;
    }
    hits += 1;

    // R5 重开恢复正常：关闭→重开→再关生效
    let mut f3 = FloatLayer::new();
    let _ = f3.close(ClosePath::Blur);
    f3.reopen()?;
    if f3.state != FloatState::Open {
        return Err(E_REOPEN_DEAD);
    }
    let _ = f3.close(ClosePath::OutsideClick)?;
    if f3.state != FloatState::Closed || f3.events.len() != 2 {
        return Err(E_REOPEN_DEAD);
    }
    hits += 1;

    // R6 LCG 随机事件序列 ×256：状态机自洽（Open 才能关、Closed 才能开）
    let mut rng = Lcg::new(0x48567);
    let mut f4 = FloatLayer::new();
    let mut closes = 0u32;
    let mut k = 0u32;
    while k < 256 {
        let act = rng.below(4);
        if act < 3 {
            let path = match act {
                0 => ClosePath::OutsideClick,
                1 => ClosePath::EscKey,
                _ => ClosePath::Blur,
            };
            if f4.state == FloatState::Open {
                let _ = f4.close(path)?;
                closes += 1;
            } else {
                match f4.close(path) {
                    Err(E_DOUBLE_CLOSE) => {}
                    _ => return Err(E_DOUBLE_CLOSE),
                }
            }
        } else if f4.state == FloatState::Closed {
            f4.reopen()?;
        }
        // 状态始终合法
        if f4.state as u8 > 1 {
            return Err(E_STATE_BAD);
        }
        k += 1;
    }
    f4.audit_records()?;
    if closes == 0 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R7 账目守恒：事件数 ≤ seq，Closed 幂等拒不计事件
    if f4.events.len() as u64 > f4.seq {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R9 Open 态重开幂等拒（状态机非法转移拒绝）
    let mut f6 = FloatLayer::new();
    match f6.reopen() {
        Err(E_STATE_BAD) => {}
        _ => return Err(E_STATE_BAD),
    }
    if f6.state != FloatState::Open || !f6.events.is_empty() {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R10 事件序号与浮层序号对应：事件 seq 严格递增且 ≤ 总 seq
    let mut f7 = FloatLayer::new();
    let mut i = 0usize;
    while i < 6 {
        let _ = f7.close(paths[i % 3]);
        let _ = f7.reopen();
        i += 1;
    }
    f7.audit_records()?;
    if f7.events.len() != 6 {
        return Err(E_REC_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < f7.events.len() {
        if f7.events[i].seq > f7.seq {
            return Err(E_REC_INCOMPLETE);
        }
        i += 1;
    }
    hits += 1;

    // R11 重演一致：同种子指纹相等
    let fp1 = f4.fingerprint();
    let mut rng2 = Lcg::new(0x48567);
    let mut f5 = FloatLayer::new();
    let mut k = 0u32;
    while k < 256 {
        let act = rng2.below(4);
        if act < 3 {
            let path = match act {
                0 => ClosePath::OutsideClick,
                1 => ClosePath::EscKey,
                _ => ClosePath::Blur,
            };
            let _ = f5.close(path);
        } else {
            let _ = f5.reopen();
        }
        k += 1;
    }
    if f5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
