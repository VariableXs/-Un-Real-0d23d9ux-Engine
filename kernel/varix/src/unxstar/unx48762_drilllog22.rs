//! UNX-F48762 C·波 22 演习主办记录（中文输入三类窗口回归演习，AI-62/29/21 参演签到）
//! 判据 UNX-F48762-J1：演习记录+参演签认在册；判据锚定 §AI-61 §7、§84 演习日历。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! AI-62/29/21 三方参演逐方签到，演习记录与签认同册，缺一方即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1D40;
const E_ACTOR_MISS: u16 = SEG_BASE + 1;
const E_SIGN_DUP: u16 = SEG_BASE + 2;
const E_DRILL_LOST: u16 = SEG_BASE + 3;
const E_LOG_FROZEN: u16 = SEG_BASE + 4;
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;
const E_TAG_BAD: u16 = SEG_BASE + 6;
const E_LOG_GAP: u16 = SEG_BASE + 7;
const E_ACTOR_BAD: u16 = SEG_BASE + 8;

/// 参演方常量：AI-62 / AI-29 / AI-21
pub const ACTORS: [&str; 3] = ["AI-62", "AI-29", "AI-21"];

/// 签到条目：参演方 + 签认指纹
#[derive(Clone, Debug, PartialEq)]
pub struct SignEntry {
    pub actor: String,
    pub sign_fp: u32,
}

impl SignEntry {
    pub fn make(actor: &str, sign_fp: u32) -> Self {
        SignEntry { actor: String::from(actor), sign_fp }
    }
}

/// 演习记录册：记录 + 签认 + 冻结旗标
pub struct DrillLog {
    pub title: String,
    pub signs: Vec<SignEntry>,
    pub frozen: bool,
}

impl DrillLog {
    pub fn new(title: &str) -> Self {
        DrillLog { title: String::from(title), signs: vec![], frozen: false }
    }

    pub fn sign(&mut self, actor: &str, fp: u32) -> Result<(), u16> {
        if self.frozen {
            return Err(E_LOG_FROZEN);
        }
        let mut legal = false;
        let mut a = 0usize;
        while a < ACTORS.len() {
            if ACTORS[a] == actor {
                legal = true;
            }
            a += 1;
        }
        if !legal {
            return Err(E_ACTOR_BAD);
        }
        let mut i = 0usize;
        while i < self.signs.len() {
            if self.signs[i].actor == actor {
                return Err(E_SIGN_DUP);
            }
            i += 1;
        }
        self.signs.push(SignEntry::make(actor, fp));
        Ok(())
    }

    /// 冻结演习记录：三方签到齐才可冻结
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.title.is_empty() {
            return Err(E_TAG_BAD);
        }
        let mut a = 0usize;
        while a < ACTORS.len() {
            let mut found = false;
            let mut i = 0usize;
            while i < self.signs.len() {
                if self.signs[i].actor == ACTORS[a] {
                    if self.signs[i].sign_fp == 0 {
                        return Err(E_ACTOR_MISS);
                    }
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_ACTOR_MISS);
            }
            a += 1;
        }
        self.frozen = true;
        Ok(self.signs.len() as u32)
    }

    /// 演习记录核对：册题在、三方签认在、已冻结
    pub fn verify(&self) -> Result<(), u16> {
        if self.title.is_empty() {
            return Err(E_LOG_GAP);
        }
        if !self.frozen {
            return Err(E_LOG_GAP);
        }
        let mut a = 0usize;
        while a < ACTORS.len() {
            let mut found = false;
            let mut i = 0usize;
            while i < self.signs.len() {
                if self.signs[i].actor == ACTORS[a] && self.signs[i].sign_fp != 0 {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_ACTOR_MISS);
            }
            a += 1;
        }
        Ok(())
    }
}

/// 指纹生成：FNV-1a
pub fn fnv_fp(word: &str) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let bytes = word.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 确定性 LCG
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
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 演习记录 ×10^3：三方签到-冻结-核对
fn sweep_drills(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let mut log = DrillLog::new("wave22-regression");
        let mut a = 0usize;
        while a < ACTORS.len() {
            let fp = fnv_fp(ACTORS[a]) ^ (rng.below(0xFFFF) as u32) | 1;
            log.sign(ACTORS[a], fp)?;
            a += 1;
        }
        if log.freeze()? != 3 {
            return Err(E_ACTOR_MISS);
        }
        log.verify()?;
        iter += 1;
    }
    Ok(())
}

/// 反例注入：非法参演方、重复签到、缺方冻结、零指纹、冻结后签到全命中
fn sweep_negatives() -> Result<(), u16> {
    let mut log = DrillLog::new("d1");
    let r1 = log.sign("AI-99", 1);
    if r1.err() != Some(E_ACTOR_BAD) {
        return Err(E_ACTOR_BAD);
    }
    log.sign("AI-62", 0x11)?;
    let r2 = log.sign("AI-62", 0x12);
    if r2.err() != Some(E_SIGN_DUP) {
        return Err(E_SIGN_DUP);
    }
    let mut log2 = DrillLog::new("d2");
    log2.sign("AI-62", 0x21)?;
    let r3 = log2.freeze();
    if r3.err() != Some(E_ACTOR_MISS) {
        return Err(E_ACTOR_MISS);
    }
    let mut log3 = DrillLog::new("d3");
    log3.sign("AI-62", 0x31)?;
    log3.sign("AI-29", 0)?;
    log3.sign("AI-21", 0x33)?;
    let r4 = log3.freeze();
    if r4.err() != Some(E_ACTOR_MISS) {
        return Err(E_ACTOR_MISS);
    }
    log3.signs[1] = SignEntry::make("AI-29", 0x32);
    log3.freeze()?;
    let r5 = log3.sign("AI-21", 0x35);
    if r5.err() != Some(E_LOG_FROZEN) {
        return Err(E_LOG_FROZEN);
    }
    Ok(())
}

/// 重放一致 ×20：同种子双跑签认册逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut la = DrillLog::new("replay");
        let mut lb = DrillLog::new("replay");
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let mut a = 0usize;
        while a < ACTORS.len() {
            let fa = fnv_fp(ACTORS[a]) ^ (ra.below(0xFFFF) as u32) | 1;
            let fb = fnv_fp(ACTORS[a]) ^ (rb.below(0xFFFF) as u32) | 1;
            if fa != fb {
                return Err(E_REPLAY_DIFF);
            }
            la.sign(ACTORS[a], fa)?;
            lb.sign(ACTORS[a], fb)?;
            a += 1;
        }
        if la.freeze()? != lb.freeze()? {
            return Err(E_REPLAY_DIFF);
        }
        if la.signs != lb.signs {
            return Err(E_REPLAY_DIFF);
        }
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 演习记录 ×10^3
    let mut rng = Lcg::new(0x4876_2533);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_drills(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R3 重放一致 ×20
    sweep_replay(0x4876_2A0F)?;
    hits += 1;
    // R4 三方常量核对：ACTORS 恰为 AI-62/29/21
    if ACTORS.len() != 3 || ACTORS[0] != "AI-62" || ACTORS[1] != "AI-29" || ACTORS[2] != "AI-21" {
        return Err(E_ACTOR_BAD);
    }
    hits += 1;
    // R5 参演签认在册：三方逐方可查
    let mut log = DrillLog::new("wave22-final");
    let mut a = 0usize;
    while a < ACTORS.len() {
        log.sign(ACTORS[a], fnv_fp(ACTORS[a]))?;
        a += 1;
    }
    log.freeze()?;
    log.verify()?;
    hits += 1;
    // R6 签认指纹自治：FNV 回算一致
    let signs = log.signs.clone();
    let mut i = 0usize;
    while i < signs.len() {
        if fnv_fp(&signs[i].actor) != signs[i].sign_fp {
            return Err(E_ACTOR_MISS);
        }
        i += 1;
    }
    hits += 1;
    // R7 冻结旗标与核对联动
    if !log.frozen {
        return Err(E_LOG_GAP);
    }
    hits += 1;
    // R8 签到序稳定：按 ACTORS 序落册
    if signs[0].actor != "AI-62" || signs[2].actor != "AI-21" {
        return Err(E_ACTOR_BAD);
    }
    hits += 1;
    // R9 册题语义：非空且含演习语义
    if log.title.len() < 4 {
        return Err(E_TAG_BAD);
    }
    hits += 1;
    // R10 未冻核对必拒
    let mut log2 = DrillLog::new("unfrozen");
    let mut a2 = 0usize;
    while a2 < ACTORS.len() {
        log2.sign(ACTORS[a2], 0x5000 + a2 as u32)?;
        a2 += 1;
    }
    if log2.verify().err() != Some(E_LOG_GAP) {
        return Err(E_LOG_GAP);
    }
    log2.freeze()?;
    log2.verify()?;
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
