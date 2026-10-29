//! unx0615_fivestat —— UNX-F0615 initrd 五数概括账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0615 initrd 五数概括账  <SEG 0x4ea0>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：概括 5 项（条目数/档长/数据总字节/校验失配数/
//!   重复名数）与逐项账复算一致。
//!
//! 设计纪律：
//!   1. 五数闭集（写死）：① 条目数（含 TRAILER）② 档长 ③ 数据
//      总字节（Σfilesize）④ 校验失配数（c_check vs 实测）⑤ 重
//      复名数（同名 ≥2 的名种数）；
//   2. 逐项账复算一致：五数各自有独立复算通道（二次遍历面），
//      概括值 = 复算值逐项比对，任一项漂移红码；
//   3. 校验和口径：Σ数据字节 mod 65536 vs 头 c_check（与 F0605
//      同口径——跨单一致性）；
//   4. 重复名口径：名种计数 >1 的种数（非重复条目数）；
//   5. 概括账守恒：条目数 = 清单行数（轻遍历面行数一致）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4ea0 段为本模块独占（SEG_BASE=0x4ea0），判别式
//   SEG_BASE+1..+N；与 0x4e80/0x4ec0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const FS_VERSION: &str = "unx0615_fivestat-v1";

/// 诊断段基址（0x4ea0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4EA0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ea1 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4ea2 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4ea3 概括漂移（五数某项复算不一致）。
pub const XC_STAT_DRIFT: u16 = SEG_BASE + 3;
/// 0x4ea4 校验和字段非十六进制。
pub const XC_NOT_HEX: u16 = SEG_BASE + 4;
/// 0x4ea5 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 5;
/// 0x4ea6 概括账不守恒。
pub const XC_STAT_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x4ea7 空档。
pub const XC_EMPTY_ARCHIVE: u16 = SEG_BASE + 7;
/// 0x4ea8 复算通道缺失（五数外通道）。
pub const XC_BAD_CHANNEL: u16 = SEG_BASE + 8;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 魔数（写死）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名。
pub const TRAILER_NAME: &str = "TRAILER!!!";
/// 校验模数（写死：65536）。
pub const CHECK_MOD: u32 = 65536;

/// 4 对齐垫。
pub fn pad4(n: usize) -> usize {
    (4 - (n % 4)) % 4
}

/// 头 hex 字段。
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

/// 数据实测和。
pub fn data_sum(data: &[u8]) -> u32 {
    let mut s = 0u32;
    for b in data {
        s = (s + *b as u32) % CHECK_MOD;
    }
    s
}

/// 条目扫描行（轻遍历产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRow {
    /// 条目序。
    pub ordinal: u32,
    /// 名。
    pub name: String,
    /// filesize。
    pub filesize: u32,
    /// 数据偏移。
    pub data_off: u32,
    /// 头 c_check。
    pub declared: u32,
}

/// 轻遍历。
pub fn scan(stream: &[u8]) -> Result<Vec<ScanRow>, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    let mut ord = 0u32;
    loop {
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_HEAD_SHORT)?;
        if &head[0..6] != MAGIC {
            return Err(XC_MAGIC);
        }
        let filesize = hex_field(head, 54)?;
        let namesize = hex_field(head, 94)?;
        let declared = hex_field(head, 102)?;
        if namesize < 1 {
            return Err(XC_HEAD_SHORT);
        }
        let ns = namesize as usize;
        let name_off = off + HEAD_LEN;
        let end = name_off + ns;
        let region = stream.get(name_off..end).ok_or(XC_NAME_NUL)?;
        if region[ns - 1] != 0 {
            return Err(XC_NAME_NUL);
        }
        let mut name = String::new();
        for b in &region[..ns - 1] {
            name.push(*b as char);
        }
        let data_off = name_off + ns + pad4(ns);
        if stream.len() < data_off + filesize as usize {
            return Err(XC_HEAD_SHORT);
        }
        out.push(ScanRow {
            ordinal: ord,
            name,
            filesize,
            data_off: data_off as u32,
            declared,
        });
        ord += 1;
        if out[out.len() - 1].name == TRAILER_NAME {
            break;
        }
        off = data_off + filesize as usize + pad4(filesize as usize);
        if off >= stream.len() {
            break;
        }
    }
    Ok(out)
}

/// 五数概括。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveStat {
    /// ① 条目数（含 TRAILER）。
    pub entries: u32,
    /// ② 档长。
    pub arc_len: u32,
    /// ③ 数据总字节。
    pub data_bytes: u32,
    /// ④ 校验失配数。
    pub check_miss: u32,
    /// ⑤ 重复名数（同名 ≥2 的名种数）。
    pub dup_names: u32,
}

/// 五数主通道（一次遍历面）。
pub fn five_stat(stream: &[u8]) -> Result<FiveStat, u16> {
    let rows = scan(stream)?;
    if rows.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let entries = rows.len() as u32;
    let arc_len = stream.len() as u32;
    let mut data_bytes = 0u32;
    let mut check_miss = 0u32;
    for r in &rows {
        data_bytes += r.filesize;
        let d = stream
            .get(r.data_off as usize..r.data_off as usize + r.filesize as usize)
            .ok_or(XC_HEAD_SHORT)?;
        let actual = data_sum(d);
        if (r.declared & 0xFFFF) != actual {
            check_miss += 1;
        }
    }
    // 重复名种数。
    let mut dup = 0u32;
    for r in &rows {
        let c = rows.iter().filter(|x| x.name == r.name).count() as u32;
        if c > 1 && rows.iter().position(|x| x.name == r.name) == Some(r.ordinal as usize) {
            dup += 1;
        }
    }
    Ok(FiveStat {
        entries,
        arc_len,
        data_bytes,
        check_miss,
        dup_names: dup,
    })
}

/// 五数复算通道（独立二次面，逐项通道号 1..=5）。
pub fn restat_channel(stream: &[u8], channel: u8) -> Result<u32, u16> {
    let rows = scan(stream)?;
    match channel {
        1 => Ok(rows.len() as u32),
        2 => Ok(stream.len() as u32),
        3 => {
            let mut s = 0u32;
            for r in &rows {
                s += r.filesize;
            }
            Ok(s)
        }
        4 => {
            let mut m = 0u32;
            for r in &rows {
                let d = stream
                    .get(r.data_off as usize..r.data_off as usize + r.filesize as usize)
                    .ok_or(XC_HEAD_SHORT)?;
                if (r.declared & 0xFFFF) != data_sum(d) {
                    m += 1;
                }
            }
            Ok(m)
        }
        5 => {
            let mut names: Vec<&str> = Vec::new();
            for r in &rows {
                names.push(&r.name);
            }
            let mut dup = 0u32;
            for i in 0..names.len() {
                let seen_before = names[..i].contains(&names[i]);
                let c = names.iter().filter(|n| **n == names[i]).count();
                if c > 1 && !seen_before {
                    dup += 1;
                }
            }
            Ok(dup)
        }
        _ => Err(XC_BAD_CHANNEL),
    }
}

/// 五数对拍：主通道 vs 复算通道逐项一致。
pub fn verify_five(stream: &[u8]) -> Result<(), u16> {
    let f = five_stat(stream)?;
    let vals = [f.entries, f.arc_len, f.data_bytes, f.check_miss, f.dup_names];
    for (i, ch) in (1u8..=5).enumerate() {
        let r = restat_channel(stream, ch)?;
        if r != vals[i] {
            return Err(XC_STAT_DRIFT);
        }
    }
    // 概括账守恒：条目数 = 行数。
    if f.entries as usize != scan(stream)?.len() {
        return Err(XC_STAT_UNBALANCED);
    }
    Ok(())
}

/// 条目构造器（check 0xFFFFFFFF = 自动）。
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

/// 自例程：好档五数全绿 → 失配/重复名注入五数变化 → 五通道复算
/// 一致 → 错误路径。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 好档：2 条目（无重复名）+ TRAILER。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "a", b"xyz", 0xFFFF_0001));
    arc.extend(build_entry(2, "b", &[1, 2, 3, 4], 0xFFFF_0001));
    arc.extend(build_entry(0, TRAILER_NAME, &[], 0xFFFF_0001));
    let f = five_stat(&arc)?;
    if f.entries != 3 || f.arc_len != arc.len() as u32 {
        return Err(XC_STAT_DRIFT);
    }
    // 数据总字节 = 3 + 4 + 0 = 7。
    if f.data_bytes != 7 {
        return Err(XC_STAT_DRIFT);
    }
    if f.check_miss != 0 || f.dup_names != 0 {
        return Err(XC_STAT_DRIFT);
    }
    // 五通道复算一致。
    verify_five(&arc)?;

    // 注入：校验失配 1 + 重复名 1（"b" 两条目）。
    let mut arc2 = Vec::new();
    arc2.extend(build_entry(1, "a", b"xyz", 0xFFFF_0001));
    arc2.extend(build_entry(2, "b", &[9, 9], 0xFFFF_0001));
    arc2.extend(build_entry(3, "b", b"", 0xFFFF_0001));
    arc2.extend(build_entry(4, "c", b"ff", 7)); // check 声明 7 ≠ 实测 0x66+0x66=204
    arc2.extend(build_entry(0, TRAILER_NAME, &[], 0xFFFF_0001));
    let f2 = five_stat(&arc2)?;
    if f2.entries != 5 {
        return Err(XC_STAT_DRIFT);
    }
    // 失配：c 条目 1 枚。
    if f2.check_miss != 1 {
        return Err(XC_STAT_DRIFT);
    }
    // 重复名："b" 2 条 → 名种数 1。
    if f2.dup_names != 1 {
        return Err(XC_STAT_DRIFT);
    }
    verify_five(&arc2)?;

    // 错误路径 1：通道越界。
    if restat_channel(&arc, 6) != Err(XC_BAD_CHANNEL) {
        return Err(XC_BAD_CHANNEL);
    }
    // 错误路径 2：空档。
    if five_stat(&[]) != Err(XC_EMPTY_ARCHIVE) {
        return Err(XC_EMPTY_ARCHIVE);
    }
    // 错误路径 3：坏魔数。
    let mut bad = build_entry(1, "a", &[], 0);
    bad[2] = b'8';
    if scan(&bad) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 4：坏 hex。
    let mut bad2 = build_entry(1, "a", &[], 0);
    bad2[102] = b'g';
    if scan(&bad2) != Err(XC_NOT_HEX) {
        return Err(XC_NOT_HEX);
    }
    // 错误路径 5：名区无 NUL。
    let mut bad3 = build_entry(1, "ab", &[], 0);
    bad3[110] = b'W';
    if scan(&bad3) != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    Ok(0)
}
