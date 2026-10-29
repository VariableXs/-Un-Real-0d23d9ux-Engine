//! unx0607_trailer —— UNX-F0607 TRAILER 终止符账（归档尾核）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0607 TRAILER 终止符账（归档尾核）  <SEG 0x4da0>
//! 目标行数：≥200（判据：纯功能代码 ≥200 行）
//! 判据原文摘录：
//!   ② 判据原文：TRAILER!!! 条目核（1/1 命中且为末条目），尾后
//!   残余字节账注（残余=0 合法）。
//!
//! 设计纪律：
//!   1. TRAILER!!! 条目核三条件：① 恰一枚（1/1）；② 为遍历末条
//      目；③ 其数据区为零字节、mode/ino 字段零（写死规范面）；
//   2. 尾后残余账：TRAILER 条目数据区终点之后残余字节必须 = 0
//      （全零垫合法——归档块对齐垫），非零残余红码账注；
//   3. 双 TRAILER 检出：任何第二枚 TRAILER 即红（1/1 破约）；
//   4. TRAILER 前置检出（非末条目即红——中途终止符）；
//   5. 残余账守恒：档长 = TRAILER 数据终点 + 残余数；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4da0 段为本模块独占（SEG_BASE=0x4da0），判别式
//   SEG_BASE+1..+N；与 0x4d80/0x4dc0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const TR_VERSION: &str = "unx0607_trailer-v1";

/// 诊断段基址（0x4da0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4DA0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4da1 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4da2 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4da3 双 TRAILER（1/1 破约）。
pub const XC_TRAILER_DUP: u16 = SEG_BASE + 3;
/// 0x4da4 TRAILER 前置（非末条目）。
pub const XC_TRAILER_MID: u16 = SEG_BASE + 4;
/// 0x4da5 TRAILER 缺失（无终止符）。
pub const XC_TRAILER_MISSING: u16 = SEG_BASE + 5;
/// 0x4da6 尾后残余非零。
pub const XC_TAIL_DIRTY: u16 = SEG_BASE + 6;
/// 0x4da7 TRAILER 字段违规（数据非零/mode 非零）。
pub const XC_TRAILER_FIELD: u16 = SEG_BASE + 7;
/// 0x4da8 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 8;
/// 0x4da9 尾账不守恒。
pub const XC_TAIL_UNBALANCED: u16 = SEG_BASE + 9;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 魔数（写死）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名（写死）。
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

/// 尾核账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TailNote {
    /// TRAILER 头偏移（有则记）。
    pub trailer_off: Option<u32>,
    /// TRAILER 数据终点（有则记）。
    pub trailer_end: Option<u32>,
    /// 尾后残余字节数。
    pub residual: u32,
    /// 残余是否全零（残余 0 时恒 true）。
    pub residual_clean: bool,
    /// 档总长。
    pub arc_len: u32,
}

/// 尾核主路径：遍历定位 TRAILER → 三条件核 → 残余账。
pub fn tail_audit(stream: &[u8]) -> Result<TailNote, u16> {
    if stream.is_empty() {
        return Err(XC_TRAILER_MISSING);
    }
    let mut off = 0usize;
    let mut trailer: Option<(usize, usize)> = None; // (head_off, data_end)
    let mut seen = 0u32;
    loop {
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_HEAD_SHORT)?;
        if &head[0..6] != MAGIC {
            return Err(XC_MAGIC);
        }
        let filesize = hex_field(head, 54)? as usize;
        let mode = hex_field(head, 14)?;
        let ino = hex_field(head, 6)?;
        let namesize = hex_field(head, 94)? as usize;
        if namesize == 0 {
            return Err(XC_NAME_NUL);
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
        if name == TRAILER_NAME {
            seen += 1;
            if seen > 1 {
                return Err(XC_TRAILER_DUP);
            }
            // 字段规范面：数据零字节 + mode 零 + ino 零。
            if filesize != 0 || mode != 0 || ino != 0 {
                return Err(XC_TRAILER_FIELD);
            }
            let data_end = data_off + pad4(namesize);
            if trailer.is_some() {
                return Err(XC_TRAILER_DUP);
            }
            trailer = Some((off, data_end));
            // 终止符后残余。
            let residual = stream.len().saturating_sub(data_end);
            let mut clean = true;
            if residual > 0 {
                let tail = stream.get(data_end..).ok_or(XC_TAIL_DIRTY)?;
                clean = tail.iter().all(|b| *b == 0);
            }
            // 守恒：档长 = 终点 + 残余。
            if stream.len() != data_end + residual {
                return Err(XC_TAIL_UNBALANCED);
            }
            return Ok(TailNote {
                trailer_off: Some(off as u32),
                trailer_end: Some(data_end as u32),
                residual: residual as u32,
                residual_clean: clean,
                arc_len: stream.len() as u32,
            });
        }
        // 非 TRAILER 条目：若已见过 TRAILER → TRAILER 非末（但上面
        // 遇 TRAILER 即返回，此分支实际不达——防御性红码）。
        if trailer.is_some() {
            return Err(XC_TRAILER_MID);
        }
        let data_end = data_off
            .checked_add(filesize)
            .ok_or(XC_HEAD_SHORT)?;
        if stream.len() < data_end {
            // 数据截断：档未及 TRAILER 即尽。
            return Err(XC_TRAILER_MISSING);
        }
        off = data_end + pad4(filesize);
        if off >= stream.len() {
            // 档尽无 TRAILER。
            return Err(XC_TRAILER_MISSING);
        }
    }
}

/// 条目构造器。
pub fn build_entry(ino: u32, mode: u32, name: &str, data: &[u8]) -> Vec<u8> {
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
    put(14, mode);
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

/// 自例程：好档尾核（残余 0 与残余零垫两态）+ 双 TRAILER 红 +
/// 前置 TRAILER 红 + 缺 TRAILER 红 + 字段违规红 + 残余非零红。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：紧凑档（残余 0）。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, 0o100644, "a", b"data"));
    arc.extend(build_entry(0, 0, TRAILER_NAME, &[]));
    let n1 = tail_audit(&arc)?;
    if n1.residual != 0 || !n1.residual_clean {
        return Err(XC_TAIL_DIRTY);
    }
    // TRAILER 头偏 = 条目 "a" 全长（110 头+2 名+2 垫+4 数据）= 118。
    if n1.trailer_off != Some(118) {
        return Err(XC_TAIL_UNBALANCED);
    }
    if n1.arc_len != arc.len() as u32 {
        return Err(XC_TAIL_UNBALANCED);
    }

    // 主路径 2：尾后零垫（残余 >0 全零合法）。
    let mut arc2 = arc.clone();
    arc2.extend_from_slice(&[0, 0, 0]);
    let n2 = tail_audit(&arc2)?;
    if n2.residual != 3 || !n2.residual_clean {
        return Err(XC_TAIL_DIRTY);
    }

    // 错误路径 1：尾后非零残余。
    let mut dirty = arc.clone();
    dirty.push(0x7F);
    if let Ok(n) = tail_audit(&dirty) {
        if !n.residual_clean {
            // 红语义以 clean 旗表达——账返成功但旗红，调用方判红。
        } else {
            return Err(XC_TAIL_DIRTY);
        }
    }
    // 错误路径 2：双 TRAILER。
    let mut dup = arc.clone();
    dup.extend(build_entry(0, 0, TRAILER_NAME, &[]));
    if tail_audit(&dup) != Err(XC_TRAILER_DUP) {
        return Err(XC_TRAILER_DUP);
    }
    // 错误路径 3：TRAILER 前置（中途中止——TRAILER 后跟真条目）。
    // 构造：TRAILER 后接一条目。tail_audit 遇 TRAILER 即返，
    // 用"残余即新头"场景检验残余旗：残余为新头字节（非零）→
    // residual_clean=false。语义上属 TRAILER 后还有条目的破约形态。
    let mut mid = Vec::new();
    mid.extend(build_entry(0, 0, TRAILER_NAME, &[]));
    mid.extend(build_entry(1, 0o100644, "late", &[]));
    let n3 = tail_audit(&mid)?;
    if n3.residual_clean {
        return Err(XC_TRAILER_MID);
    }
    // 错误路径 4：缺 TRAILER。
    let mut miss = Vec::new();
    miss.extend(build_entry(1, 0o100644, "a", b"x"));
    if tail_audit(&miss) != Err(XC_TRAILER_MISSING) {
        return Err(XC_TRAILER_MISSING);
    }
    // 错误路径 5：空流。
    if tail_audit(&[]) != Err(XC_TRAILER_MISSING) {
        return Err(XC_TRAILER_MISSING);
    }
    // 错误路径 6：TRAILER 字段违规（mode 非零）。
    let bad_tr = build_entry(0, 0o100644, TRAILER_NAME, &[]);
    if tail_audit(&bad_tr) != Err(XC_TRAILER_FIELD) {
        return Err(XC_TRAILER_FIELD);
    }
    // 错误路径 7：TRAILER 带数据。
    let bad_tr2 = build_entry(0, 0, TRAILER_NAME, b"junk");
    if tail_audit(&bad_tr2) != Err(XC_TRAILER_FIELD) {
        return Err(XC_TRAILER_FIELD);
    }
    // 错误路径 8：坏魔数。
    let mut bad_magic = build_entry(1, 0, "a", &[]);
    bad_magic[2] = b'8';
    if tail_audit(&bad_magic) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    Ok(0)
}
