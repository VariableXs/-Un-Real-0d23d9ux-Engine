//! unx0603_cpiomagic —— UNX-F0603 cpio newc 魔数识别（0x070701）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0603 cpio newc 魔数识别（0x070701）  <SEG 0x4d20>
//! 目标行数：≥220（判据：纯功能代码 ≥220 行）
//! 判据原文摘录：
//!   ② 判据原文：魔数核（首条目 0x070701 6 字节 1/1 命中），
//!   newc 格式版本账注（c_magic 字段）。
//!
//! 设计纪律：
//!   1. newc 头 110 字节，首字段 c_magic 占 [0..6)（写死布局）；
//      魔数合法闭集仅一枚：ASCII "070701"（0x30 0x37 ×4 0x30 0x31）；
//   2. 魔数核三面：首条目命中（1/1）、非首条目逐条命中、伪魔数
//      拒识（"070702" 是 CRC 格式魔数，属他格式红码）；
//   3. 版本账注：c_magic 识别结果映射格式版本账（newc v1），
//      非 newc 魔数逐枚红码且红因入账（可诊断面）；
//   4. 扫描面：在任意字节流中定位所有 newc 魔数出现点（偏移账），
//      命中账守恒：Σ命中 = 扫描报告条数；
//   5. 禁浮点禁 std，无 unwrap/expect/panic；越界切片用 get 面；
//   6. 确定性：同输入同账，账可逐位复算。
//!
//! 诊断段声明：0x4d20 段为本模块独占（SEG_BASE=0x4d20），判别式
//   SEG_BASE+1..+N；与 0x4d00/0x4d40 邻段不重叠。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const CM_VERSION: &str = "unx0603_cpiomagic-v1";

/// 诊断段基址（0x4d20 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4D20;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4d21 魔数缺位（流短于 6 字节）。
pub const XC_MAGIC_SHORT: u16 = SEG_BASE + 1;
/// 0x4d22 魔数失配（非 "070701"）。
pub const XC_MAGIC_MISMATCH: u16 = SEG_BASE + 2;
/// 0x4d23 他格式魔数（070702/070707 等近亲）。
pub const XC_MAGIC_FOREIGN: u16 = SEG_BASE + 3;
/// 0x4d24 头长不足（<110 字节，魔数虽中头不完整）。
pub const XC_HEAD_SHORT: u16 = SEG_BASE + 4;
/// 0x4d25 扫描空流。
pub const XC_SCAN_EMPTY: u16 = SEG_BASE + 5;
/// 0x4d26 命中账不守恒。
pub const XC_HIT_UNBALANCED: u16 = SEG_BASE + 6;

/// newc 头长（写死：110 字节）。
pub const HEAD_LEN: usize = 110;
/// 魔数宽（写死：6 字节 ASCII）。
pub const MAGIC_LEN: usize = 6;

/// newc 魔数（写死："070701"）。
pub const MAGIC_NEWC: [u8; MAGIC_LEN] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x31];
/// 近亲魔数：CRC 格式 "070702"（他格式，红码面）。
pub const MAGIC_CRC: [u8; MAGIC_LEN] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x32];
/// 近亲魔数：portable ASCII "070707"（他格式，红码面）。
pub const MAGIC_PORT: [u8; MAGIC_LEN] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x37];

/// 魔数识别结果闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagicVerdict {
    /// newc v1 命中。
    Newc,
    /// CRC 格式（他格式）。
    CrcFormat,
    /// portable ASCII（他格式）。
    Portable,
    /// 未知魔数。
    Unknown,
}

/// 单点识别：6 字节窗 → 判定。
pub fn identify(window: &[u8]) -> MagicVerdict {
    if window.len() < MAGIC_LEN {
        return MagicVerdict::Unknown;
    }
    let w = &window[..MAGIC_LEN];
    if w == &MAGIC_NEWC[..] {
        MagicVerdict::Newc
    } else if w == &MAGIC_CRC[..] {
        MagicVerdict::CrcFormat
    } else if w == &MAGIC_PORT[..] {
        MagicVerdict::Portable
    } else {
        MagicVerdict::Unknown
    }
}

/// 魔数核（判据面）：流首 6 字节必须是 newc 魔数；命中后头完整
/// 性另核（≥110 字节）。
pub fn magic_check(stream: &[u8]) -> Result<(), u16> {
    if stream.len() < MAGIC_LEN {
        return Err(XC_MAGIC_SHORT);
    }
    match identify(stream) {
        MagicVerdict::Newc => {}
        MagicVerdict::CrcFormat | MagicVerdict::Portable => return Err(XC_MAGIC_FOREIGN),
        MagicVerdict::Unknown => return Err(XC_MAGIC_MISMATCH),
    }
    if stream.len() < HEAD_LEN {
        return Err(XC_HEAD_SHORT);
    }
    Ok(())
}

/// newc 格式版本账注：识别结果 → 版本账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatNote {
    /// 偏移。
    pub offset: u32,
    /// 判定码：1=newc 2=crc 3=portable 4=unknown。
    pub verdict_code: u8,
    /// 版本注（newc 固定 1）。
    pub format_ver: u8,
}

impl FormatNote {
    /// 判定 → 码。
    pub fn code_of(v: MagicVerdict) -> u8 {
        match v {
            MagicVerdict::Newc => 1,
            MagicVerdict::CrcFormat => 2,
            MagicVerdict::Portable => 3,
            MagicVerdict::Unknown => 4,
        }
    }
}

/// 命中账条目（newc 命中点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitEntry {
    /// 命中偏移。
    pub offset: u32,
    /// 条目序（0 起）。
    pub ordinal: u32,
}

/// 流扫描：定位全部 newc 魔数（步进 1 逐窗——穷尽面，确定性）。
/// 近亲魔数（CRC/Portable）同时入"他格式账"，与命中账分离。
pub struct ScanBook {
    /// newc 命中账。
    pub hits: Vec<HitEntry>,
    /// 他格式账。
    pub foreign: Vec<FormatNote>,
    /// 扫描流长。
    pub scanned: u32,
}

impl ScanBook {
    /// 扫描主路径。
    pub fn scan(stream: &[u8]) -> Result<ScanBook, u16> {
        if stream.is_empty() {
            return Err(XC_SCAN_EMPTY);
        }
        let mut book = ScanBook {
            hits: Vec::new(),
            foreign: Vec::new(),
            scanned: stream.len() as u32,
        };
        if stream.len() < MAGIC_LEN {
            // 全流皆短：无窗可扫，空账合法（scanned 已记）。
            return Ok(book);
        }
        let mut ord = 0u32;
        for off in 0..=(stream.len() - MAGIC_LEN) {
            let win = stream
                .get(off..off + MAGIC_LEN)
                .ok_or(XC_MAGIC_SHORT)?;
            match identify(win) {
                MagicVerdict::Newc => {
                    book.hits.push(HitEntry {
                        offset: off as u32,
                        ordinal: ord,
                    });
                    ord += 1;
                }
                MagicVerdict::CrcFormat | MagicVerdict::Portable => {
                    book.foreign.push(FormatNote {
                        offset: off as u32,
                        verdict_code: FormatNote::code_of(match identify(win) {
                            MagicVerdict::CrcFormat => MagicVerdict::CrcFormat,
                            _ => MagicVerdict::Portable,
                        }),
                        format_ver: 0,
                    });
                }
                MagicVerdict::Unknown => {}
            }
        }
        // 守恒：命中账序号连续 0..n-1。
        for (i, h) in book.hits.iter().enumerate() {
            if h.ordinal != i as u32 {
                return Err(XC_HIT_UNBALANCED);
            }
        }
        Ok(book)
    }

    /// 守恒核：命中账条数 = 序号最大值 + 1（连续）。
    pub fn conserved(&self) -> Result<(), u16> {
        let n = self.hits.len();
        if n > 0 && self.hits[n - 1].ordinal != (n - 1) as u32 {
            return Err(XC_HIT_UNBALANCED);
        }
        Ok(())
    }
}

/// newc 条目头构造辅助：110 字节头，c_magic 填 newc，其余按
/// 八字段十六进制布局（这里只填魔数字段，余零——魔数单测面）。
pub fn build_head() -> [u8; HEAD_LEN] {
    let mut h = [0u8; HEAD_LEN];
    h[..MAGIC_LEN].copy_from_slice(&MAGIC_NEWC);
    h
}

/// 首条目帧构造：魔数 + 填充（判据原文场景：首条目 0x070701）。
pub fn build_first_entry(pad: u8) -> [u8; HEAD_LEN] {
    let mut h = build_head();
    for b in h[MAGIC_LEN..].iter_mut() {
        *b = pad;
    }
    h
}

/// 自例程：魔数三面核 + 扫描账 + 错误路径。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径 1：首条目命中。
    let head = build_first_entry(0x30);
    magic_check(&head)?;
    // 识别面。
    if identify(&MAGIC_NEWC) != MagicVerdict::Newc {
        return Err(XC_MAGIC_MISMATCH);
    }
    if identify(&MAGIC_CRC) != MagicVerdict::CrcFormat {
        return Err(XC_MAGIC_FOREIGN);
    }
    if identify(&MAGIC_PORT) != MagicVerdict::Portable {
        return Err(XC_MAGIC_FOREIGN);
    }
    // 主路径 2：流内多点扫描（newc + 杂质 + 近亲）。
    let mut stream: Vec<u8> = Vec::new();
    stream.extend_from_slice(&build_head()); // 0 处 newc
    stream.extend_from_slice(&[0x00, 0x11]); // 杂质（错位 2）
    stream.extend_from_slice(&MAGIC_CRC); // 112 处近亲
    stream.extend_from_slice(&[0xFF; 4]);
    stream.extend_from_slice(&MAGIC_NEWC); // 122 处 newc
    let book = ScanBook::scan(&stream)?;
    book.conserved()?;
    // 命中：0 与 122 两处。
    if book.hits.len() != 2 || book.hits[0].offset != 0 || book.hits[1].offset != 122 {
        return Err(XC_HIT_UNBALANCED);
    }
    // 他格式账：112 处 1 枚。
    if book.foreign.len() != 1 || book.foreign[0].offset != 112 {
        return Err(XC_MAGIC_FOREIGN);
    }
    if book.scanned != stream.len() as u32 {
        return Err(XC_HIT_UNBALANCED);
    }
    // 首条目魔数核（判据原文 1/1 命中面）。
    if book.hits[0].ordinal != 0 {
        return Err(XC_HIT_UNBALANCED);
    }

    // 错误路径 1：流短。
    if magic_check(&MAGIC_NEWC[..4]) != Err(XC_MAGIC_SHORT) {
        return Err(XC_MAGIC_SHORT);
    }
    // 错误路径 2：魔数失配。
    let mut bad = build_head();
    bad[5] = 0x39; // "070709"
    if magic_check(&bad) != Err(XC_MAGIC_MISMATCH) {
        return Err(XC_MAGIC_MISMATCH);
    }
    // 错误路径 3：近亲魔数拒识。
    let mut crc_head = build_head();
    crc_head[..MAGIC_LEN].copy_from_slice(&MAGIC_CRC);
    if magic_check(&crc_head) != Err(XC_MAGIC_FOREIGN) {
        return Err(XC_MAGIC_FOREIGN);
    }
    // 错误路径 4：头长不足。
    if magic_check(&MAGIC_NEWC) != Err(XC_HEAD_SHORT) {
        return Err(XC_HEAD_SHORT);
    }
    // 错误路径 5：空流扫描。
    if ScanBook::scan(&[]).err() != Some(XC_SCAN_EMPTY) {
        return Err(XC_SCAN_EMPTY);
    }
    // 错误路径 6：未知魔数。
    let junk = [0x00u8; HEAD_LEN];
    if magic_check(&junk) != Err(XC_MAGIC_MISMATCH) {
        return Err(XC_MAGIC_MISMATCH);
    }
    // 确定性：同流两扫同账。
    let b2 = ScanBook::scan(&stream)?;
    if b2.hits.len() != book.hits.len() || b2.foreign.len() != book.foreign.len() {
        return Err(XC_HIT_UNBALANCED);
    }
    Ok(0)
}
