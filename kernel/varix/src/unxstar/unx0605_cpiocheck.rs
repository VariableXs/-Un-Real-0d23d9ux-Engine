//! unx0605_cpiocheck —— UNX-F0605 条目头校验和账（c_check 复核）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0605 条目头校验和账（c_check 复核）  <SEG 0x4d60>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：校验和复核账齐（逐条目全核——c_check 与数据实
//!   测和 mod 65536 一致），失配检出注入红。
//!
//! 设计纪律：
//!   1. newc CRC 校验和口径（写死）：c_check = Σ(数据字节) 对
//      65536 取模——逐字节无符号累加，宽 32 位累计、截断 16 位；
//   2. 逐条目全核：遍历档内每条目，实测和与头内 c_check 字段
//      （[102..110) hex）对拍；全核通过 = 账齐；
//   3. 失配检出注入红：注入改写数据任一字节 → 复核必红，红账
//      记（条目序，期望，实测）三元组，不许只报"红"；
//   4. 零数据条目（目录）：实测和 0，头 check 0 → 平；
//   5. 复核账守恒：核对数 = 条目数（含 TRAILER），平账数 + 红账
//      数 = 核对数；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，越界红码不 panic。
//!
//! 诊断段声明：0x4d60 段为本模块独占（SEG_BASE=0x4d60），判别式
//   SEG_BASE+1..+N；与 0x4d40/0x4d80 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CC_VERSION: &str = "unx0605_cpiocheck-v1";

/// 诊断段基址（0x4d60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4D60;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4d61 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4d62 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4d63 字段非十六进制。
pub const XC_NOT_HEX: u16 = SEG_BASE + 3;
/// 0x4d64 校验和失配（c_check ≠ 实测和 mod 65536）。
pub const XC_CHECK_MISMATCH: u16 = SEG_BASE + 4;
/// 0x4d65 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 5;
/// 0x4d66 数据区越界。
pub const XC_DATA_TRUNC: u16 = SEG_BASE + 6;
/// 0x4d67 复核账不守恒（核对数 ≠ 条目数）。
pub const XC_AUDIT_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4d68 namesize 非法。
pub const XC_BAD_NAMESIZE: u16 = SEG_BASE + 8;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 模数（写死：65536 = 2^16）。
pub const CHECK_MOD: u32 = 65536;
/// 魔数（写死）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名。
pub const TRAILER_NAME: &str = "TRAILER!!!";

/// 数据实测和：Σ字节 mod 65536。
pub fn data_sum(data: &[u8]) -> u32 {
    let mut s = 0u32;
    for b in data {
        s = (s + *b as u32) % CHECK_MOD;
    }
    s
}

/// 头字段 hex 解析（8 字符）。
pub fn hex_field(head: &[u8], off: usize) -> Result<u32, u16> {
    if head.len() < off + 8 {
        return Err(XC_HEAD_SHORT);
    }
    let mut v = 0u32;
    for i in 0..8 {
        let c = head[off + i];
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'f' => (c - b'a' + 10) as u32,
            b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => return Err(XC_NOT_HEX),
        };
        v = (v << 4) | d;
    }
    Ok(v)
}

/// 条目定位（轻遍历）：产出（条目序，头偏，filesize，数据偏，名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryLoc {
    /// 条目序。
    pub ordinal: u32,
    /// 头偏移。
    pub head_off: usize,
    /// filesize。
    pub filesize: u32,
    /// 数据区偏移。
    pub data_off: usize,
    /// 条目名。
    pub name: String,
    /// 头内 c_check 声明值。
    pub declared: u32,
}

/// 4 对齐垫。
pub fn pad4(n: usize) -> usize {
    (4 - (n % 4)) % 4
}

/// 轻遍历（不复核数据，只定位）。
pub fn locate(stream: &[u8]) -> Result<Vec<EntryLoc>, u16> {
    let mut out = Vec::new();
    let mut off = 0usize;
    let mut ord = 0u32;
    loop {
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_HEAD_SHORT)?;
        if &head[0..6] != MAGIC {
            return Err(XC_MAGIC);
        }
        let filesize = hex_field(head, 54)?;
        let namesize = hex_field(head, 94)? as usize;
        let declared = hex_field(head, 102)?;
        if namesize == 0 {
            return Err(XC_BAD_NAMESIZE);
        }
        let name_off = off + HEAD_LEN;
        let end = name_off + namesize;
        let region = stream.get(name_off..end).ok_or(XC_NAME_NUL)?;
        if region[namesize - 1] != 0 {
            return Err(XC_NAME_NUL);
        }
        let mut name = String::new();
        for b in &region[..namesize - 1] {
            name.push(*b as char);
        }
        let data_off = name_off + namesize + pad4(namesize);
        out.push(EntryLoc {
            ordinal: ord,
            head_off: off,
            filesize,
            data_off,
            name: name.clone(),
            declared,
        });
        ord += 1;
        if name == TRAILER_NAME {
            break;
        }
        let data_end = data_off
            .checked_add(filesize as usize)
            .ok_or(XC_DATA_TRUNC)?;
        if stream.len() < data_end {
            return Err(XC_DATA_TRUNC);
        }
        off = data_end + pad4(filesize as usize);
    }
    Ok(out)
}

/// 复核红账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckRed {
    /// 条目序。
    pub ordinal: u32,
    /// 头声明 c_check。
    pub expect: u32,
    /// 实测和。
    pub actual: u32,
}

/// 复核总账。
#[derive(Debug, Default)]
pub struct CheckBook {
    /// 平账数。
    pub ok_count: u32,
    /// 红账。
    pub reds: Vec<CheckRed>,
    /// 核对数。
    pub audited: u32,
}

impl CheckBook {
    /// 守恒：ok + reds = audited。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.ok_count + self.reds.len() as u32 != self.audited {
            return Err(XC_AUDIT_UNBALANCED);
        }
        Ok(())
    }

    /// 全绿判定。
    pub fn all_green(&self) -> bool {
        self.audited > 0 && self.reds.is_empty()
    }
}

/// 逐条目全核主路径：遍历 → 逐条对拍 c_check vs 实测和。
pub fn audit_all(stream: &[u8]) -> Result<CheckBook, u16> {
    let locs = locate(stream)?;
    if locs.is_empty() {
        return Err(XC_HEAD_SHORT);
    }
    let mut book = CheckBook::default();
    for l in &locs {
        book.audited += 1;
        let data = stream
            .get(l.data_off..l.data_off + l.filesize as usize)
            .ok_or(XC_DATA_TRUNC)?;
        let actual = data_sum(data);
        // 声明值对拍：头声明的 c_check 截 16 位与实测比。
        let expect = l.declared & 0xFFFF;
        if expect != actual {
            book.reds.push(CheckRed {
                ordinal: l.ordinal,
                expect,
                actual,
            });
        } else {
            book.ok_count += 1;
        }
    }
    book.conserved()?;
    Ok(book)
}

/// 条目构造器（带 c_check 注入）：check 参数 0xFFFFFFFF 表示自动算。
pub fn build_entry(ino: u32, name: &str, data: &[u8], check: u32) -> Vec<u8> {
    let mut h = [0u8; HEAD_LEN];
    h[0..6].copy_from_slice(MAGIC);
    let mut put = |off: usize, v: u32| {
        let be = v.to_be_bytes();
        for i in 0..8 {
            let nib = if i % 2 == 0 {
                (be[i / 2] >> 4) & 0xF
            } else {
                be[i / 2] & 0xF
            };
            h[off + i] = if nib < 10 { b'0' + nib } else { b'a' + nib - 10 };
        }
    };
    put(6, ino);
    put(54, data.len() as u32);
    put(94, (name.len() + 1) as u32);
    let c = if check == 0xFFFF_0001 { data_sum(data) } else { check };
    put(102, c);
    let mut out = Vec::from(h.as_slice());
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    for _ in 0..pad4(name.len() + 1) {
        out.push(0);
    }
    out.extend_from_slice(data);
    for _ in 0..pad4(data.len()) {
        out.push(0);
    }
    out
}

/// 自例程：好档全绿 → 注入改字必红且红账定位 → 零数据条目平 →
/// 错误路径（截断/坏 hex/坏魔数/账守恒）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径：三真实条目（含零数据目录）+ TRAILER。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "dir", &[], 0xFFFF_0001));
    arc.extend(build_entry(2, "file", &[1, 2, 3, 4, 5], 0xFFFF_0001));
    arc.extend(build_entry(3, "big", &[0xFF; 300], 0xFFFF_0001));
    arc.extend(build_entry(0, TRAILER_NAME, &[], 0xFFFF_0001));

    let book = audit_all(&arc)?;
    book.conserved()?;
    if !book.all_green() {
        return Err(XC_CHECK_MISMATCH);
    }
    // 核对数 = 条目数（4，含 TRAILER）。
    if book.audited != 4 {
        return Err(XC_AUDIT_UNBALANCED);
    }
    // 数据和独立复算：file 条目 1+2+3+4+5 = 15。
    let locs = locate(&arc)?;
    let f = &locs[1];
    let d = arc.get(f.data_off..f.data_off + 5).ok_or(XC_DATA_TRUNC)?;
    if data_sum(d) != 15 {
        return Err(XC_CHECK_MISMATCH);
    }
    // big 条目：300×0xFF mod 65536 = 76500 mod 65536 = 10964。
    let b = &locs[2];
    let db = arc.get(b.data_off..b.data_off + 300).ok_or(XC_DATA_TRUNC)?;
    if data_sum(db) != 300 * 255 % CHECK_MOD {
        return Err(XC_CHECK_MISMATCH);
    }

    // 注入红：改 file 条目数据首字节 1→9。
    let mut bad = arc.clone();
    bad[f.data_off] = 9;
    let book2 = audit_all(&bad)?;
    book2.conserved()?;
    if book2.all_green() {
        return Err(XC_CHECK_MISMATCH);
    }
    // 红账定位：恰 1 红，条目序 1，期望 15 实测 23。
    if book2.reds.len() != 1 {
        return Err(XC_AUDIT_UNBALANCED);
    }
    let r = book2.reds[0];
    if r.ordinal != 1 || r.expect != 15 || r.actual != 23 {
        return Err(XC_CHECK_MISMATCH);
    }

    // 错误路径 1：数据截断。
    let cut = &arc[..f.data_off + 2];
    if audit_all(cut).err() != Some(XC_DATA_TRUNC) {
        return Err(XC_DATA_TRUNC);
    }
    // 错误路径 2：坏 hex（check 字段塞 'z'）。
    let mut bad2 = build_entry(1, "a", b"xy", 0xFFFF_0001);
    bad2[102] = b'z';
    if audit_all(&bad2).err() != Some(XC_NOT_HEX) {
        return Err(XC_NOT_HEX);
    }
    // 错误路径 3：坏魔数。
    let mut bad3 = build_entry(1, "a", &[], 0);
    bad3[5] = b'2';
    if audit_all(&bad3).err() != Some(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 4：空流。
    if audit_all(&[]).err() != Some(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    // 错误路径 5：坏账守恒（手工构造不一致账）。
    let mut cb = CheckBook::default();
    cb.ok_count = 5;
    cb.audited = 3;
    if cb.conserved() != Err(XC_AUDIT_UNBALANCED) {
        return Err(XC_AUDIT_UNBALANCED);
    }
    Ok(0)
}
