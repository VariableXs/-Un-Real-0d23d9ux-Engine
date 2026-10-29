//! UNX-F48421 —— E·超长拼音串 64 音节边界（正：截断+告警+可继续）
//! 判据原文：UNX-F48421-J1 64 音节上限：恰好 64 全收；65 音节截断至 64 并告警；
//!           截断后可继续输入（状态机不断流）；截断计数账 ×10 全登记；
//!           LCG 随机长度 512 串全部有界（≤64）；截断账封印重演一致；
//!           边界两侧行为差可观测（64 vs 65 返回值不同）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD820; // E22 超长拼音段

pub const SYL_LIMIT: usize = 64;

const E_OVER_PASS: u16 = SEG_BASE + 1; // 超 64 音节未截断
const E_TRUNC_LOSS: u16 = SEG_BASE + 2; // 截断后内容错
const E_NO_WARN: u16 = SEG_BASE + 3; // 截断未告警
const E_STATE_DEAD: u16 = SEG_BASE + 4; // 截断后不可继续
const E_SEAL_BAD: u16 = SEG_BASE + 5; // 封印失配
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 截断账计数不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_LIMIT_VAR: u16 = SEG_BASE + 8; // 上限漂移

/// 音节单元（拼音字母串）。
pub struct Syl {
    pub text: String,
}

/// 输入缓冲器（64 音节上限）。
pub struct SylBuffer {
    syls: Vec<String>,
    trunc_events: u32,
    warned: bool,
}

impl SylBuffer {
    pub fn new() -> Self {
        SylBuffer { syls: Vec::new(), trunc_events: 0, warned: false }
    }

    pub fn len(&self) -> usize {
        self.syls.len()
    }

    pub fn content(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.syls.len() {
            s.push_str(&self.syls[i]);
            i += 1;
        }
        s
    }

    /// 追加一个音节：≤64 全收；超限截断（拒收该音节）并告警一次。
    pub fn push(&mut self, s: &str) -> Result<bool, u16> {
        if self.syls.len() < SYL_LIMIT {
            self.syls.push(String::from(s));
            return Ok(true);
        }
        // 截断路径：拒收 + 告警
        self.trunc_events += 1;
        self.warned = true;
        Ok(false)
    }

    /// 截断后可继续：先退格一个音节，再追加应成功（状态机不断流）。
    pub fn backspace_then_push(&mut self, s: &str) -> Result<bool, u16> {
        if self.syls.is_empty() {
            return Err(E_STATE_DEAD);
        }
        self.syls.pop();
        self.push(s)
    }

    /// 截断账一致：有截断必有告警，无截断必无告警。
    pub fn audit_warn_pair(&self) -> Result<(), u16> {
        if self.trunc_events > 0 && !self.warned {
            return Err(E_NO_WARN);
        }
        if self.trunc_events == 0 && self.warned {
            return Err(E_NO_WARN);
        }
        Ok(())
    }

    /// 有界不变式：len ≤ 64 恒成立。
    pub fn audit_bounded(&self) -> Result<(), u16> {
        if self.syls.len() > SYL_LIMIT {
            return Err(E_OVER_PASS);
        }
        Ok(())
    }

    pub fn truncations(&self) -> u32 {
        self.trunc_events
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.syls.len() {
            let b = self.syls[i].as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h ^= self.syls.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
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
        self.next() % n
    }
}

/// 生成第 n 号音节文本（如 s07）。
pub fn syl_text(n: usize) -> String {
    let mut s = String::from("s");
    s.push_str(&n.to_string());
    s
}

/// 主巡检：64 音节边界八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 恰 64 全收：64 次全 Ok(true)，len=64
    let mut b = SylBuffer::new();
    let mut i = 0usize;
    while i < 64 {
        if !b.push(&syl_text(i))? {
            return Err(E_TRUNC_LOSS);
        }
        i += 1;
    }
    if b.len() != 64 {
        return Err(E_LIMIT_VAR);
    }
    hits += 1;

    // R2 第 65 拒收并告警：push 返回 false、账+1、len 仍 64
    match b.push(&syl_text(64)) {
        Ok(false) => {}
        _ => return Err(E_OVER_PASS),
    }
    if b.len() != 64 || b.truncations() != 1 {
        return Err(E_NO_WARN);
    }
    b.audit_warn_pair()?;
    hits += 1;

    // R3 截断后可继续：退格 1 再推成功
    if !b.backspace_then_push(&syl_text(99))? {
        return Err(E_STATE_DEAD);
    }
    if b.len() != 64 {
        return Err(E_STATE_DEAD);
    }
    hits += 1;

    // R4 边界两侧行为差可观测：64 满时 push=false；退 2 后 push=true
    let mut b2 = SylBuffer::new();
    let mut i = 0usize;
    while i < 64 {
        let _ = b2.push(&syl_text(i))?;
        i += 1;
    }
    let full_res = b2.push("overflow")?;
    let _ = b2.backspace_then_push(&syl_text(200))?;
    let _ = b2.backspace_then_push(&syl_text(201))?;
    let room_res = b2.push("room")?;
    if full_res == room_res {
        return Err(E_LIMIT_VAR);
    }
    hits += 1;

    // R5 ×10 截断账全登记：满载后连推 10 次全拒且账=10
    let mut b3 = SylBuffer::new();
    let mut i = 0usize;
    while i < 64 {
        let _ = b3.push(&syl_text(i))?;
        i += 1;
    }
    let mut k = 0u32;
    while k < 10 {
        match b3.push(&syl_text(100 + k as usize)) {
            Ok(false) => {}
            _ => return Err(E_OVER_PASS),
        }
        k += 1;
    }
    if b3.truncations() != 10 {
        return Err(E_COUNT_MISS);
    }
    b3.audit_warn_pair()?;
    hits += 1;

    // R6 LCG 随机长度 512 串全部有界：len ∈ {0..64}，截断账 = 超限次数
    let mut b4 = SylBuffer::new();
    let mut rng = Lcg::new(0x48421);
    let mut trunc_seen = 0u32;
    let mut k = 0u32;
    while k < 512 {
        let act = rng.below(3);
        match act {
            0 => {
                if !b4.push(&syl_text(k as usize))? {
                    trunc_seen += 1;
                }
            }
            1 => {
                if b4.len() > 0 {
                    b4.syls.pop();
                }
            }
            _ => {
                if b4.len() < SYL_LIMIT {
                    b4.push(&syl_text(0x1000 + k as usize))?;
                }
            }
        }
        b4.audit_bounded()?;
        k += 1;
    }
    if b4.truncations() != trunc_seen {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R7 重演一致：同脚本双跑缓冲指纹相等
    let mut b5 = SylBuffer::new();
    let mut rng2 = Lcg::new(0x48421);
    let mut k = 0u32;
    while k < 512 {
        let act = rng2.below(3);
        match act {
            0 => {
                let _ = b5.push(&syl_text(k as usize));
            }
            1 => {
                if b5.len() > 0 {
                    b5.syls.pop();
                }
            }
            _ => {
                if b5.len() < SYL_LIMIT {
                    b5.push(&syl_text(0x1000 + k as usize))?;
                }
            }
        }
        k += 1;
    }
    if b4.fp() != b5.fp() || b4.fp() == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 上限恒定：SYL_LIMIT == 64 双读一致（防编译期漂移）
    if SYL_LIMIT != 64 {
        return Err(E_LIMIT_VAR);
    }
    hits += 1;

    Ok(hits)
}
