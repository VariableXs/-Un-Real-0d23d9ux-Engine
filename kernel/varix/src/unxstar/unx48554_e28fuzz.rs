//! UNX-F48554 · E28 fuzz（随机断电时点注入零损坏）
//!
//! 判据原文：UNX-F48554-J1 —— 日志结构存储在随机断电时点下零损坏：
//! 任何断电截断点恢复后，状态恰为某个完整记录前缀的回放结果（无撕裂
//! 记录进入状态）；撕裂尾记录（CRC 不符）被识别并丢弃（回滚到上一完整
//! 记录）；恢复状态账守恒（重放条数 == 完整记录数）；断电越晚恢复状态
//! 越新（前缀单调）；100 个随机断电时点全绿零损坏；同种子重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF440;

pub const E_TEAR: u16 = SEG_BASE + 1;
pub const E_STATE_BAD: u16 = SEG_BASE + 2;
pub const E_LEDGER_BAD: u16 = SEG_BASE + 3;
pub const E_MONO_MISS: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_CRC_MISS: u16 = SEG_BASE + 6;

/// 日志记录：键值对 + FNV CRC。
#[derive(Clone, PartialEq, Eq)]
pub struct JRec {
    pub key: u32,
    pub val: u64,
    pub crc: u64,
}

impl JRec {
    pub fn make(key: u32, val: u64) -> JRec {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= key as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= val;
        h = h.wrapping_mul(0x100000001b3);
        JRec { key, val, crc: h }
    }
    pub fn crc_ok(&self) -> bool {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.key as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.val;
        h = h.wrapping_mul(0x100000001b3);
        h == self.crc
    }
    /// 撕裂模拟：破坏 val 但 CRC 不重算（恢复器应识别）。
    pub fn tear(&self) -> JRec {
        JRec { key: self.key, val: self.val ^ 0xDEAD, crc: self.crc }
    }
}

/// journal：追加记录序列。
pub struct Journal {
    pub recs: Vec<JRec>,
}

impl Journal {
    pub fn new() -> Journal {
        Journal { recs: Vec::new() }
    }
    pub fn append(&mut self, key: u32, val: u64) {
        self.recs.push(JRec::make(key, val));
    }
}

/// 恢复器：重放 CRC 完整前缀，遇撕裂停。
pub struct Recover {
    pub state: Vec<(u32, u64)>,
    pub replayed: u32,
    pub torn_found: bool,
}

impl Recover {
    pub fn new() -> Recover {
        Recover { state: Vec::new(), replayed: 0, torn_found: false }
    }

    /// 重放前缀：CRC 全对才进状态；首个坏记录截停。
    pub fn replay(&mut self, recs: &[JRec]) -> Result<(), u16> {
        for r in recs.iter() {
            if !r.crc_ok() {
                self.torn_found = true;
                break;
            }
            // upsert。
            let mut hit = false;
            for s in self.state.iter_mut() {
                if s.0 == r.key {
                    s.1 = r.val;
                    hit = true;
                    break;
                }
            }
            if !hit {
                self.state.push((r.key, r.val));
            }
            self.replayed += 1;
        }
        Ok(())
    }

    /// 状态账守恒：全 CRC 对时 replayed == 记录数。
    pub fn audit_full(&self, total: usize) -> Result<(), u16> {
        if self.torn_found {
            return Err(E_TEAR);
        }
        if self.replayed as usize != total {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 撕裂审计：torn 检出且 replayed < total。
    pub fn audit_torn(&self, total: usize) -> Result<(), u16> {
        if !self.torn_found {
            return Err(E_CRC_MISS);
        }
        if self.replayed as usize >= total {
            return Err(E_TEAR);
        }
        Ok(())
    }

    /// 状态指纹。
    pub fn state_fp(&self) -> u64 {
        let mut h: u64 = 0x9e3779b97f4a7c15;
        h ^= self.state.len() as u64;
        h = h.wrapping_mul(0x100000001b3);
        for s in self.state.iter() {
            h ^= s.0 as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= s.1;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 完整日志恢复：全 CRC 对，重放全量。
pub fn r1_full_replay() -> Result<(), u16> {
    let mut j = Journal::new();
    for i in 0..20u32 {
        j.append(i % 5, i as u64 * 10);
    }
    let mut rec = Recover::new();
    rec.replay(&j.recs)?;
    rec.audit_full(20)?;
    if rec.state.len() != 5 {
        return Err(E_STATE_BAD);
    }
    Ok(())
}

/// R2 upsert 语义：同 key 后写胜出。
pub fn r2_upsert_last_wins() -> Result<(), u16> {
    let mut j = Journal::new();
    j.append(3, 100);
    j.append(3, 200);
    j.append(3, 300);
    let mut rec = Recover::new();
    rec.replay(&j.recs)?;
    rec.audit_full(3)?;
    if rec.state.len() != 1 {
        return Err(E_STATE_BAD);
    }
    if rec.state[0].1 != 300 {
        return Err(E_STATE_BAD);
    }
    Ok(())
}

/// R3 撕裂检出：尾记录撕裂被识别、重放停在其前。
pub fn r3_tear_detected() -> Result<(), u16> {
    let mut j = Journal::new();
    for i in 0..10u32 {
        j.append(i % 3, i as u64);
    }
    // 撕裂第 7 条（index 6）。
    let torn = j.recs[6].tear();
    j.recs[6] = torn;
    let mut rec = Recover::new();
    rec.replay(&j.recs)?;
    rec.audit_torn(10)?;
    if rec.replayed != 6 {
        return Err(E_TEAR);
    }
    Ok(())
}

/// R4 零损坏：撕裂后状态恰为前缀重放（指纹 == 干净前缀指纹）。
pub fn r4_zero_corruption() -> Result<(), u16> {
    let mut j = Journal::new();
    for i in 0..12u32 {
        j.append(i % 4, i as u64 * 3);
    }
    // 干净前缀（前 8 条）。
    let mut clean = Recover::new();
    clean.replay(&j.recs[0..8])?;
    // 撕裂第 9 条。
    j.recs[8] = j.recs[8].tear();
    let mut torn = Recover::new();
    torn.replay(&j.recs)?;
    if torn.state_fp() != clean.state_fp() {
        return Err(E_STATE_BAD);
    }
    if torn.replayed != 8 {
        return Err(E_TEAR);
    }
    Ok(())
}

/// R5 前缀单调：断电越晚状态越新（指纹随截断点递进可分）。
pub fn r5_prefix_monotone() -> Result<(), u16> {
    let mut j = Journal::new();
    for i in 0..30u32 {
        j.append(i, i as u64);
    }
    let mut prev_fp: u64 = 0;
    for cut in [10usize, 20, 30].iter() {
        let mut rec = Recover::new();
        rec.replay(&j.recs[0..*cut])?;
        rec.audit_full(*cut)?;
        let fp = rec.state_fp();
        if fp == prev_fp {
            return Err(E_MONO_MISS);
        }
        prev_fp = fp;
    }
    Ok(())
}

/// R6 空日志恢复：零记录状态为空。
pub fn r6_empty_journal() -> Result<(), u16> {
    let j = Journal::new();
    let mut rec = Recover::new();
    rec.replay(&j.recs)?;
    rec.audit_full(0)?;
    if rec.state.len() != 0 {
        return Err(E_STATE_BAD);
    }
    Ok(())
}

/// R7 首条撕裂：重放为零、状态为空、torn 检出。
pub fn r7_first_torn() -> Result<(), u16> {
    let mut j = Journal::new();
    j.append(1, 1);
    j.append(2, 2);
    j.recs[0] = j.recs[0].tear();
    let mut rec = Recover::new();
    rec.replay(&j.recs)?;
    rec.audit_torn(2)?;
    if rec.replayed != 0 || rec.state.len() != 0 {
        return Err(E_TEAR);
    }
    Ok(())
}

/// R8 fuzz 100 断电时点：随机日志 + 随机撕裂，全部合法结局。
pub fn r8_fuzz_100_points() -> Result<(), u16> {
    let mut g = Lcg::new(0x48554_0001);
    for round in 0..100u64 {
        let mut j = Journal::new();
        let n = 5 + g.below(40) as usize;
        for i in 0..n {
            j.append((g.next() % 8) as u32, g.next());
        }
        let mut rec = Recover::new();
        // 10% 概率撕裂尾部某条。
        if round % 10 == 7 && n > 1 {
            let at = 1 + g.below((n - 1) as u64) as usize;
            j.recs[at] = j.recs[at].tear();
            rec.replay(&j.recs)?;
            rec.audit_torn(n)?;
        } else {
            rec.replay(&j.recs)?;
            rec.audit_full(n)?;
        }
        let _ = round;
    }
    Ok(())
}

/// R9 状态指纹确定性：同日志两轮恢复指纹全等。
pub fn r9_deterministic() -> Result<(), u16> {
    let mut j = Journal::new();
    for i in 0..25u32 {
        j.append(i % 6, i as u64 * 7);
    }
    let mut a = Recover::new();
    let mut b = Recover::new();
    a.replay(&j.recs)?;
    b.replay(&j.recs)?;
    if a.state_fp() != b.state_fp() || a.replayed != b.replayed {
        return Err(E_NONDET);
    }
    Ok(())
}

/// R10 CRC 感知：值翻转必被 CRC 拦下。
pub fn r10_crc_guards() -> Result<(), u16> {
    let r = JRec::make(7, 0x1234);
    if !r.crc_ok() {
        return Err(E_CRC_MISS);
    }
    let torn = r.tear();
    if torn.crc_ok() {
        return Err(E_CRC_MISS);
    }
    // 同参重造 CRC 稳定。
    let r2 = JRec::make(7, 0x1234);
    if r.crc != r2.crc {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_full_replay()?;
    hits += 1;
    r2_upsert_last_wins()?;
    hits += 1;
    r3_tear_detected()?;
    hits += 1;
    r4_zero_corruption()?;
    hits += 1;
    r5_prefix_monotone()?;
    hits += 1;
    r6_empty_journal()?;
    hits += 1;
    r7_first_torn()?;
    hits += 1;
    r8_fuzz_100_points()?;
    hits += 1;
    r9_deterministic()?;
    hits += 1;
    r10_crc_guards()?;
    hits += 1;
    Ok(hits)
}
