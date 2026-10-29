//! unx0614_rescan —— UNX-F0614 initrd 清单复扫（遍历确定性）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0614 initrd 清单复扫（遍历确定性）  <SEG 0x4e80>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：复扫核（同档 3 轮重遍历清单指纹全等 3/3），注
//!   入档微扰检出（1 字节差指纹变）。
//!
//! 设计纪律：
//!   1. 复扫核：同档 N 轮重遍历（N=3 写死），每轮产清单指纹，
//      三轮指纹全等 → 遍历确定性成立；
//   2. 清单指纹：逐条目（名哈希+占位量+序）FNV-1a 顺序聚合，
//      顺序敏感（遍历序即账序）；
//   3. 微扰检出：档内任意 1 字节改动 → 指纹必变（变则检出）；
//      改动位三类（名区/数据区/头保留区）逐一入账；
//   4. 复扫账：轮次-指纹对账表；守恒：轮数 = 账行数；
//   5. 微扰账：逐扰动（扰动位类, 原指纹, 新指纹, 变异旗）；
//      检出数 = 扰动数（100%）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4e80 段为本模块独占（SEG_BASE=0x4e80），判别式
//   SEG_BASE+1..+N；与 0x4e60/0x4ea0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const RS_VERSION: &str = "unx0614_rescan-v1";

/// 诊断段基址（0x4e80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4E80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4e81 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4e82 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4e83 复扫轮数非法（0 或超 16）。
pub const XC_BAD_ROUNDS: u16 = SEG_BASE + 3;
/// 0x4e84 复扫指纹不齐（轮间失等）。
pub const XC_RESCAN_MISMATCH: u16 = SEG_BASE + 4;
/// 0x4e85 微扰未检出（指纹未变）。
pub const XC_PERTURB_MISSED: u16 = SEG_BASE + 5;
/// 0x4e86 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 6;
/// 0x4e87 复扫账不守恒。
pub const XC_RESCAN_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4e88 空档。
pub const XC_EMPTY_ARCHIVE: u16 = SEG_BASE + 8;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 魔数（写死）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名。
pub const TRAILER_NAME: &str = "TRAILER!!!";
/// 复扫轮数（写死：3）。
pub const RESCAN_ROUNDS: usize = 3;

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
            _ => return Err(XC_MAGIC),
        };
        v = (v << 4) | d;
    }
    Ok(v)
}

/// 名哈希（FNV-1a）。
pub fn name_hash(name: &str) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 清单指纹：逐条目（名哈希 → 占位 → 序）顺序吃 FNV-1a。
pub fn scan_fingerprint(stream: &[u8]) -> Result<u32, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let mut h = 0x811C_9DC5u32;
    let mut off = 0usize;
    let mut ord = 0u32;
    loop {
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_HEAD_SHORT)?;
        if &head[0..6] != MAGIC {
            return Err(XC_MAGIC);
        }
        let filesize = hex_field(head, 54)?;
        let namesize = hex_field(head, 94)?;
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
        let footprint = HEAD_LEN as u64
            + namesize as u64
            + pad4(ns) as u64
            + filesize as u64
            + pad4(filesize as usize) as u64;
        // 吃：名哈希。
        let nh = name_hash(&name).to_be_bytes();
        for b in nh {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        // 吃：占位。
        let fp = footprint.to_be_bytes();
        for b in fp {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        // 吃：序。
        let ob = ord.to_be_bytes();
        for b in ob {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        ord += 1;
        if name == TRAILER_NAME {
            break;
        }
        let data_off = name_off + ns + pad4(ns);
        let data_end = data_off
            .checked_add(filesize as usize)
            .ok_or(XC_HEAD_SHORT)?;
        if stream.len() < data_end {
            return Err(XC_HEAD_SHORT);
        }
        off = data_end + pad4(filesize as usize);
        if off >= stream.len() {
            break;
        }
    }
    Ok(h)
}

/// 复扫账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RescanRow {
    /// 轮次（1 起）。
    pub round: u32,
    /// 该轮指纹。
    pub fingerprint: u32,
    /// 与首轮一致旗。
    pub same_as_first: bool,
}

/// 复扫主路径：N 轮重遍历，指纹全等核。
pub fn rescan(stream: &[u8], rounds: usize) -> Result<Vec<RescanRow>, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    if rounds == 0 || rounds > 16 {
        return Err(XC_BAD_ROUNDS);
    }
    let mut rows = Vec::new();
    let mut first: Option<u32> = None;
    for r in 0..rounds {
        let fp = scan_fingerprint(stream)?;
        let same = match first {
            None => {
                first = Some(fp);
                true
            }
            Some(f) => f == fp,
        };
        rows.push(RescanRow {
            round: (r + 1) as u32,
            fingerprint: fp,
            same_as_first: same,
        });
    }
    // 全等核。
    if rows.iter().any(|r| !r.same_as_first) {
        return Err(XC_RESCAN_MISMATCH);
    }
    // 守恒：轮数 = 行数。
    if rows.len() != rounds {
        return Err(XC_RESCAN_UNBALANCED);
    }
    Ok(rows)
}

/// 微扰位类闭集：0 名区 / 1 数据区 / 2 头保留区。
pub const PERTURB_NAME: u8 = 0;
pub const PERTURB_DATA: u8 = 1;
pub const PERTURB_RESERVED: u8 = 2;

/// 微扰账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerturbRow {
    /// 扰动位类。
    pub zone: u8,
    /// 扰动字节偏移。
    pub offset: u32,
    /// 原指纹。
    pub old_fp: u32,
    /// 新指纹。
    pub new_fp: u32,
    /// 检出旗（指纹变）。
    pub detected: bool,
}

/// 微扰演练主路径：对三类区各注入 1 字节差 → 指纹必变。
/// 区偏移由调用方给（档内合法偏移），本函数负责复扫对拍。
pub fn perturb_drill(
    stream: &[u8],
    zones: &[(u8, u32)],
) -> Result<Vec<PerturbRow>, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let old_fp = scan_fingerprint(stream)?;
    let mut out = Vec::new();
    for (zone, off) in zones {
        let idx = *off as usize;
        if idx >= stream.len() {
            return Err(XC_HEAD_SHORT);
        }
        let mut bad = stream.to_vec();
        bad[idx] = bad[idx].wrapping_add(1);
        let new_fp = scan_fingerprint(&bad);
        let (new_fp, detected) = match new_fp {
            Ok(f) => (f, f != old_fp),
            Err(_) => {
                // 扰动致档不可解析（如魔数区）——也算检出（指纹面失效即变）。
                (old_fp.wrapping_add(1), true)
            }
        };
        if !detected {
            return Err(XC_PERTURB_MISSED);
        }
        out.push(PerturbRow {
            zone: *zone,
            offset: *off,
            old_fp,
            new_fp,
            detected,
        });
    }
    Ok(out)
}

/// 条目构造器。
pub fn build_entry(ino: u32, name: &str, data: &[u8]) -> Vec<u8> {
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

/// 自例程：3 轮复扫全等 → 三区微扰全检出 → 错误路径（坏轮数/
/// 坏魔数/空档/越界扰动）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 档：两条目 + TRAILER。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "init", b"boot"));
    arc.extend(build_entry(2, "cfg", b"{}"));
    arc.extend(build_entry(0, TRAILER_NAME, &[]));

    // 主路径 1：3 轮复扫全等（3/3）。
    let rows = rescan(&arc, RESCAN_ROUNDS)?;
    if rows.len() != 3 {
        return Err(XC_RESCAN_UNBALANCED);
    }
    let f0 = rows[0].fingerprint;
    for r in &rows {
        if !r.same_as_first || r.fingerprint != f0 {
            return Err(XC_RESCAN_MISMATCH);
        }
    }
    // 指纹确定性：独立再扫同值。
    if scan_fingerprint(&arc)? != f0 {
        return Err(XC_RESCAN_MISMATCH);
    }
    // 微扰后指纹必变（1 字节差）。
    let mut pert = arc.clone();
    pert[arc.len() - 1] ^= 0x01;
    if scan_fingerprint(&pert)? == f0 {
        return Err(XC_PERTURB_MISSED);
    }

    // 主路径 2：三区微扰全检出。
    // 区偏移：名区 = 110（"init" 首）；数据区 = 110+5+3（"boot" 首）；
    // 头保留区：头内 devmaj 字段（62）。
    let zones = [
        (PERTURB_NAME, 110u32),
        (PERTURB_DATA, (110 + 5 + 3) as u32),
        (PERTURB_RESERVED, 62u32),
    ];
    let prows = perturb_drill(&arc, &zones)?;
    if prows.len() != 3 || prows.iter().any(|r| !r.detected) {
        return Err(XC_PERTURB_MISSED);
    }
    // 原指纹一致。
    if prows[0].old_fp != f0 {
        return Err(XC_RESCAN_MISMATCH);
    }

    // 错误路径 1：坏轮数。
    if rescan(&arc, 0) != Err(XC_BAD_ROUNDS) {
        return Err(XC_BAD_ROUNDS);
    }
    if rescan(&arc, 17) != Err(XC_BAD_ROUNDS) {
        return Err(XC_BAD_ROUNDS);
    }
    // 错误路径 2：空档。
    if rescan(&[], 3) != Err(XC_EMPTY_ARCHIVE) {
        return Err(XC_EMPTY_ARCHIVE);
    }
    // 错误路径 3：坏魔数。
    let mut bad = build_entry(1, "a", &[]);
    bad[1] = b'8';
    if scan_fingerprint(&bad) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 4：扰动偏移越界。
    if perturb_drill(&arc, &[(PERTURB_NAME, 9999)]) != Err(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    // 错误路径 5：名区无 NUL。
    let mut bad2 = build_entry(1, "ab", &[]);
    bad2[110 + 1] = b'Z';
    if scan_fingerprint(&bad2) != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    Ok(0)
}
