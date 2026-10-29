//! unx0613_manifest —— UNX-F0613 条目清单封账（initrd 账 v1）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0613 条目清单封账（initrd 账 v1）  <SEG 0x4e60>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：清单封账 v1（条目清单+守恒式+红线声明三段），
//!   封后写拦截 100%，mock diff 零差。
//!
//! 设计纪律：
//!   1. 清单三段（写死结构）：① 条目清单（名+序+占位量逐行）；
//      ② 守恒式行（Σ占位 = 档长，误差 0）；③ 红线声明行（封账
//      后零写声明）——三段齐一才可封；
//   2. 封账：清单指纹 = FNV-1a 逐行吃（名+序+占位）；封后清单
//      只读（再产清单即封账内重算，账面不得变更）；
//   3. 封后写拦截 100%：封账状态机的写面接口全红码（封账级）；
//   4. mock diff 零差：消费方 mock 以同算法重建清单，与封账清
//      单逐行对拍，零差 = 一致（指纹相等 + 行数相等 + 逐行等）；
//   5. 清单账守恒：行数 = 条目数；占位和 = 档长；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4e60 段为本模块独占（SEG_BASE=0x4e60），判别式
//   SEG_BASE+1..+N；与 0x4e40/0x4e80 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const MF_VERSION: &str = "unx0613_manifest-v1";

/// 诊断段基址（0x4e60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4E60;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4e61 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4e62 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4e63 清单三段缺一。
pub const XC_MANIFEST_INCOMPLETE: u16 = SEG_BASE + 3;
/// 0x4e64 封后写拦截（红线）。
pub const XC_SEALED_WRITE: u16 = SEG_BASE + 4;
/// 0x4e65 mock diff 有差。
pub const XC_MOCK_DIFF: u16 = SEG_BASE + 5;
/// 0x4e66 清单账不守恒。
pub const XC_MANIFEST_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x4e67 未封账就出消费面。
pub const XC_NOT_SEALED: u16 = SEG_BASE + 7;
/// 0x4e68 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 8;
/// 0x4e69 守恒式不齐（Σ占位 ≠ 档长）。
pub const XC_CONSERV_GAP: u16 = SEG_BASE + 9;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 魔数（写死）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名。
pub const TRAILER_NAME: &str = "TRAILER!!!";

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

/// 条目占位量。
pub fn entry_footprint(namesize: u32, filesize: u32) -> u64 {
    HEAD_LEN as u64
        + namesize as u64
        + pad4(namesize as usize) as u64
        + filesize as u64
        + pad4(filesize as usize) as u64
}

/// 清单行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestRow {
    /// 条目序。
    pub ordinal: u32,
    /// 条目名。
    pub name: String,
    /// 占位量。
    pub footprint: u64,
}

/// 清单三段。
#[derive(Clone, Debug)]
pub struct Manifest {
    /// 段一：条目清单。
    pub rows: Vec<ManifestRow>,
    /// 段二：守恒式行（Σ占位, 档长, 误差）。
    pub conserv: (u64, u64, u64),
    /// 段三：红线声明（true = 封账后零写）。
    pub redline: bool,
    /// 封账旗。
    pub sealed: bool,
    /// 清单指纹。
    pub fingerprint: u32,
}

/// FNV-1a 清单指纹：逐行吃（序+名+占位）。
pub fn manifest_fp(rows: &[ManifestRow]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for r in rows {
        let mut eat = |b: u8, h: &mut u32| {
            *h ^= b as u32;
            *h = h.wrapping_mul(0x0100_0193);
        };
        let o = r.ordinal.to_be_bytes();
        for b in o {
            eat(b, &mut h);
        }
        for b in r.name.as_bytes() {
            eat(*b, &mut h);
        }
        eat(0, &mut h);
        let f = r.footprint.to_be_bytes();
        for b in f {
            eat(b, &mut h);
        }
    }
    h
}

/// 清单构建主路径：遍历档 → 三段齐 → 守恒核（未封账态）。
pub fn build_manifest(stream: &[u8]) -> Result<Manifest, u16> {
    if stream.is_empty() {
        return Err(XC_HEAD_SHORT);
    }
    let arc_len = stream.len() as u64;
    let mut rows = Vec::new();
    let mut off = 0usize;
    let mut ord = 0u32;
    let mut saw_trailer = false;
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
        rows.push(ManifestRow {
            ordinal: ord,
            name: name.clone(),
            footprint: entry_footprint(namesize, filesize),
        });
        ord += 1;
        if name == TRAILER_NAME {
            saw_trailer = true;
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
    if !saw_trailer {
        return Err(XC_MANIFEST_INCOMPLETE);
    }
    // 段二：守恒式。
    let sum: u64 = rows.iter().map(|r| r.footprint).sum();
    let conserv = (sum, arc_len, abs64(sum, arc_len));
    if conserv.2 != 0 {
        return Err(XC_CONSERV_GAP);
    }
    // 段三：红线声明。
    let m = Manifest {
        fingerprint: manifest_fp(&rows),
        rows,
        conserv,
        redline: true,
        sealed: false,
    };
    manifest_conserved(&m)?;
    Ok(m)
}

/// 绝对差（无浮点）。
pub fn abs64(a: u64, b: u64) -> u64 {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// 清单账守恒：行数 ≥1 且守恒式齐（误差 0）且红线声明在。
pub fn manifest_conserved(m: &Manifest) -> Result<(), u16> {
    if m.rows.is_empty() {
        return Err(XC_MANIFEST_UNBALANCED);
    }
    if m.conserv.2 != 0 {
        return Err(XC_MANIFEST_UNBALANCED);
    }
    if !m.redline {
        return Err(XC_MANIFEST_INCOMPLETE);
    }
    Ok(())
}

/// 封账：守恒核过 → sealed 落账。二次封红码。
pub fn seal_manifest(m: &mut Manifest) -> Result<u32, u16> {
    if m.sealed {
        return Err(XC_SEALED_WRITE);
    }
    manifest_conserved(m)?;
    m.sealed = true;
    Ok(m.fingerprint)
}

/// 封后写拦截面：任何清单变更意图红码（封账级）。
pub fn manifest_write(m: &Manifest, _row: ManifestRow) -> Result<(), u16> {
    if m.sealed {
        return Err(XC_SEALED_WRITE);
    }
    // 未封账态写也不许（清单构建期只读——构建即定型）。
    Err(XC_SEALED_WRITE)
}

/// mock diff 零差：消费方以同算法重建清单，对拍封账清单。
pub fn mock_diff(sealed: &Manifest, rebuilt: &Manifest) -> Result<(), u16> {
    if !sealed.sealed {
        return Err(XC_NOT_SEALED);
    }
    if sealed.rows.len() != rebuilt.rows.len() {
        return Err(XC_MOCK_DIFF);
    }
    if sealed.fingerprint != rebuilt.fingerprint {
        return Err(XC_MOCK_DIFF);
    }
    for (a, b) in sealed.rows.iter().zip(rebuilt.rows.iter()) {
        if a != b {
            return Err(XC_MOCK_DIFF);
        }
    }
    if sealed.conserv != rebuilt.conserv {
        return Err(XC_MOCK_DIFF);
    }
    Ok(())
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

/// 自例程：清单三段 → 封账 → 封后写拦截 → mock 零差 → 错误路径
/// （缺 TRAILER/守恒破/二次封/未封消费）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "init", b"boot"));
    arc.extend(build_entry(2, "cfg", b"{}"));
    arc.extend(build_entry(0, TRAILER_NAME, &[]));
    let mut m = build_manifest(&arc)?;
    // 三段齐。
    if m.rows.len() != 3 || !m.redline || m.conserv.2 != 0 {
        return Err(XC_MANIFEST_INCOMPLETE);
    }
    // 守恒：Σ占位 = 档长。
    if m.conserv.0 != arc.len() as u64 {
        return Err(XC_CONSERV_GAP);
    }
    // 逐行占位核。
    if m.rows[0].footprint != 110 + 5 + 3 + 4 + 0 {
        return Err(XC_MANIFEST_UNBALANCED);
    }
    if m.rows[2].name != TRAILER_NAME {
        return Err(XC_MANIFEST_UNBALANCED);
    }
    // 封后写拦截（未封先测写也拦）。
    let row = ManifestRow {
        ordinal: 9,
        name: String::from("rogue"),
        footprint: 1,
    };
    if manifest_write(&m, row.clone()) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // 封账。
    let fp = seal_manifest(&mut m)?;
    if !m.sealed {
        return Err(XC_NOT_SEALED);
    }
    if fp != manifest_fp(&m.rows) {
        return Err(XC_MOCK_DIFF);
    }
    // 二次封红码。
    if seal_manifest(&mut m) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // 封后写拦截 100%。
    if manifest_write(&m, row) != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // mock 零差：同算法重建。
    let rebuilt = build_manifest(&arc)?;
    mock_diff(&m, &rebuilt)?;
    // mock diff 有差：改档重建。
    let mut arc2 = arc.clone();
    arc2.extend(build_entry(3, "extra", b"x"));
    let rebuilt2 = build_manifest(&arc2)?;
    if mock_diff(&m, &rebuilt2) != Err(XC_MOCK_DIFF) {
        return Err(XC_MOCK_DIFF);
    }
    // 未封账消费面红码。
    let mut m2 = build_manifest(&arc)?;
    if mock_diff(&m2, &rebuilt) != Err(XC_NOT_SEALED) {
        return Err(XC_NOT_SEALED);
    }
    let _ = seal_manifest(&mut m2);

    // 错误路径 1：缺 TRAILER。
    let mut notrail = Vec::new();
    notrail.extend(build_entry(1, "a", b"z"));
    if build_manifest(&notrail).err() != Some(XC_MANIFEST_INCOMPLETE) {
        return Err(XC_MANIFEST_INCOMPLETE);
    }
    // 错误路径 2：坏魔数。
    let mut bad = build_entry(1, "a", &[]);
    bad[5] = b'3';
    if build_manifest(&bad).err() != Some(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 3：守恒破（尾部加杂字节——清单 SUM 不含）。
    let mut dirty = arc.clone();
    dirty.extend_from_slice(&[0xAA, 0xBB]);
    if build_manifest(&dirty).err() != Some(XC_CONSERV_GAP) {
        return Err(XC_CONSERV_GAP);
    }
    // 错误路径 4：空档。
    if build_manifest(&[]).err() != Some(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    // 红线声明缺失面。
    let mut m3 = build_manifest(&arc)?;
    m3.redline = false;
    if manifest_conserved(&m3) != Err(XC_MANIFEST_INCOMPLETE) {
        return Err(XC_MANIFEST_INCOMPLETE);
    }
    Ok(0)
}
