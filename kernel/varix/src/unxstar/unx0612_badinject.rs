//! unx0612_badinject —— UNX-F0612 烂档注入演练（三类检出）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0612 烂档注入演练（三类检出）  <SEG 0x4e40>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：烂档注入检出 100%（3/3——魔数烂/头烂/截断档），
//!   好档误伤 0。
//!
//! 设计纪律：
//!   1. 烂档三类闭集（写死）：Ⅰ 魔数烂（首 6 字节非 070701）/
//      Ⅱ 头烂（八字段 hex 破或 namesize 0/名区无 NUL）/
//      Ⅲ 截断档（数据区越档尾或 TRAILER 缺失）；
//   2. 检出器三检顺序（写死）：Ⅰ→Ⅱ→Ⅲ（首检优先，红因取首
//      检命中类——不叠报）；
//   3. 好档误伤 0：检出器对好档必须放行（红因 = 无）；
//   4. 注入演练账：逐注入（注入类, 红因码, 注入点）三列，检出
//      计数 = 注入数（100%），误伤计数 = 0；
//   5. 对账守恒：检出数 + 误伤数 = 演练总数；
//   6. 禁浮点禁 std，无 unwrap/expect/panic，确定性。
//!
//! 诊断段声明：0x4e40 段为本模块独占（SEG_BASE=0x4e40），判别式
//   SEG_BASE+1..+N；与 0x4e20/0x4e60 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const BI_VERSION: &str = "unx0612_badinject-v1";

/// 诊断段基址（0x4e40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4E40;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4e41 Ⅰ 魔数烂。
pub const XC_BAD_MAGIC: u16 = SEG_BASE + 1;
/// 0x4e42 Ⅱ 头烂（字段/名区）。
pub const XC_BAD_HEAD: u16 = SEG_BASE + 2;
/// 0x4e43 Ⅲ 截断档。
pub const XC_TRUNCATED: u16 = SEG_BASE + 3;
/// 0x4e44 检出类别越界（三类外）。
pub const XC_BAD_CLASS: u16 = SEG_BASE + 4;
/// 0x4e45 好档误伤（检出器对好档报红）。
pub const XC_FALSE_POSITIVE: u16 = SEG_BASE + 5;
/// 0x4e46 演练账不守恒。
pub const XC_DRILL_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x4e47 注入类未检出（<100%）。
pub const XC_MISSED: u16 = SEG_BASE + 7;
/// 0x4e48 空档演练。
pub const XC_EMPTY_DRILL: u16 = SEG_BASE + 8;

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
        return Err(XC_BAD_HEAD);
    }
    let mut v = 0u32;
    for i in 0..8 {
        let c = head[off + i];
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'f' => (c - b'a' + 10) as u32,
            b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => return Err(XC_BAD_HEAD),
        };
        v = (v << 4) | d;
    }
    Ok(v)
}

/// 检出器主路径（三检顺序 Ⅰ→Ⅱ→Ⅲ；好档放行 Ok(())）。
/// Ⅲ 截断：任意条目数据区越档尾，或遍历尽而无 TRAILER。
pub fn detect(stream: &[u8]) -> Result<(), u16> {
    if stream.is_empty() {
        return Err(XC_TRUNCATED);
    }
    // Ⅰ：魔数烂。
    if stream.len() < 6 || &stream[0..6] != MAGIC {
        return Err(XC_BAD_MAGIC);
    }
    let mut off = 0usize;
    let mut saw_trailer = false;
    loop {
        // Ⅱ：头烂。
        let head = stream.get(off..off + HEAD_LEN).ok_or(XC_BAD_HEAD)?;
        if &head[0..6] != MAGIC {
            return Err(XC_BAD_MAGIC);
        }
        let filesize = hex_field(head, 54)?;
        let namesize = hex_field(head, 94)? as usize;
        if namesize < 1 || namesize > HEAD_LEN + 4096 {
            return Err(XC_BAD_HEAD);
        }
        let name_off = off + HEAD_LEN;
        let end = name_off.checked_add(namesize).ok_or(XC_BAD_HEAD)?;
        let region = stream.get(name_off..end).ok_or(XC_TRUNCATED)?;
        if region[namesize - 1] != 0 {
            return Err(XC_BAD_HEAD);
        }
        let mut name = String::new();
        for b in &region[..namesize - 1] {
            name.push(*b as char);
        }
        let data_off = name_off + namesize + pad4(namesize);
        let data_end = data_off
            .checked_add(filesize as usize)
            .ok_or(XC_TRUNCATED)?;
        // Ⅲ：截断（数据区越尾）。
        if stream.len() < data_end {
            return Err(XC_TRUNCATED);
        }
        if name == TRAILER_NAME {
            saw_trailer = true;
            break;
        }
        off = data_end + pad4(filesize as usize);
        // Ⅲ：档尽无 TRAILER。
        if off >= stream.len() {
            return Err(XC_TRUNCATED);
        }
    }
    if !saw_trailer {
        return Err(XC_TRUNCATED);
    }
    Ok(())
}

/// 注入类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectClass {
    /// Ⅰ 魔数烂。
    MagicRotten = 1,
    /// Ⅱ 头烂。
    HeadRotten = 2,
    /// Ⅲ 截断档。
    Truncated = 3,
}

impl InjectClass {
    /// 期望红因。
    pub fn expect_xc(&self) -> u16 {
        match self {
            InjectClass::MagicRotten => XC_BAD_MAGIC,
            InjectClass::HeadRotten => XC_BAD_HEAD,
            InjectClass::Truncated => XC_TRUNCATED,
        }
    }

    /// 类号。
    pub fn code(&self) -> u8 {
        *self as u8
    }
}

/// 演练账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRow {
    /// 注入类号。
    pub class: u8,
    /// 实测红因（0 = 放行误伤）。
    pub seen_xc: u16,
    /// 是否检出（红因 = 预期）。
    pub detected: bool,
}

/// 演练总账。
#[derive(Debug, Default)]
pub struct DrillBook {
    pub rows: Vec<DrillRow>,
    /// 检出数。
    pub detected: u32,
    /// 误伤数。
    pub false_hits: u32,
}

impl DrillBook {
    /// 守恒：检出 + 误伤 = 演练总数。
    pub fn conserved(&self) -> Result<(), u16> {
        if (self.detected + self.false_hits) as usize != self.rows.len() {
            return Err(XC_DRILL_UNBALANCED);
        }
        Ok(())
    }

    /// 100% 判定：全检出且零误伤。
    pub fn full_detection(&self) -> Result<(), u16> {
        if self.false_hits != 0 {
            return Err(XC_FALSE_POSITIVE);
        }
        if self.detected as usize != self.rows.len() {
            return Err(XC_MISSED);
        }
        Ok(())
    }
}

/// 注入器：按类注入烂档（返回注入档副本）。
pub fn inject(good: &[u8], class: InjectClass) -> Result<Vec<u8>, u16> {
    let mut bad = good.to_vec();
    match class {
        InjectClass::MagicRotten => {
            if bad.len() >= 1 {
                bad[0] = b'8';
            }
        }
        InjectClass::HeadRotten => {
            // 名区 NUL 改非零（头烂形态之一）。
            if bad.len() > HEAD_LEN {
                bad[HEAD_LEN] = b'x';
            } else {
                return Err(XC_BAD_HEAD);
            }
        }
        InjectClass::Truncated => {
            // 砍掉尾部 4 字节（截断形态）。
            if bad.len() > 4 {
                bad.truncate(bad.len() - 4);
            } else {
                return Err(XC_TRUNCATED);
            }
        }
    }
    Ok(bad)
}

/// 注入演练主路径：好档基线 → 逐类注入 → 检出对拍 → 落账。
pub fn run_drill(good: &[u8]) -> Result<DrillBook, u16> {
    if good.is_empty() {
        return Err(XC_EMPTY_DRILL);
    }
    // 好档基线：放行（误伤 0 的前提实证）。
    detect(good).map_err(|_| XC_FALSE_POSITIVE)?;
    let mut book = DrillBook::default();
    for class in [
        InjectClass::MagicRotten,
        InjectClass::HeadRotten,
        InjectClass::Truncated,
    ] {
        let bad = inject(good, class)?;
        let seen = match detect(&bad) {
            Ok(()) => 0u16,
            Err(e) => e,
        };
        let detected = seen == class.expect_xc();
        if detected {
            book.detected += 1;
        } else {
            book.false_hits += 1;
        }
        book.rows.push(DrillRow {
            class: class.code(),
            seen_xc: seen,
            detected,
        });
    }
    book.conserved()?;
    Ok(book)
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

/// 自例程：好档基线放行 → 三类注入 3/3 检出 → 好档误伤 0 →
/// 守恒 → 错误路径（空档/检测器直判三形/注入器边界）。
pub fn drill() -> Result<u32, u16> {
    // 好档。
    let mut good = Vec::new();
    good.extend(build_entry(1, "a", b"xyz"));
    good.extend(build_entry(2, "b", &[1, 2, 3, 4, 5, 6, 7, 8]));
    good.extend(build_entry(0, TRAILER_NAME, &[]));

    // 主路径：三类注入演练。
    let book = run_drill(&good)?;
    book.conserved()?;
    book.full_detection()?;
    if book.rows.len() != 3 {
        return Err(XC_DRILL_UNBALANCED);
    }
    // 逐类红因核对。
    if book.rows[0].seen_xc != XC_BAD_MAGIC {
        return Err(XC_MISSED);
    }
    if book.rows[1].seen_xc != XC_BAD_HEAD {
        return Err(XC_MISSED);
    }
    if book.rows[2].seen_xc != XC_TRUNCATED {
        return Err(XC_MISSED);
    }
    // 好档直判放行（误伤 0 再证）。
    if detect(&good).is_err() {
        return Err(XC_FALSE_POSITIVE);
    }

    // 检出器直判三形。
    // Ⅰ：空流判截断（首检 Ⅲ 优先级下空流无魔数——三检顺序 Ⅰ 在
    // 前，但流短于 6 无法读魔数，归 Ⅲ 截断形态）。
    if detect(&[]) != Err(XC_TRUNCATED) {
        return Err(XC_TRUNCATED);
    }
    // Ⅰ：魔数烂。
    let mut badm = good.clone();
    badm[3] = b'9';
    if detect(&badm) != Err(XC_BAD_MAGIC) {
        return Err(XC_BAD_MAGIC);
    }
    // Ⅱ：namesize 0。
    let mut badh = build_entry(1, "a", &[]);
    for i in 0..8 {
        badh[94 + i] = b'0';
    }
    if detect(&badh) != Err(XC_BAD_HEAD) {
        return Err(XC_BAD_HEAD);
    }
    // Ⅱ：名区无 NUL。
    let mut badh2 = build_entry(1, "ab", &[]);
    badh2[110] = b'Q';
    if detect(&badh2) != Err(XC_BAD_HEAD) {
        return Err(XC_BAD_HEAD);
    }
    // Ⅲ：尾部截断。
    let badt = &good[..good.len() - 8];
    if detect(badt) != Err(XC_TRUNCATED) {
        return Err(XC_TRUNCATED);
    }
    // 注入器边界：短档注入头烂红。
    let short = build_entry(1, "a", &[]);
    if inject(&short[..HEAD_LEN], InjectClass::HeadRotten) != Err(XC_BAD_HEAD) {
        return Err(XC_BAD_HEAD);
    }
    // 错误路径：空档演练。
    if run_drill(&[]).err() != Some(XC_EMPTY_DRILL) {
        return Err(XC_EMPTY_DRILL);
    }
    // 账守恒破坏面。
    let mut bk = DrillBook::default();
    bk.detected = 2;
    bk.false_hits = 1; // 行数 0 → 和 3 ≠ 0
    if bk.conserved() != Err(XC_DRILL_UNBALANCED) {
        return Err(XC_DRILL_UNBALANCED);
    }
    Ok(0)
}
