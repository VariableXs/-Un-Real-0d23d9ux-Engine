//! UNX-F50092 字体 fuzz 面（坏字体文件注入不崩）（0x437a0 段）
//!
//! 锚点：UNX-F50092-J1 fuzz ×10^4 零崩；判据锚定 §AI-63 §5。
//!
//! 为什么字库必须有独立的 fuzz 面：字体来自外部（用户上传、网络下载、
//! 打印机内嵌），一个越界的 `loca` 表就能让光栅化在取字形时读到文件外。
//! 内核路径零 panic，所以本模块把「最小 sfnt 视图」解析器写成全 `Result`
//! 形态：每个字段读取先做区间校验，坏字体只得到一个落在 0x437a0 段闭集
//! 内的诊断码，而不是崩溃。
//!
//! 诊断码闭集：`SEG_BASE + n`，n ∈ [1,20]，20 条全部被真实构造（解析
//! 分支 + 注入核）。段内不再细分，避免码值漂移。
//!
//! 结构清单：
//! * 解析器 [`parse_sfnt`]：sfnt 头 → 表目录 → head/maxp/loca/cmap/glyf；
//! * 状态机 [`FuzzCampaign`]（Seeded→Running→Judged，重复执行显式拒绝）；
//! * 语料器 [`Corpus`]（xorshift64 确定性变异，逐位可复跑）；
//! * 报告 [`FuzzReport`]（接受/拒绝/越界码计数 + 结构摘要）；
//! * 注入核 [`injection_core`]：17 类坏字体逐类断言具体诊断码。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x437a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x437a0;

pub const E_EMPTY: u16 = 1;
pub const E_MAGIC: u16 = 2;
pub const E_TRUNCATED: u16 = 3;
pub const E_TABLECOUNT: u16 = 4;
pub const E_DIROVERFLOW: u16 = 5;
pub const E_TABLECUT: u16 = 6;
pub const E_MISSING: u16 = 7;
pub const E_DUPTAG: u16 = 8;
pub const E_HEADSHORT: u16 = 9;
pub const E_HEADMAGIC: u16 = 10;
pub const E_HEADUNITS: u16 = 11;
pub const E_MAXPGLYPHS: u16 = 12;
pub const E_LOCAFORMAT: u16 = 13;
pub const E_LOCASHORT: u16 = 14;
pub const E_LOCAORDER: u16 = 15;
pub const E_CMAPFORMAT: u16 = 16;
pub const E_CHECKSUM: u16 = 17;
pub const E_GLYPHRANGE: u16 = 18;
pub const E_GLYPHID: u16 = 19;
pub const E_ORDER: u16 = 20;

/// 表目录条数上限。sfnt 规范未给硬上限，64 足够覆盖任何真实字库，
/// 也给「表目录把文件撑爆」这类坏样本一个明确的拒绝点。
pub const MAX_TABLES: u16 = 64;

/// 字体结构错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    Empty,
    Magic,
    Truncated,
    TableCount,
    DirOverflow,
    TableCut,
    MissingTable,
    DuplicateTag,
    HeadShort,
    HeadMagic,
    HeadUnits,
    MaxpGlyphs,
    LocaFormat,
    LocaShort,
    LocaOrder,
    CmapFormat,
    Checksum,
    GlyphRange,
    GlyphId,
    Order,
}

impl FuzzError {
    /// 段内序号（闭集 1..=20）。
    pub fn ordinal(self) -> u16 {
        match self {
            FuzzError::Empty => E_EMPTY,
            FuzzError::Magic => E_MAGIC,
            FuzzError::Truncated => E_TRUNCATED,
            FuzzError::TableCount => E_TABLECOUNT,
            FuzzError::DirOverflow => E_DIROVERFLOW,
            FuzzError::TableCut => E_TABLECUT,
            FuzzError::MissingTable => E_MISSING,
            FuzzError::DuplicateTag => E_DUPTAG,
            FuzzError::HeadShort => E_HEADSHORT,
            FuzzError::HeadMagic => E_HEADMAGIC,
            FuzzError::HeadUnits => E_HEADUNITS,
            FuzzError::MaxpGlyphs => E_MAXPGLYPHS,
            FuzzError::LocaFormat => E_LOCAFORMAT,
            FuzzError::LocaShort => E_LOCASHORT,
            FuzzError::LocaOrder => E_LOCAORDER,
            FuzzError::CmapFormat => E_CMAPFORMAT,
            FuzzError::Checksum => E_CHECKSUM,
            FuzzError::GlyphRange => E_GLYPHRANGE,
            FuzzError::GlyphId => E_GLYPHID,
            FuzzError::Order => E_ORDER,
        }
    }

    /// 全仓诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            FuzzError::Empty => "empty font buffer",
            FuzzError::Magic => "sfnt version unrecognized",
            FuzzError::Truncated => "short read inside table",
            FuzzError::TableCount => "table count zero or above cap",
            FuzzError::DirOverflow => "table directory exceeds file",
            FuzzError::TableCut => "table offset/length outside file",
            FuzzError::MissingTable => "required table missing",
            FuzzError::DuplicateTag => "duplicate table tag",
            FuzzError::HeadShort => "head table shorter than 54",
            FuzzError::HeadMagic => "head magic mismatch",
            FuzzError::HeadUnits => "unitsPerEm out of range",
            FuzzError::MaxpGlyphs => "numGlyphs is zero",
            FuzzError::LocaFormat => "indexToLocFormat not 0/1",
            FuzzError::LocaShort => "loca table too short",
            FuzzError::LocaOrder => "loca offsets not monotonic",
            FuzzError::CmapFormat => "cmap subtable unsupported",
            FuzzError::Checksum => "table checksum mismatch",
            FuzzError::GlyphRange => "loca end beyond glyf",
            FuzzError::GlyphId => "glyph id out of range",
            FuzzError::Order => "campaign state order violated",
        }
    }
}

/// 表目录条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableInfo {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub offset: u32,
    pub length: u32,
}

/// 最小 sfnt 视图：只保留 fuzz 判据需要的字段。全部来自显式解析，
/// 不做「差不多就读过去」的宽松兜底。
#[derive(Clone, Debug)]
pub struct Sfnt {
    pub num_tables: u16,
    pub units_per_em: u16,
    pub num_glyphs: u16,
    pub loca_long: bool,
    pub cmap_format: u16,
    pub glyf_len: u32,
    pub tables: Vec<TableInfo>,
    pub loca: Vec<u32>,
    /// 结构摘要：同一字节流两次解析必须逐位相同。
    pub digest: u64,
}

impl Sfnt {
    /// 取字形在 glyf 中的区间；越界与倒序都显式失败。
    pub fn glyph_extent(&self, gid: u16) -> Result<(u32, u32), FuzzError> {
        if gid >= self.num_glyphs {
            return Err(FuzzError::GlyphId);
        }
        let a = match self.loca.get(gid as usize) {
            Some(v) => *v,
            None => return Err(FuzzError::Truncated),
        };
        let b = match self.loca.get(gid as usize + 1) {
            Some(v) => *v,
            None => return Err(FuzzError::Truncated),
        };
        if b > self.glyf_len {
            return Err(FuzzError::GlyphRange);
        }
        Ok((a, b))
    }
}

fn be_u16(d: &[u8], at: usize) -> Option<u16> {
    let a = d.get(at)?;
    let b = d.get(at + 1)?;
    Some(((*a as u16) << 8) | *b as u16)
}

fn be_u32(d: &[u8], at: usize) -> Option<u32> {
    let a = d.get(at)?;
    let b = d.get(at + 1)?;
    let c = d.get(at + 2)?;
    let e = d.get(at + 3)?;
    Some(((*a as u32) << 24) | ((*b as u32) << 16) | ((*c as u32) << 8) | *e as u32)
}

fn read_tag(d: &[u8], at: usize) -> Result<[u8; 4], FuzzError> {
    let mut t = [0u8; 4];
    let mut k = 0usize;
    while k < 4 {
        t[k] = match d.get(at + k) {
            Some(b) => *b,
            None => return Err(FuzzError::Truncated),
        };
        k += 1;
    }
    Ok(t)
}

fn slice_of(d: &[u8], off: usize, len: usize) -> Result<&[u8], FuzzError> {
    let end = off.checked_add(len).ok_or(FuzzError::TableCut)?;
    d.get(off..end).ok_or(FuzzError::TableCut)
}

fn table_of<'a>(tables: &'a [TableInfo], tag: &[u8; 4]) -> Result<&'a TableInfo, FuzzError> {
    let mut i = 0usize;
    while let Some(t) = tables.get(i) {
        if &t.tag == tag {
            return Ok(t);
        }
        i += 1;
    }
    Err(FuzzError::MissingTable)
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x1000_0000_01b3)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while let Some(b) = bytes.get(i) {
        h = mix(h, *b as u64);
        i += 1;
    }
    h
}

/// 整表求和（按 sfnt 的 4 字节大端字）。`head` 的 checkSumAdjustment
/// （表内偏移 8..12）按规范置零后再求和，否则任何 `head` 都不可能自洽。
fn table_sum(data: &[u8], off: usize, len: usize, is_head: bool) -> u32 {
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < len {
        let mut w = 0u32;
        let mut k = 0usize;
        while k < 4 {
            let pos = i + k;
            let raw = if pos < len {
                match data.get(off + pos) {
                    Some(b) => *b,
                    None => 0,
                }
            } else {
                0
            };
            let b = if is_head && pos >= 8 && pos < 12 {
                0
            } else {
                raw
            };
            w = (w << 8) | b as u32;
            k += 1;
        }
        sum = sum.wrapping_add(w);
        i += 4;
    }
    sum
}

fn verify_checksums(data: &[u8], tables: &[TableInfo]) -> Result<(), FuzzError> {
    let mut i = 0usize;
    while let Some(t) = tables.get(i) {
        let is_head = &t.tag == b"head";
        let sum = table_sum(data, t.offset as usize, t.length as usize, is_head);
        if sum != t.checksum {
            return Err(FuzzError::Checksum);
        }
        i += 1;
    }
    Ok(())
}

/// 解析最小 sfnt。结构检查全部在「读表内容」之前完成；校验收在最后，
/// 这样坏样本报出的是最贴近根因的码，而不是一律 `Checksum`。
pub fn parse_sfnt(data: &[u8]) -> Result<Sfnt, FuzzError> {
    if data.is_empty() {
        return Err(FuzzError::Empty);
    }
    if data.len() < 12 {
        return Err(FuzzError::Truncated);
    }
    let ver = be_u32(data, 0).ok_or(FuzzError::Truncated)?;
    if ver != 0x0001_0000 && ver != 0x4F54_544F && ver != 0x7472_7565 {
        return Err(FuzzError::Magic);
    }
    let count = be_u16(data, 4).ok_or(FuzzError::Truncated)?;
    if count == 0 || count > MAX_TABLES {
        return Err(FuzzError::TableCount);
    }
    let dir_end = 12usize + count as usize * 16;
    if dir_end > data.len() {
        return Err(FuzzError::DirOverflow);
    }
    let mut tables: Vec<TableInfo> = Vec::with_capacity(count as usize);
    let mut i = 0usize;
    while i < count as usize {
        let e = 12 + i * 16;
        let tag = read_tag(data, e)?;
        let checksum = be_u32(data, e + 4).ok_or(FuzzError::Truncated)?;
        let offset = be_u32(data, e + 8).ok_or(FuzzError::Truncated)?;
        let length = be_u32(data, e + 12).ok_or(FuzzError::Truncated)?;
        let off = offset as usize;
        let len = length as usize;
        let end = off.checked_add(len).ok_or(FuzzError::TableCut)?;
        if off < dir_end || end > data.len() {
            return Err(FuzzError::TableCut);
        }
        let mut j = 0usize;
        while let Some(t) = tables.get(j) {
            if t.tag == tag {
                return Err(FuzzError::DuplicateTag);
            }
            j += 1;
        }
        tables.push(TableInfo {
            tag,
            checksum,
            offset,
            length,
        });
        i += 1;
    }

    let head_t = table_of(&tables, b"head")?;
    let head = slice_of(data, head_t.offset as usize, head_t.length as usize)?;
    if head.len() < 54 {
        return Err(FuzzError::HeadShort);
    }
    let magic = be_u32(head, 12).ok_or(FuzzError::Truncated)?;
    if magic != 0x5F0F_3CF5 {
        return Err(FuzzError::HeadMagic);
    }
    let units = be_u16(head, 18).ok_or(FuzzError::Truncated)?;
    if !(16..=16384).contains(&units) {
        return Err(FuzzError::HeadUnits);
    }
    let loc_fmt = be_u16(head, 50).ok_or(FuzzError::Truncated)?;
    if loc_fmt > 1 {
        return Err(FuzzError::LocaFormat);
    }

    let maxp_t = table_of(&tables, b"maxp")?;
    let maxp = slice_of(data, maxp_t.offset as usize, maxp_t.length as usize)?;
    if maxp.len() < 6 {
        return Err(FuzzError::Truncated);
    }
    let num_glyphs = be_u16(maxp, 4).ok_or(FuzzError::Truncated)?;
    if num_glyphs == 0 {
        return Err(FuzzError::MaxpGlyphs);
    }

    let loca_long = loc_fmt == 1;
    let item = if loca_long { 4usize } else { 2usize };
    let need = (num_glyphs as usize + 1) * item;
    let loca_t = table_of(&tables, b"loca")?;
    let loca = slice_of(data, loca_t.offset as usize, loca_t.length as usize)?;
    if loca.len() < need {
        return Err(FuzzError::LocaShort);
    }
    let mut offs: Vec<u32> = Vec::with_capacity(num_glyphs as usize + 1);
    let mut prev = 0u32;
    let mut k = 0usize;
    while k <= num_glyphs as usize {
        let v = if loca_long {
            be_u32(loca, k * 4).ok_or(FuzzError::LocaShort)?
        } else {
            (be_u16(loca, k * 2).ok_or(FuzzError::LocaShort)? as u32) * 2
        };
        if k == 0 && v != 0 {
            return Err(FuzzError::LocaOrder);
        }
        if v < prev {
            return Err(FuzzError::LocaOrder);
        }
        prev = v;
        offs.push(v);
        k += 1;
    }

    let cmap_t = table_of(&tables, b"cmap")?;
    let cmap = slice_of(data, cmap_t.offset as usize, cmap_t.length as usize)?;
    if cmap.len() < 4 {
        return Err(FuzzError::CmapFormat);
    }
    let fmt = be_u16(cmap, 0).ok_or(FuzzError::CmapFormat)?;
    let cmap_ok = match fmt {
        4 => match be_u16(cmap, 2) {
            Some(l) => l as usize >= 4 && l as usize <= cmap.len(),
            None => false,
        },
        12 => {
            if cmap.len() < 16 {
                false
            } else {
                match be_u32(cmap, 4) {
                    Some(l) => l as usize >= 16 && l as usize <= cmap.len(),
                    None => false,
                }
            }
        }
        _ => false,
    };
    if !cmap_ok {
        return Err(FuzzError::CmapFormat);
    }

    let glyf_t = table_of(&tables, b"glyf")?;
    let glyf_len = glyf_t.length;
    if let Some(last) = offs.get(num_glyphs as usize) {
        if *last > glyf_len {
            return Err(FuzzError::GlyphRange);
        }
    }

    verify_checksums(data, &tables)?;

    // 摘要只吃「解析后的结构」，因此两次解析一致等价于结构一致。
    let mut dg = 0xcbf2_9ce4_8422_2325u64;
    dg = mix(dg, num_glyphs as u64);
    dg = mix(dg, units as u64);
    dg = mix(dg, fmt as u64);
    let mut i = 0usize;
    while let Some(t) = tables.get(i) {
        let mut b = 0usize;
        while b < 4 {
            if let Some(x) = t.tag.get(b) {
                dg = mix(dg, *x as u64);
            }
            b += 1;
        }
        dg = mix(dg, t.offset as u64);
        dg = mix(dg, t.length as u64);
        i += 1;
    }

    Ok(Sfnt {
        num_tables: count,
        units_per_em: units,
        num_glyphs,
        loca_long,
        cmap_format: fmt,
        glyf_len,
        tables,
        loca: offs,
        digest: dg,
    })
}

// ---------------------------------------------------------------------------
// 最小合法字库（供语料器当种子，避免读外部文件）
// ---------------------------------------------------------------------------

const HEAD_OFF: usize = 92;
const HEAD_LEN: usize = 54;
const MAXP_OFF: usize = 148;
const MAXP_LEN: usize = 6;
const LOCA_OFF: usize = 156;
const LOCA_LEN: usize = 10;
const CMAP_OFF: usize = 168;
const CMAP_LEN: usize = 24;
const GLYF_OFF: usize = 192;
const GLYF_LEN: usize = 32;
const FONT_LEN: usize = 224;

fn put_u16(v: &mut Vec<u8>, at: usize, val: u16) {
    let bytes = [(val >> 8) as u8, val as u8];
    let mut k = 0usize;
    while k < 2 {
        if let (Some(s), Some(b)) = (v.get_mut(at + k), bytes.get(k)) {
            *s = *b;
        }
        k += 1;
    }
}

fn put_u32(v: &mut Vec<u8>, at: usize, val: u32) {
    let bytes = [
        (val >> 24) as u8,
        (val >> 16) as u8,
        (val >> 8) as u8,
        val as u8,
    ];
    let mut k = 0usize;
    while k < 4 {
        if let (Some(s), Some(b)) = (v.get_mut(at + k), bytes.get(k)) {
            *s = *b;
        }
        k += 1;
    }
}

fn put_tag(v: &mut Vec<u8>, at: usize, tag: &[u8; 4]) {
    let mut k = 0usize;
    while k < 4 {
        if let (Some(s), Some(b)) = (v.get_mut(at + k), tag.get(k)) {
            *s = *b;
        }
        k += 1;
    }
}

fn table_layout() -> [([u8; 4], usize, usize); 5] {
    [
        (*b"head", HEAD_OFF, HEAD_LEN),
        (*b"maxp", MAXP_OFF, MAXP_LEN),
        (*b"loca", LOCA_OFF, LOCA_LEN),
        (*b"cmap", CMAP_OFF, CMAP_LEN),
        (*b"glyf", GLYF_OFF, GLYF_LEN),
    ]
}

/// 构造 4 字形 / 1000 upem / 短 loca 的最小合法字库。表校验和按本模块
/// 的求和口径回填，保证 `parse_sfnt` 能完整走通。
pub fn build_minimal_font() -> Vec<u8> {
    let mut v: Vec<u8> = Vec::with_capacity(FONT_LEN);
    let mut i = 0usize;
    while i < FONT_LEN {
        v.push(0u8);
        i += 1;
    }
    put_u32(&mut v, 0, 0x0001_0000);
    put_u16(&mut v, 4, 5);
    let layout = table_layout();
    let mut k = 0usize;
    while k < 5 {
        if let Some(entry) = layout.get(k) {
            let e = 12 + k * 16;
            put_tag(&mut v, e, &entry.0);
            put_u32(&mut v, e + 8, entry.1 as u32);
            put_u32(&mut v, e + 12, entry.2 as u32);
        }
        k += 1;
    }
    put_u32(&mut v, HEAD_OFF, 0x0001_0000);
    put_u32(&mut v, HEAD_OFF + 4, 0x0001_0000);
    put_u32(&mut v, HEAD_OFF + 12, 0x5F0F_3CF5);
    put_u16(&mut v, HEAD_OFF + 18, 1000);
    put_u16(&mut v, HEAD_OFF + 40, 1000);
    put_u16(&mut v, HEAD_OFF + 42, 1000);
    put_u16(&mut v, HEAD_OFF + 46, 8);
    put_u16(&mut v, HEAD_OFF + 50, 0);
    put_u32(&mut v, MAXP_OFF, 0x0001_0000);
    put_u16(&mut v, MAXP_OFF + 4, 4);
    let locs = [0u16, 4, 8, 12, 16];
    let mut k = 0usize;
    while k < 5 {
        if let Some(x) = locs.get(k) {
            put_u16(&mut v, LOCA_OFF + k * 2, *x);
        }
        k += 1;
    }
    put_u16(&mut v, CMAP_OFF, 4);
    put_u16(&mut v, CMAP_OFF + 2, 24);
    put_u16(&mut v, CMAP_OFF + 6, 2);
    let mut k = 0usize;
    while k < GLYF_LEN {
        if let Some(s) = v.get_mut(GLYF_OFF + k) {
            *s = (k as u8).wrapping_mul(7).wrapping_add(3);
        }
        k += 1;
    }
    let mut k = 0usize;
    while k < 5 {
        if let Some(entry) = layout.get(k) {
            let sum = table_sum(&v, entry.1, entry.2, &entry.0 == b"head");
            put_u32(&mut v, 12 + k * 16 + 4, sum);
        }
        k += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 确定性语料 + 状态机
// ---------------------------------------------------------------------------

struct Rng {
    s: u64,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng { s: seed | 1 }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x
    }
}

/// 一次变异：小概率截断（制造短文件），否则翻 1..=6 个字节。
fn mutate(base: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut v = base.to_vec();
    if rng.next_u64() % 10 == 0 && !v.is_empty() {
        let cut = (rng.next_u64() as usize) % v.len();
        v.truncate(cut);
        return v;
    }
    let flips = 1 + (rng.next_u64() as usize) % 6;
    let mut k = 0usize;
    while k < flips {
        if !v.is_empty() {
            let pos = (rng.next_u64() as usize) % v.len();
            let val = rng.next_u64() as u8;
            if let Some(slot) = v.get_mut(pos) {
                *slot = val;
            }
        }
        k += 1;
    }
    v
}

/// fuzz 报告。`out_of_set` 必须恒为 0：任何诊断码越出闭集都说明
/// 错误路径失控。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub iterations: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub out_of_set: usize,
    pub digest: u64,
}

/// 状态机档位。`execute` 只允许从 Seeded 出发一次；重复执行返回
/// `Order`，避免「同一会话跑两遍」污染报告语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzStage {
    Seeded,
    Running,
    Judged,
}

/// fuzz 会话：持有种子字库与 RNG，执行后冻结为 Judged。
pub struct FuzzCampaign {
    stage: FuzzStage,
    rng: Rng,
    base: Vec<u8>,
    report: FuzzReport,
}

impl FuzzCampaign {
    /// 建立会话（种子字库由 [`build_minimal_font`] 现场构造，不读盘）。
    pub fn new(seed: u64) -> FuzzCampaign {
        FuzzCampaign {
            stage: FuzzStage::Seeded,
            rng: Rng::new(seed),
            base: build_minimal_font(),
            report: FuzzReport {
                iterations: 0,
                accepted: 0,
                rejected: 0,
                out_of_set: 0,
                digest: 0,
            },
        }
    }

    /// 当前档位。
    pub fn stage(&self) -> FuzzStage {
        self.stage
    }

    /// 最近一次报告（未执行时为全零）。
    pub fn report(&self) -> FuzzReport {
        self.report
    }

    /// 执行 N 轮变异解析。第 0 轮喂种子本身，保证接受分支至少命中一次。
    pub fn execute(&mut self, n: usize) -> Result<FuzzReport, FuzzError> {
        if self.stage != FuzzStage::Seeded {
            return Err(FuzzError::Order);
        }
        self.stage = FuzzStage::Running;
        let base = self.base.clone();
        let mut rng = Rng { s: self.rng.s };
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        let mut out_of_set = 0usize;
        let mut digest = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0usize;
        while i < n {
            let case = if i == 0 {
                base.clone()
            } else {
                mutate(&base, &mut rng)
            };
            match parse_sfnt(&case) {
                Ok(f) => {
                    accepted += 1;
                    digest = mix(digest, f.digest);
                }
                Err(e) => {
                    rejected += 1;
                    let ord = e.ordinal();
                    if ord == 0 || ord as usize > E_ORDER as usize {
                        out_of_set += 1;
                    }
                    digest = mix(digest, e.code() as u64);
                }
            }
            i += 1;
        }
        let rep = FuzzReport {
            iterations: n,
            accepted,
            rejected,
            out_of_set,
            digest,
        };
        self.rng.s = rng.s;
        self.report = rep;
        self.stage = FuzzStage::Judged;
        Ok(rep)
    }
}

// ---------------------------------------------------------------------------
// 注入核：17 类坏样本，逐类断言具体诊断码
// ---------------------------------------------------------------------------

fn expect_code(data: &[u8], want: FuzzError) -> bool {
    parse_sfnt(data).err() == Some(want)
}

/// 用固定偏移改写最小字库，返回 (命中数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let base = build_minimal_font();
    let mut caught = 0usize;
    let total = 17usize;

    if expect_code(&[], FuzzError::Empty) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u32(&mut v, 0, 0x9999_9999);
    if expect_code(&v, FuzzError::Magic) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, 4, 0);
    if expect_code(&v, FuzzError::TableCount) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, 4, 64);
    if expect_code(&v, FuzzError::DirOverflow) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u32(&mut v, 12 + 8, 0xFFFF_FF00);
    if expect_code(&v, FuzzError::TableCut) {
        caught += 1;
    }
    let mut v = base.clone();
    put_tag(&mut v, 28, b"head");
    if expect_code(&v, FuzzError::DuplicateTag) {
        caught += 1;
    }
    let mut v = base.clone();
    put_tag(&mut v, 76, b"glyX");
    if expect_code(&v, FuzzError::MissingTable) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u32(&mut v, 12 + 12, 40);
    if expect_code(&v, FuzzError::HeadShort) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u32(&mut v, HEAD_OFF + 12, 0x1234_5678);
    if expect_code(&v, FuzzError::HeadMagic) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, HEAD_OFF + 18, 0);
    if expect_code(&v, FuzzError::HeadUnits) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, MAXP_OFF + 4, 0);
    if expect_code(&v, FuzzError::MaxpGlyphs) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, HEAD_OFF + 50, 2);
    if expect_code(&v, FuzzError::LocaFormat) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u32(&mut v, 44 + 12, 4);
    if expect_code(&v, FuzzError::LocaShort) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, LOCA_OFF + 8, 1);
    if expect_code(&v, FuzzError::LocaOrder) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, CMAP_OFF, 6);
    if expect_code(&v, FuzzError::CmapFormat) {
        caught += 1;
    }
    let mut v = base.clone();
    if let Some(s) = v.get_mut(GLYF_OFF + 5) {
        *s ^= 0xFF;
    }
    if expect_code(&v, FuzzError::Checksum) {
        caught += 1;
    }
    let mut v = base.clone();
    put_u16(&mut v, LOCA_OFF + 8, 40);
    if expect_code(&v, FuzzError::GlyphRange) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 运行 UNX-F50092 的 14 条判据。
pub fn run_unx50092_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50092");
    let font = build_minimal_font();

    let parsed = parse_sfnt(&font);
    let (ok, det) = match &parsed {
        Ok(f) => {
            let ok = f.num_tables == 5
                && f.units_per_em == 1000
                && f.num_glyphs == 4
                && !f.loca_long
                && f.cmap_format == 4
                && f.glyf_len == 32;
            (ok, format!("tables={} glyphs={}", f.num_tables, f.num_glyphs))
        }
        Err(e) => (false, format!("err={}", e.code())),
    };
    s.add_dyn("50092-J1-种子解析", ok, &det);

    let digest_a = match &parsed {
        Ok(f) => f.digest,
        Err(_) => 0,
    };
    let digest_b = match parse_sfnt(&font) {
        Ok(f) => f.digest,
        Err(_) => 1,
    };
    let ok = digest_a != 0 && digest_a == digest_b;
    s.add("50092-J1-解析确定", ok, if ok { "" } else { "两次摘要不同" });

    let mut prefixes_rejected = true;
    let mut p = 0usize;
    while p < font.len() {
        if let Some(pre) = font.get(..p) {
            if parse_sfnt(pre).is_ok() {
                prefixes_rejected = false;
                break;
            }
        }
        p += 1;
    }
    let ok = prefixes_rejected && parse_sfnt(&font).is_ok();
    let det = format!("prefix<{} 全拒", font.len());
    s.add_dyn("50092-J1-截断必拒", ok, &det);

    let ok = match &parsed {
        Ok(f) => {
            f.glyph_extent(0) == Ok((0, 8))
                && f.glyph_extent(2) == Ok((16, 24))
                && f.glyph_extent(3) == Ok((24, 32))
        }
        Err(_) => false,
    };
    s.add("50092-J1-字形区间", ok, if ok { "" } else { "loca 区间错" });

    let ok = match &parsed {
        Ok(f) => f.glyph_extent(4).err() == Some(FuzzError::GlyphId),
        Err(_) => false,
    };
    s.add("50092-J1-字形越界", ok, if ok { "" } else { "越界未拒" });

    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50092-J1-注入必抓", ok, &det);

    let mut c1 = FuzzCampaign::new(0x5EED);
    let r1 = c1.execute(10_000);
    let (ok, det) = match &r1 {
        Ok(r) => {
            let ok = r.iterations == 10_000
                && r.accepted + r.rejected == 10_000
                && r.out_of_set == 0
                && r.accepted >= 1
                && r.rejected >= 1;
            (
                ok,
                format!(
                    "acc={} rej={} oos={}",
                    r.accepted, r.rejected, r.out_of_set
                ),
            )
        }
        Err(e) => (false, format!("err={}", e.code())),
    };
    s.add_dyn("50092-J1-fuzz万次零崩", ok, &det);

    let mut c2 = FuzzCampaign::new(0x5EED);
    let r2 = c2.execute(10_000);
    let ok = match (&r1, &r2) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    s.add("50092-J1-fuzz可复跑", ok, if ok { "" } else { "同种子报告不同" });

    let ok = c1.stage() == FuzzStage::Judged && c2.stage() == FuzzStage::Judged;
    s.add("50092-J1-状态到judged", ok, if ok { "" } else { "档位未推进" });

    let ok = c1.execute(1).err() == Some(FuzzError::Order);
    s.add("50092-J1-重复执行拒", ok, if ok { "" } else { "重复执行被放行" });

    let all = [
        FuzzError::Empty,
        FuzzError::Magic,
        FuzzError::Truncated,
        FuzzError::TableCount,
        FuzzError::DirOverflow,
        FuzzError::TableCut,
        FuzzError::MissingTable,
        FuzzError::DuplicateTag,
        FuzzError::HeadShort,
        FuzzError::HeadMagic,
        FuzzError::HeadUnits,
        FuzzError::MaxpGlyphs,
        FuzzError::LocaFormat,
        FuzzError::LocaShort,
        FuzzError::LocaOrder,
        FuzzError::CmapFormat,
        FuzzError::Checksum,
        FuzzError::GlyphRange,
        FuzzError::GlyphId,
        FuzzError::Order,
    ];
    let mut unique = true;
    let mut in_range = true;
    let mut i = 0usize;
    while let Some(a) = all.get(i) {
        let ca = a.code();
        if ca < SEG_BASE + 1 || ca > SEG_BASE + 20 {
            in_range = false;
        }
        let mut j = i + 1;
        while let Some(b) = all.get(j) {
            if b.code() == ca {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = unique && in_range && all.len() == 20;
    let det = format!("codes={} 唯一={}", all.len(), unique);
    s.add_dyn("50092-J1-码闭集", ok, &det);

    let ok = FuzzError::Checksum.message().len() > 0
        && FuzzError::Checksum.code() == SEG_BASE + E_CHECKSUM as u32;
    s.add("50092-J1-码值稳定", ok, if ok { "" } else { "码值漂移" });

    let ok = match parse_sfnt(&font) {
        Ok(f) => f.num_tables == 5,
        Err(_) => false,
    } && parse_sfnt(&[]).err() == Some(FuzzError::Empty);
    s.add("50092-J1-空与完好", ok, if ok { "" } else { "端点行为错" });

    let digest_empty = fnv1a64(&[]);
    let ok = digest_empty == 0xcbf2_9ce4_8422_2325u64 && fnv1a64(&[0u8]) != digest_empty;
    s.add("50092-J1-摘要敏感", ok, if ok { "" } else { "摘要不敏感" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50092_checks_green() {
        let set = run_unx50092_checks();
        assert!(set.all_passed(), "UNX-F50092 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50092_fuzz_small_no_panic() {
        let mut c = FuzzCampaign::new(7);
        match c.execute(500) {
            Ok(r) => {
                assert_eq!(r.iterations, 500);
                assert_eq!(r.out_of_set, 0);
                assert_eq!(r.accepted + r.rejected, 500);
            }
            Err(e) => assert!(false, "campaign failed: {}", e.code()),
        }
    }
}
