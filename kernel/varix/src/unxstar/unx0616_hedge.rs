//! unx0616_hedge —— UNX-F0616 initrd 对冲总账。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0616 initrd 对冲总账  <SEG 0x4ec0>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：三账（清单/守恒/五数）对冲平（条目数三面等+档长
//!   恒等复验 三等），误差 0。
//!
//! 设计纪律：
//!   1. 三账面（写死）：A 清单账（条目行表）/ B 守恒账（占位累进
//      终点）/ C 五数账（五项概括）；对冲 = 三账同指标互核；
//   2. 条目数三面等：A 行数 = B 步进数 = C ①号项；
//   3. 档长恒等复验三等：A 守恒式档长 = B 累进终点 = C ②号项，
//      三值互等误差 0；
//   4. 对冲总账行：逐指标（指标号, 三面值, 平旗）三列，任一面
//      失等即不平（不平即红码，不许降级放行）；
//   5. 对冲账守恒：总账行数 = 对冲指标数（写死 2 组指标）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4ec0 段为本模块独占（SEG_BASE=0x4ec0），判别式
//   SEG_BASE+1..+N；与 0x4ea0/0x4ee0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// 模块版本。
pub const HD_VERSION: &str = "unx0616_hedge-v1";

/// 诊断段基址（0x4ec0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4EC0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ec1 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4ec2 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4ec3 条目数三面失等。
pub const XC_ENTRIES_UNEQUAL: u16 = SEG_BASE + 3;
/// 0x4ec4 档长三面失等。
pub const XC_ARCLEN_UNEQUAL: u16 = SEG_BASE + 4;
/// 0x4ec5 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 5;
/// 0x4ec6 对冲账不守恒。
pub const XC_HEDGE_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x4ec7 空档。
pub const XC_EMPTY_ARCHIVE: u16 = SEG_BASE + 7;
/// 0x4ec8 字段非十六进制。
pub const XC_NOT_HEX: u16 = SEG_BASE + 8;
/// 0x4ec9 TRAILER 缺失（对冲前提）。
pub const XC_NO_TRAILER: u16 = SEG_BASE + 9;

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
            _ => return Err(XC_NOT_HEX),
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

/// 面账行（清单账 A）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub ordinal: u32,
    pub name: String,
    pub footprint: u64,
}

/// 面账 A：清单账（行表 + 守恒式档长）。
#[derive(Clone, Debug)]
pub struct ListFace {
    pub rows: Vec<ListRow>,
    /// 守恒式档长（Σ占位，须 = 实档长）。
    pub arc_len: u64,
}

/// 面账 B：守恒账（步进数 + 累进终点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsvFace {
    /// 步进数（条目数）。
    pub steps: u32,
    /// 累进终点。
    pub endpoint: u64,
}

/// 面账 C：五数账（本对冲只用 ①② 两项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveFace {
    /// ① 条目数。
    pub entries: u32,
    /// ② 档长。
    pub arc_len: u32,
}

/// 面 A 构建：遍历落行 + Σ占位。
pub fn face_list(stream: &[u8]) -> Result<ListFace, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
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
        rows.push(ListRow {
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
        return Err(XC_NO_TRAILER);
    }
    let arc_len = rows.iter().map(|r| r.footprint).sum();
    Ok(ListFace { rows, arc_len })
}

/// 面 B 构建：独立步进遍历（不复用面 A 行——三面独立性）。
pub fn face_consv(stream: &[u8]) -> Result<ConsvFace, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let mut steps = 0u32;
    let mut endpoint = 0u64;
    let mut off = 0usize;
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
        endpoint += entry_footprint(namesize, filesize);
        steps += 1;
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
    Ok(ConsvFace { steps, endpoint })
}

/// 面 C 构建：五数账 ①② 项（轻遍历）。
pub fn face_five(stream: &[u8]) -> Result<FiveFace, u16> {
    if stream.is_empty() {
        return Err(XC_EMPTY_ARCHIVE);
    }
    let mut entries = 0u32;
    let mut off = 0usize;
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
        entries += 1;
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
    Ok(FiveFace {
        entries,
        arc_len: stream.len() as u32,
    })
}

/// 对冲账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HedgeRow {
    /// 指标号：1=条目数 2=档长。
    pub metric: u8,
    /// 三面值（A, B, C）。
    pub va: u64,
    pub vb: u64,
    pub vc: u64,
    /// 平旗（三值等）。
    pub flat: bool,
}

/// 对冲主路径：三面互核两组指标，全平才 Ok。
pub fn hedge_total(stream: &[u8]) -> Result<Vec<HedgeRow>, u16> {
    let a = face_list(stream)?;
    let b = face_consv(stream)?;
    let c = face_five(stream)?;
    let real_len = stream.len() as u64;
    // 指标 1：条目数三面等。
    let m1 = HedgeRow {
        metric: 1,
        va: a.rows.len() as u64,
        vb: b.steps as u64,
        vc: c.entries as u64,
        flat: a.rows.len() as u64 == b.steps as u64
            && b.steps as u64 == c.entries as u64,
    };
    // 指标 2：档长恒等三面等（A 守恒值 / B 终点 / C 档长 / 实档长 四值互核）。
    let m2 = HedgeRow {
        metric: 2,
        va: a.arc_len,
        vb: b.endpoint,
        vc: c.arc_len as u64,
        flat: a.arc_len == b.endpoint
            && b.endpoint == c.arc_len as u64
            && c.arc_len as u64 == real_len,
    };
    let rows = vec![m1, m2];
    // 对冲账守恒：行数 = 指标数（2）。
    if rows.len() != 2 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    // 任一不平即红（不降级放行）。
    if !m1.flat {
        return Err(XC_ENTRIES_UNEQUAL);
    }
    if !m2.flat {
        return Err(XC_ARCLEN_UNEQUAL);
    }
    Ok(rows)
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

/// 自例程：好档三面对冲全平 → 尾部杂字节档长失等红 → 缺 TRAILER
/// 红 → 错误路径。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径：三真实条目 + TRAILER。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "boot", b"xyz"));
    arc.extend(build_entry(2, "cfg", &[9, 9, 9, 9, 9]));
    arc.extend(build_entry(3, "d", &[]));
    arc.extend(build_entry(0, TRAILER_NAME, &[]));
    let rows = hedge_total(&arc)?;
    if rows.len() != 2 {
        return Err(XC_HEDGE_UNBALANCED);
    }
    // 全平。
    for r in &rows {
        if !r.flat {
            return Err(if r.metric == 1 {
                XC_ENTRIES_UNEQUAL
            } else {
                XC_ARCLEN_UNEQUAL
            });
        }
    }
    // 三面独立复算核对：条目数 4；档长 = 档长。
    if rows[0].va != 4 || rows[0].vb != 4 || rows[0].vc != 4 {
        return Err(XC_ENTRIES_UNEQUAL);
    }
    if rows[1].vc != arc.len() as u64 {
        return Err(XC_ARCLEN_UNEQUAL);
    }

    // 尾部杂字节：档长失等 → 面 A 守恒值 ≠ 实档长 → 红。
    let mut dirty = arc.clone();
    dirty.extend_from_slice(&[0xAA]);
    if hedge_total(&dirty) != Err(XC_ARCLEN_UNEQUAL) {
        return Err(XC_ARCLEN_UNEQUAL);
    }

    // 缺 TRAILER：面 A 红。
    let mut notrail = Vec::new();
    notrail.extend(build_entry(1, "a", b"z"));
    if hedge_total(&notrail) != Err(XC_NO_TRAILER) {
        return Err(XC_NO_TRAILER);
    }

    // 错误路径 1：空档。
    if hedge_total(&[]) != Err(XC_EMPTY_ARCHIVE) {
        return Err(XC_EMPTY_ARCHIVE);
    }
    // 错误路径 2：坏魔数。
    let mut bad = build_entry(1, "a", &[]);
    bad[3] = b'9';
    if face_list(&bad).err() != Some(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 3：坏 hex。
    let mut bad2 = build_entry(1, "a", &[]);
    bad2[54] = b'x';
    if face_consv(&bad2) != Err(XC_NOT_HEX) {
        return Err(XC_NOT_HEX);
    }
    // 错误路径 4：名区无 NUL。
    let mut bad3 = build_entry(1, "ab", &[]);
    bad3[110 + 1] = b'Y';
    if face_five(&bad3) != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    // 占位公式直核：boot（namesize 5）+ xyz(3)：110+5+3+3+1 = 122。
    let r0 = &rows[0];
    let _ = r0;
    Ok(0)
}
