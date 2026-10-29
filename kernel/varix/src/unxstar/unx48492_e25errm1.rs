//! UNX-F48492 —— E25 错误码族 E 段（8 条）
//! 判据原文：UNX-F48492-J1 E25 段错误码族共 8 条，码值 SEG_BASE+1 起连续，
//! 名称/严重度/处置建议三字段齐全且互异；族外查询必落空；段外越界必拒。
//! 判据落地（drill 场景）：R1 族规模恰 8；R2 码值连续升序且防重；
//! R3 名称唯一非空；R4 严重度落在 1..=5；R5 处置建议非空且互异；
//! R6 全族查询回环；R7 族外/段外查询落空；R8 LCG 探针按序命中；
//! R9 指纹稳定；R10 高严重度配额受控。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE7C0;

// ── 族内 8 条诊断码（+1 起连续）─────────────────────────────────
const E_SHIFT_LEAK: u16 = SEG_BASE + 1; // 长按 Shift 误切泄漏
const E_SHIFT_FORGE: u16 = SEG_BASE + 2; // 时序伪装未识破
const E_DBL_COMMIT: u16 = SEG_BASE + 3; // 双击双份上屏
const E_STORM_PASS: u16 = SEG_BASE + 4; // 风暴超配额放行
const E_CAPS_FLIP: u16 = SEG_BASE + 5; // 抖动翻转漂移
const E_MOD_WRONG: u16 = SEG_BASE + 6; // 修饰键错态
const E_IM_LEAK: u16 = SEG_BASE + 7; // 组合期快捷键泄漏
const E_COMMIT_SEIZE: u16 = SEG_BASE + 8; // 组合态提交抢注穿透

// ── 族外审计码（+0x09..，≤ +0x1F）───────────────────────────────
const E_AUD_GAP: u16 = SEG_BASE + 9; // 码值不连续
const E_AUD_NAME: u16 = SEG_BASE + 10; // 名称空/重复
const E_AUD_SEV: u16 = SEG_BASE + 11; // 严重度越界
const E_AUD_ACT: u16 = SEG_BASE + 12; // 建议空/重复
const E_AUD_MISS: u16 = SEG_BASE + 13; // 探针未命中
const E_AUD_OOB: u16 = SEG_BASE + 14; // 族码越出段窗
const E_AUD_FP: u16 = SEG_BASE + 15; // 族指纹漂移
const E_AUD_QUOTA: u16 = SEG_BASE + 16; // 高严重度超配额

/// 族条目：码值 / 名称 / 严重度 / 处置建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub sev: u8,
    pub action: &'static str,
}

/// E25 段 8 条错误码族（冻结清单，顺序即码值升序）。
pub const FAMILY: [ErrEntry; 8] = [
    ErrEntry { code: E_SHIFT_LEAK, name: "E25-SHIFT-LEAK", sev: 3, action: "复核长按路径 latch 记账" },
    ErrEntry { code: E_SHIFT_FORGE, name: "E25-SHIFT-FORGE", sev: 4, action: "补齐时序伪装识别闸" },
    ErrEntry { code: E_DBL_COMMIT, name: "E25-DBL-COMMIT", sev: 4, action: "核对双击窗口去重账" },
    ErrEntry { code: E_STORM_PASS, name: "E25-STORM-PASS", sev: 2, action: "压低防抖窗口配额" },
    ErrEntry { code: E_CAPS_FLIP, name: "E25-CAPS-FLIP", sev: 3, action: "复核抖动消除窗边界" },
    ErrEntry { code: E_MOD_WRONG, name: "E25-MOD-WRONG", sev: 5, action: "重放交错流核对终态账" },
    ErrEntry { code: E_IM_LEAK, name: "E25-IM-LEAK", sev: 4, action: "补组合期屏蔽层埋点" },
    ErrEntry { code: E_COMMIT_SEIZE, name: "E25-COMMIT-SEIZE", sev: 5, action: "核对抢注拦截闸序" },
];

/// 高严重度（sev ≥ 4）配额：全族至多 5 条。
pub const HIGH_SEV_QUOTA: usize = 5;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
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

/// 码值 → 条目（线性扫描，族外返回 None）。
pub fn lookup(code: u16) -> Option<&'static ErrEntry> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].code == code {
            return Some(&FAMILY[i]);
        }
        i += 1;
    }
    None
}

/// 码值 → 处置建议（族外统一落 "未知诊断码"）。
pub fn detail(code: u16) -> &'static str {
    match lookup(code) {
        Some(e) => e.action,
        None => "未知诊断码",
    }
}

/// 族指纹：码值/名称/严重度/建议逐条串联后 FNV。
pub fn family_fingerprint() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < FAMILY.len() {
        let e = &FAMILY[i];
        buf.extend_from_slice(&e.code.to_le_bytes());
        buf.push(e.sev);
        buf.extend_from_slice(e.name.as_bytes());
        buf.push(0x1F);
        buf.extend_from_slice(e.action.as_bytes());
        buf.push(0x1E);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 高严重度条数统计。
pub fn high_sev_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].sev >= 4 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 族条目行（呈现面）：code=<n> name=<名> sev=<n>。
pub fn entry_line(e: &ErrEntry) -> String {
    let mut s = String::new();
    s.push_str("code=");
    s.push_str(&e.code.to_string());
    s.push_str(" name=");
    s.push_str(e.name);
    s.push_str(" sev=");
    s.push_str(&e.sev.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 族规模恰 8。
    if FAMILY.len() != 8 {
        return Err(E_AUD_GAP);
    }
    hits += 1;

    // R2 码值连续升序、防重、不越段窗。
    let mut i = 0usize;
    while i < FAMILY.len() {
        let expect = SEG_BASE + 1 + i as u16;
        if FAMILY[i].code != expect {
            return Err(E_AUD_GAP);
        }
        if FAMILY[i].code > SEG_BASE + 0x1F {
            return Err(E_AUD_OOB);
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].code == FAMILY[j].code {
                return Err(E_AUD_GAP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 名称唯一非空。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].name.is_empty() {
            return Err(E_AUD_NAME);
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].name == FAMILY[j].name {
                return Err(E_AUD_NAME);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R4 严重度落在 1..=5。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].sev < 1 || FAMILY[i].sev > 5 {
            return Err(E_AUD_SEV);
        }
        i += 1;
    }
    hits += 1;

    // R5 处置建议非空且互异。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].action.is_empty() {
            return Err(E_AUD_ACT);
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].action == FAMILY[j].action {
                return Err(E_AUD_ACT);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R6 全族查询回环：按码值查回条目逐字段一致。
    let mut i = 0usize;
    while i < FAMILY.len() {
        match lookup(FAMILY[i].code) {
            Some(e) => {
                if *e != FAMILY[i] {
                    return Err(E_AUD_MISS);
                }
            }
            None => return Err(E_AUD_MISS),
        }
        i += 1;
    }
    hits += 1;

    // R7 族外/段外查询落空：-1、+0x20、0 均查不到。
    if lookup(SEG_BASE).is_some() || lookup(SEG_BASE + 0x20).is_some() || lookup(0).is_some() {
        return Err(E_AUD_OOB);
    }
    hits += 1;

    // R8 LCG 随机探针：16 次取样按序命中。
    let mut lcg = Lcg::new(0x48492);
    let mut t = 0u32;
    while t < 16 {
        let idx = lcg.below(8) as usize;
        match lookup(SEG_BASE + 1 + idx as u16) {
            Some(e) => {
                if e.code != SEG_BASE + 1 + idx as u16 {
                    return Err(E_AUD_MISS);
                }
            }
            None => return Err(E_AUD_MISS),
        }
        t += 1;
    }
    hits += 1;

    // R9 指纹稳定：同族两次指纹一致且非零。
    let f1 = family_fingerprint();
    let f2 = family_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_AUD_FP);
    }
    hits += 1;

    // R10 高严重度配额：sev≥4 恰 5 条（E_SHIFT_FORGE/E_DBL_COMMIT/E_IM_LEAK/E_MOD_WRONG/E_COMMIT_SEIZE）。
    let hc = high_sev_count();
    if hc as usize != HIGH_SEV_QUOTA {
        return Err(E_AUD_QUOTA);
    }
    // 呈现行烟测：三段前缀在位。
    let line = entry_line(&FAMILY[0]);
    if !has_kw(line.as_bytes(), b"code=") || !has_kw(line.as_bytes(), b"name=") || !has_kw(line.as_bytes(), b"sev=") {
        return Err(E_AUD_FP);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
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
