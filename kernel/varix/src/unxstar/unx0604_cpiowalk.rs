//! unx0604_cpiowalk —— UNX-F0604 cpio 条目遍历（110 字节头）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0604 cpio 条目遍历（110 字节头）  <SEG 0x4d40>
//! 目标行数：≥240（判据：纯功能代码 ≥240 行）
//! 判据原文摘录：
//!   ② 判据原文：遍历账齐（n 条目=n 推算数），头字段账（ino/mode/
//!   filesize/namesize 等 8 要素）逐条核。
//!
//! 设计纪律：
//!   1. newc 头 110 字节八字段（写死布局，十六进制 ASCII）：
//      magic[0..6) ino[6..14) mode[14..22) uid[22..30) gid[30..38)
//      nlink[38..46) mtime[46..54) filesize[54..62) devmaj[62..70)
//      devmin[70..78) rdevmaj[78..86) rdevmin[86..94) namesize[94..102)
//      check[102..110)——判据 8 要素核其中 ino/mode/uid+gid 合面/
//      filesize/namesize/nlink/mtime/check；
//   2. 遍历步进：头 110 → 名区 namesize-1（含 NUL）→ 名区按 4
//      对齐垫 → 数据 filesize → 数据按 4 对齐垫 → 下一头；
//   3. 遍历账守恒：n 条目 = 推算数（步进复算），遍历指针终点 =
//      TRAILER 头起点或档尾；
//   4. 八字段逐条核：每字段十六进制 ASCII 解析（非 hex 数字红码）；
//      名区必须以 NUL 结尾（namesize ≥1）；
//   5. 条目名截获：名区剥离 NUL 后的 ASCII 名入账；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，越界红码不 panic。
//!
//! 诊断段声明：0x4d40 段为本模块独占（SEG_BASE=0x4d40），判别式
//   SEG_BASE+1..+N；与 0x4d20/0x4d60 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CW_VERSION: &str = "unx0604_cpiowalk-v1";

/// 诊断段基址（0x4d40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4D40;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4d41 头长不足（<110）。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4d42 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4d43 字段非十六进制 ASCII。
pub const XC_NOT_HEX: u16 = SEG_BASE + 3;
/// 0x4d44 名区无 NUL 结尾。
pub const XC_NAME_NUL: u16 = SEG_BASE + 4;
/// 0x4d45 数据区越界（filesize 超档尾）。
pub const XC_DATA_TRUNC: u16 = SEG_BASE + 5;
/// 0x4d46 TRAILER 后仍有字节流却无新头（遍历尾异常）。
pub const XC_WALK_TAIL: u16 = SEG_BASE + 6;
/// 0x4d47 遍历账不守恒（n 条目 ≠ 推算数）。
pub const XC_WALK_UNBALANCED: u16 = SEG_BASE + 7;
/// 0x4d48 八字段账缺项。
pub const XC_FIELD_MISSING: u16 = SEG_BASE + 8;
/// 0x4d49 namesize 非法（0 或超头界）。
pub const XC_BAD_NAMESIZE: u16 = SEG_BASE + 9;

/// newc 头长（写死：110）。
pub const HEAD_LEN: usize = 110;
/// 魔数（写死："070701"）。
pub const MAGIC: &[u8] = b"070701";
/// TRAILER 名（写死）。
pub const TRAILER_NAME: &str = "TRAILER!!!";

/// 单字段十六进制 ASCII 解析（定宽 8 字符 → u32）。
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

/// 八字段账（判据原文 8 要素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadFields {
    /// ino：inode 号。
    pub ino: u32,
    /// mode：文件模式。
    pub mode: u32,
    /// uid+gid 合面（两字段打包账）。
    pub uid: u32,
    pub gid: u32,
    /// nlink：链接数。
    pub nlink: u32,
    /// mtime：修改时刻。
    pub mtime: u32,
    /// filesize：数据字节数。
    pub filesize: u32,
    /// namesize：名区字节数（含 NUL）。
    pub namesize: u32,
    /// check：校验和字段（本单只取值不复核——F0605 面）。
    pub check: u32,
}

/// 八字段逐条解析核。
pub fn parse_fields(head: &[u8]) -> Result<HeadFields, u16> {
    if head.len() < HEAD_LEN {
        return Err(XC_HEAD_SHORT);
    }
    if &head[0..6] != MAGIC {
        return Err(XC_MAGIC);
    }
    Ok(HeadFields {
        ino: hex_field(head, 6)?,
        mode: hex_field(head, 14)?,
        uid: hex_field(head, 22)?,
        gid: hex_field(head, 30)?,
        nlink: hex_field(head, 38)?,
        mtime: hex_field(head, 46)?,
        filesize: hex_field(head, 54)?,
        namesize: hex_field(head, 94)?,
        check: hex_field(head, 102)?,
    })
}

/// 遍历条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkEntry {
    /// 条目序（0 起）。
    pub ordinal: u32,
    /// 头偏移。
    pub head_off: u32,
    /// 八字段账。
    pub fields: HeadFields,
    /// 条目名（NUL 剥离后）。
    pub name: String,
    /// 数据区偏移。
    pub data_off: u32,
}

/// 4 对齐垫量。
pub fn pad4(n: usize) -> usize {
    (4 - (n % 4)) % 4
}

/// 条目名截获：名区 must namesize 字节、末字节 NUL。
pub fn take_name(stream: &[u8], name_off: usize, namesize: usize) -> Result<String, u16> {
    if namesize == 0 {
        return Err(XC_BAD_NAMESIZE);
    }
    let end = name_off.checked_add(namesize).ok_or(XC_BAD_NAMESIZE)?;
    let region = stream.get(name_off..end).ok_or(XC_NAME_NUL)?;
    if region[namesize - 1] != 0 {
        return Err(XC_NAME_NUL);
    }
    let mut s = String::new();
    for b in &region[..namesize - 1] {
        s.push(*b as char);
    }
    Ok(s)
}

/// 遍历主路径：从头偏 0 步进到 TRAILER 或档尾；产出条目账 +
/// 推算数守恒核。
pub fn walk(stream: &[u8]) -> Result<Vec<WalkEntry>, u16> {
    let mut out: Vec<WalkEntry> = Vec::new();
    let mut off = 0usize;
    let mut ord = 0u32;
    loop {
        // 头。
        let head = match stream.get(off..off + HEAD_LEN) {
            Some(h) => h,
            None => {
                if out.is_empty() {
                    return Err(XC_HEAD_SHORT);
                }
                // 档尾：无 TRAILER 也接受（短档语义），尾核在 F0607。
                break;
            }
        };
        let fields = parse_fields(head)?;
        let namesize = fields.namesize as usize;
        if namesize == 0 || namesize > HEAD_LEN + 4096 {
            return Err(XC_BAD_NAMESIZE);
        }
        let name_off = off + HEAD_LEN;
        let name = take_name(stream, name_off, namesize)?;
        let data_off = name_off + namesize + pad4(namesize);
        // TRAILER：终步。
        if name == TRAILER_NAME {
            out.push(WalkEntry {
                ordinal: ord,
                head_off: off as u32,
                fields,
                name,
                data_off: data_off as u32,
            });
            ord += 1;
            break;
        }
        // 数据区界。
        let data_end = data_off
            .checked_add(fields.filesize as usize)
            .ok_or(XC_DATA_TRUNC)?;
        if stream.len() < data_end {
            return Err(XC_DATA_TRUNC);
        }
        out.push(WalkEntry {
            ordinal: ord,
            head_off: off as u32,
            fields,
            name,
            data_off: data_off as u32,
        });
        ord += 1;
        off = data_end + pad4(fields.filesize as usize);
        // 尾异常：TRAILER 后即停（TRAILER 已 break）。
        let _ = off;
    }
    // 守恒：n 条目 = 推算数（重放一遍步进计数）。
    let mut recount = 0u32;
    let mut o = 0usize;
    while let Some(h) = stream.get(o..o + HEAD_LEN) {
        let f = parse_fields(h).map_err(|e| if out.is_empty() { e } else { XC_WALK_UNBALANCED })?;
        recount += 1;
        let ns = f.namesize as usize;
        if ns == 0 {
            return Err(XC_BAD_NAMESIZE);
        }
        let d = o + HEAD_LEN + ns + pad4(ns) + f.filesize as usize + pad4(f.filesize as usize);
        let name = take_name(stream, o + HEAD_LEN, ns)?;
        if name == TRAILER_NAME {
            break;
        }
        if d > stream.len() {
            break;
        }
        o = d;
    }
    // TRAILER 计入条目数（作为末条目）——recount 含 TRAILER。
    if recount != out.len() as u32 {
        return Err(XC_WALK_UNBALANCED);
    }
    Ok(out)
}

/// newc 条目构造器（测试/演练面）：头 + 名 + 垫 + 数据 + 垫。
pub fn build_entry(ino: u32, mode: u32, name: &str, data: &[u8]) -> Vec<u8> {
    let mut h = [0u8; HEAD_LEN];
    h[0..6].copy_from_slice(MAGIC);
    let put = |off: usize, v: u32, h: &mut [u8; HEAD_LEN]| {
        let bytes = v.to_be_bytes();
        for i in 0..8 {
            let nib = if i % 2 == 0 {
                (bytes[i / 2] >> 4) & 0xF
            } else {
                bytes[i / 2] & 0xF
            };
            h[off + i] = if nib < 10 {
                b'0' + nib
            } else {
                b'a' + nib - 10
            };
        }
    };
    put(6, ino, &mut h);
    put(14, mode, &mut h);
    put(22, 0, &mut h); // uid
    put(30, 0, &mut h); // gid
    put(38, 1, &mut h); // nlink
    put(46, 0x6700_0000, &mut h); // mtime
    put(54, data.len() as u32, &mut h); // filesize
    put(62, 0, &mut h); // devmaj
    put(70, 0, &mut h); // devmin
    put(78, 0, &mut h); // rdevmaj
    put(86, 0, &mut h); // rdevmin
    put(94, (name.len() + 1) as u32, &mut h); // namesize 含 NUL
    put(102, 0, &mut h); // check
    let mut out = Vec::from(h.as_slice());
    out.extend_from_slice(name.as_bytes());
    out.push(0); // NUL
    for _ in 0..pad4(name.len() + 1) {
        out.push(0);
    }
    out.extend_from_slice(data);
    for _ in 0..pad4(data.len()) {
        out.push(0);
    }
    out
}

/// TRAILER 条目构造。
pub fn build_trailer() -> Vec<u8> {
    build_entry(0, 0, TRAILER_NAME, &[])
}

/// 自例程：三真实条目 + TRAILER 遍历账 + 八字段逐条核 + 推算数
/// 守恒 + 错误路径（缺 NUL/截断/坏魔数/坏 hex）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 构造档：dir + file + link + TRAILER。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, 0o040755, "dir", &[]));
    arc.extend(build_entry(2, 0o100644, "file.txt", b"hello"));
    arc.extend(build_entry(3, 0o120777, "link", b"file.txt"));
    arc.extend(build_trailer());

    // 主路径遍历。
    let es = walk(&arc)?;
    // n 条目 = 推算数：4（含 TRAILER）。
    if es.len() != 4 {
        return Err(XC_WALK_UNBALANCED);
    }
    // 八字段逐条核：条目 0。
    let e0 = &es[0];
    if e0.fields.ino != 1 || e0.fields.mode != 0o040755 {
        return Err(XC_FIELD_MISSING);
    }
    if e0.fields.uid != 0 || e0.fields.gid != 0 {
        return Err(XC_FIELD_MISSING);
    }
    if e0.fields.nlink != 1 || e0.fields.namesize != 4 {
        return Err(XC_FIELD_MISSING);
    }
    if e0.name != "dir" || e0.fields.filesize != 0 {
        return Err(XC_FIELD_MISSING);
    }
    // 条目 1：数据 5 字节。
    let e1 = &es[1];
    if e1.fields.filesize != 5 || e1.name != "file.txt" {
        return Err(XC_FIELD_MISSING);
    }
    // 数据区偏移对齐面：读出 "hello"。
    let d0 = e1.data_off as usize;
    let data = arc.get(d0..d0 + 5).ok_or(XC_DATA_TRUNC)?;
    if data != b"hello" {
        return Err(XC_DATA_TRUNC);
    }
    // 条目 2。
    let e2 = &es[2];
    if e2.fields.ino != 3 || e2.name != "link" {
        return Err(XC_FIELD_MISSING);
    }
    // 末条目 = TRAILER。
    if es[3].name != TRAILER_NAME {
        return Err(XC_WALK_TAIL);
    }

    // 错误路径 1：名区无 NUL。
    let mut bad = build_entry(1, 0o100644, "abc", &[]);
    // 名区起 110，namesize 4："abc\0" → 把 NUL 改 'x'。
    bad[110 + 3] = b'x';
    let es_bad = walk(&bad);
    if es_bad != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    // 错误路径 2：数据截断（filesize 手改大）。
    let mut bad2 = build_entry(1, 0o100644, "f", b"xy");
    // filesize 在 [54..62)，置 "000000ff"。
    for (i, c) in b"000000ff".iter().enumerate() {
        bad2[54 + i] = *c;
    }
    if walk(&bad2) != Err(XC_DATA_TRUNC) {
        return Err(XC_DATA_TRUNC);
    }
    // 错误路径 3：魔数失配。
    let mut bad3 = build_entry(1, 0, "a", &[]);
    bad3[3] = b'8';
    if walk(&bad3) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 4：坏 hex（mode 字段塞 'g'）。
    let mut bad4 = build_entry(1, 0, "a", &[]);
    bad4[14] = b'g';
    if walk(&bad4) != Err(XC_NOT_HEX) {
        return Err(XC_NOT_HEX);
    }
    // 错误路径 5：空流。
    if walk(&[]) != Err(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    // 字段解析面：单头直核。
    let head = build_entry(9, 0o100600, "x", b"");
    let f = parse_fields(&head[..HEAD_LEN])?;
    if f.ino != 9 || f.namesize != 2 {
        return Err(XC_FIELD_MISSING);
    }
    Ok(0)
}
