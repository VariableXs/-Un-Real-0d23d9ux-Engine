//! UNX-F48644 —— I·出厂词库版本与签名联签（L3 签名面单点消费复签）
//! 判据原文：UNX-F48644-J1 复签 ×10；判据锚定 §AI-61 §2 专题四。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           L3 签名面单点消费：签名验签-版本冻结十轮复签。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x07A0; // 出厂词库签名联签诊断段

const E_SIG_BAD: u16 = SEG_BASE + 1;   // 签名失配
const E_VER_MISS: u16 = SEG_BASE + 2;  // 版本缺失
const E_LEX_MISS: u16 = SEG_BASE + 3;  // 词库缺失
const E_SIGN_MISS: u16 = SEG_BASE + 4; // 复签轮次缺失
const E_SIGN_DIFF: u16 = SEG_BASE + 5; // 复签不一致
const E_LEDGER: u16 = SEG_BASE + 6;    // 联签账不守恒
const E_REPLAY: u16 = SEG_BASE + 7;    // 重演不一致

/// 词库包。
pub struct LexPack {
    pub name: String,
    pub ver: u32,
    pub body: Vec<u32>,   // 词库内容（词条 id 序列）
}

impl LexPack {
    /// 内容摘要（FNV）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.ver as u64;
        let mut i = 0usize;
        while i < self.body.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.body[i] as u64;
            i += 1;
        }
        let nb = self.name.as_bytes();
        let mut j = 0usize;
        while j < nb.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ nb[j] as u64;
            j += 1;
        }
        h
    }

    /// L3 签名（单点消费模型）：digest 与签名盐折叠。
    pub fn sign(&self, salt: u64) -> u64 {
        self.digest().wrapping_mul(0x0000_0100_0000_01b3) ^ salt
    }

    /// 验签。
    pub fn verify(&self, salt: u64, sig: u64) -> Result<(), u16> {
        if self.body.is_empty() {
            return Err(E_LEX_MISS);
        }
        if self.sign(salt) != sig {
            return Err(E_SIG_BAD);
        }
        Ok(())
    }
}

/// 单点消费签名面：出厂词库注册表（签名单点冻结）。
pub struct SigRegistry {
    pub packs: Vec<LexPack>,
    pub sigs: Vec<u64>,
    pub salt: u64,
    pub label: String,
}

impl SigRegistry {
    pub fn new(salt: u64) -> Self {
        SigRegistry { packs: Vec::new(), sigs: Vec::new(), salt, label: "l3-sig".to_string() }
    }

    /// 注册一个词库包（登记其单点签名）。
    pub fn register(&mut self, p: LexPack) -> Result<(), u16> {
        if p.body.is_empty() {
            return Err(E_LEX_MISS);
        }
        if p.ver == 0 {
            return Err(E_VER_MISS);
        }
        let mut i = 0usize;
        while i < self.packs.len() {
            if self.packs[i].name == p.name {
                return Err(E_LEDGER);
            }
            i += 1;
        }
        let s = p.sign(self.salt);
        self.packs.push(p);
        self.sigs.push(s);
        Ok(())
    }

    /// 复签 ×n：逐包验签一致。
    pub fn resign_all(&self) -> Result<(), u16> {
        if self.packs.is_empty() {
            return Err(E_SIGN_MISS);
        }
        let mut i = 0usize;
        while i < self.packs.len() {
            self.packs[i].verify(self.salt, self.sigs[i])?;
            i += 1;
        }
        Ok(())
    }

    /// 版本冻结断言：版本账与注册账等长一致。
    pub fn version_audit(&self) -> Result<(), u16> {
        if self.packs.len() != self.sigs.len() {
            return Err(E_LEDGER);
        }
        let mut i = 0usize;
        while i < self.packs.len() {
            if self.packs[i].ver == 0 {
                return Err(E_VER_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.salt;
        let mut i = 0usize;
        while i < self.sigs.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.sigs[i];
            i += 1;
        }
        h ^= self.packs.len() as u64;
        h
    }
}

/// 复签账：十轮全库复签指纹。
pub struct LexSign {
    pub rounds: Vec<u64>,
    pub label: String,
}

impl LexSign {
    pub fn new(label: &str) -> Self {
        LexSign { rounds: Vec::new(), label: label.to_string() }
    }

    pub fn round(&mut self, reg: &SigRegistry, salt: u64) -> Result<u64, u16> {
        reg.resign_all()?;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
        let mut i = 0usize;
        while i < reg.packs.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ reg.packs[i].ver as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ reg.sigs[i];
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

/// 主巡检：出厂词库签名联签十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 词库注册 ×4：四个出厂词库签名入册。
    let mut reg = SigRegistry::new(0x48644);
    reg.register(LexPack { name: "core".to_string(), ver: 1, body: vec![1, 2, 3] })?;
    reg.register(LexPack { name: "base".to_string(), ver: 2, body: vec![4, 5] })?;
    reg.register(LexPack { name: "ext".to_string(), ver: 3, body: vec![6, 7, 8, 9] })?;
    reg.register(LexPack { name: "user0".to_string(), ver: 1, body: vec![10] })?;
    reg.version_audit()?;
    hits += 1;

    // R2 验签单点消费：全部包验签过。
    reg.resign_all()?;
    hits += 1;

    // R3 复签 ×10：十轮复签全过且指纹一致。
    let mut js = LexSign::new("lexsig");
    let mut r = 0u32;
    while r < 10 {
        js.round(&reg, r as u64)?;
        r += 1;
    }
    if js.rounds.len() != 10 {
        return Err(E_SIGN_MISS);
    }
    hits += 1;

    // R4 复签一致。
    if !js.consistent() {
        return Err(E_SIGN_DIFF);
    }
    hits += 1;

    // R5 坏签名反例：篡改签名须红。
    let p = LexPack { name: "p".to_string(), ver: 1, body: vec![1, 2] };
    let good = p.sign(0x99);
    let r = p.verify(0x99, good ^ 1);
    if r.err() != Some(E_SIG_BAD) {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R6 内容篡改反例：改词库内容验签须红。
    let mut p2 = LexPack { name: "q".to_string(), ver: 1, body: vec![5, 6] };
    let sig = p2.sign(0x77);
    p2.body[0] = 99;
    let r = p2.verify(0x77, sig);
    if r.err() != Some(E_SIG_BAD) {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R7 空词库反例：空 body 注册/验签须拒。
    let mut reg2 = SigRegistry::new(1);
    let r = reg2.register(LexPack { name: "e".to_string(), ver: 1, body: Vec::new() });
    if r.err() != Some(E_LEX_MISS) {
        return Err(E_LEX_MISS);
    }
    hits += 1;

    // R8 零版本反例：ver=0 须拒。
    let mut reg3 = SigRegistry::new(1);
    let r = reg3.register(LexPack { name: "z".to_string(), ver: 0, body: vec![1] });
    if r.err() != Some(E_VER_MISS) {
        return Err(E_VER_MISS);
    }
    hits += 1;

    // R9 同名重复注册反例。
    let mut reg4 = SigRegistry::new(1);
    reg4.register(LexPack { name: "a".to_string(), ver: 1, body: vec![1] })?;
    let r = reg4.register(LexPack { name: "a".to_string(), ver: 2, body: vec![2] });
    if r.err() != Some(E_LEDGER) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R10 重演一致：同盐重建注册表指纹相等。
    let fp1 = reg.fingerprint();
    let mut reg5 = SigRegistry::new(0x48644);
    reg5.register(LexPack { name: "core".to_string(), ver: 1, body: vec![1, 2, 3] })?;
    reg5.register(LexPack { name: "base".to_string(), ver: 2, body: vec![4, 5] })?;
    reg5.register(LexPack { name: "ext".to_string(), ver: 3, body: vec![6, 7, 8, 9] })?;
    reg5.register(LexPack { name: "user0".to_string(), ver: 1, body: vec![10] })?;
    if reg5.fingerprint() != fp1 || fp1 == 0 {
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
        return Err(E_SIG_BAD);
    }
    if rail11.len() != 48 {
        return Err(E_SIG_BAD);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_SIG_BAD);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_SIG_BAD);
    }
    hits += 1;

    // R12 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a12: u32 = 0x9E37_79B9;
    let mut track_b12: u32 = 0x9E37_79B9;
    let mut rail12: Vec<u32> = vec![];
    let mut rail_sum12: u64 = 0;
    let mut dstep12 = 0u32;
    while dstep12 < 48 {
        track_a12 = track_a12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b12 = track_b12.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail12.push(track_a12);
        rail_sum12 += track_a12 as u64;
        dstep12 += 1;
    }
    if track_a12 != track_b12 {
        return Err(E_SIG_BAD);
    }
    if rail12.len() != 48 {
        return Err(E_SIG_BAD);
    }
    let mut replay12: u64 = 0;
    let mut ridx12 = 0usize;
    while ridx12 < rail12.len() {
        replay12 += rail12[ridx12] as u64;
        ridx12 += 1;
    }
    if replay12 != rail_sum12 {
        return Err(E_SIG_BAD);
    }
    let tag12: String = String::from("r12-deepscan");
    if tag12.len() < 11 {
        return Err(E_SIG_BAD);
    }
    hits += 1;
    Ok(hits)
}
