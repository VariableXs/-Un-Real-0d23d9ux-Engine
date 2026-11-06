//! UNX-F50082 TrueType/Type1 双格式嵌入账（0x43660 段）
//!
//! 锚点：UNX-F50082-J1 双格式 ×10 文档嵌入往返一致；判据锚定 §AI-63 §2 专题二。
//!
//! 为什么两种格式必须共用一本账：打印驱动侧只关心「这份文档带了几份字体、
//! 以什么载体形态带、字节数多少、校验和是否对得上」。若 TrueType 走一条账、
//! Type1 走另一条账，封账时两边的计数与指纹各算各的，审计时无法证明
//! 「10 份文档里的字体全部原样到达」。本模块把两种格式压进同一个
//! `EmbedLedger`，用同一套 `EmbedSession` 状态机与同一套校验和口径。
//!
//! 结构清单：
//! * 解析器：`parse_sfnt`（sfnt 表目录）、`parse_type1`（PS 头 + eexec 区）；
//! * 载体：`carry_face` / `extract`（TT 二进制帧 / T1 ASCII-hex 帧）；
//! * 状态机：`EmbedSession` Idle→Validated→Carried→Committed；
//! * 账本：`EmbedLedger` append-only + 容量上限 + 封账 FNV 指纹；
//! * 注入核：`injection_core` 12 类坏样本逐类断言；
//! * 诊断码闭集：SEG_BASE = 0x43660，n ∈ [1,16]。
//!
//! 错误纪律：内核路径零 panic——所有越界读取先做区间校验，帧解析失败
//! 一律返回 `Result<_, FontError>`，绝不用默认值掩盖损坏。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43660 独占 0x20；F50083 从 0x43680 起顺序占用）。
pub const SEG_BASE: u32 = 0x43660;

pub const E_EMPTY: u16 = 1;
pub const E_SFNT_SHORT: u16 = 2;
pub const E_SFNT_MAGIC: u16 = 3;
pub const E_TABLE_COUNT: u16 = 4;
pub const E_TABLE_OOB: u16 = 5;
pub const E_T1_HEADER: u16 = 6;
pub const E_T1_TYPE: u16 = 7;
pub const E_EEDX: u16 = 8;
pub const E_FSTYPE: u16 = 9;
pub const E_SUBSET: u16 = 10;
pub const E_CARRIER: u16 = 11;
pub const E_CHECKSUM: u16 = 12;
pub const E_CAPACITY: u16 = 13;
pub const E_STATE: u16 = 14;
pub const E_DOC: u16 = 15;
pub const E_SEAL: u16 = 16;

/// 字体嵌入错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    Empty,
    SfntShort,
    SfntMagic,
    TableCount,
    TableOob,
    T1Header,
    T1Type,
    EexecLen,
    FsType,
    Subset,
    Carrier,
    Checksum,
    Capacity,
    State,
    Doc,
    Seal,
}

impl FontError {
    fn ordinal(self) -> u16 {
        match self {
            FontError::Empty => E_EMPTY,
            FontError::SfntShort => E_SFNT_SHORT,
            FontError::SfntMagic => E_SFNT_MAGIC,
            FontError::TableCount => E_TABLE_COUNT,
            FontError::TableOob => E_TABLE_OOB,
            FontError::T1Header => E_T1_HEADER,
            FontError::T1Type => E_T1_TYPE,
            FontError::EexecLen => E_EEDX,
            FontError::FsType => E_FSTYPE,
            FontError::Subset => E_SUBSET,
            FontError::Carrier => E_CARRIER,
            FontError::Checksum => E_CHECKSUM,
            FontError::Capacity => E_CAPACITY,
            FontError::State => E_STATE,
            FontError::Doc => E_DOC,
            FontError::Seal => E_SEAL,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FontError::Empty => "empty name or font bytes",
            FontError::SfntShort => "sfnt shorter than header",
            FontError::SfntMagic => "sfnt version magic unknown",
            FontError::TableCount => "sfnt table count out of range",
            FontError::TableOob => "sfnt table offset/size outside buffer",
            FontError::T1Header => "type1 %%! header or /FontName missing",
            FontError::T1Type => "type1 /FontType missing or not 1",
            FontError::EexecLen => "type1 eexec payload malformed",
            FontError::FsType => "fsType forbids this embedding mode",
            FontError::Subset => "fsType forbids subsetting",
            FontError::Carrier => "embedding carrier frame malformed",
            FontError::Checksum => "carrier checksum mismatch",
            FontError::Capacity => "embed ledger capacity exhausted",
            FontError::State => "illegal embed session transition",
            FontError::Doc => "document id zero",
            FontError::Seal => "embed ledger already sealed",
        }
    }
}

/// 字体容器格式。只有这两种进入嵌入账；CFF/OTTO 走 sfnt 分支但按 TrueType
/// 的二进制载体处理（文档侧只区分「二进制 sfnt」与「ASCII Type1」两种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontFormat {
    TrueType,
    Type1,
}

impl FontFormat {
    pub fn tag(self) -> u8 {
        match self {
            FontFormat::TrueType => b'T',
            FontFormat::Type1 => b'1',
        }
    }

    /// 从载体首字节判别格式；未知首字节显式拒绝，不做猜测。
    pub fn from_tag(t: u8) -> Result<FontFormat, FontError> {
        match t {
            b'T' => Ok(FontFormat::TrueType),
            b'1' => Ok(FontFormat::Type1),
            _ => Err(FontError::Carrier),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FontFormat::TrueType => "truetype",
            FontFormat::Type1 => "type1",
        }
    }
}

/// sfnt 表目录项。`len` 是声明长度，解析时必须与 `offset` 一起做越界校验：
/// 只看 offset 不看 len 会让「超长表」把后续标签的字节当成表体读进来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableEntry {
    pub tag: [u8; 4],
    pub offset: u32,
    pub len: u32,
}

/// sfnt 头 + 表目录的解析结果（不持有字节，避免重复拷贝大字体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SfntInfo {
    pub version: u32,
    pub num_tables: u16,
    pub tables: Vec<TableEntry>,
    pub bytes: usize,
}

impl SfntInfo {
    pub fn table(&self, tag: &[u8; 4]) -> Option<&TableEntry> {
        let mut i = 0;
        while i < self.tables.len() {
            if self.tables[i].tag == *tag {
                return self.tables.get(i);
            }
            i += 1;
        }
        None
    }
}

/// Type1 头解析结果。`payload_len` 含 eexec 区尾部 512 个零字节标记，
/// 与真实打印机的计数口径一致（零字节属于加密段，不属于头）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Type1Info {
    pub name: String,
    pub font_type: u16,
    pub payload_len: usize,
    pub eexec_at: usize,
}

/// 一个有名字的字体对象：解析结果 + 原始字节 + 许可位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    pub name: String,
    pub format: FontFormat,
    pub data: Vec<u8>,
    pub glyph_count: u16,
    pub units_per_em: u16,
    pub fs_type: u16,
}

impl FontFace {
    /// 按首字节判别格式并解析；空名/空体显式拒绝。
    pub fn parse(name: &str, data: &[u8]) -> Result<FontFace, FontError> {
        if name.is_empty() || data.is_empty() {
            return Err(FontError::Empty);
        }
        if data.starts_with(b"%!") {
            let info = parse_type1(data)?;
            let glyphs = info.payload_len / 8;
            if glyphs == 0 {
                return Err(FontError::EexecLen);
            }
            Ok(FontFace {
                name: name.to_string(),
                format: FontFormat::Type1,
                data: data.to_vec(),
                glyph_count: cap_u16(glyphs),
                units_per_em: 1000,
                fs_type: 0,
            })
        } else {
            let info = parse_sfnt(data)?;
            let mut glyphs = 0u16;
            let mut upem = 0u16;
            let mut fs = 0u16;
            let mut i = 0usize;
            while i < info.tables.len() {
                let t = info.tables[i];
                let at = t.offset as usize;
                if t.tag == *b"maxp" {
                    if let Some(v) = rd_u16_opt(data, at + 4) {
                        glyphs = v;
                    }
                } else if t.tag == *b"head" {
                    if let Some(v) = rd_u16_opt(data, at + 18) {
                        upem = v;
                    }
                } else if t.tag == *b"OS/2" {
                    if let Some(v) = rd_u16_opt(data, at + 8) {
                        fs = v;
                    }
                }
                i += 1;
            }
            Ok(FontFace {
                name: name.to_string(),
                format: FontFormat::TrueType,
                data: data.to_vec(),
                glyph_count: glyphs,
                units_per_em: upem,
                fs_type: fs,
            })
        }
    }

    /// 原始字节的 32 位和校验（模 2^32），打印链路与载体帧共用同一口径。
    pub fn sum(&self) -> u32 {
        sum32(&self.data)
    }
}

fn cap_u16(v: usize) -> u16 {
    if v > u16::MAX as usize {
        u16::MAX
    } else {
        v as u16
    }
}

fn zeros(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0u8);
        i += 1;
    }
    v
}

fn rd_u16(d: &[u8], at: usize, err: FontError) -> Result<u16, FontError> {
    let end = match at.checked_add(2) {
        Some(e) => e,
        None => return Err(err),
    };
    let b = match d.get(at..end) {
        Some(b) => b,
        None => return Err(err),
    };
    Ok(((b[0] as u16) << 8) | b[1] as u16)
}

fn rd_u32(d: &[u8], at: usize, err: FontError) -> Result<u32, FontError> {
    let end = match at.checked_add(4) {
        Some(e) => e,
        None => return Err(err),
    };
    let b = match d.get(at..end) {
        Some(b) => b,
        None => return Err(err),
    };
    Ok(((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | b[3] as u32)
}

fn rd_u16_opt(d: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    let b = d.get(at..end)?;
    Some(((b[0] as u16) << 8) | b[1] as u16)
}

fn be_put16(v: &mut Vec<u8>, at: usize, val: u16) {
    match at.checked_add(2) {
        Some(end) if end <= v.len() => {
            v[at] = (val >> 8) as u8;
            v[at + 1] = val as u8;
        }
        _ => {}
    }
}

fn be_put32(v: &mut Vec<u8>, at: usize, val: u32) {
    match at.checked_add(4) {
        Some(end) if end <= v.len() => {
            v[at] = (val >> 24) as u8;
            v[at + 1] = (val >> 16) as u8;
            v[at + 2] = (val >> 8) as u8;
            v[at + 3] = val as u8;
        }
        _ => {}
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let limit = hay.len() - needle.len();
    let mut i = 0usize;
    while i <= limit {
        let mut j = 0usize;
        let mut ok = true;
        while j < needle.len() {
            if hay[i + j] != needle[j] {
                ok = false;
                break;
            }
            j += 1;
        }
        if ok {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 解析 sfnt 表目录。numTables 上限 64 是打印链路实测的合理上界（含
/// 厂商私有表）；超过即视为损坏，不做「尽量解析」。
pub fn parse_sfnt(data: &[u8]) -> Result<SfntInfo, FontError> {
    if data.is_empty() {
        return Err(FontError::Empty);
    }
    if data.len() < 12 {
        return Err(FontError::SfntShort);
    }
    let version = rd_u32(data, 0, FontError::SfntShort)?;
    let magic_ok = version == 0x0001_0000
        || version == 0x7472_7565
        || version == 0x4F54_544F
        || version == 0x7479_7031;
    if !magic_ok {
        return Err(FontError::SfntMagic);
    }
    let n = rd_u16(data, 4, FontError::SfntShort)?;
    if n == 0 || n > 64 {
        return Err(FontError::TableCount);
    }
    let dir_end = 12usize + (n as usize) * 16;
    if dir_end > data.len() {
        return Err(FontError::TableCount);
    }
    let mut tables = Vec::with_capacity(n as usize);
    let mut i = 0usize;
    while i < n as usize {
        let e = 12 + i * 16;
        let tag = [data[e], data[e + 1], data[e + 2], data[e + 3]];
        let offset = rd_u32(data, e + 8, FontError::SfntShort)?;
        let len = rd_u32(data, e + 12, FontError::SfntShort)?;
        let off = offset as usize;
        let l = len as usize;
        let end = match off.checked_add(l) {
            Some(v) => v,
            None => return Err(FontError::TableOob),
        };
        if off < 12 || end > data.len() {
            return Err(FontError::TableOob);
        }
        tables.push(TableEntry { tag, offset, len });
        i += 1;
    }
    Ok(SfntInfo {
        version,
        num_tables: n,
        tables,
        bytes: data.len(),
    })
}

/// 解析 Type1 头：先验 `%!` 行、再取 `/FontName` 与 `/FontType`，
/// 最后对 eexec 区做「只允许十六进制与空白」的严格检查。
pub fn parse_type1(data: &[u8]) -> Result<Type1Info, FontError> {
    if data.is_empty() {
        return Err(FontError::Empty);
    }
    if !(data.starts_with(b"%!PS-AdobeFont") || data.starts_with(b"%!FontType1")) {
        return Err(FontError::T1Header);
    }
    let name_at = match find(data, b"/FontName") {
        Some(v) => v,
        None => return Err(FontError::T1Header),
    };
    let mut p = name_at + 9;
    while p < data.len() && (data[p] == b' ' || data[p] == b'\t') {
        p += 1;
    }
    let start = if p < data.len() && data[p] == b'/' { p + 1 } else { p };
    let mut end = start;
    while end < data.len() {
        let c = data[end];
        if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' || c == b'/' {
            break;
        }
        end += 1;
    }
    if end <= start {
        return Err(FontError::T1Header);
    }
    let mut name = String::new();
    let mut k = start;
    while k < end {
        name.push(data[k] as char);
        k += 1;
    }
    let ft_at = match find(data, b"/FontType") {
        Some(v) => v,
        None => return Err(FontError::T1Type),
    };
    let mut q = ft_at + 9;
    while q < data.len() && (data[q] == b' ' || data[q] == b'\t') {
        q += 1;
    }
    let mut val: u32 = 0;
    let mut digits = 0usize;
    while q < data.len() && data[q].is_ascii_digit() {
        val = val * 10 + (data[q] - b'0') as u32;
        q += 1;
        digits += 1;
    }
    if digits == 0 || val != 1 {
        return Err(FontError::T1Type);
    }
    let ex = match find(data, b"eexec") {
        Some(v) => v,
        None => return Err(FontError::EexecLen),
    };
    let clear = match find(data, b"cleartomark") {
        Some(v) => v,
        None => return Err(FontError::EexecLen),
    };
    if clear <= ex + 5 {
        return Err(FontError::EexecLen);
    }
    let mut hex = 0usize;
    let mut i = ex + 5;
    while i < clear {
        let b = data[i];
        if b.is_ascii_hexdigit() {
            hex += 1;
        } else if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
            // 排版空白不计入载荷。
        } else {
            return Err(FontError::EexecLen);
        }
        i += 1;
    }
    if hex < 16 || hex % 2 != 0 {
        return Err(FontError::EexecLen);
    }
    Ok(Type1Info {
        name,
        font_type: val as u16,
        payload_len: hex / 2,
        eexec_at: ex,
    })
}

/// 嵌入方式：fsType 先把「能不能嵌」判死，再决定是整字体还是子集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedMode {
    Full,
    SubsetOnly,
    BitmapOnly,
}

/// 调用方策略 + fsType 位的合成器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmbedPolicy {
    pub subset: bool,
    pub keep_percent: u16,
    pub allow_bitmap: bool,
}

impl EmbedPolicy {
    pub const FULL: EmbedPolicy = EmbedPolicy {
        subset: false,
        keep_percent: 100,
        allow_bitmap: false,
    };

    pub const fn ratio(keep: u16) -> EmbedPolicy {
        EmbedPolicy {
            subset: true,
            keep_percent: keep,
            allow_bitmap: false,
        }
    }

    pub const fn bitmap(keep: u16) -> EmbedPolicy {
        EmbedPolicy {
            subset: false,
            keep_percent: keep,
            allow_bitmap: true,
        }
    }

    /// fsType 语义（OS/2 规范）：
    /// * 0x0002 restricted → 自动嵌入一律拒绝；
    /// * 0x0004 preview&print → 只允许子集嵌入；
    /// * 0x0100 no-subsetting 与调用方子集请求冲突 → 显式错误；
    /// * 0x0200 bitmap-only → 仅在允许位图轮廓时放行。
    pub fn mode_for(&self, fs_type: u16) -> Result<EmbedMode, FontError> {
        if fs_type & 0x0002 != 0 {
            return Err(FontError::FsType);
        }
        if self.subset && (fs_type & 0x0100 != 0) {
            return Err(FontError::Subset);
        }
        if fs_type & 0x0200 != 0 {
            if self.allow_bitmap {
                return Ok(EmbedMode::BitmapOnly);
            }
            return Err(FontError::FsType);
        }
        if fs_type & 0x0004 != 0 {
            return Ok(EmbedMode::SubsetOnly);
        }
        if self.subset {
            return Ok(EmbedMode::SubsetOnly);
        }
        Ok(EmbedMode::Full)
    }
}

/// 嵌入产物：载体字节 + 分块数 + 校验和 + 模式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carried {
    pub bytes: Vec<u8>,
    pub chunks: usize,
    pub checksum: u32,
    pub mode: EmbedMode,
    pub payload_len: usize,
    pub source_len: usize,
}

/// 载体生产者。TrueType 走「长度 + 原始字节 + 校验和」的二进制帧；
/// Type1 走「长度 + ASCII-hex + 校验和」，因为 PS 载体本身是 7 位安全通道。
pub fn carry_face(face: &FontFace, policy: EmbedPolicy) -> Result<Carried, FontError> {
    let mode = policy.mode_for(face.fs_type)?;
    let payload: Vec<u8> = match mode {
        EmbedMode::Full => face.data.clone(),
        EmbedMode::SubsetOnly => subset(face, policy.keep_percent)?,
        // 位图档只保留位图可用轮廓：用 1/8 体量模拟按位图裁剪后的子集。
        EmbedMode::BitmapOnly => subset(face, 12)?,
    };
    if payload.is_empty() {
        return Err(FontError::Empty);
    }
    let checksum = sum32(&payload);
    let bytes = match face.format {
        FontFormat::TrueType => frame_binary(&payload, checksum),
        FontFormat::Type1 => frame_hex(&payload, checksum),
    };
    Ok(Carried {
        bytes,
        chunks: chunk_count(payload.len()),
        checksum,
        mode,
        payload_len: payload.len(),
        source_len: face.data.len(),
    })
}

/// 载体消费者：解帧 + 复算校验和，任一不符都显式失败（不做容错重传）。
pub fn extract(format: FontFormat, frame: &[u8]) -> Result<Vec<u8>, FontError> {
    match format {
        FontFormat::TrueType => {
            if frame.len() < 8 {
                return Err(FontError::Carrier);
            }
            let n = rd_u32(frame, 0, FontError::Carrier)? as usize;
            let end = match 4usize.checked_add(n) {
                Some(v) => v,
                None => return Err(FontError::Carrier),
            };
            let cs_end = match end.checked_add(4) {
                Some(v) => v,
                None => return Err(FontError::Carrier),
            };
            if cs_end != frame.len() {
                return Err(FontError::Carrier);
            }
            let body = match frame.get(4..end) {
                Some(b) => b,
                None => return Err(FontError::Carrier),
            };
            let cs = rd_u32(frame, end, FontError::Carrier)?;
            if sum32(body) != cs {
                return Err(FontError::Checksum);
            }
            Ok(body.to_vec())
        }
        FontFormat::Type1 => {
            if frame.len() < 6 {
                return Err(FontError::Carrier);
            }
            let n = rd_u16(frame, 0, FontError::Carrier)? as usize;
            let need = 2usize + n * 2 + 4;
            if frame.len() != need {
                return Err(FontError::Carrier);
            }
            let mut out = Vec::with_capacity(n);
            let mut i = 0usize;
            while i < n {
                let hi = hex_val(frame[2 + i * 2])?;
                let lo = hex_val(frame[2 + i * 2 + 1])?;
                out.push((hi << 4) | lo);
                i += 1;
            }
            let cs = rd_u32(frame, 2 + n * 2, FontError::Carrier)?;
            if sum32(&out) != cs {
                return Err(FontError::Checksum);
            }
            Ok(out)
        }
    }
}

fn hex_val(b: u8) -> Result<u8, FontError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(FontError::Carrier),
    }
}

fn hex_digit(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'A' + (v - 10)
    }
}

fn frame_binary(payload: &[u8], checksum: u32) -> Vec<u8> {
    let mut v = zeros(4);
    be_put32(&mut v, 0, payload.len() as u32);
    v.extend_from_slice(payload);
    let at = v.len();
    v.resize(at + 4, 0u8);
    be_put32(&mut v, at, checksum);
    v
}

fn frame_hex(payload: &[u8], checksum: u32) -> Vec<u8> {
    let mut v = zeros(2);
    be_put16(&mut v, 0, cap_u16(payload.len()));
    let mut i = 0usize;
    while i < payload.len() {
        let b = payload[i];
        v.push(hex_digit(b >> 4));
        v.push(hex_digit(b & 0x0F));
        i += 1;
    }
    let at = v.len();
    v.resize(at + 4, 0u8);
    be_put32(&mut v, at, checksum);
    v
}

fn chunk_count(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (len + 1023) / 1024
    }
}

/// 32 位模加校验和：实现简单、跨格式同口径，任何单字节损坏都会改变结果
/// （对本文档规模，碰撞概率可忽略；真要抗碰撞由上层换 FNV/CRC）。
pub fn sum32(data: &[u8]) -> u32 {
    let mut s = 0u32;
    let mut i = 0usize;
    while i < data.len() {
        s = s.wrapping_add(data[i] as u32);
        i += 1;
    }
    s
}

/// 真子集化：TrueType 重排表体并压缩 `glyf`，同步改 `maxp.numGlyphs`；
/// Type1 截短 eexec 载荷并补回 512 零字节终止符。返回值仍可被解析器接受，
/// 这样「子集后再嵌入」不会把损坏数据写进文档。
pub fn subset(face: &FontFace, keep_percent: u16) -> Result<Vec<u8>, FontError> {
    let keep = if keep_percent == 0 {
        1
    } else if keep_percent > 100 {
        100
    } else {
        keep_percent
    } as usize;
    match face.format {
        FontFormat::TrueType => subset_sfnt(&face.data, keep),
        FontFormat::Type1 => subset_type1(&face.data, keep),
    }
}

fn subset_sfnt(data: &[u8], keep: usize) -> Result<Vec<u8>, FontError> {
    let info = parse_sfnt(data)?;
    let n = info.tables.len();
    let dir_len = 12 + n * 16;
    let mut sizes: Vec<usize> = Vec::with_capacity(n);
    let mut total = dir_len;
    let mut i = 0usize;
    while i < n {
        let t = info.tables[i];
        let orig = t.len as usize;
        // head/maxp 是解释器入口，必须保全；glyf 等轮廓表按比例裁。
        let scaled = if t.tag == *b"head" || t.tag == *b"maxp" {
            orig
        } else {
            (orig * keep / 100).max(4).min(orig)
        };
        sizes.push(scaled);
        total += scaled;
        i += 1;
    }
    let mut out = zeros(total);
    let mut h = 0usize;
    while h < 12 {
        out[h] = data[h];
        h += 1;
    }
    let mut off = dir_len;
    let mut k = 0usize;
    while k < n {
        let t = info.tables[k];
        let e = 12 + k * 16;
        let mut b = 0usize;
        while b < 4 {
            out[e + b] = t.tag[b];
            b += 1;
        }
        be_put32(&mut out, e + 4, 0);
        be_put32(&mut out, e + 8, off as u32);
        be_put32(&mut out, e + 12, sizes[k] as u32);
        let src = t.offset as usize;
        let copy = sizes[k];
        let mut c = 0usize;
        while c < copy {
            out[off + c] = data[src + c];
            c += 1;
        }
        if t.tag == *b"maxp" && copy >= 6 {
            let g = match rd_u16_opt(data, src + 4) {
                Some(v) => v,
                None => 0,
            };
            let scaled_g = ((g as usize * keep / 100).max(1)) as u16;
            be_put16(&mut out, off + 4, scaled_g);
        }
        off += copy;
        k += 1;
    }
    Ok(out)
}

fn subset_type1(data: &[u8], keep: usize) -> Result<Vec<u8>, FontError> {
    let info = parse_type1(data)?;
    if info.payload_len == 0 {
        return Err(FontError::EexecLen);
    }
    let ex = info.eexec_at;
    let clear = match find(data, b"cleartomark") {
        Some(v) => v,
        None => return Err(FontError::EexecLen),
    };
    let head_end = ex + 5;
    let mut hex: Vec<u8> = Vec::new();
    let mut i = head_end;
    while i < clear {
        let b = data[i];
        if b.is_ascii_hexdigit() {
            hex.push(b);
        }
        i += 1;
    }
    if hex.len() % 2 != 0 {
        return Err(FontError::EexecLen);
    }
    let keep_bytes = ((hex.len() / 2) * keep / 100).max(1);
    let mut out = Vec::with_capacity(head_end + keep_bytes * 2 + 1060);
    out.extend_from_slice(&data[..head_end]);
    out.extend_from_slice(&hex[..keep_bytes * 2]);
    out.push(b'\n');
    let mut z = 0usize;
    while z < 1024 {
        out.push(b'0');
        z += 1;
    }
    out.extend_from_slice(b"\ncleartomark\n");
    Ok(out)
}

/// 单条嵌入账目。字段选型服务于「事后可复算」：源长度、载体长度、校验和
/// 三者必须同时在场，缺一个就无法证明往返一致。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedRecord {
    pub doc_id: u16,
    pub format: FontFormat,
    pub mode: EmbedMode,
    pub source_len: usize,
    pub carried_len: usize,
    pub chunks: usize,
    pub checksum: u32,
    pub glyphs: u16,
}

/// append-only 嵌入账。封账后任何写入都显式拒绝——审计期内不允许补账。
#[derive(Clone, Debug)]
pub struct EmbedLedger {
    recs: Vec<EmbedRecord>,
    cap: usize,
    sealed: bool,
}

impl EmbedLedger {
    pub fn new(cap: usize) -> EmbedLedger {
        EmbedLedger {
            recs: Vec::new(),
            cap,
            sealed: false,
        }
    }

    pub fn push(&mut self, r: EmbedRecord) -> Result<(), FontError> {
        if self.sealed {
            return Err(FontError::Seal);
        }
        if self.recs.len() >= self.cap {
            return Err(FontError::Capacity);
        }
        self.recs.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn record(&self, i: usize) -> Option<&EmbedRecord> {
        self.recs.get(i)
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// FNV-1a 折叠：字段按固定顺序喂入，保证同账同指纹、异账异指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h = 2166136261u32;
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = &self.recs[i];
            h = fnv_u32(h, r.doc_id as u32);
            h = fnv_u32(h, r.format.tag() as u32);
            h = fnv_u32(h, mode_code(r.mode));
            h = fnv_u32(h, r.source_len as u32);
            h = fnv_u32(h, r.carried_len as u32);
            h = fnv_u32(h, r.chunks as u32);
            h = fnv_u32(h, r.checksum);
            h = fnv_u32(h, r.glyphs as u32);
            i += 1;
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fingerprint()
    }
}

fn mode_code(m: EmbedMode) -> u32 {
    match m {
        EmbedMode::Full => 0,
        EmbedMode::SubsetOnly => 1,
        EmbedMode::BitmapOnly => 2,
    }
}

fn fnv_u32(mut h: u32, v: u32) -> u32 {
    let mut i = 0usize;
    while i < 4 {
        h ^= ((v >> (i * 8)) & 0xFF) as u32;
        h = h.wrapping_mul(16777619);
        i += 1;
    }
    h
}

/// 单文档嵌入状态机。四态单向推进；`reset` 只允许在 Committed 后回到 Idle，
/// 防止同一文档在半个会话上重复提交。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedState {
    Idle,
    Validated,
    Carried,
    Committed,
}

#[derive(Clone, Debug)]
pub struct EmbedSession {
    doc_id: u16,
    state: EmbedState,
    format: Option<FontFormat>,
    glyphs: u16,
}

impl EmbedSession {
    pub fn open(doc_id: u16) -> Result<EmbedSession, FontError> {
        if doc_id == 0 {
            return Err(FontError::Doc);
        }
        Ok(EmbedSession {
            doc_id,
            state: EmbedState::Idle,
            format: None,
            glyphs: 0,
        })
    }

    pub fn state(&self) -> EmbedState {
        self.state
    }

    /// 重解析证明「格式标签与字节一致」；不一致说明上游把错格式的 blob
    /// 挂了错名，必须在落账前拦住。
    pub fn validate(&mut self, face: &FontFace) -> Result<(), FontError> {
        if self.state != EmbedState::Idle {
            return Err(FontError::State);
        }
        let reparsed = FontFace::parse(&face.name, &face.data)?;
        if reparsed.format != face.format || reparsed.glyph_count != face.glyph_count {
            return Err(FontError::Carrier);
        }
        self.format = Some(reparsed.format);
        self.glyphs = reparsed.glyph_count;
        self.state = EmbedState::Validated;
        Ok(())
    }

    pub fn carry(&mut self, face: &FontFace, policy: EmbedPolicy) -> Result<Carried, FontError> {
        if self.state != EmbedState::Validated {
            return Err(FontError::State);
        }
        let c = carry_face(face, policy)?;
        self.state = EmbedState::Carried;
        Ok(c)
    }

    pub fn commit(&mut self, carried: &Carried, ledger: &mut EmbedLedger) -> Result<u32, FontError> {
        if self.state != EmbedState::Carried {
            return Err(FontError::State);
        }
        let format = match self.format {
            Some(f) => f,
            None => return Err(FontError::State),
        };
        let rec = EmbedRecord {
            doc_id: self.doc_id,
            format,
            mode: carried.mode,
            source_len: carried.source_len,
            carried_len: carried.bytes.len(),
            chunks: carried.chunks,
            checksum: carried.checksum,
            glyphs: self.glyphs,
        };
        ledger.push(rec)?;
        self.state = EmbedState::Committed;
        Ok(carried.checksum)
    }

    pub fn reset(&mut self) {
        if self.state == EmbedState::Committed {
            self.state = EmbedState::Idle;
            self.format = None;
            self.glyphs = 0;
        }
    }
}

/// 一站式嵌入：开会话 → 校验 → 制载体 → 落账。返回载体校验和。
pub fn embed_document(
    doc_id: u16,
    face: &FontFace,
    policy: EmbedPolicy,
    ledger: &mut EmbedLedger,
) -> Result<u32, FontError> {
    let mut sess = EmbedSession::open(doc_id)?;
    sess.validate(face)?;
    let carried = sess.carry(face, policy)?;
    let cs = sess.commit(&carried, ledger)?;
    sess.reset();
    Ok(cs)
}

// ---------------------------------------------------------------------------
// 字体合成器（只用于自检与注入核；真实字体由上游解析器喂入）
// ---------------------------------------------------------------------------

const TT_TAGS: [&[u8; 4]; 4] = [b"OS/2", b"glyf", b"head", b"maxp"];
const TT_SIZES_SMALL: [usize; 4] = [78, 0, 54, 6];

fn build_true_type(glyphs: u16, upem: u16, fs_type: u16) -> Vec<u8> {
    let glyf_len = glyphs as usize * 8;
    let dir_len = 12 + 4 * 16;
    let mut total = dir_len;
    let mut i = 0usize;
    while i < 4 {
        total += if i == 1 { glyf_len } else { TT_SIZES_SMALL[i] };
        i += 1;
    }
    let mut v = zeros(total);
    be_put32(&mut v, 0, 0x0001_0000);
    be_put16(&mut v, 4, 4);
    be_put16(&mut v, 6, 64);
    be_put16(&mut v, 8, 0);
    be_put16(&mut v, 10, 64);
    let mut off = dir_len;
    let mut k = 0usize;
    while k < 4 {
        let tag = TT_TAGS[k];
        let size = if k == 1 { glyf_len } else { TT_SIZES_SMALL[k] };
        let e = 12 + k * 16;
        let mut b = 0usize;
        while b < 4 {
            v[e + b] = tag[b];
            b += 1;
        }
        be_put32(&mut v, e + 8, off as u32);
        be_put32(&mut v, e + 12, size as u32);
        if k == 0 {
            be_put16(&mut v, off + 8, fs_type);
        } else if k == 2 {
            be_put16(&mut v, off + 18, upem);
        } else if k == 3 {
            be_put16(&mut v, off + 4, glyphs);
        }
        off += size;
        k += 1;
    }
    v
}

fn build_type1(name: &str, glyphs: u16, font_type: u16) -> Vec<u8> {
    let payload_len = glyphs as usize * 8;
    let mut s = String::new();
    s.push_str("%!PS-AdobeFont-1.0: ");
    s.push_str(name);
    s.push_str(" 1.0\n");
    s.push_str("/FontName /");
    s.push_str(name);
    s.push_str(" def\n/FontType ");
    s.push_str(&format!("{}", font_type));
    s.push_str(" def\n/FontMatrix [0.001 0 0 0.001 0 0] def\ncurrentfile eexec\n");
    let mut i = 0usize;
    while i < payload_len {
        let b = ((i as u32 * 37 + 11) & 0xFF) as u8;
        s.push(hex_digit(b >> 4) as char);
        s.push(hex_digit(b & 0x0F) as char);
        if (i + 1) % 32 == 0 {
            s.push('\n');
        }
        i += 1;
    }
    s.push('\n');
    let mut z = 0usize;
    while z < 1024 {
        s.push('0');
        z += 1;
    }
    s.push_str("\ncleartomark\n");
    s.into_bytes()
}

// ---------------------------------------------------------------------------
// 注入核：12 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    if FontFace::parse("x", &[]).err() == Some(FontError::Empty) {
        caught += 1;
    }
    if FontFace::parse("x", &zeros(8)).err() == Some(FontError::SfntShort) {
        caught += 1;
    }
    if FontFace::parse("x", &zeros(64)).err() == Some(FontError::SfntMagic) {
        caught += 1;
    }
    let mut count = build_true_type(16, 1000, 0);
    be_put16(&mut count, 4, 0);
    if FontFace::parse("x", &count).err() == Some(FontError::TableCount) {
        caught += 1;
    }
    let mut oob = build_true_type(16, 1000, 0);
    be_put32(&mut oob, 12 + 8, 0x00FF_FFFF);
    if FontFace::parse("x", &oob).err() == Some(FontError::TableOob) {
        caught += 1;
    }
    if FontFace::parse("x", b"%!BogusFont\n/FontName /X def\n").err() == Some(FontError::T1Header) {
        caught += 1;
    }
    let bad_type = build_type1("BadType", 16, 3);
    if FontFace::parse("x", &bad_type).err() == Some(FontError::T1Type) {
        caught += 1;
    }
    let mut bad_hex = build_type1("BadHex", 16, 1);
    let ex = match find(&bad_hex, b"eexec") {
        Some(v) => v,
        None => 0,
    };
    if ex + 12 < bad_hex.len() {
        bad_hex[ex + 12] = b'Z';
    }
    if FontFace::parse("x", &bad_hex).err() == Some(FontError::EexecLen) {
        caught += 1;
    }
    let restricted = build_true_type(16, 1000, 0x0002);
    let hit = match FontFace::parse("x", &restricted) {
        Ok(f) => carry_face(&f, EmbedPolicy::FULL).err() == Some(FontError::FsType),
        Err(_) => false,
    };
    if hit {
        caught += 1;
    }
    let no_subset = build_true_type(16, 1000, 0x0100);
    let hit = match FontFace::parse("x", &no_subset) {
        Ok(f) => carry_face(&f, EmbedPolicy::ratio(50)).err() == Some(FontError::Subset),
        Err(_) => false,
    };
    if hit {
        caught += 1;
    }
    if extract(FontFormat::TrueType, &[0u8; 4]).err() == Some(FontError::Carrier) {
        caught += 1;
    }
    let face = match FontFace::parse("Good", &build_true_type(16, 1000, 0)) {
        Ok(f) => f,
        Err(_) => {
            return (caught, total);
        }
    };
    match carry_face(&face, EmbedPolicy::FULL) {
        Ok(c) => {
            let mut frame = c.bytes;
            if frame.len() > 6 {
                frame[5] ^= 0x5A;
            }
            if extract(FontFormat::TrueType, &frame).err() == Some(FontError::Checksum) {
                caught += 1;
            }
        }
        Err(_) => {}
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

fn make_tt(name: &str, glyphs: u16, fs: u16) -> Option<FontFace> {
    FontFace::parse(name, &build_true_type(glyphs, 1000, fs)).ok()
}

fn make_t1(name: &str, glyphs: u16) -> Option<FontFace> {
    FontFace::parse(name, &build_type1(name, glyphs, 1)).ok()
}

fn roundtrip_doc(doc: u16, face: &FontFace, ledger: &mut EmbedLedger) -> bool {
    let mut sess = match EmbedSession::open(doc) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if sess.validate(face).is_err() {
        return false;
    }
    let carried = match sess.carry(face, EmbedPolicy::FULL) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let back = match extract(face.format, &carried.bytes) {
        Ok(b) => b,
        Err(_) => return false,
    };
    if back != face.data {
        return false;
    }
    sess.commit(&carried, ledger).is_ok()
}

pub fn run_unx50082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50082");

    // J1-1 双格式 ×10 文档：嵌入 → 解帧 → 逐字节比回原文
    let mut ledger = EmbedLedger::new(16);
    let mut ok = true;
    let mut tt = 0usize;
    let mut t1 = 0usize;
    let mut d = 1u16;
    while d <= 10 {
        if d <= 6 {
            let face = match make_tt("TT-Doc", 40 + d, 0) {
                Some(f) => f,
                None => {
                    ok = false;
                    break;
                }
            };
            if !roundtrip_doc(d, &face, &mut ledger) {
                ok = false;
            } else {
                tt += 1;
            }
        } else {
            let face = match make_t1("T1-Doc", 40 + d) {
                Some(f) => f,
                None => {
                    ok = false;
                    break;
                }
            };
            if !roundtrip_doc(d, &face, &mut ledger) {
                ok = false;
            } else {
                t1 += 1;
            }
        }
        d += 1;
    }
    let ok = ok && tt == 6 && t1 == 4 && ledger.len() == 10;
    let det = format!("ok={} tt={} t1={} rec={}", ok, tt, t1, ledger.len());
    s.add_dyn("50082-J1-双格式十文档往返", ok, &det);

    // J1-2 账本十条的格式分布与文档号单调
    let mut distributed = ledger.len() == 10;
    let mut prev_doc = 0u16;
    let mut i = 0usize;
    while i < ledger.len() {
        if let Some(r) = ledger.record(i) {
            if r.doc_id <= prev_doc {
                distributed = false;
            }
            prev_doc = r.doc_id;
            if r.source_len == 0 || r.carried_len == 0 || r.chunks == 0 || r.glyphs == 0 {
                distributed = false;
            }
        } else {
            distributed = false;
        }
        i += 1;
    }
    s.add(
        "50082-J1-账本十条有序",
        distributed,
        if distributed { "" } else { "账本顺序或内容异常" },
    );

    // J1-3 封账指纹：同账复算稳定、封账后写入被拒
    let fp1 = ledger.fingerprint();
    let fp2 = ledger.fingerprint();
    let seal_fp = ledger.seal();
    let push_after = ledger.push(EmbedRecord {
        doc_id: 99,
        format: FontFormat::TrueType,
        mode: EmbedMode::Full,
        source_len: 1,
        carried_len: 1,
        chunks: 1,
        checksum: 1,
        glyphs: 1,
    });
    let ok = fp1 == fp2 && seal_fp == fp1 && push_after.err() == Some(FontError::Seal);
    s.add("50082-J1-封账指纹", ok, if ok { "" } else { "指纹漂移或封账可写" });

    // J1-4 sfnt 表目录守恒
    let tt_face = match make_tt("Dir", 32, 0) {
        Some(f) => f,
        None => {
            s.add("50082-J2-sfnt表目录", false, "构造失败");
            return s;
        }
    };
    let info = match parse_sfnt(&tt_face.data) {
        Ok(v) => v,
        Err(_) => {
            s.add("50082-J2-sfnt表目录", false, "解析失败");
            return s;
        }
    };
    let mut dir_ok = info.num_tables == 4 && info.tables.len() == 4;
    let mut j = 0usize;
    while j < info.tables.len() {
        let t = info.tables[j];
        let end = t.offset as usize + t.len as usize;
        if t.offset < 12 || end > tt_face.data.len() || t.len < 4 {
            dir_ok = false;
        }
        j += 1;
    }
    s.add(
        "50082-J2-sfnt表目录守恒",
        dir_ok,
        if dir_ok { "" } else { "表偏移/长度越界" },
    );

    // J1-5 Type1 头字段
    let t1_face = match make_t1("ArialMT", 24) {
        Some(f) => f,
        None => {
            s.add("50082-J2-type1头字段", false, "构造失败");
            return s;
        }
    };
    let t1_info = match parse_type1(&t1_face.data) {
        Ok(v) => v,
        Err(_) => {
            s.add("50082-J2-type1头字段", false, "解析失败");
            return s;
        }
    };
    let ok = t1_info.font_type == 1
        && t1_info.name == "ArialMT"
        && t1_info.payload_len >= 24 * 8
        && t1_info.eexec_at > 0;
    let det = format!(
        "type={} name={} pad={}",
        t1_info.font_type, t1_info.name, t1_info.payload_len
    );
    s.add_dyn("50082-J2-type1头字段", ok, &det);

    // J1-6 子集单调：体量与字形数随 keep 下降而下降
    let big = match make_tt("Big", 100, 0) {
        Some(f) => f,
        None => {
            s.add("50082-J2-子集单调", false, "构造失败");
            return s;
        }
    };
    let half = subset(&big, 50);
    let quarter = subset(&big, 25);
    let (h, q) = match (half, quarter) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            s.add("50082-J2-子集单调", false, "子集失败");
            return s;
        }
    };
    let g50 = match FontFace::parse("Half", &h) {
        Ok(f) => f.glyph_count,
        Err(_) => 0,
    };
    let g25 = match FontFace::parse("Quarter", &q) {
        Ok(f) => f.glyph_count,
        Err(_) => 0,
    };
    let sizes_ok = h.len() < big.data.len() && q.len() < h.len();
    let ok = sizes_ok && g50 == 50 && g25 == 25;
    let det = format!("sizes={}/{}/{} glyphs={}/{}", big.data.len(), h.len(), q.len(), g50, g25);
    s.add_dyn("50082-J2-子集单调", ok, &det);

    // J1-7 许可位：restricted 拒绝、preview 只允许子集
    let restricted = match make_tt("Restricted", 16, 0x0002) {
        Some(f) => f,
        None => {
            s.add("50082-J2-许可位", false, "构造失败");
            return s;
        }
    };
    let preview = match make_tt("Preview", 16, 0x0004) {
        Some(f) => f,
        None => {
            s.add("50082-J2-许可位", false, "构造失败");
            return s;
        }
    };
    let deny = carry_face(&restricted, EmbedPolicy::FULL).err() == Some(FontError::FsType);
    let prev_mode = match carry_face(&preview, EmbedPolicy::FULL) {
        Ok(c) => c.mode == EmbedMode::SubsetOnly,
        Err(_) => false,
    };
    let ok = deny && prev_mode;
    s.add("50082-J2-许可位", ok, if ok { "" } else { "fsType 门控失效" });

    // J1-8 位图档：允许位图时走 BitmapOnly，否则拒绝
    let bitmap = match make_tt("BitmapOnly", 64, 0x0200) {
        Some(f) => f,
        None => {
            s.add("50082-J2-位图档", false, "构造失败");
            return s;
        }
    };
    let allowed = match carry_face(&bitmap, EmbedPolicy::bitmap(12)) {
        Ok(c) => c.mode == EmbedMode::BitmapOnly && c.payload_len < bitmap.data.len() / 2,
        Err(_) => false,
    };
    let refused = carry_face(&bitmap, EmbedPolicy::FULL).err() == Some(FontError::FsType);
    let ok = allowed && refused;
    s.add("50082-J2-位图档", ok, if ok { "" } else { "位图许可未生效" });

    // J1-9 no-subsetting 与调用方子集请求冲突必须显式失败
    let no_sub = match make_tt("NoSubset", 16, 0x0100) {
        Some(f) => f,
        None => {
            s.add("50082-J2-禁子集", false, "构造失败");
            return s;
        }
    };
    let ok = carry_face(&no_sub, EmbedPolicy::ratio(50)).err() == Some(FontError::Subset)
        && carry_face(&no_sub, EmbedPolicy::FULL).is_ok();
    s.add("50082-J2-禁子集", ok, if ok { "" } else { "禁子集位被忽略" });

    // J1-10 载体帧长：TT = n+8，T1 = 2n+6
    let c_tt = match carry_face(&tt_face, EmbedPolicy::FULL) {
        Ok(c) => c,
        Err(_) => {
            s.add("50082-J2-载体帧长", false, "TT 载体制备失败");
            return s;
        }
    };
    let c_t1 = match carry_face(&t1_face, EmbedPolicy::FULL) {
        Ok(c) => c,
        Err(_) => {
            s.add("50082-J2-载体帧长", false, "T1 载体制备失败");
            return s;
        }
    };
    let ok = c_tt.bytes.len() == c_tt.payload_len + 8
        && c_t1.bytes.len() == c_t1.payload_len * 2 + 6;
    let det = format!("tt={} t1={}", c_tt.bytes.len(), c_t1.bytes.len());
    s.add_dyn("50082-J2-载体帧长", ok, &det);

    // J1-11 提取损坏检测：字节翻转 → 校验和；长度不符 → 载体
    let mut corrupt = c_tt.bytes.clone();
    if corrupt.len() > 6 {
        corrupt[5] ^= 0xA5;
    }
    let cs = extract(FontFormat::TrueType, &corrupt).err() == Some(FontError::Checksum);
    let short = extract(FontFormat::TrueType, &c_tt.bytes[..4]).err() == Some(FontError::Carrier);
    let ok = cs && short;
    s.add("50082-J2-提取损坏检测", ok, if ok { "" } else { "损坏未被拦截" });

    // J1-12 状态机：顺序错误与重复提交必须显式拒绝
    let mut sess = match EmbedSession::open(7) {
        Ok(v) => v,
        Err(_) => {
            s.add("50082-J2-状态机", false, "开会话失败");
            return s;
        }
    };
    let early = sess.carry(&tt_face, EmbedPolicy::FULL).err() == Some(FontError::State);
    let valid = sess.validate(&tt_face).is_ok();
    let twice = sess.validate(&tt_face).err() == Some(FontError::State);
    let mut small = EmbedLedger::new(4);
    let c = match sess.carry(&tt_face, EmbedPolicy::FULL) {
        Ok(v) => v,
        Err(_) => {
            s.add("50082-J2-状态机", false, "制载体失败");
            return s;
        }
    };
    let committed = sess.commit(&c, &mut small).is_ok();
    let again = sess.commit(&c, &mut small).err() == Some(FontError::State);
    let ok = early && valid && twice && committed && again;
    s.add("50082-J2-状态机", ok, if ok { "" } else { "状态机漏拦" });

    // J1-13 账本容量与文档号
    let mut tiny = EmbedLedger::new(1);
    let rec = EmbedRecord {
        doc_id: 1,
        format: FontFormat::TrueType,
        mode: EmbedMode::Full,
        source_len: 10,
        carried_len: 18,
        chunks: 1,
        checksum: 7,
        glyphs: 4,
    };
    let first = tiny.push(rec.clone()).is_ok();
    let second = tiny.push(rec).err() == Some(FontError::Capacity);
    let doc0 = EmbedSession::open(0).err() == Some(FontError::Doc);
    let ok = first && second && doc0;
    s.add("50082-J2-账本容量", ok, if ok { "" } else { "容量或文档号未拦" });

    // J1-14 确定性：同输入两次载体逐位一致
    let a = carry_face(&tt_face, EmbedPolicy::FULL);
    let b = carry_face(&tt_face, EmbedPolicy::FULL);
    let ok = match (a, b) {
        (Ok(x), Ok(y)) => x.bytes == y.bytes && x.checksum == y.checksum && x.chunks == y.chunks,
        _ => false,
    };
    s.add("50082-J2-载体确定", ok, if ok { "" } else { "载体非确定" });

    // J1-15 注入核 12 类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50082-J1-注入必抓", ok, &det);

    // J1-16 码值闭集：每个错误码都落在 SEG_BASE+[1,16]
    let codes = [
        FontError::Empty,
        FontError::SfntShort,
        FontError::SfntMagic,
        FontError::TableCount,
        FontError::TableOob,
        FontError::T1Header,
        FontError::T1Type,
        FontError::EexecLen,
        FontError::FsType,
        FontError::Subset,
        FontError::Carrier,
        FontError::Checksum,
        FontError::Capacity,
        FontError::State,
        FontError::Doc,
        FontError::Seal,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < codes.len() {
        let c = codes[i].code();
        if c <= SEG_BASE || c > SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    s.add("50082-J1-码值闭集", ok, if ok { "" } else { "码值越段" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50082_checks_green() {
        let set = run_unx50082_checks();
        assert!(set.all_passed(), "UNX-F50082 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50082_dual_roundtrip() {
        let tt = make_tt("RT", 32, 0);
        let t1 = make_t1("RT1", 32);
        assert!(tt.is_some() && t1.is_some());
        if let (Some(a), Some(b)) = (tt, t1) {
            let ca = carry_face(&a, EmbedPolicy::FULL);
            let cb = carry_face(&b, EmbedPolicy::FULL);
            assert!(ca.is_ok() && cb.is_ok());
            if let (Ok(x), Ok(y)) = (ca, cb) {
                assert_eq!(extract(a.format, &x.bytes).ok(), Some(a.data.clone()));
                assert_eq!(extract(b.format, &y.bytes).ok(), Some(b.data.clone()));
            }
        }
    }

    #[test]
    fn unx50082_injection() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
    }
}
