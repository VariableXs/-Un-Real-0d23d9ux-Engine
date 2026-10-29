//! UNX-F48663 —— I·三体裁准确率账打包（J-2 账打包供 O5 消费）
//! 判据原文：UNX-F48663-J1 打包 ×1 字段齐、试产校准定标注；
//! 判据锚定 §AI-61 §5 J-2。
//! 判据落地（drill 场景）：R1 打包 ×1 生成；R2 三体全录；R3 bp 值域；
//! R4 字数下限；R5 校验和可复算；R6 缺体检出；R7 打包指纹；
//! R8 消费位就绪；R9 呈现行；R10 试产校准定标注。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：三体裁 = 诗/文/话（G_SHI/G_WEN/G_HUA），每体裁样张
//! 5,000 字冻结；准确率以基点（bp）计，10,000 = 100%；打包供 O5
//! 消费，字段齐与试产校准定标注为判据双锚，缺一即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0A40;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_BUILD: u16 = SEG_BASE + 1; // 打包生成失败
const E_GENRE: u16 = SEG_BASE + 2; // 体裁缺失或越界
const E_BP: u16 = SEG_BASE + 3; // 准确率越出 0..=10000bp
const E_CHARS: u16 = SEG_BASE + 4; // 字数不足 5000
const E_CSUM: u16 = SEG_BASE + 5; // 校验和复算不一致
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_CONSUME: u16 = SEG_BASE + 7; // 消费位未就绪
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_CALIB: u16 = SEG_BASE + 9; // 试产校准定标注缺失
const E_DUP: u16 = SEG_BASE + 10; // 体裁重复录入

/// 体裁闭集。
pub const G_SHI: u8 = 1; // 诗
pub const G_WEN: u8 = 2; // 文
pub const G_HUA: u8 = 3; // 话
/// J-2 账体裁数。
pub const GENRE_N: usize = 3;
/// 每体样张字数（冻结口径）。
pub const GENRE_CHARS: u32 = 5_000;
/// O5 消费版次。
pub const PACK_FOR_O5: u32 = 1;

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

/// 一条体裁准确率账目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GenreAcc {
    pub genre: u8,
    pub acc_bp: u32,   // 准确率（基点，10000 = 100%）
    pub chars: u32,    // 样张字数
}

impl GenreAcc {
    pub fn genre_ok(g: u8) -> bool {
        g == G_SHI || g == G_WEN || g == G_HUA
    }

    /// 账目域回归。
    pub fn range_ok(&self) -> Result<(), u16> {
        if !GenreAcc::genre_ok(self.genre) {
            return Err(E_GENRE);
        }
        if self.acc_bp > 10_000 {
            return Err(E_BP);
        }
        if self.chars < GENRE_CHARS {
            return Err(E_CHARS);
        }
        Ok(())
    }

    /// 序列化（校验和用）。
    pub fn bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = vec![];
        buf.push(self.genre);
        buf.extend_from_slice(&self.acc_bp.to_le_bytes());
        buf.extend_from_slice(&self.chars.to_le_bytes());
        buf
    }
}

/// J-2 三体裁准确率账打包。
#[derive(Clone)]
pub struct AccPack {
    pub rev: u32,
    pub entries: Vec<GenreAcc>,
    pub calib_pending: bool, // 试产校准定标注：true = 数值待试产校准
}

impl AccPack {
    pub fn new() -> AccPack {
        AccPack { rev: PACK_FOR_O5, entries: vec![], calib_pending: true }
    }

    /// 录入一账：体裁闭集 + 体裁唯一。
    pub fn put(&mut self, g: GenreAcc) -> Result<(), u16> {
        g.range_ok()?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].genre == g.genre {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.entries.push(g);
        Ok(())
    }

    /// 字段齐：三体全录。
    pub fn complete(&self) -> Result<(), u16> {
        if self.entries.len() != GENRE_N {
            return Err(E_GENRE);
        }
        let mut want = 1u8;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].genre != want {
                return Err(E_GENRE);
            }
            want += 1;
            i += 1;
        }
        Ok(())
    }

    /// 校验和：rev + 试产位 + 账目序列表 FNV。
    pub fn checksum(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.rev.to_le_bytes());
        buf.push(self.calib_pending as u8);
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }

    /// 消费位就绪：rev 冻结、三体齐、bp 全部非零。
    pub fn consume_ready(&self) -> Result<(), u16> {
        if self.rev != PACK_FOR_O5 {
            return Err(E_CONSUME);
        }
        self.complete()?;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].acc_bp == 0 {
                return Err(E_CONSUME);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 体裁中文名（呈现辅助）。
pub fn genre_name(g: u8) -> &'static str {
    if g == G_SHI {
        "shi"
    } else if g == G_WEN {
        "wen"
    } else {
        "hua"
    }
}

/// 单账体裁判定名（查表防漂移）。
pub fn genre_of(entry: &GenreAcc) -> &'static str {
    genre_name(entry.genre)
}

/// 构造 ×1 打包：确定性三体账录入。
pub fn build_pack(g: &mut Lcg) -> Result<AccPack, u16> {
    let mut p = AccPack::new();
    let mut k: u8 = 1;
    while k <= 3 {
        let bp = 9_000 + g.below(900);
        p.put(GenreAcc { genre: k, acc_bp: bp, chars: GENRE_CHARS })?;
        k += 1;
    }
    Ok(p)
}

/// 呈现行：g=<体数> rev=<版次> cs=<校验和低 32 位> calib=<0|1>。
pub fn pack_line(p: &AccPack) -> String {
    let cs = p.checksum() as u32;
    let mut out = String::new();
    out.push_str("g=");
    out.push_str(&p.entries.len().to_string());
    out.push_str(" rev=");
    out.push_str(&p.rev.to_string());
    out.push_str(" cs=");
    out.push_str(&cs.to_string());
    out.push_str(" calib=");
    out.push_str(&(p.calib_pending as u32).to_string());
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
    let mut g = Lcg::new(0x486_63);

    // R1 打包 ×1 生成。
    let pack = build_pack(&mut g)?;
    if pack.entries.is_empty() {
        return Err(E_BUILD);
    }
    hits += 1;

    // R2 三体全录：诗/文/话按序齐备。
    // 录入序即消费序：O5 按体裁闭集顺序读取，不重排。
    pack.complete()?;
    if genre_of(&pack.entries[0]) != "shi" || genre_of(&pack.entries[1]) != "wen"
        || genre_of(&pack.entries[2]) != "hua"
    {
        return Err(E_GENRE);
    }
    hits += 1;

    // R3 bp 值域：逐账 0..=10000。
    let mut i = 0usize;
    while i < pack.entries.len() {
        if pack.entries[i].acc_bp > 10_000 {
            return Err(E_BP);
        }
        i += 1;
    }
    hits += 1;

    // R4 字数下限：逐账 ≥5000。
    let mut i = 0usize;
    while i < pack.entries.len() {
        if pack.entries[i].chars < GENRE_CHARS {
            return Err(E_CHARS);
        }
        i += 1;
    }
    hits += 1;

    // R5 校验和可复算：两次一致且非零。
    let c1 = pack.checksum();
    let c2 = pack.checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_CSUM);
    }
    hits += 1;

    // R6 缺体检出：删一体后 complete 红；缺 bp 越域红。
    let mut thin = pack.clone();
    thin.entries.remove(2);
    match thin.complete() {
        Err(E_GENRE) => {}
        _ => return Err(E_GENRE),
    }
    let bad = GenreAcc { genre: G_WEN, acc_bp: 12_000, chars: GENRE_CHARS };
    match bad.range_ok() {
        Err(E_BP) => hits += 1,
        _ => return Err(E_BP),
    }

    // R7 打包指纹：同账两次指纹一致。
    let f1 = pack.checksum();
    let f2 = pack.checksum();
    if f1 != f2 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 消费位就绪：rev 冻结、三体齐、bp 非零。
    // 消费位是 O5 读取的唯一入口：rev 漂移或体缺即拒收。
    pack.consume_ready()?;
    hits += 1;

    // R9 呈现行：g=3 rev=1 calib=1。
    let line = pack_line(&pack);
    if !has_kw(line.as_bytes(), b"g=3") || !has_kw(line.as_bytes(), b"rev=1")
        || !has_kw(line.as_bytes(), b"calib=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 试产校准定标注：calib_pending 为真（阈值未冻结如实标注）。
    if !pack.calib_pending {
        return Err(E_CALIB);
    }
    // 体裁重复录入拒收。
    let mut probe = AccPack::new();
    probe.put(GenreAcc { genre: G_SHI, acc_bp: 9_500, chars: GENRE_CHARS })?;
    match probe.put(GenreAcc { genre: G_SHI, acc_bp: 9_600, chars: GENRE_CHARS }) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    Ok(hits)
}
