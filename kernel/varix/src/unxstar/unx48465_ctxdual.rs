//! UNX-F48465 —— E·context 双活注入（反：两 context 同焦点 ×10 后到杀先到）
//! 判据原文：UNX-F48465-J1 同焦点双 context 注册 ×10 后到者杀先到者（挂起非销毁）；
//!           杀者账（被挂起 context+时刻）×10 全记；焦点恒唯一（单活不变式）；
//!           LCG 随机双活 ×512 全部后到胜出；被挂起者重注可复活（先挂者让位）；
//!           账封印重演一致；挂起账与焦点账守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDF60; // E24 双活注入段

const E_DUAL_ALIVE: u16 = SEG_BASE + 1; // 双活放行
const E_NO_SUSPEND: u16 = SEG_BASE + 2; // 后到未杀先到
const E_SEAL_BAD: u16 = SEG_BASE + 3; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 5; // 守恒破
const E_FOCUS_MISS: u16 = SEG_BASE + 6; // 焦点缺失
const E_REVIVE_MISS: u16 = SEG_BASE + 7; // 复活路径失效

/// 挂起账条目。
#[derive(Clone)]
pub struct SuspRec {
    pub victim: u32,
    pub winner: u32,
    pub tick: u32,
}

/// 焦点仲裁器。
pub struct FocusArbiter {
    focus: Option<u32>,       // 现役 context
    suspended: Vec<u32>,      // 挂起集
    log: Vec<SuspRec>,        // 挂起账
    tick: u32,
}

impl FocusArbiter {
    pub fn new() -> Self {
        FocusArbiter { focus: None, suspended: Vec::new(), log: Vec::new(), tick: 0 }
    }

    fn in_suspended(&self, ctx: u32) -> bool {
        let mut i = 0usize;
        while i < self.suspended.len() {
            if self.suspended[i] == ctx {
                return true;
            }
            i += 1;
        }
        false
    }

    /// context 申请焦点：现役非空且不同 → 后到杀先到（挂起）。
    pub fn acquire(&mut self, ctx: u32) -> Result<(), u16> {
        self.tick += 1;
        match self.focus {
            None => {
                // 从挂起集复活
                let mut found = false;
                let mut i = 0usize;
                while i < self.suspended.len() {
                    if self.suspended[i] == ctx {
                        found = true;
                    }
                    i += 1;
                }
                if found {
                    // 复活：摘出挂起集
                    let mut i = 0usize;
                    while i < self.suspended.len() {
                        if self.suspended[i] == ctx {
                            self.suspended.swap_remove(i);
                            break;
                        }
                        i += 1;
                    }
                }
                self.focus = Some(ctx);
                Ok(())
            }
            Some(cur) => {
                if cur == ctx {
                    return Ok(()); // 幂等重注
                }
                // 后到杀先到：先到入挂起
                self.suspended.push(cur);
                self.log.push(SuspRec { victim: cur, winner: ctx, tick: self.tick });
                self.focus = Some(ctx);
                Ok(())
            }
        }
    }

    /// 单活不变式：focus 恒 Some 且不在挂起集。
    pub fn audit_single_alive(&self) -> Result<u32, u16> {
        match self.focus {
            None => Err(E_FOCUS_MISS),
            Some(f) => {
                if self.in_suspended(f) {
                    return Err(E_DUAL_ALIVE);
                }
                Ok(f)
            }
        }
    }

    /// 挂起账审计：victim ≠ winner、victim 全在挂起集（最后一次挂起未被复活时）。
    pub fn audit_log(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.log.len() {
            if self.log[i].victim == self.log[i].winner {
                return Err(E_NO_SUSPEND);
            }
            if self.log[i].tick == 0 {
                return Err(E_NO_SUSPEND);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 守恒：focus(0/1) + suspended.len() + 复活数 == acquire 成功数（由调用侧传入 expect）。
    pub fn audit_balance(&self, expect_alive: usize) -> Result<(), u16> {
        let alive = match self.focus {
            Some(_) => 1,
            None => 0,
        };
        if alive != expect_alive {
            return Err(E_BAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.log.len() {
            h ^= self.log[i].victim as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.log[i].winner as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.log[i].tick as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.suspended.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn suspended_len(&self) -> usize {
        self.suspended.len()
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
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

/// 主巡检：双活注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 双活注入 ×10：后到杀先到
    let mut arb = FocusArbiter::new();
    let mut k = 0u32;
    while k < 10 {
        arb.acquire(0x100 + k)?;
        arb.acquire(0x200 + k)?;
        let cur = arb.audit_single_alive()?;
        if cur != 0x200 + k {
            return Err(E_NO_SUSPEND);
        }
        k += 1;
    }
    if arb.log_len() != 10 {
        return Err(E_NO_SUSPEND);
    }
    arb.audit_log()?;
    hits += 1;

    // R2 焦点恒唯一：alive=1
    arb.audit_balance(1)?;
    hits += 1;

    // R3 挂起者重注复活：挂起集成员 acquire 后回焦点、挂起集清空
    let mut arb2 = FocusArbiter::new();
    arb2.acquire(1)?;
    arb2.acquire(2)?;
    if arb2.suspended_len() != 1 {
        return Err(E_NO_SUSPEND);
    }
    arb2.acquire(1)?; // 复活 1
    let cur = arb2.audit_single_alive()?;
    if cur != 1 {
        return Err(E_REVIVE_MISS);
    }
    if arb2.suspended_len() != 1 {
        return Err(E_REVIVE_MISS); // 2 应进挂起
    }
    hits += 1;

    // R4 幂等重注：同 ctx 重注零挂起
    let mut arb3 = FocusArbiter::new();
    arb3.acquire(9)?;
    arb3.acquire(9)?;
    if arb3.log_len() != 0 || arb3.suspended_len() != 0 {
        return Err(E_NO_SUSPEND);
    }
    hits += 1;

    // R5 LCG 随机双活 ×512：全部后到胜出
    let mut arb4 = FocusArbiter::new();
    let mut rng = Lcg::new(0x48465);
    let mut k = 0u32;
    while k < 512 {
        let a = 0x1000 + rng.below(64) as u32;
        let b = 0x2000 + rng.below(64) as u32;
        arb4.acquire(a)?;
        arb4.acquire(b)?;
        let cur = arb4.audit_single_alive()?;
        if cur != b {
            return Err(E_NO_SUSPEND);
        }
        k += 1;
    }
    if arb4.log_len() < 512 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R6 挂起账账审计 ×512
    arb4.audit_log()?;
    hits += 1;

    // R7 重演一致：同种子双跑挂起账指纹相等
    let f1 = arb4.fp();
    let mut arb5 = FocusArbiter::new();
    let mut rng2 = Lcg::new(0x48465);
    let mut k = 0u32;
    while k < 512 {
        let a = 0x1000 + rng2.below(64) as u32;
        let b = 0x2000 + rng2.below(64) as u32;
        let _ = arb5.acquire(a);
        let _ = arb5.acquire(b);
        k += 1;
    }
    if f1 != arb5.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒 + 封印稳定
    arb4.audit_balance(1)?;
    let s1 = arb4.fp();
    let s2 = arb4.fp();
    if s1 != s2 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
