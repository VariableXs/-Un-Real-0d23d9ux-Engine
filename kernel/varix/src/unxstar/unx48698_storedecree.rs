//! UNX-F48698 —— I·L3 商店下架词库处置面（下架不删本地已装词库声明）
//! 判据原文：UNX-F48698-J1 处置 ×10 行为符合声明；判据锚定 体验十二。
//! 判据落地（drill 场景）：R1 声明入册；R2 下架 ×10 本地保留；
//! R3 已装位不变；R4 新装拒绝；R5 已装用户不受扰；R6 处置留痕；
//! R7 指纹稳定；R8 呈现行；R9 计数守恒；R10 声明冻结。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0EA0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_DECL: u16 = SEG_BASE + 1; // 声明未入册
const E_LOCAL: u16 = SEG_BASE + 2; // 下架误删本地词库
const E_INSTALLED: u16 = SEG_BASE + 3; // 已装位被改动
pub const E_NEWINSTALL: u16 = SEG_BASE + 4; // 下架后新装未拒
const E_TRACE: u16 = SEG_BASE + 5; // 处置留痕缺失
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒
const E_DUP: u16 = SEG_BASE + 9; // 重复下架

/// 下架处置轮次。
pub const DELIST_N: u32 = 10;
/// 处置声明（冻结口径）。
pub const DECREE: &str = "delist-keeps-local";

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

/// 一条商店词库条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LexEntry {
    pub lex_id: u32,
    pub store_listed: bool,  // 商店在架位
    pub local_installed: bool, // 本地已装位（下架不动）
    pub local_fp: u64,       // 本地词库指纹（下架不变）
}

impl LexEntry {
    /// 下架合规：商店在架位翻假，本地两字段不动。
    pub fn delist_ok(&self, before: &LexEntry) -> Result<(), u16> {
        if self.local_installed != before.local_installed {
            return Err(E_INSTALLED);
        }
        if self.local_fp != before.local_fp {
            return Err(E_LOCAL);
        }
        if self.store_listed {
            return Err(E_LOCAL);
        }
        Ok(())
    }
}

/// L3 商店处置面。
pub struct StoreFace {
    pub decree: String,
    pub entries: Vec<LexEntry>,
    pub traces: Vec<(u32, bool)>, // (lex_id, local_kept)
    pub delisted: u32,
    pub new_install_denied: u32,
}

impl StoreFace {
    pub fn new() -> StoreFace {
        StoreFace {
            decree: DECREE.to_string(),
            entries: vec![],
            traces: vec![],
            delisted: 0,
            new_install_denied: 0,
        }
    }

    /// 声明入册。
    pub fn declare(&self) -> Result<(), u16> {
        if self.decree != DECREE {
            return Err(E_DECL);
        }
        Ok(())
    }

    /// 上架一条（初始在架 + 已装）。
    pub fn shelve(&mut self, lex_id: u32, local_fp: u64) -> Result<(), u16> {
        self.entries.push(LexEntry {
            lex_id,
            store_listed: true,
            local_installed: true,
            local_fp,
        });
        Ok(())
    }

    /// 下架：商店位翻假、本地保留，留痕。
    pub fn delist(&mut self, lex_id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].lex_id == lex_id {
                if !self.entries[i].store_listed {
                    return Err(E_DUP);
                }
                let before = self.entries[i].clone();
                self.entries[i].store_listed = false;
                self.entries[i].delist_ok(&before)?;
                self.traces.push((lex_id, true));
                self.delisted += 1;
                return Ok(());
            }
            i += 1;
        }
        Err(E_TRACE)
    }

    /// 已装位不变断言：全账已装位与指纹不因下架而变。
    pub fn locals_intact(&self, fps: &[(u32, u64)]) -> Result<(), u16> {
        let mut i = 0usize;
        while i < fps.len() {
            let mut j = 0usize;
            let mut found = false;
            while j < self.entries.len() {
                if self.entries[j].lex_id == fps[i].0 {
                    found = true;
                    if !self.entries[j].local_installed || self.entries[j].local_fp != fps[i].1 {
                        return Err(E_INSTALLED);
                    }
                }
                j += 1;
            }
            if !found {
                return Err(E_TRACE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 下架后新装拒绝。
    pub fn install_new(&mut self, lex_id: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].lex_id == lex_id && !self.entries[i].store_listed {
                self.new_install_denied += 1;
                return Err(E_NEWINSTALL);
            }
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(self.decree.as_bytes());
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].lex_id.to_le_bytes());
            buf.push(self.entries[i].store_listed as u8);
            buf.push(self.entries[i].local_installed as u8);
            buf.extend_from_slice(&self.entries[i].local_fp.to_le_bytes());
            i += 1;
        }
        buf.extend_from_slice(&self.delisted.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 呈现行：dl=<n> kept=<n> deny=<n>。
pub fn face_line(f: &StoreFace) -> String {
    let mut kept: u32 = 0;
    let mut i = 0usize;
    while i < f.traces.len() {
        if f.traces[i].1 {
            kept += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("dl=");
    out.push_str(&f.delisted.to_string());
    out.push_str(" kept=");
    out.push_str(&kept.to_string());
    out.push_str(" deny=");
    out.push_str(&f.new_install_denied.to_string());
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
    let mut g = Lcg::new(0x486_98);

    // R1 声明入册 + 上架 10 条。
    let mut face = StoreFace::new();
    face.declare()?;
    let mut fps: Vec<(u32, u64)> = vec![];
    let mut k: u32 = 1;
    while k <= DELIST_N {
        let fp = 0x1E00 + g.below(0x1000) as u64;
        face.shelve(k, fp)?;
        fps.push((k, fp));
        k += 1;
    }
    hits += 1;

    // R2 下架 ×10 本地保留。
    let mut k = 1u32;
    while k <= DELIST_N {
        face.delist(k)?;
        k += 1;
    }
    if face.delisted != DELIST_N {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R3 已装位不变。
    face.locals_intact(&fps)?;
    hits += 1;

    // R4 下架后新装拒绝（抽样 3 条）。
    let mut k = 1u32;
    while k <= 3 {
        match face.install_new(k) {
            Err(E_NEWINSTALL) => {}
            _ => return Err(E_NEWINSTALL),
        }
        k += 1;
    }
    if face.new_install_denied != 3 {
        return Err(E_NEWINSTALL);
    }
    hits += 1;

    // R5 已装用户不受扰：全账 local_installed 全真。
    let mut i = 0usize;
    while i < face.entries.len() {
        if !face.entries[i].local_installed {
            return Err(E_INSTALLED);
        }
        i += 1;
    }
    hits += 1;

    // R6 处置留痕：×10 笔、kept 全真、ID 对应。
    if face.traces.len() as u32 != DELIST_N {
        return Err(E_TRACE);
    }
    let mut i = 0usize;
    while i < face.traces.len() {
        if !face.traces[i].1 || face.traces[i].0 != i as u32 + 1 {
            return Err(E_TRACE);
        }
        i += 1;
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = face.fp();
    let f2 = face.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：dl=10 kept=10 deny=3。
    let line = face_line(&face);
    if !has_kw(line.as_bytes(), b"dl=10") || !has_kw(line.as_bytes(), b"kept=10")
        || !has_kw(line.as_bytes(), b"deny=3")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 计数守恒：留痕数 == 下架数。
    if face.traces.len() as u32 != face.delisted {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 声明冻结：重复下架拒 + 声明串冻结。
    match face.delist(2) {
        Err(E_DUP) => {}
        _ => return Err(E_DUP),
    }
    if DECREE != "delist-keeps-local" {
        return Err(E_DECL);
    }
    hits += 1;

    Ok(hits)
}
