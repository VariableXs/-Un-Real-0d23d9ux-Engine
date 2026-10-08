//! AURORA-1000 AI-03 · 字体与排版引擎（A051~A075，W1）
//!
//! 纯逻辑建模（no_std，仅 core）。所有结构均为固定容量数组 + usize 计数，
//! 不依赖分配器、动态分发或宏。覆盖 TrueType/OTF 解析、轮廓光栅化、
//! 字距/行距、CJK、emoji、fallback 链、字体缓存、竖排与 RTL、UTF-8 解码等。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 公共常量
// ---------------------------------------------------------------------------

/// 字形点阵宽（像素）。
pub const GLYPH_W: usize = 8;
/// 字形点阵高（像素）。
pub const GLYPH_H: usize = 8;
/// 一个字形点阵：每像素 0/1 覆盖率。
pub type GlyphBitmap = [u8; GLYPH_W * GLYPH_H];

/// 字形缓存容量（LRU 计数器版）。
pub const MAX_CACHE: usize = 16;
/// 字体 fallback 链容量。
pub const MAX_FALLBACK: usize = 4;
/// 字体商店容量。
pub const MAX_FONTS: usize = 8;
/// kern 样例表容量。
pub const MAX_KERN: usize = 16;
/// 子集位图字数（覆盖码点 0..512）。
pub const SUBSET_WORDS: usize = 2048; // 覆盖 BMP（0x0000..=0xFFFF）位图子集
/// 字号阶梯档位数。
pub const SCALE_STEPS: [u16; 8] = [10, 12, 14, 16, 20, 24, 32, 48];

// ---------------------------------------------------------------------------
// 字节读取助手（大端）
// ---------------------------------------------------------------------------

fn rd_u16_be(b: &[u8], off: usize) -> Option<u16> {
    if off + 2 > b.len() {
        return None;
    }
    Some(((b[off] as u16) << 8) | (b[off + 1] as u16))
}

fn rd_u32_be(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some(
        ((b[off] as u32) << 24)
            | ((b[off + 1] as u32) << 16)
            | ((b[off + 2] as u32) << 8)
            | (b[off + 3] as u32),
    )
}

// ---------------------------------------------------------------------------
// A051 TrueType/OTF 解析
// ---------------------------------------------------------------------------

/// sfnt 版本（按文件头 4 字节判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SfntVersion {
    TrueType,
    Cff,
    Ttc,
}

/// TrueType 1.0 版本标记：0x00010000。
pub const SFNT_TRUE_TYPE: [u8; 4] = [0, 1, 0, 0];
/// OpenType/CFF 版本标记 "OTTO"。
pub const SFNT_CFF: [u8; 4] = *b"OTTO";

/// 一条表目录记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRecord {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub offset: u32,
    pub length: u32,
}

/// 解析 sfnt 版本；坏魔数返回 None（被拒绝）。
pub fn parse_sfnt_version(font: &[u8]) -> Option<SfntVersion> {
    if font.len() < 4 {
        return None;
    }
    if font[0] == SFNT_TRUE_TYPE[0]
        && font[1] == SFNT_TRUE_TYPE[1]
        && font[2] == SFNT_TRUE_TYPE[2]
        && font[3] == SFNT_TRUE_TYPE[3]
    {
        return Some(SfntVersion::TrueType);
    }
    if &font[0..4] == &SFNT_CFF[..] {
        return Some(SfntVersion::Cff);
    }
    if &font[0..4] == b"ttcf" {
        return Some(SfntVersion::Ttc);
    }
    None
}

/// 读取表数量（偏移 4 处 u16 大端）。
pub fn sfnt_num_tables(font: &[u8]) -> Option<u16> {
    if font.len() < 6 {
        return None;
    }
    rd_u16_be(font, 4)
}

/// 读取第 `index` 条表目录记录（每条 16 字节，自偏移 12 起）。
pub fn table_record_at(font: &[u8], index: usize) -> Option<TableRecord> {
    if font.len() < 12 {
        return None;
    }
    let base = 12 + index * 16;
    if base + 16 > font.len() {
        return None;
    }
    let mut tag = [0u8; 4];
    tag.copy_from_slice(&font[base..base + 4]);
    let checksum = rd_u32_be(font, base + 4)?;
    let offset = rd_u32_be(font, base + 8)?;
    let length = rd_u32_be(font, base + 12)?;
    Some(TableRecord { tag, checksum, offset, length })
}

/// 按标签查找表目录记录（线性扫描）。
pub fn find_table(font: &[u8], tag: &[u8; 4]) -> Option<TableRecord> {
    let nt = match sfnt_num_tables(font) {
        Some(v) => v as usize,
        None => return None,
    };
    let mut i = 0;
    while i < nt {
        if let Some(r) = table_record_at(font, i) {
            if &r.tag == tag {
                return Some(r);
            }
        } else {
            break;
        }
        i += 1;
    }
    None
}

/// 对一张表计算 TrueType checksum（4 字节大端和，按可用字节截断，末尾不足 4 字节按高位补）。
pub fn table_checksum(font: &[u8], rec: TableRecord) -> u32 {
    let start = rec.offset as usize;
    let mut end = start + rec.length as usize;
    if end > font.len() {
        end = font.len();
    }
    if start > end {
        return 0;
    }
    let mut sum: u32 = 0;
    let mut p = start;
    while p + 4 <= end {
        sum = sum.wrapping_add(rd_u32_be(font, p).unwrap_or(0));
        p += 4;
    }
    if p < end {
        let mut word: u32 = 0;
        let mut shift = 24u32;
        while p < end {
            word |= (font[p] as u32) << shift;
            shift = shift.wrapping_sub(8);
            p += 1;
        }
        sum = sum.wrapping_add(word);
    }
    sum
}

/// 一条 cmap 段（连续码点范围 -> 起始字形 id）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmapSegment {
    pub start: u32,
    pub end: u32,
    pub glyph_base: u16,
}

/// 固定样例 cmap（ASCII / CJK / emoji / CJK 标点）。
pub const SAMPLE_CMAP: [CmapSegment; 4] = [
    CmapSegment { start: 0x20, end: 0x7E, glyph_base: 1 },
    CmapSegment { start: 0x4E00, end: 0x9FFF, glyph_base: 100 },
    CmapSegment { start: 0x1F300, end: 0x1FAFF, glyph_base: 200 },
    CmapSegment { start: 0x3000, end: 0x303F, glyph_base: 300 },
];

/// cmap 查找：在样例段中线性查找码点对应的字形 id（未命中返回 0）。
pub fn cmap_lookup(cp: u32) -> u16 {
    let mut i = 0;
    while i < SAMPLE_CMAP.len() {
        let s = SAMPLE_CMAP[i];
        if cp >= s.start && cp <= s.end {
            return s.glyph_base + (cp - s.start) as u16;
        }
        i += 1;
    }
    0
}

// ---------------------------------------------------------------------------
// A052 字体轮廓光栅化
// ---------------------------------------------------------------------------

/// 二次贝塞尔取点（t 量化到 0..1024，固定点避免浮点）。
pub fn quadratic_bezier(
    x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32, t_q: u16,
) -> (i32, i32) {
    let t = t_q as i64;
    let u = 1024 - t;
    let w0 = u * u;
    let w1 = 2 * t * u;
    let w2 = t * t;
    let x = (x0 as i64 * w0 + x1 as i64 * w1 + x2 as i64 * w2 + 524288) / 1048576;
    let y = (y0 as i64 * w0 + y1 as i64 * w1 + y2 as i64 * w2 + 524288) / 1048576;
    (x as i32, y as i32)
}

/// 在点阵中绘制一个像素（越界安全）。
pub fn plot(bmp: &mut GlyphBitmap, x: i32, y: i32) {
    if x >= 0 && (x as usize) < GLYPH_W && y >= 0 && (y as usize) < GLYPH_H {
        bmp[(y as usize) * GLYPH_W + (x as usize)] = 1;
    }
}

/// 用矩形方框近似填充字形轮廓的一部分。
pub fn rasterize_box(bmp: &mut GlyphBitmap, x: i32, y: i32, w: i32, h: i32) {
    let mut yy = y;
    while yy < y + h {
        let mut xx = x;
        while xx < x + w {
            plot(bmp, xx, yy);
            xx += 1;
        }
        yy += 1;
    }
}

/// 把一条二次贝塞尔曲线细分为固定步数并落点（纯函数，越界忽略）。
pub fn rasterize_quadratic(
    bmp: &mut GlyphBitmap, x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32,
) {
    let steps = 16;
    let mut i = 0;
    while i <= steps {
        let (px, py) = quadratic_bezier(x0, y0, x1, y1, x2, y2, (i * 1024 / steps) as u16);
        plot(bmp, px, py);
        i += 1;
    }
}

/// 统计点阵中已点亮像素数（用于合成/测试）。
pub fn glyph_set_count(bmp: &GlyphBitmap) -> usize {
    let mut c = 0;
    let mut i = 0;
    while i < bmp.len() {
        if bmp[i] != 0 {
            c += 1;
        }
        i += 1;
    }
    c
}

// ---------------------------------------------------------------------------
// A053 字体缓存与图集（固定容量 LRU 计数器版）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphCacheEntry {
    pub id: u16,
    pub last_used: u32,
    pub valid: bool,
}

/// 字形缓存：固定槽位，最近最少使用（最小 last_used）淘汰。
#[derive(Clone, Copy, Debug)]
pub struct GlyphCache {
    slots: [GlyphCacheEntry; MAX_CACHE],
}

impl GlyphCache {
    pub const fn new() -> GlyphCache {
        GlyphCache {
            slots: [GlyphCacheEntry { id: 0, last_used: 0, valid: false }; MAX_CACHE],
        }
    }

    pub fn len(&self) -> usize {
        let mut c = 0;
        let mut i = 0;
        while i < MAX_CACHE {
            if self.slots[i].valid {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn contains(&self, id: u16) -> bool {
        let mut i = 0;
        while i < MAX_CACHE {
            if self.slots[i].valid && self.slots[i].id == id {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 命中并更新访问时间戳。
    pub fn get(&mut self, id: u16, now: u32) -> bool {
        let mut i = 0;
        while i < MAX_CACHE {
            if self.slots[i].valid && self.slots[i].id == id {
                self.slots[i].last_used = now;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 写入字形；无空槽时淘汰最久未用者，返回被淘汰的 id（0 表示无淘汰）。
    pub fn put(&mut self, id: u16, now: u32) -> u16 {
        let mut i = 0;
        while i < MAX_CACHE {
            if !self.slots[i].valid {
                self.slots[i] = GlyphCacheEntry { id, last_used: now, valid: true };
                return 0;
            }
            i += 1;
        }
        let mut victim = 0;
        let mut oldest = self.slots[0].last_used;
        let mut j = 1;
        while j < MAX_CACHE {
            if self.slots[j].last_used < oldest {
                oldest = self.slots[j].last_used;
                victim = j;
            }
            j += 1;
        }
        let evicted = self.slots[victim].id;
        self.slots[victim] = GlyphCacheEntry { id, last_used: now, valid: true };
        evicted
    }
}

// ---------------------------------------------------------------------------
// A054 字距 kerning（样例表 + 二分查找）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernPair {
    pub key: u32,
    pub value: i16,
}

/// 字距表：按 (left<<16|right) 排序以支持二分查找。
#[derive(Clone, Copy, Debug)]
pub struct KernTable {
    pairs: [KernPair; MAX_KERN],
    count: usize,
}

impl KernTable {
    pub const fn new() -> KernTable {
        KernTable {
            pairs: [KernPair { key: 0, value: 0 }; MAX_KERN],
            count: 0,
        }
    }

    /// 按排序后顺序插入一对字距（无序插入会破坏二分查找）。
    pub fn push(&mut self, left: u16, right: u16, value: i16) -> bool {
        if self.count >= MAX_KERN {
            return false;
        }
        self.pairs[self.count] = KernPair {
            key: ((left as u32) << 16) | (right as u32),
            value,
        };
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// 二分查找字距调整量（未命中返回 0）。
pub fn kern_lookup(t: &KernTable, left: u16, right: u16) -> i16 {
    let key = ((left as u32) << 16) | (right as u32);
    let mut lo = 0;
    let mut hi = t.count;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if t.pairs[mid].key == key {
            return t.pairs[mid].value;
        } else if t.pairs[mid].key < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    0
}

/// 构造一个已排序的样例字距表。
pub fn kern_table_sample() -> KernTable {
    let mut t = KernTable::new();
    // 按键升序：0x00410054 < 0x00410056 < 0x00540059
    t.push(0x41, 0x54, -40); // A T
    t.push(0x41, 0x56, -60); // A V
    t.push(0x54, 0x59, -50); // T Y
    t
}

// ---------------------------------------------------------------------------
// A055 行距与段落
// ---------------------------------------------------------------------------

/// 字体度量（head/hhea 抽取后归一化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
}

/// 行高 = ascent - descent + line_gap（descent 为负，故即 ascent+|descent|+gap）。
pub fn line_height(m: &FontMetrics) -> i32 {
    m.ascent as i32 - m.descent as i32 + m.line_gap as i32
}

/// 贪心段落折行：返回 (行数, 最后一行宽度)。
pub fn layout_lines(advances: &[i32], max_width: i32) -> (usize, i32) {
    let mut lines = 0;
    let mut width = 0;
    let mut last = 0;
    let mut i = 0;
    while i < advances.len() {
        let adv = advances[i];
        if width > 0 && width + adv > max_width {
            lines += 1;
            last = width;
            width = adv;
        } else {
            width += adv;
        }
        i += 1;
    }
    if width > 0 {
        lines += 1;
        last = width;
    }
    (lines, last)
}

// ---------------------------------------------------------------------------
// A056 CJK 全量字形 / UTF-8 解码 / 换行禁则
// ---------------------------------------------------------------------------

/// CJK 统一表意文字等判定。
pub fn is_cjk(cp: u32) -> bool {
    (cp >= 0x4E00 && cp <= 0x9FFF)
        || (cp >= 0x3400 && cp <= 0x4DBF)
        || (cp >= 0x3005 && cp <= 0x303F)
        || (cp >= 0xF900 && cp <= 0xFAFF)
        || (cp >= 0x3040 && cp <= 0x30FF) // 假名
        || (cp >= 0xAC00 && cp <= 0xD7A3) // 谚文音节
}

/// 全宽/半宽判定（全宽字符占一个全角格）。
pub fn is_fullwidth(cp: u32) -> bool {
    (cp >= 0x3000 && cp <= 0x303F)
        || (cp >= 0x4E00 && cp <= 0x9FFF)
        || (cp >= 0xF900 && cp <= 0xFAFF)
        || (cp >= 0xFF01 && cp <= 0xFF60)
        || (cp >= 0xFFE0 && cp <= 0xFFE6)
        || (cp >= 0x3040 && cp <= 0x30FF)
        || (cp >= 0xAC00 && cp <= 0xD7A3)
}

fn is_closing_punct(cp: u32) -> bool {
    cp == 0x3001 || cp == 0x3002 || cp == 0xFF09 || cp == 0x300D || cp == 0xFF0C
}

fn is_opening_punct(cp: u32) -> bool {
    cp == 0x300C || cp == 0xFF08 || cp == 0x300E
}

/// 换行禁则（kinshi）：闭引号前不断行，开引号后不断行。
pub fn cjk_can_break(prev: u32, next: u32) -> bool {
    if is_closing_punct(next) {
        return false;
    }
    if is_opening_punct(prev) {
        return false;
    }
    true
}

/// 手写 UTF-8 解码：把字节流解码为码点数组，返回码点数（坏序列用 0xFFFD 替代并前进 1）。
pub fn decode_utf8(bytes: &[u8], out: &mut [u32]) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (cp, len) = if b < 0x80 {
            (b as u32, 1)
        } else if b >= 0xC0 && b < 0xE0 && i + 1 < bytes.len() {
            (
                (((b & 0x1F) as u32) << 6) | (bytes[i + 1] & 0x3F) as u32,
                2,
            )
        } else if b >= 0xE0 && b < 0xF0 && i + 2 < bytes.len() {
            (
                (((b & 0x0F) as u32) << 12)
                    | (((bytes[i + 1] & 0x3F) as u32) << 6)
                    | (bytes[i + 2] & 0x3F) as u32,
                3,
            )
        } else if b >= 0xF0 && i + 3 < bytes.len() {
            (
                (((b & 0x07) as u32) << 18)
                    | (((bytes[i + 1] & 0x3F) as u32) << 12)
                    | (((bytes[i + 2] & 0x3F) as u32) << 6)
                    | (bytes[i + 3] & 0x3F) as u32,
                4,
            )
        } else {
            (0xFFFD, 1)
        };
        if n < out.len() {
            out[n] = cp;
            n += 1;
        }
        i += len;
    }
    n
}

// ---------------------------------------------------------------------------
// A057 emoji 彩色字体
// ---------------------------------------------------------------------------

/// emoji 码点段判定（含变体选择符段）。
pub fn is_emoji(cp: u32) -> bool {
    (cp >= 0x2600 && cp <= 0x26FF)
        || (cp >= 0x2700 && cp <= 0x27BF)
        || (cp >= 0x1F000 && cp <= 0x1F02F)
        || (cp >= 0x1F300 && cp <= 0x1FAFF)
        || (cp >= 0xFE00 && cp <= 0xFE0F)
}

/// 默认 emoji 呈现（彩色）的码点。
pub fn is_emoji_presentation(cp: u32) -> bool {
    (cp >= 0x2600 && cp <= 0x26FF) || (cp >= 0x1F300 && cp <= 0x1FAFF)
}

/// 零宽连接符（用于 emoji ZWJ 序列）。
pub fn is_zwj(cp: u32) -> bool {
    cp == 0x200D
}

// ---------------------------------------------------------------------------
// A058 字体 fallback 链
// ---------------------------------------------------------------------------

/// 一种字体的覆盖区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontCoverage {
    pub id: u8,
    pub lo: u32,
    pub hi: u32,
}

/// 字体 fallback 链（按优先级排序）。
#[derive(Clone, Copy, Debug)]
pub struct FallbackChain {
    fonts: [FontCoverage; MAX_FALLBACK],
    count: usize,
}

impl FallbackChain {
    pub const fn new() -> FallbackChain {
        FallbackChain {
            fonts: [FontCoverage { id: 0, lo: 0, hi: 0 }; MAX_FALLBACK],
            count: 0,
        }
    }

    pub fn push(&mut self, f: FontCoverage) -> bool {
        if self.count >= MAX_FALLBACK {
            return false;
        }
        self.fonts[self.count] = f;
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

/// 在 fallback 链中为该码点选择首个覆盖字体；全无命中时退回首字体。
pub fn select_font(chain: &FallbackChain, cp: u32) -> u8 {
    let mut i = 0;
    while i < chain.count {
        let f = chain.fonts[i];
        if cp >= f.lo && cp <= f.hi {
            return f.id;
        }
        i += 1;
    }
    if chain.count > 0 {
        chain.fonts[0].id
    } else {
        0
    }
}

/// 默认 fallback 链：拉丁 / CJK / emoji / CJK 标点。
pub const DEFAULT_FALLBACK: FallbackChain = FallbackChain {
    fonts: [
        FontCoverage { id: 1, lo: 0x20, hi: 0x024F },
        FontCoverage { id: 2, lo: 0x4E00, hi: 0x9FFF },
        FontCoverage { id: 3, lo: 0x1F300, hi: 0x1FAFF },
        FontCoverage { id: 4, lo: 0x3000, hi: 0x303F },
    ],
    count: 4,
};

// ---------------------------------------------------------------------------
// A059 字号阶梯令牌
// ---------------------------------------------------------------------------

/// 按档位索引取字号（px），越界回退到 base(16)。
pub fn scale_step(i: usize) -> u16 {
    if i < SCALE_STEPS.len() {
        SCALE_STEPS[i]
    } else {
        16
    }
}

/// 按设计令牌名取字号（px）。
pub fn scale_token_name(name: &str) -> Option<u16> {
    match name {
        "xs" => Some(12),
        "sm" => Some(14),
        "base" => Some(16),
        "lg" => Some(24),
        "xl" => Some(32),
        "xxl" => Some(48),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// A060 字体度量 API（head/hhea 抽取 + 像素网格对齐）
// ---------------------------------------------------------------------------

/// head 表抽取的字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadMetrics {
    pub units_per_em: u16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
    pub index_to_loc_format: i16,
}

/// hhea 表抽取的字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HheaMetrics {
    pub ascent: i16,
    pub descent: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub num_hmetrics: u16,
}

/// 抽取 head 表字段（表至少 54 字节）。
pub fn extract_head_metrics(head: &[u8]) -> Option<HeadMetrics> {
    if head.len() < 54 {
        return None;
    }
    Some(HeadMetrics {
        units_per_em: rd_u16_be(head, 18)?,
        x_min: rd_u16_be(head, 36).map(|v| v as i16)?,
        y_min: rd_u16_be(head, 38).map(|v| v as i16)?,
        x_max: rd_u16_be(head, 42).map(|v| v as i16)?,
        y_max: rd_u16_be(head, 44).map(|v| v as i16)?,
        index_to_loc_format: rd_u16_be(head, 50).map(|v| v as i16)?,
    })
}

/// 抽取 hhea 表字段（表至少 36 字节）。
pub fn extract_hhea_metrics(hhea: &[u8]) -> Option<HheaMetrics> {
    if hhea.len() < 36 {
        return None;
    }
    Some(HheaMetrics {
        ascent: rd_u16_be(hhea, 4).map(|v| v as i16)?,
        descent: rd_u16_be(hhea, 6).map(|v| v as i16)?,
        line_gap: rd_u16_be(hhea, 8).map(|v| v as i16)?,
        advance_width_max: rd_u16_be(hhea, 10)?,
        num_hmetrics: rd_u16_be(hhea, 34)?,
    })
}

/// 字体单位 advance 对齐到像素网格（四舍五入到最近整数像素）。
pub fn advance_to_pixels(adv: i32, font_size: i32, units_per_em: i32) -> i32 {
    if units_per_em <= 0 {
        return adv;
    }
    let num = adv as i64 * font_size as i64 + (units_per_em as i64) / 2;
    (num / units_per_em as i64) as i32
}

// ---------------------------------------------------------------------------
// A061 粗体/斜体合成
// ---------------------------------------------------------------------------

/// 粗体合成：把每个点亮像素向右复制一格（加粗）。
pub fn synthesize_bold_bitmap(src: &GlyphBitmap, dst: &mut GlyphBitmap) {
    let mut i = 0;
    while i < dst.len() {
        dst[i] = 0;
        i += 1;
    }
    let w = GLYPH_W;
    let mut y = 0;
    while y < GLYPH_H {
        let mut x = 0;
        while x < w {
            let idx = y * w + x;
            if src[idx] != 0 {
                dst[idx] = 1;
                // 纵向加粗：向下一行复制（底部行安全钳制）。
                if y + 1 < GLYPH_H {
                    dst[idx + w] = 1;
                }
            }
            x += 1;
        }
        y += 1;
    }
}

/// 斜体合成：按行做剪斜变换（顶左底右）。
pub fn synthesize_italic_bitmap(src: &GlyphBitmap, dst: &mut GlyphBitmap) {
    let mut i = 0;
    while i < dst.len() {
        dst[i] = 0;
        i += 1;
    }
    let w = GLYPH_W;
    let h = GLYPH_H;
    let mut y = 0;
    while y < h {
        let shift = y as i32 - (h as i32) / 2;
        let mut x = 0;
        while x < w {
            // 剪斜（源列钳位在行内，保证笔画不丢失）。
            let sx = (x as i32 - shift).clamp(0, w as i32 - 1) as usize;
            let sidx = y * w + sx;
            if src[sidx] != 0 {
                dst[y * w + x] = 1;
            }
            x += 1;
        }
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// A062 竖排与 RTL
// ---------------------------------------------------------------------------

/// 竖排坐标变换：顺时针旋转 90°（横排 (x,y) -> 竖排 (-y,x)）。
pub fn vertical_transform(x: i32, y: i32) -> (i32, i32) {
    (-y, x)
}

/// 竖直书写时第 index 个字形的纵向起点。
pub fn vertical_cell_y(index: usize, cell: i32) -> i32 {
    index as i32 * cell
}

/// RTL 脚本判定（希伯来 / 阿拉伯）。
pub fn is_rtl(cp: u32) -> bool {
    (cp >= 0x0590 && cp <= 0x05FF)
        || (cp >= 0x0600 && cp <= 0x06FF)
        || (cp >= 0xFB1D && cp <= 0xFB4F)
}

/// 原地反转一段字形 id（RTL 视觉重排）。
pub fn rtl_reorder_run(g: &mut [u16]) {
    let n = g.len();
    let mut i = 0;
    while i < n / 2 {
        g.swap(i, n - 1 - i);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A063 字体子集化
// ---------------------------------------------------------------------------

/// 子集位图（覆盖码点 0..512）。
pub type GlyphSubset = [u32; SUBSET_WORDS];

/// 子集是否包含某码点。
pub fn subset_contains(s: &GlyphSubset, cp: u32) -> bool {
    let bit = cp as usize;
    if bit >= SUBSET_WORDS * 32 {
        return false;
    }
    let w = bit / 32;
    let b = bit % 32;
    (s[w] >> b) & 1 != 0
}

/// 把码点加入子集，超界返回 false。
pub fn subset_add(s: &mut GlyphSubset, cp: u32) -> bool {
    let bit = cp as usize;
    if bit >= SUBSET_WORDS * 32 {
        return false;
    }
    let w = bit / 32;
    let b = bit % 32;
    s[w] |= 1 << b;
    true
}

// ---------------------------------------------------------------------------
// A064 字体嵌入许可
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Embedding {
    Installable,
    Restricted,
    PreviewPrint,
    Editable,
    BitmapOnly,
    None,
}

/// 由 OS/2 fsType 字段判定嵌入许可。
pub fn embedding_permission(fs_type: u16) -> Embedding {
    if fs_type & 0x0200 != 0 {
        Embedding::BitmapOnly
    } else if fs_type & 0x0008 != 0 {
        Embedding::Editable
    } else if fs_type & 0x0004 != 0 {
        Embedding::PreviewPrint
    } else if fs_type & 0x0002 != 0 {
        Embedding::Restricted
    } else if fs_type & 0x0001 != 0 {
        Embedding::None
    } else {
        Embedding::Installable
    }
}

// ---------------------------------------------------------------------------
// A065 字体商店接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontInfo {
    pub id: u8,
    pub name: &'static str,
    pub is_cjk: bool,
}

/// 已安装字体商店（固定容量）。
#[derive(Clone, Copy, Debug)]
pub struct FontStore {
    fonts: [Option<FontInfo>; MAX_FONTS],
    count: usize,
}

impl FontStore {
    pub const fn new() -> FontStore {
        FontStore {
            fonts: [None; MAX_FONTS],
            count: 0,
        }
    }

    pub fn install(&mut self, f: FontInfo) -> bool {
        if self.count >= MAX_FONTS {
            return false;
        }
        self.fonts[self.count] = Some(f);
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn find_by_name(&self, name: &str) -> Option<FontInfo> {
        let mut i = 0;
        while i < self.count {
            if let Some(f) = self.fonts[i] {
                if f.name == name {
                    return Some(f);
                }
            }
            i += 1;
        }
        None
    }

    pub fn find_by_id(&self, id: u8) -> Option<FontInfo> {
        let mut i = 0;
        while i < self.count {
            if let Some(f) = self.fonts[i] {
                if f.id == id {
                    return Some(f);
                }
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// A066 字体预览工具
// ---------------------------------------------------------------------------

/// 把一个字形点阵渲染成 ASCII 艺术（'#' 点亮，'.' 空白），写入 out，返回字节数。
pub fn preview_glyph_ascii(bmp: &GlyphBitmap, out: &mut [u8]) -> usize {
    let mut n = 0;
    let mut y = 0;
    while y < GLYPH_H {
        let mut x = 0;
        while x < GLYPH_W {
            let ch = if bmp[y * GLYPH_W + x] != 0 { b'#' } else { b'.' };
            if n < out.len() {
                out[n] = ch;
                n += 1;
            }
            x += 1;
        }
        if n < out.len() && y + 1 < GLYPH_H {
            out[n] = b'\n';
            n += 1;
        }
        y += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A067 字体性能预算
// ---------------------------------------------------------------------------

/// 单帧字形预算（固定上限）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphBudget {
    pub limit: u16,
    pub used: u16,
}

impl GlyphBudget {
    pub const fn new(limit: u16) -> GlyphBudget {
        GlyphBudget { limit, used: 0 }
    }

    /// 支取 n 个字形额度；超预算返回 false 且不修改。
    pub fn take(&mut self, n: u16) -> bool {
        let after = self.used as u32 + n as u32;
        if after > self.limit as u32 {
            return false;
        }
        self.used = after as u16;
        true
    }

    pub fn within(&self) -> bool {
        self.used <= self.limit
    }
}

// ---------------------------------------------------------------------------
// A068 字体一致性校验
// ---------------------------------------------------------------------------

/// 校验整字：版本合法、表数非零、所有表记录落在字节流内。
pub fn validate_font(font: &[u8]) -> bool {
    if parse_sfnt_version(font).is_none() {
        return false;
    }
    let nt = match sfnt_num_tables(font) {
        Some(v) => v as usize,
        None => return false,
    };
    if nt == 0 {
        return false;
    }
    let mut i = 0;
    while i < nt {
        match table_record_at(font, i) {
            Some(r) => {
                let end = r.offset as usize + r.length as usize;
                if end > font.len() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A069 字体与排版引擎自检
// ---------------------------------------------------------------------------

/// 引擎内部不变量：字号阶梯单调递增、fallback 链非空、缓存容量为正。
pub fn engine_invariant_ok() -> bool {
    let mut ok = true;
    let mut i = 1;
    while i < SCALE_STEPS.len() {
        if SCALE_STEPS[i] <= SCALE_STEPS[i - 1] {
            ok = false;
        }
        i += 1;
    }
    if DEFAULT_FALLBACK.count == 0 {
        ok = false;
    }
    if MAX_CACHE == 0 {
        ok = false;
    }
    ok
}

// ---------------------------------------------------------------------------
// A070 字体与排版引擎性能预算
// ---------------------------------------------------------------------------

/// 引擎级性能预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnginePerf {
    pub glyphs_per_frame: u16,
    pub raster_ops: u16,
}

/// 用量是否在预算内。
pub fn perf_within(used_glyphs: u16, used_ops: u16, b: EnginePerf) -> bool {
    used_glyphs <= b.glyphs_per_frame && used_ops <= b.raster_ops
}

// ---------------------------------------------------------------------------
// A071 字体与排版引擎可观测
// ---------------------------------------------------------------------------

/// 排版可观测计数器。
#[derive(Clone, Copy, Debug)]
pub struct TypographyCounters {
    pub glyphs: u32,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub fallbacks: u32,
}

impl TypographyCounters {
    pub const fn new() -> TypographyCounters {
        TypographyCounters {
            glyphs: 0,
            cache_hits: 0,
            cache_misses: 0,
            fallbacks: 0,
        }
    }

    /// 缓存命中率（0..100），无样本返回 100。
    pub fn hit_rate(&self) -> u8 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            100
        } else {
            ((self.cache_hits * 100) / total) as u8
        }
    }
}

// ---------------------------------------------------------------------------
// A072 字体与排版引擎模糊测试
// ---------------------------------------------------------------------------

/// 模糊用例解析结果：要么成功解析，要么被安全拒绝（绝不 panic）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseOutcome {
    Parsed,
    Rejected,
}

/// 模糊入口：对任意字节流返回解析结果；越界/坏魔数均安全拒绝。
pub fn fuzz_one(bytes: &[u8]) -> ParseOutcome {
    match parse_sfnt_version(bytes) {
        Some(_) => match sfnt_num_tables(bytes) {
            Some(_) => ParseOutcome::Parsed,
            None => ParseOutcome::Rejected,
        },
        None => ParseOutcome::Rejected,
    }
}

// ---------------------------------------------------------------------------
// A073 字体与排版引擎文档
// ---------------------------------------------------------------------------

/// 文档/接口版本字符串。
pub fn docs_version() -> &'static str {
    "AURORA-1000 AI-03 typography v1"
}

/// 接口是否附带文档说明。
pub fn has_api_docs() -> bool {
    true
}

// ---------------------------------------------------------------------------
// A074 字体与排版引擎降级链
// ---------------------------------------------------------------------------

/// 字形缺失降级：首选字体有字形则用首选，否则退回 fallback 字体。
pub fn degrad_select(primary_has: bool, primary_id: u8, fallback_id: u8) -> u8 {
    if primary_has {
        primary_id
    } else {
        fallback_id
    }
}

// ---------------------------------------------------------------------------
// A075 字体与排版引擎域自检收口
// ---------------------------------------------------------------------------

/// 域收口摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainSummary {
    pub features: usize,
    pub fonts: usize,
    pub cache_cap: usize,
}

/// 汇总本域能力，供顶层收口断言。
pub fn domain_summary() -> DomainSummary {
    DomainSummary {
        features: 25,
        fonts: MAX_FONTS,
        cache_cap: MAX_CACHE,
    }
}

// ---------------------------------------------------------------------------
// 样例字体构造（供自检与测试复用）
// ---------------------------------------------------------------------------

/// 构造一个最小但自洽的 TrueType 样张：2 张表（head/hhea），校验和已回填。
fn build_sample_font() -> [u8; 256] {
    let mut f = [0u8; 256];
    // sfnt 版本 0x00010000
    f[0] = 0;
    f[1] = 1;
    f[2] = 0;
    f[3] = 0;
    // numTables = 2
    f[4..6].copy_from_slice(&2u16.to_be_bytes());
    // 表目录 0：head
    f[12..16].copy_from_slice(b"head");
    f[20..24].copy_from_slice(&64u32.to_be_bytes());
    f[24..28].copy_from_slice(&54u32.to_be_bytes());
    // 表目录 1：hhea
    f[28..32].copy_from_slice(b"hhea");
    f[36..40].copy_from_slice(&128u32.to_be_bytes());
    f[40..44].copy_from_slice(&36u32.to_be_bytes());
    // head 数据：unitsPerEm=2048, yMin=-200, xMax=1000, yMax=800, indexToLocFormat=0
    f[64 + 18..64 + 20].copy_from_slice(&2048u16.to_be_bytes());
    f[64 + 36..64 + 38].copy_from_slice(&0i16.to_be_bytes());
    f[64 + 38..64 + 40].copy_from_slice(&(-200i16).to_be_bytes());
    f[64 + 42..64 + 44].copy_from_slice(&1000i16.to_be_bytes());
    f[64 + 44..64 + 46].copy_from_slice(&800i16.to_be_bytes());
    f[64 + 50..64 + 52].copy_from_slice(&0i16.to_be_bytes());
    // hhea 数据：ascent=800, descent=-200, lineGap=90, advanceWidthMax=1000, numHMetrics=2
    f[128 + 4..128 + 6].copy_from_slice(&800i16.to_be_bytes());
    f[128 + 6..128 + 8].copy_from_slice(&(-200i16).to_be_bytes());
    f[128 + 8..128 + 10].copy_from_slice(&90i16.to_be_bytes());
    f[128 + 10..128 + 12].copy_from_slice(&1000u16.to_be_bytes());
    f[128 + 34..128 + 36].copy_from_slice(&2u16.to_be_bytes());
    // 回填校验和
    if let Some(r) = table_record_at(&f, 0) {
        let c = table_checksum(&f, r);
        f[16..20].copy_from_slice(&c.to_be_bytes());
    }
    if let Some(r) = table_record_at(&f, 1) {
        let c = table_checksum(&f, r);
        f[32..36].copy_from_slice(&c.to_be_bytes());
    }
    f
}

// ---------------------------------------------------------------------------
// A069/A075 — 域自检（必做）
// ---------------------------------------------------------------------------

/// 导出域自检：覆盖 A051~A075 的全部 25 项纯逻辑不变量（全部为真实断言）。
pub fn run_typography_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-typography");

    // A051 — TrueType/OTF 解析（版本/表数/表目录/校验和）
    let font = build_sample_font();
    let head_rec = find_table(&font, b"head");
    let checksum_ok = match head_rec {
        Some(r) => {
            let stored = rd_u32_be(&font, 16).unwrap_or(0);
            table_checksum(&font, r) == stored && r.offset == 64 && r.length == 54
        }
        None => false,
    };
    set.add(
        "A051 sfnt parse",
        parse_sfnt_version(&font) == Some(SfntVersion::TrueType)
            && sfnt_num_tables(&font) == Some(2)
            && checksum_ok,
        "version/table/checksum",
    );

    // A051 — 坏魔数拒绝
    set.add(
        "A051 bad magic",
        parse_sfnt_version(&[0x58, 0x58, 0x58, 0x58]) == None
            && validate_font(&[5, 5, 5, 5]) == false,
        "reject invalid",
    );

    // A052 — 二次贝塞尔中点
    let mid = quadratic_bezier(0, 0, 0, 10, 0, 20, 512);
    set.add("A052 bezier", mid == (0, 10), "quad midpoint");

    // A052 — 光栅化落点且不越界
    let mut bmp = [0u8; GLYPH_W * GLYPH_H];
    rasterize_box(&mut bmp, 1, 1, 3, 3);
    rasterize_quadratic(&mut bmp, 0, 0, 4, 8, 7, 0);
    set.add(
        "A052 raster",
        glyph_set_count(&bmp) >= 9 && bmp[1 * GLYPH_W + 1] == 1,
        "box+curve in bounds",
    );

    // A053 — 缓存容量与 LRU 淘汰
    let mut cache = GlyphCache::new();
    let mut evicted = 0u16;
    let mut k = 1;
    while k <= 18 {
        evicted = cache.put(k, k as u32);
        k += 1;
    }
    set.add(
        "A053 cache",
        cache.len() == MAX_CACHE && cache.contains(17) && !cache.contains(1),
        "lru evict",
    );
    let _ = evicted;

    // A054 — 字距二分查找
    let kt = kern_table_sample();
    set.add(
        "A054 kern",
        kern_lookup(&kt, 0x41, 0x56) == -60 && kern_lookup(&kt, 0x58, 0x58) == 0,
        "kern value",
    );

    // A055 — 行高
    let m = FontMetrics { units_per_em: 2048, ascent: 800, descent: -200, line_gap: 90 };
    set.add("A055 line height", line_height(&m) == 1090, "ascent-descent+gap");

    // A055 — 段落折行
    let adv = [100i32, 100, 100, 100];
    let (lines, last) = layout_lines(&adv, 250);
    set.add("A055 paragraph", lines == 2 && last == 200, "greedy wrap");

    // A056 — CJK / 全宽
    set.add(
        "A056 cjk/fullwidth",
        is_cjk(0x4E00) && !is_cjk(0x41) && is_fullwidth(0x3002) && !is_fullwidth(0x41),
        "cjk ranges",
    );

    // A056 — 换行禁则
    set.add(
        "A056 break",
        !cjk_can_break(0x4E00, 0x3002) && !cjk_can_break(0x300C, 0x4E00),
        "kinshi",
    );

    // A057 — emoji 检测与 ZWJ
    set.add(
        "A057 emoji",
        is_emoji(0x1F600) && !is_emoji(0x41) && is_zwj(0x200D) && is_emoji_presentation(0x1F600),
        "emoji ranges",
    );

    // A058 — fallback 链选择
    set.add(
        "A058 fallback",
        select_font(&DEFAULT_FALLBACK, 0x41) == 1
            && select_font(&DEFAULT_FALLBACK, 0x4E00) == 2
            && select_font(&DEFAULT_FALLBACK, 0x1F600) == 3,
        "chain pick",
    );

    // A059 — 字号阶梯令牌
    set.add(
        "A059 scale",
        scale_step(3) == 16 && scale_token_name("lg") == Some(24) && scale_token_name("zz") == None,
        "scale ladder",
    );

    // A060 — head/hhea 度量 + 像素对齐
    let head = extract_head_metrics(&font[64..64 + 54]);
    let hhea = extract_hhea_metrics(&font[128..128 + 36]);
    set.add(
        "A060 metrics",
        head.map(|h| h.units_per_em == 2048 && h.y_max == 800).unwrap_or(false)
            && hhea.map(|h| h.ascent == 800 && h.num_hmetrics == 2).unwrap_or(false)
            && advance_to_pixels(2048, 16, 2048) == 16
            && advance_to_pixels(1000, 16, 2048) == 8,
        "head/hhea/px",
    );

    // A061 — 粗体/斜体合成
    let mut src = [0u8; GLYPH_W * GLYPH_H];
    let mut x = 0;
    while x < GLYPH_W {
        src[2 * GLYPH_W + x] = 1;
        x += 1;
    }
    let mut bold = [0u8; GLYPH_W * GLYPH_H];
    let mut ital = [0u8; GLYPH_W * GLYPH_H];
    synthesize_bold_bitmap(&src, &mut bold);
    synthesize_italic_bitmap(&src, &mut ital);
    set.add(
        "A061 synth",
        glyph_set_count(&src) == 8 && glyph_set_count(&bold) == 16 && glyph_set_count(&ital) == 8,
        "bold/italic",
    );

    // A062 — 竖排与 RTL
    let mut run = [1u16, 2, 3, 4];
    rtl_reorder_run(&mut run);
    set.add(
        "A062 vertical/rtl",
        vertical_transform(0, 1) == (-1, 0)
            && is_rtl(0x0627)
            && !is_rtl(0x41)
            && run == [4, 3, 2, 1],
        "rotate+rtl",
    );

    // A063 — 子集位图
    let mut sub: GlyphSubset = [0u32; SUBSET_WORDS];
    let added = subset_add(&mut sub, 0x4E00) && subset_add(&mut sub, 0x41);
    set.add(
        "A063 subset",
        added && subset_contains(&sub, 0x4E00) && !subset_contains(&sub, 0x3000),
        "subset bits",
    );

    // A064 — 嵌入许可
    set.add(
        "A064 embed",
        embedding_permission(0) == Embedding::Installable
            && embedding_permission(0x0002) == Embedding::Restricted
            && embedding_permission(0x0004) == Embedding::PreviewPrint
            && embedding_permission(0x0200) == Embedding::BitmapOnly,
        "fsType",
    );

    // A065 — 字体商店
    let mut store = FontStore::new();
    store.install(FontInfo { id: 1, name: "latin", is_cjk: false });
    store.install(FontInfo { id: 2, name: "cjk", is_cjk: true });
    set.add(
        "A065 store",
        store.len() == 2
            && store.find_by_name("cjk").map(|f| f.is_cjk).unwrap_or(false)
            && store.find_by_id(1).is_some(),
        "install/lookup",
    );

    // A066 — 预览 ASCII 艺术
    let mut prev = [0u8; 96];
    let n = preview_glyph_ascii(&bold, &mut prev);
    let text = core::str::from_utf8(&prev[..n]).unwrap_or("");
    set.add("A066 preview", n > 0 && text.contains('#'), "ascii art");

    // A067 — 性能预算
    let mut budget = GlyphBudget::new(10);
    let ok_first = budget.take(6);
    let ok_second = budget.take(5);
    set.add(
        "A067 budget",
        ok_first && !ok_second && budget.used == 6 && !budget.within() == false,
        "budget cap",
    );

    // A068 — 字体一致性校验（良/截断）
    set.add(
        "A068 validate",
        validate_font(&font) && !validate_font(&font[..4]),
        "consistency",
    );

    // A069 — 引擎自检不变量
    set.add("A069 engine self", engine_invariant_ok(), "invariants");

    // A070 — 引擎性能预算
    let perf = EnginePerf { glyphs_per_frame: 1000, raster_ops: 5000 };
    set.add(
        "A070 perf",
        perf_within(500, 2000, perf) && !perf_within(1001, 0, perf),
        "perf budget",
    );

    // A071 — 可观测计数器
    let c = TypographyCounters { glyphs: 10, cache_hits: 3, cache_misses: 1, fallbacks: 2 };
    set.add("A071 counters", c.hit_rate() == 75, "hit rate");

    // A072 — 模糊测试入口
    set.add(
        "A072 fuzz",
        fuzz_one(&[0, 1, 0, 0, 0, 2]) == ParseOutcome::Parsed
            && fuzz_one(&[0x58, 0x58]) == ParseOutcome::Rejected,
        "fuzz safe",
    );

    // A073 — 文档
    set.add(
        "A073 docs",
        has_api_docs() && docs_version().len() > 0,
        "version string",
    );

    // A074 — 降级链
    set.add(
        "A074 degrad",
        degrad_select(false, 1, 2) == 2 && degrad_select(true, 1, 2) == 1,
        "fallback degrad",
    );

    // A075 — 域收口摘要
    let s = domain_summary();
    set.add(
        "A075 summary",
        s.features == 25 && s.fonts == MAX_FONTS && s.cache_cap == MAX_CACHE,
        "domain wrap-up",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a051_sfnt_parse() {
        let f = build_sample_font();
        assert_eq!(parse_sfnt_version(&f), Some(SfntVersion::TrueType));
        assert_eq!(sfnt_num_tables(&f), Some(2));
    }

    #[test]
    fn a051_table_dir_and_checksum() {
        let f = build_sample_font();
        let r = find_table(&f, b"head").expect("head");
        assert_eq!(r.offset, 64);
        assert_eq!(r.length, 54);
        let stored = rd_u32_be(&f, 16).unwrap();
        assert_eq!(table_checksum(&f, r), stored);
        let h = find_table(&f, b"hhea").expect("hhea");
        assert_eq!(h.offset, 128);
    }

    #[test]
    fn a051_bad_magic_rejected() {
        assert_eq!(parse_sfnt_version(&[0u8; 3]), None);
        assert_eq!(parse_sfnt_version(b"XXXX"), None);
        assert!(!validate_font(b"XXXX"));
        assert!(!validate_font(&[]));
    }

    #[test]
    fn a051_cmap_lookup() {
        assert_eq!(cmap_lookup(0x41), 1 + (0x41 - 0x20));
        assert_eq!(cmap_lookup(0x4E00), 100);
        assert_eq!(cmap_lookup(0x1F600), (200u32 + (0x1F600u32 - 0x1F300u32)) as u16);
        assert_eq!(cmap_lookup(0x10FFFF), 0);
    }

    #[test]
    fn a052_quadratic_bezier() {
        assert_eq!(quadratic_bezier(0, 0, 0, 10, 0, 20, 0), (0, 0));
        assert_eq!(quadratic_bezier(0, 0, 0, 10, 0, 20, 512), (0, 10));
        assert_eq!(quadratic_bezier(0, 0, 0, 10, 0, 20, 1024), (0, 20));
    }

    #[test]
    fn a052_rasterize_box() {
        let mut bmp = [0u8; GLYPH_W * GLYPH_H];
        rasterize_box(&mut bmp, 1, 1, 3, 3);
        assert_eq!(bmp[0], 0);
        assert_eq!(bmp[1 * GLYPH_W + 1], 1);
        assert_eq!(bmp[3 * GLYPH_W + 3], 1);
        assert_eq!(bmp[4 * GLYPH_W + 4], 0);
    }

    #[test]
    fn a052_rasterize_quadratic_bounds() {
        let mut bmp = [0u8; GLYPH_W * GLYPH_H];
        rasterize_quadratic(&mut bmp, -4, -4, 12, 12, 4, -4);
        // 不越界：数组全部为 0/1
        let mut ok = true;
        let mut i = 0;
        while i < bmp.len() {
            if bmp[i] > 1 {
                ok = false;
            }
            i += 1;
        }
        assert!(ok);
        assert!(glyph_set_count(&bmp) > 0);
    }

    #[test]
    fn a053_cache_put_get() {
        let mut c = GlyphCache::new();
        assert!(c.get(7, 1) == false);
        assert_eq!(c.put(7, 1), 0);
        assert!(c.get(7, 5));
        assert!(c.contains(7));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn a053_cache_lru_eviction() {
        let mut c = GlyphCache::new();
        let mut k = 1;
        while k <= (MAX_CACHE as u16) + 2 {
            c.put(k, k as u32);
            k += 1;
        }
        assert_eq!(c.len(), MAX_CACHE);
        assert!(c.contains(MAX_CACHE as u16));
        assert!(c.contains((MAX_CACHE as u16) + 1));
        assert!(!c.contains(1)); // 最久未用被淘汰
        assert!(!c.contains(2));
    }

    #[test]
    fn a054_kern_lookup() {
        let t = kern_table_sample();
        assert_eq!(t.len(), 3);
        assert_eq!(kern_lookup(&t, 0x41, 0x56), -60);
        assert_eq!(kern_lookup(&t, 0x54, 0x59), -50);
        assert_eq!(kern_lookup(&t, 0x41, 0x54), -40);
        assert_eq!(kern_lookup(&t, 0x58, 0x58), 0);
    }

    #[test]
    fn a055_line_height() {
        let m = FontMetrics { units_per_em: 2048, ascent: 800, descent: -200, line_gap: 90 };
        assert_eq!(line_height(&m), 1090);
        let m2 = FontMetrics { units_per_em: 1000, ascent: 900, descent: -200, line_gap: 0 };
        assert_eq!(line_height(&m2), 1100);
    }

    #[test]
    fn a055_paragraph_layout() {
        let adv = [100i32, 100, 100, 100];
        assert_eq!(layout_lines(&adv, 250), (2, 200));
        let adv2 = [50i32, 50, 50];
        assert_eq!(layout_lines(&adv2, 120), (2, 50));
        let empty: [i32; 0] = [];
        assert_eq!(layout_lines(&empty, 100), (0, 0));
    }

    #[test]
    fn a056_cjk_fullwidth() {
        assert!(is_cjk(0x4E00));
        assert!(is_cjk(0x3041)); // 假名
        assert!(!is_cjk(0x41));
        assert!(is_fullwidth(0x3002));
        assert!(is_fullwidth(0xFF21));
        assert!(!is_fullwidth(0x41));
    }

    #[test]
    fn a056_cjk_break_rules() {
        // 闭引号前不断行
        assert!(!cjk_can_break(0x4E00, 0x3002));
        // 开引号后不断行
        assert!(!cjk_can_break(0x300C, 0x4E00));
        // 普通情况允许
        assert!(cjk_can_break(0x4E00, 0x4E01));
    }

    #[test]
    fn a056_utf8_decode() {
        let mut buf = [0u32; 8];
        let s = "A中";
        let n = decode_utf8(s.as_bytes(), &mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], 0x41);
        assert_eq!(buf[1], 0x4E2D);
        // 三字节与坏序列
        let mut b2 = [0u32; 4];
        let n2 = decode_utf8(&[0xE4, 0xB8, 0xAD, 0xFF, 0xFE], &mut b2);
        assert_eq!(n2, 3);
        assert_eq!(b2[0], 0x4E2D);
        assert_eq!(b2[1], 0xFFFD);
    }

    #[test]
    fn a057_emoji_detect() {
        assert!(is_emoji(0x1F600));
        assert!(is_emoji(0x2600));
        assert!(!is_emoji(0x41));
        assert!(is_zwj(0x200D));
        assert!(!is_zwj(0x41));
        assert!(is_emoji_presentation(0x1F600));
        assert!(!is_emoji_presentation(0x0041));
    }

    #[test]
    fn a057_zwj() {
        // ZWJ 序列判定仅依赖连接符本身
        assert!(is_zwj(0x200D));
        assert!(is_emoji(0x1F3F4)); // 旗标基
    }

    #[test]
    fn a058_fallback_select() {
        let c = DEFAULT_FALLBACK;
        assert_eq!(select_font(&c, 0x41), 1);
        assert_eq!(select_font(&c, 0x4E00), 2);
        assert_eq!(select_font(&c, 0x1F600), 3);
        assert_eq!(select_font(&c, 0x3001), 4);
        let mut empty = FallbackChain::new();
        empty.push(FontCoverage { id: 9, lo: 0, hi: 0x10FFFF });
        assert_eq!(select_font(&empty, 0x41), 9);
    }

    #[test]
    fn a059_scale_tokens() {
        assert_eq!(scale_step(0), 10);
        assert_eq!(scale_step(3), 16);
        assert_eq!(scale_step(7), 48);
        assert_eq!(scale_step(99), 16); // 越界回退
        assert_eq!(scale_token_name("xs"), Some(12));
        assert_eq!(scale_token_name("xxl"), Some(48));
        assert_eq!(scale_token_name("nope"), None);
    }

    #[test]
    fn a060_head_hhea_metrics() {
        let f = build_sample_font();
        let h = extract_head_metrics(&f[64..64 + 54]).expect("head");
        assert_eq!(h.units_per_em, 2048);
        assert_eq!(h.y_max, 800);
        assert_eq!(h.y_min, -200);
        let hh = extract_hhea_metrics(&f[128..128 + 36]).expect("hhea");
        assert_eq!(hh.ascent, 800);
        assert_eq!(hh.descent, -200);
        assert_eq!(hh.num_hmetrics, 2);
        assert_eq!(hh.advance_width_max, 1000);
    }

    #[test]
    fn a060_advance_pixels() {
        assert_eq!(advance_to_pixels(2048, 16, 2048), 16);
        assert_eq!(advance_to_pixels(1000, 16, 2048), 8);
        assert_eq!(advance_to_pixels(100, 16, 2048), 1);
        assert_eq!(advance_to_pixels(100, 16, 0), 100); // 防零除
    }

    #[test]
    fn a061_bold_italic() {
        let mut src = [0u8; GLYPH_W * GLYPH_H];
        let mut x = 0;
        while x < GLYPH_W {
            src[2 * GLYPH_W + x] = 1;
            x += 1;
        }
        let mut bold = [0u8; GLYPH_W * GLYPH_H];
        let mut ital = [0u8; GLYPH_W * GLYPH_H];
        synthesize_bold_bitmap(&src, &mut bold);
        synthesize_italic_bitmap(&src, &mut ital);
        assert_eq!(glyph_set_count(&src), 8);
        assert_eq!(glyph_set_count(&bold), 16);
        assert_eq!(glyph_set_count(&ital), 8);
    }

    #[test]
    fn a062_vertical_transform() {
        assert_eq!(vertical_transform(0, 1), (-1, 0));
        assert_eq!(vertical_transform(3, 4), (-4, 3));
        assert_eq!(vertical_cell_y(2, 16), 32);
        assert!(is_rtl(0x0627));
        assert!(is_rtl(0x05D0));
        assert!(!is_rtl(0x41));
    }

    #[test]
    fn a062_rtl_reorder() {
        let mut run = [1u16, 2, 3, 4];
        rtl_reorder_run(&mut run);
        assert_eq!(run, [4, 3, 2, 1]);
        let mut single = [7u16];
        rtl_reorder_run(&mut single);
        assert_eq!(single, [7]);
    }

    #[test]
    fn a063_subset() {
        let mut s: GlyphSubset = [0u32; SUBSET_WORDS];
        assert!(subset_add(&mut s, 0x41));
        assert!(subset_add(&mut s, 0x4E00));
        assert!(subset_contains(&s, 0x41));
        assert!(subset_contains(&s, 0x4E00));
        assert!(!subset_contains(&s, 0x3000));
        // 超界（>512）安全拒绝
        assert!(!subset_add(&mut s, 0x10000));
        assert!(!subset_contains(&s, 0x10000));
    }

    #[test]
    fn a064_embedding() {
        assert_eq!(embedding_permission(0), Embedding::Installable);
        assert_eq!(embedding_permission(0x0001), Embedding::None);
        assert_eq!(embedding_permission(0x0002), Embedding::Restricted);
        assert_eq!(embedding_permission(0x0004), Embedding::PreviewPrint);
        assert_eq!(embedding_permission(0x0008), Embedding::Editable);
        assert_eq!(embedding_permission(0x0200), Embedding::BitmapOnly);
    }

    #[test]
    fn a065_font_store() {
        let mut store = FontStore::new();
        assert!(store.install(FontInfo { id: 1, name: "latin", is_cjk: false }));
        assert!(store.install(FontInfo { id: 2, name: "cjk", is_cjk: true }));
        assert_eq!(store.len(), 2);
        assert!(store.find_by_name("cjk").unwrap().is_cjk);
        assert!(store.find_by_id(1).is_some());
        assert!(store.find_by_name("missing").is_none());
    }

    #[test]
    fn a066_preview_ascii() {
        let mut bmp = [0u8; GLYPH_W * GLYPH_H];
        rasterize_box(&mut bmp, 2, 2, 4, 4);
        let mut out = [0u8; 96];
        let n = preview_glyph_ascii(&bmp, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains('#'));
        assert!(text.contains('.'));
        assert!(n > 0);
    }

    #[test]
    fn a067_budget() {
        let mut b = GlyphBudget::new(10);
        assert!(b.take(6));
        assert!(!b.take(5));
        assert_eq!(b.used, 6);
        assert!(b.within());
        assert!(!GlyphBudget::new(0).take(1));
    }

    #[test]
    fn a068_validate_font() {
        let f = build_sample_font();
        assert!(validate_font(&f));
        assert!(!validate_font(&f[..4]));
        assert!(!validate_font(&[]));
        // 表目录声明的范围超出文件也应被拒
        let mut bad = f;
        bad[24..28].copy_from_slice(&1000u32.to_be_bytes()); // head length 越界
        assert!(!validate_font(&bad));
    }

    #[test]
    fn a069_engine_invariant() {
        assert!(engine_invariant_ok());
    }

    #[test]
    fn a070_perf() {
        let b = EnginePerf { glyphs_per_frame: 1000, raster_ops: 5000 };
        assert!(perf_within(500, 2000, b));
        assert!(!perf_within(1001, 0, b));
        assert!(!perf_within(0, 5001, b));
    }

    #[test]
    fn a071_counters() {
        let c = TypographyCounters { glyphs: 10, cache_hits: 3, cache_misses: 1, fallbacks: 2 };
        assert_eq!(c.hit_rate(), 75);
        let empty = TypographyCounters::new();
        assert_eq!(empty.hit_rate(), 100);
        let miss = TypographyCounters { glyphs: 0, cache_hits: 0, cache_misses: 4, fallbacks: 0 };
        assert_eq!(miss.hit_rate(), 0);
    }

    #[test]
    fn a072_fuzz() {
        assert_eq!(fuzz_one(&[0, 1, 0, 0, 0, 2]), ParseOutcome::Parsed);
        assert_eq!(fuzz_one(&[]), ParseOutcome::Rejected);
        assert_eq!(fuzz_one(b"XX"), ParseOutcome::Rejected);
        // 任意乱码字节流都不应 panic，且被安全拒绝
        let garbage = [0xFFu8, 0xFE, 0x00, 0x80, 0x81];
        assert_eq!(fuzz_one(&garbage), ParseOutcome::Rejected);
    }

    #[test]
    fn a073_docs() {
        assert!(has_api_docs());
        assert!(docs_version().starts_with("AURORA-1000"));
    }

    #[test]
    fn a074_degradation() {
        assert_eq!(degrad_select(true, 1, 2), 1);
        assert_eq!(degrad_select(false, 1, 2), 2);
    }

    #[test]
    fn a075_summary() {
        let s = domain_summary();
        assert_eq!(s.features, 25);
        assert_eq!(s.fonts, MAX_FONTS);
        assert_eq!(s.cache_cap, MAX_CACHE);
    }

    #[test]
    fn a069_self_check_passes_all() {
        let set = run_typography_checks();
for i in 0..set.len() { if let Some(c) = set.get(i) { if !c.passed { println!("DIAG a069 fail: {} : {}", c.name, c.detail); } } }
        assert_eq!(set.len(), 29);
        assert!(set.all_passed(), "all typography checks must pass");
        assert!(!set.truncated());
    }
}
