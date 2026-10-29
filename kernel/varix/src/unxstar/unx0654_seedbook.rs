//! UNX-F0654 · 域级种子簿（LCG 种子登记与复现）· 诊断段 0x5340（本单独占）
//!
//! 目标行数：≥200（纯功能代码）
//! 判据原文摘录：UNX-F0654-J1 种子登记簿（轮号/种子/用途三列）账齐，同种子复现率 100%。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 种子簿三列账（轮号/种子/用途），轮号唯一、种子非零，逐行可复现。
//!  3. 复现协议：登记种子 → 派生序列 → 两次派生逐位比对 → 复现率入账。
//!  4. 异种注入：登记种子与实际用种不符必须检出，复现率绝不虚记。
//!  5. 诊断段 0x5340 为本模块独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5340;

// ── 诊断码闭集 ───────────────────────────────────────────────
pub const E_SEED0: u16 = SEG_BASE + 1;   // 种子为 0（LCG 平凡化）
pub const E_DUPROUND: u16 = SEG_BASE + 2; // 轮号重复登记
pub const E_NOENTRY: u16 = SEG_BASE + 3; // 簿内无此轮登记
pub const E_SEEDMIS: u16 = SEG_BASE + 4; // 实际用种与登记不符
pub const E_SEQ: u16 = SEG_BASE + 5;     // 派生序列长度为 0
pub const E_NOREPRO: u16 = SEG_BASE + 6; // 同种子两次派生不一致

/// 用途闭集（域回归场景四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedPurpose {
    /// 判据集洗牌（执行顺序随机化）。
    Shuffle,
    /// 样本生成（回归样本体）。
    Sample,
    /// 故障注入点位选取。
    Inject,
    /// 压力负载整形。
    Load,
}

impl SeedPurpose {
    pub fn name(self) -> &'static str {
        match self {
            SeedPurpose::Shuffle => "判据洗牌",
            SeedPurpose::Sample => "样本生成",
            SeedPurpose::Inject => "故障注入",
            SeedPurpose::Load => "压力负载",
        }
    }
}

/// 种子簿账行：三列（轮号/种子/用途）+ 派生指纹。
#[derive(Clone, Debug)]
pub struct SeedEntry {
    /// 轮号（1 起，簿内唯一）。
    pub round: u16,
    /// 登记种子（非 0）。
    pub seed: u64,
    /// 用途。
    pub purpose: SeedPurpose,
    /// 该种子派生序列的 FNV 指纹（复现对账锚点）。
    pub seq_fp: u64,
}

/// LCG64：x' = a·x + c（模 2^64，用环绕乘加实现）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_SEED0);
        }
        Ok(Lcg(seed | 1))
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

/// 派生 n 长序列（确定性）。
pub fn derive_seq(seed: u64, n: usize) -> Result<Vec<u64>, u16> {
    if n == 0 {
        return Err(E_SEQ);
    }
    let mut g = Lcg::new(seed)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(g.next_u64());
    }
    Ok(out)
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn seq_fp(seq: &[u64]) -> u64 {
    let mut buf = vec![0u8; seq.len() * 8];
    for (i, v) in seq.iter().enumerate() {
        buf[i * 8] = (v >> 56) as u8;
        buf[i * 8 + 1] = (v >> 48) as u8;
        buf[i * 8 + 2] = (v >> 40) as u8;
        buf[i * 8 + 3] = (v >> 32) as u8;
        buf[i * 8 + 4] = (v >> 24) as u8;
        buf[i * 8 + 5] = (v >> 16) as u8;
        buf[i * 8 + 6] = (v >> 8) as u8;
        buf[i * 8 + 7] = *v as u8;
    }
    fnv1a64(&buf)
}

/// 种子登记簿。
pub struct SeedBook {
    pub entries: Vec<SeedEntry>,
    /// 派生序列长度（复现协议固定参数）。
    pub seq_len: usize,
    /// 复现成功次数 / 复现尝试次数。
    pub repro_ok: u32,
    pub repro_try: u32,
}

impl SeedBook {
    pub fn new(seq_len: usize) -> Result<SeedBook, u16> {
        if seq_len == 0 {
            return Err(E_SEQ);
        }
        Ok(SeedBook { entries: Vec::new(), seq_len, repro_ok: 0, repro_try: 0 })
    }

    /// 登记：轮号唯一，种子非零，登记时即算派生指纹锚点。
    pub fn register(&mut self, round: u16, seed: u64, purpose: SeedPurpose) -> Result<(), u16> {
        if seed == 0 {
            return Err(E_SEED0);
        }
        if self.entries.iter().any(|e| e.round == round) {
            return Err(E_DUPROUND);
        }
        let fp = seq_fp(&derive_seq(seed, self.seq_len)?);
        self.entries.push(SeedEntry { round, seed, purpose, seq_fp: fp });
        Ok(())
    }

    /// 复现核：取登记种子重派生，与锚点指纹比；同种子复现率必须 100%。
    pub fn verify_round(&mut self, round: u16) -> Result<bool, u16> {
        let e = self
            .entries
            .iter()
            .find(|e| e.round == round)
            .ok_or(E_NOENTRY)?;
        let fp = seq_fp(&derive_seq(e.seed, self.seq_len)?);
        self.repro_try += 1;
        if fp == e.seq_fp {
            self.repro_ok += 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 异种注入核：实际用种 ≠ 登记种 → 必须指纹失配（检出即通过）。
    pub fn inject_mismatch_must_catch(&mut self, round: u16, wrong_seed: u64) -> Result<(), u16> {
        let e = self
            .entries
            .iter()
            .find(|e| e.round == round)
            .ok_or(E_NOENTRY)?;
        if wrong_seed == e.seed {
            return Err(E_SEEDMIS);
        }
        let fp = seq_fp(&derive_seq(wrong_seed, self.seq_len)?);
        if fp == e.seq_fp {
            return Err(E_NOREPRO);
        }
        Ok(())
    }

    /// 簿齐判定：三列齐（轮号/种子/用途均有值）且复现率 = 100%。
    pub fn audit(&mut self) -> Result<(usize, u32, u32), u16> {
        for e in &self.entries {
            if e.seed == 0 {
                return Err(E_SEED0);
            }
            let _ = e.purpose.name(); // 用途列可解引
        }
        let rounds = self.entries.len();
        for i in 0..rounds {
            let r = self.entries[i].round;
            if !self.verify_round(r)? {
                return Err(E_NOREPRO);
            }
        }
        Ok((rounds, self.repro_ok, self.repro_try))
    }

    /// 复现率（千分比整数）。
    pub fn repro_permille(&self) -> u32 {
        if self.repro_try == 0 {
            return 0;
        }
        self.repro_ok * 1000 / self.repro_try
    }

    /// 簿摘要行（零手抄，全部派生）。
    pub fn summary(&self) -> String {
        let mut s = String::from("种子簿: 行数=");
        s.push_str(&self.entries.len().to_string());
        s.push_str(" 复现=");
        s.push_str(&self.repro_permille().to_string());
        s.push_str("/1000");
        s
    }
}

/// 自例程：登记 10 轮三列账齐 + 复现 100% + 异种注入全检出 → 0。
pub fn drill() -> Result<u32, u16> {
    let mut book = SeedBook::new(64)?;
    let purposes = [
        SeedPurpose::Shuffle,
        SeedPurpose::Sample,
        SeedPurpose::Inject,
        SeedPurpose::Load,
    ];
    for r in 1..=10u16 {
        // 种子确定性派生：轮号混频，非 0
        let seed = 0x0654_0000 ^ (r as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1;
        book.register(r, seed, purposes[(r as usize) % 4])?;
    }
    let (rounds, ok, try_n) = book.audit()?;
    if rounds != 10 || ok != try_n || book.repro_permille() != 1000 {
        return Err(E_NOREPRO);
    }
    // 注入 1：重复轮号 → 拦截
    if book.register(3, 12345, SeedPurpose::Sample) != Err(E_DUPROUND) {
        return Err(SEG_BASE + 15);
    }
    // 注入 2：零种子 → 拦截
    if book.register(11, 0, SeedPurpose::Sample) != Err(E_SEED0) {
        return Err(SEG_BASE + 16);
    }
    // 注入 3：异种对照，5 处必须全部检出指纹失配
    for r in 1..=5u16 {
        book.inject_mismatch_must_catch(r, 0xDEAD_0001 ^ r as u64)?;
    }
    // 注入 4：查无此轮 → 拦截
    if book.verify_round(99).is_ok() {
        return Err(SEG_BASE + 17);
    }
    Ok(0)
}
