//! unx0632_dcov —— UNX-F0632 域覆盖率账（判据全覆盖终核）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0632 · 域覆盖率账（判据全覆盖终核） · 目标 ≥220 行
//! 判据原文摘录：覆盖终核（域判据 660×映射——已深化批全覆盖断言），
//!   漏覆盖检出注入红。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 判据全域 660 条（来源五数账），判据号闭集 1..660，号外拒收。
//!   2. 覆盖账 = 位图账：[u64; 11] 共 704 位（660 有效位），一位一判据，
//!      已深化批断言置位即"已覆盖"。
//!   3. 全覆盖断言：低 660 位全 1 才放行；漏覆盖注入（清一位）必被
//!      检出并定位判据号。
//!   4. 映射面：判据号 → 已深化批号（33 批）映射账，批号闭集外拒收，
//!      映射行数 = 覆盖位数（两账一致恒等）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x50C0 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x50C0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x50C0;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x50CF;

/// XcCode：判据号越界。
pub const XC_BAD_CRIT: u16 = SEG_BASE + 1; // 0x50C1 判据号 1..660 闭集外
/// XcCode：批号越界。
pub const XC_BAD_BATCH: u16 = SEG_BASE + 2; // 0x50C2 批号 0..33 闭集外
/// XcCode：重复置位。
pub const XC_DUP_COVER: u16 = SEG_BASE + 3; // 0x50C3 一判据一覆盖
/// XcCode：漏覆盖。
pub const XC_COVER_MISS: u16 = SEG_BASE + 4; // 0x50C4 位图有 0 位
/// XcCode：映射账与位图账脱节。
pub const XC_MAP_MISMATCH: u16 = SEG_BASE + 5; // 0x50C5 行数 ≠ 覆盖位数
/// XcCode：批数不符。
pub const XC_BATCH_N: u16 = SEG_BASE + 6; // 0x50C6 批数 ≠ 33

/// 域判据总数（判据原文 660）。
pub const CRIT_TOTAL: u32 = 660;
/// 位图字数（660 位 → 11 个 u64）。
pub const WORDS: usize = 11;
/// 已深化批数。
pub const BATCH_N: usize = 33;

/// 覆盖位图账本 + 判据→批映射账。
#[derive(Clone, Debug)]
pub struct CovBook {
    /// 位图（低 660 位有效）。
    bits: [u64; WORDS],
    /// 映射账：行 = (判据号, 批号)（按判据号升序）。
    map: Vec<(u16, usize)>,
}

impl CovBook {
    /// 空账。
    pub fn new() -> Self {
        CovBook {
            bits: [0; WORDS],
            map: Vec::new(),
        }
    }

    /// 判据号 → (字, 位)。
    fn cell(crit_id: u16) -> Result<(usize, u32), u16> {
        if crit_id == 0 || crit_id as u32 > CRIT_TOTAL {
            return Err(XC_BAD_CRIT);
        }
        let i = crit_id as usize - 1;
        Ok((i / 64, (i % 64) as u32))
    }

    /// 置一覆盖位（去重）。
    pub fn cover(&mut self, crit_id: u16, batch: usize) -> Result<(), u16> {
        let (w, b) = Self::cell(crit_id)?;
        if batch >= BATCH_N {
            return Err(XC_BAD_BATCH);
        }
        if self.bits[w] & (1u64 << b) != 0 {
            return Err(XC_DUP_COVER);
        }
        self.bits[w] |= 1u64 << b;
        self.map.push((crit_id, batch));
        self.map.sort_by_key(|r| r.0);
        Ok(())
    }

    /// 查一位。
    pub fn covered(&self, crit_id: u16) -> Result<bool, u16> {
        let (w, b) = Self::cell(crit_id)?;
        Ok(self.bits[w] & (1u64 << b) != 0)
    }

    /// 覆盖位数。
    pub fn covered_n(&self) -> usize {
        let mut n = 0usize;
        for (i, w) in self.bits.iter().enumerate() {
            let mut ww = *w;
            // 尾字高位（660..704）不计。
            if i == WORDS - 1 {
                let valid = (CRIT_TOTAL as usize % 64) as u32; // 660 % 64 = 52
                if valid > 0 {
                    ww &= (1u64 << valid) - 1;
                }
            }
            n += ww.count_ones() as usize;
        }
        n
    }

    /// 漏覆盖定位：返回最小未覆盖判据号（全覆盖回 None）。
    pub fn first_miss(&self) -> Option<u16> {
        for id in 1..=CRIT_TOTAL as u16 {
            match self.covered(id) {
                Ok(false) => return Some(id),
                Ok(true) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// 全覆盖断言：低 660 位全 1。
    pub fn full_covered(&self) -> Result<usize, u16> {
        if self.covered_n() != CRIT_TOTAL as usize {
            return Err(XC_COVER_MISS);
        }
        Ok(CRIT_TOTAL as usize)
    }

    /// 两账一致恒等：映射行数 = 覆盖位数，且映射判据号逐一置位。
    pub fn map_eq_bits(&self) -> Result<usize, u16> {
        if self.map.len() != self.covered_n() {
            return Err(XC_MAP_MISMATCH);
        }
        for (id, b) in &self.map {
            if !self.covered(*id)? {
                return Err(XC_MAP_MISMATCH);
            }
            if *b >= BATCH_N {
                return Err(XC_BAD_BATCH);
            }
        }
        Ok(self.map.len())
    }

    /// 批分布核：33 批全有覆盖。
    pub fn batch_dist(&self) -> Result<[u32; BATCH_N], u16> {
        let mut dist = [0u32; BATCH_N];
        for (_, b) in &self.map {
            if *b >= BATCH_N {
                return Err(XC_BAD_BATCH);
            }
            dist[*b] += 1;
        }
        if dist.iter().any(|d| *d == 0) {
            return Err(XC_BATCH_N);
        }
        Ok(dist)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "域覆盖率账 · 覆盖 {}/{} · 映射行 {}",
            self.covered_n(),
            CRIT_TOTAL,
            self.map.len()
        );
        if let Some(m) = self.first_miss() {
            s.push_str(&alloc::format!("\n  最小漏覆盖判据 F0{}", m));
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 标准全覆盖构造器：判据 id 覆盖到批 (id-1) % 33（33 批全有分布：
/// 660 = 33*20，每批恰 20 条）。
pub fn build_full_cov() -> Result<CovBook, u16> {
    let mut b = CovBook::new();
    for id in 1..=CRIT_TOTAL as u16 {
        b.cover(id, ((id as u32 - 1) % BATCH_N as u32) as usize)?;
    }
    b.full_covered()?;
    b.map_eq_bits()?;
    Ok(b)
}

/// 自例程：主路径（660 全覆盖→两账一致→批分布）+ 错误路径（越界/重置/漏覆盖/脱节）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let b = build_full_cov()?;
    if b.full_covered()? != CRIT_TOTAL as usize {
        return Err(XC_COVER_MISS);
    }
    if b.first_miss().is_some() {
        return Err(XC_COVER_MISS);
    }
    let dist = b.batch_dist()?;
    // 每批 20 条（660/33 = 20 整除）。
    if dist.iter().any(|d| *d != 20) {
        return Err(XC_BATCH_N);
    }
    if b.fingerprint() == 0 {
        return Err(XC_MAP_MISMATCH);
    }

    // 错误路径 1：判据号越界（0 与 661）。
    let mut b1 = CovBook::new();
    if b1.cover(0, 0) != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    if b1.cover(661, 0) != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    // 错误路径 2：批号越界。
    let mut b2 = CovBook::new();
    if b2.cover(1, BATCH_N) != Err(XC_BAD_BATCH) {
        return Err(XC_BAD_BATCH);
    }
    // 错误路径 3：重复置位。
    let mut b3 = CovBook::new();
    b3.cover(1, 0)?;
    if b3.cover(1, 1) != Err(XC_DUP_COVER) {
        return Err(XC_DUP_COVER);
    }
    // 错误路径 4：漏覆盖注入（清一位 → 定位）。
    let mut b4 = build_full_cov()?;
    b4.bits[0] &= !(1u64); // 清判据 1
    if b4.full_covered() != Err(XC_COVER_MISS) {
        return Err(XC_COVER_MISS);
    }
    if b4.first_miss() != Some(1) {
        return Err(XC_COVER_MISS);
    }
    // 错误路径 5：两账脱节（抽一行映射）。
    let mut b5 = build_full_cov()?;
    b5.map.pop();
    if b5.map_eq_bits() != Err(XC_MAP_MISMATCH) {
        return Err(XC_MAP_MISMATCH);
    }
    // 错误路径 6：批分布断链（全部映射到批 0）。
    let mut b6 = CovBook::new();
    for id in 1..=CRIT_TOTAL as u16 {
        b6.cover(id, 0)?;
    }
    if b6.full_covered().is_err() {
        return Err(XC_COVER_MISS);
    }
    if b6.batch_dist() != Err(XC_BATCH_N) {
        return Err(XC_BATCH_N);
    }
    Ok(0)
}
