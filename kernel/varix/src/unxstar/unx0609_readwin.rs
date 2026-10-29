//! unx0609_readwin —— UNX-F0609 条目只读窗口原语（数据读）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0609 条目只读窗口原语（数据读）  <SEG 0x4de0>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：窗口原语账齐（按条目开窗读——10/10 用例读出与
//!   基准一致），越窗读拦截 100%。
//!
//! 设计纪律：
//!   1. 窗口原语：按（条目名，窗偏移，窗长）在条目数据区开只读
//      窗——窗必须完全落在 [0, filesize) 界内；
//   2. 越窗拦截 100%：窗尾 > filesize / 窗偏倒挂（offset+len 溢
//      出面）/ 空窗（len 0）三形全红码；
//   3. 10/10 用例账（写死）：窗首读 / 窗中读 / 窗尾读 / 全窗读 /
//      单字节窗 / 目录零长条目拒窗 / 越尾 1 字节拒 / 大偏移拒 /
//      溢出面拒 / 逐窗拼接对拍基准；
//   4. 只读红线：窗口原语不产写路径（原语签名只返借用面副本）；
//   5. 窗读账守恒：Σ窗长 = Σ读出字节数（逐窗可复算）；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4de0 段为本模块独占（SEG_BASE=0x4de0），判别式
//   SEG_BASE+1..+N；与 0x4dc0/0x4e00 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const RW_VERSION: &str = "unx0609_readwin-v1";

/// 诊断段基址（0x4de0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4DE0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4de1 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4de2 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4de3 条目未命中。
pub const XC_ENTRY_MISS: u16 = SEG_BASE + 3;
/// 0x4de4 越窗读（窗尾 > filesize）。
pub const XC_WINDOW_OVER: u16 = SEG_BASE + 4;
/// 0x4de5 空窗（窗长 0）。
pub const XC_WINDOW_EMPTY: u16 = SEG_BASE + 5;
/// 0x4de6 窗偏移加窗长溢出。
pub const XC_WINDOW_SPAN: u16 = SEG_BASE + 6;
/// 0x4de7 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 7;
/// 0x4de8 窗读账不守恒。
pub const XC_WIN_UNBALANCED: u16 = SEG_BASE + 8;
/// 0x4de9 拼接对拍失配。
pub const XC_CONCAT_MISMATCH: u16 = SEG_BASE + 9;

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

/// 条目定位行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryLoc {
    /// 条目名。
    pub name: String,
    /// 数据区偏移。
    pub data_off: u32,
    /// filesize。
    pub filesize: u32,
}

/// 名表提取（轻遍历；TRAILER 不入表）。
pub fn locate_entries(stream: &[u8]) -> Result<Vec<EntryLoc>, u16> {
    let mut out = Vec::new();
    let mut off = 0usize;
    loop {
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_HEAD_SHORT)?;
        if &head[0..6] != MAGIC {
            return Err(XC_MAGIC);
        }
        let filesize = hex_field(head, 54)?;
        let namesize = hex_field(head, 94)? as usize;
        if namesize == 0 {
            return Err(XC_HEAD_SHORT);
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
            break;
        }
        let data_end = data_off + filesize as usize;
        if stream.len() < data_end {
            return Err(XC_HEAD_SHORT);
        }
        out.push(EntryLoc {
            name,
            data_off: data_off as u32,
            filesize,
        });
        off = data_end + pad4(filesize as usize);
        if off >= stream.len() {
            return Err(XC_HEAD_SHORT);
        }
    }
    if out.is_empty() {
        return Err(XC_HEAD_SHORT);
    }
    Ok(out)
}

/// 窗口原语主路径：按条目名开只读窗，返回窗内副本。
/// 越窗/空窗/溢出全红码，窗内容 = stream[data_off+off .. +off+len]。
pub fn read_window(
    stream: &[u8],
    table: &[EntryLoc],
    name: &str,
    off: u32,
    len: u32,
) -> Result<Vec<u8>, u16> {
    if name.is_empty() {
        return Err(XC_ENTRY_MISS);
    }
    if len == 0 {
        return Err(XC_WINDOW_EMPTY);
    }
    let loc = table
        .iter()
        .find(|l| l.name == name)
        .ok_or(XC_ENTRY_MISS)?;
    if loc.filesize == 0 {
        // 零长条目（目录）无窗可开。
        return Err(XC_WINDOW_OVER);
    }
    // 溢出面：off+len 必须 ≤ filesize 且不回卷。
    let end = off
        .checked_add(len)
        .ok_or(XC_WINDOW_SPAN)?;
    if end > loc.filesize {
        return Err(XC_WINDOW_OVER);
    }
    let start = loc.data_off as usize + off as usize;
    let stop = loc.data_off as usize + end as usize;
    let win = stream
        .get(start..stop)
        .ok_or(XC_WINDOW_OVER)?;
    Ok(win.to_vec())
}

/// 窗读账：逐窗落账 + 守恒。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinRecord {
    /// 条目名。
    pub name: String,
    /// 窗偏。
    pub off: u32,
    /// 窗长。
    pub len: u32,
    /// 读出字节和（窗内容逐字节和 mod 65536——对拍面）。
    pub content_sum: u32,
}

/// 拼接对拍：逐窗副本顺序连接 = 基准全量（等价全窗读）。
pub fn concat_check(parts: &[Vec<u8>], whole: &[u8]) -> Result<(), u16> {
    let mut joined: Vec<u8> = Vec::new();
    for p in parts {
        joined.extend_from_slice(p);
    }
    if joined.len() != whole.len() {
        return Err(XC_CONCAT_MISMATCH);
    }
    for (a, b) in joined.iter().zip(whole.iter()) {
        if a != b {
            return Err(XC_CONCAT_MISMATCH);
        }
    }
    Ok(())
}

/// 窗读账总装。
#[derive(Debug, Default)]
pub struct WinBook {
    pub records: Vec<WinRecord>,
    /// Σ窗长。
    pub total_len: u32,
}

impl WinBook {
    /// 开窗入账。
    pub fn open(
        &mut self,
        stream: &[u8],
        table: &[EntryLoc],
        name: &str,
        off: u32,
        len: u32,
    ) -> Result<Vec<u8>, u16> {
        let win = read_window(stream, table, name, off, len)?;
        if win.len() as u32 != len {
            return Err(XC_WIN_UNBALANCED);
        }
        let mut s = 0u32;
        for b in &win {
            s = (s + *b as u32) % 65536;
        }
        self.records.push(WinRecord {
            name: String::from(name),
            off,
            len,
            content_sum: s,
        });
        self.total_len += len;
        Ok(win)
    }

    /// 守恒：Σ记录窗长 = total_len。
    pub fn conserved(&self) -> Result<(), u16> {
        let mut sum = 0u32;
        for r in &self.records {
            sum += r.len;
        }
        if sum != self.total_len {
            return Err(XC_WIN_UNBALANCED);
        }
        Ok(())
    }
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

/// 自例程：10/10 用例账 + 越窗拦截 + 拼接对拍 + 守恒 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 档：file(8 字节 0x10..0x17) / dir(零长) / big(16 字节)。
    let fdata: Vec<u8> = (0x10u8..0x18u8).collect();
    let bdata: Vec<u8> = (0x00u8..0x10u8).collect();
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "file", &fdata));
    arc.extend(build_entry(2, "dir", &[]));
    arc.extend(build_entry(3, "big", &bdata));
    arc.extend(build_entry(0, TRAILER_NAME, &[]));

    let table = locate_entries(&arc)?;
    if table.len() != 3 {
        return Err(XC_HEAD_SHORT);
    }
    let mut wb = WinBook::default();

    // 10/10 用例（读出与基准一致）。
    // 1 窗首读。
    let w1 = wb.open(&arc, &table, "file", 0, 3)?;
    if w1 != fdata[..3] {
        return Err(XC_CONCAT_MISMATCH);
    }
    // 2 窗中读。
    let w2 = wb.open(&arc, &table, "file", 3, 2)?;
    if w2 != fdata[3..5] {
        return Err(XC_CONCAT_MISMATCH);
    }
    // 3 窗尾读。
    let w3 = wb.open(&arc, &table, "file", 6, 2)?;
    if w3 != fdata[6..8] {
        return Err(XC_CONCAT_MISMATCH);
    }
    // 4 全窗读。
    let w4 = wb.open(&arc, &table, "file", 0, 8)?;
    if w4 != fdata {
        return Err(XC_CONCAT_MISMATCH);
    }
    // 5 单字节窗。
    let w5 = wb.open(&arc, &table, "big", 15, 1)?;
    if w5 != bdata[15..16] {
        return Err(XC_CONCAT_MISMATCH);
    }
    // 6 目录零长条目拒窗。
    if wb.open(&arc, &table, "dir", 0, 1) != Err(XC_WINDOW_OVER) {
        return Err(XC_WINDOW_OVER);
    }
    // 7 越尾 1 字节拒。
    if wb.open(&arc, &table, "file", 7, 2) != Err(XC_WINDOW_OVER) {
        return Err(XC_WINDOW_OVER);
    }
    // 8 大偏移拒。
    if wb.open(&arc, &table, "file", 100, 1) != Err(XC_WINDOW_OVER) {
        return Err(XC_WINDOW_OVER);
    }
    // 9 溢出面拒（u32 边界）。
    if wb.open(&arc, &table, "file", u32::MAX - 1, 4) != Err(XC_WINDOW_SPAN) {
        return Err(XC_WINDOW_SPAN);
    }
    // 10 未命中条目拒。
    if wb.open(&arc, &table, "ghost", 0, 1) != Err(XC_ENTRY_MISS) {
        return Err(XC_ENTRY_MISS);
    }
    // 空窗拒。
    if wb.open(&arc, &table, "file", 0, 0) != Err(XC_WINDOW_EMPTY) {
        return Err(XC_WINDOW_EMPTY);
    }
    wb.conserved()?;

    // 拼接对拍：file 三窗拼接 = 全量。
    concat_check(&[w1, w2, w3], &fdata)?;
    // big 满窗 = 全量。
    let wb_full = read_window(&arc, &table, "big", 0, 16)?;
    concat_check(&[wb_full], &bdata)?;

    // 窗读账守恒面：Σ窗长 = 3+2+2+8+1 = 16。
    if wb.total_len != 16 {
        return Err(XC_WIN_UNBALANCED);
    }

    // 错误路径：坏魔数 / 空 NUL / 空表。
    let mut bad = build_entry(1, "x", b"y");
    bad[4] = b'8';
    if locate_entries(&bad) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    let mut bad2 = build_entry(1, "ab", &[]);
    bad2[110 + 1] = b'Q';
    if locate_entries(&bad2) != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    let single_tr = build_entry(0, TRAILER_NAME, &[]);
    if locate_entries(&single_tr) != Err(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    Ok(0)
}
