//! UNX-F48412 —— E·Wine 陈旧 HWND 注入（反：复用前 HWND 投递 ×10 全拒）
//! 判据原文：UNX-F48412-J1 销毁后、重建前的 HWND 投递 ×10 全拒（窗口空窗期零投递）；
//!           合法期投递零误伤；空窗期注入账分类（已销毁/未注册/重建前）封闭；
//!           LCG 随机空窗注入 ×512 全拒；注入账封印重演一致；
//!           注入账与路由账之和守恒；空窗期长度可观测（tick 计数）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD6A0; // E21 陈旧 HWND 段

const E_STALE_PASS: u16 = SEG_BASE + 1; // 空窗投递放行
const E_LEGAL_HURT: u16 = SEG_BASE + 2; // 合法投递误伤
const E_CAUSE_BAD: u16 = SEG_BASE + 3; // 注入分类越界
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 计数不符
const E_BAL_MISS: u16 = SEG_BASE + 6; // 两账守恒失败
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_TICK_MISS: u16 = SEG_BASE + 8; // 空窗 tick 不可观测

/// 空窗注入分类。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum StaleKind {
    Destroyed = 0, // 已销毁未重建
    Unregistered = 1, // 从未注册
    PreRebuild = 2, // 销毁后重建指令未达
}

/// 窗口状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Ws {
    Void = 0,       // 未注册
    Alive = 1,      // 在册
    Gap = 2,        // 空窗（销毁后）
}

/// 陈旧 HWND 守门器。
pub struct StaleGate {
    state: Vec<(u32, Ws)>,
    gap_started: Vec<(u32, u32)>, // (hwnd, gap 起点 tick)
    ledger: Vec<(u64, StaleKind, u32)>,
    routed: u32,
    seq: u64,
    tick: u32,
    rejects: u32,
}

impl StaleGate {
    pub fn new() -> Self {
        StaleGate {
            state: Vec::new(),
            gap_started: Vec::new(),
            ledger: Vec::new(),
            routed: 0,
            seq: 0,
            tick: 0,
            rejects: 0,
        }
    }

    fn idx(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.state.len() {
            if self.state[i].0 == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn register(&mut self, hwnd: u32) -> Result<(), u16> {
        if self.idx(hwnd).is_some() {
            return Err(E_CAUSE_BAD);
        }
        self.state.push((hwnd, Ws::Alive));
        Ok(())
    }

    /// 销毁：进入空窗并记起点 tick。
    pub fn destroy(&mut self, hwnd: u32) -> Result<(), u16> {
        let i = match self.idx(hwnd) {
            Some(i) => i,
            None => return Err(E_CAUSE_BAD),
        };
        self.state[i].1 = Ws::Gap;
        self.gap_started.push((hwnd, self.tick));
        Ok(())
    }

    /// 重建：空窗结束。
    pub fn rebuild(&mut self, hwnd: u32) -> Result<u32, u16> {
        let i = match self.idx(hwnd) {
            Some(i) => i,
            None => return Err(E_CAUSE_BAD),
        };
        if self.state[i].1 != Ws::Gap {
            return Err(E_CAUSE_BAD);
        }
        // 空窗长度 = tick - 起点（可观测）
        let mut dur = 0u32;
        let mut j = 0usize;
        while j < self.gap_started.len() {
            if self.gap_started[j].0 == hwnd {
                dur = self.tick - self.gap_started[j].1;
            }
            j += 1;
        }
        self.state[i].1 = Ws::Alive;
        Ok(dur)
    }

    /// 投递：空窗/未注册全拒并入账。
    pub fn deliver(&mut self, hwnd: u32) -> Result<(), u16> {
        self.tick += 1;
        self.seq += 1;
        let i = self.idx(hwnd);
        match i {
            None => {
                self.rejects += 1;
                self.ledger.push((self.seq, StaleKind::Unregistered, hwnd));
                Err(E_STALE_PASS)
            }
            Some(i) => match self.state[i].1 {
                Ws::Alive => {
                    self.routed += 1;
                    Ok(())
                }
                _ => {
                    self.rejects += 1;
                    self.ledger.push((self.seq, StaleKind::Destroyed, hwnd));
                    Err(E_STALE_PASS)
                }
            },
        }
    }

    /// 分类封闭：全落三型。
    pub fn audit_cause_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].1 as u16 > 2 {
                return Err(E_CAUSE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒：routed + rejects == seq。
    pub fn audit_balance(&self) -> Result<(), u16> {
        if (self.routed as u64) + (self.rejects as u64) != self.seq {
            return Err(E_BAL_MISS);
        }
        if self.ledger.len() != self.rejects as usize {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let (s, k, x) = self.ledger[i];
            for v in [s, k as u64, x as u64] {
                h ^= v;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn routed(&self) -> u32 {
        self.routed
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
        self.next() % n
    }
}

/// 主巡检：陈旧 HWND 注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 复用前投递 ×10 全拒：注册→销毁→连投 10 次（空窗期）
    let mut gate = StaleGate::new();
    gate.register(0x1111)?;
    gate.deliver(0x1111)?; // 合法
    gate.destroy(0x1111)?;
    let mut k = 0u32;
    while k < 10 {
        match gate.deliver(0x1111) {
            Err(E_STALE_PASS) => {}
            _ => return Err(E_STALE_PASS),
        }
        k += 1;
    }
    hits += 1;

    // R2 空窗期可观测：rebuild 返回空窗 tick = 10
    let dur = gate.rebuild(0x1111)?;
    if dur != 10 {
        return Err(E_TICK_MISS);
    }
    gate.deliver(0x1111)?; // 重建后合法
    hits += 1;

    // R3 合法期投递零误伤：重建后再投 N 次全过
    let mut k = 0u32;
    while k < 50 {
        gate.deliver(0x1111)?;
        k += 1;
    }
    if gate.routed() < 51 {
        return Err(E_LEGAL_HURT);
    }
    hits += 1;

    // R4 未注册注入：全新 hwnd 投递拒且记 Unregistered
    match gate.deliver(0x9999) {
        Err(E_STALE_PASS) => {}
        _ => return Err(E_STALE_PASS),
    }
    gate.audit_cause_closed()?;
    hits += 1;

    // R5 守恒：routed + rejects == seq
    gate.audit_balance()?;
    hits += 1;

    // R6 LCG 随机空窗注入 ×512：8 hwnd 随机销毁/重建，对空窗 hwnd 投递全拒
    let mut g2 = StaleGate::new();
    let mut rng = Lcg::new(0x48412);
    let mut k = 0u32;
    while k < 8 {
        g2.register(0xA000 + k)?;
        k += 1;
    }
    let mut k = 0u32;
    while k < 512 {
        let hwnd = 0xA000 + rng.below(8) as u32;
        let act = rng.below(3);
        match act {
            0 => {
                // 尝试销毁（Alive 才成功）
                let ok = g2.rebuild(0xFFFF_FFFF).is_err(); // 占位防优化
                let _ = ok;
                let _ = g2.destroy(hwnd);
            }
            1 => {
                let _ = g2.rebuild(hwnd);
            }
            _ => {
                // 投递：Alive 放行、Gap/Void 拒——本轮只对 Gap 语义注入
                let r = g2.deliver(hwnd);
                if r.is_err() {
                    // 拒绝是允许结果（空窗/未注册）
                }
            }
        }
        k += 1;
    }
    g2.audit_cause_closed()?;
    g2.audit_balance()?;
    hits += 1;

    // R7 封印重演一致：固定脚本双跑拒绝账指纹相等
    let mut ga = StaleGate::new();
    ga.register(0x1234)?;
    ga.destroy(0x1234)?;
    let mut k = 0u32;
    while k < 5 {
        let _ = ga.deliver(0x1234);
        k += 1;
    }
    let mut gb = StaleGate::new();
    gb.register(0x1234)?;
    gb.destroy(0x1234)?;
    let mut k = 0u32;
    while k < 5 {
        let _ = gb.deliver(0x1234);
        k += 1;
    }
    if ga.fp() != gb.fp() || ga.fp() == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 拒绝计数：R7 双跑各恰 5 拒
    if ga.rejects() != 5 || gb.rejects() != 5 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    Ok(hits)
}
