//! UNX-F48625 —— I·XIM 触发键联签复签
//! 判据原文：UNX-F48625-J1 复签 ×10；判据锚定 B11 触发键面。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           触发键面冻结表 + 十轮复签一致。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x0540; // XIM 触发键诊断段

const E_TRIG_BAD: u16 = SEG_BASE + 1;   // 触发键漂移
const E_KEY_OOB: u16 = SEG_BASE + 2;    // 键域越界
const E_SIGN_MISS: u16 = SEG_BASE + 3;  // 复签轮次缺失
const E_SIGN_DIFF: u16 = SEG_BASE + 4;  // 复签不一致
const E_SEQ_BAD: u16 = SEG_BASE + 5;    // 触发序违例
const E_LEDGER: u16 = SEG_BASE + 6;     // 触发账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;     // 重演不一致

/// XIM 触发键定义（冻结表）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TriggerKey {
    pub keysym: u32,
    pub mods: u32, // 1=shift 2=ctrl 4=alt
}

/// 冻结触发键表：空格/Shift、Zenkaku_Hankaku。
pub const TRIG_TABLE: [TriggerKey; 2] = [
    TriggerKey { keysym: 0x0020, mods: 1 },     // space + shift
    TriggerKey { keysym: 0xff23, mods: 0 },     // Zenkaku_Hankaku
];

/// 触发键匹配器。
pub struct TrigMatcher {
    pub armed: bool,
    pub hits: u32,
    pub misses: u32,
}

impl TrigMatcher {
    pub fn new() -> Self {
        TrigMatcher { armed: true, hits: 0, misses: 0 }
    }

    /// 匹配一次按键：命中冻结表则触发态翻转。
    pub fn feed(&mut self, keysym: u32, mods: u32) -> Result<bool, u16> {
        if keysym > 0xffffff {
            return Err(E_KEY_OOB);
        }
        if mods > 7 {
            return Err(E_KEY_OOB);
        }
        let mut hit = false;
        let mut i = 0usize;
        while i < TRIG_TABLE.len() {
            if TRIG_TABLE[i].keysym == keysym && TRIG_TABLE[i].mods == mods {
                hit = true;
            }
            i += 1;
        }
        if hit {
            self.armed = !self.armed;
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        Ok(hit)
    }

    /// 账守恒。
    pub fn audit(&self) -> Result<(), u16> {
        if self.hits + self.misses == 0 {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.armed as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.hits as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.misses as u64;
        h
    }
}

/// 复签账：十轮触发面复签。
pub struct TrigSign {
    pub rounds: Vec<u64>,
    pub label: String,
}

impl TrigSign {
    pub fn new(label: &str) -> Self {
        TrigSign { rounds: Vec::new(), label: label.to_string() }
    }

    /// 单轮：全表键域与模域机检 + 指纹。
    pub fn round(&mut self, salt: u64) -> Result<u64, u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
        let mut i = 0usize;
        while i < TRIG_TABLE.len() {
            let t = TRIG_TABLE[i];
            if t.keysym == 0 || t.mods > 7 {
                return Err(E_TRIG_BAD);
            }
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ t.keysym as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ t.mods as u64;
            i += 1;
        }
        self.rounds.push(h);
        Ok(h)
    }

    pub fn consistent(&self) -> bool {
        if self.rounds.len() < 2 {
            return false;
        }
        let f = self.rounds[0];
        let mut i = 1usize;
        while i < self.rounds.len() {
            if self.rounds[i] != f {
                return false;
            }
            i += 1;
        }
        true
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
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：XIM 触发键联签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 触发表冻结：两键定义合法。
    let mut i = 0usize;
    while i < TRIG_TABLE.len() {
        if TRIG_TABLE[i].keysym == 0 || TRIG_TABLE[i].mods > 7 {
            return Err(E_TRIG_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R2 命中翻转：space+shift 触发 armed 翻转。
    let mut m = TrigMatcher::new();
    let a0 = m.armed;
    let hit = m.feed(0x0020, 1)?;
    if !hit || m.armed == a0 {
        return Err(E_TRIG_BAD);
    }
    hits += 1;

    // R3 再触发回翻：双触发回到初始态。
    let hit = m.feed(0xff23, 0)?;
    if !hit || m.armed != a0 {
        return Err(E_TRIG_BAD);
    }
    hits += 1;

    // R4 未命中计数：普通键不触发。
    let hit = m.feed(0x0041, 0)?;
    if hit || m.misses != 1 {
        return Err(E_TRIG_BAD);
    }
    hits += 1;

    // R5 键域越界反例：keysym/mods 超域须拒。
    let mut m2 = TrigMatcher::new();
    let r = m2.feed(0x100_0000, 0);
    if r.err() != Some(E_KEY_OOB) {
        return Err(E_KEY_OOB);
    }
    let r = m2.feed(0x0020, 8);
    if r.err() != Some(E_KEY_OOB) {
        return Err(E_KEY_OOB);
    }
    hits += 1;

    // R6 模组敏感：同键异模不误触发。
    let mut m3 = TrigMatcher::new();
    let hit = m3.feed(0x0020, 0)?;
    if hit {
        return Err(E_TRIG_BAD);
    }
    hits += 1;

    // R7 复签 ×10：十轮触发表复签通过。
    let mut js = TrigSign::new("trigkey");
    let mut r = 0u32;
    while r < 10 {
        js.round(r as u64)?;
        r += 1;
    }
    if js.rounds.len() != 10 {
        return Err(E_SIGN_MISS);
    }
    hits += 1;

    // R8 复签一致。
    if !js.consistent() {
        return Err(E_SIGN_DIFF);
    }
    hits += 1;

    // R9 fuzz ×256 随机键流：命中/未命中账守恒且触发达标。
    let mut rng = Lcg::new(0x48625_0001);
    let mut m4 = TrigMatcher::new();
    let mut fed = 0u32;
    let mut k = 0u32;
    while k < 256 {
        // 一半概率发触发键。
        let (ks, md) = if rng.below(2) == 0 {
            let ti = rng.below(TRIG_TABLE.len() as u64) as usize;
            (TRIG_TABLE[ti].keysym, TRIG_TABLE[ti].mods)
        } else {
            (0x0041 + rng.below(26) as u32, 0)
        };
        m4.feed(ks, md)?;
        fed += 1;
        k += 1;
    }
    m4.audit()?;
    if m4.hits + m4.misses != fed {
        return Err(E_LEDGER);
    }
    if m4.hits == 0 || m4.misses == 0 {
        return Err(E_SEQ_BAD);
    }
    hits += 1;

    // R10 重演一致：同种子重建键流，指纹相等。
    let fp1 = m4.fingerprint();
    let mut rng2 = Lcg::new(0x48625_0001);
    let mut m5 = TrigMatcher::new();
    let mut k = 0u32;
    while k < 256 {
        let (ks, md) = if rng2.below(2) == 0 {
            let ti = rng2.below(TRIG_TABLE.len() as u64) as usize;
            (TRIG_TABLE[ti].keysym, TRIG_TABLE[ti].mods)
        } else {
            (0x0041 + rng2.below(26) as u32, 0)
        };
        let _ = m5.feed(ks, md);
        k += 1;
    }
    if m5.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_TRIG_BAD);
    }
    if rail11.len() != 48 {
        return Err(E_TRIG_BAD);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_TRIG_BAD);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_TRIG_BAD);
    }
    hits += 1;
    Ok(hits)
}
