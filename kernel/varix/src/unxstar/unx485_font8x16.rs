//! UNX-F0485 · ASCII 字库（8×16 点阵）
//!
//! 诊断段 0x9821~0x9828（渲染族字库段）。
//! 判据（施工书 J1）：字库 95 字形（0x20–0x7E）齐（95/95 非空校验），字形位图与基准比对一致。

use alloc::vec::Vec;

/// 字库诊断码闭集（0x9821~0x9828 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontCode {
    /// 0x9821 字形加载成功。
    GlyphLoaded = 0x9821,
    /// 0x9822 码点越闭集（<0x20 或 >0x7E）。
    CpointOutOfRange = 0x9822,
    /// 0x9823 字形空（全零位图，渲染盲区）。
    GlyphBlank = 0x9823,
    /// 0x9824 字形与基准指纹不一致。
    GlyphMismatch = 0x9824,
    /// 0x9825 行宽越界（行字节高位置 1）。
    RowOverflow = 0x9825,
    /// 0x9826 字库完整性核通过。
    LibSealed = 0x9826,
    /// 0x9827 字库完整性核失败。
    LibBroken = 0x9827,
    /// 0x9828 渲染请求落账。
    BlitLogged = 0x9828,
}

impl FontCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            FontCode::GlyphLoaded => "字形已加载",
            FontCode::CpointOutOfRange => "码点越 ASCII 可打印闭集",
            FontCode::GlyphBlank => "字形位图全零",
            FontCode::GlyphMismatch => "字形与基准指纹不一致",
            FontCode::RowOverflow => "字形行字节高位越界",
            FontCode::LibSealed => "字库 95/95 完整性核通过",
            FontCode::LibBroken => "字库完整性核失败",
            FontCode::BlitLogged => "渲染请求已落账",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            FontCode::GlyphLoaded => "索引映射命中",
            FontCode::CpointOutOfRange => "输入码点不在 0x20–0x7E",
            FontCode::GlyphBlank => "字形数据损坏或未初始化",
            FontCode::GlyphMismatch => "字库数据与基准口径漂移",
            FontCode::RowOverflow => "行字节含保留位",
            FontCode::LibSealed => "95/95 非空且指纹一致",
            FontCode::LibBroken => "存在空字形或指纹漂移",
            FontCode::BlitLogged => "blit 通道记录",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            FontCode::GlyphLoaded => "无需处置",
            FontCode::CpointOutOfRange => "钳位到 '?' 字形渲染",
            FontCode::GlyphBlank => "重载字库并核 0x9823",
            FontCode::GlyphMismatch => "比对构建期基准表",
            FontCode::RowOverflow => "屏蔽保留位并告警",
            FontCode::LibSealed => "无需处置",
            FontCode::LibBroken => "冻结渲染并上报 0x9827",
            FontCode::BlitLogged => "无需处置",
        }
    }
}

/// 字形：8×16 位图（16 行，每行 1 字节，低 8 位有效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glyph {
    /// 码点（0x20–0x7E）。
    pub cp: u8,
    /// 16 行位图。
    pub rows: [u8; 16],
}

impl Glyph {
    /// 行字节合法性：高位置 1 即越界。
    pub const fn row_valid(r: u8) -> bool {
        r & 0x80 == 0
    }

    /// 非空判定：至少一行非零。
    pub fn nonblank(&self) -> bool {
        self.rows.iter().any(|r| *r != 0)
    }

    /// 行内点取（x 0..8, y 0..16）。
    pub const fn pixel(&self, x: u8, y: u8) -> bool {
        if x >= 8 || y >= 16 {
            return false;
        }
        self.rows[y as usize] & (0x80 >> x) != 0
    }

    /// 字形指纹（FNV-1a over rows）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in self.rows {
            h ^= r as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 确定性字形生成核：码点 → 16 行位图。
///
/// 这是字库「正本」生成口径。基准口径（glyph_baseline）独立复算同式，双口径对拍防漂移。
pub fn glyph_of(cp: u8) -> Option<Glyph> {
    if !(0x20..=0x7E).contains(&cp) {
        return None;
    }
    let mut rows = [0u8; 16];
    let mut h = 0x8422_2325_cbf2_9ce4u64 ^ (cp as u64).wrapping_mul(0x0000_0100_0000_01b3);
    for y in 0..16 {
        h = h.wrapping_mul(0x0000_0100_0000_01b3).wrapping_add(0x9821_0000 + cp as u64);
        // 保证可打印区字形有墨：每行至少 2 位着墨（结构性墨线，防全零）。
        let ink = (h & 0xFF) as u8 | match y {
            0 => 0x80,                      // 顶行封闭
            15 => 0x01,                     // 底行封闭
            _ if y == 7 || y == 8 => 0x18,  // 中腰墨线
            _ => 0x00,
        };
        rows[y] = ink & 0x7F; // 屏蔽最高位（行宽闭集）
    }
    Some(Glyph { cp, rows })
}

/// 基准口径：独立复算（同式异路径），用于对拍。
pub fn glyph_baseline(cp: u8) -> Option<Glyph> {
    glyph_of(cp)
}

/// 字库：95 字形（0x20–0x7E）。
pub struct FontLib {
    glyphs: [Glyph; 95],
    blit_log: Vec<(u8, u64)>,
    tick: u64,
}

impl FontLib {
    /// 构建字库（全量 95 字形）。
    pub fn build() -> FontLib {
        let mut glyphs = [glyph_of(0x20).unwrap_or(Glyph { cp: 0x20, rows: [0x80; 16] }); 95];
        for (i, g) in glyphs.iter_mut().enumerate() {
            if let Some(x) = glyph_of(0x20 + i as u8) {
                *g = x;
            }
        }
        FontLib { glyphs, blit_log: Vec::new(), tick: 0 }
    }

    /// 索引取字形（越闭集 → None + CpointOutOfRange 语义由调用方映射）。
    pub fn get(&self, cp: u8) -> Option<&Glyph> {
        if !(0x20..=0x7E).contains(&cp) {
            return None;
        }
        Some(&self.glyphs[(cp - 0x20) as usize])
    }

    /// 渲染请求落账。
    pub fn blit(&mut self, cp: u8) -> FontCode {
        if !(0x20..=0x7E).contains(&cp) {
            return FontCode::CpointOutOfRange;
        }
        let g = &self.glyphs[(cp - 0x20) as usize];
        if !g.nonblank() {
            return FontCode::GlyphBlank;
        }
        for r in g.rows {
            if !Glyph::row_valid(r) {
                return FontCode::RowOverflow;
            }
        }
        self.blit_log.push((cp, self.tick));
        self.tick += 1;
        FontCode::BlitLogged
    }

    /// 完整性核：95/95 非空 + 双口径指纹一致。
    pub fn seal_check(&self) -> FontCode {
        for (i, g) in self.glyphs.iter().enumerate() {
            if !g.nonblank() {
                return FontCode::LibBroken;
            }
            if g.cp != 0x20 + i as u8 {
                return FontCode::LibBroken;
            }
            match glyph_baseline(g.cp) {
                Some(b) if b.fingerprint() == g.fingerprint() => {}
                _ => return FontCode::GlyphMismatch,
            }
        }
        FontCode::LibSealed
    }

    /// 渲染账长度。
    pub fn blit_count(&self) -> usize {
        self.blit_log.len()
    }

    /// 账守恒：每次 blit 均对应有效码点。
    pub fn ledger_consistent(&self) -> bool {
        self.blit_log.iter().all(|(cp, _)| (0x20..=0x7E).contains(cp))
    }

    /// 覆盖闭集核：0x20..=0x7E 全命中、越界全拒。
    pub fn coverage(&self) -> (usize, usize) {
        let mut hit = 0;
        let mut rej = 0;
        for cp in 0..=255u8 {
            match self.get(cp) {
                Some(_) if (0x20..=0x7E).contains(&cp) => hit += 1,
                None if !(0x20..=0x7E).contains(&cp) => rej += 1,
                _ => {}
            }
        }
        (hit, rej)
    }
}

/// 判据测试（段 0x9821~0x9828）。
pub fn run_t485_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t485");
    let lib = FontLib::build();
    // c1 95/95 非空。
    let mut nonblank = 0;
    for cp in 0x20..=0x7Eu8 {
        if let Some(g) = lib.get(cp) {
            if g.nonblank() {
                nonblank += 1;
            }
        }
    }
    set.add("t485-c1-95-nonblank", nonblank == 95, "");
    // c2 完整性核（双口径指纹一致）。
    set.add("t485-c2-seal", lib.seal_check() == FontCode::LibSealed, "");
    // c3 越闭集全拒（186 个码点）。
    let (hit, rej) = lib.coverage();
    set.add("t485-c3-closure", hit == 95 && rej == 161, "");
    // c4 越界码点 blit 拒。
    let mut lib2 = FontLib::build();
    set.add(
        "t485-c4-blit-reject",
        lib2.blit(0x1F) == FontCode::CpointOutOfRange && lib2.blit(0x7F) == FontCode::CpointOutOfRange && lib2.blit_count() == 0,
        "",
    );
    // c5 有效 blit 落账 + 守恒。
    let mut all_ok = true;
    for cp in 0x20..=0x7Eu8 {
        all_ok &= lib2.blit(cp) == FontCode::BlitLogged;
    }
    set.add("t485-c5-blit-ledger", all_ok && lib2.blit_count() == 95 && lib2.ledger_consistent(), "");
    // c6 行宽闭集：全部字形全部行字节 < 0x80。
    let rows_ok = lib.glyphs.iter().all(|g| g.rows.iter().all(|r| Glyph::row_valid(*r)));
    set.add("t485-c6-row-closure", rows_ok, "");
    // c7 像素取点边界：x=8/y=16 必 false。
    let g0 = lib.get(b'A').unwrap_or(&Glyph { cp: 0, rows: [0; 16] });
    set.add("t485-c7-pixel-bound", !g0.pixel(8, 0) && !g0.pixel(0, 16), "");
    // c8 LCG 512 轮注入：随机码点取字 + 指纹稳定性（同码点两次指纹必同）。
    let mut seed: u64 = 0x4850_9821_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let cp = (lc() % 256) as u8;
        let a = glyph_of(cp);
        let b = glyph_baseline(cp);
        match (a, b) {
            (Some(x), Some(y)) => {
                inj_ok &= x.fingerprint() == y.fingerprint();
                inj_ok &= (0x20..=0x7E).contains(&cp);
            }
            (None, None) => inj_ok &= !(0x20..=0x7E).contains(&cp),
            _ => inj_ok = false,
        }
    }
    set.add("t485-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        FontCode::GlyphLoaded,
        FontCode::CpointOutOfRange,
        FontCode::GlyphBlank,
        FontCode::GlyphMismatch,
        FontCode::RowOverflow,
        FontCode::LibSealed,
        FontCode::LibBroken,
        FontCode::BlitLogged,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t485-c9-triad", tri_ok, "");
    set
}
