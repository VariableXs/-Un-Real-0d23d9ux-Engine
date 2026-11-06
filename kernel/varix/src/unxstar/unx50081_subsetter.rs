//! UNX-F50081 字体子集化器（字形裁剪到实际使用集）（0x43640 段）
//!
//! 锚点：UNX-F50081-J1 子集化 ×20 字形集逐字形比对一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段：`SEG_BASE = 0x43640`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 11`，
//! 逐项见 [`FontError`]。
//!
//! 打印文档嵌入字体前必须把字形裁到**实际使用的字符集**，否则一个 CJK 字体
//! 动辄几 MB。裁剪的正确性标准不是「文件变小」，而是「每个用到的字符，在
//! 子集里拿到的轮廓与前进宽度必须与原字体逐字节一致」——J1 就是按这个口径
//! 对 20 组字形集逐字形比对。
//!
//! 二进制格式（GSF1，大端；测试与解析共用同一编码器）：
//!
//! ```text
//! magic "GSF1" | glyph_count u16 | cmap_count u16
//! cmap ×N      | cp u32 | gid u16
//! glyph ×N     | gid u16 | advance u16 | outline_len u16 | outline[..]
//! ```
//!
//! 关键纪律：
//! * gid 0 必须是第一条（.notdef），且只能出现一次；子集永远保留 .notdef；
//! * 子集按**源字形顺序**重排，新 gid = 保留序下标（稳定、可复算）；
//! * cmap 只保留「指向被保留 gid」的条目，未用字符彻底消失；
//! * 未映射字符显式 `Missing`，绝不静默回退到 .notdef（那会印出豆腐块）；
//! * 解析器每次读取先查区间，越界一律 `Trunc`，内核路径零 panic。
//!
//! 结构清单：
//! * [`Font`]：源字体（字形表 + cmap），带 parse / render / subset；
//! * [`SubsetFont`]：子集结果（紧凑 gid + 重映射表 + 新 cmap）；
//! * [`Glyph`] / [`CmapEntry`] / [`Rendered`]：字形、映射与渲染结果；
//! * [`injection_core`]：11 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43640 独占 0x20）。
pub const SEG_BASE: u32 = 0x43640;

const E_MAGIC: u16 = 1;
const E_TRUNC: u16 = 2;
const E_COUNT: u16 = 3;
const E_GID: u16 = 4;
const E_DUP_GID: u16 = 5;
const E_CMAP_DUP: u16 = 6;
const E_CMAP_GID: u16 = 7;
const E_ADVANCE: u16 = 8;
const E_NODEF: u16 = 9;
const E_MISSING: u16 = 10;
const E_TEXT: u16 = 11;

/// 字体子集化错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    /// 魔数不是 GSF1。
    Magic,
    /// 缓冲在读取过程中耗尽。
    Truncated,
    /// 字形数 / cmap 数不合法（0 或超上限）。
    Count,
    /// 非首条出现 gid 0。
    Gid,
    /// gid 重复。
    DuplicateGid,
    /// 同一码位在 cmap 里出现两次。
    CmapDuplicate,
    /// cmap 指向不存在的 gid。
    CmapGid,
    /// 前进宽度为 0 或超上限。
    Advance,
    /// 第一条不是 .notdef（gid 0）。
    NoNotdef,
    /// 使用的字符没有字形。
    Missing,
    /// 使用文本超长。
    Text,
}

impl FontError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            FontError::Magic => E_MAGIC,
            FontError::Truncated => E_TRUNC,
            FontError::Count => E_COUNT,
            FontError::Gid => E_GID,
            FontError::DuplicateGid => E_DUP_GID,
            FontError::CmapDuplicate => E_CMAP_DUP,
            FontError::CmapGid => E_CMAP_GID,
            FontError::Advance => E_ADVANCE,
            FontError::NoNotdef => E_NODEF,
            FontError::Missing => E_MISSING,
            FontError::Text => E_TEXT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            FontError::Magic => "font magic is not GSF1",
            FontError::Truncated => "font buffer truncated",
            FontError::Count => "glyph or cmap count out of range",
            FontError::Gid => "gid zero appears after the first glyph",
            FontError::DuplicateGid => "glyph id duplicated",
            FontError::CmapDuplicate => "codepoint mapped twice",
            FontError::CmapGid => "cmap points to missing glyph",
            FontError::Advance => "advance width zero or too large",
            FontError::NoNotdef => "first glyph is not .notdef (gid 0)",
            FontError::Missing => "used codepoint has no glyph",
            FontError::Text => "used text exceeds 256 characters",
        }
    }
}

/// 单字体最多字形数（纸张文档内嵌子集足够）。
pub const MAX_GLYPHS: usize = 64;
/// cmap 条目上限。
pub const MAX_CMAP: usize = 64;
/// 前进宽度上限（1/1000 em）。
pub const MAX_ADVANCE: u16 = 4096;
/// 一次子集化接受的最大字符数。
pub const MAX_TEXT_CHARS: usize = 256;
/// 格式魔数。
pub const MAGIC: [u8; 4] = [b'G', b'S', b'F', b'1'];
/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 一个字形。`gid` 在子集中会被重编号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// 字形号。
    pub gid: u16,
    /// 前进宽度（1/1000 em）。
    pub advance: u16,
    /// 轮廓字节（本模块只要求逐字节可比较，不解释内容）。
    pub outline: Vec<u8>,
}

/// 码位 → 字形号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmapEntry {
    /// Unicode 码位。
    pub cp: u32,
    /// 字形号。
    pub gid: u16,
}

/// 渲染结果：一次字符取形的可比较快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    /// 码位。
    pub cp: u32,
    /// 字形号（子集中是新 gid）。
    pub gid: u16,
    /// 前进宽度。
    pub advance: u16,
    /// 轮廓字节。
    pub outline: Vec<u8>,
}

impl Rendered {
    /// 形状等价：码位 / 宽度 / 轮廓逐字节一致（gid 允许因重编号不同）。
    pub fn same_shape(&self, other: &Rendered) -> bool {
        self.cp == other.cp && self.advance == other.advance && self.outline == other.outline
    }
}

fn rd_u16(b: &[u8], at: usize) -> Result<u16, FontError> {
    let hi = match b.get(at) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    let lo = match b.get(at + 1) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    Ok(((hi as u16) << 8) | lo as u16)
}

fn rd_u32(b: &[u8], at: usize) -> Result<u32, FontError> {
    let a = match b.get(at) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    let b1 = match b.get(at + 1) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    let c = match b.get(at + 2) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    let d = match b.get(at + 3) {
        Some(v) => *v,
        None => return Err(FontError::Truncated),
    };
    Ok(((a as u32) << 24) | ((b1 as u32) << 16) | ((c as u32) << 8) | d as u32)
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

/// 编码成 GSF1 二进制。顺序与 `parse` 严格互逆。
pub fn encode_font(glyphs: &[Glyph], cmap: &[CmapEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    push_u16(&mut out, glyphs.len() as u16);
    push_u16(&mut out, cmap.len() as u16);
    let mut i = 0usize;
    while i < cmap.len() {
        if let Some(e) = cmap.get(i) {
            push_u32(&mut out, e.cp);
            push_u16(&mut out, e.gid);
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < glyphs.len() {
        if let Some(g) = glyphs.get(j) {
            push_u16(&mut out, g.gid);
            push_u16(&mut out, g.advance);
            push_u16(&mut out, g.outline.len() as u16);
            out.extend_from_slice(&g.outline);
        }
        j += 1;
    }
    out
}

fn find_cmap(cmap: &[CmapEntry], cp: u32) -> Option<u16> {
    let mut i = 0usize;
    while i < cmap.len() {
        if let Some(e) = cmap.get(i) {
            if e.cp == cp {
                return Some(e.gid);
            }
        }
        i += 1;
    }
    None
}

fn find_glyph(glyphs: &[Glyph], gid: u16) -> Option<&Glyph> {
    let mut i = 0usize;
    while i < glyphs.len() {
        if let Some(g) = glyphs.get(i) {
            if g.gid == gid {
                return Some(g);
            }
        }
        i += 1;
    }
    None
}

fn contains_gid(list: &[u16], gid: u16) -> bool {
    let mut i = 0usize;
    while i < list.len() {
        if let Some(v) = list.get(i) {
            if *v == gid {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn contains_cp(list: &[u32], cp: u32) -> bool {
    let mut i = 0usize;
    while i < list.len() {
        if let Some(v) = list.get(i) {
            if *v == cp {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn render_with(
    glyphs: &[Glyph],
    cmap: &[CmapEntry],
    text: &str,
) -> Result<Vec<Rendered>, FontError> {
    let mut out = Vec::new();
    let mut n = 0usize;
    let mut it = text.chars();
    loop {
        let c = match it.next() {
            Some(v) => v,
            None => break,
        };
        n += 1;
        if n > MAX_TEXT_CHARS {
            return Err(FontError::Text);
        }
        let cp = c as u32;
        let gid = match find_cmap(cmap, cp) {
            Some(g) => g,
            None => return Err(FontError::Missing),
        };
        let g = match find_glyph(glyphs, gid) {
            Some(v) => v,
            None => return Err(FontError::Missing),
        };
        out.push(Rendered { cp, gid, advance: g.advance, outline: g.outline.clone() });
    }
    Ok(out)
}

/// 源字体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font {
    glyphs: Vec<Glyph>,
    cmap: Vec<CmapEntry>,
}

impl Font {
    /// 解析 GSF1 二进制。所有越界 / 结构错误显式返回。
    pub fn parse(bytes: &[u8]) -> Result<Font, FontError> {
        let magic = match bytes.get(0..4) {
            Some(m) => m,
            None => return Err(FontError::Truncated),
        };
        if magic != &MAGIC[..] {
            return Err(FontError::Magic);
        }
        let gc = rd_u16(bytes, 4)? as usize;
        let cc = rd_u16(bytes, 6)? as usize;
        if gc == 0 || gc > MAX_GLYPHS || cc > MAX_CMAP {
            return Err(FontError::Count);
        }
        let mut at = 8usize;
        let mut cmap = Vec::with_capacity(cc);
        let mut i = 0usize;
        while i < cc {
            let cp = rd_u32(bytes, at)?;
            let gid = rd_u16(bytes, at + 4)?;
            at += 6;
            if contains_cp(&cmap_cps(&cmap), cp) {
                return Err(FontError::CmapDuplicate);
            }
            cmap.push(CmapEntry { cp, gid });
            i += 1;
        }
        let mut glyphs: Vec<Glyph> = Vec::with_capacity(gc);
        let mut j = 0usize;
        while j < gc {
            let gid = rd_u16(bytes, at)?;
            let advance = rd_u16(bytes, at + 2)?;
            let len = rd_u16(bytes, at + 4)? as usize;
            at += 6;
            let slice = match bytes.get(at..at + len) {
                Some(s) => s,
                None => return Err(FontError::Truncated),
            };
            let outline = slice.to_vec();
            at += len;
            if advance == 0 || advance > MAX_ADVANCE {
                return Err(FontError::Advance);
            }
            if gid == 0 && j != 0 {
                return Err(FontError::Gid);
            }
            let mut k = 0usize;
            while k < glyphs.len() {
                if let Some(g) = glyphs.get(k) {
                    if g.gid == gid {
                        return Err(FontError::DuplicateGid);
                    }
                }
                k += 1;
            }
            glyphs.push(Glyph { gid, advance, outline });
            j += 1;
        }
        // 第一条必须是 .notdef，否则子集没有兜底字形。
        match glyphs.first() {
            Some(g) if g.gid == 0 => {}
            _ => return Err(FontError::NoNotdef),
        }
        // cmap 不得指向缺号字形。
        let mut m = 0usize;
        while m < cmap.len() {
            if let Some(e) = cmap.get(m) {
                if find_glyph(&glyphs, e.gid).is_none() {
                    return Err(FontError::CmapGid);
                }
            }
            m += 1;
        }
        Ok(Font { glyphs, cmap })
    }

    /// 字形数。
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// cmap 条目数。
    pub fn cmap_len(&self) -> usize {
        self.cmap.len()
    }

    /// 读一个字形。
    pub fn glyph(&self, idx: usize) -> Option<&Glyph> {
        self.glyphs.get(idx)
    }

    /// 按码位查字形号。
    pub fn lookup(&self, cp: u32) -> Option<u16> {
        find_cmap(&self.cmap, cp)
    }

    /// 渲染文本（用于与子集结果逐字形比对）。
    pub fn render(&self, text: &str) -> Result<Vec<Rendered>, FontError> {
        render_with(&self.glyphs, &self.cmap, text)
    }

    /// 子集化：只保留 .notdef 与实际使用字符的字形，按源顺序重编号。
    pub fn subset(&self, used: &str) -> Result<SubsetFont, FontError> {
        let mut cps: Vec<u32> = Vec::new();
        let mut n = 0usize;
        let mut it = used.chars();
        loop {
            let c = match it.next() {
                Some(v) => v,
                None => break,
            };
            n += 1;
            if n > MAX_TEXT_CHARS {
                return Err(FontError::Text);
            }
            let cp = c as u32;
            if !contains_cp(&cps, cp) {
                cps.push(cp);
            }
        }
        let mut keep: Vec<u16> = Vec::new();
        keep.push(0);
        let mut i = 0usize;
        while i < cps.len() {
            let cp = match cps.get(i) {
                Some(v) => *v,
                None => break,
            };
            let gid = match self.lookup(cp) {
                Some(g) => g,
                None => return Err(FontError::Missing),
            };
            if !contains_gid(&keep, gid) {
                keep.push(gid);
            }
            i += 1;
        }
        let mut glyphs: Vec<Glyph> = Vec::new();
        let mut map: Vec<(u16, u16)> = Vec::new();
        let mut gi = 0usize;
        while gi < self.glyphs.len() {
            let g = match self.glyphs.get(gi) {
                Some(v) => v,
                None => break,
            };
            if contains_gid(&keep, g.gid) {
                let new_gid = glyphs.len() as u16;
                glyphs.push(Glyph { gid: new_gid, advance: g.advance, outline: g.outline.clone() });
                map.push((g.gid, new_gid));
            }
            gi += 1;
        }
        let mut cmap: Vec<CmapEntry> = Vec::new();
        let mut ci = 0usize;
        while ci < self.cmap.len() {
            let e = match self.cmap.get(ci) {
                Some(v) => v,
                None => break,
            };
            if contains_gid(&keep, e.gid) {
                let mut ni = 0usize;
                while ni < map.len() {
                    if let Some(pair) = map.get(ni) {
                        if pair.0 == e.gid {
                            cmap.push(CmapEntry { cp: e.cp, gid: pair.1 });
                            break;
                        }
                    }
                    ni += 1;
                }
            }
            ci += 1;
        }
        Ok(SubsetFont { glyphs, cmap, old_to_new: map })
    }
}

fn cmap_cps(cmap: &[CmapEntry]) -> Vec<u32> {
    let mut out = Vec::with_capacity(cmap.len());
    let mut i = 0usize;
    while i < cmap.len() {
        if let Some(e) = cmap.get(i) {
            out.push(e.cp);
        }
        i += 1;
    }
    out
}

/// 子集字体：gid 已压紧，cmap 只覆盖保留字形。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubsetFont {
    glyphs: Vec<Glyph>,
    cmap: Vec<CmapEntry>,
    old_to_new: Vec<(u16, u16)>,
}

impl SubsetFont {
    /// 字形数（含 .notdef）。
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// cmap 条目数。
    pub fn cmap_len(&self) -> usize {
        self.cmap.len()
    }

    /// 读一个字形。
    pub fn glyph(&self, idx: usize) -> Option<&Glyph> {
        self.glyphs.get(idx)
    }

    /// 按码位查（新）字形号。
    pub fn lookup(&self, cp: u32) -> Option<u16> {
        find_cmap(&self.cmap, cp)
    }

    /// 旧 gid → 新 gid。
    pub fn new_gid_of(&self, old: u16) -> Option<u16> {
        let mut i = 0usize;
        while i < self.old_to_new.len() {
            if let Some(p) = self.old_to_new.get(i) {
                if p.0 == old {
                    return Some(p.1);
                }
            }
            i += 1;
        }
        None
    }

    /// 渲染文本（与源字体比对用）。
    pub fn render(&self, text: &str) -> Result<Vec<Rendered>, FontError> {
        render_with(&self.glyphs, &self.cmap, text)
    }

    /// 编码回 GSF1（子集本身也是合法字体，可再次解析）。
    pub fn to_bytes(&self) -> Vec<u8> {
        encode_font(&self.glyphs, &self.cmap)
    }

    /// 指纹：字形表 + cmap 的 FNV-1a 折叠（确定性判据用）。
    pub fn checksum(&self) -> u32 {
        fingerprint(&self.glyphs, &self.cmap)
    }
}

fn fingerprint(glyphs: &[Glyph], cmap: &[CmapEntry]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0usize;
    while i < glyphs.len() {
        if let Some(g) = glyphs.get(i) {
            h = fold_u16(h, g.gid);
            h = fold_u16(h, g.advance);
            h = fold_u16(h, g.outline.len() as u16);
            let mut k = 0usize;
            while k < g.outline.len() {
                if let Some(b) = g.outline.get(k) {
                    h = (h ^ *b as u32).wrapping_mul(FNV_PRIME);
                }
                k += 1;
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < cmap.len() {
        if let Some(e) = cmap.get(j) {
            h = (h ^ (e.cp >> 24) as u32).wrapping_mul(FNV_PRIME);
            h = (h ^ ((e.cp >> 16) & 0xff)).wrapping_mul(FNV_PRIME);
            h = (h ^ ((e.cp >> 8) & 0xff)).wrapping_mul(FNV_PRIME);
            h = (h ^ (e.cp & 0xff)).wrapping_mul(FNV_PRIME);
            h = fold_u16(h, e.gid);
        }
        j += 1;
    }
    h
}

fn fold_u16(h: u32, v: u16) -> u32 {
    let a = (h ^ (v >> 8) as u32).wrapping_mul(FNV_PRIME);
    (a ^ (v & 0xff) as u32).wrapping_mul(FNV_PRIME)
}

// ---------------------------------------------------------------------------
// 样本字体与注入核
// ---------------------------------------------------------------------------

/// 生成一组样本字体字节：seed<15 用 ASCII 码位，否则用 CJK 码位。
pub fn sample_bytes(seed: u8) -> Vec<u8> {
    let n = 8usize + (seed as usize % 5);
    let base: u32 = if seed < 15 { 0x41 } else { 0x4E00 };
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(n);
    glyphs.push(Glyph { gid: 0, advance: 600, outline: vec![0u8, 0u8, 0u8, 0u8] });
    let mut i = 1usize;
    while i < n {
        let gid = i as u16;
        let advance = 400 + gid * 7 + seed as u16;
        let outline = vec![seed, gid as u8, 0xA0, seed ^ gid as u8];
        glyphs.push(Glyph { gid, advance, outline });
        i += 1;
    }
    let mut cmap: Vec<CmapEntry> = Vec::with_capacity(n);
    let mut j = 1usize;
    while j < n {
        cmap.push(CmapEntry { cp: base + (j as u32 - 1), gid: j as u16 });
        j += 1;
    }
    encode_font(&glyphs, &cmap)
}

/// 解析后的样本字体；生成路径不可能失败，失败时退化为空字体而不是 panic。
pub fn sample_font(seed: u8) -> Font {
    match Font::parse(&sample_bytes(seed)) {
        Ok(f) => f,
        Err(_) => Font { glyphs: Vec::new(), cmap: Vec::new() },
    }
}

/// 取字体 cmap 前 k 个码位组成使用文本（用于 20 组比对）。
pub fn used_text(font: &Font, k: usize) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < k {
        if let Some(e) = font.cmap.get(i) {
            if let Some(c) = char::from_u32(e.cp) {
                out.push(c);
            }
        }
        i += 1;
    }
    out
}

/// 坏样本注入核：11 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 11usize;

    // 1 魔数错误
    let bad_magic = [b'N', b'O', b'P', b'E', 0u8, 0, 0, 0];
    if Font::parse(&bad_magic).err() == Some(FontError::Magic) {
        caught += 1;
    }
    // 2 截断（前 6 字节）
    let valid = sample_bytes(0);
    let mut cut: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < 6 {
        if let Some(b) = valid.get(i) {
            cut.push(*b);
        }
        i += 1;
    }
    if Font::parse(&cut).err() == Some(FontError::Truncated) {
        caught += 1;
    }
    // 3 零字形
    if Font::parse(&encode_font(&[], &[])).err() == Some(FontError::Count) {
        caught += 1;
    }
    // 4 非首条 gid 0
    let g0 = Glyph { gid: 0, advance: 500, outline: vec![0u8] };
    let g0b = Glyph { gid: 0, advance: 500, outline: vec![1u8] };
    if Font::parse(&encode_font(&[g0.clone(), g0b], &[])).err() == Some(FontError::Gid) {
        caught += 1;
    }
    // 5 gid 重复
    let ga = Glyph { gid: 5, advance: 500, outline: vec![2u8] };
    let gb = Glyph { gid: 5, advance: 600, outline: vec![3u8] };
    if Font::parse(&encode_font(&[g0.clone(), ga, gb], &[])).err() == Some(FontError::DuplicateGid) {
        caught += 1;
    }
    // 6 cmap 重复码位
    let gc = Glyph { gid: 1, advance: 500, outline: vec![4u8] };
    let dup_cmap = [CmapEntry { cp: 65, gid: 1 }, CmapEntry { cp: 65, gid: 1 }];
    if Font::parse(&encode_font(&[g0.clone(), gc.clone()], &dup_cmap)).err()
        == Some(FontError::CmapDuplicate)
    {
        caught += 1;
    }
    // 7 cmap 指向缺失 gid
    let miss_cmap = [CmapEntry { cp: 65, gid: 9 }];
    if Font::parse(&encode_font(&[g0.clone(), gc.clone()], &miss_cmap)).err()
        == Some(FontError::CmapGid)
    {
        caught += 1;
    }
    // 8 前进宽度为 0
    let zero_adv = Glyph { gid: 1, advance: 0, outline: vec![5u8] };
    if Font::parse(&encode_font(&[g0.clone(), zero_adv], &[])).err() == Some(FontError::Advance) {
        caught += 1;
    }
    // 9 首条不是 .notdef
    let no_notdef = Glyph { gid: 1, advance: 500, outline: vec![6u8] };
    if Font::parse(&encode_font(&[no_notdef], &[])).err() == Some(FontError::NoNotdef) {
        caught += 1;
    }
    // 10 使用字符缺字形
    if sample_font(0).subset("Z").err() == Some(FontError::Missing) {
        caught += 1;
    }
    // 11 使用文本超长
    let mut long = String::new();
    let mut k = 0usize;
    while k < MAX_TEXT_CHARS + 1 {
        long.push('A');
        k += 1;
    }
    if sample_font(0).subset(&long).err() == Some(FontError::Text) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn glyph_sets_consistent() -> (usize, usize) {
    // 返回 (一致组数, 组总数)。逐字形比对宽度与轮廓字节。
    let mut matched = 0usize;
    let total = 20usize;
    let mut seed = 0u8;
    while seed < total as u8 {
        let font = sample_font(seed);
        let text = used_text(&font, 3);
        let orig = font.render(&text);
        // 子集先取形再渲染：两侧都是 Rendered 序列，可直接逐字形比。
        let sub = match font.subset(&text) {
            Ok(v) => v.render(&text),
            Err(_) => Err(FontError::Missing),
        };
        let mut good = orig.is_ok() && sub.is_ok();
        if let (Ok(a), Ok(b)) = (&orig, &sub) {
            if a.len() != b.len() {
                good = false;
            } else {
                let mut i = 0usize;
                while i < a.len() {
                    if let (Some(x), Some(y)) = (a.get(i), b.get(i)) {
                        if !x.same_shape(y) {
                            good = false;
                        }
                    } else {
                        good = false;
                    }
                    i += 1;
                }
            }
        }
        if good {
            matched += 1;
        }
        seed += 1;
    }
    (matched, total)
}

/// 跑 UNX-F50081 的 16 条判据（含 J1 二十组逐字形比对与注入核）。
pub fn run_unx50081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50081");

    // J1-1 二十组字形集逐字形比对一致（宽度 + 轮廓字节）。
    let (matched, total) = glyph_sets_consistent();
    let ok = matched == total;
    let det = format!("一致 {}/{}", matched, total);
    s.add_dyn("50081-J1-二十组逐字形", ok, &det);

    let font = sample_font(7);
    let text = used_text(&font, 4);
    let sub = match font.subset(&text) {
        Ok(v) => v,
        // 样本字体构造保证可子集化；万一失败也退化为空子集，让判据显红而非 panic。
        Err(_) => SubsetFont { glyphs: Vec::new(), cmap: Vec::new(), old_to_new: Vec::new() },
    };

    // J1-2 裁剪生效：子集字形数 = 使用唯一字数 + .notdef，且小于原字体。
    let ok = sub.glyph_count() == 5 && sub.glyph_count() < font.glyph_count();
    let det = format!("sub={} src={}", sub.glyph_count(), font.glyph_count());
    s.add_dyn("50081-J1-裁剪生效", ok, &det);

    // J1-3 .notdef 保留：子集 0 号字形仍是 gid 0。
    let ok = match sub.glyph(0) {
        Some(g) => g.gid == 0,
        None => false,
    };
    s.add("50081-J1-notdef保留", ok, if ok { "" } else { "子集丢失 notdef" });

    // J1-4 使用字符全覆盖：文本每个字符都能在子集里查到。
    let mut cov = true;
    let mut it = text.chars();
    loop {
        let c = match it.next() {
            Some(v) => v,
            None => break,
        };
        if sub.lookup(c as u32).is_none() {
            cov = false;
        }
    }
    s.add("50081-J1-使用全覆盖", cov, if cov { "" } else { "子集缺使用字符" });

    // J1-5 未用字符裁掉：原字体里存在但未使用的码位，在子集中查不到。
    let mut dropped = true;
    let mut seen = 0usize;
    let mut i = 0usize;
    while i < font.cmap_len() {
        if let Some(e) = font.cmap.get(i) {
            let used = text.chars().any(|c| c as u32 == e.cp);
            if !used {
                seen += 1;
                if sub.lookup(e.cp).is_some() {
                    dropped = false;
                }
            }
        }
        i += 1;
    }
    let ok = dropped && seen > 0;
    let det = format!("未用={} 全裁={}", seen, dropped as u8);
    s.add_dyn("50081-J1-未用裁掉", ok, &det);

    // J1-6 二进制往返：子集编码 → 解析 → 渲染与子集直接渲染一致。
    let bytes = sub.to_bytes();
    let reparsed = Font::parse(&bytes);
    let ok = match &reparsed {
        Ok(f) => match (sub.render(&text), f.render(&text)) {
            (Ok(a), Ok(b)) => a == b && a.len() == text.chars().count(),
            _ => false,
        },
        Err(_) => false,
    };
    s.add("50081-J1-子集往返", ok, if ok { "" } else { "子集编码解析不一致" });

    // J1-7 幂等：同一字体做两次子集，指纹与字形数完全一致。
    let again = font.subset(&text);
    let ok = match &again {
        Ok(v) => v.checksum() == sub.checksum() && v.glyph_count() == sub.glyph_count(),
        Err(_) => false,
    };
    s.add("50081-J1-子集幂等", ok, if ok { "" } else { "两次子集不一致" });

    // J1-8 二级子集：子集解析后再子集化，结果不变（紧凑 gid 稳定）。
    let ok = match &reparsed {
        Ok(f) => match f.subset(&text) {
            Ok(v) => v.checksum() == sub.checksum() && v.glyph_count() == sub.glyph_count(),
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50081-J1-二级子集", ok, if ok { "" } else { "子集二次裁剪漂移" });

    // J1-9 重映射单调：保留字形的新 gid 严格按源顺序递增。
    let mut monotone = true;
    let mut last = 0u16;
    let mut gi = 1usize;
    while gi < sub.glyph_count() {
        match sub.glyph(gi) {
            Some(g) => {
                if g.gid <= last {
                    monotone = false;
                }
                last = g.gid;
            }
            None => monotone = false,
        }
        gi += 1;
    }
    s.add("50081-J1-重映射单调", monotone, if monotone { "" } else { "新 gid 非递增" });

    // J1-10 未映射字符必拒（不静默回退 notdef）。
    let ok = font.subset("#").err() == Some(FontError::Missing);
    s.add("50081-J1-缺字必拒", ok, if ok { "" } else { "缺字被静默接受" });

    // J1-11 文本超长必拒（257 字符）。
    let mut long = String::new();
    let mut k = 0usize;
    while k < MAX_TEXT_CHARS + 1 {
        long.push('A');
        k += 1;
    }
    let ok = font.subset(&long).err() == Some(FontError::Text);
    s.add("50081-J1-文本上限", ok, if ok { "" } else { "超长文本被接受" });

    // J1-12 解析器坏样本：魔数 / 截断 / 零字形 / 无 notdef。
    let bad_magic = [b'X', b'X', b'X', b'X', 0u8, 0, 0, 0];
    let no_notdef = Glyph { gid: 3, advance: 500, outline: vec![1u8] };
    let ok = Font::parse(&bad_magic).err() == Some(FontError::Magic)
        && Font::parse(&[b'G', b'S', b'F', b'1', 0u8]).err() == Some(FontError::Truncated)
        && Font::parse(&encode_font(&[], &[])).err() == Some(FontError::Count)
        && Font::parse(&encode_font(&[no_notdef], &[])).err() == Some(FontError::NoNotdef);
    s.add("50081-J1-解析必拒", ok, if ok { "" } else { "坏字体被受理" });

    // J1-13 结构性坏样本：重复 gid / 重复码位 / 悬空 cmap / 零宽度。
    let g0 = Glyph { gid: 0, advance: 600, outline: vec![0u8] };
    let d1 = Glyph { gid: 2, advance: 500, outline: vec![1u8] };
    let d2 = Glyph { gid: 2, advance: 600, outline: vec![2u8] };
    let zero_adv = Glyph { gid: 1, advance: 0, outline: vec![3u8] };
    let cmap_dup = [CmapEntry { cp: 65, gid: 1 }, CmapEntry { cp: 65, gid: 2 }];
    let cmap_gap = [CmapEntry { cp: 65, gid: 9 }];
    let one = Glyph { gid: 1, advance: 500, outline: vec![4u8] };
    let ok = Font::parse(&encode_font(&[g0.clone(), d1, d2], &[])).err()
        == Some(FontError::DuplicateGid)
        && Font::parse(&encode_font(&[g0.clone(), one.clone(), Glyph { gid: 2, advance: 500, outline: vec![5u8] }], &cmap_dup)).err()
            == Some(FontError::CmapDuplicate)
        && Font::parse(&encode_font(&[g0.clone(), one.clone()], &cmap_gap)).err()
            == Some(FontError::CmapGid)
        && Font::parse(&encode_font(&[g0.clone(), zero_adv], &[])).err() == Some(FontError::Advance);
    s.add("50081-J1-结构必拒", ok, if ok { "" } else { "结构坏样本漏抓" });

    // J1-14 编码往返：原始字节 → 解析 → 再编码，逐字节一致。
    let raw = sample_bytes(2);
    let ok = match Font::parse(&raw) {
        Ok(f) => match f.glyph(0) {
            Some(_) => {
                let mut glyphs: Vec<Glyph> = Vec::with_capacity(f.glyph_count());
                let mut i = 0usize;
                while i < f.glyph_count() {
                    if let Some(g) = f.glyph(i) {
                        glyphs.push(g.clone());
                    }
                    i += 1;
                }
                encode_font(&glyphs, &f.cmap) == raw
            }
            None => false,
        },
        Err(_) => false,
    };
    s.add("50081-J1-编码往返", ok, if ok { "" } else { "编码不互逆" });

    // J1-15 多字节（CJK）：seed 15..19 的码位是 3 字节 UTF-8，逐字形仍一致。
    let cjk = sample_font(16);
    let cjk_text = used_text(&cjk, 3);
    let cjk_sub = match cjk.subset(&cjk_text) {
        Ok(v) => v.render(&cjk_text),
        Err(_) => Err(FontError::Missing),
    };
    let ok = cjk_text.chars().count() == 3
        && cjk_text.len() > 3
        && match (cjk.render(&cjk_text), cjk_sub) {
            (Ok(a), Ok(b)) => {
                let mut eq = a.len() == b.len();
                let mut i = 0usize;
                while i < a.len() {
                    if let (Some(x), Some(y)) = (a.get(i), b.get(i)) {
                        if !x.same_shape(y) {
                            eq = false;
                        }
                    } else {
                        eq = false;
                    }
                    i += 1;
                }
                eq
            }
            _ => false,
        };
    let det = format!("chars={} bytes={}", cjk_text.chars().count(), cjk_text.len());
    s.add_dyn("50081-J1-CJK子集", ok, &det);

    // J1-16 确定性：同字体同文本两次子集，字节流逐字节一致。
    let ok = match (font.subset(&text), font.subset(&text)) {
        (Ok(a), Ok(b)) => a.to_bytes() == b.to_bytes() && a.checksum() == b.checksum(),
        _ => false,
    };
    s.add("50081-J1-子集确定", ok, if ok { "" } else { "子集字节流漂移" });

    // 注入核：11 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50081-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50081_checks_green() {
        let set = run_unx50081_checks();
        assert!(set.all_passed(), "UNX-F50081 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50081_subset_drops_unused() {
        let f = sample_font(5);
        let t = used_text(&f, 2);
        let sub = f.subset(&t);
        assert!(sub.is_ok());
        match sub {
            Ok(s) => {
                assert_eq!(s.glyph_count(), 3);
                let first_cp = match t.chars().next() {
                    Some(c) => c as u32,
                    None => 0,
                };
                assert_eq!(s.lookup(first_cp), Some(1));
            }
            Err(_) => {}
        }
    }

    #[test]
    fn unx50081_parse_and_encode_roundtrip() {
        let raw = sample_bytes(9);
        let f = Font::parse(&raw);
        assert!(f.is_ok());
        match f {
            Ok(font) => {
                let mut glyphs = Vec::new();
                let mut i = 0usize;
                while i < font.glyph_count() {
                    if let Some(g) = font.glyph(i) {
                        glyphs.push(g.clone());
                    }
                    i += 1;
                }
                assert_eq!(encode_font(&glyphs, &font.cmap), raw);
            }
            Err(_) => {}
        }
    }
}
