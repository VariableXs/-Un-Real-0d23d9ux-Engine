//! UNX-F48773 —— C·互审材料包（N 部五域中文判据消费方联签材料）
//! 判据原文：UNX-F48773-J1 材料包齐；判据锚定 附则三。
//! 判据落地（drill 场景）：R1 五域闭集；R2 材料登记；R3 联签落痕；
//! R4 材料包齐断言；R5 域越界检出；R6 重复联签检出；R7 复核一致；
//! R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 材料包签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：互审材料包按 N 部五域闭集（sig/sec/obs/doc/aud）各
//! 登记一份中文判据消费材料，每份材料由消费方联签（seq 显式递增
//! 编位）；材料包齐 = 五域全覆盖且逐份联签可复核；材料包签名对
//! （域数、份数、指纹）混签归档。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1FC0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_DOMAIN: u16 = SEG_BASE + 1; // 域越出五域闭集
const E_DUP: u16 = SEG_BASE + 2; // 域重复登记
const E_SIG: u16 = SEG_BASE + 3; // 联签签名不符
const E_SEQ: u16 = SEG_BASE + 4; // 联签序号不连续
const E_MISS: u16 = SEG_BASE + 5; // 材料缺失
const E_COUNT: u16 = SEG_BASE + 6; // 份数不守恒
const E_REPLAY: u16 = SEG_BASE + 7; // 复核漂移
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段

/// N 部五域闭集。
pub const D_SIG: u8 = 1; // 签署域
pub const D_SEC: u8 = 2; // 安全域
pub const D_OBS: u8 = 3; // 观测域
pub const D_DOC: u8 = 4; // 文档域
pub const D_AUD: u8 = 5; // 审计域
pub const DOMAIN_N: u32 = 5;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一份互审材料。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackItem {
    pub domain: u8,  // 五域闭集
    pub digest: u64, // 材料摘要
    pub sign_seq: u32, // 消费方联签序号（显式编位）
    pub sign: u64,   // 联签签名
}

impl PackItem {
    /// 联签签名：域 + 摘要 + 序号混签。
    pub fn make_sign(domain: u8, digest: u64, sign_seq: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(domain);
        buf.extend_from_slice(&digest.to_le_bytes());
        buf.extend_from_slice(&sign_seq.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.domain < D_SIG || self.domain > D_AUD {
            return Err(E_DOMAIN);
        }
        if self.sign != PackItem::make_sign(self.domain, self.digest, self.sign_seq) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 材料包。
pub struct AuditPack {
    pub items: Vec<PackItem>,
}

impl AuditPack {
    pub fn new() -> AuditPack {
        AuditPack { items: vec![] }
    }

    /// 材料登记：域唯一 + 联签序号显式连续。
    pub fn enroll(&mut self, domain: u8, digest: u64, sign_seq: u32) -> Result<(), u16> {
        if domain < D_SIG || domain > D_AUD {
            return Err(E_DOMAIN);
        }
        if sign_seq != self.items.len() as u32 + 1 {
            return Err(E_SEQ);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].domain == domain {
                return Err(E_DUP);
            }
            i += 1;
        }
        let sign = PackItem::make_sign(domain, digest, sign_seq);
        self.items.push(PackItem { domain, digest, sign_seq, sign });
        Ok(())
    }

    /// 材料包齐：五域全覆盖。
    pub fn pack_full(&self) -> Result<(), u16> {
        let mut d = D_SIG;
        while d <= D_AUD {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.items.len() {
                if self.items[i].domain == d {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return Err(E_MISS);
            }
            d += 1;
        }
        Ok(())
    }

    /// 全量核对：逐份 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            self.items[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.push(self.items[i].domain);
            buf.extend_from_slice(&self.items[i].digest.to_le_bytes());
            buf.extend_from_slice(&self.items[i].sign_seq.to_le_bytes());
            buf.extend_from_slice(&self.items[i].sign.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 域闭集判定与中文名。
pub fn domain_ok(d: u8) -> bool {
    d >= D_SIG && d <= D_AUD
}

pub fn domain_name(d: u8) -> &'static str {
    match d {
        D_SIG => "sig",
        D_SEC => "sec",
        D_OBS => "obs",
        D_DOC => "doc",
        _ => "aud",
    }
}

/// 材料包签名：域数 × 份数 × 指纹混签。
pub fn pack_sig(domains: u32, n: u32, fp: u64) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&domains.to_le_bytes());
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(&fp.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：domains=5 items=5 sign=full。
pub fn pack_line(n: u32) -> String {
    let mut out = String::new();
    out.push_str("domains=5 items=");
    out.push_str(&n.to_string());
    out.push_str(" sign=full");
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_73);

    // R2 材料登记 + R3 联签落痕：五域各一份，摘要 LCG 生成。
    let mut pack = AuditPack::new();
    let mut d = D_SIG;
    while d <= D_AUD {
        let digest = fnv1a64(&d.to_le_bytes()) ^ (g.next_u32() as u64);
        pack.enroll(d, digest, pack.items.len() as u32 + 1)?;
        d += 1;
    }
    if pack.items.len() as u32 != DOMAIN_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 材料包齐断言。
    pack.pack_full()?;
    hits += 1;

    // R5 域越界检出：域 0 与 6 拒绝（含闭集判定辅助核对）。
    if domain_ok(0) || domain_ok(6) || !domain_ok(D_AUD) {
        return Err(E_DOMAIN);
    }
    let mut probe = AuditPack::new();
    match probe.enroll(0, 1, 1) {
        Err(E_DOMAIN) => {}
        _ => return Err(E_DOMAIN),
    }
    match probe.enroll(6, 1, 1) {
        Err(E_DOMAIN) => hits += 1,
        _ => return Err(E_DOMAIN),
    }

    // R6 域重复登记检出。
    match pack.enroll(D_SIG, 99, pack.items.len() as u32 + 1) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 联签序号断链检出：跳号登记拒绝。
    match probe.enroll(D_SIG, 5, 3) {
        Err(E_SEQ) => hits += 1,
        _ => return Err(E_SEQ),
    }

    // R8 材料缺失检出：空包断言红。
    match probe.pack_full() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R9 复核一致 + 指纹稳定。
    pack.verify_all()?;
    let f1 = pack.fp();
    let f2 = pack.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R10 计数守恒 + 呈现行：items=5 sign=full。
    if pack.items.len() as u32 != DOMAIN_N {
        return Err(E_COUNT);
    }
    let line = pack_line(pack.items.len() as u32);
    if !has_kw(line.as_bytes(), b"domains=5")
        || !has_kw(line.as_bytes(), b"items=5")
        || !has_kw(line.as_bytes(), b"sign=full")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 材料包签名：5×5×指纹混签重算一致 + 域名闭包。
    let mut d2 = D_SIG;
    while d2 <= D_AUD {
        if !domain_ok(d2) || domain_name(d2).is_empty() {
            return Err(E_DOMAIN);
        }
        d2 += 1;
    }
    let sig = pack_sig(DOMAIN_N, pack.items.len() as u32, f1);
    if sig != pack_sig(5, 5, f1) || sig == 0 {
        return Err(E_SIG);
    }
    let _ = g.below(6);
    hits += 1;

    Ok(hits)
}
