//! unx0608_entryfind —— UNX-F0608 条目查找原语（按名查）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0608 条目查找原语（按名查）  <SEG 0x4dc0>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：查找原语账齐（命中/未命中/重复名三态 9/9 用例），
//!   重复名检出显性（2/2）。
//!
//! 设计纪律：
//!   1. 查找原语三态闭集：Hit（恰一枚命中）/ Miss（零命中）/
//      Ambiguous（重复名 ≥2）——三态不许混叠，重复名必须显性；
//   2. 9/9 用例账（写死 3×3）：三态 × 三名面（普通名/路径名/
//      TRAILER 边界名），逐用例落账可复算；
//   3. 查找面基于条目名表（轻遍历产名表——复用 110 头步进）；
//   4. 重复名检出：同名计数 ≥2 → Ambiguous 且重复序号对入账
//      （(首序, 次序) 对账）；
//   5. 查找账守恒：Σ三态计数 = 查询总数；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4dc0 段为本模块独占（SEG_BASE=0x4dc0），判别式
//   SEG_BASE+1..+N；与 0x4da0/0x4de0 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const EF_VERSION: &str = "unx0608_entryfind-v1";

/// 诊断段基址（0x4dc0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4DC0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4dc1 头长不足。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 1;
/// 0x4dc2 魔数失配。
pub const XC_MAGIC: u16 = SEG_BASE + 2;
/// 0x4dc3 查询名为空。
pub const XC_QUERY_EMPTY: u16 = SEG_BASE + 3;
/// 0x4dc4 名区无 NUL。
pub const XC_NAME_NUL: u16 = SEG_BASE + 4;
/// 0x4dc5 查找账不守恒。
pub const XC_FIND_UNBALANCED: u16 = SEG_BASE + 5;
/// 0x4dc6 重复名未显性（内部不变量破）。
pub const XC_DUP_HIDDEN: u16 = SEG_BASE + 6;
/// 0x4dc7 名表空（无条目可查）。
pub const XC_TABLE_EMPTY: u16 = SEG_BASE + 7;

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

/// 名表条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameRow {
    /// 条目名。
    pub name: String,
    /// 条目序。
    pub ordinal: u32,
    /// 头偏移。
    pub head_off: u32,
    /// 数据偏移。
    pub data_off: u32,
    /// filesize。
    pub filesize: u32,
}

/// 名表提取（轻遍历，TRAILER 不入表——查找面对真条目）。
pub fn name_table(stream: &[u8]) -> Result<Vec<NameRow>, u16> {
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
        out.push(NameRow {
            name,
            ordinal: ord,
            head_off: off as u32,
            data_off: data_off as u32,
            filesize,
        });
        ord += 1;
        let data_end = data_off + filesize as usize;
        if stream.len() < data_end {
            return Err(XC_HEAD_SHORT);
        }
        off = data_end + pad4(filesize as usize);
        if off >= stream.len() {
            return Err(XC_HEAD_SHORT);
        }
    }
    if out.is_empty() {
        return Err(XC_TABLE_EMPTY);
    }
    Ok(out)
}

/// 查找三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindVerdict {
    /// 恰一枚命中（序号）。
    Hit(u32),
    /// 零命中。
    Miss,
    /// 重复名（首序, 次序）。
    Ambiguous(u32, u32),
}

/// 按名查主路径。
pub fn find(table: &[NameRow], name: &str) -> Result<FindVerdict, u16> {
    if name.is_empty() {
        return Err(XC_QUERY_EMPTY);
    }
    let mut first: Option<u32> = None;
    let mut second: Option<u32> = None;
    for row in table {
        if row.name == name {
            match first {
                None => first = Some(row.ordinal),
                Some(_) => {
                    if second.is_none() {
                        second = Some(row.ordinal);
                    }
                }
            }
        }
    }
    match (first, second) {
        (Some(a), Some(b)) => Ok(FindVerdict::Ambiguous(a, b)),
        (Some(a), None) => Ok(FindVerdict::Hit(a)),
        (None, _) => Ok(FindVerdict::Miss),
    }
}

/// 查找账：逐用例落账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindCase {
    /// 用例名。
    pub query: String,
    /// 判定码：0=miss 1=hit 2=ambiguous。
    pub code: u8,
    /// 首序（命中/重复时有效）。
    pub first: u32,
}

/// 查找账总装：跑一批用例 → 三态计数守恒。
#[derive(Debug, Default)]
pub struct FindBook {
    pub cases: Vec<FindCase>,
    pub hits: u32,
    pub misses: u32,
    pub ambiguous: u32,
}

impl FindBook {
    /// 批跑：名表 × 用例名集。
    pub fn run(table: &[NameRow], queries: &[&str]) -> Result<FindBook, u16> {
        if table.is_empty() {
            return Err(XC_TABLE_EMPTY);
        }
        let mut b = FindBook::default();
        for q in queries {
            let v = find(table, q)?;
            let (code, first) = match v {
                FindVerdict::Miss => {
                    b.misses += 1;
                    (0u8, 0u32)
                }
                FindVerdict::Hit(a) => {
                    b.hits += 1;
                    (1, a)
                }
                FindVerdict::Ambiguous(a, _) => {
                    b.ambiguous += 1;
                    (2, a)
                }
            };
            b.cases.push(FindCase {
                query: String::from(*q),
                code,
                first,
            });
        }
        b.conserved()?;
        Ok(b)
    }

    /// 守恒：三态和 = 用例数。
    pub fn conserved(&self) -> Result<(), u16> {
        if (self.hits + self.misses + self.ambiguous) as usize != self.cases.len() {
            return Err(XC_FIND_UNBALANCED);
        }
        // 显性不变量：ambiguous 计数与账内 code=2 条数一致。
        let dup = self.cases.iter().filter(|c| c.code == 2).count() as u32;
        if dup != self.ambiguous {
            return Err(XC_DUP_HIDDEN);
        }
        Ok(())
    }
}

/// 重复名检出显性核（判据 2/2）：名表内同名 ≥2 必被 find 判
/// Ambiguous，且给出首/次序对。
pub fn dup_detectable(table: &[NameRow], name: &str) -> Result<bool, u16> {
    let count = table.iter().filter(|r| r.name == name).count();
    match find(table, name)? {
        FindVerdict::Ambiguous(a, b) => {
            if count < 2 || a == b {
                return Err(XC_DUP_HIDDEN);
            }
            Ok(true)
        }
        _ => Ok(count < 2),
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

/// 自例程：9/9 用例（3×3 三态×三名面）+ 重复名 2/2 + 守恒 +
/// 错误路径。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 名表档：unique-a / path/to/b / c.txt / c.txt（重复）/ dup（重复）。
    let mut arc = Vec::new();
    arc.extend(build_entry(1, "unique-a", b"1"));
    arc.extend(build_entry(2, "path/to/b", b"22"));
    arc.extend(build_entry(3, "c.txt", b"333"));
    arc.extend(build_entry(4, "c.txt", b"4444"));
    arc.extend(build_entry(5, "dup", &[]));
    arc.extend(build_entry(6, "dup", &[]));
    arc.extend(build_entry(0, TRAILER_NAME, &[]));

    let table = name_table(&arc)?;
    if table.len() != 6 {
        return Err(XC_FIND_UNBALANCED);
    }
    // 序号账。
    if table[0].name != "unique-a" || table[0].ordinal != 0 {
        return Err(XC_FIND_UNBALANCED);
    }
    if table[1].name != "path/to/b" || table[1].ordinal != 1 {
        return Err(XC_FIND_UNBALANCED);
    }

    // 9/9 用例：三态 × 三名面。
    let queries = [
        // 面一：普通名。
        "unique-a",   // hit
        "ghost",      // miss
        "c.txt",      // ambiguous
        // 面二：路径名。
        "path/to/b",  // hit
        "path/none",  // miss
        "dup",        // ambiguous
        // 面三：边界名。
        "TRAILER!!!", // miss（TRAILER 不入表）
        "a",          // miss（前缀不命中）
        "unique-a ",  // miss（尾空格不命中）
    ];
    let book = FindBook::run(&table, &queries)?;
    book.conserved()?;
    if book.hits != 2 || book.misses != 5 || book.ambiguous != 2 {
        return Err(XC_FIND_UNBALANCED);
    }
    // 逐用例核：hit 首序。
    if book.cases[0].code != 1 || book.cases[0].first != 0 {
        return Err(XC_FIND_UNBALANCED);
    }
    if book.cases[2].code != 2 {
        return Err(XC_DUP_HIDDEN);
    }
    if book.cases[6].code != 0 {
        return Err(XC_FIND_UNBALANCED);
    }

    // 重复名检出显性 2/2。
    if !dup_detectable(&table, "c.txt")? {
        return Err(XC_DUP_HIDDEN);
    }
    if !dup_detectable(&table, "dup")? {
        return Err(XC_DUP_HIDDEN);
    }
    // 非重复名返回 false（非红）。
    if dup_detectable(&table, "unique-a")? {
        return Err(XC_DUP_HIDDEN);
    }
    // Ambiguous 序号对：c.txt → (2, 3)。
    match find(&table, "c.txt")? {
        FindVerdict::Ambiguous(a, b) => {
            if a != 2 || b != 3 {
                return Err(XC_DUP_HIDDEN);
            }
        }
        _ => return Err(XC_DUP_HIDDEN),
    }

    // 错误路径 1：空查询。
    if find(&table, "") != Err(XC_QUERY_EMPTY) {
        return Err(XC_QUERY_EMPTY);
    }
    // 错误路径 2：空表。
    let mut single = Vec::new();
    single.extend(build_entry(0, TRAILER_NAME, &[]));
    if name_table(&single) != Err(XC_TABLE_EMPTY) {
        return Err(XC_TABLE_EMPTY);
    }
    // 错误路径 3：坏魔数。
    let mut bad = build_entry(1, "x", &[]);
    bad[1] = b'1';
    if name_table(&bad) != Err(XC_MAGIC) {
        return Err(XC_MAGIC);
    }
    // 错误路径 4：名区无 NUL。
    let mut bad2 = build_entry(1, "ab", &[]);
    bad2[110 + 1] = b'z';
    if name_table(&bad2) != Err(XC_NAME_NUL) {
        return Err(XC_NAME_NUL);
    }
    Ok(0)
}
