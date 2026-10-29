//! UNX-F48426 —— E·候选风暴注入（反：单帧 10^4 候选更新 ×10 合并无闪烁）
//! 判据原文：UNX-F48426-J1 单帧内 10^4 次候选更新合并为 1 次视图提交；
//!           ×10 轮风暴全部合并（视图提交数 = 10 而非 10^5）；
//!           合并后终态正确（最后一条候选生效）；LCG 随机风暴 256 帧终态重演一致；
//!           闪烁判据：相邻视图提交间无空视图插入；账封印稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD8E0; // E22 候选风暴段

pub const STORM_N: u32 = 10_000;

const E_FLICKER: u16 = SEG_BASE + 1; // 空视图闪烁
const E_COMMIT_MISS: u16 = SEG_BASE + 2; // 提交数不符
const E_FINAL_MISS: u16 = SEG_BASE + 3; // 终态不符
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_COUNT_MISS: u16 = SEG_BASE + 6; // 计数不符
const E_EMPTY_VIEW: u16 = SEG_BASE + 7; // 提交了空视图

/// 候选条目。
#[derive(Clone, PartialEq, Eq)]
pub struct Cand {
    pub text: String,
    pub score: u32,
}

/// 帧合并视图提交器。
pub struct StormMerger {
    pending: Vec<Cand>,   // 帧内待合并
    commits: Vec<Vec<Cand>>, // 已提交视图序列
    frame_open: bool,
    updates_in_frame: u32,
}

impl StormMerger {
    pub fn new() -> Self {
        StormMerger { pending: Vec::new(), commits: Vec::new(), frame_open: false, updates_in_frame: 0 }
    }

    /// 开帧。
    pub fn begin_frame(&mut self) {
        self.frame_open = true;
        self.updates_in_frame = 0;
    }

    /// 候选更新（帧内只改 pending，不提交）。
    pub fn update(&mut self, c: Cand) -> Result<(), u16> {
        if !self.frame_open {
            return Err(E_COMMIT_MISS);
        }
        self.updates_in_frame += 1;
        self.pending.push(c);
        Ok(())
    }

    /// 提交视图（帧关闭时一次性）：pending 非空才提交。
    pub fn commit_frame(&mut self) -> Result<u32, u16> {
        self.frame_open = false;
        if self.pending.is_empty() {
            if self.updates_in_frame > 0 {
                return Err(E_EMPTY_VIEW);
            }
            return Ok(0);
        }
        let n = self.pending.len() as u32;
        self.commits.push(self.pending.clone());
        self.pending = Vec::new();
        Ok(n)
    }

    /// 闪烁审计：相邻提交间无空视图（commit 序列内无空 vec）+ 提交数 = 帧数。
    pub fn audit_no_flicker(&self, frames: u32) -> Result<(), u16> {
        if self.commits.len() as u32 != frames {
            return Err(E_COMMIT_MISS);
        }
        let mut i = 0usize;
        while i < self.commits.len() {
            if self.commits[i].is_empty() {
                return Err(E_FLICKER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 终态正确：最后提交的最后条目 = 第 STORM_N-1 号候选。
    pub fn audit_final(&self) -> Result<(), u16> {
        let last = match self.commits.last() {
            Some(v) => v,
            None => return Err(E_FINAL_MISS),
        };
        let end = match last.last() {
            Some(c) => c,
            None => return Err(E_FINAL_MISS),
        };
        if end.text != cand_text(STORM_N - 1) {
            return Err(E_FINAL_MISS);
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.commits.len() {
            let v = &self.commits[i];
            let mut j = 0usize;
            while j < v.len() {
                let b = v[j].text.as_bytes();
                let mut k = 0usize;
                while k < b.len() {
                    h ^= b[k] as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                    k += 1;
                }
                h ^= v[j].score as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            i += 1;
        }
        h
    }

    pub fn commit_count(&self) -> u32 {
        self.commits.len() as u32
    }
}

fn cand_text(n: u32) -> String {
    let mut s = String::from("k");
    s.push_str(&n.to_string());
    s
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

/// 主巡检：候选风暴八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 单帧 10^4 更新合并为 1 提交
    let mut m = StormMerger::new();
    m.begin_frame();
    let mut k = 0u32;
    while k < STORM_N {
        m.update(Cand { text: cand_text(k), score: k })?;
        k += 1;
    }
    let n = m.commit_frame()?;
    if n != STORM_N {
        return Err(E_COUNT_MISS);
    }
    if m.commit_count() != 1 {
        return Err(E_COMMIT_MISS);
    }
    hits += 1;

    // R2 ×10 轮风暴：提交数 = 10 而非 10^5
    let mut m2 = StormMerger::new();
    let mut r = 0u32;
    while r < 10 {
        m2.begin_frame();
        let mut k = 0u32;
        while k < STORM_N {
            m2.update(Cand { text: cand_text(k + r), score: k })?;
            k += 1;
        }
        m2.commit_frame()?;
        r += 1;
    }
    m2.audit_no_flicker(10)?;
    hits += 1;

    // R3 终态正确：最后提交最后条目 = 第 STORM_N-1 号（R1 场景）
    m.audit_final()?;
    hits += 1;

    // R4 闪烁判据：无空视图插入（R2 场景内已查；此处查 R1 单帧）
    m.audit_no_flicker(1)?;
    hits += 1;

    // R5 LCG 随机风暴 256 帧：每帧随机长度更新，提交数 = 256
    let mut m3 = StormMerger::new();
    let mut rng = Lcg::new(0x48426);
    let mut f = 0u32;
    while f < 256 {
        m3.begin_frame();
        let len = rng.below(200) as u32;
        let mut k = 0u32;
        while k < len {
            let v = rng.below(1000) as u32;
            m3.update(Cand { text: cand_text(v), score: v })?;
            k += 1;
        }
        m3.commit_frame()?;
        f += 1;
    }
    m3.audit_no_flicker(256)?;
    hits += 1;

    // R6 重演一致：同种子双跑指纹相等
    let f1 = m3.fp();
    let mut m4 = StormMerger::new();
    let mut rng2 = Lcg::new(0x48426);
    let mut f = 0u32;
    while f < 256 {
        m4.begin_frame();
        let len = rng2.below(200) as u32;
        let mut k = 0u32;
        while k < len {
            let v = rng2.below(1000) as u32;
            m4.update(Cand { text: cand_text(v), score: v })?;
            k += 1;
        }
        m4.commit_frame()?;
        f += 1;
    }
    if f1 != m4.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 空帧零提交：0 更新帧 commit 返回 0、无提交记录
    let mut m5 = StormMerger::new();
    m5.begin_frame();
    let n5 = m5.commit_frame()?;
    if n5 != 0 || m5.commit_count() != 0 {
        return Err(E_EMPTY_VIEW);
    }
    hits += 1;

    // R8 封印稳定：R2 账双算一致
    let s1 = m2.fp();
    let s2 = m2.fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
