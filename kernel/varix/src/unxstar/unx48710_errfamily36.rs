//! UNX-F48710 —— I36 错误码族 I 段（6 条）
//! 判据原文：UNX-F48710-J1 错误码 ×6 零漂移；判据锚定 D1 同源体系。
//! 判据落地（drill 场景）：R1 段基址冻结；R2 六码连续编位；
//! R3 码值互异；R4 族口径一致；R5 漂移检出；R6 指纹稳定；
//! R7 复扫一致；R8 呈现行；R9 同源锚定；R10 全族演练覆盖。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：错误码族 I 段 6 条自 SEG_BASE+1 起连续编位，码义
//! 逐条固定（转单悬挂/占比欠额/收点越界/窗型越界/埋点缺失/
//! 对账未达）；零漂移 = 码值与码义复扫一致且互异。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1020;

// ── 错误码族 I 段（SEG_BASE+1 起连续 6 条）─────────────────────
const E_TRANSFER_HANG: u16 = SEG_BASE + 1; // 转单悬挂
const E_RATIO_SHORT: u16 = SEG_BASE + 2; // 占比欠额
const E_PEER_RANGE: u16 = SEG_BASE + 3; // 收点越界
const E_WIN_RANGE: u16 = SEG_BASE + 4; // 窗型越界
const E_OBS_MISS: u16 = SEG_BASE + 5; // 埋点缺失
const E_RECON_SHORT: u16 = SEG_BASE + 6; // 对账未达

/// 错误码族条目。
pub const FAMILY_N: u32 = 6;

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

/// 错误码条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
}

/// 族全表：码值连续 + 码义固定。
pub const FAMILY: [ErrEntry; 6] = [
    ErrEntry { code: E_TRANSFER_HANG, name: "transfer_hang" },
    ErrEntry { code: E_RATIO_SHORT, name: "ratio_short" },
    ErrEntry { code: E_PEER_RANGE, name: "peer_range" },
    ErrEntry { code: E_WIN_RANGE, name: "win_range" },
    ErrEntry { code: E_OBS_MISS, name: "obs_miss" },
    ErrEntry { code: E_RECON_SHORT, name: "recon_short" },
];

/// 同源锚定：码族全部落入本段窗。
pub fn family_in_segment() -> bool {
    let mut i = 0usize;
    while i < FAMILY.len() {
        let c = FAMILY[i].code;
        if c <= SEG_BASE || c > SEG_BASE + 0x1F {
            return false;
        }
        i += 1;
    }
    true
}

/// 码值连续性：+1 逐条递进。
pub fn family_contiguous() -> bool {
    let mut i = 1usize;
    while i < FAMILY.len() {
        if FAMILY[i].code != FAMILY[i - 1].code + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 码值互异性。
pub fn family_distinct() -> bool {
    let mut i = 0usize;
    while i < FAMILY.len() {
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].code == FAMILY[j].code || FAMILY[i].name == FAMILY[j].name {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 族指纹。
pub fn family_fp() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < FAMILY.len() {
        buf.extend_from_slice(&FAMILY[i].code.to_le_bytes());
        buf.extend_from_slice(FAMILY[i].name.as_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 码族演练账：逐条登记某码曾被触发演练。
pub struct ErrDrill {
    pub fired: Vec<u16>,
}

impl ErrDrill {
    pub fn new() -> ErrDrill {
        ErrDrill { fired: vec![] }
    }

    /// 登记一次触发：码必须属于族内。
    pub fn fire(&mut self, code: u16) -> Result<(), u16> {
        let mut ok = false;
        let mut i = 0usize;
        while i < FAMILY.len() {
            if FAMILY[i].code == code {
                ok = true;
            }
            i += 1;
        }
        if !ok {
            return Err(E_PEER_RANGE);
        }
        self.fired.push(code);
        Ok(())
    }

    /// 全族覆盖：6 条码各触发 ≥1 次。
    pub fn full_cover(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < FAMILY.len() {
            let mut hit = false;
            let mut j = 0usize;
            while j < self.fired.len() {
                if self.fired[j] == FAMILY[i].code {
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return Err(E_OBS_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 演练指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.fired.len() {
            buf.extend_from_slice(&self.fired[i].to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 呈现行：n=6 seg=<基址> head=<首码hex>。
pub fn family_line() -> String {
    let mut out = String::new();
    out.push_str("n=6 seg=");
    out.push_str(&SEG_BASE.to_string());
    out.push_str(" head=");
    out.push_str(&FAMILY[0].code.to_string());
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_10);

    // R1 段基址冻结：0x1020。
    if SEG_BASE != 0x1020 {
        return Err(E_RECON_SHORT);
    }
    hits += 1;

    // R2 六码连续编位。
    if FAMILY.len() as u32 != FAMILY_N || !family_contiguous() {
        return Err(E_RATIO_SHORT);
    }
    hits += 1;

    // R3 码值互异 + 码义互异。
    if !family_distinct() {
        return Err(E_PEER_RANGE);
    }
    hits += 1;

    // R4 族口径一致：同源锚定段窗。
    if !family_in_segment() {
        return Err(E_WIN_RANGE);
    }
    hits += 1;

    // R5 漂移检出：首码必须恰为 SEG_BASE+1。
    if FAMILY[0].code != SEG_BASE + 1 {
        return Err(E_OBS_MISS);
    }
    hits += 1;

    // R6 指纹稳定。
    let f1 = family_fp();
    let f2 = family_fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_OBS_MISS);
    }
    hits += 1;

    // R7 复扫一致：全表再扫结果不变。
    if !family_contiguous() || !family_in_segment() || !family_distinct() {
        return Err(E_RECON_SHORT);
    }
    hits += 1;

    // R8 呈现行：n=6 seg=4128（0x1020 十进制）。
    let line = family_line();
    if !has_kw(line.as_bytes(), b"n=6") || !has_kw(line.as_bytes(), b"seg=4128") {
        return Err(E_OBS_MISS);
    }
    hits += 1;

    // R9 同源锚定：每条码与首码差即位序。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].code != FAMILY[0].code + i as u16 {
            return Err(E_PEER_RANGE);
        }
        i += 1;
    }
    hits += 1;

    // R10 计数守恒：全族演练覆盖 6 条码。
    let mut d = ErrDrill::new();
    let mut i2 = 0usize;
    while i2 < FAMILY.len() {
        d.fire(FAMILY[i2].code)?;
        i2 += 1;
    }
    d.full_cover()?;
    if d.fired.len() as u32 != FAMILY_N {
        return Err(E_RECON_SHORT);
    }
    // 演练指纹稳定。
    let d1 = d.fp();
    let d2 = d.fp();
    if d1 != d2 || d1 == 0 {
        return Err(E_OBS_MISS);
    }
    // LCG 扰动路径确定性消费。
    let _ = g.below(8);
    hits += 1;

    Ok(hits)
}
